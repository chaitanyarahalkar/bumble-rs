use bumble_hci::HciPacket;
use bumble_transport::{
    Error, FileTransport, H4Transport, PacketFramer, PacketLayout, PacketSink, PacketSource,
    TcpServer, TcpTransport, UdpTransport,
};
use std::fs;
use std::io::Cursor;
use std::net::UdpSocket;
use std::time::{SystemTime, UNIX_EPOCH};

fn packets() -> Vec<HciPacket> {
    [
        vec![0x01, 0x03, 0x0c, 0x00],
        vec![0x02, 0x01, 0x20, 0x03, 0x00, 0xaa, 0xbb, 0xcc],
        vec![0x03, 0x01, 0x00, 0x02, 0x11, 0x22],
        vec![0x04, 0x0f, 0x04, 0x00, 0x01, 0x03, 0x0c],
        vec![0x05, 0x01, 0x10, 0x02, 0x00, 0x33, 0x44],
    ]
    .iter()
    .map(|bytes| HciPacket::from_bytes(bytes).unwrap())
    .collect()
}

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

#[test]
fn framer_accepts_every_fragment_boundary_and_coalesced_packets() {
    let expected = packets();
    let bytes: Vec<u8> = expected.iter().flat_map(HciPacket::to_bytes).collect();

    for split in 0..=bytes.len() {
        let mut framer = PacketFramer::new();
        let mut actual = framer.feed(&bytes[..split]).unwrap();
        actual.extend(framer.feed(&bytes[split..]).unwrap());
        assert_eq!(actual, expected, "split {split}");
        assert!(framer.is_empty());
    }
}

#[test]
fn framer_supports_extended_packet_layouts_and_rejects_unknown_types() {
    let mut framer = PacketFramer::new();
    assert!(matches!(
        framer.feed(&[0xff, 0x00]),
        Err(Error::InvalidPacketType(0xff))
    ));
    assert!(framer.is_empty());

    framer
        .register_layout(0xff, PacketLayout::new(1, 0))
        .unwrap();
    let packet = framer.feed(&[0xff, 0x03, 1, 2, 3]).unwrap().remove(0);
    assert_eq!(packet.to_bytes(), [0xff, 0x03, 1, 2, 3]);
}

#[test]
fn stream_transport_detects_clean_and_truncated_eof() {
    let expected = packets();
    let bytes: Vec<u8> = expected.iter().flat_map(HciPacket::to_bytes).collect();
    let mut transport = H4Transport::new(Cursor::new(bytes));
    for packet in expected {
        assert_eq!(transport.read_packet().unwrap(), Some(packet));
    }
    assert_eq!(transport.read_packet().unwrap(), None);

    let mut truncated = H4Transport::new(Cursor::new(vec![0x02, 1, 0, 4, 0, 1]));
    assert!(matches!(
        truncated.read_packet(),
        Err(Error::TruncatedPacket(6))
    ));
}

#[test]
fn file_transport_reads_and_writes_h4_packets() {
    let path = temporary_path("file");
    let expected = packets();
    fs::write(&path, expected[0].to_bytes()).unwrap();

    {
        let mut transport = FileTransport::open(&path).unwrap();
        assert_eq!(transport.read_packet().unwrap(), Some(expected[0].clone()));
        transport.write_packet(&expected[1]).unwrap();
        transport.flush().unwrap();
    }

    let bytes = fs::read(&path).unwrap();
    assert_eq!(
        bytes,
        [expected[0].to_bytes(), expected[1].to_bytes()].concat()
    );
    fs::remove_file(path).unwrap();
}

#[test]
fn tcp_client_and_server_exchange_packets() {
    let server = TcpServer::bind("127.0.0.1:0").unwrap();
    let address = server.local_addr().unwrap();
    let mut client = TcpTransport::connect(address).unwrap();
    let mut accepted = server.accept().unwrap();
    let expected = packets();

    client.write_packet(&expected[0]).unwrap();
    assert_eq!(accepted.read_packet().unwrap(), Some(expected[0].clone()));
    accepted.write_packet(&expected[1]).unwrap();
    assert_eq!(client.read_packet().unwrap(), Some(expected[1].clone()));
    assert_eq!(client.peer_addr().unwrap(), address);
}

#[test]
fn split_tcp_transport_exchanges_packets_in_both_directions() {
    let server = TcpServer::bind("127.0.0.1:0").unwrap();
    let address = server.local_addr().unwrap();
    let client = TcpTransport::connect(address).unwrap();
    let accepted = server.accept().unwrap();
    let (mut client_source, mut client_sink) = client.try_split().unwrap();
    let (mut server_source, mut server_sink) = accepted.try_split().unwrap();
    let expected = packets();

    client_sink.write_packet(&expected[0]).unwrap();
    assert_eq!(
        server_source.read_packet().unwrap(),
        Some(expected[0].clone())
    );
    server_sink.write_packet(&expected[1]).unwrap();
    assert_eq!(
        client_source.read_packet().unwrap(),
        Some(expected[1].clone())
    );
}

#[test]
fn udp_transport_parses_coalesced_datagrams_and_replies() {
    let first_socket = UdpSocket::bind("127.0.0.1:0").unwrap();
    let second_socket = UdpSocket::bind("127.0.0.1:0").unwrap();
    let first_address = first_socket.local_addr().unwrap();
    let second_address = second_socket.local_addr().unwrap();
    first_socket.connect(second_address).unwrap();
    second_socket.connect(first_address).unwrap();
    let mut first = UdpTransport::from_socket(first_socket);
    let mut second = UdpTransport::from_socket(second_socket);
    let expected = packets();

    let coalesced = [expected[0].to_bytes(), expected[1].to_bytes()].concat();
    first.get_socket().send(&coalesced).unwrap();
    assert_eq!(second.read_packet().unwrap(), Some(expected[0].clone()));
    assert_eq!(second.read_packet().unwrap(), Some(expected[1].clone()));
    second.write_packet(&expected[3]).unwrap();
    assert_eq!(first.read_packet().unwrap(), Some(expected[3].clone()));
}

#[cfg(unix)]
#[test]
fn unix_client_and_server_exchange_packets() {
    use bumble_transport::{UnixServer, UnixTransport};

    let path = temporary_path("unix");
    let server = UnixServer::bind(&path).unwrap();
    let mut client = UnixTransport::connect(&path).unwrap();
    let mut accepted = server.accept().unwrap();
    let expected = packets();

    client.write_packet(&expected[0]).unwrap();
    assert_eq!(accepted.read_packet().unwrap(), Some(expected[0].clone()));
    accepted.write_packet(&expected[1]).unwrap();
    assert_eq!(client.read_packet().unwrap(), Some(expected[1].clone()));
    fs::remove_file(path).unwrap();
}
