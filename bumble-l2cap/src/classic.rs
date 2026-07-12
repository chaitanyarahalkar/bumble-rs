//! Synchronous, sans-I/O BR/EDR connection-oriented L2CAP channels.
//!
//! [`ChannelManager`] owns the signaling state and emits complete [`L2capPdu`]
//! values through [`ChannelManager::poll_outbound`]. A host or an in-memory
//! test relay feeds peer PDUs back through [`ChannelManager::process_pdu`].

use std::collections::{BTreeMap, VecDeque};

use crate::{
    decode_configuration_options, encode_configuration_options, ConfigurationOption, ControlFrame,
    Error, L2capPdu, Result, CONFIGURATION_OPTION_MTU, CONFIGURATION_SUCCESS,
    CONFIGURATION_UNACCEPTABLE_PARAMETERS, CONFIGURATION_UNKNOWN_OPTIONS,
    CONNECTION_REFUSED_NO_RESOURCES_AVAILABLE, CONNECTION_REFUSED_PSM_NOT_SUPPORTED,
    CONNECTION_SUCCESSFUL, L2CAP_SIGNALING_CID,
};

pub const L2CAP_MIN_BR_EDR_MTU: u16 = 48;
pub const L2CAP_DEFAULT_MTU: u16 = 2048;
pub const L2CAP_ACL_U_DYNAMIC_CID_RANGE_START: u16 = 0x0040;
pub const L2CAP_ACL_U_DYNAMIC_CID_RANGE_END: u16 = 0xffff;
pub const L2CAP_PSM_DYNAMIC_RANGE_START: u32 = 0x1001;
pub const L2CAP_PSM_DYNAMIC_RANGE_END: u32 = 0xffff;

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
    pub state: ClassicChannelState,
    /// `Some(0)` after a successful connection response, or the peer's refusal
    /// result after a failed outgoing connection attempt.
    pub connection_result: Option<u16>,
    incoming: bool,
    peer_configuration_received: bool,
    configuration_response_received: bool,
    open_announced: bool,
    received: VecDeque<Vec<u8>>,
}

impl ClassicChannel {
    fn outgoing(psm: u32, source_cid: u16, spec: ClassicChannelSpec) -> Self {
        Self {
            psm,
            source_cid,
            destination_cid: 0,
            mtu: spec.mtu,
            peer_mtu: L2CAP_MIN_BR_EDR_MTU,
            state: ClassicChannelState::WaitConnectResponse,
            connection_result: None,
            incoming: false,
            peer_configuration_received: false,
            configuration_response_received: false,
            open_announced: false,
            received: VecDeque::new(),
        }
    }

    fn incoming(psm: u32, source_cid: u16, destination_cid: u16, spec: ClassicChannelSpec) -> Self {
        Self {
            psm,
            source_cid,
            destination_cid,
            mtu: spec.mtu,
            peer_mtu: L2CAP_MIN_BR_EDR_MTU,
            state: ClassicChannelState::Configuring,
            connection_result: Some(CONNECTION_SUCCESSFUL),
            incoming: true,
            peer_configuration_received: false,
            configuration_response_received: false,
            open_announced: false,
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
    servers: BTreeMap<u32, ClassicChannelSpec>,
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

    pub fn process_bytes(&mut self, bytes: &[u8]) -> Result<()> {
        self.process_pdu(L2capPdu::from_bytes(bytes)?)
    }

    pub fn process_pdu(&mut self, pdu: L2capPdu) -> Result<()> {
        if pdu.cid == L2CAP_SIGNALING_CID {
            return self.process_control(ControlFrame::from_bytes(&pdu.payload)?);
        }

        let channel = self.channels.get_mut(&pdu.cid).ok_or_else(|| {
            Error::InvalidPacket(format!("channel not found for CID {:#06x}", pdu.cid))
        })?;
        if channel.state != ClassicChannelState::Open {
            return Err(Error::InvalidPacket(format!(
                "channel {:#06x} is not open",
                pdu.cid
            )));
        }
        if pdu.payload.len() > channel.mtu as usize {
            return Err(Error::InvalidPacket(format!(
                "incoming SDU exceeds channel MTU {}",
                channel.mtu
            )));
        }
        channel.received.push_back(pdu.payload);
        Ok(())
    }

    pub fn send(&mut self, source_cid: u16, sdu: &[u8]) -> Result<()> {
        let channel = self.channels.get(&source_cid).ok_or_else(|| {
            Error::InvalidPacket(format!("channel not found for CID {source_cid:#06x}"))
        })?;
        if channel.state != ClassicChannelState::Open {
            return Err(Error::InvalidPacket("channel not open".into()));
        }
        if sdu.len() > channel.peer_mtu as usize {
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
            for option in options {
                if option.option_type == CONFIGURATION_OPTION_MTU {
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
                } else if !option.hint {
                    result = CONFIGURATION_UNKNOWN_OPTIONS;
                    response_options = vec![option];
                    break;
                }
            }
            if result == CONFIGURATION_SUCCESS && flags & 1 == 0 {
                channel.peer_configuration_received = true;
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
        let (remote_cid, mtu) = {
            let channel = self.channels.get(&local_cid).ok_or_else(|| {
                Error::InvalidPacket(format!("channel not found for CID {local_cid:#06x}"))
            })?;
            (channel.destination_cid, channel.mtu)
        };
        let identifier = self.next_identifier();
        let options = encode_configuration_options(&[ConfigurationOption::new(
            CONFIGURATION_OPTION_MTU,
            mtu.to_le_bytes().to_vec(),
        )])?;
        self.queue_control(ControlFrame::ConfigureRequest {
            identifier,
            destination_cid: remote_cid,
            flags: 0,
            options,
        });
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
