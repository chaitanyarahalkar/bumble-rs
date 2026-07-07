//! GENERATED oracle-pinned tests: every typed HCI command round-trips
//! byte-exact against packet bytes captured from real Python Bumble
//! (`bumble.hci`), and re-parses to the same variant. Values are distinct and
//! position-revealing so the layout — not just the length — is pinned.
#![allow(clippy::redundant_clone)]

use bumble_hci::{CodingFormat, Command, HciPacket};
use bumble::{Address, AddressType};

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}
fn check(cmd: Command, expected: &str) {
    let packet = HciPacket::Command(cmd);
    let bytes = packet.to_bytes();
    assert_eq!(hex(&bytes), expected, "serialize mismatch");
    let back = HciPacket::from_bytes(&bytes).expect("parse");
    assert_eq!(back, packet, "round-trip mismatch");
}

#[test]
fn cmd_inquiry() {
    check(Command::Inquiry {
            lap: 197121,
            inquiry_length: 4,
            num_responses: 5,
    }, "010104050102030405");
}

#[test]
fn cmd_inquirycancel() {
    check(Command::InquiryCancel, "01020400");
}

#[test]
fn cmd_createconnection() {
    check(Command::CreateConnection {
            bd_addr: Address::from_bytes([1, 2, 3, 4, 5, 6], AddressType::RANDOM_DEVICE),
            packet_type: 2055,
            page_scan_repetition_mode: 9,
            reserved: 10,
            clock_offset: 3083,
            allow_role_switch: 13,
    }, "0105040d0102030405060708090a0b0c0d");
}

#[test]
fn cmd_disconnect() {
    check(Command::Disconnect {
            connection_handle: 513,
            reason: 3,
    }, "01060403010203");
}

#[test]
fn cmd_createconnectioncancel() {
    check(Command::CreateConnectionCancel {
            bd_addr: Address::from_bytes([1, 2, 3, 4, 5, 6], AddressType::RANDOM_DEVICE),
    }, "01080406010203040506");
}

#[test]
fn cmd_acceptconnectionrequest() {
    check(Command::AcceptConnectionRequest {
            bd_addr: Address::from_bytes([1, 2, 3, 4, 5, 6], AddressType::RANDOM_DEVICE),
            role: 7,
    }, "0109040701020304050607");
}

#[test]
fn cmd_rejectconnectionrequest() {
    check(Command::RejectConnectionRequest {
            bd_addr: Address::from_bytes([1, 2, 3, 4, 5, 6], AddressType::RANDOM_DEVICE),
            reason: 7,
    }, "010a040701020304050607");
}

#[test]
fn cmd_linkkeyrequestreply() {
    check(Command::LinkKeyRequestReply {
            bd_addr: Address::from_bytes([1, 2, 3, 4, 5, 6], AddressType::RANDOM_DEVICE),
            link_key: [7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22],
    }, "010b04160102030405060708090a0b0c0d0e0f10111213141516");
}

#[test]
fn cmd_linkkeyrequestnegativereply() {
    check(Command::LinkKeyRequestNegativeReply {
            bd_addr: Address::from_bytes([1, 2, 3, 4, 5, 6], AddressType::RANDOM_DEVICE),
    }, "010c0406010203040506");
}

#[test]
fn cmd_pincoderequestreply() {
    check(Command::PinCodeRequestReply {
            bd_addr: Address::from_bytes([1, 2, 3, 4, 5, 6], AddressType::RANDOM_DEVICE),
            pin_code_length: 7,
            pin_code: [8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23],
    }, "010d04170102030405060708090a0b0c0d0e0f1011121314151617");
}

#[test]
fn cmd_pincoderequestnegativereply() {
    check(Command::PinCodeRequestNegativeReply {
            bd_addr: Address::from_bytes([1, 2, 3, 4, 5, 6], AddressType::RANDOM_DEVICE),
    }, "010e0406010203040506");
}

#[test]
fn cmd_changeconnectionpackettype() {
    check(Command::ChangeConnectionPacketType {
            connection_handle: 513,
            packet_type: 1027,
    }, "010f040401020304");
}

#[test]
fn cmd_authenticationrequested() {
    check(Command::AuthenticationRequested {
            connection_handle: 513,
    }, "011104020102");
}

#[test]
fn cmd_setconnectionencryption() {
    check(Command::SetConnectionEncryption {
            connection_handle: 513,
            encryption_enable: 3,
    }, "01130403010203");
}

#[test]
fn cmd_remotenamerequest() {
    check(Command::RemoteNameRequest {
            bd_addr: Address::from_bytes([1, 2, 3, 4, 5, 6], AddressType::RANDOM_DEVICE),
            page_scan_repetition_mode: 7,
            reserved: 8,
            clock_offset: 2569,
    }, "0119040a0102030405060708090a");
}

#[test]
fn cmd_readremotesupportedfeatures() {
    check(Command::ReadRemoteSupportedFeatures {
            connection_handle: 513,
    }, "011b04020102");
}

#[test]
fn cmd_readremoteextendedfeatures() {
    check(Command::ReadRemoteExtendedFeatures {
            connection_handle: 513,
            page_number: 3,
    }, "011c0403010203");
}

#[test]
fn cmd_readremoteversioninformation() {
    check(Command::ReadRemoteVersionInformation {
            connection_handle: 513,
    }, "011d04020102");
}

#[test]
fn cmd_readclockoffset() {
    check(Command::ReadClockOffset {
            connection_handle: 513,
    }, "011f04020102");
}

#[test]
fn cmd_acceptsynchronousconnectionrequest() {
    check(Command::AcceptSynchronousConnectionRequest {
            bd_addr: Address::from_bytes([1, 2, 3, 4, 5, 6], AddressType::RANDOM_DEVICE),
            transmit_bandwidth: 168364039,
            receive_bandwidth: 235736075,
            max_latency: 4111,
            voice_setting: 4625,
            retransmission_effort: 19,
            packet_type: 5396,
    }, "012904150102030405060708090a0b0c0d0e0f101112131415");
}

#[test]
fn cmd_rejectsynchronousconnectionrequest() {
    check(Command::RejectSynchronousConnectionRequest {
            bd_addr: Address::from_bytes([1, 2, 3, 4, 5, 6], AddressType::RANDOM_DEVICE),
            reason: 7,
    }, "012a040701020304050607");
}

#[test]
fn cmd_iocapabilityrequestreply() {
    check(Command::IoCapabilityRequestReply {
            bd_addr: Address::from_bytes([1, 2, 3, 4, 5, 6], AddressType::RANDOM_DEVICE),
            io_capability: 7,
            oob_data_present: 8,
            authentication_requirements: 9,
    }, "012b0409010203040506070809");
}

#[test]
fn cmd_userconfirmationrequestreply() {
    check(Command::UserConfirmationRequestReply {
            bd_addr: Address::from_bytes([1, 2, 3, 4, 5, 6], AddressType::RANDOM_DEVICE),
    }, "012c0406010203040506");
}

#[test]
fn cmd_userconfirmationrequestnegativereply() {
    check(Command::UserConfirmationRequestNegativeReply {
            bd_addr: Address::from_bytes([1, 2, 3, 4, 5, 6], AddressType::RANDOM_DEVICE),
    }, "012d0406010203040506");
}

