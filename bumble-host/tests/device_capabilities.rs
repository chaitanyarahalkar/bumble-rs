use bumble::{Address, AddressType};
use bumble_hci::{
    AclDataPacket, Command, Event, HciPacket, IsoDataPacket, LeMetaEvent, ReturnParameters,
    HCI_LE_READ_ALL_LOCAL_SUPPORTED_FEATURES_COMMAND, HCI_LE_READ_BUFFER_SIZE_V2_COMMAND,
    HCI_LE_READ_LOCAL_SUPPORTED_FEATURES_COMMAND,
    HCI_LE_READ_MAXIMUM_ADVERTISING_DATA_LENGTH_COMMAND,
    HCI_LE_READ_NUMBER_OF_SUPPORTED_ADVERTISING_SETS_COMMAND,
    HCI_LE_READ_SUGGESTED_DEFAULT_DATA_LENGTH_COMMAND, HCI_LE_SET_EVENT_MASK_COMMAND,
    HCI_LE_WRITE_SUGGESTED_DEFAULT_DATA_LENGTH_COMMAND, HCI_READ_BUFFER_SIZE_COMMAND,
    HCI_READ_LOCAL_EXTENDED_FEATURES_COMMAND, HCI_READ_LOCAL_SUPPORTED_COMMANDS_COMMAND,
    HCI_READ_LOCAL_SUPPORTED_FEATURES_COMMAND, HCI_READ_LOCAL_VERSION_INFORMATION_COMMAND,
    HCI_SET_EVENT_MASK_COMMAND, HCI_SET_EVENT_MASK_PAGE_2_COMMAND,
};
use bumble_host::{
    ControllerBufferInfo, Device, DeviceEvent, HostTransport, LeSuggestedDefaultDataLength,
    LocalVersionInformation, HOST_DEFAULT_MAXIMUM_ADVERTISING_DATA_LENGTH, HOST_EVENT_MASK,
    HOST_EVENT_MASK_PAGE_2, HOST_LE_EVENT_MASK, HOST_LE_EVENT_MASK_LEGACY,
    HOST_SUGGESTED_MAX_TX_OCTETS, HOST_SUGGESTED_MAX_TX_TIME, LE_1M_PHY, LE_2M_PHY, LE_CODED_PHY,
    LE_FEATURE_2M_PHY, LE_FEATURE_CODED_PHY, LE_FEATURE_PERIODIC_ADVERTISING,
    LMP_FEATURE_INTERLACED_INQUIRY_SCAN, LMP_FEATURE_INTERLACED_PAGE_SCAN,
};

#[derive(Default)]
struct ScriptedTransport {
    commands: Vec<Command>,
    events: Vec<HciPacket>,
    acl_packets: Vec<AclDataPacket>,
    iso_packets: Vec<IsoDataPacket>,
}

impl HostTransport for ScriptedTransport {
    fn handle_command(&mut self, _controller_id: usize, command: Command) {
        self.commands.push(command);
    }

