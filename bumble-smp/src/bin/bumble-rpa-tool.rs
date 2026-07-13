use bumble::{Address, AddressType};
use bumble_crypto::random_128;
use bumble_smp::{generate_resolvable_private_address, verify_resolvable_private_address};
use std::process::ExitCode;

const GREEN: &str = "\x1b[32m";
const RED: &str = "\x1b[31m";
const RESET: &str = "\x1b[0m";

#[derive(Clone, Debug, PartialEq, Eq)]
enum Command {
    GenerateIrk,
    GenerateRpa([u8; 16]),
    VerifyRpa { irk: [u8; 16], rpa: Address },
}

fn usage() -> &'static str {
    "usage:\n  bumble-rpa-tool gen-irk\n  bumble-rpa-tool gen-rpa <irk>\n  bumble-rpa-tool verify-rpa <irk> <rpa>"
}

fn parse_irk(value: &str) -> Result<[u8; 16], String> {
    let value: String = value
        .chars()
        .filter(|character| !character.is_ascii_whitespace())
        .collect();
    if value.len() != 32 || !value.is_ascii() {
        return Err("IRK must contain exactly 16 hexadecimal bytes".into());
    }
    let mut irk = [0u8; 16];
    for (index, byte) in irk.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&value[index * 2..index * 2 + 2], 16)
            .map_err(|_| "IRK contains a non-hexadecimal digit".to_string())?;
    }
    Ok(irk)
}

fn parse_args(arguments: impl IntoIterator<Item = String>) -> Result<Command, String> {
    let mut arguments = arguments.into_iter();
    let _program = arguments.next();
    let command = arguments
        .next()
        .ok_or_else(|| "missing command".to_string())?;
    let command = match command.as_str() {
        "gen-irk" => Command::GenerateIrk,
        "gen-rpa" => Command::GenerateRpa(parse_irk(
            &arguments
                .next()
                .ok_or_else(|| "missing IRK for gen-rpa".to_string())?,
        )?),
        "verify-rpa" => {
            let irk = parse_irk(
                &arguments
                    .next()
                    .ok_or_else(|| "missing IRK for verify-rpa".to_string())?,
            )?;
            let rpa = Address::parse(
                &arguments
                    .next()
                    .ok_or_else(|| "missing RPA for verify-rpa".to_string())?,
                AddressType::RANDOM_DEVICE,
            )
            .map_err(|error| error.to_string())?;
            Command::VerifyRpa { irk, rpa }
        }
        "-h" | "--help" => return Err(usage().into()),
        _ => return Err(format!("unknown command {command:?}")),
    };
    if arguments.next().is_some() {
        return Err("too many arguments".into());
    }
    Ok(command)
}

fn lower_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn execute(command: Command) -> String {
    match command {
        Command::GenerateIrk => lower_hex(&random_128()),
        Command::GenerateRpa(irk) => generate_resolvable_private_address(&irk).to_string(false),
        Command::VerifyRpa { irk, rpa } => {
            if verify_resolvable_private_address(&irk, &rpa) {
                format!("{GREEN}Verified{RESET}")
            } else {
                format!("{RED}Not Verified{RESET}")
            }
        }
    }
}

fn main() -> ExitCode {
    match parse_args(std::env::args()).map(execute) {
        Ok(output) => {
            println!("{output}");
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
    use bumble_smp::resolvable_private_address;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(ToString::to_string).collect()
    }

    #[test]
    fn parses_all_upstream_commands_and_flexible_hex() {
        assert_eq!(
            parse_args(args(&["tool", "gen-irk"])),
            Ok(Command::GenerateIrk)
        );
        assert_eq!(
            parse_args(args(&[
                "tool",
                "gen-rpa",
                "9B7D390A A6101034 05ADC857 A33402EC",
            ])),
            Ok(Command::GenerateRpa([
                0x9B, 0x7D, 0x39, 0x0A, 0xA6, 0x10, 0x10, 0x34, 0x05, 0xAD, 0xC8, 0x57, 0xA3, 0x34,
                0x02, 0xEC,
            ]))
        );
        assert!(matches!(
            parse_args(args(&[
                "tool",
                "verify-rpa",
                "9b7d390aa610103405adc857a33402ec",
                "70:81:94:0D:FB:AA",
            ])),
            Ok(Command::VerifyRpa { .. })
        ));
    }

    #[test]
    fn generates_and_verifies_values() {
        let generated_irk = execute(Command::GenerateIrk);
        assert_eq!(generated_irk.len(), 32);
        assert!(generated_irk
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()));

        let irk = [
            0x9B, 0x7D, 0x39, 0x0A, 0xA6, 0x10, 0x10, 0x34, 0x05, 0xAD, 0xC8, 0x57, 0xA3, 0x34,
            0x02, 0xEC,
        ];
        let generated_rpa = execute(Command::GenerateRpa(irk));
        let address = Address::parse(&generated_rpa, AddressType::RANDOM_DEVICE).unwrap();
        assert!(verify_resolvable_private_address(&irk, &address));

        let known_rpa = resolvable_private_address(&irk, [0x94, 0x81, 0x70]);
        assert_eq!(
            execute(Command::VerifyRpa {
                irk,
                rpa: known_rpa.clone(),
            }),
            "\x1b[32mVerified\x1b[0m"
        );
        assert_eq!(
            execute(Command::VerifyRpa {
                irk: [0; 16],
                rpa: known_rpa,
            }),
            "\x1b[31mNot Verified\x1b[0m"
        );
    }

    #[test]
    fn rejects_malformed_commands_keys_addresses_and_extra_arguments() {
        assert!(parse_args(args(&["tool"])).is_err());
        assert!(parse_args(args(&["tool", "unknown"])).is_err());
        assert!(parse_args(args(&["tool", "gen-irk", "extra"])).is_err());
        assert!(parse_args(args(&["tool", "gen-rpa", "00"])).is_err());
        assert!(parse_args(args(&[
            "tool",
            "verify-rpa",
            "00000000000000000000000000000000",
            "bad-address",
        ]))
        .is_err());
    }
}
