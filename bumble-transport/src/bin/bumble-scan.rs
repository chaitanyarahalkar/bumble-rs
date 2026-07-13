use bumble::keys::{JsonKeyStore, KeyStore};
use bumble::{Address, AddressType, AdvertisingData, DataType};
use bumble_hci::{
    AdvertisingReport, Command, Event, ExtendedAdvertisingReport, HciPacket, LeMetaEvent,
};
use bumble_smp::AddressResolver;
use bumble_transport::{
    open_transport, CommandResponse, HciCommandChannel, PacketSink, PacketSource,
};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const DEFAULT_ADDRESS: &str = "F0:F1:F2:F3:F4:F5";
const HOST_EVENT_MASK: [u8; 8] = [0xFF, 0x9F, 0xFF, 0xBF, 0x07, 0xF8, 0xBF, 0x3D];
const HOST_LE_EVENT_MASK: [u8; 8] = [0xFF, 0xFF, 0xF7, 0xFF, 0x0F, 0xED, 0x7B, 0x00];
const LEGACY_LE_EVENT_MASK: [u8; 8] = [0x1F, 0, 0, 0, 0, 0, 0, 0];
const DISPLAY_MIN_RSSI: i16 = -105;
const DISPLAY_MAX_RSSI: i16 = -30;
const RSSI_BAR_WIDTH: i16 = 30;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phy {
    OneM,
    Coded,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Args {
    min_rssi: Option<i8>,
    passive: bool,
    scan_interval_ms: u16,
    scan_window_ms: u16,
    phy: Option<Phy>,
    filter_duplicates: bool,
    raw: bool,
    irks: Vec<String>,
    keystore_file: Option<PathBuf>,
    device_config: Option<PathBuf>,
    transport: String,
}

fn usage() -> &'static str {
    "usage: bumble-scan [--min-rssi RSSI] [--passive] [--scan-interval MS] [--scan-window MS] [--phy 1m|coded] [--filter-duplicates true|false] [--raw] [--irk IRK_HEX:ADDRESS] [--keystore-file PATH] [--device-config PATH] <transport>"
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

fn parse_args(arguments: impl IntoIterator<Item = String>) -> Result<Args, String> {
    let mut arguments = arguments.into_iter();
    let _program = arguments.next();
    let mut min_rssi = None;
    let mut passive = false;
    let mut scan_interval_ms = 60;
    let mut scan_window_ms = 60;
    let mut phy = None;
    let mut filter_duplicates = true;
    let mut raw = false;
    let mut irks = Vec::new();
    let mut keystore_file = None;
    let mut device_config = None;
    let mut transport = None;

    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "-h" | "--help" => return Err(usage().into()),
            "--passive" => {
                passive = true;
                continue;
            }
            "--raw" => {
                raw = true;
                continue;
            }
            _ => {}
        }
        if let Some(value) = option_value(&argument, "--min-rssi", &mut arguments)? {
            min_rssi = Some(
                value
                    .parse()
                    .map_err(|_| "minimum RSSI must be an 8-bit integer".to_string())?,
            );
            continue;
        }
        if let Some(value) = option_value(&argument, "--scan-interval", &mut arguments)? {
            scan_interval_ms = value
                .parse()
                .map_err(|_| "scan interval must be an integer".to_string())?;
            continue;
        }
        if let Some(value) = option_value(&argument, "--scan-window", &mut arguments)? {
            scan_window_ms = value
                .parse()
                .map_err(|_| "scan window must be an integer".to_string())?;
            continue;
        }
        if let Some(value) = option_value(&argument, "--phy", &mut arguments)? {
            phy = Some(match value.as_str() {
                "1m" => Phy::OneM,
                "coded" => Phy::Coded,
                _ => return Err("PHY must be 1m or coded".into()),
            });
            continue;
        }
        if let Some(value) = option_value(&argument, "--filter-duplicates", &mut arguments)? {
            filter_duplicates = parse_bool(&value, "--filter-duplicates")?;
            continue;
        }
        if let Some(value) = option_value(&argument, "--irk", &mut arguments)? {
            irks.push(value);
            continue;
        }
        if let Some(value) = option_value(&argument, "--keystore-file", &mut arguments)? {
            keystore_file = Some(PathBuf::from(value));
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
            return Err("only one transport may be specified".into());
        }
    }
    if scan_interval_ms < scan_window_ms {
        return Err("scan interval must be greater than or equal to scan window".into());
    }
    if scan_interval_ms == 0 || scan_window_ms == 0 {
        return Err("scan interval and window must be positive".into());
    }
    Ok(Args {
        min_rssi,
        passive,
        scan_interval_ms,
        scan_window_ms,
        phy,
        filter_duplicates,
        raw,
        irks,
        keystore_file,
        device_config,
        transport: transport.ok_or_else(|| "missing transport".to_string())?,
    })
}

