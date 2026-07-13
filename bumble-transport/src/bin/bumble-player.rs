use bumble::advertising_data::Type as AdvertisingDataType;
use bumble::keys::{JsonKeyStore, KeyStore};
use bumble::{
    Address, AddressType, AdvertisingData, ClassOfDevice, MajorDeviceClass, MajorServiceClasses,
    Uuid,
};
use bumble_a2dp::media::{
    packetize_aac, packetize_opus, packetize_sbc, parse_ogg_opus, AacFrame,
    OpusChannelMode as ParsedOpusChannelMode, OpusPacket, SbcFrame,
};
use bumble_a2dp::sdp::{
    make_audio_source_sdp_record, parse_sdp_record, ProfileVersion as A2dpProfileVersion,
    ServiceRole, AUDIO_SINK_SERVICE_UUID,
};
use bumble_a2dp::transport::DeviceMediaTransport;
use bumble_a2dp::{
    AacChannels, AacMediaCodecInformation, AacObjectType, AacSamplingFrequency, CodecType,
    MediaCodecInformation, OpusChannelMode, OpusFrameSize, OpusMediaCodecInformation,
    OpusSamplingFrequency, SbcAllocationMethod, SbcBlockLength, SbcChannelMode,
    SbcMediaCodecInformation, SbcSamplingFrequency, SbcSubbands,
};
use bumble_avctp::{DeviceProtocol as AvctpDeviceProtocol, AVCTP_PSM};
use bumble_avdtp::host::DeviceSession as AvdtpDeviceSession;
use bumble_avdtp::session::Session as AvdtpSession;
use bumble_avdtp::{
    EndpointInfo, MediaType, Message as AvdtpMessage, ServiceCapabilities, ServiceCategory,
    StreamEndpointType, AVDTP_PSM,
};
use bumble_avrcp::{Runtime as AvrcpRuntime, RuntimeEvent as AvrcpRuntimeEvent, AVRCP_PID};
use bumble_hci::{Command, ReturnParameters};
use bumble_host::{Device, LocalLink};
use bumble_l2cap::{ClassicChannelSpec, ClassicChannelState};
use bumble_rtp::MediaPacket;
use bumble_sdp::service::{
    AttributeId, SdpClient, SdpRequestHandler, SdpServer, SdpTransport, TransportError,
};
use bumble_sdp::{SdpPdu, SDP_PSM};
use bumble_smp::PairingConfig;
use bumble_transport::{
    open_split_transport, ClassicPairingSession, CommandResponse, ExternalHost,
    ExternalHostActivity,
};
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Duration, Instant};

const DEFAULT_NAME: &str = "Bumble Player";
const CLASSIC_L2CAP_MTU: u16 = 2048;
const COMMAND_TIMEOUT: Duration = Duration::from_secs(5);
const PROCEDURE_TIMEOUT: Duration = Duration::from_secs(30);
const PAIRING_TIMEOUT: Duration = Duration::from_secs(120);
const POLL_INTERVAL: Duration = Duration::from_millis(10);
const A2DP_SERVICE_HANDLE: u32 = 0x0001_0001;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AudioFormat {
    Auto,
    Sbc,
    Aac,
    Opus,
}

