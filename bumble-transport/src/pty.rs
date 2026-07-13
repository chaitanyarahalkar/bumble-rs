use crate::{H4Transport, PacketSink, PacketSource, Result};
use bumble_hci::HciPacket;
use serialport::{SerialPort, TTYPort};
use std::fs;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};

/// A raw pseudo-terminal H4 endpoint. The peer uses [`replica_path`](Self::replica_path).
pub struct PtyTransport {
    inner: H4Transport<TTYPort>,
    replica: TTYPort,
    replica_path: PathBuf,
    symlink_path: Option<PathBuf>,
}

impl PtyTransport {
    pub fn open(symlink_path: Option<impl AsRef<Path>>) -> Result<Self> {
        let (primary, replica) = TTYPort::pair()?;
        let replica_path = replica
            .name()
            .map(PathBuf::from)
            .ok_or_else(|| crate::Error::InvalidSpec("PTY has no replica path".into()))?;
        let symlink_path = symlink_path.map(|path| path.as_ref().to_path_buf());
        if let Some(link) = &symlink_path {
            symlink(&replica_path, link)?;
        }
        Ok(Self {
            inner: H4Transport::new(primary),
            replica,
            replica_path,
            symlink_path,
        })
    }

    pub fn replica_path(&self) -> &Path {
        &self.replica_path
    }

    pub fn replica(&self) -> &TTYPort {
        &self.replica
    }

    pub fn replica_mut(&mut self) -> &mut TTYPort {
        &mut self.replica
    }
}

impl PacketSource for PtyTransport {
    fn read_packet(&mut self) -> Result<Option<HciPacket>> {
        self.inner.read_packet()
    }
}

impl PacketSink for PtyTransport {
    fn write_packet(&mut self, packet: &HciPacket) -> Result<()> {
        self.inner.write_packet(packet)
    }

    fn flush(&mut self) -> Result<()> {
        self.inner.flush()
    }
}

impl Drop for PtyTransport {
    fn drop(&mut self) {
        if let Some(path) = &self.symlink_path {
            let _ = fs::remove_file(path);
        }
    }
}
