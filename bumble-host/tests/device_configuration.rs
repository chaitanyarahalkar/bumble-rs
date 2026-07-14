use std::time::{SystemTime, UNIX_EPOCH};

use bumble::{Address, AddressType};
use bumble_controller::{Controller, LocalLink};
use bumble_hci::{AclDataPacket, Command, HciPacket, IsoDataPacket};
use bumble_host::{
    pump, Device, DeviceConfiguration, DeviceConfigurationError, DevicePowerError, HostTransport,
    DEVICE_DEFAULT_ADDRESS, DEVICE_DEFAULT_ADVERTISING_INTERVAL, DEVICE_DEFAULT_LE_RPA_TIMEOUT,
    DEVICE_DEFAULT_NAME,
};
use bumble_smp::verify_resolvable_private_address;

#[derive(Default)]
struct CapturingTransport {
    commands: Vec<(usize, Command)>,
}

impl HostTransport for CapturingTransport {
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
        Vec::new()
    }
}

#[test]
fn device_configuration_defaults_match_upstream() {
    let config = DeviceConfiguration::default();

    assert_eq!(config.name, DEVICE_DEFAULT_NAME);
    assert_eq!(
        config.address,
        Address::parse(DEVICE_DEFAULT_ADDRESS, AddressType::RANDOM_DEVICE).unwrap()
    );
    assert_eq!(config.class_of_device, 0);
    assert_eq!(
        config.advertising_data,
        [vec![7, 0x09], DEVICE_DEFAULT_NAME.as_bytes().to_vec()].concat()
    );
    assert!(config.scan_response_data.is_empty());
    assert_eq!(
        config.advertising_interval_min,
        DEVICE_DEFAULT_ADVERTISING_INTERVAL
    );
    assert_eq!(
        config.advertising_interval_max,
        DEVICE_DEFAULT_ADVERTISING_INTERVAL
    );
    assert!(config.le_enabled);
    assert!(!config.classic_enabled);
    assert!(config.classic_sc_enabled);
    assert!(config.classic_ssp_enabled);
    assert!(config.classic_smp_enabled);
    assert!(config.classic_accept_any);
    assert!(config.classic_interlaced_scan_enabled);
    assert!(config.connectable);
    assert!(config.discoverable);
    assert_eq!(config.le_rpa_timeout, DEVICE_DEFAULT_LE_RPA_TIMEOUT);
    assert_eq!(config.irk, vec![0; 16]);
    assert_eq!(config.io_capability, 0x03);
    assert_eq!(config.l2cap_extended_features, vec![0x80, 0x20, 0x08]);
    assert!(config.gatt_services.is_empty());
    assert!(config.extra.is_empty());
}

#[test]
fn json_loading_matches_upstream_special_cases_and_dynamic_keys() {
    let mut config = DeviceConfiguration::from_json_str(
        r#"{
            "name": "Configured",
            "address": "F0:F1:F2:F3:F4:F5",
            "advertising_data": "",
            "scan_response_data": "03095253",
            "advertising_interval": 2000,
            "advertising_interval_min": 100,
            "classic_enabled": true,
            "le_privacy_enabled": true,
            "keystore": "JsonKeyStore",
            "identity_address_type": 1,
            "l2cap_extended_features": [1, 4],
            "gatt_services": [{"uuid": "180F"}],
            "custom_option": {"nested": true}
        }"#,
    )
    .unwrap();

    assert_eq!(config.name, "Configured");
    assert_eq!(
        config.address,
        Address::parse("F0:F1:F2:F3:F4:F5", AddressType::RANDOM_DEVICE).unwrap()
    );
    assert_eq!(
        config.irk,
        vec![
            0xF5, 0xF4, 0xF3, 0xF2, 0xF1, 0xF0, 0xF5, 0xF4, 0xF3, 0xF2, 0xF1, 0xF0, 0xF5, 0xF4,
            0xF3, 0xF2,
        ]
    );
    assert_eq!(
        config.advertising_data,
        [vec![11, 0x09], b"Configured".to_vec()].concat()
    );
    assert_eq!(config.scan_response_data, vec![3, 0x09, b'R', b'S']);
    assert_eq!(config.advertising_interval_min, 100.0);
    assert_eq!(config.advertising_interval_max, 2000.0);
    assert!(config.classic_enabled);
    assert!(config.le_privacy_enabled);
    assert_eq!(config.keystore.as_deref(), Some("JsonKeyStore"));
    assert_eq!(config.identity_address_type, Some(1));
    assert_eq!(config.l2cap_extended_features, vec![1, 4]);
    assert_eq!(config.gatt_services[0]["uuid"], "180F");
    assert_eq!(config.extra["custom_option"]["nested"], true);

    config
        .load_from_json_str(
            r#"{
                "irk": "000102030405060708090A0B0C0D0E0F",
                "advertising_data": "020106",
                "advertising_interval_max": 250,
                "keystore": null,
                "identity_address_type": null
            }"#,
        )
        .unwrap();
    assert_eq!(config.irk, (0u8..=15).collect::<Vec<_>>());
    assert_eq!(config.advertising_data, vec![2, 1, 6]);
    assert_eq!(config.advertising_interval_min, 100.0);
    assert_eq!(config.advertising_interval_max, 250.0);
    assert_eq!(config.keystore, None);
    assert_eq!(config.identity_address_type, None);

    let generated = DeviceConfiguration::from_json_str("{}").unwrap();
    assert_eq!(generated.irk.len(), 16);
}

