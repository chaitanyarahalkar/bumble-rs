//! GENERATED oracle-pinned tests: every typed HCI event/LE-meta sub-event
//! round-trips byte-exact against packet bytes captured from real Python Bumble.
#![allow(clippy::redundant_clone)]

use bumble::{Address, AddressType};
use bumble_hci::{Event, HciPacket, LeMetaEvent};

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}
fn check(ev: Event, expected: &str) {
    let packet = HciPacket::Event(ev);
    let bytes = packet.to_bytes();
    assert_eq!(hex(&bytes), expected, "serialize mismatch");
    assert_eq!(
        HciPacket::from_bytes(&bytes).expect("parse"),
        packet,
        "round-trip mismatch"
    );
}

#[test]
fn evt_inquirycomplete() {
    check(Event::InquiryComplete { status: 1 }, "04010101");
}

#[test]
fn evt_inquiryresult() {
    check(
        Event::InquiryResult {
            bd_addr: vec![Address::from_bytes(
                [1, 2, 3, 4, 5, 6],
                AddressType::RANDOM_DEVICE,
            )],
            page_scan_repetition_mode: vec![7],
            reserved_0: vec![8],
            reserved_1: vec![9],
            class_of_device: vec![789258],
            clock_offset: vec![3597],
        },
        "04020f010102030405060708090a0b0c0d0e",
    );
}

#[test]
fn evt_connectioncomplete() {
    check(
        Event::ConnectionComplete {
            status: 1,
            connection_handle: 770,
            bd_addr: Address::from_bytes([4, 5, 6, 7, 8, 9], AddressType::RANDOM_DEVICE),
            link_type: 10,
            encryption_enabled: 11,
        },
        "04030b0102030405060708090a0b",
    );
}

#[test]
fn evt_connectionrequest() {
    check(
        Event::ConnectionRequest {
            bd_addr: Address::from_bytes([1, 2, 3, 4, 5, 6], AddressType::RANDOM_DEVICE),
            class_of_device: 591879,
            link_type: 10,
        },
        "04040a0102030405060708090a",
    );
}

#[test]
fn evt_disconnectioncomplete() {
    check(
        Event::DisconnectionComplete {
            status: 1,
            connection_handle: 770,
            reason: 4,
        },
        "04050401020304",
    );
}

#[test]
fn evt_authenticationcomplete() {
    check(
        Event::AuthenticationComplete {
            status: 1,
            connection_handle: 770,
        },
        "040603010203",
    );
}

#[test]
fn evt_remotenamerequestcomplete() {
    check(Event::RemoteNameRequestComplete {
            status: 1,
            bd_addr: Address::from_bytes([2, 3, 4, 5, 6, 7], AddressType::RANDOM_DEVICE),
            remote_name: [8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44, 45, 46, 47, 48, 49, 50, 51, 52, 53, 54, 55, 56, 57, 58, 59, 60, 61, 62, 63, 64, 65, 66, 67, 68, 69, 70, 71, 72, 73, 74, 75, 76, 77, 78, 79, 80, 81, 82, 83, 84, 85, 86, 87, 88, 89, 90, 91, 92, 93, 94, 95, 96, 97, 98, 99, 100, 101, 102, 103, 104, 105, 106, 107, 108, 109, 110, 111, 112, 113, 114, 115, 116, 117, 118, 119, 120, 121, 122, 123, 124, 125, 126, 127, 128, 129, 130, 131, 132, 133, 134, 135, 136, 137, 138, 139, 140, 141, 142, 143, 144, 145, 146, 147, 148, 149, 150, 151, 152, 153, 154, 155, 156, 157, 158, 159, 160, 161, 162, 163, 164, 165, 166, 167, 168, 169, 170, 171, 172, 173, 174, 175, 176, 177, 178, 179, 180, 181, 182, 183, 184, 185, 186, 187, 188, 189, 190, 191, 192, 193, 194, 195, 196, 197, 198, 199, 200, 201, 202, 203, 204, 205, 206, 207, 208, 209, 210, 211, 212, 213, 214, 215, 216, 217, 218, 219, 220, 221, 222, 223, 224, 225, 226, 227, 228, 229, 230, 231, 232, 233, 234, 235, 236, 237, 238, 239, 240, 241, 242, 243, 244, 245, 246, 247, 248, 249, 250, 251, 252, 253, 254, 255],
    }, "0407ff0102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f202122232425262728292a2b2c2d2e2f303132333435363738393a3b3c3d3e3f404142434445464748494a4b4c4d4e4f505152535455565758595a5b5c5d5e5f606162636465666768696a6b6c6d6e6f707172737475767778797a7b7c7d7e7f808182838485868788898a8b8c8d8e8f909192939495969798999a9b9c9d9e9fa0a1a2a3a4a5a6a7a8a9aaabacadaeafb0b1b2b3b4b5b6b7b8b9babbbcbdbebfc0c1c2c3c4c5c6c7c8c9cacbcccdcecfd0d1d2d3d4d5d6d7d8d9dadbdcdddedfe0e1e2e3e4e5e6e7e8e9eaebecedeeeff0f1f2f3f4f5f6f7f8f9fafbfcfdfeff");
}

