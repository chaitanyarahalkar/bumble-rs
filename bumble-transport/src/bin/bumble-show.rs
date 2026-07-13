use bumble_hci::HciPacket;
use bumble_transport::{BtSnoopReader, H4Transport, PacketSource, SnoopDirection};
use std::ffi::OsString;
use std::fs::File;
use std::io::BufReader;
use std::path::PathBuf;
use std::process::ExitCode;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum InputFormat {
    #[default]
    H4,
    Snoop,
}

#[derive(Debug, PartialEq, Eq)]
struct Args {
    format: InputFormat,
    vendors: Vec<String>,
    filename: PathBuf,
}

fn usage() -> &'static str {
    "usage: bumble-show [--format h4|snoop] [--vendor android|zephyr] <filename>"
}

fn parse_format(value: &str) -> Result<InputFormat, String> {
    match value {
        "h4" => Ok(InputFormat::H4),
        "snoop" => Ok(InputFormat::Snoop),
        _ => Err(format!("unsupported input format {value:?}")),
    }
}

fn parse_vendor(value: String) -> Result<String, String> {
    match value.as_str() {
        "android" | "zephyr" => Ok(value),
        _ => Err(format!("unsupported HCI vendor {value:?}")),
    }
}

fn option_value(
    argument: &str,
    option: &str,
    arguments: &mut impl Iterator<Item = OsString>,
) -> Result<Option<String>, String> {
    if argument == option {
        let value = arguments
            .next()
            .ok_or_else(|| format!("missing value for {option}"))?;
        return value
            .into_string()
            .map(Some)
            .map_err(|_| format!("{option} value is not valid UTF-8"));
    }
    Ok(argument
        .strip_prefix(&format!("{option}="))
        .map(ToOwned::to_owned))
}

fn parse_args(arguments: impl IntoIterator<Item = OsString>) -> Result<Args, String> {
    let mut arguments = arguments.into_iter();
    let _program = arguments.next();
    let mut format = InputFormat::H4;
    let mut vendors = Vec::new();
    let mut filename = None;

    while let Some(argument) = arguments.next() {
        if let Some(argument_str) = argument.to_str() {
            if argument_str == "-h" || argument_str == "--help" {
                return Err(usage().into());
            }
            if let Some(value) = option_value(argument_str, "--format", &mut arguments)? {
                format = parse_format(&value)?;
                continue;
            }
            if let Some(value) = option_value(argument_str, "--vendor", &mut arguments)? {
                vendors.push(parse_vendor(value)?);
                continue;
            }
            if argument_str.starts_with('-') {
                return Err(format!("unknown option {argument_str:?}"));
            }
        }
        if filename.replace(PathBuf::from(argument)).is_some() {
            return Err("only one input filename may be specified".into());
        }
    }

    let filename = filename.ok_or_else(|| "missing input filename".to_string())?;
    Ok(Args {
        format,
        vendors,
        filename,
    })
}

fn print_packet(
    index: usize,
    packet: &HciPacket,
    direction: SnoopDirection,
    timestamp_micros: Option<u64>,
) {
    let direction = match direction {
        SnoopDirection::HostToController => "H->C",
        SnoopDirection::ControllerToHost => "C->H",
    };
    let timestamp = timestamp_micros
        .map(|timestamp| format!(" {}.{:06}", timestamp / 1_000_000, timestamp % 1_000_000))
        .unwrap_or_default();
    println!("[{index:8}]{timestamp} {direction} {packet:#?}");
}

fn show_h4(file: File) -> bumble_transport::Result<()> {
    let mut reader = H4Transport::new(BufReader::new(file));
    let mut index = 0usize;
    while let Some(packet) = reader.read_packet()? {
        index += 1;
        print_packet(index, &packet, SnoopDirection::HostToController, None);
    }
    Ok(())
}

fn show_snoop(file: File) -> bumble_transport::Result<()> {
    let mut reader = BtSnoopReader::new(BufReader::new(file))?;
    let mut index = 0usize;
    while let Some(record) = reader.read_record()? {
        index += 1;
        if record.is_truncated() {
            println!(
                "[{index:8}] [TRUNCATED {}/{}]",
                record.included_length, record.original_length
            );
            continue;
        }
        if let Some(packet) = record.packet()? {
            print_packet(
                index,
                &packet,
                record.direction(),
                Some(record.unix_timestamp_micros()?),
            );
        }
    }
    Ok(())
}

fn run(args: Args) -> Result<(), String> {
    // Vendor codecs are statically linked in Rust. Accepting these options keeps
    // command-line compatibility with Bumble's dynamic Python registration.
    let _vendors = args.vendors;
    let file = File::open(&args.filename)
        .map_err(|error| format!("failed to open {}: {error}", args.filename.display()))?;
    match args.format {
        InputFormat::H4 => show_h4(file),
        InputFormat::Snoop => show_snoop(file),
    }
    .map_err(|error| error.to_string())
}

fn main() -> ExitCode {
    match parse_args(std::env::args_os()).and_then(run) {
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

    fn args(values: &[&str]) -> Vec<OsString> {
        values.iter().map(OsString::from).collect()
    }

    #[test]
    fn parses_upstream_options_in_separate_and_equals_forms() {
        assert_eq!(
            parse_args(args(&[
                "bumble-show",
                "--format",
                "snoop",
                "--vendor=android",
                "--vendor",
                "zephyr",
                "capture.btsnoop",
            ])),
            Ok(Args {
                format: InputFormat::Snoop,
                vendors: vec!["android".into(), "zephyr".into()],
                filename: "capture.btsnoop".into(),
            })
        );
    }

    #[test]
    fn rejects_unknown_values_and_missing_filename() {
        assert!(parse_args(args(&["bumble-show", "--format", "pcap", "x"])).is_err());
        assert!(parse_args(args(&["bumble-show", "--vendor", "intel", "x"])).is_err());
        assert!(parse_args(args(&["bumble-show"])).is_err());
    }

    #[test]
    fn decodes_h4_and_btsnoop_files_end_to_end() {
        let base = std::env::temp_dir().join(format!("bumble-show-{}", std::process::id()));
        let h4_path = base.with_extension("h4");
        let snoop_path = base.with_extension("btsnoop");
        let packet = [0x01, 0x03, 0x0C, 0x00];
        std::fs::write(&h4_path, packet).unwrap();

        let mut snoop = b"btsnoop\0".to_vec();
        snoop.extend_from_slice(&1u32.to_be_bytes());
        snoop.extend_from_slice(&1002u32.to_be_bytes());
        snoop.extend_from_slice(&4u32.to_be_bytes());
        snoop.extend_from_slice(&4u32.to_be_bytes());
        snoop.extend_from_slice(&0x10u32.to_be_bytes());
        snoop.extend_from_slice(&0u32.to_be_bytes());
        snoop.extend_from_slice(&0x00DC_DDB3_0F2F_8000u64.to_be_bytes());
        snoop.extend_from_slice(&packet);
        std::fs::write(&snoop_path, snoop).unwrap();

        assert!(run(Args {
            format: InputFormat::H4,
            vendors: Vec::new(),
            filename: h4_path.clone(),
        })
        .is_ok());
        assert!(run(Args {
            format: InputFormat::Snoop,
            vendors: vec!["android".into(), "zephyr".into()],
            filename: snoop_path.clone(),
        })
        .is_ok());

        std::fs::remove_file(h4_path).unwrap();
        std::fs::remove_file(snoop_path).unwrap();
    }
}