#[test]
fn cmd_userpasskeyrequestreply() {
    check(Command::UserPasskeyRequestReply {
            bd_addr: Address::from_bytes([1, 2, 3, 4, 5, 6], AddressType::RANDOM_DEVICE),
            numeric_value: 168364039,
    }, "012e040a0102030405060708090a");
}

#[test]
fn cmd_userpasskeyrequestnegativereply() {
    check(Command::UserPasskeyRequestNegativeReply {
            bd_addr: Address::from_bytes([1, 2, 3, 4, 5, 6], AddressType::RANDOM_DEVICE),
    }, "012f0406010203040506");
}

#[test]
fn cmd_remoteoobdatarequestreply() {
    check(Command::RemoteOobDataRequestReply {
            bd_addr: Address::from_bytes([1, 2, 3, 4, 5, 6], AddressType::RANDOM_DEVICE),
            c: [7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22],
            r: [23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38],
    }, "013004260102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f20212223242526");
}

#[test]
fn cmd_remoteoobdatarequestnegativereply() {
    check(Command::RemoteOobDataRequestNegativeReply {
            bd_addr: Address::from_bytes([1, 2, 3, 4, 5, 6], AddressType::RANDOM_DEVICE),
    }, "01330406010203040506");
}

#[test]
fn cmd_iocapabilityrequestnegativereply() {
    check(Command::IoCapabilityRequestNegativeReply {
            bd_addr: Address::from_bytes([1, 2, 3, 4, 5, 6], AddressType::RANDOM_DEVICE),
            reason: 7,
    }, "0134040701020304050607");
}

#[test]
fn cmd_enhancedsetupsynchronousconnection() {
    check(Command::EnhancedSetupSynchronousConnection {
            connection_handle: 513,
            transmit_bandwidth: 100992003,
            receive_bandwidth: 168364039,
            transmit_coding_format: CodingFormat { coding_format: 2, company_id: 0, vendor_specific_codec_id: 0 },
            receive_coding_format: CodingFormat { coding_format: 2, company_id: 0, vendor_specific_codec_id: 0 },
            transmit_codec_frame_size: 3083,
            receive_codec_frame_size: 3597,
            input_bandwidth: 303108111,
            output_bandwidth: 370480147,
            input_coding_format: CodingFormat { coding_format: 2, company_id: 0, vendor_specific_codec_id: 0 },
            output_coding_format: CodingFormat { coding_format: 2, company_id: 0, vendor_specific_codec_id: 0 },
            input_coded_data_size: 6167,
            output_coded_data_size: 6681,
            input_pcm_data_format: 27,
            output_pcm_data_format: 28,
            input_pcm_sample_payload_msb_position: 29,
            output_pcm_sample_payload_msb_position: 30,
            input_data_path: 31,
            output_data_path: 32,
            input_transport_unit_size: 33,
            output_transport_unit_size: 34,
            max_latency: 9251,
            packet_type: 9765,
            retransmission_effort: 39,
    }, "013d043b0102030405060708090a020000000002000000000b0c0d0e0f10111213141516020000000002000000001718191a1b1c1d1e1f2021222324252627");
}

#[test]
fn cmd_enhancedacceptsynchronousconnectionrequest() {
    check(Command::EnhancedAcceptSynchronousConnectionRequest {
            bd_addr: Address::from_bytes([1, 2, 3, 4, 5, 6], AddressType::RANDOM_DEVICE),
            transmit_bandwidth: 168364039,
            receive_bandwidth: 235736075,
            transmit_coding_format: CodingFormat { coding_format: 2, company_id: 0, vendor_specific_codec_id: 0 },
            receive_coding_format: CodingFormat { coding_format: 2, company_id: 0, vendor_specific_codec_id: 0 },
            transmit_codec_frame_size: 4111,
            receive_codec_frame_size: 4625,
            input_bandwidth: 370480147,
            output_bandwidth: 437852183,
            input_coding_format: CodingFormat { coding_format: 2, company_id: 0, vendor_specific_codec_id: 0 },
            output_coding_format: CodingFormat { coding_format: 2, company_id: 0, vendor_specific_codec_id: 0 },
            input_coded_data_size: 7195,
            output_coded_data_size: 7709,
            input_pcm_data_format: 31,
            output_pcm_data_format: 32,
            input_pcm_sample_payload_msb_position: 33,
            output_pcm_sample_payload_msb_position: 34,
            input_data_path: 35,
            output_data_path: 36,
            input_transport_unit_size: 37,
            output_transport_unit_size: 38,
            max_latency: 10279,
            packet_type: 10793,
            retransmission_effort: 43,
    }, "013e043f0102030405060708090a0b0c0d0e020000000002000000000f101112131415161718191a020000000002000000001b1c1d1e1f202122232425262728292a2b");
}

#[test]
fn cmd_truncatedpage() {
    check(Command::TruncatedPage {
            bd_addr: Address::from_bytes([1, 2, 3, 4, 5, 6], AddressType::RANDOM_DEVICE),
            page_scan_repetition_mode: 7,
            clock_offset: 2312,
    }, "013f0409010203040506070809");
}

#[test]
fn cmd_truncatedpagecancel() {
    check(Command::TruncatedPageCancel {
            bd_addr: Address::from_bytes([1, 2, 3, 4, 5, 6], AddressType::RANDOM_DEVICE),
    }, "01400406010203040506");
}

#[test]
fn cmd_setconnectionlessperipheralbroadcast() {
    check(Command::SetConnectionlessPeripheralBroadcast {
            enable: 1,
            lt_addr: 2,
            lpo_allowed: 3,
            packet_type: 1284,
            interval_min: 1798,
            interval_max: 2312,
            supervision_timeout: 2826,
    }, "0141040b0102030405060708090a0b");
}

#[test]
fn cmd_setconnectionlessperipheralbroadcastreceive() {
    check(Command::SetConnectionlessPeripheralBroadcastReceive {
            enable: 1,
            bd_addr: Address::from_bytes([2, 3, 4, 5, 6, 7], AddressType::RANDOM_DEVICE),
            lt_addr: 8,
            interval: 2569,
            clock_offset: 235736075,
            next_connectionless_peripheral_broadcast_clock: 303108111,
            supervision_timeout: 5139,
            remote_timing_accuracy: 21,
            skip: 22,
            packet_type: 6167,
            afh_channel_map: [25, 26, 27, 28, 29, 30, 31, 32, 33, 34],
    }, "014204220102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f202122");
}

#[test]
fn cmd_startsynchronizationtrain() {
    check(Command::StartSynchronizationTrain, "01430400");
}

#[test]
fn cmd_receivesynchronizationtrain() {
    check(Command::ReceiveSynchronizationTrain {
            bd_addr: Address::from_bytes([1, 2, 3, 4, 5, 6], AddressType::RANDOM_DEVICE),
            sync_scan_timeout: 2055,
            sync_scan_window: 2569,
            sync_scan_interval: 3083,
    }, "0144040c0102030405060708090a0b0c");
}

