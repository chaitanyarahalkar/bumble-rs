use bumble_avc::{CommandType, OperationId, ResponseCode};
use bumble_avctp::{L2capProtocol, Message, AVCTP_PSM};
use bumble_avrcp::{
    BasicDelegate, Capability, CapabilityId, CharacterSetId, Command, Error, Event, EventId,
    Runtime, RuntimeEvent, StatusCode, AVRCP_PID,
};
use bumble_l2cap::{ChannelManager, ClassicChannelSpec};

fn relay(left: &mut ChannelManager, right: &mut ChannelManager) -> usize {
    let mut count = 0;
    while let Some(pdu) = left.poll_outbound() {
        right.process_pdu(pdu).unwrap();
        count += 1;
    }
    count
}

fn setup() -> (ChannelManager, L2capProtocol, ChannelManager, L2capProtocol) {
    let mut controller_manager = ChannelManager::new();
    let mut target_manager = ChannelManager::new();
    target_manager
        .register_server(Some(AVCTP_PSM.into()), ClassicChannelSpec { mtu: 48 })
        .unwrap();
    let controller_cid = controller_manager
        .connect(AVCTP_PSM.into(), ClassicChannelSpec { mtu: 48 })
        .unwrap();
    for _ in 0..32 {
        let count = relay(&mut controller_manager, &mut target_manager)
            + relay(&mut target_manager, &mut controller_manager);
        if count == 0 {
            break;
        }
    }
    let target_cid = target_manager.poll_accepted_channel().unwrap();
    let controller = L2capProtocol::new(controller_cid, &controller_manager).unwrap();
    let mut target = L2capProtocol::new(target_cid, &target_manager).unwrap();
    target.register_pid(AVRCP_PID);
    (controller_manager, controller, target_manager, target)
}

fn send_all(
    protocol: &L2capProtocol,
    manager: &mut ChannelManager,
    messages: impl IntoIterator<Item = Message>,
) {
    for message in messages {
        protocol.send(manager, &message).unwrap();
    }
}

fn process<D: bumble_avrcp::Delegate>(
    protocol: &mut L2capProtocol,
    manager: &mut ChannelManager,
    runtime: &mut Runtime<D>,
) -> Vec<RuntimeEvent> {
    protocol.poll(manager).unwrap();
    let mut observed = Vec::new();
    for message in protocol.take_messages() {
        for event in runtime.handle_message(message).unwrap() {
            if let RuntimeEvent::Send(message) = &event {
                protocol.send(manager, message).unwrap();
            }
            observed.push(event);
        }
    }
    observed
}

#[test]
fn typed_transactions_cross_both_fragmentation_layers() {
    let (
        mut controller_manager,
        mut controller_transport,
        mut target_manager,
        mut target_transport,
    ) = setup();
    let mut controller = Runtime::new(40);
    let delegate = BasicDelegate {
        supported_company_ids: (0..20).map(|index| 0x100000 + index).collect(),
        ..BasicDelegate::default()
    };
    let expected_capabilities: Vec<_> = delegate
        .supported_company_ids
        .iter()
        .copied()
        .map(Capability::CompanyId)
        .collect();
    let mut target = Runtime::with_delegate(delegate, 40);

    let messages = controller
        .begin_command(
            CommandType::STATUS,
            &Command::GetCapabilities {
                capability_id: CapabilityId::COMPANY_ID,
            },
        )
        .unwrap();
    assert_eq!(messages.len(), 1);
    send_all(&controller_transport, &mut controller_manager, messages);
    assert_eq!(relay(&mut controller_manager, &mut target_manager), 1);

    let target_events = process(&mut target_transport, &mut target_manager, &mut target);
    assert!(target_events
        .iter()
        .any(|event| matches!(event, RuntimeEvent::Command { .. })));
    assert!(relay(&mut target_manager, &mut controller_manager) > 2);

    let events = process(
        &mut controller_transport,
        &mut controller_manager,
        &mut controller,
    );
    assert_eq!(controller.pending_count(), 0);
    assert!(events.iter().any(|event| matches!(
        event,
        RuntimeEvent::Response {
            response_code: ResponseCode::IMPLEMENTED_OR_STABLE,
            response: bumble_avrcp::Response::GetCapabilities { capabilities, .. },
            ..
        } if capabilities == &expected_capabilities
    )));

    let messages = controller
        .begin_command(
            CommandType::CONTROL,
            &Command::Search {
                character_set_id: CharacterSetId::UTF_8,
                search_string: "missing".into(),
            },
        )
        .unwrap();
    send_all(&controller_transport, &mut controller_manager, messages);
    relay(&mut controller_manager, &mut target_manager);
    process(&mut target_transport, &mut target_manager, &mut target);
    relay(&mut target_manager, &mut controller_manager);
    let rejected = process(
        &mut controller_transport,
        &mut controller_manager,
        &mut controller,
    );
    assert!(rejected.iter().any(|event| matches!(
        event,
        RuntimeEvent::Response {
            response_code: ResponseCode::REJECTED,
            response: bumble_avrcp::Response::Rejected {
                status: StatusCode::INVALID_PARAMETER,
                ..
            },
            ..
        }
    )));
}