#[test]
fn file_constructor_and_configured_advertising_are_live() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "bumble-rs-device-config-{}-{unique}.json",
        std::process::id()
    ));
    std::fs::write(
        &path,
        r#"{
            "name": "File Device",
            "irk": "00112233445566778899AABBCCDDEEFF",
            "advertising_data": "020106",
            "scan_response_data": "03095253",
            "advertising_interval_min": 100,
            "advertising_interval_max": 200
        }"#,
    )
    .unwrap();

    let mut device = Device::from_config_file(7, &path).unwrap();
    std::fs::remove_file(path).unwrap();
    assert_eq!(device.controller_id(), 7);
    assert_eq!(device.config.name, "File Device");

    let mut transport = CapturingTransport::default();
    assert!(device.start_configured_advertising(&mut transport));
    assert_eq!(transport.commands.len(), 4);
    assert_eq!(transport.commands[0].0, 7);
    assert_eq!(
        transport.commands[0].1,
        Command::LeSetAdvertisingData {
            advertising_data: vec![2, 1, 6],
        }
    );
    assert_eq!(
        transport.commands[1].1,
        Command::LeSetScanResponseData {
            scan_response_data: vec![3, 0x09, b'R', b'S'],
        }
    );
    assert!(matches!(
        &transport.commands[2].1,
        Command::LeSetAdvertisingParameters {
            advertising_interval_min: 160,
            advertising_interval_max: 320,
            advertising_type: 0,
            own_address_type: 1,
            ..
        }
    ));
    assert_eq!(
        transport.commands[3].1,
        Command::LeSetAdvertisingEnable {
            advertising_enable: 1,
        }
    );
}

#[test]
fn default_power_on_generates_and_programs_a_static_address() {
    let mut device = Device::new(3);
    let mut transport = CapturingTransport::default();

    device.power_on(&mut transport).unwrap();

    assert!(device.is_powered_on());
    assert!(device.static_address().is_static());
    assert_eq!(device.random_address(), device.static_address());
    assert_eq!(transport.commands.len(), 4);
    assert_eq!(transport.commands[0], (3, Command::Reset));
    assert_eq!(transport.commands[1], (3, Command::ReadBdAddr));
    assert_eq!(
        transport.commands[2],
        (
            3,
            Command::WriteLeHostSupport {
                le_supported_host: 1,
                simultaneous_le_host: 0,
            },
        )
    );
    assert_eq!(
        transport.commands[3].1,
        Command::LeSetRandomAddress {
            random_address: device.random_address().clone(),
        }
    );

    device.power_off();
    assert!(!device.is_powered_on());
}

