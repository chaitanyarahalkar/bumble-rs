use bumble_hci::HciPacket;
use bumble_transport::android_emulator_proto as proto;
use bumble_transport::{
    open_split_transport, open_transport, AndroidEmulatorIo, AndroidEmulatorMode,
    AndroidEmulatorPacket, AndroidEmulatorSpec, AndroidEmulatorTransport, Error, PacketSink,
    PacketSource, SystemAndroidEmulatorTransport, DEFAULT_ANDROID_EMULATOR_ADDRESS,
};
use prost::Message;
use std::collections::VecDeque;
use std::net::TcpListener;
use std::thread;
use tokio::sync::{mpsc, oneshot};
use tokio_stream::wrappers::{ReceiverStream, TcpListenerStream};
use tonic::{Request, Response, Status, Streaming};

#[derive(Default)]
struct MockIo {
    incoming: VecDeque<bumble_transport::Result<Option<AndroidEmulatorPacket>>>,
    outgoing: Vec<AndroidEmulatorPacket>,
}

impl AndroidEmulatorIo for MockIo {
    fn recv(&mut self) -> bumble_transport::Result<Option<AndroidEmulatorPacket>> {
        self.incoming.pop_front().unwrap_or(Ok(None))
    }

    fn send(&mut self, packet: AndroidEmulatorPacket) -> bumble_transport::Result<()> {
        self.outgoing.push(packet);
        Ok(())
    }
}

fn reset_command() -> HciPacket {
    HciPacket::from_bytes(&[0x01, 0x03, 0x0c, 0x00]).unwrap()
}

fn command_complete() -> HciPacket {
    HciPacket::from_bytes(&[0x04, 0x0e, 0x04, 0x01, 0x03, 0x0c, 0x00]).unwrap()
}

#[test]
fn emulator_spec_matches_upstream_forms() {
    assert_eq!(
        AndroidEmulatorSpec::parse(None).unwrap(),
        AndroidEmulatorSpec {
            server_address: DEFAULT_ANDROID_EMULATOR_ADDRESS.into(),
            mode: AndroidEmulatorMode::Host,
        }
    );
    assert_eq!(
        AndroidEmulatorSpec::parse(Some("localhost:9554")).unwrap(),
        AndroidEmulatorSpec {
            server_address: "localhost:9554".into(),
            mode: AndroidEmulatorMode::Host,
        }
    );
    assert_eq!(
        AndroidEmulatorSpec::parse(Some("mode=controller")).unwrap(),
        AndroidEmulatorSpec {
            server_address: DEFAULT_ANDROID_EMULATOR_ADDRESS.into(),
            mode: AndroidEmulatorMode::Controller,
        }
    );
    assert_eq!(
        AndroidEmulatorSpec::parse(Some("[::1]:9554,mode=controller")).unwrap(),
        AndroidEmulatorSpec {
            server_address: "[::1]:9554".into(),
            mode: AndroidEmulatorMode::Controller,
        }
    );
    assert!(matches!(
        AndroidEmulatorSpec::parse(Some("mode=invalid")),
        Err(Error::InvalidSpec(_))
    ));
}

#[test]
fn emulator_packet_preserves_every_h4_packet_type() {
    let packets = [
        vec![0x01, 0x03, 0x0c, 0x00],
        vec![0x02, 0x01, 0x20, 0x03, 0x00, 0xaa, 0xbb, 0xcc],
        vec![0x03, 0x01, 0x00, 0x02, 0x11, 0x22],
        vec![0x04, 0x0f, 0x04, 0x00, 0x01, 0x03, 0x0c],
        vec![0x05, 0x01, 0x10, 0x02, 0x00, 0x33, 0x44],
    ];
    for bytes in packets {
        let hci = HciPacket::from_bytes(&bytes).unwrap();
        let emulator = AndroidEmulatorPacket::from_hci(&hci);
        assert_eq!(emulator.packet_type(), bytes[0]);
        assert_eq!(emulator.payload(), &bytes[1..]);
        assert_eq!(emulator.into_hci().unwrap(), hci);
    }
    assert!(matches!(
        AndroidEmulatorPacket::new(0, Vec::new()),
        Err(Error::InvalidPacketType(0))
    ));
}

#[test]
fn emulator_protobuf_packet_matches_upstream_wire_tags() {
    let packet = proto::HciPacket {
        r#type: 1,
        packet: vec![0x03, 0x0c, 0x00],
    };
    assert_eq!(
        packet.encode_to_vec(),
        [0x08, 0x01, 0x12, 0x03, 0x03, 0x0c, 0x00]
    );
    assert_eq!(
        proto::HciPacket::decode(packet.encode_to_vec().as_slice()).unwrap(),
        packet
    );
}

