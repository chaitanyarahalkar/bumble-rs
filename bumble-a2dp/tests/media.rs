use bumble_a2dp::media::{
    packetize_aac, packetize_opus, packetize_sbc, parse_ogg_opus, AacFrame, AacProfile,
    OpusChannelMode, SbcFrame,
};

#[test]
fn sbc_parser_matches_upstream_fixture() {
    let data = [0x9C, 0x80, 0x08, 0x00, 0, 0, 0, 0, 0, 0];
    let (frame, consumed) = SbcFrame::parse(&data).unwrap();
    assert_eq!(consumed, data.len());
    assert_eq!(frame.sampling_frequency, 44_100);
    assert_eq!(frame.block_count, 4);
    assert_eq!(frame.channel_mode, 0);
    assert_eq!(frame.allocation_method, 0);
    assert_eq!(frame.subband_count, 4);
    assert_eq!(frame.bitpool, 8);
    assert_eq!(frame.payload, data);
    assert_eq!(frame.sample_count(), 16);
}

#[test]
fn sbc_packet_source_matches_upstream_and_flushes_final_frame() {
    let bytes = [0x9C, 0x80, 0x08, 0x00, 0, 0, 0, 0, 0, 0].repeat(2);
    let frames = SbcFrame::parse_stream(&bytes).unwrap();
    let packets = packetize_sbc(&frames, 23).unwrap();
    assert_eq!(packets.len(), 2);
    assert_eq!(packets[0].sequence_number, 0);
    assert_eq!(packets[0].timestamp, 0);
    assert_eq!(packets[0].payload, [&[1][..], &bytes[..10]].concat());
    assert_eq!(packets[1].sequence_number, 1);
    assert_eq!(packets[1].timestamp, 16);
    assert_eq!(packets[1].payload, [&[1][..], &bytes[10..]].concat());
}

#[test]
fn sbc_parser_and_packetizer_reject_incomplete_or_oversized_frames() {
    assert!(SbcFrame::parse(&[]).is_err());
    assert!(SbcFrame::parse(&[0, 0, 0, 0]).is_err());
    assert!(SbcFrame::parse(&[0x9C, 0x80, 0x08, 0]).is_err());
    let frame = SbcFrame::parse(&[0x9C, 0x80, 0x08, 0, 0, 0, 0, 0, 0, 0])
        .unwrap()
        .0;
    assert!(packetize_sbc(&[frame], 22).is_err());
}

#[test]
fn aac_parser_and_packet_source_match_upstream_fixtures() {
    let bytes = [0xFF, 0xF0, 0x10, 0x00, 0x01, 0xA0, 0x00, 0, 0, 0, 0, 0, 0];
    let (frame, consumed) = AacFrame::parse(&bytes).unwrap();
    assert_eq!(consumed, bytes.len());
    assert_eq!(frame.profile, AacProfile::Main);
    assert_eq!(frame.sampling_frequency, 44_100);
    assert_eq!(frame.channel_configuration, 0);
    assert_eq!(frame.payload, [0; 6]);

    let packets = packetize_aac(&[frame]).unwrap();
    assert_eq!(packets.len(), 1);
    assert_eq!(packets[0].sequence_number, 0);
    assert_eq!(packets[0].timestamp, 0);
    assert_eq!(
        packets[0].payload,
        [0x20, 0x00, 0x12, 0x00, 0x00, 0x00, 0x30, 0, 0, 0, 0, 0, 0]
    );
}

#[test]
fn aac_stream_timestamps_and_errors_are_deterministic() {
    let bytes = [0xFF, 0xF0, 0x10, 0x00, 0x01, 0xA0, 0x00, 0, 0, 0, 0, 0, 0].repeat(2);
    let frames = AacFrame::parse_stream(&bytes).unwrap();
    let packets = packetize_aac(&frames).unwrap();
    assert_eq!(packets[0].timestamp, 0);
    assert_eq!(packets[1].timestamp, 1024);
    assert_eq!(packets[1].sequence_number, 1);

    assert!(AacFrame::parse(&[]).is_err());
    assert!(AacFrame::parse(&[0; 7]).is_err());
    let mut truncated = bytes[..13].to_vec();
    truncated[4] = 0x02;
    assert!(AacFrame::parse(&truncated).is_err());
}

fn opus_fixture() -> Vec<u8> {
    let mut data = b"OggS".to_vec();
    data.extend_from_slice(&[0, 0x02]);
    data.extend_from_slice(&0u64.to_le_bytes());
    data.extend_from_slice(&2u32.to_le_bytes());
    data.extend_from_slice(&2u32.to_le_bytes());
    data.extend_from_slice(&0u32.to_le_bytes());
    data.push(3);
    data.extend_from_slice(&[10, 8, 10]);
    data.extend_from_slice(b"OpusHead\0\0");
    data.extend_from_slice(b"OpusTags");
    data.extend_from_slice(b"0123456789");
    data
}

#[test]
fn opus_parser_and_packet_source_match_upstream_fixture() {
    let packets = parse_ogg_opus(&opus_fixture()).unwrap();
    assert_eq!(packets.len(), 1);
    assert_eq!(packets[0].channel_mode, OpusChannelMode::Stereo);
    assert_eq!(packets[0].duration_ms, 20);
    assert_eq!(packets[0].sampling_frequency, 48_000);
    assert_eq!(packets[0].payload, b"0123456789");

    let rtp = packetize_opus(&packets).unwrap();
    assert_eq!(rtp.len(), 1);
    assert_eq!(rtp[0].sequence_number, 0);
    assert_eq!(rtp[0].timestamp, 0);
    assert_eq!(
        rtp[0].payload,
        b"\x01"
            .iter()
            .chain(b"0123456789")
            .copied()
            .collect::<Vec<_>>()
    );
}

#[test]
fn opus_multi_page_sequence_and_malformed_inputs_are_checked() {
    let mut first = opus_fixture();
    // Append a second selected-stream page containing one more audio packet.
    first.extend_from_slice(b"OggS");
    first.extend_from_slice(&[0, 0]);
    first.extend_from_slice(&0u64.to_le_bytes());
    first.extend_from_slice(&2u32.to_le_bytes());
    first.extend_from_slice(&3u32.to_le_bytes());
    first.extend_from_slice(&0u32.to_le_bytes());
    first.push(1);
    first.push(3);
    first.extend_from_slice(b"abc");
    let packets = parse_ogg_opus(&first).unwrap();
    assert_eq!(packetize_opus(&packets).unwrap()[1].timestamp, 960);

    assert!(parse_ogg_opus(b"Ogg").is_err());
    let mut bad_capture = opus_fixture();
    bad_capture[0] = b'X';
    assert!(parse_ogg_opus(&bad_capture).is_err());
    let mut bad_sequence = first;
    let second_page = opus_fixture().len();
    bad_sequence[second_page + 18..second_page + 22].copy_from_slice(&9u32.to_le_bytes());
    assert!(parse_ogg_opus(&bad_sequence).is_err());
}
