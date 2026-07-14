use bumble::keys::{Key, KeyStore, MemoryKeyStore, PairingKeys};
use bumble::{Address, AddressType};
use bumble_controller::{Controller, LocalLink};
use bumble_hci::Command;
use bumble_host::{pump, Device, DeviceConfiguration};
use bumble_smp::resolvable_private_address;

fn address(value: &str, address_type: AddressType) -> Address {
    Address::parse(value, address_type).unwrap()
}

#[test]
fn controller_resolving_list_connects_identity_to_rpa_and_routes_acl() {
    let irk = [0x35; 16];
    let identity = address("C4:F2:17:1A:1D:BB", AddressType::RANDOM_DEVICE);
    let rpa = resolvable_private_address(&irk, [0x11, 0x22, 0x73]);
    let central_public = address("00:00:00:00:00:01", AddressType::PUBLIC_DEVICE);
    let peripheral_public = address("00:00:00:00:00:02", AddressType::PUBLIC_DEVICE);

    let mut store = MemoryKeyStore::new();
    store
        .update(
            &identity.to_string(false),
            PairingKeys {
                address_type: Some(AddressType::RANDOM_DEVICE),
                irk: Some(Key::new(irk.to_vec())),
                ..PairingKeys::default()
            },
        )
        .unwrap();
    let resolving_keys = store.get_resolving_keys().unwrap();

    let mut link = LocalLink::new();
    let central = link.add_controller(Controller::new("C", central_public));
    let peripheral = link.add_controller(Controller::new("P", peripheral_public));
    let mut devices = [Device::new(central), Device::new(peripheral)];
    assert_eq!(
        devices[0].configure_address_resolution(&mut link, &resolving_keys, [0x47; 16]),
        1
    );
    link.handle_command(
        peripheral,
        Command::LeSetRandomAddress {
            random_address: rpa.clone(),
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
            random_address: address("C4:F2:17:1A:1D:AA", AddressType::RANDOM_DEVICE),
        },
    );
    link.handle_command(
        central,
        Command::LeCreateConnection {
            le_scan_interval: 16,
            le_scan_window: 16,
            initiator_filter_policy: 0,
            peer_address_type: 1,
            peer_address: identity.clone(),
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
    pump(&mut link, &mut devices);

    assert!(devices[0].is_connected());
    assert!(devices[1].is_connected());
    let reported = devices[0].peer_address().unwrap();
    assert_eq!(reported.address_bytes(), identity.address_bytes());
    assert_eq!(reported.address_type(), AddressType::RANDOM_IDENTITY);
    assert_ne!(reported.address_bytes(), rpa.address_bytes());

    assert!(devices[0].send_l2cap(&mut link, 0x0040, b"resolved route"));
    pump(&mut link, &mut devices);
    assert_eq!(
        devices[1].take_l2cap(0x0040),
        vec![b"resolved route".to_vec()]
    );
}

#[test]
fn configured_power_on_loads_bond_irks_into_the_controller_resolver() {
    let peer_irk = [0x35; 16];
    let peer_identity = address("C4:F2:17:1A:1D:BB", AddressType::RANDOM_DEVICE);
    let peer_rpa = resolvable_private_address(&peer_irk, [0x11, 0x22, 0x73]);
    let mut store = MemoryKeyStore::new();
    store
        .update(
            &peer_identity.to_string(false),
            PairingKeys {
                address_type: Some(AddressType::RANDOM_DEVICE),
                irk: Some(Key::new(peer_irk.to_vec())),
                ..PairingKeys::default()
            },
        )
        .unwrap();

    let mut link = LocalLink::new();
    let central = link.add_controller(Controller::new(
        "C",
        address("00:00:00:00:00:01", AddressType::PUBLIC_DEVICE),
    ));
    let peripheral = link.add_controller(Controller::new(
        "P",
        address("00:00:00:00:00:02", AddressType::PUBLIC_DEVICE),
    ));
    let mut central_device = Device::from_config(
        central,
        DeviceConfiguration {
            address: address("C4:F2:17:1A:1D:AA", AddressType::RANDOM_DEVICE),
            address_resolution_offload: true,
            gap_service_enabled: false,
            gatt_service_enabled: false,
            ..DeviceConfiguration::default()
        },
    )
    .unwrap();
    central_device.set_key_store(store);
    let mut devices = [central_device, Device::new(peripheral)];

    devices[0].power_on(&mut link).unwrap();
    pump(&mut link, &mut devices);
    assert!(devices[0].take_key_store_errors().is_empty());

    link.handle_command(
        peripheral,
        Command::LeSetRandomAddress {
            random_address: peer_rpa.clone(),
        },
    );
    link.handle_command(
        peripheral,
        Command::LeSetAdvertisingEnable {
            advertising_enable: 1,
        },
    );
    devices[0].connect_le(&mut link, peer_identity.clone());
    pump(&mut link, &mut devices);

    assert!(devices[0].is_connected());
    let reported = devices[0].peer_address().unwrap();
    assert_eq!(reported.address_bytes(), peer_identity.address_bytes());
    assert_eq!(reported.address_type(), AddressType::RANDOM_IDENTITY);
    assert_ne!(reported.address_bytes(), peer_rpa.address_bytes());
}
