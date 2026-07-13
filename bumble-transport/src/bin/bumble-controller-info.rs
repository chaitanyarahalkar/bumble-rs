use bumble::company_name;
use bumble_hci::{Command, HciPacket, ReturnParameters};
use bumble_transport::{
    open_transport, CommandResponse, HciCommandChannel, PacketSink, PacketSource,
};
use std::process::ExitCode;
use std::thread;
use std::time::{Duration, Instant};

#[derive(Clone, Debug, PartialEq, Eq)]
struct Args {
    latency_probes: usize,
    latency_probe_interval_ms: u64,
    latency_probe_command: Option<Command>,
    transport: String,
}

fn usage() -> &'static str {
    "usage: bumble-controller-info [--latency-probes N] [--latency-probe-interval MS] [--latency-probe-command HEX] <transport>"
}

fn decode_hex(value: &str) -> Result<Vec<u8>, String> {
    let value: String = value
        .chars()
        .filter(|character| !character.is_ascii_whitespace())
        .collect();
    if !value.len().is_multiple_of(2) || !value.is_ascii() {
        return Err("command hex must contain complete hexadecimal bytes".into());
    }
    value
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let pair = std::str::from_utf8(pair).expect("ASCII checked");
            u8::from_str_radix(pair, 16)
                .map_err(|_| "command contains a non-hexadecimal digit".to_string())
        })
        .collect()
}

fn parse_probe_command(value: &str) -> Result<Command, String> {
    match HciPacket::from_bytes(&decode_hex(value)?).map_err(|error| error.to_string())? {
        HciPacket::Command(command) => Ok(command),
        _ => Err("latency probe must be a complete HCI Command packet".into()),
    }
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
    let mut latency_probes = 0usize;
    let mut latency_probe_interval_ms = 0u64;
    let mut latency_probe_command = None;
    let mut transport = None;
    while let Some(argument) = arguments.next() {
        if matches!(argument.as_str(), "-h" | "--help") {
            return Err(usage().into());
        }
        if let Some(value) = option_value(&argument, "--latency-probes", &mut arguments)? {
            latency_probes = value
                .parse()
                .map_err(|_| "latency probe count must be an integer".to_string())?;
            continue;
        }
        if let Some(value) = option_value(&argument, "--latency-probe-interval", &mut arguments)? {
            latency_probe_interval_ms = value
                .parse()
                .map_err(|_| "latency probe interval must be an integer".to_string())?;
            continue;
        }
        if let Some(value) = option_value(&argument, "--latency-probe-command", &mut arguments)? {
            latency_probe_command = Some(parse_probe_command(&value)?);
            continue;
        }
        if argument.starts_with('-') {
            return Err(format!("unknown option {argument:?}"));
        }
        if transport.replace(argument).is_some() {
            return Err("only one transport may be specified".into());
        }
    }
    Ok(Args {
        latency_probes,
        latency_probe_interval_ms,
        latency_probe_command,
        transport: transport.ok_or_else(|| "missing transport".to_string())?,
    })
}

fn bytes_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn query<T: PacketSource + PacketSink>(
    channel: &mut HciCommandChannel<T>,
    command: Command,
) -> Result<Option<ReturnParameters>, String> {
    match channel
        .send_command(command)
        .map_err(|error| error.to_string())?
    {
        CommandResponse::Complete {
            return_parameters, ..
        } if return_parameters.status().unwrap_or(0) == 0 => Ok(Some(return_parameters)),
        CommandResponse::Complete { .. } | CommandResponse::Status { .. } => Ok(None),
    }
}

fn reset<T: PacketSource + PacketSink>(channel: &mut HciCommandChannel<T>) -> Result<(), String> {
    let response = channel
        .send_command(Command::Reset)
        .map_err(|error| error.to_string())?;
    match response.status() {
        Some(0) => Ok(()),
        Some(status) => Err(format!("HCI Reset failed with status {status:#04x}")),
        None => Err("HCI Reset returned no status".into()),
    }
}