#[test]
fn evt_encryptionchange() {
    check(
        Event::EncryptionChange {
            status: 1,
            connection_handle: 770,
            encryption_enabled: 4,
        },
        "04080401020304",
    );
}

#[test]
fn evt_readremotesupportedfeaturescomplete() {
    check(
        Event::ReadRemoteSupportedFeaturesComplete {
            status: 1,
            connection_handle: 770,
            lmp_features: [4, 5, 6, 7, 8, 9, 10, 11],
        },
        "040b0b0102030405060708090a0b",
    );
}

#[test]
fn evt_readremoteversioninformationcomplete() {
    check(
        Event::ReadRemoteVersionInformationComplete {
            status: 1,
            connection_handle: 770,
            version: 4,
            manufacturer_name: 1541,
            subversion: 2055,
        },
        "040c080102030405060708",
    );
}

#[test]
fn evt_qossetupcomplete() {
    check(
        Event::QosSetupComplete {
            status: 1,
            connection_handle: 770,
            unused: 4,
            service_type: 5,
        },
        "040d050102030405",
    );
}

#[test]
fn evt_commandstatus() {
    check(
        Event::CommandStatus {
            status: 1,
            num_hci_command_packets: 2,
            command_opcode: 1027,
        },
        "040f0401020304",
    );
}

#[test]
fn evt_rolechange() {
    check(
        Event::RoleChange {
            status: 1,
            bd_addr: Address::from_bytes([2, 3, 4, 5, 6, 7], AddressType::RANDOM_DEVICE),
            new_role: 8,
        },
        "0412080102030405060708",
    );
}

#[test]
fn evt_numberofcompletedpackets() {
    check(
        Event::NumberOfCompletedPackets {
            connection_handles: vec![513],
            num_completed_packets: vec![1027],
        },
        "0413050101020304",
    );
}

#[test]
fn evt_modechange() {
    check(
        Event::ModeChange {
            status: 1,
            connection_handle: 770,
            current_mode: 4,
            interval: 1541,
        },
        "041406010203040506",
    );
}

#[test]
fn evt_pincoderequest() {
    check(
        Event::PinCodeRequest {
            bd_addr: Address::from_bytes([1, 2, 3, 4, 5, 6], AddressType::RANDOM_DEVICE),
        },
        "041606010203040506",
    );
}

#[test]
fn evt_linkkeyrequest() {
    check(
        Event::LinkKeyRequest {
            bd_addr: Address::from_bytes([1, 2, 3, 4, 5, 6], AddressType::RANDOM_DEVICE),
        },
        "041706010203040506",
    );
}

#[test]
fn evt_linkkeynotification() {
    check(
        Event::LinkKeyNotification {
            bd_addr: Address::from_bytes([1, 2, 3, 4, 5, 6], AddressType::RANDOM_DEVICE),
            link_key: [7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22],
            key_type: 23,
        },
        "0418170102030405060708090a0b0c0d0e0f1011121314151617",
    );
}

#[test]
fn evt_maxslotschange() {
    check(
        Event::MaxSlotsChange {
            connection_handle: 513,
            lmp_max_slots: 3,
        },
        "041b03010203",
    );
}

#[test]
fn evt_readclockoffsetcomplete() {
    check(
        Event::ReadClockOffsetComplete {
            status: 1,
            connection_handle: 770,
            clock_offset: 1284,
        },
        "041c050102030405",
    );
}

#[test]
fn evt_connectionpackettypechanged() {
    check(
        Event::ConnectionPacketTypeChanged {
            status: 1,
            connection_handle: 770,
            packet_type: 1284,
        },
        "041d050102030405",
    );
}

#[test]
fn evt_pagescanrepetitionmodechange() {
    check(
        Event::PageScanRepetitionModeChange {
            bd_addr: Address::from_bytes([1, 2, 3, 4, 5, 6], AddressType::RANDOM_DEVICE),
            page_scan_repetition_mode: 7,
        },
        "04200701020304050607",
    );
}

