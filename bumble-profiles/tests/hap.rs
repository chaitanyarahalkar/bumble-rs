use bumble_att::AttPdu;
use bumble_gatt::{AccessContext, AttTransport, GattClient, GattServer};
use bumble_profiles::hap::{
    DynamicPresets, HearingAccessHandles, HearingAccessNotification, HearingAccessService,
    HearingAccessServiceProxy, HearingAidFeatures, HearingAidType, IndependentPresets,
    PresetAvailability, PresetChangedOperation, PresetControlPointEvent,
    PresetControlPointOperation, PresetProperties, PresetRecord, PresetSynchronizationSupport,
    PresetWritable, WritablePresetsSupport,
};

fn features(synchronization: bool) -> HearingAidFeatures {
    HearingAidFeatures {
        hearing_aid_type: HearingAidType::MONAURAL_HEARING_AID,
        preset_synchronization_support: if synchronization {
            PresetSynchronizationSupport::PRESET_SYNCHRONIZATION_IS_SUPPORTED
        } else {
            PresetSynchronizationSupport::PRESET_SYNCHRONIZATION_IS_NOT_SUPPORTED
        },
        independent_presets: IndependentPresets::IDENTICAL_PRESET_RECORD,
        dynamic_presets: DynamicPresets::PRESET_RECORDS_DOES_NOT_CHANGE,
        writable_presets_support: WritablePresetsSupport::WRITABLE_PRESET_RECORDS_SUPPORTED,
    }
}

fn presets() -> Vec<PresetRecord> {
    vec![
        PresetRecord::new(1, "foo preset"),
        PresetRecord::new(50, "bar preset"),
        PresetRecord::new(5, "foobar preset"),
        PresetRecord {
            index: 78,
            name: "unavailable preset".into(),
            properties: PresetProperties {
                writable: PresetWritable::CANNOT_BE_WRITTEN,
                is_available: PresetAvailability::IS_UNAVAILABLE,
            },
        },
    ]
}

fn setup(
    synchronization: bool,
) -> (
    HearingAccessService,
    HearingAccessHandles,
    GattServer,
    GattClient,
    HearingAccessServiceProxy,
) {
    let service = HearingAccessService::new(features(synchronization), presets()).unwrap();
    let mut server = GattServer::from_definitions(vec![service.definition().unwrap()]).unwrap();
    let handles = service.bind(&mut server).unwrap();
    let mut client = GattClient::new();
    let proxy = HearingAccessServiceProxy::discover(&mut client, &mut server)
        .unwrap()
        .unwrap();
    {
        let mut encrypted = EncryptedTransport(&mut server);
        proxy
            .setup_subscription(&mut client, &mut encrypted)
            .unwrap();
    }
    (service, handles, server, client, proxy)
}

fn write(
    proxy: &HearingAccessServiceProxy,
    client: &mut GattClient,
    server: &mut GattServer,
    operation: &PresetControlPointOperation,
) -> bumble_profiles::Result<()> {
    proxy.write_control_point(client, &mut EncryptedTransport(server), operation)
}

fn drain(
    service: &HearingAccessService,
    handles: HearingAccessHandles,
    proxy: &HearingAccessServiceProxy,
    client: &mut GattClient,
    server: &GattServer,
) -> Vec<PresetControlPointEvent> {
    service
        .take_pending_events(handles)
        .unwrap()
        .into_iter()
        .filter_map(|event| process_event(event, proxy, client, server))
        .collect()
}

fn process_event(
    event: HearingAccessNotification,
    proxy: &HearingAccessServiceProxy,
    client: &mut GattClient,
    server: &GattServer,
) -> Option<PresetControlPointEvent> {
    if event.indicate {
        let _confirmation = client
            .on_indication(&server.indicate(event.handle, event.value.clone()))
            .unwrap();
        Some(
            proxy
                .event_from_indication(event.handle, &event.value)
                .unwrap(),
        )
    } else {
        assert!(client
            .on_notification(&server.notify(event.handle, event.value.clone()))
            .unwrap());
        assert_eq!(
            proxy
                .active_index_from_notification(event.handle, &event.value)
                .unwrap(),
            event.value[0]
        );
        None
    }
}

