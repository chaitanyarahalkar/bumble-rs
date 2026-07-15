use bumble::{Address, AddressType};
use bumble_controller::{Controller, LocalLink, ROLE_PERIPHERAL};
use bumble_host::{pump, Device, DeviceConfiguration};
use bumble_l2cap::{
    INFORMATION_RESULT_NOT_SUPPORTED, INFORMATION_RESULT_SUCCESS,
    INFORMATION_TYPE_CONNECTIONLESS_MTU, INFORMATION_TYPE_EXTENDED_FEATURES_SUPPORTED,
    INFORMATION_TYPE_FIXED_CHANNELS_SUPPORTED,
};

fn public_address(value: &str) -> Address {
    Address::parse(value, AddressType::PUBLIC_DEVICE).unwrap()
}

fn random_address(value: &str) -> Address {
    Address::parse(value, AddressType::RANDOM_DEVICE).unwrap()
}

#[test]
fn configured_classic_auto_accepts_and_advertises_l2cap_capabilities() {
    let initiator_address = public_address("11:11:11:11:11:11");
    let responder_address = public_address("22:22:22:22:22:22");
    let mut link = LocalLink::new();
    let initiator_id = link.add_controller(Controller::new("A", initiator_address));
    let responder_id = link.add_controller(Controller::new("B", responder_address.clone()));
    let mut devices = [
        Device::new(initiator_id),
        Device::from_config(
            responder_id,
            DeviceConfiguration {
                classic_enabled: true,
                classic_accept_any: true,
                classic_smp_enabled: true,
                l2cap_extended_features: vec![0x0080, 0x0020, 0x0008],
                ..DeviceConfiguration::default()
            },
        )
        .unwrap(),
    ];

    devices[0].connect_classic(&mut link, responder_address);
    pump(&mut link, &mut devices);

    assert!(devices[0].classic_connection_handle().is_some());
    assert!(devices[1].classic_connection_handle().is_some());
    assert_eq!(devices[1].classic_connection_role(), Some(ROLE_PERIPHERAL));
    assert!(devices[1].take_classic_connection_requests().is_empty());

    let handle = devices[0].classic_connection_handle().unwrap();
    let requests = [
        INFORMATION_TYPE_CONNECTIONLESS_MTU,
        INFORMATION_TYPE_EXTENDED_FEATURES_SUPPORTED,
        INFORMATION_TYPE_FIXED_CHANNELS_SUPPORTED,
        0xFFFF,
    ];
    for info_type in requests {
        devices[0]
            .request_l2cap_information(&mut link, handle, info_type)
            .unwrap();
    }
    pump(&mut link, &mut devices);

    let responses = devices[0].take_l2cap_information_responses(handle);
    assert_eq!(responses.len(), 4);
    assert_eq!(responses[0].result, INFORMATION_RESULT_SUCCESS);
    assert_eq!(responses[0].data, [0x00, 0x04]);
    assert_eq!(responses[1].data, [0xA8, 0x00, 0x00, 0x00]);
    assert_eq!(
        responses[2].data,
        [0xF2, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]
    );
    assert_eq!(responses[3].result, INFORMATION_RESULT_NOT_SUPPORTED);
    assert!(responses[3].data.is_empty());
}

#[test]
fn disabled_classic_auto_accept_preserves_the_explicit_request() {
    let initiator_address = public_address("33:33:33:33:33:33");
    let responder_address = public_address("44:44:44:44:44:44");
    let mut link = LocalLink::new();
    let initiator_id = link.add_controller(Controller::new("A", initiator_address.clone()));
    let responder_id = link.add_controller(Controller::new("B", responder_address.clone()));
    let mut devices = [
        Device::new(initiator_id),
        Device::from_config(
            responder_id,
            DeviceConfiguration {
                classic_enabled: true,
                classic_accept_any: false,
                ..DeviceConfiguration::default()
            },
        )
        .unwrap(),
    ];

    devices[0].connect_classic(&mut link, responder_address);
    devices[0].poll(&mut link);
    link.pump_classic();
    devices[1].poll(&mut link);

    assert!(devices[0].classic_connection_handle().is_none());
    assert!(devices[1].classic_connection_handle().is_none());
    assert_eq!(
        devices[1].take_classic_connection_requests(),
        std::slice::from_ref(&initiator_address)
    );

    devices[1].accept_classic(&mut link, initiator_address);
    pump(&mut link, &mut devices);
    assert_eq!(devices[1].classic_connection_role(), Some(ROLE_PERIPHERAL));
}

#[test]
fn configured_l2cap_features_are_reported_over_le_signaling() {
    let central_address = random_address("C4:F2:17:1A:1D:AA");
    let peripheral_address = random_address("C4:F2:17:1A:1D:BB");
    let mut link = LocalLink::new();
    let central_id = link.add_controller(Controller::new(
        "central",
        public_address("00:00:00:00:00:01"),
    ));
    let peripheral_id = link.add_controller(Controller::new(
        "peripheral",
        public_address("00:00:00:00:00:02"),
    ));
    let mut devices = [
        Device::new(central_id),
        Device::from_config(
            peripheral_id,
            DeviceConfiguration {
                address: peripheral_address.clone(),
                classic_smp_enabled: false,
                l2cap_extended_features: vec![0x0001, 0x0004],
                ..DeviceConfiguration::default()
            },
        )
        .unwrap(),
    ];
    devices[0].set_random_address(&mut link, central_address);
    devices[1].set_random_address(&mut link, peripheral_address.clone());
    assert!(devices[1].start_advertising(&mut link, &[]));
    devices[0].connect_le(&mut link, peripheral_address);
    pump(&mut link, &mut devices);

    let handle = devices[0].connection_handle().unwrap();
    devices[0]
        .request_l2cap_information(
            &mut link,
            handle,
            INFORMATION_TYPE_EXTENDED_FEATURES_SUPPORTED,
        )
        .unwrap();
    pump(&mut link, &mut devices);
    let responses = devices[0].take_l2cap_information_responses(handle);
    assert_eq!(responses.len(), 1);
    assert_eq!(responses[0].result, INFORMATION_RESULT_SUCCESS);
    assert_eq!(responses[0].data, [0x05, 0x00, 0x00, 0x00]);
}
