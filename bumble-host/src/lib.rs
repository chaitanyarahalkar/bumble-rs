//! bumble-host — the host-side glue of the [`google/bumble`](https://github.com/google/bumble)
//! port.
//!
//! **Slice 10** of the incremental port: a [`Device`] that owns the sequencing
//! the earlier integration tests wired by hand — wrapping ATT PDUs in L2CAP and
//! ACL to send, and unwrapping received ACL back up to ATT. This turns the
//! cross-layer composition into a real library capability.
//!
//! A `Device` sits above a [`bumble_controller::Controller`] (addressed by id
//! on a shared [`bumble_controller::LocalLink`]). It:
//! - learns its connection handle from the LE Connection Complete event,
//! - sends ATT PDUs on the ATT channel with [`Device::send_att`],
//! - on [`Device::poll`], processes inbound ACL: an optional server-role
//!   [`bumble_gatt::AttServer`] answers requests automatically; other ATT PDUs (responses /
//!   notifications) are queued for the client to collect.
//!
//! [`pump`] drives a set of devices to quiescence, which is all the
//! (synchronous) event loop this port needs.
//!
//! ## Scope
//!
//! ATT traffic over the fixed ATT CID plus raw fixed/dynamic L2CAP channels,
//! with controller-buffer-sized ACL fragmentation/reassembly. High-level
//! legacy and extended advertising, scanning, and connection setup are also
//! available, along with CIG/CIS control and ISO SDU fragmentation/reassembly.
//! Deferred: direct integration of the LE signaling manager, periodic-
//! advertising synchronization, and multiple simultaneous connections per
//! device.

use std::collections::{BTreeMap, BTreeSet};

use bumble::Address;
use bumble_att::AttPdu;
use bumble_controller::LocalLink;
use bumble_gatt::AttRequestHandler;
use bumble_hci::{
    fragment_l2cap_pdu, AclDataPacket, AclDataPacketAssembler, AdvertisingReport, CodingFormat,
    Command, Event, ExtendedAdvertisingReport, HciPacket, IsoDataPacket, LeMetaEvent,
    SynchronousDataPacket,
};
use bumble_l2cap::L2capPdu;

mod data_queue;
pub use data_queue::{DataPacketQueue, DataPacketQueueError};

