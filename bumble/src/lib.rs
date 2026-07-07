//! bumble — a Rust port of the core Bluetooth primitives from
//! [`google/bumble`](https://github.com/google/bumble).
//!
//! This is **slice 1** of an incremental port: the shared types that every
//! higher layer (HCI, L2CAP, ATT/GATT, SMP) depends on. It is intentionally
//! self-contained — no async, no I/O, no hardware, std-only.
//!
//! Modules:
//! - [`uuid`] — Bluetooth UUIDs (16/32/128-bit).
//! - [`address`] — Bluetooth device addresses.
//! - [`appearance`] — GAP Appearance.
//! - [`class_of_device`] — Class of Device.
//! - [`advertising_data`] — Advertising Data (raw TLV).

pub mod address;
pub mod advertising_data;
pub mod appearance;
pub mod class_of_device;
pub mod company_ids;
pub mod data_types;
pub mod uuid;

pub use address::{Address, AddressType};
pub use advertising_data::AdvertisingData;
pub use appearance::{Appearance, Category};
pub use class_of_device::{ClassOfDevice, MajorDeviceClass, MajorServiceClasses};
pub use company_ids::company_name;
pub use data_types::DataType;
pub use uuid::Uuid;

use core::fmt;

/// Errors produced by this crate. Mirrors the subset of `bumble.core`
/// exceptions relevant to the ported types.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// An argument was malformed (e.g. bad UUID / address string).
    InvalidArgument(String),
    /// A serialized buffer could not be parsed.
    InvalidPacket(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::InvalidArgument(m) => write!(f, "invalid argument: {m}"),
            Error::InvalidPacket(m) => write!(f, "invalid packet: {m}"),
        }
    }
}

impl std::error::Error for Error {}

/// Crate-wide result alias.
pub type Result<T> = core::result::Result<T, Error>;

/// Return the first key in `map` whose value equals `value`.
///
/// Mirrors `bumble.core.get_dict_key_by_value`, modeled here over an
/// association list (a slice of `(key, value)` pairs) since Rust has no
/// dynamic `dict`.
pub fn get_dict_key_by_value<K: Clone, V: PartialEq>(map: &[(K, V)], value: &V) -> Option<K> {
    map.iter().find(|(_, v)| v == value).map(|(k, _)| k.clone())
}

/// Decode a big-endian hex string into bytes. Rejects odd-length or non-hex
/// input. Shared by [`uuid`] and [`address`] parsing.
pub(crate) fn hex_decode(s: &str) -> Result<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return Err(Error::InvalidArgument(format!("odd-length hex: {s:?}")));
    }
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(s.len() / 2);
    let mut i = 0;
    while i < bytes.len() {
        let hi = hex_nibble(bytes[i])?;
        let lo = hex_nibble(bytes[i + 1])?;
        out.push((hi << 4) | lo);
        i += 2;
    }
    Ok(out)
}

fn hex_nibble(c: u8) -> Result<u8> {
    match c {
        b'0'..=b'9' => Ok(c - b'0'),
        b'a'..=b'f' => Ok(c - b'a' + 10),
        b'A'..=b'F' => Ok(c - b'A' + 10),
        _ => Err(Error::InvalidArgument(format!(
            "invalid hex digit: {:?}",
            c as char
        ))),
    }
}

/// Uppercase hex encoding of `bytes` (no separator).
pub(crate) fn hex_upper(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{b:02X}"));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    // Ported from bumble tests/core_test.py::test_get_dict_key_by_value
    #[test]
    fn test_get_dict_key_by_value() {
        let dictionary = [("A", 1), ("B", 2)];
        assert_eq!(get_dict_key_by_value(&dictionary, &1), Some("A"));
        assert_eq!(get_dict_key_by_value(&dictionary, &2), Some("B"));
        assert_eq!(get_dict_key_by_value(&dictionary, &3), None);
    }

    #[test]
    fn hex_decode_roundtrip() {
        assert_eq!(hex_decode("00112233").unwrap(), vec![0, 0x11, 0x22, 0x33]);
        assert_eq!(hex_upper(&[0x0a, 0xbc]), "0ABC");
        assert!(hex_decode("0").is_err());
        assert!(hex_decode("zz").is_err());
    }
}
