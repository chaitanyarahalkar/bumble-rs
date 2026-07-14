use bumble::{Address, AddressType};
use bumble_controller::{Controller, LocalLink, LINK_TYPE_ESCO};
use bumble_hfp::audio::parameters_for_codec;
use bumble_hfp::AudioCodec;
use bumble_host::Device;

fn address(value: &str) -> Address {
    Address::parse(value, AddressType::PUBLIC_DEVICE).unwrap()
}

#[test]
fn hfp_msbc_audio_lifecycle_through_device_api() {
    let central_address = address("11:11:11:11:11:11");
    let peripheral_address = address("22:22:22:22:22:22");
    let mut link = LocalLink::new();
    let central_id = link.add_controller(Controller::new("HF", central_address.clone()));
    let peripheral_id = link.add_controller(Controller::new("AG", peripheral_address.clone()));
    let mut hf = Device::new(central_id);
    let mut ag = Device::new(peripheral_id);

    hf.connect_classic(&mut link, peripheral_address.clone());
    hf.poll(&mut link); // command status
    link.pump_classic();
    ag.poll(&mut link); // ACL connection request
    ag.accept_classic(&mut link, central_address.clone());
    ag.poll(&mut link); // command status + peripheral connection complete
    link.pump_classic();
    hf.poll(&mut link); // central connection complete
    let hf_acl = hf.classic_connection_handle().unwrap();
    assert!(ag.classic_connection_handle().is_some());
    assert_eq!(
        hf.classic_connection_role(),
        Some(bumble_controller::ROLE_CENTRAL)
    );
    assert_eq!(
        ag.classic_connection_role(),
        Some(bumble_controller::ROLE_PERIPHERAL)
    );

    hf.switch_classic_role(
        &mut link,
        peripheral_address.clone(),
        bumble_controller::ROLE_PERIPHERAL,
    );
    hf.poll(&mut link); // command status
    link.pump_classic();
    hf.poll(&mut link);
    ag.poll(&mut link);
    assert_eq!(
        hf.classic_connection_role(),
        Some(bumble_controller::ROLE_PERIPHERAL)
    );
    assert_eq!(
        ag.classic_connection_role(),
        Some(bumble_controller::ROLE_CENTRAL)
    );

    // HFP's negotiated mSBC codec selects its normative T1 eSCO parameters.
    let parameters = parameters_for_codec(AudioCodec::Msbc);
    hf.send_hci_command(&mut link, parameters.setup_command(hf_acl));
    hf.poll(&mut link); // setup command status
    link.pump_classic();
    ag.poll(&mut link); // synchronous connection request
    assert_eq!(
        ag.take_synchronous_requests(),
        [(central_address.clone(), LINK_TYPE_ESCO)]
    );
    ag.send_hci_command(&mut link, parameters.accept_command(central_address));
    ag.poll(&mut link); // accept status + peripheral synchronous complete
    link.pump_classic();
    hf.poll(&mut link); // central synchronous complete

    let hf_sync = hf.synchronous_connections()[0].connection_handle;
    let ag_sync = ag.synchronous_connections()[0].connection_handle;
    assert_eq!(hf.synchronous_connections()[0].air_mode, 3); // transparent for mSBC
    assert_eq!(ag.synchronous_connections()[0].air_mode, 3);

    assert!(hf.send_synchronous(&mut link, hf_sync, 0, b"mSBC-frame"));
    ag.poll(&mut link);
    let received = ag.take_synchronous_inbox();
    assert_eq!(received.len(), 1);
    assert_eq!(received[0].connection_handle, ag_sync);
    assert_eq!(received[0].data, b"mSBC-frame");

    assert!(hf.disconnect_handle(&mut link, hf_sync, 0x13));
    hf.poll(&mut link);
    link.pump_classic();
    ag.poll(&mut link);
    assert!(hf.synchronous_connections().is_empty());
    assert!(ag.synchronous_connections().is_empty());
    assert!(hf.classic_connection_handle().is_some());
    assert!(ag.classic_connection_handle().is_some());
}
