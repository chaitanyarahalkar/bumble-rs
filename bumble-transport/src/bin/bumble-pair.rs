use bumble::advertising_data::Type as AdvertisingDataType;
use bumble::keys::{JsonKeyStore, KeyStore, PairingKeys};
use bumble::{Address, AddressType, AdvertisingData, Uuid};
use bumble_hci::metadata::supported_command_names;
use bumble_hci::{Command, ReturnParameters};
use bumble_host::Device;
use bumble_smp::{
    IdentityAddressType, IoCapability, OobConfig, OobContext, OobData, OobLegacyContext,
    PairingCapabilities, PairingConfig, PairingDelegate,
};
use bumble_transport::{
    open_split_transport, ClassicCtkdPairingSession, ClassicPairingSession, CommandResponse,
    ExternalHost, ExternalHostActivity, LePairingSession,
};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Duration, Instant};

const DEFAULT_ADDRESS: &str = "F0:F1:F2:F3:F4:F5";
const COMMAND_TIMEOUT: Duration = Duration::from_secs(5);
const PAIRING_TIMEOUT: Duration = Duration::from_secs(120);
const CTKD_TIMEOUT: Duration = Duration::from_secs(10);
const PROCEDURE_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Le,
    Classic,
    Dual,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ConnectionKind {
    Le(u16),
    Classic(u16),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum IoMode {
    Keyboard,
    Display,
    DisplayKeyboard,
    DisplayYesNo,
    None,
}

impl IoMode {
    fn capability(self) -> IoCapability {
        match self {
            Self::Keyboard => IoCapability::KeyboardOnly,
            Self::Display => IoCapability::DisplayOnly,
            Self::DisplayKeyboard => IoCapability::KeyboardDisplay,
            Self::DisplayYesNo => IoCapability::DisplayYesNo,
            Self::None => IoCapability::NoInputNoOutput,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Args {
    mode: Mode,
    secure_connections: bool,
    mitm: bool,
    bond: bool,
    ctkd: bool,
    advertising_address: Option<String>,
    identity_address: Option<String>,
    linger: bool,
    io: IoMode,
    oob: Option<String>,
    prompt: bool,
    request: bool,
    print_keys: bool,
    keystore_file: Option<PathBuf>,
    advertise_service_uuids: Vec<String>,
    advertise_appearance: Option<String>,
    device_config: PathBuf,
    transport: String,
    address_or_name: Option<String>,
}

#[derive(Clone, Debug)]
struct DeviceConfig {
    name: String,
    address: Address,
    json_keystore: bool,
}

fn usage() -> &'static str {
    "usage: bumble-pair [--mode le|classic|dual] [--sc BOOL] [--mitm BOOL] [--bond BOOL] [--ctkd BOOL] [--advertising-address random|public] [--identity-address random|public] [--linger] [--io keyboard|display|display+keyboard|display+yes/no|none] [--oob HEX|-] [--prompt] [--request] [--print-keys] [--keystore-file PATH] [--advertise-service-uuid UUID] [--advertise-appearance APPEARANCE] <device-config> <transport> [address-or-name]"
}

fn option_value(
    argument: &str,
    option: &str,
    arguments: &mut impl Iterator<Item = String>,
) -> Result<Option<String>, String> {
    if argument == option {
        return arguments
            .next()
            .map(Some)
            .ok_or_else(|| format!("missing value for {option}"));
    }
    Ok(argument
        .strip_prefix(&format!("{option}="))
        .map(ToOwned::to_owned))
}

fn parse_bool(value: &str, option: &str) -> Result<bool, String> {
    match value.to_ascii_lowercase().as_str() {
        "true" | "1" | "yes" | "on" => Ok(true),
        "false" | "0" | "no" | "off" => Ok(false),
        _ => Err(format!("{option} must be true or false")),
    }
}

fn parse_choice(value: String, option: &str, choices: &[&str]) -> Result<String, String> {
    if choices.contains(&value.as_str()) {
        Ok(value)
    } else {
        Err(format!("{option} must be one of {}", choices.join(", ")))
    }
}

fn parse_args(arguments: impl IntoIterator<Item = String>) -> Result<Args, String> {
    let mut arguments = arguments.into_iter();
    let _program = arguments.next();
    let mut args = Args {
        mode: Mode::Le,
        secure_connections: true,
        mitm: true,
        bond: true,
        ctkd: true,
        advertising_address: None,
        identity_address: None,
        linger: false,
        io: IoMode::DisplayKeyboard,
        oob: None,
        prompt: false,
        request: false,
        print_keys: false,
        keystore_file: None,
        advertise_service_uuids: Vec::new(),
        advertise_appearance: None,
        device_config: PathBuf::new(),
        transport: String::new(),
        address_or_name: None,
    };
    let mut positional = Vec::new();
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "-h" | "--help" => return Err(usage().into()),
            "--linger" => {
                args.linger = true;
                continue;
            }
            "--prompt" => {
                args.prompt = true;
                continue;
            }
            "--request" => {
                args.request = true;
                continue;
            }
            "--print-keys" => {
                args.print_keys = true;
                continue;
            }
            _ => {}
        }
        if let Some(value) = option_value(&argument, "--mode", &mut arguments)? {
            args.mode = match value.as_str() {
                "le" => Mode::Le,
                "classic" => Mode::Classic,
                "dual" => Mode::Dual,
                _ => return Err("--mode must be le, classic, or dual".into()),
            };
            continue;
        }
        if let Some(value) = option_value(&argument, "--sc", &mut arguments)? {
            args.secure_connections = parse_bool(&value, "--sc")?;
            continue;
        }
        if let Some(value) = option_value(&argument, "--mitm", &mut arguments)? {
            args.mitm = parse_bool(&value, "--mitm")?;
            continue;
        }
        if let Some(value) = option_value(&argument, "--bond", &mut arguments)? {
            args.bond = parse_bool(&value, "--bond")?;
            continue;
        }
        if let Some(value) = option_value(&argument, "--ctkd", &mut arguments)? {
            args.ctkd = parse_bool(&value, "--ctkd")?;
            continue;
        }
        if let Some(value) = option_value(&argument, "--advertising-address", &mut arguments)? {
            args.advertising_address = Some(parse_choice(
                value,
                "--advertising-address",
                &["random", "public"],
            )?);
            continue;
        }
        if let Some(value) = option_value(&argument, "--identity-address", &mut arguments)? {
            args.identity_address = Some(parse_choice(
                value,
                "--identity-address",
                &["random", "public"],
            )?);
            continue;
        }
        if let Some(value) = option_value(&argument, "--io", &mut arguments)? {
            args.io = match value.as_str() {
                "keyboard" => IoMode::Keyboard,
                "display" => IoMode::Display,
                "display+keyboard" => IoMode::DisplayKeyboard,
                "display+yes/no" => IoMode::DisplayYesNo,
                "none" => IoMode::None,
                _ => return Err("invalid --io capability".into()),
            };
            continue;
        }
        if let Some(value) = option_value(&argument, "--oob", &mut arguments)? {
            args.oob = Some(value);
            continue;
        }
        if let Some(value) = option_value(&argument, "--keystore-file", &mut arguments)? {
            args.keystore_file = Some(PathBuf::from(value));
            continue;
        }
        if let Some(value) = option_value(&argument, "--advertise-service-uuid", &mut arguments)? {
            args.advertise_service_uuids.push(value);
            continue;
        }
        if let Some(value) = option_value(&argument, "--advertise-appearance", &mut arguments)? {
            args.advertise_appearance = Some(value);
            continue;
        }
        if argument.starts_with('-') {
            return Err(format!("unknown option {argument:?}"));
        }
        positional.push(argument);
    }
    if !(2..=3).contains(&positional.len()) {
        return Err(usage().into());
    }
    args.device_config = PathBuf::from(positional.remove(0));
    args.transport = positional.remove(0);
    args.address_or_name = positional.pop();
    Ok(args)
}

fn load_device_config(path: &Path) -> Result<DeviceConfig, String> {
    let bytes = std::fs::read(path)
        .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
    let config: serde_json::Value = serde_json::from_slice(&bytes)
        .map_err(|error| format!("invalid device config: {error}"))?;
    let address = config
        .get("address")
        .and_then(serde_json::Value::as_str)
        .unwrap_or(DEFAULT_ADDRESS);
    Ok(DeviceConfig {
        name: config
            .get("name")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("Bumble")
            .to_owned(),
        address: Address::parse(address, AddressType::RANDOM_DEVICE)
            .map_err(|error| error.to_string())?,
        json_keystore: config.get("keystore").and_then(serde_json::Value::as_str)
            == Some("JsonKeyStore"),
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

fn report_name(data: &[u8]) -> Option<String> {
    let advertising_data = AdvertisingData::from_bytes(data);
    advertising_data
        .get(AdvertisingDataType::COMPLETE_LOCAL_NAME)
        .or_else(|| advertising_data.get(AdvertisingDataType::SHORTENED_LOCAL_NAME))
        .map(|name| String::from_utf8_lossy(&name).into_owned())
}

fn wait_for_connection(
    host: &mut ExternalHost,
    device: &mut Device,
    peer: Option<&Address>,
) -> Result<u16, String> {
    let deadline = Instant::now() + PROCEDURE_TIMEOUT;
    loop {
        device.poll(host);
        let handle = peer
            .and_then(|peer| device.connection_handle_for_peer(peer))
            .or_else(|| peer.is_none().then(|| device.connection_handle()).flatten());
        if let Some(handle) = handle {
            return Ok(handle);
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err("timed out waiting for connection".into());
        }
        match host
            .wait_for_activity(remaining)
            .map_err(|error| error.to_string())?
        {
            ExternalHostActivity::Packet => {}
            ExternalHostActivity::Timeout => return Err("timed out waiting for connection".into()),
            ExternalHostActivity::Ended => {
                return Err("HCI transport ended while waiting for connection".into())
            }
        }
    }
}

fn resolve_name(
    host: &mut ExternalHost,
    device: &mut Device,
    wanted_name: &str,
    own_address_type: u8,
) -> Result<Address, String> {
    command(
        host,
        Command::LeSetScanParameters {
            le_scan_type: 1,
            le_scan_interval: 0x0010,
            le_scan_window: 0x0010,
            own_address_type,
            scanning_filter_policy: 0,
        },
        "setting scan parameters",
    )?;
    command(
        host,
        Command::LeSetScanEnable {
            le_scan_enable: 1,
            filter_duplicates: 0,
        },
        "enabling scan",
    )?;
    let deadline = Instant::now() + PROCEDURE_TIMEOUT;
    let result = loop {
        device.poll(host);
        let legacy = device
            .take_advertising_reports()
            .into_iter()
            .find(|report| report_name(&report.data).as_deref() == Some(wanted_name))
            .map(|report| report.address);
        let extended = device
            .take_extended_advertising_reports()
            .into_iter()
            .find(|report| report_name(&report.data).as_deref() == Some(wanted_name))
            .map(|report| report.address);
        if let Some(address) = legacy.or(extended) {
            break Ok(address);
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            break Err(format!("timed out resolving peer name {wanted_name:?}"));
        }
        match host
            .wait_for_activity(remaining)
            .map_err(|error| error.to_string())?
        {
            ExternalHostActivity::Packet => {}
            ExternalHostActivity::Timeout => {
                break Err(format!("timed out resolving peer name {wanted_name:?}"))
            }
            ExternalHostActivity::Ended => break Err("HCI transport ended while scanning".into()),
        }
    };
    let disabled = command(
        host,
        Command::LeSetScanEnable {
            le_scan_enable: 0,
            filter_duplicates: 0,
        },
        "disabling scan",
    );
    match (result, disabled) {
        (Err(error), _) => Err(error),
        (Ok(_), Err(error)) => Err(error),
        (Ok(address), Ok(_)) => Ok(address),
    }
}

fn connect(
    host: &mut ExternalHost,
    device: &mut Device,
    peer: Address,
    own_address_type: u8,
) -> Result<u16, String> {
    command(
        host,
        Command::LeCreateConnection {
            le_scan_interval: 0x0010,
            le_scan_window: 0x0010,
            initiator_filter_policy: 0,
            peer_address_type: u8::from(!peer.is_public()),
            peer_address: peer.clone(),
            own_address_type,
            connection_interval_min: 24,
            connection_interval_max: 40,
            max_latency: 0,
            supervision_timeout: 42,
            min_ce_length: 0,
            max_ce_length: 0,
        },
        "creating LE connection",
    )?;
    wait_for_connection(host, device, Some(&peer))
}

fn add_ad_structure(data: &mut Vec<u8>, data_type: u8, value: &[u8]) -> Result<(), String> {
    let length = u8::try_from(value.len() + 1).map_err(|_| "advertising value is too long")?;
    data.push(length);
    data.push(data_type);
    data.extend_from_slice(value);
    Ok(())
}

fn advertising_data(args: &Args, name: &str) -> Result<Vec<u8>, String> {
    let mut data = Vec::new();
    add_ad_structure(&mut data, 0x01, &[0x05])?;
    let service_uuids = if args.advertise_service_uuids.is_empty() {
        vec![Uuid::from_16_bits(0x180D)]
    } else {
        args.advertise_service_uuids
            .iter()
            .map(|uuid| Uuid::parse(uuid).map_err(|error| error.to_string()))
            .collect::<Result<Vec<_>, _>>()?
    };
    for (length, data_type) in [(2, 0x02), (4, 0x04), (16, 0x06)] {
        let values = service_uuids
            .iter()
            .map(|uuid| uuid.to_bytes(false))
            .filter(|value| value.len() == length)
            .flatten()
            .collect::<Vec<_>>();
        if !values.is_empty() {
            add_ad_structure(&mut data, data_type, &values)?;
        }
    }
    if let Some(appearance) = &args.advertise_appearance {
        let value = appearance
            .parse::<u16>()
            .map_err(|_| "advertise appearance must currently be a numeric ID".to_string())?;
        add_ad_structure(&mut data, 0x19, &value.to_le_bytes())?;
    }
    let remaining = 31usize.saturating_sub(data.len() + 2);
    let name = name.as_bytes();
    let shown_name = &name[..name.len().min(remaining)];
    add_ad_structure(
        &mut data,
        if shown_name.len() == name.len() {
            0x09
        } else {
            0x08
        },
        shown_name,
    )?;
    if data.len() > 31 {
        return Err("advertising data exceeds the 31-byte legacy limit".into());
    }
    Ok(data)
}

fn start_le_advertising(
    host: &mut ExternalHost,
    own_address_type: u8,
    data: Vec<u8>,
) -> Result<(), String> {
    command(
        host,
        Command::LeSetAdvertisingParameters {
            advertising_interval_min: 0x0800,
            advertising_interval_max: 0x0800,
            advertising_type: 0,
            own_address_type,
            peer_address_type: 0,
            peer_address: Address::from_bytes([0; 6], AddressType::PUBLIC_DEVICE),
            advertising_channel_map: 7,
            advertising_filter_policy: 0,
        },
        "setting advertising parameters",
    )?;
    command(
        host,
        Command::LeSetAdvertisingData {
            advertising_data: data,
        },
        "setting advertising data",
    )?;
    command(
        host,
        Command::LeSetAdvertisingEnable {
            advertising_enable: 1,
        },
        "enabling advertising",
    )?;
    Ok(())
}

fn wait_for_classic_connection(
    host: &mut ExternalHost,
    device: &mut Device,
    peer: Option<&Address>,
) -> Result<u16, String> {
    let deadline = Instant::now() + PROCEDURE_TIMEOUT;
    loop {
        device.poll(host);
        let handle = match peer {
            Some(peer) => device.classic_connection_handle_for_peer(peer),
            None => device.classic_connection_handle(),
        };
        if let Some(handle) = handle {
            return Ok(handle);
        }
        for request in device.take_classic_connection_requests() {
            if peer.is_none_or(|peer| *peer == request) {
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

fn enable_classic_encryption(
    host: &mut ExternalHost,
    device: &mut Device,
    connection_handle: u16,
) -> Result<(), String> {
    if device.is_classic_encrypted_on_handle(connection_handle) {
        return Ok(());
    }
    if !device.set_classic_encryption_on_handle(host, connection_handle, true) {
        return Err(format!(
            "failed to request encryption on Classic handle {connection_handle:#06x}"
        ));
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

fn run_classic_ctkd(
    host: &mut ExternalHost,
    device: &mut Device,
    mut session: ClassicCtkdPairingSession,
) -> Result<(ClassicCtkdPairingSession, PairingKeys), String> {
    let keys = session
        .run_to_completion(host, device, CTKD_TIMEOUT)
        .map_err(|error| error.to_string())?;
    Ok((session, keys))
}

fn start_classic_listening(host: &mut ExternalHost, name: &str) -> Result<(), String> {
    let mut inquiry_response = [0; 240];
    let mut name_data = Vec::new();
    add_ad_structure(&mut name_data, 0x09, name.as_bytes())?;
    if name_data.len() > inquiry_response.len() {
        return Err("local name is too long for an inquiry response".into());
    }
    inquiry_response[..name_data.len()].copy_from_slice(&name_data);
    command(
        host,
        Command::WriteExtendedInquiryResponse {
            fec_required: 0,
            extended_inquiry_response: inquiry_response,
        },
        "writing extended inquiry response",
    )?;
    command(
        host,
        Command::WriteScanEnable { scan_enable: 0x03 },
        "enabling inquiry and page scans",
    )?;
    Ok(())
}

fn wait_for_incoming(
    host: &mut ExternalHost,
    device: &mut Device,
    mode: Mode,
) -> Result<ConnectionKind, String> {
    let deadline = Instant::now() + PROCEDURE_TIMEOUT;
    loop {
        device.poll(host);
        if mode != Mode::Classic {
            if let Some(handle) = device.connection_handle() {
                return Ok(ConnectionKind::Le(handle));
            }
        }
        if mode != Mode::Le {
            if let Some(handle) = device.classic_connection_handle() {
                return Ok(ConnectionKind::Classic(handle));
            }
            for request in device.take_classic_connection_requests() {
                device.accept_classic(host, request);
            }
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err("timed out waiting for an incoming connection".into());
        }
        match host
            .wait_for_activity(remaining)
            .map_err(|error| error.to_string())?
        {
            ExternalHostActivity::Packet => {}
            ExternalHostActivity::Timeout => {
                return Err("timed out waiting for an incoming connection".into())
            }
            ExternalHostActivity::Ended => {
                return Err("HCI transport ended while waiting for a connection".into())
            }
        }
    }
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
        Command::Inquiry {
            lap: 0x9E8B33,
            inquiry_length: 8,
            num_responses: 0,
        },
        "starting Classic inquiry",
    )?;
    let deadline = Instant::now() + PROCEDURE_TIMEOUT;
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
        let remaining = deadline.saturating_duration_since(Instant::now());
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
                return Err(format!(
                    "timed out resolving Classic peer name {wanted_name:?}"
                ));
            }
            match host
                .wait_for_activity(remaining)
                .map_err(|error| error.to_string())?
            {
                ExternalHostActivity::Packet => {}
                ExternalHostActivity::Timeout | ExternalHostActivity::Ended => {
                    return Err(format!(
                        "timed out resolving Classic peer name {wanted_name:?}"
                    ))
                }
            }
        }
    }
    Err(format!("Classic peer name {wanted_name:?} was not found"))
}

fn decode_hex(value: &str) -> Result<Vec<u8>, String> {
    let compact: String = value
        .chars()
        .filter(|character| *character != ':')
        .collect();
    if !compact.len().is_multiple_of(2) {
        return Err("hex input must contain complete bytes".into());
    }
    (0..compact.len())
        .step_by(2)
        .map(|index| {
            u8::from_str_radix(&compact[index..index + 2], 16)
                .map_err(|_| "invalid hex input".to_string())
        })
        .collect()
}

fn pairing_config(args: &Args, local_address: &Address) -> Result<PairingConfig, String> {
    let oob = match args.oob.as_deref() {
        None => None,
        Some(value) => {
            let our_context = OobContext::new(None, None);
            let mut legacy_context =
                (!args.secure_connections).then(|| OobLegacyContext::new(None));
            let peer_data = if value == "-" {
                None
            } else {
                let data = OobData::from_ad(&AdvertisingData::from_bytes(&decode_hex(value)?));
                if !args.secure_connections {
                    legacy_context = data.legacy_context;
                    if legacy_context.is_none() {
                        return Err("OOB pairing in legacy mode requires TK".into());
                    }
                }
                data.shared_data
            };
            let share = OobData {
                address: Some(local_address.clone()),
                role: None,
                shared_data: Some(our_context.share()),
                legacy_context: if args.secure_connections {
                    None
                } else {
                    legacy_context.clone()
                },
            };
            println!("@@@ OOB SHARE: {}", hex(&share.to_ad().to_bytes()));
            if let Some(legacy) = &legacy_context {
                println!("@@@ OOB TK: {}", hex(&legacy.tk));
            }
            Some(OobConfig {
                our_context: Some(our_context),
                peer_data,
                legacy_context,
            })
        }
    };
    Ok(PairingConfig {
        secure_connections: args.secure_connections,
        ct2: args.ctkd,
        mitm: args.mitm,
        bonding: args.bond,
        capabilities: PairingCapabilities {
            io_capability: args.io.capability(),
            ..PairingCapabilities::default()
        },
        identity_address_type: match args.identity_address.as_deref() {
            Some("public") => Some(IdentityAddressType::Public),
            Some("random") => Some(IdentityAddressType::Random),
            _ => None,
        },
        oob,
    })
}

#[derive(Clone)]
struct CliDelegate {
    prompt_for_acceptance: bool,
}

impl CliDelegate {
    fn prompt_raw(&self, message: &str) -> String {
        print!("{message}");
        let _ = std::io::stdout().flush();
        let mut answer = String::new();
        std::io::stdin().read_line(&mut answer).ok();
        answer.trim().to_owned()
    }

    fn prompt(&self, message: &str) -> String {
        self.prompt_raw(message).to_ascii_lowercase()
    }

    fn yes_no(&self, message: &str) -> bool {
        loop {
            match self.prompt(message).as_str() {
                "yes" | "y" => return true,
                "no" | "n" => return false,
                _ => println!("please answer yes or no"),
            }
        }
    }
}

impl PairingDelegate for CliDelegate {
    fn accept(&mut self) -> bool {
        !self.prompt_for_acceptance || self.yes_no(">>> Accept pairing request? ")
    }

    fn confirm(&mut self, auto: bool) -> bool {
        auto || self.yes_no(">>> Confirm pairing? ")
    }

    fn compare_numbers(&mut self, number: u32, digits: u8) -> bool {
        self.yes_no(&format!(
            ">>> Does the other device display {number:0width$}? ",
            width = usize::from(digits)
        ))
    }

    fn get_number(&mut self) -> Option<u32> {
        loop {
            let answer = self.prompt(">>> Enter PIN: ");
            if answer.is_empty() {
                return None;
            }
            if let Ok(number) = answer.parse() {
                return Some(number);
            }
        }
    }

    fn get_string(&mut self, max_length: usize) -> Option<String> {
        loop {
            let answer = self.prompt_raw(">>> Enter PIN (1-16 bytes): ");
            if answer.is_empty() {
                return None;
            }
            if answer.len() <= max_length {
                return Some(answer);
            }
            println!("PIN must be at most {max_length} bytes");
        }
    }

    fn display_number(&mut self, number: u32, digits: u8) {
        println!("### PIN: {number:0width$}", width = usize::from(digits));
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn print_pairing_keys(prefix: &str, keys: &PairingKeys) -> Result<(), String> {
    for line in keys.to_json().map_err(|error| error.to_string())?.lines() {
        println!("{prefix}{line}");
    }
    Ok(())
}

fn print_store(store: &dyn KeyStore) -> Result<(), String> {
    for (name, keys) in store.get_all().map_err(|error| error.to_string())? {
        println!("@@@ {name}");
        print_pairing_keys("@@@ ", &keys)?;
    }
    Ok(())
}

fn run(args: Args) -> Result<(), String> {
    let config = load_device_config(&args.device_config)?;
    println!("<<< connecting to HCI...");
    let transport = open_split_transport(&args.transport).map_err(|error| error.to_string())?;
    let mut host = ExternalHost::new(transport);
    let mut device = Device::new(0);
    let controller_info = host
        .initialize_device(&mut device, COMMAND_TIMEOUT)
        .map_err(|error| error.to_string())?;
    println!("<<< connected");

    let supported = supported_command_names(&controller_info.supported_commands);
    let classic_enabled = args.mode != Mode::Le;
    let le_enabled = args.mode != Mode::Classic;
    if classic_enabled {
        if !supported.contains(&"HCI_WRITE_SIMPLE_PAIRING_MODE_COMMAND") {
            return Err("controller does not support Classic Secure Simple Pairing".into());
        }
        command(
            &mut host,
            Command::WriteSimplePairingMode {
                simple_pairing_mode: 1,
            },
            "enabling Secure Simple Pairing",
        )?;
        if args.secure_connections
            && supported.contains(&"HCI_WRITE_SECURE_CONNECTIONS_HOST_SUPPORT_COMMAND")
        {
            command(
                &mut host,
                Command::WriteSecureConnectionsHostSupport {
                    secure_connections_host_support: 1,
                },
                "enabling Classic Secure Connections host support",
            )?;
        }
        let mut local_name = [0; 248];
        let name = config.name.as_bytes();
        let length = name.len().min(local_name.len());
        local_name[..length].copy_from_slice(&name[..length]);
        command(
            &mut host,
            Command::WriteLocalName { local_name },
            "writing Classic local name",
        )?;
    }

    let public_address = if classic_enabled
        || (args.address_or_name.is_none() && args.advertising_address.as_deref() == Some("public"))
    {
        let response = command(&mut host, Command::ReadBdAddr, "reading public address")?;
        Some(match response.return_parameters() {
            Some(ReturnParameters::ReadBdAddr { bd_addr, .. }) => bd_addr.clone(),
            other => return Err(format!("unexpected Read BD_ADDR response: {other:?}")),
        })
    } else {
        None
    };
    let le_address = if args.address_or_name.is_none()
        && args.advertising_address.as_deref() == Some("public")
    {
        public_address
            .clone()
            .ok_or_else(|| "controller public address was not read".to_string())?
    } else {
        config.address
    };
    let own_address_type = u8::from(!le_address.is_public());
    if le_enabled && own_address_type != 0 {
        command(
            &mut host,
            Command::LeSetRandomAddress {
                random_address: le_address.clone(),
            },
            "setting random address",
        )?;
    }

    let namespace = public_address
        .as_ref()
        .unwrap_or(&le_address)
        .to_string(false);
    let mut store = args
        .keystore_file
        .as_ref()
        .map(|path| JsonKeyStore::new(Some(&namespace), path))
        .or_else(|| {
            config
                .json_keystore
                .then(|| JsonKeyStore::with_default_path(Some(&namespace)))
        });
    if args.print_keys {
        if let Some(store) = store.as_ref() {
            println!("@@@ Pairing Keys:");
            print_store(store)?;
        }
    }

    let (connection, outgoing) = if let Some(target) = args.address_or_name.as_deref() {
        if args.mode == Mode::Le {
            let peer = match Address::parse(target, AddressType::RANDOM_DEVICE) {
                Ok(address) => address,
                Err(_) => resolve_name(&mut host, &mut device, target, own_address_type)?,
            };
            println!("=== Connecting to {peer} over LE...");
            (
                ConnectionKind::Le(connect(&mut host, &mut device, peer, own_address_type)?),
                true,
            )
        } else {
            let peer = match Address::parse(target, AddressType::PUBLIC_DEVICE) {
                Ok(address) => address,
                Err(_) => resolve_classic_name(&mut host, &mut device, target)?,
            };
            println!("=== Connecting to {peer} over Classic...");
            device.connect_classic(&mut host, peer.clone());
            (
                ConnectionKind::Classic(wait_for_classic_connection(
                    &mut host,
                    &mut device,
                    Some(&peer),
                )?),
                true,
            )
        }
    } else {
        if le_enabled {
            start_le_advertising(
                &mut host,
                own_address_type,
                advertising_data(&args, &config.name)?,
            )?;
            println!("Ready for LE connections on {le_address}");
        }
        if classic_enabled {
            start_classic_listening(&mut host, &config.name)?;
            println!(
                "Ready for Classic connections on {}",
                public_address
                    .as_ref()
                    .expect("Classic mode reads the public address")
            );
        }
        (wait_for_incoming(&mut host, &mut device, args.mode)?, false)
    };

    let connected = match connection {
        ConnectionKind::Le(handle) => {
            let peer = device
                .le_connection(handle)
                .map(|connection| connection.peer_address.clone())
                .ok_or_else(|| "LE connection disappeared before pairing".to_string())?;
            println!("<<< LE Connection: {peer}");
            println!("*** Pairing starting");
            let prompt = args.prompt;
            let delegate_factory = Box::new(move |_, _| {
                Box::new(CliDelegate {
                    prompt_for_acceptance: prompt,
                }) as Box<dyn PairingDelegate>
            });
            let session_config = pairing_config(&args, &le_address)?;
            let mut pairing = LePairingSession::new(
                &device,
                handle,
                le_address,
                session_config,
                delegate_factory,
            )
            .map_err(|error| error.to_string())?;
            let keys = if args.request {
                pairing
                    .request_peer(&mut host, &mut device)
                    .map_err(|error| error.to_string())?;
                pairing
                    .run_to_completion(&mut host, &mut device, PAIRING_TIMEOUT)
                    .map_err(|error| error.to_string())?
            } else if outgoing {
                pairing
                    .pair(&mut host, &mut device, PAIRING_TIMEOUT)
                    .map_err(|error| error.to_string())?
            } else {
                pairing.listen(&device).map_err(|error| error.to_string())?;
                pairing
                    .run_to_completion(&mut host, &mut device, PAIRING_TIMEOUT)
                    .map_err(|error| error.to_string())?
            };
            println!("*** Paired! (peer identity={peer})");
            print_pairing_keys("*** ", &keys)?;
            println!("@@@ Connection is encrypted");
            if args.bond {
                if let Some(store) = store.as_mut() {
                    pairing
                        .store_bond(store)
                        .map_err(|error| error.to_string())?;
                }
            }
            ConnectionKind::Le(handle)
        }
        ConnectionKind::Classic(handle) => {
            let peer = device
                .classic_connection(handle)
                .map(|connection| connection.peer_address.clone())
                .ok_or_else(|| "Classic connection disappeared before pairing".to_string())?;
            println!("<<< Classic Connection: {peer}");
            println!("*** Pairing starting");
            let stored_keys = store
                .as_ref()
                .map(|store| store.get(&peer.to_string(false)))
                .transpose()
                .map_err(|error| error.to_string())?
                .flatten();
            let local_address = public_address.as_ref().unwrap_or(&le_address).clone();
            let session_config = pairing_config(&args, &local_address)?;
            let mut pairing = ClassicPairingSession::new(
                &device,
                handle,
                session_config.clone(),
                Box::new(CliDelegate {
                    prompt_for_acceptance: args.prompt,
                }),
                stored_keys,
            )
            .map_err(|error| error.to_string())?;
            let keys = if args.request {
                pairing
                    .request_peer(&mut host, &mut device)
                    .map_err(|error| error.to_string())?;
                pairing
                    .run_to_completion(&mut host, &mut device, PAIRING_TIMEOUT)
                    .map_err(|error| error.to_string())?
            } else if outgoing {
                pairing
                    .pair(&mut host, &mut device, PAIRING_TIMEOUT)
                    .map_err(|error| error.to_string())?
            } else {
                pairing.listen(&device).map_err(|error| error.to_string())?;
                pairing
                    .run_to_completion(&mut host, &mut device, PAIRING_TIMEOUT)
                    .map_err(|error| error.to_string())?
            };
            println!("*** Paired [Classic]! (peer identity={peer})");
            print_pairing_keys("*** ", &keys)?;
            let ctkd_completed = if args.ctkd
                && args.secure_connections
                && matches!(keys.link_key_type, Some(0x07 | 0x08))
            {
                println!("*** CTKD over BR/EDR starting");
                let attempt = (|| {
                    let link_key = keys.link_key.as_ref().ok_or_else(|| {
                        "Classic pairing completed without a link key".to_string()
                    })?;
                    let link_key_value: [u8; 16] =
                        link_key.value.as_slice().try_into().map_err(|_| {
                            "Classic pairing returned a malformed link key".to_string()
                        })?;
                    enable_classic_encryption(&mut host, &mut device, handle)?;
                    let session = if args.request {
                        ClassicCtkdPairingSession::new_responder(
                            &device,
                            handle,
                            local_address,
                            session_config,
                            link_key_value,
                            link_key.authenticated,
                        )
                    } else {
                        ClassicCtkdPairingSession::new(
                            &device,
                            handle,
                            local_address,
                            session_config,
                            link_key_value,
                            link_key.authenticated,
                        )
                    }
                    .map_err(|error| error.to_string())?;
                    run_classic_ctkd(&mut host, &mut device, session)
                })();
                match attempt {
                    Ok((ctkd, ctkd_keys)) => {
                        println!("*** CTKD complete");
                        print_pairing_keys("*** ", &ctkd_keys)?;
                        if args.bond {
                            if let Some(store) = store.as_mut() {
                                ctkd.store_bond(store).map_err(|error| error.to_string())?;
                            }
                        }
                        true
                    }
                    Err(error) => {
                        eprintln!("*** CTKD unavailable: {error}");
                        false
                    }
                }
            } else {
                if args.ctkd {
                    if !args.secure_connections {
                        println!("*** CTKD skipped because Secure Connections is disabled");
                    } else {
                        println!(
                            "*** CTKD skipped because the Classic link key is not P-256 Secure Connections"
                        );
                    }
                }
                false
            };
            if args.bond && !ctkd_completed {
                if let Some(store) = store.as_mut() {
                    pairing
                        .store_bond(store)
                        .map_err(|error| error.to_string())?;
                }
            }
            ConnectionKind::Classic(handle)
        }
    };

    if args.linger {
        let is_connected = |device: &Device| match connected {
            ConnectionKind::Le(handle) => device.is_connected_on_handle(handle),
            ConnectionKind::Classic(handle) => device.classic_connection(handle).is_some(),
        };
        while is_connected(&device) {
            device.poll(&mut host);
            match host
                .wait_for_activity(Duration::from_secs(60))
                .map_err(|error| error.to_string())?
            {
                ExternalHostActivity::Packet | ExternalHostActivity::Timeout => {}
                ExternalHostActivity::Ended => break,
            }
        }
    } else {
        match connected {
            ConnectionKind::Le(handle) => {
                device.select_connection(handle);
                device.disconnect(&mut host, 0x13);
            }
            ConnectionKind::Classic(handle) => {
                device.disconnect_handle(&mut host, handle, 0x13);
            }
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

    #[test]
    fn parses_upstream_cli_surface() {
        assert_eq!(
            parse_args(
                [
                    "pair",
                    "--mode",
                    "le",
                    "--sc=false",
                    "--mitm",
                    "false",
                    "--bond=true",
                    "--ctkd=false",
                    "--advertising-address",
                    "random",
                    "--identity-address=public",
                    "--linger",
                    "--io",
                    "none",
                    "--oob=-",
                    "--prompt",
                    "--request",
                    "--print-keys",
                    "--keystore-file",
                    "keys.json",
                    "--advertise-service-uuid",
                    "180D",
                    "--advertise-service-uuid=12345678",
                    "--advertise-appearance",
                    "833",
                    "device.json",
                    "usb:0",
                    "C4:F2:17:1A:1D:BB",
                ]
                .map(str::to_string),
            )
            .unwrap(),
            Args {
                mode: Mode::Le,
                secure_connections: false,
                mitm: false,
                bond: true,
                ctkd: false,
                advertising_address: Some("random".into()),
                identity_address: Some("public".into()),
                linger: true,
                io: IoMode::None,
                oob: Some("-".into()),
                prompt: true,
                request: true,
                print_keys: true,
                keystore_file: Some(PathBuf::from("keys.json")),
                advertise_service_uuids: vec!["180D".into(), "12345678".into()],
                advertise_appearance: Some("833".into()),
                device_config: PathBuf::from("device.json"),
                transport: "usb:0".into(),
                address_or_name: Some("C4:F2:17:1A:1D:BB".into()),
            }
        );
        assert!(parse_args(["pair", "device.json"].map(str::to_string)).is_err());
    }

    #[test]
    fn builds_bounded_advertising_data_with_uuid_and_appearance() {
        let mut args = parse_args(["pair", "device.json", "usb:0"].map(str::to_string)).unwrap();
        args.advertise_service_uuids = vec!["180D".into(), "12345678".into()];
        args.advertise_appearance = Some("833".into());
        let bytes = advertising_data(&args, "Bumble").unwrap();
        assert!(bytes.len() <= 31);
        let parsed = AdvertisingData::from_bytes(&bytes);
        assert_eq!(
            parsed.get(AdvertisingDataType::APPEARANCE),
            Some(833u16.to_le_bytes().to_vec())
        );
        assert_eq!(
            parsed.get(AdvertisingDataType::COMPLETE_LOCAL_NAME),
            Some(b"Bumble".to_vec())
        );
    }

    #[test]
    fn cli_delegate_maps_confirmation_and_number_display() {
        let args = parse_args(
            ["pair", "--io", "display+yes/no", "device.json", "usb:0"].map(str::to_string),
        )
        .unwrap();
        assert_eq!(args.io.capability(), IoCapability::DisplayYesNo);
        let delegate = CliDelegate {
            prompt_for_acceptance: false,
        };
        assert!(!delegate.prompt_for_acceptance);
    }
}
