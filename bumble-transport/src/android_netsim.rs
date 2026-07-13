use crate::{AndroidEmulatorPacket, Error, PacketSink, PacketSource, Result};
use bumble_hci::HciPacket;
use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::net::{SocketAddr, TcpListener};
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use tokio::sync::{mpsc as tokio_mpsc, oneshot};
use tokio_stream::wrappers::{TcpListenerStream, UnboundedReceiverStream};
use tonic::{Request, Response, Status, Streaming};

pub const DEFAULT_ANDROID_NETSIM_NAME: &str = "bumble0";
pub const DEFAULT_ANDROID_NETSIM_MANUFACTURER: &str = "Bumble";
pub const DEFAULT_ANDROID_NETSIM_VARIANT: &str = "";

pub type AndroidNetsimPacket = AndroidEmulatorPacket;

#[allow(non_camel_case_types, clippy::large_enum_variant)]
#[doc(hidden)]
pub mod android_netsim_proto {
    pub mod common {
        tonic::include_proto!("netsim.common");
    }
    pub mod startup {
        tonic::include_proto!("netsim.startup");
    }
    pub mod packet {
        tonic::include_proto!("netsim.packet");
    }
}

use android_netsim_proto::{common as common_proto, packet as proto, startup as startup_proto};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AndroidNetsimMode {
    #[default]
    Host,
    Controller,
}

impl AndroidNetsimMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Host => "host",
            Self::Controller => "controller",
        }
    }

    fn parse(value: &str) -> Result<Self> {
        match value {
            "host" => Ok(Self::Host),
            "controller" => Ok(Self::Controller),
            _ => Err(Error::InvalidSpec(format!(
                "Android netsim mode must be host or controller: {value}"
            ))),
        }
    }
}

/// Parsed Android netsim endpoint and startup options.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AndroidNetsimSpec {
    pub host: Option<String>,
    pub port: u16,
    pub mode: AndroidNetsimMode,
    pub instance: u32,
    pub options: BTreeMap<String, String>,
}

impl Default for AndroidNetsimSpec {
    fn default() -> Self {
        Self {
            host: None,
            port: 0,
            mode: AndroidNetsimMode::Host,
            instance: 0,
            options: BTreeMap::new(),
        }
    }
}

impl AndroidNetsimSpec {
    pub fn parse(parameters: Option<&str>) -> Result<Self> {
        let parameters = parameters.unwrap_or_default();
        let parts: Vec<&str> = if parameters.is_empty() {
            Vec::new()
        } else {
            parameters.split(',').collect()
        };
        let mut spec = Self::default();
        let options_start = if parts.first().is_some_and(|part| part.contains(':')) {
            let (host, port) = parts[0].rsplit_once(':').ok_or_else(|| {
                Error::InvalidSpec("Android netsim endpoint must be <host>:<port>".into())
            })?;
            if host.is_empty() || port.is_empty() {
                return Err(Error::InvalidSpec(
                    "Android netsim endpoint must be <host>:<port>".into(),
                ));
            }
            spec.host = Some(host.into());
            spec.port = port
                .parse::<u16>()
                .map_err(|_| Error::InvalidSpec(format!("invalid Android netsim port: {port}")))?;
            1
        } else {
            0
        };

        for option in &parts[options_start..] {
            let (name, value) = option.split_once('=').ok_or_else(|| {
                Error::InvalidSpec(format!(
                    "invalid Android netsim option, expected name=value: {option}"
                ))
            })?;
            if name.is_empty() || value.is_empty() {
                return Err(Error::InvalidSpec(format!(
                    "invalid Android netsim option: {option}"
                )));
            }
            spec.options.insert(name.into(), value.into());
        }

        spec.mode = AndroidNetsimMode::parse(
            spec.options
                .get("mode")
                .map(String::as_str)
                .unwrap_or("host"),
        )?;
        spec.instance = spec
            .options
            .get("instance")
            .map(String::as_str)
            .unwrap_or("0")
            .parse::<u32>()
            .map_err(|_| Error::InvalidSpec("invalid Android netsim instance".into()))?;
        if spec.mode == AndroidNetsimMode::Controller && spec.host.is_none() {
            return Err(Error::InvalidSpec(
                "Android netsim controller mode requires <host>:<port>".into(),
            ));
        }
        Ok(spec)
    }