#[test]
fn evt_inquiryresultwithrssi() {
    check(
        Event::InquiryResultWithRssi {
            bd_addr: vec![Address::from_bytes(
                [1, 2, 3, 4, 5, 6],
                AddressType::RANDOM_DEVICE,
            )],
            page_scan_repetition_mode: vec![7],
            reserved: vec![8],
            class_of_device: vec![723465],
            clock_offset: vec![3340],
            rssi: vec![5],
        },
        "04220f010102030405060708090a0b0c0d05",
    );
}

#[test]
fn evt_readremoteextendedfeaturescomplete() {
    check(
        Event::ReadRemoteExtendedFeaturesComplete {
            status: 1,
            connection_handle: 770,
            page_number: 4,
            maximum_page_number: 5,
            extended_lmp_features: [6, 7, 8, 9, 10, 11, 12, 13],
        },
        "04230d0102030405060708090a0b0c0d",
    );
}

#[test]
fn evt_synchronousconnectioncomplete() {
    check(
        Event::SynchronousConnectionComplete {
            status: 1,
            connection_handle: 770,
            bd_addr: Address::from_bytes([4, 5, 6, 7, 8, 9], AddressType::RANDOM_DEVICE),
            link_type: 10,
            transmission_interval: 11,
            retransmission_window: 12,
            rx_packet_length: 3597,
            tx_packet_length: 4111,
            air_mode: 17,
        },
        "042c110102030405060708090a0b0c0d0e0f1011",
    );
}

#[test]
fn evt_synchronousconnectionchanged() {
    check(
        Event::SynchronousConnectionChanged {
            status: 1,
            connection_handle: 770,
            transmission_interval: 4,
            retransmission_window: 5,
            rx_packet_length: 1798,
            tx_packet_length: 2312,
        },
        "042d09010203040506070809",
    );
}

#[test]
fn evt_sniffsubrating() {
    check(
        Event::SniffSubrating {
            status: 1,
            connection_handle: 770,
            max_tx_latency: 1284,
            max_rx_latency: 1798,
            min_remote_timeout: 2312,
            min_local_timeout: 2826,
        },
        "042e0b0102030405060708090a0b",
    );
}

#[test]
fn evt_extendedinquiryresult() {
    check(Event::ExtendedInquiryResult {
            num_responses: 1,
            bd_addr: Address::from_bytes([2, 3, 4, 5, 6, 7], AddressType::RANDOM_DEVICE),
            page_scan_repetition_mode: 8,
            reserved: 9,
            class_of_device: 789258,
            clock_offset: 3597,
            rssi: 5,
            extended_inquiry_response: [15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44, 45, 46, 47, 48, 49, 50, 51, 52, 53, 54, 55, 56, 57, 58, 59, 60, 61, 62, 63, 64, 65, 66, 67, 68, 69, 70, 71, 72, 73, 74, 75, 76, 77, 78, 79, 80, 81, 82, 83, 84, 85, 86, 87, 88, 89, 90, 91, 92, 93, 94, 95, 96, 97, 98, 99, 100, 101, 102, 103, 104, 105, 106, 107, 108, 109, 110, 111, 112, 113, 114, 115, 116, 117, 118, 119, 120, 121, 122, 123, 124, 125, 126, 127, 128, 129, 130, 131, 132, 133, 134, 135, 136, 137, 138, 139, 140, 141, 142, 143, 144, 145, 146, 147, 148, 149, 150, 151, 152, 153, 154, 155, 156, 157, 158, 159, 160, 161, 162, 163, 164, 165, 166, 167, 168, 169, 170, 171, 172, 173, 174, 175, 176, 177, 178, 179, 180, 181, 182, 183, 184, 185, 186, 187, 188, 189, 190, 191, 192, 193, 194, 195, 196, 197, 198, 199, 200, 201, 202, 203, 204, 205, 206, 207, 208, 209, 210, 211, 212, 213, 214, 215, 216, 217, 218, 219, 220, 221, 222, 223, 224, 225, 226, 227, 228, 229, 230, 231, 232, 233, 234, 235, 236, 237, 238, 239, 240, 241, 242, 243, 244, 245, 246, 247, 248, 249, 250, 251, 252, 253, 254],
    }, "042fff0102030405060708090a0b0c0d0e050f101112131415161718191a1b1c1d1e1f202122232425262728292a2b2c2d2e2f303132333435363738393a3b3c3d3e3f404142434445464748494a4b4c4d4e4f505152535455565758595a5b5c5d5e5f606162636465666768696a6b6c6d6e6f707172737475767778797a7b7c7d7e7f808182838485868788898a8b8c8d8e8f909192939495969798999a9b9c9d9e9fa0a1a2a3a4a5a6a7a8a9aaabacadaeafb0b1b2b3b4b5b6b7b8b9babbbcbdbebfc0c1c2c3c4c5c6c7c8c9cacbcccdcecfd0d1d2d3d4d5d6d7d8d9dadbdcdddedfe0e1e2e3e4e5e6e7e8e9eaebecedeeeff0f1f2f3f4f5f6f7f8f9fafbfcfdfe");
}

