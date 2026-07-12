//! Audio/Video Control Transport Protocol messages and Classic L2CAP binding.

use core::fmt;
use std::collections::BTreeSet;

use bumble_l2cap::{ChannelManager, ClassicChannelState};

pub const AVCTP_PSM: u16 = 0x0017;
pub const AVCTP_BROWSING_PSM: u16 = 0x001B;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    Truncated(&'static str),
    Invalid(&'static str),
    PayloadTooLong,
    L2cap(bumble_l2cap::Error),
    ChannelNotOpen(u16),
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for Error {}

impl From<bumble_l2cap::Error> for Error {
    fn from(value: bumble_l2cap::Error) -> Self {
        Self::L2cap(value)
    }
}

pub type Result<T> = core::result::Result<T, Error>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum PacketType {
    Single = 0,
    Start = 1,
    Continue = 2,
    End = 3,
}

impl PacketType {
    fn from_bits(value: u8) -> Self {
        match value & 3 {
            0 => Self::Single,
            1 => Self::Start,
            2 => Self::Continue,
            _ => Self::End,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Message {
    pub transaction_label: u8,
    pub is_command: bool,
    pub ipid: bool,
    pub pid: u16,
    pub payload: Vec<u8>,
}

impl Message {
    pub fn command(transaction_label: u8, pid: u16, payload: Vec<u8>) -> Self {
        Self {
            transaction_label,
            is_command: true,
            ipid: false,
            pid,
            payload,
        }
    }

    pub fn response(transaction_label: u8, pid: u16, payload: Vec<u8>) -> Self {
        Self {
            transaction_label,
            is_command: false,
            ipid: false,
            pid,
            payload,
        }
    }

    pub fn ipid(transaction_label: u8, pid: u16) -> Self {
        Self {
            transaction_label,
            is_command: false,
            ipid: true,
            pid,
            payload: Vec::new(),
        }
    }

    fn header(&self, packet_type: PacketType) -> Result<u8> {
        if self.transaction_label > 0x0F || (self.is_command && self.ipid) {
            return Err(Error::Invalid("AVCTP flags"));
        }
        Ok((self.transaction_label << 4)
            | ((packet_type as u8) << 2)
            | (u8::from(!self.is_command) << 1)
            | u8::from(self.ipid))
    }

    pub fn encode_pdus(&self, mtu: usize) -> Result<Vec<Vec<u8>>> {
        if mtu < 4 {
            return Err(Error::Invalid("AVCTP MTU"));
        }
        if self.payload.len() + 3 <= mtu {
            let mut pdu = vec![self.header(PacketType::Single)?];
            pdu.extend_from_slice(&self.pid.to_be_bytes());
            pdu.extend_from_slice(&self.payload);
            return Ok(vec![pdu]);
        }
        let start_capacity = mtu - 4;
        let continuation_capacity = mtu - 3;
        let continuation_count =
            (self.payload.len() - start_capacity).div_ceil(continuation_capacity);
        let packet_count = 1 + continuation_count;
        let packet_count = u8::try_from(packet_count).map_err(|_| Error::PayloadTooLong)?;
        let mut pdus = Vec::with_capacity(usize::from(packet_count));
        let mut start = vec![self.header(PacketType::Start)?, packet_count];
        start.extend_from_slice(&self.pid.to_be_bytes());
        start.extend_from_slice(&self.payload[..start_capacity]);
        pdus.push(start);
        let chunks: Vec<_> = self.payload[start_capacity..]
            .chunks(continuation_capacity)
            .collect();
        for (index, chunk) in chunks.iter().enumerate() {
            let packet_type = if index + 1 == chunks.len() {
                PacketType::End
            } else {
                PacketType::Continue
            };
            let mut pdu = vec![self.header(packet_type)?];
            pdu.extend_from_slice(&self.pid.to_be_bytes());
            pdu.extend_from_slice(chunk);
            pdus.push(pdu);
        }
        Ok(pdus)
    }
}

#[derive(Debug, Default)]
pub struct MessageAssembler {
    pending: Option<PendingMessage>,
}

#[derive(Debug)]
struct PendingMessage {
    transaction_label: u8,
    is_command: bool,
    ipid: bool,
    pid: u16,
    expected_packets: u8,
    received_packets: u8,
    payload: Vec<u8>,
}

impl MessageAssembler {
    pub fn push(&mut self, pdu: &[u8]) -> Result<Option<Message>> {
        let Some(header) = pdu.first().copied() else {
            return Ok(None);
        };
        let transaction_label = header >> 4;
        let packet_type = PacketType::from_bits(header >> 2);
        let is_command = header & 0x02 == 0;
        let ipid = header & 0x01 != 0;
        if is_command && ipid {
            self.pending = None;
            return Ok(None);
        }
        let (pid_offset, payload_offset) = match packet_type {
            PacketType::Start => (2, 4),
            _ => (1, 3),
        };
        if pdu.len() < payload_offset {
            return Ok(None);
        }
        let pid = u16::from_be_bytes([pdu[pid_offset], pdu[pid_offset + 1]]);
        match packet_type {
            PacketType::Single => {
                self.pending = None;
                Ok(Some(Message {
                    transaction_label,
                    is_command,
                    ipid,
                    pid,
                    payload: pdu[payload_offset..].to_vec(),
                }))
            }
            PacketType::Start => {
                let expected_packets = pdu[1];
                if expected_packets < 2 {
                    return Ok(None);
                }
                self.pending = Some(PendingMessage {
                    transaction_label,
                    is_command,
                    ipid,
                    pid,
                    expected_packets,
                    received_packets: 1,
                    payload: pdu[payload_offset..].to_vec(),
                });
                Ok(None)
            }
            PacketType::Continue | PacketType::End => {
                let Some(pending) = self.pending.as_mut() else {
                    return Ok(None);
                };
                if pending.transaction_label != transaction_label
                    || pending.is_command != is_command
                    || pending.ipid != ipid
                    || pending.pid != pid
                {
                    self.pending = None;
                    return Ok(None);
                }
                pending.received_packets = pending.received_packets.saturating_add(1);
                pending.payload.extend_from_slice(&pdu[payload_offset..]);
                if pending.received_packets > pending.expected_packets {
                    self.pending = None;
                    return Ok(None);
                }
                if packet_type == PacketType::End {
                    let pending = self.pending.take().expect("pending message exists");
                    if pending.received_packets != pending.expected_packets {
                        return Ok(None);
                    }
                    Ok(Some(Message {
                        transaction_label,
                        is_command,
                        ipid,
                        pid,
                        payload: pending.payload,
                    }))
                } else {
                    Ok(None)
                }
            }
        }
    }
}

#[derive(Debug)]
pub struct L2capProtocol {
    source_cid: u16,
    peer_mtu: usize,
    accepted_pids: BTreeSet<u16>,
    assembler: MessageAssembler,
    inbox: Vec<Message>,
}

impl L2capProtocol {
    pub fn new(source_cid: u16, manager: &ChannelManager) -> Result<Self> {
        let channel = manager
            .channel(source_cid)
            .ok_or(Error::ChannelNotOpen(source_cid))?;
        if channel.state != ClassicChannelState::Open {
            return Err(Error::ChannelNotOpen(source_cid));
        }
        Ok(Self {
            source_cid,
            peer_mtu: usize::from(channel.peer_mtu),
            accepted_pids: BTreeSet::new(),
            assembler: MessageAssembler::default(),
            inbox: Vec::new(),
        })
    }

    pub fn register_pid(&mut self, pid: u16) {
        self.accepted_pids.insert(pid);
    }

    pub fn send(&self, manager: &mut ChannelManager, message: &Message) -> Result<()> {
        for pdu in message.encode_pdus(self.peer_mtu)? {
            manager.send(self.source_cid, &pdu)?;
        }
        Ok(())
    }

    pub fn poll(&mut self, manager: &mut ChannelManager) -> Result<usize> {
        let mut count = 0;
        loop {
            let sdu = manager
                .channel_mut(self.source_cid)
                .ok_or(Error::ChannelNotOpen(self.source_cid))?
                .pop_received();
            let Some(sdu) = sdu else {
                return Ok(count);
            };
            if let Some(message) = self.assembler.push(&sdu)? {
                if message.is_command && !self.accepted_pids.contains(&message.pid) {
                    self.send(
                        manager,
                        &Message::ipid(message.transaction_label, message.pid),
                    )?;
                } else {
                    self.inbox.push(message);
                }
            }
            count += 1;
        }
    }

    pub fn take_messages(&mut self) -> Vec<Message> {
        core::mem::take(&mut self.inbox)
    }
}
