use bumble::keys::{KeyStore, MemoryKeyStore};
use bumble::{Address, AddressType, Key, PairingKeys};
use bumble_controller::{Controller, LocalLink};
use bumble_hci::{
    AclDataPacket, AdvertisingReport, Command, Event, HciPacket, IsoDataPacket, LeMetaEvent,
};
use bumble_host::{
    pump, Device, DeviceEvent, HostTransport, PeerLookupError, PeerLookupResult,
    PeerLookupTransport,
};
use bumble_smp::resolvable_private_address;

#[derive(Default)]
struct ScriptedTransport {
    commands: Vec<Command>,
    events: Vec<HciPacket>,
}

impl HostTransport for ScriptedTransport {
    fn handle_command(&mut self, _controller_id: usize, command: Command) {
        self.commands.push(command);
    }

    fn send_acl_packet(&mut self, _controller_id: usize, _packet: AclDataPacket) -> bool {
        false
    }

    fn send_synchronous_data(
        &mut self,
        _controller_id: usize,
        _connection_handle: u16,
        _packet_status: u8,
        _data: &[u8],
    ) -> bool {
        false
    }

    fn send_iso_packet(&mut self, _controller_id: usize, _packet: IsoDataPacket) -> bool {
        false
    }

    fn drain_host_events(&mut self, _controller_id: usize) -> Vec<HciPacket> {
        core::mem::take(&mut self.events)
    }
}

fn public_address(value: &str) -> Address {
    Address::parse(value, AddressType::PUBLIC_DEVICE).unwrap()
}

fn random_identity(value: &str) -> Address {
    Address::parse(value, AddressType::RANDOM_IDENTITY).unwrap()
}

fn local_name(name: &str, complete: bool) -> Vec<u8> {
    let mut data = Vec::with_capacity(name.len() + 2);
    data.push((name.len() + 1) as u8);
    data.push(if complete { 0x09 } else { 0x08 });
    data.extend_from_slice(name.as_bytes());
    data
}

fn legacy_advertisement(address: Address, data: Vec<u8>) -> HciPacket {
    HciPacket::Event(Event::LeMeta(LeMetaEvent::AdvertisingReport {
        reports: vec![
            AdvertisingReport {
                event_type: 0x00,
                address_type: 1,
                address: address.clone(),
                data,
                rssi: -40,
            },
            AdvertisingReport {
                event_type: 0x04,
                address_type: 1,
                address,
                data: Vec::new(),
                rssi: -39,
            },
        ],
    }))
}

#[test]
fn le_name_lookup_owns_scan_and_emits_a_correlated_result() {
    let peer = random_identity("C0:00:00:00:00:02");
    let mut device = Device::new(0);
    let mut transport = ScriptedTransport::default();
    let lookup_id = device.find_peer_by_name(&mut transport, "Named LE", PeerLookupTransport::Le);

    assert!(device.is_scanning());
    assert!(device.is_peer_lookup_pending(lookup_id));
    assert!(matches!(
        transport.commands.as_slice(),
        [
            Command::LeSetScanParameters { .. },
            Command::LeSetScanEnable {
                le_scan_enable: 1,
                filter_duplicates: 1,
            }
        ]
    ));

    transport.events.push(legacy_advertisement(
        peer.clone(),
        local_name("Named LE", true),
    ));
    assert!(device.poll(&mut transport));

    let result = PeerLookupResult {
        lookup_id,
        transport: PeerLookupTransport::Le,
        peer_address: peer,
    };
    assert_eq!(device.take_peer_lookup_results(), vec![result.clone()]);
    assert!(!device.is_peer_lookup_pending(lookup_id));
    assert!(!device.is_scanning());
    assert_eq!(device.pending_peer_lookup_count(), 0);
    assert_eq!(
        transport.commands.last(),
        Some(&Command::LeSetScanEnable {
            le_scan_enable: 0,
            filter_duplicates: 0,
        })
    );
    assert!(device
        .take_device_events()
        .contains(&DeviceEvent::PeerFound(result)));
}

#[test]
fn lookup_preserves_an_application_owned_scan_and_concurrent_cancel_waits_for_last() {
    let mut device = Device::new(0);
    let mut transport = ScriptedTransport::default();
    device.start_scanning(&mut transport, true, false);
    let application_command_count = transport.commands.len();

    let matching = device.find_peer_by_name(&mut transport, "Keep Scan", PeerLookupTransport::Le);
    assert_eq!(transport.commands.len(), application_command_count);
    transport.events.push(legacy_advertisement(
        random_identity("C0:00:00:00:00:03"),
        local_name("Keep Scan", false),
    ));
    assert!(device.poll(&mut transport));
    assert!(!device.is_peer_lookup_pending(matching));
    assert!(device.is_scanning());
    assert_eq!(transport.commands.len(), application_command_count);

    device.stop_scanning(&mut transport);
    let first = device.find_peer_by_name(&mut transport, "First", PeerLookupTransport::Le);
    let second = device.find_peer_by_name(&mut transport, "Second", PeerLookupTransport::Le);
    assert!(device.cancel_peer_lookup(&mut transport, first));
    assert!(device.is_scanning());
    assert!(device.is_peer_lookup_pending(second));
    assert!(device.cancel_peer_lookup(&mut transport, second));
    assert!(!device.is_scanning());
    assert!(!device.cancel_peer_lookup(&mut transport, second));

    let _le = device.find_peer_by_name(&mut transport, "Flush LE", PeerLookupTransport::Le);
    let _classic = device.find_peer_by_name(
        &mut transport,
        "Flush Classic",
        PeerLookupTransport::Classic,
    );
    assert_eq!(device.pending_peer_lookup_count(), 2);
    device.power_off();
    assert_eq!(device.pending_peer_lookup_count(), 0);
    assert!(!device.is_scanning());
    assert!(!device.is_discovering());
}