#[test]
fn cmd_remoteoobextendeddatarequestreply() {
    check(Command::RemoteOobExtendedDataRequestReply {
            bd_addr: Address::from_bytes([1, 2, 3, 4, 5, 6], AddressType::RANDOM_DEVICE),
            c_192: [7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22],
            r_192: [23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38],
            c_256: [39, 40, 41, 42, 43, 44, 45, 46, 47, 48, 49, 50, 51, 52, 53, 54],
            r_256: [55, 56, 57, 58, 59, 60, 61, 62, 63, 64, 65, 66, 67, 68, 69, 70],
    }, "014504460102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f202122232425262728292a2b2c2d2e2f303132333435363738393a3b3c3d3e3f40414243444546");
}

#[test]
fn cmd_sniffmode() {
    check(Command::SniffMode {
            connection_handle: 513,
            sniff_max_interval: 1027,
            sniff_min_interval: 1541,
            sniff_attempt: 2055,
            sniff_timeout: 2569,
    }, "0103080a0102030405060708090a");
}

#[test]
fn cmd_exitsniffmode() {
    check(Command::ExitSniffMode {
            connection_handle: 513,
    }, "010408020102");
}

#[test]
fn cmd_switchrole() {
    check(Command::SwitchRole {
            bd_addr: Address::from_bytes([1, 2, 3, 4, 5, 6], AddressType::RANDOM_DEVICE),
            role: 7,
    }, "010b080701020304050607");
}

#[test]
fn cmd_writelinkpolicysettings() {
    check(Command::WriteLinkPolicySettings {
            connection_handle: 513,
            link_policy_settings: 1027,
    }, "010d080401020304");
}

#[test]
fn cmd_writedefaultlinkpolicysettings() {
    check(Command::WriteDefaultLinkPolicySettings {
            default_link_policy_settings: 513,
    }, "010f08020102");
}

#[test]
fn cmd_sniffsubrating() {
    check(Command::SniffSubrating {
            connection_handle: 513,
            maximum_latency: 1027,
            minimum_remote_timeout: 1541,
            minimum_local_timeout: 2055,
    }, "011108080102030405060708");
}

#[test]
fn cmd_seteventmask() {
    check(Command::SetEventMask {
            event_mask: [1, 2, 3, 4, 5, 6, 7, 8],
    }, "01010c080102030405060708");
}

#[test]
fn cmd_reset() {
    check(Command::Reset, "01030c00");
}

#[test]
fn cmd_seteventfilter() {
    check(Command::SetEventFilter {
            filter_type: 1,
            filter_condition: vec![2, 3, 4, 5],
    }, "01050c050102030405");
}

#[test]
fn cmd_readstoredlinkkey() {
    check(Command::ReadStoredLinkKey {
            bd_addr: Address::from_bytes([1, 2, 3, 4, 5, 6], AddressType::RANDOM_DEVICE),
            read_all_flag: 7,
    }, "010d0c0701020304050607");
}

#[test]
fn cmd_deletestoredlinkkey() {
    check(Command::DeleteStoredLinkKey {
            bd_addr: Address::from_bytes([1, 2, 3, 4, 5, 6], AddressType::RANDOM_DEVICE),
            delete_all_flag: 7,
    }, "01120c0701020304050607");
}

#[test]
fn cmd_writelocalname() {
    check(Command::WriteLocalName {
            local_name: [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44, 45, 46, 47, 48, 49, 50, 51, 52, 53, 54, 55, 56, 57, 58, 59, 60, 61, 62, 63, 64, 65, 66, 67, 68, 69, 70, 71, 72, 73, 74, 75, 76, 77, 78, 79, 80, 81, 82, 83, 84, 85, 86, 87, 88, 89, 90, 91, 92, 93, 94, 95, 96, 97, 98, 99, 100, 101, 102, 103, 104, 105, 106, 107, 108, 109, 110, 111, 112, 113, 114, 115, 116, 117, 118, 119, 120, 121, 122, 123, 124, 125, 126, 127, 128, 129, 130, 131, 132, 133, 134, 135, 136, 137, 138, 139, 140, 141, 142, 143, 144, 145, 146, 147, 148, 149, 150, 151, 152, 153, 154, 155, 156, 157, 158, 159, 160, 161, 162, 163, 164, 165, 166, 167, 168, 169, 170, 171, 172, 173, 174, 175, 176, 177, 178, 179, 180, 181, 182, 183, 184, 185, 186, 187, 188, 189, 190, 191, 192, 193, 194, 195, 196, 197, 198, 199, 200, 201, 202, 203, 204, 205, 206, 207, 208, 209, 210, 211, 212, 213, 214, 215, 216, 217, 218, 219, 220, 221, 222, 223, 224, 225, 226, 227, 228, 229, 230, 231, 232, 233, 234, 235, 236, 237, 238, 239, 240, 241, 242, 243, 244, 245, 246, 247, 248],
    }, "01130cf80102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f202122232425262728292a2b2c2d2e2f303132333435363738393a3b3c3d3e3f404142434445464748494a4b4c4d4e4f505152535455565758595a5b5c5d5e5f606162636465666768696a6b6c6d6e6f707172737475767778797a7b7c7d7e7f808182838485868788898a8b8c8d8e8f909192939495969798999a9b9c9d9e9fa0a1a2a3a4a5a6a7a8a9aaabacadaeafb0b1b2b3b4b5b6b7b8b9babbbcbdbebfc0c1c2c3c4c5c6c7c8c9cacbcccdcecfd0d1d2d3d4d5d6d7d8d9dadbdcdddedfe0e1e2e3e4e5e6e7e8e9eaebecedeeeff0f1f2f3f4f5f6f7f8");
}

#[test]
fn cmd_readlocalname() {
    check(Command::ReadLocalName, "01140c00");
}

#[test]
fn cmd_writeconnectionaccepttimeout() {
    check(Command::WriteConnectionAcceptTimeout {
            connection_accept_timeout: 513,
    }, "01160c020102");
}

#[test]
fn cmd_writepagetimeout() {
    check(Command::WritePageTimeout {
            page_timeout: 513,
    }, "01180c020102");
}

#[test]
fn cmd_writescanenable() {
    check(Command::WriteScanEnable {
            scan_enable: 1,
    }, "011a0c0101");
}

#[test]
fn cmd_readpagescanactivity() {
    check(Command::ReadPageScanActivity, "011b0c00");
}

#[test]
fn cmd_writepagescanactivity() {
    check(Command::WritePageScanActivity {
            page_scan_interval: 513,
            page_scan_window: 1027,
    }, "011c0c0401020304");
}

#[test]
fn cmd_writeinquiryscanactivity() {
    check(Command::WriteInquiryScanActivity {
            inquiry_scan_interval: 513,
            inquiry_scan_window: 1027,
    }, "011e0c0401020304");
}

#[test]
fn cmd_readauthenticationenable() {
    check(Command::ReadAuthenticationEnable, "011f0c00");
}

#[test]
fn cmd_writeauthenticationenable() {
    check(Command::WriteAuthenticationEnable {
            authentication_enable: 1,
    }, "01200c0101");
}