impl AudioFormat {
    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "auto" => Ok(Self::Auto),
            "sbc" => Ok(Self::Sbc),
            "aac" => Ok(Self::Aac),
            "opus" => Ok(Self::Opus),
            _ => Err("--audio-format must be auto, sbc, aac, or opus".into()),
        }
    }

    fn infer(path: &Path) -> Result<Self, String> {
        match path
            .extension()
            .and_then(|extension| extension.to_str())
            .map(str::to_ascii_lowercase)
            .as_deref()
        {
            Some("sbc") => Ok(Self::Sbc),
            Some("aac" | "adts") => Ok(Self::Aac),
            Some("ogg") => Ok(Self::Opus),
            _ => Err("unable to determine audio format from file extension".into()),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum PlayerCommand {
    Discover,
    Inquire {
        address: String,
    },
    Pair {
        address: String,
    },
    Play {
        address: Option<String>,
        audio_format: AudioFormat,
        audio_file: PathBuf,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Args {
    hci_transport: String,
    device_config: Option<PathBuf>,
    authenticate: bool,
    encrypt: bool,
    command: PlayerCommand,
}

#[derive(Clone, Debug)]
struct DeviceConfig {
    name: String,
    class_of_device: u32,
}

fn usage() -> &'static str {
    "usage: bumble-player --hci-transport TRANSPORT [--device-config PATH] [--authenticate] [--encrypt] <discover | inquire ADDRESS | pair ADDRESS | play [--connect ADDRESS] [-f|--audio-format auto|sbc|aac|opus] AUDIO_FILE>"
}

fn option_value(
    argument: &str,
    short: Option<&str>,
    long: &str,
    arguments: &mut VecDeque<String>,
) -> Result<Option<String>, String> {
    if short.is_some_and(|short| argument == short) || argument == long {
        return arguments
            .pop_front()
            .map(Some)
            .ok_or_else(|| format!("missing value for {long}"));
    }
    Ok(short
        .and_then(|short| argument.strip_prefix(&format!("{short}=")))
        .or_else(|| argument.strip_prefix(&format!("{long}=")))
        .map(ToOwned::to_owned))
}

fn parse_play(mut arguments: VecDeque<String>) -> Result<PlayerCommand, String> {
    let mut address = None;
    let mut audio_format = AudioFormat::Auto;
    let mut audio_file = None;
    while let Some(argument) = arguments.pop_front() {
        if matches!(argument.as_str(), "-h" | "--help") {
            return Err(usage().into());
        }
        if let Some(value) = option_value(&argument, None, "--connect", &mut arguments)? {
            address = Some(value);
            continue;
        }
        if let Some(value) = option_value(&argument, Some("-f"), "--audio-format", &mut arguments)?
        {
            audio_format = AudioFormat::parse(&value)?;
            continue;
        }
        if argument.starts_with('-') {
            return Err(format!("unknown option {argument:?}"));
        }
        if audio_file.replace(PathBuf::from(argument)).is_some() {
            return Err(usage().into());
        }
    }
    Ok(PlayerCommand::Play {
        address,
        audio_format,
        audio_file: audio_file.ok_or_else(|| usage().to_string())?,
    })
}

fn parse_args(arguments: impl IntoIterator<Item = String>) -> Result<Args, String> {
    let mut arguments: VecDeque<_> = arguments.into_iter().skip(1).collect();
    let mut hci_transport = None;
    let mut device_config = None;
    let mut authenticate = false;
    // This intentionally matches upstream's Click declaration: --encrypt is a
    // flag whose default is true, so encryption is enabled even when omitted.
    let mut encrypt = true;
    let command = loop {
        let argument = arguments.pop_front().ok_or_else(|| usage().to_string())?;
        if matches!(argument.as_str(), "-h" | "--help") {
            return Err(usage().into());
        }
        if argument == "--authenticate" {
            authenticate = true;
            continue;
        }
        if argument == "--encrypt" {
            encrypt = true;
            continue;
        }
        if let Some(value) = option_value(&argument, None, "--hci-transport", &mut arguments)? {
            hci_transport = Some(value);
            continue;
        }
        if let Some(value) = option_value(&argument, None, "--device-config", &mut arguments)? {
            device_config = Some(PathBuf::from(value));
            continue;
        }
        break match argument.as_str() {
            "discover" => {
                if !arguments.is_empty() {
                    return Err(usage().into());
                }
                PlayerCommand::Discover
            }
            "inquire" | "pair" => {
                let address = arguments.pop_front().ok_or_else(|| usage().to_string())?;
                if !arguments.is_empty() {
                    return Err(usage().into());
                }
                if argument == "inquire" {
                    PlayerCommand::Inquire { address }
                } else {
                    PlayerCommand::Pair { address }
                }
            }
            "play" => parse_play(arguments)?,
            _ if argument.starts_with('-') => return Err(format!("unknown option {argument:?}")),
            _ => return Err(usage().into()),
        };
    };
    Ok(Args {
        hci_transport: hci_transport.ok_or_else(|| "--hci-transport is required".to_string())?,
        device_config,
        authenticate,
        encrypt,
        command,
    })
}

fn default_class_of_device() -> u32 {
    ClassOfDevice::new(MajorServiceClasses::AUDIO, MajorDeviceClass::AUDIO_VIDEO, 0).to_int()
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
            class_of_device: default_class_of_device(),
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
            .unwrap_or_else(default_class_of_device),
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
    command: Command,
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
    let name_len = name.len().min(local_name.len());
    local_name[..name_len].copy_from_slice(&name[..name_len]);
    command(
        host,
        Command::WriteLocalName { local_name },
        "writing local name",
    )?;
    command(
        host,
        Command::WriteClassOfDevice {
            class_of_device: config.class_of_device,
        },
        "writing Class of Device",
    )?;
    command(
        host,
        Command::WriteSimplePairingMode {
            simple_pairing_mode: 1,
        },
        "enabling Secure Simple Pairing",
    )?;
    let mut extended_inquiry_response = [0; 240];
    let eir_name_len = name.len().min(extended_inquiry_response.len() - 2);
    extended_inquiry_response[0] = (eir_name_len + 1) as u8;
    extended_inquiry_response[1] = AdvertisingDataType::COMPLETE_LOCAL_NAME.0;
    extended_inquiry_response[2..2 + eir_name_len].copy_from_slice(&name[..eir_name_len]);
    command(
        host,
        Command::WriteExtendedInquiryResponse {
            fec_required: 0,
            extended_inquiry_response,
        },
        "writing extended inquiry response",
    )?;
    Ok(())
}

fn read_public_address(host: &mut ExternalHost) -> Result<Address, String> {
    let response = command(host, Command::ReadBdAddr, "reading public address")?;
    match response.return_parameters() {
        Some(ReturnParameters::ReadBdAddr { bd_addr, .. }) => Ok(bd_addr.clone()),
        _ => Err("controller did not return a public address".into()),
    }
}

fn set_scan_enabled(host: &mut ExternalHost, enabled: bool) -> Result<(), String> {
    command(
        host,
        Command::WriteScanEnable {
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
                return Err("HCI transport ended while waiting for Classic connection".into())
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
    device.take_classic_inquiry_result_details();
    device.take_classic_inquiry_complete();
    device.take_classic_remote_names();
    command(
        host,
        Command::Inquiry {
            lap: 0x9E8B33,
            inquiry_length: 8,
            num_responses: 0,
        },
        "starting Classic inquiry",
    )?;
    let inquiry_deadline = Instant::now() + PROCEDURE_TIMEOUT;
    let mut candidates = Vec::new();
    let completed = loop {
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
            break true;
        }
        let remaining = inquiry_deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            break false;
        }
        match host
            .wait_for_activity(remaining)
            .map_err(|error| error.to_string())?
        {
            ExternalHostActivity::Packet => {}
            ExternalHostActivity::Timeout | ExternalHostActivity::Ended => break false,
        }
    };
    if !completed {
        let _ = command(host, Command::InquiryCancel, "canceling Classic inquiry");
    }

    for address in candidates {
        command(
            host,
            Command::RemoteNameRequest {
                bd_addr: address.clone(),
                page_scan_repetition_mode: 2,
                reserved: 0,
                clock_offset: 0,
            },
            "requesting Classic remote name",
        )?;
        let name_deadline = Instant::now() + PROCEDURE_TIMEOUT;
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
            let remaining = name_deadline.saturating_duration_since(Instant::now());
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

fn secure_connection(
    host: &mut ExternalHost,
    device: &mut Device,
    connection_handle: u16,
    authenticate: bool,
    encrypt: bool,
    store: &mut JsonKeyStore,
) -> Result<(), String> {
    if authenticate || encrypt {
        println!("*** Authenticating...");
        authenticate_classic(host, device, connection_handle, store)?;
        println!("*** Authenticated");
    }
    if encrypt {
        println!("*** Enabling encryption...");
        encrypt_classic(host, device, connection_handle)?;
        println!("*** Encryption on");
    }
    Ok(())
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

struct ExternalSdpTransport<'a> {
    host: &'a mut ExternalHost,
    device: &'a mut Device,
    services: &'a mut AncillaryServices,
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
            self.services
                .poll(self.host, self.device, self.connection_handle)
                .map_err(TransportError)?;
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

fn find_a2dp_sink_service(
    host: &mut ExternalHost,
    device: &mut Device,
    services: &mut AncillaryServices,
    connection_handle: u16,
) -> Result<A2dpProfileVersion, String> {
    let source_cid = open_classic_channel(host, device, connection_handle, SDP_PSM)?;
    let services = {
        let transport = ExternalSdpTransport {
            host,
            device,
            services,
            connection_handle,
            source_cid,
        };
        SdpClient::new(transport)
            .service_search_attribute(
                &[Uuid::from_16_bits(AUDIO_SINK_SERVICE_UUID)],
                &[AttributeId::Range(0, 0xFFFF)],
            )
            .map_err(|error| error.to_string())?
    };
    device
        .disconnect_classic_channel(host, connection_handle, source_cid)
        .map_err(|error| error.to_string())?;
    services
        .iter()
        .filter_map(|attributes| parse_sdp_record(attributes))
        .find(|service| service.role == ServiceRole::Sink)
        .map(|service| service.avdtp_version)
        .ok_or_else(|| "no A2DP sink service found".into())
}

fn local_sdp_server(peer_mtu: u16) -> SdpServer {
    let mut server = SdpServer::new(peer_mtu);
    server.add_service(
        A2DP_SERVICE_HANDLE,
        make_audio_source_sdp_record(A2DP_SERVICE_HANDLE, A2dpProfileVersion::V1_3),
    );
    server
}

struct SdpEndpoint {
    source_cid: u16,
    server: SdpServer,
}

impl SdpEndpoint {
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

struct AvrcpEndpoint {
    protocol: AvctpDeviceProtocol,
    runtime: AvrcpRuntime,
}

impl AvrcpEndpoint {
    fn new(device: &Device, connection_handle: u16, source_cid: u16) -> Result<Self, String> {
        let mut protocol = AvctpDeviceProtocol::new(device, connection_handle, source_cid)
            .map_err(|error| error.to_string())?;
        protocol.register_pid(AVRCP_PID);
        Ok(Self {
            protocol,
            runtime: AvrcpRuntime::new(512),
        })
    }

    fn poll(&mut self, link: &mut LocalLink, device: &mut Device) -> Result<(), String> {
        self.protocol
            .poll(link, device)
            .map_err(|error| error.to_string())?;
        for message in self.protocol.take_messages() {
            for event in self
                .runtime
                .handle_message(message)
                .map_err(|error| error.to_string())?
            {
                if let AvrcpRuntimeEvent::Send(message) = event {
                    self.protocol
                        .send(link, device, &message)
                        .map_err(|error| error.to_string())?;
                }
            }
        }
        Ok(())
    }
}

#[derive(Default)]
struct AncillaryServices {
    sdp: Vec<SdpEndpoint>,
    avrcp: Vec<AvrcpEndpoint>,
}

impl AncillaryServices {
    fn poll(
        &mut self,
        link: &mut LocalLink,
        device: &mut Device,
        connection_handle: u16,
    ) -> Result<(), String> {
        for source_cid in device.take_accepted_classic_channels(connection_handle) {
            let channel = device
                .classic_channel(connection_handle, source_cid)
                .ok_or_else(|| "accepted Classic channel disappeared".to_string())?;
            match u16::try_from(channel.psm).ok() {
                Some(SDP_PSM) => self.sdp.push(SdpEndpoint {
                    source_cid,
                    server: local_sdp_server(channel.peer_mtu),
                }),
                Some(AVCTP_PSM) => {
                    self.avrcp
                        .push(AvrcpEndpoint::new(device, connection_handle, source_cid)?)
                }
                _ => {}
            }
        }
        for endpoint in &mut self.sdp {
            endpoint.poll(link, device, connection_handle)?;
        }
        for endpoint in &mut self.avrcp {
            endpoint.poll(link, device)?;
        }
        self.sdp.retain(|endpoint| {
            device
                .classic_channel(connection_handle, endpoint.source_cid)
                .is_some_and(|channel| channel.state != ClassicChannelState::Closed)
        });
        self.avrcp.retain(|endpoint| {
            device
                .classic_channel(connection_handle, endpoint.protocol.source_cid())
                .is_some_and(|channel| channel.state != ClassicChannelState::Closed)
        });
        Ok(())
    }
}

fn avdtp_request(
    host: &mut ExternalHost,
    device: &mut Device,
    signaling: &mut AvdtpDeviceSession,
    services: &mut AncillaryServices,
    request: AvdtpMessage,
) -> Result<AvdtpMessage, String> {
    let label = signaling
        .send_command(host, device, request)
        .map_err(|error| error.to_string())?;
    let deadline = Instant::now() + PROCEDURE_TIMEOUT;
    loop {
        device.poll(host);
        signaling
            .poll(host, device)
            .map_err(|error| error.to_string())?;
        services.poll(host, device, signaling.connection_handle())?;
        if let Some(response) = signaling.take_response(label) {
            return Ok(response);
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err("timed out waiting for AVDTP response".into());
        }
        match host
            .wait_for_activity(remaining)
            .map_err(|error| error.to_string())?
        {
            ExternalHostActivity::Packet => {}
            ExternalHostActivity::Timeout => {
                return Err("timed out waiting for AVDTP response".into())
            }
            ExternalHostActivity::Ended => {
                return Err("transport ended while waiting for AVDTP response".into())
            }
        }
    }
}

#[derive(Clone, Debug)]
enum AudioData {
    Sbc(Vec<SbcFrame>),
    Aac(Vec<AacFrame>),
    Opus(Vec<OpusPacket>),
}

impl AudioData {
    fn load(path: &Path, format: AudioFormat) -> Result<Self, String> {
        let format = if format == AudioFormat::Auto {
            AudioFormat::infer(path)?
        } else {
            format
        };
        let bytes = std::fs::read(path)
            .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
        let audio = match format {
            AudioFormat::Sbc => {
                Self::Sbc(SbcFrame::parse_stream(&bytes).map_err(|error| error.to_string())?)
            }
            AudioFormat::Aac => {
                Self::Aac(AacFrame::parse_stream(&bytes).map_err(|error| error.to_string())?)
            }
            AudioFormat::Opus => {
                Self::Opus(parse_ogg_opus(&bytes).map_err(|error| error.to_string())?)
            }
            AudioFormat::Auto => unreachable!("auto format was resolved"),
        };
        if audio.is_empty() {
            return Err("audio file contains no media frames".into());
        }
        Ok(audio)
    }

    fn is_empty(&self) -> bool {
        match self {
            Self::Sbc(frames) => frames.is_empty(),
            Self::Aac(frames) => frames.is_empty(),
            Self::Opus(packets) => packets.is_empty(),
        }
    }

    fn codec_type(&self) -> CodecType {
        match self {
            Self::Sbc(_) => CodecType::SBC,
            Self::Aac(_) => CodecType::MPEG_2_4_AAC,
            Self::Opus(_) => CodecType::NON_A2DP,
        }
    }

    fn sampling_frequency(&self) -> u32 {
        match self {
            Self::Sbc(frames) => frames[0].sampling_frequency,
            Self::Aac(frames) => frames[0].sampling_frequency,
            Self::Opus(packets) => packets[0].sampling_frequency,
        }
    }

    fn describe(&self) -> String {
        match self {
            Self::Sbc(frames) => format!("SBC format: {:?}", frames[0]),
            Self::Aac(frames) => format!("AAC format: {:?}", frames[0]),
            Self::Opus(packets) => format!("Opus format: {:?}", packets[0]),
        }
    }

    fn codec_information(
        &self,
        remote: Option<&MediaCodecInformation>,
    ) -> Result<MediaCodecInformation, String> {
        Ok(match self {
            Self::Sbc(frames) => {
                let frame = &frames[0];
                let (minimum_bitpool_value, maximum_bitpool_value) = match remote {
                    Some(MediaCodecInformation::Sbc(info)) => {
                        (info.minimum_bitpool_value, info.maximum_bitpool_value)
                    }
                    _ => (2, 40),
                };
                MediaCodecInformation::Sbc(SbcMediaCodecInformation {
                    sampling_frequency: match frame.sampling_frequency {
                        16_000 => SbcSamplingFrequency::SF_16000,
                        32_000 => SbcSamplingFrequency::SF_32000,
                        44_100 => SbcSamplingFrequency::SF_44100,
                        48_000 => SbcSamplingFrequency::SF_48000,
                        _ => return Err("unsupported SBC sampling frequency".into()),
                    },
                    channel_mode: match frame.channel_mode {
                        0 => SbcChannelMode::MONO,
                        1 => SbcChannelMode::DUAL_CHANNEL,
                        2 => SbcChannelMode::STEREO,
                        3 => SbcChannelMode::JOINT_STEREO,
                        _ => return Err("unsupported SBC channel mode".into()),
                    },
                    block_length: match frame.block_count {
                        4 => SbcBlockLength::BL_4,
                        8 => SbcBlockLength::BL_8,
                        12 => SbcBlockLength::BL_12,
                        16 => SbcBlockLength::BL_16,
                        _ => return Err("unsupported SBC block length".into()),
                    },
                    subbands: match frame.subband_count {
                        4 => SbcSubbands::S_4,
                        8 => SbcSubbands::S_8,
                        _ => return Err("unsupported SBC subband count".into()),
                    },
                    allocation_method: if frame.allocation_method == 0 {
                        SbcAllocationMethod::LOUDNESS
                    } else {
                        SbcAllocationMethod::SNR
                    },
                    minimum_bitpool_value,
                    maximum_bitpool_value,
                })
            }
            Self::Aac(frames) => {
                let frame = &frames[0];
                MediaCodecInformation::Aac(AacMediaCodecInformation {
                    object_type: AacObjectType::MPEG_2_AAC_LC,
                    sampling_frequency: aac_sampling_frequency(frame.sampling_frequency)?,
                    channels: if frame.channel_configuration == 1 {
                        AacChannels::MONO
                    } else {
                        AacChannels::STEREO
                    },
                    vbr: true,
                    bitrate: 128_000,
                })
            }
            Self::Opus(packets) => {
                let packet = &packets[0];
                MediaCodecInformation::Opus(OpusMediaCodecInformation {
                    channel_mode: match packet.channel_mode {
                        ParsedOpusChannelMode::Mono => OpusChannelMode::MONO,
                        ParsedOpusChannelMode::Stereo => OpusChannelMode::STEREO,
                        ParsedOpusChannelMode::DualMono => OpusChannelMode::DUAL_MONO,
                    },
                    frame_size: if packet.duration_ms == 10 {
                        OpusFrameSize::FS_10MS
                    } else {
                        OpusFrameSize::FS_20MS
                    },
                    sampling_frequency: OpusSamplingFrequency::SF_48000,
                })
            }
        })
    }

    fn packetize(&self, mtu: usize) -> Result<Vec<MediaPacket>, String> {
        match self {
            Self::Sbc(frames) => packetize_sbc(frames, mtu),
            Self::Aac(frames) => packetize_aac(frames),
            Self::Opus(packets) => packetize_opus(packets),
        }
        .map_err(|error| error.to_string())
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
        _ => Err("unsupported AAC sampling frequency".into()),
    }
}

fn media_codec(capabilities: &[ServiceCapabilities]) -> Option<MediaCodecInformation> {
    capabilities.iter().find_map(|capability| {
        let ServiceCapabilities::MediaCodec {
            media_type,
            media_codec_type,
            media_codec_information,
        } = capability
        else {
            return None;
        };
        (*media_type == MediaType::AUDIO)
            .then(|| {
                MediaCodecInformation::parse(CodecType(*media_codec_type), media_codec_information)
                    .ok()
            })
            .flatten()
    })
}

fn codec_matches(audio: &AudioData, codec: &MediaCodecInformation) -> bool {
    matches!(
        (audio, codec),
        (AudioData::Sbc(_), MediaCodecInformation::Sbc(_))
            | (AudioData::Aac(_), MediaCodecInformation::Aac(_))
            | (AudioData::Opus(_), MediaCodecInformation::Opus(_))
    )
}

#[derive(Clone, Debug)]
struct RemoteEndpoint {
    info: EndpointInfo,
    capabilities: Vec<ServiceCapabilities>,
}

fn discover_remote_endpoints(
    host: &mut ExternalHost,
    device: &mut Device,
    signaling: &mut AvdtpDeviceSession,
    services: &mut AncillaryServices,
) -> Result<Vec<RemoteEndpoint>, String> {
    let endpoints = match avdtp_request(
        host,
        device,
        signaling,
        services,
        AvdtpMessage::DiscoverCommand,
    )? {
        AvdtpMessage::DiscoverResponse { endpoints } => endpoints,
        response => return Err(format!("AVDTP Discover rejected: {response:?}")),
    };
    let mut discovered = Vec::with_capacity(endpoints.len());
    for info in endpoints {
        let all_capabilities = avdtp_request(
            host,
            device,
            signaling,
            services,
            AvdtpMessage::GetAllCapabilitiesCommand {
                acp_seid: info.seid,
            },
        )?;
        let capabilities = match all_capabilities {
            AvdtpMessage::GetAllCapabilitiesResponse { capabilities } => capabilities,
            _ => match avdtp_request(
                host,
                device,
                signaling,
                services,
                AvdtpMessage::GetCapabilitiesCommand {
                    acp_seid: info.seid,
                },
            )? {
                AvdtpMessage::GetCapabilitiesResponse { capabilities } => capabilities,
                response => {
                    println!(
                        "@@@ endpoint {} capability request: {response:?}",
                        info.seid
                    );
                    Vec::new()
                }
            },
        };
        discovered.push(RemoteEndpoint { info, capabilities });
    }
    Ok(discovered)
}

fn select_sink(
    audio: &AudioData,
    endpoints: &[RemoteEndpoint],
) -> Result<(u8, MediaCodecInformation, bool), String> {
    endpoints
        .iter()
        .filter(|endpoint| {
            !endpoint.info.in_use
                && endpoint.info.media_type == MediaType::AUDIO
                && endpoint.info.endpoint_type == StreamEndpointType::SINK
        })
        .find_map(|endpoint| {
            let codec = media_codec(&endpoint.capabilities)?;
            codec_matches(audio, &codec).then(|| {
                let delay_reporting = endpoint
                    .capabilities
                    .iter()
                    .any(|capability| capability.category() == ServiceCategory::DELAY_REPORTING);
                (endpoint.info.seid, codec, delay_reporting)
            })
        })
        .ok_or_else(|| format!("no compatible {:?} sink found", audio.codec_type()))
}

fn open_avdtp_signaling(
    host: &mut ExternalHost,
    device: &mut Device,
    services: &mut AncillaryServices,
    connection_handle: u16,
) -> Result<AvdtpDeviceSession, String> {
    let version = find_a2dp_sink_service(host, device, services, connection_handle)?;
    println!("AVDTP Version: {}.{}", version.0 >> 8, version.0 & 0xFF);
    let source_cid = open_classic_channel(host, device, connection_handle, AVDTP_PSM)?;
    AvdtpDeviceSession::new(
        device,
        connection_handle,
        source_cid,
        AvdtpSession::default(),
    )
    .map_err(|error| error.to_string())
}

fn wait_until(
    host: &mut ExternalHost,
    device: &mut Device,
    signaling: &mut AvdtpDeviceSession,
    services: &mut AncillaryServices,
    target: Instant,
) -> Result<(), String> {
    while Instant::now() < target {
        device.poll(host);
        signaling
            .poll(host, device)
            .map_err(|error| error.to_string())?;
        services.poll(host, device, signaling.connection_handle())?;
        let wait = target
            .saturating_duration_since(Instant::now())
            .min(POLL_INTERVAL);
        if wait.is_zero() {
            break;
        }
        if host
            .wait_for_activity(wait)
            .map_err(|error| error.to_string())?
            == ExternalHostActivity::Ended
        {
            return Err("transport ended while streaming".into());
        }
    }
    Ok(())
}

fn wait_for_acl_output(
    host: &mut ExternalHost,
    device: &mut Device,
    signaling: &mut AvdtpDeviceSession,
    services: &mut AncillaryServices,
) -> Result<(), String> {
    let deadline = Instant::now() + PROCEDURE_TIMEOUT;
    while !device.acl_output_is_drained(signaling.connection_handle()) {
        wait_until(
            host,
            device,
            signaling,
            services,
            (Instant::now() + POLL_INTERVAL).min(deadline),
        )?;
        if Instant::now() >= deadline {
            return Err("timed out waiting for controller ACL output".into());
        }
    }
    Ok(())
}

fn stream_audio(
    host: &mut ExternalHost,
    device: &mut Device,
    services: &mut AncillaryServices,
    connection_handle: u16,
    audio: &AudioData,
) -> Result<(), String> {
    let mut signaling = open_avdtp_signaling(host, device, services, connection_handle)?;
    let endpoints = discover_remote_endpoints(host, device, &mut signaling, services)?;
    println!("@@@ Found {} endpoints", endpoints.len());
    for endpoint in &endpoints {
        println!("@@@ {:?} {:?}", endpoint.info, endpoint.capabilities);
    }
    let (sink_seid, remote_codec, delay_reporting) = select_sink(audio, &endpoints)?;
    println!("### Selected sink: {sink_seid}");
    let codec = audio.codec_information(Some(&remote_codec))?;
    println!("Source media codec: {codec:?}");
    let mut configuration = vec![ServiceCapabilities::empty(ServiceCategory::MEDIA_TRANSPORT)];
    configuration.push(
        codec
            .to_avdtp_capability()
            .map_err(|error| error.to_string())?,
    );
    if delay_reporting {
        configuration.push(ServiceCapabilities::empty(ServiceCategory::DELAY_REPORTING));
    }
    let source_seid = signaling.session_mut().add_endpoint(
        MediaType::AUDIO,
        StreamEndpointType::SOURCE,
        configuration.clone(),
    );
    match avdtp_request(
        host,
        device,
        &mut signaling,
        services,
        AvdtpMessage::SetConfigurationCommand {
            acp_seid: sink_seid,
            int_seid: source_seid,
            capabilities: configuration,
        },
    )? {
        AvdtpMessage::SetConfigurationResponse => {}
        response => return Err(format!("AVDTP SetConfiguration rejected: {response:?}")),
    }
    match avdtp_request(
        host,
        device,
        &mut signaling,
        services,
        AvdtpMessage::OpenCommand {
            acp_seid: sink_seid,
        },
    )? {
        AvdtpMessage::OpenResponse => {}
        response => return Err(format!("AVDTP Open rejected: {response:?}")),
    }
    let media_cid = open_classic_channel(host, device, connection_handle, AVDTP_PSM)?;
    let media = DeviceMediaTransport::new(device, connection_handle, media_cid)
        .map_err(|error| error.to_string())?;
    let packets = audio.packetize(usize::from(media.peer_mtu()))?;
    match avdtp_request(
        host,
        device,
        &mut signaling,
        services,
        AvdtpMessage::StartCommand {
            acp_seids: vec![sink_seid],
        },
    )? {
        AvdtpMessage::StartResponse => {}
        response => return Err(format!("AVDTP Start rejected: {response:?}")),
    }

    println!("*** Streaming {} RTP packets", packets.len());
    let started = Instant::now();
    let sampling_frequency = f64::from(audio.sampling_frequency());
    for packet in &packets {
        let target =
            started + Duration::from_secs_f64(f64::from(packet.timestamp) / sampling_frequency);
        wait_until(host, device, &mut signaling, services, target)?;
        wait_for_acl_output(host, device, &mut signaling, services)?;
        media
            .send(host, device, packet)
            .map_err(|error| error.to_string())?;
    }
    wait_for_acl_output(host, device, &mut signaling, services)?;
    match avdtp_request(
        host,
        device,
        &mut signaling,
        services,
        AvdtpMessage::CloseCommand {
            acp_seid: sink_seid,
        },
    )? {
        AvdtpMessage::CloseResponse => {}
        response => return Err(format!("AVDTP Close rejected: {response:?}")),
    }
    device
        .disconnect_classic_channel(host, connection_handle, media_cid)
        .map_err(|error| error.to_string())?;
    device
        .disconnect_classic_channel(host, connection_handle, signaling.source_cid())
        .map_err(|error| error.to_string())?;
    println!("*** Playback completed");
    Ok(())
}

fn format_eir(data: &[u8]) -> Vec<String> {
    AdvertisingData::from_bytes(data)
        .ad_structures
        .into_iter()
        .map(|(kind, value)| {
            if matches!(
                kind,
                AdvertisingDataType::COMPLETE_LOCAL_NAME
                    | AdvertisingDataType::SHORTENED_LOCAL_NAME
            ) {
                format!("type {:#04x}: {}", kind.0, String::from_utf8_lossy(&value))
            } else {
                let bytes = value
                    .iter()
                    .map(|byte| format!("{byte:02X}"))
                    .collect::<Vec<_>>()
                    .join(" ");
                format!("type {:#04x}: {bytes}", kind.0)
            }
        })
        .collect()
}

fn run_discover(host: &mut ExternalHost, device: &mut Device) -> Result<(), String> {
    set_scan_enabled(host, true)?;
    loop {
        device.take_classic_inquiry_result_details();
        device.take_classic_inquiry_complete();
        command(
            host,
            Command::Inquiry {
                lap: 0x9E8B33,
                inquiry_length: 8,
                num_responses: 0,
            },
            "starting Classic inquiry",
        )?;
        loop {
            device.poll(host);
            for result in device.take_classic_inquiry_result_details() {
                let class = ClassOfDevice::from_int(result.class_of_device);
                println!(">>> {}:", result.peer_address.to_string(false));
                println!("  Device Class (raw): {:06X}", result.class_of_device);
                println!("  Device Class: {class}");
                println!(
                    "  Device Services: {}",
                    class.major_service_classes().composite_name()
                );
                if let Some(rssi) = result.rssi {
                    println!("  RSSI: {rssi}");
                }
                for line in format_eir(&result.extended_inquiry_response) {
                    println!("  {line}");
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
            match host
                .wait_for_activity(Duration::from_secs(2))
                .map_err(|error| error.to_string())?
            {
                ExternalHostActivity::Packet | ExternalHostActivity::Timeout => {}
                ExternalHostActivity::Ended => return Ok(()),
            }
        }
    }
}

fn run_pair(
    host: &mut ExternalHost,
    device: &mut Device,
    address: &str,
    store: &mut JsonKeyStore,
) -> Result<(), String> {
    let peer =
        Address::parse(address, AddressType::PUBLIC_DEVICE).map_err(|error| error.to_string())?;
    println!("Connecting to {peer}...");
    let connection_handle = connect_classic(host, device, peer)?;
    println!("Pairing...");
    authenticate_classic(host, device, connection_handle, store)?;
    println!("Pairing completed");
    Ok(())
}

fn run_inquire(
    host: &mut ExternalHost,
    device: &mut Device,
    services: &mut AncillaryServices,
    address: &str,
    authenticate: bool,
    encrypt: bool,
    store: &mut JsonKeyStore,
) -> Result<(), String> {
    let peer =
        Address::parse(address, AddressType::PUBLIC_DEVICE).map_err(|error| error.to_string())?;
    println!("Connecting to {peer}...");
    let connection_handle = connect_classic(host, device, peer)?;
    secure_connection(
        host,
        device,
        connection_handle,
        authenticate,
        encrypt,
        store,
    )?;
    let mut signaling = open_avdtp_signaling(host, device, services, connection_handle)?;
    let endpoints = discover_remote_endpoints(host, device, &mut signaling, services)?;
    println!("@@@ Found {} endpoints", endpoints.len());
    for endpoint in endpoints {
        println!("@@@ {:?} {:?}", endpoint.info, endpoint.capabilities);
    }
    Ok(())
}

#[derive(Clone, Copy)]
struct SecurityOptions {
    authenticate: bool,
    encrypt: bool,
}

struct PlayRequest<'a> {
    address: Option<&'a str>,
    audio_format: AudioFormat,
    audio_file: &'a Path,
}

fn run_play(
    host: &mut ExternalHost,
    device: &mut Device,
    services: &mut AncillaryServices,
    request: PlayRequest<'_>,
    security: SecurityOptions,
    store: &mut JsonKeyStore,
) -> Result<(), String> {
    let audio = AudioData::load(request.audio_file, request.audio_format)?;
    println!("{}", audio.describe());
    let connection_handle = if let Some(address) = request.address {
        let peer = resolve_classic_peer(host, device, address)?;
        println!("Connecting to {peer}...");
        let handle = connect_classic(host, device, peer)?;
        secure_connection(
            host,
            device,
            handle,
            security.authenticate,
            security.encrypt,
            store,
        )?;
        handle
    } else {
        println!("Waiting for an incoming connection...");
        set_scan_enabled(host, true)?;
        let handle = wait_for_classic_connection(host, device, None, true)?;
        set_scan_enabled(host, false)?;
        handle
    };
    println!("--- Connected on handle {connection_handle:#06x}");
    stream_audio(host, device, services, connection_handle, &audio)
}

fn run(args: Args) -> Result<(), String> {
    let config = load_device_config(args.device_config.as_deref())?;
    println!("<<< connecting to HCI...");
    let transport = open_split_transport(&args.hci_transport).map_err(|error| error.to_string())?;
    let mut host = ExternalHost::new(transport);
    let mut device = Device::new(0);
    host.initialize_device(&mut device, COMMAND_TIMEOUT)
        .map_err(|error| error.to_string())?;
    configure_identity(&mut host, &config)?;
    let public_address = read_public_address(&mut host)?;
    println!(
        "Player Bluetooth Address: {}",
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
            Some(u32::from(AVCTP_PSM)),
            ClassicChannelSpec {
                mtu: CLASSIC_L2CAP_MTU,
            },
        )
        .map_err(|error| error.to_string())?;
    let namespace = public_address.to_string(false);
    let mut store = JsonKeyStore::with_default_path(Some(&namespace));
    let mut services = AncillaryServices::default();
    println!("<<< connected");
    match args.command {
        PlayerCommand::Discover => run_discover(&mut host, &mut device),
        PlayerCommand::Pair { address } => run_pair(&mut host, &mut device, &address, &mut store),
        PlayerCommand::Inquire { address } => run_inquire(
            &mut host,
            &mut device,
            &mut services,
            &address,
            args.authenticate,
            args.encrypt,
            &mut store,
        ),
        PlayerCommand::Play {
            address,
            audio_format,
            audio_file,
        } => run_play(
            &mut host,
            &mut device,
            &mut services,
            PlayRequest {
                address: address.as_deref(),
                audio_format,
                audio_file: &audio_file,
            },
            SecurityOptions {
                authenticate: args.authenticate,
                encrypt: args.encrypt,
            },
            &mut store,
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

    #[test]
    fn parses_upstream_cli_shapes_and_encrypt_default() {
        let discover =
            parse_args(["player", "--hci-transport", "usb:0", "discover"].map(str::to_string))
                .unwrap();
        assert_eq!(discover.command, PlayerCommand::Discover);
        assert!(discover.encrypt);
        assert!(!discover.authenticate);

        let inquire = parse_args(
            [
                "player",
                "--hci-transport=tcp-client:localhost:1234",
                "--device-config",
                "device.json",
                "--authenticate",
                "inquire",
                "C4:F2:17:1A:1D:BB",
            ]
            .map(str::to_string),
        )
        .unwrap();
        assert!(inquire.authenticate);
        assert_eq!(inquire.device_config, Some(PathBuf::from("device.json")));
        assert!(matches!(inquire.command, PlayerCommand::Inquire { .. }));

        let pair = parse_args(
            [
                "player",
                "--hci-transport",
                "usb:0",
                "pair",
                "C4:F2:17:1A:1D:BB",
            ]
            .map(str::to_string),
        )
        .unwrap();
        assert!(matches!(pair.command, PlayerCommand::Pair { .. }));

        let play = parse_args(
            [
                "player",
                "--hci-transport",
                "usb:0",
                "play",
                "--connect",
                "C4:F2:17:1A:1D:BB",
                "-f",
                "opus",
                "music.ogg",
            ]
            .map(str::to_string),
        )
        .unwrap();
        assert_eq!(
            play.command,
            PlayerCommand::Play {
                address: Some("C4:F2:17:1A:1D:BB".into()),
                audio_format: AudioFormat::Opus,
                audio_file: PathBuf::from("music.ogg"),
            }
        );
    }

    #[test]
    fn infers_supported_audio_extensions() {
        assert_eq!(
            AudioFormat::infer(Path::new("track.SBC")).unwrap(),
            AudioFormat::Sbc
        );
        assert_eq!(
            AudioFormat::infer(Path::new("track.adts")).unwrap(),
            AudioFormat::Aac
        );
        assert_eq!(
            AudioFormat::infer(Path::new("track.ogg")).unwrap(),
            AudioFormat::Opus
        );
        assert!(AudioFormat::infer(Path::new("track.wav")).is_err());
    }

    #[test]
    fn derives_sbc_configuration_and_uses_sink_bitpool_range() {
        let frames = SbcFrame::parse_stream(&[0x9C, 0x80, 0x08, 0, 0, 0, 0, 0, 0, 0]).unwrap();
        let audio = AudioData::Sbc(frames);
        let remote = MediaCodecInformation::Sbc(SbcMediaCodecInformation {
            sampling_frequency: SbcSamplingFrequency::SF_44100,
            channel_mode: SbcChannelMode::MONO,
            block_length: SbcBlockLength::BL_4,
            subbands: SbcSubbands::S_4,
            allocation_method: SbcAllocationMethod::LOUDNESS,
            minimum_bitpool_value: 5,
            maximum_bitpool_value: 53,
        });
        let MediaCodecInformation::Sbc(config) = audio.codec_information(Some(&remote)).unwrap()
        else {
            panic!("expected SBC codec information");
        };
        assert_eq!(config.sampling_frequency, SbcSamplingFrequency::SF_44100);
        assert_eq!(config.channel_mode, SbcChannelMode::MONO);
        assert_eq!(config.minimum_bitpool_value, 5);
        assert_eq!(config.maximum_bitpool_value, 53);
    }

    #[test]
    fn parses_numeric_and_hex_device_class_values() {
        assert_eq!(
            parse_class_of_device(&serde_json::json!(0x240400)),
            Some(0x240400)
        );
        assert_eq!(
            parse_class_of_device(&serde_json::json!("0x240400")),
            Some(0x240400)
        );
    }
}
