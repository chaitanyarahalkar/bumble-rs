//! HCI acceptance suite. Ported from google/bumble `tests/hci_test.py`.
//!
//! Each test asserts the serialized bytes against a ground-truth hex literal
//! captured from real Python Bumble (`bytes(x).hex()`) — the load-bearing
//! correctness check. It then verifies that parsing round-trips to the same
//! wire bytes and dispatches to the same typed variant (mirroring, and in fact
//! strengthening, Bumble's `basic_check`). Wire bytes — not struct equality —
//! are the round-trip oracle here, because an address's type qualifier is not
//! carried on the wire for these fields.

use bumble::{Address, AddressType};
use bumble_hci::{
    map_null_terminated_utf8_string, AdvertisingReport, CodingFormat, Command, Event,
    ExtendedAdvertisingReport, HciPacket, IsoDataPacket, LeMetaEvent, ReturnParameters,
};

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn unhex(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}

/// Do two packets dispatch to the same typed variant (packet-level, and the
/// command/event/LE-meta inner variant)?
fn same_variant(a: &HciPacket, b: &HciPacket) -> bool {
    use std::mem::discriminant as d;
    if d(a) != d(b) {
        return false;
    }
    match (a, b) {
        (HciPacket::Command(x), HciPacket::Command(y)) => d(x) == d(y),
        (HciPacket::Event(x), HciPacket::Event(y)) => {
            d(x) == d(y)
                && match (x, y) {
                    (Event::LeMeta(p), Event::LeMeta(q)) => d(p) == d(q),
                    _ => true,
                }
        }
        _ => true,
    }
}

/// Serialize, compare to the Python oracle bytes, then parse and confirm the
/// round-trip is byte-stable and dispatches to the same typed variant.
fn check(packet: HciPacket, expected_hex: &str) {
    let bytes = packet.to_bytes();
    assert_eq!(hex(&bytes), expected_hex, "serialization vs Python oracle");
    let parsed = HciPacket::from_bytes(&bytes).unwrap();
    assert_eq!(parsed.to_bytes(), bytes, "round-trip must be byte-stable");
    assert!(
        same_variant(&parsed, &packet),
        "must dispatch to the same typed variant; got {parsed:?}"
    );
}

fn addr(s: &str) -> Address {
    Address::parse(s, AddressType::RANDOM_DEVICE).unwrap()
}

// hci_test.py::test_HCI_Event (generic events)
#[test]
fn test_hci_event() {
    check(
        HciPacket::Event(Event::Generic {
            event_code: 0xF9,
            parameters: vec![],
        }),
        "04f900",
    );
    check(
        HciPacket::Event(Event::Generic {
            event_code: 0xF8,
            parameters: unhex("aabbcc"),
        }),
        "04f803aabbcc",
    );
}

// hci_test.py::test_HCI_Command (generic command)
#[test]
fn test_hci_command() {
    check(
        HciPacket::Command(Command::Generic {
            op_code: 0x5566,
            parameters: vec![],
        }),
        "01665500",
    );
    check(
        HciPacket::Command(Command::Generic {
            op_code: 0x5566,
            parameters: unhex("aabbcc"),
        }),
        "01665503aabbcc",
    );
}

// hci_test.py::test_custom_command (unregistered op code -> Generic)
#[test]
fn test_custom_command() {
    check(
        HciPacket::Command(Command::Generic {
            op_code: 0x7788,
            parameters: vec![],
        }),
        "01887700",
    );
}

// hci_test.py::test_custom_event (unregistered event code -> Generic)
#[test]
fn test_custom_event() {
    check(
        HciPacket::Event(Event::Generic {
            event_code: 0x99,
            parameters: vec![],
        }),
        "049900",
    );
}

// hci_test.py::test_custom_le_meta_event (unregistered sub-event -> LeMeta Generic)
#[test]
fn test_custom_le_meta_event() {
    check(
        HciPacket::Event(Event::LeMeta(LeMetaEvent::Generic {
            subevent_code: 0xFF,
            parameters: vec![],
        })),
        "043e01ff",
    );
}

