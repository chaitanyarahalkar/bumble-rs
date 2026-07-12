use bumble_avdtp::l2cap::L2capSession;
use bumble_avdtp::session::Session;
use bumble_avdtp::{
    MediaType, Message, ServiceCapabilities, ServiceCategory, State, StreamEndpointType, AVDTP_PSM,
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

fn drive(
    client_manager: &mut ChannelManager,
    client: &mut L2capSession,
    server_manager: &mut ChannelManager,
    server: &mut L2capSession,
) {
    for _ in 0..128 {
        let mut count = relay(client_manager, server_manager);
        count += relay(server_manager, client_manager);
        count += client.poll(client_manager).unwrap();
        count += server.poll(server_manager).unwrap();
        if count == 0 {
            return;
        }
    }
    panic!("AVDTP/L2CAP stack did not quiesce");
}

fn request(
    message: Message,
    client_manager: &mut ChannelManager,
    client: &mut L2capSession,
    server_manager: &mut ChannelManager,
    server: &mut L2capSession,
) -> Message {
    let label = client.send_command(client_manager, message).unwrap();
    drive(client_manager, client, server_manager, server);
    client.take_response(label).expect("AVDTP response")
}

#[test]
fn signaling_and_stream_lifecycle_run_over_classic_l2cap() {
    let mut client_manager = ChannelManager::new();
    let mut server_manager = ChannelManager::new();
    server_manager
        .register_server(Some(AVDTP_PSM.into()), ClassicChannelSpec { mtu: 48 })
        .unwrap();
    let client_cid = client_manager
        .connect(AVDTP_PSM.into(), ClassicChannelSpec { mtu: 48 })
        .unwrap();
    for _ in 0..32 {
        let count = relay(&mut client_manager, &mut server_manager)
            + relay(&mut server_manager, &mut client_manager);
        if count == 0 {
            break;
        }
    }
    let server_cid = server_manager.poll_accepted_channel().unwrap();

    let mut server_session = Session::default();
    let capabilities = vec![
        ServiceCapabilities::empty(ServiceCategory::MEDIA_TRANSPORT),
        ServiceCapabilities::MediaCodec {
            media_type: MediaType::AUDIO,
            media_codec_type: 0,
            media_codec_information: (0..60).collect(),
        },
        ServiceCapabilities::empty(ServiceCategory::DELAY_REPORTING),
    ];
    let sink = server_session.add_endpoint(
        MediaType::AUDIO,
        StreamEndpointType::SINK,
        capabilities.clone(),
    );
    let mut client = L2capSession::new(client_cid, &client_manager, Session::default()).unwrap();
    let mut server = L2capSession::new(server_cid, &server_manager, server_session).unwrap();

    assert!(matches!(
        request(
            Message::DiscoverCommand,
            &mut client_manager,
            &mut client,
            &mut server_manager,
            &mut server,
        ),
        Message::DiscoverResponse { endpoints } if endpoints.len() == 1 && endpoints[0].seid == sink
    ));
    assert_eq!(
        request(
            Message::GetAllCapabilitiesCommand { acp_seid: sink },
            &mut client_manager,
            &mut client,
            &mut server_manager,
            &mut server,
        ),
        Message::GetAllCapabilitiesResponse {
            capabilities: capabilities.clone()
        }
    );

    // This command exceeds the 48-byte minimum Classic MTU and therefore exercises AVDTP
    // fragmentation and reassembly through live L2CAP signaling.
    assert_eq!(
        request(
            Message::SetConfigurationCommand {
                acp_seid: sink,
                int_seid: 1,
                capabilities,
            },
            &mut client_manager,
            &mut client,
            &mut server_manager,
            &mut server,
        ),
        Message::SetConfigurationResponse
    );
    for (command, response, state) in [
        (
            Message::OpenCommand { acp_seid: sink },
            Message::OpenResponse,
            State::OPEN,
        ),
        (
            Message::StartCommand {
                acp_seids: vec![sink],
            },
            Message::StartResponse,
            State::STREAMING,
        ),
        (
            Message::SuspendCommand {
                acp_seids: vec![sink],
            },
            Message::SuspendResponse,
            State::OPEN,
        ),
        (
            Message::CloseCommand { acp_seid: sink },
            Message::CloseResponse,
            State::IDLE,
        ),
    ] {
        assert_eq!(
            request(
                command,
                &mut client_manager,
                &mut client,
                &mut server_manager,
                &mut server,
            ),
            response
        );
        assert_eq!(server.session().endpoint(sink).unwrap().state, state);
    }
}
