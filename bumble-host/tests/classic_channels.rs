use bumble::{Address, AddressType};
use bumble_controller::{Controller, LocalLink};
use bumble_host::{pump, Device};
use bumble_l2cap::{ClassicChannelSpec, ClassicChannelState};

fn address(value: &str) -> Address {
    Address::parse(value, AddressType::PUBLIC_DEVICE).unwrap()
}

fn connect_classic(
    link: &mut LocalLink,
    initiator: &mut Device,
    responder: &mut Device,
    initiator_address: &Address,
    responder_address: &Address,
) {
    initiator.connect_classic(link, responder_address.clone());
    initiator.poll(link);
    link.pump_classic();
    responder.poll(link);
    responder.accept_classic(link, initiator_address.clone());
    responder.poll(link);
    link.pump_classic();
    initiator.poll(link);
}

#[test]
fn classic_dynamic_channels_run_over_device_acl_transport() {
    let initiator_address = address("11:11:11:11:11:11");
    let responder_address = address("22:22:22:22:22:22");
    let mut link = LocalLink::new();
    let initiator_id = link.add_controller(Controller::new("A", initiator_address.clone()));
    let responder_id = link.add_controller(Controller::new("B", responder_address.clone()));
    let mut devices = [Device::new(initiator_id), Device::new(responder_id)];
    devices[1]
        .register_classic_channel_server(Some(3), ClassicChannelSpec { mtu: 512 })
        .unwrap();
    let [initiator, responder] = &mut devices;
    connect_classic(
        &mut link,
        initiator,
        responder,
        &initiator_address,
        &responder_address,
    );
    let initiator_handle = devices[0].classic_connection_handle().unwrap();
    let responder_handle = devices[1].classic_connection_handle().unwrap();

    let initiator_cid = devices[0]
        .connect_classic_channel(
            &mut link,
            initiator_handle,
            3,
            ClassicChannelSpec { mtu: 600 },
        )
        .unwrap();
    pump(&mut link, &mut devices);
    let responder_cid = devices[1]
        .take_accepted_classic_channels(responder_handle)
        .into_iter()
        .next()
        .expect("server accepted channel");
    assert_eq!(
        devices[0]
            .classic_channel(initiator_handle, initiator_cid)
            .unwrap()
            .state,
        ClassicChannelState::Open
    );
    assert_eq!(
        devices[1]
            .classic_channel(responder_handle, responder_cid)
            .unwrap()
            .peer_mtu,
        600
    );

    devices[0]
        .send_classic_channel_sdu(
            &mut link,
            initiator_handle,
            initiator_cid,
            b"device-backed Classic L2CAP",
        )
        .unwrap();
    pump(&mut link, &mut devices);
    assert_eq!(
        devices[1].take_classic_channel_sdus(responder_handle, responder_cid),
        [b"device-backed Classic L2CAP".to_vec()]
    );

    devices[1]
        .send_classic_channel_sdu(&mut link, responder_handle, responder_cid, b"reply")
        .unwrap();
    pump(&mut link, &mut devices);
    assert_eq!(
        devices[0].take_classic_channel_sdus(initiator_handle, initiator_cid),
        [b"reply".to_vec()]
    );

    devices[0]
        .disconnect_classic_channel(&mut link, initiator_handle, initiator_cid)
        .unwrap();
    pump(&mut link, &mut devices);
    assert_eq!(
        devices[0]
            .classic_channel(initiator_handle, initiator_cid)
            .unwrap()
            .state,
        ClassicChannelState::Closed
    );
    assert_eq!(
        devices[1]
            .classic_channel(responder_handle, responder_cid)
            .unwrap()
            .state,
        ClassicChannelState::Closed
    );
    assert!(devices[0].take_classic_channel_errors().is_empty());
    assert!(devices[1].take_classic_channel_errors().is_empty());
}
