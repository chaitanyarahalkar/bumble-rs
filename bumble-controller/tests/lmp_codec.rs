use bumble_controller::lmp::{CodecError, Opcode, Packet};

#[test]
fn registered_packets_match_upstream_payload_oracles() {
    let cases = vec![
        (
            Packet::Accepted {
                response_opcode: Opcode::LMP_HOST_CONNECTION_REQ,
            },
            vec![0x03, 0x33],
        ),
        (
            Packet::NotAccepted {
                response_opcode: Opcode::LMP_SWITCH_REQ,
                error_code: 0x0C,
            },
            vec![0x04, 0x13, 0x0C],
        ),
        (
            Packet::AcceptedExt {
                response_opcode: Opcode::LMP_FEATURES_REQ_EXT,
            },
            vec![0x7F, 0x01, 0x7F, 0x03],
        ),
        (
            Packet::NotAcceptedExt {
                response_opcode: Opcode::LMP_FEATURES_REQ_EXT,
                error_code: 0x1A,
            },
            vec![0x7F, 0x02, 0x7F, 0x03, 0x1A],
        ),
        (
            Packet::AuRand {
                random_number: [
                    0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A, 0x0B, 0x0C,
                    0x0D, 0x0E, 0x0F,
                ],
            },
            vec![
                0x0B, 0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A, 0x0B, 0x0C,
                0x0D, 0x0E, 0x0F,
            ],
        ),
        (Packet::Detach { error_code: 0x13 }, vec![0x07, 0x13]),
        (
            Packet::EscoLinkReq {
                esco_handle: 1,
                esco_lt_addr: 2,
                timing_control_flags: 3,
                d_esco: 4,
                t_esco: 5,
                w_esco: 6,
                esco_packet_type_c_to_p: 7,
                esco_packet_type_p_to_c: 8,
                packet_length_c_to_p: 0x1234,
                packet_length_p_to_c: 0x5678,
                air_mode: 9,
                negotiation_state: 10,
            },
            vec![
                0x7F, 0x0C, 1, 2, 3, 4, 5, 6, 7, 8, 0x34, 0x12, 0x78, 0x56, 9, 10,
            ],
        ),
        (Packet::HostConnectionReq, vec![0x33]),
        (
            Packet::RemoveEscoLinkReq {
                esco_handle: 1,
                error_code: 0x13,
            },
            vec![0x7F, 0x0D, 1, 0x13],
        ),
        (
            Packet::RemoveScoLinkReq {
                sco_handle: 2,
                error_code: 0x14,
            },
            vec![0x2C, 2, 0x14],
        ),
        (
            Packet::ScoLinkReq {
                sco_handle: 1,
                timing_control_flags: 2,
                d_sco: 3,
                t_sco: 4,
                sco_packet: 5,
                air_mode: 6,
            },
            vec![0x2B, 1, 2, 3, 4, 5, 6],
        ),
        (
            Packet::SwitchReq {
                switch_instant: 0x1234_5678,
            },
            vec![0x13, 0x78, 0x56, 0x34, 0x12],
        ),
        (
            Packet::NameReq {
                name_offset: 0x1234,
            },
            vec![0x01, 0x34, 0x12],
        ),
        (
            Packet::NameRes {
                name_offset: 1,
                name_length: 3,
                name_fragment: b"abc".to_vec(),
            },
            vec![0x02, 0x01, 0x00, 0x03, 0x00, 0x00, b'a', b'b', b'c'],
        ),
        (
            Packet::FeaturesReq {
                features: [0, 1, 2, 3, 4, 5, 6, 7],
            },
            vec![0x27, 0, 1, 2, 3, 4, 5, 6, 7],
        ),
        (
            Packet::FeaturesRes {
                features: [8, 9, 10, 11, 12, 13, 14, 15],
            },
            vec![0x28, 8, 9, 10, 11, 12, 13, 14, 15],
        ),
        (
            Packet::FeaturesReqExt {
                features_page: 2,
                features: [0, 1, 2, 3, 4, 5, 6, 7],
            },
            vec![0x7F, 0x03, 2, 0, 1, 2, 3, 4, 5, 6, 7],
        ),
        (
            Packet::FeaturesResExt {
                features_page: 2,
                max_features_page: 3,
                features: [8, 9, 10, 11, 12, 13, 14, 15],
            },
            vec![0x7F, 0x04, 2, 3, 8, 9, 10, 11, 12, 13, 14, 15],
        ),
    ];

    assert_eq!(cases.len(), 18);
    for (packet, expected) in cases {
        assert_eq!(packet.to_bytes().unwrap(), expected);
        assert_eq!(Packet::from_bytes(&expected).unwrap(), packet);
    }
}

#[test]
fn open_opcodes_and_unknown_payloads_round_trip() {
    for packet in [
        Packet::Unknown {
            opcode: Opcode(0x55),
            payload: vec![1, 2, 3],
        },
        Packet::Unknown {
            opcode: Opcode(0x7C99),
            payload: vec![4, 5],
        },
        Packet::Unknown {
            opcode: Opcode(0x7F7E),
            payload: vec![],
        },
    ] {
        let bytes = packet.to_bytes().unwrap();
        assert_eq!(Packet::from_bytes(&bytes).unwrap(), packet);
    }
}

#[test]
fn malformed_packets_report_bounded_errors() {
    assert!(matches!(
        Packet::from_bytes(&[]),
        Err(CodecError::Truncated { .. })
    ));
    assert!(matches!(
        Packet::from_bytes(&[0x7F]),
        Err(CodecError::Truncated { .. })
    ));
    assert_eq!(
        Packet::from_bytes(&[0x07]),
        Err(CodecError::InvalidLength {
            opcode: Opcode::LMP_DETACH,
            expected: 1,
            actual: 0,
        })
    );
    assert!(matches!(
        Packet::from_bytes(&[0x02, 0, 0, 0, 0]),
        Err(CodecError::Truncated { .. })
    ));
    assert_eq!(
        Packet::NameRes {
            name_offset: 0,
            name_length: 0x0100_0000,
            name_fragment: vec![],
        }
        .to_bytes(),
        Err(CodecError::NameLengthOutOfRange(0x0100_0000))
    );
}
