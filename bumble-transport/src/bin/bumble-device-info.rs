use bumble::advertising_data::Type as AdvertisingDataType;
use bumble::{Address, AddressType, AdvertisingData};
use bumble_gatt::{AttTransport, GattClient};
use bumble_hci::Command;
use bumble_host::Device;
use bumble_profiles::battery_service::BatteryServiceProxy;
use bumble_profiles::device_information_service::DeviceInformationServiceProxy;
use bumble_profiles::gap::GenericAccessServiceProxy;
use bumble_profiles::pacs::PublishedAudioCapabilitiesServiceProxy;
use bumble_profiles::tmap::TelephonyAndMediaAudioServiceProxy;
use bumble_profiles::vcs::VolumeControlServiceProxy;
use bumble_smp::PairingConfig;
use bumble_transport::{
    open_split_transport, CommandResponse, ExternalAttTransport, ExternalHost,
    ExternalHostActivity, LePairingSession,
};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Duration, Instant};

const DEFAULT_ADDRESS: &str = "F0:F1:F2:F3:F4:F5";
const COMMAND_TIMEOUT: Duration = Duration::from_secs(5);
const PROCEDURE_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Clone, Debug, PartialEq, Eq)]
struct Args {
    device_config: Option<PathBuf>,
    encrypt: bool,
    transport: String,
    address_or_name: Option<String>,
}

fn usage() -> &'static str {
    "usage: bumble-device-info [--device-config PATH] [--encrypt] <transport> [address-or-name]"
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