#[test]
fn feature_preset_and_control_wire_models_are_exact() {
    let features = features(false);
    assert_eq!(features.to_byte(), 0x21);
    assert_eq!(HearingAidFeatures::from_byte(0x21), features);
    let preset = PresetRecord::new(1, "foo preset");
    assert_eq!(
        PresetRecord::from_bytes(&preset.to_bytes().unwrap()).unwrap(),
        preset
    );
    for operation in [
        PresetControlPointOperation::ReadPresets {
            start_index: 1,
            count: 0xFF,
        },
        PresetControlPointOperation::WritePresetName {
            index: 1,
            name: "new name".into(),
        },
        PresetControlPointOperation::SetActivePreset(50),
        PresetControlPointOperation::SetNextPreset,
        PresetControlPointOperation::SetPreviousPreset,
        PresetControlPointOperation::SetActivePresetSynchronizedLocally(5),
        PresetControlPointOperation::SetNextPresetSynchronizedLocally,
        PresetControlPointOperation::SetPreviousPresetSynchronizedLocally,
    ] {
        let bytes = operation.to_bytes().unwrap();
        assert_eq!(
            PresetControlPointOperation::from_bytes(&bytes).unwrap(),
            operation
        );
    }
    let changed = PresetChangedOperation::GenericUpdate {
        previous_index: 1,
        preset_record: PresetRecord::new(1, "new name"),
    };
    let bytes = changed.to_bytes(true).unwrap();
    assert_eq!(
        PresetChangedOperation::from_bytes(&bytes).unwrap(),
        (changed, true)
    );
    assert!(PresetControlPointOperation::from_bytes(&[0x05]).is_err());
    assert!(PresetRecord::new(1, "").to_bytes().is_err());
}

#[test]
fn live_hap_reads_presets_and_updates_active_index_with_wraparound() {
    let (service, handles, mut server, mut client, proxy) = setup(false);
    {
        let mut encrypted = EncryptedTransport(&mut server);
        assert_eq!(
            proxy.read_features(&mut client, &mut encrypted).unwrap(),
            features(false)
        );
        assert_eq!(
            proxy
                .read_active_preset_index(&mut client, &mut encrypted)
                .unwrap(),
            1
        );
    }

    write(
        &proxy,
        &mut client,
        &mut server,
        &PresetControlPointOperation::ReadPresets {
            start_index: 1,
            count: 0xFF,
        },
    )
    .unwrap();
    let events = drain(&service, handles, &proxy, &mut client, &server);
    assert_eq!(events.len(), 4);
    let indices = events
        .iter()
        .map(|event| match event {
            PresetControlPointEvent::ReadPresetResponse { preset_record, .. } => {
                preset_record.index
            }
            _ => panic!("unexpected event"),
        })
        .collect::<Vec<_>>();
    assert_eq!(indices, [1, 5, 50, 78]);
    assert!(matches!(
        events.last(),
        Some(PresetControlPointEvent::ReadPresetResponse { is_last: true, .. })
    ));

    for (operation, expected) in [
        (PresetControlPointOperation::SetNextPreset, 5),
        (PresetControlPointOperation::SetNextPreset, 50),
        (PresetControlPointOperation::SetNextPreset, 1),
        (PresetControlPointOperation::SetPreviousPreset, 50),
    ] {
        write(&proxy, &mut client, &mut server, &operation).unwrap();
        let events = service.take_pending_events(handles).unwrap();
        assert_eq!(events.len(), 1);
        assert!(!events[0].indicate);
        assert_eq!(events[0].value, [expected]);
        process_event(events[0].clone(), &proxy, &mut client, &server);
        assert_eq!(service.active_preset_index().unwrap(), expected);
    }
    assert!(write(
        &proxy,
        &mut client,
        &mut server,
        &PresetControlPointOperation::SetActivePreset(78),
    )
    .is_err());
}

#[test]
fn live_hap_writable_names_changes_and_synchronized_peer_updates() {
    let (service, handles, mut server, mut client, proxy) = setup(true);
    write(
        &proxy,
        &mut client,
        &mut server,
        &PresetControlPointOperation::WritePresetName {
            index: 1,
            name: "renamed".into(),
        },
    )
    .unwrap();
    let events = drain(&service, handles, &proxy, &mut client, &server);
    assert!(matches!(
        &events[0],
        PresetControlPointEvent::PresetChanged {
            operation: PresetChangedOperation::GenericUpdate { preset_record, .. },
            is_last: true,
        } if preset_record.name == "renamed"
    ));
    assert!(write(
        &proxy,
        &mut client,
        &mut server,
        &PresetControlPointOperation::WritePresetName {
            index: 78,
            name: "blocked".into(),
        },
    )
    .is_err());

    service.set_preset_available(78, true).unwrap();
    let events = drain(&service, handles, &proxy, &mut client, &server);
    assert!(matches!(
        events[0],
        PresetControlPointEvent::PresetChanged {
            operation: PresetChangedOperation::PresetRecordAvailable(78),
            ..
        }
    ));
    service.delete_preset(5).unwrap();
    let events = drain(&service, handles, &proxy, &mut client, &server);
    assert!(matches!(
        events[0],
        PresetControlPointEvent::PresetChanged {
            operation: PresetChangedOperation::PresetRecordDeleted(5),
            ..
        }
    ));

    let peer = HearingAccessService::new(features(true), presets()).unwrap();
    service.set_other_server_in_binaural_set(&peer).unwrap();
    write(
        &proxy,
        &mut client,
        &mut server,
        &PresetControlPointOperation::SetActivePresetSynchronizedLocally(50),
    )
    .unwrap();
    assert_eq!(service.active_preset_index().unwrap(), 50);
    assert_eq!(peer.active_preset_index().unwrap(), 50);
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
