//! Synchronous LE credit-based channel segmentation and credit accounting.

use std::collections::{BTreeMap, VecDeque};

use crate::{ControlFrame, Error, L2capPdu, Result, L2CAP_LE_SIGNALING_CID};

pub const L2CAP_LE_CREDIT_BASED_CONNECTION_MAX_CREDITS: u16 = u16::MAX;
pub const L2CAP_LE_CREDIT_BASED_CONNECTION_MIN_MTU: u16 = 23;
pub const L2CAP_LE_CREDIT_BASED_CONNECTION_MAX_MTU: u16 = u16::MAX;
pub const L2CAP_LE_CREDIT_BASED_CONNECTION_MIN_MPS: u16 = 23;
pub const L2CAP_LE_CREDIT_BASED_CONNECTION_MAX_MPS: u16 = 65_533;
pub const L2CAP_LE_CREDIT_BASED_CONNECTION_DEFAULT_MTU: u16 = 2048;
pub const L2CAP_LE_CREDIT_BASED_CONNECTION_DEFAULT_MPS: u16 = 2048;
pub const L2CAP_LE_CREDIT_BASED_CONNECTION_DEFAULT_INITIAL_CREDITS: u16 = 256;
pub const L2CAP_LE_U_DYNAMIC_CID_RANGE_START: u16 = 0x0040;
pub const L2CAP_LE_U_DYNAMIC_CID_RANGE_END: u16 = 0x007F;
pub const L2CAP_LE_PSM_DYNAMIC_RANGE_START: u16 = 0x0080;
pub const L2CAP_LE_PSM_DYNAMIC_RANGE_END: u16 = 0x00FF;
pub const L2CAP_CREDIT_BASED_CONNECTION_MAX_CHANNELS: usize = 5;

pub const LE_CONNECTION_SUCCESSFUL: u16 = 0x0000;
pub const LE_CONNECTION_REFUSED_PSM_NOT_SUPPORTED: u16 = 0x0002;
pub const LE_CONNECTION_REFUSED_NO_RESOURCES: u16 = 0x0004;
pub const LE_CONNECTION_REFUSED_SOURCE_CID_ALREADY_ALLOCATED: u16 = 0x000A;
pub const LE_CONNECTION_REFUSED_UNACCEPTABLE_PARAMETERS: u16 = 0x000B;

pub const CREDIT_BASED_CONNECTION_ALL_SUCCESSFUL: u16 = 0x0000;
pub const CREDIT_BASED_CONNECTION_REFUSED_SPSM_NOT_SUPPORTED: u16 = 0x0002;
pub const CREDIT_BASED_CONNECTION_REFUSED_NO_RESOURCES: u16 = 0x0004;
pub const CREDIT_BASED_CONNECTION_REFUSED_INVALID_SOURCE_CID: u16 = 0x0009;
pub const CREDIT_BASED_CONNECTION_REFUSED_SOURCE_CID_ALREADY_ALLOCATED: u16 = 0x000A;
pub const CREDIT_BASED_CONNECTION_REFUSED_UNACCEPTABLE_PARAMETERS: u16 = 0x000B;
pub const CREDIT_BASED_CONNECTION_REFUSED_INVALID_PARAMETERS: u16 = 0x000C;

pub const CREDIT_BASED_RECONFIGURATION_SUCCESSFUL: u16 = 0x0000;
pub const CREDIT_BASED_RECONFIGURATION_FAILED_MTU_REDUCTION: u16 = 0x0001;
pub const CREDIT_BASED_RECONFIGURATION_FAILED_MPS_REDUCTION: u16 = 0x0002;
pub const CREDIT_BASED_RECONFIGURATION_FAILED_INVALID_CIDS: u16 = 0x0003;
pub const CREDIT_BASED_RECONFIGURATION_FAILED_UNACCEPTABLE_PARAMETERS: u16 = 0x0004;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LeCreditBasedChannelSpec {
    pub psm: Option<u16>,
    pub mtu: u16,
    pub mps: u16,
    pub max_credits: u16,
}

impl Default for LeCreditBasedChannelSpec {
    fn default() -> Self {
        Self {
            psm: None,
            mtu: L2CAP_LE_CREDIT_BASED_CONNECTION_DEFAULT_MTU,
            mps: L2CAP_LE_CREDIT_BASED_CONNECTION_DEFAULT_MPS,
            max_credits: L2CAP_LE_CREDIT_BASED_CONNECTION_DEFAULT_INITIAL_CREDITS,
        }
    }
}

