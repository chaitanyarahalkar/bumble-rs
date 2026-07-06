//! HCI acceptance suite. Ported from google/bumble `tests/hci_test.py`.
//!
//! Each test asserts the serialized bytes against a ground-truth hex literal
//! captured from the real Python Bumble (`bytes(x).hex()`) — this is the
//! load-bearing correctness check. It then verifies that parsing dispatches to
//! the correct typed variant and round-trips (the mutual-inverse supplement),
//! mirroring Bumble's `basic_check`.

use bumble::{Address, AddressType};
use bumble_hci::{Command, Event, HciPacket, IsoDataPacket, LeMetaEvent};

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn unhex(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}

/// Serialize, compare against the Python oracle bytes, then parse-and-compare
/// (round-trip). Mirrors and strengthens Bumble's `basic_check`.
fn check(packet: HciPacket, expected_hex: &str) {
    let bytes = packet.to_bytes();
    assert_eq!(hex(&bytes), expected_hex, "serialization vs Python oracle");
    let parsed = HciPacket::from_bytes(&bytes).unwrap();
    assert_eq!(
        parsed, packet,
        "parse must reconstruct the same typed value"
    );
    assert_eq!(parsed.to_bytes(), bytes, "re-serialization must be stable");
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
            le_event_mask: [0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77],
        }),
        "010120080011223344556677",
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
}