fn decode_hex(value: &str) -> Result<Vec<u8>, String> {
    let value: String = value
        .chars()
        .filter(|character| !character.is_ascii_whitespace())
        .collect();
    if !value.len().is_multiple_of(2) || !value.is_ascii() {
        return Err("hex value must contain complete bytes".into());
    }
    value
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let pair = std::str::from_utf8(pair).expect("ASCII checked");
            u8::from_str_radix(pair, 16).map_err(|_| "invalid hexadecimal digit".to_string())
        })
        .collect()
}

fn configured_address(path: Option<&Path>) -> Result<Address, String> {
    let address = match path {
        Some(path) => {
            let bytes = std::fs::read(path)
                .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
            let config: serde_json::Value = serde_json::from_slice(&bytes)
                .map_err(|error| format!("invalid device config: {error}"))?;
            config
                .get("address")
                .and_then(serde_json::Value::as_str)
                .unwrap_or(DEFAULT_ADDRESS)
                .to_owned()
        }
        None => DEFAULT_ADDRESS.into(),
    };
    Address::parse(&address, AddressType::RANDOM_DEVICE).map_err(|error| error.to_string())
}

fn parse_irk(value: &str) -> Result<(Vec<u8>, Address), String> {
    let (irk, address) = value
        .split_once(':')
        .ok_or_else(|| "IRK must use IRK_HEX:ADDRESS syntax".to_string())?;
    let irk = decode_hex(irk)?;
    if irk.len() != 16 {
        return Err("IRK must contain exactly 16 bytes".into());
    }
    let address = Address::parse(address, AddressType::RANDOM_DEVICE)
        .map_err(|error| format!("invalid IRK identity address: {error}"))?;
    Ok((irk, address))
}

fn load_resolver(args: &Args, local_address: &Address) -> Result<Option<AddressResolver>, String> {
    let mut keys = Vec::new();
    if let Some(path) = &args.keystore_file {
        let namespace = local_address.to_string(false);
        keys.extend(
            JsonKeyStore::new(Some(&namespace), path)
                .get_resolving_keys()
                .map_err(|error| error.to_string())?,
        );
    }
    for value in &args.irks {
        keys.push(parse_irk(value)?);
    }
    Ok((!keys.is_empty()).then(|| AddressResolver::new(keys)))
}

fn response_succeeded(response: &CommandResponse) -> bool {
    response.status() == Some(0)
}

fn require_success<T: PacketSource + PacketSink>(
    channel: &mut HciCommandChannel<T>,
    command: Command,
) -> Result<(), String> {
    let opcode = command.op_code();
    let response = channel
        .send_command(command)
        .map_err(|error| error.to_string())?;
    if response_succeeded(&response) {
        Ok(())
    } else {
        Err(format!(
            "HCI command {opcode:#06x} failed with status {:?}",
            response.status()
        ))
    }
}

fn scan_units(milliseconds: u16) -> u16 {
    ((u32::from(milliseconds) * 8) / 5) as u16
}

