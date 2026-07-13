use bumble::advertising_data::Type as AdvertisingDataType;
use bumble::keys::{JsonKeyStore, KeyStore};
use bumble::{Address, AddressType};
use bumble_a2dp::sdp::{make_audio_sink_sdp_record, ProfileVersion as A2dpProfileVersion};
use bumble_a2dp::transport::DeviceMediaTransport;
use bumble_a2dp::{
    AacChannels, AacMediaCodecInformation, AacObjectType, AacSamplingFrequency,
    MediaCodecInformation, OpusChannelMode, OpusFrameSize, OpusMediaCodecInformation,
    OpusSamplingFrequency, SbcAllocationMethod, SbcBlockLength, SbcChannelMode,
    SbcMediaCodecInformation, SbcSamplingFrequency, SbcSubbands,
};
use bumble_avdtp::host::DeviceSession as AvdtpDeviceSession;
use bumble_avdtp::session::{Session as AvdtpSession, SessionEvent};
use bumble_avdtp::{
    MediaType, Message as AvdtpMessage, ServiceCapabilities, ServiceCategory, StreamEndpointType,
    AVDTP_PSM,
};
use bumble_codecs::AacAudioRtpPacket;
use bumble_hci::{Command as HciCommand, ReturnParameters};
use bumble_host::{Device, LocalLink};
use bumble_l2cap::{ClassicChannelSpec, ClassicChannelState};
use bumble_rtp::MediaPacket;
use bumble_sdp::service::{SdpRequestHandler, SdpServer};
use bumble_sdp::{SdpPdu, SDP_PSM};
use bumble_smp::PairingConfig;
use bumble_transport::{
    open_split_transport, ClassicPairingSession, CommandResponse, ExternalHost,
    ExternalHostActivity,
};
use serde_json::json;
use std::collections::VecDeque;
use std::fs::File;
use std::io::{ErrorKind, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command as ProcessCommand, ExitCode, Stdio};
use std::sync::mpsc::{self, SyncSender, TryRecvError, TrySendError};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};
use tungstenite::{Message as WebSocketMessage, WebSocket};

const DEFAULT_NAME: &str = "Bumble Speaker";
const DEFAULT_CLASS_OF_DEVICE: u32 = 0x240414;
const DEFAULT_UI_PORT: u16 = 7654;
const CLASSIC_L2CAP_MTU: u16 = 2048;
const COMMAND_TIMEOUT: Duration = Duration::from_secs(5);
const PROCEDURE_TIMEOUT: Duration = Duration::from_secs(30);
const PAIRING_TIMEOUT: Duration = Duration::from_secs(120);
const POLL_INTERVAL: Duration = Duration::from_millis(10);
const SDP_SERVICE_HANDLE: u32 = 0x0001_0001;
const UI_QUEUE_LIMIT: usize = 1024;

const SPEAKER_HTML: &str = include_str!("speaker/speaker.html");
const SPEAKER_JS: &str = include_str!("speaker/speaker.js");
const SPEAKER_CSS: &str = include_str!("speaker/speaker.css");
const SPEAKER_LOGO: &str = include_str!("speaker/logo.svg");

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Codec {
    Sbc,
    Aac,
    Opus,
}

