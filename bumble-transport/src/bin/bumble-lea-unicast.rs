use bumble::advertising_data::Type as AdvertisingDataType;
use bumble::{Address, AddressType, AdvertisingData};
use bumble_audio::{AudioInput, PcmFormat, WaveAudioInput};
use bumble_codecs::lc3::{Lc3Decoder, Lc3Encoder, Lc3FrameDuration, Lc3StreamConfig};
use bumble_gatt::GattServer;
use bumble_hci::{CodingFormat, Command as HciCommand};
use bumble_host::{Device, ExtendedAdvertisingConfig, LocalLink};
use bumble_profiles::ascs::{
    AseEndpoint, AseState, AudioStreamControlHandles, AudioStreamControlService,
};
use bumble_profiles::bap::{
    AudioLocation, CodecSpecificCapabilities, CodecSpecificConfiguration, ContextType,
    SupportedFrameDuration, SupportedSamplingFrequency, UnicastServerAdvertisingData,
};
use bumble_profiles::gap::GenericAccessService;
use bumble_profiles::le_audio::Metadata;
use bumble_profiles::pacs::{
    AudioContexts, PacCodecCapabilities, PacRecord, PublishedAudioCapabilitiesService,
    PUBLISHED_AUDIO_CAPABILITIES_SERVICE,
};
use bumble_transport::{open_split_transport, CommandResponse, ExternalHost, ExternalHostActivity};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::io::{ErrorKind, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::mpsc::{self, SyncSender, TryRecvError, TrySendError};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};
use tungstenite::{Message as WebSocketMessage, WebSocket};

const DEFAULT_NAME: &str = "Bumble LE Headphone";
const DEFAULT_CLASS_OF_DEVICE: u32 = 0x244418;
const DEFAULT_ADDRESS: &str = "F1:F2:F3:F4:F5:F6";
const DEFAULT_UI_PORT: u16 = 7654;
const COMMAND_TIMEOUT: Duration = Duration::from_secs(5);
const POLL_INTERVAL: Duration = Duration::from_millis(2);
const UI_QUEUE_LIMIT: usize = 1024;
const SINK_ASE_ID: u8 = 1;
const SOURCE_ASE_ID: u8 = 2;
const INDEX_HTML: &str = include_str!("lea_unicast/index.html");

#[derive(Clone, Debug, PartialEq, Eq)]
struct Args {
    ui_port: u16,
    device_config: Option<PathBuf>,
    transport: String,
    lc3_file: PathBuf,
}

#[derive(Clone, Debug)]
struct DeviceConfig {
    name: String,
    class_of_device: u32,
    address: Address,
}

fn usage() -> &'static str {
    "usage: bumble-lea-unicast [--ui-port PORT] [--device-config PATH] TRANSPORT LC3_FILE"
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

fn parse_args(arguments: impl IntoIterator<Item = String>) -> Result<Args, String> {
    let mut arguments: VecDeque<_> = arguments.into_iter().skip(1).collect();
    let mut ui_port = DEFAULT_UI_PORT;
    let mut device_config = None;
    let mut positional = Vec::new();
    while let Some(argument) = arguments.pop_front() {
        if matches!(argument.as_str(), "-h" | "--help") {
            return Err(usage().into());
        }
        if let Some(value) = option_value(&argument, "--ui-port", &mut arguments)? {
            ui_port = value
                .parse()
                .map_err(|_| "--ui-port must be between 0 and 65535".to_string())?;
            continue;
        }
        if let Some(value) = option_value(&argument, "--device-config", &mut arguments)? {
            device_config = Some(PathBuf::from(value));
            continue;
        }
        if argument.starts_with('-') {
            return Err(format!("unknown option {argument:?}"));
        }
        positional.push(argument);
    }
    if positional.len() != 2 {
        return Err(usage().into());
    }
    Ok(Args {
        ui_port,
        device_config,
        transport: positional.remove(0),
        lc3_file: PathBuf::from(positional.remove(0)),
    })
}

fn parse_class_of_device(value: &serde_json::Value) -> Option<u32> {
    value
        .as_u64()
        .and_then(|value| u32::try_from(value).ok())
        .or_else(|| {
            value.as_str().and_then(|value| {
                u32::from_str_radix(value.strip_prefix("0x").unwrap_or(value), 16).ok()
            })
        })
}

fn parse_random_address(value: &str) -> Result<Address, String> {
    Address::parse(value, AddressType::RANDOM_DEVICE).map_err(|error| error.to_string())
}

fn load_device_config(path: Option<&Path>) -> Result<DeviceConfig, String> {
    let mut config = DeviceConfig {
        name: DEFAULT_NAME.into(),
        class_of_device: DEFAULT_CLASS_OF_DEVICE,
        address: parse_random_address(DEFAULT_ADDRESS)?,
    };
    let Some(path) = path else {
        return Ok(config);
    };
    let bytes = std::fs::read(path)
        .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
    let value: serde_json::Value = serde_json::from_slice(&bytes)
        .map_err(|error| format!("invalid device config: {error}"))?;
    if let Some(name) = value.get("name").and_then(serde_json::Value::as_str) {
        config.name = name.into();
    }
    if let Some(class_of_device) = value.get("class_of_device").and_then(parse_class_of_device) {
        config.class_of_device = class_of_device;
    }
    if let Some(address) = value.get("address").and_then(serde_json::Value::as_str) {
        config.address = parse_random_address(address)?;
    }
    Ok(config)
}

fn require_success(response: CommandResponse, context: &str) -> Result<(), String> {
    if response.status() == Some(0) {
        Ok(())
    } else {
        Err(format!(
            "{context} failed with HCI status {:?}",
            response.status()
        ))
    }
}

