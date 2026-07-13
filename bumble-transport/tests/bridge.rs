use bumble_hci::HciPacket;
use bumble_transport::{
    BridgeDirection, FilteredPacket, HciBridge, PacketSink, PacketSource, Result,
};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

#[derive(Debug, Default)]
struct QueueSource(VecDeque<HciPacket>);

impl QueueSource {
    fn with_packet(packet: HciPacket) -> Self {
        Self(VecDeque::from([packet]))
    }
}

impl PacketSource for QueueSource {
    fn read_packet(&mut self) -> Result<Option<HciPacket>> {
        Ok(self.0.pop_front())
    }
}

#[derive(Clone, Debug, Default)]
struct QueueSink(Arc<Mutex<Vec<HciPacket>>>);

impl PacketSink for QueueSink {
    fn write_packet(&mut self, packet: &HciPacket) -> Result<()> {
        self.0.lock().unwrap().push(packet.clone());
        Ok(())
    }
}

fn reset() -> HciPacket {
    HciPacket::from_bytes(&[0x01, 0x03, 0x0C, 0x00]).unwrap()
}

fn command_complete() -> HciPacket {
    HciPacket::from_bytes(&[0x04, 0x0E, 0x04, 0x01, 0x03, 0x0C, 0x00]).unwrap()
}

#[test]
fn bridge_forwards_and_traces_both_directions() {
    let host_sink = QueueSink::default();
    let controller_sink = QueueSink::default();
    let traced = Arc::new(Mutex::new(Vec::new()));
    let mut bridge = HciBridge::new(
        QueueSource::with_packet(reset()),
        host_sink.clone(),
        QueueSource::with_packet(command_complete()),
        controller_sink.clone(),
    );
    let traced_for_callback = traced.clone();
    bridge.set_trace(Some(Box::new(move |direction, packet| {
        traced_for_callback
            .lock()
            .unwrap()
            .push((direction, packet.clone()));
    })));

    assert!(bridge.forward_host_packet().unwrap());
    assert!(!bridge.forward_host_packet().unwrap());
    assert!(bridge.forward_controller_packet().unwrap());
    assert!(!bridge.forward_controller_packet().unwrap());

    assert_eq!(*controller_sink.0.lock().unwrap(), vec![reset()]);
    assert_eq!(*host_sink.0.lock().unwrap(), vec![command_complete()]);
    assert_eq!(
        *traced.lock().unwrap(),
        vec![
            (BridgeDirection::HostToController, reset()),
            (BridgeDirection::ControllerToHost, command_complete())
        ]
    );
}

#[test]
fn filter_can_replace_a_forwarded_packet() {
    let host_sink = QueueSink::default();
    let controller_sink = QueueSink::default();
    let mut bridge = HciBridge::new(
        QueueSource::with_packet(reset()),
        host_sink.clone(),
        QueueSource::default(),
        controller_sink.clone(),
    );
    bridge.set_host_to_controller_filter(Some(Box::new(|packet| {
        assert_eq!(packet, &reset());
        Ok(Some(FilteredPacket::forward(command_complete())))
    })));

    assert!(bridge.forward_host_packet().unwrap());
    assert!(host_sink.0.lock().unwrap().is_empty());
    assert_eq!(*controller_sink.0.lock().unwrap(), vec![command_complete()]);
}

#[test]
fn short_circuit_response_returns_to_packet_sender_without_trace() {
    let host_sink = QueueSink::default();
    let controller_sink = QueueSink::default();
    let trace_count = Arc::new(Mutex::new(0));
    let mut bridge = HciBridge::new(
        QueueSource::with_packet(reset()),
        host_sink.clone(),
        QueueSource::default(),
        controller_sink.clone(),
    );
    bridge.set_host_to_controller_filter(Some(Box::new(|_| {
        Ok(Some(FilteredPacket::respond(command_complete())))
    })));
    let trace_count_for_callback = trace_count.clone();
    bridge.set_trace(Some(Box::new(move |_, _| {
        *trace_count_for_callback.lock().unwrap() += 1;
    })));

    assert!(bridge.forward_host_packet().unwrap());
    assert_eq!(*host_sink.0.lock().unwrap(), vec![command_complete()]);
    assert!(controller_sink.0.lock().unwrap().is_empty());
    assert_eq!(*trace_count.lock().unwrap(), 0);
}

#[test]
fn controller_filter_can_respond_to_controller() {
    let host_sink = QueueSink::default();
    let controller_sink = QueueSink::default();
    let mut bridge = HciBridge::new(
        QueueSource::default(),
        host_sink.clone(),
        QueueSource::with_packet(command_complete()),
        controller_sink.clone(),
    );
    bridge.set_controller_to_host_filter(Some(Box::new(|_| {
        Ok(Some(FilteredPacket::respond(reset())))
    })));

    assert!(bridge.forward_controller_packet().unwrap());
    assert!(host_sink.0.lock().unwrap().is_empty());
    assert_eq!(*controller_sink.0.lock().unwrap(), vec![reset()]);
}