// hci_test.py::test_HCI_Reset_Command
#[test]
fn test_hci_reset_command() {
    check(HciPacket::Command(Command::Reset), "01030c00");
}

// hci_test.py::test_HCI_Disconnect_Command
#[test]
fn test_hci_disconnect_command() {
    check(
        HciPacket::Command(Command::Disconnect {
            connection_handle: 0x0002,
            reason: 0x13,
        }),
        "01060403020013",
    );
}

// hci_test.py::test_HCI_PIN_Code_Request_Reply_Command
#[test]
fn test_hci_pin_code_request_reply_command() {
    let mut pin_code = [0u8; 16];
    pin_code[..4].copy_from_slice(b"1234");
    check(
        HciPacket::Command(Command::PinCodeRequestReply {
            bd_addr: addr("00:11:22:33:44:55"),
            pin_code_length: 4,
            pin_code,
        }),
        "010d04175544332211000431323334000000000000000000000000",
    );
}

// hci_test.py::test_HCI_Set_Event_Mask_Command
#[test]
fn test_hci_set_event_mask_command() {
    check(
        HciPacket::Command(Command::SetEventMask {
            event_mask: [0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77],
        }),
        "01010c080011223344556677",
    );
}

// hci_test.py::test_HCI_LE_Set_Event_Mask_Command
#[test]
fn test_hci_le_set_event_mask_command() {
    check(
        HciPacket::Command(Command::LeSetEventMask {
            le_event_mask: [0x01, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00],
        }),
        "010120080100000000010000",
    );
}

// hci_test.py::test_HCI_LE_Set_Random_Address_Command
#[test]
fn test_hci_le_set_random_address_command() {
    check(
        HciPacket::Command(Command::LeSetRandomAddress {
            random_address: addr("00:11:22:33:44:55"),
        }),
        "01052006554433221100",
    );
}

// hci_test.py::test_HCI_LE_Set_Advertising_Parameters_Command
#[test]
fn test_hci_le_set_advertising_parameters_command() {
    check(
        HciPacket::Command(Command::LeSetAdvertisingParameters {
            advertising_interval_min: 20,
            advertising_interval_max: 30,
            advertising_type: 0x03, // ADV_NONCONN_IND
            own_address_type: 0,    // PUBLIC_DEVICE
            peer_address_type: 1,   // RANDOM_DEVICE
            peer_address: addr("00:11:22:33:44:55"),
            advertising_channel_map: 0x03,
            advertising_filter_policy: 1,
        }),
        "0106200f14001e000300015544332211000301",
    );
}

// hci_test.py::test_HCI_LE_Set_Advertising_Data_Command
#[test]
fn test_hci_le_set_advertising_data_command() {
    check(
        HciPacket::Command(Command::LeSetAdvertisingData {
            advertising_data: unhex("aabbcc"),
        }),
        "0108202003aabbcc00000000000000000000000000000000000000000000000000000000",
    );
}

// HCI_LE_Set_Advertising_Enable (used by the slice-3 controller scenario)
#[test]
fn test_hci_le_set_advertising_enable_command() {
    check(
        HciPacket::Command(Command::LeSetAdvertisingEnable {
            advertising_enable: 1,
        }),
        "010a200101",
    );
}

// hci_test.py::test_HCI_LE_Set_Scan_Parameters_Command
#[test]
fn test_hci_le_set_scan_parameters_command() {
    check(
        HciPacket::Command(Command::LeSetScanParameters {
            le_scan_type: 1,
            le_scan_interval: 20,
            le_scan_window: 10,
            own_address_type: 1,
            scanning_filter_policy: 0,
        }),
        "010b20070114000a000100",
    );
}

// hci_test.py::test_HCI_LE_Set_Scan_Enable_Command
#[test]
fn test_hci_le_set_scan_enable_command() {
    check(
        HciPacket::Command(Command::LeSetScanEnable {
            le_scan_enable: 1,
            filter_duplicates: 0,
        }),
        "010c20020100",
    );
}

