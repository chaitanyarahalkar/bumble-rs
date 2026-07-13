use bumble::keys::{JsonKeyStore, Key, KeyStore, KeyStoreError, MemoryKeyStore, PairingKeys};
use bumble::{Address, AddressType};
use bumble_hci::{Command, ReturnParameters};
use bumble_transport::{
    open_transport, CommandResponse, HciCommandChannel, PacketSink, PacketSource,
};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const YELLOW: &str = "\x1b[33m";
const CYAN: &str = "\x1b[36m";
const GREEN: &str = "\x1b[32m";
const RESET: &str = "\x1b[0m";

#[derive(Clone, Debug, PartialEq, Eq)]
enum Source {
    KeyStoreFile(PathBuf),
    Controller {
        transport: String,
        device_config: Option<PathBuf>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Args {
    source: Source,
    namespace: Option<String>,
    address: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ControllerConfig {
    address: Address,
    keystore: Option<String>,
}

impl Default for ControllerConfig {
    fn default() -> Self {
        Self {
            address: Address::from_bytes([0; 6], AddressType::RANDOM_DEVICE),
            keystore: None,
        }
    }
}

fn usage() -> &'static str {
    "usage: bumble-unbond --keystore-file <file> [--namespace <name>] [address]\n       bumble-unbond --hci-transport <transport> [device-config] [address]"
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
    let mut keystore_file = None;
    let mut hci_transport = None;
    let mut namespace = None;
    let mut positional = Vec::new();

    while let Some(argument) = arguments.next() {
        if matches!(argument.as_str(), "-h" | "--help") {
            return Err(usage().into());
        }
        if let Some(value) = option_value(&argument, "--keystore-file", &mut arguments)? {
            keystore_file = Some(PathBuf::from(value));
            continue;
        }
        if let Some(value) = option_value(&argument, "--hci-transport", &mut arguments)? {
            hci_transport = Some(value);
            continue;
        }
        if let Some(value) = option_value(&argument, "--namespace", &mut arguments)? {
            namespace = Some(value);
            continue;
        }
        if argument.starts_with('-') {
            return Err(format!("unknown option {argument:?}"));
        }
        positional.push(argument);
    }

    let (source, address) = match (keystore_file, hci_transport) {
        (Some(file), None) => {
            if positional.len() > 1 {
                return Err("file mode accepts at most one address".into());
            }
            (Source::KeyStoreFile(file), positional.into_iter().next())
        }
        (None, Some(transport)) => {
            if positional.len() > 2 {
                return Err("controller mode accepts device-config and address".into());
            }
            let mut positional = positional.into_iter();
            let device_config = positional.next().map(PathBuf::from);
            let address = positional.next();
            (
                Source::Controller {
                    transport,
                    device_config,
                },
                address,
            )
        }
        (Some(_), Some(_)) => {
            return Err("--keystore-file and --hci-transport are mutually exclusive".into())
        }
        (None, None) => {
            return Err("either --keystore-file or --hci-transport must be specified".into())
        }
    };

    Ok(Args {
        source,
        namespace,
        address,
    })
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn render_key(lines: &mut Vec<String>, name: &str, key: Option<&Key>) {
    let Some(key) = key else {
        return;
    };
    lines.push(format!("  {CYAN}{name}{RESET}:"));
    lines.push(format!("    {GREEN}value{RESET}: {}", hex(&key.value)));
    lines.push(format!(
        "    {GREEN}authenticated{RESET}: {}",
        key.authenticated
    ));
    if let Some(ediv) = key.ediv {
        lines.push(format!("    {GREEN}ediv{RESET}: {ediv}"));
    }
    if let Some(rand) = key.rand.as_deref() {
        lines.push(format!("    {GREEN}rand{RESET}: {}", hex(rand)));
    }
    if let Some(sign_counter) = key.sign_counter {
        lines.push(format!("    {GREEN}sign_counter{RESET}: {sign_counter}"));
    }
}

fn render_keys(name: &str, keys: &PairingKeys) -> String {
    let mut lines = vec![format!("{YELLOW}{name}{RESET}")];
    if let Some(address_type) = keys.address_type {
        lines.push(format!("  {CYAN}address_type{RESET}: {}", address_type.0));
    }
    render_key(&mut lines, "ltk", keys.ltk.as_ref());
    render_key(&mut lines, "ltk_central", keys.ltk_central.as_ref());
    render_key(&mut lines, "ltk_peripheral", keys.ltk_peripheral.as_ref());
    render_key(&mut lines, "irk", keys.irk.as_ref());
    render_key(&mut lines, "csrk", keys.csrk.as_ref());
    render_key(&mut lines, "local_csrk", keys.local_csrk.as_ref());
    render_key(&mut lines, "link_key", keys.link_key.as_ref());
    if let Some(link_key_type) = keys.link_key_type {
        lines.push(format!("  {CYAN}link_key_type{RESET}: {link_key_type}"));
    }
    lines.join("\n")
}

fn execute_with_store<S: KeyStore>(mut store: S, address: Option<&str>) -> Result<String, String> {
    if let Some(address) = address {
        return match store.delete(address) {
            Ok(()) => Ok(String::new()),
            Err(KeyStoreError::NotFound(_)) => Ok("!!! pairing not found".into()),
            Err(error) => Err(error.to_string()),
        };
    }

    store
        .get_all()
        .map(|entries| {
            entries
                .iter()
                .map(|(name, keys)| render_keys(name, keys))
                .collect::<Vec<_>>()
                .join("\n\n")
        })
        .map_err(|error| error.to_string())
}

fn load_controller_config(filename: Option<&Path>) -> Result<ControllerConfig, String> {
    let Some(filename) = filename else {
        return Ok(ControllerConfig::default());
    };
    let bytes = std::fs::read(filename)
        .map_err(|error| format!("failed to read {}: {error}", filename.display()))?;
    let value: Value = serde_json::from_slice(&bytes)
        .map_err(|error| format!("failed to parse {}: {error}", filename.display()))?;
    let object = value
        .as_object()
        .ok_or_else(|| format!("{} must contain a JSON object", filename.display()))?;

    let address = match object.get("address") {
        None => ControllerConfig::default().address,
        Some(Value::String(address)) => Address::parse(address, AddressType::RANDOM_DEVICE)
            .map_err(|error| format!("invalid device address {address:?}: {error}"))?,
        Some(_) => return Err("device config address must be a string".into()),
    };
    let keystore = match object.get("keystore") {
        None | Some(Value::Null) => None,
        Some(Value::String(keystore)) => Some(keystore.clone()),
        Some(_) => return Err("device config keystore must be a string or null".into()),
    };
    Ok(ControllerConfig { address, keystore })
}

fn is_zero_address(address: &Address) -> bool {
    address.address_bytes() == &[0; 6]
}

fn reset_and_read_public_address<T: PacketSource + PacketSink>(
    transport: T,
) -> Result<Option<Address>, String> {
    let mut channel = HciCommandChannel::new(transport);
    let reset = channel
        .send_command(Command::Reset)
        .map_err(|error| error.to_string())?;
    match reset.status() {
        Some(0) => {}
        Some(status) => return Err(format!("HCI Reset failed with status {status:#04x}")),
        None => return Err("HCI Reset returned no status".into()),
    }

    match channel
        .send_command(Command::ReadBdAddr)
        .map_err(|error| error.to_string())?
    {
        CommandResponse::Complete {
            return_parameters: ReturnParameters::ReadBdAddr { status: 0, bd_addr },
            ..
        } if !is_zero_address(&bd_addr) => Ok(Some(bd_addr)),
        CommandResponse::Complete { .. } | CommandResponse::Status { .. } => Ok(None),
    }
}

fn execute_controller<T: PacketSource + PacketSink>(
    transport: T,
    device_config: Option<&Path>,
    namespace: Option<&str>,
    address: Option<&str>,
) -> Result<String, String> {
    let config = load_controller_config(device_config)?;
    let public_address = reset_and_read_public_address(transport)?;
    let Some(keystore_config) = config.keystore.as_deref() else {
        return execute_with_store(MemoryKeyStore::new(), address);
    };
    let (keystore_type, filename) = keystore_config
        .split_once(':')
        .map_or((keystore_config, None), |(kind, filename)| {
            (kind, (!filename.is_empty()).then_some(filename))
        });
    if keystore_type != "JsonKeyStore" {
        return execute_with_store(MemoryKeyStore::new(), address);
    }

    let inferred_namespace = public_address
        .as_ref()
        .map(|address| address.to_string(false))
        .or_else(|| (!is_zero_address(&config.address)).then(|| config.address.to_string(false)));
    let namespace = namespace.or(inferred_namespace.as_deref());
    if let Some(filename) = filename {
        execute_with_store(JsonKeyStore::new(namespace, filename), address)
    } else {
        execute_with_store(JsonKeyStore::with_default_path(namespace), address)
    }
}

fn execute(args: Args) -> Result<String, String> {
    match args.source {
        Source::KeyStoreFile(filename) => execute_with_store(
            JsonKeyStore::new(args.namespace.as_deref(), filename),
            args.address.as_deref(),
        ),
        Source::Controller {
            transport,
            device_config,
        } => {
            let transport = open_transport(&transport).map_err(|error| error.to_string())?;
            execute_controller(
                transport,
                device_config.as_deref(),
                args.namespace.as_deref(),
                args.address.as_deref(),
            )
        }
    }
}

fn main() -> ExitCode {
    match parse_args(std::env::args()).and_then(execute) {
        Ok(output) => {
            if !output.is_empty() {
                println!("{output}");
            }
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{error}\n{}", usage());
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bumble::{keys::Key, AddressType};
    use bumble_hci::{Event, HciPacket};
    use bumble_transport::Result as TransportResult;
    use std::collections::{BTreeMap, VecDeque};
    use std::time::{SystemTime, UNIX_EPOCH};

    struct MockTransport {
        responses: BTreeMap<u16, ReturnParameters>,
        inbound: VecDeque<HciPacket>,
    }

    impl PacketSource for MockTransport {
        fn read_packet(&mut self) -> TransportResult<Option<HciPacket>> {
            Ok(self.inbound.pop_front())
        }
    }

    impl PacketSink for MockTransport {
        fn write_packet(&mut self, packet: &HciPacket) -> TransportResult<()> {
            let HciPacket::Command(command) = packet else {
                panic!("expected command")
            };
            let return_parameters = self
                .responses
                .get(&command.op_code())
                .cloned()
                .unwrap_or(ReturnParameters::Status { status: 1 });
            self.inbound
                .push_back(HciPacket::Event(Event::CommandComplete {
                    num_hci_command_packets: 1,
                    command_opcode: command.op_code(),
                    return_parameters,
                }));
            Ok(())
        }
    }

    fn mock_transport(public_address: Option<Address>) -> MockTransport {
        let mut responses = BTreeMap::from([(
            Command::Reset.op_code(),
            ReturnParameters::Status { status: 0 },
        )]);
        responses.insert(
            Command::ReadBdAddr.op_code(),
            public_address.map_or(ReturnParameters::Status { status: 1 }, |bd_addr| {
                ReturnParameters::ReadBdAddr { status: 0, bd_addr }
            }),
        );
        MockTransport {
            responses,
            inbound: VecDeque::new(),
        }
    }

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(ToString::to_string).collect()
    }

    fn path(label: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "bumble-unbond-{label}-{}-{unique}.json",
            std::process::id()
        ))
    }

    #[test]
    fn parses_file_and_controller_forms() {
        assert_eq!(
            parse_args(args(&[
                "unbond",
                "--keystore-file=keys.json",
                "--namespace",
                "controller-a",
                "C4:F2:17:1A:1D:BB",
            ])),
            Ok(Args {
                source: Source::KeyStoreFile("keys.json".into()),
                namespace: Some("controller-a".into()),
                address: Some("C4:F2:17:1A:1D:BB".into()),
            })
        );
        assert!(matches!(
            parse_args(args(&[
                "unbond",
                "--hci-transport",
                "usb:0",
                "device.json",
                "peer",
            ])),
            Ok(Args {
                source: Source::Controller { .. },
                address: Some(_),
                ..
            })
        ));
    }

    #[test]
    fn lists_deletes_and_reports_missing_pairings() {
        let path = path("keys");
        let mut store = JsonKeyStore::new(Some("controller-a"), &path);
        store
            .update(
                "C4:F2:17:1A:1D:BB",
                PairingKeys {
                    address_type: Some(AddressType::RANDOM_DEVICE),
                    ltk: Some(Key {
                        value: vec![0xAA; 16],
                        authenticated: true,
                        ediv: Some(7),
                        rand: Some(vec![0xBB; 8]),
                        sign_counter: None,
                    }),
                    ..PairingKeys::default()
                },
            )
            .unwrap();

        let source = Source::KeyStoreFile(path.clone());
        let listed = execute(Args {
            source: source.clone(),
            namespace: Some("controller-a".into()),
            address: None,
        })
        .unwrap();
        assert!(listed.contains("C4:F2:17:1A:1D:BB"));
        assert!(listed.contains("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"));
        assert!(listed.contains("authenticated\x1b[0m: true"));

        assert_eq!(
            execute(Args {
                source: source.clone(),
                namespace: Some("controller-a".into()),
                address: Some("missing".into()),
            }),
            Ok("!!! pairing not found".into())
        );
        assert_eq!(
            execute(Args {
                source,
                namespace: Some("controller-a".into()),
                address: Some("C4:F2:17:1A:1D:BB".into()),
            }),
            Ok(String::new())
        );
        assert!(store.get_all().unwrap().is_empty());
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn controller_mode_uses_public_address_namespace() {
        let keys_path = path("controller-keys");
        let config_path = path("controller-config");
        let public_address =
            Address::parse("01:23:45:67:89:AB/P", AddressType::PUBLIC_DEVICE).unwrap();
        let peer = "C4:F2:17:1A:1D:BB";
        let mut store = JsonKeyStore::new(Some("01:23:45:67:89:AB"), &keys_path);
        store.update(peer, PairingKeys::default()).unwrap();
        std::fs::write(
            &config_path,
            serde_json::to_vec(&serde_json::json!({
                "address": "C0:00:00:00:00:01",
                "keystore": format!("JsonKeyStore:{}", keys_path.display()),
            }))
            .unwrap(),
        )
        .unwrap();

        let listed = execute_controller(
            mock_transport(Some(public_address.clone())),
            Some(&config_path),
            None,
            None,
        )
        .unwrap();
        assert!(listed.contains(peer));
        assert_eq!(
            execute_controller(
                mock_transport(Some(public_address)),
                Some(&config_path),
                None,
                Some(peer),
            ),
            Ok(String::new())
        );
        assert!(store.get_all().unwrap().is_empty());
        std::fs::remove_file(keys_path).unwrap();
        std::fs::remove_file(config_path).unwrap();
    }

    #[test]
    fn controller_mode_falls_back_to_configured_address() {
        let keys_path = path("fallback-keys");
        let config_path = path("fallback-config");
        let namespace = "C0:00:00:00:00:02";
        let peer = "C4:F2:17:1A:1D:BB";
        let mut store = JsonKeyStore::new(Some(namespace), &keys_path);
        store.update(peer, PairingKeys::default()).unwrap();
        std::fs::write(
            &config_path,
            serde_json::to_vec(&serde_json::json!({
                "address": namespace,
                "keystore": format!("JsonKeyStore:{}", keys_path.display()),
            }))
            .unwrap(),
        )
        .unwrap();

        let listed =
            execute_controller(mock_transport(None), Some(&config_path), None, None).unwrap();
        assert!(listed.contains(peer));
        std::fs::remove_file(keys_path).unwrap();
        std::fs::remove_file(config_path).unwrap();
    }

    #[test]
    fn controller_mode_uses_memory_without_json_keystore() {
        assert_eq!(
            execute_controller(
                mock_transport(Some(Address::from_bytes(
                    [1, 2, 3, 4, 5, 6],
                    AddressType::PUBLIC_DEVICE,
                ))),
                None,
                None,
                None,
            ),
            Ok(String::new())
        );
    }

    #[test]
    fn validates_device_config() {
        let config_path = path("invalid-config");
        std::fs::write(&config_path, br#"{"address": 7}"#).unwrap();
        assert_eq!(
            load_controller_config(Some(&config_path)).unwrap_err(),
            "device config address must be a string"
        );
        std::fs::remove_file(config_path).unwrap();
    }

    #[test]
    fn rejects_ambiguous_modes() {
        assert!(parse_args(args(&["unbond"])).is_err());
        assert!(parse_args(args(&[
            "unbond",
            "--keystore-file",
            "a",
            "--hci-transport",
            "b",
        ]))
        .is_err());
    }
}
