use bumble::{Address, AddressType, Uuid};
use bumble_hci::Command;
use bumble_host::{Device, LocalLink};
use bumble_l2cap::{ClassicChannelSpec, ClassicChannelState};
use bumble_rfcomm::mux::{DlcState, Multiplexer, MultiplexerState, Role};
use bumble_rfcomm::{
    RfcommFrame, RFCOMM_DEFAULT_INITIAL_CREDITS, RFCOMM_DEFAULT_MAX_FRAME_SIZE,
    RFCOMM_DYNAMIC_CHANNEL_NUMBER_END, RFCOMM_DYNAMIC_CHANNEL_NUMBER_START, RFCOMM_PSM,
};
use bumble_sdp::service::{
    AttributeId, SdpClient, SdpRequestHandler, SdpServer, SdpTransport, TransportError,
};
use bumble_sdp::{public_browse_root, DataElement, SdpPdu, ServiceAttribute, SDP_PSM};
use bumble_smp::PairingConfig;
use bumble_transport::{
    open_split_transport, ClassicPairingSession, CommandResponse, ExternalHost,
    ExternalHostActivity,
};
use std::collections::{BTreeMap, VecDeque};
use std::io::{self, ErrorKind, Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Duration, Instant};

const DEFAULT_RFCOMM_UUID: &str = "E6D55659-C8B4-4B85-96BB-B1143AF6D3AE";
const DEFAULT_CLIENT_TCP_PORT: u16 = 9544;
const DEFAULT_SERVER_TCP_PORT: u16 = 9545;
const CLASSIC_L2CAP_MTU: u16 = 2048;
const TRACE_MAX_SIZE: usize = 48;
const TCP_READ_CHUNK: usize = 4096;
const COMMAND_TIMEOUT: Duration = Duration::from_secs(5);
const PROCEDURE_TIMEOUT: Duration = Duration::from_secs(30);
const TCP_CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const POLL_INTERVAL: Duration = Duration::from_millis(10);