#[test]
fn evt_encryptionkeyrefreshcomplete() {
    check(
        Event::EncryptionKeyRefreshComplete {
            status: 1,
            connection_handle: 770,
        },
        "043003010203",
    );
}

#[test]
fn evt_iocapabilityrequest() {
    check(
        Event::IoCapabilityRequest {
            bd_addr: Address::from_bytes([1, 2, 3, 4, 5, 6], AddressType::RANDOM_DEVICE),
        },
        "043106010203040506",
    );
}

#[test]
fn evt_iocapabilityresponse() {
    check(
        Event::IoCapabilityResponse {
            bd_addr: Address::from_bytes([1, 2, 3, 4, 5, 6], AddressType::RANDOM_DEVICE),
            io_capability: 7,
            oob_data_present: 8,
            authentication_requirements: 9,
        },
        "043209010203040506070809",
    );
}

#[test]
fn evt_userconfirmationrequest() {
    check(
        Event::UserConfirmationRequest {
            bd_addr: Address::from_bytes([1, 2, 3, 4, 5, 6], AddressType::RANDOM_DEVICE),
            numeric_value: 168364039,
        },
        "04330a0102030405060708090a",
    );
}

#[test]
fn evt_userpasskeyrequest() {
    check(
        Event::UserPasskeyRequest {
            bd_addr: Address::from_bytes([1, 2, 3, 4, 5, 6], AddressType::RANDOM_DEVICE),
        },
        "043406010203040506",
    );
}

#[test]
fn evt_remoteoobdatarequest() {
    check(
        Event::RemoteOobDataRequest {
            bd_addr: Address::from_bytes([1, 2, 3, 4, 5, 6], AddressType::RANDOM_DEVICE),
        },
        "043506010203040506",
    );
}

#[test]
fn evt_simplepairingcomplete() {
    check(
        Event::SimplePairingComplete {
            status: 1,
            bd_addr: Address::from_bytes([2, 3, 4, 5, 6, 7], AddressType::RANDOM_DEVICE),
        },
        "04360701020304050607",
    );
}

#[test]
fn evt_linksupervisiontimeoutchanged() {
    check(
        Event::LinkSupervisionTimeoutChanged {
            connection_handle: 513,
            link_supervision_timeout: 1027,
        },
        "04380401020304",
    );
}

#[test]
fn evt_enhancedflushcomplete() {
    check(Event::EnhancedFlushComplete { handle: 513 }, "0439020102");
}

#[test]
fn evt_userpasskeynotification() {
    check(
        Event::UserPasskeyNotification {
            bd_addr: Address::from_bytes([1, 2, 3, 4, 5, 6], AddressType::RANDOM_DEVICE),
            passkey: 168364039,
        },
        "043b0a0102030405060708090a",
    );
}

#[test]
fn evt_keypressnotification() {
    check(
        Event::KeypressNotification {
            bd_addr: Address::from_bytes([1, 2, 3, 4, 5, 6], AddressType::RANDOM_DEVICE),
            notification_type: 7,
        },
        "043c0701020304050607",
    );
}

#[test]
fn evt_remotehostsupportedfeaturesnotification() {
    check(
        Event::RemoteHostSupportedFeaturesNotification {
            bd_addr: Address::from_bytes([1, 2, 3, 4, 5, 6], AddressType::RANDOM_DEVICE),
            host_supported_features: [7, 8, 9, 10, 11, 12, 13, 14],
        },
        "043d0e0102030405060708090a0b0c0d0e",
    );
}

#[test]
fn evt_encryptionchangev2() {
    check(
        Event::EncryptionChangeV2 {
            status: 1,
            connection_handle: 770,
            encryption_enabled: 4,
            encryption_key_size: 5,
        },
        "0459050102030405",
    );
}

#[test]
fn evt_vendor() {
    check(
        Event::Vendor {
            data: vec![1, 2, 3, 4],
        },
        "04ff0401020304",
    );
}

