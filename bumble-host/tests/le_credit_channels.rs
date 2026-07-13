use bumble::{Address, AddressType};
use bumble_controller::{Controller, LocalLink};
use bumble_host::{pump, Device};
use bumble_l2cap::{
    LeCreditBasedChannelSpec, CREDIT_BASED_RECONFIGURATION_SUCCESSFUL,
    L2CAP_LE_PSM_DYNAMIC_RANGE_START,
};

fn address(value: &str) -> Address {
    Address::parse(value, AddressType::RANDOM_DEVICE).unwrap()
}

#[test]
fn le_credit_channels_run_over_device_acl_transport() {
    let central_address = address("C4:F2:17:1A:1D:AA");
    let peripheral_address = address("C4:F2:17:1A:1D:BB");
    let mut link = LocalLink::new();
    let central_id = link.add_controller(Controller::new("central", address("00:00:00:00:00:01")));
    let peripheral_id =
        link.add_controller(Controller::new("peripheral", address("00:00:00:00:00:02")));
    let mut devices = [Device::new(central_id), Device::new(peripheral_id)];
    let psm = devices[1]
        .register_le_credit_server(LeCreditBasedChannelSpec {
            mtu: 80,
            mps: 23,
            max_credits: 1,
            ..LeCreditBasedChannelSpec::default()
        })
        .unwrap();
    assert_eq!(psm, L2CAP_LE_PSM_DYNAMIC_RANGE_START);
    devices[0].set_random_address(&mut link, central_address);
    devices[1].set_random_address(&mut link, peripheral_address.clone());
    assert!(devices[1].start_advertising(&mut link, &[]));
    devices[0].connect_le(&mut link, peripheral_address);
    pump(&mut link, &mut devices);
    let central_handle = devices[0].connection_handle().unwrap();
    let peripheral_handle = devices[1].connection_handle().unwrap();

    let central_cid = devices[0]
        .connect_le_credit_channel(
            &mut link,
            central_handle,
            psm,
            LeCreditBasedChannelSpec {
                mtu: 90,
                mps: 25,
                max_credits: 1,
                ..LeCreditBasedChannelSpec::default()
            },
        )
        .unwrap();
    pump(&mut link, &mut devices);
    let peripheral_cid = devices[1]
        .take_accepted_le_credit_channels(peripheral_handle)
        .into_iter()
        .next()
        .expect("server accepted channel");
    assert_eq!(
        devices[0].le_credit_connection_result(central_handle, central_cid),
        Some(0)
    );
    assert_eq!(
        devices[0]
            .le_credit_channel(central_handle, central_cid)
            .unwrap()
            .peer_mtu,
        80
    );

    let central_payload: Vec<u8> = (0..=255).cycle().take(511).collect();
    devices[0]
        .send_le_credit_sdu(&mut link, central_handle, central_cid, &central_payload)
        .unwrap();
    pump(&mut link, &mut devices);
    assert_eq!(
        devices[1]
            .take_le_credit_sdus(peripheral_handle, peripheral_cid)
            .concat(),
        central_payload
    );

    let peripheral_payload = b"reply through Device and real HCI ACL fragments".repeat(5);
    devices[1]
        .send_le_credit_sdu(
            &mut link,
            peripheral_handle,
            peripheral_cid,
            &peripheral_payload,
        )
        .unwrap();
    pump(&mut link, &mut devices);
    assert_eq!(
        devices[0]
            .take_le_credit_sdus(central_handle, central_cid)
            .concat(),
        peripheral_payload
    );

    let identifier = devices[0]
        .reconfigure_le_credit_channels(&mut link, central_handle, &[central_cid], 100, 30)
        .unwrap();
    pump(&mut link, &mut devices);
    assert_eq!(
        devices[0].le_credit_reconfiguration_result(central_handle, identifier),
        Some(CREDIT_BASED_RECONFIGURATION_SUCCESSFUL)
    );

    devices[0]
        .disconnect_le_credit_channel(&mut link, central_handle, central_cid)
        .unwrap();
    pump(&mut link, &mut devices);
    assert!(devices[0]
        .le_credit_channel(central_handle, central_cid)
        .is_none());
    assert!(devices[1]
        .le_credit_channel(peripheral_handle, peripheral_cid)
        .is_none());
    assert!(devices[0].take_le_credit_errors().is_empty());
    assert!(devices[1].take_le_credit_errors().is_empty());
}
