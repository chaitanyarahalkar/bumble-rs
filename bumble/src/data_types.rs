//! Typed Advertising Data structures (Core Spec Supplement, Part A).
//!
//! Ported from `bumble.data_types`. Where [`crate::AdvertisingData`] handles raw
//! type-length-value structures, this module gives each standard AD type a
//! decoded [`DataType`] representation, with byte-exact encode/decode. Convert
//! between the two with [`AdvertisingData::data_types`] and
//! [`AdvertisingData::append_data_type`].
//!
//! Types with no defined layout upstream (e.g. BigInfo) fall through to
//! [`DataType::Generic`].

use crate::advertising_data::Type;
use crate::{Address, AddressType, AdvertisingData, Appearance, ClassOfDevice, Uuid};

/// A decoded Advertising Data structure.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DataType {
    Flags(u32),
    IncompleteListOf16BitServiceUuids(Vec<Uuid>),
    CompleteListOf16BitServiceUuids(Vec<Uuid>),
    IncompleteListOf32BitServiceUuids(Vec<Uuid>),
    CompleteListOf32BitServiceUuids(Vec<Uuid>),
    IncompleteListOf128BitServiceUuids(Vec<Uuid>),
    CompleteListOf128BitServiceUuids(Vec<Uuid>),
    ShortenedLocalName(String),
    CompleteLocalName(String),
    TxPowerLevel(i8),
    ClassOfDevice(ClassOfDevice),
    ManufacturerSpecificData {
        company_identifier: u16,
        data: Vec<u8>,
    },
    SimplePairingHashC192([u8; 16]),
    SimplePairingRandomizerR192([u8; 16]),
    SimplePairingHashC256([u8; 16]),
    SimplePairingRandomizerR256([u8; 16]),
    LeSecureConnectionsConfirmationValue([u8; 16]),
    LeSecureConnectionsRandomValue([u8; 16]),
    SecurityManagerTkValue([u8; 16]),
    SecurityManagerOutOfBandFlags(u8),
    PeripheralConnectionIntervalRange {
        min: u16,
        max: u16,
    },
    ListOf16BitServiceSolicitationUuids(Vec<Uuid>),
    ListOf32BitServiceSolicitationUuids(Vec<Uuid>),
    ListOf128BitServiceSolicitationUuids(Vec<Uuid>),
    ServiceData16BitUuid {
        service_uuid: Uuid,
        data: Vec<u8>,
    },
    ServiceData32BitUuid {
        service_uuid: Uuid,
        data: Vec<u8>,
    },
    ServiceData128BitUuid {
        service_uuid: Uuid,
        data: Vec<u8>,
    },
    PublicTargetAddress(Address),
    RandomTargetAddress(Address),
    Appearance(Appearance),
    AdvertisingInterval(u16),
    LeBluetoothDeviceAddress(Address),
    LeRole(u8),
    Uri(String),
    LeSupportedFeatures(u64),
    ChannelMapUpdateIndication {
        chm: u64,
        instant: u16,
    },
    AdvertisingIntervalLong(u32),
    BroadcastCode(String),
    BroadcastName(String),
    ResolvableSetIdentifier([u8; 6]),
    /// Any AD type this module does not decode: the raw value bytes.
    Generic {
        ad_type: u8,
        data: Vec<u8>,
    },
}

/// The minimal little-endian byte representation of `v` (at least one byte).
fn minimal_le(v: u64) -> Vec<u8> {
    if v == 0 {
        return vec![0];
    }
    let mut out = Vec::new();
    let mut v = v;
    while v != 0 {
        out.push((v & 0xFF) as u8);
        v >>= 8;
    }
    out
}

/// Little-endian integer from up to 8 bytes.
fn le_uint(data: &[u8]) -> u64 {
    let mut v = 0u64;
    for (i, b) in data.iter().take(8).enumerate() {
        v |= (*b as u64) << (8 * i);
    }
    v
}

fn encode_uuids(uuids: &[Uuid]) -> Vec<u8> {
    let mut out = Vec::new();
    for u in uuids {
        out.extend_from_slice(&u.to_bytes(false));
    }
    out
}

fn decode_uuids(data: &[u8], size: usize) -> Vec<Uuid> {
    data.chunks_exact(size)
        .filter_map(|c| Uuid::from_bytes(c).ok())
        .collect()
}

