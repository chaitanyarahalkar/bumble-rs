use bumble::advertising_data::Type as AdvertisingDataType;
use bumble::{Address, AddressType, AdvertisingData, Uuid};
use bumble_att::AttPdu;
use bumble_gatt::{
    properties, AttTransport, CharacteristicProxy, DescriptorProxy, DynamicValue, GattClient,
    GattServer, ServiceProxy,
};
use bumble_hci::{Command as HciCommand, ReturnParameters};
use bumble_host::Device;
use bumble_profiles::gap::{
    GenericAccessService, APPEARANCE_CHARACTERISTIC, DEVICE_NAME_CHARACTERISTIC,
    GENERIC_ACCESS_SERVICE,
};
use bumble_smp::PairingConfig;
use bumble_transport::{
    open_split_transport, CommandResponse, ExternalAttTransport, ExternalHost,
    ExternalHostActivity, LePairingSession,
};
use regex::Regex;
use std::collections::{BTreeMap, VecDeque};
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const DEFAULT_NAME: &str = "Bumble";
const COMMAND_TIMEOUT: Duration = Duration::from_secs(5);
const PROCEDURE_TIMEOUT: Duration = Duration::from_secs(30);
const PAIRING_TIMEOUT: Duration = Duration::from_secs(120);
const POLL_INTERVAL: Duration = Duration::from_millis(50);
const RSSI_MONITOR_INTERVAL: Duration = Duration::from_secs(5);
const DEFAULT_RSSI_BAR_WIDTH: usize = 20;
const DISPLAY_MIN_RSSI: i8 = -100;
const DISPLAY_MAX_RSSI: i8 = -30;

#[derive(Clone, Debug, PartialEq, Eq)]
struct Args {
    device_config: Option<PathBuf>,
    transport: String,
}

fn usage() -> &'static str {
    "usage: bumble-console [--device-config PATH] TRANSPORT"
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
    let mut device_config = None;
    let mut transport = None;
    while let Some(argument) = arguments.pop_front() {
        if matches!(argument.as_str(), "-h" | "--help") {
            return Err(usage().into());
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
        device_config,
        transport: transport.ok_or_else(|| usage().to_string())?,
    })
}

#[derive(Clone, Debug)]
struct DeviceConfig {
    name: String,
    address: Address,
}

fn generated_static_address() -> Address {
    let seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let mut bytes = [0u8; 6];
    for (index, byte) in bytes.iter_mut().enumerate() {
        *byte = (seed >> (index * 8)) as u8;
    }
    bytes[5] = (bytes[5] & 0x3F) | 0xC0;
    Address::from_bytes(bytes, AddressType::RANDOM_DEVICE)
}

