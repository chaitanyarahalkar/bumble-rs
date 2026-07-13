use bumble::{Address, AddressType};
use bumble_hci::Command;
use bumble_host::{Device, LocalLink};
use bumble_l2cap::LeCreditBasedChannelSpec;
use bumble_transport::{open_split_transport, CommandResponse, ExternalHost, ExternalHostActivity};
use std::collections::{BTreeMap, VecDeque};
use std::io::{self, ErrorKind, Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Duration, Instant};

const COMMAND_TIMEOUT: Duration = Duration::from_secs(5);
const CONNECTION_TIMEOUT: Duration = Duration::from_secs(30);
const TCP_CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const POLL_INTERVAL: Duration = Duration::from_millis(10);
const TCP_READ_CHUNK: usize = 4096;

#[derive(Clone, Debug, PartialEq, Eq)]
enum Mode {
    Server {
        tcp_host: String,
        tcp_port: u16,
    },
    Client {
        bluetooth_address: String,
        tcp_host: String,
        tcp_port: u16,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Args {
    device_config: PathBuf,
    hci_transport: String,
    spec: LeCreditBasedChannelSpec,
    mode: Mode,
}

fn usage() -> &'static str {
    "usage: bumble-l2cap-bridge --device-config PATH --hci-transport TRANSPORT [--psm PSM] [--l2cap-max-credits N] [--l2cap-mtu N] [--l2cap-mps N] <server [--tcp-host HOST] [--tcp-port PORT] | client BLUETOOTH-ADDRESS [--tcp-host HOST] [--tcp-port PORT]>"
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

fn number(value: String, option: &str) -> Result<u16, String> {
    value
        .parse::<u16>()
        .map_err(|_| format!("invalid value {value:?} for {option}"))
}

fn parse_mode(mode: &str, mut arguments: VecDeque<String>) -> Result<Mode, String> {
    let (mut tcp_host, mut tcp_port) = if mode == "server" {
        ("localhost".to_string(), 9544)
    } else {
        ("_".to_string(), 9543)
    };
    let mut bluetooth_address = None;
    while let Some(argument) = arguments.pop_front() {
        if matches!(argument.as_str(), "-h" | "--help") {
            return Err(usage().into());
        }
        if let Some(value) = option_value(&argument, "--tcp-host", &mut arguments)? {
            tcp_host = value;
            continue;
        }
        if let Some(value) = option_value(&argument, "--tcp-port", &mut arguments)? {
            tcp_port = number(value, "--tcp-port")?;
            continue;
        }
        if argument.starts_with('-') {
            return Err(format!("unknown option {argument:?}"));
        }
        if mode == "server" || bluetooth_address.replace(argument).is_some() {
            return Err(usage().into());
        }
    }
    match mode {
        "server" => Ok(Mode::Server { tcp_host, tcp_port }),
        "client" => Ok(Mode::Client {
            bluetooth_address: bluetooth_address.ok_or_else(|| usage().to_string())?,
            tcp_host,
            tcp_port,
        }),
        _ => Err(usage().into()),
    }
}

fn parse_args(arguments: impl IntoIterator<Item = String>) -> Result<Args, String> {
    let mut arguments: VecDeque<_> = arguments.into_iter().skip(1).collect();
    let mut device_config = None;
    let mut hci_transport = None;
    let mut psm = 1234;
    let mut max_credits = 128;
    let mut mtu = 1024;
    let mut mps = 1024;
    let mode = loop {
        let argument = arguments.pop_front().ok_or_else(|| usage().to_string())?;
        if matches!(argument.as_str(), "server" | "client") {
            break argument;
        }
        if matches!(argument.as_str(), "-h" | "--help") {
            return Err(usage().into());
        }
        if let Some(value) = option_value(&argument, "--device-config", &mut arguments)? {
            device_config = Some(PathBuf::from(value));
            continue;
        }
        if let Some(value) = option_value(&argument, "--hci-transport", &mut arguments)? {
            hci_transport = Some(value);
            continue;
        }
        if let Some(value) = option_value(&argument, "--psm", &mut arguments)? {
            psm = number(value, "--psm")?;
            continue;
        }
        if let Some(value) = option_value(&argument, "--l2cap-max-credits", &mut arguments)? {
            max_credits = number(value, "--l2cap-max-credits")?;
            continue;
        }
        if let Some(value) = option_value(&argument, "--l2cap-mtu", &mut arguments)? {
            mtu = number(value, "--l2cap-mtu")?;
            continue;
        }
        if let Some(value) = option_value(&argument, "--l2cap-mps", &mut arguments)? {
            mps = number(value, "--l2cap-mps")?;
            continue;
        }
        return Err(if argument.starts_with('-') {
            format!("unknown option {argument:?}")
        } else {
            usage().into()
        });
    };
    let spec = LeCreditBasedChannelSpec {
        psm: Some(psm),
        mtu,
        mps,
        max_credits,
    }
    .validate()
    .map_err(|error| error.to_string())?;
    Ok(Args {
        device_config: device_config.ok_or_else(|| "--device-config is required".to_string())?,
        hci_transport: hci_transport.ok_or_else(|| "--hci-transport is required".to_string())?,
        spec,
        mode: parse_mode(&mode, arguments)?,
    })
}

fn configured_address(path: &Path) -> Result<Address, String> {
    let bytes = std::fs::read(path)
        .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
    let config: serde_json::Value = serde_json::from_slice(&bytes)
        .map_err(|error| format!("invalid device config: {error}"))?;
    let address = config
        .get("address")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| "device config does not contain an address".to_string())?;
    Address::parse(address, AddressType::RANDOM_DEVICE).map_err(|error| error.to_string())
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

fn wait_for_connection(
    host: &mut ExternalHost,
    device: &mut Device,
    peer: Option<&Address>,
) -> Result<u16, String> {
    let deadline = Instant::now() + CONNECTION_TIMEOUT;
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

fn connect_bluetooth(
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PipeState {
    Pending,
    Open,
    Closed,
}

struct TcpPipe {
    connection_handle: u16,
    source_cid: u16,
    stream: TcpStream,
    outgoing_channel: bool,
    pending_to_tcp: VecDeque<u8>,
    reading_paused: bool,
}

impl TcpPipe {
    fn new(
        connection_handle: u16,
        source_cid: u16,
        stream: TcpStream,
        outgoing_channel: bool,
    ) -> io::Result<Self> {
        stream.set_nonblocking(true)?;
        stream.set_nodelay(true)?;
        Ok(Self {
            connection_handle,
            source_cid,
            stream,
            outgoing_channel,
            pending_to_tcp: VecDeque::new(),
            reading_paused: false,
        })
    }

    fn set_reading_paused(
        &mut self,
        device: &mut Device,
        link: &mut LocalLink,
        paused: bool,
    ) -> Result<(), String> {
        if self.reading_paused == paused {
            return Ok(());
        }
        device
            .set_le_credit_reading_paused(link, self.connection_handle, self.source_cid, paused)
            .map_err(|error| error.to_string())?;
        self.reading_paused = paused;
        Ok(())
    }

    fn disconnect(&mut self, device: &mut Device, link: &mut LocalLink) -> Result<(), String> {
        let _ = self.stream.shutdown(Shutdown::Both);
        if device
            .le_credit_channel(self.connection_handle, self.source_cid)
            .is_some()
        {
            device
                .disconnect_le_credit_channel(link, self.connection_handle, self.source_cid)
                .map_err(|error| error.to_string())?;
        }
        Ok(())
    }

    fn flush_tcp(&mut self) -> io::Result<()> {
        while !self.pending_to_tcp.is_empty() {
            let front = self.pending_to_tcp.as_slices().0;
            match self.stream.write(front) {
                Ok(0) => return Err(io::Error::new(ErrorKind::BrokenPipe, "TCP socket closed")),
                Ok(written) => {
                    self.pending_to_tcp.drain(..written);
                }
                Err(error) if error.kind() == ErrorKind::WouldBlock => break,
                Err(error) => return Err(error),
            }
        }
        Ok(())
    }

    fn pump(&mut self, device: &mut Device, link: &mut LocalLink) -> Result<PipeState, String> {
        let channel = device.le_credit_channel(self.connection_handle, self.source_cid);
        if channel.is_none() {
            if self.outgoing_channel
                && device
                    .le_credit_connection_result(self.connection_handle, self.source_cid)
                    .is_none()
            {
                return Ok(PipeState::Pending);
            }
            let _ = self.stream.shutdown(Shutdown::Both);
            return Ok(PipeState::Closed);
        }

        for sdu in device.take_le_credit_sdus(self.connection_handle, self.source_cid) {
            self.pending_to_tcp.extend(sdu);
        }
        if !self.pending_to_tcp.is_empty() {
            if let Err(error) = self.flush_tcp() {
                eprintln!("!!! TCP write failed: {error}");
                self.disconnect(device, link)?;
                return Ok(PipeState::Closed);
            }
            self.set_reading_paused(device, link, !self.pending_to_tcp.is_empty())?;
        } else {
            self.set_reading_paused(device, link, false)?;
        }

        if !self.pending_to_tcp.is_empty()
            || !device.le_credit_output_is_drained(self.connection_handle, self.source_cid)
        {
            return Ok(PipeState::Open);
        }
        let peer_mtu = usize::from(
            device
                .le_credit_channel(self.connection_handle, self.source_cid)
                .expect("channel was checked above")
                .peer_mtu,
        );
        let mut buffer = vec![0; peer_mtu.min(TCP_READ_CHUNK)];
        match self.stream.read(&mut buffer) {
            Ok(0) => {
                self.disconnect(device, link)?;
                Ok(PipeState::Closed)
            }
            Ok(read) => {
                device
                    .send_le_credit_sdu(
                        link,
                        self.connection_handle,
                        self.source_cid,
                        &buffer[..read],
                    )
                    .map_err(|error| error.to_string())?;
                Ok(PipeState::Open)
            }
            Err(error) if error.kind() == ErrorKind::WouldBlock => Ok(PipeState::Open),
            Err(error) => {
                eprintln!("!!! TCP read failed: {error}");
                self.disconnect(device, link)?;
                Ok(PipeState::Closed)
            }
        }
    }
}

fn connect_tcp(host: &str, port: u16) -> io::Result<TcpStream> {
    let addresses: Vec<_> = (host, port).to_socket_addrs()?.collect();
    let mut last_error = None;
    for address in addresses {
        match TcpStream::connect_timeout(&address, TCP_CONNECT_TIMEOUT) {
            Ok(stream) => return Ok(stream),
            Err(error) => last_error = Some(error),
        }
    }
    Err(last_error.unwrap_or_else(|| {
        io::Error::new(ErrorKind::AddrNotAvailable, "host resolved to no addresses")
    }))
}

fn wait_tick(host: &mut ExternalHost) -> Result<bool, String> {
    match host
        .wait_for_activity(POLL_INTERVAL)
        .map_err(|error| error.to_string())?
    {
        ExternalHostActivity::Packet | ExternalHostActivity::Timeout => Ok(true),
        ExternalHostActivity::Ended => Ok(false),
    }
}

fn check_l2cap_errors(device: &mut Device) -> Result<(), String> {
    if let Some((handle, error)) = device.take_le_credit_errors().into_iter().next() {
        Err(format!("L2CAP error on handle {handle:#06x}: {error}"))
    } else {
        Ok(())
    }
}

fn pump_pipes(
    pipes: &mut BTreeMap<u16, TcpPipe>,
    device: &mut Device,
    host: &mut ExternalHost,
) -> Result<(), String> {
    let cids: Vec<_> = pipes.keys().copied().collect();
    for cid in cids {
        let state = pipes
            .get_mut(&cid)
            .expect("CID came from the map")
            .pump(device, host)?;
        if state == PipeState::Closed {
            println!("*** L2CAP channel {cid:#06x} closed");
            pipes.remove(&cid);
        }
    }
    Ok(())
}

fn run_server_connection(
    host: &mut ExternalHost,
    device: &mut Device,
    handle: u16,
    tcp_host: &str,
    tcp_port: u16,
) -> Result<bool, String> {
    let mut pipes = BTreeMap::new();
    loop {
        device.poll(host);
        check_l2cap_errors(device)?;
        for cid in device.take_accepted_le_credit_channels(handle) {
            println!("*** L2CAP channel {cid:#06x}");
            println!("### Connecting to TCP {tcp_host}:{tcp_port}...");
            match connect_tcp(tcp_host, tcp_port) {
                Ok(stream) => {
                    println!("### Connected");
                    match TcpPipe::new(handle, cid, stream, false) {
                        Ok(pipe) => {
                            pipes.insert(cid, pipe);
                        }
                        Err(error) => {
                            eprintln!("!!! TCP setup failed: {error}");
                            device
                                .disconnect_le_credit_channel(host, handle, cid)
                                .map_err(|error| error.to_string())?;
                        }
                    }
                }
                Err(error) => {
                    eprintln!("!!! Connection failed: {error}");
                    device
                        .disconnect_le_credit_channel(host, handle, cid)
                        .map_err(|error| error.to_string())?;
                }
            }
        }
        pump_pipes(&mut pipes, device, host)?;
        if !device.is_connected_on_handle(handle) {
            println!("@@@ Bluetooth disconnection");
            return Ok(true);
        }
        if !wait_tick(host)? {
            return Ok(false);
        }
    }
}

fn run_server(
    host: &mut ExternalHost,
    device: &mut Device,
    own_address_type: u8,
    spec: LeCreditBasedChannelSpec,
    tcp_host: &str,
    tcp_port: u16,
) -> Result<(), String> {
    let psm = device
        .register_le_credit_server(spec)
        .map_err(|error| error.to_string())?;
    println!("### Listening for channel connection on PSM {psm}");
    loop {
        println!("### Waiting for Bluetooth connection...");
        let handle = advertise_and_wait(host, device, own_address_type)?;
        println!("@@@ Bluetooth connection on handle {handle:#06x}");
        if !run_server_connection(host, device, handle, tcp_host, tcp_port)? {
            return Ok(());
        }
    }
}

fn run_client(
    host: &mut ExternalHost,
    device: &mut Device,
    own_address_type: u8,
    spec: LeCreditBasedChannelSpec,
    bluetooth_address: &str,
    tcp_host: &str,
    tcp_port: u16,
) -> Result<(), String> {
    let peer = Address::parse(bluetooth_address, AddressType::RANDOM_DEVICE)
        .map_err(|error| error.to_string())?;
    println!("### Connecting to {peer}...");
    let handle = connect_bluetooth(host, device, peer, own_address_type)?;
    println!("### Connected on handle {handle:#06x}");

    let bind_host = if tcp_host == "_" { "0.0.0.0" } else { tcp_host };
    let listener = TcpListener::bind((bind_host, tcp_port)).map_err(|error| error.to_string())?;
    listener
        .set_nonblocking(true)
        .map_err(|error| error.to_string())?;
    println!(
        "### Listening for TCP connections on {}",
        listener.local_addr().map_err(|error| error.to_string())?
    );
    let psm = spec.psm.expect("validated bridge spec has a PSM");
    let mut pipes = BTreeMap::new();
    loop {
        device.poll(host);
        check_l2cap_errors(device)?;
        loop {
            match listener.accept() {
                Ok((stream, peer)) => {
                    println!("<<< TCP connection from {peer}");
                    let cid = match device.connect_le_credit_channel(host, handle, psm, spec) {
                        Ok(cid) => cid,
                        Err(error) => {
                            eprintln!("!!! L2CAP connection failed: {error}");
                            let _ = stream.shutdown(Shutdown::Both);
                            continue;
                        }
                    };
                    println!(">>> Opening L2CAP channel {cid:#06x} on PSM {psm}");
                    match TcpPipe::new(handle, cid, stream, true) {
                        Ok(pipe) => {
                            pipes.insert(cid, pipe);
                        }
                        Err(error) => {
                            return Err(format!("TCP setup failed: {error}"));
                        }
                    }
                }
                Err(error) if error.kind() == ErrorKind::WouldBlock => break,
                Err(error) => return Err(error.to_string()),
            }
        }
        pump_pipes(&mut pipes, device, host)?;
        if !device.is_connected_on_handle(handle) {
            return Err("Bluetooth connection ended".into());
        }
        if !wait_tick(host)? {
            return Ok(());
        }
    }
}

fn run(args: Args) -> Result<(), String> {
    let local_address = configured_address(&args.device_config)?;
    println!("<<< connecting to HCI...");
    let transport = open_split_transport(&args.hci_transport).map_err(|error| error.to_string())?;
    let mut host = ExternalHost::new(transport);
    let mut device = Device::new(0);
    host.initialize_device(&mut device, COMMAND_TIMEOUT)
        .map_err(|error| error.to_string())?;
    println!("<<< connected");
    let own_address_type = u8::from(!local_address.is_public());
    if own_address_type != 0 {
        command(
            &mut host,
            Command::LeSetRandomAddress {
                random_address: local_address,
            },
            "setting local random address",
        )?;
    }
    match args.mode {
        Mode::Server { tcp_host, tcp_port } => run_server(
            &mut host,
            &mut device,
            own_address_type,
            args.spec,
            &tcp_host,
            tcp_port,
        ),
        Mode::Client {
            bluetooth_address,
            tcp_host,
            tcp_port,
        } => run_client(
            &mut host,
            &mut device,
            own_address_type,
            args.spec,
            &bluetooth_address,
            &tcp_host,
            tcp_port,
        ),
    }
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
    use bumble_host::pump;
    use std::net::TcpListener;

    fn address(value: &str) -> Address {
        Address::parse(value, AddressType::RANDOM_DEVICE).unwrap()
    }

    #[test]
    fn parses_upstream_server_and_client_cli_shapes() {
        let server = parse_args(
            [
                "l2cap-bridge",
                "--device-config",
                "device.json",
                "--hci-transport=usb:0",
                "--psm",
                "2345",
                "--l2cap-max-credits",
                "64",
                "--l2cap-mtu",
                "512",
                "--l2cap-mps",
                "128",
                "server",
                "--tcp-host",
                "example.com",
                "--tcp-port",
                "9000",
            ]
            .map(str::to_string),
        )
        .unwrap();
        assert_eq!(server.device_config, PathBuf::from("device.json"));
        assert_eq!(server.hci_transport, "usb:0");
        assert_eq!(server.spec.psm, Some(2345));
        assert_eq!(server.spec.max_credits, 64);
        assert_eq!(server.spec.mtu, 512);
        assert_eq!(server.spec.mps, 128);
        assert_eq!(
            server.mode,
            Mode::Server {
                tcp_host: "example.com".into(),
                tcp_port: 9000,
            }
        );

        let client = parse_args(
            [
                "l2cap-bridge",
                "--device-config=config.json",
                "--hci-transport",
                "tcp-client:localhost:1234",
                "client",
                "C4:F2:17:1A:1D:BB",
            ]
            .map(str::to_string),
        )
        .unwrap();
        assert_eq!(
            client.mode,
            Mode::Client {
                bluetooth_address: "C4:F2:17:1A:1D:BB".into(),
                tcp_host: "_".into(),
                tcp_port: 9543,
            }
        );
        assert!(parse_args(["l2cap-bridge", "server"].map(str::to_string)).is_err());
        assert!(parse_args(
            [
                "l2cap-bridge",
                "--device-config=x",
                "--hci-transport=y",
                "--l2cap-max-credits=0",
                "server",
            ]
            .map(str::to_string)
        )
        .is_err());
    }

    #[test]
    fn tcp_pipe_bridges_both_directions_over_live_le_credit_channel() {
        let mut link = ControllerLocalLink::new();
        let central_id =
            link.add_controller(Controller::new("central", address("00:00:00:00:00:01")));
        let peripheral_id =
            link.add_controller(Controller::new("peripheral", address("00:00:00:00:00:02")));
        let mut devices = [Device::new(central_id), Device::new(peripheral_id)];
        let spec = LeCreditBasedChannelSpec {
            psm: Some(0x1234),
            mtu: 128,
            mps: 64,
            max_credits: 4,
        };
        devices[1].register_le_credit_server(spec).unwrap();
        let peripheral_address = address("C4:F2:17:1A:1D:BB");
        devices[0].set_random_address(&mut link, address("C4:F2:17:1A:1D:AA"));
        devices[1].set_random_address(&mut link, peripheral_address.clone());
        assert!(devices[1].start_advertising(&mut link, &[]));
        devices[0].connect_le(&mut link, peripheral_address);
        pump(&mut link, &mut devices);
        let central_handle = devices[0].connection_handle().unwrap();
        let peripheral_handle = devices[1].connection_handle().unwrap();
        let central_cid = devices[0]
            .connect_le_credit_channel(&mut link, central_handle, 0x1234, spec)
            .unwrap();
        pump(&mut link, &mut devices);
        let peripheral_cid = devices[1]
            .take_accepted_le_credit_channels(peripheral_handle)
            .into_iter()
            .next()
            .unwrap();

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let mut tcp_peer = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (bridge_stream, _) = listener.accept().unwrap();
        tcp_peer
            .set_read_timeout(Some(Duration::from_secs(1)))
            .unwrap();
        let mut pipe = TcpPipe::new(central_handle, central_cid, bridge_stream, false).unwrap();

        tcp_peer.write_all(b"TCP to L2CAP").unwrap();
        let deadline = Instant::now() + Duration::from_secs(1);
        let tcp_to_l2cap = loop {
            assert_eq!(
                pipe.pump(&mut devices[0], &mut link).unwrap(),
                PipeState::Open
            );
            pump(&mut link, &mut devices);
            let received = devices[1]
                .take_le_credit_sdus(peripheral_handle, peripheral_cid)
                .concat();
            if !received.is_empty() {
                break received;
            }
            assert!(Instant::now() < deadline, "TCP data was not bridged");
            std::thread::yield_now();
        };
        assert_eq!(tcp_to_l2cap, b"TCP to L2CAP");

        devices[1]
            .send_le_credit_sdu(
                &mut link,
                peripheral_handle,
                peripheral_cid,
                b"L2CAP to TCP",
            )
            .unwrap();
        pump(&mut link, &mut devices);
        assert_eq!(
            pipe.pump(&mut devices[0], &mut link).unwrap(),
            PipeState::Open
        );
        let mut received = [0; 12];
        tcp_peer.read_exact(&mut received).unwrap();
        assert_eq!(&received, b"L2CAP to TCP");
    }
}
