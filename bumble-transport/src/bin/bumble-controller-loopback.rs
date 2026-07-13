use bumble_hci::{
    AclDataPacket, AclDataPacketAssembler, Command, Event, HciPacket, LeMetaEvent,
    ReturnParameters, SynchronousDataPacket, HCI_ACL_PB_FIRST_NON_FLUSHABLE,
};
use bumble_transport::{
    open_transport, CommandResponse, HciCommandChannel, PacketSink, PacketSource,
};
use std::collections::VecDeque;
use std::process::ExitCode;
use std::thread;
use std::time::{Duration, Instant};

const LOOPBACK_MODE_LOCAL: u8 = 1;
// Connection Complete, Number Of Completed Packets, Synchronous Connection
// Complete, and LE Meta. Command Complete/Status cannot be masked.
const LOOPBACK_EVENT_MASK: [u8; 8] = [0x04, 0x00, 0x04, 0x00, 0x00, 0x08, 0x00, 0x20];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ConnectionType {
    Acl,
    Sco,
}

impl ConnectionType {
    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "acl" => Ok(Self::Acl),
            "sco" => Ok(Self::Sco),
            _ => Err("connection type must be acl or sco".into()),
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Acl => "ACL",
            Self::Sco => "SCO",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TestMode {
    Throughput,
    Rtt,
}

impl TestMode {
    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "throughput" => Ok(Self::Throughput),
            "rtt" => Ok(Self::Rtt),
            _ => Err("mode must be throughput or rtt".into()),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Args {
    packet_size: usize,
    packet_count: usize,
    connection_type: ConnectionType,
    mode: TestMode,
    interval_ms: u64,
    transport: String,
}

fn usage() -> &'static str {
    "usage: bumble-controller-loopback [--packet-size SIZE] [--packet-count COUNT] [--connection-type acl|sco] [--mode throughput|rtt] [--interval MS] <transport>"
}

fn option_value(
    argument: &str,
    short: Option<&str>,
    long: &str,
    arguments: &mut impl Iterator<Item = String>,
) -> Result<Option<String>, String> {
    if argument == long || short.is_some_and(|short| argument == short) {
        return arguments
            .next()
            .map(Some)
            .ok_or_else(|| format!("missing value for {long}"));
    }
    Ok(argument
        .strip_prefix(&format!("{long}="))
        .map(ToOwned::to_owned))
}

fn parse_bounded(value: &str, name: &str, minimum: usize, maximum: usize) -> Result<usize, String> {
    let value = value
        .parse::<usize>()
        .map_err(|_| format!("{name} must be an integer"))?;
    if !(minimum..=maximum).contains(&value) {
        return Err(format!("{name} must be between {minimum} and {maximum}"));
    }
    Ok(value)
}

fn parse_args(arguments: impl IntoIterator<Item = String>) -> Result<Args, String> {
    let mut arguments = arguments.into_iter();
    let _program = arguments.next();
    let mut packet_size = 500;
    let mut packet_count = 10;
    let mut connection_type = ConnectionType::Acl;
    let mut mode = TestMode::Throughput;
    let mut interval_ms = 100;
    let mut transport = None;
    while let Some(argument) = arguments.next() {
        if matches!(argument.as_str(), "-h" | "--help") {
            return Err(usage().into());
        }
        if let Some(value) = option_value(&argument, Some("-s"), "--packet-size", &mut arguments)? {
            packet_size = parse_bounded(&value, "packet size", 8, 4096)?;
            continue;
        }
        if let Some(value) = option_value(&argument, Some("-c"), "--packet-count", &mut arguments)?
        {
            packet_count = parse_bounded(&value, "packet count", 1, 65535)?;
            continue;
        }
        if let Some(value) =
            option_value(&argument, Some("-t"), "--connection-type", &mut arguments)?
        {
            connection_type = ConnectionType::parse(&value)?;
            continue;
        }
        if let Some(value) = option_value(&argument, Some("-m"), "--mode", &mut arguments)? {
            mode = TestMode::parse(&value)?;
            continue;
        }
        if let Some(value) = option_value(&argument, None, "--interval", &mut arguments)? {
            interval_ms = value
                .parse()
                .map_err(|_| "interval must be a non-negative integer".to_string())?;
            continue;
        }
        if argument.starts_with('-') {
            return Err(format!("unknown option {argument:?}"));
        }
        if transport.replace(argument).is_some() {
            return Err("only one transport may be specified".into());
        }
    }
    if connection_type == ConnectionType::Sco && packet_size > u8::MAX as usize {
        return Err("the maximum packet size for SCO is 255".into());
    }
    Ok(Args {
        packet_size,
        packet_count,
        connection_type,
        mode,
        interval_ms,
        transport: transport.ok_or_else(|| "missing transport".to_string())?,
    })
}

fn send_success<T: PacketSource + PacketSink>(
    channel: &mut HciCommandChannel<T>,
    command: Command,
    name: &str,
) -> Result<(), String> {
    match channel
        .send_command(command)
        .map_err(|error| error.to_string())?
        .status()
    {
        Some(0) => Ok(()),
        Some(status) => Err(format!("{name} failed with status {status:#04x}")),
        None => Err(format!("{name} returned no status")),
    }
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
        } if return_parameters.status() == Some(0) => Ok(Some(return_parameters)),
        CommandResponse::Complete { .. } | CommandResponse::Status { .. } => Ok(None),
    }
}