    pub fn name(&self) -> &str {
        self.options
            .get("name")
            .map(String::as_str)
            .unwrap_or(DEFAULT_ANDROID_NETSIM_NAME)
    }

    pub fn variant(&self) -> &str {
        self.options
            .get("variant")
            .map(String::as_str)
            .unwrap_or(DEFAULT_ANDROID_NETSIM_VARIANT)
    }

    pub fn normalized_host(&self) -> &str {
        match self.host.as_deref() {
            Some("_") | None => "localhost",
            Some(host) => host,
        }
    }

    fn endpoint_uri(&self) -> String {
        format!("http://{}:{}", self.normalized_host(), self.port)
    }
}

pub fn netsim_ini_file_name(instance: u32) -> String {
    if instance == 0 {
        "netsim.ini".into()
    } else {
        format!("netsim_{instance}.ini")
    }
}

pub fn default_netsim_ini_dir() -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        std::env::var_os("TMPDIR").map(PathBuf::from).or_else(|| {
            std::env::var_os("HOME")
                .map(PathBuf::from)
                .map(|home| home.join("Library/Caches/TemporaryItems"))
        })
    }
    #[cfg(target_os = "linux")]
    {
        std::env::var_os("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .or_else(|| {
                let path =
                    PathBuf::from(std::env::var_os("TMPDIR").unwrap_or_else(|| "/tmp".into()));
                path.is_dir().then_some(path)
            })
    }
    #[cfg(target_os = "windows")]
    {
        std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .map(|path| path.join("Temp"))
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        None
    }
}

pub fn find_netsim_grpc_port_in(directory: &Path, instance: u32) -> Result<Option<u16>> {
    let path = directory.join(netsim_ini_file_name(instance));
    let contents = match fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    for line in contents.lines() {
        if let Some(("grpc.port", value)) = line.split_once('=') {
            return value
                .trim()
                .parse::<u16>()
                .map(Some)
                .map_err(|_| Error::InvalidSpec("invalid grpc.port in netsim INI file".into()));
        }
    }
    Ok(None)
}

pub fn find_netsim_grpc_port(instance: u32) -> Result<Option<u16>> {
    match default_netsim_ini_dir() {
        Some(directory) => find_netsim_grpc_port_in(&directory, instance),
        None => Ok(None),
    }
}

struct NetsimIniRegistration {
    path: PathBuf,
}

impl NetsimIniRegistration {
    fn publish(port: u16, instance: u32) -> Option<Self> {
        let directory = default_netsim_ini_dir()?;
        if !directory.is_dir() {
            return None;
        }
        let path = directory.join(netsim_ini_file_name(instance));
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .ok()?;
        if writeln!(file, "grpc.port={port}").is_err() {
            let _ = fs::remove_file(&path);
            return None;
        }
        Some(Self { path })
    }
}

impl Drop for NetsimIniRegistration {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

fn packet_into_proto(packet: AndroidNetsimPacket) -> proto::HciPacket {
    let (packet_type, packet) = packet.into_parts();
    proto::HciPacket {
        packet_type: i32::from(packet_type),
        packet,
    }
}

fn packet_from_proto(packet: proto::HciPacket) -> Result<AndroidNetsimPacket> {
    let packet_type = u8::try_from(packet.packet_type).map_err(|_| {
        Error::Remote(format!(
            "Android netsim returned invalid HCI packet type {}",
            packet.packet_type
        ))
    })?;
    AndroidNetsimPacket::new(packet_type, packet.packet)
}

fn initial_request(spec: &AndroidNetsimSpec) -> proto::PacketRequest {
    let chip = startup_proto::Chip {
        kind: common_proto::ChipKind::Bluetooth as i32,
        manufacturer: DEFAULT_ANDROID_NETSIM_MANUFACTURER.into(),
        ..Default::default()
    };
    let device_info = startup_proto::DeviceInfo {
        name: spec.name().into(),
        kind: "BUMBLE".into(),
        version: env!("CARGO_PKG_VERSION").into(),
        sdk_version: format!("rust-{}", env!("CARGO_PKG_RUST_VERSION")),
        build_id: format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH),
        variant: spec.variant().into(),
        arch: std::env::consts::ARCH.into(),
    };
    proto::PacketRequest {
        request_type: Some(proto::packet_request::RequestType::InitialInfo(
            startup_proto::ChipInfo {
                name: spec.name().into(),
                chip: Some(chip),
                device_info: Some(device_info),
            },
        )),
    }
}

