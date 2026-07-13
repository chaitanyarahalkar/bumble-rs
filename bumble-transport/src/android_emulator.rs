use crate::{Error, PacketSink, PacketSource, Result};
use bumble_hci::HciPacket;
use std::io;
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use tokio::sync::{mpsc as tokio_mpsc, oneshot};
use tokio_stream::wrappers::UnboundedReceiverStream;

pub const DEFAULT_ANDROID_EMULATOR_ADDRESS: &str = "localhost:8554";

#[allow(non_camel_case_types)]
#[doc(hidden)]
pub mod android_emulator_proto {
    tonic::include_proto!("android.emulation.bluetooth");
}

use android_emulator_proto as proto;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AndroidEmulatorMode {
    #[default]
    Host,
    Controller,
}

impl AndroidEmulatorMode {
    fn parse(value: &str) -> Result<Self> {
        match value {
            "host" => Ok(Self::Host),
            "controller" => Ok(Self::Controller),
            _ => Err(Error::InvalidSpec(format!(
                "Android emulator mode must be host or controller: {value}"
            ))),
        }
    }
}

/// Parsed Android emulator gRPC endpoint configuration.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AndroidEmulatorSpec {
    pub server_address: String,
    pub mode: AndroidEmulatorMode,
}

impl Default for AndroidEmulatorSpec {
    fn default() -> Self {
        Self {
            server_address: DEFAULT_ANDROID_EMULATOR_ADDRESS.into(),
            mode: AndroidEmulatorMode::Host,
        }
    }
}

impl AndroidEmulatorSpec {
    pub fn parse(parameters: Option<&str>) -> Result<Self> {
        let Some(parameters) = parameters.filter(|value| !value.is_empty()) else {
            return Ok(Self::default());
        };

        let mut spec = Self::default();
        for parameter in parameters.split(',') {
            if let Some(mode) = parameter.strip_prefix("mode=") {
                spec.mode = AndroidEmulatorMode::parse(mode)?;
            } else if parameter.is_empty() {
                return Err(Error::InvalidSpec(
                    "Android emulator endpoint contains an empty parameter".into(),
                ));
            } else {
                spec.server_address = parameter.into();
            }
        }
        Ok(spec)
    }

    fn endpoint_uri(&self) -> String {
        if self.server_address.contains("://") {
            self.server_address.clone()
        } else {
            format!("http://{}", self.server_address)
        }
    }
}

/// Wire representation used by Android's emulator gRPC services.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AndroidEmulatorPacket {
    packet_type: u8,
    payload: Vec<u8>,
}

impl AndroidEmulatorPacket {
    pub fn new(packet_type: u8, payload: Vec<u8>) -> Result<Self> {
        if !(1..=5).contains(&packet_type) {
            return Err(Error::InvalidPacketType(packet_type));
        }
        Ok(Self {
            packet_type,
            payload,
        })
    }

    pub fn packet_type(&self) -> u8 {
        self.packet_type
    }

    pub fn payload(&self) -> &[u8] {
        &self.payload
    }

    pub fn into_parts(self) -> (u8, Vec<u8>) {
        (self.packet_type, self.payload)
    }

    pub fn from_hci(packet: &HciPacket) -> Self {
        let bytes = packet.to_bytes();
        Self {
            packet_type: bytes[0],
            payload: bytes[1..].to_vec(),
        }
    }

    pub fn into_hci(self) -> Result<HciPacket> {
        let mut bytes = Vec::with_capacity(1 + self.payload.len());
        bytes.push(self.packet_type);
        bytes.extend_from_slice(&self.payload);
        Ok(HciPacket::from_bytes(&bytes)?)
    }

    fn into_proto(self) -> proto::HciPacket {
        proto::HciPacket {
            r#type: i32::from(self.packet_type),
            packet: self.payload,
        }
    }

    fn from_proto(packet: proto::HciPacket) -> Result<Self> {
        let packet_type = u8::try_from(packet.r#type).map_err(|_| {
            Error::InvalidSpec(format!(
                "Android emulator returned invalid HCI packet type {}",
                packet.r#type
            ))
        })?;
        Self::new(packet_type, packet.packet)
    }
}

pub trait AndroidEmulatorIo {
    fn recv(&mut self) -> Result<Option<AndroidEmulatorPacket>>;
    fn send(&mut self, packet: AndroidEmulatorPacket) -> Result<()>;
}

