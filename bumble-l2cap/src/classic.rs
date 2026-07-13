//! Synchronous, sans-I/O BR/EDR connection-oriented L2CAP channels.
//!
//! [`ChannelManager`] owns the signaling state and emits complete [`L2capPdu`]
//! values through [`ChannelManager::poll_outbound`]. A host or an in-memory
//! test relay feeds peer PDUs back through [`ChannelManager::process_pdu`].

use std::collections::{BTreeMap, VecDeque};

use crate::{
    crc_16, decode_configuration_options, encode_configuration_options, ConfigurationOption,
    ControlFrame, Error, ErtmConfig, ErtmEngine, L2capPdu, Result, CONFIGURATION_OPTION_FCS,
    CONFIGURATION_OPTION_MTU, CONFIGURATION_OPTION_RETRANSMISSION_AND_FLOW_CONTROL,
    CONFIGURATION_SUCCESS, CONFIGURATION_UNACCEPTABLE_PARAMETERS, CONFIGURATION_UNKNOWN_OPTIONS,
    CONNECTION_REFUSED_NO_RESOURCES_AVAILABLE, CONNECTION_REFUSED_PSM_NOT_SUPPORTED,
    CONNECTION_SUCCESSFUL, L2CAP_SIGNALING_CID,
};

pub const L2CAP_MIN_BR_EDR_MTU: u16 = 48;
pub const L2CAP_DEFAULT_MTU: u16 = 2048;
pub const L2CAP_DEFAULT_MPS: u16 = 1010;
pub const L2CAP_ACL_U_DYNAMIC_CID_RANGE_START: u16 = 0x0040;
pub const L2CAP_ACL_U_DYNAMIC_CID_RANGE_END: u16 = 0xffff;
pub const L2CAP_PSM_DYNAMIC_RANGE_START: u32 = 0x1001;
pub const L2CAP_PSM_DYNAMIC_RANGE_END: u32 = 0xffff;
pub const DEFAULT_ERTM_TX_WINDOW_SIZE: u8 = 63;
pub const DEFAULT_ERTM_MAX_RETRANSMISSIONS: u8 = 1;
pub const DEFAULT_ERTM_RETRANSMISSION_TIMEOUT_MS: u16 = 2_000;
pub const DEFAULT_ERTM_MONITOR_TIMEOUT_MS: u16 = 12_000;
pub const RETRANSMISSION_MODE_ENHANCED: u8 = 0x03;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClassicChannelSpec {
    pub mtu: u16,
}

