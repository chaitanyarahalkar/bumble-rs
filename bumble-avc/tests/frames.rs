use bumble_avc::*;

#[test]
fn upstream_generic_and_extended_subunit_vectors_parse() {
    assert!(Frame::from_bytes(&hex("11480000")).is_err());

    let frame = Frame::from_bytes(&hex("014D0208")).unwrap();
    assert!(matches!(
        frame,
        Frame::Command {
            command_type: CommandType::STATUS,
            subunit_type: SubunitType::PANEL,
            subunit_id: 7,
            body: FrameBody::Raw {
                opcode: OperationCode(8),
                ..
            },
        }
    ));

    let frame = Frame::from_bytes(&hex("014DFF0108")).unwrap();
    assert!(matches!(
        frame,
        Frame::Command {
            subunit_id: 260,
            ..
        }
    ));
    assert_eq!(frame.to_bytes().unwrap(), hex("014DFF0108"));
}

#[test]
fn upstream_vendor_dependent_command_is_byte_exact() {
    let bytes = hex("0148000019581000000103");
    let frame = Frame::from_bytes(&bytes).unwrap();
    assert_eq!(
        frame,
        Frame::Command {
            command_type: CommandType::STATUS,
            subunit_type: SubunitType::PANEL,
            subunit_id: 0,
            body: FrameBody::VendorDependent {
                company_id: 0x001958,
                data: hex("1000000103"),
            },
        }
    );
    assert_eq!(frame.to_bytes().unwrap(), bytes);
}

#[test]
fn pass_through_press_release_and_operation_data_round_trip() {
    let pressed = Frame::pass_through_command(StateFlag::Pressed, OperationId::PLAY);
    let bytes = pressed.to_bytes().unwrap();
    assert_eq!(bytes, [0x00, 0x48, 0x7C, 0x44, 0x00]);
    assert_eq!(Frame::from_bytes(&bytes).unwrap(), pressed);

    let released = Frame::Command {
        command_type: CommandType::CONTROL,
        subunit_type: SubunitType::PANEL,
        subunit_id: 0,
        body: FrameBody::PassThrough {
            state: StateFlag::Released,
            operation_id: OperationId::VENDOR_UNIQUE,
            data: vec![1, 2, 3],
        },
    };
    assert_eq!(
        Frame::from_bytes(&released.to_bytes().unwrap()).unwrap(),
        released
    );
}

#[test]
fn malformed_remote_frames_return_errors() {
    assert!(Frame::from_bytes(&[]).is_err());
    assert!(Frame::from_bytes(&[0, 0x4E, 0]).is_err());
    assert!(Frame::from_bytes(&[0, 0x4D, 0]).is_err());
    assert!(Frame::from_bytes(&[0, 0x48, 0x00, 1, 2]).is_err());
    assert!(Frame::from_bytes(&[0, 0x48, 0x7C, 0x44, 3, 1]).is_err());
}

fn hex(value: &str) -> Vec<u8> {
    value
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let digit = |byte: u8| match byte {
                b'0'..=b'9' => byte - b'0',
                b'a'..=b'f' => byte - b'a' + 10,
                b'A'..=b'F' => byte - b'A' + 10,
                _ => panic!("invalid hex"),
            };
            digit(pair[0]) << 4 | digit(pair[1])
        })
        .collect()
}
