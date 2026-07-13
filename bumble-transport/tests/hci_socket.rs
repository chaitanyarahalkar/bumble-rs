use bumble_hci::HciPacket;
use bumble_transport::{
    Error, HciSocketAddress, HciSocketIo, HciSocketSpec, HciSocketTransport, PacketSink,
    PacketSource, HCI_CHANNEL_USER,
};
use std::collections::VecDeque;
use std::io;

#[derive(Default)]
struct MockSocket {
    reads: VecDeque<io::Result<Vec<u8>>>,
    writes: Vec<Vec<u8>>,
    next_send_size: Option<usize>,
}

impl HciSocketIo for MockSocket {
    fn recv(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        match self.reads.pop_front() {
            Some(Ok(bytes)) => {
                assert!(bytes.len() <= buffer.len());
                buffer[..bytes.len()].copy_from_slice(&bytes);
                Ok(bytes.len())
            }
            Some(Err(error)) => Err(error),
            None => Ok(0),
        }
    }

    fn send(&mut self, packet: &[u8]) -> io::Result<usize> {
        self.writes.push(packet.to_vec());
        Ok(self.next_send_size.take().unwrap_or(packet.len()))
    }
}

fn reset_command() -> HciPacket {
    HciPacket::from_bytes(&[0x01, 0x03, 0x0c, 0x00]).unwrap()
}

fn command_complete() -> HciPacket {
    HciPacket::from_bytes(&[0x04, 0x0e, 0x04, 0x01, 0x03, 0x0c, 0x00]).unwrap()
}

#[test]
fn adapter_spec_matches_upstream_default_and_integer_forms() {
    assert_eq!(HciSocketSpec::parse(None).unwrap().adapter_index, 0);
    assert_eq!(HciSocketSpec::parse(Some("")).unwrap().adapter_index, 0);
    assert_eq!(
        HciSocketSpec::parse(Some(" 42 ")).unwrap().adapter_index,
        42
    );
    assert!(matches!(
        HciSocketSpec::parse(Some("-1")),
        Err(Error::InvalidSpec(_))
    ));
    assert!(matches!(
        HciSocketSpec::parse(Some("65536")),
        Err(Error::InvalidSpec(_))
    ));
}

#[test]
fn user_channel_address_has_exact_linux_sockaddr_hci_layout() {
    let address = HciSocketAddress::user_channel(0x1234);
    assert_eq!(address.family, 31);
    assert_eq!(address.adapter_index, 0x1234);
    assert_eq!(address.channel, HCI_CHANNEL_USER);
    assert_eq!(
        address.to_ne_bytes(),
        [
            31u16.to_ne_bytes(),
            0x1234u16.to_ne_bytes(),
            HCI_CHANNEL_USER.to_ne_bytes(),
        ]
        .concat()
        .as_slice()
    );
}

#[test]
fn packet_io_handles_fragmented_and_coalesced_h4() {
    let command = reset_command();
    let event = command_complete();
    let bytes = [command.to_bytes(), event.to_bytes()].concat();
    let mut socket = MockSocket::default();
    socket.reads.push_back(Ok(bytes[..2].to_vec()));
    socket.reads.push_back(Ok(bytes[2..].to_vec()));
    let mut transport = HciSocketTransport::from_io(socket, 7);

    assert_eq!(transport.adapter_index(), 7);
    assert_eq!(transport.read_packet().unwrap(), Some(command));
    assert_eq!(transport.read_packet().unwrap(), Some(event));
    assert_eq!(transport.read_packet().unwrap(), None);
}

#[test]
fn packet_io_preserves_complete_writes_and_rejects_partial_send() {
    let command = reset_command();
    let mut transport = HciSocketTransport::from_io(MockSocket::default(), 0);
    transport.write_packet(&command).unwrap();
    assert_eq!(transport.get_ref().writes, [command.to_bytes()]);

    transport.get_mut().next_send_size = Some(2);
    assert!(
        matches!(transport.write_packet(&command), Err(Error::Io(error)) if error.kind() == io::ErrorKind::WriteZero)
    );
}

#[test]
fn packet_io_propagates_receive_errors_and_truncated_eof() {
    let mut failed = MockSocket::default();
    failed
        .reads
        .push_back(Err(io::Error::new(io::ErrorKind::BrokenPipe, "closed")));
    let mut transport = HciSocketTransport::from_io(failed, 0);
    assert!(
        matches!(transport.read_packet(), Err(Error::Io(error)) if error.kind() == io::ErrorKind::BrokenPipe)
    );

    let mut truncated = MockSocket::default();
    truncated.reads.push_back(Ok(vec![0x04, 0x0e]));
    let mut transport = HciSocketTransport::from_io(truncated, 0);
    assert!(matches!(
        transport.read_packet(),
        Err(Error::TruncatedPacket(2))
    ));
}
