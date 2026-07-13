//! Auracast scanner, assistant, receiver, and transmitter.
//!
//! This is the Rust port of `apps/auracast.py`. It deliberately keeps the
//! upstream command names and option spellings so existing Bumble examples can
//! be translated without learning a second interface.

use bumble::advertising_data::Type as AdvertisingDataType;
use bumble::{Address, AddressType, AdvertisingData};
use bumble_audio::{
    create_audio_input, create_audio_output, AudioInput, Endianness, PcmFormat, SampleType,
};
use bumble_codecs::lc3::{Lc3Decoder, Lc3Encoder, Lc3FrameDuration, Lc3StreamConfig};
use bumble_gatt::GattClient;
use bumble_hci::{CodingFormat, Command as HciCommand};
use bumble_host::{
    BigInfoReport, BigParameters, BigSyncParameters, Device, ExtendedAdvertisingConfig,
    PeriodicAdvertisingConfig,
};
use bumble_profiles::bap::{
    AudioLocation, BasicAudioAnnouncement, BasicAudioBis, BasicAudioSubgroup,
    BroadcastAudioAnnouncement, CodecSpecificConfiguration, FrameDuration, SamplingFrequency,
    BASIC_AUDIO_ANNOUNCEMENT_SERVICE, BROADCAST_AUDIO_ANNOUNCEMENT_SERVICE,
};
use bumble_profiles::bass::{
    BroadcastAudioScanServiceProxy, ControlPointOperation, PeriodicAdvertisingSyncParams,
    SubgroupInfo,
};
use bumble_profiles::le_audio::{Metadata, MetadataEntry, MetadataTag};
use bumble_profiles::pbp::{
    PublicBroadcastAnnouncement, PublicBroadcastFeatures, PUBLIC_BROADCAST_ANNOUNCEMENT_SERVICE,
};
use bumble_smp::PairingConfig;
use bumble_transport::{
    open_split_transport, CommandResponse, ExternalAttTransport, ExternalHost,
    ExternalHostActivity, LePairingSession,
};
use serde::Deserialize;
use std::collections::VecDeque;
use std::fs;
use std::process::ExitCode;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const DEFAULT_DEVICE_NAME: &str = "Bumble Auracast";
const DEFAULT_DEVICE_ADDRESS: &str = "F0:F1:F2:F3:F4:F5";
const DEFAULT_SYNC_TIMEOUT: f64 = 5.0;
const DEFAULT_ATT_MTU: u16 = 256;
const DEFAULT_FRAME_DURATION_US: u32 = 10_000;
const DEFAULT_BITRATE: u32 = 80_000;
const DEFAULT_BROADCAST_ID: u32 = 123_456;
const DEFAULT_BROADCAST_NAME: &str = "Bumble Auracast";
const DEFAULT_LANGUAGE: &str = "en";
const DEFAULT_PROGRAM_INFO: &str = "Disco";
const COMMAND_TIMEOUT: Duration = Duration::from_secs(5);
const PROCEDURE_TIMEOUT: Duration = Duration::from_secs(15);
const BROADCASTING_AUDIO_SOURCE_APPEARANCE: u16 = 0x0884;