fn to_array<const N: usize>(data: &[u8]) -> [u8; N] {
    let mut out = [0u8; N];
    let n = data.len().min(N);
    out[..n].copy_from_slice(&data[..n]);
    out
}

impl DataType {
    /// The AD type identifier for this structure.
    pub fn ad_type(&self) -> Type {
        use DataType::*;
        Type(match self {
            Flags(_) => Type::FLAGS.0,
            IncompleteListOf16BitServiceUuids(_) => {
                Type::INCOMPLETE_LIST_OF_16_BIT_SERVICE_CLASS_UUIDS.0
            }
            CompleteListOf16BitServiceUuids(_) => {
                Type::COMPLETE_LIST_OF_16_BIT_SERVICE_CLASS_UUIDS.0
            }
            IncompleteListOf32BitServiceUuids(_) => {
                Type::INCOMPLETE_LIST_OF_32_BIT_SERVICE_CLASS_UUIDS.0
            }
            CompleteListOf32BitServiceUuids(_) => {
                Type::COMPLETE_LIST_OF_32_BIT_SERVICE_CLASS_UUIDS.0
            }
            IncompleteListOf128BitServiceUuids(_) => {
                Type::INCOMPLETE_LIST_OF_128_BIT_SERVICE_CLASS_UUIDS.0
            }
            CompleteListOf128BitServiceUuids(_) => {
                Type::COMPLETE_LIST_OF_128_BIT_SERVICE_CLASS_UUIDS.0
            }
            ShortenedLocalName(_) => Type::SHORTENED_LOCAL_NAME.0,
            CompleteLocalName(_) => Type::COMPLETE_LOCAL_NAME.0,
            TxPowerLevel(_) => Type::TX_POWER_LEVEL.0,
            ClassOfDevice(_) => Type::CLASS_OF_DEVICE.0,
            ManufacturerSpecificData { .. } => Type::MANUFACTURER_SPECIFIC_DATA.0,
            SimplePairingHashC192(_) => 0x0E,
            SimplePairingRandomizerR192(_) => 0x0F,
            SimplePairingHashC256(_) => 0x1D,
            SimplePairingRandomizerR256(_) => 0x1E,
            LeSecureConnectionsConfirmationValue(_) => 0x22,
            LeSecureConnectionsRandomValue(_) => 0x23,
            SecurityManagerTkValue(_) => 0x10,
            SecurityManagerOutOfBandFlags(_) => 0x11,
            PeripheralConnectionIntervalRange { .. } => 0x12,
            ListOf16BitServiceSolicitationUuids(_) => 0x14,
            ListOf32BitServiceSolicitationUuids(_) => 0x1F,
            ListOf128BitServiceSolicitationUuids(_) => 0x15,
            ServiceData16BitUuid { .. } => 0x16,
            ServiceData32BitUuid { .. } => 0x20,
            ServiceData128BitUuid { .. } => 0x21,
            PublicTargetAddress(_) => 0x17,
            RandomTargetAddress(_) => 0x18,
            Appearance(_) => Type::APPEARANCE.0,
            AdvertisingInterval(_) => Type::ADVERTISING_INTERVAL.0,
            LeBluetoothDeviceAddress(_) => 0x1B,
            LeRole(_) => 0x1C,
            Uri(_) => Type::URI.0,
            LeSupportedFeatures(_) => 0x27,
            ChannelMapUpdateIndication { .. } => 0x28,
            AdvertisingIntervalLong(_) => 0x2F,
            BroadcastCode(_) => 0x2D,
            BroadcastName(_) => Type::BROADCAST_NAME.0,
            ResolvableSetIdentifier(_) => 0x2E,
            Generic { ad_type, .. } => *ad_type,
        })
    }