#[derive(Clone, Debug, PartialEq, Eq)]
enum Mode {
    Server {
        tcp_host: String,
        tcp_port: u16,
    },
    Client {
        bluetooth_address: String,
        tcp_host: String,
        tcp_port: u16,
        authenticate: bool,
        encrypt: bool,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Args {
    device_config: Option<PathBuf>,
    hci_transport: String,
    trace: bool,
    channel: u8,
    uuid: Uuid,
    mode: Mode,
}

fn usage() -> &'static str {
    "usage: bumble-rfcomm-bridge [--device-config PATH] --hci-transport TRANSPORT [--trace] [--channel 0..30] [--uuid UUID] <server [--tcp-host HOST] [--tcp-port PORT] | client BLUETOOTH-ADDRESS [--tcp-host HOST] [--tcp-port PORT] [--authenticate] [--encrypt]>"
}

fn option_value(
    argument: &str,
    option: &str,
    arguments: &mut VecDeque<String>,
) -> Result<Option<String>, String> {
    if argument == option {
        return arguments
            .pop_front()
            .map(Some)
            .ok_or_else(|| format!("missing value for {option}"));
    }
    Ok(argument
        .strip_prefix(&format!("{option}="))
        .map(ToOwned::to_owned))
}

fn u16_value(value: String, option: &str) -> Result<u16, String> {
    value
        .parse::<u16>()
        .map_err(|_| format!("invalid value {value:?} for {option}"))
}

fn parse_mode(mode: &str, mut arguments: VecDeque<String>) -> Result<Mode, String> {
    let (mut tcp_host, mut tcp_port) = if mode == "server" {
        ("localhost".to_string(), DEFAULT_SERVER_TCP_PORT)
    } else {
        ("_".to_string(), DEFAULT_CLIENT_TCP_PORT)
    };
    let mut address = None;
    let mut authenticate = false;
    let mut encrypt = false;
    while let Some(argument) = arguments.pop_front() {
        if matches!(argument.as_str(), "-h" | "--help") {
            return Err(usage().into());
        }
        if argument == "--authenticate" && mode == "client" {
            authenticate = true;
            continue;
        }
        if argument == "--encrypt" && mode == "client" {
            encrypt = true;
            continue;
        }
        if let Some(value) = option_value(&argument, "--tcp-host", &mut arguments)? {
            tcp_host = value;
            continue;
        }
        if let Some(value) = option_value(&argument, "--tcp-port", &mut arguments)? {
            tcp_port = u16_value(value, "--tcp-port")?;
            continue;
        }
        if argument.starts_with('-') {
            return Err(format!("unknown option {argument:?}"));
        }
        if mode == "server" || address.replace(argument).is_some() {
            return Err(usage().into());
        }
    }
    match mode {
        "server" => Ok(Mode::Server { tcp_host, tcp_port }),
        "client" => Ok(Mode::Client {
            bluetooth_address: address.ok_or_else(|| usage().to_string())?,
            tcp_host,
            tcp_port,
            authenticate,
            encrypt,
        }),
        _ => Err(usage().into()),
    }
}

fn parse_args(arguments: impl IntoIterator<Item = String>) -> Result<Args, String> {
    let mut arguments: VecDeque<_> = arguments.into_iter().skip(1).collect();
    let mut device_config = None;
    let mut hci_transport = None;
    let mut trace = false;
    let mut channel = 0;
    let mut uuid = Uuid::parse(DEFAULT_RFCOMM_UUID).expect("default UUID is valid");
    let mode = loop {
        let argument = arguments.pop_front().ok_or_else(|| usage().to_string())?;
        if matches!(argument.as_str(), "server" | "client") {
            break argument;
        }
        if matches!(argument.as_str(), "-h" | "--help") {
            return Err(usage().into());
        }
        if argument == "--trace" {
            trace = true;
            continue;
        }
        if let Some(value) = option_value(&argument, "--device-config", &mut arguments)? {
            device_config = Some(PathBuf::from(value));
            continue;
        }
        if let Some(value) = option_value(&argument, "--hci-transport", &mut arguments)? {
            hci_transport = Some(value);
            continue;
        }
        if let Some(value) = option_value(&argument, "--channel", &mut arguments)? {
            channel = u8::try_from(u16_value(value, "--channel")?)
                .map_err(|_| "RFCOMM channel must be between 0 and 30".to_string())?;
            continue;
        }
        if let Some(value) = option_value(&argument, "--uuid", &mut arguments)? {
            uuid = Uuid::parse(&value).map_err(|error| error.to_string())?;
            continue;
        }
        return Err(if argument.starts_with('-') {
            format!("unknown option {argument:?}")
        } else {
            usage().into()
        });
    };
    if channel > RFCOMM_DYNAMIC_CHANNEL_NUMBER_END {
        return Err("RFCOMM channel must be between 0 and 30".into());
    }
    Ok(Args {
        device_config,
        hci_transport: hci_transport.ok_or_else(|| "--hci-transport is required".to_string())?,
        trace,
        channel,
        uuid,
        mode: parse_mode(&mode, arguments)?,
    })
}

fn configured_name(path: Option<&Path>) -> Result<String, String> {
    let Some(path) = path else {
        return Ok("Bumble".into());
    };
    let bytes = std::fs::read(path)
        .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
    let config: serde_json::Value = serde_json::from_slice(&bytes)
        .map_err(|error| format!("invalid device config: {error}"))?;
    Ok(config
        .get("name")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("Bumble")
        .to_string())
}

fn require_success(response: CommandResponse, context: &str) -> Result<CommandResponse, String> {
    if response.status() == Some(0) {
        Ok(response)
    } else {
        Err(format!(
            "{context} failed with HCI status {:?}",
            response.status()
        ))
    }
}

fn command(
    host: &mut ExternalHost,
    command: Command,
    context: &str,
) -> Result<CommandResponse, String> {
    require_success(
        host.send_command(command, COMMAND_TIMEOUT)
            .map_err(|error| error.to_string())?,
        context,
    )
}

fn set_available(host: &mut ExternalHost, name: &str, available: bool) -> Result<(), String> {
    if available {
        let mut local_name = [0; 248];
        let bytes = name.as_bytes();
        let length = bytes.len().min(local_name.len());
        local_name[..length].copy_from_slice(&bytes[..length]);
        command(
            host,
            Command::WriteLocalName { local_name },
            "writing local name",
        )?;
    }
    command(
        host,
        Command::WriteScanEnable {
            scan_enable: if available { 0x03 } else { 0x00 },
        },
        if available {
            "enabling inquiry and page scans"
        } else {
            "disabling inquiry and page scans"
        },
    )?;
    Ok(())
}

fn wait_for_classic_connection(
    host: &mut ExternalHost,
    device: &mut Device,
    peer: Option<&Address>,
    accept_requests: bool,
    timeout: Option<Duration>,
) -> Result<Option<u16>, String> {
    let deadline = timeout.map(|timeout| Instant::now() + timeout);
    loop {
        device.poll(host);
        let handle = match peer {
            Some(peer) => device.classic_connection_handle_for_peer(peer),
            None => device.classic_connection_handle(),
        };
        if handle.is_some() {
            return Ok(handle);
        }
        if accept_requests {
            for request in device.take_classic_connection_requests() {
                device.accept_classic(host, request);
            }
        }
        let wait = deadline
            .map(|deadline| deadline.saturating_duration_since(Instant::now()))
            .unwrap_or(Duration::from_secs(1));
        if wait.is_zero() {
            return Err("timed out waiting for Classic connection".into());
        }
        match host
            .wait_for_activity(wait)
            .map_err(|error| error.to_string())?
        {
            ExternalHostActivity::Packet | ExternalHostActivity::Timeout if deadline.is_none() => {}
            ExternalHostActivity::Packet => {}
            ExternalHostActivity::Timeout => {
                return Err("timed out waiting for Classic connection".into())
            }
            ExternalHostActivity::Ended => return Ok(None),
        }
    }
}

fn connect_classic(
    host: &mut ExternalHost,
    device: &mut Device,
    peer: Address,
) -> Result<u16, String> {
    device.connect_classic(host, peer.clone());
    wait_for_classic_connection(host, device, Some(&peer), false, Some(PROCEDURE_TIMEOUT))?
        .ok_or_else(|| "HCI transport ended while connecting".into())
}

fn authenticate_classic(
    host: &mut ExternalHost,
    device: &mut Device,
    connection_handle: u16,
) -> Result<(), String> {
    let mut pairing = ClassicPairingSession::accept_all(
        device,
        connection_handle,
        PairingConfig {
            bonding: false,
            mitm: false,
            ..PairingConfig::default()
        },
        None,
    )
    .map_err(|error| error.to_string())?;
    pairing
        .pair(host, device, PROCEDURE_TIMEOUT)
        .map_err(|error| error.to_string())?;
    Ok(())
}

fn encrypt_classic(
    host: &mut ExternalHost,
    device: &mut Device,
    connection_handle: u16,
) -> Result<(), String> {
    if device.is_classic_encrypted_on_handle(connection_handle) {
        return Ok(());
    }
    if !device.set_classic_encryption_on_handle(host, connection_handle, true) {
        return Err("failed to request Classic encryption".into());
    }
    let deadline = Instant::now() + PROCEDURE_TIMEOUT;
    loop {
        device.poll(host);
        if device.is_classic_encrypted_on_handle(connection_handle) {
            return Ok(());
        }
        if device.classic_connection(connection_handle).is_none() {
            return Err("Classic connection ended before encryption completed".into());
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err("timed out waiting for Classic encryption".into());
        }
        match host
            .wait_for_activity(remaining)
            .map_err(|error| error.to_string())?
        {
            ExternalHostActivity::Packet => {}
            ExternalHostActivity::Timeout => {
                return Err("timed out waiting for Classic encryption".into())
            }
            ExternalHostActivity::Ended => {
                return Err("transport ended before Classic encryption completed".into())
            }
        }
    }
}

fn wait_for_classic_channel(
    host: &mut ExternalHost,
    device: &mut Device,
    connection_handle: u16,
    source_cid: u16,
) -> Result<(), String> {
    let deadline = Instant::now() + PROCEDURE_TIMEOUT;
    loop {
        device.poll(host);
        let channel = device
            .classic_channel(connection_handle, source_cid)
            .ok_or_else(|| "Classic channel disappeared".to_string())?;
        match channel.state {
            ClassicChannelState::Open => return Ok(()),
            ClassicChannelState::Closed => {
                return Err(format!(
                    "Classic channel connection failed with result {:?}",
                    channel.connection_result
                ))
            }
            _ => {}
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err("timed out opening Classic L2CAP channel".into());
        }
        match host
            .wait_for_activity(remaining)
            .map_err(|error| error.to_string())?
        {
            ExternalHostActivity::Packet => {}
            ExternalHostActivity::Timeout => {
                return Err("timed out opening Classic L2CAP channel".into())
            }
            ExternalHostActivity::Ended => {
                return Err("transport ended while opening Classic L2CAP channel".into())
            }
        }
    }
}

fn service_record(uuid: Uuid, channel: u8) -> Vec<ServiceAttribute> {
    vec![
        ServiceAttribute {
            id: 0x0000,
            value: DataElement::unsigned_integer_32(0x0001_0001),
        },
        ServiceAttribute {
            id: 0x0005,
            value: DataElement::sequence([DataElement::uuid(public_browse_root())]),
        },
        ServiceAttribute {
            id: 0x0004,
            value: DataElement::sequence([
                DataElement::sequence([DataElement::uuid(Uuid::from_16_bits(0x0100))]),
                DataElement::sequence([
                    DataElement::uuid(Uuid::from_16_bits(RFCOMM_PSM)),
                    DataElement::unsigned_integer_8(channel),
                ]),
            ]),
        },
        ServiceAttribute {
            id: 0x0001,
            value: DataElement::sequence([DataElement::uuid(uuid)]),
        },
    ]
}

fn rfcomm_channel(attributes: &[ServiceAttribute]) -> Option<u8> {
    let DataElement::Sequence(protocols) = ServiceAttribute::find(attributes, 0x0004)? else {
        return None;
    };
    let DataElement::Sequence(rfcomm) = protocols.get(1)? else {
        return None;
    };
    if rfcomm.first()? != &DataElement::uuid(Uuid::from_16_bits(RFCOMM_PSM)) {
        return None;
    }
    match rfcomm.get(1)? {
        DataElement::UnsignedInteger { value, .. } => u8::try_from(*value).ok(),
        _ => None,
    }
}

struct ExternalSdpTransport<'a> {
    host: &'a mut ExternalHost,
    device: &'a mut Device,
    connection_handle: u16,
    source_cid: u16,
}