fn configure_scan<T: PacketSource + PacketSink>(
    channel: &mut HciCommandChannel<T>,
    args: &Args,
    local_address: Address,
) -> Result<(), String> {
    require_success(channel, Command::Reset)?;
    require_success(
        channel,
        Command::SetEventMask {
            event_mask: HOST_EVENT_MASK,
        },
    )?;
    let le_event_mask = channel
        .send_command(Command::LeSetEventMask {
            le_event_mask: HOST_LE_EVENT_MASK,
        })
        .map_err(|error| error.to_string())?;
    if !response_succeeded(&le_event_mask) {
        require_success(
            channel,
            Command::LeSetEventMask {
                le_event_mask: LEGACY_LE_EVENT_MASK,
            },
        )?;
    }
    require_success(
        channel,
        Command::LeSetRandomAddress {
            random_address: local_address,
        },
    )?;

    let (scanning_phys, phy_count) = match args.phy {
        Some(Phy::OneM) => (0x01, 1),
        Some(Phy::Coded) => (0x04, 1),
        None => (0x05, 2),
    };
    let interval = scan_units(args.scan_interval_ms);
    let window = scan_units(args.scan_window_ms);
    let extended = channel
        .send_command(Command::LeSetExtendedScanParameters {
            own_address_type: 1,
            scanning_filter_policy: 0,
            scanning_phys,
            scan_types: vec![u8::from(!args.passive); phy_count],
            scan_intervals: vec![interval; phy_count],
            scan_windows: vec![window; phy_count],
        })
        .map_err(|error| error.to_string())?;
    if response_succeeded(&extended) {
        return require_success(
            channel,
            Command::LeSetExtendedScanEnable {
                enable: 1,
                filter_duplicates: u8::from(args.filter_duplicates),
                duration: 0,
                period: 0,
            },
        );
    }
    if args.phy == Some(Phy::Coded) {
        return Err("controller does not support coded-PHY extended scanning".into());
    }
    require_success(
        channel,
        Command::LeSetScanParameters {
            le_scan_type: u8::from(!args.passive),
            le_scan_interval: interval,
            le_scan_window: window,
            own_address_type: 1,
            scanning_filter_policy: 0,
        },
    )?;
    require_success(
        channel,
        Command::LeSetScanEnable {
            le_scan_enable: 1,
            filter_duplicates: u8::from(args.filter_duplicates),
        },
    )
}

fn make_rssi_bar(rssi: i8) -> String {
    const BLOCKS: [&str; 8] = ["", "▏", "▎", "▍", "▌", "▋", "▊", "▉"];
    let clamped = i16::from(rssi).clamp(DISPLAY_MIN_RSSI, DISPLAY_MAX_RSSI);
    let ticks =
        (clamped - DISPLAY_MIN_RSSI) * RSSI_BAR_WIDTH * 8 / (DISPLAY_MAX_RSSI - DISPLAY_MIN_RSSI);
    format!(
        "{}{}",
        "█".repeat((ticks / 8) as usize),
        BLOCKS[(ticks % 8) as usize]
    )
}

fn address_type_name(address: &Address) -> &'static str {
    match address.address_type() {
        AddressType::PUBLIC_DEVICE => "PUBLIC",
        AddressType::RANDOM_DEVICE => "RANDOM",
        AddressType::PUBLIC_IDENTITY => "PUBLIC_ID",
        AddressType::RANDOM_IDENTITY => "RANDOM_ID",
        _ => "UNKNOWN",
    }
}

fn phy_name(phy: u8) -> &'static str {
    match phy {
        0 => "NONE",
        1 => "1M",
        2 => "2M",
        3 => "CODED",
        _ => "UNKNOWN",
    }
}

