use bumble::{Address, AddressType};
use bumble_controller::{Controller, LocalLink};
use bumble_hci::{
    AclDataPacket, Command, Event, HciPacket, IsoDataPacket, HCI_REMOTE_NAME_REQUEST_COMMAND,
};
use bumble_host::{pump, Device, DeviceEvent, HostTransport, RemoteNameError, RemoteNameResult};

#[derive(Default)]
struct EventTransport {
    events: Vec<HciPacket>,
    commands: Vec<(usize, Command)>,
}

impl HostTransport for EventTransport {
    fn handle_command(&mut self, controller_id: usize, command: Command) {
        self.commands.push((controller_id, command));
    }

    fn send_acl_packet(&mut self, _controller_id: usize, _packet: AclDataPacket) -> bool {
        false
    }

    fn send_synchronous_data(
        &mut self,
        _controller_id: usize,
        _connection_handle: u16,
        _packet_status: u8,
        _data: &[u8],
    ) -> bool {
        false
    }

    fn send_iso_packet(&mut self, _controller_id: usize, _packet: IsoDataPacket) -> bool {
        false
    }

    fn drain_host_events(&mut self, _controller_id: usize) -> Vec<HciPacket> {
        core::mem::take(&mut self.events)
    }
}

fn address(value: &str) -> Address {
    Address::parse(value, AddressType::PUBLIC_DEVICE).unwrap()
}

fn remote_name(name: &[u8]) -> [u8; 248] {
    let mut bytes = [0; 248];
    bytes[..name.len()].copy_from_slice(name);
    bytes
}

#[test]
fn address_remote_name_request_runs_over_the_live_classic_link() {
    let requester_address = address("10:20:30:40:50:01");
    let peer_address = address("10:20:30:40:50:02");
    let mut link = LocalLink::new();
    let requester_id = link.add_controller(Controller::new("Requester", requester_address));
    let peer_id = link.add_controller(Controller::new("An Awesome Name", peer_address.clone()));
    let mut devices = [Device::new(requester_id), Device::new(peer_id)];

    devices[0].request_remote_name(&mut link, peer_address.clone());
    pump(&mut link, &mut devices);

    assert_eq!(devices[0].pending_remote_name_count(), 0);
    assert_eq!(
        devices[0].take_classic_remote_name_results(),
        vec![RemoteNameResult {
            peer_address,
            result: Ok("An Awesome Name".into()),
        }]
    );
}

#[test]
fn connection_remote_name_request_retains_the_utf8_peer_name() {
    let handle = 0x0040;
    let peer = address("10:20:30:40:50:60");
    let mut transport = EventTransport {
        events: vec![HciPacket::Event(Event::ConnectionComplete {
            status: 0,
            connection_handle: handle,
            bd_addr: peer.clone(),
            link_type: 1,
            encryption_enabled: 0,
        })],
        ..EventTransport::default()
    };
    let mut device = Device::new(7);
    assert!(device.poll(&mut transport));
    device.take_device_events();

    assert!(!device.request_remote_name_on_handle(&mut transport, 0x0BAD));
    assert!(device.request_remote_name_on_handle(&mut transport, handle));
    assert!(device.is_remote_name_pending(&peer));
    assert_eq!(device.pending_remote_name_count(), 1);
    assert_eq!(
        transport.commands,
        vec![(
            7,
            Command::RemoteNameRequest {
                bd_addr: peer.clone(),
                page_scan_repetition_mode: 2,
                reserved: 0,
                clock_offset: 0,
            },
        )]
    );

    let mut name = remote_name(b"Living Room");
    name[12] = 0xFF;
    transport.events.extend([
        HciPacket::Event(Event::CommandStatus {
            status: 0,
            num_hci_command_packets: 1,
            command_opcode: HCI_REMOTE_NAME_REQUEST_COMMAND,
        }),
        HciPacket::Event(Event::RemoteNameRequestComplete {
            status: 0,
            bd_addr: peer.clone(),
            remote_name: name,
        }),
    ]);
    assert!(device.poll(&mut transport));

    assert_eq!(device.pending_remote_name_count(), 0);
    assert!(!device.is_remote_name_pending(&peer));
    assert_eq!(
        device
            .classic_connection(handle)
            .and_then(|connection| connection.peer_name.as_deref()),
        Some("Living Room")
    );
    assert_eq!(
        device.take_classic_remote_name_results(),
        vec![RemoteNameResult {
            peer_address: peer.clone(),
            result: Ok("Living Room".into()),
        }]
    );
    assert_eq!(
        device.take_classic_remote_names(),
        vec![(0, peer.clone(), "Living Room".into())]
    );
    assert_eq!(
        device.take_device_events(),
        vec![DeviceEvent::RemoteName {
            status: 0,
            peer_address: peer,
            name: "Living Room".into(),
        }]
    );
}