// hci_test.py::test_HCI_LE_Create_Connection_Command
#[test]
fn test_hci_le_create_connection_command() {
    check(
        HciPacket::Command(Command::LeCreateConnection {
            le_scan_interval: 4,
            le_scan_window: 5,
            initiator_filter_policy: 1,
            peer_address_type: 1,
            peer_address: addr("00:11:22:33:44:55"),
            own_address_type: 2,
            connection_interval_min: 7,
            connection_interval_max: 8,
            max_latency: 9,
            supervision_timeout: 10,
            min_ce_length: 11,
            max_ce_length: 12,
        }),
        "010d2019040005000101554433221100020700080009000a000b000c00",
    );
}

// hci_test.py::test_HCI_LE_Extended_Create_Connection_Command
#[test]
fn test_hci_le_extended_create_connection_command() {
    check(
        HciPacket::Command(Command::LeExtendedCreateConnection {
            initiator_filter_policy: 0,
            own_address_type: 0,
            peer_address_type: 1,
            peer_address: addr("00:11:22:33:44:55"),
            initiating_phys: 3,
            scan_intervals: vec![10, 11],
            scan_windows: vec![12, 13],
            connection_interval_mins: vec![14, 15],
            connection_interval_maxs: vec![16, 17],
            max_latencies: vec![18, 19],
            supervision_timeouts: vec![20, 21],
            min_ce_lengths: vec![100, 101],
            max_ce_lengths: vec![102, 103],
        }),
        "0143202a000001554433221100030a000c000e00100012001400640066000b000d000f0011001300150065006700",
    );
}

// hci_test.py::test_HCI_LE_Add_Device_To_Filter_Accept_List_Command
#[test]
fn test_hci_le_add_device_to_filter_accept_list_command() {
    check(
        HciPacket::Command(Command::LeAddDeviceToFilterAcceptList {
            address_type: 1,
            address: addr("00:11:22:33:44:55"),
        }),
        "0111200701554433221100",
    );
}

// hci_test.py::test_HCI_LE_Remove_Device_From_Filter_Accept_List_Command
#[test]
fn test_hci_le_remove_device_from_filter_accept_list_command() {
    check(
        HciPacket::Command(Command::LeRemoveDeviceFromFilterAcceptList {
            address_type: 1,
            address: addr("00:11:22:33:44:55"),
        }),
        "0112200701554433221100",
    );
}

// hci_test.py::test_HCI_LE_Connection_Update_Command
#[test]
fn test_hci_le_connection_update_command() {
    check(
        HciPacket::Command(Command::LeConnectionUpdate {
            connection_handle: 0x0002,
            connection_interval_min: 10,
            connection_interval_max: 20,
            max_latency: 7,
            supervision_timeout: 3,
            min_ce_length: 100,
            max_ce_length: 200,
        }),
        "0113200e02000a001400070003006400c800",
    );
}

// hci_test.py::test_HCI_LE_Read_Remote_Features_Command
#[test]
fn test_hci_le_read_remote_features_command() {
    check(
        HciPacket::Command(Command::LeReadRemoteFeatures {
            connection_handle: 0x0002,
        }),
        "011620020200",
    );
}

// hci_test.py::test_HCI_LE_Set_Default_PHY_Command
#[test]
fn test_hci_le_set_default_phy_command() {
    check(
        HciPacket::Command(Command::LeSetDefaultPhy {
            all_phys: 0,
            tx_phys: 1,
            rx_phys: 1,
        }),
        "01312003000101",
    );
}

// hci_test.py::test_HCI_LE_Set_Extended_Scan_Parameters_Command
#[test]
fn test_hci_le_set_extended_scan_parameters_command() {
    check(
        HciPacket::Command(Command::LeSetExtendedScanParameters {
            own_address_type: 1, // RANDOM_DEVICE
            scanning_filter_policy: 1,
            scanning_phys: 0x15, // bits 0,2,4
            scan_types: vec![1, 1, 0],
            scan_intervals: vec![1, 2, 3],
            scan_windows: vec![4, 5, 6],
        }),
        "01412012010115010100040001020005000003000600",
    );
}

