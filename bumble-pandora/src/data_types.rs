use crate::proto::{data_types, DataTypes, DiscoverabilityMode};
use bumble::advertising_data::Type;
use bumble::AdvertisingData;
use std::collections::HashMap;
use tonic::Status;

const PERIPHERAL_CONNECTION_INTERVAL_RANGE: Type = Type(0x12);
const SERVICE_SOLICITATION_16: Type = Type(0x14);
const SERVICE_SOLICITATION_128: Type = Type(0x15);
const SERVICE_DATA_16: Type = Type(0x16);
const PUBLIC_TARGET_ADDRESS: Type = Type(0x17);
const RANDOM_TARGET_ADDRESS: Type = Type(0x18);
const SERVICE_SOLICITATION_32: Type = Type(0x1F);
const SERVICE_DATA_32: Type = Type(0x20);
const SERVICE_DATA_128: Type = Type(0x21);
const LE_SUPPORTED_FEATURES: Type = Type(0x27);

fn uuid_bytes(value: &str, length: usize) -> Result<Vec<u8>, Status> {
    let compact = value.replace('-', "");
    if !compact.len().is_multiple_of(2) {
        return Err(Status::invalid_argument(format!(
            "invalid UUID {value:?}: odd hexadecimal length"
        )));
    }
    let mut bytes = compact
        .as_bytes()
        .chunks_exact(2)
        .map(|digits| {
            std::str::from_utf8(digits)
                .ok()
                .and_then(|digits| u8::from_str_radix(digits, 16).ok())
                .ok_or_else(|| Status::invalid_argument(format!("invalid UUID {value:?}")))
        })
        .collect::<Result<Vec<_>, _>>()?;
    if bytes.len() != length {
        return Err(Status::invalid_argument(format!(
            "UUID {value:?} must contain {} hexadecimal digits",
            length * 2
        )));
    }
    bytes.reverse();
    Ok(bytes)
}

fn uuid_string(value: &[u8]) -> String {
    let mut value = value.to_vec();
    value.reverse();
    let plain = value
        .iter()
        .map(|byte| format!("{byte:02X}"))
        .collect::<String>();
    if value.len() == 16 {
        format!(
            "{}-{}-{}-{}-{}",
            &plain[0..8],
            &plain[8..12],
            &plain[12..16],
            &plain[16..20],
            &plain[20..32]
        )
    } else {
        plain
    }
}

fn append_uuid_list(
    structures: &mut Vec<(Type, Vec<u8>)>,
    kind: Type,
    values: &[String],
    length: usize,
) -> Result<(), Status> {
    if !values.is_empty() {
        let mut data = Vec::with_capacity(values.len() * length);
        for value in values {
            data.extend_from_slice(&uuid_bytes(value, length)?);
        }
        structures.push((kind, data));
    }
    Ok(())
}

fn append_service_data(
    structures: &mut Vec<(Type, Vec<u8>)>,
    kind: Type,
    values: &HashMap<String, Vec<u8>>,
    length: usize,
) -> Result<(), Status> {
    let mut values = values.iter().collect::<Vec<_>>();
    values.sort_by(|left, right| left.0.cmp(right.0));
    for (uuid, value) in values {
        let mut data = uuid_bytes(uuid, length)?;
        data.extend_from_slice(value);
        structures.push((kind, data));
    }
    Ok(())
}

