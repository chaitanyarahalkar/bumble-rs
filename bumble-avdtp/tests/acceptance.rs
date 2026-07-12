use bumble_avdtp::{
    EndpointInfo, ErrorCode, MediaType, Message, MessageAssembler, MessageType,
    ServiceCapabilities, ServiceCategory, SignalIdentifier, StreamEndpointType,
};

fn transport() -> ServiceCapabilities {
    ServiceCapabilities::empty(ServiceCategory::MEDIA_TRANSPORT)
}

fn delay_reporting() -> ServiceCapabilities {
    ServiceCapabilities::empty(ServiceCategory::DELAY_REPORTING)
}

fn sbc() -> ServiceCapabilities {
    ServiceCapabilities::MediaCodec {
        media_type: MediaType::AUDIO,
        media_codec_type: 0,
        media_codec_information: vec![0x21, 0x15, 0x02, 0xFA],
    }
}

#[test]
fn all_upstream_signaling_messages_have_pinned_payloads_and_round_trip() {
    use Message::*;

    let capabilities = vec![transport(), sbc(), delay_reporting()];
    let transport_only = vec![transport()];
    let bad_seid = ErrorCode::BAD_ACP_SEID;
    let cases = vec![
        (DiscoverCommand, vec![]),
        (
            DiscoverResponse {
                endpoints: vec![EndpointInfo {
                    seid: 1,
                    in_use: true,
                    media_type: MediaType::AUDIO,
                    endpoint_type: StreamEndpointType::SINK,
                }],
            },
            vec![0x06, 0x08],
        ),
        (GetCapabilitiesCommand { acp_seid: 1 }, vec![0x04]),
        (
            GetCapabilitiesResponse {
                capabilities: capabilities.clone(),
            },
            vec![
                0x01, 0x00, 0x07, 0x06, 0x00, 0x00, 0x21, 0x15, 0x02, 0xFA, 0x08, 0x00,
            ],
        ),
        (
            GetCapabilitiesReject {
                error_code: bad_seid,
            },
            vec![0x12],
        ),
        (GetAllCapabilitiesCommand { acp_seid: 1 }, vec![0x04]),
        (
            GetAllCapabilitiesResponse {
                capabilities: transport_only.clone(),
            },
            vec![0x01, 0x00],
        ),
        (
            GetAllCapabilitiesReject {
                error_code: bad_seid,
            },
            vec![0x12],
        ),
        (
            SetConfigurationCommand {
                acp_seid: 1,
                int_seid: 2,
                capabilities: transport_only.clone(),
            },
            vec![0x04, 0x08, 0x01, 0x00],
        ),
        (SetConfigurationResponse, vec![]),
        (
            SetConfigurationReject {
                service_category: ServiceCategory::MEDIA_TRANSPORT,
                error_code: ErrorCode::UNSUPPORTED_CONFIGURATION,
            },
            vec![0x01, 0x29],
        ),
        (GetConfigurationCommand { acp_seid: 1 }, vec![0x04]),
        (
            GetConfigurationResponse {
                capabilities: transport_only.clone(),
            },
            vec![0x01, 0x00],
        ),
        (
            GetConfigurationReject {
                error_code: bad_seid,
            },
            vec![0x12],
        ),
        (
            ReconfigureCommand {
                acp_seid: 1,
                capabilities: transport_only,
            },
            vec![0x04, 0x01, 0x00],
        ),
        (ReconfigureResponse, vec![]),
        (
            ReconfigureReject {
                service_category: ServiceCategory::MEDIA_TRANSPORT,
                error_code: ErrorCode::UNSUPPORTED_CONFIGURATION,
            },
            vec![0x01, 0x29],
        ),
        (OpenCommand { acp_seid: 1 }, vec![0x04]),
        (OpenResponse, vec![]),
        (
            OpenReject {
                error_code: bad_seid,
            },
            vec![0x12],
        ),
        (
            StartCommand {
                acp_seids: vec![1, 2],
            },
            vec![0x04, 0x08],
        ),
        (StartResponse, vec![]),
        (
            StartReject {
                acp_seid: 1,
                error_code: ErrorCode::BAD_STATE,
            },
            vec![0x04, 0x31],
        ),
        (CloseCommand { acp_seid: 1 }, vec![0x04]),
        (CloseResponse, vec![]),
        (
            CloseReject {
                error_code: bad_seid,
            },
            vec![0x12],
        ),
        (
            SuspendCommand {
                acp_seids: vec![1, 2],
            },
            vec![0x04, 0x08],
        ),
        (SuspendResponse, vec![]),
        (
            SuspendReject {
                acp_seid: 1,
                error_code: ErrorCode::BAD_STATE,
            },
            vec![0x04, 0x31],
        ),
        (AbortCommand { acp_seid: 1 }, vec![0x04]),
        (AbortResponse, vec![]),
        (
            SecurityControlCommand {
                acp_seid: 1,
                data: b"foo".to_vec(),
            },
            vec![0x04, b'f', b'o', b'o'],
        ),
        (SecurityControlResponse, vec![]),
        (
            SecurityControlReject {
                error_code: bad_seid,
            },
            vec![0x12],
        ),
        (GeneralReject, vec![]),
        (
            DelayReportCommand {
                acp_seid: 1,
                delay: 100,
            },
            vec![0x04, 0x00, 0x64],
        ),
        (DelayReportResponse, vec![]),
        (
            DelayReportReject {
                error_code: bad_seid,
            },
            vec![0x12],
        ),
    ];

    assert_eq!(cases.len(), 38);
    for (message, expected_payload) in cases {
        assert_eq!(message.payload().unwrap(), expected_payload, "{message:?}");
        assert_eq!(
            Message::parse(
                message.signal_identifier(),
                message.message_type(),
                &expected_payload,
            )
            .unwrap(),
            message
        );
    }
}

#[test]
fn fragmented_pdus_are_pinned_and_reassembled() {
    let message = Message::SecurityControlCommand {
        acp_seid: 1,
        data: (0..20).collect(),
    };
    let pdus = message.encode_pdus(5, 8).unwrap();
    assert_eq!(
        pdus,
        [
            vec![0x54, 0x0B, 0x04, 0x04, 0, 1, 2, 3],
            vec![0x58, 4, 5, 6, 7, 8, 9, 10],
            vec![0x58, 11, 12, 13, 14, 15, 16, 17],
            vec![0x5C, 18, 19],
        ]
    );

    let mut assembler = MessageAssembler::default();
    for pdu in &pdus[..3] {
        assert_eq!(assembler.push(pdu).unwrap(), None);
    }
    assert_eq!(assembler.push(&pdus[3]).unwrap(), Some((5, message)));
}

#[test]
fn truncated_and_malformed_remote_pdus_are_dropped_without_panicking() {
    let mut assembler = MessageAssembler::default();
    for pdu in [&[][..], &[0x00], &[0x04], &[0x44, 0x10]] {
        assert_eq!(assembler.push(pdu).unwrap(), None);
    }

    // A capability claims four bytes but carries only one.
    assert!(ServiceCapabilities::parse_all(&[0x07, 0x04, 0x00]).is_err());

    // Unknown messages stay lossless for future/vendor signal identifiers.
    let unknown = Message::parse(SignalIdentifier(0x3F), MessageType::Command, &[1, 2, 3]).unwrap();
    assert_eq!(unknown.payload().unwrap(), [1, 2, 3]);
}
