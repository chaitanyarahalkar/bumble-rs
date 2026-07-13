use bumble_gatt::{GattClient, GattServer};
use bumble_profiles::ams::{
    AmsClient, AmsEvent, AmsService, AmsServiceProxy, EntityId, PlayerAttributeId, RemoteCommandId,
    TrackAttributeId,
};
use bumble_profiles::ancs::{
    ActionId, AncsClient, AncsCommand, AncsDate, AncsResponse, AncsResponseAssembler, AncsService,
    AncsServiceProxy, AppAttributeId, CategoryId, EventFlags, EventId, Notification,
    NotificationAttributeId, NotificationAttributeRequest, NotificationAttributeValue,
};

#[test]
fn live_ams_commands_observation_and_truncated_updates() {
    let service = AmsService::default();
    let mut server = GattServer::from_definitions(vec![service.definition()]).unwrap();
    let handles = service.bind(&mut server).unwrap();
    let mut gatt_client = GattClient::new();
    let proxy = AmsServiceProxy::discover(&mut gatt_client, &mut server)
        .unwrap()
        .unwrap();
    proxy.start(&mut gatt_client, &mut server).unwrap();

    proxy
        .command(&mut gatt_client, &mut server, RemoteCommandId::PLAY)
        .unwrap();
    assert_eq!(service.take_command().unwrap(), Some(RemoteCommandId::PLAY));
    proxy
        .observe(
            &mut gatt_client,
            &mut server,
            EntityId::TRACK,
            &[TrackAttributeId::ARTIST.0, TrackAttributeId::TITLE.0],
        )
        .unwrap();
    assert_eq!(
        service.take_observation().unwrap(),
        Some((
            EntityId::TRACK,
            vec![TrackAttributeId::ARTIST.0, TrackAttributeId::TITLE.0]
        ))
    );

    service
        .set_supported_commands(&[
            RemoteCommandId::PLAY,
            RemoteCommandId::PAUSE,
            RemoteCommandId::NEXT_TRACK,
        ])
        .unwrap();
    service
        .update_entity(
            EntityId::PLAYER,
            PlayerAttributeId::NAME.0,
            b"Bumble Player".to_vec(),
            Some(4),
        )
        .unwrap();
    service
        .update_entity(
            EntityId::PLAYER,
            PlayerAttributeId::PLAYBACK_INFO.0,
            b"1,1.0,12.5".to_vec(),
            None,
        )
        .unwrap();
    let notifications = service.take_pending_notifications(handles).unwrap();
    let mut client = AmsClient::default();
    for (handle, value) in notifications {
        assert!(gatt_client
            .on_notification(&server.notify(handle, value.clone()))
            .unwrap());
        if handle == handles.remote_command {
            assert!(matches!(
                client.on_remote_command_notification(&value),
                AmsEvent::SupportedCommands(_)
            ));
        } else {
            client
                .on_entity_update_notification(&proxy, &mut gatt_client, &mut server, &value)
                .unwrap();
        }
    }
    assert_eq!(client.player_name, "Bumble Player");
    assert_eq!(client.player_playback_info.playback_state.0, 1);
    assert_eq!(client.player_playback_info.elapsed_time, 12.5);
    assert!(client
        .supported_commands
        .contains(&RemoteCommandId::NEXT_TRACK));
    proxy.stop(&mut gatt_client, &mut server).unwrap();
}

#[test]
fn ancs_notification_and_commands_are_byte_exact() {
    let notification = Notification {
        event_id: EventId::NOTIFICATION_ADDED,
        event_flags: EventFlags::IMPORTANT | EventFlags::POSITIVE_ACTION,
        category_id: CategoryId::EMAIL,
        category_count: 2,
        notification_uid: 0x1234_5678,
    };
    assert_eq!(
        notification.to_bytes(),
        [0, 0x0A, 6, 2, 0x78, 0x56, 0x34, 0x12]
    );
    assert_eq!(
        Notification::from_bytes(&notification.to_bytes()).unwrap(),
        notification
    );

    let commands = vec![
        AncsCommand::GetNotificationAttributes {
            notification_uid: notification.notification_uid,
            attributes: vec![
                NotificationAttributeRequest::with_max_length(NotificationAttributeId::TITLE, 100)
                    .unwrap(),
                NotificationAttributeRequest::new(NotificationAttributeId::MESSAGE_SIZE),
            ],
        },
        AncsCommand::GetAppAttributes {
            app_identifier: "com.example.mail".into(),
            attributes: vec![AppAttributeId::DISPLAY_NAME],
        },
        AncsCommand::PerformNotificationAction {
            notification_uid: notification.notification_uid,
            action: ActionId::POSITIVE,
        },
    ];
    for command in commands {
        let bytes = command.to_bytes().unwrap();
        assert_eq!(AncsCommand::from_bytes(&bytes).unwrap(), command);
    }
    assert!(AncsCommand::from_bytes(&[2, 1]).is_err());
    assert!(Notification::from_bytes(&[0; 7]).is_err());
}

