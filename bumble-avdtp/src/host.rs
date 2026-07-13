//! AVDTP signaling bound to a Classic L2CAP channel owned by a host [`Device`].

use bumble_host::{Device, LocalLink};
use bumble_l2cap::ClassicChannelState;

use crate::l2cap::{BindingError, Result};
use crate::session::Session;
use crate::{Error as AvdtpError, Message, MessageAssembler, MessageType};

/// AVDTP signaling over one `Device`-managed Classic L2CAP channel.
#[derive(Debug)]
pub struct DeviceSession {
    connection_handle: u16,
    source_cid: u16,
    peer_mtu: usize,
    next_transaction_label: u8,
    assembler: MessageAssembler,
    session: Session,
    responses: Vec<(u8, Message)>,
}

impl DeviceSession {
    pub fn new(
        device: &Device,
        connection_handle: u16,
        source_cid: u16,
        session: Session,
    ) -> Result<Self> {
        let channel = device
            .classic_channel(connection_handle, source_cid)
            .ok_or(BindingError::ChannelNotOpen(source_cid))?;
        if channel.state != ClassicChannelState::Open {
            return Err(BindingError::ChannelNotOpen(source_cid));
        }
        Ok(Self {
            connection_handle,
            source_cid,
            peer_mtu: usize::from(channel.peer_mtu),
            next_transaction_label: 0,
            assembler: MessageAssembler::default(),
            session,
            responses: Vec::new(),
        })
    }

    pub fn connection_handle(&self) -> u16 {
        self.connection_handle
    }

    pub fn source_cid(&self) -> u16 {
        self.source_cid
    }

    pub fn peer_mtu(&self) -> usize {
        self.peer_mtu
    }

    pub fn session(&self) -> &Session {
        &self.session
    }

    pub fn session_mut(&mut self) -> &mut Session {
        &mut self.session
    }

    pub fn send_command(
        &mut self,
        link: &mut LocalLink,
        device: &mut Device,
        command: Message,
    ) -> Result<u8> {
        if command.message_type() != MessageType::Command {
            return Err(BindingError::Avdtp(AvdtpError::Invalid(
                "outbound message is not a command",
            )));
        }
        let label = self.next_transaction_label;
        self.next_transaction_label = (self.next_transaction_label + 1) & 0x0F;
        self.send_message(link, device, label, &command)?;
        Ok(label)
    }

    pub fn take_response(&mut self, transaction_label: u8) -> Option<Message> {
        let index = self
            .responses
            .iter()
            .position(|(label, _)| *label == transaction_label)?;
        Some(self.responses.remove(index).1)
    }

    /// Consume all received signaling SDUs, answer commands, and retain
    /// responses until the initiating application takes them by label.
    pub fn poll(&mut self, link: &mut LocalLink, device: &mut Device) -> Result<usize> {
        let sdus = device.take_classic_channel_sdus(self.connection_handle, self.source_cid);
        let mut processed = 0;
        for sdu in sdus {
            if let Some((label, message)) = self.assembler.push(&sdu)? {
                if message.message_type() == MessageType::Command {
                    let response = self.session.handle_command(message);
                    self.send_message(link, device, label, &response)?;
                } else {
                    self.responses.push((label, message));
                }
            }
            processed += 1;
        }
        Ok(processed)
    }

    fn send_message(
        &self,
        link: &mut LocalLink,
        device: &mut Device,
        transaction_label: u8,
        message: &Message,
    ) -> Result<()> {
        for pdu in message.encode_pdus(transaction_label, self.peer_mtu)? {
            device.send_classic_channel_sdu(link, self.connection_handle, self.source_cid, &pdu)?;
        }
        Ok(())
    }
}