fn ensure_loopback_supported<T: PacketSource + PacketSink>(
    channel: &mut HciCommandChannel<T>,
) -> Result<(), String> {
    if let Some(ReturnParameters::ReadLocalSupportedCommands {
        supported_commands, ..
    }) = query(channel, Command::ReadLocalSupportedCommands)?
    {
        if supported_commands[16] & 0b11 != 0b11 {
            return Err("loopback mode not supported".into());
        }
    }
    Ok(())
}

fn buffer_limits<T: PacketSource + PacketSink>(
    channel: &mut HciCommandChannel<T>,
    connection_type: ConnectionType,
) -> Result<(usize, usize), String> {
    let classic = query(channel, Command::ReadBufferSize)?;
    match connection_type {
        ConnectionType::Sco => match classic {
            Some(ReturnParameters::ReadBufferSize {
                hc_synchronous_data_packet_length,
                hc_total_num_synchronous_data_packets,
                ..
            }) if hc_synchronous_data_packet_length > 0
                && hc_total_num_synchronous_data_packets > 0 =>
            {
                Ok((
                    usize::from(hc_synchronous_data_packet_length),
                    usize::from(hc_total_num_synchronous_data_packets),
                ))
            }
            _ => Err("no SCO packet queue".into()),
        },
        ConnectionType::Acl => {
            if let Some(ReturnParameters::ReadBufferSize {
                hc_acl_data_packet_length,
                hc_total_num_acl_data_packets,
                ..
            }) = classic
            {
                if hc_acl_data_packet_length > 4 && hc_total_num_acl_data_packets > 0 {
                    return Ok((
                        usize::from(hc_acl_data_packet_length) - 4,
                        usize::from(hc_total_num_acl_data_packets),
                    ));
                }
            }
            if let Some(ReturnParameters::LeReadBufferSizeV2 {
                le_acl_data_packet_length,
                total_num_le_acl_data_packets,
                ..
            }) = query(channel, Command::LeReadBufferSizeV2)?
            {
                if le_acl_data_packet_length > 4 && total_num_le_acl_data_packets > 0 {
                    return Ok((
                        usize::from(le_acl_data_packet_length) - 4,
                        usize::from(total_num_le_acl_data_packets),
                    ));
                }
            }
            if let Some(ReturnParameters::LeReadBufferSize {
                le_acl_data_packet_length,
                total_num_le_acl_data_packets,
                ..
            }) = query(channel, Command::LeReadBufferSize)?
            {
                if le_acl_data_packet_length > 4 && total_num_le_acl_data_packets > 0 {
                    return Ok((
                        usize::from(le_acl_data_packet_length) - 4,
                        usize::from(total_num_le_acl_data_packets),
                    ));
                }
            }
            Err("no ACL packet queue".into())
        }
    }
}

