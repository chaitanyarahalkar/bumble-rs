use bumble::{appearance::Category, Appearance, Uuid};
use bumble_gatt::{
    properties, CharacteristicDefinition, DescriptorDefinition, GattClient, GattError, GattServer,
    ServiceDefinition,
};
use bumble_profiles::battery_service::{BatteryService, BatteryServiceProxy};
use bumble_profiles::device_information_service::{
    DeviceInformationService, DeviceInformationServiceProxy,
};
use bumble_profiles::gap::{GenericAccessService, GenericAccessServiceProxy};
use bumble_profiles::gatt_service::{
    GenericAttributeProfileService, GenericAttributeProfileServiceProxy,
    DATABASE_HASH_CHARACTERISTIC,
};
use bumble_profiles::heart_rate_service::{
    BodySensorLocation, HeartRateMeasurement, HeartRateService, HeartRateServiceProxy,
    CONTROL_POINT_NOT_SUPPORTED,
};
use bumble_profiles::Error;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::Arc;

fn uuid(value: u16) -> Uuid {
    Uuid::from_16_bits(value)
}

#[test]
fn gap_service_truncates_name_and_typed_proxy_reads_values() {
    let appearance = Appearance::new(Category::COMPUTER, 3);
    let service = GenericAccessService::new("x".repeat(300), appearance);
    let definition = service.definition();
    assert_eq!(definition.characteristics[0].value.len(), 248);
    assert_eq!(definition.characteristics[1].value, appearance.to_bytes());

    let mut server = GattServer::from_definitions(vec![definition]).unwrap();
    let mut client = GattClient::new();
    let proxy = GenericAccessServiceProxy::discover(&mut client, &mut server)
        .unwrap()
        .unwrap();
    assert_eq!(
        proxy
            .device_name
            .unwrap()
            .read_value(&mut client, &mut server, false)
            .unwrap(),
        "x".repeat(248)
    );
    assert_eq!(
        proxy
            .appearance
            .unwrap()
            .read_value(&mut client, &mut server, false)
            .unwrap(),
        appearance
    );
}

#[test]
fn battery_service_reads_dynamic_level_and_exposes_notify_cccd() {
    let level = Arc::new(AtomicU8::new(1));
    let read_level = Arc::clone(&level);
    let service = BatteryService::new(move |_| read_level.load(Ordering::SeqCst));
    let mut server = GattServer::from_definitions(vec![service.definition()]).unwrap();
    let value_handle = service.bind(&mut server).unwrap();
    let mut client = GattClient::new();
    let proxy = BatteryServiceProxy::discover(&mut client, &mut server)
        .unwrap()
        .unwrap();
    assert_eq!(proxy.battery_level.proxy().handle, value_handle);
    assert_eq!(
        proxy
            .battery_level
            .read_value(&mut client, &mut server, false)
            .unwrap(),
        1
    );
    level.store(99, Ordering::SeqCst);
    assert_eq!(
        proxy
            .battery_level
            .read_value(&mut client, &mut server, false)
            .unwrap(),
        99
    );
    let descriptors = client
        .discover_descriptors(&mut server, proxy.battery_level.proxy())
        .unwrap();
    assert_eq!(descriptors.len(), 1);
    assert_eq!(descriptors[0].uuid, uuid(0x2902));
}

#[test]
fn device_information_system_id_and_optional_proxy_fields_round_trip() {
    let packed = DeviceInformationService::pack_system_id(0xA1_B2_C3, 0x12_3456_789A).unwrap();
    assert_eq!(
        DeviceInformationService::unpack_system_id(&packed).unwrap(),
        (0xA1_B2_C3, 0x12_3456_789A)
    );
    assert!(DeviceInformationService::pack_system_id(0x100_0000, 0).is_err());

    let service = DeviceInformationService {
        manufacturer_name: Some("Bumble".into()),
        model_number: Some("Rust-1".into()),
        system_id: Some((0xA1_B2_C3, 0x12_3456_789A)),
        ieee_regulatory_certification_data_list: Some(vec![1, 2, 3]),
        ..Default::default()
    };
    let mut server = GattServer::from_definitions(vec![service.definition().unwrap()]).unwrap();
    let mut client = GattClient::new();
    let proxy = DeviceInformationServiceProxy::discover(&mut client, &mut server)
        .unwrap()
        .unwrap();
    assert_eq!(
        proxy
            .manufacturer_name
            .unwrap()
            .read_value(&mut client, &mut server, false)
            .unwrap(),
        "Bumble"
    );
    assert_eq!(
        proxy
            .system_id
            .unwrap()
            .read_value(&mut client, &mut server, false)
            .unwrap(),
        (0xA1_B2_C3, 0x12_3456_789A)
    );
    assert!(proxy.serial_number.is_none());
    let regulatory = proxy.ieee_regulatory_certification_data_list.unwrap();
    assert_eq!(
        client
            .read_value(&mut server, regulatory.handle, false)
            .unwrap(),
        [1, 2, 3]
    );
}

#[test]
fn heart_rate_measurement_matches_all_upstream_flag_combinations() {
    for heart_rate in [1, 1000] {
        for sensor_contact in [Some(true), Some(false), None] {
            for energy in [Some(2), None] {
                for rr in [Some(vec![3.0, 4.0, 5.0]), None] {
                    let measurement = HeartRateMeasurement::try_new(
                        heart_rate,
                        sensor_contact,
                        energy,
                        rr.clone(),
                    )
                    .unwrap();
                    assert_eq!(
                        HeartRateMeasurement::decode(&measurement.encode()).unwrap(),
                        measurement
                    );
                }
            }
        }
    }
    assert!(HeartRateMeasurement::try_new(65536, None, None, None).is_err());
    assert!(HeartRateMeasurement::try_new(1, None, None, Some(vec![-1.0])).is_err());
    assert!(HeartRateMeasurement::decode(&[0x10, 1, 0]).is_err());
}

