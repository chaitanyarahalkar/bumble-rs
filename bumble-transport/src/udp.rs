use crate::{PacketFramer, PacketSink, PacketSource, Result, MAX_HCI_PACKET_SIZE};
use bumble_hci::HciPacket;
use std::collections::VecDeque;
use std::net::{SocketAddr, ToSocketAddrs, UdpSocket};

#[derive(Debug)]
pub struct UdpTransport {
    socket: UdpSocket,
    framer: PacketFramer,
    pending: VecDeque<HciPacket>,
}

impl UdpTransport {
    pub fn bind(
        local_address: impl ToSocketAddrs,
        remote_address: impl ToSocketAddrs,
    ) -> Result<Self> {
        let socket = UdpSocket::bind(local_address)?;
        socket.connect(remote_address)?;
        Ok(Self::from_socket(socket))
    }

    pub fn from_socket(socket: UdpSocket) -> Self {
        Self {
            socket,
            framer: PacketFramer::new(),
            pending: VecDeque::new(),
        }
    }

    pub fn local_addr(&self) -> Result<SocketAddr> {
        Ok(self.socket.local_addr()?)
    }

    pub fn peer_addr(&self) -> Result<SocketAddr> {
        Ok(self.socket.peer_addr()?)
    }

    pub fn get_socket(&self) -> &UdpSocket {
        &self.socket
    }
}

impl PacketSource for UdpTransport {
    fn read_packet(&mut self) -> Result<Option<HciPacket>> {
        if let Some(packet) = self.pending.pop_front() {
            return Ok(Some(packet));
        }
        let mut datagram = vec![0u8; MAX_HCI_PACKET_SIZE];
        loop {
            let count = self.socket.recv(&mut datagram)?;
            self.pending.extend(self.framer.feed(&datagram[..count])?);
            if let Some(packet) = self.pending.pop_front() {
                return Ok(Some(packet));
            }
        }
    }
}

impl PacketSink for UdpTransport {
    fn write_packet(&mut self, packet: &HciPacket) -> Result<()> {
        let bytes = packet.to_bytes();
        let sent = self.socket.send(&bytes)?;
        if sent != bytes.len() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::WriteZero,
                "partial HCI UDP datagram",
            )
            .into());
        }
        Ok(())
    }
}
