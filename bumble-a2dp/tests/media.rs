use bumble_a2dp::media::{packetize_sbc, SbcFrame};

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
