use bumble_a2dp::profile::A2dpClient;
use bumble_a2dp::{
    MediaCodecInformation, SbcAllocationMethod, SbcBlockLength, SbcChannelMode,
    SbcMediaCodecInformation, SbcSamplingFrequency, SbcSubbands,
};
use bumble_avdtp::l2cap::L2capSession;
use bumble_avdtp::session::Session;
use bumble_avdtp::{
    MediaType, ServiceCapabilities, ServiceCategory, State, StreamEndpointType, AVDTP_PSM,
};
use bumble_l2cap::{ChannelManager, ClassicChannelSpec};

fn relay(left: &mut ChannelManager, right: &mut ChannelManager) -> usize {
    let mut count = 0;
    while let Some(pdu) = left.poll_outbound() {
        right.process_pdu(pdu).unwrap();
        count += 1;
    }
    count
}

fn sbc() -> MediaCodecInformation {
    MediaCodecInformation::Sbc(SbcMediaCodecInformation {
        sampling_frequency: SbcSamplingFrequency::SF_44100,
        channel_mode: SbcChannelMode::JOINT_STEREO,
        block_length: SbcBlockLength::BL_16,
        subbands: SbcSubbands::S_8,
        allocation_method: SbcAllocationMethod::LOUDNESS,
        minimum_bitpool_value: 2,
        maximum_bitpool_value: 53,
    })
}

#[test]
fn discovers_selects_and_drives_remote_sink_lifecycle() {
    let mut client_manager = ChannelManager::new();
    let mut server_manager = ChannelManager::new();
    server_manager
        .register_server(Some(AVDTP_PSM.into()), ClassicChannelSpec { mtu: 128 })
        .unwrap();
    let client_cid = client_manager
        .connect(AVDTP_PSM.into(), ClassicChannelSpec { mtu: 128 })
        .unwrap();
    for _ in 0..32 {
        let count = relay(&mut client_manager, &mut server_manager)
            + relay(&mut server_manager, &mut client_manager);
        if count == 0 {
            break;
        }
    }
    let server_cid = server_manager.poll_accepted_channel().unwrap();

    let codec = sbc();
    let mut responder = Session::default();
    let sink_seid = responder.add_endpoint(
        MediaType::AUDIO,
        StreamEndpointType::SINK,
        vec![
            ServiceCapabilities::empty(ServiceCategory::MEDIA_TRANSPORT),
            codec.to_avdtp_capability().unwrap(),
        ],
    );
    let mut client_signaling =
        L2capSession::new(client_cid, &client_manager, Session::default()).unwrap();
    let server_signaling = Rc::new(RefCell::new(
        L2capSession::new(server_cid, &server_manager, responder).unwrap(),
    ));
    let driven_server = Rc::clone(&server_signaling);

    let drive = |client_manager: &mut ChannelManager,
                 client: &mut L2capSession|
     -> bumble_a2dp::profile::Result<()> {
        relay(client_manager, &mut server_manager);
        driven_server
            .borrow_mut()
            .poll(&mut server_manager)
            .map_err(bumble_a2dp::profile::Error::from)?;
        relay(&mut server_manager, client_manager);
        client
            .poll(client_manager)
            .map_err(bumble_a2dp::profile::Error::from)?;
        Ok(())
    };
    let mut client = A2dpClient::new(&mut client_manager, &mut client_signaling, drive);
    let endpoints = client.discover().unwrap();
    let sink = client
        .find_compatible_sink(&endpoints, &codec)
        .unwrap()
        .unwrap();
    assert_eq!(sink.info.seid, sink_seid);
    let stream = client.configure_open_start(1, sink, &codec).unwrap();
    assert_eq!(
        server_signaling
            .borrow()
            .session()
            .endpoint(sink_seid)
            .unwrap()
            .state,
        State::STREAMING
    );
    client.suspend(stream).unwrap();
    assert_eq!(
        server_signaling
            .borrow()
            .session()
            .endpoint(sink_seid)
            .unwrap()
            .state,
        State::OPEN
    );
    client.start(stream).unwrap();
    client.close(stream).unwrap();
    assert_eq!(
        server_signaling
            .borrow()
            .session()
            .endpoint(sink_seid)
            .unwrap()
            .state,
        State::IDLE
    );
}
use std::cell::RefCell;
use std::rc::Rc;