fn configure_identity(host: &mut ExternalHost, config: &DeviceConfig) -> Result<(), String> {
    let mut local_name = [0; 248];
    let name = config.name.as_bytes();
    let length = name.len().min(local_name.len());
    local_name[..length].copy_from_slice(&name[..length]);
    require_success(
        host.send_command(HciCommand::WriteLocalName { local_name }, COMMAND_TIMEOUT)
            .map_err(|error| error.to_string())?,
        "writing local name",
    )?;
    require_success(
        host.send_command(
            HciCommand::WriteClassOfDevice {
                class_of_device: config.class_of_device,
            },
            COMMAND_TIMEOUT,
        )
        .map_err(|error| error.to_string())?,
        "writing Class of Device",
    )
}

fn sink_pac_record() -> PacRecord {
    PacRecord {
        coding_format: CodingFormat::LC3,
        codec_specific_capabilities: PacCodecCapabilities::Standard(CodecSpecificCapabilities {
            supported_sampling_frequencies: SupportedSamplingFrequency::FREQ_8000
                | SupportedSamplingFrequency::FREQ_16000
                | SupportedSamplingFrequency::FREQ_24000
                | SupportedSamplingFrequency::FREQ_32000
                | SupportedSamplingFrequency::FREQ_48000,
            supported_frame_durations: SupportedFrameDuration::DURATION_10000_US_SUPPORTED,
            supported_audio_channel_count: vec![1, 2],
            min_octets_per_codec_frame: 26,
            max_octets_per_codec_frame: 240,
            supported_max_codec_frames_per_sdu: 2,
        }),
        metadata: Metadata::default(),
    }
}

fn source_pac_record() -> PacRecord {
    PacRecord {
        coding_format: CodingFormat::LC3,
        codec_specific_capabilities: PacCodecCapabilities::Standard(CodecSpecificCapabilities {
            supported_sampling_frequencies: SupportedSamplingFrequency::FREQ_8000
                | SupportedSamplingFrequency::FREQ_16000
                | SupportedSamplingFrequency::FREQ_24000
                | SupportedSamplingFrequency::FREQ_32000
                | SupportedSamplingFrequency::FREQ_48000,
            supported_frame_durations: SupportedFrameDuration::DURATION_10000_US_SUPPORTED,
            supported_audio_channel_count: vec![1],
            min_octets_per_codec_frame: 30,
            max_octets_per_codec_frame: 100,
            supported_max_codec_frames_per_sdu: 1,
        }),
        metadata: Metadata::default(),
    }
}

fn build_gatt(
    name: &str,
) -> Result<
    (
        GattServer,
        AudioStreamControlService,
        AudioStreamControlHandles,
    ),
    String,
> {
    let contexts = AudioContexts {
        sink: ContextType(0xFFFF),
        source: ContextType(0xFFFF),
    };
    let mut pacs = PublishedAudioCapabilitiesService::new(contexts, contexts);
    pacs.sink_pac = vec![sink_pac_record()];
    pacs.sink_audio_locations = Some(AudioLocation::FRONT_LEFT | AudioLocation::FRONT_RIGHT);
    pacs.source_pac = vec![source_pac_record()];
    pacs.source_audio_locations = Some(AudioLocation::FRONT_LEFT);
    let ascs = AudioStreamControlService::new(&[SINK_ASE_ID], &[SOURCE_ASE_ID])
        .map_err(|error| error.to_string())?;
    let mut server = GattServer::from_definitions(vec![
        GenericAccessService::from_packed_appearance(name, 0).definition(),
        pacs.definition().map_err(|error| error.to_string())?,
        ascs.definition().map_err(|error| error.to_string())?,
    ])
    .map_err(|error| error.to_string())?;
    let handles = ascs.bind(&mut server).map_err(|error| error.to_string())?;
    Ok((server, ascs, handles))
}

fn advertising_data(name: &str) -> Result<Vec<u8>, String> {
    let mut data = AdvertisingData {
        ad_structures: vec![
            (
                AdvertisingDataType::COMPLETE_LOCAL_NAME,
                name.as_bytes().to_vec(),
            ),
            (AdvertisingDataType::FLAGS, vec![0x06]),
            (
                AdvertisingDataType::INCOMPLETE_LIST_OF_16_BIT_SERVICE_CLASS_UUIDS,
                PUBLISHED_AUDIO_CAPABILITIES_SERVICE.to_le_bytes().to_vec(),
            ),
        ],
    }
    .to_bytes();
    data.extend_from_slice(
        &UnicastServerAdvertisingData::default()
            .to_bytes()
            .map_err(|error| error.to_string())?,
    );
    Ok(data)
}

fn start_advertising(
    link: &mut LocalLink,
    device: &mut Device,
    config: &DeviceConfig,
    data: &[u8],
) -> Result<(), String> {
    let mut parameters =
        ExtendedAdvertisingConfig::connectable_scannable(0, config.address.clone());
    parameters.event_properties = 0x0001;
    parameters.interval_min = 100;
    parameters.interval_max = 100;
    device.set_random_address(link, config.address.clone());
    if !device.start_extended_advertising(link, &parameters, data, &[]) {
        return Err("failed to configure LE Audio advertising".into());
    }
    Ok(())
}

