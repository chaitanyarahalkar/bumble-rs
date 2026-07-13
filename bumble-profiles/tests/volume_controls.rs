use bumble::Uuid;
use bumble_att::AttPdu;
use bumble_gatt::{AccessContext, AttTransport, GattClient, GattError, GattServer};
use bumble_profiles::aics::{
    error_code as aics_error, AudioInputControlPointOpcode, AudioInputControlService,
    AudioInputControlServiceProxy, AudioInputState, AudioInputStatus, GainMode,
    GainSettingsProperties, Mute,
};
use bumble_profiles::vcs::{
    error_code as vcs_error, VolumeControlPointOpcode, VolumeControlService,
    VolumeControlServiceProxy, VolumeFlags, VolumeState,
};
use bumble_profiles::vocs::{
    error_code as vocs_error, AudioLocation, VolumeOffsetControlService,
    VolumeOffsetControlServiceProxy, VolumeOffsetState, AUDIO_LOCATION_CHARACTERISTIC,
    AUDIO_OUTPUT_DESCRIPTION_CHARACTERISTIC, VOLUME_OFFSET_CONTROL_POINT_CHARACTERISTIC,
};
use bumble_profiles::{Error, Result};

#[test]
fn volume_state_models_are_byte_exact_and_checked() {
    let volume = VolumeState {
        volume_setting: 32,
        mute: 1,
        change_counter: 7,
    };
    assert_eq!(volume.encode(), [32, 1, 7]);
    assert_eq!(VolumeState::decode(&volume.encode()).unwrap(), volume);
    assert!(VolumeState::decode(&[1, 2]).is_err());

    let offset = VolumeOffsetState {
        volume_offset: -255,
        change_counter: 9,
    };
    assert_eq!(offset.encode(), [1, 0xFF, 9]);
    assert_eq!(VolumeOffsetState::decode(&offset.encode()).unwrap(), offset);

    let input = AudioInputState {
        gain_settings: 120,
        mute: Mute::MUTED,
        gain_mode: GainMode::AUTOMATIC,
        change_counter: 4,
    };
    assert_eq!(input.encode(), [120, 1, 3, 4]);
    assert_eq!(AudioInputState::decode(&input.encode()).unwrap(), input);
    assert_eq!(
        GainSettingsProperties::decode(&[1, 2, 200]).unwrap(),
        GainSettingsProperties {
            gain_settings_unit: 1,
            gain_settings_minimum: 2,
            gain_settings_maximum: 200,
        }
    );
    assert_eq!(
        (AudioLocation::FRONT_LEFT | AudioLocation::FRONT_RIGHT).channel_count(),
        2
    );
}