fn connection_handle(
    packet: &HciPacket,
    connection_type: ConnectionType,
) -> Result<Option<u16>, String> {
    match (connection_type, packet) {
        (
            ConnectionType::Acl,
            HciPacket::Event(Event::ConnectionComplete {
                status,
                connection_handle,
                ..
            }),
        )
        | (
            ConnectionType::Acl,
            HciPacket::Event(Event::LeMeta(LeMetaEvent::ConnectionComplete {
                status,
                connection_handle,
                ..
            })),
        )
        | (
            ConnectionType::Acl,
            HciPacket::Event(Event::LeMeta(LeMetaEvent::EnhancedConnectionComplete {
                status,
                connection_handle,
                ..
            })),
        ) => {
            if *status == 0 {
                Ok(Some(*connection_handle))
            } else {
                Err(format!(
                    "loopback ACL connection failed with status {status:#04x}"
                ))
            }
        }
        (
            ConnectionType::Sco,
            HciPacket::Event(Event::SynchronousConnectionComplete {
                status,
                connection_handle,
                ..
            }),
        ) => {
            if *status == 0 {
                Ok(Some(*connection_handle))
            } else {
                Err(format!(
                    "loopback SCO connection failed with status {status:#04x}"
                ))
            }
        }
        _ => Ok(None),
    }
}

fn next_packet<T: PacketSource>(
    transport: &mut T,
    pending: &mut VecDeque<HciPacket>,
) -> Result<HciPacket, String> {
    if let Some(packet) = pending.pop_front() {
        return Ok(packet);
    }
    transport
        .read_packet()
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "transport ended before loopback completed".to_string())
}

fn wait_for_connection<T: PacketSource>(
    transport: &mut T,
    pending: &mut VecDeque<HciPacket>,
    connection_type: ConnectionType,
) -> Result<u16, String> {
    loop {
        let packet = next_packet(transport, pending)?;
        if let Some(handle) = connection_handle(&packet, connection_type)? {
            return Ok(handle);
        }
    }
}

fn payload(counter: usize, packet_size: usize) -> Vec<u8> {
    let mut payload = vec![0; packet_size];
    payload[..2].copy_from_slice(&(counter as u16).to_le_bytes());
    payload
}

fn write_payload<T: PacketSink>(
    transport: &mut T,
    connection_type: ConnectionType,
    connection_handle: u16,
    payload: Vec<u8>,
) -> Result<(), String> {
    let packet = match connection_type {
        ConnectionType::Acl => {
            let mut data = Vec::with_capacity(payload.len() + 4);
            data.extend_from_slice(&(payload.len() as u16).to_le_bytes());
            data.extend_from_slice(&0u16.to_le_bytes());
            data.extend_from_slice(&payload);
            HciPacket::AclData(AclDataPacket {
                connection_handle,
                pb_flag: HCI_ACL_PB_FIRST_NON_FLUSHABLE,
                bc_flag: 0,
                data_total_length: data.len() as u16,
                data,
            })
        }
        ConnectionType::Sco => HciPacket::SyncData(SynchronousDataPacket {
            connection_handle,
            packet_status: 0,
            data_total_length: payload.len() as u8,
            data: payload,
        }),
    };
    transport
        .write_packet(&packet)
        .and_then(|()| transport.flush())
        .map_err(|error| error.to_string())
}

fn echoed_payload(
    packet: &HciPacket,
    connection_type: ConnectionType,
    connection_handle: u16,
    acl_assembler: &mut AclDataPacketAssembler,
) -> Result<Option<Vec<u8>>, String> {
    match (connection_type, packet) {
        (ConnectionType::Acl, HciPacket::AclData(packet)) => {
            if packet.connection_handle != connection_handle {
                return Err(format!(
                    "received ACL data for handle {:#06x}, expected {connection_handle:#06x}",
                    packet.connection_handle
                ));
            }
            let Some(pdu) = acl_assembler
                .feed(packet)
                .map_err(|error| error.to_string())?
            else {
                return Ok(None);
            };
            if pdu.len() < 4 {
                return Err("looped ACL packet has a truncated L2CAP header".into());
            }
            Ok(Some(pdu[4..].to_vec()))
        }
        (ConnectionType::Sco, HciPacket::SyncData(packet)) => {
            if packet.connection_handle != connection_handle {
                return Err(format!(
                    "received SCO data for handle {:#06x}, expected {connection_handle:#06x}",
                    packet.connection_handle
                ));
            }
            Ok(Some(packet.data.clone()))
        }
        _ => Ok(None),
    }
}