#[test]
fn cmd_readclassofdevice() {
    check(Command::ReadClassOfDevice, "01230c00");
}

#[test]
fn cmd_writeclassofdevice() {
    check(Command::WriteClassOfDevice {
            class_of_device: 197121,
    }, "01240c03010203");
}

#[test]
fn cmd_readvoicesetting() {
    check(Command::ReadVoiceSetting, "01250c00");
}

#[test]
fn cmd_writevoicesetting() {
    check(Command::WriteVoiceSetting {
            voice_setting: 513,
    }, "01260c020102");
}

#[test]
fn cmd_readsynchronousflowcontrolenable() {
    check(Command::ReadSynchronousFlowControlEnable, "012e0c00");
}

#[test]
fn cmd_writesynchronousflowcontrolenable() {
    check(Command::WriteSynchronousFlowControlEnable {
            synchronous_flow_control_enable: 1,
    }, "012f0c0101");
}

#[test]
fn cmd_setcontrollertohostflowcontrol() {
    check(Command::SetControllerToHostFlowControl {
            flow_control_enable: 1,
    }, "01310c0101");
}

#[test]
fn cmd_hostbuffersize() {
    check(Command::HostBufferSize {
            host_acl_data_packet_length: 513,
            host_synchronous_data_packet_length: 3,
            host_total_num_acl_data_packets: 1284,
            host_total_num_synchronous_data_packets: 1798,
    }, "01330c0701020304050607");
}

#[test]
fn cmd_writelinksupervisiontimeout() {
    check(Command::WriteLinkSupervisionTimeout {
            handle: 513,
            link_supervision_timeout: 1027,
    }, "01370c0401020304");
}

#[test]
fn cmd_readnumberofsupportediac() {
    check(Command::ReadNumberOfSupportedIac, "01380c00");
}

#[test]
fn cmd_readcurrentiaclap() {
    check(Command::ReadCurrentIacLap, "01390c00");
}

#[test]
fn cmd_writeinquiryscantype() {
    check(Command::WriteInquiryScanType {
            scan_type: 1,
    }, "01430c0101");
}

#[test]
fn cmd_writeinquirymode() {
    check(Command::WriteInquiryMode {
            inquiry_mode: 1,
    }, "01450c0101");
}

#[test]
fn cmd_readpagescantype() {
    check(Command::ReadPageScanType, "01460c00");
}

#[test]
fn cmd_writepagescantype() {
    check(Command::WritePageScanType {
            page_scan_type: 1,
    }, "01470c0101");
}

#[test]
fn cmd_writeextendedinquiryresponse() {
    check(Command::WriteExtendedInquiryResponse {
            fec_required: 1,
            extended_inquiry_response: [2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44, 45, 46, 47, 48, 49, 50, 51, 52, 53, 54, 55, 56, 57, 58, 59, 60, 61, 62, 63, 64, 65, 66, 67, 68, 69, 70, 71, 72, 73, 74, 75, 76, 77, 78, 79, 80, 81, 82, 83, 84, 85, 86, 87, 88, 89, 90, 91, 92, 93, 94, 95, 96, 97, 98, 99, 100, 101, 102, 103, 104, 105, 106, 107, 108, 109, 110, 111, 112, 113, 114, 115, 116, 117, 118, 119, 120, 121, 122, 123, 124, 125, 126, 127, 128, 129, 130, 131, 132, 133, 134, 135, 136, 137, 138, 139, 140, 141, 142, 143, 144, 145, 146, 147, 148, 149, 150, 151, 152, 153, 154, 155, 156, 157, 158, 159, 160, 161, 162, 163, 164, 165, 166, 167, 168, 169, 170, 171, 172, 173, 174, 175, 176, 177, 178, 179, 180, 181, 182, 183, 184, 185, 186, 187, 188, 189, 190, 191, 192, 193, 194, 195, 196, 197, 198, 199, 200, 201, 202, 203, 204, 205, 206, 207, 208, 209, 210, 211, 212, 213, 214, 215, 216, 217, 218, 219, 220, 221, 222, 223, 224, 225, 226, 227, 228, 229, 230, 231, 232, 233, 234, 235, 236, 237, 238, 239, 240, 241],
    }, "01520cf10102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f202122232425262728292a2b2c2d2e2f303132333435363738393a3b3c3d3e3f404142434445464748494a4b4c4d4e4f505152535455565758595a5b5c5d5e5f606162636465666768696a6b6c6d6e6f707172737475767778797a7b7c7d7e7f808182838485868788898a8b8c8d8e8f909192939495969798999a9b9c9d9e9fa0a1a2a3a4a5a6a7a8a9aaabacadaeafb0b1b2b3b4b5b6b7b8b9babbbcbdbebfc0c1c2c3c4c5c6c7c8c9cacbcccdcecfd0d1d2d3d4d5d6d7d8d9dadbdcdddedfe0e1e2e3e4e5e6e7e8e9eaebecedeeeff0f1");
}

#[test]
fn cmd_writesimplepairingmode() {
    check(Command::WriteSimplePairingMode {
            simple_pairing_mode: 1,
    }, "01560c0101");
}

#[test]
fn cmd_readlocaloobdata() {
    check(Command::ReadLocalOobData, "01570c00");
}

#[test]
fn cmd_readinquiryresponsetransmitpowerlevel() {
    check(Command::ReadInquiryResponseTransmitPowerLevel, "01580c00");
}

#[test]
fn cmd_readdefaulterroneousdatareporting() {
    check(Command::ReadDefaultErroneousDataReporting, "015a0c00");
}

#[test]
fn cmd_seteventmaskpage2() {
    check(Command::SetEventMaskPage2 {
            event_mask_page_2: [1, 2, 3, 4, 5, 6, 7, 8],
    }, "01630c080102030405060708");
}

#[test]
fn cmd_readlehostsupport() {
    check(Command::ReadLeHostSupport, "016c0c00");
}

#[test]
fn cmd_writelehostsupport() {
    check(Command::WriteLeHostSupport {
            le_supported_host: 1,
            simultaneous_le_host: 2,
    }, "016d0c020102");
}

#[test]
fn cmd_writesecureconnectionshostsupport() {
    check(Command::WriteSecureConnectionsHostSupport {
            secure_connections_host_support: 1,
    }, "017a0c0101");
}

#[test]
fn cmd_writeauthenticatedpayloadtimeout() {
    check(Command::WriteAuthenticatedPayloadTimeout {
            connection_handle: 513,
            authenticated_payload_timeout: 1027,
    }, "017c0c0401020304");
}

#[test]
fn cmd_readlocaloobextendeddata() {
    check(Command::ReadLocalOobExtendedData, "017d0c00");
}

#[test]
fn cmd_configuredatapath() {
    check(Command::ConfigureDataPath {
            data_path_direction: 1,
            data_path_id: 2,
            vendor_specific_config: vec![3, 4, 5, 6],
    }, "01830c06010203040506");
}

#[test]
fn cmd_readlocalversioninformation() {
    check(Command::ReadLocalVersionInformation, "01011000");
}

#[test]
fn cmd_readlocalsupportedcommands() {
    check(Command::ReadLocalSupportedCommands, "01021000");
}

