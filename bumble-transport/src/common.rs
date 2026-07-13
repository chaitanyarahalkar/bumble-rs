use bumble_hci::{
    HciPacket, HCI_ACL_DATA_PACKET, HCI_COMMAND_PACKET, HCI_EVENT_PACKET, HCI_ISO_DATA_PACKET,
    HCI_SYNCHRONOUS_DATA_PACKET,
};
use core::fmt;
use std::collections::{BTreeMap, VecDeque};
use std::io::{self, Read, Write};

/// Largest H4 packet representable by the standard two-byte length fields.
pub const MAX_HCI_PACKET_SIZE: usize = 1 + 4 + u16::MAX as usize;

#[derive(Debug)]
pub enum Error {
    Io(io::Error),
    Hci(bumble_hci::Error),
    Serial(serialport::Error),
    Usb(rusb::Error),
    WebSocket(tungstenite::Error),
    InvalidPacketType(u8),
    InvalidLayout,
    InvalidSpec(String),
    Unsupported(String),
    PacketTooLarge(usize),
    TruncatedPacket(usize),
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "transport I/O error: {error}"),
            Self::Hci(error) => write!(formatter, "{error}"),
            Self::Serial(error) => write!(formatter, "serial transport error: {error}"),
            Self::Usb(error) => write!(formatter, "USB transport error: {error}"),
            Self::WebSocket(error) => write!(formatter, "WebSocket transport error: {error}"),
            Self::InvalidPacketType(packet_type) => {
                write!(formatter, "invalid HCI packet type {packet_type:#04x}")
            }
            Self::InvalidLayout => write!(formatter, "invalid HCI packet layout"),
            Self::InvalidSpec(message) => write!(formatter, "invalid transport spec: {message}"),
            Self::Unsupported(feature) => write!(formatter, "unsupported transport: {feature}"),
            Self::PacketTooLarge(size) => write!(formatter, "HCI packet is too large: {size}"),
            Self::TruncatedPacket(size) => {
                write!(
                    formatter,
                    "transport ended with {size} buffered packet bytes"
                )
            }
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Hci(error) => Some(error),
            Self::Serial(error) => Some(error),
            Self::Usb(error) => Some(error),
            Self::WebSocket(error) => Some(error),
            _ => None,
        }
    }
}

impl From<io::Error> for Error {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<bumble_hci::Error> for Error {
    fn from(error: bumble_hci::Error) -> Self {
        Self::Hci(error)
    }
}

impl From<serialport::Error> for Error {
    fn from(error: serialport::Error) -> Self {
        Self::Serial(error)
    }
}

impl From<rusb::Error> for Error {
    fn from(error: rusb::Error) -> Self {
        Self::Usb(error)
    }
}

impl From<tungstenite::Error> for Error {
    fn from(error: tungstenite::Error) -> Self {
        Self::WebSocket(error)
    }
}

pub type Result<T> = core::result::Result<T, Error>;

/// Location and width of a packet's payload-length field.
///
/// `length_offset` is measured after the H4 packet type byte. Standard HCI
/// layouts use either a one-byte or little-endian two-byte field.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PacketLayout {
    pub length_size: u8,
    pub length_offset: u8,
}

impl PacketLayout {
    pub const fn new(length_size: u8, length_offset: u8) -> Self {
        Self {
            length_size,
            length_offset,
        }
    }

    fn header_size(self) -> Result<usize> {
        if !matches!(self.length_size, 1 | 2) {
            return Err(Error::InvalidLayout);
        }
        Ok(1 + usize::from(self.length_offset) + usize::from(self.length_size))
    }

    fn body_length(self, packet_prefix: &[u8]) -> Result<usize> {
        let offset = 1 + usize::from(self.length_offset);
        match self.length_size {
            1 => Ok(usize::from(packet_prefix[offset])),
            2 => Ok(usize::from(u16::from_le_bytes([
                packet_prefix[offset],
                packet_prefix[offset + 1],
            ]))),
            _ => Err(Error::InvalidLayout),
        }
    }
}

fn standard_layout(packet_type: u8) -> Option<PacketLayout> {
    match packet_type {
        HCI_COMMAND_PACKET => Some(PacketLayout::new(1, 2)),
        HCI_ACL_DATA_PACKET => Some(PacketLayout::new(2, 2)),
        HCI_SYNCHRONOUS_DATA_PACKET => Some(PacketLayout::new(1, 2)),
        HCI_EVENT_PACKET => Some(PacketLayout::new(1, 1)),
        HCI_ISO_DATA_PACKET => Some(PacketLayout::new(2, 2)),
        _ => None,
    }
}