/// Synchronous packet adapter over an Android emulator endpoint.
pub struct AndroidEmulatorTransport<B> {
    io: B,
    spec: AndroidEmulatorSpec,
}

impl<B> AndroidEmulatorTransport<B> {
    pub fn from_io(io: B, spec: AndroidEmulatorSpec) -> Self {
        Self { io, spec }
    }

    pub fn spec(&self) -> &AndroidEmulatorSpec {
        &self.spec
    }

    pub fn get_ref(&self) -> &B {
        &self.io
    }

    pub fn get_mut(&mut self) -> &mut B {
        &mut self.io
    }

    pub fn into_inner(self) -> B {
        self.io
    }
}

impl<B: AndroidEmulatorIo> PacketSource for AndroidEmulatorTransport<B> {
    fn read_packet(&mut self) -> Result<Option<HciPacket>> {
        self.io
            .recv()?
            .map(AndroidEmulatorPacket::into_hci)
            .transpose()
    }
}

impl<B: AndroidEmulatorIo> PacketSink for AndroidEmulatorTransport<B> {
    fn write_packet(&mut self, packet: &HciPacket) -> Result<()> {
        self.io.send(AndroidEmulatorPacket::from_hci(packet))
    }
}

/// Tonic-backed bidirectional stream running on a dedicated current-thread
/// runtime so the public transport remains synchronous like the other backends.
pub struct GrpcAndroidEmulatorIo {
    outgoing: tokio_mpsc::UnboundedSender<proto::HciPacket>,
    incoming: mpsc::Receiver<Result<Option<AndroidEmulatorPacket>>>,
    lifetime: Arc<GrpcAndroidEmulatorLifetime>,
}

struct GrpcAndroidEmulatorLifetime {
    shutdown: Mutex<Option<oneshot::Sender<()>>>,
    worker: Mutex<Option<thread::JoinHandle<()>>>,
}

impl Drop for GrpcAndroidEmulatorLifetime {
    fn drop(&mut self) {
        if let Some(shutdown) = self
            .shutdown
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .take()
        {
            let _ = shutdown.send(());
        }
        if let Some(worker) = self
            .worker
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .take()
        {
            let _ = worker.join();
        }
    }
}

impl GrpcAndroidEmulatorIo {
    pub fn connect(spec: AndroidEmulatorSpec) -> Result<Self> {
        let (outgoing, outgoing_receiver) = tokio_mpsc::unbounded_channel();
        let (incoming_sender, incoming) = mpsc::channel();
        let (ready_sender, ready) = mpsc::sync_channel(1);
        let (shutdown, shutdown_receiver) = oneshot::channel();
        let worker = thread::Builder::new()
            .name("bumble-android-emulator-grpc".into())
            .spawn(move || {
                let runtime = match tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                {
                    Ok(runtime) => runtime,
                    Err(error) => {
                        let _ = ready_sender.send(Err(error.into()));
                        return;
                    }
                };
                runtime.block_on(run_grpc_worker(
                    spec,
                    outgoing_receiver,
                    incoming_sender,
                    ready_sender,
                    shutdown_receiver,
                ));
            })?;

        match ready.recv() {
            Ok(Ok(())) => Ok(Self {
                outgoing,
                incoming,
                lifetime: Arc::new(GrpcAndroidEmulatorLifetime {
                    shutdown: Mutex::new(Some(shutdown)),
                    worker: Mutex::new(Some(worker)),
                }),
            }),
            Ok(Err(error)) => {
                let _ = worker.join();
                Err(error)
            }
            Err(_) => {
                let _ = worker.join();
                Err(io::Error::new(
                    io::ErrorKind::BrokenPipe,
                    "Android emulator gRPC worker stopped during startup",
                )
                .into())
            }
        }
    }
}

impl AndroidEmulatorIo for GrpcAndroidEmulatorIo {
    fn recv(&mut self) -> Result<Option<AndroidEmulatorPacket>> {
        match self.incoming.recv() {
            Ok(result) => result,
            Err(_) => Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "Android emulator gRPC response worker stopped unexpectedly",
            )
            .into()),
        }
    }

    fn send(&mut self, packet: AndroidEmulatorPacket) -> Result<()> {
        self.outgoing.send(packet.into_proto()).map_err(|_| {
            io::Error::new(
                io::ErrorKind::BrokenPipe,
                "Android emulator gRPC request stream is closed",
            )
            .into()
        })
    }
}

