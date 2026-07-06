//! Acceptance suite — the 7 Bumble Python tests ported 1:1 through the crate's
//! public API. This is the contract mirror of `tests/core_test.py` and
//! `tests/hci_test.py::test_address` from google/bumble.

use std::collections::HashSet;

use bumble::advertising_data::Type as AdType;
use bumble::appearance::Category;
use bumble::class_of_device::{MajorDeviceClass, MajorServiceClasses};
use bumble::{
    get_dict_key_by_value, Address, AddressType, AdvertisingData, Appearance, ClassOfDevice, Uuid,
};

// core_test.py::test_ad_data
#[test]
fn test_ad_data() {
    let data = vec![2u8, AdType::TX_POWER_LEVEL.0, 123];
    let mut ad = AdvertisingData::from_bytes(&data);
    assert_eq!(ad.to_bytes(), data);
    assert_eq!(ad.get(AdType::COMPLETE_LOCAL_NAME), None);
    assert_eq!(ad.get(AdType::TX_POWER_LEVEL), Some(vec![123]));
    assert_eq!(
        ad.get_all(AdType::COMPLETE_LOCAL_NAME),
        Vec::<Vec<u8>>::new()
    );
    assert_eq!(ad.get_all(AdType::TX_POWER_LEVEL), vec![vec![123]]);

    let data2 = vec![2u8, AdType::TX_POWER_LEVEL.0, 234];
    ad.append(&data2);
    let mut combined = data.clone();
    combined.extend_from_slice(&data2);
    assert_eq!(ad.to_bytes(), combined);
    assert_eq!(ad.get(AdType::COMPLETE_LOCAL_NAME), None);
    assert_eq!(ad.get(AdType::TX_POWER_LEVEL), Some(vec![123]));
    assert_eq!(
        ad.get_all(AdType::COMPLETE_LOCAL_NAME),
        Vec::<Vec<u8>>::new()
    );
    assert_eq!(
        ad.get_all(AdType::TX_POWER_LEVEL),
        vec![vec![123], vec![234]]
    );
}

// core_test.py::test_get_dict_key_by_value
#[test]
fn test_get_dict_key_by_value() {
    let dictionary = [("A", 1), ("B", 2)];
    assert_eq!(get_dict_key_by_value(&dictionary, &1), Some("A"));
    assert_eq!(get_dict_key_by_value(&dictionary, &2), Some("B"));
    assert_eq!(get_dict_key_by_value(&dictionary, &3), None);
}

// core_test.py::test_uuid_to_hex_str
#[test]
fn test_uuid_to_hex_str() {
    assert_eq!(Uuid::parse("b5ea").unwrap().to_hex_str(""), "B5EA");
    assert_eq!(Uuid::parse("df5ce654").unwrap().to_hex_str(""), "DF5CE654");
    assert_eq!(
        Uuid::parse("df5ce654-e059-11ed-b5ea-0242ac120002")
            .unwrap()
            .to_hex_str(""),
        "DF5CE654E05911EDB5EA0242AC120002"
    );
    assert_eq!(Uuid::parse("b5ea").unwrap().to_hex_str("-"), "B5EA");
    assert_eq!(Uuid::parse("df5ce654").unwrap().to_hex_str("-"), "DF5CE654");
    assert_eq!(
        Uuid::parse("df5ce654-e059-11ed-b5ea-0242ac120002")
            .unwrap()
            .to_hex_str("-"),
        "DF5CE654-E059-11ED-B5EA-0242AC120002"
    );
}

// core_test.py::test_uuid_hash
#[test]
fn test_uuid_hash() {
    let uuid = Uuid::parse("1234").unwrap();
    let uuid_128 = Uuid::from_bytes(&uuid.to_bytes(true)).unwrap();

    let set: HashSet<Uuid> = [uuid_128.clone()].into_iter().collect();
    assert!(set.contains(&uuid));

    let set2: HashSet<Uuid> = [uuid.clone()].into_iter().collect();
    assert!(set2.contains(&uuid_128));
}

// core_test.py::test_appearance
#[test]
fn test_appearance() {
    let a = Appearance::new(Category::COMPUTER, 0x03 /* LAPTOP */);
    assert_eq!(a.to_string(), "COMPUTER/LAPTOP");
    assert_eq!(a.to_int(), 0x0083);

    let a = Appearance::new(Category::HUMAN_INTERFACE_DEVICE, 0x77);
    assert_eq!(
        a.to_string(),
        "HUMAN_INTERFACE_DEVICE/HumanInterfaceDeviceSubcategory[119]"
    );
    assert_eq!(a.to_int(), 0x03C0 | 0x77);

    let a = Appearance::from_int(0x0381);
    assert_eq!(a.category(), Category::BLOOD_PRESSURE);
    assert_eq!(a.subcategory(), 0x01 /* ARM_BLOOD_PRESSURE */);
    assert_eq!(a.to_int(), 0x381);

    let a = Appearance::from_int(0x038A);
    assert_eq!(a.category(), Category::BLOOD_PRESSURE);
    assert_eq!(a.subcategory(), 0x0A);
    assert_eq!(a.to_int(), 0x038A);

    let a = Appearance::from_int(0x3333);
    assert_eq!(a.category(), Category(0xCC));
    assert_eq!(a.subcategory(), 0x33);
    assert_eq!(a.to_int(), 0x3333);
}

// core_test.py::test_class_of_device
#[test]
fn test_class_of_device() {
    let c1 = ClassOfDevice::new(
        MajorServiceClasses::AUDIO | MajorServiceClasses::RENDERING,
        MajorDeviceClass::AUDIO_VIDEO,
        0x0D, // CAMCORDER
    );
    assert_eq!(
        c1.to_string(),
        "ClassOfDevice(RENDERING|AUDIO,AUDIO_VIDEO/CAMCORDER)"
    );

    let c2 = ClassOfDevice::new(
        MajorServiceClasses::AUDIO,
        MajorDeviceClass::AUDIO_VIDEO,
        0x123,
    );
    assert_eq!(c2.to_string(), "ClassOfDevice(AUDIO,AUDIO_VIDEO/0x123)");
}

// hci_test.py::test_address
#[test]
fn test_address() {
    let a = Address::parse("C4:F2:17:1A:1D:BB", AddressType::RANDOM_DEVICE).unwrap();
    assert!(!a.is_public());
    assert!(a.is_random());
    assert_eq!(a.address_type(), AddressType::RANDOM_DEVICE);
    assert!(!a.is_resolvable());
    assert!(!a.is_resolved());
    assert!(a.is_static());
}
