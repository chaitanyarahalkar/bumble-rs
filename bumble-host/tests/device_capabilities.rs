use bumble_hci::{
    AclDataPacket, Command, Event, HciPacket, IsoDataPacket, ReturnParameters,
    HCI_LE_READ_ALL_LOCAL_SUPPORTED_FEATURES_COMMAND, HCI_LE_READ_LOCAL_SUPPORTED_FEATURES_COMMAND,
    HCI_READ_LOCAL_EXTENDED_FEATURES_COMMAND, HCI_READ_LOCAL_SUPPORTED_COMMANDS_COMMAND,
    HCI_READ_LOCAL_SUPPORTED_FEATURES_COMMAND, HCI_READ_LOCAL_VERSION_INFORMATION_COMMAND,
};
use bumble_host::{
    Device, DeviceEvent, HostTransport, LocalVersionInformation, LE_1M_PHY, LE_2M_PHY,
    LE_CODED_PHY, LE_FEATURE_2M_PHY, LE_FEATURE_CODED_PHY, LE_FEATURE_PERIODIC_ADVERTISING,
    LMP_FEATURE_INTERLACED_INQUIRY_SCAN, LMP_FEATURE_INTERLACED_PAGE_SCAN,
};

#[derive(Default)]
struct ScriptedTransport {
    commands: Vec<Command>,
    events: Vec<HciPacket>,
}

impl HostTransport for ScriptedTransport {
    fn handle_command(&mut self, _controller_id: usize, command: Command) {
        self.commands.push(command);
    }

    fn send_acl_packet(&mut self, _controller_id: usize, _packet: AclDataPacket) -> bool {
        false
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

    fn send_iso_packet(&mut self, _controller_id: usize, _packet: IsoDataPacket) -> bool {
        false
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

    assert!(transport.commands.is_empty());
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

    assert!(transport.commands.is_empty());
    assert_eq!(device.local_lmp_feature_page(0), Some(&[0xAA; 8]));
    assert_eq!(device.local_lmp_feature_status(0), Some(0));
    assert_eq!(device.local_lmp_feature_page(1), None);
    assert_eq!(device.local_lmp_feature_status(1), Some(0x01));
    assert_eq!(device.local_lmp_features_max_page(), Some(2));
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
    assert!(transport.commands.is_empty());
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
    transport.events.push(command_complete(
        HCI_LE_READ_ALL_LOCAL_SUPPORTED_FEATURES_COMMAND,
        ReturnParameters::Status { status: 0x01 },
    ));
    assert!(device.poll(&mut transport));

    assert_eq!(device.local_le_features_status(), Some(0x01));
    assert_eq!(device.local_le_features(), None);
    assert_eq!(device.local_le_features_max_page(), None);
    assert!(!device.supports_le_extended_advertising());
}
