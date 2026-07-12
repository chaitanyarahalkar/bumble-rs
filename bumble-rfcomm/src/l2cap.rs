//! RFCOMM multiplexer binding for a live Classic L2CAP channel.

use core::fmt;

use bumble_l2cap::{ChannelManager, ClassicChannelState};

use crate::mux::{Multiplexer, Role};
use crate::{Error as RfcommError, RfcommFrame};

#[derive(Debug)]
pub enum BindingError {
    L2cap(bumble_l2cap::Error),
    Rfcomm(RfcommError),
    ChannelNotOpen(u16),
}

impl fmt::Display for BindingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BindingError::L2cap(error) => write!(f, "L2CAP: {error}"),
            BindingError::Rfcomm(error) => write!(f, "RFCOMM: {error}"),
            BindingError::ChannelNotOpen(cid) => {
                write!(f, "Classic L2CAP channel {cid:#06x} is not open")
            }
        }
    }
}

impl std::error::Error for BindingError {}

impl From<bumble_l2cap::Error> for BindingError {
    fn from(value: bumble_l2cap::Error) -> Self {
        BindingError::L2cap(value)
    }
}

impl From<RfcommError> for BindingError {
    fn from(value: RfcommError) -> Self {
        BindingError::Rfcomm(value)
    }
}

pub type Result<T> = core::result::Result<T, BindingError>;

/// An RFCOMM [`Multiplexer`] attached to one open Classic L2CAP channel.
#[derive(Debug)]
pub struct L2capMultiplexer {
    source_cid: u16,
    multiplexer: Multiplexer,
}

impl L2capMultiplexer {
    /// Bind a new multiplexer. The RFCOMM frame ceiling is derived from the
    /// channel's negotiated peer MTU, exactly as upstream's wrapper does.
    pub fn new(role: Role, source_cid: u16, manager: &ChannelManager) -> Result<Self> {
        let channel = manager
            .channel(source_cid)
            .ok_or(BindingError::ChannelNotOpen(source_cid))?;
        if channel.state != ClassicChannelState::Open {
            return Err(BindingError::ChannelNotOpen(source_cid));
        }
        Ok(Self {
            source_cid,
            multiplexer: Multiplexer::new(role, channel.peer_mtu),
        })
    }

    pub fn source_cid(&self) -> u16 {
        self.source_cid
    }

    pub fn multiplexer(&self) -> &Multiplexer {
        &self.multiplexer
    }

    pub fn multiplexer_mut(&mut self) -> &mut Multiplexer {
        &mut self.multiplexer
    }

    pub fn connect(&mut self, manager: &mut ChannelManager) -> Result<()> {
        self.multiplexer.connect()?;
        self.flush(manager)?;
        Ok(())
    }

    pub fn open_dlc(
        &mut self,
        manager: &mut ChannelManager,
        channel: u8,
        max_frame_size: u16,
        initial_credits: u16,
    ) -> Result<()> {
        self.multiplexer
            .open_dlc(channel, max_frame_size, initial_credits)?;
        self.flush(manager)?;
        Ok(())
    }

    pub fn write(&mut self, manager: &mut ChannelManager, dlci: u8, data: &[u8]) -> Result<()> {
        self.multiplexer.write(dlci, data)?;
        self.flush(manager)?;
        Ok(())
    }

    pub fn disconnect(&mut self, manager: &mut ChannelManager) -> Result<()> {
        self.multiplexer.disconnect()?;
        self.flush(manager)?;
        Ok(())
    }

    /// Consume all RFCOMM SDUs currently delivered by L2CAP, advance the
    /// multiplexer, then flush any response frames back to the channel.
    pub fn poll(&mut self, manager: &mut ChannelManager) -> Result<usize> {
        let mut processed = 0;
        loop {
            let sdu = manager
                .channel_mut(self.source_cid)
                .ok_or(BindingError::ChannelNotOpen(self.source_cid))?
                .pop_received();
            let Some(sdu) = sdu else {
                break;
            };
            let frame = RfcommFrame::from_bytes(&sdu)?;
            self.multiplexer.on_pdu(&frame);
            processed += 1;
        }
        processed += self.flush(manager)?;
        Ok(processed)
    }

    /// Serialize and send every frame queued by the multiplexer as one L2CAP
    /// SDU, preserving RFCOMM frame boundaries and ordering.
    pub fn flush(&mut self, manager: &mut ChannelManager) -> Result<usize> {
        let frames = self.multiplexer.drain_outgoing();
        let count = frames.len();
        for frame in frames {
            manager.send(self.source_cid, &frame.to_bytes()?)?;
        }
        Ok(count)
    }
}
