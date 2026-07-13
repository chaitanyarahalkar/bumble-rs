use bumble::keys::{JsonKeyStore, Key, KeyStore, KeyStoreError, PairingKeys};
use std::path::PathBuf;
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

fn execute(args: Args) -> Result<String, String> {
    let Source::KeyStoreFile(filename) = args.source else {
        return Err(
            "controller-backed unbonding requires external host bootstrap and is not yet available"
                .into(),
        );
    };
    let mut store = JsonKeyStore::new(args.namespace.as_deref(), filename);
    if let Some(address) = args.address {
        return match store.delete(&address) {
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
    use std::time::{SystemTime, UNIX_EPOCH};

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(ToString::to_string).collect()
    }

    fn path() -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "bumble-unbond-{}-{unique}.json",
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
        let path = path();
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
    fn rejects_ambiguous_or_unavailable_modes() {
        assert!(parse_args(args(&["unbond"])).is_err());
        assert!(parse_args(args(&[
            "unbond",
            "--keystore-file",
            "a",
            "--hci-transport",
            "b",
        ]))
        .is_err());
        let controller = parse_args(args(&["unbond", "--hci-transport", "usb:0"])).unwrap();
        assert!(execute(controller).is_err());
    }
}
