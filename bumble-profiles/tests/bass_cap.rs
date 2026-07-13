use bumble::{Address, AddressType};
use bumble_att::AttPdu;
use bumble_gatt::{AccessContext, AttTransport, GattClient, GattServer};
use bumble_profiles::bass::{
    BigEncryption, BroadcastAudioScanService, BroadcastAudioScanServiceProxy,
    BroadcastReceiveState, ControlPointOperation, PeriodicAdvertisingSyncParams,
    PeriodicAdvertisingSyncState, SubgroupInfo,
};
use bumble_profiles::cap::{CommonAudioService, CommonAudioServiceProxy};
use bumble_profiles::csip::{CoordinatedSetIdentificationService, SirkType};

fn address() -> Address {
    Address::parse("AA:BB:CC:DD:EE:FF", AddressType::PUBLIC_DEVICE).unwrap()
}

fn subgroups() -> Vec<SubgroupInfo> {
    vec![
        SubgroupInfo {
            bis_sync: 6677,
            metadata: vec![0x11, 0x22, 0x33],
        },
        SubgroupInfo {
            bis_sync: 8899,
            metadata: vec![0x45, 0x67],
        },
    ]
}

#[test]
fn all_bass_control_operations_match_upstream_wire_forms() {
    let operations = vec![
        ControlPointOperation::RemoteScanStopped,
        ControlPointOperation::RemoteScanStarted,
        ControlPointOperation::AddSource {
            advertiser_address: address(),
            advertising_sid: 34,
            broadcast_id: 123_456,
            pa_sync: PeriodicAdvertisingSyncParams::SYNCHRONIZE_TO_PA_PAST_NOT_AVAILABLE,
            pa_interval: 456,
            subgroups: Vec::new(),
        },
        ControlPointOperation::AddSource {
            advertiser_address: address(),
            advertising_sid: 34,
            broadcast_id: 123_456,
            pa_sync: PeriodicAdvertisingSyncParams::SYNCHRONIZE_TO_PA_PAST_NOT_AVAILABLE,
            pa_interval: 456,
            subgroups: subgroups(),
        },
        ControlPointOperation::ModifySource {
            source_id: 12,
            pa_sync: PeriodicAdvertisingSyncParams::SYNCHRONIZE_TO_PA_PAST_NOT_AVAILABLE,
            pa_interval: 567,
            subgroups: subgroups(),
        },
        ControlPointOperation::SetBroadcastCode {
            source_id: 7,
            broadcast_code: [
                0xA0, 0xA1, 0xA2, 0xA3, 0xA4, 0xA5, 0xA6, 0xA7, 0xA8, 0xA9, 0xAA, 0xAB, 0xAC, 0xAD,
                0xAE, 0xAF,
            ],
        },
        ControlPointOperation::RemoveSource { source_id: 7 },
    ];
    for operation in operations {
        let bytes = operation.to_bytes().unwrap();
        assert_eq!(
            ControlPointOperation::from_bytes(&bytes).unwrap(),
            operation
        );
        assert!(ControlPointOperation::from_bytes(&bytes[..bytes.len() - 1]).is_err());
    }
    assert_eq!(
        ControlPointOperation::AddSource {
            advertiser_address: address(),
            advertising_sid: 34,
            broadcast_id: 123_456,
            pa_sync: PeriodicAdvertisingSyncParams::SYNCHRONIZE_TO_PA_PAST_NOT_AVAILABLE,
            pa_interval: 456,
            subgroups: Vec::new(),
        }
        .to_bytes()
        .unwrap(),
        [
            0x02, 0x00, 0xFF, 0xEE, 0xDD, 0xCC, 0xBB, 0xAA, 34, 0x40, 0xE2, 0x01, 0x02, 0xC8, 0x01,
            0x00,
        ]
    );
    assert!(ControlPointOperation::from_bytes(&[0xFF]).is_err());
}

