use bumble_l2cap::{ChannelManager, ClassicChannelState};

use crate::{Error, Message, Result, HID_CONTROL_PSM, HID_INTERRUPT_PSM};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HidChannel {
    Control,
    Interrupt,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReceivedMessage {
    pub channel: HidChannel,
    pub message: Message,
}

/// The paired HID control and interrupt channels over live Classic L2CAP.
#[derive(Clone, Debug)]
pub struct L2capTransport {
    control_cid: u16,
    interrupt_cid: u16,
    control_peer_mtu: usize,
    interrupt_peer_mtu: usize,
}

impl L2capTransport {
    pub fn new(control_cid: u16, interrupt_cid: u16, manager: &ChannelManager) -> Result<Self> {
        let control_peer_mtu = validate_channel(manager, control_cid, HID_CONTROL_PSM)?;
        let interrupt_peer_mtu = validate_channel(manager, interrupt_cid, HID_INTERRUPT_PSM)?;
        Ok(Self {
            control_cid,
            interrupt_cid,
            control_peer_mtu,
            interrupt_peer_mtu,
        })
    }

    pub fn control_peer_mtu(&self) -> usize {
        self.control_peer_mtu
    }

    pub fn interrupt_peer_mtu(&self) -> usize {
        self.interrupt_peer_mtu
    }

    pub fn send_control(&self, manager: &mut ChannelManager, message: &Message) -> Result<()> {
        self.send(manager, HidChannel::Control, message)
    }

    pub fn send_interrupt(&self, manager: &mut ChannelManager, message: &Message) -> Result<()> {
        self.send(manager, HidChannel::Interrupt, message)
    }

    pub fn send(
        &self,
        manager: &mut ChannelManager,
        channel: HidChannel,
        message: &Message,
    ) -> Result<()> {
        let bytes = message.to_bytes()?;
        let (cid, peer_mtu) = match channel {
            HidChannel::Control => (self.control_cid, self.control_peer_mtu),
            HidChannel::Interrupt => (self.interrupt_cid, self.interrupt_peer_mtu),
        };
        if bytes.len() > peer_mtu {
            return Err(Error::Invalid("HIDP message exceeds peer MTU"));
        }
        manager.send(cid, &bytes)?;
        Ok(())
    }

    pub fn take_messages(&self, manager: &mut ChannelManager) -> Result<Vec<ReceivedMessage>> {
        let mut messages = Vec::new();
        for (channel, cid) in [
            (HidChannel::Control, self.control_cid),
            (HidChannel::Interrupt, self.interrupt_cid),
        ] {
            loop {
                let pdu = manager
                    .channel_mut(cid)
                    .ok_or(Error::ChannelNotOpen(cid))?
                    .pop_received();
                let Some(pdu) = pdu else { break };
                messages.push(ReceivedMessage {
                    channel,
                    message: Message::from_bytes(&pdu)?,
                });
            }
        }
        Ok(messages)
    }
}

fn validate_channel(manager: &ChannelManager, cid: u16, expected_psm: u16) -> Result<usize> {
    let channel = manager.channel(cid).ok_or(Error::ChannelNotOpen(cid))?;
    if channel.state != ClassicChannelState::Open {
        return Err(Error::ChannelNotOpen(cid));
    }
    if channel.psm != u32::from(expected_psm) {
        return Err(Error::Invalid("HIDP channel PSM"));
    }
    Ok(usize::from(channel.peer_mtu))
}
