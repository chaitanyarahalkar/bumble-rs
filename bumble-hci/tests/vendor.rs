use bumble_hci::vendor::{android, zephyr};
use bumble_hci::{Event, HciPacket};

#[test]
fn android_vendor_commands_have_exact_hci_envelopes() {
    assert_eq!(
        android::LeGetVendorCapabilitiesCommand
            .to_command()
            .to_bytes(),
        [0x01, 0x53, 0xFD, 0x00]
    );
    assert_eq!(
        android::LeApcfCommand {
            opcode: android::LeApcfOpcode::SERVICE_UUID,
            payload: vec![0x0A, 0x0B],
        }
        .to_command()
        .to_bytes(),
        [0x01, 0x57, 0xFD, 0x03, 0x03, 0x0A, 0x0B]
    );
    assert_eq!(
        android::GetControllerActivityEnergyInfoCommand
            .to_command()
            .to_bytes(),
        [0x01, 0x59, 0xFD, 0x00]
    );
    assert_eq!(
        android::A2dpHardwareOffloadCommand {
            opcode: android::A2dpHardwareOffloadOpcode::START_A2DP_OFFLOAD,
            payload: vec![1, 2, 3],
        }
        .to_command()
        .to_bytes(),
        [0x01, 0x5D, 0xFD, 0x04, 0x01, 1, 2, 3]
    );
    assert_eq!(
        android::DynamicAudioBufferCommand {
            opcode: android::DynamicAudioBufferOpcode::GET_AUDIO_BUFFER_TIME_CAPABILITY,
            payload: vec![0xAA],
        }
        .to_command()
        .to_bytes(),
        [0x01, 0x5F, 0xFD, 0x02, 0x01, 0xAA]
    );
}

#[test]
fn android_capabilities_accept_every_historical_prefix() {
    let old =
        android::LeGetVendorCapabilitiesReturnParameters::parse(&[0x00, 0x07, 0x01, 0x34, 0x12]);
    assert_eq!(old.status, 0);
    assert_eq!(old.max_advt_instances, 7);
    assert_eq!(old.offloaded_resolution_of_private_address, 1);
    assert_eq!(old.total_scan_results_storage, 0x1234);
    assert_eq!(old.max_irk_list_sz, 0);

    let latest = android::LeGetVendorCapabilitiesReturnParameters::parse(&[
        0, 1, 2, 0x04, 0x03, 5, 6, 7, 8, 0x0A, 0x09, 0x0C, 0x0B, 13, 14, 15, 0x13, 0x12, 0x11,
        0x10, 20, 0x18, 0x17, 0x16, 0x15,
    ]);
    assert_eq!(latest.version_supported, 0x090A);
    assert_eq!(latest.total_num_of_advt_tracked, 0x0B0C);
    assert_eq!(latest.a2dp_source_offload_capability_mask, 0x10111213);
    assert_eq!(latest.bluetooth_quality_report_support, 20);
    assert_eq!(latest.dynamic_audio_buffer_support, 0x15161718);
}

#[test]
fn android_return_parameters_are_typed_and_bounded() {
    let apcf = android::LeApcfReturnParameters::parse(&[0, 0xFF, 1, 2]).unwrap();
    assert_eq!(apcf.opcode, android::LeApcfOpcode::READ_EXTENDED_FEATURES);
    assert_eq!(apcf.payload, [1, 2]);

    let energy = android::GetControllerActivityEnergyInfoReturnParameters::parse(&[
        0, 1, 0, 0, 0, 2, 0, 0, 0, 3, 0, 0, 0, 4, 0, 0, 0,
    ])
    .unwrap();
    assert_eq!(energy.total_tx_time_ms, 1);
    assert_eq!(energy.total_rx_time_ms, 2);
    assert_eq!(energy.total_idle_time_ms, 3);
    assert_eq!(energy.total_energy_used, 4);
    assert!(android::LeApcfReturnParameters::parse(&[0]).is_err());
    assert!(android::GetControllerActivityEnergyInfoReturnParameters::parse(&[0; 16]).is_err());
}