#[test]
fn cmd_readlocalsupportedfeatures() {
    check(Command::ReadLocalSupportedFeatures, "01031000");
}

#[test]
fn cmd_readlocalextendedfeatures() {
    check(Command::ReadLocalExtendedFeatures {
            page_number: 1,
    }, "0104100101");
}

#[test]
fn cmd_readbuffersize() {
    check(Command::ReadBufferSize, "01051000");
}

#[test]
fn cmd_readbdaddr() {
    check(Command::ReadBdAddr, "01091000");
}

#[test]
fn cmd_readlocalsupportedcodecs() {
    check(Command::ReadLocalSupportedCodecs, "010b1000");
}

#[test]
fn cmd_readlocalsupportedcodecsv2() {
    check(Command::ReadLocalSupportedCodecsV2, "010d1000");
}

#[test]
fn cmd_readrssi() {
    check(Command::ReadRssi {
            handle: 513,
    }, "010514020102");
}

#[test]
fn cmd_readencryptionkeysize() {
    check(Command::ReadEncryptionKeySize {
            connection_handle: 513,
    }, "010814020102");
}

#[test]
fn cmd_readloopbackmode() {
    check(Command::ReadLoopbackMode, "01011800");
}

#[test]
fn cmd_writeloopbackmode() {
    check(Command::WriteLoopbackMode {
            loopback_mode: 1,
    }, "0102180101");
}

#[test]
fn cmd_leseteventmask() {
    check(Command::LeSetEventMask {
            le_event_mask: [1, 2, 3, 4, 5, 6, 7, 8],
    }, "010120080102030405060708");
}

#[test]
fn cmd_lereadbuffersize() {
    check(Command::LeReadBufferSize, "01022000");
}

#[test]
fn cmd_lereadlocalsupportedfeatures() {
    check(Command::LeReadLocalSupportedFeatures, "01032000");
}

#[test]
fn cmd_lesetrandomaddress() {
    check(Command::LeSetRandomAddress {
            random_address: Address::from_bytes([1, 2, 3, 4, 5, 6], AddressType::RANDOM_DEVICE),
    }, "01052006010203040506");
}

#[test]
fn cmd_lesetadvertisingparameters() {
    check(Command::LeSetAdvertisingParameters {
            advertising_interval_min: 513,
            advertising_interval_max: 1027,
            advertising_type: 5,
            own_address_type: 6,
            peer_address_type: 7,
            peer_address: Address::from_bytes([8, 9, 10, 11, 12, 13], AddressType::RANDOM_DEVICE),
            advertising_channel_map: 14,
            advertising_filter_policy: 15,
    }, "0106200f0102030405060708090a0b0c0d0e0f");
}

#[test]
fn cmd_lereadadvertisingphysicalchanneltxpower() {
    check(Command::LeReadAdvertisingPhysicalChannelTxPower, "01072000");
}

#[test]
fn cmd_lesetadvertisingdata() {
    check(Command::LeSetAdvertisingData {
            advertising_data: vec![1, 2, 3],
    }, "010820200301020300000000000000000000000000000000000000000000000000000000");
}

#[test]
fn cmd_lesetscanresponsedata() {
    check(Command::LeSetScanResponseData {
            scan_response_data: vec![1, 2, 3],
    }, "010920200301020300000000000000000000000000000000000000000000000000000000");
}

#[test]
fn cmd_lesetadvertisingenable() {
    check(Command::LeSetAdvertisingEnable {
            advertising_enable: 1,
    }, "010a200101");
}

#[test]
fn cmd_lesetscanparameters() {
    check(Command::LeSetScanParameters {
            le_scan_type: 1,
            le_scan_interval: 770,
            le_scan_window: 1284,
            own_address_type: 6,
            scanning_filter_policy: 7,
    }, "010b200701020304050607");
}

#[test]
fn cmd_lesetscanenable() {
    check(Command::LeSetScanEnable {
            le_scan_enable: 1,
            filter_duplicates: 2,
    }, "010c20020102");
}

#[test]
fn cmd_lecreateconnection() {
    check(Command::LeCreateConnection {
            le_scan_interval: 513,
            le_scan_window: 1027,
            initiator_filter_policy: 5,
            peer_address_type: 6,
            peer_address: Address::from_bytes([7, 8, 9, 10, 11, 12], AddressType::RANDOM_DEVICE),
            own_address_type: 13,
            connection_interval_min: 3854,
            connection_interval_max: 4368,
            max_latency: 4882,
            supervision_timeout: 5396,
            min_ce_length: 5910,
            max_ce_length: 6424,
    }, "010d20190102030405060708090a0b0c0d0e0f10111213141516171819");
}

#[test]
fn cmd_lecreateconnectioncancel() {
    check(Command::LeCreateConnectionCancel, "010e2000");
}

#[test]
fn cmd_lereadfilteracceptlistsize() {
    check(Command::LeReadFilterAcceptListSize, "010f2000");
}

#[test]
fn cmd_leclearfilteracceptlist() {
    check(Command::LeClearFilterAcceptList, "01102000");
}

#[test]
fn cmd_leadddevicetofilteracceptlist() {
    check(Command::LeAddDeviceToFilterAcceptList {
            address_type: 1,
            address: Address::from_bytes([2, 3, 4, 5, 6, 7], AddressType::RANDOM_DEVICE),
    }, "0111200701020304050607");
}

#[test]
fn cmd_leremovedevicefromfilteracceptlist() {
    check(Command::LeRemoveDeviceFromFilterAcceptList {
            address_type: 1,
            address: Address::from_bytes([2, 3, 4, 5, 6, 7], AddressType::RANDOM_DEVICE),
    }, "0112200701020304050607");
}

#[test]
fn cmd_leconnectionupdate() {
    check(Command::LeConnectionUpdate {
            connection_handle: 513,
            connection_interval_min: 1027,
            connection_interval_max: 1541,
            max_latency: 2055,
            supervision_timeout: 2569,
            min_ce_length: 3083,
            max_ce_length: 3597,
    }, "0113200e0102030405060708090a0b0c0d0e");
}

#[test]
fn cmd_lereadremotefeatures() {
    check(Command::LeReadRemoteFeatures {
            connection_handle: 513,
    }, "011620020102");
}

#[test]
fn cmd_lerand() {
    check(Command::LeRand, "01182000");
}

#[test]
fn cmd_leenableencryption() {
    check(Command::LeEnableEncryption {
            connection_handle: 513,
            random_number: [3, 4, 5, 6, 7, 8, 9, 10],
            encrypted_diversifier: 3083,
            long_term_key: [13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28],
    }, "0119201c0102030405060708090a0b0c0d0e0f101112131415161718191a1b1c");
}

#[test]
fn cmd_lelongtermkeyrequestreply() {
    check(Command::LeLongTermKeyRequestReply {
            connection_handle: 513,
            long_term_key: [3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18],
    }, "011a20120102030405060708090a0b0c0d0e0f101112");
}

#[test]
fn cmd_lelongtermkeyrequestnegativereply() {
    check(Command::LeLongTermKeyRequestNegativeReply {
            connection_handle: 513,
    }, "011b20020102");
}