fn stream_config(endpoint: &AseEndpoint) -> Result<Lc3StreamConfig, String> {
    if endpoint.codec_id != CodingFormat::LC3 {
        return Err(format!(
            "ASE {} selected unsupported codec {:?}",
            endpoint.ase_id, endpoint.codec_id
        ));
    }
    let configuration =
        CodecSpecificConfiguration::from_bytes(&endpoint.codec_specific_configuration)
            .map_err(|error| error.to_string())?;
    let sampling_frequency = configuration
        .sampling_frequency
        .ok_or_else(|| "LC3 configuration omits sampling frequency".to_string())?
        .hz()
        .map_err(|error| error.to_string())?;
    let frame_duration = match configuration
        .frame_duration
        .ok_or_else(|| "LC3 configuration omits frame duration".to_string())?
        .microseconds()
        .map_err(|error| error.to_string())?
    {
        7_500 => Lc3FrameDuration::SevenPointFiveMs,
        10_000 => Lc3FrameDuration::TenMs,
        _ => return Err("unsupported LC3 frame duration".into()),
    };
    let channels = configuration
        .audio_channel_allocation
        .map(AudioLocation::channel_count)
        .filter(|count| *count != 0)
        .unwrap_or(1) as usize;
    Lc3StreamConfig {
        sampling_frequency,
        frame_duration,
        channels,
        octets_per_codec_frame: usize::from(
            configuration
                .octets_per_codec_frame
                .ok_or_else(|| "LC3 configuration omits octets per codec frame".to_string())?,
        ),
        codec_frames_per_sdu: usize::from(configuration.codec_frames_per_sdu.unwrap_or(1)),
    }
    .validate()
    .map_err(|error| error.to_string())
}

struct PcmLoopSource {
    input: WaveAudioInput,
    input_format: PcmFormat,
    target_rate: u32,
    target_channels: usize,
    frames: VecDeque<Vec<i16>>,
    position: f64,
}

impl PcmLoopSource {
    fn open(path: &Path, target_rate: u32, target_channels: usize) -> Result<Self, String> {
        let mut input = WaveAudioInput::new(path);
        let input_format = input.open().map_err(|error| error.to_string())?;
        let mut source = Self {
            input,
            input_format,
            target_rate,
            target_channels,
            frames: VecDeque::new(),
            position: 0.0,
        };
        source.ensure_frames(2)?;
        Ok(source)
    }

    fn read_source_frame(&mut self) -> Result<Vec<i16>, String> {
        let bytes = self
            .input
            .read_frame(1)
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "WAVE input contains no PCM samples".to_string())?;
        let expected = self
            .input_format
            .bytes_per_frame()
            .map_err(|error| error.to_string())?;
        if bytes.len() != expected {
            return Err("WAVE input ended in the middle of a PCM frame".into());
        }
        Ok(bytes
            .chunks_exact(2)
            .map(|sample| i16::from_le_bytes([sample[0], sample[1]]))
            .collect())
    }

    fn ensure_frames(&mut self, count: usize) -> Result<(), String> {
        while self.frames.len() < count {
            let frame = self.read_source_frame()?;
            self.frames.push_back(frame);
        }
        Ok(())
    }

    fn mapped_channel(frame: &[i16], output_channel: usize, output_channels: usize) -> i16 {
        if frame.len() == output_channels {
            return frame[output_channel];
        }
        if frame.len() == 1 {
            return frame[0];
        }
        if output_channels == 1 {
            let sum: i64 = frame.iter().map(|sample| i64::from(*sample)).sum();
            return (sum / frame.len() as i64) as i16;
        }
        frame[output_channel.min(frame.len() - 1)]
    }

    fn read_samples(&mut self, sample_count: usize) -> Result<Vec<i16>, String> {
        if !sample_count.is_multiple_of(self.target_channels) {
            return Err("LC3 PCM sample count is not channel aligned".into());
        }
        let mut output = Vec::with_capacity(sample_count);
        let step = f64::from(self.input_format.sample_rate) / f64::from(self.target_rate);
        for _ in 0..sample_count / self.target_channels {
            self.ensure_frames(2)?;
            let first = &self.frames[0];
            let second = &self.frames[1];
            for channel in 0..self.target_channels {
                let first = f64::from(Self::mapped_channel(first, channel, self.target_channels));
                let second = f64::from(Self::mapped_channel(second, channel, self.target_channels));
                output.push((first + (second - first) * self.position).round() as i16);
            }
            self.position += step;
            while self.position >= 1.0 {
                self.position -= 1.0;
                self.frames.pop_front();
                self.ensure_frames(2)?;
            }
        }
        Ok(output)
    }
}

#[derive(Clone, Debug)]
enum UiFrame {
    Text(String),
    Binary(Vec<u8>),
}

#[derive(Clone)]
struct UiServer {
    clients: Arc<Mutex<Vec<SyncSender<UiFrame>>>>,
    format: Arc<Mutex<Option<Lc3StreamConfig>>>,
    port: u16,
}

impl UiServer {
    fn start(port: u16) -> Result<Self, String> {
        let listener = TcpListener::bind(("127.0.0.1", port)).map_err(|error| error.to_string())?;
        let port = listener
            .local_addr()
            .map_err(|error| error.to_string())?
            .port();
        let server = Self {
            clients: Arc::new(Mutex::new(Vec::new())),
            format: Arc::new(Mutex::new(None)),
            port,
        };
        let clients = Arc::clone(&server.clients);
        let format = Arc::clone(&server.format);
        thread::Builder::new()
            .name("bumble-lea-unicast-ui".into())
            .spawn(move || {
                for stream in listener.incoming() {
                    match stream {
                        Ok(stream) => {
                            let clients = Arc::clone(&clients);
                            let format = Arc::clone(&format);
                            let _ = thread::Builder::new()
                                .name("bumble-lea-unicast-ui-client".into())
                                .spawn(move || handle_ui_connection(stream, clients, format));
                        }
                        Err(error) => eprintln!("LE Audio UI accept error: {error}"),
                    }
                }
            })
            .map_err(|error| error.to_string())?;
        println!("UI HTTP server at http://127.0.0.1:{}", server.port());
        Ok(server)
    }