#[test]
fn meta_connectioncomplete() {
    check(
        Event::LeMeta(LeMetaEvent::ConnectionComplete {
            status: 1,
            connection_handle: 770,
            role: 4,
            peer_address_type: 5,
            peer_address: Address::from_bytes([6, 7, 8, 9, 10, 11], AddressType::RANDOM_DEVICE),
            connection_interval: 3340,
            peripheral_latency: 3854,
            supervision_timeout: 4368,
            central_clock_accuracy: 18,
        }),
        "043e13010102030405060708090a0b0c0d0e0f101112",
    );
}

#[test]
fn meta_connectionupdatecomplete() {
    check(
        Event::LeMeta(LeMetaEvent::ConnectionUpdateComplete {
            status: 1,
            connection_handle: 770,
            connection_interval: 1284,
            peripheral_latency: 1798,
            supervision_timeout: 2312,
        }),
        "043e0a03010203040506070809",
    );
}

#[test]
fn meta_readremotefeaturescomplete() {
    check(
        Event::LeMeta(LeMetaEvent::ReadRemoteFeaturesComplete {
            status: 1,
            connection_handle: 770,
            le_features: [4, 5, 6, 7, 8, 9, 10, 11],
        }),
        "043e0c040102030405060708090a0b",
    );
}

#[test]
fn meta_longtermkeyrequest() {
    check(
        Event::LeMeta(LeMetaEvent::LongTermKeyRequest {
            connection_handle: 513,
            random_number: [3, 4, 5, 6, 7, 8, 9, 10],
            encryption_diversifier: 3083,
        }),
        "043e0d050102030405060708090a0b0c",
    );
}

#[test]
fn meta_remoteconnectionparameterrequest() {
    check(
        Event::LeMeta(LeMetaEvent::RemoteConnectionParameterRequest {
            connection_handle: 513,
            interval_min: 1027,
            interval_max: 1541,
            max_latency: 2055,
            timeout: 2569,
        }),
        "043e0b060102030405060708090a",
    );
}

#[test]
fn meta_datalengthchange() {
    check(
        Event::LeMeta(LeMetaEvent::DataLengthChange {
            connection_handle: 513,
            max_tx_octets: 1027,
            max_tx_time: 1541,
            max_rx_octets: 2055,
            max_rx_time: 2569,
        }),
        "043e0b070102030405060708090a",
    );
}

#[test]
fn meta_enhancedconnectioncomplete() {
    check(
        Event::LeMeta(LeMetaEvent::EnhancedConnectionComplete {
            status: 1,
            connection_handle: 770,
            role: 4,
            peer_address_type: 5,
            peer_address: Address::from_bytes([6, 7, 8, 9, 10, 11], AddressType::RANDOM_DEVICE),
            local_resolvable_private_address: Address::from_bytes(
                [12, 13, 14, 15, 16, 17],
                AddressType::RANDOM_DEVICE,
            ),
            peer_resolvable_private_address: Address::from_bytes(
                [18, 19, 20, 21, 22, 23],
                AddressType::RANDOM_DEVICE,
            ),
            connection_interval: 6424,
            peripheral_latency: 6938,
            supervision_timeout: 7452,
            central_clock_accuracy: 30,
        }),
        "043e1f0a0102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e",
    );
}

#[test]
fn meta_phyupdatecomplete() {
    check(
        Event::LeMeta(LeMetaEvent::PhyUpdateComplete {
            status: 1,
            connection_handle: 770,
            tx_phy: 4,
            rx_phy: 5,
        }),
        "043e060c0102030405",
    );
}

#[test]
fn meta_periodicadvertisingsyncestablished() {
    check(
        Event::LeMeta(LeMetaEvent::PeriodicAdvertisingSyncEstablished {
            status: 1,
            sync_handle: 770,
            advertising_sid: 4,
            advertiser_address_type: 5,
            advertiser_address: Address::from_bytes(
                [6, 7, 8, 9, 10, 11],
                AddressType::RANDOM_DEVICE,
            ),
            advertiser_phy: 12,
            periodic_advertising_interval: 3597,
            advertiser_clock_accuracy: 15,
        }),
        "043e100e0102030405060708090a0b0c0d0e0f",
    );
}

#[test]
fn meta_periodicadvertisingreport() {
    check(
        Event::LeMeta(LeMetaEvent::PeriodicAdvertisingReport {
            sync_handle: 513,
            tx_power: 5,
            rssi: 5,
            cte_type: 3,
            data_status: 4,
            data: vec![5, 6, 7],
        }),
        "043e0b0f01020505030403050607",
    );
}