#[test]
fn cmd_lereadsupportedstates() {
    check(Command::LeReadSupportedStates, "011c2000");
}

#[test]
fn cmd_leremoteconnectionparameterrequestreply() {
    check(Command::LeRemoteConnectionParameterRequestReply {
            connection_handle: 513,
            interval_min: 1027,
            interval_max: 1541,
            max_latency: 2055,
            timeout: 2569,
            min_ce_length: 3083,
            max_ce_length: 3597,
    }, "0120200e0102030405060708090a0b0c0d0e");
}

#[test]
fn cmd_leremoteconnectionparameterrequestnegativereply() {
    check(Command::LeRemoteConnectionParameterRequestNegativeReply {
            connection_handle: 513,
            reason: 3,
    }, "01212003010203");
}

#[test]
fn cmd_lesetdatalength() {
    check(Command::LeSetDataLength {
            connection_handle: 513,
            tx_octets: 1027,
            tx_time: 1541,
    }, "01222006010203040506");
}

#[test]
fn cmd_lereadsuggesteddefaultdatalength() {
    check(Command::LeReadSuggestedDefaultDataLength, "01232000");
}

#[test]
fn cmd_lewritesuggesteddefaultdatalength() {
    check(Command::LeWriteSuggestedDefaultDataLength {
            suggested_max_tx_octets: 513,
            suggested_max_tx_time: 1027,
    }, "0124200401020304");
}

#[test]
fn cmd_lereadlocalp256publickey() {
    check(Command::LeReadLocalP256PublicKey, "01252000");
}

#[test]
fn cmd_leadddevicetoresolvinglist() {
    check(Command::LeAddDeviceToResolvingList {
            peer_identity_address_type: 1,
            peer_identity_address: Address::from_bytes([2, 3, 4, 5, 6, 7], AddressType::RANDOM_DEVICE),
            peer_irk: [8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23],
            local_irk: [24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39],
    }, "012720270102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f2021222324252627");
}

#[test]
fn cmd_leclearresolvinglist() {
    check(Command::LeClearResolvingList, "01292000");
}

#[test]
fn cmd_lereadresolvinglistsize() {
    check(Command::LeReadResolvingListSize, "012a2000");
}

#[test]
fn cmd_lesetaddressresolutionenable() {
    check(Command::LeSetAddressResolutionEnable {
            address_resolution_enable: 1,
    }, "012d200101");
}

#[test]
fn cmd_lesetresolvableprivateaddresstimeout() {
    check(Command::LeSetResolvablePrivateAddressTimeout {
            rpa_timeout: 513,
    }, "012e20020102");
}

#[test]
fn cmd_lereadmaximumdatalength() {
    check(Command::LeReadMaximumDataLength, "012f2000");
}

#[test]
fn cmd_lereadphy() {
    check(Command::LeReadPhy {
            connection_handle: 513,
    }, "013020020102");
}

#[test]
fn cmd_lesetdefaultphy() {
    check(Command::LeSetDefaultPhy {
            all_phys: 1,
            tx_phys: 2,
            rx_phys: 3,
    }, "01312003010203");
}

#[test]
fn cmd_lesetphy() {
    check(Command::LeSetPhy {
            connection_handle: 513,
            all_phys: 3,
            tx_phys: 4,
            rx_phys: 5,
            phy_options: 1798,
    }, "0132200701020304050607");
}

#[test]
fn cmd_lesetadvertisingsetrandomaddress() {
    check(Command::LeSetAdvertisingSetRandomAddress {
            advertising_handle: 1,
            random_address: Address::from_bytes([2, 3, 4, 5, 6, 7], AddressType::RANDOM_DEVICE),
    }, "0135200701020304050607");
}

#[test]
fn cmd_lesetextendedadvertisingparameters() {
    check(Command::LeSetExtendedAdvertisingParameters {
            advertising_handle: 1,
            advertising_event_properties: 770,
            primary_advertising_interval_min: 394500,
            primary_advertising_interval_max: 591879,
            primary_advertising_channel_map: 10,
            own_address_type: 11,
            peer_address_type: 12,
            peer_address: Address::from_bytes([13, 14, 15, 16, 17, 18], AddressType::RANDOM_DEVICE),
            advertising_filter_policy: 19,
            advertising_tx_power: 20,
            primary_advertising_phy: 21,
            secondary_advertising_max_skip: 22,
            secondary_advertising_phy: 23,
            advertising_sid: 24,
            scan_request_notification_enable: 25,
    }, "013620190102030405060708090a0b0c0d0e0f10111213141516171819");
}

#[test]
fn cmd_lesetextendedadvertisingdata() {
    check(Command::LeSetExtendedAdvertisingData {
            advertising_handle: 1,
            operation: 2,
            fragment_preference: 3,
            advertising_data: vec![4, 5, 6],
    }, "0137200701020303040506");
}

#[test]
fn cmd_lesetextendedscanresponsedata() {
    check(Command::LeSetExtendedScanResponseData {
            advertising_handle: 1,
            operation: 2,
            fragment_preference: 3,
            scan_response_data: vec![4, 5, 6],
    }, "0138200701020303040506");
}

#[test]
fn cmd_lesetextendedadvertisingenable() {
    check(Command::LeSetExtendedAdvertisingEnable {
            enable: 1,
            advertising_handles: vec![2],
            durations: vec![1027],
            max_extended_advertising_events: vec![5],
    }, "01392006010102030405");
}

#[test]
fn cmd_lereadmaximumadvertisingdatalength() {
    check(Command::LeReadMaximumAdvertisingDataLength, "013a2000");
}

#[test]
fn cmd_lereadnumberofsupportedadvertisingsets() {
    check(Command::LeReadNumberOfSupportedAdvertisingSets, "013b2000");
}

#[test]
fn cmd_leremoveadvertisingset() {
    check(Command::LeRemoveAdvertisingSet {
            advertising_handle: 1,
    }, "013c200101");
}

#[test]
fn cmd_leclearadvertisingsets() {
    check(Command::LeClearAdvertisingSets, "013d2000");
}

#[test]
fn cmd_lesetperiodicadvertisingparameters() {
    check(Command::LeSetPeriodicAdvertisingParameters {
            advertising_handle: 1,
            periodic_advertising_interval_min: 770,
            periodic_advertising_interval_max: 1284,
            periodic_advertising_properties: 1798,
    }, "013e200701020304050607");
}

#[test]
fn cmd_lesetperiodicadvertisingdata() {
    check(Command::LeSetPeriodicAdvertisingData {
            advertising_handle: 1,
            operation: 2,
            advertising_data: vec![3, 4, 5],
    }, "013f2006010203030405");
}

#[test]
fn cmd_lesetperiodicadvertisingenable() {
    check(Command::LeSetPeriodicAdvertisingEnable {
            enable: 1,
            advertising_handle: 2,
    }, "014020020102");
}

#[test]
fn cmd_lesetextendedscanenable() {
    check(Command::LeSetExtendedScanEnable {
            enable: 1,
            filter_duplicates: 2,
            duration: 1027,
            period: 1541,
    }, "01422006010203040506");
}