    fn send_acl_packet(&mut self, _controller_id: usize, packet: AclDataPacket) -> bool {
        self.acl_packets.push(packet);
        true
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

    fn send_iso_packet(&mut self, _controller_id: usize, packet: IsoDataPacket) -> bool {
        self.iso_packets.push(packet);
        true
    }

    fn drain_host_events(&mut self, _controller_id: usize) -> Vec<HciPacket> {
        core::mem::take(&mut self.events)
    }
}

fn command_complete(command_opcode: u16, return_parameters: ReturnParameters) -> HciPacket {
    HciPacket::Event(Event::CommandComplete {
        num_hci_command_packets: 1,
        command_opcode,
        return_parameters,
    })
}

fn host_mask_commands(le_event_mask: [u8; 8]) -> Vec<Command> {
    vec![
        Command::SetEventMask {
            event_mask: HOST_EVENT_MASK,
        },
        Command::LeSetEventMask { le_event_mask },
    ]
}

fn complete_host_masks(transport: &mut ScriptedTransport) {
    transport.events.extend([
        command_complete(
            HCI_SET_EVENT_MASK_COMMAND,
            ReturnParameters::Status { status: 0 },
        ),
        command_complete(
            HCI_LE_SET_EVENT_MASK_COMMAND,
            ReturnParameters::Status { status: 0 },
        ),
    ]);
}

#[test]
fn power_on_selects_legacy_le_feature_fallback_and_exposes_capabilities() {
    let mut device = Device::new(0);
    let mut transport = ScriptedTransport::default();
    device.power_on(&mut transport).unwrap();
    assert_eq!(
        transport.commands.last(),
        Some(&Command::ReadLocalSupportedCommands)
    );
    transport.commands.clear();

    let mut supported_commands = [0; 64];
    supported_commands[25] = 1 << 2;
    transport.events.push(command_complete(
        HCI_READ_LOCAL_SUPPORTED_COMMANDS_COMMAND,
        ReturnParameters::ReadLocalSupportedCommands {
            status: 0,
            supported_commands,
        },
    ));
    assert!(device.poll(&mut transport));
    assert_eq!(
        transport.commands,
        vec![Command::LeReadLocalSupportedFeatures]
    );
    transport.commands.clear();

    let mut features = [0; 8];
    features[1] = 0x29; // 2M, Coded, and Periodic Advertising.
    transport.events.push(command_complete(
        HCI_LE_READ_LOCAL_SUPPORTED_FEATURES_COMMAND,
        ReturnParameters::LeReadLocalSupportedFeatures {
            status: 0,
            le_features: features,
        },
    ));
    assert!(device.poll(&mut transport));
    assert_eq!(transport.commands, host_mask_commands(HOST_LE_EVENT_MASK));
    complete_host_masks(&mut transport);
    assert!(device.poll(&mut transport));

    assert_eq!(device.local_supported_commands_status(), Some(0));
    assert_eq!(device.local_supported_commands(), Some(&supported_commands));
    assert_eq!(device.local_le_features_status(), Some(0));
    assert_eq!(device.local_le_features(), Some(features.as_slice()));
    assert_eq!(device.local_le_features_max_page(), None);
    assert!(device.supports_le_features(&[
        LE_FEATURE_2M_PHY,
        LE_FEATURE_CODED_PHY,
        LE_FEATURE_PERIODIC_ADVERTISING,
    ]));
    assert_eq!(device.supports_le_phy(LE_1M_PHY), Ok(true));
    assert_eq!(device.supports_le_phy(LE_2M_PHY), Ok(true));
    assert_eq!(device.supports_le_phy(LE_CODED_PHY), Ok(true));
    assert!(!device.supports_le_extended_advertising());
    assert!(device.supports_le_periodic_advertising());
    assert!(device.host_initialization_complete());
    assert!(device.host_initialization_succeeded());
}

#[test]
fn power_on_discovers_local_version_and_every_lmp_feature_page() {
    let mut device = Device::new(0);
    device.config.classic_enabled = true;
    device.config.classic_interlaced_scan_enabled = true;
    let mut transport = ScriptedTransport::default();
    device.power_on(&mut transport).unwrap();
    transport.commands.clear();

    let mut supported_commands = [0; 64];
    supported_commands[14] = (1 << 3) | (1 << 6);
    transport.events.push(command_complete(
        HCI_READ_LOCAL_SUPPORTED_COMMANDS_COMMAND,
        ReturnParameters::ReadLocalSupportedCommands {
            status: 0,
            supported_commands,
        },
    ));
    assert!(device.poll(&mut transport));
    assert_eq!(
        transport.commands,
        vec![
            Command::ReadLocalVersionInformation,
            Command::ReadLocalExtendedFeatures { page_number: 0 },
        ]
    );
    transport.commands.clear();

    let mut page_0 = [0; 8];
    page_0[3] = 0x30;
    transport.events.extend([
        command_complete(
            HCI_READ_LOCAL_VERSION_INFORMATION_COMMAND,
            ReturnParameters::ReadLocalVersionInformation {
                status: 0,
                hci_version: 13,
                hci_subversion: 0x1234,
                lmp_version: 12,
                company_identifier: 0x00E0,
                lmp_subversion: 0x5678,
            },
        ),
        command_complete(
            HCI_READ_LOCAL_EXTENDED_FEATURES_COMMAND,
            ReturnParameters::ReadLocalExtendedFeatures {
                status: 0,
                page_number: 0,
                maximum_page_number: 2,
                extended_lmp_features: page_0,
            },
        ),
    ]);
    assert!(device.poll(&mut transport));
    assert_eq!(
        transport.commands,
        vec![Command::ReadLocalExtendedFeatures { page_number: 1 }]
    );
    transport.commands.clear();

    transport.events.push(command_complete(
        HCI_READ_LOCAL_EXTENDED_FEATURES_COMMAND,
        ReturnParameters::ReadLocalExtendedFeatures {
            status: 0,
            page_number: 1,
            maximum_page_number: 2,
            extended_lmp_features: [0x11; 8],
        },
    ));
    assert!(device.poll(&mut transport));
    assert_eq!(
        transport.commands,
        vec![Command::ReadLocalExtendedFeatures { page_number: 2 }]
    );
    transport.commands.clear();

    transport.events.push(command_complete(
        HCI_READ_LOCAL_EXTENDED_FEATURES_COMMAND,
        ReturnParameters::ReadLocalExtendedFeatures {
            status: 0,
            page_number: 2,
            maximum_page_number: 2,
            extended_lmp_features: [0x22; 8],
        },
    ));
    assert!(device.poll(&mut transport));
    assert_eq!(transport.commands, host_mask_commands(HOST_LE_EVENT_MASK));
    transport.commands.clear();
    complete_host_masks(&mut transport);
    assert!(device.poll(&mut transport));
    assert_eq!(
        transport.commands,
        vec![
            Command::WritePageScanType { page_scan_type: 1 },
            Command::WriteInquiryScanType { scan_type: 1 },
        ]
    );

    assert_eq!(
        device.local_version(),
        Some(LocalVersionInformation {
            hci_version: 13,
            hci_subversion: 0x1234,
            lmp_version: 12,
            company_identifier: 0x00E0,
            lmp_subversion: 0x5678,
        })
    );
    assert_eq!(device.local_version_status(), Some(0));
    assert_eq!(device.local_lmp_features_max_page(), Some(2));
    assert_eq!(device.local_lmp_feature_page(0), Some(&page_0));
    assert_eq!(device.local_lmp_feature_page(1), Some(&[0x11; 8]));
    assert_eq!(device.local_lmp_feature_page(2), Some(&[0x22; 8]));
    assert_eq!(device.local_lmp_feature_status(0), Some(0));
    assert_eq!(device.local_lmp_feature_status(1), Some(0));
    assert_eq!(device.local_lmp_feature_status(2), Some(0));
    assert!(device.supports_lmp_features(&[
        LMP_FEATURE_INTERLACED_INQUIRY_SCAN,
        LMP_FEATURE_INTERLACED_PAGE_SCAN,
    ]));
    assert!(device.host_initialization_succeeded());
}

#[test]
fn legacy_lmp_feature_failure_is_retained_without_enabling_scan_modes() {
    let mut device = Device::new(0);
    device.config.classic_enabled = true;
    device.config.classic_interlaced_scan_enabled = true;
    let mut transport = ScriptedTransport::default();
    let mut supported_commands = [0; 64];
    supported_commands[14] = 1 << 5;
    transport.events.push(command_complete(
        HCI_READ_LOCAL_SUPPORTED_COMMANDS_COMMAND,
        ReturnParameters::ReadLocalSupportedCommands {
            status: 0,
            supported_commands,
        },
    ));
    assert!(device.poll(&mut transport));
    assert_eq!(
        transport.commands,
        vec![Command::ReadLocalSupportedFeatures]
    );
    transport.commands.clear();

    transport.events.push(command_complete(
        HCI_READ_LOCAL_SUPPORTED_FEATURES_COMMAND,
        ReturnParameters::Status { status: 0x01 },
    ));
    assert!(device.poll(&mut transport));

    assert_eq!(transport.commands, host_mask_commands(HOST_LE_EVENT_MASK));
    assert_eq!(device.local_lmp_feature_status(0), Some(0x01));
    assert_eq!(device.local_lmp_feature_page(0), None);
    assert_eq!(device.local_lmp_features_max_page(), None);
    assert!(!device.supports_lmp_feature(LMP_FEATURE_INTERLACED_PAGE_SCAN));
}

#[test]
fn failed_extended_lmp_page_stops_the_sequence_with_correlated_status() {
    let mut device = Device::new(0);
    let mut transport = ScriptedTransport::default();
    let mut supported_commands = [0; 64];
    supported_commands[14] = 1 << 6;
    transport.events.push(command_complete(
        HCI_READ_LOCAL_SUPPORTED_COMMANDS_COMMAND,
        ReturnParameters::ReadLocalSupportedCommands {
            status: 0,
            supported_commands,
        },
    ));
    assert!(device.poll(&mut transport));
    assert_eq!(
        transport.commands,
        vec![Command::ReadLocalExtendedFeatures { page_number: 0 }]
    );
    transport.commands.clear();

    transport.events.push(command_complete(
        HCI_READ_LOCAL_EXTENDED_FEATURES_COMMAND,
        ReturnParameters::ReadLocalExtendedFeatures {
            status: 0,
            page_number: 0,
            maximum_page_number: 2,
            extended_lmp_features: [0xAA; 8],
        },
    ));
    assert!(device.poll(&mut transport));
    assert_eq!(
        transport.commands,
        vec![Command::ReadLocalExtendedFeatures { page_number: 1 }]
    );
    transport.commands.clear();

    transport.events.push(command_complete(
        HCI_READ_LOCAL_EXTENDED_FEATURES_COMMAND,
        ReturnParameters::Status { status: 0x01 },
    ));
    assert!(device.poll(&mut transport));

    assert_eq!(transport.commands, host_mask_commands(HOST_LE_EVENT_MASK));
    assert_eq!(device.local_lmp_feature_page(0), Some(&[0xAA; 8]));
    assert_eq!(device.local_lmp_feature_status(0), Some(0));
    assert_eq!(device.local_lmp_feature_page(1), None);
    assert_eq!(device.local_lmp_feature_status(1), Some(0x01));
    assert_eq!(device.local_lmp_features_max_page(), Some(2));
}

#[test]
fn reset_installs_version_compatible_masks_and_v2_packet_pools() {
    let mut device = Device::new(0);
    let mut transport = ScriptedTransport::default();
    let mut supported_commands = [0; 64];
    supported_commands[14] = (1 << 3) | (1 << 7);
    supported_commands[22] = 1 << 2;
    supported_commands[41] = 1 << 5;
    transport.events.push(command_complete(
        HCI_READ_LOCAL_SUPPORTED_COMMANDS_COMMAND,
        ReturnParameters::ReadLocalSupportedCommands {
            status: 0,
            supported_commands,
        },
    ));
    assert!(device.poll(&mut transport));
    assert_eq!(
        transport.commands,
        vec![Command::ReadLocalVersionInformation]
    );
    transport.commands.clear();

    transport.events.push(command_complete(
        HCI_READ_LOCAL_VERSION_INFORMATION_COMMAND,
        ReturnParameters::ReadLocalVersionInformation {
            status: 0,
            hci_version: 6,
            hci_subversion: 0x0102,
            lmp_version: 6,
            company_identifier: 0x00E0,
            lmp_subversion: 0x0304,
        },
    ));
    assert!(device.poll(&mut transport));
    assert_eq!(
        transport.commands,
        vec![
            Command::SetEventMask {
                event_mask: HOST_EVENT_MASK,
            },
            Command::SetEventMaskPage2 {
                event_mask_page_2: HOST_EVENT_MASK_PAGE_2,
            },
            Command::LeSetEventMask {
                le_event_mask: HOST_LE_EVENT_MASK_LEGACY,
            },
            Command::ReadBufferSize,
            Command::LeReadBufferSizeV2,
        ]
    );

    transport.events.extend([
        command_complete(
            HCI_SET_EVENT_MASK_COMMAND,
            ReturnParameters::Status { status: 0 },
        ),
        command_complete(
            HCI_SET_EVENT_MASK_PAGE_2_COMMAND,
            ReturnParameters::Status { status: 0 },
        ),
        command_complete(
            HCI_LE_SET_EVENT_MASK_COMMAND,
            ReturnParameters::Status { status: 0 },
        ),
        command_complete(
            HCI_READ_BUFFER_SIZE_COMMAND,
            ReturnParameters::ReadBufferSize {
                status: 0,
                hc_acl_data_packet_length: 1021,
                hc_synchronous_data_packet_length: 0,
                hc_total_num_acl_data_packets: 10,
                hc_total_num_synchronous_data_packets: 0,
            },
        ),
        command_complete(
            HCI_LE_READ_BUFFER_SIZE_V2_COMMAND,
            ReturnParameters::LeReadBufferSizeV2 {
                status: 0,
                le_acl_data_packet_length: 251,
                total_num_le_acl_data_packets: 2,
                iso_data_packet_length: 120,
                total_num_iso_data_packets: 2,
            },
        ),
    ]);
    assert!(device.poll(&mut transport));

    assert!(device.host_initialization_complete());
    assert!(device.host_initialization_succeeded());
    assert_eq!(device.event_mask_status(), Some(0));
    assert_eq!(device.event_mask_page_2_status(), Some(0));
    assert_eq!(device.le_event_mask_status(), Some(0));
    assert_eq!(device.classic_buffer_status(), Some(0));
    assert_eq!(
        device.classic_acl_buffer(),
        Some(ControllerBufferInfo {
            data_packet_length: 1021,
            total_num_data_packets: 10,
        })
    );
    assert_eq!(device.le_buffer_status(), Some(0));
    assert_eq!(
        device.le_acl_buffer(),
        Some(ControllerBufferInfo {
            data_packet_length: 251,
            total_num_data_packets: 2,
        })
    );
    assert_eq!(
        device.iso_buffer(),
        Some(ControllerBufferInfo {
            data_packet_length: 120,
            total_num_data_packets: 2,
        })
    );
    assert_eq!(device.acl_data_packet_length(), 1021);
    assert_eq!(device.acl_max_in_flight(), 10);
    assert_eq!(device.le_acl_data_packet_length(), 251);
    assert_eq!(device.le_acl_max_in_flight(), 2);
    assert_eq!(device.iso_data_packet_length(), Some(120));
    assert_eq!(device.iso_max_in_flight(), Some(2));

    let connection_handle = 0x0040;
    transport.events.push(HciPacket::Event(Event::LeMeta(
        LeMetaEvent::ConnectionComplete {
            status: 0,
            connection_handle,
            role: 0,
            peer_address_type: 1,
            peer_address: Address::parse("C0:00:00:00:00:40", AddressType::RANDOM_DEVICE).unwrap(),
            connection_interval: 24,
            peripheral_latency: 0,
            supervision_timeout: 72,
            central_clock_accuracy: 0,
        },
    )));
    assert!(device.poll(&mut transport));
    assert!(device.send_l2cap_on_handle(&mut transport, connection_handle, 0x0040, &[0xAB; 600],));
    assert_eq!(transport.acl_packets.len(), 2);
    assert!(transport
        .acl_packets
        .iter()
        .all(|packet| usize::from(packet.data_total_length) <= 251));
    assert_eq!(device.acl_packets_pending(), 3);

    transport
        .events
        .push(HciPacket::Event(Event::NumberOfCompletedPackets {
            connection_handles: vec![connection_handle],
            num_completed_packets: vec![2],
        }));
    assert!(device.poll(&mut transport));
    assert_eq!(transport.acl_packets.len(), 3);
    assert_eq!(device.acl_packets_pending(), 1);
    transport
        .events
        .push(HciPacket::Event(Event::NumberOfCompletedPackets {
            connection_handles: vec![connection_handle],
            num_completed_packets: vec![1],
        }));
    assert!(device.poll(&mut transport));
    assert_eq!(device.acl_packets_pending(), 0);

    let cis_handle = 0x0050;
    transport.events.push(HciPacket::Event(Event::LeMeta(
        LeMetaEvent::CisEstablished {
            status: 0,
            connection_handle: cis_handle,
            cig_sync_delay: 1,
            cis_sync_delay: 2,
            transport_latency_c_to_p: 3,
            transport_latency_p_to_c: 4,
            phy_c_to_p: 1,
            phy_p_to_c: 2,
            nse: 3,
            bn_c_to_p: 4,
            bn_p_to_c: 5,
            ft_c_to_p: 6,
            ft_p_to_c: 7,
            max_pdu_c_to_p: 120,
            max_pdu_p_to_c: 121,
            iso_interval: 8,
        },
    )));
    assert!(device.poll(&mut transport));
    assert!(device.send_iso_sdu(&mut transport, cis_handle, &[0xCD; 300]));
    assert_eq!(transport.iso_packets.len(), 2);
    assert!(transport
        .iso_packets
        .iter()
        .all(|packet| usize::from(packet.data_total_length) <= 120));
    assert_eq!(device.iso_packets_pending(), Some(3));
    assert!(!device.iso_output_is_drained(cis_handle));

    transport
        .events
        .push(HciPacket::Event(Event::NumberOfCompletedPackets {
            connection_handles: vec![cis_handle],
            num_completed_packets: vec![2],
        }));
    assert!(device.poll(&mut transport));
    assert_eq!(transport.iso_packets.len(), 3);
    assert_eq!(device.iso_packets_pending(), Some(1));
    transport
        .events
        .push(HciPacket::Event(Event::NumberOfCompletedPackets {
            connection_handles: vec![cis_handle],
            num_completed_packets: vec![1],
        }));
    assert!(device.poll(&mut transport));
    assert_eq!(device.iso_packets_pending(), Some(0));
    assert!(device.iso_output_is_drained(cis_handle));
}

#[test]
fn zero_sized_legacy_le_pool_uses_the_classic_acl_queue() {
    let mut device = Device::new(0);
    let mut transport = ScriptedTransport::default();
    let mut supported_commands = [0; 64];
    supported_commands[14] = 1 << 7;
    supported_commands[25] = 1 << 1;
    transport.events.push(command_complete(
        HCI_READ_LOCAL_SUPPORTED_COMMANDS_COMMAND,
        ReturnParameters::ReadLocalSupportedCommands {
            status: 0,
            supported_commands,
        },
    ));
    assert!(device.poll(&mut transport));
    assert_eq!(
        transport.commands,
        vec![
            Command::SetEventMask {
                event_mask: HOST_EVENT_MASK,
            },
            Command::LeSetEventMask {
                le_event_mask: HOST_LE_EVENT_MASK,
            },
            Command::ReadBufferSize,
            Command::LeReadBufferSize,
        ]
    );

    complete_host_masks(&mut transport);
    transport.events.extend([
        command_complete(
            HCI_READ_BUFFER_SIZE_COMMAND,
            ReturnParameters::ReadBufferSize {
                status: 0,
                hc_acl_data_packet_length: 512,
                hc_synchronous_data_packet_length: 0,
                hc_total_num_acl_data_packets: 7,
                hc_total_num_synchronous_data_packets: 0,
            },
        ),
        command_complete(
            bumble_hci::HCI_LE_READ_BUFFER_SIZE_COMMAND,
            ReturnParameters::LeReadBufferSize {
                status: 0,
                le_acl_data_packet_length: 0,
                total_num_le_acl_data_packets: 0,
            },
        ),
    ]);
    assert!(device.poll(&mut transport));

    assert!(device.host_initialization_succeeded());
    assert_eq!(
        device.le_acl_buffer(),
        Some(ControllerBufferInfo {
            data_packet_length: 0,
            total_num_data_packets: 0,
        })
    );
    assert_eq!(device.acl_data_packet_length(), 512);
    assert_eq!(device.le_acl_data_packet_length(), 512);
    assert_eq!(device.acl_max_in_flight(), 7);
    assert_eq!(device.le_acl_max_in_flight(), 7);
    assert_eq!(device.iso_buffer(), None);
}

#[test]
fn reset_reconciles_suggested_data_length_and_tolerates_advertising_query_failure() {
    let mut device = Device::new(0);
    let mut transport = ScriptedTransport::default();
    let mut supported_commands = [0; 64];
    supported_commands[33] = 0x80;
    supported_commands[34] = 0x01;
    supported_commands[36] = 0xC0;
    transport.events.push(command_complete(
        HCI_READ_LOCAL_SUPPORTED_COMMANDS_COMMAND,
        ReturnParameters::ReadLocalSupportedCommands {
            status: 0,
            supported_commands,
        },
    ));
    assert!(device.poll(&mut transport));
    assert_eq!(
        transport.commands,
        vec![
            Command::SetEventMask {
                event_mask: HOST_EVENT_MASK,
            },
            Command::LeSetEventMask {
                le_event_mask: HOST_LE_EVENT_MASK,
            },
            Command::LeReadSuggestedDefaultDataLength,
            Command::LeReadNumberOfSupportedAdvertisingSets,
            Command::LeReadMaximumAdvertisingDataLength,
        ]
    );

    complete_host_masks(&mut transport);
    transport.events.extend([
        command_complete(
            HCI_LE_READ_SUGGESTED_DEFAULT_DATA_LENGTH_COMMAND,
            ReturnParameters::LeReadSuggestedDefaultDataLength {
                status: 0,
                suggested_max_tx_octets: 27,
                suggested_max_tx_time: 0x0148,
            },
        ),
        command_complete(
            HCI_LE_READ_NUMBER_OF_SUPPORTED_ADVERTISING_SETS_COMMAND,
            ReturnParameters::Status { status: 0x0C },
        ),
        command_complete(
            HCI_LE_READ_MAXIMUM_ADVERTISING_DATA_LENGTH_COMMAND,
            ReturnParameters::LeReadMaximumAdvertisingDataLength {
                status: 0,
                max_advertising_data_length: 1_650,
            },
        ),
    ]);
    assert!(device.poll(&mut transport));
    assert_eq!(
        device.suggested_default_data_length(),
        Some(LeSuggestedDefaultDataLength {
            suggested_max_tx_octets: 27,
            suggested_max_tx_time: 0x0148,
        })
    );
    assert_eq!(
        transport.commands.last(),
        Some(&Command::LeWriteSuggestedDefaultDataLength {
            suggested_max_tx_octets: HOST_SUGGESTED_MAX_TX_OCTETS,
            suggested_max_tx_time: HOST_SUGGESTED_MAX_TX_TIME,
        })
    );
    assert!(!device.host_initialization_complete());

    transport.events.push(command_complete(
        HCI_LE_WRITE_SUGGESTED_DEFAULT_DATA_LENGTH_COMMAND,
        ReturnParameters::Status { status: 0 },
    ));
    assert!(device.poll(&mut transport));
    assert!(device.host_initialization_complete());
    assert!(device.host_initialization_succeeded());
    assert_eq!(device.suggested_default_data_length_read_status(), Some(0));
    assert_eq!(device.suggested_default_data_length_write_status(), Some(0));
    assert_eq!(
        device.suggested_default_data_length(),
        Some(LeSuggestedDefaultDataLength {
            suggested_max_tx_octets: HOST_SUGGESTED_MAX_TX_OCTETS,
            suggested_max_tx_time: HOST_SUGGESTED_MAX_TX_TIME,
        })
    );
    assert_eq!(
        device.number_of_supported_advertising_sets_status(),
        Some(0x0C)
    );
    assert_eq!(device.number_of_supported_advertising_sets(), 0);
    assert_eq!(device.maximum_advertising_data_length_status(), Some(0));
    assert_eq!(device.maximum_advertising_data_length(), 1_650);
}

#[test]
fn failed_suggested_data_length_read_fails_host_initialization_without_a_write() {
    let mut device = Device::new(0);
    let mut transport = ScriptedTransport::default();
    let mut supported_commands = [0; 64];
    supported_commands[33] = 0x80;
    supported_commands[34] = 0x01;
    transport.events.push(command_complete(
        HCI_READ_LOCAL_SUPPORTED_COMMANDS_COMMAND,
        ReturnParameters::ReadLocalSupportedCommands {
            status: 0,
            supported_commands,
        },
    ));
    assert!(device.poll(&mut transport));
    complete_host_masks(&mut transport);
    transport.events.push(command_complete(
        HCI_LE_READ_SUGGESTED_DEFAULT_DATA_LENGTH_COMMAND,
        ReturnParameters::Status { status: 0x01 },
    ));
    assert!(device.poll(&mut transport));

    assert!(device.host_initialization_complete());
    assert!(!device.host_initialization_succeeded());
    assert_eq!(
        device.suggested_default_data_length_read_status(),
        Some(0x01)
    );
    assert_eq!(device.suggested_default_data_length(), None);
    assert_eq!(device.suggested_default_data_length_write_status(), None);
    assert!(!transport
        .commands
        .iter()
        .any(|command| matches!(command, Command::LeWriteSuggestedDefaultDataLength { .. })));
}

#[test]
fn reset_flushes_ready_state_and_restarts_capability_discovery() {
    let mut device = Device::new(0);
    let mut transport = ScriptedTransport::default();
    device.power_on(&mut transport).unwrap();
    device.take_device_events();
    transport.commands.clear();

    device.reset(&mut transport);

    assert!(device.is_powered_on());
    assert_eq!(device.take_device_events(), vec![DeviceEvent::Flush]);
    assert_eq!(
        transport.commands,
        vec![Command::Reset, Command::ReadLocalSupportedCommands]
    );
    assert_eq!(device.local_supported_commands(), None);
    assert_eq!(device.local_version(), None);
    assert_eq!(device.local_lmp_features_max_page(), None);
    assert_eq!(device.local_le_features(), None);
    assert!(!device.host_initialization_complete());
    assert_eq!(device.event_mask_status(), None);
    assert_eq!(device.classic_acl_buffer(), None);
    assert_eq!(device.le_acl_buffer(), None);
    assert_eq!(device.iso_buffer(), None);
    assert_eq!(device.suggested_default_data_length_read_status(), None);
    assert_eq!(device.suggested_default_data_length(), None);
    assert_eq!(device.suggested_default_data_length_write_status(), None);
    assert_eq!(device.number_of_supported_advertising_sets_status(), None);
    assert_eq!(device.number_of_supported_advertising_sets(), 0);
    assert_eq!(device.maximum_advertising_data_length_status(), None);
    assert_eq!(
        device.maximum_advertising_data_length(),
        HOST_DEFAULT_MAXIMUM_ADVERTISING_DATA_LENGTH
    );
}

#[test]
fn unsupported_le_feature_queries_leave_capabilities_unknown() {
    let mut device = Device::new(0);
    let mut transport = ScriptedTransport {
        events: vec![command_complete(
            HCI_READ_LOCAL_SUPPORTED_COMMANDS_COMMAND,
            ReturnParameters::ReadLocalSupportedCommands {
                status: 0,
                supported_commands: [0; 64],
            },
        )],
        ..ScriptedTransport::default()
    };

    assert!(device.poll(&mut transport));
    assert_eq!(transport.commands, host_mask_commands(HOST_LE_EVENT_MASK));
    assert_eq!(device.local_supported_commands_status(), Some(0));
    assert_eq!(device.local_le_features_status(), None);
    assert_eq!(device.local_le_features(), None);
    assert!(!device.supports_le_extended_advertising());
    assert!(!device.supports_le_periodic_advertising());
    assert_eq!(device.supports_le_phy(LE_1M_PHY), Ok(true));
    assert_eq!(device.supports_le_phy(LE_2M_PHY), Ok(false));
}

#[test]
fn failed_all_page_feature_query_leaves_capabilities_unknown() {
    let mut supported_commands = [0; 64];
    supported_commands[47] = 1 << 2;
    let mut device = Device::new(0);
    let mut transport = ScriptedTransport {
        events: vec![command_complete(
            HCI_READ_LOCAL_SUPPORTED_COMMANDS_COMMAND,
            ReturnParameters::ReadLocalSupportedCommands {
                status: 0,
                supported_commands,
            },
        )],
        ..ScriptedTransport::default()
    };

    assert!(device.poll(&mut transport));
    assert_eq!(
        transport.commands,
        vec![Command::LeReadAllLocalSupportedFeatures]
    );
    transport.commands.clear();
    transport.events.push(command_complete(
        HCI_LE_READ_ALL_LOCAL_SUPPORTED_FEATURES_COMMAND,
        ReturnParameters::Status { status: 0x01 },
    ));
    assert!(device.poll(&mut transport));
    assert_eq!(transport.commands, host_mask_commands(HOST_LE_EVENT_MASK));

    assert_eq!(device.local_le_features_status(), Some(0x01));
    assert_eq!(device.local_le_features(), None);
    assert_eq!(device.local_le_features_max_page(), None);
    assert!(!device.supports_le_extended_advertising());
}
