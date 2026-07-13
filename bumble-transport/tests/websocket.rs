use bumble_hci::HciPacket;
use bumble_transport::{
    open_transport, PacketSink, PacketSource, WebSocketServer, WebSocketTransport,
};

fn packets() -> Vec<HciPacket> {
    [
        vec![0x01, 0x03, 0x0c, 0x00],
        vec![0x04, 0x0f, 0x04, 0x00, 0x01, 0x03, 0x0c],
    ]
    .iter()
    .map(|bytes| HciPacket::from_bytes(bytes).unwrap())
    .collect()
}

#[test]
fn websocket_client_server_carry_binary_hci_and_ignore_text() {
    let server = WebSocketServer::bind("127.0.0.1:0").unwrap();
    let address = server.local_addr().unwrap();
    let expected = packets();
    let server_expected = expected.clone();
    let server_thread = std::thread::spawn(move || {
        let mut accepted = server.accept().unwrap();
        assert_eq!(
            accepted.read_packet().unwrap(),
            Some(server_expected[0].clone())
        );
        accepted.write_text("not an HCI packet").unwrap();
        accepted
            .write_binary([server_expected[0].to_bytes(), server_expected[1].to_bytes()].concat())
            .unwrap();
        accepted.flush().unwrap();
    });

    let mut client = WebSocketTransport::connect(&format!("ws://{address}/hci")).unwrap();
    client.write_packet(&expected[0]).unwrap();
    assert_eq!(client.read_packet().unwrap(), Some(expected[0].clone()));
    assert_eq!(client.read_packet().unwrap(), Some(expected[1].clone()));
    server_thread.join().unwrap();
}

#[test]
fn websocket_client_dispatch_preserves_metadata() {
    let server = WebSocketServer::bind("127.0.0.1:0").unwrap();
    let address = server.local_addr().unwrap();
    let expected = packets().remove(0);
    let server_expected = expected.clone();
    let server_thread = std::thread::spawn(move || {
        let mut accepted = server.accept().unwrap();
        assert_eq!(accepted.read_packet().unwrap(), Some(server_expected));
    });

    let mut client =
        open_transport(&format!("ws-client:[role=host]ws://{address}/controller")).unwrap();
    assert_eq!(client.metadata["role"], "host");
    client.write_packet(&expected).unwrap();
    server_thread.join().unwrap();
}

#[test]
fn websocket_split_halves_exchange_packets_in_both_directions() {
    let server = WebSocketServer::bind("127.0.0.1:0").unwrap();
    let address = server.local_addr().unwrap();
    let expected = packets();
    let server_expected = expected.clone();
    let server_thread = std::thread::spawn(move || {
        let accepted = server.accept().unwrap();
        let (mut source, mut sink) = accepted.try_split().unwrap();
        assert_eq!(
            source.read_packet().unwrap(),
            Some(server_expected[0].clone())
        );
        sink.write_packet(&server_expected[1]).unwrap();
        sink.flush().unwrap();
    });

    let client = WebSocketTransport::connect(&format!("ws://{address}/hci")).unwrap();
    let (mut source, mut sink) = client.try_split().unwrap();
    sink.write_packet(&expected[0]).unwrap();
    sink.flush().unwrap();
    assert_eq!(source.read_packet().unwrap(), Some(expected[1].clone()));
    server_thread.join().unwrap();
}
