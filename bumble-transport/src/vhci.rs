use crate::{Error, H4Transport, PacketSink, PacketSource, Result};
use bumble_hci::HciPacket;
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::Path;

pub const HCI_VENDOR_PACKET: u8 = 0xff;
pub const HCI_BREDR: u8 = 0x00;

/// Linux VHCI device transport with Bumble's vendor bootstrap handshake.
pub struct VhciTransport<T> {
    inner: H4Transport<T>,
    hci_index: u16,
    controller_type: u8,
}

impl<T: Read + Write> VhciTransport<T> {
    /// Configure a newly opened VHCI stream and consume its four-byte index
    /// response before normal H4 framing begins.
    pub fn from_io(mut io: T, controller_type: u8) -> Result<Self> {
        io.write_all(&[HCI_VENDOR_PACKET, controller_type])?;
        io.flush()?;
        let mut response = [0u8; 4];
        io.read_exact(&mut response)?;
        if response[0] != HCI_VENDOR_PACKET {
            return Err(Error::InvalidSpec(format!(
                "VHCI expected vendor response, got {:#04x}",
                response[0]
            )));
        }
        let hci_index = u16::from_be_bytes([response[2], response[3]]);
        Ok(Self {
            inner: H4Transport::new(io),
            hci_index,
            controller_type,
        })
    }

    pub fn hci_index(&self) -> u16 {
        self.hci_index
    }

    pub fn controller_type(&self) -> u8 {
        self.controller_type
    }

    pub fn get_ref(&self) -> &T {
        self.inner.get_ref()
    }

    pub fn get_mut(&mut self) -> &mut T {
        self.inner.get_mut()
    }
}

impl VhciTransport<File> {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let file = OpenOptions::new().read(true).write(true).open(path)?;
        Self::from_io(file, HCI_BREDR)
    }
}

impl<T: Read> PacketSource for VhciTransport<T> {
    fn read_packet(&mut self) -> Result<Option<HciPacket>> {
        self.inner.read_packet()
    }
}

impl<T: Write> PacketSink for VhciTransport<T> {
    fn write_packet(&mut self, packet: &HciPacket) -> Result<()> {
        self.inner.write_packet(packet)
    }

    fn flush(&mut self) -> Result<()> {
        self.inner.flush()
    }
}