fn run_loopback<T: PacketSource + PacketSink>(
    transport: T,
    args: &Args,
    mut emit: impl FnMut(String),
) -> Result<(), String> {
    let mut channel = HciCommandChannel::new(transport);
    send_success(&mut channel, Command::Reset, "HCI Reset")?;
    send_success(
        &mut channel,
        Command::SetEventMask {
            event_mask: LOOPBACK_EVENT_MASK,
        },
        "Set Event Mask",
    )?;
    ensure_loopback_supported(&mut channel)?;
    let (max_packet_size, window) = buffer_limits(&mut channel, args.connection_type)?;
    if args.packet_size > max_packet_size {
        return Err(format!(
            "packet size ({}) larger than max supported size ({max_packet_size})",
            args.packet_size
        ));
    }

    emit("### Setting loopback mode".into());
    send_success(
        &mut channel,
        Command::WriteLoopbackMode {
            loopback_mode: LOOPBACK_MODE_LOCAL,
        },
        "Write Loopback Mode",
    )?;
    emit("### Checking loopback mode".into());
    match query(&mut channel, Command::ReadLoopbackMode)? {
        Some(ReturnParameters::ReadLoopbackMode { loopback_mode, .. })
            if loopback_mode == LOOPBACK_MODE_LOCAL => {}
        Some(ReturnParameters::ReadLoopbackMode { .. }) => {
            return Err("loopback mode mismatch".into());
        }
        _ => return Err("loopback mode not supported".into()),
    }

    let (mut transport, pending) = channel.into_parts();
    let mut pending = pending.into();
    let connection_handle =
        wait_for_connection(&mut transport, &mut pending, args.connection_type)?;
    emit(format!("### Connected (handle={connection_handle:#06x})"));
    emit("=== Start sending".into());

    let started = Instant::now();
    let mut send_timestamps = Vec::with_capacity(args.packet_count);
    let mut rtts = Vec::with_capacity(args.packet_count);
    let mut next_to_send = 0usize;
    let mut received = 0usize;
    let mut in_flight = 0usize;
    let mut first_received = None;
    let mut last_received = None;
    let mut bytes_received = 0usize;
    let mut acl_assembler = AclDataPacketAssembler::new();

    while received < args.packet_count {
        let send_limit = if args.mode == TestMode::Rtt {
            1
        } else {
            window
        };
        while next_to_send < args.packet_count && in_flight < send_limit {
            emit(format!(
                ">>> Sending {} packet {next_to_send}: {} bytes",
                args.connection_type.label(),
                args.packet_size
            ));
            send_timestamps.push(Instant::now());
            write_payload(
                &mut transport,
                args.connection_type,
                connection_handle,
                payload(next_to_send, args.packet_size),
            )?;
            next_to_send += 1;
            in_flight += 1;
        }

        let packet = next_packet(&mut transport, &mut pending)?;
        let Some(payload) = echoed_payload(
            &packet,
            args.connection_type,
            connection_handle,
            &mut acl_assembler,
        )?
        else {
            continue;
        };
        if payload.len() != args.packet_size {
            return Err(format!(
                "received packet has {} bytes, expected {}",
                payload.len(),
                args.packet_size
            ));
        }
        let counter = usize::from(u16::from_le_bytes([payload[0], payload[1]]));
        if counter != received {
            return Err(format!(
                "received packet {counter}, expected packet {received}"
            ));
        }
        let now = Instant::now();
        let rtt = now.duration_since(send_timestamps[counter]).as_secs_f64();
        rtts.push(rtt);
        emit(format!(
            "<<< Received packet {counter}: {} bytes, RTT={rtt:.4}",
            payload.len()
        ));
        if let Some(first) = first_received {
            let last = last_received.expect("set with first_received");
            bytes_received += payload.len();
            if args.mode == TestMode::Throughput {
                let instant =
                    payload.len() as f64 / now.duration_since(last).as_secs_f64().max(f64::EPSILON);
                let average = bytes_received as f64
                    / now.duration_since(first).as_secs_f64().max(f64::EPSILON);
                emit(format!(
                    "@@@ RX speed: instant={instant:.4}, average={average:.4}"
                ));
            }
        } else {
            first_received = Some(now);
        }
        last_received = Some(now);
        received += 1;
        in_flight -= 1;

        if args.mode == TestMode::Rtt && next_to_send < args.packet_count {
            let target = Duration::from_millis(args.interval_ms);
            let elapsed = send_timestamps[counter].elapsed();
            if elapsed < target {
                thread::sleep(target - elapsed);
            }
        }
    }

    emit("@@@ Received last packet".into());
    emit("=== Done!".into());
    let bytes_sent = args.packet_size * args.packet_count;
    let elapsed = started.elapsed().as_secs_f64().max(f64::EPSILON);
    if args.mode == TestMode::Throughput {
        emit(format!(
            "@@@ TX speed: average={:.4} ({bytes_sent} bytes in {elapsed:.2} seconds)",
            bytes_sent as f64 / elapsed
        ));
    } else {
        let minimum = rtts.iter().copied().fold(f64::INFINITY, f64::min);
        let maximum = rtts.iter().copied().fold(0.0, f64::max);
        let average = rtts.iter().sum::<f64>() / rtts.len() as f64;
        emit(format!(
            "RTTs: min={minimum:.4}, max={maximum:.4}, avg={average:.4}"
        ));
    }
    Ok(())
}