fn tuple(target: &mut Vec<u8>, id: u8, value: &[u8]) {
    target.push(id);
    target.extend_from_slice(&(value.len() as u16).to_le_bytes());
    target.extend_from_slice(value);
}

#[test]
fn ancs_fragmented_notification_and_app_responses_are_typed() {
    let uid: u32 = 0x1234_5678;
    let mut response = vec![0];
    response.extend_from_slice(&uid.to_le_bytes());
    tuple(&mut response, NotificationAttributeId::TITLE.0, b"Hello");
    tuple(
        &mut response,
        NotificationAttributeId::MESSAGE_SIZE.0,
        b"42",
    );
    tuple(
        &mut response,
        NotificationAttributeId::DATE.0,
        b"20260713T142530",
    );
    let mut assembler = AncsResponseAssembler::notification(uid, 3);
    assert!(assembler.push(&response[..7]).unwrap().is_none());
    assert!(assembler.push(&response[7..15]).unwrap().is_none());
    let parsed = assembler.push(&response[15..]).unwrap().unwrap();
    let AncsResponse::NotificationAttributes { attributes, .. } = parsed else {
        panic!("wrong response")
    };
    assert_eq!(
        attributes[1].value,
        NotificationAttributeValue::MessageSize(42)
    );
    assert_eq!(
        attributes[2].value,
        NotificationAttributeValue::Date(AncsDate {
            year: 2026,
            month: 7,
            day: 13,
            hour: 14,
            minute: 25,
            second: 30,
        })
    );

    let mut app_response = vec![1];
    app_response.extend_from_slice(b"com.example.mail\0");
    tuple(&mut app_response, 0, b"Mail");
    let mut app = AncsResponseAssembler::app("com.example.mail", 1);
    assert!(app.push(&app_response[..5]).unwrap().is_none());
    assert!(matches!(
        app.push(&app_response[5..]).unwrap(),
        Some(AncsResponse::AppAttributes { attributes, .. }) if attributes[0].value == "Mail"
    ));
}

#[test]
fn live_ancs_client_receives_notifications_and_fragmented_data() {
    let service = AncsService::default();
    let mut server = GattServer::from_definitions(vec![service.definition()]).unwrap();
    let handles = service.bind(&mut server).unwrap();
    let mut gatt_client = GattClient::new();
    let proxy = AncsServiceProxy::discover(&mut gatt_client, &mut server)
        .unwrap()
        .unwrap();
    let mut client = AncsClient::default();
    client.start(&proxy, &mut gatt_client, &mut server).unwrap();

    let uid: u32 = 0x1234_5678;
    let requests =
        vec![
            NotificationAttributeRequest::with_max_length(NotificationAttributeId::TITLE, 64)
                .unwrap(),
        ];
    client
        .begin_notification_attributes(&proxy, &mut gatt_client, &mut server, uid, requests.clone())
        .unwrap();
    assert_eq!(
        service.take_command().unwrap(),
        Some(AncsCommand::GetNotificationAttributes {
            notification_uid: uid,
            attributes: requests,
        })
    );

    let notification = Notification {
        event_id: EventId::NOTIFICATION_ADDED,
        event_flags: EventFlags::IMPORTANT,
        category_id: CategoryId::EMAIL,
        category_count: 1,
        notification_uid: uid,
    };
    service.notify(notification).unwrap();
    let mut response = vec![0];
    response.extend_from_slice(&uid.to_le_bytes());
    tuple(&mut response, NotificationAttributeId::TITLE.0, b"Inbox");
    service.send_data(response[..4].to_vec()).unwrap();
    service.send_data(response[4..].to_vec()).unwrap();

    let pending = service.take_pending_notifications(handles).unwrap();
    let mut parsed_notification = None;
    let mut parsed_response = None;
    for (handle, value) in pending {
        assert!(gatt_client
            .on_notification(&server.notify(handle, value.clone()))
            .unwrap());
        if handle == handles.notification_source {
            parsed_notification = Some(proxy.notification_from_value(handle, &value).unwrap());
        } else {
            assert!(proxy.is_data_source(handle));
            if let Some(response) = client.on_data(&value).unwrap() {
                parsed_response = Some(response);
            }
        }
    }
    assert_eq!(parsed_notification, Some(notification));
    assert!(matches!(
        parsed_response,
        Some(AncsResponse::NotificationAttributes { attributes, .. })
            if attributes[0].value == NotificationAttributeValue::Text("Inbox".into())
    ));

    client
        .perform_action(
            &proxy,
            &mut gatt_client,
            &mut server,
            uid,
            ActionId::NEGATIVE,
        )
        .unwrap();
    assert_eq!(
        service.take_command().unwrap(),
        Some(AncsCommand::PerformNotificationAction {
            notification_uid: uid,
            action: ActionId::NEGATIVE,
        })
    );
    client.stop(&proxy, &mut gatt_client, &mut server).unwrap();
}