pub trait AndroidNetsimIo {
    fn recv(&mut self) -> Result<Option<AndroidNetsimPacket>>;
    fn send(&mut self, packet: AndroidNetsimPacket) -> Result<()>;
}

pub struct AndroidNetsimTransport<B> {
    io: B,
    spec: AndroidNetsimSpec,
}

impl<B> AndroidNetsimTransport<B> {
    pub fn from_io(io: B, spec: AndroidNetsimSpec) -> Self {
        Self { io, spec }
    }

    pub fn spec(&self) -> &AndroidNetsimSpec {
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

impl<B: AndroidNetsimIo> PacketSource for AndroidNetsimTransport<B> {
    fn read_packet(&mut self) -> Result<Option<HciPacket>> {
        self.io
            .recv()?
            .map(AndroidNetsimPacket::into_hci)
            .transpose()
    }
}

impl<B: AndroidNetsimIo> PacketSink for AndroidNetsimTransport<B> {
    fn write_packet(&mut self, packet: &HciPacket) -> Result<()> {
        self.io.send(AndroidNetsimPacket::from_hci(packet))
    }
}

pub struct GrpcAndroidNetsimHostIo {
    outgoing: tokio_mpsc::UnboundedSender<proto::PacketRequest>,
    incoming: mpsc::Receiver<Result<Option<AndroidNetsimPacket>>>,
    lifetime: Arc<GrpcAndroidNetsimLifetime>,
}

struct GrpcAndroidNetsimLifetime {
    shutdown: Mutex<Option<oneshot::Sender<()>>>,
    worker: Mutex<Option<thread::JoinHandle<()>>>,
    _ini_registration: Option<NetsimIniRegistration>,
}

impl Drop for GrpcAndroidNetsimLifetime {
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

impl GrpcAndroidNetsimHostIo {
    pub fn connect(spec: AndroidNetsimSpec) -> Result<Self> {
        let (outgoing, outgoing_receiver) = tokio_mpsc::unbounded_channel();
        outgoing.send(initial_request(&spec)).map_err(|_| {
            io::Error::new(io::ErrorKind::BrokenPipe, "netsim startup queue closed")
        })?;
        let (incoming_sender, incoming) = mpsc::channel();
        let (ready_sender, ready) = mpsc::sync_channel(1);
        let (shutdown, shutdown_receiver) = oneshot::channel();
        let worker = thread::Builder::new()
            .name("bumble-android-netsim-host".into())
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
                runtime.block_on(run_host_worker(
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
                lifetime: Arc::new(GrpcAndroidNetsimLifetime {
                    shutdown: Mutex::new(Some(shutdown)),
                    worker: Mutex::new(Some(worker)),
                    _ini_registration: None,
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
                    "Android netsim host worker stopped during startup",
                )
                .into())
            }
        }
    }
}

impl AndroidNetsimIo for GrpcAndroidNetsimHostIo {
    fn recv(&mut self) -> Result<Option<AndroidNetsimPacket>> {
        match self.incoming.recv() {
            Ok(result) => result,
            Err(_) => Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "Android netsim host response worker stopped unexpectedly",
            )
            .into()),
        }
    }

    fn send(&mut self, packet: AndroidNetsimPacket) -> Result<()> {
        self.outgoing
            .send(proto::PacketRequest {
                request_type: Some(proto::packet_request::RequestType::HciPacket(
                    packet_into_proto(packet),
                )),
            })
            .map_err(|_| {
                io::Error::new(
                    io::ErrorKind::BrokenPipe,
                    "Android netsim request stream is closed",
                )
                .into()
            })
    }
}

async fn run_host_worker(
    spec: AndroidNetsimSpec,
    outgoing: tokio_mpsc::UnboundedReceiver<proto::PacketRequest>,
    incoming: mpsc::Sender<Result<Option<AndroidNetsimPacket>>>,
    ready: mpsc::SyncSender<Result<()>>,
    mut shutdown: oneshot::Receiver<()>,
) {
    let mut client =
        match proto::packet_streamer_client::PacketStreamerClient::connect(spec.endpoint_uri())
            .await
        {
            Ok(client) => client,
            Err(error) => {
                let _ = ready.send(Err(error.into()));
                return;
            }
        };
    let mut stream = match client
        .stream_packets(UnboundedReceiverStream::new(outgoing))
        .await
    {
        Ok(response) => response.into_inner(),
        Err(error) => {
            let _ = ready.send(Err(error.into()));
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
                    Ok(Some(response)) => {
                        let result = match response.response_type {
                            Some(proto::packet_response::ResponseType::HciPacket(packet)) => {
                                packet_from_proto(packet).map(Some)
                            }
                            Some(proto::packet_response::ResponseType::Error(error)) => {
                                Err(Error::Remote(error))
                            }
                            Some(proto::packet_response::ResponseType::Packet(_)) | None => {
                                Err(Error::Remote("unsupported Android netsim response type".into()))
                            }
                        };
                        if incoming.send(result).is_err() {
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

#[derive(Default)]
struct ControllerState {
    next_lease: u64,
    lease: Option<u64>,
    outgoing:
        Option<tokio_mpsc::UnboundedSender<std::result::Result<proto::PacketResponse, Status>>>,
}

struct ControllerService {
    state: Arc<Mutex<ControllerState>>,
    incoming: mpsc::Sender<Result<Option<AndroidNetsimPacket>>>,
}

#[tonic::async_trait]
impl proto::packet_streamer_server::PacketStreamer for ControllerService {
    type StreamPacketsStream =
        UnboundedReceiverStream<std::result::Result<proto::PacketResponse, Status>>;

    async fn stream_packets(
        &self,
        request: Request<Streaming<proto::PacketRequest>>,
    ) -> std::result::Result<Response<Self::StreamPacketsStream>, Status> {
        let (outgoing, receiver) = tokio_mpsc::unbounded_channel();
        tokio::spawn(pump_controller_client(
            request.into_inner(),
            outgoing,
            Arc::clone(&self.state),
            self.incoming.clone(),
        ));
        Ok(Response::new(UnboundedReceiverStream::new(receiver)))
    }
}

struct ControllerLease {
    state: Arc<Mutex<ControllerState>>,
    id: u64,
}

impl Drop for ControllerLease {
    fn drop(&mut self) {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        if state.lease == Some(self.id) {
            state.lease = None;
            state.outgoing = None;
        }
    }
}

fn controller_error(message: impl Into<String>) -> proto::PacketResponse {
    proto::PacketResponse {
        response_type: Some(proto::packet_response::ResponseType::Error(message.into())),
    }
}

async fn pump_controller_client(
    mut requests: Streaming<proto::PacketRequest>,
    outgoing: tokio_mpsc::UnboundedSender<std::result::Result<proto::PacketResponse, Status>>,
    state: Arc<Mutex<ControllerState>>,
    incoming: mpsc::Sender<Result<Option<AndroidNetsimPacket>>>,
) {
    let initial = match requests.message().await {
        Ok(Some(request)) => request,
        Ok(None) => return,
        Err(error) => {
            let _ = incoming.send(Err(error.into()));
            return;
        }
    };
    let Some(proto::packet_request::RequestType::InitialInfo(info)) = initial.request_type else {
        let _ = outgoing.send(Ok(controller_error("Expected initial_info")));
        return;
    };
    let chip_kind = info.chip.as_ref().map(|chip| chip.kind).unwrap_or_default();
    if chip_kind != common_proto::ChipKind::Bluetooth as i32 {
        let _ = outgoing.send(Ok(controller_error("Unsupported chip type")));
        return;
    }

    let lease_id = {
        let mut state = state.lock().unwrap_or_else(|error| error.into_inner());
        if state.lease.is_some() {
            let _ = outgoing.send(Ok(controller_error("Device busy")));
            return;
        }
        state.next_lease = state.next_lease.wrapping_add(1);
        let lease_id = state.next_lease;
        state.lease = Some(lease_id);
        state.outgoing = Some(outgoing.clone());
        lease_id
    };
    let _lease = ControllerLease {
        state: Arc::clone(&state),
        id: lease_id,
    };

    loop {
        let request = match requests.message().await {
            Ok(Some(request)) => request,
            Ok(None) => break,
            Err(error) => {
                let _ = incoming.send(Err(error.into()));
                break;
            }
        };
        match request.request_type {
            Some(proto::packet_request::RequestType::HciPacket(packet)) => {
                match packet_from_proto(packet) {
                    Ok(packet) => {
                        if incoming.send(Ok(Some(packet))).is_err() {
                            break;
                        }
                    }
                    Err(error) => {
                        let _ = outgoing.send(Ok(controller_error(error.to_string())));
                    }
                }
            }
            _ => {
                let _ = outgoing.send(Ok(controller_error("Unexpected request type")));
            }
        }
    }
}

pub struct GrpcAndroidNetsimControllerIo {
    incoming: mpsc::Receiver<Result<Option<AndroidNetsimPacket>>>,
    state: Arc<Mutex<ControllerState>>,
    local_address: SocketAddr,
    lifetime: Arc<GrpcAndroidNetsimLifetime>,
}

impl GrpcAndroidNetsimControllerIo {
    pub fn bind(spec: &AndroidNetsimSpec) -> Result<Self> {
        let host = match spec.host.as_deref() {
            Some("_") | None => "127.0.0.1",
            Some(host) => host,
        };
        let listener = TcpListener::bind(format!("{host}:{}", spec.port))?;
        listener.set_nonblocking(true)?;
        let local_address = listener.local_addr()?;
        let state = Arc::new(Mutex::new(ControllerState::default()));
        let (incoming_sender, incoming) = mpsc::channel();
        let (ready_sender, ready) = mpsc::sync_channel(1);
        let (shutdown, shutdown_receiver) = oneshot::channel();
        let worker_state = Arc::clone(&state);
        let worker_incoming = incoming_sender.clone();
        let worker = thread::Builder::new()
            .name("bumble-android-netsim-controller".into())
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
                runtime.block_on(async move {
                    let listener = match tokio::net::TcpListener::from_std(listener) {
                        Ok(listener) => listener,
                        Err(error) => {
                            let _ = ready_sender.send(Err(error.into()));
                            return;
                        }
                    };
                    let service = ControllerService {
                        state: worker_state,
                        incoming: worker_incoming.clone(),
                    };
                    if ready_sender.send(Ok(())).is_err() {
                        return;
                    }
                    let server = tonic::transport::Server::builder()
                        .add_service(proto::packet_streamer_server::PacketStreamerServer::new(
                            service,
                        ))
                        .serve_with_incoming(TcpListenerStream::new(listener));
                    tokio::pin!(server);
                    tokio::select! {
                        result = &mut server => {
                            if let Err(error) = result {
                                let _ = worker_incoming.send(Err(error.into()));
                            }
                        }
                        _ = shutdown_receiver => {}
                    }
                });
            })?;

        match ready.recv() {
            Ok(Ok(())) => Ok(Self {
                incoming,
                state,
                local_address,
                lifetime: Arc::new(GrpcAndroidNetsimLifetime {
                    shutdown: Mutex::new(Some(shutdown)),
                    worker: Mutex::new(Some(worker)),
                    _ini_registration: NetsimIniRegistration::publish(
                        local_address.port(),
                        spec.instance,
                    ),
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
                    "Android netsim controller worker stopped during startup",
                )
                .into())
            }
        }
    }

    pub fn local_address(&self) -> SocketAddr {
        self.local_address
    }
}

impl AndroidNetsimIo for GrpcAndroidNetsimControllerIo {
    fn recv(&mut self) -> Result<Option<AndroidNetsimPacket>> {
        match self.incoming.recv() {
            Ok(result) => result,
            Err(_) => Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "Android netsim controller worker stopped unexpectedly",
            )
            .into()),
        }
    }

    fn send(&mut self, packet: AndroidNetsimPacket) -> Result<()> {
        let outgoing = self
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .outgoing
            .clone();
        let Some(outgoing) = outgoing else {
            return Ok(());
        };
        outgoing
            .send(Ok(proto::PacketResponse {
                response_type: Some(proto::packet_response::ResponseType::HciPacket(
                    packet_into_proto(packet),
                )),
            }))
            .map_err(|_| {
                io::Error::new(
                    io::ErrorKind::BrokenPipe,
                    "Android netsim response stream is closed",
                )
                .into()
            })
    }
}

pub struct AndroidNetsimPacketSource {
    incoming: mpsc::Receiver<Result<Option<AndroidNetsimPacket>>>,
    _lifetime: Arc<GrpcAndroidNetsimLifetime>,
}

impl PacketSource for AndroidNetsimPacketSource {
    fn read_packet(&mut self) -> Result<Option<HciPacket>> {
        match self.incoming.recv() {
            Ok(result) => result?.map(AndroidNetsimPacket::into_hci).transpose(),
            Err(_) => Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "Android netsim worker stopped unexpectedly",
            )
            .into()),
        }
    }
}

enum AndroidNetsimPacketSinkKind {
    Host(tokio_mpsc::UnboundedSender<proto::PacketRequest>),
    Controller(Arc<Mutex<ControllerState>>),
}

pub struct AndroidNetsimPacketSink {
    kind: AndroidNetsimPacketSinkKind,
    _lifetime: Arc<GrpcAndroidNetsimLifetime>,
}

impl PacketSink for AndroidNetsimPacketSink {
    fn write_packet(&mut self, packet: &HciPacket) -> Result<()> {
        let packet = AndroidNetsimPacket::from_hci(packet);
        match &self.kind {
            AndroidNetsimPacketSinkKind::Host(outgoing) => outgoing
                .send(proto::PacketRequest {
                    request_type: Some(proto::packet_request::RequestType::HciPacket(
                        packet_into_proto(packet),
                    )),
                })
                .map_err(|_| {
                    io::Error::new(
                        io::ErrorKind::BrokenPipe,
                        "Android netsim request stream is closed",
                    )
                    .into()
                }),
            AndroidNetsimPacketSinkKind::Controller(state) => {
                let outgoing = state
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .outgoing
                    .clone();
                let Some(outgoing) = outgoing else {
                    return Ok(());
                };
                outgoing
                    .send(Ok(proto::PacketResponse {
                        response_type: Some(proto::packet_response::ResponseType::HciPacket(
                            packet_into_proto(packet),
                        )),
                    }))
                    .map_err(|_| {
                        io::Error::new(
                            io::ErrorKind::BrokenPipe,
                            "Android netsim response stream is closed",
                        )
                        .into()
                    })
            }
        }
    }
}

pub enum SystemAndroidNetsimIo {
    Host(GrpcAndroidNetsimHostIo),
    Controller(GrpcAndroidNetsimControllerIo),
}

impl AndroidNetsimIo for SystemAndroidNetsimIo {
    fn recv(&mut self) -> Result<Option<AndroidNetsimPacket>> {
        match self {
            Self::Host(io) => io.recv(),
            Self::Controller(io) => io.recv(),
        }
    }