fn parse_args(arguments: impl IntoIterator<Item = String>) -> Result<Args, String> {
    let mut arguments = arguments.into_iter();
    let _program = arguments.next();
    let mut device_config = None;
    let mut encrypt = false;
    let mut positional = Vec::new();
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "-h" | "--help" => return Err(usage().into()),
            "--encrypt" => {
                encrypt = true;
                continue;
            }
            _ => {}
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
    if !(1..=2).contains(&positional.len()) {
        return Err(usage().into());
    }
    Ok(Args {
        device_config,
        encrypt,
        transport: positional.remove(0),
        address_or_name: positional.pop(),
    })
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
        let handle = match peer {
            Some(peer) => device.connection_handle_for_peer(peer),
            None => device.connection_handle(),
        };
        if let Some(handle) = handle {
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
    let disable_result = command(
        host,
        Command::LeSetScanEnable {
            le_scan_enable: 0,
            filter_duplicates: 0,
        },
        "disabling scan",
    );
    match (result, disable_result) {
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

fn advertise_and_wait(
    host: &mut ExternalHost,
    device: &mut Device,
    own_address_type: u8,
) -> Result<u16, String> {
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
            advertising_data: vec![2, 0x01, 0x06, 7, 0x09, b'B', b'u', b'm', b'b', b'l', b'e'],
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
    wait_for_connection(host, device, None)
}

fn append_section(output: &mut Vec<String>, result: Result<Option<Vec<String>>, String>) {
    match result {
        Ok(Some(lines)) => output.extend(lines),
        Ok(None) => {}
        Err(error) => output.push(format!("ERROR: {error}")),
    }
}

fn gap_information(
    client: &mut GattClient,
    transport: &mut impl AttTransport,
) -> Result<Option<Vec<String>>, String> {
    let Some(proxy) = GenericAccessServiceProxy::discover(client, transport)
        .map_err(|error| error.to_string())?
    else {
        return Ok(None);
    };
    let mut lines = vec!["### Generic Access Profile".into()];
    if let Some(characteristic) = &proxy.device_name {
        let value = characteristic
            .read_value(client, transport, false)
            .map_err(|error| error.to_string())?;
        lines.push(format!(" Device Name: {value}"));
    }
    if let Some(characteristic) = &proxy.appearance {
        let value = characteristic
            .read_value(client, transport, false)
            .map_err(|error| error.to_string())?;
        lines.push(format!(" Appearance: {value:?}"));
    }
    lines.push(String::new());
    Ok(Some(lines))
}

fn device_information(
    client: &mut GattClient,
    transport: &mut impl AttTransport,
) -> Result<Option<Vec<String>>, String> {
    let Some(proxy) = DeviceInformationServiceProxy::discover(client, transport)
        .map_err(|error| error.to_string())?
    else {
        return Ok(None);
    };
    let mut lines = vec!["### Device Information".into()];
    for (label, characteristic) in [
        ("  Manufacturer Name:", proxy.manufacturer_name.as_ref()),
        ("  Model Number:     ", proxy.model_number.as_ref()),
        ("  Serial Number:    ", proxy.serial_number.as_ref()),
        ("  Firmware Revision:", proxy.firmware_revision.as_ref()),
    ] {
        if let Some(characteristic) = characteristic {
            let value = characteristic
                .read_value(client, transport, false)
                .map_err(|error| error.to_string())?;
            lines.push(format!("{label} {value}"));
        }
    }
    lines.push(String::new());
    Ok(Some(lines))
}

fn battery_information(
    client: &mut GattClient,
    transport: &mut impl AttTransport,
) -> Result<Option<Vec<String>>, String> {
    let Some(proxy) =
        BatteryServiceProxy::discover(client, transport).map_err(|error| error.to_string())?
    else {
        return Ok(None);
    };
    let level = proxy
        .battery_level
        .read_value(client, transport, false)
        .map_err(|error| error.to_string())?;
    Ok(Some(vec![
        "### Battery Information".into(),
        format!("  Battery Level: {level}"),
        String::new(),
    ]))
}

fn tmap_information(
    client: &mut GattClient,
    transport: &mut impl AttTransport,
) -> Result<Option<Vec<String>>, String> {
    let Some(proxy) = TelephonyAndMediaAudioServiceProxy::discover(client, transport)
        .map_err(|error| error.to_string())?
    else {
        return Ok(None);
    };
    let role = proxy
        .read_role(client, transport)
        .map_err(|error| error.to_string())?;
    Ok(Some(vec![
        "### Telephony And Media Audio Service".into(),
        format!("  Role: {role:?}"),
        String::new(),
    ]))
}

fn pacs_information(
    client: &mut GattClient,
    transport: &mut impl AttTransport,
) -> Result<Option<Vec<String>>, String> {
    let Some(proxy) = PublishedAudioCapabilitiesServiceProxy::discover(client, transport)
        .map_err(|error| error.to_string())?
    else {
        return Ok(None);
    };
    let available = proxy
        .read_available_contexts(client, transport)
        .map_err(|error| error.to_string())?;
    let supported = proxy
        .read_supported_contexts(client, transport)
        .map_err(|error| error.to_string())?;
    let mut lines = vec![
        "### Published Audio Capabilities Service".into(),
        format!("  Available Audio Contexts: {available:?}"),
        format!("  Supported Audio Contexts: {supported:?}"),
    ];
    if let Some(characteristic) = &proxy.sink_pac {
        let value =
            PublishedAudioCapabilitiesServiceProxy::read_pac(characteristic, client, transport)
                .map_err(|error| error.to_string())?;
        lines.push(format!("  Sink PAC:                 {value:?}"));
    }
    if let Some(characteristic) = &proxy.sink_audio_locations {
        let value = PublishedAudioCapabilitiesServiceProxy::read_audio_locations(
            characteristic,
            client,
            transport,
        )
        .map_err(|error| error.to_string())?;
        lines.push(format!("  Sink Audio Locations:     {value:?}"));
    }
    if let Some(characteristic) = &proxy.source_pac {
        let value =
            PublishedAudioCapabilitiesServiceProxy::read_pac(characteristic, client, transport)
                .map_err(|error| error.to_string())?;
        lines.push(format!("  Source PAC:               {value:?}"));
    }
    if let Some(characteristic) = &proxy.source_audio_locations {
        let value = PublishedAudioCapabilitiesServiceProxy::read_audio_locations(
            characteristic,
            client,
            transport,
        )
        .map_err(|error| error.to_string())?;
        lines.push(format!("  Source Audio Locations:   {value:?}"));
    }
    lines.push(String::new());
    Ok(Some(lines))
}

fn volume_information(
    client: &mut GattClient,
    transport: &mut impl AttTransport,
) -> Result<Option<Vec<String>>, String> {
    let Some(proxy) = VolumeControlServiceProxy::discover(client, transport)
        .map_err(|error| error.to_string())?
    else {
        return Ok(None);
    };
    let state = proxy
        .read_volume_state(client, transport)
        .map_err(|error| error.to_string())?;
    let flags = proxy
        .read_volume_flags(client, transport)
        .map_err(|error| error.to_string())?;
    Ok(Some(vec![
        "### Volume Control Service".into(),
        format!("  Volume State: {state:?}"),
        format!("  Volume Flags: {flags:?}"),
    ]))
}

fn show_device_info(transport: &mut impl AttTransport) -> Result<String, String> {
    let mut client = GattClient::new();
    let services = client
        .discover_services(transport)
        .map_err(|error| error.to_string())?;
    let mut output = vec![
        "### Discovering Services and Characteristics".to_string(),
        "=== Services ===".to_string(),
    ];
    for service in &services {
        output.push(format!(
            "SERVICE {:#06x}..={:#06x} {:?}",
            service.handle, service.end_group_handle, service.uuid
        ));
        for characteristic in client
            .discover_characteristics(transport, service)
            .map_err(|error| error.to_string())?
        {
            output.push(format!(
                "  CHARACTERISTIC declaration={:#06x} value={:#06x} properties={:#04x} {:?}",
                characteristic.declaration_handle,
                characteristic.handle,
                characteristic.properties,
                characteristic.uuid
            ));
        }
    }
    output.push(String::new());

    let section = gap_information(&mut client, transport);
    append_section(&mut output, section);
    let section = device_information(&mut client, transport);
    append_section(&mut output, section);
    let section = battery_information(&mut client, transport);
    append_section(&mut output, section);
    let section = tmap_information(&mut client, transport);
    append_section(&mut output, section);
    let section = pacs_information(&mut client, transport);
    append_section(&mut output, section);
    let section = volume_information(&mut client, transport);
    append_section(&mut output, section);

    Ok(output.join("\n"))
}

fn run(args: Args) -> Result<(), String> {
    let local_address = configured_address(args.device_config.as_deref())?;
    let transport = open_split_transport(&args.transport).map_err(|error| error.to_string())?;
    let mut host = ExternalHost::new(transport);
    let mut device = Device::new(0);
    host.initialize_device(&mut device, COMMAND_TIMEOUT)
        .map_err(|error| error.to_string())?;
    let own_address_type = u8::from(!local_address.is_public());
    if own_address_type != 0 {
        command(
            &mut host,
            Command::LeSetRandomAddress {
                random_address: local_address.clone(),
            },
            "setting local random address",
        )?;
    }

    let handle = if let Some(target) = args.address_or_name {
        let peer = match Address::parse(&target, AddressType::RANDOM_DEVICE) {
            Ok(address) => address,
            Err(_) => resolve_name(&mut host, &mut device, &target, own_address_type)?,
        };
        println!(">>> Connecting to {peer}...");
        let handle = connect(&mut host, &mut device, peer, own_address_type)?;
        println!(">>> Connected");
        handle
    } else {
        println!("### Waiting for connection...");
        advertise_and_wait(&mut host, &mut device, own_address_type)?
    };
    if args.encrypt {
        println!("+++ Encrypting connection...");
        let mut pairing = LePairingSession::accept_all(
            &device,
            handle,
            local_address,
            PairingConfig {
                mitm: false,
                ..PairingConfig::default()
            },
        )
        .map_err(|error| error.to_string())?;
        pairing
            .pair(&mut host, &mut device, PROCEDURE_TIMEOUT)
            .map_err(|error| error.to_string())?;
        println!("+++ Encryption established");
    }
    let mut att = ExternalAttTransport::new(&mut host, &mut device, handle, PROCEDURE_TIMEOUT)
        .map_err(|error| error.to_string())?;
    println!("{}", show_device_info(&mut att)?);
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
    use bumble::{appearance::Category, Appearance};
    use bumble_att::AttPdu;
    use bumble_gatt::{AccessContext, GattServer};
    use bumble_profiles::bap::{AudioLocation, ContextType};
    use bumble_profiles::battery_service::BatteryService;
    use bumble_profiles::device_information_service::DeviceInformationService;
    use bumble_profiles::gap::GenericAccessService;
    use bumble_profiles::pacs::{AudioContexts, PublishedAudioCapabilitiesService};
    use bumble_profiles::tmap::{Role, TelephonyAndMediaAudioService};
    use bumble_profiles::vcs::{VolumeControlService, VolumeFlags, VolumeState};

    #[test]
    fn parses_upstream_cli_shape() {
        assert_eq!(
            parse_args(
                [
                    "device-info",
                    "--device-config",
                    "device.json",
                    "--encrypt",
                    "usb:0",
                    "C4:F2:17:1A:1D:BB",
                ]
                .map(str::to_string)
            ),
            Ok(Args {
                device_config: Some(PathBuf::from("device.json")),
                encrypt: true,
                transport: "usb:0".into(),
                address_or_name: Some("C4:F2:17:1A:1D:BB".into()),
            })
        );
        assert!(parse_args(["device-info"].map(str::to_string)).is_err());
        assert!(parse_args(["device-info", "one", "two", "three"].map(str::to_string)).is_err());
    }

    struct EncryptedTransport<'a>(&'a mut GattServer);

    impl AttTransport for EncryptedTransport<'_> {
        fn request(&mut self, request: &AttPdu) -> AttPdu {
            self.0.on_request_with_context(
                request,
                AccessContext {
                    bearer_id: 1,
                    encrypted: true,
                    authenticated: false,
                    authorized: false,
                },
            )
        }
    }

    #[test]
    fn renders_all_upstream_profile_sections() {
        let gap = GenericAccessService::new("Bumble", Appearance::new(Category::COMPUTER, 3));
        let device_information = DeviceInformationService {
            manufacturer_name: Some("Google".into()),
            model_number: Some("Bumble-Rust".into()),
            serial_number: Some("1234".into()),
            firmware_revision: Some("1.0".into()),
            ..Default::default()
        };
        let battery = BatteryService::with_level(91);
        let tmap =
            TelephonyAndMediaAudioService::new(Role::CALL_GATEWAY | Role::UNICAST_MEDIA_SENDER);
        let contexts = AudioContexts {
            sink: ContextType::MEDIA,
            source: ContextType::CONVERSATIONAL,
        };
        let mut pacs = PublishedAudioCapabilitiesService::new(contexts, contexts);
        pacs.sink_audio_locations = Some(AudioLocation::FRONT_LEFT | AudioLocation::FRONT_RIGHT);
        let vcs = VolumeControlService::new()
            .initial_state(VolumeState {
                volume_setting: 64,
                mute: 0,
                change_counter: 7,
            })
            .volume_flags(VolumeFlags::VOLUME_SETTING_PERSISTED);
        let mut server = GattServer::from_definitions(vec![
            gap.definition(),
            device_information.definition().unwrap(),
            battery.definition(),
            tmap.definition(),
            pacs.definition().unwrap(),
            vcs.definition(),
        ])
        .unwrap();
        battery.bind(&mut server).unwrap();
        vcs.bind(&mut server).unwrap();

        let output = show_device_info(&mut EncryptedTransport(&mut server)).unwrap();
        for expected in [
            "=== Services ===",
            "### Generic Access Profile",
            " Device Name: Bumble",
            "### Device Information",
            "  Manufacturer Name: Google",
            "  Model Number:      Bumble-Rust",
            "  Serial Number:     1234",
            "  Firmware Revision: 1.0",
            "### Battery Information",
            "  Battery Level: 91",
            "### Telephony And Media Audio Service",
            "### Published Audio Capabilities Service",
            "  Sink Audio Locations:",
            "### Volume Control Service",
            "volume_setting: 64",
            "VolumeFlags(1)",
        ] {
            assert!(
                output.contains(expected),
                "missing {expected:?} in:\n{output}"
            );
        }
    }

    #[test]
    fn profile_read_errors_do_not_hide_later_sections() {
        let mut gap = GenericAccessService::default().definition();
        gap.characteristics[1].value = vec![0];
        let battery = BatteryService::with_level(50);
        let mut server = GattServer::from_definitions(vec![gap, battery.definition()]).unwrap();
        battery.bind(&mut server).unwrap();

        let output = show_device_info(&mut server).unwrap();
        assert!(output.contains("ERROR:"));
        assert!(output.contains("### Battery Information"));
        assert!(output.find("ERROR:") < output.find("### Battery Information"));
    }
}
