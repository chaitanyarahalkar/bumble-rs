use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use bumble::Uuid;
use bumble_gatt::{
    permissions, properties, AdapterError, ByteOrder, ByteSerializable, Characteristic,
    CharacteristicAdapter, CharacteristicDefinition, CharacteristicProxy,
    CharacteristicProxyAdapter, DelegatedCodec, DescriptorDefinition, EnumCodec, GattClient,
    GattServer, IntConvertible, MappedCodec, PackedCodec, PackedValue, SerializableCodec, Service,
    ServiceDefinition, Utf8Codec, ValueCodec,
};

#[test]
fn delegated_packed_mapped_and_utf8_codecs_match_upstream_vectors() {
    let delegated = DelegatedCodec::new(
        |value: &Vec<u8>| Ok(value.iter().rev().copied().collect()),
        |value| Ok(value.iter().rev().copied().collect::<Vec<_>>()),
    );
    assert_eq!(delegated.encode(&vec![3, 4, 5]).unwrap(), vec![5, 4, 3]);
    assert_eq!(delegated.decode(&[3, 4, 5]).unwrap(), vec![5, 4, 3]);
    assert_eq!(
        DelegatedCodec::<u8>::encoder(|value| Ok(vec![*value]))
            .decode(&[1])
            .unwrap_err(),
        AdapterError::MissingDecoder
    );

    let single = PackedCodec::new(">H").unwrap();
    assert_eq!(single.size(), 2);
    assert_eq!(
        single.encode(&PackedValue::Unsigned(1234)).unwrap(),
        vec![0x04, 0xD2]
    );
    assert_eq!(
        single.decode(&[0x04, 0xD2]).unwrap(),
        PackedValue::Unsigned(1234)
    );

    let multiple = PackedCodec::new(">HH").unwrap();
    let tuple = PackedValue::Tuple(vec![
        PackedValue::Unsigned(1234),
        PackedValue::Unsigned(5678),
    ]);
    assert_eq!(
        multiple.encode(&tuple).unwrap(),
        vec![0x04, 0xD2, 0x16, 0x2E]
    );
    assert_eq!(multiple.decode(&[0x04, 0xD2, 0x16, 0x2E]).unwrap(), tuple);

    let mapped = MappedCodec::new(">HH", ["v1", "v2"]).unwrap();
    let values = BTreeMap::from([
        ("v1".to_string(), PackedValue::Unsigned(1234)),
        ("v2".to_string(), PackedValue::Unsigned(5678)),
    ]);
    assert_eq!(
        mapped.encode(&values).unwrap(),
        vec![0x04, 0xD2, 0x16, 0x2E]
    );
    assert_eq!(mapped.decode(&[0x04, 0xD2, 0x16, 0x2E]).unwrap(), values);

    let text = "Hello π".to_string();
    assert_eq!(Utf8Codec.encode(&text).unwrap(), "Hello π".as_bytes());
    assert_eq!(Utf8Codec.decode("Hello π".as_bytes()).unwrap(), text);
    assert!(Utf8Codec.decode(&[0xFF]).is_err());
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Pair(u32, u32);

impl ByteSerializable for Pair {
    fn to_bytes(&self) -> Vec<u8> {
        [self.0.to_be_bytes(), self.1.to_be_bytes()].concat()
    }

    fn from_bytes(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() != 8 {
            return Err("pair requires 8 bytes".into());
        }
        Ok(Self(
            u32::from_be_bytes(bytes[..4].try_into().unwrap()),
            u32::from_be_bytes(bytes[4..].try_into().unwrap()),
        ))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Example {
    One = 1234,
    Two = 5678,
}

impl IntConvertible for Example {
    fn to_u64(&self) -> u64 {
        *self as u64
    }

    fn from_u64(value: u64) -> Result<Self, String> {
        match value {
            1234 => Ok(Self::One),
            5678 => Ok(Self::Two),
            _ => Err(format!("unknown Example value {value}")),
        }
    }
}

#[test]
fn serializable_and_enum_codecs_round_trip() {
    let serializable = SerializableCodec::<Pair>::default();
    let pair = Pair(3, 4);
    assert_eq!(
        serializable.encode(&pair).unwrap(),
        vec![0, 0, 0, 3, 0, 0, 0, 4]
    );
    assert_eq!(
        serializable.decode(&[0, 0, 0, 3, 0, 0, 0, 4]).unwrap(),
        pair
    );

    let big = EnumCodec::<Example>::new(3, ByteOrder::Big).unwrap();
    assert_eq!(big.encode(&Example::Two).unwrap(), vec![0x00, 0x16, 0x2E]);
    assert_eq!(big.decode(&[0x00, 0x16, 0x2E]).unwrap(), Example::Two);
    let little = EnumCodec::<Example>::new(3, ByteOrder::Little).unwrap();
    assert_eq!(
        little.encode(&Example::One).unwrap(),
        vec![0xD2, 0x04, 0x00]
    );
    assert_eq!(little.decode(&[0xD2, 0x04, 0x00]).unwrap(), Example::One);
}

#[test]
fn typed_proxy_reads_writes_and_decodes_cached_values() {
    let mut server = GattServer::new(vec![Service {
        uuid: Uuid::from_16_bits(0x180F),
        characteristics: vec![Characteristic {
            uuid: Uuid::from_16_bits(0x2A19),
            properties: properties::READ | properties::WRITE,
            value: vec![0x04, 0xD2],
        }],
    }]);
    let proxy = CharacteristicProxy {
        declaration_handle: 2,
        handle: 3,
        end_group_handle: 3,
        properties: properties::READ | properties::WRITE,
        uuid: Uuid::from_16_bits(0x2A19),
    };
    let adapter = CharacteristicProxyAdapter::new(proxy, PackedCodec::new(">H").unwrap());
    let mut client = GattClient::new();
    assert_eq!(
        adapter.read_value(&mut client, &mut server, false).unwrap(),
        PackedValue::Unsigned(1234)
    );
    assert_eq!(
        adapter.decode_cached(&client).unwrap(),
        Some(PackedValue::Unsigned(1234))
    );
    adapter
        .write_value(&mut client, &mut server, &PackedValue::Unsigned(5678), true)
        .unwrap();
    assert_eq!(
        client.read_value(&mut server, 3, false).unwrap(),
        vec![0x16, 0x2E]
    );
}

#[test]
fn server_adapter_binds_typed_state_to_dynamic_attribute() {
    let definition = CharacteristicDefinition {
        uuid: Uuid::from_16_bits(0x2A00),
        properties: properties::READ | properties::WRITE,
        permissions: permissions::READABLE | permissions::WRITEABLE,
        value: b"placeholder".to_vec(),
        descriptors: Vec::<DescriptorDefinition>::new(),
    };
    let adapter = CharacteristicAdapter::new(definition.clone(), Utf8Codec);
    let state = Arc::new(Mutex::new("Hello π".to_string()));
    let mut server = GattServer::from_definitions(vec![ServiceDefinition {
        uuid: Uuid::from_16_bits(0x1800),
        primary: true,
        included_services: vec![],
        characteristics: vec![definition],
    }])
    .unwrap();
    server
        .set_dynamic_value(3, adapter.dynamic_value(Arc::clone(&state)))
        .unwrap();

    let mut client = GattClient::new();
    assert_eq!(
        client.read_value(&mut server, 3, false).unwrap(),
        "Hello π".as_bytes()
    );
    client
        .write_value(&mut server, 3, "Updated".as_bytes().to_vec(), true)
        .unwrap();
    assert_eq!(&*state.lock().unwrap(), "Updated");
}

#[test]
fn packed_codec_handles_standard_scalar_string_and_padding_forms() {
    let codec = PackedCodec::new("<bB?2x4s3pfd").unwrap();
    let value = PackedValue::Tuple(vec![
        PackedValue::Signed(-2),
        PackedValue::Unsigned(250),
        PackedValue::Bool(true),
        PackedValue::Bytes(b"xy".to_vec()),
        PackedValue::Bytes(b"ab".to_vec()),
        PackedValue::Float(1.5),
        PackedValue::Float(-2.25),
    ]);
    let encoded = codec.encode(&value).unwrap();
    assert_eq!(encoded.len(), codec.size());
    let decoded = codec.decode(&encoded).unwrap();
    assert_eq!(
        decoded,
        PackedValue::Tuple(vec![
            PackedValue::Signed(-2),
            PackedValue::Unsigned(250),
            PackedValue::Bool(true),
            PackedValue::Bytes(b"xy\0\0".to_vec()),
            PackedValue::Bytes(b"ab".to_vec()),
            PackedValue::Float(1.5),
            PackedValue::Float(-2.25),
        ])
    );
}

#[test]
fn packed_codec_matches_python_314_native_half_and_complex_oracles() {
    let half_big = PackedCodec::new(">e").unwrap();
    assert_eq!(
        half_big.encode(&PackedValue::Float(1.5)).unwrap(),
        [0x3E, 0x00]
    );
    assert_eq!(
        half_big.decode(&[0x3E, 0x00]).unwrap(),
        PackedValue::Float(1.5)
    );
    let half_little = PackedCodec::new("<e").unwrap();
    assert_eq!(
        half_little.encode(&PackedValue::Float(-2.25)).unwrap(),
        [0x80, 0xC0]
    );

    let complex32 = PackedCodec::new(">F").unwrap();
    assert_eq!(
        complex32.encode(&PackedValue::Complex(1.0, 2.0)).unwrap(),
        [0x3F, 0x80, 0, 0, 0x40, 0, 0, 0]
    );
    assert_eq!(
        complex32
            .decode(&[0x3F, 0x80, 0, 0, 0x40, 0, 0, 0])
            .unwrap(),
        PackedValue::Complex(1.0, 2.0)
    );
    let complex64 = PackedCodec::new(">D").unwrap();
    assert_eq!(
        complex64.encode(&PackedValue::Complex(1.0, 2.0)).unwrap(),
        [0x3F, 0xF0, 0, 0, 0, 0, 0, 0, 0x40, 0, 0, 0, 0, 0, 0, 0,]
    );

    let zero_string = PackedCodec::new("0s").unwrap();
    assert_eq!(zero_string.size(), 0);
    assert_eq!(
        zero_string
            .encode(&PackedValue::Bytes(b"ignored".to_vec()))
            .unwrap(),
        Vec::<u8>::new()
    );
    assert_eq!(
        PackedCodec::new("0p").unwrap().decode(&[]).unwrap(),
        PackedValue::Bytes(vec![])
    );
    assert!(PackedCodec::new(">n").is_err());

    #[cfg(all(
        target_endian = "little",
        target_pointer_width = "64",
        not(target_os = "windows")
    ))]
    {
        let native = PackedCodec::new("@bhi").unwrap();
        let values = PackedValue::Tuple(vec![
            PackedValue::Signed(-2),
            PackedValue::Signed(0x1234),
            PackedValue::Signed(0x1234_5678),
        ]);
        assert_eq!(native.size(), 8);
        assert_eq!(
            native.encode(&values).unwrap(),
            [0xFE, 0, 0x34, 0x12, 0x78, 0x56, 0x34, 0x12]
        );
        assert_eq!(
            native
                .decode(&[0xFE, 0, 0x34, 0x12, 0x78, 0x56, 0x34, 0x12])
                .unwrap(),
            values
        );

        let long = PackedCodec::new("@bl").unwrap();
        assert_eq!(long.size(), 16);
        assert_eq!(
            long.encode(&PackedValue::Tuple(vec![
                PackedValue::Signed(-2),
                PackedValue::Signed(0x1234_5678),
            ]))
            .unwrap(),
            [0xFE, 0, 0, 0, 0, 0, 0, 0, 0x78, 0x56, 0x34, 0x12, 0, 0, 0, 0,]
        );

        let pointer = PackedCodec::new("@nNP").unwrap();
        assert_eq!(pointer.size(), 24);
        assert_eq!(
            pointer
                .encode(&PackedValue::Tuple(vec![
                    PackedValue::Signed(-2),
                    PackedValue::Unsigned(0x1234_5678),
                    PackedValue::Unsigned(0x1234),
                ]))
                .unwrap(),
            [
                0xFE, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x78, 0x56, 0x34, 0x12, 0, 0, 0, 0,
                0x34, 0x12, 0, 0, 0, 0, 0, 0,
            ]
        );

        let tail_alignment = PackedCodec::new("@llh0l").unwrap();
        assert_eq!(tail_alignment.size(), 24);
        assert_eq!(
            tail_alignment
                .encode(&PackedValue::Tuple(vec![
                    PackedValue::Signed(1),
                    PackedValue::Signed(2),
                    PackedValue::Signed(3),
                ]))
                .unwrap(),
            [1, 0, 0, 0, 0, 0, 0, 0, 2, 0, 0, 0, 0, 0, 0, 0, 3, 0, 0, 0, 0, 0, 0, 0,]
        );
    }
}