#[test]
fn remote_name_failures_are_correlated_and_invalid_utf8_is_rejected() {
    let rejected = address("10:00:00:00:00:01");
    let failed = address("10:00:00:00:00:02");
    let malformed = address("10:00:00:00:00:03");
    let mut transport = EventTransport::default();
    let mut device = Device::new(0);

    device.request_remote_name(&mut transport, rejected.clone());
    device.request_remote_name(&mut transport, failed.clone());
    device.request_remote_name(&mut transport, malformed.clone());
    assert_eq!(device.pending_remote_name_count(), 3);

    transport.events.extend([
        HciPacket::Event(Event::CommandStatus {
            status: 0x0C,
            num_hci_command_packets: 1,
            command_opcode: HCI_REMOTE_NAME_REQUEST_COMMAND,
        }),
        HciPacket::Event(Event::CommandStatus {
            status: 0,
            num_hci_command_packets: 1,
            command_opcode: HCI_REMOTE_NAME_REQUEST_COMMAND,
        }),
        HciPacket::Event(Event::RemoteNameRequestComplete {
            status: 0x04,
            bd_addr: failed.clone(),
            remote_name: [0; 248],
        }),
        HciPacket::Event(Event::CommandStatus {
            status: 0,
            num_hci_command_packets: 1,
            command_opcode: HCI_REMOTE_NAME_REQUEST_COMMAND,
        }),
        HciPacket::Event(Event::RemoteNameRequestComplete {
            status: 0,
            bd_addr: malformed.clone(),
            remote_name: remote_name(&[0xF0, 0x28, 0x8C, 0x28]),
        }),
    ]);
    assert!(device.poll(&mut transport));

    let malformed_error = RemoteNameError::InvalidUtf8 {
        valid_up_to: 0,
        error_len: Some(1),
    };
    assert_eq!(device.pending_remote_name_count(), 0);
    assert_eq!(
        device.take_classic_remote_name_results(),
        vec![
            RemoteNameResult {
                peer_address: rejected.clone(),
                result: Err(RemoteNameError::HciStatus(0x0C)),
            },
            RemoteNameResult {
                peer_address: failed.clone(),
                result: Err(RemoteNameError::HciStatus(0x04)),
            },
            RemoteNameResult {
                peer_address: malformed.clone(),
                result: Err(malformed_error),
            },
        ]
    );
    assert!(device.take_classic_remote_names().is_empty());
    assert_eq!(
        device.take_device_events(),
        vec![
            DeviceEvent::RemoteNameFailure {
                peer_address: rejected,
                error: RemoteNameError::HciStatus(0x0C),
            },
            DeviceEvent::RemoteNameFailure {
                peer_address: failed,
                error: RemoteNameError::HciStatus(0x04),
            },
            DeviceEvent::RemoteNameFailure {
                peer_address: malformed,
                error: malformed_error,
            },
        ]
    );
}

#[test]
fn power_off_cancels_pending_remote_name_requests() {
    let peer = address("10:00:00:00:00:04");
    let mut transport = EventTransport::default();
    let mut device = Device::new(0);

    device.request_remote_name(&mut transport, peer.clone());
    assert!(device.is_remote_name_pending(&peer));
    device.power_off();

    assert_eq!(device.pending_remote_name_count(), 0);
    assert!(device.take_classic_remote_name_results().is_empty());
    assert_eq!(device.take_device_events(), vec![DeviceEvent::Flush]);
}