    fn port(&self) -> u16 {
        self.port
    }

    fn broadcast(&self, frame: UiFrame) {
        self.clients
            .lock()
            .expect("UI client lock poisoned")
            .retain(|client| match client.try_send(frame.clone()) {
                Ok(()) | Err(TrySendError::Full(_)) => true,
                Err(TrySendError::Disconnected(_)) => false,
            });
    }

    fn set_format(&self, config: Lc3StreamConfig) {
        *self.format.lock().expect("UI format lock poisoned") = Some(config);
        self.broadcast(UiFrame::Text(format_message(config)));
    }

    fn send_audio(&self, samples: &[i16]) {
        let mut bytes = Vec::with_capacity(samples.len() * 2);
        for sample in samples {
            bytes.extend_from_slice(&sample.to_le_bytes());
        }
        self.broadcast(UiFrame::Binary(bytes));
    }
}

fn format_message(config: Lc3StreamConfig) -> String {
    json!({
        "type": "format",
        "params": {
            "sample_rate": config.sampling_frequency,
            "channels": config.channels,
        }
    })
    .to_string()
}

fn handle_ui_connection(
    mut stream: TcpStream,
    clients: Arc<Mutex<Vec<SyncSender<UiFrame>>>>,
    format: Arc<Mutex<Option<Lc3StreamConfig>>>,
) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let mut request = [0; 4096];
    let Ok(length) = stream.peek(&mut request) else {
        return;
    };
    let request_text = String::from_utf8_lossy(&request[..length]);
    let path = request_text
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .unwrap_or("/")
        .to_string();
    if path == "/channel" {
        handle_websocket(stream, clients, format);
        return;
    }
    let _ = stream.read(&mut request);
    let (status, content_type, body) = if matches!(path.as_str(), "/" | "/index.html") {
        ("200 OK", "text/html; charset=utf-8", INDEX_HTML)
    } else {
        ("404 Not Found", "text/plain; charset=utf-8", "not found")
    };
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(response.as_bytes());
}

fn send_ui_frame(socket: &mut WebSocket<TcpStream>, frame: UiFrame) -> bool {
    match frame {
        UiFrame::Text(text) => socket.send(WebSocketMessage::Text(text.into())),
        UiFrame::Binary(bytes) => socket.send(WebSocketMessage::Binary(bytes.into())),
    }
    .is_ok()
}

