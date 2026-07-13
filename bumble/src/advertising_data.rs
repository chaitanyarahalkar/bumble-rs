//! Advertising Data (Core Spec Supplement, Part A) — raw TLV codec.
//!
//! Ported from `bumble.core.AdvertisingData`. This slice implements the raw
//! type-length-value handling only: `from_bytes` / `append` / `get` /
//! `get_all` / `to_bytes`. The typed `DataType` value hierarchy from
//! `bumble.data_types` is deferred to a later slice.

/// Advertising Data structure type (open enum, newtype over `u8`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Type(pub u8);

impl Type {
    pub const FLAGS: Type = Type(0x01);
    pub const INCOMPLETE_LIST_OF_16_BIT_SERVICE_CLASS_UUIDS: Type = Type(0x02);
    pub const COMPLETE_LIST_OF_16_BIT_SERVICE_CLASS_UUIDS: Type = Type(0x03);
    pub const INCOMPLETE_LIST_OF_32_BIT_SERVICE_CLASS_UUIDS: Type = Type(0x04);
    pub const COMPLETE_LIST_OF_32_BIT_SERVICE_CLASS_UUIDS: Type = Type(0x05);
    pub const INCOMPLETE_LIST_OF_128_BIT_SERVICE_CLASS_UUIDS: Type = Type(0x06);
    pub const COMPLETE_LIST_OF_128_BIT_SERVICE_CLASS_UUIDS: Type = Type(0x07);
    pub const SHORTENED_LOCAL_NAME: Type = Type(0x08);
    pub const COMPLETE_LOCAL_NAME: Type = Type(0x09);
    pub const TX_POWER_LEVEL: Type = Type(0x0A);
    pub const CLASS_OF_DEVICE: Type = Type(0x0D);
    pub const SECURITY_MANAGER_TK_VALUE: Type = Type(0x10);
    pub const APPEARANCE: Type = Type(0x19);
    pub const ADVERTISING_INTERVAL: Type = Type(0x1A);
    pub const LE_BLUETOOTH_DEVICE_ADDRESS: Type = Type(0x1B);
    pub const LE_ROLE: Type = Type(0x1C);
    pub const LE_SECURE_CONNECTIONS_CONFIRMATION_VALUE: Type = Type(0x22);
    pub const LE_SECURE_CONNECTIONS_RANDOM_VALUE: Type = Type(0x23);
    pub const URI: Type = Type(0x24);
    pub const BROADCAST_NAME: Type = Type(0x30);
    pub const MANUFACTURER_SPECIFIC_DATA: Type = Type(0xFF);
}

/// A parsed Advertising Data blob: an ordered list of `(type, value)` TLVs.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AdvertisingData {
    /// The TLV structures, in order.
    pub ad_structures: Vec<(Type, Vec<u8>)>,
}

impl AdvertisingData {
    /// An empty Advertising Data.
    pub fn new() -> AdvertisingData {
        AdvertisingData::default()
    }

    /// Parse Advertising Data from a serialized buffer.
    pub fn from_bytes(data: &[u8]) -> AdvertisingData {
        let mut ad = AdvertisingData::new();
        ad.append(data);
        ad
    }

    /// Parse and append more TLV structures from a buffer.
    ///
    /// Faithful to Bumble's parser: a `length` byte, then a type byte, then
    /// `length - 1` data bytes; zero-length structures are skipped.
    pub fn append(&mut self, data: &[u8]) {
        let mut offset = 0usize;
        while offset + 1 < data.len() {
            let length = data[offset] as usize;
            offset += 1;
            if length > 0 {
                let ad_type = data[offset];
                let start = offset + 1;
                let end = (offset + length).min(data.len());
                let value = if start <= end {
                    data[start..end].to_vec()
                } else {
                    Vec::new()
                };
                self.ad_structures.push((Type(ad_type), value));
            }
            offset += length;
        }
    }

    /// The raw value of the first structure of the given type, if present.
    pub fn get(&self, ad_type: Type) -> Option<Vec<u8>> {
        self.ad_structures
            .iter()
            .find(|(t, _)| *t == ad_type)
            .map(|(_, v)| v.clone())
    }

    /// The raw values of all structures of the given type, in order.
    pub fn get_all(&self, ad_type: Type) -> Vec<Vec<u8>> {
        self.ad_structures
            .iter()
            .filter(|(t, _)| *t == ad_type)
            .map(|(_, v)| v.clone())
            .collect()
    }

    /// Serialize back to bytes. Each structure is `[len(data)+1, type, data…]`.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        for (t, v) in &self.ad_structures {
            out.push((v.len() + 1) as u8);
            out.push(t.0);
            out.extend_from_slice(v);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Ported from bumble tests/core_test.py::test_ad_data
    #[test]
    fn test_ad_data() {
        let data = vec![2u8, Type::TX_POWER_LEVEL.0, 123];
        let mut ad = AdvertisingData::from_bytes(&data);
        assert_eq!(ad.to_bytes(), data);
        assert_eq!(ad.get(Type::COMPLETE_LOCAL_NAME), None);
        assert_eq!(ad.get(Type::TX_POWER_LEVEL), Some(vec![123]));
        assert_eq!(ad.get_all(Type::COMPLETE_LOCAL_NAME), Vec::<Vec<u8>>::new());
        assert_eq!(ad.get_all(Type::TX_POWER_LEVEL), vec![vec![123]]);

        let data2 = vec![2u8, Type::TX_POWER_LEVEL.0, 234];
        ad.append(&data2);
        let mut combined = data.clone();
        combined.extend_from_slice(&data2);
        assert_eq!(ad.to_bytes(), combined);
        assert_eq!(ad.get(Type::COMPLETE_LOCAL_NAME), None);
        assert_eq!(ad.get(Type::TX_POWER_LEVEL), Some(vec![123]));
        assert_eq!(ad.get_all(Type::COMPLETE_LOCAL_NAME), Vec::<Vec<u8>>::new());
        assert_eq!(ad.get_all(Type::TX_POWER_LEVEL), vec![vec![123], vec![234]]);
    }
}