pub(crate) fn unpack(
    data_types: &DataTypes,
    local_name: &str,
    class_of_device: u32,
) -> Result<Vec<u8>, Status> {
    let mut structures = Vec::new();
    append_uuid_list(
        &mut structures,
        Type::INCOMPLETE_LIST_OF_16_BIT_SERVICE_CLASS_UUIDS,
        &data_types.incomplete_service_class_uuids16,
        2,
    )?;
    append_uuid_list(
        &mut structures,
        Type::COMPLETE_LIST_OF_16_BIT_SERVICE_CLASS_UUIDS,
        &data_types.complete_service_class_uuids16,
        2,
    )?;
    append_uuid_list(
        &mut structures,
        Type::INCOMPLETE_LIST_OF_32_BIT_SERVICE_CLASS_UUIDS,
        &data_types.incomplete_service_class_uuids32,
        4,
    )?;
    append_uuid_list(
        &mut structures,
        Type::COMPLETE_LIST_OF_32_BIT_SERVICE_CLASS_UUIDS,
        &data_types.complete_service_class_uuids32,
        4,
    )?;
    append_uuid_list(
        &mut structures,
        Type::INCOMPLETE_LIST_OF_128_BIT_SERVICE_CLASS_UUIDS,
        &data_types.incomplete_service_class_uuids128,
        16,
    )?;
    append_uuid_list(
        &mut structures,
        Type::COMPLETE_LIST_OF_128_BIT_SERVICE_CLASS_UUIDS,
        &data_types.complete_service_class_uuids128,
        16,
    )?;

    match data_types.shortened_local_name_oneof.as_ref() {
        Some(data_types::ShortenedLocalNameOneof::ShortenedLocalName(name)) => {
            structures.push((Type::SHORTENED_LOCAL_NAME, name.as_bytes().to_vec()));
        }
        Some(data_types::ShortenedLocalNameOneof::IncludeShortenedLocalName(true)) => {
            structures.push((
                Type::SHORTENED_LOCAL_NAME,
                local_name.as_bytes().iter().copied().take(8).collect(),
            ));
        }
        _ => {}
    }
    match data_types.complete_local_name_oneof.as_ref() {
        Some(data_types::CompleteLocalNameOneof::CompleteLocalName(name)) => {
            structures.push((Type::COMPLETE_LOCAL_NAME, name.as_bytes().to_vec()));
        }
        Some(data_types::CompleteLocalNameOneof::IncludeCompleteLocalName(true)) => {
            structures.push((Type::COMPLETE_LOCAL_NAME, local_name.as_bytes().to_vec()));
        }
        _ => {}
    }
    match data_types.tx_power_level_oneof {
        Some(data_types::TxPowerLevelOneof::TxPowerLevel(value)) => {
            let value = u8::try_from(value)
                .map_err(|_| Status::invalid_argument("TX power level exceeds one byte"))?;
            structures.push((Type::TX_POWER_LEVEL, vec![value]));
        }
        Some(data_types::TxPowerLevelOneof::IncludeTxPowerLevel(true)) => {
            return Err(Status::invalid_argument(
                "controller-selected TX power advertising data is unsupported",
            ));
        }
        _ => {}
    }
    match data_types.class_of_device_oneof {
        Some(data_types::ClassOfDeviceOneof::ClassOfDevice(value)) => {
            if value > 0x00FF_FFFF {
                return Err(Status::invalid_argument(
                    "class of device exceeds three bytes",
                ));
            }
            structures.push((Type::CLASS_OF_DEVICE, value.to_le_bytes()[..3].to_vec()));
        }
        Some(data_types::ClassOfDeviceOneof::IncludeClassOfDevice(true)) => {
            structures.push((
                Type::CLASS_OF_DEVICE,
                class_of_device.to_le_bytes()[..3].to_vec(),
            ));
        }
        _ => {}
    }
    if data_types.peripheral_connection_interval_min != 0 {
        let maximum = if data_types.peripheral_connection_interval_max == 0 {
            data_types.peripheral_connection_interval_min
        } else {
            data_types.peripheral_connection_interval_max
        };
        let minimum = u16::try_from(data_types.peripheral_connection_interval_min)
            .map_err(|_| Status::invalid_argument("connection interval minimum exceeds u16"))?;
        let maximum = u16::try_from(maximum)
            .map_err(|_| Status::invalid_argument("connection interval maximum exceeds u16"))?;
        let mut value = minimum.to_le_bytes().to_vec();
        value.extend_from_slice(&maximum.to_le_bytes());
        structures.push((PERIPHERAL_CONNECTION_INTERVAL_RANGE, value));
    }
    append_uuid_list(
        &mut structures,
        SERVICE_SOLICITATION_16,
        &data_types.service_solicitation_uuids16,
        2,
    )?;
    append_uuid_list(
        &mut structures,
        SERVICE_SOLICITATION_32,
        &data_types.service_solicitation_uuids32,
        4,
    )?;
    append_uuid_list(
        &mut structures,
        SERVICE_SOLICITATION_128,
        &data_types.service_solicitation_uuids128,
        16,
    )?;
    append_service_data(
        &mut structures,
        SERVICE_DATA_16,
        &data_types.service_data_uuid16,
        2,
    )?;
    append_service_data(
        &mut structures,
        SERVICE_DATA_32,
        &data_types.service_data_uuid32,
        4,
    )?;
    append_service_data(
        &mut structures,
        SERVICE_DATA_128,
        &data_types.service_data_uuid128,
        16,
    )?;
    if !data_types.public_target_addresses.is_empty() {
        if data_types
            .public_target_addresses
            .iter()
            .any(|address| address.len() != 6)
        {
            return Err(Status::invalid_argument(
                "public target addresses must contain six bytes",
            ));
        }
        structures.push((
            PUBLIC_TARGET_ADDRESS,
            data_types.public_target_addresses.concat(),
        ));
    }
    if !data_types.random_target_addresses.is_empty() {
        if data_types
            .random_target_addresses
            .iter()
            .any(|address| address.len() != 6)
        {
            return Err(Status::invalid_argument(
                "random target addresses must contain six bytes",
            ));
        }
        structures.push((
            RANDOM_TARGET_ADDRESS,
            data_types.random_target_addresses.concat(),
        ));
    }
    if data_types.appearance != 0 {
        let value = u16::try_from(data_types.appearance)
            .map_err(|_| Status::invalid_argument("appearance exceeds u16"))?;
        structures.push((Type::APPEARANCE, value.to_le_bytes().to_vec()));
    }
    match data_types.advertising_interval_oneof {
        Some(data_types::AdvertisingIntervalOneof::AdvertisingInterval(value)) => {
            let value = u16::try_from(value)
                .map_err(|_| Status::invalid_argument("advertising interval exceeds u16"))?;
            structures.push((Type::ADVERTISING_INTERVAL, value.to_le_bytes().to_vec()));
        }
        Some(data_types::AdvertisingIntervalOneof::IncludeAdvertisingInterval(true)) => {
            return Err(Status::invalid_argument(
                "controller-selected advertising interval data is unsupported",
            ));
        }
        _ => {}
    }
    if !data_types.uri.is_empty() {
        structures.push((Type::URI, data_types.uri.as_bytes().to_vec()));
    }
    if !data_types.le_supported_features.is_empty() {
        structures.push((
            LE_SUPPORTED_FEATURES,
            data_types.le_supported_features.clone(),
        ));
    }
    if !data_types.manufacturer_specific_data.is_empty() {
        structures.push((
            Type::MANUFACTURER_SPECIFIC_DATA,
            data_types.manufacturer_specific_data.clone(),
        ));
    }
    let flags = match DiscoverabilityMode::try_from(data_types.le_discoverability_mode) {
        Ok(DiscoverabilityMode::DiscoverableLimited) => 0x01,
        Ok(DiscoverabilityMode::DiscoverableGeneral) => 0x02,
        _ => 0,
    };
    if flags != 0 {
        structures.push((Type::FLAGS, vec![flags]));
    }
    if structures.iter().any(|(_, value)| value.len() > 254) {
        return Err(Status::invalid_argument(
            "an advertising-data structure exceeds 254 bytes",
        ));
    }
    Ok(AdvertisingData {
        ad_structures: structures,
    }
    .to_bytes())
}

