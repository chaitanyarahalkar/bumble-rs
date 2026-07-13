use bumble_gatt::{GattClient, GattServer};
use bumble_hci::CodingFormat;
use bumble_profiles::ascs::{
    AseControlResponse, AseEvent, AseMetadataParameters, AseOpcode, AseOperation, AseResponseCode,
    AseState, AseStatus, AudioStreamControlHandles, AudioStreamControlService,
    AudioStreamControlServiceProxy, ConfigCodecParameters, ConfigQosParameters,
};
use bumble_profiles::bap::{
    AudioLocation, CodecSpecificConfiguration, FrameDuration, SamplingFrequency,
};
use bumble_profiles::le_audio::{Metadata, MetadataEntry, MetadataTag};

fn lc3() -> CodingFormat {
    CodingFormat {
        coding_format: 0x06,
        company_id: 0,
        vendor_specific_codec_id: 0,
    }
}

fn codec_configuration() -> Vec<u8> {
    CodecSpecificConfiguration {
        sampling_frequency: Some(SamplingFrequency::FREQ_48000),
        frame_duration: Some(FrameDuration::DURATION_10000_US),
        audio_channel_allocation: Some(AudioLocation::FRONT_LEFT),
        octets_per_codec_frame: Some(120),
        codec_frames_per_sdu: Some(1),
    }
    .to_bytes()
}

fn metadata(name: &[u8]) -> Vec<u8> {
    Metadata::new(vec![MetadataEntry::new(
        MetadataTag::PROGRAM_INFO,
        name.to_vec(),
    )])
    .to_bytes()
    .unwrap()
}

#[test]
fn all_ase_operations_round_trip_and_reject_malformed_pdus() {
    let operations = vec![
        AseOperation::ConfigCodec(vec![
            ConfigCodecParameters {
                ase_id: 1,
                target_latency: 3,
                target_phy: 5,
                codec_id: lc3(),
                codec_specific_configuration: b"foo".to_vec(),
            },
            ConfigCodecParameters {
                ase_id: 2,
                target_latency: 4,
                target_phy: 6,
                codec_id: lc3(),
                codec_specific_configuration: b"bar".to_vec(),
            },
        ]),
        AseOperation::ConfigQos(vec![ConfigQosParameters {
            ase_id: 1,
            cig_id: 2,
            cis_id: 3,
            sdu_interval: 0x000605,
            framing: 1,
            phy: 2,
            max_sdu: 0x0403,
            retransmission_number: 7,
            max_transport_latency: 0x0908,
            presentation_delay: 0x000B0A,
        }]),
        AseOperation::Enable(vec![AseMetadataParameters {
            ase_id: 1,
            metadata: Vec::new(),
        }]),
        AseOperation::ReceiverStartReady(vec![1, 2]),
        AseOperation::Disable(vec![1, 2]),
        AseOperation::ReceiverStopReady(vec![1, 2]),
        AseOperation::UpdateMetadata(vec![AseMetadataParameters {
            ase_id: 1,
            metadata: Vec::new(),
        }]),
        AseOperation::Release(vec![1, 2]),
    ];
    for operation in operations {
        let bytes = operation.to_bytes().unwrap();
        assert_eq!(AseOperation::from_bytes(&bytes).unwrap(), operation);
        assert!(AseOperation::from_bytes(&bytes[..bytes.len() - 1]).is_err());
    }
    assert!(AseOperation::from_bytes(&[]).is_err());
    assert!(AseOperation::from_bytes(&[0xFF, 0]).is_err());
    assert!(AseOperation::ConfigQos(vec![ConfigQosParameters {
        ase_id: 1,
        cig_id: 1,
        cis_id: 1,
        sdu_interval: 0x0100_0000,
        framing: 0,
        phy: 1,
        max_sdu: 10,
        retransmission_number: 1,
        max_transport_latency: 10,
        presentation_delay: 0,
    }])
    .to_bytes()
    .is_err());
}