// hci_test.py::test_HCI_LE_Set_Extended_Advertising_Enable_Command
#[test]
fn test_hci_le_set_extended_advertising_enable_command() {
    // Parse from the exact wire bytes the upstream test uses, then check fields.
    let wire = unhex("0139200e010301050008020600090307000a");
    let parsed = HciPacket::from_bytes(&wire).unwrap();
    match &parsed {
        HciPacket::Command(Command::LeSetExtendedAdvertisingEnable {
            enable,
            advertising_handles,
            durations,
            max_extended_advertising_events,
        }) => {
            assert_eq!(*enable, 1);
            assert_eq!(advertising_handles, &vec![1, 2, 3]);
            assert_eq!(durations, &vec![5, 6, 7]);
            assert_eq!(max_extended_advertising_events, &vec![8, 9, 10]);
        }
        other => panic!("expected LeSetExtendedAdvertisingEnable, got {other:?}"),
    }
    assert_eq!(parsed.to_bytes(), wire);

    check(
        HciPacket::Command(Command::LeSetExtendedAdvertisingEnable {
            enable: 1,
            advertising_handles: vec![1, 2, 3],
            durations: vec![5, 6, 7],
            max_extended_advertising_events: vec![8, 9, 10],
        }),
        "0139200e010301050008020600090307000a",
    );
}

// hci_test.py::test_HCI_LE_Setup_ISO_Data_Path_Command
#[test]
fn test_hci_le_setup_iso_data_path_command() {
    // Parse from the exact wire bytes the upstream test uses, then check fields.
    let wire = unhex("016e200d60000001030000000000000000");
    let parsed = HciPacket::from_bytes(&wire).unwrap();
    match &parsed {
        HciPacket::Command(Command::LeSetupIsoDataPath {
            connection_handle,
            data_path_direction,
            data_path_id,
            codec_id,
            controller_delay,
            codec_configuration,
        }) => {
            assert_eq!(*connection_handle, 0x0060);
            assert_eq!(*data_path_direction, 0x00);
            assert_eq!(*data_path_id, 0x01);
            assert_eq!(*codec_id, CodingFormat::TRANSPARENT);
            assert_eq!(*controller_delay, 0);
            assert!(codec_configuration.is_empty());
        }
        other => panic!("expected LeSetupIsoDataPath, got {other:?}"),
    }
    assert_eq!(parsed.to_bytes(), wire);

    check(
        HciPacket::Command(Command::LeSetupIsoDataPath {
            connection_handle: 0x0060,
            data_path_direction: 0x00,
            data_path_id: 0x01,
            codec_id: CodingFormat::TRANSPARENT,
            controller_delay: 0,
            codec_configuration: vec![],
        }),
        "016e200d60000001030000000000000000",
    );
}

// hci_test.py::test_HCI_Read_Local_Version_Information_Command
#[test]
fn test_hci_read_local_version_information_command() {
    check(
        HciPacket::Command(Command::ReadLocalVersionInformation),
        "01011000",
    );
}

// hci_test.py::test_HCI_Read_Local_Supported_Commands_Command
#[test]
fn test_hci_read_local_supported_commands_command() {
    check(
        HciPacket::Command(Command::ReadLocalSupportedCommands),
        "01021000",
    );
}

// hci_test.py::test_HCI_Read_Local_Supported_Features_Command
#[test]
fn test_hci_read_local_supported_features_command() {
    check(
        HciPacket::Command(Command::ReadLocalSupportedFeatures),
        "01031000",
    );
}

