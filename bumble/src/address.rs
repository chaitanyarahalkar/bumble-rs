//! Bluetooth device addresses (Vol 6, Part B - 1.3).
//!
//! Ported from `bumble.hci.Address`. Address bytes are stored little-endian:
//! `bytes[0]` is the LSB, `bytes[5]` is the MSB. Strings are big-endian
//! (`"C4:F2:17:1A:1D:BB"`).

use crate::{hex_decode, Error, Result};

/// The type qualifier attached to an address. Open enum (newtype over `u8`) so
/// values outside the named set round-trip unchanged.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AddressType(pub u8);

impl AddressType {
    pub const PUBLIC_DEVICE: AddressType = AddressType(0x00);
    pub const RANDOM_DEVICE: AddressType = AddressType(0x01);
    pub const PUBLIC_IDENTITY: AddressType = AddressType(0x02);
    pub const RANDOM_IDENTITY: AddressType = AddressType(0x03);
    pub const UNABLE_TO_RESOLVE: AddressType = AddressType(0xFE);
    pub const ANONYMOUS: AddressType = AddressType(0xFF);
}

/// A 48-bit Bluetooth address plus its type qualifier.
#[derive(Clone, Debug)]
pub struct Address {
    /// Little-endian: `bytes[0]` = LSB, `bytes[5]` = MSB.
    bytes: [u8; 6],
    address_type: AddressType,
}

impl Address {
    /// Build from little-endian bytes.
    pub fn from_bytes(bytes: [u8; 6], address_type: AddressType) -> Address {
        Address {
            bytes,
            address_type,
        }
    }

    /// Parse from a big-endian hex string, with optional `:` separators.
    ///
    /// A trailing `/P` forces [`AddressType::PUBLIC_DEVICE`] and overrides the
    /// `address_type` argument (matching Bumble).
    pub fn parse(address: &str, mut address_type: AddressType) -> Result<Address> {
        let mut s = address.to_string();

        // '/P' suffix → public device address (Bumble strips the last 2 chars).
        if s.ends_with('P') {
            address_type = AddressType::PUBLIC_DEVICE;
            let new_len = s.len().saturating_sub(2);
            s.truncate(new_len);
        }

        // Form with ':' separators (e.g. "00:11:22:33:44:55" → 17 chars).
        if s.len() == 17 {
            s = s.replace(':', "");
        }

        let be = hex_decode(&s)?;
        if be.len() != 6 {
            return Err(Error::InvalidArgument("invalid address length".into()));
        }

        // Big-endian string → little-endian storage.
        let mut bytes = [0u8; 6];
        for (i, b) in be.iter().rev().enumerate() {
            bytes[i] = *b;
        }

        Ok(Address {
            bytes,
            address_type,
        })
    }

    /// The address bytes, little-endian.
    pub fn address_bytes(&self) -> &[u8; 6] {
        &self.bytes
    }

    /// The address type qualifier.
    pub fn address_type(&self) -> AddressType {
        self.address_type
    }

    /// `true` if this is a public address (device or identity).
    pub fn is_public(&self) -> bool {
        self.address_type == AddressType::PUBLIC_DEVICE
            || self.address_type == AddressType::PUBLIC_IDENTITY
    }

    /// `true` if this is a random address (i.e. not public).
    pub fn is_random(&self) -> bool {
        !self.is_public()
    }

    /// `true` if this address has been resolved to an identity address.
    pub fn is_resolved(&self) -> bool {
        self.address_type == AddressType::PUBLIC_IDENTITY
            || self.address_type == AddressType::RANDOM_IDENTITY
    }

    /// `true` if this is a Resolvable Private Address (top two MSB bits `0b01`).
    pub fn is_resolvable(&self) -> bool {
        self.address_type == AddressType::RANDOM_DEVICE && (self.bytes[5] >> 6 == 1)
    }

    /// `true` if this is a Random Static Address (top two MSB bits `0b11`).
    pub fn is_static(&self) -> bool {
        self.is_random() && (self.bytes[5] >> 6 == 3)
    }

    /// Big-endian `AA:BB:...` string. Appends `/P` for public addresses when
    /// `with_type_qualifier` is set (matching Bumble).
    pub fn to_string(&self, with_type_qualifier: bool) -> String {
        let mut be = self.bytes;
        be.reverse();
        let hex: Vec<String> = be.iter().map(|b| format!("{b:02X}")).collect();
        let result = hex.join(":");
        if with_type_qualifier && self.is_public() {
            format!("{result}/P")
        } else {
            result
        }
    }
}

impl core::fmt::Display for Address {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.to_string(true))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Ported from bumble tests/hci_test.py::test_address
    #[test]
    fn test_address() {
        let a = Address::parse("C4:F2:17:1A:1D:BB", AddressType::RANDOM_DEVICE).unwrap();
        assert!(!a.is_public());
        assert!(a.is_random());
        assert_eq!(a.address_type(), AddressType::RANDOM_DEVICE);
        assert!(!a.is_resolvable());
        assert!(!a.is_resolved());
        assert!(a.is_static());
    }

    #[test]
    fn parse_public_suffix() {
        let a = Address::parse("00:11:22:33:44:55/P", AddressType::RANDOM_DEVICE).unwrap();
        assert_eq!(a.address_type(), AddressType::PUBLIC_DEVICE);
        assert!(a.is_public());
        // Little-endian storage: reversed of 00 11 22 33 44 55.
        assert_eq!(a.address_bytes(), &[0x55, 0x44, 0x33, 0x22, 0x11, 0x00]);
        assert_eq!(a.to_string(true), "00:11:22:33:44:55/P");
    }
}
