//! bumble-sdp — a Rust port of the Service Discovery Protocol (SDP) codec from
//! [`google/bumble`](https://github.com/google/bumble).
//!
//! **Slice 16** of the incremental port, and the first piece of Classic
//! Bluetooth (BR/EDR) infrastructure: SDP is the protocol a classic device
//! uses to discover which services a peer offers and how to reach them, and
//! its self-describing [`DataElement`] encoding is the value format every
//! classic profile (RFCOMM/SPP, A2DP, AVRCP, HFP, HID, …) builds its service
//! records from. Depends on the `bumble` crate for [`bumble::Uuid`].
//!
//! ## Scope
//!
//! Implemented, byte-for-byte against upstream:
//!
//! - [`DataElement`] — the recursive type-length-value element format
//!   (Vol 3, Part B - 3.3): nil, unsigned/signed integers (1/2/4/8 bytes),
//!   UUIDs (16/32/128-bit), text strings, booleans, sequences, alternatives
//!   and URLs, including all eight size-index encodings.
//! - [`ServiceAttribute`] — the `(attribute-id, value)` pair a service record
//!   is built from, and the flat alternating-element list encoding.
//! - [`SdpPdu`] — the seven Protocol Data Units (Vol 3, Part B - 4.4–4.7):
//!   Error Response, Service Search Request/Response, Service Attribute
//!   Request/Response and Service Search Attribute Request/Response, with the
//!   common `[pdu-id, transaction-id, parameter-length, parameters…]` framing.
//!
//! Deferred (matching the rest of this port's synchronous, codec-first
//! approach): the asyncio `Client`/`Server` runtime, the continuation-state
//! reassembly loop, and the higher-level service-record database. The
//! continuation-state bytes are carried verbatim on every PDU, so a caller can
//! still drive multi-packet exchanges by hand.
//!
//! ## Oracle
//!
//! Every serialization in the tests is pinned to a hex literal captured from
//! upstream Python Bumble at commit
//! `1d26b99865f96a3e7359009424c0ddf2934acd0b`. The capture imported
//! `bumble.sdp` through two inert shims (`typing_extensions` — type-only
//! `Self`/`override`; `pyee` — event-emitter infrastructure); neither touches
//! the serialization path, so the byte oracle is upstream's own
//! `bytes(data_element)` / `bytes(pdu)` output. Unknown element **type** codes
//! (outside the nine the spec defines) are rejected on parse rather than
//! preserved — SDP closes the type space, unlike the open op-code spaces
//! elsewhere in the port.

use bumble::Uuid;
use core::fmt;

/// SDP PDU identifiers (Vol 3, Part B - 4.2).
pub mod pdu_id {
    pub const SDP_ERROR_RESPONSE: u8 = 0x01;
    pub const SDP_SERVICE_SEARCH_REQUEST: u8 = 0x02;
    pub const SDP_SERVICE_SEARCH_RESPONSE: u8 = 0x03;
    pub const SDP_SERVICE_ATTRIBUTE_REQUEST: u8 = 0x04;
    pub const SDP_SERVICE_ATTRIBUTE_RESPONSE: u8 = 0x05;
    pub const SDP_SERVICE_SEARCH_ATTRIBUTE_REQUEST: u8 = 0x06;
    pub const SDP_SERVICE_SEARCH_ATTRIBUTE_RESPONSE: u8 = 0x07;
}

/// SDP error codes carried by [`SdpPdu::ErrorResponse`] (Vol 3, Part B - 4.4.1).
pub mod error_code {
    pub const INVALID_SDP_VERSION: u16 = 0x0001;
    pub const INVALID_SERVICE_RECORD_HANDLE: u16 = 0x0002;
    pub const INVALID_REQUEST_SYNTAX: u16 = 0x0003;
    pub const INVALID_PDU_SIZE: u16 = 0x0004;
    pub const INVALID_CONTINUATION_STATE: u16 = 0x0005;
    pub const INSUFFICIENT_RESOURCES_TO_SATISFY_REQUEST: u16 = 0x0006;
}

/// The SDP Protocol/Service Multiplexer (the L2CAP PSM SDP runs over).
pub const SDP_PSM: u16 = 0x0001;

/// The well-known Public Browse Root service class (`0x1002`), the group a
/// device advertises browsable services under.
pub fn public_browse_root() -> Uuid {
    Uuid::from_16_bits(0x1002)
}

