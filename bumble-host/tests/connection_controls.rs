use bumble::{Address, AddressType};
use bumble_controller::{Controller, LocalLink};
use bumble_hci::{AclDataPacket, Command, Event, HciPacket, IsoDataPacket, LeMetaEvent};
use bumble_host::{
    pump, Device, HostTransport, LeConnectionControlEvent, LeConnectionParameters,
    LeConnectionRateParameters, LeConnectionUpdateParameters, LeDataLength, LePhy,
    LeSubrateRequestParameters,
};

fn random_address(value: &str) -> Address {
    Address::parse(value, AddressType::RANDOM_DEVICE).unwrap()
}

#[test]
fn device_drives_common_le_connection_controls_end_to_end() {
    let central_address = random_address("C0:00:00:00:00:01");
    let peripheral_address = random_address("C0:00:00:00:00:02");
    let mut link = LocalLink::new();
    let central_id = link.add_controller(Controller::new(
        "central",
        random_address("00:00:00:00:00:01"),
    ));
    let peripheral_id = link.add_controller(Controller::new(
        "peripheral",
        random_address("00:00:00:00:00:02"),
    ));
    let mut devices = [Device::new(central_id), Device::new(peripheral_id)];

    devices[0].set_random_address(&mut link, central_address);
    devices[1].set_random_address(&mut link, peripheral_address.clone());
    assert!(devices[1].start_advertising(&mut link, &[]));
    devices[0].connect_le(&mut link, peripheral_address);
    pump(&mut link, &mut devices);
    let handle = devices[0].connection_handle().unwrap();

    assert!(devices[0].set_data_length_on_handle(&mut link, handle, 200, 1_000));
    pump(&mut link, &mut devices);
    assert_eq!(
        devices[0].le_connection(handle).unwrap().data_length,
        Some(LeDataLength {
            max_tx_octets: 200,
            max_tx_time: 1_000,
            max_rx_octets: 200,
            max_rx_time: 1_000,
        })
    );

    assert!(devices[0].set_phy_on_handle(&mut link, handle, Some(0x02), Some(0x04), 0));
    pump(&mut link, &mut devices);
    assert_eq!(
        devices[0].le_connection(handle).unwrap().phy,
        LePhy {
            tx_phy: 2,
            rx_phy: 3,
        }
    );

    assert!(devices[0].read_phy_on_handle(&mut link, handle));
    pump(&mut link, &mut devices);
    let connection = devices[0].le_connection(handle).unwrap();
    assert_eq!(
        connection.phy,
        LePhy {
            tx_phy: 2,
            rx_phy: 3,
        }
    );
    assert_eq!(connection.rssi, None);

    let events = devices[0].take_connection_control_events();
    assert_eq!(
        events,
        vec![
            LeConnectionControlEvent::DataLengthRequestComplete {
                status: 0,
                connection_handle: handle,
            },
            LeConnectionControlEvent::DataLengthChange {
                connection_handle: handle,
                data_length: LeDataLength {
                    max_tx_octets: 200,
                    max_tx_time: 1_000,
                    max_rx_octets: 200,
                    max_rx_time: 1_000,
                },
            },
            LeConnectionControlEvent::PhyUpdate {
                status: 0,
                connection_handle: handle,
                phy: LePhy {
                    tx_phy: 2,
                    rx_phy: 3,
                },
            },
            LeConnectionControlEvent::PhyRead {
                status: 0,
                connection_handle: handle,
                phy: LePhy {
                    tx_phy: 2,
                    rx_phy: 3,
                },
            },
        ]
    );

    let update = LeConnectionUpdateParameters {
        connection_interval_min: 18,
        connection_interval_max: 24,
        max_latency: 3,
        supervision_timeout: 200,
        min_ce_length: 0,
        max_ce_length: 0,
    };
    let rate = LeConnectionRateParameters {
        connection_interval_min: 60,
        connection_interval_max: 80,
        subrate_min: 2,
        subrate_max: 4,
        max_latency: 5,
        continuation_number: 1,
        supervision_timeout: 300,
        min_ce_length: 0,
        max_ce_length: 0,
    };
    assert!(devices[0].update_connection_parameters_on_handle(&mut link, handle, update));
    assert!(devices[0].update_connection_rate_on_handle(&mut link, handle, rate));
    assert!(devices[0].read_rssi_on_handle(&mut link, handle));
    pump(&mut link, &mut devices);
    assert_eq!(
        devices[0].take_connection_control_events(),
        vec![
            LeConnectionControlEvent::CommandStatus {
                command_opcode: bumble_hci::HCI_LE_CONNECTION_UPDATE_COMMAND,
                status: 0x01,
                connection_handle: Some(handle),
            },
            LeConnectionControlEvent::CommandStatus {
                command_opcode: bumble_hci::HCI_LE_CONNECTION_RATE_REQUEST_COMMAND,
                status: 0x01,
                connection_handle: Some(handle),
            },
            LeConnectionControlEvent::CommandStatus {
                command_opcode: bumble_hci::HCI_READ_RSSI_COMMAND,
                status: 0x01,
                connection_handle: Some(handle),
            },
        ]
    );

    assert!(!devices[0].update_connection_parameters_on_handle(&mut link, 0x0FFF, update));
    assert!(!devices[0].update_connection_rate_on_handle(&mut link, 0x0FFF, rate));
    assert!(!devices[0].set_data_length_on_handle(&mut link, handle, 26, 1_000));
    assert!(!devices[0].set_data_length_on_handle(&mut link, handle, 200, 0x0147));
    assert!(!devices[0].set_phy_on_handle(&mut link, 0x0FFF, None, None, 0));
    assert!(!devices[0].read_phy_on_handle(&mut link, 0x0FFF));
    assert!(!devices[0].read_rssi_on_handle(&mut link, 0x0FFF));
}