// hci_test.py::test_HCI_Command_Complete_Event
#[test]
fn test_hci_command_complete_event() {
    // With a serializable object (LE_Read_Buffer_Size return parameters).
    check(
        HciPacket::Event(Event::CommandComplete {
            num_hci_command_packets: 34,
            command_opcode: 0x2002, // HCI_LE_READ_BUFFER_SIZE_COMMAND
            return_parameters: ReturnParameters::LeReadBufferSize {
                status: 0,
                le_acl_data_packet_length: 1234,
                total_num_le_acl_data_packets: 56,
            },
        }),
        "040e0722022000d20438",
    );

    // With a simple integer status.
    let event3 = HciPacket::Event(Event::CommandComplete {
        num_hci_command_packets: 1,
        command_opcode: 0x0c03, // HCI_RESET_COMMAND
        return_parameters: ReturnParameters::Status { status: 9 },
    });
    check(event3.clone(), "040e0401030c09");
    match HciPacket::from_bytes(&event3.to_bytes()).unwrap() {
        HciPacket::Event(Event::CommandComplete {
            return_parameters, ..
        }) => assert_eq!(return_parameters.status(), Some(9)),
        other => panic!("expected CommandComplete, got {other:?}"),
    }
}

// hci_test.py::test_return_parameters
#[test]
fn test_return_parameters() {
    // Reset: status only. 0x3C = ADVERTISING_TIMEOUT_ERROR.
    let p = ReturnParameters::parse(0x0c03, &unhex("3c")).unwrap();
    assert_eq!(p.status(), Some(0x3c));
    assert!(matches!(p, ReturnParameters::Status { .. }));

    // Read_BD_ADDR, full (SUCCESS) response.
    let p = ReturnParameters::parse(0x1009, &unhex("00001122334455")).unwrap();
    assert_eq!(p.status(), Some(0));
    assert!(matches!(p, ReturnParameters::ReadBdAddr { .. }));

    // Read_Local_Name: status + 248-byte name field.
    let mut name_params = unhex("0068656c6c6f"); // status=0 + "hello"
    name_params.resize(1 + 248, 0);
    let p = ReturnParameters::parse(0x0c14, &name_params).unwrap();
    assert_eq!(p.status(), Some(0));
    match &p {
        ReturnParameters::ReadLocalName { local_name, .. } => {
            assert_eq!(local_name.len(), 248);
            assert_eq!(map_null_terminated_utf8_string(local_name), "hello");
        }
        other => panic!("expected ReadLocalName, got {other:?}"),
    }

    // Read_BD_ADDR error (short) response -> status only.
    // 0x01 = UNKNOWN_HCI_COMMAND_ERROR.
    let p = ReturnParameters::parse(0x1009, &unhex("010011223344")).unwrap();
    assert!(matches!(p, ReturnParameters::Status { .. }));
    assert_eq!(p.status(), Some(1));
}

// hci_test.py::test_HCI_Read_Local_Supported_Codecs_Command_Complete
#[test]
fn test_read_local_supported_codecs_command_complete() {
    // status, num=3, [A_LOG=1, CVSD=2, LINEAR_PCM=4], vendor_num=0
    let p = ReturnParameters::parse(0x100b, &[0, 3, 1, 2, 4, 0]).unwrap();
    match &p {
        ReturnParameters::ReadLocalSupportedCodecs {
            standard_codec_ids, ..
        } => assert_eq!(standard_codec_ids, &vec![1, 2, 4]),
        other => panic!("expected ReadLocalSupportedCodecs, got {other:?}"),
    }
}

// hci_test.py::test_HCI_Read_Local_Supported_Codecs_V2_Command_Complete
#[test]
fn test_read_local_supported_codecs_v2_command_complete() {
    // status, num=3, pairs (A_LOG,BR_EDR_ACL)(CVSD,BR_EDR_SCO)(LINEAR_PCM,LE_CIS), vendor_num=0
    let p = ReturnParameters::parse(0x100d, &[0, 3, 1, 1, 2, 2, 4, 4, 0]).unwrap();
    match &p {
        ReturnParameters::ReadLocalSupportedCodecsV2 {
            standard_codec_ids,
            standard_codec_transports,
            ..
        } => {
            assert_eq!(standard_codec_ids, &vec![1, 2, 4]);
            assert_eq!(standard_codec_transports, &vec![1, 2, 4]);
        }
        other => panic!("expected ReadLocalSupportedCodecsV2, got {other:?}"),
    }
}

