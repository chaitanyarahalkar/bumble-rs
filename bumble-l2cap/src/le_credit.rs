//! Synchronous LE credit-based channel segmentation and credit accounting.

use std::collections::VecDeque;

use crate::{Error, Result};

pub const L2CAP_LE_CREDIT_BASED_CONNECTION_MAX_CREDITS: u16 = u16::MAX;
pub const L2CAP_LE_CREDIT_BASED_CONNECTION_MIN_MTU: u16 = 23;
pub const L2CAP_LE_CREDIT_BASED_CONNECTION_MAX_MTU: u16 = u16::MAX;
pub const L2CAP_LE_CREDIT_BASED_CONNECTION_MIN_MPS: u16 = 23;
pub const L2CAP_LE_CREDIT_BASED_CONNECTION_MAX_MPS: u16 = 65_533;
pub const L2CAP_LE_CREDIT_BASED_CONNECTION_DEFAULT_MTU: u16 = 2048;
pub const L2CAP_LE_CREDIT_BASED_CONNECTION_DEFAULT_MPS: u16 = 2048;
pub const L2CAP_LE_CREDIT_BASED_CONNECTION_DEFAULT_INITIAL_CREDITS: u16 = 256;

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
        if self.peer_credits <= self.peer_credit_threshold {
            let grant = self.peer_max_credits - self.peer_credits;
            if grant != 0 {
                self.pending_credit_grants.push_back(grant);
                self.peer_credits = self.peer_max_credits;
            }
        }

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

    pub fn disconnect(&mut self) -> Result<()> {
        self.ensure_connected()?;
        self.state = LeCreditBasedChannelState::Disconnected;
        self.output_stream.clear();
        self.output_sdu.clear();
        self.outbound_pdus.clear();
        self.reset_input();
        Ok(())
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

    fn reset_input(&mut self) {
        self.input_sdu.clear();
        self.input_sdu_length = None;
    }
}