fn drain_events(
    service: &AudioStreamControlService,
    handles: &AudioStreamControlHandles,
    proxy: &AudioStreamControlServiceProxy,
    client: &mut GattClient,
    server: &GattServer,
) -> Vec<AseEvent> {
    service
        .take_pending_notifications(handles)
        .unwrap()
        .into_iter()
        .map(|(handle, value)| {
            assert!(client
                .on_notification(&server.notify(handle, value.clone()))
                .unwrap());
            proxy.event_from_notification(handle, &value).unwrap()
        })
        .collect()
}

fn assert_success_response(event: &AseEvent, opcode: AseOpcode, count: usize) {
    let AseEvent::ControlPoint(AseControlResponse {
        opcode: actual_opcode,
        responses,
    }) = event
    else {
        panic!("expected control-point response, got {event:?}");
    };
    assert_eq!(*actual_opcode, opcode);
    assert_eq!(responses.len(), count);
    assert!(responses
        .iter()
        .all(|response| response.code == AseResponseCode::SUCCESS));
}

fn event_state(event: &AseEvent) -> AseState {
    let AseEvent::Status(AseStatus { state, .. }) = event else {
        panic!("expected ASE status, got {event:?}");
    };
    *state
}

#[test]
fn live_ascs_sink_and_source_follow_complete_state_machine() {
    let service = AudioStreamControlService::new(&[1], &[2]).unwrap();
    let mut server = GattServer::from_definitions(vec![service.definition().unwrap()]).unwrap();
    let handles = service.bind(&mut server).unwrap();
    let mut client = GattClient::new();
    let proxy = AudioStreamControlServiceProxy::discover(&mut client, &mut server)
        .unwrap()
        .unwrap();
    assert_eq!(proxy.sink_ase.len(), 1);
    assert_eq!(proxy.source_ase.len(), 1);
    proxy.subscribe_all(&mut client, &mut server).unwrap();
    assert_eq!(
        AudioStreamControlServiceProxy::read_ase(&proxy.sink_ase[0], &mut client, &mut server,)
            .unwrap()
            .state,
        AseState::IDLE
    );

    let configuration = codec_configuration();
    proxy
        .write_operation(
            &mut client,
            &mut server,
            &AseOperation::ConfigCodec(vec![
                ConfigCodecParameters {
                    ase_id: 1,
                    target_latency: 3,
                    target_phy: 2,
                    codec_id: lc3(),
                    codec_specific_configuration: configuration.clone(),
                },
                ConfigCodecParameters {
                    ase_id: 2,
                    target_latency: 4,
                    target_phy: 2,
                    codec_id: lc3(),
                    codec_specific_configuration: configuration,
                },
            ]),
        )
        .unwrap();
    let events = drain_events(&service, &handles, &proxy, &mut client, &server);
    assert_success_response(&events[0], AseOpcode::ConfigCodec, 2);
    assert_eq!(event_state(&events[1]), AseState::CODEC_CONFIGURED);
    assert_eq!(event_state(&events[2]), AseState::CODEC_CONFIGURED);

    proxy
        .write_operation(
            &mut client,
            &mut server,
            &AseOperation::ConfigQos(vec![
                ConfigQosParameters {
                    ase_id: 1,
                    cig_id: 1,
                    cis_id: 1,
                    sdu_interval: 100,
                    framing: 0,
                    phy: 1,
                    max_sdu: 100,
                    retransmission_number: 16,
                    max_transport_latency: 150,
                    presentation_delay: 10,
                },
                ConfigQosParameters {
                    ase_id: 2,
                    cig_id: 1,
                    cis_id: 1,
                    sdu_interval: 100,
                    framing: 0,
                    phy: 1,
                    max_sdu: 100,
                    retransmission_number: 16,
                    max_transport_latency: 150,
                    presentation_delay: 10,
                },
            ]),
        )
        .unwrap();
    let events = drain_events(&service, &handles, &proxy, &mut client, &server);
    assert_success_response(&events[0], AseOpcode::ConfigQos, 2);
    assert_eq!(event_state(&events[1]), AseState::QOS_CONFIGURED);
    assert_eq!(event_state(&events[2]), AseState::QOS_CONFIGURED);

    proxy
        .write_operation(
            &mut client,
            &mut server,
            &AseOperation::Enable(vec![
                AseMetadataParameters {
                    ase_id: 1,
                    metadata: metadata(b"sink"),
                },
                AseMetadataParameters {
                    ase_id: 2,
                    metadata: metadata(b"source"),
                },
            ]),
        )
        .unwrap();
    let events = drain_events(&service, &handles, &proxy, &mut client, &server);
    assert_success_response(&events[0], AseOpcode::Enable, 2);
    assert_eq!(event_state(&events[1]), AseState::ENABLING);
    assert_eq!(event_state(&events[2]), AseState::ENABLING);

    assert_eq!(service.establish_cis(1, 1).unwrap(), [1, 2]);
    let events = drain_events(&service, &handles, &proxy, &mut client, &server);
    assert_eq!(event_state(&events[0]), AseState::STREAMING);
    assert_eq!(event_state(&events[1]), AseState::ENABLING);

    proxy
        .write_operation(
            &mut client,
            &mut server,
            &AseOperation::ReceiverStartReady(vec![2]),
        )
        .unwrap();
    let events = drain_events(&service, &handles, &proxy, &mut client, &server);
    assert_success_response(&events[0], AseOpcode::ReceiverStartReady, 1);
    assert_eq!(event_state(&events[1]), AseState::STREAMING);

    proxy
        .write_operation(&mut client, &mut server, &AseOperation::Disable(vec![1, 2]))
        .unwrap();
    let events = drain_events(&service, &handles, &proxy, &mut client, &server);
    assert_success_response(&events[0], AseOpcode::Disable, 2);
    assert_eq!(event_state(&events[1]), AseState::QOS_CONFIGURED);
    assert_eq!(event_state(&events[2]), AseState::DISABLING);

    proxy
        .write_operation(
            &mut client,
            &mut server,
            &AseOperation::ReceiverStopReady(vec![2]),
        )
        .unwrap();
    let events = drain_events(&service, &handles, &proxy, &mut client, &server);
    assert_success_response(&events[0], AseOpcode::ReceiverStopReady, 1);
    assert_eq!(event_state(&events[1]), AseState::QOS_CONFIGURED);

    proxy
        .write_operation(&mut client, &mut server, &AseOperation::Release(vec![1, 2]))
        .unwrap();
    let events = drain_events(&service, &handles, &proxy, &mut client, &server);
    assert_success_response(&events[0], AseOpcode::Release, 2);
    assert_eq!(event_state(&events[1]), AseState::RELEASING);
    assert_eq!(event_state(&events[2]), AseState::IDLE);
    assert_eq!(event_state(&events[3]), AseState::RELEASING);
    assert_eq!(event_state(&events[4]), AseState::IDLE);
}

#[test]
fn ascs_reports_invalid_ids_and_transitions() {
    let service = AudioStreamControlService::new(&[1], &[]).unwrap();
    let mut server = GattServer::from_definitions(vec![service.definition().unwrap()]).unwrap();
    let handles = service.bind(&mut server).unwrap();
    let mut client = GattClient::new();
    let proxy = AudioStreamControlServiceProxy::discover(&mut client, &mut server)
        .unwrap()
        .unwrap();
    proxy.subscribe_all(&mut client, &mut server).unwrap();
    proxy
        .write_operation(
            &mut client,
            &mut server,
            &AseOperation::Disable(vec![1, 99]),
        )
        .unwrap();
    let events = drain_events(&service, &handles, &proxy, &mut client, &server);
    let AseEvent::ControlPoint(response) = &events[0] else {
        panic!("missing response")
    };
    assert_eq!(
        response.responses[0].code,
        AseResponseCode::INVALID_ASE_STATE_MACHINE_TRANSITION
    );
    assert_eq!(response.responses[1].code, AseResponseCode::INVALID_ASE_ID);
    assert_eq!(event_state(&events[1]), AseState::IDLE);
}