/// The fixed L2CAP channel id for the Attribute Protocol.
pub const ATT_CID: u16 = 0x0004;
/// The fixed L2CAP channel id for LE SMP.
pub const SMP_CID: u16 = 0x0006;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SynchronousConnectionInfo {
    pub connection_handle: u16,
    pub peer_address: Address,
    pub link_type: u8,
    pub air_mode: u8,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CisRequestInfo {
    pub acl_connection_handle: u16,
    pub cis_connection_handle: u16,
    pub cig_id: u8,
    pub cis_id: u8,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IsoSdu {
    pub connection_handle: u16,
    pub packet_sequence_number: u16,
    pub packet_status_flag: u8,
    pub data: Vec<u8>,
}

#[derive(Debug, Default)]
struct IsoSduAssembler {
    pending: Option<IsoSdu>,
    expected_length: usize,
}

impl IsoSduAssembler {
    fn push(&mut self, packet: IsoDataPacket) -> Option<IsoSdu> {
        match packet.pb_flag {
            0b00 | 0b10 => {
                let (Some(sequence), Some(length), Some(status)) = (
                    packet.packet_sequence_number,
                    packet.iso_sdu_length,
                    packet.packet_status_flag,
                ) else {
                    self.pending = None;
                    return None;
                };
                let sdu = IsoSdu {
                    connection_handle: packet.connection_handle,
                    packet_sequence_number: sequence,
                    packet_status_flag: status,
                    data: packet.iso_sdu_fragment,
                };
                if packet.pb_flag == 0b10 {
                    self.pending = None;
                    return (sdu.data.len() == usize::from(length)).then_some(sdu);
                }
                self.expected_length = usize::from(length);
                self.pending = Some(sdu);
                None
            }
            0b01 | 0b11 => {
                let pending = self.pending.as_mut()?;
                pending.data.extend_from_slice(&packet.iso_sdu_fragment);
                if pending.data.len() > self.expected_length {
                    self.pending = None;
                    return None;
                }
                if packet.pb_flag == 0b11 {
                    let complete = self.pending.take().expect("pending ISO SDU exists");
                    return (complete.data.len() == self.expected_length).then_some(complete);
                }
                None
            }
            _ => None,
        }
    }
}

/// Parameters for one high-level extended-advertising set. Values map directly
/// to `LE Set Extended Advertising Parameters`; data is supplied separately so
/// the host can fragment it to HCI-sized commands.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExtendedAdvertisingConfig {
    pub handle: u8,
    pub event_properties: u16,
    pub interval_min: u32,
    pub interval_max: u32,
    pub channel_map: u8,
    pub own_address_type: u8,
    pub peer_address_type: u8,
    pub peer_address: Address,
    pub filter_policy: u8,
    pub tx_power: i8,
    pub primary_phy: u8,
    pub secondary_max_skip: u8,
    pub secondary_phy: u8,
    pub sid: u8,
    pub scan_request_notification: bool,
    pub duration: u16,
    pub max_events: u8,
    pub random_address: Option<Address>,
}

impl ExtendedAdvertisingConfig {
    pub fn connectable_scannable(handle: u8, random_address: Address) -> Self {
        Self {
            handle,
            event_properties: 0x0003,
            interval_min: 0x20,
            interval_max: 0x40,
            channel_map: 7,
            own_address_type: 1,
            peer_address_type: 0,
            peer_address: Address::from_bytes([0; 6], bumble::AddressType::PUBLIC_DEVICE),
            filter_policy: 0,
            tx_power: 0,
            primary_phy: 1,
            secondary_max_skip: 0,
            secondary_phy: 1,
            sid: handle & 0x0F,
            scan_request_notification: false,
            duration: 0,
            max_events: 0,
            random_address: Some(random_address),
        }
    }
}

/// A host attached to a controller on a [`LocalLink`]. Owns the
/// ATT↔L2CAP↔ACL sequencing.
pub struct Device {
    controller_id: usize,
    server: Option<Box<dyn AttRequestHandler>>,
    connection_handle: Option<u16>,
    connection_role: Option<u8>,
    peer_address: Option<Address>,
    classic_connection_handle: Option<u16>,
    synchronous_connections: Vec<SynchronousConnectionInfo>,
    synchronous_requests: Vec<(Address, u8)>,
    synchronous_inbox: Vec<SynchronousDataPacket>,
    cis_requests: Vec<CisRequestInfo>,
    configured_cis_handles: Vec<u16>,
    established_cis_handles: BTreeSet<u16>,
    iso_sequence_numbers: BTreeMap<u16, u16>,
    iso_assemblers: BTreeMap<u16, IsoSduAssembler>,
    iso_inbox: Vec<IsoSdu>,
    inbox: Vec<AttPdu>,
    /// Received payloads on non-ATT L2CAP channels, as `(cid, payload)`.
    l2cap_inbox: Vec<(u16, Vec<u8>)>,
    security_requests: Vec<u8>,
    advertising_reports: Vec<AdvertisingReport>,
    extended_advertising_reports: Vec<ExtendedAdvertisingReport>,
    acl_data_packet_length: usize,
    acl_assemblers: BTreeMap<u16, AclDataPacketAssembler>,
    acl_packet_queue: DataPacketQueue<AclDataPacket>,
    encrypted_handles: BTreeSet<u16>,
}

impl Device {
    /// A client-only device (no attribute server).
    pub fn new(controller_id: usize) -> Device {
        Device {
            controller_id,
            server: None,
            connection_handle: None,
            connection_role: None,
            peer_address: None,
            classic_connection_handle: None,
            synchronous_connections: Vec::new(),
            synchronous_requests: Vec::new(),
            synchronous_inbox: Vec::new(),
            cis_requests: Vec::new(),
            configured_cis_handles: Vec::new(),
            established_cis_handles: BTreeSet::new(),
            iso_sequence_numbers: BTreeMap::new(),
            iso_assemblers: BTreeMap::new(),
            iso_inbox: Vec::new(),
            inbox: Vec::new(),
            l2cap_inbox: Vec::new(),
            security_requests: Vec::new(),
            advertising_reports: Vec::new(),
            extended_advertising_reports: Vec::new(),
            acl_data_packet_length: 27,
            acl_assemblers: BTreeMap::new(),
            acl_packet_queue: DataPacketQueue::new(64).expect("nonzero ACL queue capacity"),
            encrypted_handles: BTreeSet::new(),
        }
    }

    /// A device that also answers ATT requests using the given handler
    /// (an [`bumble_gatt::AttServer`] or a full [`bumble_gatt::GattServer`]).
    pub fn with_server(controller_id: usize, server: impl AttRequestHandler + 'static) -> Device {
        Device {
            controller_id,
            server: Some(Box::new(server)),
            connection_handle: None,
            connection_role: None,
            peer_address: None,
            classic_connection_handle: None,
            synchronous_connections: Vec::new(),
            synchronous_requests: Vec::new(),
            synchronous_inbox: Vec::new(),
            cis_requests: Vec::new(),
            configured_cis_handles: Vec::new(),
            established_cis_handles: BTreeSet::new(),
            iso_sequence_numbers: BTreeMap::new(),
            iso_assemblers: BTreeMap::new(),
            iso_inbox: Vec::new(),
            inbox: Vec::new(),
            l2cap_inbox: Vec::new(),
            security_requests: Vec::new(),
            advertising_reports: Vec::new(),
            extended_advertising_reports: Vec::new(),
            acl_data_packet_length: 27,
            acl_assemblers: BTreeMap::new(),
            acl_packet_queue: DataPacketQueue::new(64).expect("nonzero ACL queue capacity"),
            encrypted_handles: BTreeSet::new(),
        }
    }

    pub fn controller_id(&self) -> usize {
        self.controller_id
    }

    /// The connection handle, once connected (and `None` after disconnection).
    pub fn connection_handle(&self) -> Option<u16> {
        self.connection_handle
    }

    /// `true` while a connection is established.
    pub fn is_connected(&self) -> bool {
        self.connection_handle.is_some()
    }

    pub fn connection_role(&self) -> Option<u8> {
        self.connection_role
    }

    pub fn peer_address(&self) -> Option<&Address> {
        self.peer_address.as_ref()
    }

    pub fn set_random_address(&mut self, link: &mut LocalLink, address: Address) {
        self.send_hci_command(
            link,
            Command::LeSetRandomAddress {
                random_address: address,
            },
        );
    }

    pub fn start_advertising(&mut self, link: &mut LocalLink, data: &[u8]) -> bool {
        if data.len() > 31 {
            return false;
        }
        self.send_hci_command(
            link,
            Command::LeSetAdvertisingParameters {
                advertising_interval_min: 0x0800,
                advertising_interval_max: 0x0800,
                advertising_type: 0,
                own_address_type: 1,
                peer_address_type: 0,
                peer_address: Address::from_bytes([0; 6], bumble::AddressType::PUBLIC_DEVICE),
                advertising_channel_map: 7,
                advertising_filter_policy: 0,
            },
        );
        self.send_hci_command(
            link,
            Command::LeSetAdvertisingData {
                advertising_data: data.to_vec(),
            },
        );
        self.send_hci_command(
            link,
            Command::LeSetAdvertisingEnable {
                advertising_enable: 1,
            },
        );
        true
    }

    pub fn stop_advertising(&mut self, link: &mut LocalLink) {
        self.send_hci_command(
            link,
            Command::LeSetAdvertisingEnable {
                advertising_enable: 0,
            },
        );
    }

    /// Configure and enable one extended-advertising set. Data larger than a
    /// single HCI command is fragmented with the standard first/intermediate/
    /// last operations; the controller reassembles up to Bumble's 1650-byte
    /// advertised maximum.
    pub fn start_extended_advertising(
        &mut self,
        link: &mut LocalLink,
        config: &ExtendedAdvertisingConfig,
        data: &[u8],
        scan_response_data: &[u8],
    ) -> bool {
        if data.len() > 0x0672 || scan_response_data.len() > 0x0672 || config.sid > 0x0F {
            return false;
        }
        if let Some(random_address) = config.random_address.clone() {
            self.send_hci_command(
                link,
                Command::LeSetAdvertisingSetRandomAddress {
                    advertising_handle: config.handle,
                    random_address,
                },
            );
        }
        self.send_hci_command(
            link,
            Command::LeSetExtendedAdvertisingParameters {
                advertising_handle: config.handle,
                advertising_event_properties: config.event_properties,
                primary_advertising_interval_min: config.interval_min,
                primary_advertising_interval_max: config.interval_max,
                primary_advertising_channel_map: config.channel_map,
                own_address_type: config.own_address_type,
                peer_address_type: config.peer_address_type,
                peer_address: config.peer_address.clone(),
                advertising_filter_policy: config.filter_policy,
                advertising_tx_power: config.tx_power as u8,
                primary_advertising_phy: config.primary_phy,
                secondary_advertising_max_skip: config.secondary_max_skip,
                secondary_advertising_phy: config.secondary_phy,
                advertising_sid: config.sid,
                scan_request_notification_enable: u8::from(config.scan_request_notification),
            },
        );
        self.send_extended_advertising_data(link, config.handle, data, false);
        self.send_extended_advertising_data(link, config.handle, scan_response_data, true);
        self.send_hci_command(
            link,
            Command::LeSetExtendedAdvertisingEnable {
                enable: 1,
                advertising_handles: vec![config.handle],
                durations: vec![config.duration],
                max_extended_advertising_events: vec![config.max_events],
            },
        );
        true
    }

    pub fn stop_extended_advertising(&mut self, link: &mut LocalLink, handle: u8) {
        self.send_hci_command(
            link,
            Command::LeSetExtendedAdvertisingEnable {
                enable: 0,
                advertising_handles: vec![handle],
                durations: vec![0],
                max_extended_advertising_events: vec![0],
            },
        );
    }

    fn send_extended_advertising_data(
        &mut self,
        link: &mut LocalLink,
        handle: u8,
        data: &[u8],
        scan_response: bool,
    ) {
        let chunks: Vec<_> = if data.is_empty() {
            vec![&[][..]]
        } else {
            data.chunks(251).collect()
        };
        for (index, chunk) in chunks.iter().enumerate() {
            let operation = if chunks.len() == 1 {
                0x03
            } else if index == 0 {
                0x01
            } else if index + 1 == chunks.len() {
                0x02
            } else {
                0x00
            };
            let command = if scan_response {
                Command::LeSetExtendedScanResponseData {
                    advertising_handle: handle,
                    operation,
                    fragment_preference: 1,
                    scan_response_data: chunk.to_vec(),
                }
            } else {
                Command::LeSetExtendedAdvertisingData {
                    advertising_handle: handle,
                    operation,
                    fragment_preference: 1,
                    advertising_data: chunk.to_vec(),
                }
            };
            self.send_hci_command(link, command);
        }
    }

    pub fn start_scanning(&mut self, link: &mut LocalLink, active: bool, filter_duplicates: bool) {
        self.send_hci_command(
            link,
            Command::LeSetScanParameters {
                le_scan_type: u8::from(active),
                le_scan_interval: 0x0010,
                le_scan_window: 0x0010,
                own_address_type: 1,
                scanning_filter_policy: 0,
            },
        );
        self.send_hci_command(
            link,
            Command::LeSetScanEnable {
                le_scan_enable: 1,
                filter_duplicates: u8::from(filter_duplicates),
            },
        );
    }

    pub fn stop_scanning(&mut self, link: &mut LocalLink) {
        self.send_hci_command(
            link,
            Command::LeSetScanEnable {
                le_scan_enable: 0,
                filter_duplicates: 0,
            },
        );
    }

    pub fn start_extended_scanning(
        &mut self,
        link: &mut LocalLink,
        active: bool,
        filter_duplicates: bool,
    ) {
        self.send_hci_command(
            link,
            Command::LeSetExtendedScanParameters {
                own_address_type: 1,
                scanning_filter_policy: 0,
                scanning_phys: 1,
                scan_types: vec![u8::from(active)],
                scan_intervals: vec![0x0010],
                scan_windows: vec![0x0010],
            },
        );
        self.send_hci_command(
            link,
            Command::LeSetExtendedScanEnable {
                enable: 1,
                filter_duplicates: u8::from(filter_duplicates),
                duration: 0,
                period: 0,
            },
        );
    }

    pub fn stop_extended_scanning(&mut self, link: &mut LocalLink) {
        self.send_hci_command(
            link,
            Command::LeSetExtendedScanEnable {
                enable: 0,
                filter_duplicates: 0,
                duration: 0,
                period: 0,
            },
        );
    }

    pub fn take_advertising_reports(&mut self) -> Vec<AdvertisingReport> {
        std::mem::take(&mut self.advertising_reports)
    }

    pub fn take_extended_advertising_reports(&mut self) -> Vec<ExtendedAdvertisingReport> {
        std::mem::take(&mut self.extended_advertising_reports)
    }

    pub fn connect_le(&mut self, link: &mut LocalLink, peer_address: Address) {
        self.send_hci_command(
            link,
            Command::LeCreateConnection {
                le_scan_interval: 0x0010,
                le_scan_window: 0x0010,
                initiator_filter_policy: 0,
                peer_address_type: u8::from(!peer_address.is_public()),
                peer_address,
                own_address_type: 1,
                connection_interval_min: 24,
                connection_interval_max: 40,
                max_latency: 0,
                supervision_timeout: 42,
                min_ce_length: 0,
                max_ce_length: 0,
            },
        );
        link.establish_connections();
    }

    pub fn connect_le_extended(&mut self, link: &mut LocalLink, peer_address: Address) {
        self.send_hci_command(
            link,
            Command::LeExtendedCreateConnection {
                initiator_filter_policy: 0,
                own_address_type: 1,
                peer_address_type: u8::from(!peer_address.is_public()),
                peer_address,
                initiating_phys: 1,
                scan_intervals: vec![0x0010],
                scan_windows: vec![0x0010],
                connection_interval_mins: vec![24],
                connection_interval_maxs: vec![40],
                max_latencies: vec![0],
                supervision_timeouts: vec![42],
                min_ce_lengths: vec![0],
                max_ce_lengths: vec![0],
            },
        );
        link.establish_connections();
    }

    pub fn is_encrypted(&self) -> bool {
        self.connection_handle
            .is_some_and(|handle| self.encrypted_handles.contains(&handle))
    }

    pub fn is_classic_encrypted(&self) -> bool {
        self.classic_connection_handle
            .is_some_and(|handle| self.encrypted_handles.contains(&handle))
    }

    /// Enable LE encryption with a pairing-derived STK/LTK. The peer receives
    /// the corresponding LL encryption request through the virtual link.
    pub fn enable_encryption(&mut self, link: &mut LocalLink, key: [u8; 16]) -> bool {
        self.enable_encryption_with_parameters(link, key, 0, [0; 8])
    }

    /// Enable LE encryption from a persisted Legacy or SC bond. Legacy bonds
    /// preserve their EDIV/RAND metadata; SC bonds pass zero values.
    pub fn enable_encryption_with_parameters(
        &mut self,
        link: &mut LocalLink,
        key: [u8; 16],
        encrypted_diversifier: u16,
        random_number: [u8; 8],
    ) -> bool {
        let Some(connection_handle) = self.connection_handle else {
            return false;
        };
        self.send_hci_command(
            link,
            Command::LeEnableEncryption {
                connection_handle,
                random_number,
                encrypted_diversifier,
                long_term_key: key,
            },
        );
        true
    }

    /// Program the controller resolving list from `KeyStore::get_resolving_keys`.
    /// Invalid-length IRKs are skipped; the returned count is the number loaded.
    pub fn configure_address_resolution(
        &mut self,
        link: &mut LocalLink,
        resolving_keys: &[(Vec<u8>, Address)],
        local_irk: [u8; 16],
    ) -> usize {
        self.send_hci_command(link, Command::LeClearResolvingList);
        let mut loaded = 0;
        for (peer_irk, identity) in resolving_keys {
            let Ok(peer_irk) = peer_irk.as_slice().try_into() else {
                continue;
            };
            self.send_hci_command(
                link,
                Command::LeAddDeviceToResolvingList {
                    peer_identity_address_type: u8::from(!identity.is_public()),
                    peer_identity_address: identity.clone(),
                    peer_irk,
                    local_irk,
                },
            );
            loaded += 1;
        }
        self.send_hci_command(
            link,
            Command::LeSetAddressResolutionEnable {
                address_resolution_enable: u8::from(loaded != 0),
            },
        );
        loaded
    }

    pub fn classic_connection_handle(&self) -> Option<u16> {
        self.classic_connection_handle
    }

    pub fn set_classic_encryption(&mut self, link: &mut LocalLink, enabled: bool) -> bool {
        let Some(connection_handle) = self.classic_connection_handle else {
            return false;
        };
        self.send_hci_command(
            link,
            Command::SetConnectionEncryption {
                connection_handle,
                encryption_enable: u8::from(enabled),
            },
        );
        true
    }

    pub fn synchronous_connections(&self) -> &[SynchronousConnectionInfo] {
        &self.synchronous_connections
    }

    pub fn take_synchronous_requests(&mut self) -> Vec<(Address, u8)> {
        std::mem::take(&mut self.synchronous_requests)
    }

    pub fn take_synchronous_inbox(&mut self) -> Vec<SynchronousDataPacket> {
        std::mem::take(&mut self.synchronous_inbox)
    }

    /// Configure a CIG using Bumble's deterministic in-process defaults. The
    /// allocated CIS handles become available through
    /// [`Device::take_configured_cis_handles`] after [`pump`].
    pub fn configure_cig(&mut self, link: &mut LocalLink, cig_id: u8, cis_ids: &[u8]) -> bool {
        if cis_ids.is_empty() || cis_ids.len() > u8::MAX as usize {
            return false;
        }
        let count = cis_ids.len();
        self.send_hci_command(
            link,
            Command::LeSetCigParameters {
                cig_id,
                sdu_interval_c_to_p: 10_000,
                sdu_interval_p_to_c: 10_000,
                worst_case_sca: 0,
                packing: 0,
                framing: 0,
                max_transport_latency_c_to_p: 10,
                max_transport_latency_p_to_c: 10,
                cis_id: cis_ids.to_vec(),
                max_sdu_c_to_p: vec![251; count],
                max_sdu_p_to_c: vec![251; count],
                phy_c_to_p: vec![1; count],
                phy_p_to_c: vec![1; count],
                rtn_c_to_p: vec![3; count],
                rtn_p_to_c: vec![3; count],
            },
        );
        true
    }

    pub fn take_configured_cis_handles(&mut self) -> Vec<u16> {
        std::mem::take(&mut self.configured_cis_handles)
    }

    pub fn create_cis(&mut self, link: &mut LocalLink, cis_handle: u16) -> bool {
        let Some(acl_handle) = self.connection_handle else {
            return false;
        };
        self.send_hci_command(
            link,
            Command::LeCreateCis {
                cis_connection_handle: vec![cis_handle],
                acl_connection_handle: vec![acl_handle],
            },
        );
        true
    }

    pub fn take_cis_requests(&mut self) -> Vec<CisRequestInfo> {
        std::mem::take(&mut self.cis_requests)
    }

    pub fn accept_cis(&mut self, link: &mut LocalLink, cis_handle: u16) {
        self.send_hci_command(
            link,
            Command::LeAcceptCisRequest {
                connection_handle: cis_handle,
            },
        );
    }

    pub fn established_cis_handles(&self) -> impl Iterator<Item = u16> + '_ {
        self.established_cis_handles.iter().copied()
    }

    pub fn setup_iso_data_path(
        &mut self,
        link: &mut LocalLink,
        cis_handle: u16,
        direction: u8,
    ) -> bool {
        if !self.established_cis_handles.contains(&cis_handle) || direction > 1 {
            return false;
        }
        self.send_hci_command(
            link,
            Command::LeSetupIsoDataPath {
                connection_handle: cis_handle,
                data_path_direction: direction,
                data_path_id: 0,
                codec_id: CodingFormat::TRANSPARENT,
                controller_delay: 0,
                codec_configuration: Vec::new(),
            },
        );
        true
    }

    pub fn remove_iso_data_path(
        &mut self,
        link: &mut LocalLink,
        cis_handle: u16,
        directions: u8,
    ) -> bool {
        if !self.established_cis_handles.contains(&cis_handle) || directions & !0x03 != 0 {
            return false;
        }
        self.send_hci_command(
            link,
            Command::LeRemoveIsoDataPath {
                connection_handle: cis_handle,
                data_path_direction: directions,
            },
        );
        true
    }

    /// Fragment and send one ISO SDU through an established CIS. The 960-byte
    /// controller packet size and first-fragment SDU-info overhead match
    /// upstream Bumble's software-controller defaults.
    pub fn send_iso_sdu(&mut self, link: &mut LocalLink, cis_handle: u16, sdu: &[u8]) -> bool {
        const ISO_PACKET_LENGTH: usize = 960;
        const SDU_INFO_LENGTH: usize = 4;
        if !self.established_cis_handles.contains(&cis_handle) || sdu.len() > 0x0FFF {
            return false;
        }
        let sequence = *self.iso_sequence_numbers.entry(cis_handle).or_default();
        let mut offset = 0;
        loop {
            let first = offset == 0;
            let capacity = ISO_PACKET_LENGTH - if first { SDU_INFO_LENGTH } else { 0 };
            let end = (offset + capacity).min(sdu.len());
            let last = end == sdu.len();
            let fragment = sdu[offset..end].to_vec();
            let packet = IsoDataPacket {
                connection_handle: cis_handle,
                pb_flag: match (first, last) {
                    (true, true) => 0b10,
                    (true, false) => 0b00,
                    (false, true) => 0b11,
                    (false, false) => 0b01,
                },
                ts_flag: 0,
                data_total_length: (fragment.len() + if first { SDU_INFO_LENGTH } else { 0 })
                    as u16,
                time_stamp: None,
                packet_sequence_number: first.then_some(sequence),
                iso_sdu_length: first.then_some(sdu.len() as u16),
                packet_status_flag: first.then_some(0),
                iso_sdu_fragment: fragment,
            };
            if !link.send_iso_packet(self.controller_id, packet) {
                return false;
            }
            if last {
                break;
            }
            offset = end;
        }
        self.iso_sequence_numbers
            .insert(cis_handle, sequence.wrapping_add(1));
        true
    }

    pub fn take_iso_sdus(&mut self, cis_handle: u16) -> Vec<IsoSdu> {
        let (matching, rest): (Vec<_>, Vec<_>) = std::mem::take(&mut self.iso_inbox)
            .into_iter()
            .partition(|sdu| sdu.connection_handle == cis_handle);
        self.iso_inbox = rest;
        matching
    }

    /// Submit any typed HCI command through this device's attached controller.
    pub fn send_hci_command(&mut self, link: &mut LocalLink, command: Command) {
        link.handle_command(self.controller_id, command);
    }

    pub fn connect_classic(&mut self, link: &mut LocalLink, peer_address: Address) {
        self.send_hci_command(
            link,
            Command::CreateConnection {
                bd_addr: peer_address,
                packet_type: 0,
                page_scan_repetition_mode: 0,
                reserved: 0,
                clock_offset: 0,
                allow_role_switch: 0,
            },
        );
    }

    pub fn accept_classic(&mut self, link: &mut LocalLink, peer_address: Address) {
        self.send_hci_command(
            link,
            Command::AcceptConnectionRequest {
                bd_addr: peer_address,
                role: 0,
            },
        );
    }

    pub fn send_synchronous(
        &mut self,
        link: &mut LocalLink,
        connection_handle: u16,
        packet_status: u8,
        data: &[u8],
    ) -> bool {
        link.send_synchronous_data(self.controller_id, connection_handle, packet_status, data)
    }

    pub fn disconnect_handle(
        &mut self,
        link: &mut LocalLink,
        connection_handle: u16,
        reason: u8,
    ) -> bool {
        link.disconnect(self.controller_id, connection_handle, reason)
    }

    /// Disconnect the current connection with the given reason. Both this device
    /// and the peer receive a Disconnection Complete (processed on the next
    /// [`pump`]).
    pub fn disconnect(&mut self, link: &mut LocalLink, reason: u8) -> bool {
        let Some(handle) = self.connection_handle else {
            return false;
        };
        link.disconnect(self.controller_id, handle, reason)
    }

    /// `true` if this device has an attribute server (server role).
    pub fn has_server(&self) -> bool {
        self.server.is_some()
    }

    /// Set the controller's maximum ACL data payload, normally learned from
    /// Read Buffer Size / LE Read Buffer Size.
    pub fn set_acl_data_packet_length(&mut self, length: usize) -> bool {
        if length == 0 || length > u16::MAX as usize {
            return false;
        }
        self.acl_data_packet_length = length;
        true
    }

    /// Set the controller's total ACL packet window while no packets are
    /// pending. This mirrors Read Buffer Size's packet-count field.
    pub fn set_acl_max_in_flight(&mut self, count: usize) -> bool {
        if self.acl_packet_queue.pending() != 0 {
            return false;
        }
        let Ok(queue) = DataPacketQueue::new(count) else {
            return false;
        };
        self.acl_packet_queue = queue;
        true
    }

    pub fn acl_packets_pending(&self) -> usize {
        self.acl_packet_queue.pending()
    }

    /// Remove and return the ATT PDUs received so far that were not handled by
    /// the server (i.e. responses and notifications destined for a client).
    pub fn take_inbox(&mut self) -> Vec<AttPdu> {
        std::mem::take(&mut self.inbox)
    }

    /// Send a raw payload on an L2CAP channel to the peer. Requires an
    /// established connection.
    pub fn send_l2cap(&mut self, link: &mut LocalLink, cid: u16, payload: &[u8]) -> bool {
        let Some(handle) = self.connection_handle else {
            return false;
        };
        self.send_l2cap_on_handle(link, handle, cid, payload)
    }

    pub fn send_l2cap_on_handle(
        &mut self,
        link: &mut LocalLink,
        handle: u16,
        cid: u16,
        payload: &[u8],
    ) -> bool {
        let frame = L2capPdu::new(cid, payload.to_vec()).to_bytes(false);
        let Ok(fragments) =
            fragment_l2cap_pdu(handle, 0, self.acl_data_packet_length, &frame, false)
        else {
            return false;
        };
        for packet in fragments {
            self.acl_packet_queue.enqueue(packet, handle);
        }
        self.flush_acl_queue(link)
    }

    /// Send an ATT PDU to the peer on the ATT channel.
    pub fn send_att(&mut self, link: &mut LocalLink, pdu: &AttPdu) -> bool {
        self.send_l2cap(link, ATT_CID, &pdu.to_bytes())
    }

    /// Remove and return payloads received on the given (non-ATT) L2CAP channel,
    /// e.g. SMP on CID `0x0006`.
    pub fn take_l2cap(&mut self, cid: u16) -> Vec<Vec<u8>> {
        let (matching, rest): (Vec<_>, Vec<_>) = std::mem::take(&mut self.l2cap_inbox)
            .into_iter()
            .partition(|(c, _)| *c == cid);
        self.l2cap_inbox = rest;
        matching.into_iter().map(|(_, payload)| payload).collect()
    }

    /// Remove Security Request authentication bitmasks observed on the SMP
    /// fixed channel. The raw PDU remains available through [`Self::take_l2cap`].
    pub fn take_security_requests(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.security_requests)
    }

    /// Send an unsolicited Handle Value Notification for `value_handle` to the
    /// peer (server → client). The peer collects it from its inbox.
    pub fn notify(&mut self, link: &mut LocalLink, value_handle: u16, value: Vec<u8>) -> bool {
        self.send_att(
            link,
            &AttPdu::HandleValueNotification {
                attribute_handle: value_handle,
                attribute_value: value,
            },
        )
    }

    /// Drain and process this device's controller events. Returns `true` if any
    /// event was consumed (used by [`pump`] to detect quiescence).
    pub fn poll(&mut self, link: &mut LocalLink) -> bool {
        let events = link.drain_host_events(self.controller_id);
        if events.is_empty() {
            return false;
        }

        for event in events {
            match event {
                HciPacket::Event(Event::LeMeta(LeMetaEvent::ConnectionComplete {
                    connection_handle,
                    role,
                    peer_address,
                    ..
                })) => {
                    self.connection_handle = Some(connection_handle);
                    self.connection_role = Some(role);
                    self.peer_address = Some(peer_address);
                }
                HciPacket::Event(Event::LeMeta(LeMetaEvent::AdvertisingReport { reports })) => {
                    self.advertising_reports.extend(reports);
                }
                HciPacket::Event(Event::LeMeta(LeMetaEvent::ExtendedAdvertisingReport {
                    reports,
                })) => {
                    self.extended_advertising_reports.extend(reports);
                }
                HciPacket::Event(Event::LeMeta(LeMetaEvent::CisRequest {
                    acl_connection_handle,
                    cis_connection_handle,
                    cig_id,
                    cis_id,
                })) => self.cis_requests.push(CisRequestInfo {
                    acl_connection_handle,
                    cis_connection_handle,
                    cig_id,
                    cis_id,
                }),
                HciPacket::Event(Event::LeMeta(LeMetaEvent::CisEstablished {
                    status: 0,
                    connection_handle,
                    ..
                })) => {
                    self.established_cis_handles.insert(connection_handle);
                    self.iso_sequence_numbers
                        .entry(connection_handle)
                        .or_default();
                }
                HciPacket::Event(Event::CommandComplete {
                    command_opcode,
                    return_parameters: bumble_hci::ReturnParameters::Raw { data },
                    ..
                }) if command_opcode == bumble_hci::HCI_LE_SET_CIG_PARAMETERS_COMMAND => {
                    if data.len() >= 3 && data[0] == 0 {
                        let count = usize::from(data[2]);
                        if data.len() == 3 + count * 2 {
                            self.configured_cis_handles = data[3..]
                                .chunks_exact(2)
                                .map(|bytes| u16::from_le_bytes([bytes[0], bytes[1]]))
                                .collect();
                        }
                    }
                }
                HciPacket::Event(Event::DisconnectionComplete {
                    connection_handle, ..
                }) => {
                    self.encrypted_handles.remove(&connection_handle);
                    self.established_cis_handles.remove(&connection_handle);
                    self.iso_sequence_numbers.remove(&connection_handle);
                    self.iso_assemblers.remove(&connection_handle);
                    self.iso_inbox
                        .retain(|sdu| sdu.connection_handle != connection_handle);
                    self.acl_assemblers.remove(&connection_handle);
                    self.acl_packet_queue.flush(connection_handle);
                    if self.connection_handle == Some(connection_handle) {
                        self.connection_handle = None;
                        self.connection_role = None;
                        self.peer_address = None;
                    }
                    if self.classic_connection_handle == Some(connection_handle) {
                        self.classic_connection_handle = None;
                    }
                    self.synchronous_connections
                        .retain(|connection| connection.connection_handle != connection_handle);
                }
                HciPacket::Event(Event::ConnectionComplete {
                    status: 0,
                    connection_handle,
                    link_type: 1,
                    ..
                }) => self.classic_connection_handle = Some(connection_handle),
                HciPacket::Event(Event::ConnectionRequest {
                    bd_addr, link_type, ..
                }) if link_type != 1 => self.synchronous_requests.push((bd_addr, link_type)),
                HciPacket::Event(Event::SynchronousConnectionComplete {
                    status: 0,
                    connection_handle,
                    bd_addr,
                    link_type,
                    air_mode,
                    ..
                }) => self
                    .synchronous_connections
                    .push(SynchronousConnectionInfo {
                        connection_handle,
                        peer_address: bd_addr,
                        link_type,
                        air_mode,
                    }),
                HciPacket::Event(Event::NumberOfCompletedPackets {
                    connection_handles,
                    num_completed_packets,
                }) => {
                    for (handle, count) in connection_handles
                        .into_iter()
                        .zip(num_completed_packets.into_iter())
                    {
                        let _ = self
                            .acl_packet_queue
                            .on_packets_completed(usize::from(count), handle);
                    }
                    self.flush_acl_queue(link);
                }
                HciPacket::Event(Event::EncryptionChange {
                    status,
                    connection_handle,
                    encryption_enabled,
                }) => {
                    if status == 0 && encryption_enabled != 0 {
                        self.encrypted_handles.insert(connection_handle);
                    } else {
                        self.encrypted_handles.remove(&connection_handle);
                    }
                }
                HciPacket::AclData(acl) => self.on_acl(link, acl),
                HciPacket::SyncData(packet) => self.synchronous_inbox.push(packet),
                HciPacket::IsoData(packet) => {
                    let handle = packet.connection_handle;
                    if let Some(sdu) = self.iso_assemblers.entry(handle).or_default().push(packet) {
                        self.iso_inbox.push(sdu);
                    }
                }
                _ => {}
            }
        }
        true
    }

    fn flush_acl_queue(&mut self, link: &mut LocalLink) -> bool {
        let mut success = true;
        while let Some(packet) = self.acl_packet_queue.poll_ready() {
            let handle = packet.connection_handle;
            if !link.send_acl_packet(self.controller_id, packet) {
                let _ = self.acl_packet_queue.on_packets_completed(1, handle);
                success = false;
            }
        }
        success
    }

    fn on_acl(&mut self, link: &mut LocalLink, acl: AclDataPacket) {
        let handle = acl.connection_handle;
        let Ok(Some(data)) = self.acl_assemblers.entry(handle).or_default().feed(&acl) else {
            return;
        };
        let Ok(l2cap) = L2capPdu::from_bytes(&data) else {
            return;
        };
        // Non-ATT channels (e.g. SMP on 0x0006) are queued raw for the caller.
        if l2cap.cid != ATT_CID {
            if l2cap.cid == SMP_CID && l2cap.payload.len() == 2 && l2cap.payload[0] == 0x0B {
                self.security_requests.push(l2cap.payload[1]);
            }
            self.l2cap_inbox.push((l2cap.cid, l2cap.payload));
            return;
        }
        let Ok(pdu) = AttPdu::from_bytes(&l2cap.payload) else {
            return;
        };

        // ATT commands are server inputs but never produce a response.
        if pdu.is_command() {
            if let Some(server) = self.server.as_mut() {
                let _ = server.handle_request(&pdu);
            }
            return;
        }

        // A server answers requests automatically; everything else is for the
        // client (this device's user) to collect.
        if is_request(&pdu) {
            let response = self
                .server
                .as_mut()
                .map(|server| server.handle_request(&pdu).to_bytes());
            if let Some(response) = response {
                self.send_l2cap_on_handle(link, handle, ATT_CID, &response);
                return;
            }
        }
        self.inbox.push(pdu);
    }
}

/// `true` if the ATT PDU is a request that expects a response.
fn is_request(pdu: &AttPdu) -> bool {
    matches!(
        pdu,
        AttPdu::ExchangeMtuRequest { .. }
            | AttPdu::ReadRequest { .. }
            | AttPdu::ReadByTypeRequest { .. }
            | AttPdu::ReadByGroupTypeRequest { .. }
            | AttPdu::WriteRequest { .. }
    )
}

/// Drive the devices until no further packets flow (quiescence). Each round
/// polls every device; the loop ends when a full round consumes nothing.
///
/// The cap is a safety backstop — a request/response exchange settles in a few
/// rounds because each ACL packet is consumed once and the server answers each
/// request exactly once.
pub fn pump(link: &mut LocalLink, devices: &mut [Device]) {
    for _ in 0..64 {
        // Commands such as LE Enable Encryption and remote-feature exchange
        // enqueue link-layer control PDUs rather than host events directly.
        link.pump_ll();
        let mut active = false;
        for device in devices.iter_mut() {
            if device.poll(link) {
                active = true;
            }
        }
        if !active {
            break;
        }
    }
}
