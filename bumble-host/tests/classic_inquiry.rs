use bumble::{Address, AddressType};
use bumble_hci::{AclDataPacket, Command, Event, HciPacket, IsoDataPacket};
use bumble_host::{Device, HostTransport};

#[derive(Default)]
struct EventTransport {
    events: Vec<HciPacket>,
}

impl HostTransport for EventTransport {
    fn handle_command(&mut self, _controller_id: usize, _command: Command) {}

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