fn unpack_uuid_list(data: &mut Vec<String>, value: &[u8], length: usize) {
    data.extend(value.chunks_exact(length).map(uuid_string));
}

pub(crate) fn pack(bytes: &[u8]) -> DataTypes {
    let advertising = AdvertisingData::from_bytes(bytes);
    let mut data = DataTypes::default();
    for (kind, value) in advertising.ad_structures {
        match kind {
            Type::INCOMPLETE_LIST_OF_16_BIT_SERVICE_CLASS_UUIDS => {
                unpack_uuid_list(&mut data.incomplete_service_class_uuids16, &value, 2)
            }
            Type::COMPLETE_LIST_OF_16_BIT_SERVICE_CLASS_UUIDS => {
                unpack_uuid_list(&mut data.complete_service_class_uuids16, &value, 2)
            }
            Type::INCOMPLETE_LIST_OF_32_BIT_SERVICE_CLASS_UUIDS => {
                unpack_uuid_list(&mut data.incomplete_service_class_uuids32, &value, 4)
            }
            Type::COMPLETE_LIST_OF_32_BIT_SERVICE_CLASS_UUIDS => {
                unpack_uuid_list(&mut data.complete_service_class_uuids32, &value, 4)
            }
            Type::INCOMPLETE_LIST_OF_128_BIT_SERVICE_CLASS_UUIDS => {
                unpack_uuid_list(&mut data.incomplete_service_class_uuids128, &value, 16)
            }
            Type::COMPLETE_LIST_OF_128_BIT_SERVICE_CLASS_UUIDS => {
                unpack_uuid_list(&mut data.complete_service_class_uuids128, &value, 16)
            }
            Type::SHORTENED_LOCAL_NAME => {
                data.shortened_local_name_oneof =
                    Some(data_types::ShortenedLocalNameOneof::ShortenedLocalName(
                        String::from_utf8_lossy(&value).into_owned(),
                    ));
            }
            Type::COMPLETE_LOCAL_NAME => {
                data.complete_local_name_oneof =
                    Some(data_types::CompleteLocalNameOneof::CompleteLocalName(
                        String::from_utf8_lossy(&value).into_owned(),
                    ));
            }
            Type::TX_POWER_LEVEL if !value.is_empty() => {
                data.tx_power_level_oneof = Some(data_types::TxPowerLevelOneof::TxPowerLevel(
                    u32::from(value[0]),
                ));
            }
            Type::CLASS_OF_DEVICE if value.len() >= 3 => {
                data.class_of_device_oneof = Some(data_types::ClassOfDeviceOneof::ClassOfDevice(
                    u32::from_le_bytes([value[0], value[1], value[2], 0]),
                ));
            }
            PERIPHERAL_CONNECTION_INTERVAL_RANGE if value.len() >= 4 => {
                data.peripheral_connection_interval_min =
                    u32::from(u16::from_le_bytes([value[0], value[1]]));
                data.peripheral_connection_interval_max =
                    u32::from(u16::from_le_bytes([value[2], value[3]]));
            }
            SERVICE_SOLICITATION_16 => {
                unpack_uuid_list(&mut data.service_solicitation_uuids16, &value, 2)
            }
            SERVICE_SOLICITATION_32 => {
                unpack_uuid_list(&mut data.service_solicitation_uuids32, &value, 4)
            }
            SERVICE_SOLICITATION_128 => {
                unpack_uuid_list(&mut data.service_solicitation_uuids128, &value, 16)
            }
            SERVICE_DATA_16 if value.len() >= 2 => {
                data.service_data_uuid16
                    .insert(uuid_string(&value[..2]), value[2..].to_vec());
            }
            SERVICE_DATA_32 if value.len() >= 4 => {
                data.service_data_uuid32
                    .insert(uuid_string(&value[..4]), value[4..].to_vec());
            }
            SERVICE_DATA_128 if value.len() >= 16 => {
                data.service_data_uuid128
                    .insert(uuid_string(&value[..16]), value[16..].to_vec());
            }
            PUBLIC_TARGET_ADDRESS => {
                data.public_target_addresses
                    .extend(value.chunks_exact(6).map(<[u8]>::to_vec));
            }
            RANDOM_TARGET_ADDRESS => {
                data.random_target_addresses
                    .extend(value.chunks_exact(6).map(<[u8]>::to_vec));
            }
            Type::APPEARANCE if value.len() >= 2 => {
                data.appearance = u32::from(u16::from_le_bytes([value[0], value[1]]));
            }
            Type::ADVERTISING_INTERVAL if value.len() >= 2 => {
                data.advertising_interval_oneof =
                    Some(data_types::AdvertisingIntervalOneof::AdvertisingInterval(
                        u32::from(u16::from_le_bytes([value[0], value[1]])),
                    ));
            }
            Type::URI => data.uri = String::from_utf8_lossy(&value).into_owned(),
            LE_SUPPORTED_FEATURES => data.le_supported_features = value,
            Type::MANUFACTURER_SPECIFIC_DATA => data.manufacturer_specific_data = value,
            Type::FLAGS if !value.is_empty() => {
                data.le_discoverability_mode = if value[0] & 0x01 != 0 {
                    DiscoverabilityMode::DiscoverableLimited as i32
                } else if value[0] & 0x02 != 0 {
                    DiscoverabilityMode::DiscoverableGeneral as i32
                } else {
                    DiscoverabilityMode::NotDiscoverable as i32
                };
            }
            _ => {}
        }
    }
    data
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn representative_data_types_round_trip() {
        let mut data = DataTypes {
            complete_service_class_uuids16: vec!["180D".into()],
            complete_service_class_uuids128: vec!["00112233-4455-6677-8899-AABBCCDDEEFF".into()],
            appearance: 0x1234,
            manufacturer_specific_data: vec![0x4C, 0x00, 1, 2],
            le_discoverability_mode: DiscoverabilityMode::DiscoverableGeneral as i32,
            ..Default::default()
        };
        data.complete_local_name_oneof =
            Some(data_types::CompleteLocalNameOneof::IncludeCompleteLocalName(true));
        data.service_data_uuid16.insert("FEAA".into(), vec![3, 4]);
        let encoded = unpack(&data, "Pandora", 0).unwrap();
        let decoded = pack(&encoded);
        assert_eq!(decoded.complete_service_class_uuids16, vec!["180D"]);
        assert_eq!(
            decoded.complete_service_class_uuids128,
            vec!["00112233-4455-6677-8899-AABBCCDDEEFF"]
        );
        assert_eq!(
            decoded.complete_local_name_oneof,
            Some(data_types::CompleteLocalNameOneof::CompleteLocalName(
                "Pandora".into()
            ))
        );
        assert_eq!(decoded.service_data_uuid16["FEAA"], vec![3, 4]);
        assert_eq!(decoded.appearance, 0x1234);
        assert_eq!(decoded.manufacturer_specific_data, vec![0x4C, 0, 1, 2]);
        assert_eq!(
            decoded.le_discoverability_mode,
            DiscoverabilityMode::DiscoverableGeneral as i32
        );
    }

    #[test]
    fn malformed_uuid_and_controller_selected_values_are_rejected() {
        let mut data = DataTypes {
            complete_service_class_uuids16: vec!["xyz".into()],
            ..Default::default()
        };
        assert!(unpack(&data, "Bumble", 0).is_err());
        data.complete_service_class_uuids16.clear();
        data.tx_power_level_oneof = Some(data_types::TxPowerLevelOneof::IncludeTxPowerLevel(true));
        assert!(unpack(&data, "Bumble", 0).is_err());
        data.tx_power_level_oneof = Some(data_types::TxPowerLevelOneof::TxPowerLevel(256));
        assert!(unpack(&data, "Bumble", 0).is_err());
        data.tx_power_level_oneof = None;
        data.class_of_device_oneof = Some(data_types::ClassOfDeviceOneof::ClassOfDevice(1 << 24));
        assert!(unpack(&data, "Bumble", 0).is_err());
        data.class_of_device_oneof = None;
        data.public_target_addresses.push(vec![1; 5]);
        assert!(unpack(&data, "Bumble", 0).is_err());
        data.public_target_addresses.clear();
        data.manufacturer_specific_data = vec![0; 255];
        assert!(unpack(&data, "Bumble", 0).is_err());
    }
}
