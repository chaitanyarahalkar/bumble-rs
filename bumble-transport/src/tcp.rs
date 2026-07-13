use crate::{H4Transport, PacketSink, PacketSource, Result};
use bumble_hci::HciPacket;
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream, ToSocketAddrs};

#[derive(Debug)]
pub struct TcpTransport {
    inner: H4Transport<TcpStream>,
}

impl TcpTransport {
    pub fn connect(address: impl ToSocketAddrs) -> Result<Self> {
        Self::from_stream(TcpStream::connect(address)?)
    }

    pub fn from_stream(stream: TcpStream) -> Result<Self> {
        stream.set_nodelay(true)?;
        Ok(Self {
            inner: H4Transport::new(stream),
        })
    }

    pub fn local_addr(&self) -> Result<SocketAddr> {
        Ok(self.inner.get_ref().local_addr()?)
    }

    pub fn peer_addr(&self) -> Result<SocketAddr> {
        Ok(self.inner.get_ref().peer_addr()?)
    }

    pub fn shutdown(&self) -> Result<()> {
        self.inner.get_ref().shutdown(Shutdown::Both)?;
        Ok(())
    }

    pub fn try_split(self) -> Result<(Self, Self)> {
        let source = Self::from_stream(self.inner.get_ref().try_clone()?)?;
        Ok((source, self))
    }
}

impl PacketSource for TcpTransport {
    fn read_packet(&mut self) -> Result<Option<HciPacket>> {
        self.inner.read_packet()
    }
}

impl PacketSink for TcpTransport {
    fn write_packet(&mut self, packet: &HciPacket) -> Result<()> {
        self.inner.write_packet(packet)
    }

    fn flush(&mut self) -> Result<()> {
        self.inner.flush()
    }
}

#[derive(Debug)]
pub struct TcpServer {
    listener: TcpListener,
}

impl TcpServer {
    pub fn bind(address: impl ToSocketAddrs) -> Result<Self> {
        Ok(Self {
            listener: TcpListener::bind(address)?,
        })
    }

    pub fn local_addr(&self) -> Result<SocketAddr> {
        Ok(self.listener.local_addr()?)
    }

    pub fn accept(&self) -> Result<TcpTransport> {
        let (stream, _) = self.listener.accept()?;
        TcpTransport::from_stream(stream)
    }
}