pub struct AndroidEmulatorPacketSource {
    incoming: mpsc::Receiver<Result<Option<AndroidEmulatorPacket>>>,
    _lifetime: Arc<GrpcAndroidEmulatorLifetime>,
}

impl PacketSource for AndroidEmulatorPacketSource {
    fn read_packet(&mut self) -> Result<Option<HciPacket>> {
        match self.incoming.recv() {
            Ok(result) => result?.map(AndroidEmulatorPacket::into_hci).transpose(),
            Err(_) => Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "Android emulator gRPC response worker stopped unexpectedly",
            )
            .into()),
        }
    }
}

pub struct AndroidEmulatorPacketSink {
    outgoing: tokio_mpsc::UnboundedSender<proto::HciPacket>,
    _lifetime: Arc<GrpcAndroidEmulatorLifetime>,
}

impl PacketSink for AndroidEmulatorPacketSink {
    fn write_packet(&mut self, packet: &HciPacket) -> Result<()> {
        self.outgoing
            .send(AndroidEmulatorPacket::from_hci(packet).into_proto())
            .map_err(|_| {
                io::Error::new(
                    io::ErrorKind::BrokenPipe,
                    "Android emulator gRPC request stream is closed",
                )
                .into()
            })
    }
}

async fn open_grpc_stream(
    spec: &AndroidEmulatorSpec,
    outgoing: tokio_mpsc::UnboundedReceiver<proto::HciPacket>,
) -> Result<tonic::Streaming<proto::HciPacket>> {
    let request = UnboundedReceiverStream::new(outgoing);
    let endpoint = spec.endpoint_uri();
    let response = match spec.mode {
        AndroidEmulatorMode::Host => {
            let mut client =
                proto::emulated_bluetooth_service_client::EmulatedBluetoothServiceClient::connect(
                    endpoint,
                )
                .await?;
            client.register_hci_device(request).await?
        }
        AndroidEmulatorMode::Controller => {
            let mut client =
                proto::vhci_forwarding_service_client::VhciForwardingServiceClient::connect(
                    endpoint,
                )
                .await?;
            client.attach_vhci(request).await?
        }
    };
    Ok(response.into_inner())
}

async fn run_grpc_worker(
    spec: AndroidEmulatorSpec,
    outgoing: tokio_mpsc::UnboundedReceiver<proto::HciPacket>,
    incoming: mpsc::Sender<Result<Option<AndroidEmulatorPacket>>>,
    ready: mpsc::SyncSender<Result<()>>,
    mut shutdown: oneshot::Receiver<()>,
) {
    let mut stream = match open_grpc_stream(&spec, outgoing).await {
        Ok(stream) => stream,
        Err(error) => {
            let _ = ready.send(Err(error));
            return;
        }
    };
    if ready.send(Ok(())).is_err() {
        return;
    }

    loop {
        tokio::select! {
            _ = &mut shutdown => break,
            message = stream.message() => {
                match message {
                    Ok(Some(packet)) => {
                        if incoming.send(AndroidEmulatorPacket::from_proto(packet).map(Some)).is_err() {
                            break;
                        }
                    }
                    Ok(None) => {
                        let _ = incoming.send(Ok(None));
                        break;
                    }
                    Err(error) => {
                        let _ = incoming.send(Err(error.into()));
                        break;
                    }
                }
            }
        }
    }
}

pub type SystemAndroidEmulatorTransport = AndroidEmulatorTransport<GrpcAndroidEmulatorIo>;

impl SystemAndroidEmulatorTransport {
    pub fn open(parameters: Option<&str>) -> Result<Self> {
        let spec = AndroidEmulatorSpec::parse(parameters)?;
        let io = GrpcAndroidEmulatorIo::connect(spec.clone())?;
        Ok(Self::from_io(io, spec))
    }

    pub fn try_split(self) -> (AndroidEmulatorPacketSource, AndroidEmulatorPacketSink) {
        let GrpcAndroidEmulatorIo {
            outgoing,
            incoming,
            lifetime,
        } = self.io;
        (
            AndroidEmulatorPacketSource {
                incoming,
                _lifetime: Arc::clone(&lifetime),
            },
            AndroidEmulatorPacketSink {
                outgoing,
                _lifetime: lifetime,
            },
        )
    }
}