    /// The serialized value bytes (the `V` in the TLV; no type or length byte).
    pub fn value_bytes(&self) -> Vec<u8> {
        use DataType::*;
        match self {
            Flags(v) => minimal_le(*v as u64),
            IncompleteListOf16BitServiceUuids(u)
            | CompleteListOf16BitServiceUuids(u)
            | IncompleteListOf32BitServiceUuids(u)
            | CompleteListOf32BitServiceUuids(u)
            | IncompleteListOf128BitServiceUuids(u)
            | CompleteListOf128BitServiceUuids(u)
            | ListOf16BitServiceSolicitationUuids(u)
            | ListOf32BitServiceSolicitationUuids(u)
            | ListOf128BitServiceSolicitationUuids(u) => encode_uuids(u),
            ShortenedLocalName(s)
            | CompleteLocalName(s)
            | Uri(s)
            | BroadcastCode(s)
            | BroadcastName(s) => s.as_bytes().to_vec(),
            TxPowerLevel(v) => vec![*v as u8],
            ClassOfDevice(c) => c.to_int().to_le_bytes()[..3].to_vec(),
            ManufacturerSpecificData {
                company_identifier,
                data,
            } => {
                let mut p = company_identifier.to_le_bytes().to_vec();
                p.extend_from_slice(data);
                p
            }
            SimplePairingHashC192(b)
            | SimplePairingRandomizerR192(b)
            | SimplePairingHashC256(b)
            | SimplePairingRandomizerR256(b)
            | LeSecureConnectionsConfirmationValue(b)
            | LeSecureConnectionsRandomValue(b)
            | SecurityManagerTkValue(b) => b.to_vec(),
            SecurityManagerOutOfBandFlags(v) | LeRole(v) => vec![*v],
            PeripheralConnectionIntervalRange { min, max } => {
                let mut p = Vec::with_capacity(4);
                p.extend_from_slice(&min.to_le_bytes());
                p.extend_from_slice(&max.to_le_bytes());
                p
            }
            ServiceData16BitUuid { service_uuid, data }
            | ServiceData32BitUuid { service_uuid, data }
            | ServiceData128BitUuid { service_uuid, data } => {
                let mut p = service_uuid.to_bytes(false);
                p.extend_from_slice(data);
                p
            }
            PublicTargetAddress(a) | RandomTargetAddress(a) => a.address_bytes().to_vec(),
            Appearance(a) => a.to_int().to_le_bytes().to_vec(),
            AdvertisingInterval(v) => v.to_le_bytes().to_vec(),
            LeBluetoothDeviceAddress(a) => {
                let mut p = vec![a.address_type().0];
                p.extend_from_slice(a.address_bytes());
                p
            }
            LeSupportedFeatures(v) => minimal_le(*v),
            ChannelMapUpdateIndication { chm, instant } => {
                let mut p = chm.to_le_bytes()[..5].to_vec();
                p.extend_from_slice(&instant.to_le_bytes());
                p
            }
            AdvertisingIntervalLong(v) => {
                let n = if *v >= 0x0100_0000 { 4 } else { 3 };
                v.to_le_bytes()[..n].to_vec()
            }
            ResolvableSetIdentifier(b) => b.to_vec(),
            Generic { data, .. } => data.clone(),
        }
    }