#[test]
fn vcs_live_controls_apply_upstream_counter_and_change_rules() {
    let service = VolumeControlService::new()
        .step_size(16)
        .initial_state(VolumeState {
            volume_setting: 32,
            mute: 1,
            change_counter: 0,
        })
        .volume_flags(VolumeFlags::VOLUME_SETTING_PERSISTED);
    let mut server = GattServer::from_definitions(vec![service.definition()]).unwrap();
    service.bind(&mut server).unwrap();
    let mut transport = EncryptedTransport(&mut server);
    let mut client = GattClient::new();
    let proxy = VolumeControlServiceProxy::discover(&mut client, &mut transport)
        .unwrap()
        .unwrap();

    assert_eq!(
        proxy
            .read_volume_state(&mut client, &mut transport)
            .unwrap(),
        VolumeState {
            volume_setting: 32,
            mute: 1,
            change_counter: 0,
        }
    );
    assert_eq!(
        proxy
            .read_volume_flags(&mut client, &mut transport)
            .unwrap(),
        VolumeFlags::VOLUME_SETTING_PERSISTED
    );
    proxy
        .write_control_point(
            &mut client,
            &mut transport,
            VolumeControlPointOpcode::RELATIVE_VOLUME_DOWN,
            0,
            None,
        )
        .unwrap();
    assert_eq!(service.state().unwrap().volume_setting, 16);
    proxy
        .write_control_point(
            &mut client,
            &mut transport,
            VolumeControlPointOpcode::UNMUTE_RELATIVE_VOLUME_UP,
            1,
            None,
        )
        .unwrap();
    assert_eq!(
        service.state().unwrap(),
        VolumeState {
            volume_setting: 32,
            mute: 0,
            change_counter: 2,
        }
    );
    proxy
        .write_control_point(
            &mut client,
            &mut transport,
            VolumeControlPointOpcode::SET_ABSOLUTE_VOLUME,
            2,
            Some(200),
        )
        .unwrap();
    assert_eq!(service.state().unwrap().change_counter, 3);
    proxy
        .write_control_point(
            &mut client,
            &mut transport,
            VolumeControlPointOpcode::SET_ABSOLUTE_VOLUME,
            3,
            Some(200),
        )
        .unwrap();
    assert_eq!(service.state().unwrap().change_counter, 3);
    assert_att_error(
        proxy.write_control_point(
            &mut client,
            &mut transport,
            VolumeControlPointOpcode::MUTE,
            99,
            None,
        ),
        vcs_error::INVALID_CHANGE_COUNTER,
    );
    assert_att_error(
        proxy.write_control_point(
            &mut client,
            &mut transport,
            VolumeControlPointOpcode(0xFF),
            3,
            None,
        ),
        vcs_error::OPCODE_NOT_SUPPORTED,
    );
}

#[test]
fn vocs_live_state_location_description_and_errors_match_upstream() {
    let service = VolumeOffsetControlService::new();
    let mut server = GattServer::from_definitions(vec![service.definition()]).unwrap();
    service.bind(&mut server).unwrap();
    let mut transport = EncryptedTransport(&mut server);
    let mut client = GattClient::new();
    let proxy = VolumeOffsetControlServiceProxy::discover(&mut client, &mut transport)
        .unwrap()
        .unwrap();

    assert_eq!(
        proxy
            .read_volume_offset_state(&mut client, &mut transport)
            .unwrap(),
        VolumeOffsetState::default()
    );
    assert_eq!(
        proxy
            .read_audio_location(&mut client, &mut transport)
            .unwrap(),
        AudioLocation::NOT_ALLOWED
    );
    assert_eq!(
        proxy
            .read_audio_output_description(&mut client, &mut transport)
            .unwrap(),
        ""
    );
    assert_att_error(
        client
            .write_value(
                &mut transport,
                proxy.volume_offset_control_point.handle,
                vec![0xFF],
                true,
            )
            .map_err(Error::from),
        vocs_error::OPCODE_NOT_SUPPORTED,
    );
    assert_att_error(
        proxy.set_volume_offset(&mut client, &mut transport, 1, 0),
        vocs_error::INVALID_CHANGE_COUNTER,
    );
    assert_att_error(
        proxy.set_volume_offset(&mut client, &mut transport, 0, -256),
        vocs_error::VALUE_OUT_OF_RANGE,
    );
    proxy
        .set_volume_offset(&mut client, &mut transport, 0, -255)
        .unwrap();
    assert_eq!(
        service.state().unwrap(),
        VolumeOffsetState {
            volume_offset: -255,
            change_counter: 1,
        }
    );
    let location = AudioLocation::FRONT_LEFT | AudioLocation::FRONT_RIGHT;
    proxy
        .write_audio_location(&mut client, &mut transport, location)
        .unwrap();
    assert_eq!(
        proxy
            .read_audio_location(&mut client, &mut transport)
            .unwrap(),
        location
    );
    proxy
        .write_audio_output_description(&mut client, &mut transport, "Left Speaker")
        .unwrap();
    assert_eq!(
        proxy
            .read_audio_output_description(&mut client, &mut transport)
            .unwrap(),
        "Left Speaker"
    );
}

