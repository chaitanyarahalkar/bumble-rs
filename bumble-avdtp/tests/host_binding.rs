use bumble::{Address, AddressType};
use bumble_avdtp::host::DeviceSession;
use bumble_avdtp::session::Session;
use bumble_avdtp::{
    MediaType, Message, ServiceCapabilities, ServiceCategory, State, StreamEndpointType, AVDTP_PSM,
};
use bumble_controller::{Controller, LocalLink};
use bumble_host::{pump, Device};
use bumble_l2cap::ClassicChannelSpec;

fn address(value: &str) -> Address {
    Address::parse(value, AddressType::PUBLIC_DEVICE).unwrap()
}

fn connect_classic(
    link: &mut LocalLink,
    devices: &mut [Device; 2],
    initiator_address: &Address,
    responder_address: &Address,
) {
    devices[0].connect_classic(link, responder_address.clone());
    devices[0].poll(link);
    link.pump_classic();
    devices[1].poll(link);
    devices[1].accept_classic(link, initiator_address.clone());
    devices[1].poll(link);
    link.pump_classic();
    devices[0].poll(link);
}

fn drive(
    link: &mut LocalLink,
    devices: &mut [Device; 2],
    initiator: &mut DeviceSession,
    responder: &mut DeviceSession,
) {
    for _ in 0..64 {
        initiator.poll(link, &mut devices[0]).unwrap();
        responder.poll(link, &mut devices[1]).unwrap();
        pump(link, devices);
    }
}

#[test]
fn avdtp_signaling_runs_over_device_managed_classic_channel() {
    let initiator_address = address("11:11:11:11:11:11");
    let responder_address = address("22:22:22:22:22:22");
    let mut link = LocalLink::new();
    let initiator_id = link.add_controller(Controller::new("A", initiator_address.clone()));
    let responder_id = link.add_controller(Controller::new("B", responder_address.clone()));
    let mut devices = [Device::new(initiator_id), Device::new(responder_id)];
    devices[1]
        .register_classic_channel_server(Some(u32::from(AVDTP_PSM)), ClassicChannelSpec { mtu: 48 })
        .unwrap();
    connect_classic(
        &mut link,
        &mut devices,
        &initiator_address,
        &responder_address,
    );
    let initiator_handle = devices[0].classic_connection_handle().unwrap();
    let responder_handle = devices[1].classic_connection_handle().unwrap();
    let initiator_cid = devices[0]
        .connect_classic_channel(
            &mut link,
            initiator_handle,
            u32::from(AVDTP_PSM),
            ClassicChannelSpec { mtu: 48 },
        )
        .unwrap();
    pump(&mut link, &mut devices);
    let responder_cid = devices[1]
        .take_accepted_classic_channels(responder_handle)
        .into_iter()
        .next()
        .unwrap();

    let capabilities = vec![
        ServiceCapabilities::empty(ServiceCategory::MEDIA_TRANSPORT),
        ServiceCapabilities::MediaCodec {
            media_type: MediaType::AUDIO,
            media_codec_type: 0,
            media_codec_information: (0..60).collect(),
        },
    ];
    let mut responder_state = Session::default();
    let sink = responder_state.add_endpoint(
        MediaType::AUDIO,
        StreamEndpointType::SINK,
        capabilities.clone(),
    );
    let mut initiator = DeviceSession::new(
        &devices[0],
        initiator_handle,
        initiator_cid,
        Session::default(),
    )
    .unwrap();
    let mut responder = DeviceSession::new(
        &devices[1],
        responder_handle,
        responder_cid,
        responder_state,
    )
    .unwrap();

    let discover = initiator
        .send_command(&mut link, &mut devices[0], Message::DiscoverCommand)
        .unwrap();
    drive(&mut link, &mut devices, &mut initiator, &mut responder);
    assert!(matches!(
        initiator.take_response(discover),
        Some(Message::DiscoverResponse { endpoints }) if endpoints[0].seid == sink
    ));

    let configure = initiator
        .send_command(
            &mut link,
            &mut devices[0],
            Message::SetConfigurationCommand {
                acp_seid: sink,
                int_seid: 1,
                capabilities,
            },
        )
        .unwrap();
    drive(&mut link, &mut devices, &mut initiator, &mut responder);
    assert_eq!(
        initiator.take_response(configure),
        Some(Message::SetConfigurationResponse)
    );

    let open = initiator
        .send_command(
            &mut link,
            &mut devices[0],
            Message::OpenCommand { acp_seid: sink },
        )
        .unwrap();
    drive(&mut link, &mut devices, &mut initiator, &mut responder);
    assert_eq!(initiator.take_response(open), Some(Message::OpenResponse));

    let start = initiator
        .send_command(
            &mut link,
            &mut devices[0],
            Message::StartCommand {
                acp_seids: vec![sink],
            },
        )
        .unwrap();
    drive(&mut link, &mut devices, &mut initiator, &mut responder);
    assert_eq!(initiator.take_response(start), Some(Message::StartResponse));
    assert_eq!(
        responder.session().endpoint(sink).unwrap().state,
        State::STREAMING
    );
    assert!(devices[0].take_classic_channel_errors().is_empty());
    assert!(devices[1].take_classic_channel_errors().is_empty());
}