// HCI_Disconnection_Complete_Event (used by the slice-13 disconnect flow)
#[test]
fn test_hci_disconnection_complete_event() {
    check(
        HciPacket::Event(Event::DisconnectionComplete {
            status: 0,
            connection_handle: 0x0002,
            reason: 0x13,
        }),
        "04050400020013",
    );
}

// hci_test.py::test_HCI_Command_Status_Event
#[test]
fn test_hci_command_status_event() {
    check(
        HciPacket::Event(Event::CommandStatus {
            status: 0,
            num_hci_command_packets: 37,
            command_opcode: 0x0406, // HCI_DISCONNECT_COMMAND
        }),
        "040f0400250604",
    );
}

// hci_test.py::test_HCI_Number_Of_Completed_Packets_Event
#[test]
fn test_hci_number_of_completed_packets_event() {
    check(
        HciPacket::Event(Event::NumberOfCompletedPackets {
            connection_handles: vec![1, 2],
            num_completed_packets: vec![3, 4],
        }),
        "041309020100030002000400",
    );
}

// hci_test.py::test_HCI_LE_Connection_Complete_Event
#[test]
fn test_hci_le_connection_complete_event() {
    check(
        HciPacket::Event(Event::LeMeta(LeMetaEvent::ConnectionComplete {
            status: 0,
            connection_handle: 1,
            role: 1,
            peer_address_type: 1,
            peer_address: addr("00:11:22:33:44:55"),
            connection_interval: 3,
            peripheral_latency: 4,
            supervision_timeout: 5,
            central_clock_accuracy: 6,
        })),
        "043e1301000100010155443322110003000400050006",
    );
}

// hci_test.py::test_HCI_LE_Connection_Update_Complete_Event
#[test]
fn test_hci_le_connection_update_complete_event() {
    check(
        HciPacket::Event(Event::LeMeta(LeMetaEvent::ConnectionUpdateComplete {
            status: 0,
            connection_handle: 7,
            connection_interval: 10,
            peripheral_latency: 3,
            supervision_timeout: 5,
        })),
        "043e0a030007000a0003000500",
    );
}

// hci_test.py::test_HCI_LE_Channel_Selection_Algorithm_Event
#[test]
fn test_hci_le_channel_selection_algorithm_event() {
    check(
        HciPacket::Event(Event::LeMeta(LeMetaEvent::ChannelSelectionAlgorithm {
            connection_handle: 7,
            channel_selection_algorithm: 1,
        })),
        "043e0414070001",
    );
}

// hci_test.py::test_HCI_LE_Advertising_Report_Event
#[test]
fn test_hci_le_advertising_report_event() {
    check(
        HciPacket::Event(Event::LeMeta(LeMetaEvent::AdvertisingReport {
            reports: vec![AdvertisingReport {
                event_type: 0, // ADV_IND
                address_type: 0,
                address: addr("00:11:22:33:44:55"),
                data: unhex("0201061106ba5689a6fabfa2bd01467d6e00fbabad08160a181604659b03"),
                rssi: 100,
            }],
        })),
        "043e2a020100005544332211001e0201061106ba5689a6fabfa2bd01467d6e00fbabad08160a181604659b0364",
    );
}