#[test]
fn heart_rate_live_read_location_and_control_point_reset() {
    let measurement =
        HeartRateMeasurement::try_new(1000, Some(true), Some(2), Some(vec![3.0, 4.0, 5.0]))
            .unwrap();
    let reset = Arc::new(AtomicBool::new(false));
    let reset_callback = Arc::clone(&reset);
    let expected = measurement.clone();
    let service = HeartRateService::new(move |_| expected.clone())
        .body_sensor_location(BodySensorLocation::FINGER)
        .reset_energy_expended(move |_| reset_callback.store(true, Ordering::SeqCst));
    let mut server = GattServer::from_definitions(vec![service.definition()]).unwrap();
    service.bind(&mut server).unwrap();
    let mut client = GattClient::new();
    let proxy = HeartRateServiceProxy::discover(&mut client, &mut server)
        .unwrap()
        .unwrap();
    assert_eq!(
        proxy
            .heart_rate_measurement
            .read_value(&mut client, &mut server, false)
            .unwrap(),
        measurement
    );
    assert_eq!(
        proxy
            .body_sensor_location
            .as_ref()
            .unwrap()
            .read_value(&mut client, &mut server, false)
            .unwrap(),
        BodySensorLocation::FINGER
    );
    proxy
        .reset_energy_expended(&mut client, &mut server)
        .unwrap();
    assert!(reset.load(Ordering::SeqCst));

    let control = proxy.heart_rate_control_point.unwrap();
    let error = control
        .write_value(&mut client, &mut server, &2, true)
        .unwrap_err();
    assert!(matches!(
        error,
        bumble_gatt::AdapterError::Gatt(GattError::Att { error_code, .. })
            if error_code == CONTROL_POINT_NOT_SUPPORTED
    ));
}

#[test]
fn gatt_database_hash_matches_upstream_vector_and_proxy_reads_it() {
    let gap = ServiceDefinition {
        uuid: uuid(0x1800),
        primary: true,
        included_services: Vec::new(),
        characteristics: vec![
            characteristic(0x2A00, properties::READ | properties::WRITE, 0x10, vec![]),
            characteristic(0x2A01, properties::READ, 0x10, vec![]),
        ],
    };
    let gatt = GenericAttributeProfileService::default();
    let glucose = ServiceDefinition {
        uuid: uuid(0x1808),
        primary: true,
        included_services: vec![3],
        characteristics: vec![CharacteristicDefinition {
            uuid: uuid(0x2A18),
            properties: properties::READ | properties::INDICATE | properties::EXTENDED_PROPERTIES,
            permissions: 0x10,
            value: Vec::new(),
            descriptors: vec![
                DescriptorDefinition {
                    uuid: uuid(0x2902),
                    permissions: 0x10,
                    value: vec![2, 0],
                },
                DescriptorDefinition {
                    uuid: uuid(0x2900),
                    permissions: 0x10,
                    value: vec![0, 0],
                },
            ],
        }],
    };
    let battery = ServiceDefinition {
        uuid: uuid(0x180F),
        primary: false,
        included_services: Vec::new(),
        characteristics: vec![characteristic(0x2A19, properties::READ, 0x10, vec![])],
    };
    let mut server =
        GattServer::from_definitions(vec![gap, gatt.definition(), glucose, battery]).unwrap();
    let expected = hex("F1CA2D48ECF58BAC8A8830BBB9FBA990");
    assert_eq!(server.database_hash(), expected.as_slice());
    let hash_handle = gatt.bind_database_hash(&mut server).unwrap().unwrap();
    assert_eq!(
        server.handles_by_uuid(&uuid(DATABASE_HASH_CHARACTERISTIC)),
        [hash_handle]
    );

    let mut client = GattClient::new();
    let proxy = GenericAttributeProfileServiceProxy::discover(&mut client, &mut server)
        .unwrap()
        .unwrap();
    let hash = proxy.database_hash_characteristic.unwrap();
    assert_eq!(
        client.read_value(&mut server, hash.handle, false).unwrap(),
        expected
    );
    assert!(proxy.service_changed_characteristic.is_some());
    assert!(proxy.client_supported_features_characteristic.is_some());
    assert!(proxy.server_supported_features_characteristic.is_none());
}

#[test]
fn profile_discovery_returns_none_when_service_is_absent() {
    let mut server = GattServer::from_definitions(Vec::new()).unwrap();
    let mut client = GattClient::new();
    assert!(BatteryServiceProxy::discover(&mut client, &mut server)
        .unwrap()
        .is_none());
    let error = Error::MissingCharacteristic(0x2A19);
    assert!(error.to_string().contains("2A19"));
}

fn characteristic(
    characteristic_uuid: u16,
    characteristic_properties: u8,
    characteristic_permissions: u8,
    value: Vec<u8>,
) -> CharacteristicDefinition {
    CharacteristicDefinition {
        uuid: uuid(characteristic_uuid),
        properties: characteristic_properties,
        permissions: characteristic_permissions,
        value,
        descriptors: Vec::new(),
    }
}

fn hex(value: &str) -> Vec<u8> {
    (0..value.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(&value[index..index + 2], 16).unwrap())
        .collect()
}