fn run_report<T: PacketSource + PacketSink>(
    transport: T,
    latency_probes: usize,
    latency_probe_interval_ms: u64,
    latency_probe_command: Option<Command>,
) -> Result<String, String> {
    let mut channel = HciCommandChannel::new(transport);
    reset(&mut channel)?;
    let mut lines = Vec::new();

    if latency_probes > 0 {
        let command = latency_probe_command.unwrap_or(Command::ReadLocalVersionInformation);
        let mut timings = Vec::with_capacity(latency_probes);
        for iteration in 0..=latency_probes {
            if latency_probe_interval_ms > 0 {
                thread::sleep(Duration::from_millis(latency_probe_interval_ms));
            }
            let start = Instant::now();
            channel
                .send_command(command.clone())
                .map_err(|error| error.to_string())?;
            if iteration > 0 {
                timings.push(start.elapsed().as_secs_f64() * 1_000.0);
            }
        }
        let minimum = timings.iter().copied().fold(f64::INFINITY, f64::min);
        let maximum = timings.iter().copied().fold(0.0, f64::max);
        let average = timings.iter().sum::<f64>() / timings.len() as f64;
        lines.push(format!(
            "HCI Command Latency: min={minimum:.2} ms, max={maximum:.2} ms, average={average:.2} ms"
        ));
    }

    if let Some(ReturnParameters::ReadLocalVersionInformation {
        hci_version,
        hci_subversion,
        lmp_version,
        company_identifier,
        lmp_subversion,
        ..
    }) = query(&mut channel, Command::ReadLocalVersionInformation)?
    {
        let company = company_name(company_identifier)
            .map(str::to_owned)
            .unwrap_or_else(|| format!("{company_identifier:#06x}"));
        lines.push("Version:".into());
        lines.push(format!("  Manufacturer: {company}"));
        lines.push(format!("  HCI Version: {hci_version:#04x}"));
        lines.push(format!("  HCI Subversion: {hci_subversion:#06x}"));
        lines.push(format!("  LMP Version: {lmp_version:#04x}"));
        lines.push(format!("  LMP Subversion: {lmp_subversion:#06x}"));
    }
    if let Some(ReturnParameters::ReadBdAddr { bd_addr, .. }) =
        query(&mut channel, Command::ReadBdAddr)?
    {
        lines.push(format!("Public Address: {}", bd_addr.to_string(false)));
    }
    if let Some(ReturnParameters::ReadLocalName { local_name, .. }) =
        query(&mut channel, Command::ReadLocalName)?
    {
        lines.push(format!(
            "Local Name: {}",
            bumble_hci::map_null_terminated_utf8_string(&local_name)
        ));
    }
    if let Some(ReturnParameters::ReadLocalSupportedCommands {
        supported_commands, ..
    }) = query(&mut channel, Command::ReadLocalSupportedCommands)?
    {
        lines.push(format!(
            "Supported Commands Bitmap: {}",
            bytes_hex(&supported_commands)
        ));
    }
    if let Some(ReturnParameters::ReadLocalSupportedFeatures { lmp_features, .. }) =
        query(&mut channel, Command::ReadLocalSupportedFeatures)?
    {
        lines.push(format!("LMP Features: {}", bytes_hex(&lmp_features)));
    }
    if let Some(ReturnParameters::LeReadLocalSupportedFeatures { le_features, .. }) =
        query(&mut channel, Command::LeReadLocalSupportedFeatures)?
    {
        lines.push(format!("LE Features: {}", bytes_hex(&le_features)));
    }
    if let Some(ReturnParameters::ReadBufferSize {
        hc_acl_data_packet_length,
        hc_total_num_acl_data_packets,
        hc_synchronous_data_packet_length,
        hc_total_num_synchronous_data_packets,
        ..
    }) = query(&mut channel, Command::ReadBufferSize)?
    {
        lines.push(format!(
            "ACL Flow Control: {hc_total_num_acl_data_packets} packets of size {hc_acl_data_packet_length}"
        ));
        lines.push(format!(
            "SCO Flow Control: {hc_total_num_synchronous_data_packets} packets of size {hc_synchronous_data_packet_length}"
        ));
    }
    match query(&mut channel, Command::LeReadBufferSizeV2)? {
        Some(ReturnParameters::LeReadBufferSizeV2 {
            le_acl_data_packet_length,
            total_num_le_acl_data_packets,
            iso_data_packet_length,
            total_num_iso_data_packets,
            ..
        }) => {
            lines.push(format!(
                "LE ACL Flow Control: {total_num_le_acl_data_packets} packets of size {le_acl_data_packet_length}"
            ));
            lines.push(format!(
                "LE ISO Flow Control: {total_num_iso_data_packets} packets of size {iso_data_packet_length}"
            ));
        }
        _ => {
            if let Some(ReturnParameters::LeReadBufferSize {
                le_acl_data_packet_length,
                total_num_le_acl_data_packets,
                ..
            }) = query(&mut channel, Command::LeReadBufferSize)?
            {
                lines.push(format!(
                    "LE ACL Flow Control: {total_num_le_acl_data_packets} packets of size {le_acl_data_packet_length}"
                ));
            }
        }
    }
    if let Some(ReturnParameters::LeReadMaximumDataLength {
        supported_max_tx_octets,
        supported_max_tx_time,
        supported_max_rx_octets,
        supported_max_rx_time,
        ..
    }) = query(&mut channel, Command::LeReadMaximumDataLength)?
    {
        lines.push(format!(
            "LE Maximum Data Length: tx:{supported_max_tx_octets}/{supported_max_tx_time}, rx:{supported_max_rx_octets}/{supported_max_rx_time}"
        ));
    }
    if let Some(ReturnParameters::LeReadSuggestedDefaultDataLength {
        suggested_max_tx_octets,
        suggested_max_tx_time,
        ..
    }) = query(&mut channel, Command::LeReadSuggestedDefaultDataLength)?
    {
        lines.push(format!(
            "LE Suggested Default Data Length: {suggested_max_tx_octets}/{suggested_max_tx_time}"
        ));
    }
    if let Some(ReturnParameters::LeReadMaximumAdvertisingDataLength {
        max_advertising_data_length,
        ..
    }) = query(&mut channel, Command::LeReadMaximumAdvertisingDataLength)?
    {
        lines.push(format!(
            "LE Maximum Advertising Data Length: {max_advertising_data_length}"
        ));
    }
    if let Some(ReturnParameters::LeReadNumberOfSupportedAdvertisingSets {
        num_supported_advertising_sets,
        ..
    }) = query(
        &mut channel,
        Command::LeReadNumberOfSupportedAdvertisingSets,
    )? {
        lines.push(format!(
            "LE Number Of Supported Advertising Sets: {num_supported_advertising_sets}"
        ));
    }
    if let Some(ReturnParameters::LeReadMinimumSupportedConnectionInterval {
        minimum_supported_connection_interval,
        group_min,
        group_max,
        group_stride,
        ..
    }) = query(
        &mut channel,
        Command::LeReadMinimumSupportedConnectionInterval,
    )? {
        lines.push(format!(
            "LE Minimum Supported Connection Interval: {} us",
            u32::from(minimum_supported_connection_interval) * 125
        ));
        for (index, ((minimum, maximum), stride)) in group_min
            .iter()
            .zip(&group_max)
            .zip(&group_stride)
            .enumerate()
        {
            lines.push(format!(
                "  Group {index}: {} us to {} us by {} us",
                u32::from(*minimum) * 125,
                u32::from(*maximum) * 125,
                u32::from(*stride) * 125
            ));
        }
    }
    if let Some(ReturnParameters::ReadLocalSupportedCodecsV2 {
        standard_codec_ids,
        standard_codec_transports,
        vendor_specific_codec_ids,
        vendor_specific_codec_transports,
        ..
    }) = query(&mut channel, Command::ReadLocalSupportedCodecsV2)?
    {
        lines.push("Codecs:".into());
        for (codec, transport) in standard_codec_ids.iter().zip(standard_codec_transports) {
            lines.push(format!(
                "  standard {codec:#04x} - transport {transport:#04x}"
            ));
        }
        for (codec, transport) in vendor_specific_codec_ids
            .iter()
            .zip(vendor_specific_codec_transports)
        {
            lines.push(format!(
                "  vendor {codec:#010x} - transport {transport:#04x}"
            ));
        }
    }
    if let Some(ReturnParameters::ReadLocalSupportedCodecs {
        standard_codec_ids,
        vendor_specific_codec_ids,
        ..
    }) = query(&mut channel, Command::ReadLocalSupportedCodecs)?
    {
        lines.push(format!(
            "Codecs (BR/EDR): standard={:?}, vendor={:?}",
            standard_codec_ids, vendor_specific_codec_ids
        ));
    }
    if let Some(ReturnParameters::ReadVoiceSetting { voice_setting, .. }) =
        query(&mut channel, Command::ReadVoiceSetting)?
    {
        lines.push(format!("Voice Setting: {voice_setting:#06x}"));
    }
    let pending = channel.take_pending_packets().len();
    if pending > 0 {
        lines.push(format!("Preserved asynchronous packets: {pending}"));
    }
    Ok(lines.join("\n"))
}