impl LeCreditBasedChannelSpec {
    pub fn validate(self) -> Result<Self> {
        if self.max_credits == 0 {
            return Err(Error::InvalidPacket("max credits out of range".into()));
        }
        if self.mtu < L2CAP_LE_CREDIT_BASED_CONNECTION_MIN_MTU {
            return Err(Error::InvalidPacket("MTU out of range".into()));
        }
        if !(L2CAP_LE_CREDIT_BASED_CONNECTION_MIN_MPS..=L2CAP_LE_CREDIT_BASED_CONNECTION_MAX_MPS)
            .contains(&self.mps)
        {
            return Err(Error::InvalidPacket("MPS out of range".into()));
        }
        Ok(self)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LeCreditBasedChannelState {
    Connected,
    Disconnecting,
    Disconnected,
}

/// One connected LE credit-based channel. PDUs and credit grants are exposed
/// through polling so a manager can put them on the data/signaling CIDs.
#[derive(Clone, Debug)]
pub struct LeCreditBasedChannel {
    pub psm: u16,
    pub source_cid: u16,
    pub destination_cid: u16,
    pub mtu: u16,
    pub mps: u16,
    /// Credits granted by the peer for outbound K-frames.
    pub credits: u16,
    pub peer_mtu: u16,
    pub peer_mps: u16,
    /// Credits this endpoint has granted to the peer.
    pub peer_credits: u16,
    pub peer_max_credits: u16,
    pub att_mtu: u16,
    pub state: LeCreditBasedChannelState,
    peer_credit_threshold: u16,
    output_stream: VecDeque<u8>,
    output_sdu: VecDeque<u8>,
    outbound_pdus: VecDeque<Vec<u8>>,
    input_sdu: Vec<u8>,
    input_sdu_length: Option<usize>,
    received_sdus: VecDeque<Vec<u8>>,
    pending_credit_grants: VecDeque<u16>,
    reading_paused: bool,
}

impl LeCreditBasedChannel {
    #[allow(clippy::too_many_arguments)]
    pub fn connected(
        psm: u16,
        source_cid: u16,
        destination_cid: u16,
        local: LeCreditBasedChannelSpec,
        peer_mtu: u16,
        peer_mps: u16,
        credits: u16,
    ) -> Result<Self> {
        let local = local.validate()?;
        if peer_mtu < L2CAP_LE_CREDIT_BASED_CONNECTION_MIN_MTU {
            return Err(Error::InvalidPacket("peer MTU out of range".into()));
        }
        if !(L2CAP_LE_CREDIT_BASED_CONNECTION_MIN_MPS..=L2CAP_LE_CREDIT_BASED_CONNECTION_MAX_MPS)
            .contains(&peer_mps)
        {
            return Err(Error::InvalidPacket("peer MPS out of range".into()));
        }
        Ok(Self {
            psm,
            source_cid,
            destination_cid,
            mtu: local.mtu,
            mps: local.mps,
            credits,
            peer_mtu,
            peer_mps,
            peer_credits: local.max_credits,
            peer_max_credits: local.max_credits,
            att_mtu: local.mtu.min(peer_mtu),
            state: LeCreditBasedChannelState::Connected,
            peer_credit_threshold: local.max_credits / 2,
            output_stream: VecDeque::new(),
            output_sdu: VecDeque::new(),
            outbound_pdus: VecDeque::new(),
            input_sdu: Vec::new(),
            input_sdu_length: None,
            received_sdus: VecDeque::new(),
            pending_credit_grants: VecDeque::new(),
            reading_paused: false,
        })
    }

    pub fn write(&mut self, data: &[u8]) -> Result<()> {
        self.ensure_connected()?;
        if data.is_empty() {
            return Err(Error::InvalidPacket("cannot queue an empty buffer".into()));
        }
        self.output_stream.extend(data.iter().copied());
        self.process_output();
        Ok(())
    }

    pub fn add_credits(&mut self, credits: u16) -> Result<()> {
        self.ensure_connected()?;
        self.credits = self
            .credits
            .checked_add(credits)
            .ok_or_else(|| Error::InvalidPacket("outbound credit overflow".into()))?;
        self.process_output();
        Ok(())
    }

    pub fn receive_pdu(&mut self, pdu: &[u8]) -> Result<()> {
        self.ensure_connected()?;
        if pdu.len() > usize::from(self.mps) {
            return Err(Error::InvalidPacket(format!(
                "incoming PDU exceeds MPS {}",
                self.mps
            )));
        }
        if self.peer_credits == 0 {
            return Err(Error::InvalidPacket(
                "received PDU after peer exhausted credits".into(),
            ));
        }
        self.peer_credits -= 1;
        self.replenish_peer_credits();

        self.input_sdu.extend_from_slice(pdu);
        if self.input_sdu_length.is_none() && self.input_sdu.len() >= 2 {
            let length = usize::from(u16::from_le_bytes([self.input_sdu[0], self.input_sdu[1]]));
            if length > usize::from(self.mtu) {
                self.reset_input();
                return Err(Error::InvalidPacket(format!(
                    "incoming SDU exceeds MTU {}",
                    self.mtu
                )));
            }
            self.input_sdu_length = Some(length);
        }
        let Some(length) = self.input_sdu_length else {
            return Ok(());
        };
        let expected = length + 2;
        if self.input_sdu.len() < expected {
            return Ok(());
        }
        if self.input_sdu.len() > expected {
            self.reset_input();
            return Err(Error::InvalidPacket("incoming SDU overflow".into()));
        }
        self.received_sdus
            .push_back(self.input_sdu[2..expected].to_vec());
        self.reset_input();
        Ok(())
    }

    pub fn poll_outbound_pdu(&mut self) -> Option<Vec<u8>> {
        self.outbound_pdus.pop_front()
    }

    pub fn poll_credit_grant(&mut self) -> Option<u16> {
        self.pending_credit_grants.pop_front()
    }

    pub fn pop_received(&mut self) -> Option<Vec<u8>> {
        self.received_sdus.pop_front()
    }

    pub fn is_drained(&self) -> bool {
        self.output_stream.is_empty() && self.output_sdu.is_empty() && self.outbound_pdus.is_empty()
    }

    /// Stop granting new receive credits while retaining the credits already
    /// advertised to the peer. This bounds buffering at the current credit
    /// window while an application-level sink applies backpressure.
    pub fn pause_reading(&mut self) -> Result<()> {
        self.ensure_connected()?;
        self.reading_paused = true;
        Ok(())
    }

    /// Resume receive-credit replenishment and immediately restore the local
    /// window when it has crossed the normal grant threshold.
    pub fn resume_reading(&mut self) -> Result<()> {
        self.ensure_connected()?;
        self.reading_paused = false;
        self.replenish_peer_credits();
        Ok(())
    }

    pub fn is_reading_paused(&self) -> bool {
        self.reading_paused
    }

    pub fn disconnect(&mut self) -> Result<()> {
        self.ensure_connected()?;
        self.begin_disconnect();
        self.complete_disconnect();
        Ok(())
    }

    /// Close the local endpoint immediately while connected or disconnecting.
    ///
    /// This mirrors upstream's abort path: no additional signaling is emitted,
    /// queued data is discarded, and a later peer response may be ignored by
    /// the owning manager.
    pub fn abort(&mut self) {
        if matches!(
            self.state,
            LeCreditBasedChannelState::Connected | LeCreditBasedChannelState::Disconnecting
        ) {
            self.complete_disconnect();
        }
    }

    fn begin_disconnect(&mut self) {
        self.state = LeCreditBasedChannelState::Disconnecting;
        self.flush();
    }

    fn complete_disconnect(&mut self) {
        self.state = LeCreditBasedChannelState::Disconnected;
        self.flush();
    }

    fn flush(&mut self) {
        self.output_stream.clear();
        self.output_sdu.clear();
        self.outbound_pdus.clear();
        self.reset_input();
    }

    fn ensure_connected(&self) -> Result<()> {
        if self.state != LeCreditBasedChannelState::Connected {
            return Err(Error::InvalidPacket("channel is not connected".into()));
        }
        Ok(())
    }

    fn process_output(&mut self) {
        while self.credits != 0 {
            if self.output_sdu.is_empty() {
                if self.output_stream.is_empty() {
                    return;
                }
                let length = self.output_stream.len().min(usize::from(self.peer_mtu));
                self.output_sdu.extend(
                    (length as u16)
                        .to_le_bytes()
                        .into_iter()
                        .chain(self.output_stream.drain(..length)),
                );
            }
            let fragment_length = self.output_sdu.len().min(usize::from(self.peer_mps));
            self.outbound_pdus
                .push_back(self.output_sdu.drain(..fragment_length).collect());
            self.credits -= 1;
        }
    }

    fn replenish_peer_credits(&mut self) {
        if self.reading_paused || self.peer_credits > self.peer_credit_threshold {
            return;
        }
        let grant = self.peer_max_credits - self.peer_credits;
        if grant != 0 {
            self.pending_credit_grants.push_back(grant);
            self.peer_credits = self.peer_max_credits;
        }
    }

    fn reset_input(&mut self) {
        self.input_sdu.clear();
        self.input_sdu_length = None;
    }

    fn reconfigure_local(&mut self, mtu: u16, mps: u16) {
        self.mtu = mtu;
        self.mps = mps;
        self.att_mtu = self.mtu.min(self.peer_mtu);
    }

    fn reconfigure_peer(&mut self, mtu: u16, mps: u16) {
        self.peer_mtu = mtu;
        self.peer_mps = mps;
        self.att_mtu = self.mtu.min(self.peer_mtu);
        self.process_output();
    }
}

#[derive(Clone, Debug)]
struct PendingConnection {
    psm: u16,
    source_cid: u16,
    spec: LeCreditBasedChannelSpec,
}

#[derive(Clone, Debug)]
struct PendingEnhancedConnection {
    psm: u16,
    source_cids: Vec<u16>,
    spec: LeCreditBasedChannelSpec,
}

#[derive(Clone, Debug)]
struct PendingReconfiguration {
    source_cids: Vec<u16>,
    mtu: u16,
    mps: u16,
}

/// A sans-I/O manager for one LE logical link. Relay polled PDUs to another
/// manager to run complete connection, transfer, credit, and disconnect flows.
#[derive(Debug, Default)]
pub struct LeCreditChannelManager {
    servers: BTreeMap<u16, LeCreditBasedChannelSpec>,
    channels: BTreeMap<u16, LeCreditBasedChannel>,
    pending: BTreeMap<u8, PendingConnection>,
    pending_enhanced: BTreeMap<u8, PendingEnhancedConnection>,
    pending_reconfigurations: BTreeMap<u8, PendingReconfiguration>,
    connection_results: BTreeMap<u16, u16>,
    reconfiguration_results: BTreeMap<u8, u16>,
    accepted_channels: VecDeque<u16>,
    outbound: VecDeque<L2capPdu>,
    identifier: u8,
}

impl LeCreditChannelManager {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register_server(&mut self, mut spec: LeCreditBasedChannelSpec) -> Result<u16> {
        spec.validate()?;
        let psm = match spec.psm {
            Some(0) => return Err(Error::InvalidPacket("LE PSM cannot be zero".into())),
            Some(psm) => psm,
            None => (L2CAP_LE_PSM_DYNAMIC_RANGE_START..=L2CAP_LE_PSM_DYNAMIC_RANGE_END)
                .find(|candidate| !self.servers.contains_key(candidate))
                .ok_or_else(|| Error::InvalidPacket("no free LE PSM".into()))?,
        };
        if self.servers.contains_key(&psm) {
            return Err(Error::InvalidPacket(format!(
                "LE PSM {psm:#06x} is already in use"
            )));
        }
        spec.psm = Some(psm);
        self.servers.insert(psm, spec);
        Ok(psm)
    }

    pub fn unregister_server(&mut self, psm: u16) -> bool {
        self.servers.remove(&psm).is_some()
    }

    pub fn connect(&mut self, psm: u16, mut spec: LeCreditBasedChannelSpec) -> Result<u16> {
        if psm == 0 {
            return Err(Error::InvalidPacket("LE PSM cannot be zero".into()));
        }
        spec.validate()?;
        spec.psm = Some(psm);
        let source_cid = self.allocate_cid()?;
        self.connection_results.remove(&source_cid);
        let identifier = self.next_identifier();
        self.pending.insert(
            identifier,
            PendingConnection {
                psm,
                source_cid,
                spec,
            },
        );
        self.queue_control(ControlFrame::LeCreditBasedConnectionRequest {
            identifier,
            le_psm: psm,
            source_cid,
            mtu: spec.mtu,
            mps: spec.mps,
            initial_credits: spec.max_credits,
        });
        Ok(source_cid)
    }

    /// Starts one enhanced credit-based signaling transaction that creates
    /// between one and five channels with identical negotiated parameters.
    pub fn connect_enhanced(
        &mut self,
        psm: u16,
        mut spec: LeCreditBasedChannelSpec,
        count: usize,
    ) -> Result<Vec<u16>> {
        if psm == 0 {
            return Err(Error::InvalidPacket("SPSM cannot be zero".into()));
        }
        if !(1..=L2CAP_CREDIT_BASED_CONNECTION_MAX_CHANNELS).contains(&count) {
            return Err(Error::InvalidPacket(
                "enhanced channel count must be between 1 and 5".into(),
            ));
        }
        spec.validate()?;
        spec.psm = Some(psm);
        let source_cids = self.allocate_cids(count)?;
        for source_cid in &source_cids {
            self.connection_results.remove(source_cid);
        }
        let identifier = self.next_identifier();
        self.pending_enhanced.insert(
            identifier,
            PendingEnhancedConnection {
                psm,
                source_cids: source_cids.clone(),
                spec,
            },
        );
        self.queue_control(ControlFrame::CreditBasedConnectionRequest {
            identifier,
            spsm: psm,
            mtu: spec.mtu,
            mps: spec.mps,
            initial_credits: spec.max_credits,
            source_cid: source_cids.clone(),
        });
        Ok(source_cids)
    }

    /// Requests a receive MTU/MPS update for one or more enhanced channels and
    /// returns the signaling identifier used to query the eventual result.
    pub fn reconfigure(&mut self, source_cids: &[u16], mtu: u16, mps: u16) -> Result<u8> {
        if source_cids.is_empty() || source_cids.len() > L2CAP_CREDIT_BASED_CONNECTION_MAX_CHANNELS
        {
            return Err(Error::InvalidPacket(
                "reconfiguration requires between 1 and 5 channels".into(),
            ));
        }
        validate_mtu_mps(mtu, mps)?;
        let mut unique = source_cids.to_vec();
        unique.sort_unstable();
        unique.dedup();
        if unique.len() != source_cids.len() {
            return Err(Error::InvalidPacket(
                "reconfiguration contains duplicate CIDs".into(),
            ));
        }

        let mut destination_cids = Vec::with_capacity(source_cids.len());
        for source_cid in source_cids {
            let channel = self
                .channels
                .get(source_cid)
                .ok_or_else(|| Error::InvalidPacket(format!("unknown LE CID {source_cid:#06x}")))?;
            channel.ensure_connected()?;
            if mtu < channel.mtu {
                return Err(Error::InvalidPacket(
                    "reconfiguration cannot reduce MTU".into(),
                ));
            }
            if source_cids.len() > 1 && mps < channel.mps {
                return Err(Error::InvalidPacket(
                    "multi-channel reconfiguration cannot reduce MPS".into(),
                ));
            }
            destination_cids.push(channel.destination_cid);
        }

        let identifier = self.next_identifier();
        self.pending_reconfigurations.insert(
            identifier,
            PendingReconfiguration {
                source_cids: source_cids.to_vec(),
                mtu,
                mps,
            },
        );
        self.queue_control(ControlFrame::CreditBasedReconfigureRequest {
            identifier,
            mtu,
            mps,
            destination_cid: destination_cids,
        });
        Ok(identifier)
    }

    pub fn channel(&self, source_cid: u16) -> Option<&LeCreditBasedChannel> {
        self.channels.get(&source_cid)
    }

    pub fn channel_mut(&mut self, source_cid: u16) -> Option<&mut LeCreditBasedChannel> {
        self.channels.get_mut(&source_cid)
    }

    /// Connected channels in local source-CID order.
    pub fn channels(&self) -> impl Iterator<Item = &LeCreditBasedChannel> {
        self.channels.values()
    }

    pub fn connection_result(&self, source_cid: u16) -> Option<u16> {
        self.connection_results.get(&source_cid).copied()
    }

    pub fn reconfiguration_result(&self, identifier: u8) -> Option<u16> {
        self.reconfiguration_results.get(&identifier).copied()
    }

    pub fn poll_accepted_channel(&mut self) -> Option<u16> {
        self.accepted_channels.pop_front()
    }

    pub fn poll_outbound(&mut self) -> Option<L2capPdu> {
        self.outbound.pop_front()
    }

    pub fn drain_outbound(&mut self) -> Vec<L2capPdu> {
        self.outbound.drain(..).collect()
    }

    pub fn process_pdu(&mut self, pdu: L2capPdu) -> Result<()> {
        if pdu.cid == L2CAP_LE_SIGNALING_CID {
            return self.process_control(ControlFrame::from_bytes(&pdu.payload)?);
        }
        let source_cid = pdu.cid;
        self.channels
            .get_mut(&source_cid)
            .ok_or_else(|| Error::InvalidPacket(format!("unknown LE CID {source_cid:#06x}")))?
            .receive_pdu(&pdu.payload)?;
        self.flush_channel(source_cid)
    }

    pub fn send(&mut self, source_cid: u16, data: &[u8]) -> Result<()> {
        self.channels
            .get_mut(&source_cid)
            .ok_or_else(|| Error::InvalidPacket(format!("unknown LE CID {source_cid:#06x}")))?
            .write(data)?;
        self.flush_channel(source_cid)
    }

    pub fn set_reading_paused(&mut self, source_cid: u16, paused: bool) -> Result<()> {
        let channel = self
            .channels
            .get_mut(&source_cid)
            .ok_or_else(|| Error::InvalidPacket(format!("unknown LE CID {source_cid:#06x}")))?;
        if paused {
            channel.pause_reading()?;
        } else {
            channel.resume_reading()?;
        }
        self.flush_channel(source_cid)
    }

    pub fn disconnect(&mut self, source_cid: u16) -> Result<()> {
        let destination_cid = {
            let channel = self
                .channels
                .get_mut(&source_cid)
                .ok_or_else(|| Error::InvalidPacket(format!("unknown LE CID {source_cid:#06x}")))?;
            channel.ensure_connected()?;
            channel.begin_disconnect();
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

    /// Abort a connected or disconnecting channel locally.
    ///
    /// Any already-queued disconnection request remains on the wire. Its peer
    /// response is harmless because responses for locally closed channels are
    /// ignored, matching upstream's channel-manager behavior.
    pub fn abort(&mut self, source_cid: u16) -> bool {
        let Some(mut channel) = self.channels.remove(&source_cid) else {
            return false;
        };
        let abortable = matches!(
            channel.state,
            LeCreditBasedChannelState::Connected | LeCreditBasedChannelState::Disconnecting
        );
        if abortable {
            channel.abort();
        } else {
            self.channels.insert(source_cid, channel);
        }
        abortable
    }

    fn process_control(&mut self, frame: ControlFrame) -> Result<()> {
        match frame {
            ControlFrame::LeCreditBasedConnectionRequest {
                identifier,
                le_psm,
                source_cid,
                mtu,
                mps,
                initial_credits,
            } => self.on_connection_request(
                identifier,
                le_psm,
                source_cid,
                mtu,
                mps,
                initial_credits,
            ),
            ControlFrame::LeCreditBasedConnectionResponse {
                identifier,
                destination_cid,
                mtu,
                mps,
                initial_credits,
                result,
            } => self.on_connection_response(
                identifier,
                destination_cid,
                mtu,
                mps,
                initial_credits,
                result,
            ),
            ControlFrame::CreditBasedConnectionRequest {
                identifier,
                spsm,
                mtu,
                mps,
                initial_credits,
                source_cid,
            } => self.on_enhanced_connection_request(
                identifier,
                spsm,
                mtu,
                mps,
                initial_credits,
                source_cid,
            ),
            ControlFrame::CreditBasedConnectionResponse {
                identifier,
                mtu,
                mps,
                initial_credits,
                result,
                destination_cid,
            } => self.on_enhanced_connection_response(
                identifier,
                mtu,
                mps,
                initial_credits,
                result,
                destination_cid,
            ),
            ControlFrame::CreditBasedReconfigureRequest {
                identifier,
                mtu,
                mps,
                destination_cid,
            } => self.on_reconfigure_request(identifier, mtu, mps, destination_cid),
            ControlFrame::CreditBasedReconfigureResponse { identifier, result } => {
                self.on_reconfigure_response(identifier, result)
            }
            ControlFrame::LeFlowControlCredit { cid, credits, .. } => {
                let source_cid = self
                    .channels
                    .iter()
                    .find_map(|(source, channel)| {
                        (channel.destination_cid == cid).then_some(*source)
                    })
                    .ok_or_else(|| {
                        Error::InvalidPacket(format!("unknown credit CID {cid:#06x}"))
                    })?;
                self.channels
                    .get_mut(&source_cid)
                    .expect("channel was just found")
                    .add_credits(credits)?;
                self.flush_channel(source_cid)
            }
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

    #[allow(clippy::too_many_arguments)]
    fn on_connection_request(
        &mut self,
        identifier: u8,
        psm: u16,
        remote_cid: u16,
        peer_mtu: u16,
        peer_mps: u16,
        credits: u16,
    ) -> Result<()> {
        let Some(spec) = self.servers.get(&psm).copied() else {
            self.queue_connection_response(
                identifier,
                0,
                LeCreditBasedChannelSpec::default(),
                0,
                LE_CONNECTION_REFUSED_PSM_NOT_SUPPORTED,
            );
            return Ok(());
        };
        if self
            .channels
            .values()
            .any(|channel| channel.destination_cid == remote_cid)
        {
            self.queue_connection_response(
                identifier,
                0,
                spec,
                0,
                LE_CONNECTION_REFUSED_SOURCE_CID_ALREADY_ALLOCATED,
            );
            return Ok(());
        }
        if peer_mtu < L2CAP_LE_CREDIT_BASED_CONNECTION_MIN_MTU
            || !(L2CAP_LE_CREDIT_BASED_CONNECTION_MIN_MPS
                ..=L2CAP_LE_CREDIT_BASED_CONNECTION_MAX_MPS)
                .contains(&peer_mps)
        {
            self.queue_connection_response(
                identifier,
                0,
                spec,
                0,
                LE_CONNECTION_REFUSED_UNACCEPTABLE_PARAMETERS,
            );
            return Ok(());
        }
        let local_cid = match self.allocate_cid() {
            Ok(cid) => cid,
            Err(_) => {
                self.queue_connection_response(
                    identifier,
                    0,
                    spec,
                    0,
                    LE_CONNECTION_REFUSED_NO_RESOURCES,
                );
                return Ok(());
            }
        };
        let channel = LeCreditBasedChannel::connected(
            psm, local_cid, remote_cid, spec, peer_mtu, peer_mps, credits,
        )?;
        self.channels.insert(local_cid, channel);
        self.accepted_channels.push_back(local_cid);
        self.queue_connection_response(
            identifier,
            local_cid,
            spec,
            spec.max_credits,
            LE_CONNECTION_SUCCESSFUL,
        );
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn on_connection_response(
        &mut self,
        identifier: u8,
        destination_cid: u16,
        peer_mtu: u16,
        peer_mps: u16,
        credits: u16,
        result: u16,
    ) -> Result<()> {
        let pending = self.pending.remove(&identifier).ok_or_else(|| {
            Error::InvalidPacket(format!("unknown LE connection response ID {identifier}"))
        })?;
        self.connection_results.insert(pending.source_cid, result);
        if result != LE_CONNECTION_SUCCESSFUL {
            return Ok(());
        }
        if destination_cid == 0 {
            return Err(Error::InvalidPacket(
                "successful response has zero destination CID".into(),
            ));
        }
        let channel = LeCreditBasedChannel::connected(
            pending.psm,
            pending.source_cid,
            destination_cid,
            pending.spec,
            peer_mtu,
            peer_mps,
            credits,
        )?;
        self.channels.insert(pending.source_cid, channel);
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn on_enhanced_connection_request(
        &mut self,
        identifier: u8,
        psm: u16,
        peer_mtu: u16,
        peer_mps: u16,
        credits: u16,
        remote_cids: Vec<u16>,
    ) -> Result<()> {
        let Some(spec) = self.servers.get(&psm).copied() else {
            self.queue_enhanced_connection_response(
                identifier,
                LeCreditBasedChannelSpec::default(),
                0,
                CREDIT_BASED_CONNECTION_REFUSED_SPSM_NOT_SUPPORTED,
                Vec::new(),
            );
            return Ok(());
        };
        if remote_cids.is_empty() || remote_cids.len() > L2CAP_CREDIT_BASED_CONNECTION_MAX_CHANNELS
        {
            self.queue_enhanced_connection_response(
                identifier,
                spec,
                0,
                CREDIT_BASED_CONNECTION_REFUSED_INVALID_PARAMETERS,
                Vec::new(),
            );
            return Ok(());
        }
        let mut unique = remote_cids.clone();
        unique.sort_unstable();
        unique.dedup();
        if unique.len() != remote_cids.len()
            || remote_cids.iter().any(|cid| {
                !(L2CAP_LE_U_DYNAMIC_CID_RANGE_START..=L2CAP_LE_U_DYNAMIC_CID_RANGE_END)
                    .contains(cid)
            })
        {
            self.queue_enhanced_connection_response(
                identifier,
                spec,
                0,
                CREDIT_BASED_CONNECTION_REFUSED_INVALID_SOURCE_CID,
                Vec::new(),
            );
            return Ok(());
        }
        if remote_cids.iter().any(|remote_cid| {
            self.channels
                .values()
                .any(|channel| channel.destination_cid == *remote_cid)
        }) {
            self.queue_enhanced_connection_response(
                identifier,
                spec,
                0,
                CREDIT_BASED_CONNECTION_REFUSED_SOURCE_CID_ALREADY_ALLOCATED,
                Vec::new(),
            );
            return Ok(());
        }
        if validate_mtu_mps(peer_mtu, peer_mps).is_err() {
            self.queue_enhanced_connection_response(
                identifier,
                spec,
                0,
                CREDIT_BASED_CONNECTION_REFUSED_UNACCEPTABLE_PARAMETERS,
                Vec::new(),
            );
            return Ok(());
        }
        let local_cids = match self.allocate_cids(remote_cids.len()) {
            Ok(cids) => cids,
            Err(_) => {
                self.queue_enhanced_connection_response(
                    identifier,
                    spec,
                    0,
                    CREDIT_BASED_CONNECTION_REFUSED_NO_RESOURCES,
                    Vec::new(),
                );
                return Ok(());
            }
        };
        let channels: Result<Vec<_>> = local_cids
            .iter()
            .zip(&remote_cids)
            .map(|(local_cid, remote_cid)| {
                LeCreditBasedChannel::connected(
                    psm,
                    *local_cid,
                    *remote_cid,
                    spec,
                    peer_mtu,
                    peer_mps,
                    credits,
                )
            })
            .collect();
        for (local_cid, channel) in local_cids.iter().copied().zip(channels?) {
            self.channels.insert(local_cid, channel);
            self.accepted_channels.push_back(local_cid);
        }
        self.queue_enhanced_connection_response(
            identifier,
            spec,
            spec.max_credits,
            CREDIT_BASED_CONNECTION_ALL_SUCCESSFUL,
            local_cids,
        );
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn on_enhanced_connection_response(
        &mut self,
        identifier: u8,
        peer_mtu: u16,
        peer_mps: u16,
        credits: u16,
        result: u16,
        destination_cids: Vec<u16>,
    ) -> Result<()> {
        let pending = self.pending_enhanced.remove(&identifier).ok_or_else(|| {
            Error::InvalidPacket(format!(
                "unknown enhanced connection response ID {identifier}"
            ))
        })?;
        for source_cid in &pending.source_cids {
            self.connection_results.insert(*source_cid, result);
        }
        if result != CREDIT_BASED_CONNECTION_ALL_SUCCESSFUL {
            return Ok(());
        }
        if destination_cids.len() != pending.source_cids.len() {
            return Err(Error::InvalidPacket(
                "enhanced response CID count mismatch".into(),
            ));
        }
        validate_mtu_mps(peer_mtu, peer_mps)?;
        let mut unique = destination_cids.clone();
        unique.sort_unstable();
        unique.dedup();
        if unique.len() != destination_cids.len()
            || destination_cids.iter().any(|cid| {
                !(L2CAP_LE_U_DYNAMIC_CID_RANGE_START..=L2CAP_LE_U_DYNAMIC_CID_RANGE_END)
                    .contains(cid)
                    || self
                        .channels
                        .values()
                        .any(|channel| channel.destination_cid == *cid)
            })
        {
            return Err(Error::InvalidPacket(
                "enhanced response contains invalid destination CIDs".into(),
            ));
        }
        let channels: Result<Vec<_>> = pending
            .source_cids
            .iter()
            .zip(&destination_cids)
            .map(|(source_cid, destination_cid)| {
                LeCreditBasedChannel::connected(
                    pending.psm,
                    *source_cid,
                    *destination_cid,
                    pending.spec,
                    peer_mtu,
                    peer_mps,
                    credits,
                )
            })
            .collect();
        for (source_cid, channel) in pending.source_cids.into_iter().zip(channels?) {
            self.channels.insert(source_cid, channel);
        }
        Ok(())
    }

    fn on_reconfigure_request(
        &mut self,
        identifier: u8,
        mtu: u16,
        mps: u16,
        destination_cids: Vec<u16>,
    ) -> Result<()> {
        let result = if destination_cids.is_empty()
            || destination_cids.len() > L2CAP_CREDIT_BASED_CONNECTION_MAX_CHANNELS
        {
            CREDIT_BASED_RECONFIGURATION_FAILED_INVALID_CIDS
        } else {
            let mut unique = destination_cids.clone();
            unique.sort_unstable();
            unique.dedup();
            if unique.len() != destination_cids.len()
                || destination_cids.iter().any(|cid| {
                    self.channels
                        .get(cid)
                        .is_none_or(|channel| channel.state != LeCreditBasedChannelState::Connected)
                })
            {
                CREDIT_BASED_RECONFIGURATION_FAILED_INVALID_CIDS
            } else if validate_mtu_mps(mtu, mps).is_err() {
                CREDIT_BASED_RECONFIGURATION_FAILED_UNACCEPTABLE_PARAMETERS
            } else if destination_cids
                .iter()
                .any(|cid| mtu < self.channels[cid].peer_mtu)
            {
                CREDIT_BASED_RECONFIGURATION_FAILED_MTU_REDUCTION
            } else if destination_cids.len() > 1
                && destination_cids
                    .iter()
                    .any(|cid| mps < self.channels[cid].peer_mps)
            {
                CREDIT_BASED_RECONFIGURATION_FAILED_MPS_REDUCTION
            } else {
                for cid in &destination_cids {
                    self.channels
                        .get_mut(cid)
                        .expect("validated channel")
                        .reconfigure_peer(mtu, mps);
                }
                CREDIT_BASED_RECONFIGURATION_SUCCESSFUL
            }
        };
        self.queue_control(ControlFrame::CreditBasedReconfigureResponse { identifier, result });
        Ok(())
    }

    fn on_reconfigure_response(&mut self, identifier: u8, result: u16) -> Result<()> {
        let pending = self
            .pending_reconfigurations
            .remove(&identifier)
            .ok_or_else(|| {
                Error::InvalidPacket(format!(
                    "unknown credit-based reconfiguration response ID {identifier}"
                ))
            })?;
        self.reconfiguration_results.insert(identifier, result);
        if result == CREDIT_BASED_RECONFIGURATION_SUCCESSFUL {
            for source_cid in pending.source_cids {
                self.channels
                    .get_mut(&source_cid)
                    .ok_or_else(|| {
                        Error::InvalidPacket(format!("unknown LE CID {source_cid:#06x}"))
                    })?
                    .reconfigure_local(pending.mtu, pending.mps);
            }
        }
        Ok(())
    }

    fn on_disconnection_request(
        &mut self,
        identifier: u8,
        destination_cid: u16,
        source_cid: u16,
    ) -> Result<()> {
        let mut channel = self.channels.remove(&destination_cid).ok_or_else(|| {
            Error::InvalidPacket(format!("unknown disconnect CID {destination_cid:#06x}"))
        })?;
        if channel.destination_cid != source_cid {
            self.channels.insert(destination_cid, channel);
            return Err(Error::InvalidPacket("disconnect CID pair mismatch".into()));
        }
        channel.complete_disconnect();
        self.queue_control(ControlFrame::DisconnectionResponse {
            identifier,
            destination_cid,
            source_cid,
        });
        Ok(())
    }

    fn on_disconnection_response(&mut self, destination_cid: u16, source_cid: u16) -> Result<()> {
        let Some(mut channel) = self.channels.remove(&source_cid) else {
            // A simultaneous peer request or a local abort may already have
            // closed and removed this channel. Upstream deliberately ignores
            // the late response in both cases.
            return Ok(());
        };
        if channel.destination_cid != destination_cid
            || channel.state != LeCreditBasedChannelState::Disconnecting
        {
            self.channels.insert(source_cid, channel);
            return Err(Error::InvalidPacket(
                "unexpected disconnect response".into(),
            ));
        }
        channel.complete_disconnect();
        Ok(())
    }

    fn queue_connection_response(
        &mut self,
        identifier: u8,
        destination_cid: u16,
        spec: LeCreditBasedChannelSpec,
        initial_credits: u16,
        result: u16,
    ) {
        self.queue_control(ControlFrame::LeCreditBasedConnectionResponse {
            identifier,
            destination_cid,
            mtu: spec.mtu,
            mps: spec.mps,
            initial_credits,
            result,
        });
    }

    fn queue_enhanced_connection_response(
        &mut self,
        identifier: u8,
        spec: LeCreditBasedChannelSpec,
        initial_credits: u16,
        result: u16,
        destination_cid: Vec<u16>,
    ) {
        self.queue_control(ControlFrame::CreditBasedConnectionResponse {
            identifier,
            mtu: spec.mtu,
            mps: spec.mps,
            initial_credits,
            result,
            destination_cid,
        });
    }

    fn flush_channel(&mut self, source_cid: u16) -> Result<()> {
        let channel = self
            .channels
            .get_mut(&source_cid)
            .ok_or_else(|| Error::InvalidPacket(format!("unknown LE CID {source_cid:#06x}")))?;
        let destination_cid = channel.destination_cid;
        let mut pdus = Vec::new();
        while let Some(payload) = channel.poll_outbound_pdu() {
            pdus.push(L2capPdu::new(destination_cid, payload));
        }
        let mut grants = Vec::new();
        while let Some(credits) = channel.poll_credit_grant() {
            grants.push((channel.source_cid, credits));
        }
        self.outbound.extend(pdus);
        for (cid, credits) in grants {
            let identifier = self.next_identifier();
            self.queue_control(ControlFrame::LeFlowControlCredit {
                identifier,
                cid,
                credits,
            });
        }
        Ok(())
    }

    fn allocate_cid(&self) -> Result<u16> {
        (L2CAP_LE_U_DYNAMIC_CID_RANGE_START..=L2CAP_LE_U_DYNAMIC_CID_RANGE_END)
            .find(|cid| !self.cid_is_reserved(*cid))
            .ok_or_else(|| Error::InvalidPacket("no free LE CID".into()))
    }

    fn allocate_cids(&self, count: usize) -> Result<Vec<u16>> {
        let cids: Vec<_> = (L2CAP_LE_U_DYNAMIC_CID_RANGE_START..=L2CAP_LE_U_DYNAMIC_CID_RANGE_END)
            .filter(|cid| !self.cid_is_reserved(*cid))
            .take(count)
            .collect();
        if cids.len() != count {
            return Err(Error::InvalidPacket("no free LE CIDs".into()));
        }
        Ok(cids)
    }

    fn cid_is_reserved(&self, cid: u16) -> bool {
        self.channels.contains_key(&cid)
            || self
                .pending
                .values()
                .any(|pending| pending.source_cid == cid)
            || self
                .pending_enhanced
                .values()
                .any(|pending| pending.source_cids.contains(&cid))
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
            .push_back(L2capPdu::new(L2CAP_LE_SIGNALING_CID, frame.to_bytes()));
    }
}

fn validate_mtu_mps(mtu: u16, mps: u16) -> Result<()> {
    if mtu < L2CAP_LE_CREDIT_BASED_CONNECTION_MIN_MTU {
        return Err(Error::InvalidPacket("MTU out of range".into()));
    }
    if !(L2CAP_LE_CREDIT_BASED_CONNECTION_MIN_MPS..=L2CAP_LE_CREDIT_BASED_CONNECTION_MAX_MPS)
        .contains(&mps)
    {
        return Err(Error::InvalidPacket("MPS out of range".into()));
    }
    Ok(())
}
