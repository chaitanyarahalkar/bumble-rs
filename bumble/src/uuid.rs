//! Bluetooth UUIDs (Vol 3, Part B - 2.5.1).
//!
//! Ported from `bumble.core.UUID`. Bytes are stored **little-endian**
//! throughout, matching Bumble; strings are big-endian. Equality and hashing
//! operate on the 128-bit expansion, so a 16-bit UUID compares equal to its
//! 128-bit form.

use crate::{hex_decode, hex_upper, Error, Result};
use std::hash::{Hash, Hasher};

/// The Bluetooth Base UUID (`0000xxxx-0000-1000-8000-00805F9B34FB`) stored
/// little-endian — i.e. the 12 high-order bytes that precede a 16/32-bit
/// short UUID's bytes within the 128-bit expansion.
const BASE_UUID_LE: [u8; 12] = [
    0xFB, 0x34, 0x9B, 0x5F, 0x80, 0x00, 0x00, 0x80, 0x00, 0x10, 0x00, 0x00,
];

/// A Bluetooth UUID: 2, 4, or 16 bytes, stored little-endian.
#[derive(Clone, Debug)]
pub struct Uuid {
    bytes: Vec<u8>,
}

impl Uuid {
    /// Build a 16-bit UUID.
    pub fn from_16_bits(value: u16) -> Uuid {
        Uuid {
            bytes: value.to_le_bytes().to_vec(),
        }
    }

    /// Build a 32-bit UUID.
    pub fn from_32_bits(value: u32) -> Uuid {
        Uuid {
            bytes: value.to_le_bytes().to_vec(),
        }
    }

    /// Build a UUID from raw little-endian bytes. Length must be 2, 4, or 16.
    pub fn from_bytes(bytes: &[u8]) -> Result<Uuid> {
        match bytes.len() {
            2 | 4 | 16 => Ok(Uuid {
                bytes: bytes.to_vec(),
            }),
            _ => Err(Error::InvalidArgument(
                "only 2, 4 and 16 bytes are allowed".into(),
            )),
        }
    }

    /// Parse a UUID from a big-endian hex string.
    ///
    /// Accepts:
    /// - 4 hex chars → 16-bit UUID (e.g. `"b5ea"`)
    /// - 8 hex chars → 32-bit UUID (e.g. `"df5ce654"`)
    /// - 32 hex chars → 128-bit UUID
    /// - 36-char dashed form (`8-4-4-4-12`)
    pub fn parse(s: &str) -> Result<Uuid> {
        let hex = if s.len() == 36 {
            let b = s.as_bytes();
            if b[8] != b'-' || b[13] != b'-' || b[18] != b'-' || b[23] != b'-' {
                return Err(Error::InvalidArgument("invalid UUID format".into()));
            }
            s.replace('-', "")
        } else {
            s.to_string()
        };

        if hex.len() != 32 && hex.len() != 8 && hex.len() != 4 {
            return Err(Error::InvalidArgument(format!(
                "invalid UUID format: {hex}"
            )));
        }

        // Big-endian hex → little-endian storage.
        let mut be = hex_decode(&hex)?;
        be.reverse();
        Ok(Uuid { bytes: be })
    }

    /// The 128-bit expansion of this UUID, little-endian.
    pub fn uuid_128_bytes(&self) -> [u8; 16] {
        let mut out = [0u8; 16];
        match self.bytes.len() {
            2 => {
                out[..12].copy_from_slice(&BASE_UUID_LE);
                out[12..14].copy_from_slice(&self.bytes);
                // out[14..16] left as 0
            }
            4 => {
                out[..12].copy_from_slice(&BASE_UUID_LE);
                out[12..16].copy_from_slice(&self.bytes);
            }
            16 => out.copy_from_slice(&self.bytes),
            _ => unreachable!("Uuid always holds 2/4/16 bytes"),
        }
        out
    }

    /// Serialize little-endian. When `force_128` is set, returns the 16-byte
    /// expansion regardless of the stored width.
    pub fn to_bytes(&self, force_128: bool) -> Vec<u8> {
        if force_128 {
            self.uuid_128_bytes().to_vec()
        } else {
            self.bytes.clone()
        }
    }

    /// Big-endian uppercase hex string. For 128-bit UUIDs, `separator` is
    /// inserted between the canonical `8-4-4-4-12` groups.
    pub fn to_hex_str(&self, separator: &str) -> String {
        match self.bytes.len() {
            2 | 4 => {
                let mut be = self.bytes.clone();
                be.reverse();
                hex_upper(&be)
            }
            16 => {
                let group = |range: std::ops::Range<usize>| {
                    let mut g = self.bytes[range].to_vec();
                    g.reverse();
                    // lowercase here; whole string is uppercased below (matches Python)
                    g.iter().map(|b| format!("{b:02x}")).collect::<String>()
                };
                let joined = [
                    group(12..16),
                    group(10..12),
                    group(8..10),
                    group(6..8),
                    group(0..6),
                ]
                .join(separator);
                joined.to_uppercase()
            }
            _ => unreachable!("Uuid always holds 2/4/16 bytes"),
        }
    }
}

impl PartialEq for Uuid {
    fn eq(&self, other: &Self) -> bool {
        self.uuid_128_bytes() == other.uuid_128_bytes()
    }
}

impl Eq for Uuid {}

impl Hash for Uuid {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.uuid_128_bytes().hash(state);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    // Ported from bumble tests/core_test.py::test_uuid_to_hex_str
    #[test]
    fn test_uuid_to_hex_str() {
        assert_eq!(Uuid::parse("b5ea").unwrap().to_hex_str(""), "B5EA");
        assert_eq!(Uuid::parse("df5ce654").unwrap().to_hex_str(""), "DF5CE654");
        assert_eq!(
            Uuid::parse("df5ce654-e059-11ed-b5ea-0242ac120002")
                .unwrap()
                .to_hex_str(""),
            "DF5CE654E05911EDB5EA0242AC120002"
        );
        assert_eq!(Uuid::parse("b5ea").unwrap().to_hex_str("-"), "B5EA");
        assert_eq!(Uuid::parse("df5ce654").unwrap().to_hex_str("-"), "DF5CE654");
        assert_eq!(
            Uuid::parse("df5ce654-e059-11ed-b5ea-0242ac120002")
                .unwrap()
                .to_hex_str("-"),
            "DF5CE654-E059-11ED-B5EA-0242AC120002"
        );
    }

    // Ported from bumble tests/core_test.py::test_uuid_hash
    #[test]
    fn test_uuid_hash() {
        let uuid = Uuid::parse("1234").unwrap();
        let uuid_128 = Uuid::from_bytes(&uuid.to_bytes(true)).unwrap();

        let set: HashSet<Uuid> = [uuid_128.clone()].into_iter().collect();
        assert!(set.contains(&uuid));

        let set2: HashSet<Uuid> = [uuid.clone()].into_iter().collect();
        assert!(set2.contains(&uuid_128));

        assert_eq!(uuid, uuid_128);
    }
}
