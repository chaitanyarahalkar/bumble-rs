use bumble_avdtp::session::{Session, SessionEvent};
use bumble_avdtp::{
    ErrorCode, MediaType, Message, ServiceCapabilities, ServiceCategory, State, StreamEndpointType,
};

fn capabilities() -> Vec<ServiceCapabilities> {
    vec![
        ServiceCapabilities::empty(ServiceCategory::MEDIA_TRANSPORT),
        ServiceCapabilities::MediaCodec {
            media_type: MediaType::AUDIO,
            media_codec_type: 0,
            media_codec_information: vec![0x21, 0x15, 0x02, 0x35],
        },
    ]
}

#[test]
fn endpoint_discovery_configuration_and_stream_lifecycle() {
    let mut session = Session::default();
    let sink = session.add_endpoint(MediaType::AUDIO, StreamEndpointType::SINK, capabilities());

    assert_eq!(
        session.handle_command(Message::DiscoverCommand),
        Message::DiscoverResponse {
            endpoints: vec![session.endpoint(sink).unwrap().info()]
        }
    );
    assert_eq!(
        session.handle_command(Message::GetAllCapabilitiesCommand { acp_seid: sink }),
        Message::GetAllCapabilitiesResponse {
            capabilities: capabilities()
        }
    );

    assert_eq!(
        session.handle_command(Message::SetConfigurationCommand {
            acp_seid: sink,
            int_seid: 2,
            capabilities: capabilities(),
        }),
        Message::SetConfigurationResponse
    );
    assert_eq!(session.endpoint(sink).unwrap().state, State::CONFIGURED);
    assert!(session.endpoint(sink).unwrap().in_use());
    assert_eq!(
        session.handle_command(Message::GetConfigurationCommand { acp_seid: sink }),
        Message::GetConfigurationResponse {
            capabilities: capabilities()
        }
    );
    assert_eq!(
        session.handle_command(Message::OpenCommand { acp_seid: sink }),
        Message::OpenResponse
    );
    assert_eq!(session.endpoint(sink).unwrap().state, State::OPEN);
    assert_eq!(
        session.handle_command(Message::StartCommand {
            acp_seids: vec![sink]
        }),
        Message::StartResponse
    );
    assert_eq!(session.endpoint(sink).unwrap().state, State::STREAMING);

    assert_eq!(
        session.handle_command(Message::DelayReportCommand {
            acp_seid: sink,
            delay: 120,
        }),
        Message::DelayReportResponse
    );
    assert_eq!(
        session.handle_command(Message::SecurityControlCommand {
            acp_seid: sink,
            data: b"key".to_vec(),
        }),
        Message::SecurityControlResponse
    );
    assert_eq!(
        session.handle_command(Message::SuspendCommand {
            acp_seids: vec![sink]
        }),
        Message::SuspendResponse
    );
    assert_eq!(session.endpoint(sink).unwrap().state, State::OPEN);

    let replacement = vec![ServiceCapabilities::empty(ServiceCategory::MEDIA_TRANSPORT)];
    assert_eq!(
        session.handle_command(Message::ReconfigureCommand {
            acp_seid: sink,
            capabilities: replacement.clone(),
        }),
        Message::ReconfigureResponse
    );
    assert_eq!(session.endpoint(sink).unwrap().configuration, replacement);
    assert_eq!(
        session.handle_command(Message::CloseCommand { acp_seid: sink }),
        Message::CloseResponse
    );
    assert_eq!(session.endpoint(sink).unwrap().state, State::IDLE);
    assert!(!session.endpoint(sink).unwrap().in_use());

    assert_eq!(
        session.take_events(),
        [
            SessionEvent::Configured {
                seid: sink,
                remote_seid: 2,
            },
            SessionEvent::Opened { seid: sink },
            SessionEvent::Started { seid: sink },
            SessionEvent::DelayReport {
                seid: sink,
                delay: 120,
            },
            SessionEvent::SecurityControl {
                seid: sink,
                data: b"key".to_vec(),
            },
            SessionEvent::Suspended { seid: sink },
            SessionEvent::Reconfigured { seid: sink },
            SessionEvent::Closed { seid: sink },
        ]
    );
}

#[test]
fn invalid_state_and_multi_endpoint_commands_are_atomic() {
    let mut session = Session::default();
    let first = session.add_endpoint(MediaType::AUDIO, StreamEndpointType::SINK, capabilities());
    let second = session.add_endpoint(MediaType::AUDIO, StreamEndpointType::SINK, capabilities());

    assert_eq!(
        session.handle_command(Message::OpenCommand { acp_seid: 63 }),
        Message::OpenReject {
            error_code: ErrorCode::BAD_ACP_SEID,
        }
    );
    assert_eq!(
        session.handle_command(Message::StartCommand {
            acp_seids: vec![first, second],
        }),
        Message::StartReject {
            acp_seid: first,
            error_code: ErrorCode::BAD_STATE,
        }
    );
    assert_eq!(session.endpoint(first).unwrap().state, State::IDLE);
    assert_eq!(session.endpoint(second).unwrap().state, State::IDLE);

    for (local, remote) in [(first, 10), (second, 11)] {
        assert_eq!(
            session.handle_command(Message::SetConfigurationCommand {
                acp_seid: local,
                int_seid: remote,
                capabilities: capabilities(),
            }),
            Message::SetConfigurationResponse
        );
        assert_eq!(
            session.handle_command(Message::OpenCommand { acp_seid: local }),
            Message::OpenResponse
        );
    }
    assert_eq!(
        session.handle_command(Message::StartCommand {
            acp_seids: vec![first, 63, second],
        }),
        Message::StartReject {
            acp_seid: 63,
            error_code: ErrorCode::BAD_ACP_SEID,
        }
    );
    assert_eq!(session.endpoint(first).unwrap().state, State::OPEN);
    assert_eq!(session.endpoint(second).unwrap().state, State::OPEN);

    assert_eq!(
        session.handle_command(Message::SetConfigurationCommand {
            acp_seid: first,
            int_seid: 12,
            capabilities: capabilities(),
        }),
        Message::SetConfigurationReject {
            service_category: ServiceCategory(0),
            error_code: ErrorCode::SEP_IN_USE,
        }
    );

    // Abort is idempotent and accepted even for an unknown endpoint upstream.
    assert_eq!(
        session.handle_command(Message::AbortCommand { acp_seid: 63 }),
        Message::AbortResponse
    );
    assert_eq!(
        session.handle_command(Message::AbortCommand { acp_seid: first }),
        Message::AbortResponse
    );
    assert_eq!(session.endpoint(first).unwrap().state, State::IDLE);
}