#[derive(Default)]
struct ScriptedTransport {
    events: Vec<HciPacket>,
    commands: Vec<Command>,
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

#[test]
fn device_correlates_connection_control_failures_with_the_requested_handle() {
    let handle = 0x0040;
    let mut transport = ScriptedTransport {
        events: vec![HciPacket::Event(Event::LeMeta(
            LeMetaEvent::ConnectionComplete {
                status: 0,
                connection_handle: handle,
                role: 0,
                peer_address_type: 1,
                peer_address: random_address("C0:00:00:00:00:02"),
                connection_interval: 24,
                peripheral_latency: 0,
                supervision_timeout: 72,
                central_clock_accuracy: 0,
            },
        ))],
        commands: Vec::new(),
    };
    let mut device = Device::new(0);
    assert!(device.poll(&mut transport));

    let update = LeConnectionUpdateParameters {
        connection_interval_min: 18,
        connection_interval_max: 24,
        max_latency: 3,
        supervision_timeout: 200,
        min_ce_length: 0,
        max_ce_length: 0,
    };
    let rate = LeConnectionRateParameters {
        connection_interval_min: 60,
        connection_interval_max: 80,
        subrate_min: 2,
        subrate_max: 4,
        max_latency: 5,
        continuation_number: 1,
        supervision_timeout: 300,
        min_ce_length: 0,
        max_ce_length: 0,
    };
    assert!(device.update_connection_parameters_on_handle(&mut transport, handle, update));
    assert!(device.update_connection_rate_on_handle(&mut transport, handle, rate));
    assert!(device.read_rssi_on_handle(&mut transport, handle));
    transport.events.extend([
        HciPacket::Event(Event::CommandStatus {
            status: 0,
            num_hci_command_packets: 1,
            command_opcode: bumble_hci::HCI_LE_CONNECTION_UPDATE_COMMAND,
        }),
        HciPacket::Event(Event::LeMeta(LeMetaEvent::ConnectionUpdateComplete {
            status: 0,
            connection_handle: handle,
            connection_interval: 18,
            peripheral_latency: 3,
            supervision_timeout: 200,
        })),
        HciPacket::Event(Event::CommandStatus {
            status: 0,
            num_hci_command_packets: 1,
            command_opcode: bumble_hci::HCI_LE_CONNECTION_RATE_REQUEST_COMMAND,
        }),
        HciPacket::Event(Event::LeMeta(LeMetaEvent::ConnectionRateChange {
            status: 0,
            connection_handle: handle,
            connection_interval: 60,
            subrate_factor: 2,
            peripheral_latency: 5,
            continuation_number: 1,
            supervision_timeout: 300,
        })),
        HciPacket::Event(Event::CommandComplete {
            num_hci_command_packets: 1,
            command_opcode: bumble_hci::HCI_READ_RSSI_COMMAND,
            return_parameters: bumble_hci::ReturnParameters::ReadRssi {
                status: 0,
                handle,
                rssi: -55,
            },
        }),
    ]);
    assert!(device.poll(&mut transport));
    assert_eq!(
        device.take_connection_control_events(),
        vec![
            LeConnectionControlEvent::ConnectionParametersUpdate {
                status: 0,
                connection_handle: handle,
                parameters: LeConnectionParameters {
                    connection_interval: 18,
                    peripheral_latency: 3,
                    supervision_timeout: 200,
                    subrate_factor: 1,
                    continuation_number: 0,
                },
            },
            LeConnectionControlEvent::ConnectionParametersUpdate {
                status: 0,
                connection_handle: handle,
                parameters: LeConnectionParameters {
                    connection_interval: 60,
                    peripheral_latency: 5,
                    supervision_timeout: 300,
                    subrate_factor: 2,
                    continuation_number: 1,
                },
            },
            LeConnectionControlEvent::RssiRead {
                status: 0,
                connection_handle: handle,
                rssi: -55,
            },
        ]
    );
    let connection = device.le_connection(handle).unwrap();
    assert_eq!(
        connection.parameters,
        LeConnectionParameters {
            connection_interval: 60,
            peripheral_latency: 5,
            supervision_timeout: 300,
            subrate_factor: 2,
            continuation_number: 1,
        }
    );
    assert_eq!(connection.rssi, Some(-55));

    device.set_default_phy(&mut transport, Some(0x02), None);
    device.set_default_connection_rate(&mut transport, rate);
    device.set_default_subrate(
        &mut transport,
        LeSubrateRequestParameters {
            subrate_min: 2,
            subrate_max: 4,
            max_latency: 5,
            continuation_number: 1,
            supervision_timeout: 300,
        },
    );
    let default_commands = &transport.commands[transport.commands.len() - 3..];
    assert!(matches!(
        &default_commands[0],
        Command::LeSetDefaultPhy {
            all_phys: 0x02,
            tx_phys: 0x02,
            rx_phys: 0,
        }
    ));
    assert!(matches!(
        &default_commands[1],
        Command::LeSetDefaultRateParameters {
            connection_interval_min: 60,
            connection_interval_max: 80,
            subrate_min: 2,
            subrate_max: 4,
            ..
        }
    ));
    assert!(matches!(
        &default_commands[2],
        Command::LeSetDefaultSubrate {
            subrate_min: 2,
            subrate_max: 4,
            ..
        }
    ));

    assert!(device.set_phy_on_handle(&mut transport, handle, Some(0x02), None, 0));
    assert!(matches!(
        transport.commands.last(),
        Some(Command::LeSetPhy {
            connection_handle: 0x0040,
            ..
        })
    ));
    transport
        .events
        .push(HciPacket::Event(Event::CommandStatus {
            status: 0x1A,
            num_hci_command_packets: 1,
            command_opcode: bumble_hci::HCI_LE_SET_PHY_COMMAND,
        }));
    assert!(device.poll(&mut transport));
    assert_eq!(
        device.take_connection_control_events(),
        vec![LeConnectionControlEvent::CommandStatus {
            command_opcode: bumble_hci::HCI_LE_SET_PHY_COMMAND,
            status: 0x1A,
            connection_handle: Some(handle),
        }]
    );
    assert_eq!(
        device.le_connection(handle).unwrap().phy,
        LePhy {
            tx_phy: 1,
            rx_phy: 1,
        }
    );

    assert!(device.read_phy_on_handle(&mut transport, handle));
    transport
        .events
        .push(HciPacket::Event(Event::CommandComplete {
            num_hci_command_packets: 1,
            command_opcode: bumble_hci::HCI_LE_READ_PHY_COMMAND,
            return_parameters: bumble_hci::ReturnParameters::Status { status: 0x02 },
        }));
    assert!(device.poll(&mut transport));
    assert_eq!(
        device.take_connection_control_events(),
        vec![LeConnectionControlEvent::CommandStatus {
            command_opcode: bumble_hci::HCI_LE_READ_PHY_COMMAND,
            status: 0x02,
            connection_handle: Some(handle),
        }]
    );
}
