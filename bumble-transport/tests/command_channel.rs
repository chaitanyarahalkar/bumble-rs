use bumble_hci::{Command, Event, HciPacket, ReturnParameters, HCI_RESET_COMMAND};
use bumble_transport::{
    CommandResponse, Error, HciCommandChannel, PacketSink, PacketSource, Result,
};
use std::collections::VecDeque;

#[derive(Default)]
struct MockTransport {
    inbound: VecDeque<HciPacket>,
    outbound: Vec<HciPacket>,
    flushes: usize,
}

impl PacketSource for MockTransport {
    fn read_packet(&mut self) -> Result<Option<HciPacket>> {
        Ok(self.inbound.pop_front())
    }
}

impl PacketSink for MockTransport {
    fn write_packet(&mut self, packet: &HciPacket) -> Result<()> {
        self.outbound.push(packet.clone());
        Ok(())
    }

    fn flush(&mut self) -> Result<()> {
        self.flushes += 1;
        Ok(())
    }
}

#[test]
fn correlates_command_complete_and_preserves_unrelated_packets() {
    let unrelated = HciPacket::Event(Event::Vendor {
        data: vec![1, 2, 3],
    });
    let wrong_complete = HciPacket::Event(Event::CommandComplete {
        num_hci_command_packets: 1,
        command_opcode: 0x1234,
        return_parameters: ReturnParameters::Raw { data: vec![0] },
    });
    let matching = HciPacket::Event(Event::CommandComplete {
        num_hci_command_packets: 2,
        command_opcode: HCI_RESET_COMMAND,
        return_parameters: ReturnParameters::Status { status: 0 },
    });
    let mut transport = MockTransport::default();
    transport
        .inbound
        .extend([unrelated.clone(), wrong_complete.clone(), matching]);
    let mut channel = HciCommandChannel::new(transport);

    let response = channel.send_command(Command::Reset).unwrap();
    assert_eq!(
        response,
        CommandResponse::Complete {
            num_hci_command_packets: 2,
            return_parameters: ReturnParameters::Status { status: 0 },
        }
    );
    assert_eq!(response.status(), Some(0));
    assert_eq!(
        response.return_parameters(),
        Some(&ReturnParameters::Status { status: 0 })
    );
    assert_eq!(channel.take_pending_packets(), [unrelated, wrong_complete]);
    let (transport, pending) = channel.into_parts();
    assert!(pending.is_empty());
    assert_eq!(transport.outbound, [HciPacket::Command(Command::Reset)]);
    assert_eq!(transport.flushes, 1);
}

#[test]
fn returns_matching_command_status() {
    let mut transport = MockTransport::default();
    transport
        .inbound
        .push_back(HciPacket::Event(Event::CommandStatus {
            status: 0x0C,
            num_hci_command_packets: 3,
            command_opcode: HCI_RESET_COMMAND,
        }));
    let mut channel = HciCommandChannel::new(transport);
    let response = channel.send_command(Command::Reset).unwrap();
    assert_eq!(
        response,
        CommandResponse::Status {
            status: 0x0C,
            num_hci_command_packets: 3,
        }
    );
    assert_eq!(response.status(), Some(0x0C));
    assert_eq!(response.return_parameters(), None);
}

#[test]
fn reports_eof_before_the_matching_response() {
    let mut channel = HciCommandChannel::new(MockTransport::default());
    assert!(matches!(
        channel.send_command(Command::Reset),
        Err(Error::Remote(message)) if message.contains("0x0c03")
    ));
}