#[test]
fn meta_periodicadvertisingsynclost() {
    check(
        Event::LeMeta(LeMetaEvent::PeriodicAdvertisingSyncLost { sync_handle: 513 }),
        "043e03100102",
    );
}

#[test]
fn meta_advertisingsetterminated() {
    check(
        Event::LeMeta(LeMetaEvent::AdvertisingSetTerminated {
            status: 1,
            advertising_handle: 2,
            connection_handle: 1027,
            num_completed_extended_advertising_events: 5,
        }),
        "043e06120102030405",
    );
}

#[test]
fn meta_channelselectionalgorithm() {
    check(
        Event::LeMeta(LeMetaEvent::ChannelSelectionAlgorithm {
            connection_handle: 513,
            channel_selection_algorithm: 3,
        }),
        "043e0414010203",
    );
}

#[test]
fn meta_periodicadvertisingsynctransferreceived() {
    check(
        Event::LeMeta(LeMetaEvent::PeriodicAdvertisingSyncTransferReceived {
            status: 1,
            connection_handle: 770,
            service_data: 1284,
            sync_handle: 1798,
            advertising_sid: 8,
            advertiser_address_type: 9,
            advertiser_address: Address::from_bytes(
                [10, 11, 12, 13, 14, 15],
                AddressType::RANDOM_DEVICE,
            ),
            advertiser_phy: 16,
            periodic_advertising_interval: 4625,
            advertiser_clock_accuracy: 19,
        }),
        "043e14180102030405060708090a0b0c0d0e0f10111213",
    );
}

#[test]
fn meta_cisestablished() {
    check(
        Event::LeMeta(LeMetaEvent::CisEstablished {
            status: 1,
            connection_handle: 770,
            cig_sync_delay: 394500,
            cis_sync_delay: 591879,
            transport_latency_c_to_p: 789258,
            transport_latency_p_to_c: 986637,
            phy_c_to_p: 16,
            phy_p_to_c: 17,
            nse: 18,
            bn_c_to_p: 19,
            bn_p_to_c: 20,
            ft_c_to_p: 21,
            ft_p_to_c: 22,
            max_pdu_c_to_p: 6167,
            max_pdu_p_to_c: 6681,
            iso_interval: 7195,
        }),
        "043e1d190102030405060708090a0b0c0d0e0f101112131415161718191a1b1c",
    );
}

#[test]
fn meta_cisrequest() {
    check(
        Event::LeMeta(LeMetaEvent::CisRequest {
            acl_connection_handle: 513,
            cis_connection_handle: 1027,
            cig_id: 5,
            cis_id: 6,
        }),
        "043e071a010203040506",
    );
}

#[test]
fn meta_createbigcomplete() {
    check(
        Event::LeMeta(LeMetaEvent::CreateBigComplete {
            status: 1,
            big_handle: 2,
            big_sync_delay: 328707,
            transport_latency_big: 526086,
            phy: 9,
            nse: 10,
            bn: 11,
            pto: 12,
            irc: 13,
            max_pdu: 3854,
            iso_interval: 4368,
            connection_handle: vec![4882],
        }),
        "043e151b0102030405060708090a0b0c0d0e0f1011011213",
    );
}

#[test]
fn meta_terminatebigcomplete() {
    check(
        Event::LeMeta(LeMetaEvent::TerminateBigComplete {
            big_handle: 1,
            reason: 2,
        }),
        "043e031c0102",
    );
}

#[test]
fn meta_bigsyncestablished() {
    check(
        Event::LeMeta(LeMetaEvent::BigSyncEstablished {
            status: 1,
            big_handle: 2,
            transport_latency_big: 328707,
            nse: 6,
            bn: 7,
            pto: 8,
            irc: 9,
            max_pdu: 2826,
            iso_interval: 3340,
            connection_handle: vec![3854],
        }),
        "043e111d0102030405060708090a0b0c0d010e0f",
    );
}

#[test]
fn meta_bigsynclost() {
    check(
        Event::LeMeta(LeMetaEvent::BigSyncLost {
            big_handle: 1,
            reason: 2,
        }),
        "043e031e0102",
    );
}

#[test]
fn meta_biginfoadvertisingreport() {
    check(
        Event::LeMeta(LeMetaEvent::BiginfoAdvertisingReport {
            sync_handle: 513,
            num_bis: 3,
            nse: 4,
            iso_interval: 1541,
            bn: 7,
            pto: 8,
            irc: 9,
            max_pdu: 2826,
            sdu_interval: 920844,
            max_sdu: 4111,
            phy: 17,
            framing: 18,
            encryption: 19,
        }),
        "043e14220102030405060708090a0b0c0d0e0f10111213",
    );
}

