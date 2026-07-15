use bumble_hci::{
    AclDataPacket, Command, Event, HciPacket, IsoDataPacket, ReturnParameters,
    HCI_LE_READ_ALL_LOCAL_SUPPORTED_FEATURES_COMMAND, HCI_LE_READ_LOCAL_SUPPORTED_FEATURES_COMMAND,
    HCI_READ_LOCAL_SUPPORTED_COMMANDS_COMMAND,
};
use bumble_host::{
    Device, HostTransport, LE_1M_PHY, LE_2M_PHY, LE_CODED_PHY, LE_FEATURE_2M_PHY,
    LE_FEATURE_CODED_PHY, LE_FEATURE_PERIODIC_ADVERTISING,
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