fn run(args: Args) -> Result<(), String> {
    println!(">>> Connecting to HCI...");
    let transport = open_transport(&args.transport).map_err(|error| error.to_string())?;
    println!(">>> Connected");
    run_loopback(transport, &args, |line| println!("{line}"))
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
    use bumble::{Address, AddressType};
    use bumble_transport::Result;

    struct MockTransport {
        inbound: VecDeque<HciPacket>,
        outbound: Vec<HciPacket>,
        connection_type: ConnectionType,
        advertise_loopback: bool,
    }

    impl MockTransport {
        fn new(connection_type: ConnectionType) -> Self {
            Self {
                inbound: VecDeque::new(),
                outbound: Vec::new(),
                connection_type,
                advertise_loopback: true,
            }
        }

        fn command_response(&mut self, command: &Command) {
            let mut supported_commands = [0; 64];
            if self.advertise_loopback {
                supported_commands[16] = 0b11;
            }
            let return_parameters = match command {
                Command::ReadLocalSupportedCommands => {
                    ReturnParameters::ReadLocalSupportedCommands {
                        status: 0,
                        supported_commands,
                    }
                }
                Command::ReadBufferSize => ReturnParameters::ReadBufferSize {
                    status: 0,
                    hc_acl_data_packet_length: 64,
                    hc_synchronous_data_packet_length: 64,
                    hc_total_num_acl_data_packets: 4,
                    hc_total_num_synchronous_data_packets: 4,
                },
                Command::ReadLoopbackMode => ReturnParameters::ReadLoopbackMode {
                    status: 0,
                    loopback_mode: LOOPBACK_MODE_LOCAL,
                },
                Command::WriteLoopbackMode { .. } => {
                    let address = Address::from_bytes([0; 6], AddressType::PUBLIC_DEVICE);
                    self.inbound
                        .push_back(HciPacket::Event(Event::ConnectionComplete {
                            status: 0,
                            connection_handle: 0x000B,
                            bd_addr: address.clone(),
                            link_type: 1,
                            encryption_enabled: 0,
                        }));
                    if self.connection_type == ConnectionType::Sco {
                        self.inbound.push_back(HciPacket::Event(
                            Event::SynchronousConnectionComplete {
                                status: 0,
                                connection_handle: 0x000C,
                                bd_addr: address,
                                link_type: 0,
                                transmission_interval: 0,
                                retransmission_window: 0,
                                rx_packet_length: 64,
                                tx_packet_length: 64,
                                air_mode: 2,
                            },
                        ));
                    }
                    ReturnParameters::Status { status: 0 }
                }
                _ => ReturnParameters::Status { status: 0 },
            };
            self.inbound
                .push_back(HciPacket::Event(Event::CommandComplete {
                    num_hci_command_packets: 1,
                    command_opcode: command.op_code(),
                    return_parameters,
                }));
        }
    }

    impl PacketSource for MockTransport {
        fn read_packet(&mut self) -> Result<Option<HciPacket>> {
            Ok(self.inbound.pop_front())
        }
    }

    impl PacketSink for MockTransport {
        fn write_packet(&mut self, packet: &HciPacket) -> Result<()> {
            self.outbound.push(packet.clone());
            match packet {
                HciPacket::Command(command) => self.command_response(command),
                HciPacket::AclData(_) | HciPacket::SyncData(_) => {
                    self.inbound.push_back(packet.clone());
                }
                _ => {}
            }
            Ok(())
        }
    }

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(ToString::to_string).collect()
    }

    fn loopback_args(connection_type: ConnectionType, mode: TestMode) -> Args {
        Args {
            packet_size: 8,
            packet_count: 3,
            connection_type,
            mode,
            interval_ms: 0,
            transport: "mock".into(),
        }
    }

    #[test]
    fn parses_upstream_options_and_ranges() {
        assert_eq!(
            parse_args(args(&[
                "loopback",
                "-s",
                "32",
                "--packet-count=7",
                "-t",
                "sco",
                "--mode",
                "rtt",
                "--interval",
                "5",
                "tcp-client:localhost:6402",
            ])),
            Ok(Args {
                packet_size: 32,
                packet_count: 7,
                connection_type: ConnectionType::Sco,
                mode: TestMode::Rtt,
                interval_ms: 5,
                transport: "tcp-client:localhost:6402".into(),
            })
        );
        assert!(parse_args(args(&["loopback", "--packet-size", "7", "x"])).is_err());
        assert!(parse_args(args(&["loopback", "-t", "sco", "-s", "256", "x"])).is_err());
        assert!(parse_args(args(&["loopback"])).is_err());
    }

    #[test]
    fn acl_loopback_uses_l2cap_packets_and_a_bounded_window() {
        let transport = MockTransport::new(ConnectionType::Acl);
        let mut lines = Vec::new();
        run_loopback(
            transport,
            &loopback_args(ConnectionType::Acl, TestMode::Throughput),
            |line| lines.push(line),
        )
        .unwrap();
        let report = lines.join("\n");
        assert!(report.contains("### Connected (handle=0x000b)"));
        assert!(report.contains(">>> Sending ACL packet 2: 8 bytes"));
        assert!(report.contains("<<< Received packet 2: 8 bytes"));
        assert!(report.contains("@@@ TX speed: average="));
    }

    #[test]
    fn sco_loopback_reports_round_trip_statistics() {
        let transport = MockTransport::new(ConnectionType::Sco);
        let mut lines = Vec::new();
        run_loopback(
            transport,
            &loopback_args(ConnectionType::Sco, TestMode::Rtt),
            |line| lines.push(line),
        )
        .unwrap();
        let report = lines.join("\n");
        assert!(report.contains("### Connected (handle=0x000c)"));
        assert!(report.contains(">>> Sending SCO packet 2: 8 bytes"));
        assert!(report.contains("RTTs: min="));
    }

    #[test]
    fn rejects_controllers_without_loopback_commands() {
        let mut transport = MockTransport::new(ConnectionType::Acl);
        transport.advertise_loopback = false;
        let error = run_loopback(
            transport,
            &loopback_args(ConnectionType::Acl, TestMode::Throughput),
            |_| {},
        )
        .unwrap_err();
        assert_eq!(error, "loopback mode not supported");
    }
}