fn receive_state(encryption: BigEncryption) -> BroadcastReceiveState {
    BroadcastReceiveState {
        source_id: 12,
        source_address: address(),
        source_adv_sid: 123,
        broadcast_id: 123_456,
        pa_sync_state: PeriodicAdvertisingSyncState::SYNCHRONIZED_TO_PA,
        big_encryption: encryption,
        bad_code: (encryption == BigEncryption::BAD_CODE).then_some([
            0xA0, 0xA1, 0xA2, 0xA3, 0xA4, 0xA5, 0xA6, 0xA7, 0xA8, 0xA9, 0xAA, 0xAB, 0xAC, 0xAD,
            0xAE, 0xAF,
        ]),
        subgroups: subgroups(),
    }
}

#[test]
fn broadcast_receive_states_round_trip_encrypted_and_bad_code_forms() {
    for state in [
        receive_state(BigEncryption::DECRYPTING),
        receive_state(BigEncryption::BAD_CODE),
    ] {
        let bytes = state.to_bytes().unwrap();
        assert_eq!(BroadcastReceiveState::from_bytes(&bytes).unwrap(), state);
        assert!(BroadcastReceiveState::from_bytes(&bytes[..bytes.len() - 1]).is_err());
    }
    let mut invalid = receive_state(BigEncryption::DECRYPTING);
    invalid.bad_code = Some([0; 16]);
    assert!(invalid.to_bytes().is_err());
}

#[test]
fn live_bass_accepts_control_operations_and_notifies_receive_state() {
    let service = BroadcastAudioScanService::new(2).unwrap();
    let mut server = GattServer::from_definitions(vec![service.definition().unwrap()]).unwrap();
    let handles = service.bind(&mut server).unwrap();
    let mut client = GattClient::new();
    let proxy = BroadcastAudioScanServiceProxy::discover(&mut client, &mut server)
        .unwrap()
        .unwrap();
    assert_eq!(proxy.broadcast_receive_states.len(), 2);
    proxy
        .subscribe_receive_states(&mut client, &mut server)
        .unwrap();

    let operation = ControlPointOperation::AddSource {
        advertiser_address: address(),
        advertising_sid: 34,
        broadcast_id: 123_456,
        pa_sync: PeriodicAdvertisingSyncParams::SYNCHRONIZE_TO_PA_PAST_NOT_AVAILABLE,
        pa_interval: 456,
        subgroups: subgroups(),
    };
    proxy
        .send_control_point_operation(&mut client, &mut server, &operation)
        .unwrap();
    assert_eq!(service.take_operation().unwrap(), Some(operation));

    let state = receive_state(BigEncryption::DECRYPTING);
    service.set_receive_state(1, Some(state.clone())).unwrap();
    let notifications = service.take_pending_notifications(&handles).unwrap();
    assert_eq!(notifications.len(), 1);
    let (handle, value) = &notifications[0];
    assert!(client
        .on_notification(&server.notify(*handle, value.clone()))
        .unwrap());
    assert_eq!(
        proxy.state_from_notification(*handle, value).unwrap(),
        Some(state.clone())
    );

    let mut encrypted = EncryptedTransport(&mut server);
    assert_eq!(
        BroadcastAudioScanServiceProxy::read_receive_state(
            &proxy.broadcast_receive_states[1],
            &mut client,
            &mut encrypted,
        )
        .unwrap(),
        Some(state)
    );
    assert!(BroadcastAudioScanServiceProxy::read_receive_state(
        &proxy.broadcast_receive_states[0],
        &mut client,
        &mut encrypted,
    )
    .unwrap()
    .is_none());
}

#[test]
fn common_audio_service_includes_and_discovers_csis() {
    let csis = CoordinatedSetIdentificationService::new(&[0x2F; 16], SirkType::Plaintext).unwrap();
    let mut server = GattServer::from_definitions(CommonAudioService::definitions(&csis)).unwrap();
    let mut client = GattClient::new();
    let proxy = CommonAudioServiceProxy::discover(&mut client, &mut server)
        .unwrap()
        .unwrap();
    assert!(proxy
        .discover_coordinated_set_service(&mut client, &mut server)
        .unwrap()
        .is_some());
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
