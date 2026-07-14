use bumble::{Address, AddressType};
use bumble_controller::{Controller, LocalLink};
use bumble_hci::{AclDataPacket, Command, Event, HciPacket, IsoDataPacket, LeMetaEvent};
use bumble_host::{pump, ConnectionFeatureTransport, Device, HostTransport};

fn public_address(value: &str) -> Address {
    Address::parse(value, AddressType::PUBLIC_DEVICE).unwrap()
}

fn random_address(value: &str) -> Address {
    Address::parse(value, AddressType::RANDOM_DEVICE).unwrap()
}

#[test]
fn device_reads_live_le_and_all_classic_feature_pages() {
    let central_address = public_address("11:11:11:11:11:11");
    let peripheral_address = public_address("22:22:22:22:22:22");
    let central_random_address = random_address("C0:00:00:00:00:01");
    let peripheral_random_address = random_address("C0:00:00:00:00:02");
    let mut link = LocalLink::new();
    let central_id = link.add_controller(Controller::new("central", central_address.clone()));
    let peripheral_id =
        link.add_controller(Controller::new("peripheral", peripheral_address.clone()));
    let mut devices = [Device::new(central_id), Device::new(peripheral_id)];

    devices[0].set_random_address(&mut link, central_random_address);
    devices[1].set_random_address(&mut link, peripheral_random_address.clone());
    assert!(devices[1].start_advertising(&mut link, &[]));
    devices[0].connect_le(&mut link, peripheral_random_address);
    pump(&mut link, &mut devices);
    let le_handle = devices[0].connection_handle().unwrap();
    assert!(devices[0].read_remote_le_features_on_handle(&mut link, le_handle));
    pump(&mut link, &mut devices);
    assert_eq!(
        devices[0]
            .le_connection(le_handle)
            .unwrap()
            .peer_le_features,
        Some([0x00, 0x10, 0x00, 0xF0, 0, 0, 0, 0])
    );

    devices[0].connect_classic(&mut link, peripheral_address.clone());
    devices[0].poll(&mut link);
    link.pump_classic();
    devices[1].poll(&mut link);
    devices[1].accept_classic(&mut link, central_address);
    pump(&mut link, &mut devices);
    let classic_handle = devices[0]
        .classic_connection_handle_for_peer(&peripheral_address)
        .unwrap();
    assert!(devices[0].read_remote_classic_features_on_handle(&mut link, classic_handle));
    pump(&mut link, &mut devices);

    let connection = devices[0].classic_connection(classic_handle).unwrap();
    assert_eq!(connection.peer_lmp_max_page_number, Some(3));
    assert_eq!(
        connection.peer_lmp_features[&0],
        [0x00, 0x00, 0x00, 0x00, 0x60, 0x00, 0x00, 0x80]
    );
    assert_eq!(connection.peer_lmp_features[&1], [0; 8]);
    assert_eq!(connection.peer_lmp_features[&2], [0; 8]);
    assert_eq!(connection.peer_lmp_features[&3], [0; 8]);
    assert!(devices[0].take_connection_feature_errors().is_empty());
    assert!(!devices[0].read_remote_le_features_on_handle(&mut link, 0x0FFF));
    assert!(!devices[0].read_remote_classic_features_on_handle(&mut link, 0x0FFF));
}

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

#[test]
fn device_reports_le_and_classic_feature_failures() {
    let le_peer = public_address("33:33:33:33:33:33");
    let classic_peer = public_address("44:44:44:44:44:44");
    let mut transport = EventTransport {
        events: vec![
            HciPacket::Event(Event::LeMeta(LeMetaEvent::ConnectionComplete {
                status: 0,
                connection_handle: 0x0040,
                role: 0,
                peer_address_type: 0,
                peer_address: le_peer,
                connection_interval: 24,
                peripheral_latency: 0,
                supervision_timeout: 72,
                central_clock_accuracy: 0,
            })),
            HciPacket::Event(Event::ConnectionComplete {
                status: 0,
                connection_handle: 0x0041,
                bd_addr: classic_peer,
                link_type: 1,
                encryption_enabled: 0,
            }),
            HciPacket::Event(Event::LeMeta(LeMetaEvent::ReadRemoteFeaturesComplete {
                status: 0x3E,
                connection_handle: 0x0040,
                le_features: [0; 8],
            })),
            HciPacket::Event(Event::ReadRemoteSupportedFeaturesComplete {
                status: 0x08,
                connection_handle: 0x0041,
                lmp_features: [0; 8],
            }),
            HciPacket::Event(Event::ReadRemoteExtendedFeaturesComplete {
                status: 0x1A,
                connection_handle: 0x0041,
                page_number: 2,
                maximum_page_number: 3,
                extended_lmp_features: [0; 8],
            }),
        ],
    };
    let mut device = Device::new(0);
    assert!(device.poll(&mut transport));

    let errors = device.take_connection_feature_errors();
    assert_eq!(errors.len(), 3);
    assert_eq!(errors[0].transport, ConnectionFeatureTransport::Le);
    assert_eq!(errors[0].connection_handle, 0x0040);
    assert_eq!(errors[0].page_number, None);
    assert_eq!(errors[0].status, 0x3E);
    assert_eq!(errors[1].transport, ConnectionFeatureTransport::Classic);
    assert_eq!(errors[1].page_number, None);
    assert_eq!(errors[1].status, 0x08);
    assert_eq!(errors[2].transport, ConnectionFeatureTransport::Classic);
    assert_eq!(errors[2].page_number, Some(2));
    assert_eq!(errors[2].status, 0x1A);
    assert_eq!(device.le_connection(0x0040).unwrap().peer_le_features, None);
    assert!(device
        .classic_connection(0x0041)
        .unwrap()
        .peer_lmp_features
        .is_empty());
}