fn load_device_config(path: Option<&Path>) -> Result<DeviceConfig, String> {
    let Some(path) = path else {
        return Ok(DeviceConfig {
            name: DEFAULT_NAME.into(),
            address: generated_static_address(),
        });
    };
    let bytes = std::fs::read(path)
        .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
    let value: serde_json::Value = serde_json::from_slice(&bytes)
        .map_err(|error| format!("invalid device config: {error}"))?;
    let name = value
        .get("name")
        .and_then(serde_json::Value::as_str)
        .unwrap_or(DEFAULT_NAME)
        .to_string();
    let address = match value.get("address").and_then(serde_json::Value::as_str) {
        Some(address) => Address::parse(address, AddressType::RANDOM_DEVICE)
            .map_err(|error| error.to_string())?,
        None => generated_static_address(),
    };
    Ok(DeviceConfig { name, address })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SwitchAction {
    Toggle,
    On,
    Off,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum ScanAction {
    Switch(SwitchAction, Option<String>),
    Clear,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum View {
    Scan,
    Log,
    Device,
    LocalServices,
    RemoteServices,
    LocalValues,
    RemoteValues,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum ConsoleCommand {
    Scan(ScanAction),
    Advertise(SwitchAction),
    Rssi(SwitchAction),
    Show(View),
    FilterAddress(String),
    Connect {
        target: String,
        phys: Option<String>,
    },
    UpdateParameters(String),
    Encrypt,
    Disconnect,
    DiscoverServices,
    DiscoverAttributes,
    RequestMtu(u16),
    Read(String),
    Write {
        selector: String,
        value: String,
    },
    LocalWrite {
        selector: String,
        value: String,
    },
    Subscribe(String),
    Unsubscribe(String),
    GetPhy,
    SetPhy(String),
    SetDefaultPhy(String),
    Exit,
}

fn parse_switch(params: &[&str], command: &str) -> Result<SwitchAction, String> {
    match params {
        [] => Ok(SwitchAction::Toggle),
        ["on"] => Ok(SwitchAction::On),
        ["off"] => Ok(SwitchAction::Off),
        _ => Err(format!("unsupported arguments for {command} command")),
    }
}

fn parse_view(value: &str) -> Result<View, String> {
    match value {
        "scan" => Ok(View::Scan),
        "log" => Ok(View::Log),
        "device" => Ok(View::Device),
        "local-services" => Ok(View::LocalServices),
        "remote-services" => Ok(View::RemoteServices),
        "local-values" => Ok(View::LocalValues),
        "remote-values" => Ok(View::RemoteValues),
        _ => Err(format!("unknown view {value:?}")),
    }
}

fn parse_console_command(line: &str) -> Result<ConsoleCommand, String> {
    let words: Vec<_> = line.split_whitespace().collect();
    let Some((keyword, params)) = words.split_first() else {
        return Err("empty command".into());
    };
    match *keyword {
        "scan" => match params {
            [] => Ok(ConsoleCommand::Scan(ScanAction::Switch(
                SwitchAction::Toggle,
                None,
            ))),
            ["on"] => Ok(ConsoleCommand::Scan(ScanAction::Switch(
                SwitchAction::On,
                None,
            ))),
            ["on", filter] if filter.starts_with("filter=") => Ok(ConsoleCommand::Scan(
                ScanAction::Switch(SwitchAction::On, Some(filter[7..].to_string())),
            )),
            ["off"] => Ok(ConsoleCommand::Scan(ScanAction::Switch(
                SwitchAction::Off,
                None,
            ))),
            ["clear"] => Ok(ConsoleCommand::Scan(ScanAction::Clear)),
            _ => Err("unsupported arguments for scan command".into()),
        },
        "advertise" => Ok(ConsoleCommand::Advertise(parse_switch(
            params,
            "advertise",
        )?)),
        "rssi" => Ok(ConsoleCommand::Rssi(parse_switch(params, "rssi")?)),
        "show" => match params {
            [] => Ok(ConsoleCommand::Show(View::Device)),
            [view] => Ok(ConsoleCommand::Show(parse_view(view)?)),
            _ => Err("expected show <view>".into()),
        },
        "filter" => match params {
            ["address", pattern] => Ok(ConsoleCommand::FilterAddress((*pattern).into())),
            _ => Err("expected filter address <pattern>".into()),
        },
        "connect" => match params {
            [target] => Ok(ConsoleCommand::Connect {
                target: (*target).into(),
                phys: None,
            }),
            [target, phys] => Ok(ConsoleCommand::Connect {
                target: (*target).into(),
                phys: Some((*phys).into()),
            }),
            _ => Err("expected connect <address> [phys]".into()),
        },
        "update-parameters" => match params {
            [parameters] => Ok(ConsoleCommand::UpdateParameters((*parameters).into())),
            _ => Err("expected update-parameters <min>-<max>/<latency>/<supervision>".into()),
        },
        "encrypt" if params.is_empty() => Ok(ConsoleCommand::Encrypt),
        "disconnect" if params.is_empty() => Ok(ConsoleCommand::Disconnect),
        "discover" => match params {
            ["services"] => Ok(ConsoleCommand::DiscoverServices),
            ["attributes"] => Ok(ConsoleCommand::DiscoverAttributes),
            _ => Err("expected discover services|attributes".into()),
        },
        "request-mtu" => match params {
            [mtu] => Ok(ConsoleCommand::RequestMtu(
                mtu.parse().map_err(|_| "invalid MTU".to_string())?,
            )),
            _ => Err("expected request-mtu <mtu>".into()),
        },
        "read" => match params {
            [selector] => Ok(ConsoleCommand::Read((*selector).into())),
            _ => Err("expected read <attribute>".into()),
        },
        "write" => match params {
            [selector, value] => Ok(ConsoleCommand::Write {
                selector: (*selector).into(),
                value: (*value).into(),
            }),
            _ => Err("expected write <attribute> <value>".into()),
        },
        "local-write" => match params {
            [selector, value] => Ok(ConsoleCommand::LocalWrite {
                selector: (*selector).into(),
                value: (*value).into(),
            }),
            _ => Err("expected local-write <attribute> <value>".into()),
        },
        "subscribe" => match params {
            [selector] => Ok(ConsoleCommand::Subscribe((*selector).into())),
            _ => Err("expected subscribe <attribute>".into()),
        },
        "unsubscribe" => match params {
            [selector] => Ok(ConsoleCommand::Unsubscribe((*selector).into())),
            _ => Err("expected unsubscribe <attribute>".into()),
        },
        "get-phy" if params.is_empty() => Ok(ConsoleCommand::GetPhy),
        "set-phy" => match params {
            [phys] => Ok(ConsoleCommand::SetPhy((*phys).into())),
            _ => Err("expected set-phy <tx_rx_phys>|<tx_phys>/<rx_phys>".into()),
        },
        "set-default-phy" => match params {
            [phys] => Ok(ConsoleCommand::SetDefaultPhy((*phys).into())),
            _ => Err("expected set-default-phy <tx_rx_phys>|<tx_phys>/<rx_phys>".into()),
        },
        "exit" | "quit" if params.is_empty() => Ok(ConsoleCommand::Exit),
        _ => Err(format!("unknown command {keyword:?}")),
    }
}

fn parse_phys(value: &str) -> Result<Option<u8>, String> {
    if value.eq_ignore_ascii_case("*") {
        return Ok(None);
    }
    let mut mask = 0u8;
    for phy in value.split(',') {
        mask |= match phy.to_ascii_lowercase().as_str() {
            "1m" => 0x01,
            "2m" => 0x02,
            "coded" => 0x04,
            _ => return Err("invalid PHY name".into()),
        };
    }
    (mask != 0)
        .then_some(Some(mask))
        .ok_or_else(|| "invalid PHY list".into())
}

fn parse_tx_rx_phys(value: &str) -> Result<(Option<u8>, Option<u8>), String> {
    if let Some((tx, rx)) = value.split_once('/') {
        Ok((parse_phys(tx)?, parse_phys(rx)?))
    } else {
        let phys = parse_phys(value)?;
        Ok((phys, phys))
    }
}

fn parse_value(value: &str) -> Result<Vec<u8>, String> {
    if let Some(hex) = value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
    {
        if hex.len() % 2 != 0 {
            return Err("hex values must contain an even number of digits".into());
        }
        return (0..hex.len())
            .step_by(2)
            .map(|index| {
                u8::from_str_radix(&hex[index..index + 2], 16)
                    .map_err(|_| "invalid hex value".to_string())
            })
            .collect();
    }
    if let Ok(number) = value.parse::<u16>() {
        return Ok(number.to_le_bytes().to_vec());
    }
    Ok(value.as_bytes().to_vec())
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn rssi_bar(rssi: i8) -> String {
    const BLOCKS: [&str; 8] = ["", "▏", "▎", "▍", "▌", "▋", "▊", "▉"];
    let numerator = i32::from(rssi.clamp(DISPLAY_MIN_RSSI, DISPLAY_MAX_RSSI) - DISPLAY_MIN_RSSI);
    let denominator = i32::from(DISPLAY_MAX_RSSI - DISPLAY_MIN_RSSI);
    let ticks = numerator as usize * DEFAULT_RSSI_BAR_WIDTH * 8 / denominator as usize;
    format!("{rssi:4} {}{}", "█".repeat(ticks / 8), BLOCKS[ticks % 8])
}

fn response_status(response: &CommandResponse) -> Option<u8> {
    response
        .status()
        .or_else(|| match response.return_parameters() {
            Some(ReturnParameters::Raw { data }) => data.first().copied(),
            _ => None,
        })
}

fn require_success(response: CommandResponse, context: &str) -> Result<CommandResponse, String> {
    if response_status(&response) == Some(0) {
        Ok(response)
    } else {
        Err(format!(
            "{context} failed with HCI status {:?}",
            response_status(&response)
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

#[derive(Clone, Debug)]
struct ScanResult {
    address: Address,
    address_type: u8,
    data: Vec<u8>,
    rssi: i8,
    connectable: bool,
}

impl ScanResult {
    fn name(&self) -> String {
        let data = AdvertisingData::from_bytes(&self.data);
        data.get(AdvertisingDataType::COMPLETE_LOCAL_NAME)
            .or_else(|| data.get(AdvertisingDataType::SHORTENED_LOCAL_NAME))
            .map(|name| String::from_utf8_lossy(&name).into_owned())
            .unwrap_or_default()
    }

    fn display(&self) -> String {
        let address_type = match self.address_type {
            0 => "P",
            1 => "R",
            2 => "PI",
            3 => "RI",
            _ => "?",
        };
        let marker = if self.connectable { "+" } else { "-" };
        format!(
            "{marker} {} [{address_type}] {:<26} {}",
            self.address.to_string(false),
            rssi_bar(self.rssi),
            self.name()
        )
    }
}

#[derive(Clone, Debug)]
struct RemoteCharacteristic {
    service: ServiceProxy,
    characteristic: CharacteristicProxy,
    descriptors: Vec<DescriptorProxy>,
}

#[derive(Clone, Debug, Default)]
struct RemoteDatabase {
    services: Vec<ServiceProxy>,
    characteristics: Vec<RemoteCharacteristic>,
    attributes: Vec<DescriptorProxy>,
}

impl RemoteDatabase {
    fn discover_all(
        client: &mut GattClient,
        transport: &mut impl AttTransport,
    ) -> Result<Self, String> {
        let services = client
            .discover_services(transport)
            .map_err(|error| error.to_string())?;
        let mut characteristics = Vec::new();
        for service in &services {
            for characteristic in client
                .discover_characteristics(transport, service)
                .map_err(|error| error.to_string())?
            {
                let descriptors = client
                    .discover_descriptors(transport, &characteristic)
                    .map_err(|error| error.to_string())?;
                characteristics.push(RemoteCharacteristic {
                    service: service.clone(),
                    characteristic,
                    descriptors,
                });
            }
        }
        Ok(Self {
            services,
            characteristics,
            attributes: Vec::new(),
        })
    }

    fn find_characteristic(&self, selector: &str) -> Result<RemoteCharacteristic, String> {
        if let Some(handle) = selector.strip_prefix('#') {
            let handle = u16::from_str_radix(handle, 16)
                .map_err(|_| "invalid attribute handle".to_string())?;
            return self
                .characteristics
                .iter()
                .find(|entry| entry.characteristic.handle == handle)
                .cloned()
                .ok_or_else(|| "no such characteristic".into());
        }
        let (service, characteristic) = selector
            .split_once('.')
            .ok_or_else(|| "expected <service>.<characteristic> or #<handle>".to_string())?;
        let service = (service != "*")
            .then(|| Uuid::parse(service).map_err(|error| error.to_string()))
            .transpose()?;
        let characteristic = Uuid::parse(characteristic).map_err(|error| error.to_string())?;
        self.characteristics
            .iter()
            .find(|entry| {
                service
                    .as_ref()
                    .is_none_or(|service| &entry.service.uuid == service)
                    && entry.characteristic.uuid == characteristic
            })
            .cloned()
            .ok_or_else(|| "no such characteristic".into())
    }

    fn cccd(entry: &RemoteCharacteristic) -> Option<u16> {
        entry
            .descriptors
            .iter()
            .find(|descriptor| descriptor.uuid == Uuid::from_16_bits(0x2902))
            .map(|descriptor| descriptor.handle)
    }
}

#[derive(Clone, Debug)]
struct LocalAttribute {
    service_uuid: Uuid,
    characteristic_uuid: Uuid,
    handle: u16,
    properties: u8,
}

type LocalValues = Arc<Mutex<BTreeMap<u16, Vec<u8>>>>;

fn build_local_gatt(name: &str) -> Result<(GattServer, LocalValues, Vec<LocalAttribute>), String> {
    let definition = GenericAccessService::from_packed_appearance(name, 0).definition();
    let mut server =
        GattServer::from_definitions(vec![definition]).map_err(|error| error.to_string())?;
    let name_handle = *server
        .handles_by_uuid(&Uuid::from_16_bits(DEVICE_NAME_CHARACTERISTIC))
        .first()
        .ok_or_else(|| "local Device Name characteristic is missing".to_string())?;
    let appearance_handle = *server
        .handles_by_uuid(&Uuid::from_16_bits(APPEARANCE_CHARACTERISTIC))
        .first()
        .ok_or_else(|| "local Appearance characteristic is missing".to_string())?;
    let values = Arc::new(Mutex::new(BTreeMap::from([
        (name_handle, name.as_bytes().to_vec()),
        (appearance_handle, 0u16.to_le_bytes().to_vec()),
    ])));
    for handle in [name_handle, appearance_handle] {
        let values = Arc::clone(&values);
        server
            .set_dynamic_value(
                handle,
                DynamicValue::read_only(move |_| {
                    values
                        .lock()
                        .map_err(|_| 0x0E)?
                        .get(&handle)
                        .cloned()
                        .ok_or(0x0A)
                }),
            )
            .map_err(|error| error.to_string())?;
    }
    let service_uuid = Uuid::from_16_bits(GENERIC_ACCESS_SERVICE);
    let attributes = vec![
        LocalAttribute {
            service_uuid: service_uuid.clone(),
            characteristic_uuid: Uuid::from_16_bits(DEVICE_NAME_CHARACTERISTIC),
            handle: name_handle,
            properties: properties::READ,
        },
        LocalAttribute {
            service_uuid,
            characteristic_uuid: Uuid::from_16_bits(APPEARANCE_CHARACTERISTIC),
            handle: appearance_handle,
            properties: properties::READ,
        },
    ];
    Ok((server, values, attributes))
}

enum InputMessage {
    Line(String),
    Ended,
    Error(String),
}

fn spawn_input() -> Receiver<InputMessage> {
    let (sender, receiver) = mpsc::channel();
    thread::Builder::new()
        .name("bumble-console-input".into())
        .spawn(move || {
            let stdin = std::io::stdin();
            for line in stdin.lock().lines() {
                match line {
                    Ok(line) => {
                        if sender.send(InputMessage::Line(line)).is_err() {
                            return;
                        }
                    }
                    Err(error) => {
                        let _ = sender.send(InputMessage::Error(error.to_string()));
                        return;
                    }
                }
            }
            let _ = sender.send(InputMessage::Ended);
        })
        .expect("console input worker starts");
    receiver
}

struct ConsoleRuntime {
    name: String,
    local_address: Address,
    public_address: Address,
    own_address_type: u8,
    scanning: bool,
    advertising: bool,
    monitor_rssi: bool,
    connection_rssi: Option<i8>,
    connection_phy: Option<(u8, u8)>,
    active_handle: Option<u16>,
    pairing: Option<LePairingSession>,
    scan_results: BTreeMap<String, ScanResult>,
    address_filter: Regex,
    client: GattClient,
    remote: RemoteDatabase,
    local_server: GattServer,
    local_values: LocalValues,
    local_attributes: Vec<LocalAttribute>,
    last_rssi: Instant,
}

impl ConsoleRuntime {
    fn new(
        config: DeviceConfig,
        public_address: Address,
        local_server: GattServer,
        local_values: LocalValues,
        local_attributes: Vec<LocalAttribute>,
    ) -> Self {
        Self {
            name: config.name,
            own_address_type: u8::from(!config.address.is_public()),
            local_address: config.address,
            public_address,
            scanning: false,
            advertising: false,
            monitor_rssi: false,
            connection_rssi: None,
            connection_phy: None,
            active_handle: None,
            pairing: None,
            scan_results: BTreeMap::new(),
            address_filter: Regex::new(".*").expect("default regex is valid"),
            client: GattClient::new(),
            remote: RemoteDatabase::default(),
            local_server,
            local_values,
            local_attributes,
            last_rssi: Instant::now(),
        }
    }

    fn connection_handle(&self, device: &Device) -> Result<u16, String> {
        self.active_handle
            .filter(|handle| device.is_connected_on_handle(*handle))
            .or_else(|| device.connection_handle())
            .ok_or_else(|| "not connected".into())
    }

    fn configure_controller(&self, host: &mut ExternalHost) -> Result<(), String> {
        if self.own_address_type != 0 {
            command(
                host,
                HciCommand::LeSetRandomAddress {
                    random_address: self.local_address.clone(),
                },
                "setting random address",
            )?;
        }
        let mut local_name = [0u8; 248];
        let bytes = self.name.as_bytes();
        let length = bytes.len().min(local_name.len());
        local_name[..length].copy_from_slice(&bytes[..length]);
        command(
            host,
            HciCommand::WriteLocalName { local_name },
            "setting local name",
        )?;
        Ok(())
    }

    fn process_reports(&mut self, device: &mut Device) {
        for report in device.take_advertising_reports() {
            self.update_scan_result(ScanResult {
                address: report.address,
                address_type: report.address_type,
                data: report.data,
                rssi: report.rssi,
                connectable: matches!(report.event_type, 0 | 1),
            });
        }
        for report in device.take_extended_advertising_reports() {
            self.update_scan_result(ScanResult {
                address: report.address,
                address_type: report.address_type,
                data: report.data,
                rssi: report.rssi,
                connectable: report.event_type & 0x0001 != 0,
            });
        }
    }

    fn update_scan_result(&mut self, result: ScanResult) {
        let address = result.address.to_string(false);
        if !self.address_filter.is_match(&address) {
            return;
        }
        let key = format!("{address}/{}", result.address_type);
        println!("{}", result.display());
        self.scan_results.insert(key, result);
    }

    fn process_connection_state(&mut self, device: &Device) -> Result<(), String> {
        if let Some(handle) = self.active_handle {
            if !device.is_connected_on_handle(handle) {
                println!("disconnected from handle {handle:#06x}");
                self.active_handle = None;
                self.connection_rssi = None;
                self.connection_phy = None;
                self.pairing = None;
                self.remote = RemoteDatabase::default();
                self.client = GattClient::new();
            }
        }
        if self.active_handle.is_none() {
            if let Some(connection) = device.le_connections().next() {
                println!(
                    "connected to {} on handle {:#06x}",
                    connection.peer_address, connection.connection_handle
                );
                self.active_handle = Some(connection.connection_handle);
            }
        }
        if let Some(handle) = self.active_handle {
            if self.pairing.is_none() {
                let mut pairing = LePairingSession::accept_all(
                    device,
                    handle,
                    self.local_address.clone(),
                    PairingConfig {
                        mitm: false,
                        ..PairingConfig::default()
                    },
                )
                .map_err(|error| error.to_string())?;
                pairing.listen(device).map_err(|error| error.to_string())?;
                self.pairing = Some(pairing);
            }
        }
        Ok(())
    }

    fn drive_pairing(
        &mut self,
        host: &mut ExternalHost,
        device: &mut Device,
    ) -> Result<(), String> {
        let Some(pairing) = &mut self.pairing else {
            return Ok(());
        };
        if pairing
            .drive_once(host, device)
            .map_err(|error| error.to_string())?
            .is_some()
        {
            println!("pairing complete; connection encrypted");
            self.pairing = None;
        }
        Ok(())
    }

    fn process_att(&mut self, host: &mut ExternalHost, device: &mut Device) -> Result<(), String> {
        let Some(handle) = self.active_handle else {
            return Ok(());
        };
        for pdu in device.take_inbox_on_handle(handle) {
            self.handle_att_pdu(host, device, handle, pdu)?;
        }
        Ok(())
    }

    fn handle_att_pdu(
        &mut self,
        host: &mut ExternalHost,
        device: &mut Device,
        handle: u16,
        pdu: AttPdu,
    ) -> Result<(), String> {
        match &pdu {
            AttPdu::HandleValueNotification {
                attribute_handle,
                attribute_value,
            } => {
                self.client
                    .on_notification(&pdu)
                    .map_err(|error| error.to_string())?;
                println!("#{attribute_handle:04X} VALUE: 0x{}", hex(attribute_value));
            }
            AttPdu::HandleValueIndication {
                attribute_handle,
                attribute_value,
            } => {
                let confirmation = self
                    .client
                    .on_indication(&pdu)
                    .map_err(|error| error.to_string())?;
                if !device.send_att_on_handle(host, handle, &confirmation) {
                    return Err("failed to confirm ATT indication".into());
                }
                println!("#{attribute_handle:04X} VALUE: 0x{}", hex(attribute_value));
            }
            _ => println!("unsolicited ATT: {pdu:?}"),
        }
        Ok(())
    }

    fn process_unsolicited(
        &mut self,
        host: &mut ExternalHost,
        device: &mut Device,
        handle: u16,
        pdus: Vec<AttPdu>,
    ) -> Result<(), String> {
        for pdu in pdus {
            self.handle_att_pdu(host, device, handle, pdu)?;
        }
        Ok(())
    }

    fn maybe_read_rssi(&mut self, host: &mut ExternalHost, device: &Device) -> Result<(), String> {
        if !self.monitor_rssi || self.last_rssi.elapsed() < RSSI_MONITOR_INTERVAL {
            return Ok(());
        }
        self.last_rssi = Instant::now();
        let handle = self.connection_handle(device)?;
        let response = command(
            host,
            HciCommand::ReadRssi { handle },
            "reading connection RSSI",
        )?;
        if let Some(ReturnParameters::Raw { data }) = response.return_parameters() {
            if data.len() >= 4 {
                self.connection_rssi = Some(data[3] as i8);
                println!("RSSI: {}", rssi_bar(data[3] as i8));
            }
        }
        Ok(())
    }

    fn set_scanning(&mut self, host: &mut ExternalHost, enabled: bool) -> Result<(), String> {
        if enabled {
            command(
                host,
                HciCommand::LeSetScanParameters {
                    le_scan_type: 1,
                    le_scan_interval: 0x0010,
                    le_scan_window: 0x0010,
                    own_address_type: self.own_address_type,
                    scanning_filter_policy: 0,
                },
                "setting scan parameters",
            )?;
        }
        command(
            host,
            HciCommand::LeSetScanEnable {
                le_scan_enable: u8::from(enabled),
                filter_duplicates: 0,
            },
            if enabled {
                "enabling scan"
            } else {
                "disabling scan"
            },
        )?;
        self.scanning = enabled;
        println!("scan {}", if enabled { "on" } else { "off" });
        Ok(())
    }

    fn advertising_data(&self) -> Vec<u8> {
        let mut data = vec![2, AdvertisingDataType::FLAGS.0, 0x06];
        let name = self.name.as_bytes();
        let length = name.len().min(26);
        data.push((length + 1) as u8);
        data.push(AdvertisingDataType::COMPLETE_LOCAL_NAME.0);
        data.extend_from_slice(&name[..length]);
        data
    }

    fn set_advertising(&mut self, host: &mut ExternalHost, enabled: bool) -> Result<(), String> {
        if enabled {
            command(
                host,
                HciCommand::LeSetAdvertisingParameters {
                    advertising_interval_min: 0x0800,
                    advertising_interval_max: 0x0800,
                    advertising_type: 0,
                    own_address_type: self.own_address_type,
                    peer_address_type: 0,
                    peer_address: Address::from_bytes([0; 6], AddressType::PUBLIC_DEVICE),
                    advertising_channel_map: 7,
                    advertising_filter_policy: 0,
                },
                "setting advertising parameters",
            )?;
            command(
                host,
                HciCommand::LeSetAdvertisingData {
                    advertising_data: self.advertising_data(),
                },
                "setting advertising data",
            )?;
        }
        command(
            host,
            HciCommand::LeSetAdvertisingEnable {
                advertising_enable: u8::from(enabled),
            },
            if enabled {
                "enabling advertising"
            } else {
                "disabling advertising"
            },
        )?;
        self.advertising = enabled;
        println!("advertising {}", if enabled { "on" } else { "off" });
        Ok(())
    }

    fn wait_for_connection(
        &mut self,
        host: &mut ExternalHost,
        device: &mut Device,
        peer: &Address,
    ) -> Result<u16, String> {
        let deadline = Instant::now() + PROCEDURE_TIMEOUT;
        loop {
            device.poll(host);
            self.process_reports(device);
            if let Some(handle) = device.connection_handle_for_peer(peer) {
                self.active_handle = Some(handle);
                return Ok(handle);
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err("connection timed out".into());
            }
            match host
                .wait_for_activity(remaining)
                .map_err(|error| error.to_string())?
            {
                ExternalHostActivity::Packet => {}
                ExternalHostActivity::Timeout => return Err("connection timed out".into()),
                ExternalHostActivity::Ended => {
                    return Err("transport ended while connecting".into())
                }
            }
        }
    }

    fn connect(
        &mut self,
        host: &mut ExternalHost,
        device: &mut Device,
        target: &str,
        phys: Option<&str>,
    ) -> Result<(), String> {
        let peer = Address::parse(target, AddressType::RANDOM_DEVICE)
            .map_err(|error| error.to_string())?;
        if self.scanning {
            self.set_scanning(host, false)?;
        }
        println!("connecting...");
        if let Some(mask) = phys.map(parse_phys).transpose()?.flatten() {
            let count = mask.count_ones() as usize;
            command(
                host,
                HciCommand::LeExtendedCreateConnection {
                    initiator_filter_policy: 0,
                    own_address_type: self.own_address_type,
                    peer_address_type: u8::from(!peer.is_public()),
                    peer_address: peer.clone(),
                    initiating_phys: mask,
                    scan_intervals: vec![0x0010; count],
                    scan_windows: vec![0x0010; count],
                    connection_interval_mins: vec![24; count],
                    connection_interval_maxs: vec![40; count],
                    max_latencies: vec![0; count],
                    supervision_timeouts: vec![42; count],
                    min_ce_lengths: vec![0; count],
                    max_ce_lengths: vec![0; count],
                },
                "creating extended LE connection",
            )?;
        } else {
            command(
                host,
                HciCommand::LeCreateConnection {
                    le_scan_interval: 0x0010,
                    le_scan_window: 0x0010,
                    initiator_filter_policy: 0,
                    peer_address_type: u8::from(!peer.is_public()),
                    peer_address: peer.clone(),
                    own_address_type: self.own_address_type,
                    connection_interval_min: 24,
                    connection_interval_max: 40,
                    max_latency: 0,
                    supervision_timeout: 42,
                    min_ce_length: 0,
                    max_ce_length: 0,
                },
                "creating LE connection",
            )?;
        }
        let handle = self.wait_for_connection(host, device, &peer)?;
        println!("connected to {peer} on handle {handle:#06x}");
        self.read_phy(host, device)?;
        Ok(())
    }

    fn read_phy(&mut self, host: &mut ExternalHost, device: &Device) -> Result<(), String> {
        let handle = self.connection_handle(device)?;
        let response = command(
            host,
            HciCommand::LeReadPhy {
                connection_handle: handle,
            },
            "reading LE PHY",
        )?;
        let Some(ReturnParameters::Raw { data }) = response.return_parameters() else {
            return Err("controller returned no LE PHY data".into());
        };
        if data.len() < 5 {
            return Err("controller returned truncated LE PHY data".into());
        }
        self.connection_phy = Some((data[3], data[4]));
        println!("PHY: RX={}, TX={}", phy_name(data[4]), phy_name(data[3]));
        Ok(())
    }

    fn with_att_transport<'a>(
        &self,
        host: &'a mut ExternalHost,
        device: &'a mut Device,
    ) -> Result<(u16, ExternalAttTransport<'a>), String> {
        let handle = self.connection_handle(device)?;
        let transport = ExternalAttTransport::new(host, device, handle, PROCEDURE_TIMEOUT)
            .map_err(|error| error.to_string())?;
        Ok((handle, transport))
    }

    fn discover_services(
        &mut self,
        host: &mut ExternalHost,
        device: &mut Device,
    ) -> Result<(), String> {
        println!("Service Discovery starting...");
        let (handle, mut transport) = self.with_att_transport(host, device)?;
        let remote = RemoteDatabase::discover_all(&mut self.client, &mut transport)?;
        let unsolicited = transport.take_unsolicited();
        drop(transport);
        self.remote = remote;
        self.process_unsolicited(host, device, handle, unsolicited)?;
        println!("Service Discovery done!");
        self.show_remote_services();
        Ok(())
    }

    fn discover_attributes(
        &mut self,
        host: &mut ExternalHost,
        device: &mut Device,
    ) -> Result<(), String> {
        println!("discovering attributes...");
        let (handle, mut transport) = self.with_att_transport(host, device)?;
        let attributes = self
            .client
            .discover_attributes(&mut transport)
            .map_err(|error| error.to_string())?;
        let unsolicited = transport.take_unsolicited();
        drop(transport);
        println!("discovered {} attributes", attributes.len());
        for attribute in &attributes {
            println!(
                "  #{:04X} {}",
                attribute.handle,
                attribute.uuid.to_hex_str("-")
            );
        }
        self.remote.attributes = attributes;
        self.process_unsolicited(host, device, handle, unsolicited)
    }

    fn read_remote(
        &mut self,
        host: &mut ExternalHost,
        device: &mut Device,
        selector: &str,
    ) -> Result<(), String> {
        let entry = self.remote.find_characteristic(selector)?;
        let (handle, mut transport) = self.with_att_transport(host, device)?;
        let value = self
            .client
            .read_value(&mut transport, entry.characteristic.handle, false)
            .map_err(|error| error.to_string())?;
        let unsolicited = transport.take_unsolicited();
        drop(transport);
        println!("VALUE: 0x{}", hex(&value));
        self.process_unsolicited(host, device, handle, unsolicited)
    }

    fn write_remote(
        &mut self,
        host: &mut ExternalHost,
        device: &mut Device,
        selector: &str,
        value: &str,
    ) -> Result<(), String> {
        let entry = self.remote.find_characteristic(selector)?;
        let value = parse_value(value)?;
        let with_response = entry.characteristic.properties & properties::WRITE != 0;
        let (handle, mut transport) = self.with_att_transport(host, device)?;
        self.client
            .write_value(
                &mut transport,
                entry.characteristic.handle,
                value,
                with_response,
            )
            .map_err(|error| error.to_string())?;
        let unsolicited = transport.take_unsolicited();
        drop(transport);
        println!("write complete");
        self.process_unsolicited(host, device, handle, unsolicited)
    }

    fn subscribe(
        &mut self,
        host: &mut ExternalHost,
        device: &mut Device,
        selector: &str,
        enabled: bool,
    ) -> Result<(), String> {
        let entry = self.remote.find_characteristic(selector)?;
        let cccd =
            RemoteDatabase::cccd(&entry).ok_or_else(|| "characteristic has no CCCD".to_string())?;
        let (handle, mut transport) = self.with_att_transport(host, device)?;
        if enabled {
            let indicate = entry.characteristic.properties & properties::NOTIFY == 0
                && entry.characteristic.properties & properties::INDICATE != 0;
            self.client
                .subscribe(&mut transport, entry.characteristic.handle, cccd, indicate)
                .map_err(|error| error.to_string())?;
        } else {
            self.client
                .unsubscribe(&mut transport, entry.characteristic.handle, cccd)
                .map_err(|error| error.to_string())?;
        }
        let unsolicited = transport.take_unsolicited();
        drop(transport);
        println!(
            "{} #{:04X}",
            if enabled {
                "subscribed to"
            } else {
                "unsubscribed from"
            },
            entry.characteristic.handle
        );
        self.process_unsolicited(host, device, handle, unsolicited)
    }

    fn request_mtu(
        &mut self,
        host: &mut ExternalHost,
        device: &mut Device,
        mtu: u16,
    ) -> Result<(), String> {
        let (handle, mut transport) = self.with_att_transport(host, device)?;
        let mtu = self
            .client
            .exchange_mtu(&mut transport, mtu)
            .map_err(|error| error.to_string())?;
        let unsolicited = transport.take_unsolicited();
        drop(transport);
        println!("ATT MTU: {mtu}");
        self.process_unsolicited(host, device, handle, unsolicited)
    }

    fn find_local_attribute(&self, selector: &str) -> Result<LocalAttribute, String> {
        if let Some(handle) = selector.strip_prefix('#') {
            let handle = u16::from_str_radix(handle, 16)
                .map_err(|_| "invalid attribute handle".to_string())?;
            return self
                .local_attributes
                .iter()
                .find(|attribute| attribute.handle == handle)
                .cloned()
                .ok_or_else(|| "unable to find local attribute".into());
        }
        let (service, characteristic) = selector
            .split_once('.')
            .ok_or_else(|| "expected <service>.<characteristic> or #<handle>".to_string())?;
        let service = Uuid::parse(service).map_err(|error| error.to_string())?;
        let characteristic = Uuid::parse(characteristic).map_err(|error| error.to_string())?;
        self.local_attributes
            .iter()
            .find(|attribute| {
                attribute.service_uuid == service && attribute.characteristic_uuid == characteristic
            })
            .cloned()
            .ok_or_else(|| "unable to find local attribute".into())
    }

    fn write_local(
        &mut self,
        host: &mut ExternalHost,
        device: &mut Device,
        selector: &str,
        value: &str,
    ) -> Result<(), String> {
        let attribute = self.find_local_attribute(selector)?;
        let value = parse_value(value)?;
        self.local_values
            .lock()
            .map_err(|_| "local value lock poisoned".to_string())?
            .insert(attribute.handle, value.clone());
        if let Some(handle) = self.active_handle {
            let pdu = if attribute.properties & properties::INDICATE != 0 {
                Some(self.local_server.indicate(attribute.handle, value.clone()))
            } else if attribute.properties & properties::NOTIFY != 0 {
                Some(self.local_server.notify(attribute.handle, value.clone()))
            } else {
                None
            };
            if let Some(pdu) = pdu {
                if !device.send_att_on_handle(host, handle, &pdu) {
                    return Err("failed to send local value update".into());
                }
            }
        }
        println!("local #{:04X} = 0x{}", attribute.handle, hex(&value));
        Ok(())
    }

    fn show_device(&self, device: &Device) {
        println!("Bumble Version:       {}", env!("CARGO_PKG_VERSION"));
        println!("Name:                 {}", self.name);
        println!("Public Address:       {}", self.public_address);
        println!("Random Address:       {}", self.local_address);
        println!("LE Enabled:           true");
        println!("Classic Enabled:      false");
        println!("Discoverable:         {}", self.advertising);
        println!("Connectable:          {}", self.advertising);
        println!("Scanning:             {}", self.scanning);
        self.show_status(device);
    }

    fn show_status(&self, device: &Device) {
        let connection = self.active_handle.and_then(|handle| {
            device.le_connection(handle).map(|connection| {
                format!(
                    "{} handle={:#06x} {}",
                    connection.peer_address,
                    handle,
                    if device.is_encrypted_on_handle(handle) {
                        "ENCRYPTED"
                    } else {
                        "NOT ENCRYPTED"
                    }
                )
            })
        });
        let phy = self
            .connection_phy
            .map(|(tx, rx)| format!(" RX={}/TX={}", phy_name(rx), phy_name(tx)))
            .unwrap_or_default();
        let rssi = self.connection_rssi.map(rssi_bar).unwrap_or_default();
        println!(
            "SCAN: {} | CONNECTION: {}{} | ATT_MTU: {} | {}",
            if self.scanning { "ON" } else { "OFF" },
            connection.unwrap_or_else(|| "NONE".into()),
            phy,
            self.client.mtu(),
            rssi
        );
    }

    fn show_scan(&self) {
        if self.scan_results.is_empty() {
            println!("no scan results");
        }
        for result in self.scan_results.values() {
            println!("{}", result.display());
        }
    }

    fn show_local_services(&self) {
        let mut previous = None;
        for attribute in &self.local_attributes {
            if previous.as_ref() != Some(&attribute.service_uuid) {
                println!("SERVICE {}", attribute.service_uuid.to_hex_str("-"));
                previous = Some(attribute.service_uuid.clone());
            }
            println!(
                "  CHARACTERISTIC #{:04X} properties={:#04x} {}",
                attribute.handle,
                attribute.properties,
                attribute.characteristic_uuid.to_hex_str("-")
            );
        }
    }

    fn show_remote_services(&self) {
        if self.remote.services.is_empty() {
            println!("no remote services discovered");
            return;
        }
        for service in &self.remote.services {
            println!(
                "SERVICE #{:04X}..#{:04X} {}",
                service.handle,
                service.end_group_handle,
                service.uuid.to_hex_str("-")
            );
            for entry in self
                .remote
                .characteristics
                .iter()
                .filter(|entry| entry.service.handle == service.handle)
            {
                println!(
                    "  CHARACTERISTIC #{:04X} properties={:#04x} {}",
                    entry.characteristic.handle,
                    entry.characteristic.properties,
                    entry.characteristic.uuid.to_hex_str("-")
                );
                for descriptor in &entry.descriptors {
                    println!(
                        "    DESCRIPTOR #{:04X} {}",
                        descriptor.handle,
                        descriptor.uuid.to_hex_str("-")
                    );
                }
            }
        }
    }

    fn show_local_values(&self) -> Result<(), String> {
        let values = self
            .local_values
            .lock()
            .map_err(|_| "local value lock poisoned".to_string())?;
        for attribute in &self.local_attributes {
            println!(
                "{} {} #{:04X} 0x{}",
                attribute.service_uuid.to_hex_str("-"),
                attribute.characteristic_uuid.to_hex_str("-"),
                attribute.handle,
                values
                    .get(&attribute.handle)
                    .map(|value| hex(value))
                    .unwrap_or_default()
            );
        }
        Ok(())
    }

    fn show_remote_values(&self) {
        for entry in &self.remote.characteristics {
            if let Some(value) = self.client.cached_value(entry.characteristic.handle) {
                println!(
                    "{} {} #{:04X} 0x{}",
                    entry.service.uuid.to_hex_str("-"),
                    entry.characteristic.uuid.to_hex_str("-"),
                    entry.characteristic.handle,
                    hex(value)
                );
            }
        }
    }

    fn show_view(&self, view: View, device: &Device) -> Result<(), String> {
        match view {
            View::Scan => self.show_scan(),
            View::Log => println!("logs are emitted inline in the Rust console"),
            View::Device => self.show_device(device),
            View::LocalServices => self.show_local_services(),
            View::RemoteServices => self.show_remote_services(),
            View::LocalValues => self.show_local_values()?,
            View::RemoteValues => self.show_remote_values(),
        }
        Ok(())
    }

    fn execute(
        &mut self,
        host: &mut ExternalHost,
        device: &mut Device,
        command_value: ConsoleCommand,
    ) -> Result<bool, String> {
        match command_value {
            ConsoleCommand::Scan(ScanAction::Clear) => {
                self.scan_results.clear();
                println!("scan results cleared");
            }
            ConsoleCommand::Scan(ScanAction::Switch(action, filter)) => {
                if let Some(filter) = filter {
                    let (kind, pattern) = filter.split_once(':').ok_or_else(|| {
                        "expected filter=address:<regular-expression>".to_string()
                    })?;
                    if kind != "address" {
                        return Err("available scan filter: address".into());
                    }
                    self.address_filter = Regex::new(pattern).map_err(|error| error.to_string())?;
                    self.scan_results.retain(|_, result| {
                        self.address_filter
                            .is_match(&result.address.to_string(false))
                    });
                }
                let enabled = match action {
                    SwitchAction::Toggle => !self.scanning,
                    SwitchAction::On => true,
                    SwitchAction::Off => false,
                };
                self.set_scanning(host, enabled)?;
            }
            ConsoleCommand::Advertise(action) => {
                let enabled = match action {
                    SwitchAction::Toggle => !self.advertising,
                    SwitchAction::On => true,
                    SwitchAction::Off => false,
                };
                self.set_advertising(host, enabled)?;
            }
            ConsoleCommand::Rssi(action) => {
                self.monitor_rssi = match action {
                    SwitchAction::Toggle => !self.monitor_rssi,
                    SwitchAction::On => true,
                    SwitchAction::Off => false,
                };
                self.last_rssi = Instant::now() - RSSI_MONITOR_INTERVAL;
                println!(
                    "RSSI monitoring {}",
                    if self.monitor_rssi { "on" } else { "off" }
                );
            }
            ConsoleCommand::Show(view) => self.show_view(view, device)?,
            ConsoleCommand::FilterAddress(pattern) => {
                self.address_filter = Regex::new(&pattern).map_err(|error| error.to_string())?;
                self.scan_results.retain(|_, result| {
                    self.address_filter
                        .is_match(&result.address.to_string(false))
                });
                println!("address filter: {pattern}");
            }
            ConsoleCommand::Connect { target, phys } => {
                self.connect(host, device, &target, phys.as_deref())?;
            }
            ConsoleCommand::Disconnect => {
                let handle = self.connection_handle(device)?;
                command(
                    host,
                    HciCommand::Disconnect {
                        connection_handle: handle,
                        reason: 0x13,
                    },
                    "disconnecting",
                )?;
            }
            ConsoleCommand::UpdateParameters(parameters) => {
                let handle = self.connection_handle(device)?;
                let (intervals, remainder) = parameters
                    .split_once('/')
                    .ok_or_else(|| "invalid parameter syntax".to_string())?;
                let (latency, supervision) = remainder
                    .split_once('/')
                    .ok_or_else(|| "invalid parameter syntax".to_string())?;
                let (interval_min, interval_max) = intervals
                    .split_once('-')
                    .ok_or_else(|| "invalid interval range".to_string())?;
                let interval_min: u16 = interval_min
                    .parse()
                    .map_err(|_| "invalid minimum interval".to_string())?;
                let interval_max: u16 = interval_max
                    .parse()
                    .map_err(|_| "invalid maximum interval".to_string())?;
                let max_latency = latency
                    .parse()
                    .map_err(|_| "invalid max latency".to_string())?;
                let supervision_timeout: u16 = supervision
                    .parse()
                    .map_err(|_| "invalid supervision timeout".to_string())?;
                command(
                    host,
                    HciCommand::LeConnectionUpdate {
                        connection_handle: handle,
                        connection_interval_min: interval_min.saturating_mul(4) / 5,
                        connection_interval_max: interval_max.saturating_mul(4) / 5,
                        max_latency,
                        supervision_timeout: supervision_timeout / 10,
                        min_ce_length: 0,
                        max_ce_length: 0,
                    },
                    "updating connection parameters",
                )?;
            }
            ConsoleCommand::Encrypt => {
                let handle = self.connection_handle(device)?;
                self.pairing = None;
                let mut pairing = LePairingSession::accept_all(
                    device,
                    handle,
                    self.local_address.clone(),
                    PairingConfig {
                        mitm: false,
                        ..PairingConfig::default()
                    },
                )
                .map_err(|error| error.to_string())?;
                pairing
                    .pair(host, device, PAIRING_TIMEOUT)
                    .map_err(|error| error.to_string())?;
                println!("connection encrypted");
            }
            ConsoleCommand::DiscoverServices => self.discover_services(host, device)?,
            ConsoleCommand::DiscoverAttributes => self.discover_attributes(host, device)?,
            ConsoleCommand::RequestMtu(mtu) => self.request_mtu(host, device, mtu)?,
            ConsoleCommand::Read(selector) => self.read_remote(host, device, &selector)?,
            ConsoleCommand::Write { selector, value } => {
                self.write_remote(host, device, &selector, &value)?;
            }
            ConsoleCommand::LocalWrite { selector, value } => {
                self.write_local(host, device, &selector, &value)?;
            }
            ConsoleCommand::Subscribe(selector) => {
                self.subscribe(host, device, &selector, true)?;
            }
            ConsoleCommand::Unsubscribe(selector) => {
                self.subscribe(host, device, &selector, false)?;
            }
            ConsoleCommand::GetPhy => self.read_phy(host, device)?,
            ConsoleCommand::SetPhy(value) => {
                let handle = self.connection_handle(device)?;
                let (tx, rx) = parse_tx_rx_phys(&value)?;
                command(
                    host,
                    HciCommand::LeSetPhy {
                        connection_handle: handle,
                        all_phys: u8::from(tx.is_none()) | (u8::from(rx.is_none()) << 1),
                        tx_phys: tx.unwrap_or(0),
                        rx_phys: rx.unwrap_or(0),
                        phy_options: 0,
                    },
                    "setting connection PHY",
                )?;
            }
            ConsoleCommand::SetDefaultPhy(value) => {
                let (tx, rx) = parse_tx_rx_phys(&value)?;
                command(
                    host,
                    HciCommand::LeSetDefaultPhy {
                        all_phys: u8::from(tx.is_none()) | (u8::from(rx.is_none()) << 1),
                        tx_phys: tx.unwrap_or(0),
                        rx_phys: rx.unwrap_or(0),
                    },
                    "setting default PHY",
                )?;
            }
            ConsoleCommand::Exit => return Ok(false),
        }
        Ok(true)
    }
}

fn phy_name(phy: u8) -> &'static str {
    match phy {
        1 => "1M",
        2 => "2M",
        3 => "CODED",
        _ => "UNKNOWN",
    }
}

fn read_public_address(host: &mut ExternalHost) -> Result<Address, String> {
    let response = command(host, HciCommand::ReadBdAddr, "reading public address")?;
    match response.return_parameters() {
        Some(ReturnParameters::ReadBdAddr { bd_addr, .. }) => Ok(bd_addr.clone()),
        _ => Err("controller did not return a public address".into()),
    }
}

fn print_help() {
    println!("commands:");
    println!("  scan [on [filter=address:REGEX]|off|clear]");
    println!("  advertise [on|off]                  rssi [on|off]");
    println!("  show scan|log|device|local-services|remote-services|local-values|remote-values");
    println!("  filter address REGEX                connect ADDRESS [1m,2m,coded|*]");
    println!("  disconnect                          encrypt");
    println!("  update-parameters MIN-MAX/LATENCY/SUPERVISION");
    println!("  get-phy                             set-phy PHYS[/PHYS]");
    println!("  set-default-phy PHYS[/PHYS]         request-mtu MTU");
    println!("  discover services|attributes        read ATTRIBUTE");
    println!("  write ATTRIBUTE VALUE               local-write ATTRIBUTE VALUE");
    println!("  subscribe ATTRIBUTE                 unsubscribe ATTRIBUTE");
    println!("  quit | exit");
}

fn run(args: Args) -> Result<(), String> {
    let config = load_device_config(args.device_config.as_deref())?;
    let (local_server, local_values, local_attributes) = build_local_gatt(&config.name)?;
    let device_server = local_server.clone();
    let transport = open_split_transport(&args.transport).map_err(|error| error.to_string())?;
    let mut host = ExternalHost::new(transport);
    let mut device = Device::with_server(0, device_server);
    host.initialize_device(&mut device, COMMAND_TIMEOUT)
        .map_err(|error| error.to_string())?;
    let public_address = read_public_address(&mut host)?;
    let mut console = ConsoleRuntime::new(
        config,
        public_address,
        local_server,
        local_values,
        local_attributes,
    );
    console.configure_controller(&mut host)?;
    println!("Bumble interactive console");
    console.show_device(&device);
    print_help();
    let input = spawn_input();
    print!("> ");
    std::io::stdout()
        .flush()
        .map_err(|error| error.to_string())?;
    let mut running = true;
    while running {
        device.poll(&mut host);
        console.process_reports(&mut device);
        console.process_connection_state(&device)?;
        console.drive_pairing(&mut host, &mut device)?;
        console.process_att(&mut host, &mut device)?;
        if console.monitor_rssi && console.active_handle.is_some() {
            console.maybe_read_rssi(&mut host, &device)?;
        }
        loop {
            match input.try_recv() {
                Ok(InputMessage::Line(line)) => {
                    if line.trim().is_empty() {
                        print!("> ");
                        std::io::stdout()
                            .flush()
                            .map_err(|error| error.to_string())?;
                        continue;
                    }
                    match parse_console_command(&line) {
                        Ok(command_value) => {
                            running = console.execute(&mut host, &mut device, command_value)?;
                        }
                        Err(error) => eprintln!("{error}"),
                    }
                    if running {
                        print!("> ");
                        std::io::stdout()
                            .flush()
                            .map_err(|error| error.to_string())?;
                    }
                }
                Ok(InputMessage::Ended) => {
                    running = false;
                    break;
                }
                Ok(InputMessage::Error(error)) => return Err(error),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    running = false;
                    break;
                }
            }
        }
        if !running {
            break;
        }
        match host
            .wait_for_activity(POLL_INTERVAL)
            .map_err(|error| error.to_string())?
        {
            ExternalHostActivity::Packet | ExternalHostActivity::Timeout => {}
            ExternalHostActivity::Ended => break,
        }
    }
    if console.scanning {
        let _ = console.set_scanning(&mut host, false);
    }
    if console.advertising {
        let _ = console.set_advertising(&mut host, false);
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
    use bumble_controller::{Controller, LocalLink as ControllerLocalLink};
    use bumble_gatt::{Characteristic, Service};
    use bumble_host::pump as pump_devices;

    struct LiveAttTransport<'a> {
        link: &'a mut ControllerLocalLink,
        devices: &'a mut [Device; 2],
        client_handle: u16,
    }

    impl AttTransport for LiveAttTransport<'_> {
        fn request(&mut self, request: &AttPdu) -> AttPdu {
            assert!(self.devices[0].send_att_on_handle(self.link, self.client_handle, request));
            pump_devices(self.link, self.devices);
            let mut responses = self.devices[0].take_inbox_on_handle(self.client_handle);
            assert_eq!(responses.len(), 1, "expected exactly one ATT response");
            responses.pop().unwrap()
        }
    }

    #[test]
    fn parses_upstream_cli_and_command_surface() {
        assert_eq!(
            parse_args(["console", "--device-config=device.json", "usb:0",].map(str::to_string))
                .unwrap(),
            Args {
                device_config: Some(PathBuf::from("device.json")),
                transport: "usb:0".into(),
            }
        );
        assert_eq!(
            parse_console_command("scan on filter=address:^C4:").unwrap(),
            ConsoleCommand::Scan(ScanAction::Switch(
                SwitchAction::On,
                Some("address:^C4:".into())
            ))
        );
        assert_eq!(
            parse_console_command("connect C4:F2:17:1A:1D:BB 1m,2m").unwrap(),
            ConsoleCommand::Connect {
                target: "C4:F2:17:1A:1D:BB".into(),
                phys: Some("1m,2m".into()),
            }
        );
        assert_eq!(
            parse_console_command("update-parameters 30-50/0/420").unwrap(),
            ConsoleCommand::UpdateParameters("30-50/0/420".into())
        );
        assert_eq!(
            parse_console_command("local-write #0003 0x0102").unwrap(),
            ConsoleCommand::LocalWrite {
                selector: "#0003".into(),
                value: "0x0102".into(),
            }
        );
        assert_eq!(parse_console_command("quit").unwrap(), ConsoleCommand::Exit);
    }

    #[test]
    fn parses_phys_values_and_rssi_bars() {
        assert_eq!(parse_phys("1m,2m").unwrap(), Some(0x03));
        assert_eq!(parse_phys("coded").unwrap(), Some(0x04));
        assert_eq!(parse_phys("*").unwrap(), None);
        assert_eq!(parse_tx_rx_phys("2m/coded").unwrap(), (Some(2), Some(4)));
        assert!(parse_phys("wifi").is_err());
        assert_eq!(parse_value("0x0102").unwrap(), [1, 2]);
        assert_eq!(parse_value("513").unwrap(), [1, 2]);
        assert_eq!(parse_value("hello").unwrap(), b"hello");
        assert_eq!(rssi_bar(-100), "-100 ");
        assert!(rssi_bar(-30).ends_with(&"█".repeat(DEFAULT_RSSI_BAR_WIDTH)));
    }

    #[test]
    fn scan_result_renders_name_type_and_connectability() {
        let result = ScanResult {
            address: Address::parse("C4:F2:17:1A:1D:BB", AddressType::RANDOM_DEVICE).unwrap(),
            address_type: 1,
            data: vec![
                7,
                AdvertisingDataType::COMPLETE_LOCAL_NAME.0,
                b'B',
                b'u',
                b'm',
                b'b',
                b'l',
                b'e',
            ],
            rssi: -45,
            connectable: true,
        };
        let display = result.display();
        assert!(display.contains("+ C4:F2:17:1A:1D:BB [R]"));
        assert!(display.contains("Bumble"));
    }

    #[test]
    fn remote_database_discovers_and_selects_real_gatt_attributes() {
        let service_uuid = Uuid::from_16_bits(0x180A);
        let characteristic_uuid = Uuid::from_16_bits(0x2A29);
        let mut server = GattServer::new(vec![Service {
            uuid: service_uuid.clone(),
            characteristics: vec![Characteristic {
                uuid: characteristic_uuid.clone(),
                properties: properties::READ | properties::WRITE,
                value: b"Google".to_vec(),
            }],
        }]);
        let mut client = GattClient::new();
        let database = RemoteDatabase::discover_all(&mut client, &mut server).unwrap();
        let selected = database
            .find_characteristic(&format!(
                "{}.{}",
                service_uuid.to_hex_str("-"),
                characteristic_uuid.to_hex_str("-")
            ))
            .unwrap();
        assert_eq!(selected.characteristic.handle, 3);
        assert_eq!(
            database
                .find_characteristic(&format!("#{:04X}", selected.characteristic.handle))
                .unwrap()
                .characteristic
                .uuid,
            characteristic_uuid
        );
        assert_eq!(
            client
                .read_value(&mut server, selected.characteristic.handle, false)
                .unwrap(),
            b"Google"
        );
    }

    #[test]
    fn cloned_local_server_observes_console_value_updates() {
        let (server, values, attributes) = build_local_gatt("Bumble").unwrap();
        let name = attributes
            .iter()
            .find(|attribute| {
                attribute.characteristic_uuid == Uuid::from_16_bits(DEVICE_NAME_CHARACTERISTIC)
            })
            .unwrap();
        let mut device_server = server.clone();
        let mut client = GattClient::new();
        assert_eq!(
            client
                .read_value(&mut device_server, name.handle, false)
                .unwrap(),
            b"Bumble"
        );
        values
            .lock()
            .unwrap()
            .insert(name.handle, b"Renamed".to_vec());
        assert_eq!(
            client
                .read_value(&mut device_server, name.handle, false)
                .unwrap(),
            b"Renamed"
        );
    }

    #[test]
    fn production_local_database_runs_over_two_controllers() {
        let central_address =
            Address::parse("C4:F2:17:1A:1D:AA", AddressType::RANDOM_DEVICE).unwrap();
        let peripheral_address =
            Address::parse("C4:F2:17:1A:1D:BB", AddressType::RANDOM_DEVICE).unwrap();
        let (server, values, attributes) = build_local_gatt("Bumble Console").unwrap();
        let name = attributes
            .iter()
            .find(|attribute| {
                attribute.characteristic_uuid == Uuid::from_16_bits(DEVICE_NAME_CHARACTERISTIC)
            })
            .unwrap()
            .clone();
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
        pump_devices(&mut link, &mut devices);
        let client_handle = devices[0].connection_handle().unwrap();
        let mut client = GattClient::new();
        let mut transport = LiveAttTransport {
            link: &mut link,
            devices: &mut devices,
            client_handle,
        };
        let database = RemoteDatabase::discover_all(&mut client, &mut transport).unwrap();
        assert_eq!(database.services[0].uuid, Uuid::from_16_bits(0x1800));
        assert_eq!(
            client
                .read_value(&mut transport, name.handle, false)
                .unwrap(),
            b"Bumble Console"
        );
        values
            .lock()
            .unwrap()
            .insert(name.handle, b"Live Rename".to_vec());
        assert_eq!(
            client
                .read_value(&mut transport, name.handle, false)
                .unwrap(),
            b"Live Rename"
        );
    }
}