#[test]
fn interim_notification_keeps_label_until_changed() {
    let (
        mut controller_manager,
        mut controller_transport,
        mut target_manager,
        mut target_transport,
    ) = setup();
    let mut controller = Runtime::new(5);
    let delegate = BasicDelegate {
        supported_events: vec![EventId::VOLUME_CHANGED],
        volume: 42,
        ..BasicDelegate::default()
    };
    let mut target = Runtime::with_delegate(delegate, 5);

    let messages = controller
        .begin_command(
            CommandType::NOTIFY,
            &Command::RegisterNotification {
                event_id: EventId::VOLUME_CHANGED,
                playback_interval: 0,
            },
        )
        .unwrap();
    send_all(&controller_transport, &mut controller_manager, messages);
    relay(&mut controller_manager, &mut target_manager);
    process(&mut target_transport, &mut target_manager, &mut target);
    relay(&mut target_manager, &mut controller_manager);
    let interim = process(
        &mut controller_transport,
        &mut controller_manager,
        &mut controller,
    );
    assert_eq!(controller.pending_count(), 1);
    assert!(interim.iter().any(|event| matches!(
        event,
        RuntimeEvent::Response {
            response_code: ResponseCode::INTERIM,
            response: bumble_avrcp::Response::RegisterNotification {
                event: Event::VolumeChanged { volume: 42 }
            },
            ..
        }
    )));

    target.delegate_mut().volume = 55;
    let changed = target.notify(Event::VolumeChanged { volume: 55 }).unwrap();
    send_all(&target_transport, &mut target_manager, changed);
    relay(&mut target_manager, &mut controller_manager);
    let final_events = process(
        &mut controller_transport,
        &mut controller_manager,
        &mut controller,
    );
    assert_eq!(controller.pending_count(), 0);
    assert!(final_events.iter().any(|event| matches!(
        event,
        RuntimeEvent::Response {
            response_code: ResponseCode::CHANGED,
            response: bumble_avrcp::Response::RegisterNotification {
                event: Event::VolumeChanged { volume: 55 }
            },
            ..
        }
    )));
}

#[test]
fn pass_through_round_trips_and_labels_are_bounded() {
    let (
        mut controller_manager,
        mut controller_transport,
        mut target_manager,
        mut target_transport,
    ) = setup();
    let mut controller = Runtime::new(32);
    let mut target = Runtime::new(32);
    let command = controller
        .begin_pass_through(OperationId::PLAY, true, vec![])
        .unwrap();
    send_all(&controller_transport, &mut controller_manager, [command]);
    relay(&mut controller_manager, &mut target_manager);
    let target_events = process(&mut target_transport, &mut target_manager, &mut target);
    assert!(target_events.iter().any(|event| matches!(
        event,
        RuntimeEvent::PassThroughCommand {
            operation_id: OperationId::PLAY,
            pressed: true,
            ..
        }
    )));
    assert_eq!(
        target.delegate().key_events,
        [(OperationId::PLAY, true, vec![])]
    );
    relay(&mut target_manager, &mut controller_manager);
    let controller_events = process(
        &mut controller_transport,
        &mut controller_manager,
        &mut controller,
    );
    assert!(controller_events.iter().any(|event| matches!(
        event,
        RuntimeEvent::PassThroughResponse {
            response_code: ResponseCode::ACCEPTED,
            operation_id: OperationId::PLAY,
            ..
        }
    )));

    for _ in 0..16 {
        controller
            .begin_command(CommandType::STATUS, &Command::GetPlayStatus)
            .unwrap();
    }
    assert_eq!(
        controller.begin_command(CommandType::STATUS, &Command::GetPlayStatus),
        Err(Error::NoTransactionLabels)
    );
}