#[test]
fn aics_live_control_matrix_and_description_match_upstream() {
    let service =
        AudioInputControlService::new().gain_settings_properties(GainSettingsProperties {
            gain_settings_unit: 1,
            gain_settings_minimum: 10,
            gain_settings_maximum: 200,
        });
    let mut server = GattServer::from_definitions(vec![service.definition()]).unwrap();
    service.bind(&mut server).unwrap();
    let mut transport = EncryptedTransport(&mut server);
    let mut client = GattClient::new();
    let proxy = AudioInputControlServiceProxy::discover(&mut client, &mut transport)
        .unwrap()
        .unwrap();

    assert_eq!(
        proxy
            .read_audio_input_state(&mut client, &mut transport)
            .unwrap(),
        AudioInputState::default()
    );
    assert_eq!(
        proxy
            .read_gain_settings_properties(&mut client, &mut transport)
            .unwrap(),
        GainSettingsProperties {
            gain_settings_unit: 1,
            gain_settings_minimum: 10,
            gain_settings_maximum: 200,
        }
    );
    assert_eq!(
        proxy
            .read_audio_input_status(&mut client, &mut transport)
            .unwrap(),
        AudioInputStatus::ACTIVE
    );
    assert_eq!(
        proxy
            .read_audio_input_type(&mut client, &mut transport)
            .unwrap(),
        "local"
    );
    assert_att_error(
        proxy.write_control_point(&mut client, &mut transport, vec![0xFF]),
        aics_error::OPCODE_NOT_SUPPORTED,
    );
    assert_att_error(
        proxy.write_control_point(
            &mut client,
            &mut transport,
            vec![AudioInputControlPointOpcode::SET_GAIN_SETTING.0, 0, 201],
        ),
        aics_error::VALUE_OUT_OF_RANGE,
    );
    proxy
        .write_control_point(
            &mut client,
            &mut transport,
            vec![AudioInputControlPointOpcode::SET_GAIN_SETTING.0, 0, 120],
        )
        .unwrap();
    assert_eq!(service.state().unwrap().gain_settings, 120);
    assert_eq!(service.state().unwrap().change_counter, 0);
    proxy
        .write_control_point(
            &mut client,
            &mut transport,
            vec![AudioInputControlPointOpcode::SET_AUTOMATIC_GAIN_MODE.0, 0],
        )
        .unwrap();
    assert_eq!(service.state().unwrap().gain_mode, GainMode::AUTOMATIC);
    assert_eq!(service.state().unwrap().change_counter, 1);
    proxy
        .write_control_point(
            &mut client,
            &mut transport,
            vec![AudioInputControlPointOpcode::SET_GAIN_SETTING.0, 1, 100],
        )
        .unwrap();
    assert_eq!(service.state().unwrap().gain_settings, 120);
    assert_att_error(
        proxy.write_control_point(
            &mut client,
            &mut transport,
            vec![AudioInputControlPointOpcode::MUTE.0, 99],
        ),
        aics_error::INVALID_CHANGE_COUNTER,
    );
    proxy
        .write_control_point(
            &mut client,
            &mut transport,
            vec![AudioInputControlPointOpcode::MUTE.0, 1],
        )
        .unwrap();
    assert_eq!(service.state().unwrap().mute, Mute::MUTED);
    assert_eq!(service.state().unwrap().change_counter, 2);
    proxy
        .write_control_point(
            &mut client,
            &mut transport,
            vec![AudioInputControlPointOpcode::UNMUTE.0, 2],
        )
        .unwrap();
    assert_eq!(service.state().unwrap().mute, Mute::NOT_MUTED);
    assert_eq!(service.state().unwrap().change_counter, 3);
    proxy
        .write_audio_input_description(&mut client, &mut transport, "Line Input")
        .unwrap();
    assert_eq!(
        proxy
            .read_audio_input_description(&mut client, &mut transport)
            .unwrap(),
        "Line Input"
    );
}