impl SdpTransport for ExternalSdpTransport<'_> {
    fn request(&mut self, request: &SdpPdu) -> Result<SdpPdu, TransportError> {
        let bytes = request
            .to_bytes()
            .map_err(|error| TransportError(error.to_string()))?;
        self.device
            .send_classic_channel_sdu(self.host, self.connection_handle, self.source_cid, &bytes)
            .map_err(|error| TransportError(error.to_string()))?;
        let deadline = Instant::now() + PROCEDURE_TIMEOUT;
        loop {
            self.device.poll(self.host);
            if let Some(bytes) = self
                .device
                .take_classic_channel_sdus(self.connection_handle, self.source_cid)
                .into_iter()
                .next()
            {
                return SdpPdu::from_bytes(&bytes)
                    .map_err(|error| TransportError(error.to_string()));
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(TransportError("timed out waiting for SDP response".into()));
            }
            match self
                .host
                .wait_for_activity(remaining)
                .map_err(|error| TransportError(error.to_string()))?
            {
                ExternalHostActivity::Packet => {}
                ExternalHostActivity::Timeout => {
                    return Err(TransportError("timed out waiting for SDP response".into()))
                }
                ExternalHostActivity::Ended => {
                    return Err(TransportError(
                        "transport ended while waiting for SDP response".into(),
                    ))
                }
            }
        }
    }
}

fn resolve_rfcomm_channel(
    host: &mut ExternalHost,
    device: &mut Device,
    connection_handle: u16,
    uuid: &Uuid,
) -> Result<u8, String> {
    let source_cid = device
        .connect_classic_channel(
            host,
            connection_handle,
            u32::from(SDP_PSM),
            ClassicChannelSpec {
                mtu: CLASSIC_L2CAP_MTU,
            },
        )
        .map_err(|error| error.to_string())?;
    wait_for_classic_channel(host, device, connection_handle, source_cid)?;
    let services = {
        let transport = ExternalSdpTransport {
            host,
            device,
            connection_handle,
            source_cid,
        };
        SdpClient::new(transport)
            .service_search_attribute(
                std::slice::from_ref(uuid),
                &[AttributeId::Range(0x0000, 0xFFFF)],
            )
            .map_err(|error| error.to_string())?
    };
    device
        .disconnect_classic_channel(host, connection_handle, source_cid)
        .map_err(|error| error.to_string())?;
    services
        .iter()
        .find_map(|attributes| rfcomm_channel(attributes))
        .ok_or_else(|| format!("RFCOMM channel with UUID {uuid:?} not found"))
}

struct SdpEndpoint {
    source_cid: u16,
    server: SdpServer,
}

impl SdpEndpoint {
    fn new(source_cid: u16, peer_mtu: u16, uuid: Uuid, channel: u8) -> Self {
        let mut server = SdpServer::new(peer_mtu);
        server.add_service(0x0001_0001, service_record(uuid, channel));
        Self { source_cid, server }
    }

    fn poll(
        &mut self,
        host: &mut LocalLink,
        device: &mut Device,
        connection_handle: u16,
    ) -> Result<(), String> {
        for bytes in device.take_classic_channel_sdus(connection_handle, self.source_cid) {
            let request = SdpPdu::from_bytes(&bytes).map_err(|error| error.to_string())?;
            let response = self.server.handle_request(&request);
            device
                .send_classic_channel_sdu(
                    host,
                    connection_handle,
                    self.source_cid,
                    &response.to_bytes().map_err(|error| error.to_string())?,
                )
                .map_err(|error| error.to_string())?;
        }
        Ok(())
    }
}