#[derive(Clone, Debug, PartialEq)]
enum Args {
    Help,
    Scan {
        filter_duplicates: bool,
        sync_timeout: f64,
        transport: String,
    },
    Assist {
        broadcast_name: Option<String>,
        source_id: Option<u8>,
        command: AssistCommand,
        transport: String,
        address: String,
    },
    Pair {
        transport: String,
        address: String,
    },
    Receive {
        transport: String,
        broadcast_id: Option<u32>,
        output: String,
        broadcast_code: Option<String>,
        sync_timeout: f64,
        subgroup: usize,
    },
    Transmit {
        transport: String,
        broadcast_list: Option<String>,
        input: Option<String>,
        input_format: String,
        broadcast_id: u32,
        broadcast_code: Option<String>,
        broadcast_name: String,
        bitrate: u32,
        manufacturer_data: Option<ManufacturerData>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AssistCommand {
    MonitorState,
    AddSource,
    ModifySource,
    RemoveSource,
}

impl AssistCommand {
    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "monitor-state" => Ok(Self::MonitorState),
            "add-source" => Ok(Self::AddSource),
            "modify-source" => Ok(Self::ModifySource),
            "remove-source" => Ok(Self::RemoveSource),
            _ => Err(format!("invalid assist command {value:?}")),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ManufacturerData {
    company_id: u16,
    data: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct BroadcastSource {
    input: String,
    input_format: String,
    bitrate: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct BroadcastConfig {
    sources: Vec<BroadcastSource>,
    public: bool,
    broadcast_id: u32,
    broadcast_name: String,
    broadcast_code: Option<String>,
    manufacturer_data: Option<ManufacturerData>,
    language: Option<String>,
    program_info: Option<String>,
}

#[derive(Debug, Deserialize)]
struct BroadcastListFile {
    #[serde(default)]
    broadcasts: Vec<BroadcastToml>,
}

#[derive(Debug, Deserialize)]
struct BroadcastToml {
    #[serde(default)]
    sources: Vec<BroadcastSourceToml>,
    #[serde(default = "default_true")]
    public: bool,
    #[serde(default = "default_broadcast_id", rename = "id")]
    broadcast_id: u32,
    name: String,
    #[serde(default, rename = "code")]
    broadcast_code: Option<String>,
    #[serde(default)]
    manufacturer_data: Option<ManufacturerDataToml>,
    #[serde(default)]
    language: Option<String>,
    #[serde(default)]
    program_info: Option<String>,
}

#[derive(Debug, Deserialize)]
struct BroadcastSourceToml {
    input: String,
    #[serde(default = "default_input_format", rename = "format")]
    input_format: String,
    #[serde(default = "default_bitrate")]
    bitrate: u32,
}

#[derive(Debug, Deserialize)]
struct ManufacturerDataToml {
    company_id: u16,
    data: String,
}

const fn default_true() -> bool {
    true
}

const fn default_broadcast_id() -> u32 {
    DEFAULT_BROADCAST_ID
}

fn default_input_format() -> String {
    "auto".into()
}

const fn default_bitrate() -> u32 {
    DEFAULT_BITRATE
}

fn usage() -> &'static str {
    "usage:\n  bumble-auracast scan [--filter-duplicates] [--sync-timeout SECONDS] TRANSPORT\n  bumble-auracast assist [--broadcast-name NAME] [--source-id ID] --command COMMAND TRANSPORT ADDRESS\n  bumble-auracast pair TRANSPORT ADDRESS\n  bumble-auracast receive [OPTIONS] TRANSPORT [BROADCAST_ID]\n  bumble-auracast transmit [OPTIONS] TRANSPORT"
}

fn take_option_value(values: &[String], index: &mut usize, name: &str) -> Result<String, String> {
    *index += 1;
    values
        .get(*index)
        .cloned()
        .ok_or_else(|| format!("{name} requires a value"))
}

fn parse_number<T: std::str::FromStr>(value: &str, name: &str) -> Result<T, String> {
    value
        .parse()
        .map_err(|_| format!("invalid {name} {value:?}"))
}

fn parse_args(arguments: impl IntoIterator<Item = impl Into<String>>) -> Result<Args, String> {
    let mut values = arguments.into_iter().map(Into::into).collect::<Vec<_>>();
    if values.is_empty() {
        return Err("missing executable name".into());
    }
    values.remove(0);
    if values
        .first()
        .is_some_and(|value| matches!(value.as_str(), "-h" | "--help"))
    {
        return Ok(Args::Help);
    }
    let command = values
        .first()
        .cloned()
        .ok_or_else(|| "missing Auracast command".to_string())?;
    values.remove(0);
    if values
        .iter()
        .any(|value| matches!(value.as_str(), "-h" | "--help"))
    {
        return Ok(Args::Help);
    }
    match command.as_str() {
        "scan" => parse_scan_args(&values),
        "assist" => parse_assist_args(&values),
        "pair" => {
            if values.len() != 2 {
                return Err("pair requires TRANSPORT and ADDRESS".into());
            }
            Ok(Args::Pair {
                transport: values[0].clone(),
                address: values[1].clone(),
            })
        }
        "receive" => parse_receive_args(&values),
        "transmit" => parse_transmit_args(&values),
        _ => Err(format!("unknown Auracast command {command:?}")),
    }
}

fn parse_scan_args(values: &[String]) -> Result<Args, String> {
    let mut filter_duplicates = false;
    let mut sync_timeout = DEFAULT_SYNC_TIMEOUT;
    let mut positional = Vec::new();
    let mut index = 0;
    while index < values.len() {
        match values[index].as_str() {
            "--filter-duplicates" => filter_duplicates = true,
            "--sync-timeout" => {
                let value = take_option_value(values, &mut index, "--sync-timeout")?;
                sync_timeout = parse_number(&value, "sync timeout")?;
            }
            option if option.starts_with('-') => return Err(format!("unknown option {option}")),
            value => positional.push(value.to_string()),
        }
        index += 1;
    }
    if positional.len() != 1 || !sync_timeout.is_finite() || sync_timeout <= 0.0 {
        return Err("scan requires one TRANSPORT and a positive sync timeout".into());
    }
    Ok(Args::Scan {
        filter_duplicates,
        sync_timeout,
        transport: positional.remove(0),
    })
}

fn parse_assist_args(values: &[String]) -> Result<Args, String> {
    let mut broadcast_name = None;
    let mut source_id = None;
    let mut command = None;
    let mut positional = Vec::new();
    let mut index = 0;
    while index < values.len() {
        match values[index].as_str() {
            "--broadcast-name" => {
                broadcast_name = Some(take_option_value(values, &mut index, "--broadcast-name")?)
            }
            "--source-id" => {
                let value = take_option_value(values, &mut index, "--source-id")?;
                source_id = Some(parse_number(&value, "source ID")?);
            }
            "--command" => {
                let value = take_option_value(values, &mut index, "--command")?;
                command = Some(AssistCommand::parse(&value)?);
            }
            option if option.starts_with('-') => return Err(format!("unknown option {option}")),
            value => positional.push(value.to_string()),
        }
        index += 1;
    }
    if positional.len() != 2 {
        return Err("assist requires TRANSPORT and ADDRESS".into());
    }
    let command = command.ok_or_else(|| "assist requires --command".to_string())?;
    if matches!(
        command,
        AssistCommand::ModifySource | AssistCommand::RemoveSource
    ) && source_id.is_none()
    {
        return Err("modify-source and remove-source require --source-id".into());
    }
    Ok(Args::Assist {
        broadcast_name,
        source_id,
        command,
        transport: positional.remove(0),
        address: positional.remove(0),
    })
}

fn parse_receive_args(values: &[String]) -> Result<Args, String> {
    let mut output = "device".to_string();
    let mut broadcast_code = None;
    let mut sync_timeout = DEFAULT_SYNC_TIMEOUT;
    let mut subgroup = 0usize;
    let mut positional = Vec::new();
    let mut index = 0;
    while index < values.len() {
        match values[index].as_str() {
            "--output" => output = take_option_value(values, &mut index, "--output")?,
            "--broadcast-code" => {
                broadcast_code = Some(take_option_value(values, &mut index, "--broadcast-code")?)
            }
            "--sync-timeout" => {
                let value = take_option_value(values, &mut index, "--sync-timeout")?;
                sync_timeout = parse_number(&value, "sync timeout")?;
            }
            "--subgroup" => {
                let value = take_option_value(values, &mut index, "--subgroup")?;
                subgroup = parse_number(&value, "subgroup")?;
            }
            option if option.starts_with('-') => return Err(format!("unknown option {option}")),
            value => positional.push(value.to_string()),
        }
        index += 1;
    }
    if positional.is_empty()
        || positional.len() > 2
        || !sync_timeout.is_finite()
        || sync_timeout <= 0.0
    {
        return Err(
            "receive requires TRANSPORT, an optional BROADCAST_ID, and a positive timeout".into(),
        );
    }
    let broadcast_id = positional
        .get(1)
        .map(|value| parse_number(value, "broadcast ID"))
        .transpose()?;
    validate_broadcast_id(broadcast_id.unwrap_or(1))?;
    if let Some(code) = &broadcast_code {
        broadcast_code_bytes(code)?;
    }
    Ok(Args::Receive {
        transport: positional.remove(0),
        broadcast_id,
        output,
        broadcast_code,
        sync_timeout,
        subgroup,
    })
}

fn parse_transmit_args(values: &[String]) -> Result<Args, String> {
    let mut broadcast_list = None;
    let mut input = None;
    let mut input_format = "auto".to_string();
    let mut broadcast_id = DEFAULT_BROADCAST_ID;
    let mut broadcast_code = None;
    let mut broadcast_name = DEFAULT_BROADCAST_NAME.to_string();
    let mut bitrate = DEFAULT_BITRATE;
    let mut manufacturer_data = None;
    let mut positional = Vec::new();
    let mut index = 0;
    while index < values.len() {
        match values[index].as_str() {
            "--broadcast-list" => {
                broadcast_list = Some(take_option_value(values, &mut index, "--broadcast-list")?)
            }
            "--input" => input = Some(take_option_value(values, &mut index, "--input")?),
            "--input-format" => {
                input_format = take_option_value(values, &mut index, "--input-format")?
            }
            "--broadcast-id" => {
                let value = take_option_value(values, &mut index, "--broadcast-id")?;
                broadcast_id = parse_number(&value, "broadcast ID")?;
            }
            "--broadcast-code" => {
                broadcast_code = Some(take_option_value(values, &mut index, "--broadcast-code")?)
            }
            "--broadcast-name" => {
                broadcast_name = take_option_value(values, &mut index, "--broadcast-name")?
            }
            "--bitrate" => {
                let value = take_option_value(values, &mut index, "--bitrate")?;
                bitrate = parse_number(&value, "bitrate")?;
            }
            "--manufacturer-data" => {
                let value = take_option_value(values, &mut index, "--manufacturer-data")?;
                manufacturer_data = Some(parse_manufacturer_data(&value)?);
            }
            option if option.starts_with('-') => return Err(format!("unknown option {option}")),
            value => positional.push(value.to_string()),
        }
        index += 1;
    }
    if positional.len() != 1 {
        return Err("transmit requires one TRANSPORT".into());
    }
    if broadcast_list.is_none() && input.is_none() {
        return Err("--input is required if --broadcast-list is not used".into());
    }
    validate_broadcast_id(broadcast_id)?;
    validate_bitrate(bitrate)?;
    if let Some(code) = &broadcast_code {
        broadcast_code_bytes(code)?;
    }
    Ok(Args::Transmit {
        transport: positional.remove(0),
        broadcast_list,
        input,
        input_format,
        broadcast_id,
        broadcast_code,
        broadcast_name,
        bitrate,
        manufacturer_data,
    })
}

fn validate_broadcast_id(broadcast_id: u32) -> Result<(), String> {
    if broadcast_id > 0x00FF_FFFF {
        Err(format!("broadcast ID {broadcast_id} exceeds 24 bits"))
    } else {
        Ok(())
    }
}

fn validate_bitrate(bitrate: u32) -> Result<(), String> {
    if bitrate == 0 || !bitrate.is_multiple_of(800) {
        return Err("bitrate must be a positive multiple of 800 bps for 10 ms LC3 frames".into());
    }
    let octets = bitrate / 800;
    if !(20..=400).contains(&octets) {
        return Err("bitrate produces an LC3 frame outside 20..=400 octets".into());
    }
    Ok(())
}

fn decode_hex(value: &str) -> Result<Vec<u8>, String> {
    if !value.len().is_multiple_of(2) {
        return Err("hex data must contain an even number of digits".into());
    }
    (0..value.len())
        .step_by(2)
        .map(|index| {
            u8::from_str_radix(&value[index..index + 2], 16)
                .map_err(|_| format!("invalid hex data {value:?}"))
        })
        .collect()
}

fn parse_manufacturer_data(value: &str) -> Result<ManufacturerData, String> {
    let (company_id, data) = value
        .split_once(':')
        .ok_or_else(|| "manufacturer data must be VENDOR-ID:DATA-HEX".to_string())?;
    Ok(ManufacturerData {
        company_id: parse_number(company_id, "manufacturer company ID")?,
        data: decode_hex(data)?,
    })
}

fn broadcast_code_bytes(value: &str) -> Result<[u8; 16], String> {
    if value.starts_with("0x") && value.len() == 34 {
        let mut code: [u8; 16] = decode_hex(&value[2..])?
            .try_into()
            .map_err(|_| "raw broadcast code must contain 16 bytes".to_string())?;
        code.reverse();
        return Ok(code);
    }
    let bytes = value.as_bytes();
    if bytes.len() > 16 {
        return Err("broadcast code must be <= 16 bytes in UTF-8 encoding".into());
    }
    let mut code = [0; 16];
    code[..bytes.len()].copy_from_slice(bytes);
    Ok(code)
}

fn parse_broadcast_list(path: &str) -> Result<Vec<BroadcastConfig>, String> {
    let source = fs::read_to_string(path)
        .map_err(|error| format!("failed to read broadcast list {path:?}: {error}"))?;
    let wire: BroadcastListFile =
        toml::from_str(&source).map_err(|error| format!("invalid broadcast list: {error}"))?;
    let mut broadcasts = wire
        .broadcasts
        .into_iter()
        .map(|broadcast| {
            let manufacturer_data = broadcast
                .manufacturer_data
                .map(|value| -> Result<ManufacturerData, String> {
                    Ok(ManufacturerData {
                        company_id: value.company_id,
                        data: decode_hex(&value.data)?,
                    })
                })
                .transpose()?;
            let sources = broadcast
                .sources
                .into_iter()
                .map(|source| {
                    validate_bitrate(source.bitrate)?;
                    Ok(BroadcastSource {
                        input: source.input,
                        input_format: source.input_format,
                        bitrate: source.bitrate,
                    })
                })
                .collect::<Result<Vec<_>, String>>()?;
            validate_broadcast_id(broadcast.broadcast_id)?;
            if sources.is_empty() {
                return Err(format!("broadcast {:?} has no sources", broadcast.name));
            }
            if let Some(code) = &broadcast.broadcast_code {
                broadcast_code_bytes(code)?;
            }
            Ok(BroadcastConfig {
                sources,
                public: broadcast.public,
                broadcast_id: broadcast.broadcast_id,
                broadcast_name: broadcast.name,
                broadcast_code: broadcast.broadcast_code,
                manufacturer_data,
                language: broadcast.language,
                program_info: broadcast.program_info,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    assign_broadcast_ids(&mut broadcasts)?;
    Ok(broadcasts)
}

fn assign_broadcast_ids(broadcasts: &mut [BroadcastConfig]) -> Result<(), String> {
    let mut assigned = Vec::new();
    let seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos()
        & 0x00FF_FFFF;
    for (index, broadcast) in broadcasts.iter_mut().enumerate() {
        if broadcast.broadcast_id == 0 {
            let mut candidate = (seed.wrapping_add(index as u32) & 0x00FF_FFFF).max(1);
            while assigned.contains(&candidate) {
                candidate = (candidate % 0x00FF_FFFF) + 1;
            }
            broadcast.broadcast_id = candidate;
        }
        if assigned.contains(&broadcast.broadcast_id) {
            return Err(format!("duplicate broadcast ID {}", broadcast.broadcast_id));
        }
        assigned.push(broadcast.broadcast_id);
    }
    Ok(())
}

fn service_data(advertising: &AdvertisingData, uuid: u16) -> Option<Vec<u8>> {
    advertising
        .ad_structures
        .iter()
        .filter(|(kind, _)| kind.0 == 0x16)
        .find_map(|(_, value)| {
            (value.len() >= 2 && value[..2] == uuid.to_le_bytes()).then(|| value[2..].to_vec())
        })
}

fn text_value(advertising: &AdvertisingData, kind: AdvertisingDataType) -> Option<String> {
    advertising
        .get(kind)
        .and_then(|value| String::from_utf8(value).ok())
}

#[derive(Clone, Debug)]
struct ScannedBroadcast {
    address: Address,
    sid: u8,
    interval: u16,
    rssi: i8,
    broadcast_id: u32,
    name: Option<String>,
    device_name: Option<String>,
    appearance: Option<u16>,
    manufacturer_data: Option<ManufacturerData>,
    public_announcement: Option<PublicBroadcastAnnouncement>,
    basic_audio_announcement: Option<BasicAudioAnnouncement>,
    biginfo: Option<BigInfoReport>,
    sync_handle: Option<u16>,
}

impl ScannedBroadcast {
    fn update_advertisement(&mut self, data: &[u8], rssi: i8, interval: u16) {
        let advertising = AdvertisingData::from_bytes(data);
        self.rssi = rssi;
        self.interval = interval;
        self.name =
            text_value(&advertising, AdvertisingDataType::BROADCAST_NAME).or(self.name.take());
        self.device_name = text_value(&advertising, AdvertisingDataType::COMPLETE_LOCAL_NAME)
            .or(self.device_name.take());
        if let Some(value) = advertising.get(AdvertisingDataType::APPEARANCE) {
            if let [low, high] = value.as_slice() {
                self.appearance = Some(u16::from_le_bytes([*low, *high]));
            }
        }
        if let Some(value) = advertising.get(AdvertisingDataType::MANUFACTURER_SPECIFIC_DATA) {
            if value.len() >= 2 {
                self.manufacturer_data = Some(ManufacturerData {
                    company_id: u16::from_le_bytes([value[0], value[1]]),
                    data: value[2..].to_vec(),
                });
            }
        }
        if let Some(value) = service_data(&advertising, PUBLIC_BROADCAST_ANNOUNCEMENT_SERVICE) {
            self.public_announcement = PublicBroadcastAnnouncement::from_bytes(&value).ok();
        }
    }

    fn ready(&self) -> bool {
        self.sync_handle.is_some()
            && self.basic_audio_announcement.is_some()
            && self.biginfo.is_some()
    }
}

struct BroadcastScanner {
    broadcasts: Vec<ScannedBroadcast>,
    queued: VecDeque<usize>,
    pending: Option<usize>,
    filter_duplicates: bool,
    sync_timeout: u16,
}

impl BroadcastScanner {
    fn new(filter_duplicates: bool, sync_timeout_seconds: f64) -> Result<Self, String> {
        let units = (sync_timeout_seconds * 100.0).ceil();
        if !units.is_finite() || !(10.0..=16_384.0).contains(&units) {
            return Err("sync timeout must be between 0.1 and 163.84 seconds".into());
        }
        Ok(Self {
            broadcasts: Vec::new(),
            queued: VecDeque::new(),
            pending: None,
            filter_duplicates,
            sync_timeout: units as u16,
        })
    }

    fn start(&mut self, host: &mut bumble_host::LocalLink, device: &mut Device) {
        device.start_extended_scanning(host, false, false);
    }

    fn stop(&mut self, host: &mut bumble_host::LocalLink, device: &mut Device) {
        device.stop_extended_scanning(host);
    }

    fn poll(
        &mut self,
        host: &mut bumble_host::LocalLink,
        device: &mut Device,
    ) -> Result<(), String> {
        device.poll(host);
        self.process_reports(device)?;
        self.process_sync_state(device);
        self.process_periodic_reports(device);
        self.process_biginfo(device);
        self.start_next_sync(host, device)?;
        Ok(())
    }

    fn process_reports(&mut self, device: &mut Device) -> Result<(), String> {
        for report in device.take_extended_advertising_reports() {
            let advertising = AdvertisingData::from_bytes(&report.data);
            let Some(value) = service_data(&advertising, BROADCAST_AUDIO_ANNOUNCEMENT_SERVICE)
            else {
                continue;
            };
            let announcement = BroadcastAudioAnnouncement::from_bytes(&value)
                .map_err(|error| format!("invalid Broadcast Audio Announcement: {error}"))?;
            if let Some(index) = self.broadcasts.iter().position(|broadcast| {
                broadcast.address == report.address
                    && broadcast.broadcast_id == announcement.broadcast_id
            }) {
                self.broadcasts[index].update_advertisement(
                    &report.data,
                    report.rssi,
                    report.periodic_advertising_interval,
                );
                continue;
            }
            let mut broadcast = ScannedBroadcast {
                address: report.address,
                sid: report.advertising_sid,
                interval: report.periodic_advertising_interval,
                rssi: report.rssi,
                broadcast_id: announcement.broadcast_id,
                name: None,
                device_name: None,
                appearance: None,
                manufacturer_data: None,
                public_announcement: None,
                basic_audio_announcement: None,
                biginfo: None,
                sync_handle: None,
            };
            broadcast.update_advertisement(
                &report.data,
                report.rssi,
                report.periodic_advertising_interval,
            );
            self.broadcasts.push(broadcast);
            self.queued.push_back(self.broadcasts.len() - 1);
        }
        Ok(())
    }

    fn process_sync_state(&mut self, device: &mut Device) {
        if !device.take_periodic_sync_errors().is_empty() {
            self.pending = None;
        }
        if let Some(index) = self.pending {
            let broadcast = &self.broadcasts[index];
            let established = device.periodic_syncs().values().find(|sync| {
                sync.advertiser_address == broadcast.address
                    && sync.advertising_sid == broadcast.sid
            });
            if let Some(sync) = established {
                self.broadcasts[index].sync_handle = Some(sync.sync_handle);
                self.broadcasts[index].interval = sync.interval;
                self.pending = None;
            }
        }
        for lost in device.take_lost_periodic_syncs() {
            if let Some(broadcast) = self
                .broadcasts
                .iter_mut()
                .find(|broadcast| broadcast.sync_handle == Some(lost))
            {
                broadcast.sync_handle = None;
                broadcast.basic_audio_announcement = None;
                broadcast.biginfo = None;
            }
        }
    }

    fn process_periodic_reports(&mut self, device: &mut Device) {
        for report in device.take_periodic_advertisements() {
            let Some(broadcast) = self
                .broadcasts
                .iter_mut()
                .find(|broadcast| broadcast.sync_handle == Some(report.sync_handle))
            else {
                continue;
            };
            let advertising = AdvertisingData::from_bytes(&report.data);
            if let Some(value) = service_data(&advertising, BASIC_AUDIO_ANNOUNCEMENT_SERVICE) {
                broadcast.basic_audio_announcement =
                    BasicAudioAnnouncement::from_bytes(&value).ok();
            }
        }
    }

    fn process_biginfo(&mut self, device: &mut Device) {
        for report in device.take_biginfo_reports() {
            if let Some(broadcast) = self
                .broadcasts
                .iter_mut()
                .find(|broadcast| broadcast.sync_handle == Some(report.sync_handle))
            {
                broadcast.biginfo = Some(report);
            }
        }
    }

    fn start_next_sync(
        &mut self,
        host: &mut bumble_host::LocalLink,
        device: &mut Device,
    ) -> Result<(), String> {
        if self.pending.is_some() {
            return Ok(());
        }
        while let Some(index) = self.queued.pop_front() {
            if self.broadcasts[index].sync_handle.is_some() {
                continue;
            }
            let broadcast = &self.broadcasts[index];
            if !device.create_periodic_advertising_sync(
                host,
                broadcast.address.clone(),
                broadcast.sid,
                0,
                self.sync_timeout,
                self.filter_duplicates,
            ) {
                return Err("controller rejected periodic advertising sync parameters".into());
            }
            self.pending = Some(index);
            break;
        }
        Ok(())
    }

    fn find_ready(&self, broadcast_id: Option<u32>, name: Option<&str>) -> Option<usize> {
        self.broadcasts.iter().position(|broadcast| {
            broadcast.ready()
                && broadcast_id.is_none_or(|wanted| wanted == broadcast.broadcast_id)
                && name.is_none_or(|wanted| broadcast.name.as_deref() == Some(wanted))
        })
    }
}

fn format_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn print_broadcast(broadcast: &ScannedBroadcast) {
    println!("Broadcast: {}", broadcast.address);
    if let Some(name) = &broadcast.name {
        println!("  Broadcast Name: {name}");
    }
    if let Some(name) = &broadcast.device_name {
        println!("  Device Name:    {name}");
    }
    if let Some(appearance) = broadcast.appearance {
        println!("  Appearance:     0x{appearance:04X}");
    }
    println!("  RSSI:           {}", broadcast.rssi);
    println!("  SID:            {}", broadcast.sid);
    println!("  Broadcast ID:   {}", broadcast.broadcast_id);
    if let Some(manufacturer) = &broadcast.manufacturer_data {
        println!(
            "  Manufacturer:   0x{:04X} -> {}",
            manufacturer.company_id,
            format_hex(&manufacturer.data)
        );
    }
    if let Some(public) = &broadcast.public_announcement {
        println!("  Public Features: 0x{:02X}", public.features.0);
        if !public.metadata.entries.is_empty() {
            println!(
                "{}",
                public.metadata.pretty_print("    ").unwrap_or_default()
            );
        }
    }
    if let Some(audio) = &broadcast.basic_audio_announcement {
        println!("  Presentation Delay: {} us", audio.presentation_delay);
        for (index, subgroup) in audio.subgroups.iter().enumerate() {
            println!("  Subgroup {index}: {} BIS", subgroup.bis.len());
            println!("    Codec: 0x{:02X}", subgroup.codec_id.coding_format);
        }
    }
    if let Some(biginfo) = &broadcast.biginfo {
        println!(
            "  BIG: {} BIS, max SDU {}, interval {} us, encrypted {}",
            biginfo.num_bis, biginfo.max_sdu, biginfo.sdu_interval, biginfo.encrypted
        );
    }
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

fn command(host: &mut ExternalHost, command: HciCommand, context: &str) -> Result<(), String> {
    require_success(
        host.send_command(command, COMMAND_TIMEOUT)
            .map_err(|error| error.to_string())?,
        context,
    )
}

fn local_address() -> Result<Address, String> {
    Address::parse(DEFAULT_DEVICE_ADDRESS, AddressType::RANDOM_DEVICE)
        .map_err(|error| error.to_string())
}

fn open_device(transport: &str) -> Result<(ExternalHost, Device, Address), String> {
    let transport = open_split_transport(transport).map_err(|error| error.to_string())?;
    let mut host = ExternalHost::new(transport);
    let mut device = Device::new(0);
    host.initialize_device(&mut device, COMMAND_TIMEOUT)
        .map_err(|error| error.to_string())?;
    let address = local_address()?;
    command(
        &mut host,
        HciCommand::LeSetRandomAddress {
            random_address: address.clone(),
        },
        "setting the local random address",
    )?;
    Ok((host, device, address))
}

fn wait_for_connection(
    host: &mut ExternalHost,
    device: &mut Device,
    peer: &Address,
) -> Result<u16, String> {
    let deadline = Instant::now() + PROCEDURE_TIMEOUT;
    loop {
        device.poll(host);
        if let Some(handle) = device.connection_handle_for_peer(peer) {
            return Ok(handle);
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err("timed out waiting for LE connection".into());
        }
        match host
            .wait_for_activity(remaining)
            .map_err(|error| error.to_string())?
        {
            ExternalHostActivity::Packet => {}
            ExternalHostActivity::Timeout => {
                return Err("timed out waiting for LE connection".into())
            }
            ExternalHostActivity::Ended => {
                return Err("HCI transport ended while waiting for LE connection".into())
            }
        }
    }
}

fn connect(
    host: &mut ExternalHost,
    device: &mut Device,
    address: &str,
) -> Result<(u16, Address), String> {
    let peer = Address::parse(address, AddressType::RANDOM_DEVICE)
        .map_err(|error| format!("invalid peer address: {error}"))?;
    println!("=== Connecting to {peer}...");
    command(
        host,
        HciCommand::LeCreateConnection {
            le_scan_interval: 0x0010,
            le_scan_window: 0x0010,
            initiator_filter_policy: 0,
            peer_address_type: u8::from(!peer.is_public()),
            peer_address: peer.clone(),
            own_address_type: 1,
            connection_interval_min: 24,
            connection_interval_max: 40,
            max_latency: 0,
            supervision_timeout: 42,
            min_ce_length: 0,
            max_ce_length: 0,
        },
        "creating LE connection",
    )?;
    let handle = wait_for_connection(host, device, &peer)?;
    println!("=== Connected on handle 0x{handle:04X}");
    Ok((handle, peer))
}

fn pair_connection(
    host: &mut ExternalHost,
    device: &mut Device,
    handle: u16,
    local_address: Address,
) -> Result<(), String> {
    println!("+++ Initiating pairing...");
    let mut pairing = LePairingSession::accept_all(
        device,
        handle,
        local_address,
        PairingConfig {
            mitm: false,
            ..PairingConfig::default()
        },
    )
    .map_err(|error| error.to_string())?;
    pairing
        .pair(host, device, PROCEDURE_TIMEOUT)
        .map_err(|error| error.to_string())?;
    println!("+++ Paired and encrypted");
    Ok(())
}

fn wait_for_activity(
    host: &mut ExternalHost,
    device: &mut Device,
    timeout: Duration,
) -> Result<bool, String> {
    device.poll(host);
    match host
        .wait_for_activity(timeout)
        .map_err(|error| error.to_string())?
    {
        ExternalHostActivity::Packet | ExternalHostActivity::Timeout => Ok(true),
        ExternalHostActivity::Ended => Ok(false),
    }
}

fn wait_for_ready_broadcast(
    host: &mut ExternalHost,
    device: &mut Device,
    scanner: &mut BroadcastScanner,
    broadcast_id: Option<u32>,
    name: Option<&str>,
) -> Result<usize, String> {
    loop {
        scanner.poll(host, device)?;
        if let Some(index) = scanner.find_ready(broadcast_id, name) {
            return Ok(index);
        }
        if !wait_for_activity(host, device, Duration::from_millis(250))? {
            return Err("HCI transport ended while scanning for a broadcast".into());
        }
    }
}

fn run_scan(transport: &str, filter_duplicates: bool, sync_timeout: f64) -> Result<(), String> {
    let (mut host, mut device, _) = open_device(transport)?;
    let mut scanner = BroadcastScanner::new(filter_duplicates, sync_timeout)?;
    scanner.start(&mut host, &mut device);
    println!("Scanning for public broadcasts...");
    let mut last_render = String::new();
    loop {
        scanner.poll(&mut host, &mut device)?;
        let render = scanner
            .broadcasts
            .iter()
            .map(|broadcast| {
                format!(
                    "{}:{}:{}:{}:{}",
                    broadcast.address,
                    broadcast.broadcast_id,
                    broadcast.rssi,
                    broadcast.basic_audio_announcement.is_some(),
                    broadcast.biginfo.is_some()
                )
            })
            .collect::<Vec<_>>()
            .join("|");
        if render != last_render {
            println!("==========================================");
            println!("Found {} broadcast(s)", scanner.broadcasts.len());
            for broadcast in &scanner.broadcasts {
                print_broadcast(broadcast);
                println!("------------------------------------------");
            }
            last_render = render;
        }
        if !wait_for_activity(&mut host, &mut device, Duration::from_millis(500))? {
            return Ok(());
        }
    }
}

fn run_pair(transport: &str, address: &str) -> Result<(), String> {
    let (mut host, mut device, local_address) = open_device(transport)?;
    let (handle, _) = connect(&mut host, &mut device, address)?;
    pair_connection(&mut host, &mut device, handle, local_address)
}

fn bass_state_snapshot(
    proxy: &BroadcastAudioScanServiceProxy,
    client: &mut GattClient,
    transport: &mut ExternalAttTransport<'_>,
) -> Result<(), String> {
    for (index, characteristic) in proxy.broadcast_receive_states.iter().enumerate() {
        let state =
            BroadcastAudioScanServiceProxy::read_receive_state(characteristic, client, transport)
                .map_err(|error| error.to_string())?;
        println!("Initial Broadcast Receive State [{index}]: {state:?}");
    }
    Ok(())
}

fn send_bass_operation(
    host: &mut ExternalHost,
    device: &mut Device,
    handle: u16,
    operation: &ControlPointOperation,
) -> Result<(), String> {
    let mut transport = ExternalAttTransport::new(host, device, handle, PROCEDURE_TIMEOUT)
        .map_err(|error| error.to_string())?;
    let mut client = GattClient::new();
    let proxy = BroadcastAudioScanServiceProxy::discover(&mut client, &mut transport)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "Broadcast Audio Scan Service not found".to_string())?;
    proxy
        .send_control_point_operation(&mut client, &mut transport, operation)
        .map_err(|error| error.to_string())
}

fn monitor_bass_states(
    host: &mut ExternalHost,
    device: &mut Device,
    handle: u16,
    proxy: &BroadcastAudioScanServiceProxy,
) -> Result<(), String> {
    loop {
        device.poll(host);
        for pdu in device.take_inbox_on_handle(handle) {
            match pdu {
                bumble_att::AttPdu::HandleValueNotification {
                    attribute_handle,
                    attribute_value,
                }
                | bumble_att::AttPdu::HandleValueIndication {
                    attribute_handle,
                    attribute_value,
                } => {
                    let state = proxy
                        .state_from_notification(attribute_handle, &attribute_value)
                        .map_err(|error| error.to_string())?;
                    println!("Broadcast Receive State Update: {state:?}");
                }
                _ => {}
            }
        }
        if !device.is_connected_on_handle(handle) {
            return Ok(());
        }
        if !wait_for_activity(host, device, Duration::from_millis(500))? {
            return Ok(());
        }
    }
}

fn run_assist(
    transport_name: &str,
    address: &str,
    broadcast_name: Option<&str>,
    source_id: Option<u8>,
    assist_command: AssistCommand,
) -> Result<(), String> {
    let (mut host, mut device, local_address) = open_device(transport_name)?;
    let (handle, _) = connect(&mut host, &mut device, address)?;
    pair_connection(&mut host, &mut device, handle, local_address)?;

    let monitor_proxy = {
        let mut transport =
            ExternalAttTransport::new(&mut host, &mut device, handle, PROCEDURE_TIMEOUT)
                .map_err(|error| error.to_string())?;
        let mut client = GattClient::new();
        let mtu = client
            .exchange_mtu(&mut transport, DEFAULT_ATT_MTU)
            .map_err(|error| error.to_string())?;
        println!("$$$ ATT MTU={mtu}");
        let proxy = BroadcastAudioScanServiceProxy::discover(&mut client, &mut transport)
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "Broadcast Audio Scan Service not found".to_string())?;
        proxy
            .subscribe_receive_states(&mut client, &mut transport)
            .map_err(|error| error.to_string())?;
        bass_state_snapshot(&proxy, &mut client, &mut transport)?;
        proxy
    };

    if assist_command == AssistCommand::MonitorState {
        return monitor_bass_states(&mut host, &mut device, handle, &monitor_proxy);
    }
    if assist_command == AssistCommand::RemoveSource {
        send_bass_operation(
            &mut host,
            &mut device,
            handle,
            &ControlPointOperation::RemoveSource {
                source_id: source_id.expect("validated source ID"),
            },
        )?;
        return monitor_bass_states(&mut host, &mut device, handle, &monitor_proxy);
    }

    send_bass_operation(
        &mut host,
        &mut device,
        handle,
        &ControlPointOperation::RemoteScanStarted,
    )?;
    println!(
        "Scanning for {}...",
        broadcast_name.unwrap_or("any broadcast")
    );
    let mut scanner = BroadcastScanner::new(false, DEFAULT_SYNC_TIMEOUT)?;
    scanner.start(&mut host, &mut device);
    let index =
        wait_for_ready_broadcast(&mut host, &mut device, &mut scanner, None, broadcast_name)?;
    scanner.stop(&mut host, &mut device);
    let broadcast = scanner.broadcasts[index].clone();
    let subgroup = broadcast
        .basic_audio_announcement
        .as_ref()
        .and_then(|announcement| announcement.subgroups.first())
        .ok_or_else(|| "broadcast has no Basic Audio Announcement subgroup".to_string())?;
    let metadata = subgroup
        .metadata
        .to_bytes()
        .map_err(|error| error.to_string())?;
    let subgroups = vec![SubgroupInfo {
        bis_sync: SubgroupInfo::ANY_BIS,
        metadata,
    }];
    match assist_command {
        AssistCommand::AddSource => {
            send_bass_operation(
                &mut host,
                &mut device,
                handle,
                &ControlPointOperation::AddSource {
                    advertiser_address: broadcast.address.clone(),
                    advertising_sid: broadcast.sid,
                    broadcast_id: broadcast.broadcast_id,
                    pa_sync: PeriodicAdvertisingSyncParams::SYNCHRONIZE_TO_PA_PAST_AVAILABLE,
                    pa_interval: 0xFFFF,
                    subgroups,
                },
            )?;
            if !device.transfer_periodic_advertising_sync_on_handle(
                &mut host,
                handle,
                broadcast.sync_handle.expect("ready broadcast has sync"),
                0,
            ) {
                return Err("failed to initiate periodic advertising sync transfer".into());
            }
            send_bass_operation(
                &mut host,
                &mut device,
                handle,
                &ControlPointOperation::RemoteScanStopped,
            )?;
        }
        AssistCommand::ModifySource => send_bass_operation(
            &mut host,
            &mut device,
            handle,
            &ControlPointOperation::ModifySource {
                source_id: source_id.expect("validated source ID"),
                pa_sync: PeriodicAdvertisingSyncParams::SYNCHRONIZE_TO_PA_PAST_NOT_AVAILABLE,
                pa_interval: 0xFFFF,
                subgroups,
            },
        )?,
        AssistCommand::MonitorState | AssistCommand::RemoveSource => unreachable!(),
    }
    monitor_bass_states(&mut host, &mut device, handle, &monitor_proxy)
}

struct TransmitSource {
    input: Box<dyn AudioInput>,
    format: PcmFormat,
    encoder: Lc3Encoder,
    frame_samples: usize,
    frame_octets: usize,
    pending: Vec<u8>,
}

impl TransmitSource {
    fn open(config: &BroadcastSource) -> Result<Self, String> {
        validate_bitrate(config.bitrate)?;
        let mut input = create_audio_input(&config.input, &config.input_format)
            .map_err(|error| error.to_string())?;
        let format = input.open().map_err(|error| error.to_string())?;
        if !matches!(format.channels, 1 | 2) {
            return Err("only one- and two-channel PCM inputs are supported".into());
        }
        if !matches!(format.sample_rate, 16_000 | 24_000 | 48_000) {
            return Err(format!(
                "sample rate {} Hz is not supported for Auracast transmit",
                format.sample_rate
            ));
        }
        let frame_octets = usize::try_from(config.bitrate / 800)
            .map_err(|_| "LC3 frame size exceeds this platform".to_string())?;
        let stream_config = Lc3StreamConfig {
            sampling_frequency: format.sample_rate,
            frame_duration: Lc3FrameDuration::TenMs,
            channels: usize::from(format.channels),
            octets_per_codec_frame: frame_octets,
            codec_frames_per_sdu: 1,
        };
        let frame_samples = stream_config.frame_samples();
        let encoder = Lc3Encoder::new(stream_config).map_err(|error| error.to_string())?;
        Ok(Self {
            input,
            format,
            encoder,
            frame_samples,
            frame_octets,
            pending: Vec::new(),
        })
    }

    fn next_pcm(&mut self) -> Result<Option<Vec<i16>>, String> {
        let byte_count = self
            .frame_samples
            .checked_mul(usize::from(self.format.channels))
            .and_then(|samples| samples.checked_mul(self.format.bytes_per_sample()))
            .ok_or_else(|| "PCM frame size overflow".to_string())?;
        while self.pending.len() < byte_count {
            match self
                .input
                .read_frame(self.frame_samples)
                .map_err(|error| error.to_string())?
            {
                Some(bytes) => self.pending.extend_from_slice(&bytes),
                None if self.pending.is_empty() => return Ok(None),
                None => return Err("audio input ended with a partial PCM frame".into()),
            }
        }
        let bytes = self.pending.drain(..byte_count).collect::<Vec<_>>();
        let samples = match self.format.sample_type {
            SampleType::Int16 => bytes
                .chunks_exact(2)
                .map(|sample| i16::from_le_bytes([sample[0], sample[1]]))
                .collect(),
            SampleType::Float32 => bytes
                .chunks_exact(4)
                .map(|sample| {
                    let value = f32::from_le_bytes([sample[0], sample[1], sample[2], sample[3]]);
                    (value.clamp(-1.0, 1.0) * f32::from(i16::MAX)).round() as i16
                })
                .collect(),
        };
        Ok(Some(samples))
    }

    fn close(&mut self) -> Result<(), String> {
        self.input.close().map_err(|error| error.to_string())
    }
}

struct TransmitBroadcast {
    config: BroadcastConfig,
    sources: Vec<TransmitSource>,
    bis_handles: Vec<u16>,
}

fn subgroup_metadata(config: &BroadcastConfig) -> Metadata {
    let mut entries = Vec::new();
    if let Some(language) = &config.language {
        entries.push(MetadataEntry::new(
            MetadataTag::LANGUAGE,
            language.as_bytes().to_vec(),
        ));
    }
    if let Some(program_info) = &config.program_info {
        entries.push(MetadataEntry::new(
            MetadataTag::PROGRAM_INFO,
            program_info.as_bytes().to_vec(),
        ));
    }
    Metadata::new(entries)
}

fn basic_audio_announcement(sources: &[TransmitSource]) -> Result<BasicAudioAnnouncement, String> {
    let mut next_bis = 1u8;
    let mut subgroups = Vec::new();
    for source in sources {
        let mut bis = Vec::new();
        for channel in 0..source.format.channels {
            let location = if channel == 0 {
                AudioLocation::FRONT_LEFT
            } else {
                AudioLocation::FRONT_RIGHT
            };
            bis.push(BasicAudioBis {
                index: next_bis,
                codec_specific_configuration: CodecSpecificConfiguration {
                    audio_channel_allocation: Some(location),
                    ..CodecSpecificConfiguration::default()
                },
            });
            next_bis = next_bis
                .checked_add(1)
                .ok_or_else(|| "too many BIS channels".to_string())?;
        }
        subgroups.push(BasicAudioSubgroup {
            codec_id: CodingFormat::LC3,
            codec_specific_configuration: CodecSpecificConfiguration {
                sampling_frequency: Some(
                    SamplingFrequency::from_hz(source.format.sample_rate)
                        .map_err(|error| error.to_string())?,
                ),
                frame_duration: Some(FrameDuration::DURATION_10000_US),
                audio_channel_allocation: None,
                octets_per_codec_frame: Some(
                    u16::try_from(source.frame_octets)
                        .map_err(|_| "LC3 frame exceeds 65535 octets".to_string())?,
                ),
                codec_frames_per_sdu: Some(1),
            },
            metadata: Metadata::default(),
            bis,
        });
    }
    Ok(BasicAudioAnnouncement {
        presentation_delay: 40_000,
        subgroups,
    })
}

fn extended_advertising_data(
    config: &BroadcastConfig,
    sources: &[TransmitSource],
) -> Result<Vec<u8>, String> {
    let mut advertising = AdvertisingData {
        ad_structures: vec![
            (
                AdvertisingDataType::COMPLETE_LOCAL_NAME,
                DEFAULT_DEVICE_NAME.as_bytes().to_vec(),
            ),
            (
                AdvertisingDataType::APPEARANCE,
                BROADCASTING_AUDIO_SOURCE_APPEARANCE.to_le_bytes().to_vec(),
            ),
            (
                AdvertisingDataType::BROADCAST_NAME,
                config.broadcast_name.as_bytes().to_vec(),
            ),
        ],
    };
    if let Some(manufacturer) = &config.manufacturer_data {
        let mut value = manufacturer.company_id.to_le_bytes().to_vec();
        value.extend_from_slice(&manufacturer.data);
        advertising
            .ad_structures
            .push((AdvertisingDataType::MANUFACTURER_SPECIFIC_DATA, value));
    }
    let mut data = advertising.to_bytes();
    if config.public {
        let mut features = PublicBroadcastFeatures(0);
        if config.broadcast_code.is_some() {
            features |= PublicBroadcastFeatures::ENCRYPTED;
        }
        for source in sources {
            features |= if source.format.sample_rate == 48_000 {
                PublicBroadcastFeatures::HIGH_QUALITY_CONFIGURATION
            } else {
                PublicBroadcastFeatures::STANDARD_QUALITY_CONFIGURATION
            };
        }
        data.extend_from_slice(
            &PublicBroadcastAnnouncement {
                features,
                metadata: subgroup_metadata(config),
            }
            .advertising_data()
            .map_err(|error| error.to_string())?,
        );
    }
    data.extend_from_slice(
        &BroadcastAudioAnnouncement::new(config.broadcast_id)
            .map_err(|error| error.to_string())?
            .advertising_data()
            .map_err(|error| error.to_string())?,
    );
    Ok(data)
}

fn wait_for_big(
    host: &mut ExternalHost,
    device: &mut Device,
    big_handle: u8,
    synchronized: bool,
) -> Result<Vec<u16>, String> {
    let deadline = Instant::now() + PROCEDURE_TIMEOUT;
    loop {
        device.poll(host);
        let handles = if synchronized {
            device.big_sync_bis_handles(big_handle)
        } else {
            device.big_bis_handles(big_handle)
        };
        if let Some(handles) = handles {
            return Ok(handles.to_vec());
        }
        if let Some((_, status)) = device
            .take_big_errors()
            .into_iter()
            .find(|(handle, _)| *handle == big_handle)
        {
            return Err(format!(
                "BIG procedure {big_handle} failed with HCI status 0x{status:02X}"
            ));
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(format!("timed out waiting for BIG {big_handle}"));
        }
        match host
            .wait_for_activity(remaining)
            .map_err(|error| error.to_string())?
        {
            ExternalHostActivity::Packet => {}
            ExternalHostActivity::Timeout => {
                return Err(format!("timed out waiting for BIG {big_handle}"))
            }
            ExternalHostActivity::Ended => {
                return Err(format!(
                    "HCI transport ended while creating BIG {big_handle}"
                ))
            }
        }
    }
}

fn configure_transmit_broadcast(
    host: &mut ExternalHost,
    device: &mut Device,
    local_address: &Address,
    index: usize,
    config: BroadcastConfig,
) -> Result<TransmitBroadcast, String> {
    let sources = config
        .sources
        .iter()
        .map(TransmitSource::open)
        .collect::<Result<Vec<_>, _>>()?;
    let num_bis = sources
        .iter()
        .map(|source| usize::from(source.format.channels))
        .sum::<usize>();
    let num_bis = u8::try_from(num_bis).map_err(|_| "broadcast has over 255 BIS channels")?;
    let advertising_handle = u8::try_from(index).map_err(|_| "over 255 broadcasts configured")?;
    if advertising_handle > 0x0F {
        return Err("at most 16 simultaneous broadcasts are supported".into());
    }
    let announcement = basic_audio_announcement(&sources)?;
    let extended_data = extended_advertising_data(&config, &sources)?;
    let periodic_data = announcement
        .advertising_data()
        .map_err(|error| error.to_string())?;
    let mut advertising_config =
        ExtendedAdvertisingConfig::connectable_scannable(advertising_handle, local_address.clone());
    advertising_config.event_properties = 0;
    advertising_config.interval_min = 100;
    advertising_config.interval_max = 1_000;
    advertising_config.sid = advertising_handle;
    if !device.start_extended_advertising(host, &advertising_config, &extended_data, &[]) {
        return Err("failed to configure extended advertising".into());
    }
    let mut periodic_config = PeriodicAdvertisingConfig::new(advertising_handle);
    periodic_config.interval_min = 100;
    periodic_config.interval_max = 1_000;
    if !device.start_periodic_advertising(host, periodic_config, &periodic_data) {
        return Err("failed to configure periodic advertising".into());
    }
    let max_sdu = sources
        .iter()
        .map(|source| source.frame_octets)
        .max()
        .ok_or_else(|| "broadcast has no audio sources".to_string())?;
    let mut big = BigParameters::new(advertising_handle, advertising_handle, num_bis);
    big.sdu_interval = DEFAULT_FRAME_DURATION_US;
    big.max_sdu = u16::try_from(max_sdu).map_err(|_| "LC3 frame exceeds 65535 octets")?;
    big.max_transport_latency = 65;
    big.rtn = 4;
    big.broadcast_code = config
        .broadcast_code
        .as_deref()
        .map(broadcast_code_bytes)
        .transpose()?;
    if !device.create_big(host, big) {
        return Err("controller rejected BIG parameters".into());
    }
    let bis_handles = wait_for_big(host, device, advertising_handle, false)?;
    for handle in &bis_handles {
        if !device.setup_iso_data_path(host, *handle, 0) {
            return Err(format!(
                "failed to set up transmit path for BIS 0x{handle:04X}"
            ));
        }
    }
    println!(
        "Broadcast {} ready: ID {}, {} BIS, {} extended bytes, {} periodic bytes",
        config.broadcast_name,
        config.broadcast_id,
        bis_handles.len(),
        extended_data.len(),
        periodic_data.len()
    );
    Ok(TransmitBroadcast {
        config,
        sources,
        bis_handles,
    })
}

fn run_transmit(transport: &str, configs: Vec<BroadcastConfig>) -> Result<(), String> {
    let (mut host, mut device, local_address) = open_device(transport)?;
    let mut broadcasts = configs
        .into_iter()
        .enumerate()
        .map(|(index, config)| {
            configure_transmit_broadcast(&mut host, &mut device, &local_address, index, config)
        })
        .collect::<Result<Vec<_>, _>>()?;
    println!("Transmitting audio");
    let mut next_frame = Instant::now();
    let frame_duration = Duration::from_micros(u64::from(DEFAULT_FRAME_DURATION_US));
    loop {
        device.poll(&mut host);
        if let Some((handle, reason)) = device.take_terminated_bigs().into_iter().next() {
            return Err(format!(
                "BIG {handle} terminated with reason 0x{reason:02X}"
            ));
        }
        for broadcast in &mut broadcasts {
            let mut bis_index = 0usize;
            for source in &mut broadcast.sources {
                let Some(pcm) = source.next_pcm()? else {
                    println!("Audio input for {} ended", broadcast.config.broadcast_name);
                    for broadcast in &mut broadcasts {
                        for source in &mut broadcast.sources {
                            source.close()?;
                        }
                    }
                    return Ok(());
                };
                let encoded = source
                    .encoder
                    .encode_sdu(&pcm)
                    .map_err(|error| error.to_string())?;
                for frame in encoded.chunks_exact(source.frame_octets) {
                    let handle = *broadcast.bis_handles.get(bis_index).ok_or_else(|| {
                        "BIG exposed fewer BIS handles than configured".to_string()
                    })?;
                    if !device.send_iso_sdu(&mut host, handle, frame) {
                        return Err(format!("failed to send ISO SDU on BIS 0x{handle:04X}"));
                    }
                    bis_index += 1;
                }
            }
        }
        next_frame += frame_duration;
        let delay = next_frame.saturating_duration_since(Instant::now());
        if !delay.is_zero() {
            std::thread::sleep(delay);
        } else if Instant::now().duration_since(next_frame) > frame_duration * 4 {
            next_frame = Instant::now();
        }
    }
}

fn merged_stream_config(
    subgroup: &BasicAudioSubgroup,
) -> Result<(Lc3StreamConfig, Vec<u8>), String> {
    if subgroup.codec_id != CodingFormat::LC3 {
        return Err(format!(
            "subgroup uses unsupported codec 0x{:02X}",
            subgroup.codec_id.coding_format
        ));
    }
    let sampling_frequency = subgroup
        .codec_specific_configuration
        .sampling_frequency
        .ok_or_else(|| "subgroup omits LC3 sampling frequency".to_string())?
        .hz()
        .map_err(|error| error.to_string())?;
    let frame_duration = match subgroup
        .codec_specific_configuration
        .frame_duration
        .ok_or_else(|| "subgroup omits LC3 frame duration".to_string())?
        .microseconds()
        .map_err(|error| error.to_string())?
    {
        7_500 => Lc3FrameDuration::SevenPointFiveMs,
        10_000 => Lc3FrameDuration::TenMs,
        _ => return Err("unsupported LC3 frame duration".into()),
    };
    let default_octets = subgroup
        .codec_specific_configuration
        .octets_per_codec_frame
        .ok_or_else(|| "subgroup omits LC3 octets per codec frame".to_string())?;
    let default_frames = subgroup
        .codec_specific_configuration
        .codec_frames_per_sdu
        .unwrap_or(1);
    let mut octets = None;
    let mut frames = None;
    let bis = subgroup
        .bis
        .iter()
        .map(|entry| {
            let entry_octets = entry
                .codec_specific_configuration
                .octets_per_codec_frame
                .unwrap_or(default_octets);
            let entry_frames = entry
                .codec_specific_configuration
                .codec_frames_per_sdu
                .unwrap_or(default_frames);
            if octets
                .replace(entry_octets)
                .is_some_and(|value| value != entry_octets)
                || frames
                    .replace(entry_frames)
                    .is_some_and(|value| value != entry_frames)
            {
                return Err("selected BIS entries use unequal LC3 frame sizes".to_string());
            }
            Ok(entry.index)
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok((
        Lc3StreamConfig {
            sampling_frequency,
            frame_duration,
            channels: bis.len(),
            octets_per_codec_frame: usize::from(octets.unwrap_or(default_octets)),
            codec_frames_per_sdu: usize::from(frames.unwrap_or(default_frames)),
        },
        bis,
    ))
}

fn pcm_i16_to_float32(samples: &[i16]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(samples.len() * 4);
    for sample in samples {
        let value = f32::from(*sample) / 32_768.0;
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes
}

fn run_receive(
    transport: &str,
    wanted_broadcast_id: Option<u32>,
    output_spec: &str,
    broadcast_code: Option<&str>,
    sync_timeout: f64,
    subgroup_index: usize,
) -> Result<(), String> {
    let mut output = create_audio_output(output_spec).map_err(|error| error.to_string())?;
    let (mut host, mut device, _) = open_device(transport)?;
    let mut scanner = BroadcastScanner::new(false, sync_timeout)?;
    scanner.start(&mut host, &mut device);
    println!("Scanning for broadcast...");
    let index = wait_for_ready_broadcast(
        &mut host,
        &mut device,
        &mut scanner,
        wanted_broadcast_id,
        None,
    )?;
    scanner.stop(&mut host, &mut device);
    let broadcast = scanner.broadcasts[index].clone();
    print_broadcast(&broadcast);
    let subgroup = broadcast
        .basic_audio_announcement
        .as_ref()
        .and_then(|announcement| announcement.subgroups.get(subgroup_index))
        .ok_or_else(|| format!("broadcast has no subgroup {subgroup_index}"))?;
    let (stream_config, bis) = merged_stream_config(subgroup)?;
    let decoder = Lc3Decoder::new(stream_config).map_err(|error| error.to_string())?;
    let sync_handle = broadcast
        .sync_handle
        .ok_or_else(|| "broadcast periodic sync was lost".to_string())?;
    let mut parameters = BigSyncParameters::new(0, sync_handle, bis);
    parameters.broadcast_code = broadcast_code.map(broadcast_code_bytes).transpose()?;
    if !device.create_big_sync(&mut host, parameters) {
        return Err("controller rejected BIG sync parameters".into());
    }
    let bis_handles = wait_for_big(&mut host, &mut device, 0, true)?;
    for handle in &bis_handles {
        if !device.setup_iso_data_path(&mut host, *handle, 1) {
            return Err(format!(
                "failed to set up receive path for BIS 0x{handle:04X}"
            ));
        }
    }
    output
        .open(PcmFormat::new(
            Endianness::Little,
            SampleType::Float32,
            stream_config.sampling_frequency,
            u16::try_from(stream_config.channels)
                .map_err(|_| "channel count exceeds 65535".to_string())?,
        ))
        .map_err(|error| error.to_string())?;
    let mut queues = vec![VecDeque::<Vec<u8>>::new(); bis_handles.len()];
    let mut byte_count = 0usize;
    let mut packet_count = 0usize;
    println!("Receiving {} BIS channel(s)", bis_handles.len());
    loop {
        device.poll(&mut host);
        for (index, handle) in bis_handles.iter().enumerate() {
            for sdu in device.take_iso_sdus(*handle) {
                if sdu.packet_status_flag == 0 {
                    queues[index].push_back(sdu.data);
                }
            }
        }
        while queues.iter().all(|queue| !queue.is_empty()) {
            let encoded = queues
                .iter_mut()
                .flat_map(|queue| queue.pop_front().expect("queue is nonempty"))
                .collect::<Vec<_>>();
            byte_count += encoded.len();
            packet_count += 1;
            let pcm = decoder
                .decode_sdu(&encoded)
                .map_err(|error| error.to_string())?;
            output
                .write(&pcm_i16_to_float32(&pcm))
                .map_err(|error| error.to_string())?;
            eprint!("\rRECEIVED: {byte_count} bytes in {packet_count} packets");
        }
        if device
            .take_terminated_bigs()
            .into_iter()
            .any(|(handle, _)| handle == 0)
        {
            eprintln!();
            output.close().map_err(|error| error.to_string())?;
            return Ok(());
        }
        if !wait_for_activity(&mut host, &mut device, Duration::from_millis(250))? {
            eprintln!();
            output.close().map_err(|error| error.to_string())?;
            return Ok(());
        }
    }
}

fn inline_broadcast_config(
    input: String,
    mut input_format: String,
    broadcast_id: u32,
    broadcast_code: Option<String>,
    broadcast_name: String,
    bitrate: u32,
    manufacturer_data: Option<ManufacturerData>,
) -> BroadcastConfig {
    if (input == "device" || input.starts_with("device:")) && input_format == "auto" {
        input_format = "int16le,48000,1".into();
    }
    BroadcastConfig {
        sources: vec![BroadcastSource {
            input,
            input_format,
            bitrate,
        }],
        public: true,
        broadcast_id,
        broadcast_name,
        broadcast_code,
        manufacturer_data,
        language: Some(DEFAULT_LANGUAGE.into()),
        program_info: Some(DEFAULT_PROGRAM_INFO.into()),
    }
}

fn run(args: Args) -> Result<(), String> {
    match args {
        Args::Help => {
            println!("{}", usage());
            Ok(())
        }
        Args::Scan {
            filter_duplicates,
            sync_timeout,
            transport,
        } => run_scan(&transport, filter_duplicates, sync_timeout),
        Args::Assist {
            broadcast_name,
            source_id,
            command,
            transport,
            address,
        } => run_assist(
            &transport,
            &address,
            broadcast_name.as_deref(),
            source_id,
            command,
        ),
        Args::Pair { transport, address } => run_pair(&transport, &address),
        Args::Receive {
            transport,
            broadcast_id,
            output,
            broadcast_code,
            sync_timeout,
            subgroup,
        } => run_receive(
            &transport,
            broadcast_id,
            &output,
            broadcast_code.as_deref(),
            sync_timeout,
            subgroup,
        ),
        Args::Transmit {
            transport,
            broadcast_list,
            input,
            input_format,
            broadcast_id,
            broadcast_code,
            broadcast_name,
            bitrate,
            manufacturer_data,
        } => {
            let mut configs = if let Some(path) = broadcast_list {
                parse_broadcast_list(&path)?
            } else {
                vec![inline_broadcast_config(
                    input.expect("validated input"),
                    input_format,
                    broadcast_id,
                    broadcast_code,
                    broadcast_name,
                    bitrate,
                    manufacturer_data,
                )]
            };
            assign_broadcast_ids(&mut configs)?;
            run_transmit(&transport, configs)
        }
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
    fn parses_all_upstream_command_shapes() {
        assert_eq!(
            parse_args([
                "auracast",
                "scan",
                "--filter-duplicates",
                "--sync-timeout",
                "2.5",
                "usb:0",
            ])
            .unwrap(),
            Args::Scan {
                filter_duplicates: true,
                sync_timeout: 2.5,
                transport: "usb:0".into(),
            }
        );
        assert_eq!(
            parse_args([
                "auracast",
                "assist",
                "--broadcast-name",
                "Radio",
                "--source-id",
                "7",
                "--command",
                "modify-source",
                "tcp-client:127.0.0.1:6402",
                "C4:F2:17:1A:1D:BB",
            ])
            .unwrap(),
            Args::Assist {
                broadcast_name: Some("Radio".into()),
                source_id: Some(7),
                command: AssistCommand::ModifySource,
                transport: "tcp-client:127.0.0.1:6402".into(),
                address: "C4:F2:17:1A:1D:BB".into(),
            }
        );
        assert_eq!(
            parse_args([
                "auracast",
                "receive",
                "--output",
                "file:out.pcm",
                "--broadcast-code",
                "secret",
                "--subgroup",
                "1",
                "usb:0",
                "123456",
            ])
            .unwrap(),
            Args::Receive {
                transport: "usb:0".into(),
                broadcast_id: Some(123_456),
                output: "file:out.pcm".into(),
                broadcast_code: Some("secret".into()),
                sync_timeout: DEFAULT_SYNC_TIMEOUT,
                subgroup: 1,
            }
        );
        assert_eq!(
            parse_args([
                "auracast",
                "transmit",
                "--input",
                "music.wav",
                "--broadcast-id",
                "42",
                "--manufacturer-data",
                "76:0102a0",
                "usb:0",
            ])
            .unwrap(),
            Args::Transmit {
                transport: "usb:0".into(),
                broadcast_list: None,
                input: Some("music.wav".into()),
                input_format: "auto".into(),
                broadcast_id: 42,
                broadcast_code: None,
                broadcast_name: DEFAULT_BROADCAST_NAME.into(),
                bitrate: DEFAULT_BITRATE,
                manufacturer_data: Some(ManufacturerData {
                    company_id: 76,
                    data: vec![1, 2, 0xA0],
                }),
            }
        );
        assert_eq!(parse_args(["auracast", "--help"]).unwrap(), Args::Help);
        assert_eq!(
            parse_args(["auracast", "pair", "usb:0", "11:22:33:44:55:66"]).unwrap(),
            Args::Pair {
                transport: "usb:0".into(),
                address: "11:22:33:44:55:66".into(),
            }
        );
    }

    #[test]
    fn validates_broadcast_codes_like_upstream() {
        assert_eq!(
            broadcast_code_bytes("hello").unwrap(),
            [b'h', b'e', b'l', b'l', b'o', 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]
        );
        assert_eq!(
            broadcast_code_bytes("0x000102030405060708090a0b0c0d0e0f").unwrap(),
            [15, 14, 13, 12, 11, 10, 9, 8, 7, 6, 5, 4, 3, 2, 1, 0]
        );
        assert!(broadcast_code_bytes("12345678901234567").is_err());
        assert!(broadcast_code_bytes("0xnot-a-valid-16-byte-broadcast-code").is_err());
    }

    #[test]
    fn parses_toml_broadcast_lists_and_assigns_zero_ids() {
        let path = std::env::temp_dir().join(format!(
            "bumble-auracast-{}-{}.toml",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::write(
            &path,
            r#"
                [[broadcasts]]
                name = "News"
                id = 0
                code = "secret"
                language = "en"
                program_info = "Headlines"

                [broadcasts.manufacturer_data]
                company_id = 76
                data = "0102ff"

                [[broadcasts.sources]]
                input = "news.wav"
                bitrate = 80000

                [[broadcasts]]
                name = "Music"
                id = 99
                public = false

                [[broadcasts.sources]]
                input = "stdin"
                format = "float32le,48000,2"
                bitrate = 96000
            "#,
        )
        .unwrap();
        let broadcasts = parse_broadcast_list(path.to_str().unwrap()).unwrap();
        fs::remove_file(path).unwrap();
        assert_eq!(broadcasts.len(), 2);
        assert_ne!(broadcasts[0].broadcast_id, 0);
        assert_eq!(broadcasts[1].broadcast_id, 99);
        assert_eq!(broadcasts[0].sources[0].input_format, "auto");
        assert_eq!(broadcasts[1].sources[0].bitrate, 96_000);
        assert_eq!(
            broadcasts[0].manufacturer_data,
            Some(ManufacturerData {
                company_id: 76,
                data: vec![1, 2, 0xFF],
            })
        );
    }

    #[test]
    fn extracts_service_data_and_builds_receive_lc3_config() {
        let announcement = BroadcastAudioAnnouncement::new(0x12_3456).unwrap();
        let bytes = announcement.advertising_data().unwrap();
        let advertising = AdvertisingData::from_bytes(&bytes);
        assert_eq!(
            service_data(&advertising, BROADCAST_AUDIO_ANNOUNCEMENT_SERVICE),
            Some(vec![0x56, 0x34, 0x12])
        );

        let subgroup = BasicAudioSubgroup {
            codec_id: CodingFormat::LC3,
            codec_specific_configuration: CodecSpecificConfiguration {
                sampling_frequency: Some(SamplingFrequency::FREQ_48000),
                frame_duration: Some(FrameDuration::DURATION_10000_US),
                audio_channel_allocation: None,
                octets_per_codec_frame: Some(100),
                codec_frames_per_sdu: Some(1),
            },
            metadata: Metadata::default(),
            bis: vec![
                BasicAudioBis {
                    index: 1,
                    codec_specific_configuration: CodecSpecificConfiguration::default(),
                },
                BasicAudioBis {
                    index: 2,
                    codec_specific_configuration: CodecSpecificConfiguration::default(),
                },
            ],
        };
        let (config, bis) = merged_stream_config(&subgroup).unwrap();
        assert_eq!(config.sampling_frequency, 48_000);
        assert_eq!(config.channels, 2);
        assert_eq!(config.octets_per_codec_frame, 100);
        assert_eq!(bis, vec![1, 2]);
    }

    #[test]
    fn rejects_invalid_cli_and_duplicate_ids() {
        assert!(parse_args(["auracast", "transmit", "usb:0"]).is_err());
        assert!(parse_args([
            "auracast",
            "assist",
            "--command",
            "remove-source",
            "usb:0",
            "11:22:33:44:55:66",
        ])
        .is_err());
        let config = inline_broadcast_config(
            "stdin".into(),
            "int16le,48000,1".into(),
            7,
            None,
            "one".into(),
            DEFAULT_BITRATE,
            None,
        );
        let mut configs = vec![config.clone(), config];
        assert!(assign_broadcast_ids(&mut configs).is_err());
    }

    #[test]
    fn scanner_correlates_extended_periodic_and_biginfo_reports() {
        use bumble_controller::{Controller, LocalLink};
        use bumble_host::pump;

        let source_address =
            Address::parse("C4:F2:17:1A:1D:D0", AddressType::RANDOM_DEVICE).unwrap();
        let receiver_address =
            Address::parse("C4:F2:17:1A:1D:D1", AddressType::RANDOM_DEVICE).unwrap();
        let mut link = LocalLink::new();
        let source_id = link.add_controller(Controller::new("source", source_address.clone()));
        let receiver_id = link.add_controller(Controller::new("receiver", receiver_address));
        let mut devices = [Device::new(source_id), Device::new(receiver_id)];

        let broadcast_id = 0x12_3456;
        let mut extended_data = AdvertisingData {
            ad_structures: vec![
                (
                    AdvertisingDataType::COMPLETE_LOCAL_NAME,
                    DEFAULT_DEVICE_NAME.as_bytes().to_vec(),
                ),
                (
                    AdvertisingDataType::BROADCAST_NAME,
                    b"Integration Radio".to_vec(),
                ),
            ],
        }
        .to_bytes();
        extended_data.extend_from_slice(
            &BroadcastAudioAnnouncement::new(broadcast_id)
                .unwrap()
                .advertising_data()
                .unwrap(),
        );
        let basic = BasicAudioAnnouncement {
            presentation_delay: 40_000,
            subgroups: vec![BasicAudioSubgroup {
                codec_id: CodingFormat::LC3,
                codec_specific_configuration: CodecSpecificConfiguration {
                    sampling_frequency: Some(SamplingFrequency::FREQ_48000),
                    frame_duration: Some(FrameDuration::DURATION_10000_US),
                    audio_channel_allocation: None,
                    octets_per_codec_frame: Some(100),
                    codec_frames_per_sdu: Some(1),
                },
                metadata: Metadata::default(),
                bis: vec![BasicAudioBis {
                    index: 1,
                    codec_specific_configuration: CodecSpecificConfiguration::default(),
                }],
            }],
        };
        let mut extended =
            ExtendedAdvertisingConfig::connectable_scannable(4, source_address.clone());
        extended.event_properties = 0;
        extended.sid = 7;
        assert!(devices[0].start_extended_advertising(&mut link, &extended, &extended_data, &[],));
        assert!(devices[0].start_periodic_advertising(
            &mut link,
            PeriodicAdvertisingConfig::new(4),
            &basic.advertising_data().unwrap(),
        ));
        let mut big = BigParameters::new(1, 4, 1);
        big.max_sdu = 100;
        assert!(devices[0].create_big(&mut link, big));
        pump(&mut link, &mut devices);

        let mut scanner = BroadcastScanner::new(false, 5.0).unwrap();
        scanner.start(&mut link, &mut devices[1]);
        link.propagate_advertising();
        pump(&mut link, &mut devices);
        scanner.poll(&mut link, &mut devices[1]).unwrap();
        assert_eq!(scanner.broadcasts.len(), 1);
        assert_eq!(scanner.broadcasts[0].broadcast_id, broadcast_id);
        assert_eq!(
            scanner.broadcasts[0].name.as_deref(),
            Some("Integration Radio")
        );

        link.propagate_advertising();
        pump(&mut link, &mut devices);
        scanner.poll(&mut link, &mut devices[1]).unwrap();
        let discovered = &scanner.broadcasts[0];
        assert!(discovered.ready());
        assert_eq!(
            discovered
                .basic_audio_announcement
                .as_ref()
                .unwrap()
                .subgroups[0]
                .bis[0]
                .index,
            1
        );
        assert_eq!(discovered.biginfo.as_ref().unwrap().max_sdu, 100);
    }
}
