use bumble_hci::HciPacket;
use bumble_transport::android_netsim_proto as proto;
use bumble_transport::{
    find_netsim_grpc_port, find_netsim_grpc_port_in, netsim_ini_file_name, open_transport,
    AndroidNetsimMode, AndroidNetsimSpec, Error, PacketSource, DEFAULT_ANDROID_NETSIM_NAME,
};
use prost::Message;
use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

fn reset_command() -> HciPacket {
    HciPacket::from_bytes(&[0x01, 0x03, 0x0c, 0x00]).unwrap()
}

fn command_complete() -> HciPacket {
    HciPacket::from_bytes(&[0x04, 0x0e, 0x04, 0x01, 0x03, 0x0c, 0x00]).unwrap()
}

fn temporary_directory() -> std::path::PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "bumble-android-netsim-{}-{nonce}",
        std::process::id()
    ))
}

#[test]
fn netsim_spec_matches_upstream_host_controller_and_option_forms() {
    assert_eq!(
        AndroidNetsimSpec::parse(None).unwrap().name(),
        DEFAULT_ANDROID_NETSIM_NAME
    );
    assert_eq!(
        AndroidNetsimSpec::parse(None).unwrap().mode,
        AndroidNetsimMode::Host
    );

    let host =
        AndroidNetsimSpec::parse(Some("[::1]:8555,name=bumble1,variant=keyboard,instance=3"))
            .unwrap();
    assert_eq!(host.host.as_deref(), Some("[::1]"));
    assert_eq!(host.port, 8555);
    assert_eq!(host.name(), "bumble1");
    assert_eq!(host.variant(), "keyboard");
    assert_eq!(host.instance, 3);

    let controller = AndroidNetsimSpec::parse(Some("_:0,mode=controller,instance=2")).unwrap();
    assert_eq!(controller.host.as_deref(), Some("_"));
    assert_eq!(controller.port, 0);
    assert_eq!(controller.mode, AndroidNetsimMode::Controller);
    assert_eq!(controller.instance, 2);

    assert!(matches!(
        AndroidNetsimSpec::parse(Some("mode=controller")),
        Err(Error::InvalidSpec(_))
    ));
    assert!(matches!(
        AndroidNetsimSpec::parse(Some("localhost:bad")),
        Err(Error::InvalidSpec(_))
    ));
    assert!(matches!(
        AndroidNetsimSpec::parse(Some("localhost:8555,broken")),
        Err(Error::InvalidSpec(_))
    ));
}

#[test]
fn netsim_ini_discovery_uses_instance_suffix_and_grpc_port_key() {
    let directory = temporary_directory();
    fs::create_dir(&directory).unwrap();
    assert_eq!(netsim_ini_file_name(0), "netsim.ini");
    assert_eq!(netsim_ini_file_name(7), "netsim_7.ini");
    assert_eq!(find_netsim_grpc_port_in(&directory, 7).unwrap(), None);

    fs::write(
        directory.join(netsim_ini_file_name(7)),
        "other=value\ngrpc.port=8877\n",
    )
    .unwrap();
    assert_eq!(find_netsim_grpc_port_in(&directory, 7).unwrap(), Some(8877));
    fs::write(
        directory.join(netsim_ini_file_name(8)),
        "grpc.port=invalid\n",
    )
    .unwrap();
    assert!(matches!(
        find_netsim_grpc_port_in(&directory, 8),
        Err(Error::InvalidSpec(_))
    ));
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn netsim_packet_request_and_response_match_upstream_wire_tags() {
    let hci = proto::packet::HciPacket {
        packet_type: 1,
        packet: vec![0x03, 0x0c, 0x00],
    };
    let request = proto::packet::PacketRequest {
        request_type: Some(proto::packet::packet_request::RequestType::HciPacket(
            hci.clone(),
        )),
    };
    let response = proto::packet::PacketResponse {
        response_type: Some(proto::packet::packet_response::ResponseType::HciPacket(hci)),
    };
    let expected = [0x12, 0x07, 0x08, 0x01, 0x12, 0x03, 0x03, 0x0c, 0x00];
    assert_eq!(request.encode_to_vec(), expected);
    assert_eq!(response.encode_to_vec(), expected);

    let error = proto::packet::PacketResponse {
        response_type: Some(proto::packet::packet_response::ResponseType::Error(
            "busy".into(),
        )),
    };
    assert_eq!(error.encode_to_vec(), [0x0a, 0x04, b'b', b'u', b's', b'y']);
}

#[test]
fn real_netsim_host_controller_exchange_and_exclusive_lease() {
    let instance = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .subsec_nanos();
    let controller = open_transport(&format!(
        "android-netsim:127.0.0.1:0,mode=controller,instance={instance}"
    ))
    .unwrap();
    assert_eq!(controller.metadata["mode"], "controller");
    let port = controller.metadata["port"].parse::<u16>().unwrap();
    assert_ne!(port, 0);
    let mut controller = controller.try_split().unwrap();

    let host_name = if find_netsim_grpc_port(instance).unwrap() == Some(port) {
        format!("android-netsim:instance={instance},name=primary")
    } else {
        format!("android-netsim:127.0.0.1:{port},mode=host,name=primary")
    };
    let host = open_transport(&host_name).unwrap();
    assert_eq!(host.metadata["mode"], "host");
    let mut host = host.try_split().unwrap();
    let command = reset_command();
    host.sink.write_packet(&command).unwrap();
    assert_eq!(controller.source.read_packet().unwrap(), Some(command));

    let event = command_complete();
    controller.sink.write_packet(&event).unwrap();
    assert_eq!(host.source.read_packet().unwrap(), Some(event));

    let mut second = open_transport(&format!(
        "android-netsim:127.0.0.1:{port},mode=host,name=secondary"
    ))
    .unwrap();
    assert!(matches!(
        second.read_packet(),
        Err(Error::Remote(message)) if message == "Device busy"
    ));

    // Controller shutdown must abort active streaming RPCs rather than wait
    // indefinitely for connected hosts to close first.
    drop(controller);
}