#[test]
fn meta_subratechange() {
    check(
        Event::LeMeta(LeMetaEvent::SubrateChange {
            status: 1,
            connection_handle: 770,
            subrate_factor: 1284,
            peripheral_latency: 1798,
            continuation_number: 2312,
            supervision_timeout: 2826,
        }),
        "043e0c230102030405060708090a0b",
    );
}

#[test]
fn meta_periodicadvertisingsyncestablishedv2() {
    check(
        Event::LeMeta(LeMetaEvent::PeriodicAdvertisingSyncEstablishedV2 {
            status: 1,
            sync_handle: 770,
            advertising_sid: 4,
            advertiser_address_type: 5,
            advertiser_address: Address::from_bytes(
                [6, 7, 8, 9, 10, 11],
                AddressType::RANDOM_DEVICE,
            ),
            advertiser_phy: 12,
            periodic_advertising_interval: 3597,
            advertiser_clock_accuracy: 15,
            num_subevents: 16,
            subevent_interval: 17,
            response_slot_delay: 18,
            response_slot_spacing: 19,
        }),
        "043e14240102030405060708090a0b0c0d0e0f10111213",
    );
}

#[test]
fn meta_periodicadvertisingreportv2() {
    check(
        Event::LeMeta(LeMetaEvent::PeriodicAdvertisingReportV2 {
            sync_handle: 513,
            tx_power: 5,
            rssi: 5,
            cte_type: 3,
            periodic_event_counter: 1284,
            subevent: 6,
            data_status: 7,
            data: vec![8, 9, 10],
        }),
        "043e0e250102050503040506070308090a",
    );
}

#[test]
fn meta_periodicadvertisingsynctransferreceivedv2() {
    check(
        Event::LeMeta(LeMetaEvent::PeriodicAdvertisingSyncTransferReceivedV2 {
            status: 1,
            connection_handle: 770,
            service_data: 1284,
            sync_handle: 1798,
            advertising_sid: 8,
            advertiser_address_type: 9,
            advertiser_address: Address::from_bytes(
                [10, 11, 12, 13, 14, 15],
                AddressType::RANDOM_DEVICE,
            ),
            advertiser_phy: 16,
            periodic_advertising_interval: 4625,
            advertiser_clock_accuracy: 19,
            num_subevents: 20,
            subevent_interval: 21,
            response_slot_delay: 22,
            response_slot_spacing: 23,
        }),
        "043e18260102030405060708090a0b0c0d0e0f1011121314151617",
    );
}

#[test]
fn meta_enhancedconnectioncompletev2() {
    check(
        Event::LeMeta(LeMetaEvent::EnhancedConnectionCompleteV2 {
            status: 1,
            connection_handle: 770,
            role: 4,
            peer_address_type: 5,
            peer_address: Address::from_bytes([6, 7, 8, 9, 10, 11], AddressType::RANDOM_DEVICE),
            local_resolvable_private_address: Address::from_bytes(
                [12, 13, 14, 15, 16, 17],
                AddressType::RANDOM_DEVICE,
            ),
            peer_resolvable_private_address: Address::from_bytes(
                [18, 19, 20, 21, 22, 23],
                AddressType::RANDOM_DEVICE,
            ),
            connection_interval: 6424,
            peripheral_latency: 6938,
            supervision_timeout: 7452,
            central_clock_accuracy: 30,
            advertising_handle: 31,
            sync_handle: 8480,
        }),
        "043e22290102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f2021",
    );
}

#[test]
fn meta_csreadremotesupportedcapabilitiescomplete() {
    check(
        Event::LeMeta(LeMetaEvent::CsReadRemoteSupportedCapabilitiesComplete {
            status: 1,
            connection_handle: 770,
            num_config_supported: 4,
            max_consecutive_procedures_supported: 1541,
            num_antennas_supported: 7,
            max_antenna_paths_supported: 8,
            roles_supported: 9,
            modes_supported: 10,
            rtt_capability: 11,
            rtt_aa_only_n: 12,
            rtt_sounding_n: 13,
            rtt_random_sequence_n: 14,
            nadm_sounding_capability: 4111,
            nadm_random_capability: 4625,
            cs_sync_phys_supported: 19,
            subfeatures_supported: 5396,
            t_ip1_times_supported: 5910,
            t_ip2_times_supported: 6424,
            t_fcs_times_supported: 6938,
            t_pm_times_supported: 7452,
            t_sw_time_supported: 30,
            tx_snr_capability: 31,
        }),
        "043e202c0102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f",
    );
}