#[test]
fn classic_name_lookup_matches_eir_and_stops_owned_inquiry() {
    let peer = public_address("22:33:44:55:66:77");
    let mut device = Device::new(0);
    let mut transport = ScriptedTransport::default();
    let lookup_id =
        device.find_peer_by_name(&mut transport, "Classic Peer", PeerLookupTransport::Classic);
    assert!(device.is_discovering());

    let mut eir = [0; 240];
    let name = local_name("Classic Peer", true);
    eir[..name.len()].copy_from_slice(&name);
    transport
        .events
        .push(HciPacket::Event(Event::ExtendedInquiryResult {
            num_responses: 1,
            bd_addr: peer.clone(),
            page_scan_repetition_mode: 1,
            reserved: 0,
            class_of_device: 0x240404,
            clock_offset: 0,
            rssi: -31,
            extended_inquiry_response: eir,
        }));
    assert!(device.poll(&mut transport));

    assert_eq!(
        device.take_peer_lookup_results(),
        vec![PeerLookupResult {
            lookup_id,
            transport: PeerLookupTransport::Classic,
            peer_address: peer,
        }]
    );
    assert!(!device.is_discovering());
    assert_eq!(transport.commands.last(), Some(&Command::InquiryCancel));

    let command_count = transport.commands.len();
    transport
        .events
        .push(HciPacket::Event(Event::InquiryComplete { status: 0 }));
    assert!(device.poll(&mut transport));
    assert!(!device.is_discovering());
    assert_eq!(transport.commands.len(), command_count);
}

#[test]
fn identity_lookup_resolves_the_current_rpa_from_bonded_keys() {
    let irk = [0x55; 16];
    let identity = random_identity("C4:F2:17:1A:1D:BB");
    let rpa = resolvable_private_address(&irk, [0x12, 0x34, 0x56]);
    let mut store = MemoryKeyStore::new();
    store
        .update(
            &identity.to_string(false),
            PairingKeys {
                address_type: Some(AddressType::RANDOM_IDENTITY),
                irk: Some(Key::new(irk.to_vec())),
                ..PairingKeys::default()
            },
        )
        .unwrap();

    let mut device = Device::new(0);
    let mut transport = ScriptedTransport::default();
    assert_eq!(
        device.find_peer_by_identity_address(&mut transport, identity.clone()),
        Err(PeerLookupError::NoAddressResolver)
    );
    device.set_key_store(store);
    assert_eq!(device.refresh_address_resolver().unwrap(), 1);
    assert!(device.address_resolver().is_some());
    let lookup_id = device
        .find_peer_by_identity_address(&mut transport, identity)
        .unwrap();

    transport
        .events
        .push(legacy_advertisement(rpa.clone(), Vec::new()));
    assert!(device.poll(&mut transport));
    assert_eq!(
        device.take_peer_lookup_results(),
        vec![PeerLookupResult {
            lookup_id,
            transport: PeerLookupTransport::Le,
            peer_address: rpa,
        }]
    );
    assert!(!device.is_scanning());
}

#[test]
fn discovered_extended_advertising_support_selects_extended_scanning() {
    let mut link = LocalLink::new();
    let controller_id = link.add_controller(Controller::new(
        "lookup",
        public_address("00:11:22:33:44:55"),
    ));
    let mut device = Device::new(controller_id);
    device.power_on(&mut link).unwrap();
    pump(&mut link, std::slice::from_mut(&mut device));
    assert!(device.supports_le_extended_advertising());
    assert!(device.address_resolver().is_some());

    let mut transport = ScriptedTransport::default();
    let lookup_id =
        device.find_peer_by_name(&mut transport, "Extended Peer", PeerLookupTransport::Le);
    assert!(matches!(
        transport.commands.as_slice(),
        [
            Command::LeSetExtendedScanParameters { .. },
            Command::LeSetExtendedScanEnable {
                enable: 1,
                filter_duplicates: 1,
                ..
            }
        ]
    ));
    assert!(device.cancel_peer_lookup(&mut transport, lookup_id));
    assert!(matches!(
        transport.commands.last(),
        Some(Command::LeSetExtendedScanEnable { enable: 0, .. })
    ));
}