/// Errors produced while parsing SDP data elements or PDUs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// The bytes are malformed, truncated, or use an unsupported encoding.
    InvalidPacket(String),
    /// A value cannot be serialized under the requested size (e.g. an integer
    /// too large for its `value_size`, or a container above the 4-byte length
    /// limit).
    InvalidArgument(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::InvalidPacket(m) => write!(f, "invalid packet: {m}"),
            Error::InvalidArgument(m) => write!(f, "invalid argument: {m}"),
        }
    }
}

impl std::error::Error for Error {}

pub type Result<T> = core::result::Result<T, Error>;

/// Build an `InvalidPacket` error for a truncated field.
pub(crate) fn truncated(what: &str) -> Error {
    Error::InvalidPacket(format!("truncated: expected more bytes for {what}"))
}

/// The maximum data-element nesting depth accepted while parsing, matching
/// upstream's `_MAX_DATA_ELEMENT_NESTING`.
pub const MAX_NESTING: usize = 32;

mod type_code {
    pub const NIL: u8 = 0;
    pub const UNSIGNED_INTEGER: u8 = 1;
    pub const SIGNED_INTEGER: u8 = 2;
    pub const UUID: u8 = 3;
    pub const TEXT_STRING: u8 = 4;
    pub const BOOLEAN: u8 = 5;
    pub const SEQUENCE: u8 = 6;
    pub const ALTERNATIVE: u8 = 7;
    pub const URL: u8 = 8;
}

/// A single SDP data element (Vol 3, Part B - 3.3): a self-describing
/// type-length-value node. Sequences and alternatives nest other elements, so
/// a whole service record is one `DataElement`.
///
/// Integers carry an explicit `size` (in bytes) because it is part of the wire
/// encoding and must round-trip exactly — e.g. the value `0x0000_FFFF` stored
/// with `size == 4` re-serializes to four bytes, not two.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DataElement {
    /// The nil element (no value).
    Nil,
    /// An unsigned integer of `size` bytes (`size` ∈ {1, 2, 4, 8}).
    UnsignedInteger { value: u64, size: u8 },
    /// A signed integer of `size` bytes (`size` ∈ {1, 2, 4, 8}).
    SignedInteger { value: i64, size: u8 },
    /// A 16-, 32- or 128-bit UUID.
    Uuid(Uuid),
    /// A text string (raw bytes; not required to be valid UTF-8).
    TextString(Vec<u8>),
    /// A boolean.
    Boolean(bool),
    /// An ordered sequence of elements.
    Sequence(Vec<DataElement>),
    /// A set of alternative elements.
    Alternative(Vec<DataElement>),
    /// A URL.
    Url(String),
}

impl DataElement {
    /// The nil element.
    pub fn nil() -> Self {
        DataElement::Nil
    }

    /// An unsigned integer with an explicit byte size (1, 2, 4 or 8).
    pub fn unsigned_integer(value: u64, size: u8) -> Self {
        DataElement::UnsignedInteger { value, size }
    }

    /// An 8-bit unsigned integer.
    pub fn unsigned_integer_8(value: u8) -> Self {
        DataElement::UnsignedInteger {
            value: value as u64,
            size: 1,
        }
    }

    /// A 16-bit unsigned integer.
    pub fn unsigned_integer_16(value: u16) -> Self {
        DataElement::UnsignedInteger {
            value: value as u64,
            size: 2,
        }
    }

    /// A 32-bit unsigned integer.
    pub fn unsigned_integer_32(value: u32) -> Self {
        DataElement::UnsignedInteger {
            value: value as u64,
            size: 4,
        }
    }

    /// A signed integer with an explicit byte size (1, 2, 4 or 8).
    pub fn signed_integer(value: i64, size: u8) -> Self {
        DataElement::SignedInteger { value, size }
    }

    /// An 8-bit signed integer.
    pub fn signed_integer_8(value: i8) -> Self {
        DataElement::SignedInteger {
            value: value as i64,
            size: 1,
        }
    }

    /// A 16-bit signed integer.
    pub fn signed_integer_16(value: i16) -> Self {
        DataElement::SignedInteger {
            value: value as i64,
            size: 2,
        }
    }

    /// A 32-bit signed integer.
    pub fn signed_integer_32(value: i32) -> Self {
        DataElement::SignedInteger {
            value: value as i64,
            size: 4,
        }
    }

    /// A UUID element.
    pub fn uuid(value: Uuid) -> Self {
        DataElement::Uuid(value)
    }

    /// A text-string element.
    pub fn text_string(value: impl Into<Vec<u8>>) -> Self {
        DataElement::TextString(value.into())
    }

    /// A boolean element.
    pub fn boolean(value: bool) -> Self {
        DataElement::Boolean(value)
    }

