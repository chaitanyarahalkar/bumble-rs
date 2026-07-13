//! Enhanced Retransmission Mode control fields and sans-I/O data engine.

use std::collections::VecDeque;

use crate::{Error, Result};

pub const ERTM_SEQUENCE_MODULUS: u8 = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum SegmentationAndReassembly {
    Unsegmented = 0,
    Start = 1,
    End = 2,
    Continuation = 3,
}

impl TryFrom<u8> for SegmentationAndReassembly {
    type Error = Error;

    fn try_from(value: u8) -> Result<Self> {
        match value {
            0 => Ok(Self::Unsegmented),
            1 => Ok(Self::Start),
            2 => Ok(Self::End),
            3 => Ok(Self::Continuation),
            _ => Err(Error::InvalidPacket("invalid ERTM SAR value".into())),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum SupervisoryFunction {
    ReceiverReady = 0,
    Reject = 1,
    ReceiverNotReady = 2,
    SelectiveReject = 3,
}

impl TryFrom<u8> for SupervisoryFunction {
    type Error = Error;

    fn try_from(value: u8) -> Result<Self> {
        match value {
            0 => Ok(Self::ReceiverReady),
            1 => Ok(Self::Reject),
            2 => Ok(Self::ReceiverNotReady),
            3 => Ok(Self::SelectiveReject),
            _ => Err(Error::InvalidPacket(
                "invalid ERTM supervisory function".into(),
            )),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EnhancedControlField {
    Information {
        tx_seq: u8,
        final_bit: bool,
        req_seq: u8,
        sar: SegmentationAndReassembly,
    },
    Supervisory {
        function: SupervisoryFunction,
        poll: bool,
        final_bit: bool,
        req_seq: u8,
    },
}

impl EnhancedControlField {
    pub fn from_bytes(data: &[u8]) -> Result<Self> {
        if data.len() < 2 {
            return Err(Error::InvalidPacket("truncated ERTM control field".into()));
        }
        if data[0] & 1 == 0 {
            Ok(Self::Information {
                tx_seq: (data[0] >> 1) & 0x3f,
                final_bit: data[0] & 0x80 != 0,
                req_seq: data[1] & 0x3f,
                sar: SegmentationAndReassembly::try_from(data[1] >> 6)?,
            })
        } else {
            Ok(Self::Supervisory {
                function: SupervisoryFunction::try_from((data[0] >> 2) & 3)?,
                poll: data[0] & 0x10 != 0,
                final_bit: data[0] & 0x80 != 0,
                req_seq: data[1] & 0x3f,
            })
        }
    }

    pub fn to_bytes(self) -> Result<[u8; 2]> {
        let validate_seq = |name: &str, value: u8| {
            if value < ERTM_SEQUENCE_MODULUS {
                Ok(value)
            } else {
                Err(Error::InvalidPacket(format!(
                    "ERTM {name} sequence number out of range"
                )))
            }
        };
        Ok(match self {
            Self::Information {
                tx_seq,
                final_bit,
                req_seq,
                sar,
            } => [
                validate_seq("transmit", tx_seq)? << 1 | u8::from(final_bit) << 7,
                validate_seq("request", req_seq)? | (sar as u8) << 6,
            ],
            Self::Supervisory {
                function,
                poll,
                final_bit,
                req_seq,
            } => [
                1 | (function as u8) << 2 | u8::from(poll) << 4 | u8::from(final_bit) << 7,
                validate_seq("request", req_seq)?,
            ],
        })
    }

    pub fn req_seq(self) -> u8 {
        match self {
            Self::Information { req_seq, .. } | Self::Supervisory { req_seq, .. } => req_seq,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ErtmConfig {
    pub local_mtu: u16,
    pub peer_mtu: u16,
    pub local_mps: u16,
    pub peer_mps: u16,
    pub tx_window_size: u8,
    /// Zero means unlimited, matching Bumble's processor convention.
    pub max_retransmissions: u8,
    /// Caller-defined logical ticks. The engine never reads a wall clock.
    pub retransmission_timeout_ticks: u32,
}

impl Default for ErtmConfig {
    fn default() -> Self {
        Self {
            local_mtu: 2048,
            peer_mtu: 2048,
            local_mps: 1010,
            peer_mps: 1010,
            tx_window_size: 63,
            max_retransmissions: 0,
            retransmission_timeout_ticks: 2_000,
        }
    }
}

impl ErtmConfig {
    pub fn validate(self) -> Result<Self> {
        if self.local_mtu == 0 || self.peer_mtu == 0 {
            return Err(Error::InvalidPacket("ERTM MTU cannot be zero".into()));
        }
        if self.local_mps == 0 || self.peer_mps == 0 {
            return Err(Error::InvalidPacket("ERTM MPS cannot be zero".into()));
        }
        if !(1..ERTM_SEQUENCE_MODULUS).contains(&self.tx_window_size) {
            return Err(Error::InvalidPacket(
                "ERTM transmit window must be between 1 and 63".into(),
            ));
        }
        if self.retransmission_timeout_ticks == 0 {
            return Err(Error::InvalidPacket(
                "ERTM retransmission timeout cannot be zero".into(),
            ));
        }
        Ok(self)
    }
}

#[derive(Clone, Debug)]
struct PendingFrame {
    payload: Vec<u8>,
    tx_seq: u8,
    sar: SegmentationAndReassembly,
    sdu_length: Option<u16>,
}

impl PendingFrame {
    fn encode(&self, req_seq: u8, final_bit: bool) -> Result<Vec<u8>> {
        let mut frame = EnhancedControlField::Information {
            tx_seq: self.tx_seq,
            final_bit,
            req_seq,
            sar: self.sar,
        }
        .to_bytes()?
        .to_vec();
        if let Some(length) = self.sdu_length {
            frame.extend_from_slice(&length.to_le_bytes());
        }
        frame.extend_from_slice(&self.payload);
        Ok(frame)
    }
}

#[derive(Clone, Debug)]
struct TransmittedFrame {
    frame: PendingFrame,
    transmissions: u16,
}

/// A deterministic ERTM processor. Poll [`Self::poll_outbound`] for complete
/// mode frames and relay them to the peer engine; delivered SDUs are returned
/// by [`Self::pop_received`].
#[derive(Clone, Debug)]
pub struct ErtmEngine {
    config: ErtmConfig,
    pending: VecDeque<PendingFrame>,
    tx_window: VecDeque<TransmittedFrame>,
    outbound: VecDeque<Vec<u8>>,
    received: VecDeque<Vec<u8>>,
    next_tx_seq: u8,
    last_acked_tx_seq: u8,
    expected_tx_seq: u8,
    last_acked_rx_seq: u8,
    remote_busy: bool,
    local_busy: bool,
    input_sdu: Vec<u8>,
    input_sdu_length: Option<usize>,
    now: u64,
    retransmission_deadline: Option<u64>,
    failed: bool,
}

impl ErtmEngine {
    pub fn new(config: ErtmConfig) -> Result<Self> {
        let config = config.validate()?;
        Ok(Self {
            config,
            pending: VecDeque::new(),
            tx_window: VecDeque::new(),
            outbound: VecDeque::new(),
            received: VecDeque::new(),
            next_tx_seq: 0,
            last_acked_tx_seq: 0,
            expected_tx_seq: 0,
            last_acked_rx_seq: 0,
            remote_busy: false,
            local_busy: false,
            input_sdu: Vec::new(),
            input_sdu_length: None,
            now: 0,
            retransmission_deadline: None,
            failed: false,
        })
    }

    pub fn send_sdu(&mut self, sdu: &[u8]) -> Result<()> {
        self.ensure_active()?;
        if sdu.len() > usize::from(self.config.peer_mtu) {
            return Err(Error::InvalidPacket(format!(
                "SDU exceeds peer MTU {}",
                self.config.peer_mtu
            )));
        }
        let mps = usize::from(self.config.peer_mps);
        if sdu.len() <= mps {
            let tx_seq = self.take_tx_seq();
            self.pending.push_back(PendingFrame {
                payload: sdu.to_vec(),
                tx_seq,
                sar: SegmentationAndReassembly::Unsegmented,
                sdu_length: None,
            });
        } else {
            let length = u16::try_from(sdu.len())
                .map_err(|_| Error::InvalidPacket("ERTM SDU length exceeds 16 bits".into()))?;
            for (index, chunk) in sdu.chunks(mps).enumerate() {
                let offset = index * mps;
                let sar = if index == 0 {
                    SegmentationAndReassembly::Start
                } else if offset + chunk.len() == sdu.len() {
                    SegmentationAndReassembly::End
                } else {
                    SegmentationAndReassembly::Continuation
                };
                let tx_seq = self.take_tx_seq();
                self.pending.push_back(PendingFrame {
                    payload: chunk.to_vec(),
                    tx_seq,
                    sar,
                    sdu_length: (sar == SegmentationAndReassembly::Start).then_some(length),
                });
            }
        }
        self.process_output()
    }

    pub fn receive_frame(&mut self, frame: &[u8]) -> Result<()> {
        self.ensure_active()?;
        let control = EnhancedControlField::from_bytes(frame)?;
        let acked = self.acknowledge(control.req_seq())?;
        match control {
            EnhancedControlField::Information {
                tx_seq,
                final_bit,
                sar,
                ..
            } => {
                if self.local_busy {
                    self.send_supervisory(SupervisoryFunction::ReceiverNotReady, false, final_bit)?;
                    return Ok(());
                }
                if tx_seq != self.expected_tx_seq {
                    self.send_supervisory(SupervisoryFunction::Reject, false, final_bit)?;
                    return Ok(());
                }
                self.expected_tx_seq = sequence_add(self.expected_tx_seq, 1);
                self.accept_information(sar, &frame[2..])?;
                self.process_output()?;
                if self.last_acked_rx_seq != self.expected_tx_seq {
                    self.send_supervisory(SupervisoryFunction::ReceiverReady, false, final_bit)?;
                }
            }
            EnhancedControlField::Supervisory {
                function,
                poll,
                final_bit,
                req_seq,
            } => {
                self.remote_busy = function == SupervisoryFunction::ReceiverNotReady;
                match function {
                    SupervisoryFunction::Reject => self.retransmit_all()?,
                    SupervisoryFunction::SelectiveReject => self.retransmit_one(req_seq)?,
                    SupervisoryFunction::ReceiverReady | SupervisoryFunction::ReceiverNotReady => {
                        if final_bit && !acked && !self.tx_window.is_empty() {
                            self.retransmit_all()?;
                        }
                    }
                }
                if poll {
                    self.send_supervisory(
                        if self.local_busy {
                            SupervisoryFunction::ReceiverNotReady
                        } else {
                            SupervisoryFunction::ReceiverReady
                        },
                        false,
                        true,
                    )?;
                }
                self.process_output()?;
            }
        }
        Ok(())
    }

    pub fn set_receiver_busy(&mut self, busy: bool) -> Result<()> {
        self.ensure_active()?;
        if self.local_busy != busy {
            self.local_busy = busy;
            self.send_supervisory(
                if busy {
                    SupervisoryFunction::ReceiverNotReady
                } else {
                    SupervisoryFunction::ReceiverReady
                },
                false,
                false,
            )?;
        }
        Ok(())
    }

    pub fn tick(&mut self, ticks: u32) -> Result<()> {
        self.ensure_active()?;
        self.now = self.now.saturating_add(u64::from(ticks));
        if self
            .retransmission_deadline
            .is_some_and(|deadline| self.now >= deadline)
            && !self.tx_window.is_empty()
        {
            self.retransmit_all()?;
        }
        Ok(())
    }

    pub fn poll_outbound(&mut self) -> Option<Vec<u8>> {
        self.outbound.pop_front()
    }

    pub fn drain_outbound(&mut self) -> Vec<Vec<u8>> {
        self.outbound.drain(..).collect()
    }

    pub fn pop_received(&mut self) -> Option<Vec<u8>> {
        self.received.pop_front()
    }

    pub fn pending_frames(&self) -> usize {
        self.pending.len() + self.tx_window.len()
    }

    pub fn unacked_frames(&self) -> usize {
        self.tx_window.len()
    }

    pub fn remote_is_busy(&self) -> bool {
        self.remote_busy
    }

    pub fn is_failed(&self) -> bool {
        self.failed
    }

    fn accept_information(&mut self, sar: SegmentationAndReassembly, payload: &[u8]) -> Result<()> {
        match sar {
            SegmentationAndReassembly::Unsegmented => {
                if !self.input_sdu.is_empty() || self.input_sdu_length.is_some() {
                    self.reset_input();
                    return Err(Error::InvalidPacket(
                        "unsegmented frame interrupted an ERTM SDU".into(),
                    ));
                }
                if payload.len() > usize::from(self.config.local_mtu) {
                    return Err(Error::InvalidPacket("ERTM SDU exceeds local MTU".into()));
                }
                self.received.push_back(payload.to_vec());
            }
            SegmentationAndReassembly::Start => {
                if payload.len() < 2 {
                    return Err(Error::InvalidPacket(
                        "ERTM start frame is missing SDU length".into(),
                    ));
                }
                if !self.input_sdu.is_empty() || self.input_sdu_length.is_some() {
                    self.reset_input();
                    return Err(Error::InvalidPacket(
                        "ERTM start frame interrupted an SDU".into(),
                    ));
                }
                let length = usize::from(u16::from_le_bytes([payload[0], payload[1]]));
                if length > usize::from(self.config.local_mtu) {
                    return Err(Error::InvalidPacket("ERTM SDU exceeds local MTU".into()));
                }
                self.input_sdu_length = Some(length);
                self.input_sdu.extend_from_slice(&payload[2..]);
                self.check_partial_length()?;
            }
            SegmentationAndReassembly::Continuation => {
                if self.input_sdu_length.is_none() {
                    return Err(Error::InvalidPacket(
                        "ERTM continuation without start".into(),
                    ));
                }
                self.input_sdu.extend_from_slice(payload);
                self.check_partial_length()?;
            }
            SegmentationAndReassembly::End => {
                let Some(length) = self.input_sdu_length else {
                    return Err(Error::InvalidPacket("ERTM end without start".into()));
                };
                self.input_sdu.extend_from_slice(payload);
                if self.input_sdu.len() != length {
                    self.reset_input();
                    return Err(Error::InvalidPacket("ERTM SDU length mismatch".into()));
                }
                self.received.push_back(std::mem::take(&mut self.input_sdu));
                self.input_sdu_length = None;
            }
        }
        Ok(())
    }

    fn check_partial_length(&mut self) -> Result<()> {
        if self
            .input_sdu_length
            .is_some_and(|length| self.input_sdu.len() >= length)
        {
            self.reset_input();
            return Err(Error::InvalidPacket(
                "ERTM segmented SDU reached its length before an end frame".into(),
            ));
        }
        Ok(())
    }

    fn acknowledge(&mut self, new_seq: u8) -> Result<bool> {
        if new_seq >= ERTM_SEQUENCE_MODULUS {
            return Err(Error::InvalidPacket(
                "ERTM acknowledgment sequence out of range".into(),
            ));
        }
        let count = usize::from(sequence_distance(self.last_acked_tx_seq, new_seq));
        if count > self.tx_window.len() {
            return Err(Error::InvalidPacket(format!(
                "ERTM acknowledged {count} frames with only {} outstanding",
                self.tx_window.len()
            )));
        }
        for _ in 0..count {
            self.tx_window.pop_front();
        }
        if count != 0 {
            self.last_acked_tx_seq = new_seq;
            self.refresh_retransmission_deadline();
        }
        Ok(count != 0)
    }

    fn process_output(&mut self) -> Result<()> {
        if self.remote_busy {
            return Ok(());
        }
        while self.tx_window.len() < usize::from(self.config.tx_window_size) {
            let Some(frame) = self.pending.pop_front() else {
                break;
            };
            let encoded = frame.encode(self.expected_tx_seq, false)?;
            self.last_acked_rx_seq = self.expected_tx_seq;
            self.tx_window.push_back(TransmittedFrame {
                frame,
                transmissions: 1,
            });
            self.outbound.push_back(encoded);
        }
        self.refresh_retransmission_deadline();
        Ok(())
    }

    fn retransmit_all(&mut self) -> Result<()> {
        if self.remote_busy {
            return Ok(());
        }
        if let Some(frame) = self
            .tx_window
            .iter()
            .find(|frame| retransmission_limit_reached(self.config.max_retransmissions, frame))
        {
            let tx_seq = frame.frame.tx_seq;
            self.failed = true;
            return Err(retransmission_limit_error(tx_seq));
        }
        for frame in &mut self.tx_window {
            frame.transmissions += 1;
            self.outbound
                .push_back(frame.frame.encode(self.expected_tx_seq, false)?);
            self.last_acked_rx_seq = self.expected_tx_seq;
        }
        self.refresh_retransmission_deadline();
        Ok(())
    }

    fn retransmit_one(&mut self, tx_seq: u8) -> Result<()> {
        if self.remote_busy {
            return Ok(());
        }
        let frame = self
            .tx_window
            .iter_mut()
            .find(|frame| frame.frame.tx_seq == tx_seq)
            .ok_or_else(|| {
                Error::InvalidPacket(format!(
                    "selective reject requested unknown ERTM sequence {tx_seq}"
                ))
            })?;
        if retransmission_limit_reached(self.config.max_retransmissions, frame) {
            let tx_seq = frame.frame.tx_seq;
            self.failed = true;
            return Err(retransmission_limit_error(tx_seq));
        }
        frame.transmissions += 1;
        self.outbound
            .push_back(frame.frame.encode(self.expected_tx_seq, false)?);
        self.last_acked_rx_seq = self.expected_tx_seq;
        self.refresh_retransmission_deadline();
        Ok(())
    }

    fn send_supervisory(
        &mut self,
        function: SupervisoryFunction,
        poll: bool,
        final_bit: bool,
    ) -> Result<()> {
        self.outbound.push_back(
            EnhancedControlField::Supervisory {
                function,
                poll,
                final_bit,
                req_seq: self.expected_tx_seq,
            }
            .to_bytes()?
            .to_vec(),
        );
        self.last_acked_rx_seq = self.expected_tx_seq;
        Ok(())
    }

    fn take_tx_seq(&mut self) -> u8 {
        let tx_seq = self.next_tx_seq;
        self.next_tx_seq = sequence_add(self.next_tx_seq, 1);
        tx_seq
    }

    fn refresh_retransmission_deadline(&mut self) {
        self.retransmission_deadline = (!self.tx_window.is_empty()).then_some(
            self.now
                .saturating_add(u64::from(self.config.retransmission_timeout_ticks)),
        );
    }

    fn reset_input(&mut self) {
        self.input_sdu.clear();
        self.input_sdu_length = None;
    }

    fn ensure_active(&self) -> Result<()> {
        if self.failed {
            Err(Error::InvalidPacket("ERTM engine has failed".into()))
        } else {
            Ok(())
        }
    }
}

fn retransmission_limit_reached(max: u8, frame: &TransmittedFrame) -> bool {
    let retransmissions = frame.transmissions.saturating_sub(1);
    max != 0 && retransmissions >= u16::from(max)
}

fn retransmission_limit_error(tx_seq: u8) -> Error {
    Error::InvalidPacket(format!(
        "maximum retransmissions exceeded for ERTM sequence {tx_seq}"
    ))
}

fn sequence_add(sequence: u8, delta: u8) -> u8 {
    sequence.wrapping_add(delta) % ERTM_SEQUENCE_MODULUS
}

fn sequence_distance(from: u8, to: u8) -> u8 {
    to.wrapping_add(ERTM_SEQUENCE_MODULUS).wrapping_sub(from) % ERTM_SEQUENCE_MODULUS
}