#[test]
fn privacy_and_optional_le_features_follow_power_on_configuration() {
    let irk = [
        0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xAA, 0xBB, 0xCC, 0xDD, 0xEE,
        0xFF,
    ];
    let config = DeviceConfiguration {
        address: Address::parse("C0:11:22:33:44:55", AddressType::RANDOM_DEVICE).unwrap(),
        irk: irk.to_vec(),
        le_privacy_enabled: true,
        address_resolution_offload: true,
        cis_enabled: true,
        le_subrate_enabled: true,
        channel_sounding_enabled: true,
        le_shorter_connection_intervals_enabled: true,
        ..DeviceConfiguration::default()
    };
    let mut device = Device::from_config(0, config).unwrap();
    let mut transport = CapturingTransport::default();

    device.power_on(&mut transport).unwrap();

    assert!(verify_resolvable_private_address(
        &irk,
        device.random_address()
    ));
    assert_eq!(transport.commands.len(), 10);
    assert!(matches!(
        transport.commands[3].1,
        Command::LeSetRandomAddress { .. }
    ));
    assert_eq!(
        transport.commands[4].1,
        Command::LeSetAddressResolutionEnable {
            address_resolution_enable: 1,
        }
    );
    for (index, bit_number) in [(5, 32), (6, 38), (7, 47), (9, 73)] {
        assert_eq!(
            transport.commands[index].1,
            Command::LeSetHostFeature {
                bit_number,
                bit_value: 1,
            }
        );
    }
    assert_eq!(
        transport.commands[8].1,
        Command::LeCsReadLocalSupportedCapabilities
    );

    let rotated = device.update_rpa(&mut transport).unwrap();
    assert!(verify_resolvable_private_address(&irk, &rotated));
    assert_eq!(device.random_address(), &rotated);
    assert_eq!(
        transport.commands.last().unwrap().1,
        Command::LeSetRandomAddress {
            random_address: rotated,
        }
    );
}

#[test]
fn classic_power_on_matches_upstream_visibility_and_security_order() {
    let config = DeviceConfiguration {
        name: "Rust Bumble".into(),
        class_of_device: 0x123456,
        le_enabled: false,
        classic_enabled: true,
        classic_ssp_enabled: false,
        classic_sc_enabled: true,
        connectable: false,
        discoverable: true,
        ..DeviceConfiguration::default()
    };
    let mut device = Device::from_config(4, config).unwrap();
    let mut transport = CapturingTransport::default();

    device.power_on(&mut transport).unwrap();

    assert_eq!(transport.commands.len(), 12);
    assert_eq!(
        transport.commands[2].1,
        Command::WriteLeHostSupport {
            le_supported_host: 0,
            simultaneous_le_host: 0,
        }
    );
    assert!(matches!(
        &transport.commands[3].1,
        Command::WriteLocalName { local_name }
            if &local_name[..11] == b"Rust Bumble"
                && local_name[11..].iter().all(|byte| *byte == 0)
    ));
    assert_eq!(
        transport.commands[4].1,
        Command::WriteClassOfDevice {
            class_of_device: 0x123456,
        }
    );
    assert_eq!(
        transport.commands[5].1,
        Command::WriteSimplePairingMode {
            simple_pairing_mode: 0,
        }
    );
    assert_eq!(
        transport.commands[6].1,
        Command::WriteSecureConnectionsHostSupport {
            secure_connections_host_support: 1,
        }
    );
    assert_eq!(
        transport.commands[7].1,
        Command::WriteScanEnable { scan_enable: 1 }
    );
    assert!(matches!(
        &transport.commands[8].1,
        Command::WriteExtendedInquiryResponse {
            fec_required: 0,
            extended_inquiry_response,
        } if &extended_inquiry_response[..13] == b"\x0C\x09Rust Bumble"
            && extended_inquiry_response[13..].iter().all(|byte| *byte == 0)
    ));
    assert_eq!(
        transport.commands[9].1,
        Command::WriteScanEnable { scan_enable: 1 }
    );
    assert_eq!(
        transport.commands[10].1,
        Command::WritePageScanType { page_scan_type: 1 }
    );
    assert_eq!(
        transport.commands[11].1,
        Command::WriteInquiryScanType { scan_type: 1 }
    );
}

