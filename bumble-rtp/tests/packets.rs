use bumble_rtp::{HeaderExtension, MediaPacket};

#[test]
fn upstream_style_media_packet_round_trips() {
    let bytes = [
        0x80, 0xE0, 0x00, 0x01, 0x03, 0x14, 0x1C, 0x6A, 0x00, 0x00, 0x00, 0x00, 0xAA, 0xBB, 0xCC,
    ];
    let packet = MediaPacket::from_bytes(&bytes).unwrap();
    assert_eq!(packet.version, 2);
    assert!(packet.marker);
    assert_eq!(packet.payload_type, 96);
    assert_eq!(packet.sequence_number, 1);
    assert_eq!(packet.timestamp, 0x0314_1C6A);
    assert_eq!(packet.payload, [0xAA, 0xBB, 0xCC]);
    assert_eq!(packet.to_bytes().unwrap(), bytes);
}

#[test]
fn csrc_extension_and_padding_are_parsed_at_correct_offsets() {
    let packet = MediaPacket {
        version: 2,
        marker: false,
        payload_type: 97,
        sequence_number: 0x1234,
        timestamp: 0x0102_0304,
        ssrc: 0x0506_0708,
        csrc_list: vec![0x1112_1314, 0x2122_2324],
        extension: Some(HeaderExtension {
            profile: 0xBEDE,
            data: vec![1, 2, 3, 4, 5, 6, 7, 8],
        }),
        payload: b"audio".to_vec(),
        padding_len: 4,
    };
    let bytes = packet.to_bytes().unwrap();
    assert_eq!(MediaPacket::from_bytes(&bytes).unwrap(), packet);
}

#[test]
fn malformed_remote_packets_return_errors() {
    assert!(MediaPacket::from_bytes(&[0; 11]).is_err());
    assert!(MediaPacket::from_bytes(&[0x82, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]).is_err());
    assert!(MediaPacket::from_bytes(&[0x90, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]).is_err());

    let mut packet = MediaPacket::new(96, 0, 0, 0, Vec::new());
    packet.extension = Some(HeaderExtension {
        profile: 0,
        data: vec![1],
    });
    assert!(packet.to_bytes().is_err());
}
