use bumble_codecs::g722::G722Decoder;

fn decode_hex(value: &str) -> Vec<u8> {
    (0..value.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(&value[index..index + 2], 16).unwrap())
        .collect()
}

#[test]
fn upstream_g722_fixture_frame_is_byte_exact() {
    // First 80-byte frame of tests/g722_sample.g722. The expected PCM was
    // produced by upstream bumble.decoder.G722Decoder.
    let encoded = decode_hex(
        "8c2084208420a02c202f9a31223929a73120b22ca22fbea2bdcd595ffd7e7cfbfd\
         fffdfffffa7eda78f8d87cf8ffda59f9ff5edb565ad3587c9219bfcfd55f9757\
         de78fb9516d13abb54707ad1dbb37b",
    );
    let expected = decode_hex(
        "ffff00000000ffffffff01000000f9fffeff1600fdffadfffdffb500ebff8efe38\
         005602160067fccafca70399159826261ec20d1c28e55b845d552533fe4800b501\
         89ffb01f8449b23fa20dbefed91afc37a138c71e770ba62bd961c4660631d90c\
         d51e2844c1563e51223d56248814dc2523584455e233e8037400ebfedcfe4f01\
         b9ff0d00330189ffbcff010102ffeaff0d000cff69007fff4c007cffa8000700\
         a2002dffd1fff4ff4d020b02a70084fea4fc07fd5900cb039303c6ff70fcdcfb\
         bffec6010502dd003500f8fe01fe64fc85fb23fdbaff9200caffb9fe6bfea0fe\
         11fe97fc23fb64fbf9fb8efb92f911f875f82afb2efd58fc1bf9c9f6c4f74c\
         fbfcfc8dfa1cf7acf44ef592f77cf9cef98ff87bf65af649f746f8b7f993fb69\
         fdfdfd8efc91fa55f954f99df8d2f5c8f50df9d6fdf0fe1bfb69f890fa2eff",
    );
    assert_eq!(encoded.len(), 80);
    assert_eq!(expected.len(), 320);
    assert_eq!(G722Decoder::new().decode_frame(&encoded), expected);
}

#[test]
fn decoder_state_is_preserved_across_frames() {
    let encoded = decode_hex(
        "8c2084208420a02c202f9a31223929a73120b22ca22fbea2bdcd595ffd7e7cfbfd\
         fffdfffffa7eda78f8d87cf8ffda59f9ff5edb565ad3587c9219bfcfd55f9757\
         de78fb9516d13abb54707ad1dbb37b",
    );
    let mut whole = G722Decoder::new();
    let expected = whole.decode_frame(&encoded);
    let mut chunked = G722Decoder::new();
    let mut actual = chunked.decode_frame(&encoded[..40]);
    actual.extend(chunked.decode_frame(&encoded[40..]));
    assert_eq!(actual, expected);
}

#[test]
fn sample_and_little_endian_byte_apis_agree() {
    let encoded = [0x8C, 0x20, 0x84, 0x20];
    let samples = G722Decoder::new().decode_samples(&encoded);
    let bytes = G722Decoder::new().decode_frame(&encoded);
    assert_eq!(samples.len(), encoded.len() * 2);
    let rebuilt: Vec<_> = samples.into_iter().flat_map(i16::to_le_bytes).collect();
    assert_eq!(bytes, rebuilt);
}
