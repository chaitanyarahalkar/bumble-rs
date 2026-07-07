//! Pairing key material, ported from `bumble.keys`.
//!
//! This is the [`PairingKeys`] data structure — the keys retained for a bonded
//! peer. The persistent key stores (`JsonKeyStore`, `MemoryKeyStore`) are async
//! I/O infrastructure and are not ported here.

use crate::AddressType;

/// A single stored key and its associated metadata.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Key {
    pub value: Vec<u8>,
    pub authenticated: bool,
    /// Encrypted Diversifier (LE Legacy LTK distribution).
    pub ediv: Option<u16>,
    /// Random value (LE Legacy LTK distribution).
    pub rand: Option<Vec<u8>>,
}

impl Key {
    /// A plain (unauthenticated) key from raw bytes.
    pub fn new(value: Vec<u8>) -> Key {
        Key {
            value,
            ..Key::default()
        }
    }
}

/// The set of keys retained for a bonded peer (Vol 3, Part H - key distribution).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PairingKeys {
    pub address_type: Option<AddressType>,
    /// Long Term Key (LE Secure Connections, or legacy when symmetric).
    pub ltk: Option<Key>,
    /// Central's LTK (LE Legacy, asymmetric).
    pub ltk_central: Option<Key>,
    /// Peripheral's LTK (LE Legacy, asymmetric).
    pub ltk_peripheral: Option<Key>,
    /// Identity Resolving Key.
    pub irk: Option<Key>,
    /// Connection Signature Resolving Key.
    pub csrk: Option<Key>,
    /// Classic BR/EDR link key.
    pub link_key: Option<Key>,
    /// Classic link key type.
    pub link_key_type: Option<u8>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_and_compares() {
        let mut keys = PairingKeys::default();
        assert!(keys.ltk.is_none());
        keys.ltk = Some(Key {
            value: vec![0xAA; 16],
            authenticated: true,
            ediv: Some(0x1234),
            rand: Some(vec![0xBB; 8]),
        });
        keys.irk = Some(Key::new(vec![0xCC; 16]));
        assert_eq!(keys.ltk.as_ref().unwrap().value, vec![0xAA; 16]);
        assert!(keys.ltk.as_ref().unwrap().authenticated);
        assert!(!keys.irk.as_ref().unwrap().authenticated);
        assert_eq!(keys.clone(), keys);
    }
}