impl Codec {
    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "sbc" => Ok(Self::Sbc),
            "aac" => Ok(Self::Aac),
            "opus" => Ok(Self::Opus),
            _ => Err("--codec must be sbc, aac, or opus".into()),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Sbc => "sbc",
            Self::Aac => "aac",
            Self::Opus => "opus",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Args {
    codec: Codec,
    sampling_frequencies: Vec<u32>,
    bitrate: Option<u32>,
    vbr: bool,
    discover: bool,
    outputs: Vec<String>,
    ui_port: u16,
    connect_address: Option<String>,
    device_config: Option<PathBuf>,
    transport: String,
}

#[derive(Clone, Debug)]
struct DeviceConfig {
    name: String,
    class_of_device: u32,
}

fn usage() -> &'static str {
    "usage: bumble-speaker [--codec sbc|aac|opus] [--sampling-frequency HZ] [--bitrate BPS] [--vbr|--no-vbr] [--discover] [--output NAME] [--ui-port PORT] [--connect ADDRESS_OR_NAME] [--device-config PATH] TRANSPORT"
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

fn parse_u32(value: String, option: &str) -> Result<u32, String> {
    value
        .parse()
        .map_err(|_| format!("invalid value {value:?} for {option}"))
}

fn parse_args(arguments: impl IntoIterator<Item = String>) -> Result<Args, String> {
    let mut arguments: VecDeque<_> = arguments.into_iter().skip(1).collect();
    let mut codec = Codec::Aac;
    let mut sampling_frequencies = Vec::new();
    let mut bitrate = None;
    let mut vbr = true;
    let mut discover = false;
    let mut outputs = Vec::new();
    let mut ui_port = DEFAULT_UI_PORT;
    let mut connect_address = None;
    let mut device_config = None;
    let mut transport = None;
    while let Some(argument) = arguments.pop_front() {
        match argument.as_str() {
            "-h" | "--help" => return Err(usage().into()),
            "--vbr" => {
                vbr = true;
                continue;
            }
            "--no-vbr" => {
                vbr = false;
                continue;
            }
            "--discover" => {
                discover = true;
                continue;
            }
            _ => {}
        }
        if let Some(value) = option_value(&argument, "--codec", &mut arguments)? {
            codec = Codec::parse(&value)?;
            continue;
        }
        if let Some(value) = option_value(&argument, "--sampling-frequency", &mut arguments)? {
            sampling_frequencies.push(parse_u32(value, "--sampling-frequency")?);
            continue;
        }
        if let Some(value) = option_value(&argument, "--bitrate", &mut arguments)? {
            bitrate = Some(parse_u32(value, "--bitrate")?);
            continue;
        }
        if let Some(value) = option_value(&argument, "--output", &mut arguments)? {
            outputs.push(value);
            continue;
        }
        if let Some(value) = option_value(&argument, "--ui-port", &mut arguments)? {
            ui_port = u16::try_from(parse_u32(value, "--ui-port")?)
                .map_err(|_| "--ui-port must be between 0 and 65535".to_string())?;
            continue;
        }
        if let Some(value) = option_value(&argument, "--connect", &mut arguments)? {
            connect_address = Some(value);
            continue;
        }
        if let Some(value) = option_value(&argument, "--device-config", &mut arguments)? {
            device_config = Some(PathBuf::from(value));
            continue;
        }
        if argument.starts_with('-') {
            return Err(format!("unknown option {argument:?}"));
        }
        if transport.replace(argument).is_some() {
            return Err(usage().into());
        }
    }
    Ok(Args {
        codec,
        sampling_frequencies,
        bitrate,
        vbr,
        discover,
        outputs,
        ui_port,
        connect_address,
        device_config,
        transport: transport.ok_or_else(|| usage().to_string())?,
    })
}

fn parse_class_of_device(value: &serde_json::Value) -> Option<u32> {
    value
        .as_u64()
        .and_then(|value| u32::try_from(value).ok())
        .or_else(|| {
            value.as_str().and_then(|value| {
                let value = value.strip_prefix("0x").unwrap_or(value);
                u32::from_str_radix(value, 16).ok()
            })
        })
}

fn load_device_config(path: Option<&Path>) -> Result<DeviceConfig, String> {
    let Some(path) = path else {
        return Ok(DeviceConfig {
            name: DEFAULT_NAME.into(),
            class_of_device: DEFAULT_CLASS_OF_DEVICE,
        });
    };
    let bytes = std::fs::read(path)
        .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
    let value: serde_json::Value = serde_json::from_slice(&bytes)
        .map_err(|error| format!("invalid device config: {error}"))?;
    Ok(DeviceConfig {
        name: value
            .get("name")
            .and_then(serde_json::Value::as_str)
            .unwrap_or(DEFAULT_NAME)
            .to_string(),
        class_of_device: value
            .get("class_of_device")
            .and_then(parse_class_of_device)
            .unwrap_or(DEFAULT_CLASS_OF_DEVICE),
    })
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
    command: HciCommand,
    context: &str,
) -> Result<CommandResponse, String> {
    require_success(
        host.send_command(command, COMMAND_TIMEOUT)
            .map_err(|error| error.to_string())?,
        context,
    )
}

fn configure_identity(host: &mut ExternalHost, config: &DeviceConfig) -> Result<(), String> {
    let mut local_name = [0; 248];
    let name = config.name.as_bytes();
    let length = name.len().min(local_name.len());
    local_name[..length].copy_from_slice(&name[..length]);
    command(
        host,
        HciCommand::WriteLocalName { local_name },
        "writing local name",
    )?;
    command(
        host,
        HciCommand::WriteClassOfDevice {
            class_of_device: config.class_of_device,
        },
        "writing Class of Device",
    )?;
    command(
        host,
        HciCommand::WriteSimplePairingMode {
            simple_pairing_mode: 1,
        },
        "enabling Secure Simple Pairing",
    )?;
    let mut extended_inquiry_response = [0; 240];
    let eir_length = name.len().min(extended_inquiry_response.len() - 2);
    extended_inquiry_response[0] = (eir_length + 1) as u8;
    extended_inquiry_response[1] = AdvertisingDataType::COMPLETE_LOCAL_NAME.0;
    extended_inquiry_response[2..2 + eir_length].copy_from_slice(&name[..eir_length]);
    command(
        host,
        HciCommand::WriteExtendedInquiryResponse {
            fec_required: 0,
            extended_inquiry_response,
        },
        "writing extended inquiry response",
    )?;
    Ok(())
}

fn read_public_address(host: &mut ExternalHost) -> Result<Address, String> {
    let response = command(host, HciCommand::ReadBdAddr, "reading public address")?;
    match response.return_parameters() {
        Some(ReturnParameters::ReadBdAddr { bd_addr, .. }) => Ok(bd_addr.clone()),
        _ => Err("controller did not return a public address".into()),
    }
}

fn set_scan_enabled(host: &mut ExternalHost, enabled: bool) -> Result<(), String> {
    command(
        host,
        HciCommand::WriteScanEnable {
            scan_enable: if enabled { 0x03 } else { 0 },
        },
        if enabled {
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
) -> Result<u16, String> {
    let deadline = Instant::now() + PROCEDURE_TIMEOUT;
    loop {
        device.poll(host);
        let handle = peer.map_or_else(
            || device.classic_connection_handle(),
            |peer| device.classic_connection_handle_for_peer(peer),
        );
        if let Some(handle) = handle {
            return Ok(handle);
        }
        if accept_requests {
            for request in device.take_classic_connection_requests() {
                device.accept_classic(host, request);
            }
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err("timed out waiting for Classic connection".into());
        }
        match host
            .wait_for_activity(remaining)
            .map_err(|error| error.to_string())?
        {
            ExternalHostActivity::Packet => {}
            ExternalHostActivity::Timeout => {
                return Err("timed out waiting for Classic connection".into())
            }
            ExternalHostActivity::Ended => {
                return Err("transport ended while waiting for Classic connection".into())
            }
        }
    }
}

fn connect_classic(
    host: &mut ExternalHost,
    device: &mut Device,
    peer: Address,
) -> Result<u16, String> {
    device.connect_classic(host, peer.clone());
    wait_for_classic_connection(host, device, Some(&peer), false)
}

fn resolve_classic_name(
    host: &mut ExternalHost,
    device: &mut Device,
    wanted_name: &str,
) -> Result<Address, String> {
    device.take_classic_inquiry_results();
    device.take_classic_inquiry_complete();
    device.take_classic_remote_names();
    command(
        host,
        HciCommand::Inquiry {
            lap: 0x9E8B33,
            inquiry_length: 8,
            num_responses: 0,
        },
        "starting Classic inquiry",
    )?;
    let deadline = Instant::now() + PROCEDURE_TIMEOUT;
    let mut candidates = Vec::new();
    loop {
        device.poll(host);
        for address in device.take_classic_inquiry_results() {
            if !candidates.contains(&address) {
                candidates.push(address);
            }
        }
        if let Some(status) = device.take_classic_inquiry_complete().pop() {
            if status != 0 {
                return Err(format!(
                    "Classic inquiry failed with HCI status {status:#04x}"
                ));
            }
            break;
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            let _ = command(host, HciCommand::InquiryCancel, "canceling Classic inquiry");
            break;
        }
        if host
            .wait_for_activity(remaining)
            .map_err(|error| error.to_string())?
            == ExternalHostActivity::Ended
        {
            return Err("transport ended during Classic inquiry".into());
        }
    }
    for address in candidates {
        command(
            host,
            HciCommand::RemoteNameRequest {
                bd_addr: address.clone(),
                page_scan_repetition_mode: 2,
                reserved: 0,
                clock_offset: 0,
            },
            "requesting Classic remote name",
        )?;
        let deadline = Instant::now() + PROCEDURE_TIMEOUT;
        loop {
            device.poll(host);
            if let Some((status, _, name)) = device
                .take_classic_remote_names()
                .into_iter()
                .find(|(_, peer, _)| *peer == address)
            {
                if status == 0 && name == wanted_name {
                    return Ok(address);
                }
                break;
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                break;
            }
            match host
                .wait_for_activity(remaining)
                .map_err(|error| error.to_string())?
            {
                ExternalHostActivity::Packet => {}
                ExternalHostActivity::Timeout | ExternalHostActivity::Ended => break,
            }
        }
    }
    Err(format!("Classic peer named {wanted_name:?} was not found"))
}

fn resolve_classic_peer(
    host: &mut ExternalHost,
    device: &mut Device,
    address_or_name: &str,
) -> Result<Address, String> {
    match Address::parse(address_or_name, AddressType::PUBLIC_DEVICE) {
        Ok(address) => Ok(address),
        Err(_) => resolve_classic_name(host, device, address_or_name),
    }
}

fn authenticate_classic(
    host: &mut ExternalHost,
    device: &mut Device,
    connection_handle: u16,
    store: &mut JsonKeyStore,
) -> Result<(), String> {
    let peer = device
        .classic_connection(connection_handle)
        .ok_or_else(|| "Classic connection disappeared".to_string())?
        .peer_address
        .clone();
    let peer_name = peer.to_string(false);
    let stored_keys = store.get(&peer_name).map_err(|error| error.to_string())?;
    let mut pairing = ClassicPairingSession::accept_all(
        device,
        connection_handle,
        PairingConfig {
            bonding: true,
            mitm: false,
            ..PairingConfig::default()
        },
        stored_keys,
    )
    .map_err(|error| error.to_string())?;
    let keys = pairing
        .pair(host, device, PAIRING_TIMEOUT)
        .map_err(|error| error.to_string())?;
    if keys.link_key.is_some() {
        store
            .update(&peer_name, keys)
            .map_err(|error| error.to_string())?;
    }
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

fn open_classic_channel(
    host: &mut ExternalHost,
    device: &mut Device,
    connection_handle: u16,
    psm: u16,
) -> Result<u16, String> {
    let source_cid = device
        .connect_classic_channel(
            host,
            connection_handle,
            u32::from(psm),
            ClassicChannelSpec {
                mtu: CLASSIC_L2CAP_MTU,
            },
        )
        .map_err(|error| error.to_string())?;
    wait_for_classic_channel(host, device, connection_handle, source_cid)?;
    Ok(source_cid)
}

fn sbc_sampling_frequency(value: u32) -> Result<SbcSamplingFrequency, String> {
    match value {
        16_000 => Ok(SbcSamplingFrequency::SF_16000),
        32_000 => Ok(SbcSamplingFrequency::SF_32000),
        44_100 => Ok(SbcSamplingFrequency::SF_44100),
        48_000 => Ok(SbcSamplingFrequency::SF_48000),
        _ => Err(format!("unsupported SBC sampling frequency {value}")),
    }
}

fn aac_sampling_frequency(value: u32) -> Result<AacSamplingFrequency, String> {
    match value {
        8_000 => Ok(AacSamplingFrequency::SF_8000),
        11_025 => Ok(AacSamplingFrequency::SF_11025),
        12_000 => Ok(AacSamplingFrequency::SF_12000),
        16_000 => Ok(AacSamplingFrequency::SF_16000),
        22_050 => Ok(AacSamplingFrequency::SF_22050),
        24_000 => Ok(AacSamplingFrequency::SF_24000),
        32_000 => Ok(AacSamplingFrequency::SF_32000),
        44_100 => Ok(AacSamplingFrequency::SF_44100),
        48_000 => Ok(AacSamplingFrequency::SF_48000),
        64_000 => Ok(AacSamplingFrequency::SF_64000),
        88_200 => Ok(AacSamplingFrequency::SF_88200),
        96_000 => Ok(AacSamplingFrequency::SF_96000),
        _ => Err(format!("unsupported AAC sampling frequency {value}")),
    }
}

fn codec_information(args: &Args) -> Result<MediaCodecInformation, String> {
    Ok(match args.codec {
        Codec::Sbc => {
            let frequencies = if args.sampling_frequencies.is_empty() {
                &[16_000, 32_000, 44_100, 48_000][..]
            } else {
                &args.sampling_frequencies
            };
            let mut sampling_frequency = SbcSamplingFrequency(0);
            for frequency in frequencies {
                sampling_frequency = sampling_frequency | sbc_sampling_frequency(*frequency)?;
            }
            MediaCodecInformation::Sbc(SbcMediaCodecInformation {
                sampling_frequency,
                channel_mode: SbcChannelMode::MONO
                    | SbcChannelMode::DUAL_CHANNEL
                    | SbcChannelMode::STEREO
                    | SbcChannelMode::JOINT_STEREO,
                block_length: SbcBlockLength::BL_4
                    | SbcBlockLength::BL_8
                    | SbcBlockLength::BL_12
                    | SbcBlockLength::BL_16,
                subbands: SbcSubbands::S_4 | SbcSubbands::S_8,
                allocation_method: SbcAllocationMethod::LOUDNESS | SbcAllocationMethod::SNR,
                minimum_bitpool_value: 2,
                maximum_bitpool_value: 53,
            })
        }
        Codec::Aac => {
            let frequencies = if args.sampling_frequencies.is_empty() {
                &[
                    8_000, 11_025, 12_000, 16_000, 22_050, 24_000, 32_000, 44_100, 48_000,
                ][..]
            } else {
                &args.sampling_frequencies
            };
            let mut sampling_frequency = AacSamplingFrequency(0);
            for frequency in frequencies {
                sampling_frequency = sampling_frequency | aac_sampling_frequency(*frequency)?;
            }
            MediaCodecInformation::Aac(AacMediaCodecInformation {
                object_type: AacObjectType::MPEG_2_AAC_LC,
                sampling_frequency,
                channels: AacChannels::MONO | AacChannels::STEREO,
                vbr: args.vbr,
                bitrate: args.bitrate.unwrap_or(256_000),
            })
        }
        Codec::Opus => {
            let frequencies = if args.sampling_frequencies.is_empty() {
                &[48_000][..]
            } else {
                &args.sampling_frequencies
            };
            if frequencies.iter().any(|frequency| *frequency != 48_000) {
                return Err("A2DP Opus only supports 48000 Hz".into());
            }
            MediaCodecInformation::Opus(OpusMediaCodecInformation {
                channel_mode: OpusChannelMode::MONO
                    | OpusChannelMode::STEREO
                    | OpusChannelMode::DUAL_MONO,
                frame_size: OpusFrameSize::FS_10MS | OpusFrameSize::FS_20MS,
                sampling_frequency: OpusSamplingFrequency::SF_48000,
            })
        }
    })
}

fn sink_capabilities(args: &Args) -> Result<Vec<ServiceCapabilities>, String> {
    Ok(vec![
        ServiceCapabilities::empty(ServiceCategory::MEDIA_TRANSPORT),
        codec_information(args)?
            .to_avdtp_capability()
            .map_err(|error| error.to_string())?,
    ])
}

fn extract_audio(codec: Codec, packet: &MediaPacket) -> Result<Vec<u8>, String> {
    match codec {
        Codec::Aac => AacAudioRtpPacket::from_bytes(&packet.payload)
            .and_then(|packet| packet.to_adts())
            .map_err(|error| error.to_string()),
        Codec::Sbc | Codec::Opus => packet
            .payload
            .get(1..)
            .map(ToOwned::to_owned)
            .ok_or_else(|| "RTP audio payload is missing its media header".into()),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StreamState {
    Idle,
    Stopped,
    Started,
    Suspended,
}

impl StreamState {
    fn as_str(self) -> &'static str {
        match self {
            Self::Idle => "IDLE",
            Self::Stopped => "STOPPED",
            Self::Started => "STARTED",
            Self::Suspended => "SUSPENDED",
        }
    }
}

#[derive(Clone, Debug)]
struct UiSnapshot {
    codec: Codec,
    stream_state: StreamState,
    connection: Option<(String, String)>,
}

#[derive(Clone, Debug)]
enum UiFrame {
    Text(String),
    Binary(Vec<u8>),
}

#[derive(Clone)]
struct UiServer {
    clients: Arc<Mutex<Vec<SyncSender<UiFrame>>>>,
    snapshot: Arc<Mutex<UiSnapshot>>,
    port: u16,
}

impl UiServer {
    fn start(port: u16, codec: Codec) -> Result<Self, String> {
        let listener = TcpListener::bind(("127.0.0.1", port)).map_err(|error| error.to_string())?;
        let port = listener
            .local_addr()
            .map_err(|error| error.to_string())?
            .port();
        let server = Self {
            clients: Arc::new(Mutex::new(Vec::new())),
            snapshot: Arc::new(Mutex::new(UiSnapshot {
                codec,
                stream_state: StreamState::Idle,
                connection: None,
            })),
            port,
        };
        let clients = Arc::clone(&server.clients);
        let snapshot = Arc::clone(&server.snapshot);
        thread::Builder::new()
            .name("bumble-speaker-ui".into())
            .spawn(move || {
                for stream in listener.incoming() {
                    match stream {
                        Ok(stream) => {
                            let clients = Arc::clone(&clients);
                            let snapshot = Arc::clone(&snapshot);
                            let _ = thread::Builder::new()
                                .name("bumble-speaker-ui-client".into())
                                .spawn(move || handle_ui_connection(stream, clients, snapshot));
                        }
                        Err(error) => eprintln!("speaker UI accept error: {error}"),
                    }
                }
            })
            .map_err(|error| error.to_string())?;
        println!("UI HTTP server at http://127.0.0.1:{port}");
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

    fn event(&self, event_type: &str) {
        self.broadcast(UiFrame::Text(
            json!({"type": event_type, "params": {}}).to_string(),
        ));
    }

    fn set_stream_state(&self, state: StreamState) {
        self.snapshot
            .lock()
            .expect("UI snapshot lock poisoned")
            .stream_state = state;
        self.event(match state {
            StreamState::Started => "start",
            StreamState::Suspended => "suspend",
            StreamState::Idle | StreamState::Stopped => "stop",
        });
    }

    fn set_connection(&self, address: String, name: String) {
        self.snapshot
            .lock()
            .expect("UI snapshot lock poisoned")
            .connection = Some((address.clone(), name.clone()));
        self.broadcast(UiFrame::Text(
            json!({
                "type": "connection",
                "params": {"peer_address": address, "peer_name": name}
            })
            .to_string(),
        ));
    }

    fn clear_connection(&self) {
        self.snapshot
            .lock()
            .expect("UI snapshot lock poisoned")
            .connection = None;
        self.event("disconnection");
    }

    fn send_audio(&self, data: Vec<u8>) {
        self.broadcast(UiFrame::Binary(data));
    }
}

fn handle_ui_connection(
    mut stream: TcpStream,
    clients: Arc<Mutex<Vec<SyncSender<UiFrame>>>>,
    snapshot: Arc<Mutex<UiSnapshot>>,
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
        handle_websocket(stream, clients, snapshot);
    } else {
        let _ = stream.read(&mut request);
        let (content_type, body) = match path.as_str() {
            "/" | "/speaker.html" => ("text/html; charset=utf-8", SPEAKER_HTML),
            "/speaker.js" => ("text/javascript; charset=utf-8", SPEAKER_JS),
            "/speaker.css" => ("text/css; charset=utf-8", SPEAKER_CSS),
            "/logo.svg" => ("image/svg+xml", SPEAKER_LOGO),
            _ => ("text/plain; charset=utf-8", "not found"),
        };
        let status = if path == "/"
            || matches!(
                path.as_str(),
                "/speaker.html" | "/speaker.js" | "/speaker.css" | "/logo.svg"
            ) {
            "200 OK"
        } else {
            "404 Not Found"
        };
        let response = format!(
            "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        let _ = stream.write_all(response.as_bytes());
    }
}

fn send_ui_frame(socket: &mut WebSocket<TcpStream>, frame: UiFrame) -> bool {
    let result = match frame {
        UiFrame::Text(text) => socket.send(WebSocketMessage::Text(text.into())),
        UiFrame::Binary(data) => socket.send(WebSocketMessage::Binary(data.into())),
    };
    result.is_ok()
}

fn hello_message(snapshot: &UiSnapshot) -> String {
    json!({
        "type": "hello",
        "params": {
            "bumble_version": env!("CARGO_PKG_VERSION"),
            "codec": snapshot.codec.as_str(),
            "streamState": snapshot.stream_state.as_str()
        }
    })
    .to_string()
}

fn handle_websocket(
    stream: TcpStream,
    clients: Arc<Mutex<Vec<SyncSender<UiFrame>>>>,
    snapshot: Arc<Mutex<UiSnapshot>>,
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
            Ok(WebSocketMessage::Text(message)) => {
                if serde_json::from_str::<serde_json::Value>(&message)
                    .ok()
                    .and_then(|message| message.get("type").cloned())
                    .and_then(|value| value.as_str().map(ToOwned::to_owned))
                    .as_deref()
                    == Some("hello")
                {
                    let snapshot = snapshot.lock().expect("UI snapshot lock poisoned").clone();
                    if !send_ui_frame(&mut socket, UiFrame::Text(hello_message(&snapshot))) {
                        return;
                    }
                    if let Some((address, name)) = snapshot.connection {
                        let message = json!({
                            "type": "connection",
                            "params": {"peer_address": address, "peer_name": name}
                        })
                        .to_string();
                        if !send_ui_frame(&mut socket, UiFrame::Text(message)) {
                            return;
                        }
                    }
                }
            }
            Ok(WebSocketMessage::Close(_)) => return,
            Ok(_) => {}
            Err(tungstenite::Error::Io(error)) if error.kind() == ErrorKind::WouldBlock => {}
            Err(_) => return,
        }
        thread::sleep(POLL_INTERVAL);
    }
}

enum AudioOutput {
    File {
        file: File,
    },
    Ffplay {
        child: Option<Child>,
        stdin: Option<ChildStdin>,
    },
}

impl AudioOutput {
    fn start(&mut self, codec: Codec) -> Result<(), String> {
        let Self::Ffplay { child, stdin } = self else {
            return Ok(());
        };
        if child.is_some() {
            return Ok(());
        }
        let mut process = ProcessCommand::new("ffplay")
            .args(["-probesize", "32", "-f", codec.as_str(), "pipe:0"])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| error.to_string())?;
        *stdin = process.stdin.take();
        *child = Some(process);
        Ok(())
    }

    fn write(&mut self, data: &[u8]) -> Result<(), String> {
        match self {
            Self::File { file } => file.write_all(data).map_err(|error| error.to_string()),
            Self::Ffplay { stdin, .. } => {
                if let Some(stdin) = stdin {
                    stdin.write_all(data).map_err(|error| error.to_string())?;
                }
                Ok(())
            }
        }
    }

    fn stop(&mut self) {
        if let Self::Ffplay { child, stdin } = self {
            stdin.take();
            if let Some(mut process) = child.take() {
                let _ = process.kill();
                let _ = process.wait();
            }
        }
    }
}

struct Outputs {
    outputs: Vec<AudioOutput>,
}

impl Outputs {
    fn new(names: &[String]) -> Result<Self, String> {
        let mut outputs = Vec::new();
        for name in names {
            if name == "@ffplay" {
                if ProcessCommand::new("ffplay")
                    .arg("-version")
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status()
                    .is_ok_and(|status| status.success())
                {
                    outputs.push(AudioOutput::Ffplay {
                        child: None,
                        stdin: None,
                    });
                } else {
                    eprintln!("ffplay not installed, @ffplay output will be disabled");
                }
            } else {
                outputs.push(AudioOutput::File {
                    file: File::create(name).map_err(|error| {
                        format!("failed to create audio output {name:?}: {error}")
                    })?,
                });
            }
        }
        Ok(Self { outputs })
    }

    fn start(&mut self, codec: Codec) -> Result<(), String> {
        for output in &mut self.outputs {
            output.start(codec)?;
        }
        Ok(())
    }

    fn write(&mut self, data: &[u8]) -> Result<(), String> {
        for output in &mut self.outputs {
            output.write(data)?;
        }
        Ok(())
    }

    fn stop(&mut self) {
        for output in &mut self.outputs {
            output.stop();
        }
    }
}

impl Drop for Outputs {
    fn drop(&mut self) {
        self.stop();
    }
}

struct SdpEndpoint {
    source_cid: u16,
    server: SdpServer,
}

impl SdpEndpoint {
    fn new(source_cid: u16, peer_mtu: u16) -> Self {
        let mut server = SdpServer::new(peer_mtu);
        server.add_service(
            SDP_SERVICE_HANDLE,
            make_audio_sink_sdp_record(SDP_SERVICE_HANDLE, A2dpProfileVersion::V1_3),
        );
        Self { source_cid, server }
    }

    fn poll(
        &mut self,
        link: &mut LocalLink,
        device: &mut Device,
        connection_handle: u16,
    ) -> Result<(), String> {
        for bytes in device.take_classic_channel_sdus(connection_handle, self.source_cid) {
            let request = SdpPdu::from_bytes(&bytes).map_err(|error| error.to_string())?;
            let response = self.server.handle_request(&request);
            device
                .send_classic_channel_sdu(
                    link,
                    connection_handle,
                    self.source_cid,
                    &response.to_bytes().map_err(|error| error.to_string())?,
                )
                .map_err(|error| error.to_string())?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug)]
enum SinkEvent {
    Configured(Vec<ServiceCapabilities>),
    Opened,
    Started,
    Suspended,
    Stopped,
    Closed,
    DelayReport(u16),
    Packet(MediaPacket),
}

struct SinkRuntime {
    connection_handle: u16,
    signaling: AvdtpDeviceSession,
    media: Option<DeviceMediaTransport>,
}

impl SinkRuntime {
    fn new(
        device: &Device,
        connection_handle: u16,
        source_cid: u16,
        capabilities: Vec<ServiceCapabilities>,
    ) -> Result<Self, String> {
        let mut session = AvdtpSession::default();
        session.add_endpoint(MediaType::AUDIO, StreamEndpointType::SINK, capabilities);
        Ok(Self {
            connection_handle,
            signaling: AvdtpDeviceSession::new(device, connection_handle, source_cid, session)
                .map_err(|error| error.to_string())?,
            media: None,
        })
    }

    fn attach_media(&mut self, device: &Device, source_cid: u16) -> Result<(), String> {
        if self.media.is_some() {
            return Err("an AVDTP media channel is already open".into());
        }
        self.media = Some(
            DeviceMediaTransport::new(device, self.connection_handle, source_cid)
                .map_err(|error| error.to_string())?,
        );
        Ok(())
    }

    fn poll(
        &mut self,
        link: &mut LocalLink,
        device: &mut Device,
    ) -> Result<Vec<SinkEvent>, String> {
        self.signaling
            .poll(link, device)
            .map_err(|error| error.to_string())?;
        let mut events = Vec::new();
        for event in self.signaling.session_mut().take_events() {
            match event {
                SessionEvent::Configured { seid, .. } | SessionEvent::Reconfigured { seid } => {
                    if let Some(endpoint) = self.signaling.session().endpoint(seid) {
                        events.push(SinkEvent::Configured(endpoint.configuration.clone()));
                    }
                }
                SessionEvent::Opened { .. } => events.push(SinkEvent::Opened),
                SessionEvent::Started { .. } => events.push(SinkEvent::Started),
                SessionEvent::Suspended { .. } => events.push(SinkEvent::Suspended),
                SessionEvent::Closed { .. } | SessionEvent::Aborted { .. } => {
                    events.push(SinkEvent::Stopped)
                }
                SessionEvent::DelayReport { delay, .. } => {
                    events.push(SinkEvent::DelayReport(delay))
                }
                SessionEvent::SecurityControl { .. } => {}
            }
        }
        if let Some(media) = &mut self.media {
            media.poll(device).map_err(|error| error.to_string())?;
            events.extend(media.take_packets().into_iter().map(SinkEvent::Packet));
            if device
                .classic_channel(self.connection_handle, media.source_cid())
                .is_some_and(|channel| channel.state == ClassicChannelState::Closed)
            {
                self.media = None;
                events.push(SinkEvent::Closed);
            }
        }
        Ok(events)
    }
}

fn avdtp_request(
    host: &mut ExternalHost,
    device: &mut Device,
    runtime: &mut SinkRuntime,
    request: AvdtpMessage,
) -> Result<AvdtpMessage, String> {
    let label = runtime
        .signaling
        .send_command(host, device, request)
        .map_err(|error| error.to_string())?;
    let deadline = Instant::now() + PROCEDURE_TIMEOUT;
    loop {
        device.poll(host);
        runtime
            .signaling
            .poll(host, device)
            .map_err(|error| error.to_string())?;
        if let Some(response) = runtime.signaling.take_response(label) {
            return Ok(response);
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err("timed out waiting for an AVDTP response".into());
        }
        match host
            .wait_for_activity(remaining)
            .map_err(|error| error.to_string())?
        {
            ExternalHostActivity::Packet => {}
            ExternalHostActivity::Timeout => {
                return Err("timed out waiting for an AVDTP response".into());
            }
            ExternalHostActivity::Ended => {
                return Err("transport ended while waiting for an AVDTP response".into());
            }
        }
    }
}

fn discover_remote_endpoints(
    host: &mut ExternalHost,
    device: &mut Device,
    runtime: &mut SinkRuntime,
) -> Result<(), String> {
    let response = avdtp_request(host, device, runtime, AvdtpMessage::DiscoverCommand)?;
    let AvdtpMessage::DiscoverResponse { endpoints } = response else {
        return Err(format!("AVDTP Discover rejected: {response:?}"));
    };
    println!("@@@ Found {} endpoints", endpoints.len());
    for endpoint in endpoints {
        let all_capabilities = avdtp_request(
            host,
            device,
            runtime,
            AvdtpMessage::GetAllCapabilitiesCommand {
                acp_seid: endpoint.seid,
            },
        )?;
        let capabilities = match all_capabilities {
            AvdtpMessage::GetAllCapabilitiesResponse { capabilities } => capabilities,
            _ => match avdtp_request(
                host,
                device,
                runtime,
                AvdtpMessage::GetCapabilitiesCommand {
                    acp_seid: endpoint.seid,
                },
            )? {
                AvdtpMessage::GetCapabilitiesResponse { capabilities } => capabilities,
                response => {
                    println!(
                        "@@@ endpoint {} capability request: {response:?}",
                        endpoint.seid
                    );
                    Vec::new()
                }
            },
        };
        println!("@@@ {endpoint:?} {capabilities:?}");
    }
    Ok(())
}

fn start_incoming_pairing(
    device: &Device,
    connection_handle: u16,
    store: &JsonKeyStore,
) -> Result<ClassicPairingSession, String> {
    let peer = device
        .classic_connection(connection_handle)
        .ok_or_else(|| "Classic connection disappeared".to_string())?
        .peer_address
        .clone();
    let stored_keys = store
        .get(&peer.to_string(false))
        .map_err(|error| error.to_string())?;
    let mut pairing = ClassicPairingSession::accept_all(
        device,
        connection_handle,
        PairingConfig {
            bonding: true,
            mitm: false,
            ..PairingConfig::default()
        },
        stored_keys,
    )
    .map_err(|error| error.to_string())?;
    pairing.listen(device).map_err(|error| error.to_string())?;
    Ok(pairing)
}

fn handle_sink_event(
    event: SinkEvent,
    codec: Codec,
    state: &mut StreamState,
    packets_received: &mut u64,
    bytes_received: &mut u64,
    outputs: &mut Outputs,
    ui: &UiServer,
) -> Result<(), String> {
    match event {
        SinkEvent::Configured(configuration) => {
            println!("Sink Configuration:");
            for capability in configuration {
                println!("  {capability:?}");
            }
        }
        SinkEvent::Opened => println!("Audio Stream Open"),
        SinkEvent::Started => {
            println!("Sink Started");
            *state = StreamState::Started;
            outputs.start(codec)?;
            ui.set_stream_state(*state);
        }
        SinkEvent::Suspended => {
            println!("Sink Suspended");
            *state = StreamState::Suspended;
            ui.set_stream_state(*state);
        }
        SinkEvent::Stopped => {
            println!("Sink Stopped");
            *state = StreamState::Stopped;
            outputs.stop();
            ui.set_stream_state(*state);
        }
        SinkEvent::Closed => {
            println!("RTP Channel Closed");
            *state = StreamState::Idle;
            outputs.stop();
            ui.set_stream_state(*state);
        }
        SinkEvent::DelayReport(delay) => println!("Delay report: {delay}"),
        SinkEvent::Packet(packet) => {
            *packets_received += 1;
            *bytes_received += packet.payload.len() as u64;
            println!(
                "[{} bytes in {} packets] {:?}",
                *bytes_received, *packets_received, packet
            );
            let audio = extract_audio(codec, &packet)?;
            outputs.write(&audio)?;
            ui.send_audio(audio);
        }
    }
    Ok(())
}

fn run(args: Args) -> Result<(), String> {
    let config = load_device_config(args.device_config.as_deref())?;
    let capabilities = sink_capabilities(&args)?;
    let mut outputs = Outputs::new(&args.outputs)?;
    let ui = UiServer::start(args.ui_port, args.codec)?;
    println!("<<< connecting to HCI...");
    let transport = open_split_transport(&args.transport).map_err(|error| error.to_string())?;
    let mut host = ExternalHost::new(transport);
    let mut device = Device::new(0);
    host.initialize_device(&mut device, COMMAND_TIMEOUT)
        .map_err(|error| error.to_string())?;
    configure_identity(&mut host, &config)?;
    let public_address = read_public_address(&mut host)?;
    println!("Speaker Name: {}", config.name);
    println!(
        "Speaker Bluetooth Address: {}",
        public_address.to_string(false)
    );
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
            Some(u32::from(AVDTP_PSM)),
            ClassicChannelSpec {
                mtu: CLASSIC_L2CAP_MTU,
            },
        )
        .map_err(|error| error.to_string())?;
    let namespace = public_address.to_string(false);
    let mut store = JsonKeyStore::with_default_path(Some(&namespace));
    let mut active_handle = None;
    let mut pairing = None;
    let mut sdp_endpoints = Vec::<SdpEndpoint>::new();
    let mut sink = None::<SinkRuntime>;
    let mut discovery_pending = args.discover;
    let mut stream_state = StreamState::Idle;
    let mut packets_received = 0u64;
    let mut bytes_received = 0u64;

    println!("Speaker ready to play, codec={}", args.codec.as_str());
    if let Some(address_or_name) = &args.connect_address {
        let peer = resolve_classic_peer(&mut host, &mut device, address_or_name)?;
        println!("=== Connecting to {peer}...");
        let handle = connect_classic(&mut host, &mut device, peer.clone())?;
        ui.set_connection(peer.to_string(false), String::new());
        let _ = command(
            &mut host,
            HciCommand::RemoteNameRequest {
                bd_addr: peer,
                page_scan_repetition_mode: 2,
                reserved: 0,
                clock_offset: 0,
            },
            "requesting Classic remote name",
        );
        println!("*** Authenticating...");
        authenticate_classic(&mut host, &mut device, handle, &mut store)?;
        println!("*** Authenticated");
        println!("*** Enabling encryption...");
        encrypt_classic(&mut host, &mut device, handle)?;
        println!("*** Encryption on");
        let source_cid = open_classic_channel(&mut host, &mut device, handle, AVDTP_PSM)?;
        sink = Some(SinkRuntime::new(
            &device,
            handle,
            source_cid,
            capabilities.clone(),
        )?);
        active_handle = Some(handle);
    } else {
        println!("Waiting for connection...");
        set_scan_enabled(&mut host, true)?;
    }

    loop {
        device.poll(&mut host);
        if active_handle.is_none() {
            for request in device.take_classic_connection_requests() {
                device.accept_classic(&mut host, request);
            }
            if let Some(handle) = device.classic_connection_handle() {
                active_handle = Some(handle);
                pairing = Some(start_incoming_pairing(&device, handle, &store)?);
                let peer = device
                    .classic_connection(handle)
                    .expect("active Classic connection")
                    .peer_address
                    .clone();
                println!("Connection: {peer}");
                ui.set_connection(peer.to_string(false), String::new());
                let _ = command(
                    &mut host,
                    HciCommand::RemoteNameRequest {
                        bd_addr: peer,
                        page_scan_repetition_mode: 2,
                        reserved: 0,
                        clock_offset: 0,
                    },
                    "requesting Classic remote name",
                );
            }
        }
        let Some(handle) = active_handle else {
            match host
                .wait_for_activity(Duration::from_secs(1))
                .map_err(|error| error.to_string())?
            {
                ExternalHostActivity::Packet | ExternalHostActivity::Timeout => continue,
                ExternalHostActivity::Ended => break,
            }
        };
        if device.classic_connection(handle).is_none() {
            println!("Disconnection");
            outputs.stop();
            ui.clear_connection();
            ui.set_stream_state(StreamState::Idle);
            active_handle = None;
            pairing = None;
            sink = None;
            sdp_endpoints.clear();
            discovery_pending = args.discover;
            set_scan_enabled(&mut host, true)?;
            continue;
        }
        if let Some(session) = &mut pairing {
            if let Some(keys) = session
                .drive_once(&mut host, &mut device)
                .map_err(|error| error.to_string())?
            {
                if keys.link_key.is_some() {
                    store
                        .update(&session.peer_address().to_string(false), keys)
                        .map_err(|error| error.to_string())?;
                }
                pairing = None;
            }
        }
        for (status, address, name) in device.take_classic_remote_names() {
            if status == 0
                && device
                    .classic_connection(handle)
                    .is_some_and(|connection| connection.peer_address == address)
            {
                ui.set_connection(address.to_string(false), name);
            }
        }
        for source_cid in device.take_accepted_classic_channels(handle) {
            let channel = device
                .classic_channel(handle, source_cid)
                .ok_or_else(|| "accepted Classic channel disappeared".to_string())?;
            match u16::try_from(channel.psm).ok() {
                Some(SDP_PSM) => {
                    sdp_endpoints.push(SdpEndpoint::new(source_cid, channel.peer_mtu));
                }
                Some(AVDTP_PSM) if sink.is_none() => {
                    println!("Audio Stream Open");
                    sink = Some(SinkRuntime::new(
                        &device,
                        handle,
                        source_cid,
                        capabilities.clone(),
                    )?);
                }
                Some(AVDTP_PSM) => {
                    println!("RTP Channel Open");
                    sink.as_mut()
                        .expect("sink exists")
                        .attach_media(&device, source_cid)?;
                }
                _ => {}
            }
        }
        for endpoint in &mut sdp_endpoints {
            endpoint.poll(&mut host, &mut device, handle)?;
        }
        sdp_endpoints.retain(|endpoint| {
            device
                .classic_channel(handle, endpoint.source_cid)
                .is_some_and(|channel| channel.state != ClassicChannelState::Closed)
        });
        if let Some(runtime) = &mut sink {
            if discovery_pending {
                discover_remote_endpoints(&mut host, &mut device, runtime)?;
                discovery_pending = false;
            }
            for event in runtime.poll(&mut host, &mut device)? {
                handle_sink_event(
                    event,
                    args.codec,
                    &mut stream_state,
                    &mut packets_received,
                    &mut bytes_received,
                    &mut outputs,
                    &ui,
                )?;
            }
            if device
                .classic_channel(handle, runtime.signaling.source_cid())
                .is_some_and(|channel| channel.state == ClassicChannelState::Closed)
            {
                println!("Audio Stream Closed");
                sink = None;
            }
        }
        match host
            .wait_for_activity(POLL_INTERVAL)
            .map_err(|error| error.to_string())?
        {
            ExternalHostActivity::Packet | ExternalHostActivity::Timeout => {}
            ExternalHostActivity::Ended => break,
        }
    }
    outputs.stop();
    let _ = ui.port();
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
    use bumble_controller::{Controller, LocalLink as ControllerLocalLink};
    use bumble_host::pump as pump_devices;

    fn test_address(value: &str) -> Address {
        Address::parse(value, AddressType::PUBLIC_DEVICE).unwrap()
    }

    fn connect_test_devices(
        link: &mut ControllerLocalLink,
        devices: &mut [Device; 2],
        source_address: &Address,
        sink_address: &Address,
    ) {
        devices[0].connect_classic(link, sink_address.clone());
        devices[0].poll(link);
        link.pump_classic();
        devices[1].poll(link);
        devices[1].accept_classic(link, source_address.clone());
        devices[1].poll(link);
        link.pump_classic();
        devices[0].poll(link);
    }

    fn drive_stream(
        link: &mut ControllerLocalLink,
        devices: &mut [Device; 2],
        source: &mut AvdtpDeviceSession,
        sink: &mut SinkRuntime,
    ) -> Vec<SinkEvent> {
        let mut events = Vec::new();
        for _ in 0..64 {
            source.poll(link, &mut devices[0]).unwrap();
            events.extend(sink.poll(link, &mut devices[1]).unwrap());
            pump_devices(link, devices);
        }
        events
    }

    fn args(codec: Codec) -> Args {
        Args {
            codec,
            sampling_frequencies: Vec::new(),
            bitrate: None,
            vbr: true,
            discover: false,
            outputs: Vec::new(),
            ui_port: DEFAULT_UI_PORT,
            connect_address: None,
            device_config: None,
            transport: "usb:0".into(),
        }
    }

    #[test]
    fn parses_complete_upstream_cli() {
        let parsed = parse_args(
            [
                "speaker",
                "--codec",
                "opus",
                "--sampling-frequency=48000",
                "--bitrate",
                "128000",
                "--no-vbr",
                "--discover",
                "--output",
                "audio.opus",
                "--output=@ffplay",
                "--ui-port",
                "9000",
                "--connect",
                "Bumble Player",
                "--device-config=device.json",
                "usb:0",
            ]
            .map(str::to_string),
        )
        .unwrap();
        assert_eq!(parsed.codec, Codec::Opus);
        assert_eq!(parsed.sampling_frequencies, [48_000]);
        assert_eq!(parsed.bitrate, Some(128_000));
        assert!(!parsed.vbr);
        assert!(parsed.discover);
        assert_eq!(parsed.outputs, ["audio.opus", "@ffplay"]);
        assert_eq!(parsed.ui_port, 9000);
        assert_eq!(parsed.connect_address.as_deref(), Some("Bumble Player"));
        assert_eq!(parsed.device_config, Some(PathBuf::from("device.json")));
        assert_eq!(parsed.transport, "usb:0");
    }

    #[test]
    fn default_codec_capabilities_match_upstream() {
        let MediaCodecInformation::Sbc(sbc) = codec_information(&args(Codec::Sbc)).unwrap() else {
            panic!("expected SBC capabilities");
        };
        assert_eq!(sbc.sampling_frequency.0, 0x0F);
        assert_eq!(sbc.channel_mode.0, 0x0F);
        assert_eq!(sbc.maximum_bitpool_value, 53);

        let MediaCodecInformation::Aac(aac) = codec_information(&args(Codec::Aac)).unwrap() else {
            panic!("expected AAC capabilities");
        };
        assert!(aac.vbr);
        assert_eq!(aac.bitrate, 256_000);
        assert_eq!(aac.channels.0, 3);

        let MediaCodecInformation::Opus(opus) = codec_information(&args(Codec::Opus)).unwrap()
        else {
            panic!("expected Opus capabilities");
        };
        assert_eq!(opus.channel_mode.0, 7);
        assert_eq!(opus.frame_size.0, 3);
    }

    #[test]
    fn extracts_sbc_and_opus_media_headers() {
        let packet = MediaPacket::new(96, 1, 0, 0, vec![1, 2, 3, 4]);
        assert_eq!(extract_audio(Codec::Sbc, &packet).unwrap(), [2, 3, 4]);
        assert_eq!(extract_audio(Codec::Opus, &packet).unwrap(), [2, 3, 4]);
        assert!(extract_audio(Codec::Sbc, &MediaPacket::new(96, 1, 0, 0, Vec::new())).is_err());
    }

    #[test]
    fn ui_serves_static_page() {
        let ui = UiServer::start(0, Codec::Aac).unwrap();
        let mut stream = TcpStream::connect(("127.0.0.1", ui.port())).unwrap();
        stream
            .write_all(b"GET /speaker.html HTTP/1.1\r\nHost: localhost\r\n\r\n")
            .unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        assert!(response.starts_with("HTTP/1.1 200 OK"));
        assert!(response.contains("Bumble Virtual Speaker"));
    }

    #[test]
    fn ui_websocket_replays_the_current_snapshot() {
        let ui = UiServer::start(0, Codec::Opus).unwrap();
        ui.set_connection("22:22:22:22:22:22".into(), "Bumble Player".into());
        ui.set_stream_state(StreamState::Started);
        let (mut socket, _) =
            tungstenite::connect(format!("ws://127.0.0.1:{}/channel", ui.port())).unwrap();
        socket
            .send(WebSocketMessage::Text(
                json!({"type": "hello"}).to_string().into(),
            ))
            .unwrap();
        let hello = socket.read().unwrap().into_text().unwrap();
        let hello: serde_json::Value = serde_json::from_str(&hello).unwrap();
        assert_eq!(hello["type"], "hello");
        assert_eq!(hello["params"]["codec"], "opus");
        assert_eq!(hello["params"]["streamState"], "STARTED");
        let connection = socket.read().unwrap().into_text().unwrap();
        let connection: serde_json::Value = serde_json::from_str(&connection).unwrap();
        assert_eq!(connection["type"], "connection");
        assert_eq!(connection["params"]["peer_address"], "22:22:22:22:22:22");
        assert_eq!(connection["params"]["peer_name"], "Bumble Player");
    }

    #[test]
    fn production_sink_receives_a_stream_over_two_controllers() {
        let source_address = test_address("11:11:11:11:11:11");
        let sink_address = test_address("22:22:22:22:22:22");
        let mut link = ControllerLocalLink::new();
        let source_id = link.add_controller(Controller::new("source", source_address.clone()));
        let sink_id = link.add_controller(Controller::new("sink", sink_address.clone()));
        let mut devices = [Device::new(source_id), Device::new(sink_id)];
        devices[1]
            .register_classic_channel_server(
                Some(u32::from(AVDTP_PSM)),
                ClassicChannelSpec {
                    mtu: CLASSIC_L2CAP_MTU,
                },
            )
            .unwrap();
        connect_test_devices(&mut link, &mut devices, &source_address, &sink_address);
        let source_handle = devices[0].classic_connection_handle().unwrap();
        let sink_handle = devices[1].classic_connection_handle().unwrap();
        let source_signaling_cid = devices[0]
            .connect_classic_channel(
                &mut link,
                source_handle,
                u32::from(AVDTP_PSM),
                ClassicChannelSpec {
                    mtu: CLASSIC_L2CAP_MTU,
                },
            )
            .unwrap();
        pump_devices(&mut link, &mut devices);
        let sink_signaling_cid = devices[1]
            .take_accepted_classic_channels(sink_handle)
            .into_iter()
            .next()
            .expect("speaker accepted the AVDTP signaling channel");
        let capabilities = sink_capabilities(&args(Codec::Sbc)).unwrap();
        let mut source = AvdtpDeviceSession::new(
            &devices[0],
            source_handle,
            source_signaling_cid,
            AvdtpSession::default(),
        )
        .unwrap();
        let mut sink = SinkRuntime::new(
            &devices[1],
            sink_handle,
            sink_signaling_cid,
            capabilities.clone(),
        )
        .unwrap();

        let discover = source
            .send_command(&mut link, &mut devices[0], AvdtpMessage::DiscoverCommand)
            .unwrap();
        let mut sink_events = drive_stream(&mut link, &mut devices, &mut source, &mut sink);
        let sink_seid = match source.take_response(discover) {
            Some(AvdtpMessage::DiscoverResponse { endpoints }) => endpoints[0].seid,
            response => panic!("unexpected Discover response: {response:?}"),
        };
        let source_seid = source.session_mut().add_endpoint(
            MediaType::AUDIO,
            StreamEndpointType::SOURCE,
            capabilities.clone(),
        );
        let configure = source
            .send_command(
                &mut link,
                &mut devices[0],
                AvdtpMessage::SetConfigurationCommand {
                    acp_seid: sink_seid,
                    int_seid: source_seid,
                    capabilities: capabilities.clone(),
                },
            )
            .unwrap();
        sink_events.extend(drive_stream(
            &mut link,
            &mut devices,
            &mut source,
            &mut sink,
        ));
        assert_eq!(
            source.take_response(configure),
            Some(AvdtpMessage::SetConfigurationResponse)
        );
        let open = source
            .send_command(
                &mut link,
                &mut devices[0],
                AvdtpMessage::OpenCommand {
                    acp_seid: sink_seid,
                },
            )
            .unwrap();
        sink_events.extend(drive_stream(
            &mut link,
            &mut devices,
            &mut source,
            &mut sink,
        ));
        assert_eq!(source.take_response(open), Some(AvdtpMessage::OpenResponse));

        let source_media_cid = devices[0]
            .connect_classic_channel(
                &mut link,
                source_handle,
                u32::from(AVDTP_PSM),
                ClassicChannelSpec {
                    mtu: CLASSIC_L2CAP_MTU,
                },
            )
            .unwrap();
        pump_devices(&mut link, &mut devices);
        let sink_media_cid = devices[1]
            .take_accepted_classic_channels(sink_handle)
            .into_iter()
            .next()
            .expect("speaker accepted the AVDTP media channel");
        sink.attach_media(&devices[1], sink_media_cid).unwrap();
        let source_media =
            DeviceMediaTransport::new(&devices[0], source_handle, source_media_cid).unwrap();
        let start = source
            .send_command(
                &mut link,
                &mut devices[0],
                AvdtpMessage::StartCommand {
                    acp_seids: vec![sink_seid],
                },
            )
            .unwrap();
        sink_events.extend(drive_stream(
            &mut link,
            &mut devices,
            &mut source,
            &mut sink,
        ));
        assert_eq!(
            source.take_response(start),
            Some(AvdtpMessage::StartResponse)
        );

        let packet = MediaPacket::new(96, 7, 1024, 0x1234, vec![1, 2, 3, 4]);
        source_media
            .send(&mut link, &mut devices[0], &packet)
            .unwrap();
        sink_events.extend(drive_stream(
            &mut link,
            &mut devices,
            &mut source,
            &mut sink,
        ));

        assert!(sink_events
            .iter()
            .any(|event| matches!(event, SinkEvent::Configured(value) if value == &capabilities)));
        assert!(sink_events
            .iter()
            .any(|event| matches!(event, SinkEvent::Opened)));
        assert!(sink_events
            .iter()
            .any(|event| matches!(event, SinkEvent::Started)));
        assert!(sink_events
            .iter()
            .any(|event| matches!(event, SinkEvent::Packet(value) if value == &packet)));
        assert!(devices[0].take_classic_channel_errors().is_empty());
        assert!(devices[1].take_classic_channel_errors().is_empty());
    }
}