fn run(args: Args) -> Result<String, String> {
    let transport = open_transport(&args.transport).map_err(|error| error.to_string())?;
    run_report(
        transport,
        args.latency_probes,
        args.latency_probe_interval_ms,
        args.latency_probe_command,
    )
}

fn main() -> ExitCode {
    match parse_args(std::env::args()).and_then(run) {
        Ok(report) => {
            println!("{report}");
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
    use bumble::{Address, AddressType};
    use bumble_hci::Event;
    use bumble_transport::Result;
    use std::collections::{BTreeMap, VecDeque};

    struct MockTransport {
        responses: BTreeMap<u16, ReturnParameters>,
        inbound: VecDeque<HciPacket>,
        outbound: Vec<Command>,
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

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(ToString::to_string).collect()
    }

    #[test]
    fn parses_upstream_latency_options() {
        assert_eq!(
            parse_args(args(&[
                "info",
                "--latency-probes=3",
                "--latency-probe-interval",
                "5",
                "--latency-probe-command",
                "01030c00",
                "tcp-client:localhost:6402",
            ])),
            Ok(Args {
                latency_probes: 3,
                latency_probe_interval_ms: 5,
                latency_probe_command: Some(Command::Reset),
                transport: "tcp-client:localhost:6402".into(),
            })
        );
        assert!(parse_args(args(&["info", "--latency-probes", "bad", "x"])).is_err());
        assert!(parse_args(args(&["info"])).is_err());
    }

    #[test]
    fn renders_available_controller_information_and_skips_unsupported_queries() {
        let address = Address::parse("00:11:22:33:44:55", AddressType::PUBLIC_DEVICE).unwrap();
        let responses = BTreeMap::from([
            (
                Command::Reset.op_code(),
                ReturnParameters::Status { status: 0 },
            ),
            (
                Command::ReadLocalVersionInformation.op_code(),
                ReturnParameters::ReadLocalVersionInformation {
                    status: 0,
                    hci_version: 13,
                    hci_subversion: 0x1234,
                    lmp_version: 12,
                    company_identifier: 0x004C,
                    lmp_subversion: 0x5678,
                },
            ),
            (
                Command::ReadBdAddr.op_code(),
                ReturnParameters::ReadBdAddr {
                    status: 0,
                    bd_addr: address,
                },
            ),
            (
                Command::ReadLocalName.op_code(),
                ReturnParameters::ReadLocalName {
                    status: 0,
                    local_name: {
                        let mut name = vec![0; 248];
                        name[..6].copy_from_slice(b"Bumble");
                        name
                    },
                },
            ),
            (
                Command::LeReadBufferSizeV2.op_code(),
                ReturnParameters::LeReadBufferSizeV2 {
                    status: 0,
                    le_acl_data_packet_length: 251,
                    total_num_le_acl_data_packets: 12,
                    iso_data_packet_length: 960,
                    total_num_iso_data_packets: 6,
                },
            ),
            (
                Command::LeReadMaximumAdvertisingDataLength.op_code(),
                ReturnParameters::LeReadMaximumAdvertisingDataLength {
                    status: 0,
                    max_advertising_data_length: 1650,
                },
            ),
            (
                Command::ReadVoiceSetting.op_code(),
                ReturnParameters::ReadVoiceSetting {
                    status: 0,
                    voice_setting: 0x0060,
                },
            ),
        ]);
        let transport = MockTransport {
            responses,
            inbound: VecDeque::new(),
            outbound: Vec::new(),
        };
        let report = run_report(transport, 0, 0, None).unwrap();
        assert!(report.contains("Manufacturer: Apple, Inc."));
        assert!(report.contains("Public Address: 00:11:22:33:44:55"));
        assert!(report.contains("Local Name: Bumble"));
        assert!(report.contains("LE ACL Flow Control: 12 packets of size 251"));
        assert!(report.contains("LE ISO Flow Control: 6 packets of size 960"));
        assert!(report.contains("LE Maximum Advertising Data Length: 1650"));
        assert!(report.contains("Voice Setting: 0x0060"));
        assert!(!report.contains("LE Features:"));
    }
}