/// Incremental parser for H4 byte streams.
#[derive(Clone, Debug, Default)]
pub struct PacketFramer {
    buffer: Vec<u8>,
    extended_layouts: BTreeMap<u8, PacketLayout>,
}

impl PacketFramer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register framing information for a vendor-defined packet type.
    pub fn register_layout(&mut self, packet_type: u8, layout: PacketLayout) -> Result<()> {
        layout.header_size()?;
        if standard_layout(packet_type).is_some() {
            return Err(Error::InvalidLayout);
        }
        self.extended_layouts.insert(packet_type, layout);
        Ok(())
    }

    pub fn unregister_layout(&mut self, packet_type: u8) -> Option<PacketLayout> {
        self.extended_layouts.remove(&packet_type)
    }

    /// Feed any number of bytes and return all newly completed packets.
    pub fn feed(&mut self, data: &[u8]) -> Result<Vec<HciPacket>> {
        self.buffer.extend_from_slice(data);
        let mut packets = Vec::new();

        loop {
            let Some(&packet_type) = self.buffer.first() else {
                break;
            };
            let layout = standard_layout(packet_type)
                .or_else(|| self.extended_layouts.get(&packet_type).copied())
                .ok_or_else(|| {
                    self.buffer.clear();
                    Error::InvalidPacketType(packet_type)
                })?;
            let header_size = layout.header_size()?;
            if self.buffer.len() < header_size {
                break;
            }
            let packet_size = header_size
                .checked_add(layout.body_length(&self.buffer)?)
                .ok_or(Error::PacketTooLarge(usize::MAX))?;
            if packet_size > MAX_HCI_PACKET_SIZE {
                self.buffer.clear();
                return Err(Error::PacketTooLarge(packet_size));
            }
            if self.buffer.len() < packet_size {
                break;
            }
            let bytes: Vec<u8> = self.buffer.drain(..packet_size).collect();
            packets.push(HciPacket::from_bytes(&bytes)?);
        }

        Ok(packets)
    }

    pub fn buffered_len(&self) -> usize {
        self.buffer.len()
    }

    pub fn is_empty(&self) -> bool {
        self.buffer.is_empty()
    }

    pub fn clear(&mut self) {
        self.buffer.clear();
    }
}

pub trait PacketSource {
    /// Read the next packet, or `None` after a clean end of stream.
    fn read_packet(&mut self) -> Result<Option<HciPacket>>;
}

pub trait PacketSink {
    fn write_packet(&mut self, packet: &HciPacket) -> Result<()>;

    fn flush(&mut self) -> Result<()> {
        Ok(())
    }
}

/// A bidirectional H4 transport over any blocking byte stream.
#[derive(Debug)]
pub struct H4Transport<T> {
    io: T,
    framer: PacketFramer,
    pending: VecDeque<HciPacket>,
}

impl<T> H4Transport<T> {
    pub fn new(io: T) -> Self {
        Self {
            io,
            framer: PacketFramer::new(),
            pending: VecDeque::new(),
        }
    }

    pub fn framer_mut(&mut self) -> &mut PacketFramer {
        &mut self.framer
    }

    pub fn get_ref(&self) -> &T {
        &self.io
    }

    pub fn get_mut(&mut self) -> &mut T {
        &mut self.io
    }

    pub fn into_inner(self) -> T {
        self.io
    }
}

impl<T: Read> PacketSource for H4Transport<T> {
    fn read_packet(&mut self) -> Result<Option<HciPacket>> {
        if let Some(packet) = self.pending.pop_front() {
            return Ok(Some(packet));
        }

        let mut bytes = [0u8; 4096];
        loop {
            let count = self.io.read(&mut bytes)?;
            if count == 0 {
                return if self.framer.is_empty() {
                    Ok(None)
                } else {
                    Err(Error::TruncatedPacket(self.framer.buffered_len()))
                };
            }
            self.pending.extend(self.framer.feed(&bytes[..count])?);
            if let Some(packet) = self.pending.pop_front() {
                return Ok(Some(packet));
            }
        }
    }
}

impl<T: Write> PacketSink for H4Transport<T> {
    fn write_packet(&mut self, packet: &HciPacket) -> Result<()> {
        self.io.write_all(&packet.to_bytes())?;
        Ok(())
    }

    fn flush(&mut self) -> Result<()> {
        self.io.flush()?;
        Ok(())
    }
}