fn quality_report_parameters(report_id: u8) -> Vec<u8> {
    let mut data = vec![android::HCI_BLUETOOTH_QUALITY_REPORT_EVENT, report_id, 0x22];
    data.extend_from_slice(&0x1234u16.to_le_bytes());
    data.extend_from_slice(&[1, (-7i8) as u8, (-55i8) as u8, 9, 10, 11]);
    data.extend_from_slice(&0x5678u16.to_le_bytes());
    for value in 1u32..=9 {
        data.extend_from_slice(&value.to_le_bytes());
    }
    data.extend_from_slice(&[1, 2, 3, 4, 5, 6]);
    data.push(12);
    for value in 10u32..=16 {
        data.extend_from_slice(&value.to_le_bytes());
    }
    data.extend_from_slice(&[0xAA, 0xBB]);
    data
}

#[test]
fn android_quality_report_decodes_from_generic_vendor_event() {
    let parameters = quality_report_parameters(0x07);
    let mut packet = vec![0x04, 0xFF, parameters.len() as u8];
    packet.extend_from_slice(&parameters);
    let HciPacket::Event(event @ Event::Vendor { .. }) = HciPacket::from_bytes(&packet).unwrap()
    else {
        panic!("expected HCI vendor event");
    };
    let report = android::BluetoothQualityReportEvent::from_event(&event)
        .unwrap()
        .unwrap();
    assert_eq!(report.quality_report_id, 0x07);
    assert_eq!(report.connection_handle, 0x1234);
    assert_eq!(report.tx_power_level, -7);
    assert_eq!(report.rssi, -55);
    assert_eq!(report.connection_piconet_clock, 1);
    assert_eq!(report.buffer_underflow_bytes, 9);
    assert_eq!(report.bdaddr.address_bytes(), &[1, 2, 3, 4, 5, 6]);
    assert_eq!(report.tx_total_packets, 10);
    assert_eq!(report.rx_unreceived_packets, 16);
    assert_eq!(report.vendor_specific_parameters, [0xAA, 0xBB]);

    assert!(
        android::BluetoothQualityReportEvent::parse_vendor_parameters(&quality_report_parameters(
            0x06
        ))
        .unwrap()
        .is_none()
    );
    assert!(android::BluetoothQualityReportEvent::parse_vendor_parameters(&[0x58, 0x01]).is_err());
}

#[test]
fn zephyr_tx_power_commands_and_returns_are_exact() {
    assert_eq!(
        zephyr::WriteTxPowerLevelCommand {
            handle_type: zephyr::TX_POWER_HANDLE_TYPE_CONN,
            connection_handle: 0x1234,
            tx_power_level: -7,
        }
        .to_command()
        .to_bytes(),
        [0x01, 0x0E, 0xFC, 0x04, 0x02, 0x34, 0x12, 0xF9]
    );
    assert_eq!(
        zephyr::ReadTxPowerLevelCommand {
            handle_type: zephyr::TX_POWER_HANDLE_TYPE_ADV,
            connection_handle: 0,
        }
        .to_command()
        .to_bytes(),
        [0x01, 0x0F, 0xFC, 0x03, 0x00, 0x00, 0x00]
    );

    let written = zephyr::WriteTxPowerLevelReturnParameters::parse(&[
        0,
        zephyr::TX_POWER_HANDLE_TYPE_CONN,
        0x34,
        0x12,
        0xF8,
    ])
    .unwrap();
    assert_eq!(written.connection_handle, 0x1234);
    assert_eq!(written.selected_tx_power_level, -8);
    let read = zephyr::ReadTxPowerLevelReturnParameters::parse(&[
        0,
        zephyr::TX_POWER_HANDLE_TYPE_SCAN,
        0,
        0,
        9,
    ])
    .unwrap();
    assert_eq!(read.tx_power_level, 9);
    assert!(zephyr::ReadTxPowerLevelReturnParameters::parse(&[0; 4]).is_err());
}