    /// A sequence element.
    pub fn sequence(elements: impl IntoIterator<Item = DataElement>) -> Self {
        DataElement::Sequence(elements.into_iter().collect())
    }

    /// An alternative element.
    pub fn alternative(elements: impl IntoIterator<Item = DataElement>) -> Self {
        DataElement::Alternative(elements.into_iter().collect())
    }

    /// A URL element.
    pub fn url(value: impl Into<String>) -> Self {
        DataElement::Url(value.into())
    }

    /// The element's type code (the top five bits of the header byte).
    pub fn type_code(&self) -> u8 {
        match self {
            DataElement::Nil => type_code::NIL,
            DataElement::UnsignedInteger { .. } => type_code::UNSIGNED_INTEGER,
            DataElement::SignedInteger { .. } => type_code::SIGNED_INTEGER,
            DataElement::Uuid(_) => type_code::UUID,
            DataElement::TextString(_) => type_code::TEXT_STRING,
            DataElement::Boolean(_) => type_code::BOOLEAN,
            DataElement::Sequence(_) => type_code::SEQUENCE,
            DataElement::Alternative(_) => type_code::ALTERNATIVE,
            DataElement::Url(_) => type_code::URL,
        }
    }

    /// Serialize this element (and, recursively, any children) to its SDP wire
    /// bytes.
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        // 1. Compute the value bytes (the `data` in upstream's terms).
        let data = self.value_bytes()?;
        let size = data.len();
        let type_code = self.type_code();

        // 2. Choose the size index and any explicit length bytes.
        let (size_index, size_bytes) = match self {
            DataElement::Nil => (0u8, Vec::new()),
            DataElement::Boolean(_) => (0, Vec::new()),
            DataElement::UnsignedInteger { .. }
            | DataElement::SignedInteger { .. }
            | DataElement::Uuid(_) => (fixed_size_index(size)?, Vec::new()),
            DataElement::TextString(_)
            | DataElement::Sequence(_)
            | DataElement::Alternative(_)
            | DataElement::Url(_) => variable_size_header(size)?,
        };

        // 3. header = type_code << 3 | size_index, then size bytes, then data.
        let mut out = Vec::with_capacity(1 + size_bytes.len() + size);
        out.push((type_code << 3) | size_index);
        out.extend_from_slice(&size_bytes);
        out.extend_from_slice(&data);
        Ok(out)
    }

    /// The `data` portion (value bytes) for this element, without the header.
    fn value_bytes(&self) -> Result<Vec<u8>> {
        Ok(match self {
            DataElement::Nil => Vec::new(),
            DataElement::UnsignedInteger { value, size } => unsigned_to_bytes(*value, *size)?,
            DataElement::SignedInteger { value, size } => signed_to_bytes(*value, *size)?,
            DataElement::Uuid(uuid) => {
                // Upstream: bytes(uuid)[::-1] — little-endian storage reversed
                // to big-endian on the wire.
                let mut be = uuid.to_bytes(false);
                be.reverse();
                be
            }
            DataElement::TextString(bytes) => bytes.clone(),
            DataElement::Boolean(b) => vec![u8::from(*b)],
            DataElement::Sequence(elements) | DataElement::Alternative(elements) => {
                let mut data = Vec::new();
                for element in elements {
                    data.extend_from_slice(&element.to_bytes()?);
                }
                data
            }
            DataElement::Url(url) => url.as_bytes().to_vec(),
        })
    }

    /// Parse a single data element beginning at `offset`, returning the offset
    /// just past it and the element.
    pub fn parse_from_bytes(data: &[u8], offset: usize) -> Result<(usize, DataElement)> {
        Parser::new(data).parse_at(offset, 0)
    }

    /// Parse a single data element from the front of `data` (any trailing bytes
    /// are ignored, matching upstream's `DataElement.from_bytes`).
    pub fn from_bytes(data: &[u8]) -> Result<DataElement> {
        Ok(Self::parse_from_bytes(data, 0)?.1)
    }
}

/// The size index for a fixed-width element (integer or UUID), from the number
/// of value bytes. Mirrors upstream's `size <= 1 → 0, 2 → 1, 4 → 2, 8 → 3,
/// 16 → 4`.
fn fixed_size_index(size: usize) -> Result<u8> {
    Ok(match size {
        0 | 1 => 0,
        2 => 1,
        4 => 2,
        8 => 3,
        16 => 4,
        _ => return Err(Error::InvalidArgument(format!("invalid data size {size}"))),
    })
}