    /// Decode an AD structure from its type and value bytes.
    // The `x if x == Type::CONST.0` arms compare against the named AD-type
    // constants (a field access on an associated const, not a plain literal),
    // which reads better than magic numbers; clippy's rewrite doesn't apply.
    #[allow(clippy::redundant_guards)]
    pub fn from_ad(ad_type: Type, data: &[u8]) -> DataType {
        use DataType::*;
        let string = || String::from_utf8_lossy(data).into_owned();
        match ad_type.0 {
            x if x == Type::FLAGS.0 => Flags(le_uint(data) as u32),
            x if x == Type::INCOMPLETE_LIST_OF_16_BIT_SERVICE_CLASS_UUIDS.0 => {
                IncompleteListOf16BitServiceUuids(decode_uuids(data, 2))
            }
            x if x == Type::COMPLETE_LIST_OF_16_BIT_SERVICE_CLASS_UUIDS.0 => {
                CompleteListOf16BitServiceUuids(decode_uuids(data, 2))
            }
            x if x == Type::INCOMPLETE_LIST_OF_32_BIT_SERVICE_CLASS_UUIDS.0 => {
                IncompleteListOf32BitServiceUuids(decode_uuids(data, 4))
            }
            x if x == Type::COMPLETE_LIST_OF_32_BIT_SERVICE_CLASS_UUIDS.0 => {
                CompleteListOf32BitServiceUuids(decode_uuids(data, 4))
            }
            x if x == Type::INCOMPLETE_LIST_OF_128_BIT_SERVICE_CLASS_UUIDS.0 => {
                IncompleteListOf128BitServiceUuids(decode_uuids(data, 16))
            }
            x if x == Type::COMPLETE_LIST_OF_128_BIT_SERVICE_CLASS_UUIDS.0 => {
                CompleteListOf128BitServiceUuids(decode_uuids(data, 16))
            }
            x if x == Type::SHORTENED_LOCAL_NAME.0 => ShortenedLocalName(string()),
            x if x == Type::COMPLETE_LOCAL_NAME.0 => CompleteLocalName(string()),
            x if x == Type::TX_POWER_LEVEL.0 => TxPowerLevel(*data.first().unwrap_or(&0) as i8),
            x if x == Type::CLASS_OF_DEVICE.0 => {
                ClassOfDevice(crate::ClassOfDevice::from_int(le_uint(data) as u32))
            }
            x if x == Type::MANUFACTURER_SPECIFIC_DATA.0 => ManufacturerSpecificData {
                company_identifier: le_uint(data.get(..2).unwrap_or(&[])) as u16,
                data: data.get(2..).unwrap_or(&[]).to_vec(),
            },
            0x0E => SimplePairingHashC192(to_array(data)),
            0x0F => SimplePairingRandomizerR192(to_array(data)),
            0x1D => SimplePairingHashC256(to_array(data)),
            0x1E => SimplePairingRandomizerR256(to_array(data)),
            0x22 => LeSecureConnectionsConfirmationValue(to_array(data)),
            0x23 => LeSecureConnectionsRandomValue(to_array(data)),
            0x10 => SecurityManagerTkValue(to_array(data)),
            0x11 => SecurityManagerOutOfBandFlags(*data.first().unwrap_or(&0)),
            0x12 => PeripheralConnectionIntervalRange {
                min: le_uint(data.get(0..2).unwrap_or(&[])) as u16,
                max: le_uint(data.get(2..4).unwrap_or(&[])) as u16,
            },
            0x14 => ListOf16BitServiceSolicitationUuids(decode_uuids(data, 2)),
            0x1F => ListOf32BitServiceSolicitationUuids(decode_uuids(data, 4)),
            0x15 => ListOf128BitServiceSolicitationUuids(decode_uuids(data, 16)),
            x if x == 0x16 => ServiceData16BitUuid {
                service_uuid: Uuid::from_bytes(data.get(..2).unwrap_or(&[]))
                    .unwrap_or(Uuid::from_16_bits(0)),
                data: data.get(2..).unwrap_or(&[]).to_vec(),
            },
            0x20 => ServiceData32BitUuid {
                service_uuid: Uuid::from_bytes(data.get(..4).unwrap_or(&[]))
                    .unwrap_or(Uuid::from_32_bits(0)),
                data: data.get(4..).unwrap_or(&[]).to_vec(),
            },
            0x21 => ServiceData128BitUuid {
                service_uuid: Uuid::from_bytes(data.get(..16).unwrap_or(&[]))
                    .unwrap_or(Uuid::from_16_bits(0)),
                data: data.get(16..).unwrap_or(&[]).to_vec(),
            },
            0x17 => PublicTargetAddress(Address::from_bytes(
                to_array(data),
                AddressType::PUBLIC_DEVICE,
            )),
            0x18 => RandomTargetAddress(Address::from_bytes(
                to_array(data),
                AddressType::RANDOM_DEVICE,
            )),
            x if x == Type::APPEARANCE.0 => {
                Appearance(crate::Appearance::from_int(le_uint(data) as u16))
            }
            x if x == Type::ADVERTISING_INTERVAL.0 => AdvertisingInterval(le_uint(data) as u16),
            0x1B => LeBluetoothDeviceAddress(Address::from_bytes(
                to_array(data.get(1..7).unwrap_or(&[])),
                AddressType(*data.first().unwrap_or(&0)),
            )),
            0x1C => LeRole(*data.first().unwrap_or(&0)),
            x if x == Type::URI.0 => Uri(string()),
            0x27 => LeSupportedFeatures(le_uint(data)),
            0x28 => ChannelMapUpdateIndication {
                chm: le_uint(data.get(0..5).unwrap_or(&[])),
                instant: le_uint(data.get(5..7).unwrap_or(&[])) as u16,
            },
            0x2F => AdvertisingIntervalLong(le_uint(data) as u32),
            0x2D => BroadcastCode(string()),
            x if x == Type::BROADCAST_NAME.0 => BroadcastName(string()),
            0x2E => ResolvableSetIdentifier(to_array(data)),
            other => Generic {
                ad_type: other,
                data: data.to_vec(),
            },
        }
    }
}

