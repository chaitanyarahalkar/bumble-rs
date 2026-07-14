use bumble::{Address, AddressType, Uuid};
use bumble_att::AttPdu;
use bumble_controller::{Controller, LocalLink};
use bumble_gatt::{
    permissions, properties, CharacteristicDefinition, GattServer, ServiceDefinition,
};
use bumble_hci::Command;
use bumble_host::{pump, Device, ATT_CID};
use bumble_l2cap::{
    LeCreditBasedChannelSpec, CREDIT_BASED_CONNECTION_ALL_SUCCESSFUL,
    CREDIT_BASED_CONNECTION_REFUSED_SPSM_NOT_SUPPORTED,
};

const VALUE_HANDLE: u16 = 3;
const CCCD_HANDLE: u16 = 4;

fn address(value: &str) -> Address {
    Address::parse(value, AddressType::RANDOM_DEVICE).unwrap()
}

fn connect(link: &mut LocalLink, central: usize, peripheral: usize) {
    link.handle_command(
        peripheral,
        Command::LeSetRandomAddress {
            random_address: address("C4:F2:17:1A:1D:BB"),
        },
    );
    link.handle_command(
        peripheral,
        Command::LeSetAdvertisingEnable {
            advertising_enable: 1,
        },
    );
    link.handle_command(
        central,
        Command::LeSetRandomAddress {
            random_address: address("C4:F2:17:1A:1D:AA"),
        },
    );
    link.handle_command(
        central,
        Command::LeCreateConnection {
            le_scan_interval: 16,
            le_scan_window: 16,
            initiator_filter_policy: 0,
            peer_address_type: 1,
            peer_address: address("C4:F2:17:1A:1D:BB"),
            own_address_type: 1,
            connection_interval_min: 24,
            connection_interval_max: 40,
            max_latency: 0,
            supervision_timeout: 42,
            min_ce_length: 0,
            max_ce_length: 0,
        },
    );
    link.establish_connections();
}

fn server() -> GattServer {
    GattServer::from_definitions(vec![ServiceDefinition {
        uuid: Uuid::from_16_bits(0xABCD),
        primary: true,
        included_services: vec![],
        characteristics: vec![CharacteristicDefinition {
            uuid: Uuid::from_16_bits(0x1234),
            properties: properties::READ
                | properties::WRITE
                | properties::NOTIFY
                | properties::INDICATE,
            permissions: permissions::READABLE | permissions::WRITEABLE,
            value: b"initial".to_vec(),
            descriptors: vec![],
        }],
    }])
    .unwrap()
}

fn fixed_request(link: &mut LocalLink, devices: &mut [Device], pdu: &AttPdu) -> AttPdu {
    assert!(devices[0].send_att(link, pdu));
    pump(link, devices);
    let responses = devices[0].take_inbox();
    assert_eq!(responses.len(), 1);
    responses.into_iter().next().unwrap()
}

fn eatt_request(
    link: &mut LocalLink,
    devices: &mut [Device],
    connection_handle: u16,
    source_cid: u16,
    pdu: &AttPdu,
) -> AttPdu {
    devices[0]
        .send_eatt(link, connection_handle, source_cid, pdu)
        .unwrap();
    pump(link, devices);
    let responses = devices[0].take_eatt_inbox_on_bearer(connection_handle, source_cid);
    assert_eq!(responses.len(), 1);
    responses.into_iter().next().unwrap()
}

