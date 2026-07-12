use bumble_a2dp::media::{packetize_aac, packetize_sbc, AacFrame, AacProfile, SbcFrame};

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