fn bytes_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn data_type_label(value: DataType) -> String {
    use DataType::*;
    match value {
        Flags(flags) => format!("Flags: {flags:#x}"),
        IncompleteListOf16BitServiceUuids(uuids) => {
            format!("Incomplete List of 16-bit Service UUIDs: {uuids:?}")
        }
        CompleteListOf16BitServiceUuids(uuids) => {
            format!("Complete List of 16-bit Service UUIDs: {uuids:?}")
        }
        IncompleteListOf32BitServiceUuids(uuids) => {
            format!("Incomplete List of 32-bit Service UUIDs: {uuids:?}")
        }
        CompleteListOf32BitServiceUuids(uuids) => {
            format!("Complete List of 32-bit Service UUIDs: {uuids:?}")
        }
        IncompleteListOf128BitServiceUuids(uuids) => {
            format!("Incomplete List of 128-bit Service UUIDs: {uuids:?}")
        }
        CompleteListOf128BitServiceUuids(uuids) => {
            format!("Complete List of 128-bit Service UUIDs: {uuids:?}")
        }
        ShortenedLocalName(name) => format!("Shortened Local Name: {name}"),
        CompleteLocalName(name) => format!("Complete Local Name: {name}"),
        TxPowerLevel(power) => format!("TX Power Level: {power} dBm"),
        ClassOfDevice(class) => format!("Class of Device: {class:?}"),
        ManufacturerSpecificData {
            company_identifier,
            data,
        } => {
            let company = bumble::company_name(company_identifier)
                .map(str::to_owned)
                .unwrap_or_else(|| format!("{company_identifier:#06x}"));
            format!(
                "Manufacturer Specific Data: {company}: {}",
                bytes_hex(&data)
            )
        }
        SimplePairingHashC192(value) => {
            format!("Simple Pairing Hash C-192: {}", bytes_hex(&value))
        }
        SimplePairingRandomizerR192(value) => {
            format!("Simple Pairing Randomizer R-192: {}", bytes_hex(&value))
        }
        SimplePairingHashC256(value) => {
            format!("Simple Pairing Hash C-256: {}", bytes_hex(&value))
        }
        SimplePairingRandomizerR256(value) => {
            format!("Simple Pairing Randomizer R-256: {}", bytes_hex(&value))
        }
        LeSecureConnectionsConfirmationValue(value) => format!(
            "LE Secure Connections Confirmation Value: {}",
            bytes_hex(&value)
        ),
        LeSecureConnectionsRandomValue(value) => {
            format!("LE Secure Connections Random Value: {}", bytes_hex(&value))
        }
        SecurityManagerTkValue(value) => {
            format!("Security Manager TK Value: {}", bytes_hex(&value))
        }
        SecurityManagerOutOfBandFlags(flags) => {
            format!("Security Manager OOB Flags: {flags:#04x}")
        }
        PeripheralConnectionIntervalRange { min, max } => {
            format!("Peripheral Connection Interval Range: {min}..{max}")
        }
        ListOf16BitServiceSolicitationUuids(uuids) => {
            format!("List of 16-bit Service Solicitation UUIDs: {uuids:?}")
        }
        ListOf32BitServiceSolicitationUuids(uuids) => {
            format!("List of 32-bit Service Solicitation UUIDs: {uuids:?}")
        }
        ListOf128BitServiceSolicitationUuids(uuids) => {
            format!("List of 128-bit Service Solicitation UUIDs: {uuids:?}")
        }
        ServiceData16BitUuid { service_uuid, data } => format!(
            "Service Data 16-bit UUID: {}: {}",
            service_uuid.to_hex_str("-"),
            bytes_hex(&data)
        ),
        ServiceData32BitUuid { service_uuid, data } => format!(
            "Service Data 32-bit UUID: {}: {}",
            service_uuid.to_hex_str("-"),
            bytes_hex(&data)
        ),
        ServiceData128BitUuid { service_uuid, data } => format!(
            "Service Data 128-bit UUID: {}: {}",
            service_uuid.to_hex_str("-"),
            bytes_hex(&data)
        ),
        PublicTargetAddress(address) => format!("Public Target Address: {address}"),
        RandomTargetAddress(address) => format!("Random Target Address: {address}"),
        Appearance(appearance) => format!("Appearance: {appearance:?}"),
        AdvertisingInterval(interval) => format!("Advertising Interval: {interval}"),
        LeBluetoothDeviceAddress(address) => {
            format!("LE Bluetooth Device Address: {address}")
        }
        LeRole(role) => format!("LE Role: {role:#04x}"),
        Uri(uri) => format!("URI: {uri}"),
        LeSupportedFeatures(features) => format!("LE Supported Features: {features:#x}"),
        ChannelMapUpdateIndication { chm, instant } => {
            format!("Channel Map Update Indication: map={chm:#x}, instant={instant}")
        }
        AdvertisingIntervalLong(interval) => {
            format!("Advertising Interval Long: {interval}")
        }
        BroadcastCode(code) => format!("Broadcast Code: {code}"),
        BroadcastName(name) => format!("Broadcast Name: {name}"),
        ResolvableSetIdentifier(identifier) => {
            format!("Resolvable Set Identifier: {}", bytes_hex(&identifier))
        }
        Generic { ad_type, data } => {
            format!("AD Type {ad_type:#04x}: {}", bytes_hex(&data))
        }
    }
}

