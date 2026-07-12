use bumble_hid::*;
use bumble_l2cap::{ChannelManager, ClassicChannelSpec};

fn relay(left: &mut ChannelManager, right: &mut ChannelManager) -> usize {
    let mut count = 0;
    while let Some(pdu) = left.poll_outbound() {
        right.process_pdu(pdu).unwrap();
        count += 1;
    }
    count
}

fn connect_channel(host: &mut ChannelManager, device: &mut ChannelManager, psm: u16) -> (u16, u16) {
    let host_cid = host
        .connect(psm.into(), ClassicChannelSpec { mtu: 64 })
        .unwrap();
    for _ in 0..32 {
        let count = relay(host, device) + relay(device, host);
        if count == 0 {
            break;
        }
    }
    (host_cid, device.poll_accepted_channel().unwrap())
}

#[derive(Default)]
struct Delegate;

impl DeviceDelegate for Delegate {
    fn get_report(
        &mut self,
        report_id: u8,
        _report_type: ReportType,
        _buffer_size: Option<u16>,
    ) -> GetSetStatus {
        GetSetStatus::success(vec![report_id.wrapping_add(1)])
    }
}

#[test]
fn control_and_interrupt_flows_cross_live_classic_channels() {
    let mut host_manager = ChannelManager::new();
    let mut device_manager = ChannelManager::new();
    device_manager
        .register_server(Some(HID_CONTROL_PSM.into()), ClassicChannelSpec { mtu: 64 })
        .unwrap();
    device_manager
        .register_server(
            Some(HID_INTERRUPT_PSM.into()),
            ClassicChannelSpec { mtu: 64 },
        )
        .unwrap();
    let (host_control, device_control) =
        connect_channel(&mut host_manager, &mut device_manager, HID_CONTROL_PSM);
    let (host_interrupt, device_interrupt) =
        connect_channel(&mut host_manager, &mut device_manager, HID_INTERRUPT_PSM);
    let host_transport = L2capTransport::new(host_control, host_interrupt, &host_manager).unwrap();
    let device_transport =
        L2capTransport::new(device_control, device_interrupt, &device_manager).unwrap();
    let mut device = DeviceRuntime::new(Delegate, device_transport.control_peer_mtu());

    host_transport
        .send_control(
            &mut host_manager,
            &HostRuntime::get_report(ReportType::INPUT_REPORT, 7, None),
        )
        .unwrap();
    relay(&mut host_manager, &mut device_manager);
    let request = device_transport
        .take_messages(&mut device_manager)
        .unwrap()
        .pop()
        .unwrap();
    assert_eq!(request.channel, HidChannel::Control);
    let events = device
        .handle_control(&request.message.to_bytes().unwrap())
        .unwrap();
    let DeviceEvent::SendControl(response) = &events[0] else {
        panic!("device must answer GET_REPORT");
    };
    device_transport
        .send_control(&mut device_manager, response)
        .unwrap();
    relay(&mut device_manager, &mut host_manager);
    let response = host_transport
        .take_messages(&mut host_manager)
        .unwrap()
        .pop()
        .unwrap();
    assert_eq!(
        HostRuntime::handle_control(&response.message.to_bytes().unwrap()).unwrap(),
        HostEvent::ControlData {
            report_type: ReportType::INPUT_REPORT,
            data: vec![7, 8]
        }
    );

    device_transport
        .send_interrupt(&mut device_manager, &device_data(vec![1, 2, 3]))
        .unwrap();
    relay(&mut device_manager, &mut host_manager);
    let report = host_transport
        .take_messages(&mut host_manager)
        .unwrap()
        .pop()
        .unwrap();
    assert_eq!(report.channel, HidChannel::Interrupt);
    assert_eq!(
        HostRuntime::handle_interrupt(&report.message.to_bytes().unwrap()).unwrap(),
        HostEvent::InterruptData {
            report_type: ReportType::INPUT_REPORT,
            data: vec![1, 2, 3]
        }
    );
}