#[test]
fn emulator_transport_maps_packets_and_propagates_errors() {
    let event = command_complete();
    let mut io = MockIo::default();
    io.incoming
        .push_back(Ok(Some(AndroidEmulatorPacket::from_hci(&event))));
    io.incoming
        .push_back(Err(Error::InvalidSpec("failed".into())));
    let mut transport = AndroidEmulatorTransport::from_io(io, AndroidEmulatorSpec::default());

    assert_eq!(transport.read_packet().unwrap(), Some(event));
    assert!(matches!(
        transport.read_packet(),
        Err(Error::InvalidSpec(_))
    ));
    let command = reset_command();
    transport.write_packet(&command).unwrap();
    assert_eq!(
        transport.get_ref().outgoing,
        [AndroidEmulatorPacket::from_hci(&command)]
    );
}

#[test]
fn emulator_open_propagates_endpoint_errors_from_worker_startup() {
    assert!(matches!(
        SystemAndroidEmulatorTransport::open(Some("http://[")),
        Err(Error::GrpcTransport(_))
    ));
}

#[derive(Clone, Copy)]
struct EchoService;

type EchoStream = ReceiverStream<Result<proto::HciPacket, Status>>;

async fn echo(request: Request<Streaming<proto::HciPacket>>) -> Response<EchoStream> {
    let mut incoming = request.into_inner();
    let (sender, receiver) = mpsc::channel(8);
    tokio::spawn(async move {
        loop {
            match incoming.message().await {
                Ok(Some(packet)) => {
                    if sender.send(Ok(packet)).await.is_err() {
                        break;
                    }
                }
                Ok(None) => break,
                Err(error) => {
                    let _ = sender.send(Err(error)).await;
                    break;
                }
            }
        }
    });
    Response::new(ReceiverStream::new(receiver))
}

#[allow(non_camel_case_types)]
#[tonic::async_trait]
impl proto::emulated_bluetooth_service_server::EmulatedBluetoothService for EchoService {
    type registerHCIDeviceStream = EchoStream;

    async fn register_hci_device(
        &self,
        request: Request<Streaming<proto::HciPacket>>,
    ) -> Result<Response<Self::registerHCIDeviceStream>, Status> {
        Ok(echo(request).await)
    }
}

#[allow(non_camel_case_types)]
#[tonic::async_trait]
impl proto::vhci_forwarding_service_server::VhciForwardingService for EchoService {
    type attachVhciStream = EchoStream;

    async fn attach_vhci(
        &self,
        request: Request<Streaming<proto::HciPacket>>,
    ) -> Result<Response<Self::attachVhciStream>, Status> {
        Ok(echo(request).await)
    }
}

struct TestServer {
    address: std::net::SocketAddr,
    shutdown: Option<oneshot::Sender<()>>,
    worker: Option<thread::JoinHandle<()>>,
}

impl TestServer {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let address = listener.local_addr().unwrap();
        let (shutdown, shutdown_receiver) = oneshot::channel();
        let worker = thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            runtime.block_on(async move {
                let listener = tokio::net::TcpListener::from_std(listener).unwrap();
                tonic::transport::Server::builder()
                    .add_service(
                        proto::emulated_bluetooth_service_server::EmulatedBluetoothServiceServer::new(
                            EchoService,
                        ),
                    )
                    .add_service(
                        proto::vhci_forwarding_service_server::VhciForwardingServiceServer::new(
                            EchoService,
                        ),
                    )
                    .serve_with_incoming_shutdown(
                        TcpListenerStream::new(listener),
                        async move {
                            let _ = shutdown_receiver.await;
                        },
                    )
                    .await
                    .unwrap();
            });
        });
        Self {
            address,
            shutdown: Some(shutdown),
            worker: Some(worker),
        }
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
        if let Some(worker) = self.worker.take() {
            worker.join().unwrap();
        }
    }
}

#[test]
fn real_grpc_host_and_controller_modes_exchange_hci_packets() {
    let server = TestServer::start();
    for mode in ["host", "controller"] {
        let mut transport =
            open_transport(&format!("android-emulator:{},mode={mode}", server.address)).unwrap();
        let packet = reset_command();
        transport.write_packet(&packet).unwrap();
        assert_eq!(transport.read_packet().unwrap(), Some(packet));
    }

    let mut transport =
        open_split_transport(&format!("android-emulator:{},mode=host", server.address)).unwrap();
    let packet = reset_command();
    transport.sink.write_packet(&packet).unwrap();
    assert_eq!(transport.source.read_packet().unwrap(), Some(packet));
}
