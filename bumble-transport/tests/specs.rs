use bumble_hci::HciPacket;
use bumble_transport::{
    open_transport, Error, PacketSink, PacketSource, SerialConfig, TransportSpec,
    DEFAULT_POST_OPEN_DELAY, DEFAULT_SERIAL_SPEED,
};
use std::collections::BTreeMap;
use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

fn temporary_path(name: &str) -> std::path::PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "bumble-transport-{name}-{}-{nonce}",
        std::process::id()
    ))
}

fn reset_packet() -> HciPacket {
    HciPacket::from_bytes(&[0x01, 0x03, 0x0c, 0x00]).unwrap()
}

#[test]
fn transport_spec_parses_upstream_metadata_forms() {
    assert_eq!(
        TransportSpec::parse("usb:[driver=rtk,role=controller,]0").unwrap(),
        TransportSpec {
            scheme: "usb".into(),
            parameters: Some("0".into()),
            metadata: BTreeMap::from([
                ("driver".into(), "rtk".into()),
                ("role".into(), "controller".into()),
            ]),
        }
    );
    assert_eq!(
        TransportSpec::parse("android-netsim").unwrap(),
        TransportSpec {
            scheme: "android-netsim".into(),
            parameters: None,
            metadata: BTreeMap::new(),
        }
    );
    assert!(matches!(
        TransportSpec::parse("tcp-client:[broken=]localhost:1"),
        Err(Error::InvalidSpec(_))
    ));

    for (name, parameters) in [
        (
            "android-netsim:[::1]:8554,mode=host[a=b,c=d]",
            "[::1]:8554,mode=host",
        ),
        (
            "android-netsim:localhost:8554,mode=host[a=b,c=d]",
            "localhost:8554,mode=host",
        ),
        (
            "android-netsim:[a=b,c=d][::1]:8554,mode=host",
            "[::1]:8554,mode=host",
        ),
        (
            "android-netsim:[a=b,c=d]localhost:8554,mode=host",
            "localhost:8554,mode=host",
        ),
    ] {
        let parsed = TransportSpec::parse(name).unwrap();
        assert_eq!(parsed.parameters.as_deref(), Some(parameters));
        assert_eq!(parsed.metadata["a"], "b");
        assert_eq!(parsed.metadata["c"], "d");
    }
}

#[test]
fn serial_config_matches_bumble_defaults_and_flags() {
    assert_eq!(
        SerialConfig::parse("/dev/tty.example").unwrap(),
        SerialConfig {
            device: "/dev/tty.example".into(),
            speed: DEFAULT_SERIAL_SPEED,
            rtscts: false,
            dsrdtr: false,
            post_open_delay: std::time::Duration::ZERO,
        }
    );
    assert_eq!(
        SerialConfig::parse("/dev/tty.example,115200,rtscts,dsrdtr,delay").unwrap(),
        SerialConfig {
            device: "/dev/tty.example".into(),
            speed: 115_200,
            rtscts: true,
            dsrdtr: true,
            post_open_delay: DEFAULT_POST_OPEN_DELAY,
        }
    );
}

#[test]
fn dispatcher_opens_file_and_preserves_metadata() {
    let path = temporary_path("dispatch-file");
    let packet = reset_packet();
    fs::write(&path, packet.to_bytes()).unwrap();
    let mut opened =
        open_transport(&format!("file:[direction=controller]{}", path.display())).unwrap();
    assert_eq!(opened.metadata["direction"], "controller");
    assert_eq!(opened.read_packet().unwrap(), Some(packet));
    fs::remove_file(path).unwrap();
}

#[test]
fn dispatcher_rejects_unknown_or_incomplete_schemes() {
    assert!(matches!(
        open_transport("made-up:value"),
        Err(Error::Unsupported(_))
    ));
    assert!(matches!(
        open_transport("tcp-client"),
        Err(Error::InvalidSpec(_))
    ));
}

#[cfg(unix)]
#[test]
fn pty_transport_is_raw_bidirectional_and_cleans_symlink() {
    use bumble_transport::PtyTransport;
    use std::io::{Read, Write};

    let link = temporary_path("pty-link");
    let packet = reset_packet();
    let mut transport = PtyTransport::open(Some(&link)).unwrap();
    assert!(link.is_symlink());
    assert_eq!(fs::read_link(&link).unwrap(), transport.replica_path());

    transport
        .replica_mut()
        .write_all(&packet.to_bytes())
        .unwrap();
    assert_eq!(transport.read_packet().unwrap(), Some(packet.clone()));
    transport.write_packet(&packet).unwrap();
    let mut bytes = vec![0; packet.to_bytes().len()];
    transport.replica_mut().read_exact(&mut bytes).unwrap();
    assert_eq!(bytes, packet.to_bytes());

    drop(transport);
    assert!(!link.exists());
}

#[cfg(unix)]
#[test]
fn serial_dispatch_talks_to_a_real_pseudo_terminal() {
    use bumble_transport::PtyTransport;

    let packet = reset_packet();
    let mut pty = PtyTransport::open(None::<&std::path::Path>).unwrap();
    let mut serial = open_transport(&format!("serial:{},0", pty.replica_path().display())).unwrap();

    serial.write_packet(&packet).unwrap();
    assert_eq!(pty.read_packet().unwrap(), Some(packet.clone()));
    pty.write_packet(&packet).unwrap();
    assert_eq!(serial.read_packet().unwrap(), Some(packet));
}