fn advertising_details(data: &[u8]) -> String {
    let values = AdvertisingData::from_bytes(data).data_types();
    if values.is_empty() {
        return "<no advertising data>".into();
    }
    values
        .into_iter()
        .map(data_type_label)
        .collect::<Vec<_>>()
        .join("\n  ")
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ProcessedAdvertisement {
    event: String,
    address: Address,
    data: Vec<u8>,
    rssi: i8,
    connectable: bool,
    scannable: bool,
    scan_response: bool,
    primary_phy: Option<u8>,
    secondary_phy: Option<u8>,
}

fn render_advertisement(
    view: &ProcessedAdvertisement,
    raw: bool,
    resolver: Option<&AddressResolver>,
) -> String {
    let mut address = view.address.clone();
    let resolution = resolver
        .and_then(|resolver| resolver.resolve(&view.address))
        .map(|resolved| {
            address = resolved;
            format!(" (resolved from {})", view.address.to_string(false))
        })
        .unwrap_or_default();
    let qualifier = if address.is_resolved() {
        ""
    } else if address.is_static() {
        " (static)"
    } else if address.is_resolvable() {
        " (resolvable)"
    } else if address.is_random() {
        " (non-resolvable)"
    } else {
        ""
    };
    let phy = match (view.primary_phy, view.secondary_phy) {
        (Some(primary), Some(secondary)) => {
            format!("  PHY: {}/{}\n", phy_name(primary), phy_name(secondary))
        }
        _ => String::new(),
    };
    let raw_event = if raw {
        format!("EVENT: {}\n", view.event)
    } else {
        String::new()
    };
    format!(
        "{raw_event}>>> {} [{}]{}{}{}:\n{phy}  RSSI:{:4} {}\n  {}",
        address.to_string(false),
        address_type_name(&address),
        qualifier,
        resolution,
        if view.connectable {
            " (connectable)"
        } else {
            ""
        },
        view.rssi,
        make_rssi_bar(view.rssi),
        advertising_details(&view.data)
    )
}

fn legacy_event_name(event_type: u8) -> &'static str {
    match event_type {
        0 => "ADV_IND",
        1 => "ADV_DIRECT_IND",
        2 => "ADV_SCAN_IND",
        3 => "ADV_NONCONN_IND",
        4 => "SCAN_RSP",
        _ => "UNKNOWN",
    }
}

fn processed_legacy(report: &AdvertisingReport) -> ProcessedAdvertisement {
    ProcessedAdvertisement {
        event: legacy_event_name(report.event_type).into(),
        address: report.address.clone(),
        data: report.data.clone(),
        rssi: report.rssi,
        connectable: matches!(report.event_type, 0 | 1),
        scannable: matches!(report.event_type, 0 | 2),
        scan_response: report.event_type == 4,
        primary_phy: None,
        secondary_phy: None,
    }
}

fn processed_extended(report: &ExtendedAdvertisingReport) -> ProcessedAdvertisement {
    ProcessedAdvertisement {
        event: format!("EXTENDED({:#06x})", report.event_type),
        address: report.address.clone(),
        data: report.data.clone(),
        rssi: report.rssi,
        connectable: report.event_type & 1 != 0,
        scannable: report.event_type & 2 != 0,
        scan_response: report.event_type & 8 != 0,
        primary_phy: Some(report.primary_phy),
        secondary_phy: Some(report.secondary_phy),
    }
}

fn packet_advertisements(packet: &HciPacket) -> Vec<ProcessedAdvertisement> {
    match packet {
        HciPacket::Event(Event::LeMeta(LeMetaEvent::AdvertisingReport { reports })) => {
            reports.iter().map(processed_legacy).collect()
        }
        HciPacket::Event(Event::LeMeta(LeMetaEvent::ExtendedAdvertisingReport { reports })) => {
            reports.iter().map(processed_extended).collect()
        }
        _ => Vec::new(),
    }
}

