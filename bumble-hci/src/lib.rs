//! bumble-hci — a Rust port of the HCI packet codec from
//! [`google/bumble`](https://github.com/google/bumble).
//!
//! **Slice 2** of the incremental port: the HCI framing layer plus a
//! representative subset of commands and events. It builds on the `bumble`
//! crate (slice 1) for the [`bumble::Address`] type.
//!
//! Every packet type indicator dispatches through [`HciPacket::from_bytes`];
//! [`HciPacket::to_bytes`] round-trips back to the wire form. Typed
//! commands/events are modeled as enum variants with a `Generic` fallback for
//! op/event codes this slice does not yet decode.
//!
//! ## Scope
//!
//! Ported: generic events; the Reset / Disconnect / Set_Event_Mask /
//! LE_Set_Event_Mask / LE_Set_Random_Address / LE_Set_Scan_Enable /
//! Read_Local_Version_Information / Read_Local_Supported_Commands /
//! Read_Local_Supported_Features commands; the Command_Status and
//! Number_Of_Completed_Packets events; the LE Connection_Complete /
//! Connection_Update_Complete / Channel_Selection_Algorithm /
//! Read_Remote_Features_Complete meta events; and the ACL / Synchronous / ISO
//! data packets and custom packets.
//!
//! Deferred to later slices: the full command/event catalog, Advertising
//! Report events, Command_Complete return-parameters, the vendor-event
//! factory, and complex multi-array commands.

pub mod codes;
pub mod command;
pub mod event;
pub mod packet;
pub mod return_parameters;

pub use bumble::{Address, AddressType};
pub use codes::*;
pub use command::{CodingFormat, Command};
pub use event::{AdvertisingReport, Event, ExtendedAdvertisingReport, LeMetaEvent};
pub use packet::{
    fragment_l2cap_pdu, AclDataPacket, AclDataPacketAssembler, CustomPacket, IsoDataPacket,
    SynchronousDataPacket, HCI_ACL_PB_CONTINUATION, HCI_ACL_PB_FIRST_FLUSHABLE,
    HCI_ACL_PB_FIRST_NON_FLUSHABLE,
};
pub use return_parameters::{map_null_terminated_utf8_string, ReturnParameters};

use core::fmt;

/// Errors produced while parsing HCI packets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// A packet or field could not be parsed (too short, bad length, …).
    InvalidPacket(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::InvalidPacket(m) => write!(f, "invalid packet: {m}"),
        }
    }
}

impl std::error::Error for Error {}

/// Crate result alias.
pub type Result<T> = core::result::Result<T, Error>;

/// A parsed HCI packet of any kind.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HciPacket {
    Command(Command),
    Event(Event),
    AclData(AclDataPacket),
    SyncData(SynchronousDataPacket),
    IsoData(IsoDataPacket),
    Custom(CustomPacket),
}

impl HciPacket {
    /// Parse a complete HCI packet, dispatching on the leading type byte.
    /// Unknown type bytes yield a [`CustomPacket`] (matching Bumble).
    pub fn from_bytes(packet: &[u8]) -> Result<HciPacket> {
        let packet_type = *packet
            .first()
            .ok_or_else(|| Error::InvalidPacket("empty packet".into()))?;
        Ok(match packet_type {
            codes::HCI_COMMAND_PACKET => HciPacket::Command(Command::from_bytes(packet)?),
            codes::HCI_ACL_DATA_PACKET => HciPacket::AclData(AclDataPacket::from_bytes(packet)?),
            codes::HCI_SYNCHRONOUS_DATA_PACKET => {
                HciPacket::SyncData(SynchronousDataPacket::from_bytes(packet)?)
            }
            codes::HCI_EVENT_PACKET => HciPacket::Event(Event::from_bytes(packet)?),
            codes::HCI_ISO_DATA_PACKET => HciPacket::IsoData(IsoDataPacket::from_bytes(packet)?),
            _ => HciPacket::Custom(CustomPacket::new(packet.to_vec())),
        })
    }

    /// Serialize the packet to its wire form.
    pub fn to_bytes(&self) -> Vec<u8> {
        match self {
            HciPacket::Command(c) => c.to_bytes(),
            HciPacket::Event(e) => e.to_bytes(),
            HciPacket::AclData(p) => p.to_bytes(),
            HciPacket::SyncData(p) => p.to_bytes(),
            HciPacket::IsoData(p) => p.to_bytes(),
            HciPacket::Custom(p) => p.to_bytes(),
        }
    }
}

/// A little cursor over a byte slice with bounds-checked little-endian reads.
pub(crate) struct Reader<'a> {
    data: &'a [u8],
    pub pos: usize,
}

impl<'a> Reader<'a> {
    pub fn new(data: &'a [u8], pos: usize) -> Self {
        Reader { data, pos }
    }

    fn need(&self, n: usize) -> Result<()> {
        if self.pos + n > self.data.len() {
            Err(Error::InvalidPacket(format!(
                "unexpected end of data: need {n} at offset {}, have {}",
                self.pos,
                self.data.len()
            )))
        } else {
            Ok(())
        }
    }

    pub fn u8(&mut self) -> Result<u8> {
        self.need(1)?;
        let v = self.data[self.pos];
        self.pos += 1;
        Ok(v)
    }

    pub fn u16_le(&mut self) -> Result<u16> {
        self.need(2)?;
        let v = u16::from_le_bytes([self.data[self.pos], self.data[self.pos + 1]]);
        self.pos += 2;
        Ok(v)
    }

    /// Read a 3-byte little-endian integer into a `u32`.
    pub fn u24_le(&mut self) -> Result<u32> {
        self.need(3)?;
        let v = u32::from_le_bytes([
            self.data[self.pos],
            self.data[self.pos + 1],
            self.data[self.pos + 2],
            0,
        ]);
        self.pos += 3;
        Ok(v)
    }

    pub fn u32_le(&mut self) -> Result<u32> {
        self.need(4)?;
        let v = u32::from_le_bytes([
            self.data[self.pos],
            self.data[self.pos + 1],
            self.data[self.pos + 2],
            self.data[self.pos + 3],
        ]);
        self.pos += 4;
        Ok(v)
    }

    pub fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        self.need(n)?;
        let slice = &self.data[self.pos..self.pos + n];
        self.pos += n;
        Ok(slice)
    }

    /// Take exactly `N` bytes as a fixed array.
    pub fn array<const N: usize>(&mut self) -> Result<[u8; N]> {
        let slice = self.take(N)?;
        let mut out = [0u8; N];
        out.copy_from_slice(slice);
        Ok(out)
    }

    pub fn rest(&mut self) -> &'a [u8] {
        let slice = &self.data[self.pos..];
        self.pos = self.data.len();
        slice
    }
}