#[test]
fn meta_csreadremotefaetablecomplete() {
    check(Event::LeMeta(LeMetaEvent::CsReadRemoteFaeTableComplete {
            status: 1,
            connection_handle: 770,
            remote_fae_table: [4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44, 45, 46, 47, 48, 49, 50, 51, 52, 53, 54, 55, 56, 57, 58, 59, 60, 61, 62, 63, 64, 65, 66, 67, 68, 69, 70, 71, 72, 73, 74, 75],
    }), "043e4c2d0102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f202122232425262728292a2b2c2d2e2f303132333435363738393a3b3c3d3e3f404142434445464748494a4b");
}

#[test]
fn meta_cssecurityenablecomplete() {
    check(
        Event::LeMeta(LeMetaEvent::CsSecurityEnableComplete {
            status: 1,
            connection_handle: 770,
        }),
        "043e042e010203",
    );
}

#[test]
fn meta_csconfigcomplete() {
    check(
        Event::LeMeta(LeMetaEvent::CsConfigComplete {
            status: 1,
            connection_handle: 770,
            config_id: 4,
            action: 5,
            main_mode_type: 6,
            sub_mode_type: 7,
            min_main_mode_steps: 8,
            max_main_mode_steps: 9,
            main_mode_repetition: 10,
            mode_0_steps: 11,
            role: 12,
            rtt_type: 13,
            cs_sync_phy: 14,
            channel_map: [15, 16, 17, 18, 19, 20, 21, 22, 23, 24],
            channel_map_repetition: 25,
            channel_selection_type: 26,
            ch3c_shape: 27,
            ch3c_jump: 28,
            reserved: 29,
            t_ip1_time: 30,
            t_ip2_time: 31,
            t_fcs_time: 32,
            t_pm_time: 33,
        }),
        "043e222f0102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f2021",
    );
}

#[test]
fn meta_csprocedureenablecomplete() {
    check(
        Event::LeMeta(LeMetaEvent::CsProcedureEnableComplete {
            status: 1,
            connection_handle: 770,
            config_id: 4,
            state: 5,
            tone_antenna_config_selection: 6,
            selected_tx_power: 5,
            subevent_len: 591879,
            subevents_per_event: 10,
            subevent_interval: 3083,
            event_interval: 3597,
            procedure_interval: 4111,
            procedure_count: 4625,
            max_procedure_len: 5139,
        }),
        "043e1630010203040506050708090a0b0c0d0e0f1011121314",
    );
}

#[test]
fn meta_cssubeventresult() {
    check(
        Event::LeMeta(LeMetaEvent::CsSubeventResult {
            connection_handle: 513,
            config_id: 3,
            start_acl_conn_event_counter: 1284,
            procedure_counter: 1798,
            frequency_compensation: 2312,
            reference_power_level: 5,
            procedure_done_status: 10,
            subevent_done_status: 11,
            abort_reason: 12,
            num_antenna_paths: 13,
            step_mode: vec![14],
            step_channel: vec![15],
            step_data: vec![vec![16, 17, 18]],
        }),
        "043e1631010203040506070809050a0b0c0d010e0f03101112",
    );
}

#[test]
fn meta_cssubeventresultcontinue() {
    check(
        Event::LeMeta(LeMetaEvent::CsSubeventResultContinue {
            connection_handle: 513,
            config_id: 3,
            procedure_done_status: 4,
            subevent_done_status: 5,
            abort_reason: 6,
            num_antenna_paths: 7,
            step_mode: vec![8],
            step_channel: vec![9],
            step_data: vec![vec![10, 11, 12]],
        }),
        "043e0f3201020304050607010809030a0b0c",
    );
}

#[test]
fn meta_cstestendcomplete() {
    check(
        Event::LeMeta(LeMetaEvent::CsTestEndComplete {
            connection_handle: 513,
            status: 3,
        }),
        "043e0433010203",
    );
}

#[test]
fn meta_connectionratechange() {
    check(
        Event::LeMeta(LeMetaEvent::ConnectionRateChange {
            status: 1,
            connection_handle: 770,
            connection_interval: 1284,
            subrate_factor: 1798,
            peripheral_latency: 2312,
            continuation_number: 2826,
            supervision_timeout: 3340,
        }),
        "043e0e370102030405060708090a0b0c0d",
    );
}