impl Default for ClassicChannelSpec {
    fn default() -> Self {
        Self {
            mtu: L2CAP_DEFAULT_MTU,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ErtmChannelSpec {
    pub mtu: u16,
    pub mps: u16,
    pub tx_window_size: u8,
    pub max_retransmissions: u8,
    pub retransmission_timeout_ms: u16,
    pub monitor_timeout_ms: u16,
    pub fcs_enabled: bool,
}

impl Default for ErtmChannelSpec {
    fn default() -> Self {
        Self {
            mtu: L2CAP_DEFAULT_MTU,
            mps: L2CAP_DEFAULT_MPS,
            tx_window_size: DEFAULT_ERTM_TX_WINDOW_SIZE,
            max_retransmissions: DEFAULT_ERTM_MAX_RETRANSMISSIONS,
            retransmission_timeout_ms: DEFAULT_ERTM_RETRANSMISSION_TIMEOUT_MS,
            monitor_timeout_ms: DEFAULT_ERTM_MONITOR_TIMEOUT_MS,
            fcs_enabled: false,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClassicChannelMode {
    Basic,
    EnhancedRetransmission,
}

#[derive(Clone, Copy, Debug)]
enum ChannelSpec {
    Basic(ClassicChannelSpec),
    Ertm(ErtmChannelSpec),
}

impl ChannelSpec {
    fn mtu(self) -> u16 {
        match self {
            Self::Basic(spec) => spec.mtu,
            Self::Ertm(spec) => spec.mtu,
        }
    }

    fn mode(self) -> ClassicChannelMode {
        match self {
            Self::Basic(_) => ClassicChannelMode::Basic,
            Self::Ertm(_) => ClassicChannelMode::EnhancedRetransmission,
        }
    }

    fn ertm(self) -> Option<ErtmChannelSpec> {
        match self {
            Self::Basic(_) => None,
            Self::Ertm(spec) => Some(spec),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClassicChannelState {
    Closed,
    WaitConnectResponse,
    Configuring,
    Open,
    WaitDisconnect,
}

#[derive(Clone, Debug)]
pub struct ClassicChannel {
    pub psm: u32,
    pub source_cid: u16,
    pub destination_cid: u16,
    pub mtu: u16,
    pub peer_mtu: u16,
    pub peer_mps: u16,
    pub mode: ClassicChannelMode,
    pub fcs_enabled: bool,
    pub state: ClassicChannelState,
    /// `Some(0)` after a successful connection response, or the peer's refusal
    /// result after a failed outgoing connection attempt.
    pub connection_result: Option<u16>,
    incoming: bool,
    peer_configuration_received: bool,
    configuration_response_received: bool,
    open_announced: bool,
    spec: ChannelSpec,
    ertm: Option<ErtmEngine>,
    received: VecDeque<Vec<u8>>,
}

impl ClassicChannel {
    fn outgoing(psm: u32, source_cid: u16, spec: ChannelSpec) -> Self {
        Self {
            psm,
            source_cid,
            destination_cid: 0,
            mtu: spec.mtu(),
            peer_mtu: L2CAP_MIN_BR_EDR_MTU,
            peer_mps: L2CAP_DEFAULT_MPS,
            mode: spec.mode(),
            fcs_enabled: false,
            state: ClassicChannelState::WaitConnectResponse,
            connection_result: None,
            incoming: false,
            peer_configuration_received: false,
            configuration_response_received: false,
            open_announced: false,
            spec,
            ertm: None,
            received: VecDeque::new(),
        }
    }

    fn incoming(psm: u32, source_cid: u16, destination_cid: u16, spec: ChannelSpec) -> Self {
        Self {
            psm,
            source_cid,
            destination_cid,
            mtu: spec.mtu(),
            peer_mtu: L2CAP_MIN_BR_EDR_MTU,
            peer_mps: L2CAP_DEFAULT_MPS,
            mode: spec.mode(),
            fcs_enabled: false,
            state: ClassicChannelState::Configuring,
            connection_result: Some(CONNECTION_SUCCESSFUL),
            incoming: true,
            peer_configuration_received: false,
            configuration_response_received: false,
            open_announced: false,
            spec,
            ertm: None,
            received: VecDeque::new(),
        }
    }

    pub fn is_open(&self) -> bool {
        self.state == ClassicChannelState::Open
    }

    pub fn pop_received(&mut self) -> Option<Vec<u8>> {
        self.received.pop_front()
    }
}

#[derive(Debug, Default)]
pub struct ChannelManager {
    servers: BTreeMap<u32, ChannelSpec>,
    channels: BTreeMap<u16, ClassicChannel>,
    accepted_channels: VecDeque<u16>,
    outbound: VecDeque<L2capPdu>,
    identifier: u8,
}

impl ChannelManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a Classic PSM. Passing `None` allocates the first valid dynamic
    /// PSM, matching upstream Bumble's deterministic allocation policy.
    pub fn register_server(&mut self, psm: Option<u32>, spec: ClassicChannelSpec) -> Result<u32> {
        validate_spec(spec)?;
        self.register_server_with_spec(psm, ChannelSpec::Basic(spec))
    }

    /// Register an Enhanced Retransmission Mode server without changing the
    /// source-compatible basic-mode [`ClassicChannelSpec`] API.
    pub fn register_ertm_server(&mut self, psm: Option<u32>, spec: ErtmChannelSpec) -> Result<u32> {
        validate_ertm_spec(spec)?;
        self.register_server_with_spec(psm, ChannelSpec::Ertm(spec))
    }

    fn register_server_with_spec(&mut self, psm: Option<u32>, spec: ChannelSpec) -> Result<u32> {
        let psm = match psm {
            Some(psm) => {
                validate_psm(psm)?;
                psm
            }
            None => (L2CAP_PSM_DYNAMIC_RANGE_START..=L2CAP_PSM_DYNAMIC_RANGE_END)
                .step_by(2)
                .find(|candidate| {
                    validate_psm(*candidate).is_ok() && !self.servers.contains_key(candidate)
                })
                .ok_or_else(|| Error::InvalidPacket("no free Classic PSM".into()))?,
        };
        if self.servers.contains_key(&psm) {
            return Err(Error::InvalidPacket(format!(
                "PSM {psm:#x} is already in use"
            )));
        }
        self.servers.insert(psm, spec);
        Ok(psm)
    }

    pub fn unregister_server(&mut self, psm: u32) -> bool {
        self.servers.remove(&psm).is_some()
    }

    /// Begin an outgoing Classic channel connection and return its local CID.
    pub fn connect(&mut self, psm: u32, spec: ClassicChannelSpec) -> Result<u16> {
        validate_psm(psm)?;
        validate_spec(spec)?;
        self.connect_with_spec(psm, ChannelSpec::Basic(spec))
    }

    pub fn connect_ertm(&mut self, psm: u32, spec: ErtmChannelSpec) -> Result<u16> {
        validate_psm(psm)?;
        validate_ertm_spec(spec)?;
        self.connect_with_spec(psm, ChannelSpec::Ertm(spec))
    }

    fn connect_with_spec(&mut self, psm: u32, spec: ChannelSpec) -> Result<u16> {
        let source_cid = self.allocate_cid()?;
        let identifier = self.next_identifier();
        self.channels
            .insert(source_cid, ClassicChannel::outgoing(psm, source_cid, spec));
        self.queue_control(ControlFrame::ConnectionRequest {
            identifier,
            psm,
            source_cid,
        });
        Ok(source_cid)
    }

    pub fn channel(&self, source_cid: u16) -> Option<&ClassicChannel> {
        self.channels.get(&source_cid)
    }

    pub fn channel_mut(&mut self, source_cid: u16) -> Option<&mut ClassicChannel> {
        self.channels.get_mut(&source_cid)
    }

    /// Return the next server-side channel that completed configuration.
    pub fn poll_accepted_channel(&mut self) -> Option<u16> {
        self.accepted_channels.pop_front()
    }

    pub fn poll_outbound(&mut self) -> Option<L2capPdu> {
        self.outbound.pop_front()
    }

    pub fn drain_outbound(&mut self) -> Vec<L2capPdu> {
        self.outbound.drain(..).collect()
    }

    /// Advance every open ERTM channel's deterministic retransmission clock.
    pub fn tick(&mut self, ticks: u32) -> Result<()> {
        let cids: Vec<_> = self
            .channels
            .iter()
            .filter_map(|(cid, channel)| channel.ertm.is_some().then_some(*cid))
            .collect();
        for cid in cids {
            self.channels
                .get_mut(&cid)
                .expect("channel was just enumerated")
                .ertm
                .as_mut()
                .expect("ERTM engine was just checked")
                .tick(ticks)?;
            self.flush_ertm(cid)?;
        }
        Ok(())
    }

    pub fn set_receiver_busy(&mut self, source_cid: u16, busy: bool) -> Result<()> {
        self.channels
            .get_mut(&source_cid)
            .ok_or_else(|| {
                Error::InvalidPacket(format!("channel not found for CID {source_cid:#06x}"))
            })?
            .ertm
            .as_mut()
            .ok_or_else(|| Error::InvalidPacket("channel is not using ERTM".into()))?
            .set_receiver_busy(busy)?;
        self.flush_ertm(source_cid)
    }

    pub fn ertm_pending_frames(&self, source_cid: u16) -> Option<usize> {
        self.channels
            .get(&source_cid)?
            .ertm
            .as_ref()
            .map(ErtmEngine::pending_frames)
    }

    pub fn process_bytes(&mut self, bytes: &[u8]) -> Result<()> {
        self.process_pdu(L2capPdu::from_bytes(bytes)?)
    }

    pub fn process_pdu(&mut self, pdu: L2capPdu) -> Result<()> {
        if pdu.cid == L2CAP_SIGNALING_CID {
            return self.process_control(ControlFrame::from_bytes(&pdu.payload)?);
        }

        let local_cid = pdu.cid;
        let channel = self.channels.get_mut(&local_cid).ok_or_else(|| {
            Error::InvalidPacket(format!("channel not found for CID {local_cid:#06x}"))
        })?;
        if channel.state != ClassicChannelState::Open {
            return Err(Error::InvalidPacket(format!(
                "channel {local_cid:#06x} is not open"
            )));
        }
        if let Some(engine) = channel.ertm.as_mut() {
            let payload = if channel.fcs_enabled {
                verify_and_strip_fcs(local_cid, &pdu.payload)?
            } else {
                pdu.payload
            };
            engine.receive_frame(&payload)?;
            self.flush_ertm(local_cid)
        } else {
            if pdu.payload.len() > usize::from(channel.mtu) {
                return Err(Error::InvalidPacket(format!(
                    "incoming SDU exceeds channel MTU {}",
                    channel.mtu
                )));
            }
            channel.received.push_back(pdu.payload);
            Ok(())
        }
    }

    pub fn send(&mut self, source_cid: u16, sdu: &[u8]) -> Result<()> {
        let channel = self.channels.get_mut(&source_cid).ok_or_else(|| {
            Error::InvalidPacket(format!("channel not found for CID {source_cid:#06x}"))
        })?;
        if channel.state != ClassicChannelState::Open {
            return Err(Error::InvalidPacket("channel not open".into()));
        }
        if let Some(engine) = channel.ertm.as_mut() {
            engine.send_sdu(sdu)?;
            self.flush_ertm(source_cid)
        } else {
            if sdu.len() > usize::from(channel.peer_mtu) {
                return Err(Error::InvalidPacket(format!(
                    "SDU exceeds peer MTU {}",
                    channel.peer_mtu
                )));
            }
            let destination_cid = channel.destination_cid;
            self.outbound
                .push_back(L2capPdu::new(destination_cid, sdu.to_vec()));
            Ok(())
        }
    }

    pub fn disconnect(&mut self, source_cid: u16) -> Result<()> {
        let destination_cid = {
            let channel = self.channels.get_mut(&source_cid).ok_or_else(|| {
                Error::InvalidPacket(format!("channel not found for CID {source_cid:#06x}"))
            })?;
            if channel.state != ClassicChannelState::Open {
                return Err(Error::InvalidPacket("channel not open".into()));
            }
            channel.state = ClassicChannelState::WaitDisconnect;
            channel.destination_cid
        };
        let identifier = self.next_identifier();
        self.queue_control(ControlFrame::DisconnectionRequest {
            identifier,
            destination_cid,
            source_cid,
        });
        Ok(())
    }

    fn process_control(&mut self, frame: ControlFrame) -> Result<()> {
        match frame {
            ControlFrame::ConnectionRequest {
                identifier,
                psm,
                source_cid,
            } => self.on_connection_request(identifier, psm, source_cid),
            ControlFrame::ConnectionResponse {
                destination_cid,
                source_cid,
                result,
                ..
            } => self.on_connection_response(destination_cid, source_cid, result),
            ControlFrame::ConfigureRequest {
                identifier,
                destination_cid,
                flags,
                options,
            } => self.on_configure_request(identifier, destination_cid, flags, &options),
            ControlFrame::ConfigureResponse {
                source_cid, result, ..
            } => self.on_configure_response(source_cid, result),
            ControlFrame::DisconnectionRequest {
                identifier,
                destination_cid,
                source_cid,
            } => self.on_disconnection_request(identifier, destination_cid, source_cid),
            ControlFrame::DisconnectionResponse {
                destination_cid,
                source_cid,
                ..
            } => self.on_disconnection_response(destination_cid, source_cid),
            _ => Ok(()),
        }
    }

    fn on_connection_request(&mut self, identifier: u8, psm: u32, remote_cid: u16) -> Result<()> {
        let Some(spec) = self.servers.get(&psm).copied() else {
            self.queue_control(ControlFrame::ConnectionResponse {
                identifier,
                destination_cid: 0,
                source_cid: remote_cid,
                result: CONNECTION_REFUSED_PSM_NOT_SUPPORTED,
                status: 0,
            });
            return Ok(());
        };

        let local_cid = match self.allocate_cid() {
            Ok(cid) => cid,
            Err(_) => {
                self.queue_control(ControlFrame::ConnectionResponse {
                    identifier,
                    destination_cid: 0,
                    source_cid: remote_cid,
                    result: CONNECTION_REFUSED_NO_RESOURCES_AVAILABLE,
                    status: 0,
                });
                return Ok(());
            }
        };
        self.channels.insert(
            local_cid,
            ClassicChannel::incoming(psm, local_cid, remote_cid, spec),
        );
        self.queue_control(ControlFrame::ConnectionResponse {
            identifier,
            destination_cid: local_cid,
            source_cid: remote_cid,
            result: CONNECTION_SUCCESSFUL,
            status: 0,
        });
        self.queue_configure_request(local_cid)
    }

    fn on_connection_response(
        &mut self,
        destination_cid: u16,
        source_cid: u16,
        result: u16,
    ) -> Result<()> {
        let channel = self.channels.get_mut(&source_cid).ok_or_else(|| {
            Error::InvalidPacket(format!(
                "connection response for unknown CID {source_cid:#06x}"
            ))
        })?;
        if channel.state != ClassicChannelState::WaitConnectResponse {
            return Err(Error::InvalidPacket(
                "connection response in invalid state".into(),
            ));
        }
        channel.connection_result = Some(result);
        if result != CONNECTION_SUCCESSFUL {
            channel.state = ClassicChannelState::Closed;
            return Ok(());
        }
        channel.destination_cid = destination_cid;
        channel.state = ClassicChannelState::Configuring;
        self.queue_configure_request(source_cid)
    }

    fn on_configure_request(
        &mut self,
        identifier: u8,
        local_cid: u16,
        flags: u16,
        encoded_options: &[u8],
    ) -> Result<()> {
        let options = decode_configuration_options(encoded_options)?;
        let mut result = CONFIGURATION_SUCCESS;
        let mut response_options = Vec::new();
        let remote_cid;
        {
            let channel = self.channels.get_mut(&local_cid).ok_or_else(|| {
                Error::InvalidPacket(format!(
                    "configuration request for unknown CID {local_cid:#06x}"
                ))
            })?;
            remote_cid = channel.destination_cid;
            let mut peer_ertm = None;
            let mut peer_fcs = None;
            for option in options {
                match option.option_type {
                    CONFIGURATION_OPTION_MTU => {
                        if option.value.len() != 2 {
                            result = CONFIGURATION_UNACCEPTABLE_PARAMETERS;
                            response_options = vec![option];
                            break;
                        }
                        let mtu = u16::from_le_bytes([option.value[0], option.value[1]]);
                        if mtu < L2CAP_MIN_BR_EDR_MTU {
                            result = CONFIGURATION_UNACCEPTABLE_PARAMETERS;
                            response_options = vec![ConfigurationOption::new(
                                CONFIGURATION_OPTION_MTU,
                                L2CAP_MIN_BR_EDR_MTU.to_le_bytes().to_vec(),
                            )];
                            break;
                        }
                        channel.peer_mtu = mtu;
                        response_options.push(option);
                    }
                    CONFIGURATION_OPTION_RETRANSMISSION_AND_FLOW_CONTROL => {
                        let Some(parameters) = decode_ertm_option(&option.value) else {
                            result = CONFIGURATION_UNACCEPTABLE_PARAMETERS;
                            response_options = vec![option];
                            break;
                        };
                        let expected_mode = match channel.mode {
                            ClassicChannelMode::Basic => 0,
                            ClassicChannelMode::EnhancedRetransmission => {
                                RETRANSMISSION_MODE_ENHANCED
                            }
                        };
                        if parameters.mode != expected_mode
                            || (parameters.mode == RETRANSMISSION_MODE_ENHANCED
                                && (parameters.tx_window_size == 0
                                    || parameters.tx_window_size >= 64
                                    || parameters.mps == 0))
                        {
                            result = CONFIGURATION_UNACCEPTABLE_PARAMETERS;
                            response_options = vec![option];
                            break;
                        }
                        peer_ertm = Some(parameters);
                        response_options.push(option);
                    }
                    CONFIGURATION_OPTION_FCS => {
                        if option.value.len() != 1 || option.value[0] > 1 {
                            result = CONFIGURATION_UNACCEPTABLE_PARAMETERS;
                            response_options = vec![option];
                            break;
                        }
                        peer_fcs = Some(option.value[0] != 0);
                        response_options.push(option);
                    }
                    _ if !option.hint => {
                        result = CONFIGURATION_UNKNOWN_OPTIONS;
                        response_options = vec![option];
                        break;
                    }
                    _ => {}
                }
            }
            if result == CONFIGURATION_SUCCESS && flags & 1 == 0 {
                if let Some(spec) = channel.spec.ertm() {
                    if let Some(parameters) = peer_ertm {
                        if peer_fcs.unwrap_or(false) != spec.fcs_enabled {
                            result = CONFIGURATION_UNACCEPTABLE_PARAMETERS;
                            response_options = vec![ConfigurationOption::new(
                                CONFIGURATION_OPTION_FCS,
                                vec![u8::from(spec.fcs_enabled)],
                            )];
                        } else {
                            channel.peer_mps = parameters.mps;
                            channel.fcs_enabled = spec.fcs_enabled;
                            channel.ertm = Some(ErtmEngine::new(ErtmConfig {
                                // Upstream's ERTM processor treats the negotiated
                                // MTUs as channel metadata and accepts 16-bit SDUs
                                // beyond them; preserve that tested behavior here.
                                local_mtu: u16::MAX,
                                peer_mtu: u16::MAX,
                                local_mps: spec.mps,
                                peer_mps: parameters.mps,
                                tx_window_size: parameters.tx_window_size,
                                max_retransmissions: parameters.max_retransmissions,
                                retransmission_timeout_ticks: u32::from(
                                    spec.retransmission_timeout_ms,
                                ),
                            })?);
                        }
                    } else {
                        result = CONFIGURATION_UNACCEPTABLE_PARAMETERS;
                        response_options = vec![ConfigurationOption::new(
                            CONFIGURATION_OPTION_RETRANSMISSION_AND_FLOW_CONTROL,
                            encode_ertm_option(spec),
                        )];
                    }
                } else if peer_ertm.is_some() || peer_fcs.unwrap_or(false) {
                    result = CONFIGURATION_UNACCEPTABLE_PARAMETERS;
                }
                if result == CONFIGURATION_SUCCESS {
                    channel.peer_configuration_received = true;
                } else {
                    channel.state = ClassicChannelState::Closed;
                    channel.connection_result = Some(result);
                }
            }
        }

        self.queue_control(ControlFrame::ConfigureResponse {
            identifier,
            source_cid: remote_cid,
            flags: 0,
            result,
            options: encode_configuration_options(&response_options)?,
        });
        self.maybe_open(local_cid);
        Ok(())
    }

    fn on_configure_response(&mut self, local_cid: u16, result: u16) -> Result<()> {
        let channel = self.channels.get_mut(&local_cid).ok_or_else(|| {
            Error::InvalidPacket(format!(
                "configuration response for unknown CID {local_cid:#06x}"
            ))
        })?;
        if result == CONFIGURATION_SUCCESS {
            channel.configuration_response_received = true;
        } else {
            channel.connection_result = Some(result);
            channel.state = ClassicChannelState::Closed;
        }
        self.maybe_open(local_cid);
        Ok(())
    }

    fn on_disconnection_request(
        &mut self,
        identifier: u8,
        local_cid: u16,
        remote_cid: u16,
    ) -> Result<()> {
        let channel = self.channels.get_mut(&local_cid).ok_or_else(|| {
            Error::InvalidPacket(format!(
                "disconnection request for unknown CID {local_cid:#06x}"
            ))
        })?;
        if channel.destination_cid != remote_cid {
            return Err(Error::InvalidPacket("disconnection CID mismatch".into()));
        }
        channel.state = ClassicChannelState::Closed;
        self.queue_control(ControlFrame::DisconnectionResponse {
            identifier,
            destination_cid: local_cid,
            source_cid: remote_cid,
        });
        Ok(())
    }

    fn on_disconnection_response(&mut self, destination_cid: u16, source_cid: u16) -> Result<()> {
        let channel = self.channels.get_mut(&source_cid).ok_or_else(|| {
            Error::InvalidPacket(format!(
                "disconnection response for unknown CID {source_cid:#06x}"
            ))
        })?;
        if channel.destination_cid != destination_cid {
            return Err(Error::InvalidPacket("disconnection CID mismatch".into()));
        }
        channel.state = ClassicChannelState::Closed;
        Ok(())
    }

    fn queue_configure_request(&mut self, local_cid: u16) -> Result<()> {
        let (remote_cid, spec) = {
            let channel = self.channels.get(&local_cid).ok_or_else(|| {
                Error::InvalidPacket(format!("channel not found for CID {local_cid:#06x}"))
            })?;
            (channel.destination_cid, channel.spec)
        };
        let identifier = self.next_identifier();
        let mut options = vec![ConfigurationOption::new(
            CONFIGURATION_OPTION_MTU,
            spec.mtu().to_le_bytes().to_vec(),
        )];
        if let Some(ertm) = spec.ertm() {
            options.push(ConfigurationOption::new(
                CONFIGURATION_OPTION_RETRANSMISSION_AND_FLOW_CONTROL,
                encode_ertm_option(ertm),
            ));
            if ertm.fcs_enabled {
                options.push(ConfigurationOption::new(CONFIGURATION_OPTION_FCS, vec![1]));
            }
        }
        let options = encode_configuration_options(&options)?;
        self.queue_control(ControlFrame::ConfigureRequest {
            identifier,
            destination_cid: remote_cid,
            flags: 0,
            options,
        });
        Ok(())
    }

    fn flush_ertm(&mut self, local_cid: u16) -> Result<()> {
        let (destination_cid, fcs_enabled, frames, received) = {
            let channel = self.channels.get_mut(&local_cid).ok_or_else(|| {
                Error::InvalidPacket(format!("channel not found for CID {local_cid:#06x}"))
            })?;
            let engine = channel
                .ertm
                .as_mut()
                .ok_or_else(|| Error::InvalidPacket("channel is not using ERTM".into()))?;
            let frames = engine.drain_outbound();
            let mut received = Vec::new();
            while let Some(sdu) = engine.pop_received() {
                received.push(sdu);
            }
            (
                channel.destination_cid,
                channel.fcs_enabled,
                frames,
                received,
            )
        };
        self.channels
            .get_mut(&local_cid)
            .expect("ERTM channel was just found")
            .received
            .extend(received);
        for frame in frames {
            let payload = if fcs_enabled {
                append_fcs(destination_cid, frame)?
            } else {
                frame
            };
            self.outbound
                .push_back(L2capPdu::new(destination_cid, payload));
        }
        Ok(())
    }

    fn maybe_open(&mut self, local_cid: u16) {
        let Some(channel) = self.channels.get_mut(&local_cid) else {
            return;
        };
        if channel.peer_configuration_received
            && channel.configuration_response_received
            && channel.state == ClassicChannelState::Configuring
        {
            channel.state = ClassicChannelState::Open;
            if channel.incoming && !channel.open_announced {
                channel.open_announced = true;
                self.accepted_channels.push_back(local_cid);
            }
        }
    }

    fn allocate_cid(&self) -> Result<u16> {
        (L2CAP_ACL_U_DYNAMIC_CID_RANGE_START..=L2CAP_ACL_U_DYNAMIC_CID_RANGE_END)
            .find(|cid| !self.channels.contains_key(cid))
            .ok_or_else(|| Error::InvalidPacket("no free Classic CID".into()))
    }

    fn next_identifier(&mut self) -> u8 {
        self.identifier = self.identifier.wrapping_add(1);
        if self.identifier == 0 {
            self.identifier = 1;
        }
        self.identifier
    }

    fn queue_control(&mut self, frame: ControlFrame) {
        self.outbound
            .push_back(L2capPdu::new(L2CAP_SIGNALING_CID, frame.to_bytes()));
    }
}

fn validate_spec(spec: ClassicChannelSpec) -> Result<()> {
    if spec.mtu < L2CAP_MIN_BR_EDR_MTU {
        return Err(Error::InvalidPacket(format!(
            "Classic MTU must be at least {L2CAP_MIN_BR_EDR_MTU}"
        )));
    }
    Ok(())
}

fn validate_ertm_spec(spec: ErtmChannelSpec) -> Result<()> {
    validate_spec(ClassicChannelSpec { mtu: spec.mtu })?;
    if spec.mps == 0 {
        return Err(Error::InvalidPacket("ERTM MPS cannot be zero".into()));
    }
    if !(1..64).contains(&spec.tx_window_size) {
        return Err(Error::InvalidPacket(
            "ERTM transmit window must be between 1 and 63".into(),
        ));
    }
    if spec.retransmission_timeout_ms == 0 || spec.monitor_timeout_ms == 0 {
        return Err(Error::InvalidPacket("ERTM timeout cannot be zero".into()));
    }
    Ok(())
}

#[derive(Clone, Copy, Debug)]
struct ErtmOption {
    mode: u8,
    tx_window_size: u8,
    max_retransmissions: u8,
    mps: u16,
}

fn encode_ertm_option(spec: ErtmChannelSpec) -> Vec<u8> {
    let mut value = vec![
        RETRANSMISSION_MODE_ENHANCED,
        spec.tx_window_size,
        spec.max_retransmissions,
    ];
    value.extend_from_slice(&spec.retransmission_timeout_ms.to_le_bytes());
    value.extend_from_slice(&spec.monitor_timeout_ms.to_le_bytes());
    value.extend_from_slice(&spec.mps.to_le_bytes());
    value
}

fn decode_ertm_option(value: &[u8]) -> Option<ErtmOption> {
    (value.len() == 9).then(|| ErtmOption {
        mode: value[0],
        tx_window_size: value[1],
        max_retransmissions: value[2],
        mps: u16::from_le_bytes([value[7], value[8]]),
    })
}

fn append_fcs(cid: u16, mut payload: Vec<u8>) -> Result<Vec<u8>> {
    let length = u16::try_from(payload.len() + 2)
        .map_err(|_| Error::InvalidPacket("ERTM frame is too large".into()))?;
    let mut checked = Vec::with_capacity(payload.len() + 4);
    checked.extend_from_slice(&length.to_le_bytes());
    checked.extend_from_slice(&cid.to_le_bytes());
    checked.extend_from_slice(&payload);
    payload.extend_from_slice(&crc_16(&checked).to_le_bytes());
    Ok(payload)
}

fn verify_and_strip_fcs(cid: u16, payload: &[u8]) -> Result<Vec<u8>> {
    if payload.len() < 2 {
        return Err(Error::InvalidPacket("ERTM frame is missing its FCS".into()));
    }
    let frame_length = u16::try_from(payload.len())
        .map_err(|_| Error::InvalidPacket("ERTM frame is too large".into()))?;
    let split = payload.len() - 2;
    let expected = u16::from_le_bytes([payload[split], payload[split + 1]]);
    let mut checked = Vec::with_capacity(payload.len() + 2);
    checked.extend_from_slice(&frame_length.to_le_bytes());
    checked.extend_from_slice(&cid.to_le_bytes());
    checked.extend_from_slice(&payload[..split]);
    if crc_16(&checked) != expected {
        return Err(Error::InvalidPacket("ERTM frame has an invalid FCS".into()));
    }
    Ok(payload[..split].to_vec())
}

fn validate_psm(psm: u32) -> Result<()> {
    if psm == 0 || psm & 1 == 0 {
        return Err(Error::InvalidPacket("invalid Classic PSM".into()));
    }
    let mut high = psm >> 8;
    while high != 0 {
        if high & 1 != 0 {
            return Err(Error::InvalidPacket("invalid Classic PSM".into()));
        }
        high >>= 8;
    }
    Ok(())
}