// hci_test.py::test_HCI_LE_Extended_Advertising_Report_Event
#[test]
fn test_hci_le_extended_advertising_report_event() {
    let encoded = "043e380d010100005544332211000103000a640200005544332211001e0201061106ba5689a6fabfa2bd01467d6e00fbabad08160a181604659b03";
    check(
        HciPacket::Event(Event::LeMeta(LeMetaEvent::ExtendedAdvertisingReport {
            reports: vec![ExtendedAdvertisingReport {
                event_type: 1, // CONNECTABLE_ADVERTISING
                address_type: 0,
                address: addr("00:11:22:33:44:55"),
                primary_phy: 1,   // LE 1M
                secondary_phy: 3, // LE Coded
                advertising_sid: 0,
                tx_power: 10,
                rssi: 100,
                periodic_advertising_interval: 2,
                direct_address_type: 0,
                direct_address: addr("00:11:22:33:44:55"),
                data: unhex("0201061106ba5689a6fabfa2bd01467d6e00fbabad08160a181604659b03"),
            }],
        })),
        encoded,
    );
    let HciPacket::Event(Event::LeMeta(LeMetaEvent::ExtendedAdvertisingReport { reports })) =
        HciPacket::from_bytes(&unhex(encoded)).unwrap()
    else {
        panic!("expected extended advertising report");
    };
    assert_eq!(
        reports[0].address.address_type(),
        AddressType::PUBLIC_DEVICE
    );
    assert_eq!(
        reports[0].direct_address.address_type(),
        AddressType::PUBLIC_DEVICE
    );
}

// hci_test.py::test_HCI_LE_Read_Remote_Features_Complete_Event
#[test]
fn test_hci_le_read_remote_features_complete_event() {
    check(
        HciPacket::Event(Event::LeMeta(LeMetaEvent::ReadRemoteFeaturesComplete {
            status: 0,
            connection_handle: 7,
            le_features: [0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77],
        })),
        "043e0c040007000011223344556677",
    );
}

// --- Slice: widened HCI command coverage (connection / encryption / PHY) ---

fn arr<const N: usize>(hex: &str) -> [u8; N] {
    let v = unhex(hex);
    let mut a = [0u8; N];
    a.copy_from_slice(&v);
    a
}

#[test]
fn test_no_param_commands() {
    check(HciPacket::Command(Command::ReadBdAddr), "01091000");
    check(HciPacket::Command(Command::ReadLocalName), "01140c00");
    check(HciPacket::Command(Command::LeReadBufferSize), "01022000");
    check(
        HciPacket::Command(Command::LeReadLocalSupportedFeatures),
        "01032000",
    );
    check(HciPacket::Command(Command::LeRand), "01182000");
}

#[test]
fn test_connection_mgmt_commands() {
    check(
        HciPacket::Command(Command::ReadRemoteVersionInformation {
            connection_handle: 0x0002,
        }),
        "011d04020200",
    );
    check(
        HciPacket::Command(Command::LeSetDataLength {
            connection_handle: 0x0002,
            tx_octets: 251,
            tx_time: 2120,
        }),
        "012220060200fb004808",
    );
    check(
        HciPacket::Command(Command::LeSetPhy {
            connection_handle: 0x0002,
            all_phys: 0,
            tx_phys: 1,
            rx_phys: 1,
            phy_options: 0,
        }),
        "0132200702000001010000",
    );
}

#[test]
fn test_encryption_commands() {
    check(
        HciPacket::Command(Command::LeEnableEncryption {
            connection_handle: 0x0002,
            random_number: arr("1122334455667788"),
            encrypted_diversifier: 0x1234,
            long_term_key: arr("000102030405060708090a0b0c0d0e0f"),
        }),
        "0119201c020011223344556677883412000102030405060708090a0b0c0d0e0f",
    );
    check(
        HciPacket::Command(Command::LeLongTermKeyRequestReply {
            connection_handle: 0x0002,
            long_term_key: arr("000102030405060708090a0b0c0d0e0f"),
        }),
        "011a20120200000102030405060708090a0b0c0d0e0f",
    );
    check(
        HciPacket::Command(Command::LeLongTermKeyRequestNegativeReply {
            connection_handle: 0x0002,
        }),
        "011b20020200",
    );
}

#[test]
fn test_encryption_and_version_events() {
    check(
        HciPacket::Event(Event::EncryptionChange {
            status: 0,
            connection_handle: 0x0002,
            encryption_enabled: 1,
        }),
        "04080400020001",
    );
    check(
        HciPacket::Event(Event::ReadRemoteVersionInformationComplete {
            status: 0,
            connection_handle: 0x0002,
            version: 0x0C,
            manufacturer_name: 0x000F,
            subversion: 0x1234,
        }),
        "040c080002000c0f003412",
    );
}