#[test]
fn configured_power_on_is_live_against_the_software_controller() {
    let public = Address::parse("00:11:22:33:44:55", AddressType::PUBLIC_DEVICE).unwrap();
    let random = Address::parse("C0:11:22:33:44:66", AddressType::RANDOM_DEVICE).unwrap();
    let mut link = LocalLink::new();
    let controller_id = link.add_controller(Controller::new("before", public.clone()));
    let config = DeviceConfiguration {
        name: "Powered".into(),
        address: random.clone(),
        class_of_device: 0x654321,
        classic_enabled: true,
        cis_enabled: true,
        le_subrate_enabled: true,
        channel_sounding_enabled: true,
        le_shorter_connection_intervals_enabled: true,
        ..DeviceConfiguration::default()
    };
    let mut device = Device::from_config(controller_id, config).unwrap();

    device.power_on(&mut link).unwrap();
    pump(&mut link, std::slice::from_mut(&mut device));

    assert_eq!(device.public_address(), Some(&public));
    assert_eq!(device.local_channel_sounding_capabilities_status(), Some(0));
    let capabilities = device.local_channel_sounding_capabilities().unwrap();
    assert_eq!(capabilities.num_config_supported, 4);
    assert_eq!(capabilities.max_consecutive_procedures_supported, 16);
    assert_eq!(capabilities.roles_supported, 3);
    let controller = link.controller(controller_id);
    assert_eq!(controller.name, "Powered");
    assert_eq!(controller.random_address(), &random);
    assert_eq!(controller.class_of_device(), 0x654321);
    assert_eq!(controller.classic_scan_enable(), 3);
    assert!(controller.secure_connections_host_support());
    assert_eq!(controller.page_scan_type(), 1);
    assert_eq!(controller.inquiry_scan_type(), 1);
    assert_eq!(
        &controller.extended_inquiry_response()[..9],
        b"\x08\x09Powered"
    );
    for bit_number in [32usize, 38, 73] {
        assert_ne!(
            controller.local_le_features()[bit_number / 8] & (1 << (bit_number % 8)),
            0
        );
    }
}

#[test]
fn power_on_validation_is_atomic() {
    let config = DeviceConfiguration {
        le_privacy_enabled: true,
        irk: vec![0; 15],
        ..DeviceConfiguration::default()
    };
    let mut device = Device::from_config(0, config).unwrap();
    let mut transport = CapturingTransport::default();
    assert_eq!(
        device.power_on(&mut transport),
        Err(DevicePowerError::InvalidIrkLength { actual: 15 })
    );
    assert!(transport.commands.is_empty());

    let config = DeviceConfiguration {
        classic_enabled: true,
        class_of_device: 0x0100_0000,
        ..DeviceConfiguration::default()
    };
    let mut device = Device::from_config(0, config).unwrap();
    assert_eq!(
        device.power_on(&mut transport),
        Err(DevicePowerError::ClassOfDeviceOutOfRange { value: 0x0100_0000 })
    );
    assert!(transport.commands.is_empty());
}

#[test]
fn invalid_typed_and_hex_fields_are_reported() {
    assert!(matches!(
        DeviceConfiguration::from_json_str(r#"{"le_enabled": "yes"}"#),
        Err(DeviceConfigurationError::Json(_))
    ));
    assert!(matches!(
        DeviceConfiguration::from_json_str(r#"{"irk": "123"}"#),
        Err(DeviceConfigurationError::InvalidField { field: "irk", .. })
    ));
    assert!(matches!(
        DeviceConfiguration::from_json_str("[]"),
        Err(DeviceConfigurationError::Json(_))
    ));
}

#[test]
fn invalid_configured_gatt_definitions_are_reported_with_paths() {
    for (json, expected) in [
        (
            r#"{"gatt_services":[{"uuid":"180F","characteristics":[{"uuid":"2A19","properties":"READ|MISSING","permissions":"READABLE"}]}]}"#,
            "[0].characteristics[0].properties: unknown flag \"MISSING\"",
        ),
        (
            r#"{"gatt_services":[{"uuid":"180F","characteristics":[{"uuid":"2A19","properties":"READ","permissions":"READABLE","descriptors":[{"descriptor_type":"2901","permission":"READABLE","permissions":"READABLE"}]}]}]}"#,
            "the key 'permission' must be renamed to 'permissions'",
        ),
        (
            r#"{"gatt_services":[{"uuid":"not-a-uuid"}]}"#,
            "[0].uuid: invalid argument",
        ),
    ] {
        let config = DeviceConfiguration::from_json_str(json).unwrap();
        let error = match Device::from_config(0, config) {
            Ok(_) => panic!("invalid configured GATT database was accepted"),
            Err(error) => error,
        };
        assert!(
            error.to_string().contains(expected),
            "{error} did not contain {expected:?}"
        );
    }
}

#[test]
fn invalid_pairing_configuration_is_rejected_during_device_construction() {
    for (config, expected_field) in [
        (
            DeviceConfiguration {
                io_capability: 5,
                ..DeviceConfiguration::default()
            },
            "io_capability",
        ),
        (
            DeviceConfiguration {
                identity_address_type: Some(2),
                ..DeviceConfiguration::default()
            },
            "identity_address_type",
        ),
    ] {
        assert!(matches!(
            Device::from_config(0, config),
            Err(DeviceConfigurationError::InvalidField { field, .. }) if field == expected_field
        ));
    }
}