#[test]
fn cmd_leperiodicadvertisingcreatesync() {
    check(Command::LePeriodicAdvertisingCreateSync {
            options: 1,
            advertising_sid: 2,
            advertiser_address_type: 3,
            advertiser_address: Address::from_bytes([4, 5, 6, 7, 8, 9], AddressType::RANDOM_DEVICE),
            skip: 2826,
            sync_timeout: 3340,
            sync_cte_type: 14,
    }, "0144200e0102030405060708090a0b0c0d0e");
}

#[test]
fn cmd_leperiodicadvertisingcreatesynccancel() {
    check(Command::LePeriodicAdvertisingCreateSyncCancel, "01452000");
}

#[test]
fn cmd_leperiodicadvertisingterminatesync() {
    check(Command::LePeriodicAdvertisingTerminateSync {
            sync_handle: 513,
    }, "014620020102");
}

#[test]
fn cmd_lereadtransmitpower() {
    check(Command::LeReadTransmitPower, "014b2000");
}

#[test]
fn cmd_lesetprivacymode() {
    check(Command::LeSetPrivacyMode {
            peer_identity_address_type: 1,
            peer_identity_address: Address::from_bytes([2, 3, 4, 5, 6, 7], AddressType::RANDOM_DEVICE),
            privacy_mode: 8,
    }, "014e20080102030405060708");
}

#[test]
fn cmd_lesetperiodicadvertisingreceiveenable() {
    check(Command::LeSetPeriodicAdvertisingReceiveEnable {
            sync_handle: 513,
            enable: 3,
    }, "01592003010203");
}

#[test]
fn cmd_leperiodicadvertisingsynctransfer() {
    check(Command::LePeriodicAdvertisingSyncTransfer {
            connection_handle: 513,
            service_data: 1027,
            sync_handle: 1541,
    }, "015a2006010203040506");
}

#[test]
fn cmd_leperiodicadvertisingsetinfotransfer() {
    check(Command::LePeriodicAdvertisingSetInfoTransfer {
            connection_handle: 513,
            service_data: 1027,
            advertising_handle: 5,
    }, "015b20050102030405");
}

#[test]
fn cmd_lesetperiodicadvertisingsynctransferparameters() {
    check(Command::LeSetPeriodicAdvertisingSyncTransferParameters {
            connection_handle: 513,
            mode: 3,
            skip: 1284,
            sync_timeout: 1798,
            cte_type: 8,
    }, "015c20080102030405060708");
}

#[test]
fn cmd_lesetdefaultperiodicadvertisingsynctransferparameters() {
    check(Command::LeSetDefaultPeriodicAdvertisingSyncTransferParameters {
            mode: 1,
            skip: 770,
            sync_timeout: 1284,
            cte_type: 6,
    }, "015d2006010203040506");
}

#[test]
fn cmd_lereadbuffersizev2() {
    check(Command::LeReadBufferSizeV2, "01602000");
}

#[test]
fn cmd_lereadisotxsync() {
    check(Command::LeReadIsoTxSync {
            connection_handle: 513,
    }, "016120020102");
}

#[test]
fn cmd_lesetcigparameters() {
    check(Command::LeSetCigParameters {
            cig_id: 1,
            sdu_interval_c_to_p: 262914,
            sdu_interval_p_to_c: 460293,
            worst_case_sca: 8,
            packing: 9,
            framing: 10,
            max_transport_latency_c_to_p: 3083,
            max_transport_latency_p_to_c: 3597,
            cis_id: vec![15],
            max_sdu_c_to_p: vec![4368],
            max_sdu_p_to_c: vec![4882],
            phy_c_to_p: vec![20],
            phy_p_to_c: vec![21],
            rtn_c_to_p: vec![22],
            rtn_p_to_c: vec![23],
    }, "016220180102030405060708090a0b0c0d0e010f1011121314151617");
}

#[test]
fn cmd_lecreatecis() {
    check(Command::LeCreateCis {
            cis_connection_handle: vec![513],
            acl_connection_handle: vec![1027],
    }, "016420050101020304");
}

#[test]
fn cmd_leremovecig() {
    check(Command::LeRemoveCig {
            cig_id: 1,
    }, "0165200101");
}

#[test]
fn cmd_leacceptcisrequest() {
    check(Command::LeAcceptCisRequest {
            connection_handle: 513,
    }, "016620020102");
}

#[test]
fn cmd_lerejectcisrequest() {
    check(Command::LeRejectCisRequest {
            connection_handle: 513,
            reason: 3,
    }, "01672003010203");
}

#[test]
fn cmd_lecreatebig() {
    check(Command::LeCreateBig {
            big_handle: 1,
            advertising_handle: 2,
            num_bis: 3,
            sdu_interval: 394500,
            max_sdu: 2055,
            max_transport_latency: 2569,
            rtn: 11,
            phy: 12,
            packing: 13,
            framing: 14,
            encryption: 15,
            broadcast_code: [16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31],
    }, "0168201f0102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f");
}

#[test]
fn cmd_leterminatebig() {
    check(Command::LeTerminateBig {
            big_handle: 1,
            reason: 2,
    }, "016a20020102");
}

#[test]
fn cmd_lebigcreatesync() {
    check(Command::LeBigCreateSync {
            big_handle: 1,
            sync_handle: 770,
            encryption: 4,
            broadcast_code: [5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20],
            mse: 21,
            big_sync_timeout: 5910,
            bis: vec![24],
    }, "016b20190102030405060708090a0b0c0d0e0f10111213141516170118");
}

#[test]
fn cmd_lebigterminatesync() {
    check(Command::LeBigTerminateSync {
            big_handle: 1,
    }, "016c200101");
}

#[test]
fn cmd_lesetupisodatapath() {
    check(Command::LeSetupIsoDataPath {
            connection_handle: 513,
            data_path_direction: 3,
            data_path_id: 4,
            codec_id: CodingFormat { coding_format: 2, company_id: 0, vendor_specific_codec_id: 0 },
            controller_delay: 460293,
            codec_configuration: vec![8, 9, 10],
    }, "016e20100102030402000000000506070308090a");
}

#[test]
fn cmd_leremoveisodatapath() {
    check(Command::LeRemoveIsoDataPath {
            connection_handle: 513,
            data_path_direction: 3,
    }, "016f2003010203");
}

#[test]
fn cmd_lesethostfeature() {
    check(Command::LeSetHostFeature {
            bit_number: 1,
            bit_value: 2,
    }, "017420020102");
}

#[test]
fn cmd_lesetdefaultsubrate() {
    check(Command::LeSetDefaultSubrate {
            subrate_min: 513,
            subrate_max: 1027,
            max_latency: 1541,
            continuation_number: 2055,
            supervision_timeout: 2569,
    }, "017d200a0102030405060708090a");
}

#[test]
fn cmd_lesubraterequest() {
    check(Command::LeSubrateRequest {
            connection_handle: 513,
            subrate_min: 1027,
            subrate_max: 1541,
            max_latency: 2055,
            continuation_number: 2569,
            supervision_timeout: 3083,
    }, "017e200c0102030405060708090a0b0c");
}