#[test]
fn test_more_le_meta_events() {
    check(
        HciPacket::Event(Event::LeMeta(LeMetaEvent::LongTermKeyRequest {
            connection_handle: 0x0002,
            random_number: arr("1122334455667788"),
            encryption_diversifier: 0x1234,
        })),
        "043e0d05020011223344556677883412",
    );
    check(
        HciPacket::Event(Event::LeMeta(LeMetaEvent::DataLengthChange {
            connection_handle: 0x0002,
            max_tx_octets: 251,
            max_tx_time: 2120,
            max_rx_octets: 251,
            max_rx_time: 2120,
        })),
        "043e0b070200fb004808fb004808",
    );
    check(
        HciPacket::Event(Event::LeMeta(LeMetaEvent::PhyUpdateComplete {
            status: 0,
            connection_handle: 0x0002,
            tx_phy: 1,
            rx_phy: 1,
        })),
        "043e060c0002000101",
    );
    check(
        HciPacket::Event(Event::LeMeta(LeMetaEvent::EnhancedConnectionComplete {
            status: 0,
            connection_handle: 1,
            role: 1,
            peer_address_type: 1,
            peer_address: addr("00:11:22:33:44:55"),
            local_resolvable_private_address: addr("00:00:00:00:00:00"),
            peer_resolvable_private_address: addr("00:00:00:00:00:00"),
            connection_interval: 3,
            peripheral_latency: 4,
            supervision_timeout: 5,
            central_clock_accuracy: 6,
        })),
        "043e1f0a000100010155443322110000000000000000000000000003000400050006",
    );
}

// hci_test.py::test_custom
#[test]
fn test_custom() {
    let data = vec![0x77, 0x02, 0x01, 0x03];
    let parsed = HciPacket::from_bytes(&data).unwrap();
    match &parsed {
        HciPacket::Custom(c) => {
            assert_eq!(c.hci_packet_type(), 0x77);
            assert_eq!(c.payload(), data.as_slice());
        }
        other => panic!("expected Custom, got {other:?}"),
    }
    assert_eq!(parsed.to_bytes(), data);
}

// hci_test.py::test_iso_data_packet
#[test]
fn test_iso_data_packet() {
    let data = unhex(
        "05616044002ac9f0a193003c00e83b477b00eba8d41dc018bf1a980f0290afe1e7c37652096697\
         52b6a535a8df61e22931ef5a36281bc77ed6a3206d984bcdabee6be831c699cb50e2",
    );
    let packet = IsoDataPacket::from_bytes(&data).unwrap();
    assert_eq!(packet.connection_handle, 0x0061);
    assert_eq!(packet.packet_status_flag, Some(0));
    assert_eq!(packet.pb_flag, 0x02);
    assert_eq!(packet.ts_flag, 0x01);
    assert_eq!(packet.data_total_length, 68);
    assert_eq!(packet.time_stamp, Some(2716911914));
    assert_eq!(packet.packet_sequence_number, Some(147));
    assert_eq!(packet.iso_sdu_length, Some(60));
    assert_eq!(
        hex(&packet.iso_sdu_fragment),
        "e83b477b00eba8d41dc018bf1a980f0290afe1e7c3765209669752b6a535a8df61e22931ef5a3\
         6281bc77ed6a3206d984bcdabee6be831c699cb50e2"
    );
    // Byte round-trip (Bumble's `assert bytes(packet) == data`).
    assert_eq!(packet.to_bytes(), data);
    // And through the top-level dispatcher.
    let hp = HciPacket::from_bytes(&data).unwrap();
    assert_eq!(hp, HciPacket::IsoData(packet));
    assert_eq!(hp.to_bytes(), data);

    let mut truncated = data.clone();
    truncated.pop();
    assert!(IsoDataPacket::from_bytes(&truncated).is_err());
    let mut overlong = data;
    overlong.push(0);
    assert!(IsoDataPacket::from_bytes(&overlong).is_err());
}
