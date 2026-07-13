use bumble_hci::codes::*;
use bumble_hci::ReturnParameters;

fn round_trip(opcode: u16, expected: ReturnParameters, bytes: &[u8]) {
    assert_eq!(expected.to_bytes(), bytes);
    assert_eq!(ReturnParameters::parse(opcode, bytes).unwrap(), expected);
}

#[test]
fn classic_controller_information_returns_are_typed() {
    round_trip(
        HCI_READ_LOCAL_VERSION_INFORMATION_COMMAND,
        ReturnParameters::ReadLocalVersionInformation {
            status: 0,
            hci_version: 0x0D,
            hci_subversion: 0x1234,
            lmp_version: 0x0C,
            company_identifier: 0x004C,
            lmp_subversion: 0x5678,
        },
        &[0x00, 0x0D, 0x34, 0x12, 0x0C, 0x4C, 0x00, 0x78, 0x56],
    );

    let supported_commands = core::array::from_fn(|index| index as u8);
    let mut command_bytes = vec![0];
    command_bytes.extend(supported_commands);
    round_trip(
        HCI_READ_LOCAL_SUPPORTED_COMMANDS_COMMAND,
        ReturnParameters::ReadLocalSupportedCommands {
            status: 0,
            supported_commands,
        },
        &command_bytes,
    );
    round_trip(
        HCI_READ_LOCAL_SUPPORTED_FEATURES_COMMAND,
        ReturnParameters::ReadLocalSupportedFeatures {
            status: 0,
            lmp_features: [1, 2, 3, 4, 5, 6, 7, 8],
        },
        &[0, 1, 2, 3, 4, 5, 6, 7, 8],
    );
    round_trip(
        HCI_READ_BUFFER_SIZE_COMMAND,
        ReturnParameters::ReadBufferSize {
            status: 0,
            hc_acl_data_packet_length: 1021,
            hc_synchronous_data_packet_length: 64,
            hc_total_num_acl_data_packets: 10,
            hc_total_num_synchronous_data_packets: 4,
        },
        &[0, 0xFD, 0x03, 64, 10, 0, 4, 0],
    );
    round_trip(
        HCI_READ_VOICE_SETTING_COMMAND,
        ReturnParameters::ReadVoiceSetting {
            status: 0,
            voice_setting: 0x0060,
        },
        &[0, 0x60, 0],
    );
    round_trip(
        HCI_READ_LOOPBACK_MODE_COMMAND,
        ReturnParameters::ReadLoopbackMode {
            status: 0,
            loopback_mode: 1,
        },
        &[0, 1],
    );
}

#[test]
fn le_controller_information_returns_are_typed() {
    round_trip(
        HCI_LE_READ_BUFFER_SIZE_V2_COMMAND,
        ReturnParameters::LeReadBufferSizeV2 {
            status: 0,
            le_acl_data_packet_length: 251,
            total_num_le_acl_data_packets: 12,
            iso_data_packet_length: 960,
            total_num_iso_data_packets: 6,
        },
        &[0, 251, 0, 12, 0xC0, 0x03, 6],
    );
    round_trip(
        HCI_LE_READ_LOCAL_SUPPORTED_FEATURES_COMMAND,
        ReturnParameters::LeReadLocalSupportedFeatures {
            status: 0,
            le_features: [0xFF, 1, 2, 3, 4, 5, 6, 7],
        },
        &[0, 0xFF, 1, 2, 3, 4, 5, 6, 7],
    );
    round_trip(
        HCI_LE_READ_SUGGESTED_DEFAULT_DATA_LENGTH_COMMAND,
        ReturnParameters::LeReadSuggestedDefaultDataLength {
            status: 0,
            suggested_max_tx_octets: 251,
            suggested_max_tx_time: 2120,
        },
        &[0, 251, 0, 0x48, 0x08],
    );
    round_trip(
        HCI_LE_READ_MAXIMUM_DATA_LENGTH_COMMAND,
        ReturnParameters::LeReadMaximumDataLength {
            status: 0,
            supported_max_tx_octets: 251,
            supported_max_tx_time: 2120,
            supported_max_rx_octets: 251,
            supported_max_rx_time: 2120,
        },
        &[0, 251, 0, 0x48, 0x08, 251, 0, 0x48, 0x08],
    );
    round_trip(
        HCI_LE_READ_MAXIMUM_ADVERTISING_DATA_LENGTH_COMMAND,
        ReturnParameters::LeReadMaximumAdvertisingDataLength {
            status: 0,
            max_advertising_data_length: 1650,
        },
        &[0, 0x72, 0x06],
    );
    round_trip(
        HCI_LE_READ_NUMBER_OF_SUPPORTED_ADVERTISING_SETS_COMMAND,
        ReturnParameters::LeReadNumberOfSupportedAdvertisingSets {
            status: 0,
            num_supported_advertising_sets: 16,
        },
        &[0, 16],
    );
    round_trip(
        HCI_LE_READ_MINIMUM_SUPPORTED_CONNECTION_INTERVAL_COMMAND,
        ReturnParameters::LeReadMinimumSupportedConnectionInterval {
            status: 0,
            minimum_supported_connection_interval: 6,
            group_min: vec![0x18, 0x30],
            group_max: vec![0x28, 0x40],
            group_stride: vec![4, 8],
        },
        &[0, 6, 2, 0x18, 0, 0x28, 0, 4, 0, 0x30, 0, 0x40, 0, 8, 0],
    );
}

#[test]
fn typed_errors_fall_back_to_status_and_truncation_is_rejected() {
    let opcodes = [
        HCI_READ_LOCAL_VERSION_INFORMATION_COMMAND,
        HCI_READ_LOCAL_SUPPORTED_COMMANDS_COMMAND,
        HCI_READ_LOCAL_SUPPORTED_FEATURES_COMMAND,
        HCI_READ_BUFFER_SIZE_COMMAND,
        HCI_READ_VOICE_SETTING_COMMAND,
        HCI_LE_READ_BUFFER_SIZE_V2_COMMAND,
        HCI_LE_READ_LOCAL_SUPPORTED_FEATURES_COMMAND,
        HCI_LE_READ_SUGGESTED_DEFAULT_DATA_LENGTH_COMMAND,
        HCI_LE_READ_MAXIMUM_DATA_LENGTH_COMMAND,
        HCI_LE_READ_MAXIMUM_ADVERTISING_DATA_LENGTH_COMMAND,
        HCI_LE_READ_NUMBER_OF_SUPPORTED_ADVERTISING_SETS_COMMAND,
        HCI_LE_READ_MINIMUM_SUPPORTED_CONNECTION_INTERVAL_COMMAND,
    ];
    for opcode in opcodes {
        assert_eq!(
            ReturnParameters::parse(opcode, &[0x0C]).unwrap(),
            ReturnParameters::Status { status: 0x0C }
        );
        assert!(ReturnParameters::parse(opcode, &[0]).is_err());
    }
}