impl AdvertisingData {
    /// Decode all structures into typed [`DataType`] values.
    pub fn data_types(&self) -> Vec<DataType> {
        self.ad_structures
            .iter()
            .map(|(t, v)| DataType::from_ad(*t, v))
            .collect()
    }

    /// Append a typed [`DataType`] as a raw TLV structure.
    pub fn append_data_type(&mut self, data_type: &DataType) {
        self.ad_structures
            .push((data_type.ad_type(), data_type.value_bytes()));
    }

    /// Build an `AdvertisingData` from typed structures.
    pub fn from_data_types(data_types: &[DataType]) -> AdvertisingData {
        let mut ad = AdvertisingData::new();
        for dt in data_types {
            ad.append_data_type(dt);
        }
        ad
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(b: &[u8]) -> String {
        b.iter().map(|x| format!("{x:02x}")).collect()
    }
    fn unhex(s: &str) -> Vec<u8> {
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
            .collect()
    }

    /// value_bytes pinned to Python oracle output, and every case round-trips
    /// via `from_ad`.
    fn check(dt: DataType, expected_value_hex: &str) {
        assert_eq!(hex(&dt.value_bytes()), expected_value_hex, "value bytes");
        assert_eq!(
            DataType::from_ad(dt.ad_type(), &dt.value_bytes()),
            dt,
            "round-trip"
        );
    }

    #[test]
    fn oracle_pinned_encodings() {
        check(DataType::Flags(0x06), "06");
        check(
            DataType::CompleteListOf16BitServiceUuids(vec![
                Uuid::from_16_bits(0x180F),
                Uuid::from_16_bits(0x180A),
            ]),
            "0f180a18",
        );
        check(DataType::CompleteLocalName("Bumble".into()), "42756d626c65");
        check(DataType::TxPowerLevel(-20), "ec");
        check(
            DataType::ManufacturerSpecificData {
                company_identifier: 0x004C,
                data: unhex("0215"),
            },
            "4c000215",
        );
        check(
            DataType::ServiceData16BitUuid {
                service_uuid: Uuid::from_16_bits(0x180F),
                data: vec![0x64],
            },
            "0f1864",
        );
        check(
            DataType::PeripheralConnectionIntervalRange { min: 6, max: 12 },
            "06000c00",
        );
        check(DataType::AdvertisingInterval(0x0800), "0008");
        check(DataType::AdvertisingIntervalLong(0x112233), "332211");
        check(
            DataType::LeSecureConnectionsConfirmationValue(to_array(&unhex(
                "000102030405060708090a0b0c0d0e0f",
            ))),
            "000102030405060708090a0b0c0d0e0f",
        );
        check(
            DataType::ResolvableSetIdentifier([0x11, 0x22, 0x33, 0x44, 0x55, 0x66]),
            "112233445566",
        );
        check(
            DataType::Uri("//example.com".into()),
            "2f2f6578616d706c652e636f6d",
        );
        check(DataType::LeRole(0), "00");
        check(DataType::SecurityManagerOutOfBandFlags(0x03), "03");
        check(
            DataType::ChannelMapUpdateIndication {
                chm: 0x1F_FFFF_FFFF,
                instant: 0x0006,
            },
            "ffffffff1f0600",
        );
        check(DataType::LeSupportedFeatures(0x01), "01");
    }

    #[test]
    fn class_of_device_and_appearance() {
        check(
            DataType::ClassOfDevice(crate::ClassOfDevice::new(
                crate::MajorServiceClasses::AUDIO,
                crate::MajorDeviceClass::AUDIO_VIDEO,
                0x0D,
            )),
            "340420",
        );
        check(
            DataType::Appearance(crate::Appearance::new(crate::Category::COMPUTER, 0x03)),
            "8300",
        );
    }

    #[test]
    fn advertising_data_round_trip() {
        let types = vec![
            DataType::Flags(0x06),
            DataType::CompleteLocalName("Bumble".into()),
            DataType::TxPowerLevel(4),
        ];
        let ad = AdvertisingData::from_data_types(&types);
        // The raw bytes round-trip, and decoding recovers the typed values.
        let reparsed = AdvertisingData::from_bytes(&ad.to_bytes());
        assert_eq!(reparsed.data_types(), types);
    }
}
