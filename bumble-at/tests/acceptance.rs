use bumble_at::{
    parse_parameters, tokenize_parameters, AtCommand, AtResponse, CommandStream, CommandSubCode,
    Parameter, ResponseStream,
};

fn value(bytes: &[u8]) -> Parameter {
    Parameter::Value(bytes.to_vec())
}

fn list(values: Vec<Parameter>) -> Parameter {
    Parameter::List(values)
}

// Ported 1:1 from google/bumble tests/at_test.py::test_tokenize_parameters.
#[test]
fn test_tokenize_parameters() {
    assert_eq!(
        tokenize_parameters(b"1, 2, 3").unwrap(),
        [
            b"1".to_vec(),
            b",".to_vec(),
            b"2".to_vec(),
            b",".to_vec(),
            b"3".to_vec()
        ]
    );
    assert_eq!(
        tokenize_parameters(b"\"1, 2, 3\"").unwrap(),
        [b"1, 2, 3".to_vec()]
    );
    assert_eq!(
        tokenize_parameters(b"(1, \"2, 3\")").unwrap(),
        [
            b"(".to_vec(),
            b"1".to_vec(),
            b",".to_vec(),
            b"2, 3".to_vec(),
            b")".to_vec(),
        ]
    );
}

// Ported 1:1 from google/bumble tests/at_test.py::test_parse_parameters.
#[test]
fn test_parse_parameters() {
    assert_eq!(
        parse_parameters(b"1, 2, 3").unwrap(),
        [value(b"1"), value(b"2"), value(b"3")]
    );
    assert_eq!(
        parse_parameters(b"1,, 3").unwrap(),
        [value(b"1"), value(b""), value(b"3")]
    );
    assert_eq!(
        parse_parameters(b"\"1, 2, 3\"").unwrap(),
        [value(b"1, 2, 3")]
    );
    assert_eq!(
        parse_parameters(b"1, (2, (3))").unwrap(),
        [
            value(b"1"),
            list(vec![value(b"2"), list(vec![value(b"3")])])
        ]
    );
    assert_eq!(
        parse_parameters(b"1, (2, \"3, 4\"), 5").unwrap(),
        [
            value(b"1"),
            list(vec![value(b"2"), value(b"3, 4")]),
            value(b"5"),
        ]
    );
}

#[test]
fn command_and_response_forms_match_upstream_hfp_parsers() {
    assert_eq!(
        AtCommand::parse(b"AT+BIND=?").unwrap(),
        AtCommand {
            code: "BIND".into(),
            sub_code: CommandSubCode::Test,
            parameters: vec![],
        }
    );
    assert_eq!(
        AtCommand::parse(b"AT+BIEV=2,100").unwrap(),
        AtCommand {
            code: "BIEV".into(),
            sub_code: CommandSubCode::Set,
            parameters: vec![value(b"2"), value(b"100")],
        }
    );
    assert_eq!(AtCommand::parse(b"ATA").unwrap().code, "A");
    assert_eq!(
        AtCommand::parse(b"ATD123456789").unwrap().parameters,
        [value(b"123456789")]
    );
    assert_eq!(
        AtResponse::parse(b"+CIND: (\"service\",(0,1)),1").unwrap(),
        AtResponse {
            code: "+CIND".into(),
            parameters: vec![
                list(vec![
                    value(b"service"),
                    list(vec![value(b"0"), value(b"1")])
                ]),
                value(b"1"),
            ],
        }
    );
}

#[test]
fn streaming_parsers_handle_fragmentation_and_coalescing() {
    let mut commands = CommandStream::default();
    assert!(commands.push(b"AT+BIE").unwrap().is_empty());
    let parsed = commands.push(b"V=2,100\rATA\r").unwrap();
    assert_eq!(parsed.len(), 2);
    assert_eq!(parsed[0].code, "BIEV");
    assert_eq!(parsed[1].code, "A");

    let mut responses = ResponseStream::default();
    assert!(responses.push(b"\r\n+CIND: 1,").unwrap().is_empty());
    let parsed = responses.push(b"0\r\n\r\nOK\r\n").unwrap();
    assert_eq!(parsed.len(), 2);
    assert_eq!(parsed[0].code, "+CIND");
    assert_eq!(parsed[0].parameters, [value(b"1"), value(b"0")]);
    assert_eq!(parsed[1].code, "OK");
}

#[test]
fn malformed_nesting_is_rejected() {
    assert!(parse_parameters(b"1)").is_err());
    assert!(parse_parameters(b"(1").is_err());
    assert!(tokenize_parameters(b"a(").is_err());
    assert!(tokenize_parameters(b"a\"b").is_err());
}