/// The `(size_index, size_bytes)` header for a variable-length element
/// (text/sequence/alternative/URL).
fn variable_size_header(size: usize) -> Result<(u8, Vec<u8>)> {
    Ok(if size <= 0xFF {
        (5, vec![size as u8])
    } else if size <= 0xFFFF {
        (6, (size as u16).to_be_bytes().to_vec())
    } else if size <= 0xFFFF_FFFF {
        (7, (size as u32).to_be_bytes().to_vec())
    } else {
        return Err(Error::InvalidArgument(format!("invalid data size {size}")));
    })
}

/// Encode an unsigned integer big-endian in exactly `size` bytes, erroring if
/// the value does not fit or `size` is not 1/2/4/8.
fn unsigned_to_bytes(value: u64, size: u8) -> Result<Vec<u8>> {
    match size {
        1 | 2 | 4 | 8 => {}
        other => {
            return Err(Error::InvalidArgument(format!(
                "invalid value_size {other}"
            )))
        }
    }
    let all = value.to_be_bytes(); // 8 bytes, big-endian
    let start = 8 - size as usize;
    if all[..start].iter().any(|&b| b != 0) {
        return Err(Error::InvalidArgument(format!(
            "value {value} does not fit in {size} bytes"
        )));
    }
    Ok(all[start..].to_vec())
}

/// Encode a signed integer big-endian (two's complement) in exactly `size`
/// bytes, erroring if the value is out of range or `size` is not 1/2/4/8.
fn signed_to_bytes(value: i64, size: u8) -> Result<Vec<u8>> {
    let bits = match size {
        1 | 2 | 4 | 8 => size as u32 * 8,
        other => {
            return Err(Error::InvalidArgument(format!(
                "invalid value_size {other}"
            )))
        }
    };
    if bits < 64 {
        let min = -(1i64 << (bits - 1));
        let max = (1i64 << (bits - 1)) - 1;
        if value < min || value > max {
            return Err(Error::InvalidArgument(format!(
                "value {value} does not fit in {size} bytes"
            )));
        }
    }
    let all = value.to_be_bytes(); // 8 bytes, big-endian two's complement
    Ok(all[8 - size as usize..].to_vec())
}

/// Recursive-descent parser for data elements, tracking nesting depth.
struct Parser<'a> {
    data: &'a [u8],
}

impl<'a> Parser<'a> {
    fn new(data: &'a [u8]) -> Self {
        Parser { data }
    }

    fn parse_at(&self, offset: usize, depth: usize) -> Result<(usize, DataElement)> {
        if offset >= self.data.len() {
            return Err(Error::InvalidPacket(format!(
                "offset {offset} exceeds len {}",
                self.data.len()
            )));
        }
        let header = self.data[offset];
        let element_type = header >> 3;
        let size_index = header & 7;
        let mut pos = offset + 1;

        // Decode the value size from the size index.
        let value_size = match size_index {
            0 => {
                if element_type == type_code::NIL {
                    0
                } else {
                    1
                }
            }
            1 => 2,
            2 => 4,
            3 => 8,
            4 => 16,
            5 => {
                let s = *self
                    .data
                    .get(pos)
                    .ok_or_else(|| Error::InvalidPacket("truncated 1-byte size".into()))?
                    as usize;
                pos += 1;
                s
            }
            6 => {
                let s = read_be_u16(self.data, pos)? as usize;
                pos += 2;
                s
            }
            7 => {
                let s = read_be_u32(self.data, pos)? as usize;
                pos += 4;
                s
            }
            _ => unreachable!("size_index is masked to 0..=7"),
        };

        let value_start = pos;
        let value_end = value_start
            .checked_add(value_size)
            .filter(|&e| e <= self.data.len())
            .ok_or_else(|| {
                Error::InvalidPacket(format!(
                    "element value ({value_size} bytes at {value_start}) runs past end"
                ))
            })?;
        let value = &self.data[value_start..value_end];

        let element = match element_type {
            type_code::NIL => DataElement::Nil,
            type_code::UNSIGNED_INTEGER => DataElement::UnsignedInteger {
                value: be_to_u64(value),
                size: value_size as u8,
            },
            type_code::SIGNED_INTEGER => DataElement::SignedInteger {
                value: be_to_i64(value),
                size: value_size as u8,
            },
            type_code::UUID => {
                // Wire is big-endian; storage is little-endian.
                let mut le = value.to_vec();
                le.reverse();
                DataElement::Uuid(
                    Uuid::from_bytes(&le)
                        .map_err(|e| Error::InvalidPacket(format!("bad UUID: {e}")))?,
                )
            }
            type_code::TEXT_STRING => DataElement::TextString(value.to_vec()),
            type_code::BOOLEAN => DataElement::Boolean(value.first() == Some(&1)),
            type_code::SEQUENCE | type_code::ALTERNATIVE => {
                if depth >= MAX_NESTING {
                    return Err(Error::InvalidPacket(format!(
                        "nesting exceeds max depth ({MAX_NESTING})"
                    )));
                }
                let mut elements = Vec::new();
                let mut inner = value_start;
                while inner < value_end {
                    let (next, child) = self.parse_at(inner, depth + 1)?;
                    elements.push(child);
                    inner = next;
                }
                if inner != value_end {
                    return Err(Error::InvalidPacket(
                        "container elements overran their length".into(),
                    ));
                }
                if element_type == type_code::SEQUENCE {
                    DataElement::Sequence(elements)
                } else {
                    DataElement::Alternative(elements)
                }
            }
            type_code::URL => DataElement::Url(
                String::from_utf8(value.to_vec())
                    .map_err(|e| Error::InvalidPacket(format!("bad URL utf-8: {e}")))?,
            ),
            other => {
                return Err(Error::InvalidPacket(format!(
                    "unsupported data element type {other}"
                )))
            }
        };

        Ok((value_end, element))
    }
}

