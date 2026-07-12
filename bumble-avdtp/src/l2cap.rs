//! AVDTP signaling bound to one live Classic L2CAP channel.

use core::fmt;

use bumble_l2cap::{ChannelManager, ClassicChannelState};

use crate::session::Session;
use crate::{Error as AvdtpError, Message, MessageAssembler, MessageType};

#[derive(Debug)]
pub enum BindingError {
    Avdtp(AvdtpError),
    L2cap(bumble_l2cap::Error),
    ChannelNotOpen(u16),
}

impl fmt::Display for BindingError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Avdtp(error) => write!(formatter, "AVDTP: {error}"),
            Self::L2cap(error) => write!(formatter, "L2CAP: {error}"),
            Self::ChannelNotOpen(cid) => {
                write!(formatter, "Classic L2CAP channel {cid:#06x} is not open")
            }
        }
    }
}

impl std::error::Error for BindingError {}

impl From<AvdtpError> for BindingError {
    fn from(value: AvdtpError) -> Self {
        Self::Avdtp(value)
    }
}

impl From<bumble_l2cap::Error> for BindingError {
    fn from(value: bumble_l2cap::Error) -> Self {
        Self::L2cap(value)
    }
}

pub type Result<T> = core::result::Result<T, BindingError>;

#[derive(Debug)]
pub struct L2capSession {
    source_cid: u16,
    peer_mtu: usize,
    next_transaction_label: u8,
    assembler: MessageAssembler,
    session: Session,
    responses: Vec<(u8, Message)>,
}

impl L2capSession {
    pub fn new(source_cid: u16, manager: &ChannelManager, session: Session) -> Result<Self> {
        let channel = manager
            .channel(source_cid)
            .ok_or(BindingError::ChannelNotOpen(source_cid))?;
        if channel.state != ClassicChannelState::Open {
            return Err(BindingError::ChannelNotOpen(source_cid));
        }
        Ok(Self {
            source_cid,
            peer_mtu: channel.peer_mtu as usize,
            next_transaction_label: 0,
            assembler: MessageAssembler::default(),
            session,
            responses: Vec::new(),
        })
    }

    pub fn session(&self) -> &Session {
        &self.session
    }

    pub fn session_mut(&mut self) -> &mut Session {
        &mut self.session
    }

    pub fn send_command(&mut self, manager: &mut ChannelManager, command: Message) -> Result<u8> {
        if command.message_type() != MessageType::Command {
            return Err(BindingError::Avdtp(AvdtpError::Invalid(
                "outbound message is not a command",
            )));
        }
        let label = self.next_transaction_label;
        self.next_transaction_label = (self.next_transaction_label + 1) & 0x0F;
        self.send_message(manager, label, &command)?;
        Ok(label)
    }

    pub fn take_response(&mut self, transaction_label: u8) -> Option<Message> {
        let index = self
            .responses
            .iter()
            .position(|(label, _)| *label == transaction_label)?;
        Some(self.responses.remove(index).1)
    }

    /// Consume every pending L2CAP signaling SDU, answer commands, and queue
    /// responses for their transaction labels.
    pub fn poll(&mut self, manager: &mut ChannelManager) -> Result<usize> {
        let mut processed = 0;
        loop {
            let sdu = manager
                .channel_mut(self.source_cid)
                .ok_or(BindingError::ChannelNotOpen(self.source_cid))?
                .pop_received();
            let Some(sdu) = sdu else {
                return Ok(processed);
            };
            if let Some((label, message)) = self.assembler.push(&sdu)? {
                if message.message_type() == MessageType::Command {
                    let response = self.session.handle_command(message);
                    self.send_message(manager, label, &response)?;
                } else {
                    self.responses.push((label, message));
                }
            }
            processed += 1;
        }
    }

    fn send_message(
        &self,
        manager: &mut ChannelManager,
        transaction_label: u8,
        message: &Message,
    ) -> Result<()> {
        for pdu in message.encode_pdus(transaction_label, self.peer_mtu)? {
            manager.send(self.source_cid, &pdu)?;
        }
        Ok(())
    }
}
