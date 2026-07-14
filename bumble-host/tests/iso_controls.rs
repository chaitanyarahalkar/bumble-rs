use bumble_hci::{
    AclDataPacket, CodingFormat, Command, Event, HciPacket, IsoDataPacket, LeMetaEvent,
    ReturnParameters,
};
use bumble_host::{Device, HostTransport, IsoControlEvent, IsoDataPathParameters, IsoTxSyncInfo};

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

fn establish_cis(device: &mut Device, transport: &mut ScriptedTransport, handle: u16) {
    transport.events.push(HciPacket::Event(Event::LeMeta(
        LeMetaEvent::CisEstablished {
            status: 0,
            connection_handle: handle,
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
    assert!(device.poll(transport));
    let _ = device.take_cis_control_events();
}

fn command_complete(command_opcode: u16, return_parameters: ReturnParameters) -> HciPacket {
    HciPacket::Event(Event::CommandComplete {
        num_hci_command_packets: 1,
        command_opcode,
        return_parameters,
    })
}

#[test]
fn custom_data_path_and_tx_sync_round_trip_through_device_state() {
    let handle = 0x0040;
    let mut device = Device::new(0);
    let mut transport = ScriptedTransport::default();
    establish_cis(&mut device, &mut transport, handle);

    let parameters = IsoDataPathParameters {
        direction: 0,
        data_path_id: 0x5A,
        codec_id: CodingFormat {
            coding_format: 0x06,
            company_id: 0x1234,
            vendor_specific_codec_id: 0x5678,
        },
        controller_delay: 0x00A1_B2C3,
        codec_configuration: vec![1, 2, 3, 4],
    };
    assert!(device.setup_iso_data_path_with_parameters(&mut transport, handle, parameters.clone(),));
    assert_eq!(
        transport.commands.pop(),
        Some(Command::LeSetupIsoDataPath {
            connection_handle: handle,
            data_path_direction: 0,
            data_path_id: 0x5A,
            codec_id: parameters.codec_id,
            controller_delay: 0x00A1_B2C3,
            codec_configuration: vec![1, 2, 3, 4],
        })
    );

    transport.events.push(command_complete(
        bumble_hci::HCI_LE_SETUP_ISO_DATA_PATH_COMMAND,
        ReturnParameters::StatusAndConnectionHandle {
            status: 0,
            connection_handle: handle,
        },
    ));
    assert!(device.poll(&mut transport));
    assert_eq!(device.iso_data_path(handle, 0), Some(&parameters));
    assert_eq!(
        device.take_iso_control_events(),
        vec![IsoControlEvent::DataPathSetup {
            status: 0,
            connection_handle: handle,
            parameters: parameters.clone(),
        }]
    );

    // Repeating setup is idempotent once the host has recorded the path.
    assert!(device.setup_iso_data_path_with_parameters(&mut transport, handle, parameters.clone(),));
    assert!(transport.commands.is_empty());

    assert!(device.read_iso_tx_sync(&mut transport, handle));
    assert_eq!(
        transport.commands.pop(),
        Some(Command::LeReadIsoTxSync {
            connection_handle: handle,
        })
    );
    let sync = IsoTxSyncInfo {
        connection_handle: handle,
        packet_sequence_number: 0x1234,
        tx_time_stamp: 0x89AB_CDEF,
        time_offset: 0x0012_3456,
    };
    transport.events.push(command_complete(
        bumble_hci::HCI_LE_READ_ISO_TX_SYNC_COMMAND,
        ReturnParameters::LeReadIsoTxSync {
            status: 0,
            connection_handle: handle,
            packet_sequence_number: sync.packet_sequence_number,
            tx_time_stamp: sync.tx_time_stamp,
            time_offset: sync.time_offset,
        },
    ));
    assert!(device.poll(&mut transport));
    assert_eq!(device.iso_tx_sync(handle), Some(&sync));
    assert_eq!(
        device.take_iso_control_events(),
        vec![IsoControlEvent::TxSync {
            status: 0,
            connection_handle: handle,
            sync: Some(sync),
        }]
    );

    assert!(device.remove_iso_data_path(&mut transport, handle, 0x01));
    assert_eq!(
        transport.commands.pop(),
        Some(Command::LeRemoveIsoDataPath {
            connection_handle: handle,
            data_path_direction: 0x01,
        })
    );
    transport.events.push(command_complete(
        bumble_hci::HCI_LE_REMOVE_ISO_DATA_PATH_COMMAND,
        ReturnParameters::StatusAndConnectionHandle {
            status: 0,
            connection_handle: handle,
        },
    ));
    assert!(device.poll(&mut transport));
    assert!(device.iso_data_path(handle, 0).is_none());
    assert_eq!(
        device.take_iso_control_events(),
        vec![IsoControlEvent::DataPathRemoved {
            status: 0,
            connection_handle: handle,
            directions: 0x01,
        }]
    );
}

#[test]
fn iso_control_validation_and_status_only_failures_are_preserved() {
    let handle = 0x0040;
    let mut device = Device::new(0);
    let mut transport = ScriptedTransport::default();
    establish_cis(&mut device, &mut transport, handle);

    let mut invalid = IsoDataPathParameters::hci(2);
    assert!(!device.setup_iso_data_path_with_parameters(&mut transport, handle, invalid.clone(),));
    invalid.direction = 0;
    invalid.controller_delay = 0x0100_0000;
    assert!(!device.setup_iso_data_path_with_parameters(&mut transport, handle, invalid.clone(),));
    invalid.controller_delay = 0;
    invalid.codec_configuration = vec![0; 256];
    assert!(!device.setup_iso_data_path_with_parameters(&mut transport, handle, invalid,));
    assert!(!device.remove_iso_data_path(&mut transport, handle, 0));
    assert!(!device.read_iso_tx_sync(&mut transport, 0x0FFF));
    assert!(transport.commands.is_empty());

    let parameters = IsoDataPathParameters::hci(0);
    assert!(device.setup_iso_data_path_with_parameters(&mut transport, handle, parameters.clone(),));
    transport.commands.clear();
    transport.events.push(command_complete(
        bumble_hci::HCI_LE_SETUP_ISO_DATA_PATH_COMMAND,
        ReturnParameters::Status { status: 0x0C },
    ));
    assert!(device.poll(&mut transport));
    assert!(device.iso_data_path(handle, 0).is_none());
    assert_eq!(
        device.take_iso_control_events(),
        vec![IsoControlEvent::DataPathSetup {
            status: 0x0C,
            connection_handle: handle,
            parameters,
        }]
    );

    assert!(device.read_iso_tx_sync(&mut transport, handle));
    transport.commands.clear();
    transport.events.push(command_complete(
        bumble_hci::HCI_LE_READ_ISO_TX_SYNC_COMMAND,
        ReturnParameters::Status { status: 0x02 },
    ));
    assert!(device.poll(&mut transport));
    assert_eq!(
        device.take_iso_control_events(),
        vec![IsoControlEvent::TxSync {
            status: 0x02,
            connection_handle: handle,
            sync: None,
        }]
    );
}