    fn send(&mut self, packet: AndroidNetsimPacket) -> Result<()> {
        match self {
            Self::Host(io) => io.send(packet),
            Self::Controller(io) => io.send(packet),
        }
    }
}

pub type SystemAndroidNetsimTransport = AndroidNetsimTransport<SystemAndroidNetsimIo>;

impl SystemAndroidNetsimTransport {
    pub fn open(parameters: Option<&str>) -> Result<Self> {
        let mut spec = AndroidNetsimSpec::parse(parameters)?;
        let io = match spec.mode {
            AndroidNetsimMode::Host => {
                if spec.port == 0 {
                    spec.port = find_netsim_grpc_port(spec.instance)?.ok_or_else(|| {
                        Error::InvalidSpec("Android netsim gRPC server port not found".into())
                    })?;
                }
                SystemAndroidNetsimIo::Host(GrpcAndroidNetsimHostIo::connect(spec.clone())?)
            }
            AndroidNetsimMode::Controller => {
                let controller = GrpcAndroidNetsimControllerIo::bind(&spec)?;
                spec.port = controller.local_address().port();
                SystemAndroidNetsimIo::Controller(controller)
            }
        };
        Ok(Self::from_io(io, spec))
    }

    pub fn try_split(self) -> (AndroidNetsimPacketSource, AndroidNetsimPacketSink) {
        match self.io {
            SystemAndroidNetsimIo::Host(GrpcAndroidNetsimHostIo {
                outgoing,
                incoming,
                lifetime,
            }) => (
                AndroidNetsimPacketSource {
                    incoming,
                    _lifetime: Arc::clone(&lifetime),
                },
                AndroidNetsimPacketSink {
                    kind: AndroidNetsimPacketSinkKind::Host(outgoing),
                    _lifetime: lifetime,
                },
            ),
            SystemAndroidNetsimIo::Controller(GrpcAndroidNetsimControllerIo {
                incoming,
                state,
                local_address: _,
                lifetime,
            }) => (
                AndroidNetsimPacketSource {
                    incoming,
                    _lifetime: Arc::clone(&lifetime),
                },
                AndroidNetsimPacketSink {
                    kind: AndroidNetsimPacketSinkKind::Controller(state),
                    _lifetime: lifetime,
                },
            ),
        }
    }
}