fn handle_websocket(
    stream: TcpStream,
    clients: Arc<Mutex<Vec<SyncSender<UiFrame>>>>,
    format: Arc<Mutex<Option<Lc3StreamConfig>>>,
) {
    let Ok(mut socket) = tungstenite::accept(stream) else {
        return;
    };
    if socket.get_mut().set_nonblocking(true).is_err() {
        return;
    }
    let (sender, receiver) = mpsc::sync_channel(UI_QUEUE_LIMIT);
    clients
        .lock()
        .expect("UI client lock poisoned")
        .push(sender);
    if let Some(config) = *format.lock().expect("UI format lock poisoned") {
        if !send_ui_frame(&mut socket, UiFrame::Text(format_message(config))) {
            return;
        }
    }
    loop {
        loop {
            match receiver.try_recv() {
                Ok(frame) => {
                    if !send_ui_frame(&mut socket, frame) {
                        return;
                    }
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => return,
            }
        }
        match socket.read() {
            Ok(WebSocketMessage::Close(_)) => return,
            Ok(_) => {}
            Err(tungstenite::Error::Io(error)) if error.kind() == ErrorKind::WouldBlock => {}
            Err(_) => return,
        }
        thread::sleep(POLL_INTERVAL);
    }
}

struct MediaRuntime {
    ascs: AudioStreamControlService,
    handles: AudioStreamControlHandles,
    cis: BTreeMap<u16, (u8, u8)>,
    configured_cis: BTreeSet<u16>,
    sink: Option<(Lc3StreamConfig, Lc3Decoder)>,
    source: Option<(Lc3StreamConfig, Lc3Encoder, PcmLoopSource)>,
    source_file: PathBuf,
    next_source_sdu: Instant,
    ui: UiServer,
}

impl MediaRuntime {
    fn new(
        ascs: AudioStreamControlService,
        handles: AudioStreamControlHandles,
        source_file: PathBuf,
        ui: UiServer,
    ) -> Self {
        Self {
            ascs,
            handles,
            cis: BTreeMap::new(),
            configured_cis: BTreeSet::new(),
            sink: None,
            source: None,
            source_file,
            next_source_sdu: Instant::now(),
            ui,
        }
    }

    fn endpoint(&self, id: u8) -> Result<AseEndpoint, String> {
        self.ascs
            .endpoint(id)
            .map_err(|error| error.to_string())?
            .ok_or_else(|| format!("missing ASE {id}"))
    }

    fn notify_pending(&self, link: &mut LocalLink, device: &mut Device) -> Result<(), String> {
        for (attribute_handle, value) in self
            .ascs
            .take_pending_notifications(&self.handles)
            .map_err(|error| error.to_string())?
        {
            if device.is_connected() && !device.notify(link, attribute_handle, value) {
                return Err("failed to send ASCS notification".into());
            }
        }
        Ok(())
    }

    fn accept_cis_requests(&mut self, link: &mut LocalLink, device: &mut Device) {
        for request in device.take_cis_requests() {
            let matches_endpoint = [SINK_ASE_ID, SOURCE_ASE_ID].into_iter().any(|ase_id| {
                self.ascs
                    .endpoint(ase_id)
                    .ok()
                    .flatten()
                    .is_some_and(|ase| {
                        ase.qos.cig_id == request.cig_id
                            && ase.qos.cis_id == request.cis_id
                            && matches!(ase.state, AseState::ENABLING | AseState::STREAMING)
                    })
            });
            if matches_endpoint {
                self.cis.insert(
                    request.cis_connection_handle,
                    (request.cig_id, request.cis_id),
                );
                device.accept_cis(link, request.cis_connection_handle);
            } else {
                eprintln!(
                    "ignoring CIS request for unconfigured CIG {} CIS {}",
                    request.cig_id, request.cis_id
                );
            }
        }
    }

    fn establish_cis(&mut self, link: &mut LocalLink, device: &mut Device) -> Result<(), String> {
        let established = device.established_cis_handles().collect::<Vec<_>>();
        let established_set = established.iter().copied().collect::<BTreeSet<_>>();
        let disconnected = self
            .configured_cis
            .difference(&established_set)
            .copied()
            .collect::<Vec<_>>();
        for handle in disconnected {
            self.configured_cis.remove(&handle);
            self.cis.remove(&handle);
        }
        for handle in established {
            if self.configured_cis.contains(&handle) {
                continue;
            }
            let Some((cig_id, cis_id)) = self.cis.get(&handle).copied() else {
                continue;
            };
            let changed = self
                .ascs
                .establish_cis(cig_id, cis_id)
                .map_err(|error| error.to_string())?;
            if changed.contains(&SINK_ASE_ID) && !device.setup_iso_data_path(link, handle, 1) {
                return Err("failed to set up sink ISO data path".into());
            }
            if changed.contains(&SOURCE_ASE_ID) && !device.setup_iso_data_path(link, handle, 0) {
                return Err("failed to set up source ISO data path".into());
            }
            self.configured_cis.insert(handle);
        }
        Ok(())
    }

    fn cis_for(&self, endpoint: &AseEndpoint) -> Option<u16> {
        self.cis.iter().find_map(|(handle, (cig_id, cis_id))| {
            (*cig_id == endpoint.qos.cig_id && *cis_id == endpoint.qos.cis_id).then_some(*handle)
        })
    }

    fn ensure_codecs(&mut self) -> Result<(), String> {
        let sink = self.endpoint(SINK_ASE_ID)?;
        if sink.state == AseState::STREAMING {
            let config = stream_config(&sink)?;
            if self.sink.as_ref().map(|(current, _)| *current) != Some(config) {
                self.sink = Some((
                    config,
                    Lc3Decoder::new(config).map_err(|error| error.to_string())?,
                ));
                self.ui.set_format(config);
                println!("Sink ASE streaming with {config:?}");
            }
        } else if !matches!(sink.state, AseState::ENABLING | AseState::QOS_CONFIGURED) {
            self.sink = None;
        }

        let source = self.endpoint(SOURCE_ASE_ID)?;
        if source.state == AseState::STREAMING {
            let config = stream_config(&source)?;
            if self.source.as_ref().map(|(current, _, _)| *current) != Some(config) {
                let encoder = Lc3Encoder::new(config).map_err(|error| error.to_string())?;
                let input = PcmLoopSource::open(
                    &self.source_file,
                    config.sampling_frequency,
                    config.channels,
                )?;
                self.source = Some((config, encoder, input));
                self.next_source_sdu = Instant::now();
                println!("Source ASE streaming with {config:?}");
            }
        } else if !matches!(source.state, AseState::ENABLING | AseState::QOS_CONFIGURED) {
            self.source = None;
        }
        Ok(())
    }

    fn receive_sink(&mut self, device: &mut Device) -> Result<(), String> {
        let endpoint = self.endpoint(SINK_ASE_ID)?;
        if endpoint.state != AseState::STREAMING {
            return Ok(());
        }
        let Some(handle) = self.cis_for(&endpoint) else {
            return Ok(());
        };
        let Some((_, decoder)) = &self.sink else {
            return Ok(());
        };
        for sdu in device.take_iso_sdus(handle) {
            if sdu.packet_status_flag != 0 {
                eprintln!(
                    "dropping invalid ISO SDU sequence {}",
                    sdu.packet_sequence_number
                );
                continue;
            }
            match decoder.decode_sdu(&sdu.data) {
                Ok(samples) => self.ui.send_audio(&samples),
                Err(error) => eprintln!("failed to decode LC3 sink SDU: {error}"),
            }
        }
        Ok(())
    }

    fn send_source(&mut self, link: &mut LocalLink, device: &mut Device) -> Result<(), String> {
        let endpoint = self.endpoint(SOURCE_ASE_ID)?;
        if endpoint.state != AseState::STREAMING || Instant::now() < self.next_source_sdu {
            return Ok(());
        }
        let Some(handle) = self.cis_for(&endpoint) else {
            return Ok(());
        };
        let Some((config, encoder, input)) = &mut self.source else {
            return Ok(());
        };
        let samples = input.read_samples(config.pcm_samples_per_sdu())?;
        let sdu = encoder
            .encode_sdu(&samples)
            .map_err(|error| error.to_string())?;
        if !device.send_iso_sdu(link, handle, &sdu) {
            return Err("failed to send LC3 source SDU".into());
        }
        let interval = if endpoint.qos.sdu_interval != 0 {
            Duration::from_micros(u64::from(endpoint.qos.sdu_interval))
        } else {
            Duration::from_micros(
                u64::from(config.frame_duration.microseconds())
                    * config.codec_frames_per_sdu as u64,
            )
        };
        self.next_source_sdu += interval;
        if self.next_source_sdu < Instant::now() {
            self.next_source_sdu = Instant::now() + interval;
        }
        Ok(())
    }

    fn reset(&mut self) -> Result<(), String> {
        self.ascs.reset().map_err(|error| error.to_string())?;
        self.cis.clear();
        self.configured_cis.clear();
        self.sink = None;
        self.source = None;
        Ok(())
    }
}

fn run(args: Args) -> Result<(), String> {
    if !args.lc3_file.is_file() {
        return Err(format!(
            "LC3 source WAVE file does not exist: {}",
            args.lc3_file.display()
        ));
    }
    let config = load_device_config(args.device_config.as_deref())?;
    let data = advertising_data(&config.name)?;
    let (server, ascs, handles) = build_gatt(&config.name)?;
    let ui = UiServer::start(args.ui_port)?;
    let transport = open_split_transport(&args.transport).map_err(|error| error.to_string())?;
    let mut host = ExternalHost::new(transport);
    let mut device = Device::with_server(0, server);
    host.initialize_device(&mut device, COMMAND_TIMEOUT)
        .map_err(|error| error.to_string())?;
    configure_identity(&mut host, &config)?;
    start_advertising(&mut host, &mut device, &config, &data)?;
    let mut media = MediaRuntime::new(ascs, handles, args.lc3_file, ui);
    let mut connected = false;
    println!("LE Audio unicast server ready as {}", config.name);
    loop {
        device.poll(&mut host);
        if device.is_connected() != connected {
            connected = device.is_connected();
            if connected {
                if let Some(peer) = device.peer_address() {
                    println!("LE connection from {}", peer.to_string(false));
                }
            } else {
                println!("LE connection closed");
                media.reset()?;
                start_advertising(&mut host, &mut device, &config, &data)?;
            }
        }
        media.accept_cis_requests(&mut host, &mut device);
        media.establish_cis(&mut host, &mut device)?;
        media.notify_pending(&mut host, &mut device)?;
        media.ensure_codecs()?;
        media.receive_sink(&mut device)?;
        media.send_source(&mut host, &mut device)?;
        match host
            .wait_for_activity(POLL_INTERVAL)
            .map_err(|error| error.to_string())?
        {
            ExternalHostActivity::Packet | ExternalHostActivity::Timeout => {}
            ExternalHostActivity::Ended => break,
        }
    }
    Ok(())
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
    use bumble_att::AttPdu;
    use bumble_controller::{Controller, LocalLink as ControllerLocalLink};
    use bumble_gatt::{AttTransport, GattClient};
    use bumble_host::pump;
    use bumble_profiles::ascs::{
        AseMetadataParameters, AseOperation, AudioStreamControlServiceProxy, ConfigCodecParameters,
        ConfigQosParameters,
    };
    use bumble_profiles::pacs::PublishedAudioCapabilitiesServiceProxy;
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_FILE: AtomicU64 = AtomicU64::new(0);

    struct LiveAttTransport<'a> {
        link: &'a mut ControllerLocalLink,
        devices: &'a mut [Device; 2],
        client_handle: u16,
    }

    impl AttTransport for LiveAttTransport<'_> {
        fn request(&mut self, request: &AttPdu) -> AttPdu {
            self.try_request(request).unwrap()
        }

        fn try_request(&mut self, request: &AttPdu) -> Result<AttPdu, String> {
            assert!(self.devices[0].send_att_on_handle(self.link, self.client_handle, request));
            pump(self.link, self.devices);
            if matches!(request, AttPdu::WriteCommand { .. }) {
                return Ok(AttPdu::WriteResponse);
            }
            let mut responses = self.devices[0].take_inbox_on_handle(self.client_handle);
            assert_eq!(responses.len(), 1, "missing response to {request:?}");
            Ok(responses.pop().unwrap())
        }
    }

    fn codec_configuration(channels: u32) -> CodecSpecificConfiguration {
        CodecSpecificConfiguration {
            sampling_frequency: Some(
                bumble_profiles::bap::SamplingFrequency::from_hz(48_000).unwrap(),
            ),
            frame_duration: Some(bumble_profiles::bap::FrameDuration::DURATION_10000_US),
            audio_channel_allocation: Some(if channels == 2 {
                AudioLocation::FRONT_LEFT | AudioLocation::FRONT_RIGHT
            } else {
                AudioLocation::FRONT_LEFT
            }),
            octets_per_codec_frame: Some(100),
            codec_frames_per_sdu: Some(1),
        }
    }

    fn wave_file(channels: u16, sample_rate: u32, samples: &[i16]) -> Vec<u8> {
        let data = samples
            .iter()
            .flat_map(|sample| sample.to_le_bytes())
            .collect::<Vec<_>>();
        let block_align = channels * 2;
        let mut wave = b"RIFF".to_vec();
        wave.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
        wave.extend_from_slice(b"WAVEfmt ");
        wave.extend_from_slice(&16u32.to_le_bytes());
        wave.extend_from_slice(&1u16.to_le_bytes());
        wave.extend_from_slice(&channels.to_le_bytes());
        wave.extend_from_slice(&sample_rate.to_le_bytes());
        wave.extend_from_slice(&(sample_rate * u32::from(block_align)).to_le_bytes());
        wave.extend_from_slice(&block_align.to_le_bytes());
        wave.extend_from_slice(&16u16.to_le_bytes());
        wave.extend_from_slice(b"data");
        wave.extend_from_slice(&(data.len() as u32).to_le_bytes());
        wave.extend_from_slice(&data);
        wave
    }

    fn temporary_wave() -> PathBuf {
        let id = NEXT_FILE.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "bumble-lea-unicast-{}-{id}.wav",
            std::process::id()
        ));
        let samples = (0..960)
            .map(|index| ((index % 80) * 300) as i16 - 12_000)
            .collect::<Vec<_>>();
        fs::write(&path, wave_file(1, 48_000, &samples)).unwrap();
        path
    }

    #[test]
    fn parses_upstream_cli_shape_and_defaults() {
        assert_eq!(
            parse_args(
                [
                    "lea-unicast",
                    "--ui-port=0",
                    "--device-config",
                    "device.json",
                    "usb:0",
                    "input.wav",
                ]
                .map(str::to_string),
            )
            .unwrap(),
            Args {
                ui_port: 0,
                device_config: Some(PathBuf::from("device.json")),
                transport: "usb:0".into(),
                lc3_file: PathBuf::from("input.wav"),
            }
        );
        assert!(parse_args(["lea-unicast", "usb:0"].map(str::to_string)).is_err());
    }

    #[test]
    fn production_pacs_advertisement_and_codec_contract_match_upstream() {
        let (mut server, _, _) = build_gatt(DEFAULT_NAME).unwrap();
        let mut client = GattClient::new();
        let pacs = PublishedAudioCapabilitiesServiceProxy::discover(&mut client, &mut server)
            .unwrap()
            .unwrap();
        let sink = PublishedAudioCapabilitiesServiceProxy::read_pac(
            pacs.sink_pac.as_ref().unwrap(),
            &mut client,
            &mut server,
        )
        .unwrap();
        let PacCodecCapabilities::Standard(capabilities) = &sink[0].codec_specific_capabilities
        else {
            panic!("expected standard LC3 capabilities")
        };
        assert_eq!(sink[0].coding_format, CodingFormat::LC3);
        assert_eq!(capabilities.supported_audio_channel_count, [1, 2]);
        assert_eq!(capabilities.min_octets_per_codec_frame, 26);
        assert_eq!(capabilities.max_octets_per_codec_frame, 240);
        assert_eq!(capabilities.supported_max_codec_frames_per_sdu, 2);
        let data = advertising_data(DEFAULT_NAME).unwrap();
        let parsed = AdvertisingData::from_bytes(&data);
        assert_eq!(
            parsed
                .get(AdvertisingDataType::COMPLETE_LOCAL_NAME)
                .unwrap(),
            DEFAULT_NAME.as_bytes()
        );
        assert_eq!(
            parsed
                .get(AdvertisingDataType::INCOMPLETE_LIST_OF_16_BIT_SERVICE_CLASS_UUIDS)
                .unwrap(),
            PUBLISHED_AUDIO_CAPABILITIES_SERVICE.to_le_bytes()
        );
    }

    #[test]
    fn wave_source_resamples_and_maps_channels() {
        let path = temporary_wave();
        let mut source = PcmLoopSource::open(&path, 24_000, 2).unwrap();
        let samples = source.read_samples(480).unwrap();
        assert_eq!(samples.len(), 480);
        assert!(samples.chunks_exact(2).all(|frame| frame[0] == frame[1]));
        assert!(samples.iter().any(|sample| *sample != 0));
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn ui_serves_page_and_streams_pcm_over_websocket() {
        let ui = UiServer::start(0).unwrap();
        let mut http = TcpStream::connect(("127.0.0.1", ui.port())).unwrap();
        http.write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\n\r\n")
            .unwrap();
        let mut response = String::new();
        http.read_to_string(&mut response).unwrap();
        assert!(response.contains("200 OK"));
        assert!(response.contains("Bumble Unicast Server"));

        let (mut socket, _) =
            tungstenite::connect(format!("ws://127.0.0.1:{}/channel", ui.port())).unwrap();
        let config = Lc3StreamConfig {
            sampling_frequency: 48_000,
            frame_duration: Lc3FrameDuration::TenMs,
            channels: 2,
            octets_per_codec_frame: 100,
            codec_frames_per_sdu: 1,
        };
        ui.set_format(config);
        let first = socket.read().unwrap();
        assert!(first.to_text().unwrap().contains("48000"));
        ui.send_audio(&[1, -2]);
        let binary = (0..3)
            .find_map(|_| {
                let message = socket.read().unwrap();
                message.is_binary().then(|| message.into_data())
            })
            .expect("missing PCM WebSocket frame");
        assert_eq!(binary.as_ref(), [1, 0, 254, 255]);
    }

    #[test]
    fn live_gatt_ascs_cis_and_lc3_media_flow_over_two_controllers() {
        let central_address =
            Address::parse("C4:F2:17:1A:1D:AA", AddressType::RANDOM_DEVICE).unwrap();
        let peripheral_address =
            Address::parse("C4:F2:17:1A:1D:BB", AddressType::RANDOM_DEVICE).unwrap();
        let (server, ascs, handles) = build_gatt(DEFAULT_NAME).unwrap();
        let mut link = ControllerLocalLink::new();
        let central_id = link.add_controller(Controller::new("central", central_address.clone()));
        let peripheral_id =
            link.add_controller(Controller::new("peripheral", peripheral_address.clone()));
        let mut devices = [
            Device::new(central_id),
            Device::with_server(peripheral_id, server),
        ];
        devices[0].set_random_address(&mut link, central_address);
        devices[1].set_random_address(&mut link, peripheral_address.clone());
        assert!(devices[1].start_advertising(&mut link, &[]));
        devices[0].connect_le(&mut link, peripheral_address);
        pump(&mut link, &mut devices);
        let client_handle = devices[0].connection_handle().unwrap();
        let mut client = GattClient::new();
        {
            let mut transport = LiveAttTransport {
                link: &mut link,
                devices: &mut devices,
                client_handle,
            };
            let proxy = AudioStreamControlServiceProxy::discover(&mut client, &mut transport)
                .unwrap()
                .unwrap();
            let sink_configuration = codec_configuration(2).to_bytes();
            let source_configuration = codec_configuration(1).to_bytes();
            proxy
                .write_operation(
                    &mut client,
                    &mut transport,
                    &AseOperation::ConfigCodec(vec![
                        ConfigCodecParameters {
                            ase_id: SINK_ASE_ID,
                            target_latency: 3,
                            target_phy: 1,
                            codec_id: CodingFormat::LC3,
                            codec_specific_configuration: sink_configuration,
                        },
                        ConfigCodecParameters {
                            ase_id: SOURCE_ASE_ID,
                            target_latency: 3,
                            target_phy: 1,
                            codec_id: CodingFormat::LC3,
                            codec_specific_configuration: source_configuration,
                        },
                    ]),
                )
                .unwrap();
            proxy
                .write_operation(
                    &mut client,
                    &mut transport,
                    &AseOperation::ConfigQos(vec![
                        ConfigQosParameters {
                            ase_id: SINK_ASE_ID,
                            cig_id: 1,
                            cis_id: 1,
                            sdu_interval: 10_000,
                            framing: 0,
                            phy: 1,
                            max_sdu: 200,
                            retransmission_number: 3,
                            max_transport_latency: 10,
                            presentation_delay: 0,
                        },
                        ConfigQosParameters {
                            ase_id: SOURCE_ASE_ID,
                            cig_id: 1,
                            cis_id: 1,
                            sdu_interval: 10_000,
                            framing: 0,
                            phy: 1,
                            max_sdu: 100,
                            retransmission_number: 3,
                            max_transport_latency: 10,
                            presentation_delay: 0,
                        },
                    ]),
                )
                .unwrap();
            proxy
                .write_operation(
                    &mut client,
                    &mut transport,
                    &AseOperation::Enable(vec![
                        AseMetadataParameters {
                            ase_id: SINK_ASE_ID,
                            metadata: Vec::new(),
                        },
                        AseMetadataParameters {
                            ase_id: SOURCE_ASE_ID,
                            metadata: Vec::new(),
                        },
                    ]),
                )
                .unwrap();
        }
        assert!(devices[0].configure_cig(&mut link, 1, &[1]));
        pump(&mut link, &mut devices);
        let central_cis = devices[0].take_configured_cis_handles()[0];
        assert!(devices[0].create_cis(&mut link, central_cis));
        pump(&mut link, &mut devices);
        let source_path = temporary_wave();
        let ui = UiServer::start(0).unwrap();
        let mut media = MediaRuntime::new(ascs, handles, source_path.clone(), ui);
        media.accept_cis_requests(&mut link, &mut devices[1]);
        pump(&mut link, &mut devices);
        let peripheral_cis = devices[1].established_cis_handles().next().unwrap();
        media.establish_cis(&mut link, &mut devices[1]).unwrap();
        assert!(devices[0].setup_iso_data_path(&mut link, central_cis, 0));
        pump(&mut link, &mut devices);
        media.ensure_codecs().unwrap();
        let sink_config =
            stream_config(&media.ascs.endpoint(SINK_ASE_ID).unwrap().unwrap()).unwrap();
        let encoder = Lc3Encoder::new(sink_config).unwrap();
        let decoder = Lc3Decoder::new(sink_config).unwrap();
        let pcm = (0..sink_config.pcm_samples_per_sdu())
            .map(|index| ((index % 64) * 400) as i16 - 12_000)
            .collect::<Vec<_>>();
        let encoded = encoder.encode_sdu(&pcm).unwrap();
        assert!(devices[0].send_iso_sdu(&mut link, central_cis, &encoded));
        pump(&mut link, &mut devices);
        let received = devices[1].take_iso_sdus(peripheral_cis);
        assert_eq!(received.len(), 1);
        let decoded = decoder.decode_sdu(&received[0].data).unwrap();
        assert_eq!(decoded.len(), pcm.len());
        assert!(decoded.iter().any(|sample| *sample != 0));

        {
            let mut transport = LiveAttTransport {
                link: &mut link,
                devices: &mut devices,
                client_handle,
            };
            let proxy = AudioStreamControlServiceProxy::discover(&mut client, &mut transport)
                .unwrap()
                .unwrap();
            proxy
                .write_operation(
                    &mut client,
                    &mut transport,
                    &AseOperation::ReceiverStartReady(vec![SOURCE_ASE_ID]),
                )
                .unwrap();
        }
        media.ensure_codecs().unwrap();
        assert!(devices[0].setup_iso_data_path(&mut link, central_cis, 1));
        pump(&mut link, &mut devices);
        media.next_source_sdu = Instant::now() - Duration::from_millis(1);
        media.send_source(&mut link, &mut devices[1]).unwrap();
        pump(&mut link, &mut devices);
        let source_config =
            stream_config(&media.ascs.endpoint(SOURCE_ASE_ID).unwrap().unwrap()).unwrap();
        let source_decoder = Lc3Decoder::new(source_config).unwrap();
        let source_sdus = devices[0].take_iso_sdus(central_cis);
        assert_eq!(source_sdus.len(), 1);
        let source_pcm = source_decoder.decode_sdu(&source_sdus[0].data).unwrap();
        assert_eq!(source_pcm.len(), source_config.pcm_samples_per_sdu());
        assert!(source_pcm.iter().any(|sample| *sample != 0));
        assert!(!media
            .ascs
            .take_pending_notifications(&media.handles)
            .unwrap()
            .is_empty());
        fs::remove_file(source_path).unwrap();
    }
}
