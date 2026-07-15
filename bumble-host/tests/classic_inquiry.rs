use bumble::{Address, AddressType};
use bumble_hci::{AclDataPacket, Command, Event, HciPacket, IsoDataPacket};
use bumble_host::{Device, HostTransport};

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

#[test]
fn device_preserves_all_classic_inquiry_metadata() {
    let basic = address("11:11:11:11:11:11");
    let with_rssi = address("22:22:22:22:22:22");
    let extended = address("33:33:33:33:33:33");
    let mut eir = [0; 240];
    eir[..8].copy_from_slice(&[7, 0x09, b'B', b'u', b'm', b'b', b'l', b'e']);
    let mut transport = EventTransport {
        events: vec![
            HciPacket::Event(Event::InquiryResult {
                bd_addr: vec![basic.clone()],
                page_scan_repetition_mode: vec![1],
                reserved_0: vec![0],
                reserved_1: vec![0],
                class_of_device: vec![0x240400],
                clock_offset: vec![0],
            }),
            HciPacket::Event(Event::InquiryResultWithRssi {
                bd_addr: vec![with_rssi.clone()],
                page_scan_repetition_mode: vec![1],
                reserved: vec![0],
                class_of_device: vec![0x200404],
                clock_offset: vec![0],
                rssi: vec![-42],
            }),
            HciPacket::Event(Event::ExtendedInquiryResult {
                num_responses: 1,
                bd_addr: extended.clone(),
                page_scan_repetition_mode: 1,
                reserved: 0,
                class_of_device: 0x200418,
                clock_offset: 0,
                rssi: -30,
                extended_inquiry_response: eir,
            }),
        ],
        ..EventTransport::default()
    };
    let mut device = Device::new(0);

    assert!(device.poll(&mut transport));
    assert_eq!(
        device.take_classic_inquiry_results(),
        vec![basic.clone(), with_rssi.clone(), extended.clone()]
    );
    let details = device.take_classic_inquiry_result_details();
    assert_eq!(details.len(), 3);
    assert_eq!(details[0].peer_address, basic);
    assert_eq!(details[0].class_of_device, 0x240400);
    assert_eq!(details[0].rssi, None);
    assert!(details[0].extended_inquiry_response.is_empty());
    assert_eq!(details[1].peer_address, with_rssi);
    assert_eq!(details[1].rssi, Some(-42));
    assert_eq!(details[2].peer_address, extended);
    assert_eq!(details[2].class_of_device, 0x200418);
    assert_eq!(details[2].rssi, Some(-30));
    assert_eq!(details[2].extended_inquiry_response, eir);
}

#[test]
fn classic_discovery_drives_commands_state_and_auto_restart() {
    let mut transport = EventTransport::default();
    let mut device = Device::new(7);

    device.start_discovery(&mut transport, true);
    assert!(device.is_discovering());
    assert!(device.discovery_auto_restart_enabled());
    assert_eq!(
        transport.commands,
        vec![
            (7, Command::WriteInquiryMode { inquiry_mode: 2 }),
            (
                7,
                Command::Inquiry {
                    lap: 0x009E_8B33,
                    inquiry_length: 8,
                    num_responses: 0,
                },
            ),
        ]
    );

    transport
        .events
        .push(HciPacket::Event(Event::InquiryComplete { status: 0 }));
    assert!(device.poll(&mut transport));
    assert!(device.is_discovering());
    assert_eq!(device.take_classic_inquiry_complete(), vec![0]);
    assert_eq!(transport.commands.len(), 4);
    assert_eq!(
        &transport.commands[2..],
        &[
            (7, Command::WriteInquiryMode { inquiry_mode: 2 }),
            (
                7,
                Command::Inquiry {
                    lap: 0x009E_8B33,
                    inquiry_length: 8,
                    num_responses: 0,
                },
            ),
        ]
    );

    device.stop_discovery(&mut transport);
    assert!(!device.is_discovering());
    assert!(device.discovery_auto_restart_enabled());
    assert_eq!(
        transport.commands.last(),
        Some(&(7, Command::InquiryCancel))
    );
}

#[test]
fn one_shot_classic_discovery_stops_after_inquiry_complete() {
    let mut transport = EventTransport::default();
    let mut device = Device::new(0);

    device.start_discovery(&mut transport, false);
    assert!(!device.discovery_auto_restart_enabled());
    transport
        .events
        .push(HciPacket::Event(Event::InquiryComplete { status: 0x01 }));
    assert!(device.poll(&mut transport));

    assert!(!device.is_discovering());
    assert!(device.discovery_auto_restart_enabled());
    assert_eq!(device.take_classic_inquiry_complete(), vec![0x01]);
    assert_eq!(transport.commands.len(), 2);
    device.stop_discovery(&mut transport);
    assert_eq!(transport.commands.len(), 2);
}

#[test]
fn discoverable_and_connectable_update_eir_and_scan_bits() {
    let mut transport = EventTransport::default();
    let mut device = Device::new(3);
    device.config.classic_enabled = true;
    device.config.name = "Rust Inquiry".into();

    device.set_discoverable(&mut transport, false).unwrap();
    assert!(!device.config.discoverable);
    assert!(matches!(
        &transport.commands[0],
        (
            3,
            Command::WriteExtendedInquiryResponse {
                fec_required: 0,
                extended_inquiry_response,
            },
        ) if &extended_inquiry_response[..14] == b"\x0D\x09Rust Inquiry"
            && extended_inquiry_response[14..].iter().all(|byte| *byte == 0)
    ));
    assert_eq!(
        transport.commands[1],
        (3, Command::WriteScanEnable { scan_enable: 2 })
    );
    assert_eq!(
        device.classic_inquiry_response(),
        match &transport.commands[0].1 {
            Command::WriteExtendedInquiryResponse {
                extended_inquiry_response,
                ..
            } => Some(extended_inquiry_response),
            _ => None,
        }
    );

    device.set_connectable(&mut transport, false);
    assert!(!device.config.connectable);
    assert_eq!(
        transport.commands[2],
        (3, Command::WriteScanEnable { scan_enable: 0 })
    );

    device.set_classic_inquiry_response(Some([0xA5; 240]));
    device.set_discoverable(&mut transport, true).unwrap();
    assert!(device.config.discoverable);
    assert_eq!(
        transport.commands[3],
        (
            3,
            Command::WriteExtendedInquiryResponse {
                fec_required: 0,
                extended_inquiry_response: [0xA5; 240],
            },
        )
    );
    assert_eq!(
        transport.commands[4],
        (3, Command::WriteScanEnable { scan_enable: 1 })
    );
}

#[test]
fn custom_inquiry_response_survives_power_cycles() {
    let mut transport = EventTransport::default();
    let mut device = Device::new(4);
    device.config.classic_enabled = true;
    device.config.le_enabled = false;
    device.set_classic_inquiry_response(Some([0x5A; 240]));

    device.power_on(&mut transport).unwrap();
    device.power_off();
    device.power_on(&mut transport).unwrap();

    let responses = transport
        .commands
        .iter()
        .filter_map(|(_, command)| match command {
            Command::WriteExtendedInquiryResponse {
                extended_inquiry_response,
                ..
            } => Some(extended_inquiry_response),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(responses, vec![&[0x5A; 240], &[0x5A; 240]]);
    assert_eq!(device.classic_inquiry_response(), Some(&[0x5A; 240]));
}