#[derive(Clone, Debug)]
struct LastAdvertisement {
    advertisement: ProcessedAdvertisement,
    data: Vec<u8>,
}

#[derive(Clone, Debug)]
struct AdvertisementAccumulator {
    passive: bool,
    entries: Vec<(Address, LastAdvertisement)>,
}

impl AdvertisementAccumulator {
    fn new(passive: bool) -> Self {
        Self {
            passive,
            entries: Vec::new(),
        }
    }

    fn update(&mut self, advertisement: ProcessedAdvertisement) -> Option<ProcessedAdvertisement> {
        let index = self
            .entries
            .iter()
            .position(|(address, _)| *address == advertisement.address);
        let last = index.map(|index| self.entries[index].1.clone());
        let (result, data) = if advertisement.scan_response {
            let result = last.as_ref().and_then(|last| {
                (!last.advertisement.scan_response).then(|| {
                    let mut merged = advertisement.clone();
                    merged.connectable = last.advertisement.connectable;
                    merged.scannable = true;
                    merged.data = last
                        .data
                        .iter()
                        .chain(&advertisement.data)
                        .copied()
                        .collect();
                    merged
                })
            });
            (result, Vec::new())
        } else {
            let emit = self.passive
                || !advertisement.scannable
                || last
                    .as_ref()
                    .is_some_and(|last| !last.advertisement.scan_response);
            (
                emit.then(|| advertisement.clone()),
                advertisement.data.clone(),
            )
        };
        let replacement = LastAdvertisement {
            advertisement,
            data,
        };
        match index {
            Some(index) => self.entries[index].1 = replacement,
            None => self
                .entries
                .push((replacement.advertisement.address.clone(), replacement)),
        }
        result
    }
}

struct AdvertisementProcessor<'a> {
    raw: bool,
    min_rssi: Option<i8>,
    resolver: Option<&'a AddressResolver>,
    accumulator: AdvertisementAccumulator,
}

impl<'a> AdvertisementProcessor<'a> {
    fn new(args: &Args, resolver: Option<&'a AddressResolver>) -> Self {
        Self {
            raw: args.raw,
            min_rssi: args.min_rssi,
            resolver,
            accumulator: AdvertisementAccumulator::new(args.passive),
        }
    }

    fn process(&mut self, packet: &HciPacket) -> Vec<String> {
        packet_advertisements(packet)
            .into_iter()
            .filter_map(|advertisement| {
                let advertisement = if self.raw {
                    Some(advertisement)
                } else {
                    self.accumulator.update(advertisement)
                }?;
                self.min_rssi
                    .is_none_or(|minimum| advertisement.rssi >= minimum)
                    .then(|| render_advertisement(&advertisement, self.raw, self.resolver))
            })
            .collect()
    }
}

#[cfg(test)]
fn render_packet(
    packet: &HciPacket,
    args: &Args,
    resolver: Option<&AddressResolver>,
) -> Vec<String> {
    AdvertisementProcessor::new(args, resolver).process(packet)
}

fn scan_transport<T: PacketSource + PacketSink>(
    transport: T,
    args: &Args,
    local_address: Address,
    resolver: Option<&AddressResolver>,
    mut emit: impl FnMut(String),
) -> Result<(), String> {
    let mut channel = HciCommandChannel::new(transport);
    configure_scan(&mut channel, args, local_address)?;
    let (mut transport, pending) = channel.into_parts();
    let mut processor = AdvertisementProcessor::new(args, resolver);
    for packet in pending {
        for output in processor.process(&packet) {
            emit(output);
        }
    }
    while let Some(packet) = transport.read_packet().map_err(|error| error.to_string())? {
        for output in processor.process(&packet) {
            emit(output);
        }
    }
    Ok(())
}

