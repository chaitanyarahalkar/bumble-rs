use crate::{Error, PacketSink, Result, SplitOpenedTransport};
use bumble_hci::{AclDataPacket, Command, HciPacket, IsoDataPacket, SynchronousDataPacket};
use bumble_host::HostTransport;
use std::collections::VecDeque;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, TryRecvError};
use std::time::Duration;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExternalHostState {
    Running,
    Ended,
    Failed(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExternalHostActivity {
    Packet,
    Timeout,
    Ended,
}

enum ReaderMessage {
    Packet(Box<HciPacket>),
    Ended,
    Failed(String),
}

/// A host-side HCI adapter backed by an independently owned packet source and
/// sink.
///
/// A reader worker waits on the blocking source while callers retain exclusive
/// ownership of the sink and [`bumble_host::Device`]. Incoming packets are
/// collected non-blockingly through [`HostTransport::drain_host_events`], or a
/// caller can use [`ExternalHost::wait_for_activity`] to efficiently drive a
/// synchronous application loop.
pub struct ExternalHost {
    sink: Box<dyn PacketSink + Send>,
    receiver: Receiver<ReaderMessage>,
    pending: VecDeque<HciPacket>,
    state: ExternalHostState,
}

impl ExternalHost {
    pub fn new(transport: SplitOpenedTransport) -> Self {
        let (sender, receiver) = mpsc::channel();
        let mut source = transport.source;
        std::thread::spawn(move || loop {
            match source.read_packet() {
                Ok(Some(packet)) => {
                    if sender
                        .send(ReaderMessage::Packet(Box::new(packet)))
                        .is_err()
                    {
                        return;
                    }
                }
                Ok(None) => {
                    let _ = sender.send(ReaderMessage::Ended);
                    return;
                }
                Err(error) => {
                    let _ = sender.send(ReaderMessage::Failed(error.to_string()));
                    return;
                }
            }
        });
        Self {
            sink: transport.sink,
            receiver,
            pending: VecDeque::new(),
            state: ExternalHostState::Running,
        }
    }

    pub fn state(&self) -> &ExternalHostState {
        &self.state
    }

    pub fn wait_for_activity(&mut self, timeout: Duration) -> Result<ExternalHostActivity> {
        if !self.pending.is_empty() {
            return Ok(ExternalHostActivity::Packet);
        }
        match &self.state {
            ExternalHostState::Ended => return Ok(ExternalHostActivity::Ended),
            ExternalHostState::Failed(message) => return Err(Error::Remote(message.clone())),
            ExternalHostState::Running => {}
        }
        match self.receiver.recv_timeout(timeout) {
            Ok(message) => self.receive_message(message),
            Err(RecvTimeoutError::Timeout) => Ok(ExternalHostActivity::Timeout),
            Err(RecvTimeoutError::Disconnected) => {
                self.state = ExternalHostState::Ended;
                Ok(ExternalHostActivity::Ended)
            }
        }
    }

    fn receive_message(&mut self, message: ReaderMessage) -> Result<ExternalHostActivity> {
        match message {
            ReaderMessage::Packet(packet) => {
                self.pending.push_back(*packet);
                Ok(ExternalHostActivity::Packet)
            }
            ReaderMessage::Ended => {
                self.state = ExternalHostState::Ended;
                Ok(ExternalHostActivity::Ended)
            }
            ReaderMessage::Failed(message) => {
                self.state = ExternalHostState::Failed(message.clone());
                Err(Error::Remote(message))
            }
        }
    }

    fn collect_available(&mut self) {
        while matches!(self.state, ExternalHostState::Running) {
            match self.receiver.try_recv() {
                Ok(message) => {
                    let _ = self.receive_message(message);
                }
                Err(TryRecvError::Empty) => return,
                Err(TryRecvError::Disconnected) => {
                    self.state = ExternalHostState::Ended;
                    return;
                }
            }
        }
    }

    fn fail(&mut self, message: impl Into<String>) {
        self.state = ExternalHostState::Failed(message.into());
    }

    fn write(&mut self, controller_id: usize, packet: HciPacket) -> bool {
        if controller_id != 0 {
            self.fail(format!(
                "external host exposes controller 0, not controller {controller_id}"
            ));
            return false;
        }
        if !matches!(self.state, ExternalHostState::Running) {
            return false;
        }
        if let Err(error) = self
            .sink
            .write_packet(&packet)
            .and_then(|()| self.sink.flush())
        {
            self.fail(error.to_string());
            return false;
        }
        true
    }
}

impl HostTransport for ExternalHost {
    fn handle_command(&mut self, controller_id: usize, command: Command) {
        self.write(controller_id, HciPacket::Command(command));
    }

    fn send_acl_packet(&mut self, controller_id: usize, packet: AclDataPacket) -> bool {
        self.write(controller_id, HciPacket::AclData(packet))
    }

    fn send_synchronous_data(
        &mut self,
        controller_id: usize,
        connection_handle: u16,
        packet_status: u8,
        data: &[u8],
    ) -> bool {
        let Ok(data_total_length) = u8::try_from(data.len()) else {
            return false;
        };
        self.write(
            controller_id,
            HciPacket::SyncData(SynchronousDataPacket {
                connection_handle,
                packet_status,
                data_total_length,
                data: data.to_vec(),
            }),
        )
    }

    fn send_iso_packet(&mut self, controller_id: usize, packet: IsoDataPacket) -> bool {
        self.write(controller_id, HciPacket::IsoData(packet))
    }

    fn drain_host_events(&mut self, controller_id: usize) -> Vec<HciPacket> {
        if controller_id != 0 {
            self.fail(format!(
                "external host exposes controller 0, not controller {controller_id}"
            ));
            return Vec::new();
        }
        self.collect_available();
        self.pending.drain(..).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{PacketSource, Result as TransportResult};
    use bumble::Address;
    use bumble_hci::{Event, LeMetaEvent};
    use bumble_host::Device;
    use std::collections::BTreeMap;
    use std::sync::{Arc, Mutex};

    struct ScriptedSource(VecDeque<TransportResult<Option<HciPacket>>>);

    impl PacketSource for ScriptedSource {
        fn read_packet(&mut self) -> TransportResult<Option<HciPacket>> {
            self.0.pop_front().unwrap_or(Ok(None))
        }
    }

    #[derive(Clone, Default)]
    struct RecordingSink(Arc<Mutex<Vec<HciPacket>>>);

    impl PacketSink for RecordingSink {
        fn write_packet(&mut self, packet: &HciPacket) -> TransportResult<()> {
            self.0.lock().unwrap().push(packet.clone());
            Ok(())
        }
    }

    struct FailingSink;

    impl PacketSink for FailingSink {
        fn write_packet(&mut self, _packet: &HciPacket) -> TransportResult<()> {
            Err(Error::Remote("write failed".into()))
        }
    }

    fn split(incoming: Vec<HciPacket>, sink: RecordingSink) -> SplitOpenedTransport {
        let mut script = incoming
            .into_iter()
            .map(|packet| Ok(Some(packet)))
            .collect::<VecDeque<_>>();
        script.push_back(Ok(None));
        SplitOpenedTransport {
            source: Box::new(ScriptedSource(script)),
            sink: Box::new(sink),
            metadata: BTreeMap::new(),
        }
    }

    #[test]
    fn sends_typed_packets_and_collects_reader_packets() {
        let address =
            Address::parse("C4:F2:17:1A:1D:BB", bumble::AddressType::RANDOM_DEVICE).unwrap();
        let incoming = HciPacket::Event(Event::LeMeta(LeMetaEvent::ConnectionComplete {
            status: 0,
            connection_handle: 0x123,
            role: 0,
            peer_address_type: 1,
            peer_address: address,
            connection_interval: 24,
            peripheral_latency: 0,
            supervision_timeout: 42,
            central_clock_accuracy: 0,
        }));
        let sink = RecordingSink::default();
        let recorded = sink.clone();
        let mut host = ExternalHost::new(split(vec![incoming.clone()], sink));

        host.handle_command(0, Command::Reset);
        assert!(host.send_acl_packet(
            0,
            AclDataPacket {
                connection_handle: 0x123,
                pb_flag: 0,
                bc_flag: 0,
                data_total_length: 2,
                data: vec![1, 2],
            }
        ));
        assert_eq!(
            recorded.0.lock().unwrap().as_slice(),
            &[
                HciPacket::Command(Command::Reset),
                HciPacket::AclData(AclDataPacket {
                    connection_handle: 0x123,
                    pb_flag: 0,
                    bc_flag: 0,
                    data_total_length: 2,
                    data: vec![1, 2],
                }),
            ]
        );
        assert_eq!(
            host.wait_for_activity(Duration::from_secs(1)).unwrap(),
            ExternalHostActivity::Packet
        );
        assert_eq!(host.drain_host_events(0), vec![incoming]);
        assert_eq!(
            host.wait_for_activity(Duration::from_secs(1)).unwrap(),
            ExternalHostActivity::Ended
        );
    }

    #[test]
    fn rejects_nonzero_controller_and_oversized_synchronous_data() {
        let sink = RecordingSink::default();
        let mut host = ExternalHost::new(split(Vec::new(), sink));
        assert!(!host.send_acl_packet(
            1,
            AclDataPacket {
                connection_handle: 0,
                pb_flag: 0,
                bc_flag: 0,
                data_total_length: 0,
                data: Vec::new(),
            }
        ));
        assert!(matches!(host.state(), ExternalHostState::Failed(_)));

        let sink = RecordingSink::default();
        let mut host = ExternalHost::new(split(Vec::new(), sink));
        assert!(!host.send_synchronous_data(0, 1, 0, &[0; 256]));
    }

    #[test]
    fn preserves_reader_and_writer_failures() {
        let read_transport = SplitOpenedTransport {
            source: Box::new(ScriptedSource(VecDeque::from([Err(Error::Remote(
                "read failed".into(),
            ))]))),
            sink: Box::new(RecordingSink::default()),
            metadata: BTreeMap::new(),
        };
        let mut host = ExternalHost::new(read_transport);
        assert!(host.wait_for_activity(Duration::from_secs(1)).is_err());
        assert_eq!(
            host.state(),
            &ExternalHostState::Failed("remote transport error: read failed".into())
        );

        let write_transport = SplitOpenedTransport {
            source: Box::new(ScriptedSource(VecDeque::new())),
            sink: Box::new(FailingSink),
            metadata: BTreeMap::new(),
        };
        let mut host = ExternalHost::new(write_transport);
        host.handle_command(0, Command::Reset);
        assert_eq!(
            host.state(),
            &ExternalHostState::Failed("remote transport error: write failed".into())
        );
    }

    #[test]
    fn drives_device_connection_state_over_external_hci() {
        let address =
            Address::parse("C4:F2:17:1A:1D:BB", bumble::AddressType::RANDOM_DEVICE).unwrap();
        let incoming = HciPacket::Event(Event::LeMeta(LeMetaEvent::ConnectionComplete {
            status: 0,
            connection_handle: 0x123,
            role: 0,
            peer_address_type: 1,
            peer_address: address.clone(),
            connection_interval: 24,
            peripheral_latency: 0,
            supervision_timeout: 42,
            central_clock_accuracy: 0,
        }));
        let sink = RecordingSink::default();
        let recorded = sink.clone();
        let mut host = ExternalHost::new(split(vec![incoming], sink));
        let mut device = Device::new(0);

        device.connect_le(&mut host, address.clone());
        assert_eq!(
            host.wait_for_activity(Duration::from_secs(1)).unwrap(),
            ExternalHostActivity::Packet
        );
        assert!(device.poll(&mut host));
        assert_eq!(device.connection_handle(), Some(0x123));
        assert_eq!(device.peer_address(), Some(&address));
        assert!(matches!(
            recorded.0.lock().unwrap().first(),
            Some(HciPacket::Command(Command::LeCreateConnection { peer_address, .. }))
                if peer_address == &address
        ));
    }
}
