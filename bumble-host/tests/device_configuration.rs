use std::time::{SystemTime, UNIX_EPOCH};

use bumble::{Address, AddressType};
use bumble_hci::{AclDataPacket, Command, HciPacket, IsoDataPacket};
use bumble_host::{
    Device, DeviceConfiguration, DeviceConfigurationError, HostTransport, DEVICE_DEFAULT_ADDRESS,
    DEVICE_DEFAULT_ADVERTISING_INTERVAL, DEVICE_DEFAULT_LE_RPA_TIMEOUT, DEVICE_DEFAULT_NAME,
};

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