#[test]
fn eatt_read_write_and_bearer_scoped_subscriptions_run_end_to_end() {
    let mut link = LocalLink::new();
    let central_id = link.add_controller(Controller::new("central", address("00:00:00:00:00:01")));
    let peripheral_id =
        link.add_controller(Controller::new("peripheral", address("00:00:00:00:00:02")));
    let mut devices = [
        Device::new(central_id),
        Device::with_server(peripheral_id, server()),
    ];
    devices[1]
        .register_eatt_server(LeCreditBasedChannelSpec::default())
        .unwrap();
    connect(&mut link, central_id, peripheral_id);
    pump(&mut link, &mut devices);
    let central_handle = devices[0].connection_handle().unwrap();
    let peripheral_handle = devices[1].connection_handle().unwrap();

    let central_cids = devices[0]
        .connect_eatt(
            &mut link,
            central_handle,
            LeCreditBasedChannelSpec::default(),
            2,
        )
        .unwrap();
    pump(&mut link, &mut devices);
    assert_eq!(central_cids.len(), 2);
    for cid in &central_cids {
        assert_eq!(
            devices[0].le_credit_connection_result(central_handle, *cid),
            Some(CREDIT_BASED_CONNECTION_ALL_SUCCESSFUL)
        );
    }
    let peripheral_cids = devices[1].eatt_bearers(peripheral_handle);
    assert_eq!(peripheral_cids.len(), 2);
    let first_peripheral_cid = devices[0]
        .le_credit_channel(central_handle, central_cids[0])
        .unwrap()
        .destination_cid;
    assert!(peripheral_cids.contains(&first_peripheral_cid));

    assert_eq!(
        eatt_request(
            &mut link,
            &mut devices,
            central_handle,
            central_cids[0],
            &AttPdu::ReadRequest {
                attribute_handle: VALUE_HANDLE,
            },
        ),
        AttPdu::ReadResponse {
            attribute_value: b"initial".to_vec(),
        }
    );
    assert_eq!(
        eatt_request(
            &mut link,
            &mut devices,
            central_handle,
            central_cids[0],
            &AttPdu::WriteRequest {
                attribute_handle: VALUE_HANDLE,
                attribute_value: b"eatt".to_vec(),
            },
        ),
        AttPdu::WriteResponse
    );
    assert_eq!(
        eatt_request(
            &mut link,
            &mut devices,
            central_handle,
            central_cids[1],
            &AttPdu::ReadRequest {
                attribute_handle: VALUE_HANDLE,
            },
        ),
        AttPdu::ReadResponse {
            attribute_value: b"eatt".to_vec(),
        }
    );

    assert_eq!(
        fixed_request(
            &mut link,
            &mut devices,
            &AttPdu::WriteRequest {
                attribute_handle: CCCD_HANDLE,
                attribute_value: 1u16.to_le_bytes().to_vec(),
            },
        ),
        AttPdu::WriteResponse
    );
    assert_eq!(
        eatt_request(
            &mut link,
            &mut devices,
            central_handle,
            central_cids[0],
            &AttPdu::WriteRequest {
                attribute_handle: CCCD_HANDLE,
                attribute_value: 1u16.to_le_bytes().to_vec(),
            },
        ),
        AttPdu::WriteResponse
    );
    assert_eq!(
        eatt_request(
            &mut link,
            &mut devices,
            central_handle,
            central_cids[1],
            &AttPdu::ReadRequest {
                attribute_handle: CCCD_HANDLE,
            },
        ),
        AttPdu::ReadResponse {
            attribute_value: 0u16.to_le_bytes().to_vec(),
        }
    );

    assert_eq!(
        devices[1]
            .notify_subscribers(&mut link, VALUE_HANDLE, b"notify", false)
            .unwrap(),
        2
    );
    pump(&mut link, &mut devices);
    assert_eq!(
        devices[0].take_inbox(),
        vec![AttPdu::HandleValueNotification {
            attribute_handle: VALUE_HANDLE,
            attribute_value: b"notify".to_vec(),
        }]
    );
    assert_eq!(
        devices[0].take_eatt_inbox_on_bearer(central_handle, central_cids[0]),
        vec![AttPdu::HandleValueNotification {
            attribute_handle: VALUE_HANDLE,
            attribute_value: b"notify".to_vec(),
        }]
    );
    assert!(devices[0]
        .take_eatt_inbox_on_bearer(central_handle, central_cids[1])
        .is_empty());

    for source_cid in [ATT_CID, central_cids[0]] {
        let request = AttPdu::WriteRequest {
            attribute_handle: CCCD_HANDLE,
            attribute_value: 2u16.to_le_bytes().to_vec(),
        };
        let response = if source_cid == ATT_CID {
            fixed_request(&mut link, &mut devices, &request)
        } else {
            eatt_request(
                &mut link,
                &mut devices,
                central_handle,
                source_cid,
                &request,
            )
        };
        assert_eq!(response, AttPdu::WriteResponse);
    }
    assert_eq!(
        devices[1]
            .indicate_subscribers_on_handle(
                &mut link,
                peripheral_handle,
                VALUE_HANDLE,
                b"indicate",
                false,
            )
            .unwrap(),
        2
    );
    pump(&mut link, &mut devices);
    assert_eq!(
        devices[0].take_inbox(),
        vec![AttPdu::HandleValueIndication {
            attribute_handle: VALUE_HANDLE,
            attribute_value: b"indicate".to_vec(),
        }]
    );
    assert_eq!(
        devices[0].take_eatt_inbox_on_bearer(central_handle, central_cids[0]),
        vec![AttPdu::HandleValueIndication {
            attribute_handle: VALUE_HANDLE,
            attribute_value: b"indicate".to_vec(),
        }]
    );
    assert!(devices[1].indication_pending(peripheral_handle, ATT_CID));
    assert!(devices[1].indication_pending(peripheral_handle, first_peripheral_cid));
    assert_eq!(
        devices[1]
            .indicate_subscribers_on_handle(
                &mut link,
                peripheral_handle,
                VALUE_HANDLE,
                b"blocked",
                false,
            )
            .unwrap(),
        0
    );
    assert!(devices[0].send_att(&mut link, &AttPdu::HandleValueConfirmation));
    devices[0]
        .send_eatt(
            &mut link,
            central_handle,
            central_cids[0],
            &AttPdu::HandleValueConfirmation,
        )
        .unwrap();
    pump(&mut link, &mut devices);
    assert!(!devices[1].indication_pending(peripheral_handle, ATT_CID));
    assert!(!devices[1].indication_pending(peripheral_handle, first_peripheral_cid));
}

#[test]
fn eatt_connection_is_refused_when_the_server_is_not_registered() {
    assert!(Device::new(99)
        .register_eatt_server(LeCreditBasedChannelSpec::default())
        .is_err());
    let mut link = LocalLink::new();
    let central_id = link.add_controller(Controller::new("central", address("00:00:00:00:00:01")));
    let peripheral_id =
        link.add_controller(Controller::new("peripheral", address("00:00:00:00:00:02")));
    let mut devices = [
        Device::new(central_id),
        Device::with_server(peripheral_id, server()),
    ];
    connect(&mut link, central_id, peripheral_id);
    pump(&mut link, &mut devices);
    let central_handle = devices[0].connection_handle().unwrap();
    let cid = devices[0]
        .connect_eatt(
            &mut link,
            central_handle,
            LeCreditBasedChannelSpec::default(),
            1,
        )
        .unwrap()[0];
    pump(&mut link, &mut devices);
    assert_eq!(
        devices[0].le_credit_connection_result(central_handle, cid),
        Some(CREDIT_BASED_CONNECTION_REFUSED_SPSM_NOT_SUPPORTED)
    );
    assert!(devices[0].le_credit_channel(central_handle, cid).is_none());
    assert!(devices[1]
        .eatt_bearers(devices[1].connection_handle().unwrap())
        .is_empty());
}