fn run(args: Args) -> Result<(), String> {
    let local_address = configured_address(args.device_config.as_deref())?;
    let resolver = load_resolver(&args, &local_address)?;
    let transport = open_transport(&args.transport).map_err(|error| error.to_string())?;
    scan_transport(
        transport,
        &args,
        local_address,
        resolver.as_ref(),
        |output| println!("{output}\n"),
    )
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
    use bumble_transport::Result;
    use std::collections::VecDeque;

    #[derive(Default)]
    struct MockTransport {
        inbound: VecDeque<HciPacket>,
        outbound: Vec<Command>,
        reject_extended: bool,
    }

    impl PacketSource for MockTransport {
        fn read_packet(&mut self) -> Result<Option<HciPacket>> {
            Ok(self.inbound.pop_front())
        }
    }

    impl PacketSink for MockTransport {
        fn write_packet(&mut self, packet: &HciPacket) -> Result<()> {
            let HciPacket::Command(command) = packet else {
                panic!("expected command")
            };
            self.outbound.push(command.clone());
            let status = if self.reject_extended
                && matches!(command, Command::LeSetExtendedScanParameters { .. })
            {
                1
            } else {
                0
            };
            self.inbound
                .push_back(HciPacket::Event(Event::CommandComplete {
                    num_hci_command_packets: 1,
                    command_opcode: command.op_code(),
                    return_parameters: bumble_hci::ReturnParameters::Status { status },
                }));
            if status == 0 && matches!(command, Command::LeSetExtendedScanEnable { .. }) {
                self.inbound.push_back(HciPacket::Event(Event::LeMeta(
                    LeMetaEvent::AdvertisingReport {
                        reports: vec![
                            AdvertisingReport {
                                event_type: 0,
                                address_type: 1,
                                address: Address::parse(
                                    "C0:11:22:33:44:55",
                                    AddressType::RANDOM_DEVICE,
                                )
                                .unwrap(),
                                data: vec![2, 1, 6, 5, 9, b'T', b'e', b's', b't'],
                                rssi: -45,
                            },
                            AdvertisingReport {
                                event_type: 4,
                                address_type: 1,
                                address: Address::parse(
                                    "C0:11:22:33:44:55",
                                    AddressType::RANDOM_DEVICE,
                                )
                                .unwrap(),
                                data: vec![2, 0x0A, 0xFB],
                                rssi: -44,
                            },
                        ],
                    },
                )));
            }
            Ok(())
        }
    }

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(ToString::to_string).collect()
    }

    fn default_args() -> Args {
        parse_args(args(&["scan", "mock"])).unwrap()
    }

    #[test]
    fn parses_upstream_options_and_validates_window() {
        assert_eq!(
            parse_args(args(&[
                "scan",
                "--min-rssi=-70",
                "--passive",
                "--scan-interval",
                "80",
                "--scan-window=40",
                "--phy",
                "coded",
                "--filter-duplicates",
                "false",
                "--raw",
                "--irk",
                "00112233445566778899aabbccddeeff:C0:11:22:33:44:55",
                "tcp-client:localhost:6402",
            ])),
            Ok(Args {
                min_rssi: Some(-70),
                passive: true,
                scan_interval_ms: 80,
                scan_window_ms: 40,
                phy: Some(Phy::Coded),
                filter_duplicates: false,
                raw: true,
                irks: vec!["00112233445566778899aabbccddeeff:C0:11:22:33:44:55".into()],
                keystore_file: None,
                device_config: None,
                transport: "tcp-client:localhost:6402".into(),
            })
        );
        assert!(parse_args(args(&[
            "scan",
            "--scan-interval",
            "20",
            "--scan-window",
            "40",
            "mock"
        ]))
        .is_err());
        assert!(parse_args(args(&["scan", "--phy", "2m", "mock"])).is_err());
    }

    #[test]
    fn scanner_configures_extended_phys_and_streams_reports() {
        let args = default_args();
        let local_address = Address::parse(DEFAULT_ADDRESS, AddressType::RANDOM_DEVICE).unwrap();
        let mut output = Vec::new();
        scan_transport(
            MockTransport::default(),
            &args,
            local_address,
            None,
            |line| output.push(line),
        )
        .unwrap();
        assert_eq!(output.len(), 1);
        assert!(output[0].contains("C0:11:22:33:44:55"));
        assert!(output[0].contains("Complete Local Name: Test"));
        assert!(output[0].contains("TX Power Level: -5 dBm"));
        assert!(output[0].contains("RSSI: -44"));
    }

    #[test]
    fn unsupported_extended_scanning_falls_back_except_for_coded_only() {
        let local_address = Address::parse(DEFAULT_ADDRESS, AddressType::RANDOM_DEVICE).unwrap();
        let mut channel = HciCommandChannel::new(MockTransport {
            reject_extended: true,
            ..MockTransport::default()
        });
        configure_scan(&mut channel, &default_args(), local_address.clone()).unwrap();
        let (transport, _) = channel.into_parts();
        assert!(transport.outbound.iter().any(|command| matches!(
            command,
            Command::SetEventMask { event_mask } if event_mask == &HOST_EVENT_MASK
        )));
        assert!(transport.outbound.iter().any(|command| matches!(
            command,
            Command::LeSetEventMask { le_event_mask } if le_event_mask == &HOST_LE_EVENT_MASK
        )));
        assert!(transport
            .outbound
            .iter()
            .any(|command| matches!(command, Command::LeSetExtendedScanParameters { .. })));
        assert!(transport
            .outbound
            .iter()
            .any(|command| matches!(command, Command::LeSetScanParameters { .. })));
        assert!(transport
            .outbound
            .iter()
            .any(|command| matches!(command, Command::LeSetScanEnable { .. })));

        let mut coded = default_args();
        coded.phy = Some(Phy::Coded);
        let mut channel = HciCommandChannel::new(MockTransport {
            reject_extended: true,
            ..MockTransport::default()
        });
        assert!(configure_scan(&mut channel, &coded, local_address).is_err());
    }

    #[test]
    fn rssi_filter_and_privacy_resolution_are_applied() {
        let mut args = default_args();
        args.min_rssi = Some(-40);
        let report = HciPacket::Event(Event::LeMeta(LeMetaEvent::AdvertisingReport {
            reports: vec![AdvertisingReport {
                event_type: 3,
                address_type: 1,
                address: Address::parse("40:11:22:33:44:55", AddressType::RANDOM_DEVICE).unwrap(),
                data: vec![],
                rssi: -60,
            }],
        }));
        assert!(render_packet(&report, &args, None).is_empty());

        let (irk, identity) =
            parse_irk("00112233445566778899aabbccddeeff:C0:11:22:33:44:55").unwrap();
        let irk_array: [u8; 16] = irk.clone().try_into().unwrap();
        let private_address = bumble_smp::resolvable_private_address(&irk_array, [1, 2, 3]);
        let resolver = AddressResolver::new([(irk, identity)]);
        let report = AdvertisingReport {
            event_type: 0,
            address_type: 1,
            address: private_address,
            data: vec![],
            rssi: -30,
        };
        let rendered = render_advertisement(&processed_legacy(&report), false, Some(&resolver));
        assert!(rendered.contains("C0:11:22:33:44:55 [RANDOM_ID]"));
        assert!(rendered.contains("resolved from"));
    }

    #[test]
    fn active_scan_coalesces_scan_response_while_passive_scan_emits_immediately() {
        let address = Address::parse("C0:11:22:33:44:55", AddressType::RANDOM_DEVICE).unwrap();
        let advertisement = processed_legacy(&AdvertisingReport {
            event_type: 0,
            address_type: 1,
            address: address.clone(),
            data: vec![2, 1, 6],
            rssi: -50,
        });
        let scan_response = processed_legacy(&AdvertisingReport {
            event_type: 4,
            address_type: 1,
            address,
            data: vec![3, 9, b'O', b'K'],
            rssi: -49,
        });

        let mut active = AdvertisementAccumulator::new(false);
        assert!(active.update(advertisement.clone()).is_none());
        let merged = active.update(scan_response).unwrap();
        assert!(merged.connectable);
        assert!(merged.scannable);
        assert_eq!(merged.data, vec![2, 1, 6, 3, 9, b'O', b'K']);

        let mut passive = AdvertisementAccumulator::new(true);
        assert_eq!(passive.update(advertisement.clone()), Some(advertisement));
    }

    #[test]
    fn rssi_bar_is_bounded() {
        assert_eq!(make_rssi_bar(-120), "");
        assert_eq!(make_rssi_bar(-30), "█".repeat(30));
        assert_eq!(make_rssi_bar(0), "█".repeat(30));
    }
}
