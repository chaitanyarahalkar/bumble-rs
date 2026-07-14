use bumble::{Address, AddressType};
use bumble_hci::{AclDataPacket, Command, Event, HciPacket, IsoDataPacket, LeMetaEvent};
use bumble_host::{
    ChannelSoundingCreateConfigParameters, ChannelSoundingDefaultSettings,
    ChannelSoundingOperation, ChannelSoundingProcedureParameters, Device, HostTransport,
    DEFAULT_CHANNEL_SOUNDING_CHANNEL_MAP,
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

fn address(value: &str) -> Address {
    Address::parse(value, AddressType::RANDOM_DEVICE).unwrap()
}

fn connection_event(connection_handle: u16) -> HciPacket {
    HciPacket::Event(Event::LeMeta(LeMetaEvent::ConnectionComplete {
        status: 0,
        connection_handle,
        role: 0,
        peer_address_type: 1,
        peer_address: address("C0:FF:EE:00:00:01"),
        connection_interval: 24,
        peripheral_latency: 0,
        supervision_timeout: 72,
        central_clock_accuracy: 0,
    }))
}

fn config_event(status: u8, connection_handle: u16, config_id: u8, action: u8) -> HciPacket {
    HciPacket::Event(Event::LeMeta(LeMetaEvent::CsConfigComplete {
        status,
        connection_handle,
        config_id,
        action,
        main_mode_type: 2,
        sub_mode_type: 0xFF,
        min_main_mode_steps: 2,
        max_main_mode_steps: 5,
        main_mode_repetition: 0,
        mode_0_steps: 3,
        role: 0,
        rtt_type: 0,
        cs_sync_phy: 1,
        channel_map: DEFAULT_CHANNEL_SOUNDING_CHANNEL_MAP,
        channel_map_repetition: 1,
        channel_selection_type: 0,
        ch3c_shape: 0,
        ch3c_jump: 3,
        reserved: 0,
        t_ip1_time: 10,
        t_ip2_time: 20,
        t_fcs_time: 30,
        t_pm_time: 40,
    }))
}

fn connected_device() -> (Device, ScriptedTransport) {
    let mut device = Device::new(0);
    let mut transport = ScriptedTransport {
        commands: Vec::new(),
        events: vec![connection_event(0x0040)],
    };
    assert!(device.poll(&mut transport));
    (device, transport)
}

#[test]
fn upstream_default_channel_map_excludes_forbidden_channels() {
    let forbidden = [0, 1, 23, 24, 25, 76, 77, 78, 79];
    for channel in forbidden {
        assert_eq!(
            DEFAULT_CHANNEL_SOUNDING_CHANNEL_MAP[channel / 8] & (1 << (channel % 8)),
            0,
            "default map enables forbidden CS channel {channel}"
        );
    }
    assert_eq!(
        ChannelSoundingCreateConfigParameters::default().channel_map,
        DEFAULT_CHANNEL_SOUNDING_CHANNEL_MAP
    );
}

#[test]
fn device_orchestrates_channel_sounding_commands_and_state() {
    let (mut device, mut transport) = connected_device();
    let handle = 0x0040;

    assert!(!device.read_remote_channel_sounding_capabilities_on_handle(&mut transport, 0x0BAD));
    assert!(device.read_remote_channel_sounding_capabilities_on_handle(&mut transport, handle));
    assert_eq!(
        transport.commands.pop().unwrap(),
        Command::LeCsReadRemoteSupportedCapabilities {
            connection_handle: handle
        }
    );

    assert!(device.set_default_channel_sounding_settings_on_handle(
        &mut transport,
        handle,
        ChannelSoundingDefaultSettings::default(),
    ));
    assert_eq!(
        transport.commands.pop().unwrap(),
        Command::LeCsSetDefaultSettings {
            connection_handle: handle,
            role_enable: 3,
            cs_sync_antenna_selection: 0xFF,
            max_tx_power: 4,
        }
    );

    let create = ChannelSoundingCreateConfigParameters::default();
    assert_eq!(
        device.create_channel_sounding_config_on_handle(&mut transport, handle, None, create,),
        Some(0)
    );
    assert_eq!(
        transport.commands.pop().unwrap(),
        Command::LeCsCreateConfig {
            connection_handle: handle,
            config_id: 0,
            create_context: 1,
            main_mode_type: 2,
            sub_mode_type: 0xFF,
            min_main_mode_steps: 2,
            max_main_mode_steps: 5,
            main_mode_repetition: 0,
            mode_0_steps: 3,
            role: 0,
            rtt_type: 0,
            cs_sync_phy: 1,
            channel_map: DEFAULT_CHANNEL_SOUNDING_CHANNEL_MAP,
            channel_map_repetition: 1,
            channel_selection_type: 0,
            ch3c_shape: 0,
            ch3c_jump: 3,
            reserved: 0,
        }
    );

    assert!(device.enable_channel_sounding_security_on_handle(&mut transport, handle));
    assert_eq!(
        transport.commands.pop().unwrap(),
        Command::LeCsSecurityEnable {
            connection_handle: handle
        }
    );

    transport.events.extend([
        HciPacket::Event(Event::LeMeta(
            LeMetaEvent::CsReadRemoteSupportedCapabilitiesComplete {
                status: 0,
                connection_handle: handle,
                num_config_supported: 4,
                max_consecutive_procedures_supported: 0x1234,
                num_antennas_supported: 2,
                max_antenna_paths_supported: 4,
                roles_supported: 3,
                modes_supported: 7,
                rtt_capability: 1,
                rtt_aa_only_n: 2,
                rtt_sounding_n: 3,
                rtt_random_sequence_n: 4,
                nadm_sounding_capability: 0x0506,
                nadm_random_capability: 0x0708,
                cs_sync_phys_supported: 3,
                subfeatures_supported: 0x090A,
                t_ip1_times_supported: 0x0B0C,
                t_ip2_times_supported: 0x0D0E,
                t_fcs_times_supported: 0x0F10,
                t_pm_times_supported: 0x1112,
                t_sw_time_supported: 0x13,
                tx_snr_capability: 0x14,
            },
        )),
        config_event(0, handle, 0, 1),
        HciPacket::Event(Event::LeMeta(LeMetaEvent::CsSecurityEnableComplete {
            status: 0,
            connection_handle: handle,
        })),
    ]);
    assert!(device.poll(&mut transport));
    let connection = device.le_connection(handle).unwrap();
    assert_eq!(
        connection
            .channel_sounding_capabilities
            .unwrap()
            .max_consecutive_procedures_supported,
        0x1234
    );
    assert_eq!(connection.channel_sounding_configs[&0].t_pm_time, 40);
    assert_eq!(
        device.take_channel_sounding_security_results(),
        [(handle, 0)]
    );

    let procedure_parameters = ChannelSoundingProcedureParameters::default();
    assert!(device.set_channel_sounding_procedure_parameters_on_handle(
        &mut transport,
        handle,
        0,
        procedure_parameters,
    ));
    assert_eq!(
        transport.commands.pop().unwrap(),
        Command::LeCsSetProcedureParameters {
            connection_handle: handle,
            config_id: 0,
            max_procedure_len: 0x2710,
            min_procedure_interval: 1,
            max_procedure_interval: 0xFF,
            max_procedure_count: 1,
            min_subevent_len: 0x0004E2,
            max_subevent_len: 0x1E8480,
            tone_antenna_config_selection: 0,
            phy: 1,
            tx_power_delta: 0,
            preferred_peer_antenna: 0,
            snr_control_initiator: 0xFF,
            snr_control_reflector: 0xFF,
        }
    );

    assert!(device.enable_channel_sounding_procedure_on_handle(&mut transport, handle, 0, true,));
    assert_eq!(
        transport.commands.pop().unwrap(),
        Command::LeCsProcedureEnable {
            connection_handle: handle,
            config_id: 0,
            enable: 1,
        }
    );
    transport.events.push(HciPacket::Event(Event::LeMeta(
        LeMetaEvent::CsProcedureEnableComplete {
            status: 0,
            connection_handle: handle,
            config_id: 0,
            state: 1,
            tone_antenna_config_selection: 2,
            selected_tx_power: -3,
            subevent_len: 0x010203,
            subevents_per_event: 4,
            subevent_interval: 5,
            event_interval: 6,
            procedure_interval: 7,
            procedure_count: 8,
            max_procedure_len: 9,
        },
    )));
    assert!(device.poll(&mut transport));
    assert_eq!(
        device
            .le_connection(handle)
            .unwrap()
            .channel_sounding_procedures[&0]
            .selected_tx_power,
        -3
    );

    assert!(device.remove_channel_sounding_config_on_handle(&mut transport, handle, 0));
    assert_eq!(
        transport.commands.pop().unwrap(),
        Command::LeCsRemoveConfig {
            connection_handle: handle,
            config_id: 0,
        }
    );
    transport.events.push(config_event(0, handle, 0, 0));
    assert!(device.poll(&mut transport));
    assert!(device
        .le_connection(handle)
        .unwrap()
        .channel_sounding_configs
        .is_empty());
    assert!(device.take_channel_sounding_errors().is_empty());
}

#[test]
fn channel_sounding_allocates_four_configs_and_reports_failures() {
    let (mut device, mut transport) = connected_device();
    let handle = 0x0040;

    for config_id in 0..4 {
        assert_eq!(
            device.create_channel_sounding_config_on_handle(
                &mut transport,
                handle,
                None,
                ChannelSoundingCreateConfigParameters::default(),
            ),
            Some(config_id)
        );
    }
    let command_count = transport.commands.len();
    assert_eq!(
        device.create_channel_sounding_config_on_handle(
            &mut transport,
            handle,
            None,
            ChannelSoundingCreateConfigParameters::default(),
        ),
        None
    );
    assert_eq!(transport.commands.len(), command_count);
    for config_id in 0..4 {
        transport.events.push(config_event(0, handle, config_id, 1));
    }
    assert!(device.poll(&mut transport));

    transport.events.extend([
        HciPacket::Event(Event::LeMeta(
            LeMetaEvent::CsReadRemoteSupportedCapabilitiesComplete {
                status: 0x11,
                connection_handle: handle,
                num_config_supported: 0,
                max_consecutive_procedures_supported: 0,
                num_antennas_supported: 0,
                max_antenna_paths_supported: 0,
                roles_supported: 0,
                modes_supported: 0,
                rtt_capability: 0,
                rtt_aa_only_n: 0,
                rtt_sounding_n: 0,
                rtt_random_sequence_n: 0,
                nadm_sounding_capability: 0,
                nadm_random_capability: 0,
                cs_sync_phys_supported: 0,
                subfeatures_supported: 0,
                t_ip1_times_supported: 0,
                t_ip2_times_supported: 0,
                t_fcs_times_supported: 0,
                t_pm_times_supported: 0,
                t_sw_time_supported: 0,
                tx_snr_capability: 0,
            },
        )),
        HciPacket::Event(Event::LeMeta(LeMetaEvent::CsSecurityEnableComplete {
            status: 0x12,
            connection_handle: handle,
        })),
        config_event(0x13, handle, 2, 1),
        HciPacket::Event(Event::LeMeta(LeMetaEvent::CsProcedureEnableComplete {
            status: 0x14,
            connection_handle: handle,
            config_id: 3,
            state: 0,
            tone_antenna_config_selection: 0,
            selected_tx_power: 0,
            subevent_len: 0,
            subevents_per_event: 0,
            subevent_interval: 0,
            event_interval: 0,
            procedure_interval: 0,
            procedure_count: 0,
            max_procedure_len: 0,
        })),
    ]);
    assert!(device.poll(&mut transport));

    let errors = device.take_channel_sounding_errors();
    assert_eq!(errors.len(), 4);
    assert_eq!(
        errors[0].operation,
        ChannelSoundingOperation::ReadRemoteCapabilities
    );
    assert_eq!(errors[0].status, 0x11);
    assert_eq!(
        errors[1].operation,
        ChannelSoundingOperation::SecurityEnable
    );
    assert_eq!(errors[2].operation, ChannelSoundingOperation::Config);
    assert_eq!(errors[2].config_id, Some(2));
    assert_eq!(
        errors[3].operation,
        ChannelSoundingOperation::ProcedureEnable
    );
    assert_eq!(
        device.take_channel_sounding_security_results(),
        [(handle, 0x12)]
    );
}