#[test]
fn cmd_lecsreadlocalsupportedcapabilities() {
    check(Command::LeCsReadLocalSupportedCapabilities, "01892000");
}

#[test]
fn cmd_lecsreadremotesupportedcapabilities() {
    check(Command::LeCsReadRemoteSupportedCapabilities {
            connection_handle: 513,
    }, "018a20020102");
}

#[test]
fn cmd_lecswritecachedremotesupportedcapabilities() {
    check(Command::LeCsWriteCachedRemoteSupportedCapabilities {
            connection_handle: 513,
            num_config_supported: 3,
            max_consecutive_procedures_supported: 1284,
            num_antennas_supported: 6,
            max_antenna_paths_supported: 7,
            roles_supported: 8,
            modes_supported: 9,
            rtt_capability: 10,
            rtt_aa_only_n: 11,
            rtt_sounding_n: 12,
            rtt_random_sequence_n: 13,
            nadm_sounding_capability: 3854,
            nadm_random_capability: 4368,
            cs_sync_phys_supported: 18,
            subfeatures_supported: 5139,
            t_ip1_times_supported: 5653,
            t_ip2_times_supported: 6167,
            t_fcs_times_supported: 6681,
            t_pm_times_supported: 7195,
            t_sw_time_supported: 29,
            tx_snr_capability: 30,
    }, "018b201e0102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e");
}

#[test]
fn cmd_lecssecurityenable() {
    check(Command::LeCsSecurityEnable {
            connection_handle: 513,
    }, "018c20020102");
}

#[test]
fn cmd_lecssetdefaultsettings() {
    check(Command::LeCsSetDefaultSettings {
            connection_handle: 513,
            role_enable: 3,
            cs_sync_antenna_selection: 4,
            max_tx_power: 5,
    }, "018d20050102030405");
}

#[test]
fn cmd_lecsreadremotefaetable() {
    check(Command::LeCsReadRemoteFaeTable {
            connection_handle: 513,
    }, "018e20020102");
}

#[test]
fn cmd_lecswritecachedremotefaetable() {
    check(Command::LeCsWriteCachedRemoteFaeTable {
            connection_handle: 513,
            remote_fae_table: [3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44, 45, 46, 47, 48, 49, 50, 51, 52, 53, 54, 55, 56, 57, 58, 59, 60, 61, 62, 63, 64, 65, 66, 67, 68, 69, 70, 71, 72, 73, 74],
    }, "018f204a0102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f202122232425262728292a2b2c2d2e2f303132333435363738393a3b3c3d3e3f404142434445464748494a");
}

#[test]
fn cmd_lecscreateconfig() {
    check(Command::LeCsCreateConfig {
            connection_handle: 513,
            config_id: 3,
            create_context: 4,
            main_mode_type: 5,
            sub_mode_type: 6,
            min_main_mode_steps: 7,
            max_main_mode_steps: 8,
            main_mode_repetition: 9,
            mode_0_steps: 10,
            role: 11,
            rtt_type: 12,
            cs_sync_phy: 13,
            channel_map: [14, 15, 16, 17, 18, 19, 20, 21, 22, 23],
            channel_map_repetition: 24,
            channel_selection_type: 25,
            ch3c_shape: 26,
            ch3c_jump: 27,
            reserved: 28,
    }, "0190201c0102030405060708090a0b0c0d0e0f101112131415161718191a1b1c");
}

#[test]
fn cmd_lecsremoveconfig() {
    check(Command::LeCsRemoveConfig {
            connection_handle: 513,
            config_id: 3,
    }, "01912003010203");
}

#[test]
fn cmd_lecssetchannelclassification() {
    check(Command::LeCsSetChannelClassification {
            channel_classification: [1, 2, 3, 4, 5, 6, 7, 8, 9, 10],
    }, "0192200a0102030405060708090a");
}

#[test]
fn cmd_lecssetprocedureparameters() {
    check(Command::LeCsSetProcedureParameters {
            connection_handle: 513,
            config_id: 3,
            max_procedure_len: 1284,
            min_procedure_interval: 1798,
            max_procedure_interval: 2312,
            max_procedure_count: 2826,
            min_subevent_len: 920844,
            max_subevent_len: 1118223,
            tone_antenna_config_selection: 18,
            phy: 19,
            tx_power_delta: 20,
            preferred_peer_antenna: 21,
            snr_control_initiator: 22,
            snr_control_reflector: 23,
    }, "019320170102030405060708090a0b0c0d0e0f1011121314151617");
}

#[test]
fn cmd_lecsprocedureenable() {
    check(Command::LeCsProcedureEnable {
            connection_handle: 513,
            config_id: 3,
            enable: 4,
    }, "0194200401020304");
}

#[test]
fn cmd_lecstest() {
    check(Command::LeCsTest {
            main_mode_type: 1,
            sub_mode_type: 2,
            main_mode_repetition: 3,
            mode_0_steps: 4,
            role: 5,
            rtt_type: 6,
            cs_sync_phy: 7,
            cs_sync_antenna_selection: 8,
            subevent_len: 723465,
            subevent_interval: 3340,
            max_num_subevents: 14,
            transmit_power_level: 15,
            t_ip1_time: 16,
            t_ip2_time: 17,
            t_fcs_time: 18,
            t_pm_time: 19,
            t_sw_time: 20,
            tone_antenna_config_selection: 21,
            reserved: 22,
            snr_control_initiator: 23,
            snr_control_reflector: 24,
            drbg_nonce: 6681,
            channel_map_repetition: 27,
            override_config: 7452,
            override_parameters_data: vec![30, 31, 32],
    }, "019520210102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d031e1f20");
}

#[test]
fn cmd_lecstestend() {
    check(Command::LeCsTestEnd, "01962000");
}

#[test]
fn cmd_leframespaceupdate() {
    check(Command::LeFrameSpaceUpdate {
            connection_handle: 513,
            frame_space_min: 1027,
            frame_space_max: 1541,
            phys: 7,
            spacing_types: 2312,
    }, "019d2009010203040506070809");
}

#[test]
fn cmd_leconnectionraterequest() {
    check(Command::LeConnectionRateRequest {
            connection_handle: 513,
            connection_interval_min: 1027,
            connection_interval_max: 1541,
            subrate_min: 2055,
            subrate_max: 2569,
            max_latency: 3083,
            continuation_number: 3597,
            supervision_timeout: 4111,
            min_ce_length: 4625,
            max_ce_length: 5139,
    }, "01a120140102030405060708090a0b0c0d0e0f1011121314");
}

#[test]
fn cmd_lesetdefaultrateparameters() {
    check(Command::LeSetDefaultRateParameters {
            connection_interval_min: 513,
            connection_interval_max: 1027,
            subrate_min: 1541,
            subrate_max: 2055,
            max_latency: 2569,
            continuation_number: 3083,
            supervision_timeout: 3597,
            min_ce_length: 4111,
            max_ce_length: 4625,
    }, "01a220120102030405060708090a0b0c0d0e0f101112");
}

#[test]
fn cmd_lereadminimumsupportedconnectioninterval() {
    check(Command::LeReadMinimumSupportedConnectionInterval, "01a32000");
}
