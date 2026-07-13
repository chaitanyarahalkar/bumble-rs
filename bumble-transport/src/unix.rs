use crate::{H4Transport, PacketSink, PacketSource, Result};
use bumble_hci::HciPacket;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;

#[derive(Debug)]
pub struct UnixTransport {
    inner: H4Transport<UnixStream>,
}

impl UnixTransport {
    pub fn connect(path: impl AsRef<Path>) -> Result<Self> {
        Ok(Self::from_stream(UnixStream::connect(path)?))
    }

    pub fn from_stream(stream: UnixStream) -> Self {
        Self {
            inner: H4Transport::new(stream),
        }
    }

    pub fn try_split(self) -> Result<(Self, Self)> {
        let source = Self::from_stream(self.inner.get_ref().try_clone()?);
        Ok((source, self))
    }
}

impl PacketSource for UnixTransport {
    fn read_packet(&mut self) -> Result<Option<HciPacket>> {
        self.inner.read_packet()
    }
}

impl PacketSink for UnixTransport {
    fn write_packet(&mut self, packet: &HciPacket) -> Result<()> {
        self.inner.write_packet(packet)
    }

    fn flush(&mut self) -> Result<()> {
        self.inner.flush()
    }
}

#[derive(Debug)]
pub struct UnixServer {
    listener: UnixListener,
}

impl UnixServer {
    pub fn bind(path: impl AsRef<Path>) -> Result<Self> {
        Ok(Self {
            listener: UnixListener::bind(path)?,
        })
    }

    pub fn accept(&self) -> Result<UnixTransport> {
        let (stream, _) = self.listener.accept()?;
        Ok(UnixTransport::from_stream(stream))
    }
}
