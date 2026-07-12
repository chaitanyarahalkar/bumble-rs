use bumble_a2dp::media::{packetize_sbc, SbcFrame};
use bumble_a2dp::transport::L2capMediaTransport;
use bumble_avdtp::AVDTP_PSM;
use bumble_l2cap::{ChannelManager, ClassicChannelSpec};

fn relay(left: &mut ChannelManager, right: &mut ChannelManager) -> usize {
    let mut count = 0;
    while let Some(pdu) = left.poll_outbound() {
        right.process_pdu(pdu).unwrap();
        count += 1;
    }
    count
}

#[test]
fn sbc_rtp_packets_cross_a_live_avdtp_media_channel() {
    let mut source_manager = ChannelManager::new();
    let mut sink_manager = ChannelManager::new();
    sink_manager
        .register_server(Some(AVDTP_PSM.into()), ClassicChannelSpec { mtu: 128 })
        .unwrap();
    let source_cid = source_manager
        .connect(AVDTP_PSM.into(), ClassicChannelSpec { mtu: 128 })
        .unwrap();
    for _ in 0..32 {
        let count = relay(&mut source_manager, &mut sink_manager)
            + relay(&mut sink_manager, &mut source_manager);
        if count == 0 {
            break;
        }
    }
    let sink_cid = sink_manager.poll_accepted_channel().unwrap();
    let source = L2capMediaTransport::new(source_cid, &source_manager).unwrap();
    let mut sink = L2capMediaTransport::new(sink_cid, &sink_manager).unwrap();

    let encoded = [0x9C, 0x80, 0x08, 0x00, 0, 0, 0, 0, 0, 0].repeat(3);
    let frames = SbcFrame::parse_stream(&encoded).unwrap();
    let packets = packetize_sbc(&frames, usize::from(source.peer_mtu())).unwrap();
    assert_eq!(packets.len(), 1);
    for packet in &packets {
        source.send(&mut source_manager, packet).unwrap();
    }
    relay(&mut source_manager, &mut sink_manager);
    assert_eq!(sink.poll(&mut sink_manager).unwrap(), 1);
    assert_eq!(sink.take_packets(), packets);
}

#[test]
fn transport_rejects_packets_larger_than_the_negotiated_mtu() {
    let mut source_manager = ChannelManager::new();
    let mut sink_manager = ChannelManager::new();
    sink_manager
        .register_server(Some(AVDTP_PSM.into()), ClassicChannelSpec { mtu: 48 })
        .unwrap();
    let source_cid = source_manager
        .connect(AVDTP_PSM.into(), ClassicChannelSpec { mtu: 48 })
        .unwrap();
    for _ in 0..32 {
        let count = relay(&mut source_manager, &mut sink_manager)
            + relay(&mut sink_manager, &mut source_manager);
        if count == 0 {
            break;
        }
    }
    let transport = L2capMediaTransport::new(source_cid, &source_manager).unwrap();
    let packet = bumble_rtp::MediaPacket::new(96, 0, 0, 0, vec![0; 64]);
    assert!(transport.send(&mut source_manager, &packet).is_err());
}
