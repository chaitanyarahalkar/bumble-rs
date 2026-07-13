//! Host-side LE resolvable-private-address support.

use bumble::{Address, AddressType};
use bumble_crypto::{ah, random_128};

#[derive(Clone, Debug)]
pub struct AddressResolver {
    resolving_keys: Vec<([u8; 16], Address)>,
}

impl AddressResolver {
    pub fn new(resolving_keys: impl IntoIterator<Item = (Vec<u8>, Address)>) -> Self {
        Self {
            resolving_keys: resolving_keys
                .into_iter()
                .filter_map(|(irk, address)| irk.try_into().ok().map(|irk| (irk, address)))
                .collect(),
        }
    }

    pub fn can_resolve_to(&self, address: &Address) -> bool {
        self.resolving_keys
            .iter()
            .any(|(_, candidate)| candidate == address)
    }

    pub fn resolve(&self, address: &Address) -> Option<Address> {
        if !address.is_resolvable() {
            return None;
        }
        let bytes = address.address_bytes();
        let hash = &bytes[..3];
        let prand = &bytes[3..];
        self.resolving_keys
            .iter()
            .find(|(irk, _)| ah(irk, prand).as_slice() == hash)
            .map(|(_, identity)| {
                let address_type = if identity.is_public() {
                    AddressType::PUBLIC_IDENTITY
                } else {
                    AddressType::RANDOM_IDENTITY
                };
                Address::from_bytes(*identity.address_bytes(), address_type)
            })
    }
}

/// Generate an RPA from a caller-supplied `prand`, useful for deterministic
/// scheduling and tests. Its two most-significant bits are forced to `0b01`.
pub fn resolvable_private_address(irk: &[u8; 16], mut prand: [u8; 3]) -> Address {
    prand[2] = (prand[2] & 0x3F) | 0x40;
    let hash = ah(irk, &prand);
    let mut bytes = [0u8; 6];
    bytes[..3].copy_from_slice(&hash);
    bytes[3..].copy_from_slice(&prand);
    Address::from_bytes(bytes, AddressType::RANDOM_DEVICE)
}

pub fn generate_resolvable_private_address(irk: &[u8; 16]) -> Address {
    let random = random_128();
    resolvable_private_address(irk, random[..3].try_into().expect("three-byte slice"))
}