struct RfcommSession {
    connection_handle: u16,
    source_cid: u16,
    multiplexer: Multiplexer,
}

impl RfcommSession {
    fn new(
        device: &Device,
        connection_handle: u16,
        source_cid: u16,
        role: Role,
    ) -> Result<Self, String> {
        let channel = device
            .classic_channel(connection_handle, source_cid)
            .filter(|channel| channel.state == ClassicChannelState::Open)
            .ok_or_else(|| "RFCOMM L2CAP channel is not open".to_string())?;
        Ok(Self {
            connection_handle,
            source_cid,
            multiplexer: Multiplexer::new(role, channel.peer_mtu),
        })
    }

    fn flush(&mut self, host: &mut LocalLink, device: &mut Device) -> Result<(), String> {
        for frame in self.multiplexer.drain_outgoing() {
            device
                .send_classic_channel_sdu(
                    host,
                    self.connection_handle,
                    self.source_cid,
                    &frame.to_bytes().map_err(|error| error.to_string())?,
                )
                .map_err(|error| error.to_string())?;
        }
        Ok(())
    }

    fn poll(&mut self, host: &mut LocalLink, device: &mut Device) -> Result<(), String> {
        for bytes in device.take_classic_channel_sdus(self.connection_handle, self.source_cid) {
            self.multiplexer
                .on_pdu(&RfcommFrame::from_bytes(&bytes).map_err(|error| error.to_string())?);
        }
        self.flush(host, device)
    }
}

fn wait_for_rfcomm_session(
    host: &mut ExternalHost,
    device: &mut Device,
    session: &mut RfcommSession,
) -> Result<(), String> {
    let deadline = Instant::now() + PROCEDURE_TIMEOUT;
    loop {
        device.poll(host);
        session.poll(host, device)?;
        match session.multiplexer.state() {
            MultiplexerState::Connected => return Ok(()),
            MultiplexerState::Disconnected => return Err("RFCOMM session was refused".into()),
            _ => {}
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err("timed out opening RFCOMM session".into());
        }
        match host
            .wait_for_activity(remaining)
            .map_err(|error| error.to_string())?
        {
            ExternalHostActivity::Packet => {}
            ExternalHostActivity::Timeout => return Err("timed out opening RFCOMM session".into()),
            ExternalHostActivity::Ended => {
                return Err("transport ended while opening RFCOMM session".into())
            }
        }
    }
}

struct Tracer {
    name: &'static str,
    last: Option<Instant>,
}

impl Tracer {
    fn new(name: &'static str) -> Self {
        Self { name, last: None }
    }