/// Decode a big-endian byte slice (0–8 bytes) as a `u64`.
fn be_to_u64(bytes: &[u8]) -> u64 {
    let mut acc = 0u64;
    for &b in bytes {
        acc = (acc << 8) | b as u64;
    }
    acc
}

/// Decode a big-endian two's-complement byte slice (0–8 bytes) as an `i64`.
fn be_to_i64(bytes: &[u8]) -> i64 {
    if bytes.is_empty() {
        return 0;
    }
    let negative = bytes[0] & 0x80 != 0;
    let mut acc: u64 = if negative { u64::MAX } else { 0 };
    for &b in bytes {
        acc = (acc << 8) | b as u64;
    }
    acc as i64
}

fn read_be_u16(data: &[u8], offset: usize) -> Result<u16> {
    data.get(offset..offset + 2)
        .map(|s| u16::from_be_bytes([s[0], s[1]]))
        .ok_or_else(|| Error::InvalidPacket("truncated 2-byte size".into()))
}

fn read_be_u32(data: &[u8], offset: usize) -> Result<u32> {
    data.get(offset..offset + 4)
        .map(|s| u32::from_be_bytes([s[0], s[1], s[2], s[3]]))
        .ok_or_else(|| Error::InvalidPacket("truncated 4-byte size".into()))
}

/// A `(attribute-id, value)` pair — the unit a service record is built from
/// (Vol 3, Part B - 2.2). A record is serialized as a [`DataElement::Sequence`]
/// of alternating unsigned-integer ids and their values; see
/// [`ServiceAttribute::list_to_data_element`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServiceAttribute {
    pub id: u16,
    pub value: DataElement,
}

impl ServiceAttribute {
    /// Create a new attribute.
    pub fn new(id: u16, value: DataElement) -> Self {
        ServiceAttribute { id, value }
    }

    /// Build the flat alternating `[id, value, id, value, …]` sequence element
    /// that encodes a service (attribute) record.
    pub fn list_to_data_element(attributes: &[ServiceAttribute]) -> DataElement {
        let mut elements = Vec::with_capacity(attributes.len() * 2);
        for attr in attributes {
            elements.push(DataElement::unsigned_integer_16(attr.id));
            elements.push(attr.value.clone());
        }
        DataElement::Sequence(elements)
    }

    /// Recover the attribute list from the flat alternating sequence produced by
    /// [`list_to_data_element`](Self::list_to_data_element). Pairs whose id is
    /// not an unsigned integer are skipped, matching upstream.
    pub fn list_from_data_elements(elements: &[DataElement]) -> Vec<ServiceAttribute> {
        let mut attributes = Vec::new();
        for pair in elements.chunks_exact(2) {
            if let DataElement::UnsignedInteger { value, .. } = &pair[0] {
                attributes.push(ServiceAttribute {
                    id: *value as u16,
                    value: pair[1].clone(),
                });
            }
        }
        attributes
    }

    /// Find the value of the attribute with `id` in a list, if present.
    pub fn find(attributes: &[ServiceAttribute], id: u16) -> Option<&DataElement> {
        attributes.iter().find(|a| a.id == id).map(|a| &a.value)
    }
}

mod pdu;
pub use pdu::SdpPdu;
