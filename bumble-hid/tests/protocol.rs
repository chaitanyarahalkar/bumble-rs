use bumble_hid::*;

#[test]
fn all_upstream_message_forms_are_byte_exact_and_round_trip() {
    let samples = [
        (
            Message::GetReport {
                report_type: ReportType::INPUT_REPORT,
                report_id: 5,
                buffer_size: None,
            },
            "4105",
        ),
        (
            Message::GetReport {
                report_type: ReportType::INPUT_REPORT,
                report_id: 5,
                buffer_size: Some(0x1234),
            },
            "49053412",
        ),
        (
            Message::SetReport {
                report_type: ReportType::OUTPUT_REPORT,
                data: vec![1, 2],
            },
            "520102",
        ),
        (Message::GetProtocol, "60"),
        (Message::SetProtocol(ProtocolMode::REPORT_PROTOCOL), "71"),
        (Message::Control(ControlCommand::SUSPEND), "13"),
        (Message::Control(ControlCommand::EXIT_SUSPEND), "14"),
        (Message::Control(ControlCommand::VIRTUAL_CABLE_UNPLUG), "15"),
        (
            Message::Data {
                report_type: ReportType::INPUT_REPORT,
                data: vec![1, 2, 3],
            },
            "a1010203",
        ),
        (Message::Handshake(Handshake::ERR_UNSUPPORTED_REQUEST), "03"),
    ];
    for (message, expected) in samples {
        let bytes = message.to_bytes().unwrap();
        assert_eq!(bytes, hex(expected), "{message:?}");
        assert_eq!(Message::from_bytes(&bytes).unwrap(), message);
    }
}

#[derive(Default)]
struct TestDelegate {
    reports: Vec<(u8, ReportType, usize, Vec<u8>)>,
    protocol: ProtocolMode,
}

impl DeviceDelegate for TestDelegate {
    fn get_report(
        &mut self,
        report_id: u8,
        _report_type: ReportType,
        _buffer_size: Option<u16>,
    ) -> GetSetStatus {
        if report_id == 7 {
            GetSetStatus::success(vec![0xAA, 0xBB])
        } else {
            GetSetStatus {
                data: vec![],
                status: GetSetReturn::REPORT_ID_NOT_FOUND,
            }
        }
    }

    fn set_report(
        &mut self,
        report_id: u8,
        report_type: ReportType,
        report_size: usize,
        data: &[u8],
    ) -> GetSetStatus {
        self.reports
            .push((report_id, report_type, report_size, data.to_vec()));
        GetSetStatus::success(vec![])
    }

    fn get_protocol(&mut self) -> GetSetStatus {
        GetSetStatus::success(vec![self.protocol.0])
    }

    fn set_protocol(&mut self, mode: ProtocolMode) -> GetSetStatus {
        self.protocol = mode;
        GetSetStatus::success(vec![])
    }
}

#[test]
fn host_device_control_dispatch_matches_upstream() {
    let mut device = DeviceRuntime::new(TestDelegate::default(), 48);
    let request = HostRuntime::get_report(ReportType::INPUT_REPORT, 7, Some(16));
    let events = device.handle_control(&request.to_bytes().unwrap()).unwrap();
    assert_eq!(
        events,
        [DeviceEvent::SendControl(Message::Data {
            report_type: ReportType::INPUT_REPORT,
            data: vec![7, 0xAA, 0xBB]
        })]
    );

    let set = HostRuntime::set_report(ReportType::OUTPUT_REPORT, vec![9, 1, 2]);
    assert_eq!(
        device.handle_control(&set.to_bytes().unwrap()).unwrap(),
        [DeviceEvent::SendControl(Message::Handshake(
            Handshake::SUCCESSFUL
        ))]
    );
    assert_eq!(
        device.delegate().reports,
        [(9, ReportType::OUTPUT_REPORT, 3, vec![1, 2])]
    );

    let set_protocol = HostRuntime::set_protocol(ProtocolMode::REPORT_PROTOCOL);
    device
        .handle_control(&set_protocol.to_bytes().unwrap())
        .unwrap();
    assert_eq!(device.delegate().protocol, ProtocolMode::REPORT_PROTOCOL);

    assert_eq!(
        device
            .handle_control(&HostRuntime::suspend().to_bytes().unwrap())
            .unwrap(),
        [DeviceEvent::Suspend]
    );
    assert_eq!(
        device
            .handle_control(&HostRuntime::virtual_cable_unplug().to_bytes().unwrap())
            .unwrap(),
        [DeviceEvent::VirtualCableUnplug]
    );
}

#[test]
fn malformed_messages_and_callback_errors_are_safe() {
    assert!(Message::from_bytes(&[]).is_err());
    assert!(Message::from_bytes(&[0x49, 1, 2]).is_err());
    assert!(Message::from_bytes(&[0x60, 1]).is_err());

    let mut device = DeviceRuntime::new(TestDelegate::default(), 3);
    let request = HostRuntime::get_report(ReportType::INPUT_REPORT, 7, None);
    assert_eq!(
        device.handle_control(&request.to_bytes().unwrap()).unwrap(),
        [DeviceEvent::SendControl(Message::Handshake(
            Handshake::ERR_INVALID_PARAMETER
        ))]
    );
    let missing = HostRuntime::get_report(ReportType::INPUT_REPORT, 8, None);
    assert_eq!(
        device.handle_control(&missing.to_bytes().unwrap()).unwrap(),
        [DeviceEvent::SendControl(Message::Handshake(
            Handshake::ERR_INVALID_REPORT_ID
        ))]
    );
}

fn hex(value: &str) -> Vec<u8> {
    value
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let high = (pair[0] as char).to_digit(16).unwrap() as u8;
            let low = (pair[1] as char).to_digit(16).unwrap() as u8;
            (high << 4) | low
        })
        .collect()
}