#[test]
fn aics_disabled_and_fixed_modes_return_application_errors() {
    for (state, opcode, expected) in [
        (
            AudioInputState {
                mute: Mute::DISABLED,
                ..AudioInputState::default()
            },
            AudioInputControlPointOpcode::UNMUTE,
            aics_error::MUTE_DISABLED,
        ),
        (
            AudioInputState {
                gain_mode: GainMode::MANUAL_ONLY,
                ..AudioInputState::default()
            },
            AudioInputControlPointOpcode::SET_AUTOMATIC_GAIN_MODE,
            aics_error::GAIN_MODE_CHANGE_NOT_ALLOWED,
        ),
        (
            AudioInputState {
                gain_mode: GainMode::AUTOMATIC_ONLY,
                ..AudioInputState::default()
            },
            AudioInputControlPointOpcode::SET_MANUAL_GAIN_MODE,
            aics_error::GAIN_MODE_CHANGE_NOT_ALLOWED,
        ),
    ] {
        let service = AudioInputControlService::new().initial_state(state);
        let mut server = GattServer::from_definitions(vec![service.definition()]).unwrap();
        service.bind(&mut server).unwrap();
        let mut transport = EncryptedTransport(&mut server);
        let mut client = GattClient::new();
        let proxy = AudioInputControlServiceProxy::discover(&mut client, &mut transport)
            .unwrap()
            .unwrap();
        assert_att_error(
            proxy.write_control_point(&mut client, &mut transport, vec![opcode.0, 0]),
            expected,
        );
    }
}

#[test]
fn vcs_discovers_included_vocs_and_aics_secondary_services() {
    let vocs = VolumeOffsetControlService::new();
    let aics = AudioInputControlService::new();
    let vcs = VolumeControlService::new().included_services(vec![0, 1]);
    let mut server =
        GattServer::from_definitions(vec![vocs.definition(), aics.definition(), vcs.definition()])
            .unwrap();
    vocs.bind(&mut server).unwrap();
    aics.bind(&mut server).unwrap();
    vcs.bind(&mut server).unwrap();
    let mut transport = EncryptedTransport(&mut server);
    let mut client = GattClient::new();
    let vcs_proxy = VolumeControlServiceProxy::discover(&mut client, &mut transport)
        .unwrap()
        .unwrap();
    let included = client
        .discover_included_services(&mut transport, &vcs_proxy.service)
        .unwrap();
    assert_eq!(
        included
            .iter()
            .map(|service| service.uuid.clone())
            .collect::<Vec<_>>(),
        [Uuid::from_16_bits(0x1845), Uuid::from_16_bits(0x1843)]
    );

    let vocs_characteristics = client
        .discover_characteristics(&mut transport, &included[0])
        .unwrap();
    let vocs_proxy =
        VolumeOffsetControlServiceProxy::from_parts(included[0].clone(), &vocs_characteristics)
            .unwrap();
    assert_eq!(
        vocs_proxy.audio_location.uuid,
        Uuid::from_16_bits(AUDIO_LOCATION_CHARACTERISTIC)
    );
    assert_eq!(
        vocs_proxy.volume_offset_control_point.uuid,
        Uuid::from_16_bits(VOLUME_OFFSET_CONTROL_POINT_CHARACTERISTIC)
    );
    assert_eq!(
        vocs_proxy.audio_output_description.uuid,
        Uuid::from_16_bits(AUDIO_OUTPUT_DESCRIPTION_CHARACTERISTIC)
    );

    let aics_characteristics = client
        .discover_characteristics(&mut transport, &included[1])
        .unwrap();
    assert!(
        AudioInputControlServiceProxy::from_parts(included[1].clone(), &aics_characteristics)
            .is_ok()
    );
}

fn assert_att_error(result: Result<()>, expected: u8) {
    assert!(matches!(
        result,
        Err(Error::Gatt(GattError::Att { error_code, .. })) if error_code == expected
    ));
}

struct EncryptedTransport<'a>(&'a mut GattServer);

impl AttTransport for EncryptedTransport<'_> {
    fn request(&mut self, request: &AttPdu) -> AttPdu {
        self.0.on_request_with_context(
            request,
            AccessContext {
                bearer_id: 1,
                encrypted: true,
                authenticated: false,
                authorized: false,
            },
        )
    }
}
