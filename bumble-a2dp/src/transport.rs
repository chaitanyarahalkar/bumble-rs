//! A2DP RTP media packets over a live Classic L2CAP channel.

use core::fmt;

use bumble_host::{Device, LocalLink};
use bumble_l2cap::{ChannelManager, ClassicChannelState};
use bumble_rtp::MediaPacket;

#[derive(Debug)]
pub enum Error {
    L2cap(bumble_l2cap::Error),
    Rtp(bumble_rtp::Error),
    ChannelNotOpen(u16),
    PacketExceedsMtu { packet: usize, peer_mtu: u16 },
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

impl From<bumble_rtp::Error> for Error {
    fn from(value: bumble_rtp::Error) -> Self {
        Self::Rtp(value)
    }
}

pub type Result<T> = core::result::Result<T, Error>;

#[derive(Debug)]
pub struct L2capMediaTransport {
    source_cid: u16,
    peer_mtu: u16,
    inbox: Vec<MediaPacket>,
}

impl L2capMediaTransport {
    pub fn new(source_cid: u16, manager: &ChannelManager) -> Result<Self> {
        let channel = manager
            .channel(source_cid)
            .ok_or(Error::ChannelNotOpen(source_cid))?;
        if channel.state != ClassicChannelState::Open {
            return Err(Error::ChannelNotOpen(source_cid));
        }
        Ok(Self {
            source_cid,
            peer_mtu: channel.peer_mtu,
            inbox: Vec::new(),
        })
    }

    pub fn source_cid(&self) -> u16 {
        self.source_cid
    }

    pub fn peer_mtu(&self) -> u16 {
        self.peer_mtu
    }

    pub fn send(&self, manager: &mut ChannelManager, packet: &MediaPacket) -> Result<()> {
        let bytes = packet.to_bytes()?;
        if bytes.len() > usize::from(self.peer_mtu) {
            return Err(Error::PacketExceedsMtu {
                packet: bytes.len(),
                peer_mtu: self.peer_mtu,
            });
        }
        manager.send(self.source_cid, &bytes)?;
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
            self.inbox.push(MediaPacket::from_bytes(&sdu)?);
            count += 1;
        }
    }

    pub fn take_packets(&mut self) -> Vec<MediaPacket> {
        core::mem::take(&mut self.inbox)
    }
}

/// RTP media over a Classic L2CAP channel owned by a host [`Device`].
#[derive(Debug)]
pub struct DeviceMediaTransport {
    connection_handle: u16,
    source_cid: u16,
    peer_mtu: u16,
    inbox: Vec<MediaPacket>,
}

impl DeviceMediaTransport {
    pub fn new(device: &Device, connection_handle: u16, source_cid: u16) -> Result<Self> {
        let channel = device
            .classic_channel(connection_handle, source_cid)
            .ok_or(Error::ChannelNotOpen(source_cid))?;
        if channel.state != ClassicChannelState::Open {
            return Err(Error::ChannelNotOpen(source_cid));
        }
        Ok(Self {
            connection_handle,
            source_cid,
            peer_mtu: channel.peer_mtu,
            inbox: Vec::new(),
        })
    }

    pub fn connection_handle(&self) -> u16 {
        self.connection_handle
    }

    pub fn source_cid(&self) -> u16 {
        self.source_cid
    }

    pub fn peer_mtu(&self) -> u16 {
        self.peer_mtu
    }

    pub fn send(
        &self,
        link: &mut LocalLink,
        device: &mut Device,
        packet: &MediaPacket,
    ) -> Result<()> {
        let bytes = packet.to_bytes()?;
        if bytes.len() > usize::from(self.peer_mtu) {
            return Err(Error::PacketExceedsMtu {
                packet: bytes.len(),
                peer_mtu: self.peer_mtu,
            });
        }
        device.send_classic_channel_sdu(link, self.connection_handle, self.source_cid, &bytes)?;
        Ok(())
    }

    pub fn poll(&mut self, device: &mut Device) -> Result<usize> {
        let sdus = device.take_classic_channel_sdus(self.connection_handle, self.source_cid);
        let count = sdus.len();
        for sdu in sdus {
            self.inbox.push(MediaPacket::from_bytes(&sdu)?);
        }
        Ok(count)
    }

    pub fn take_packets(&mut self) -> Vec<MediaPacket> {
        core::mem::take(&mut self.inbox)
    }
}