    fn trace(&mut self, data: &[u8]) {
        let now = Instant::now();
        let elapsed = self.last.map(|last| now.duration_since(last));
        let elapsed_ms = elapsed.map_or(0, |elapsed| elapsed.as_millis());
        let throughput = elapsed
            .filter(|elapsed| !elapsed.is_zero())
            .map_or(0.0, |elapsed| {
                data.len() as f64 / elapsed.as_secs_f64() / 1000.0
            });
        let mut encoded: String = data[..data.len().min(TRACE_MAX_SIZE)]
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        if data.len() > TRACE_MAX_SIZE {
            encoded.push_str("...");
        }
        println!(
            "[{}] {:4} bytes (+{:4}ms, {:7.2}kB/s) {}",
            self.name,
            data.len(),
            elapsed_ms,
            throughput,
            encoded
        );
        self.last = Some(now);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PipeState {
    Pending,
    Open,
    Closed,
}

struct RfcommPipe {
    dlci: u8,
    stream: TcpStream,
    pending_to_tcp: VecDeque<u8>,
    reading_paused: bool,
    closing: bool,
    rfcomm_to_tcp: Option<Tracer>,
    tcp_to_rfcomm: Option<Tracer>,
}

impl RfcommPipe {
    fn new(dlci: u8, stream: TcpStream, trace: bool) -> io::Result<Self> {
        stream.set_nonblocking(true)?;
        stream.set_nodelay(true)?;
        Ok(Self {
            dlci,
            stream,
            pending_to_tcp: VecDeque::new(),
            reading_paused: false,
            closing: false,
            rfcomm_to_tcp: trace.then(|| Tracer::new("RFCOMM->TCP")),
            tcp_to_rfcomm: trace.then(|| Tracer::new("TCP->RFCOMM")),
        })
    }

    fn flush_tcp(&mut self) -> io::Result<()> {
        while !self.pending_to_tcp.is_empty() {
            let front = self.pending_to_tcp.as_slices().0;
            match self.stream.write(front) {
                Ok(0) => return Err(io::Error::new(ErrorKind::BrokenPipe, "TCP socket closed")),
                Ok(written) => {
                    self.pending_to_tcp.drain(..written);
                }
                Err(error) if error.kind() == ErrorKind::WouldBlock => break,
                Err(error) => return Err(error),
            }
        }
        Ok(())
    }

    fn start_close(&mut self, session: &mut RfcommSession) -> Result<(), String> {
        if !self.closing && session.multiplexer.dlc_state(self.dlci) == Some(DlcState::Connected) {
            session
                .multiplexer
                .disconnect_dlc(self.dlci)
                .map_err(|error| error.to_string())?;
        }
        self.closing = true;
        let _ = self.stream.shutdown(Shutdown::Both);
        Ok(())
    }

    fn pump(
        &mut self,
        host: &mut LocalLink,
        device: &mut Device,
        session: &mut RfcommSession,
    ) -> Result<PipeState, String> {
        let dlc_state = session.multiplexer.dlc_state(self.dlci);
        if dlc_state.is_none() {
            if !self.closing && session.multiplexer.state() == MultiplexerState::Opening {
                return Ok(PipeState::Pending);
            }
            return Ok(PipeState::Closed);
        }
        if self.closing {
            return Ok(PipeState::Open);
        }
        if dlc_state != Some(DlcState::Connected) {
            return Ok(PipeState::Pending);
        }

        for packet in session.multiplexer.take_rx(self.dlci) {
            if let Some(tracer) = &mut self.rfcomm_to_tcp {
                tracer.trace(&packet);
            }
            self.pending_to_tcp.extend(packet);
        }
        if !self.pending_to_tcp.is_empty() {
            if let Err(error) = self.flush_tcp() {
                eprintln!("!!! TCP write failed: {error}");
                self.start_close(session)?;
                session.flush(host, device)?;
                return Ok(PipeState::Open);
            }
        }
        let paused = !self.pending_to_tcp.is_empty();
        if self.reading_paused != paused {
            session
                .multiplexer
                .set_dlc_reading_paused(self.dlci, paused)
                .map_err(|error| error.to_string())?;
            self.reading_paused = paused;
            session.flush(host, device)?;
        }
        if paused
            || session.multiplexer.dlc_pending_tx(self.dlci) != Some(0)
            || !device.classic_channel_output_is_drained(session.connection_handle)
        {
            return Ok(PipeState::Open);
        }

        let mut buffer = [0; TCP_READ_CHUNK];
        match self.stream.read(&mut buffer) {
            Ok(0) => {
                self.start_close(session)?;
                session.flush(host, device)?;
                Ok(PipeState::Open)
            }
            Ok(read) => {
                if let Some(tracer) = &mut self.tcp_to_rfcomm {
                    tracer.trace(&buffer[..read]);
                }
                session
                    .multiplexer
                    .write(self.dlci, &buffer[..read])
                    .map_err(|error| error.to_string())?;
                session.flush(host, device)?;
                Ok(PipeState::Open)
            }
            Err(error) if error.kind() == ErrorKind::WouldBlock => Ok(PipeState::Open),
            Err(error) => {
                eprintln!("!!! TCP read failed: {error}");
                self.start_close(session)?;
                session.flush(host, device)?;
                Ok(PipeState::Open)
            }
        }
    }
}

fn connect_tcp(host: &str, port: u16) -> io::Result<TcpStream> {
    let addresses: Vec<_> = (host, port).to_socket_addrs()?.collect();
    let mut last_error = None;
    for address in addresses {
        match TcpStream::connect_timeout(&address, TCP_CONNECT_TIMEOUT) {
            Ok(stream) => return Ok(stream),
            Err(error) => last_error = Some(error),
        }
    }
    Err(last_error.unwrap_or_else(|| {
        io::Error::new(ErrorKind::AddrNotAvailable, "host resolved to no addresses")
    }))
}

fn wait_tick(host: &mut ExternalHost) -> Result<bool, String> {
    match host
        .wait_for_activity(POLL_INTERVAL)
        .map_err(|error| error.to_string())?
    {
        ExternalHostActivity::Packet | ExternalHostActivity::Timeout => Ok(true),
        ExternalHostActivity::Ended => Ok(false),
    }
}

fn check_classic_channel_errors(device: &mut Device) -> Result<(), String> {
    if let Some((handle, error)) = device.take_classic_channel_errors().into_iter().next() {
        Err(format!(
            "Classic L2CAP error on handle {handle:#06x}: {error}"
        ))
    } else {
        Ok(())
    }
}

struct ServerRfcommSession {
    runtime: RfcommSession,
    pipes: BTreeMap<u8, RfcommPipe>,
}

impl ServerRfcommSession {
    fn new(runtime: RfcommSession, channel: u8) -> Self {
        let mut runtime = runtime;
        runtime.multiplexer.listen(
            channel,
            RFCOMM_DEFAULT_MAX_FRAME_SIZE,
            u16::from(RFCOMM_DEFAULT_INITIAL_CREDITS),
        );
        Self {
            runtime,
            pipes: BTreeMap::new(),
        }
    }

    fn pump(
        &mut self,
        host: &mut LocalLink,
        device: &mut Device,
        trace: bool,
        tcp_host: &str,
        tcp_port: u16,
    ) -> Result<(), String> {
        self.runtime.poll(host, device)?;
        for dlci in self.runtime.multiplexer.take_opened() {
            println!("*** RFCOMM DLC {dlci}");
            match connect_tcp(tcp_host, tcp_port) {
                Ok(stream) => {
                    println!("### Connected to TCP {tcp_host}:{tcp_port}");
                    self.pipes.insert(
                        dlci,
                        RfcommPipe::new(dlci, stream, trace).map_err(|error| error.to_string())?,
                    );
                }
                Err(error) => {
                    eprintln!("!!! TCP connection failed: {error}");
                    self.runtime
                        .multiplexer
                        .disconnect_dlc(dlci)
                        .map_err(|error| error.to_string())?;
                    self.runtime.flush(host, device)?;
                }
            }
        }
        let dlcis: Vec<_> = self.pipes.keys().copied().collect();
        for dlci in dlcis {
            let state = self
                .pipes
                .get_mut(&dlci)
                .expect("DLCI came from the map")
                .pump(host, device, &mut self.runtime)?;
            if state == PipeState::Closed {
                println!("*** RFCOMM DLC {dlci} closed");
                self.pipes.remove(&dlci);
            }
        }
        Ok(())
    }
}

struct ServerConnectionOptions<'a> {
    channel: u8,
    uuid: &'a Uuid,
    trace: bool,
    tcp_host: &'a str,
    tcp_port: u16,
}

fn run_server_connection(
    host: &mut ExternalHost,
    device: &mut Device,
    connection_handle: u16,
    options: &ServerConnectionOptions<'_>,
) -> Result<bool, String> {
    let mut pairing = ClassicPairingSession::accept_all(
        device,
        connection_handle,
        PairingConfig {
            bonding: false,
            ..PairingConfig::default()
        },
        None,
    )
    .map_err(|error| error.to_string())?;
    pairing.listen(device).map_err(|error| error.to_string())?;
    let mut pairing_active = true;
    let mut sdp_endpoints = BTreeMap::new();
    let mut rfcomm_sessions = BTreeMap::new();
    loop {
        device.poll(host);
        if pairing_active
            && pairing
                .drive_once(host, device)
                .map_err(|error| error.to_string())?
                .is_some()
        {
            pairing_active = false;
        }
        check_classic_channel_errors(device)?;
        for source_cid in device.take_accepted_classic_channels(connection_handle) {
            let channel_info = device
                .classic_channel(connection_handle, source_cid)
                .ok_or_else(|| "accepted Classic channel disappeared".to_string())?;
            match u16::try_from(channel_info.psm).ok() {
                Some(SDP_PSM) => {
                    sdp_endpoints.insert(
                        source_cid,
                        SdpEndpoint::new(
                            source_cid,
                            channel_info.peer_mtu,
                            options.uuid.clone(),
                            options.channel,
                        ),
                    );
                }
                Some(RFCOMM_PSM) => {
                    let runtime =
                        RfcommSession::new(device, connection_handle, source_cid, Role::Responder)?;
                    rfcomm_sessions.insert(
                        source_cid,
                        ServerRfcommSession::new(runtime, options.channel),
                    );
                }
                _ => {}
            }
        }
        let sdp_cids: Vec<_> = sdp_endpoints.keys().copied().collect();
        for source_cid in sdp_cids {
            if device
                .classic_channel(connection_handle, source_cid)
                .is_none_or(|channel| channel.state == ClassicChannelState::Closed)
            {
                sdp_endpoints.remove(&source_cid);
            } else {
                sdp_endpoints
                    .get_mut(&source_cid)
                    .expect("CID came from the map")
                    .poll(host, device, connection_handle)?;
            }
        }
        let rfcomm_cids: Vec<_> = rfcomm_sessions.keys().copied().collect();
        for source_cid in rfcomm_cids {
            if device
                .classic_channel(connection_handle, source_cid)
                .is_none_or(|channel| channel.state == ClassicChannelState::Closed)
            {
                rfcomm_sessions.remove(&source_cid);
            } else {
                rfcomm_sessions
                    .get_mut(&source_cid)
                    .expect("CID came from the map")
                    .pump(
                        host,
                        device,
                        options.trace,
                        options.tcp_host,
                        options.tcp_port,
                    )?;
            }
        }
        if device.classic_connection(connection_handle).is_none() {
            return Ok(true);
        }
        if !wait_tick(host)? {
            return Ok(false);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn run_server(
    host: &mut ExternalHost,
    device: &mut Device,
    name: &str,
    requested_channel: u8,
    uuid: &Uuid,
    trace: bool,
    tcp_host: &str,
    tcp_port: u16,
) -> Result<(), String> {
    let channel = if requested_channel == 0 {
        RFCOMM_DYNAMIC_CHANNEL_NUMBER_START
    } else {
        requested_channel
    };
    device
        .register_classic_channel_server(
            Some(u32::from(SDP_PSM)),
            ClassicChannelSpec {
                mtu: CLASSIC_L2CAP_MTU,
            },
        )
        .map_err(|error| error.to_string())?;
    device
        .register_classic_channel_server(
            Some(u32::from(RFCOMM_PSM)),
            ClassicChannelSpec {
                mtu: CLASSIC_L2CAP_MTU,
            },
        )
        .map_err(|error| error.to_string())?;
    println!("### Listening for RFCOMM channel {channel}");
    loop {
        set_available(host, name, true)?;
        let Some(connection_handle) = wait_for_classic_connection(host, device, None, true, None)?
        else {
            return Ok(());
        };
        println!("@@@ Bluetooth connection on handle {connection_handle:#06x}");
        set_available(host, name, false)?;
        let options = ServerConnectionOptions {
            channel,
            uuid,
            trace,
            tcp_host,
            tcp_port,
        };
        if !run_server_connection(host, device, connection_handle, &options)? {
            return Ok(());
        }
        println!("@@@ Bluetooth disconnection");
    }
}

struct ClientSession {
    runtime: RfcommSession,
    channel: u8,
}

#[allow(clippy::too_many_arguments)]
fn establish_client_session(
    host: &mut ExternalHost,
    device: &mut Device,
    bluetooth_address: &str,
    requested_channel: u8,
    uuid: &Uuid,
    authenticate: bool,
    encrypt: bool,
) -> Result<ClientSession, String> {
    let peer = Address::parse(bluetooth_address, AddressType::PUBLIC_DEVICE)
        .map_err(|error| error.to_string())?;
    let connection_handle =
        if let Some(connection_handle) = device.classic_connection_handle_for_peer(&peer) {
            connection_handle
        } else {
            println!("@@@ Connecting to Bluetooth {peer}");
            connect_classic(host, device, peer)?
        };
    println!("@@@ Bluetooth connection on handle {connection_handle:#06x}");
    if authenticate || encrypt {
        println!("@@@ Authenticating Bluetooth connection");
        authenticate_classic(host, device, connection_handle)?;
    }
    if encrypt {
        println!("@@@ Encrypting Bluetooth connection");
        encrypt_classic(host, device, connection_handle)?;
    }
    let channel = if requested_channel == 0 {
        let channel = resolve_rfcomm_channel(host, device, connection_handle, uuid)?;
        println!("### Found RFCOMM channel {channel}");
        channel
    } else {
        requested_channel
    };
    let source_cid = device
        .connect_classic_channel(
            host,
            connection_handle,
            u32::from(RFCOMM_PSM),
            ClassicChannelSpec {
                mtu: CLASSIC_L2CAP_MTU,
            },
        )
        .map_err(|error| error.to_string())?;
    wait_for_classic_channel(host, device, connection_handle, source_cid)?;
    let mut runtime = RfcommSession::new(device, connection_handle, source_cid, Role::Initiator)?;
    runtime
        .multiplexer
        .connect()
        .map_err(|error| error.to_string())?;
    runtime.flush(host, device)?;
    wait_for_rfcomm_session(host, device, &mut runtime)?;
    Ok(ClientSession { runtime, channel })
}

#[allow(clippy::too_many_arguments)]
fn run_client(
    host: &mut ExternalHost,
    device: &mut Device,
    bluetooth_address: &str,
    requested_channel: u8,
    uuid: &Uuid,
    trace: bool,
    tcp_host: &str,
    tcp_port: u16,
    authenticate: bool,
    encrypt: bool,
) -> Result<(), String> {
    set_available(host, "", false)?;
    let bind_host = if tcp_host == "_" { "0.0.0.0" } else { tcp_host };
    let listener = TcpListener::bind((bind_host, tcp_port)).map_err(|error| error.to_string())?;
    listener
        .set_nonblocking(true)
        .map_err(|error| error.to_string())?;
    println!(
        "### Listening for TCP connections on {}",
        listener.local_addr().map_err(|error| error.to_string())?
    );
    let mut session: Option<ClientSession> = None;
    let mut pipe: Option<RfcommPipe> = None;
    loop {
        device.poll(host);
        check_classic_channel_errors(device)?;
        if session.as_ref().is_some_and(|session| {
            device
                .classic_connection(session.runtime.connection_handle)
                .is_none()
        }) {
            session = None;
            pipe = None;
            println!("@@@ Bluetooth disconnection");
        }
        loop {
            match listener.accept() {
                Ok((stream, peer)) => {
                    println!("<<< TCP connection from {peer}");
                    if pipe.is_some() {
                        eprintln!("!!! TCP connection already active, rejecting new one");
                        let _ = stream.shutdown(Shutdown::Both);
                        continue;
                    }
                    if session.is_none() {
                        let established = establish_client_session(
                            host,
                            device,
                            bluetooth_address,
                            requested_channel,
                            uuid,
                            authenticate,
                            encrypt,
                        );
                        match established {
                            Ok(established) => session = Some(established),
                            Err(error) => {
                                eprintln!("!!! Bluetooth/RFCOMM connection failed: {error}");
                                let _ = stream.shutdown(Shutdown::Both);
                                continue;
                            }
                        }
                    }
                    let active = session.as_mut().expect("session was established");
                    if let Err(error) = active.runtime.multiplexer.open_dlc(
                        active.channel,
                        RFCOMM_DEFAULT_MAX_FRAME_SIZE,
                        u16::from(RFCOMM_DEFAULT_INITIAL_CREDITS),
                    ) {
                        eprintln!("!!! RFCOMM open failed: {error}");
                        let _ = stream.shutdown(Shutdown::Both);
                        continue;
                    }
                    active.runtime.flush(host, device)?;
                    match RfcommPipe::new(active.channel << 1, stream, trace) {
                        Ok(new_pipe) => pipe = Some(new_pipe),
                        Err(error) => {
                            eprintln!("!!! TCP setup failed: {error}");
                            active
                                .runtime
                                .multiplexer
                                .disconnect_dlc(active.channel << 1)
                                .map_err(|error| error.to_string())?;
                            active.runtime.flush(host, device)?;
                        }
                    }
                }
                Err(error) if error.kind() == ErrorKind::WouldBlock => break,
                Err(error) => return Err(error.to_string()),
            }
        }
        if let Some(active) = &mut session {
            active.runtime.poll(host, device)?;
            if let Some(active_pipe) = &mut pipe {
                if active_pipe.pump(host, device, &mut active.runtime)? == PipeState::Closed {
                    println!("*** RFCOMM channel closed");
                    pipe = None;
                }
            }
        }
        if !wait_tick(host)? {
            return Ok(());
        }
    }
}

fn run(args: Args) -> Result<(), String> {
    let name = configured_name(args.device_config.as_deref())?;
    println!("<<< connecting to HCI...");
    let transport = open_split_transport(&args.hci_transport).map_err(|error| error.to_string())?;
    let mut host = ExternalHost::new(transport);
    let mut device = Device::new(0);
    host.initialize_device(&mut device, COMMAND_TIMEOUT)
        .map_err(|error| error.to_string())?;
    println!("<<< connected");
    match args.mode {
        Mode::Server { tcp_host, tcp_port } => run_server(
            &mut host,
            &mut device,
            &name,
            args.channel,
            &args.uuid,
            args.trace,
            &tcp_host,
            tcp_port,
        ),
        Mode::Client {
            bluetooth_address,
            tcp_host,
            tcp_port,
            authenticate,
            encrypt,
        } => run_client(
            &mut host,
            &mut device,
            &bluetooth_address,
            args.channel,
            &args.uuid,
            args.trace,
            &tcp_host,
            tcp_port,
            authenticate,
            encrypt,
        ),
    }
}

fn main() -> ExitCode {
    match parse_args(std::env::args()).and_then(run) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}\n{}", usage());
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bumble_controller::{Controller, LocalLink as ControllerLocalLink};
    use bumble_host::pump as pump_devices;
    use bumble_sdp::service::SdpClient;

    fn test_address(value: &str) -> Address {
        Address::parse(value, AddressType::PUBLIC_DEVICE).unwrap()
    }

    fn connect_test_devices(
        link: &mut ControllerLocalLink,
        devices: &mut [Device; 2],
        initiator_address: &Address,
        responder_address: &Address,
    ) {
        devices[0].connect_classic(link, responder_address.clone());
        devices[0].poll(link);
        link.pump_classic();
        devices[1].poll(link);
        devices[1].accept_classic(link, initiator_address.clone());
        devices[1].poll(link);
        link.pump_classic();
        devices[0].poll(link);
    }

    fn drive_rfcomm(
        link: &mut ControllerLocalLink,
        devices: &mut [Device; 2],
        initiator: &mut RfcommSession,
        responder: &mut RfcommSession,
    ) {
        for _ in 0..16 {
            initiator.poll(link, &mut devices[0]).unwrap();
            responder.poll(link, &mut devices[1]).unwrap();
            pump_devices(link, devices);
        }
    }

    #[test]
    fn parses_upstream_server_and_client_cli_shapes() {
        let server = parse_args(
            [
                "rfcomm-bridge",
                "--device-config=device.json",
                "--hci-transport",
                "usb:0",
                "--trace",
                "--channel",
                "7",
                "--uuid",
                DEFAULT_RFCOMM_UUID,
                "server",
                "--tcp-host",
                "example.com",
                "--tcp-port=9000",
            ]
            .map(str::to_string),
        )
        .unwrap();
        assert_eq!(server.device_config, Some(PathBuf::from("device.json")));
        assert_eq!(server.hci_transport, "usb:0");
        assert!(server.trace);
        assert_eq!(server.channel, 7);
        assert_eq!(
            server.mode,
            Mode::Server {
                tcp_host: "example.com".into(),
                tcp_port: 9000,
            }
        );

        let client = parse_args(
            [
                "rfcomm-bridge",
                "--hci-transport=tcp-client:localhost:1234",
                "client",
                "C4:F2:17:1A:1D:BB",
                "--authenticate",
                "--encrypt",
            ]
            .map(str::to_string),
        )
        .unwrap();
        assert_eq!(
            client.mode,
            Mode::Client {
                bluetooth_address: "C4:F2:17:1A:1D:BB".into(),
                tcp_host: "_".into(),
                tcp_port: DEFAULT_CLIENT_TCP_PORT,
                authenticate: true,
                encrypt: true,
            }
        );
        assert!(parse_args(
            [
                "rfcomm-bridge",
                "--hci-transport=x",
                "--channel=31",
                "server",
            ]
            .map(str::to_string)
        )
        .is_err());
    }

    #[test]
    fn sdp_record_resolves_the_advertised_rfcomm_channel() {
        let uuid = Uuid::parse(DEFAULT_RFCOMM_UUID).unwrap();
        let record = service_record(uuid.clone(), 9);
        assert_eq!(
            ServiceAttribute::find(&record, 0x0005),
            Some(&DataElement::sequence([DataElement::uuid(
                public_browse_root()
            )]))
        );
        let mut server = SdpServer::new(1024);
        server.add_service(0x0001_0001, record);
        let services = SdpClient::new(server)
            .service_search_attribute(&[uuid], &[AttributeId::Range(0, 0xFFFF)])
            .unwrap();
        assert_eq!(
            services.iter().find_map(|service| rfcomm_channel(service)),
            Some(9)
        );
    }

    #[test]
    fn production_pipe_bridges_real_tcp_over_two_controller_rfcomm() {
        let initiator_address = test_address("11:11:11:11:11:11");
        let responder_address = test_address("22:22:22:22:22:22");
        let mut link = ControllerLocalLink::new();
        let initiator_id =
            link.add_controller(Controller::new("initiator", initiator_address.clone()));
        let responder_id =
            link.add_controller(Controller::new("responder", responder_address.clone()));
        let mut devices = [Device::new(initiator_id), Device::new(responder_id)];
        devices[1]
            .register_classic_channel_server(
                Some(u32::from(RFCOMM_PSM)),
                ClassicChannelSpec {
                    mtu: CLASSIC_L2CAP_MTU,
                },
            )
            .unwrap();
        connect_test_devices(
            &mut link,
            &mut devices,
            &initiator_address,
            &responder_address,
        );
        let initiator_handle = devices[0].classic_connection_handle().unwrap();
        let responder_handle = devices[1].classic_connection_handle().unwrap();
        let initiator_cid = devices[0]
            .connect_classic_channel(
                &mut link,
                initiator_handle,
                u32::from(RFCOMM_PSM),
                ClassicChannelSpec {
                    mtu: CLASSIC_L2CAP_MTU,
                },
            )
            .unwrap();
        pump_devices(&mut link, &mut devices);
        let responder_cid = devices[1]
            .take_accepted_classic_channels(responder_handle)
            .into_iter()
            .next()
            .expect("responder accepted RFCOMM L2CAP channel");
        let mut initiator = RfcommSession::new(
            &devices[0],
            initiator_handle,
            initiator_cid,
            Role::Initiator,
        )
        .unwrap();
        let mut responder = RfcommSession::new(
            &devices[1],
            responder_handle,
            responder_cid,
            Role::Responder,
        )
        .unwrap();
        responder.multiplexer.listen(
            1,
            RFCOMM_DEFAULT_MAX_FRAME_SIZE,
            u16::from(RFCOMM_DEFAULT_INITIAL_CREDITS),
        );
        initiator.multiplexer.connect().unwrap();
        initiator.flush(&mut link, &mut devices[0]).unwrap();
        drive_rfcomm(&mut link, &mut devices, &mut initiator, &mut responder);
        assert_eq!(initiator.multiplexer.state(), MultiplexerState::Connected);
        initiator
            .multiplexer
            .open_dlc(
                1,
                RFCOMM_DEFAULT_MAX_FRAME_SIZE,
                u16::from(RFCOMM_DEFAULT_INITIAL_CREDITS),
            )
            .unwrap();
        initiator.flush(&mut link, &mut devices[0]).unwrap();
        drive_rfcomm(&mut link, &mut devices, &mut initiator, &mut responder);
        let dlci = 2;
        assert_eq!(
            initiator.multiplexer.dlc_state(dlci),
            Some(DlcState::Connected)
        );

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let mut tcp_peer = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        tcp_peer
            .set_read_timeout(Some(Duration::from_secs(1)))
            .unwrap();
        let (bridge_stream, _) = listener.accept().unwrap();
        let mut pipe = RfcommPipe::new(dlci, bridge_stream, false).unwrap();

        tcp_peer.write_all(b"TCP to RFCOMM").unwrap();
        for _ in 0..8 {
            pipe.pump(&mut link, &mut devices[0], &mut initiator)
                .unwrap();
            drive_rfcomm(&mut link, &mut devices, &mut initiator, &mut responder);
        }
        assert_eq!(
            responder.multiplexer.take_rx(dlci).concat(),
            b"TCP to RFCOMM"
        );

        responder.multiplexer.write(dlci, b"RFCOMM to TCP").unwrap();
        responder.flush(&mut link, &mut devices[1]).unwrap();
        drive_rfcomm(&mut link, &mut devices, &mut initiator, &mut responder);
        pipe.pump(&mut link, &mut devices[0], &mut initiator)
            .unwrap();
        let mut received = [0; 13];
        tcp_peer.read_exact(&mut received).unwrap();
        assert_eq!(&received, b"RFCOMM to TCP");
        assert!(devices[0].take_classic_channel_errors().is_empty());
        assert!(devices[1].take_classic_channel_errors().is_empty());
    }
}
