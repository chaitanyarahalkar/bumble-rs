use bumble::advertising_data::Type as AdvertisingDataType;
use bumble::{Address, AddressType, AdvertisingData};
use bumble_gatt::{AttTransport, GattClient, GattError};
use bumble_hci::Command;
use bumble_host::Device;
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
    "usage: bumble-gatt-dump [--device-config PATH] [--encrypt] <transport> [address-or-name]"
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

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn dump_gatt(transport: &mut impl AttTransport) -> Result<String, GattError> {
    let mut client = GattClient::new();
    let mut output = vec!["### Discovering Services and Characteristics".to_string()];
    let services = client.discover_services(transport)?;
    output.push("=== Services ===".into());
    for service in &services {
        output.push(format!(
            "SERVICE {:#06x}..={:#06x} {:?}",
            service.handle, service.end_group_handle, service.uuid
        ));
        for characteristic in client.discover_characteristics(transport, service)? {
            output.push(format!(
                "  CHARACTERISTIC declaration={:#06x} value={:#06x} properties={:#04x} {:?}",
                characteristic.declaration_handle,
                characteristic.handle,
                characteristic.properties,
                characteristic.uuid
            ));
            for descriptor in client.discover_descriptors(transport, &characteristic)? {
                output.push(format!(
                    "    DESCRIPTOR {:#06x} {:?}",
                    descriptor.handle, descriptor.uuid
                ));
            }
        }
    }

    output.push(String::new());
    output.push("=== All Attributes ===".into());
    for attribute in client.discover_attributes(transport)? {
        output.push(format!(
            "ATTRIBUTE {:#06x} {:?}",
            attribute.handle, attribute.uuid
        ));
        match client.read_value(transport, attribute.handle, false) {
            Ok(value) => output.push(format!("  {}", hex(&value))),
            Err(GattError::Transport(error)) => return Err(GattError::Transport(error)),
            Err(error) => output.push(format!("  ERROR: {error}")),
        }
    }
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
    println!(
        "{}",
        dump_gatt(&mut att).map_err(|error| error.to_string())?
    );
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
    use bumble::Uuid;
    use bumble_gatt::{properties, Characteristic, GattServer, Service};

    #[test]
    fn parses_upstream_cli_shape() {
        assert_eq!(
            parse_args(
                [
                    "gatt-dump",
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
        assert!(parse_args(["gatt-dump"].map(str::to_string)).is_err());
        assert!(parse_args(["gatt-dump", "one", "two", "three"].map(str::to_string)).is_err());
    }

    #[test]
    fn renders_services_characteristics_descriptors_and_attributes() {
        let mut server = GattServer::new(vec![Service {
            uuid: Uuid::from_16_bits(0x180A),
            characteristics: vec![Characteristic {
                uuid: Uuid::from_16_bits(0x2A29),
                properties: properties::READ,
                value: b"Bumble".to_vec(),
            }],
        }]);
        let output = dump_gatt(&mut server).unwrap();
        assert!(output.contains("SERVICE 0x0001"));
        assert!(output.contains("CHARACTERISTIC declaration=0x0002 value=0x0003"));
        assert!(output.contains("=== All Attributes ==="));
        assert!(output.contains("42756d626c65"));
    }

    #[test]
    fn extracts_complete_and_shortened_names() {
        assert_eq!(
            report_name(&[4, 0x09, b'F', b'o', b'o']),
            Some("Foo".into())
        );
        assert_eq!(
            report_name(&[4, 0x08, b'B', b'a', b'r']),
            Some("Bar".into())
        );
    }
}
