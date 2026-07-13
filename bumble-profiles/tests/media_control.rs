use bumble_att::AttPdu;
use bumble_gatt::{AccessContext, AttTransport, GattClient, GattServer};
use bumble_profiles::mcp::{
    GenericMediaControlService, GenericMediaControlServiceProxy, GroupObjectType,
    MediaControlEvent, MediaControlPointOpcode, MediaControlPointOpcodeSupported,
    MediaControlPointResultCode, MediaControlService, MediaControlServiceProxy, MediaState,
    ObjectId, ObjectType, PlayingOrder, PlayingOrderSupported, SearchControlPointItemType,
    GENERIC_MEDIA_CONTROL_SERVICE, MEDIA_CONTROL_SERVICE,
};

#[test]
fn media_models_preserve_open_values_and_exact_object_ids() {
    assert_eq!(PlayingOrder::SHUFFLE_REPEAT.0, 0x0A);
    assert_eq!(PlayingOrderSupported::NEWEST_REPEAT.0, 0x0080);
    assert_eq!(
        (PlayingOrderSupported::SINGLE_ONCE | PlayingOrderSupported::SHUFFLE_REPEAT).0,
        0x0201
    );
    assert_eq!(MediaControlPointOpcode::GOTO_GROUP.0, 0x44);
    assert_eq!(MediaControlPointOpcodeSupported::GOTO_GROUP.0, 0x0010_0000);
    assert_eq!(
        (MediaControlPointOpcodeSupported::PLAY | MediaControlPointOpcodeSupported::PAUSE).0,
        3
    );
    assert_eq!(SearchControlPointItemType::ONLY_GROUPS.0, 0x09);
    assert_eq!(MediaState(0xFE).0, 0xFE);

    let object_id = ObjectId::new(0x0605_0403_0201).unwrap();
    assert_eq!(object_id.encode().unwrap(), [1, 2, 3, 4, 5, 6]);
    assert_eq!(ObjectId::decode(&[1, 2, 3, 4, 5, 6]).unwrap(), object_id);
    assert!(ObjectId::new(1 << 48).is_err());
    assert!(ObjectId::decode(&[1; 5]).is_err());

    let group = GroupObjectType {
        object_type: ObjectType::GROUP,
        object_id,
    };
    assert_eq!(group.encode().unwrap(), [1, 1, 2, 3, 4, 5, 6]);
    assert_eq!(
        GroupObjectType::decode(&group.encode().unwrap()).unwrap(),
        group
    );
}

#[test]
fn generic_media_control_live_subscriptions_and_control_response_match_upstream() {
    let service = GenericMediaControlService::default();
    assert_eq!(
        service.definition().uuid,
        bumble::Uuid::from_16_bits(GENERIC_MEDIA_CONTROL_SERVICE)
    );
    assert_eq!(service.definition().characteristics.len(), 9);
    let mut server = GattServer::from_definitions(vec![service.definition()]).unwrap();
    let handles = service.bind(&mut server).unwrap();
    let mut transport = EncryptedTransport(&mut server);
    let mut client = GattClient::new();
    let proxy = GenericMediaControlServiceProxy::discover(&mut client, &mut transport)
        .unwrap()
        .unwrap();
    proxy
        .subscribe_characteristics(&mut client, &mut transport)
        .unwrap();

    let name = client
        .read_value(
            &mut transport,
            proxy.media_player_name.as_ref().unwrap().handle,
            false,
        )
        .unwrap();
    assert_eq!(name, b"Bumble Player");
    assert!(proxy.media_player_icon_object_id.is_none());
    assert!(proxy.playing_order.is_none());
    assert!(proxy.search_control_point.is_none());

    proxy
        .write_control_point(&mut client, &mut transport, MediaControlPointOpcode::PAUSE)
        .unwrap();
    let response = service.take_control_response().unwrap().unwrap();
    assert_eq!(response, [MediaControlPointOpcode::PAUSE.0, 1]);
    let notification = transport
        .0
        .notify(handles.media_control_point, response.to_vec());
    assert!(client.on_notification(&notification).unwrap());
    let event = proxy
        .event_from_notification(handles.media_control_point, &response)
        .unwrap();
    assert_eq!(
        proxy
            .control_result(MediaControlPointOpcode::PAUSE, event)
            .unwrap(),
        MediaControlPointResultCode::SUCCESS
    );
}

#[test]
fn media_notification_decoding_covers_all_upstream_events() {
    let service = MediaControlService::new(Some("Rust Player"));
    assert_eq!(
        service.definition().uuid,
        bumble::Uuid::from_16_bits(MEDIA_CONTROL_SERVICE)
    );
    let mut server = GattServer::from_definitions(vec![service.definition()]).unwrap();
    let handles = service.bind(&mut server).unwrap();
    let mut transport = EncryptedTransport(&mut server);
    let mut client = GattClient::new();
    let proxy = MediaControlServiceProxy::discover(&mut client, &mut transport)
        .unwrap()
        .unwrap();
    proxy
        .subscribe_characteristics(&mut client, &mut transport)
        .unwrap();

    for (handle, value, expected) in [
        (
            handles.media_state,
            vec![MediaState::PLAYING.0],
            MediaControlEvent::MediaState(MediaState::PLAYING),
        ),
        (
            handles.track_changed,
            Vec::new(),
            MediaControlEvent::TrackChanged,
        ),
        (
            handles.track_title,
            b"My Song".to_vec(),
            MediaControlEvent::TrackTitle("My Song".into()),
        ),
        (
            handles.track_duration,
            1000i32.to_le_bytes().to_vec(),
            MediaControlEvent::TrackDuration(1000),
        ),
        (
            handles.track_position,
            (-5i32).to_le_bytes().to_vec(),
            MediaControlEvent::TrackPosition(-5),
        ),
    ] {
        let notification = transport.0.notify(handle, value.clone());
        assert!(client.on_notification(&notification).unwrap());
        assert_eq!(
            proxy.event_from_notification(handle, &value).unwrap(),
            expected
        );
    }

    assert!(proxy
        .event_from_notification(handles.track_duration, &[1, 2, 3])
        .is_err());
    assert!(proxy
        .control_result(
            MediaControlPointOpcode::PLAY,
            MediaControlEvent::ControlPoint {
                opcode: MediaControlPointOpcode::PAUSE,
                result: MediaControlPointResultCode::SUCCESS,
            },
        )
        .is_err());
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
