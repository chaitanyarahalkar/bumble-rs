use bumble::advertising_data::Type as AdvertisingDataType;
use bumble::{Address, AddressType, AdvertisingData, Uuid};
use bumble_att::AttPdu;
use bumble_gatt::{
    permissions, properties, CharacteristicDefinition, DynamicValue, GattClient, GattServer,
    ServiceDefinition, GATT_CLIENT_CHARACTERISTIC_CONFIGURATION_UUID,
};
use bumble_hci::Command;
use bumble_host::{Device, LocalLink};
use bumble_l2cap::{LeCreditBasedChannelSpec, LeCreditBasedChannelState};
use bumble_transport::{
    open_split_transport, CommandResponse, ExternalAttTransport, ExternalHost, ExternalHostActivity,
};
use std::collections::VecDeque;
use std::io::{self, ErrorKind};
use std::net::UdpSocket;
use std::process::ExitCode;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const GG_GATTLINK_SERVICE_UUID: &str = "ABBAFF00-E56A-484C-B832-8B17CF6CBFE8";
const GG_GATTLINK_RX_CHARACTERISTIC_UUID: &str = "ABBAFF01-E56A-484C-B832-8B17CF6CBFE8";
const GG_GATTLINK_TX_CHARACTERISTIC_UUID: &str = "ABBAFF02-E56A-484C-B832-8B17CF6CBFE8";
const GG_GATTLINK_L2CAP_CHANNEL_PSM_CHARACTERISTIC_UUID: &str =
    "ABBAFF03-E56A-484C-B832-8B17CF6CBFE8";
const GG_PREFERRED_MTU: u16 = 256;
const GG_L2CAP_PSM: u16 = 0x00FB;
const GG_L2CAP_MTU: u16 = 2048;
const GG_L2CAP_MPS: u16 = 2048;
const GG_L2CAP_CREDITS: u16 = 256;
const MAX_GG_PACKET_SIZE: usize = 256;
const MAX_GATT_PACKET_SIZE: usize = GG_PREFERRED_MTU as usize - 3;
const UDP_QUEUE_LIMIT: usize = 256;
const COMMAND_TIMEOUT: Duration = Duration::from_secs(5);
const PROCEDURE_TIMEOUT: Duration = Duration::from_secs(30);
const POLL_INTERVAL: Duration = Duration::from_millis(10);

#[derive(Clone, Debug, PartialEq, Eq)]
struct Args {
    hci_transport: String,
    device_address: String,
    role_or_peer_address: String,
    send_host: String,
    send_port: u16,
    receive_host: String,
    receive_port: u16,
}

fn usage() -> &'static str {
    "usage: bumble-gg-bridge HCI_TRANSPORT DEVICE_ADDRESS <node|PEER_ADDRESS> [-sh|--send-host HOST] [-sp|--send-port PORT] [-rh|--receive-host HOST] [-rp|--receive-port PORT]"
}

fn option_value(
    argument: &str,
    short: &str,
    long: &str,
    arguments: &mut VecDeque<String>,
) -> Result<Option<String>, String> {
    if argument == short || argument == long {
        return arguments
            .pop_front()
            .map(Some)
            .ok_or_else(|| format!("missing value for {long}"));
    }
    Ok(argument
        .strip_prefix(&format!("{short}="))
        .or_else(|| argument.strip_prefix(&format!("{long}=")))
        .map(ToOwned::to_owned))
}

fn port(value: String, option: &str) -> Result<u16, String> {
    value
        .parse::<u16>()
        .map_err(|_| format!("invalid port {value:?} for {option}"))
}

fn parse_args(arguments: impl IntoIterator<Item = String>) -> Result<Args, String> {
    let mut arguments: VecDeque<_> = arguments.into_iter().skip(1).collect();
    let mut positionals = Vec::new();
    let mut send_host = "127.0.0.1".to_string();
    let mut send_port = 9001;
    let mut receive_host = "127.0.0.1".to_string();
    let mut receive_port = 9000;
    while let Some(argument) = arguments.pop_front() {
        if matches!(argument.as_str(), "-h" | "--help") {
            return Err(usage().into());
        }
        if let Some(value) = option_value(&argument, "-sh", "--send-host", &mut arguments)? {
            send_host = value;
            continue;
        }
        if let Some(value) = option_value(&argument, "-sp", "--send-port", &mut arguments)? {
            send_port = port(value, "--send-port")?;
            continue;
        }
        if let Some(value) = option_value(&argument, "-rh", "--receive-host", &mut arguments)? {
            receive_host = value;
            continue;
        }
        if let Some(value) = option_value(&argument, "-rp", "--receive-port", &mut arguments)? {
            receive_port = port(value, "--receive-port")?;
            continue;
        }
        if argument.starts_with('-') {
            return Err(format!("unknown option {argument:?}"));
        }
        positionals.push(argument);
    }
    if positionals.len() != 3 {
        return Err(usage().into());
    }
    Ok(Args {
        hci_transport: positionals.remove(0),
        device_address: positionals.remove(0),
        role_or_peer_address: positionals.remove(0),
        send_host,
        send_port,
        receive_host,
        receive_port,
    })
}

fn uuid(value: &str) -> Uuid {
    Uuid::parse(value).expect("Gattlink UUID constant is valid")
}

fn coc_spec(psm: u16) -> LeCreditBasedChannelSpec {
    LeCreditBasedChannelSpec {
        psm: Some(psm),
        mtu: GG_L2CAP_MTU,
        mps: GG_L2CAP_MPS,
        max_credits: GG_L2CAP_CREDITS,
    }
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

fn connect_bluetooth(
    host: &mut ExternalHost,
    device: &mut Device,
    peer: Address,
) -> Result<u16, String> {
    command(
        host,
        Command::LeCreateConnection {
            le_scan_interval: 0x0010,
            le_scan_window: 0x0010,
            initiator_filter_policy: 0,
            peer_address_type: u8::from(!peer.is_public()),
            peer_address: peer.clone(),
            own_address_type: 1,
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

fn gg_advertising_data() -> Vec<u8> {
    AdvertisingData {
        ad_structures: vec![
            (
                AdvertisingDataType::COMPLETE_LOCAL_NAME,
                b"Bumble GG".to_vec(),
            ),
            (
                AdvertisingDataType::INCOMPLETE_LIST_OF_128_BIT_SERVICE_CLASS_UUIDS,
                uuid(GG_GATTLINK_SERVICE_UUID).to_bytes(true),
            ),
        ],
    }
    .to_bytes()
}

fn advertise_and_wait(host: &mut ExternalHost, device: &mut Device) -> Result<u16, String> {
    command(
        host,
        Command::LeSetAdvertisingParameters {
            advertising_interval_min: 0x0800,
            advertising_interval_max: 0x0800,
            advertising_type: 0,
            own_address_type: 1,
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
            advertising_data: gg_advertising_data(),
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

fn wait_for_coc(
    host: &mut ExternalHost,
    device: &mut Device,
    connection_handle: u16,
    source_cid: u16,
) -> Result<(), String> {
    let deadline = Instant::now() + PROCEDURE_TIMEOUT;
    loop {
        device.poll(host);
        let state = device
            .le_credit_channel(connection_handle, source_cid)
            .map(|channel| channel.state);
        match state {
            Some(LeCreditBasedChannelState::Connected) => return Ok(()),
            None if device
                .le_credit_connection_result(connection_handle, source_cid)
                .is_none() => {}
            Some(LeCreditBasedChannelState::Disconnected) | None => {
                return Err(format!(
                    "LE credit channel failed with result {:?}",
                    device.le_credit_connection_result(connection_handle, source_cid)
                ))
            }
            Some(LeCreditBasedChannelState::Disconnecting) => {}
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err("timed out opening Gattlink CoC".into());
        }
        match host
            .wait_for_activity(remaining)
            .map_err(|error| error.to_string())?
        {
            ExternalHostActivity::Packet => {}
            ExternalHostActivity::Timeout => return Err("timed out opening Gattlink CoC".into()),
            ExternalHostActivity::Ended => {
                return Err("HCI transport ended while opening Gattlink CoC".into())
            }
        }
    }
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

#[derive(Default)]
struct GattlinkPacketDecoder {
    packet: Vec<u8>,
    packet_size: usize,
}

impl GattlinkPacketDecoder {
    fn push_sdu(&mut self, mut sdu: &[u8]) -> Vec<Vec<u8>> {
        let mut packets = Vec::new();
        while !sdu.is_empty() {
            if self.packet_size == 0 {
                self.packet_size = usize::from(sdu[0]) + 1;
                sdu = &sdu[1..];
                continue;
            }
            let bytes_needed = self.packet_size - self.packet.len();
            let chunk = bytes_needed.min(sdu.len());
            self.packet.extend_from_slice(&sdu[..chunk]);
            sdu = &sdu[chunk..];
            if self.packet.len() == self.packet_size {
                packets.push(std::mem::take(&mut self.packet));
                self.packet_size = 0;
            }
        }
        packets
    }
}

fn frame_packet(packet: &[u8]) -> Result<Vec<u8>, String> {
    if packet.is_empty() || packet.len() > MAX_GG_PACKET_SIZE {
        return Err(format!(
            "Gattlink packet size must be between 1 and {MAX_GG_PACKET_SIZE} bytes"
        ));
    }
    let mut framed = Vec::with_capacity(packet.len() + 1);
    framed.push(u8::try_from(packet.len() - 1).expect("validated Gattlink packet size"));
    framed.extend_from_slice(packet);
    Ok(framed)
}

struct UdpEndpoint {
    receiver: UdpSocket,
    sender: UdpSocket,
    pending: VecDeque<Vec<u8>>,
}

impl UdpEndpoint {
    fn open(args: &Args) -> Result<Self, String> {
        let receiver = UdpSocket::bind((args.receive_host.as_str(), args.receive_port))
            .map_err(|error| format!("failed to bind UDP receiver: {error}"))?;
        let sender = UdpSocket::bind("0.0.0.0:0")
            .map_err(|error| format!("failed to bind UDP sender: {error}"))?;
        sender
            .connect((args.send_host.as_str(), args.send_port))
            .map_err(|error| format!("failed to connect UDP sender: {error}"))?;
        Self::from_sockets(receiver, sender).map_err(|error| error.to_string())
    }

    fn from_sockets(receiver: UdpSocket, sender: UdpSocket) -> io::Result<Self> {
        receiver.set_nonblocking(true)?;
        sender.set_nonblocking(true)?;
        Ok(Self {
            receiver,
            sender,
            pending: VecDeque::new(),
        })
    }

    fn poll_receive(&mut self) -> io::Result<()> {
        let mut buffer = [0; 65_535];
        while self.pending.len() < UDP_QUEUE_LIMIT {
            match self.receiver.recv_from(&mut buffer) {
                Ok((size, _)) if size == 0 || size > MAX_GG_PACKET_SIZE => {
                    eprintln!(
                        "!!! Dropping UDP datagram: Gattlink size must be 1..={MAX_GG_PACKET_SIZE}, got {size}"
                    );
                }
                Ok((size, _)) => {
                    println!("<<< [UDP]: {size} bytes");
                    self.pending.push_back(buffer[..size].to_vec());
                }
                Err(error) if error.kind() == ErrorKind::WouldBlock => break,
                Err(error) => return Err(error),
            }
        }
        Ok(())
    }

    fn send_packet(&self, packet: &[u8]) -> io::Result<()> {
        let sent = self.sender.send(packet)?;
        if sent == packet.len() {
            println!(">>> [UDP]: {sent} bytes");
            Ok(())
        } else {
            Err(io::Error::new(
                ErrorKind::WriteZero,
                format!("UDP socket sent {sent} of {} bytes", packet.len()),
            ))
        }
    }
}

struct CocBridge {
    connection_handle: u16,
    source_cid: u16,
    decoder: GattlinkPacketDecoder,
}

impl CocBridge {
    fn new(connection_handle: u16, source_cid: u16) -> Self {
        Self {
            connection_handle,
            source_cid,
            decoder: GattlinkPacketDecoder::default(),
        }
    }

    fn is_connected(&self, device: &Device) -> bool {
        device
            .le_credit_channel(self.connection_handle, self.source_cid)
            .is_some_and(|channel| channel.state == LeCreditBasedChannelState::Connected)
    }

    fn pump(
        &mut self,
        link: &mut LocalLink,
        device: &mut Device,
        udp: &mut UdpEndpoint,
    ) -> Result<(), String> {
        for sdu in device.take_le_credit_sdus(self.connection_handle, self.source_cid) {
            println!("<<< [L2CAP SDU]: {} bytes", sdu.len());
            for packet in self.decoder.push_sdu(&sdu) {
                println!("<<< [L2CAP PACKET]: {} bytes", packet.len());
                udp.send_packet(&packet)
                    .map_err(|error| error.to_string())?;
            }
        }
        while !udp.pending.is_empty()
            && device.le_credit_output_is_drained(self.connection_handle, self.source_cid)
        {
            let packet = udp.pending.pop_front().expect("queue is not empty");
            println!(">>> [L2CAP]: {} bytes", packet.len());
            device
                .send_le_credit_sdu(
                    link,
                    self.connection_handle,
                    self.source_cid,
                    &frame_packet(&packet)?,
                )
                .map_err(|error| error.to_string())?;
        }
        Ok(())
    }
}

type PacketQueue = Arc<Mutex<VecDeque<Vec<u8>>>>;

struct NodeGatt {
    server: GattServer,
    tx_handle: u16,
    rx_packets: PacketQueue,
}

fn build_node_gatt() -> Result<NodeGatt, String> {
    let service_uuid = uuid(GG_GATTLINK_SERVICE_UUID);
    let rx_uuid = uuid(GG_GATTLINK_RX_CHARACTERISTIC_UUID);
    let tx_uuid = uuid(GG_GATTLINK_TX_CHARACTERISTIC_UUID);
    let psm_uuid = uuid(GG_GATTLINK_L2CAP_CHANNEL_PSM_CHARACTERISTIC_UUID);
    let mut server = GattServer::from_definitions(vec![ServiceDefinition {
        uuid: service_uuid,
        primary: true,
        included_services: Vec::new(),
        characteristics: vec![
            CharacteristicDefinition {
                uuid: rx_uuid.clone(),
                properties: properties::WRITE_WITHOUT_RESPONSE,
                permissions: permissions::WRITEABLE,
                value: Vec::new(),
                descriptors: Vec::new(),
            },
            CharacteristicDefinition {
                uuid: tx_uuid.clone(),
                properties: properties::NOTIFY,
                permissions: permissions::READABLE,
                value: Vec::new(),
                descriptors: Vec::new(),
            },
            CharacteristicDefinition {
                uuid: psm_uuid.clone(),
                properties: properties::READ | properties::NOTIFY,
                permissions: permissions::READABLE,
                value: GG_L2CAP_PSM.to_le_bytes().to_vec(),
                descriptors: Vec::new(),
            },
        ],
    }])
    .map_err(|error| error.to_string())?;
    let rx_handle = server
        .handles_by_uuid(&rx_uuid)
        .into_iter()
        .next()
        .ok_or_else(|| "Gattlink RX characteristic has no value handle".to_string())?;
    let tx_handle = server
        .handles_by_uuid(&tx_uuid)
        .into_iter()
        .next()
        .ok_or_else(|| "Gattlink TX characteristic has no value handle".to_string())?;
    server
        .handles_by_uuid(&psm_uuid)
        .into_iter()
        .next()
        .ok_or_else(|| "Gattlink PSM characteristic has no value handle".to_string())?;
    let rx_packets = Arc::new(Mutex::new(VecDeque::new()));
    let callback_packets = Arc::clone(&rx_packets);
    server
        .set_dynamic_value(
            rx_handle,
            DynamicValue::write_only(move |_, value| {
                let mut packets = callback_packets.lock().map_err(|_| 0x0E)?;
                if packets.len() >= UDP_QUEUE_LIMIT {
                    return Err(0x11);
                }
                packets.push_back(value.to_vec());
                Ok(())
            }),
        )
        .map_err(|error| error.to_string())?;
    Ok(NodeGatt {
        server,
        tx_handle,
        rx_packets,
    })
}

struct HubGatt {
    client: GattClient,
    max_packet_size: usize,
    rx_handle: Option<u16>,
    tx_handle: Option<u16>,
    psm_handle: Option<u16>,
    psm: Option<u16>,
}

fn cccd_handle(
    client: &mut GattClient,
    att: &mut ExternalAttTransport<'_>,
    characteristic: &bumble_gatt::CharacteristicProxy,
) -> Result<u16, String> {
    client
        .discover_descriptors(att, characteristic)
        .map_err(|error| error.to_string())?
        .into_iter()
        .find(|descriptor| {
            descriptor.uuid == Uuid::from_16_bits(GATT_CLIENT_CHARACTERISTIC_CONFIGURATION_UUID)
        })
        .map(|descriptor| descriptor.handle)
        .ok_or_else(|| {
            format!(
                "Gattlink characteristic {:?} has no CCCD",
                characteristic.uuid
            )
        })
}

fn discover_hub_gatt(
    host: &mut ExternalHost,
    device: &mut Device,
    connection_handle: u16,
) -> Result<HubGatt, String> {
    let mut att = ExternalAttTransport::new(host, device, connection_handle, PROCEDURE_TIMEOUT)
        .map_err(|error| error.to_string())?;
    let mut client = GattClient::new();
    let server_mtu = client
        .exchange_mtu(&mut att, GG_PREFERRED_MTU)
        .map_err(|error| error.to_string())?;
    println!("### Server MTU = {server_mtu}");
    let service_uuid = uuid(GG_GATTLINK_SERVICE_UUID);
    let service = client
        .discover_services(&mut att)
        .map_err(|error| error.to_string())?
        .into_iter()
        .find(|service| service.uuid == service_uuid)
        .ok_or_else(|| "Gattlink service not found".to_string())?;
    let characteristics = client
        .discover_characteristics(&mut att, &service)
        .map_err(|error| error.to_string())?;
    let rx_uuid = uuid(GG_GATTLINK_RX_CHARACTERISTIC_UUID);
    let tx_uuid = uuid(GG_GATTLINK_TX_CHARACTERISTIC_UUID);
    let psm_uuid = uuid(GG_GATTLINK_L2CAP_CHANNEL_PSM_CHARACTERISTIC_UUID);
    let rx = characteristics
        .iter()
        .find(|characteristic| characteristic.uuid == rx_uuid)
        .cloned();
    let tx = characteristics
        .iter()
        .find(|characteristic| characteristic.uuid == tx_uuid)
        .cloned();
    let psm = characteristics
        .iter()
        .find(|characteristic| characteristic.uuid == psm_uuid)
        .cloned();
    println!("RX: {rx:?}");
    println!("TX: {tx:?}");
    println!("PSM: {psm:?}");

    let mut psm_value = None;
    if let Some(characteristic) = &psm {
        let cccd = cccd_handle(&mut client, &mut att, characteristic)?;
        client
            .subscribe(&mut att, characteristic.handle, cccd, false)
            .map_err(|error| error.to_string())?;
        let value = client
            .read_value(&mut att, characteristic.handle, false)
            .map_err(|error| error.to_string())?;
        let bytes: [u8; 2] = value
            .get(..2)
            .and_then(|value| value.try_into().ok())
            .ok_or_else(|| "Gattlink PSM value is shorter than 2 bytes".to_string())?;
        psm_value = Some(u16::from_le_bytes(bytes));
    } else if let Some(characteristic) = &tx {
        let cccd = cccd_handle(&mut client, &mut att, characteristic)?;
        client
            .subscribe(&mut att, characteristic.handle, cccd, false)
            .map_err(|error| error.to_string())?;
        println!("=== Subscribed to Gattlink TX");
    } else {
        return Err("no Gattlink TX or PSM characteristic found".into());
    }
    Ok(HubGatt {
        client,
        max_packet_size: usize::from(server_mtu.saturating_sub(3)),
        rx_handle: rx.map(|characteristic| characteristic.handle),
        tx_handle: tx.map(|characteristic| characteristic.handle),
        psm_handle: psm.map(|characteristic| characteristic.handle),
        psm: psm_value,
    })
}

fn open_coc(
    host: &mut ExternalHost,
    device: &mut Device,
    connection_handle: u16,
    psm: u16,
) -> Result<CocBridge, String> {
    println!("### Connecting with L2CAP on PSM = {psm}");
    let source_cid = device
        .connect_le_credit_channel(host, connection_handle, psm, coc_spec(psm))
        .map_err(|error| error.to_string())?;
    wait_for_coc(host, device, connection_handle, source_cid)?;
    println!("*** Connected on LE credit CID {source_cid:#06x}");
    Ok(CocBridge::new(connection_handle, source_cid))
}

fn take_dynamic_packets(queue: &PacketQueue) -> Result<Vec<Vec<u8>>, String> {
    let mut queue = queue
        .lock()
        .map_err(|_| "Gattlink RX queue lock was poisoned".to_string())?;
    Ok(queue.drain(..).collect())
}

fn run_node(
    host: &mut ExternalHost,
    device: &mut Device,
    tx_handle: u16,
    rx_packets: &PacketQueue,
    udp: &mut UdpEndpoint,
) -> Result<(), String> {
    let psm = device
        .register_le_credit_server(coc_spec(GG_L2CAP_PSM))
        .map_err(|error| error.to_string())?;
    println!("### Listening for CoC connection on PSM {psm}");
    loop {
        println!("### Advertising Bumble GG");
        let connection_handle = advertise_and_wait(host, device)?;
        println!("=== Connected on handle {connection_handle:#06x}");
        let mut coc = None;
        loop {
            device.poll(host);
            udp.poll_receive().map_err(|error| error.to_string())?;
            for source_cid in device.take_accepted_le_credit_channels(connection_handle) {
                println!("*** CoC connection on CID {source_cid:#06x}");
                coc = Some(CocBridge::new(connection_handle, source_cid));
            }
            if coc.as_ref().is_some_and(|coc| !coc.is_connected(device)) {
                println!("*** CoC disconnected, using GATT fallback");
                coc = None;
            }
            if let Some(coc) = &mut coc {
                coc.pump(host, device, udp)?;
            } else if device.acl_output_is_drained(connection_handle) {
                if let Some(packet) = udp.pending.pop_front() {
                    if packet.len() > MAX_GATT_PACKET_SIZE {
                        eprintln!(
                            "!!! Dropping {}-byte UDP datagram: GATT fallback limit is {MAX_GATT_PACKET_SIZE}",
                            packet.len()
                        );
                    } else if !device.notify_on_handle(host, connection_handle, tx_handle, packet) {
                        return Err("failed to send Gattlink TX notification".into());
                    } else {
                        println!(">>> [GATT TX]");
                    }
                }
            }
            for packet in take_dynamic_packets(rx_packets)? {
                println!("<<< [GATT RX]: {} bytes", packet.len());
                udp.send_packet(&packet)
                    .map_err(|error| error.to_string())?;
            }
            for (handle, error) in device.take_le_credit_errors() {
                eprintln!("!!! L2CAP error on handle {handle:#06x}: {error}");
                coc = None;
            }
            if !device.is_connected_on_handle(connection_handle) {
                println!("!!! Disconnected");
                break;
            }
            if !wait_tick(host)? {
                return Ok(());
            }
        }
    }
}

fn run_hub(
    host: &mut ExternalHost,
    device: &mut Device,
    peer_address: &str,
    udp: &mut UdpEndpoint,
) -> Result<(), String> {
    let peer = Address::parse(peer_address, AddressType::RANDOM_DEVICE)
        .map_err(|error| error.to_string())?;
    println!("=== Connecting to {peer}...");
    let connection_handle = connect_bluetooth(host, device, peer)?;
    println!("=== Connected on handle {connection_handle:#06x}");
    let mut gatt = discover_hub_gatt(host, device, connection_handle)?;
    let mut coc = match gatt.psm {
        Some(psm) => Some(open_coc(host, device, connection_handle, psm)?),
        None => None,
    };
    loop {
        device.poll(host);
        udp.poll_receive().map_err(|error| error.to_string())?;
        if coc.as_ref().is_some_and(|coc| !coc.is_connected(device)) {
            println!("*** CoC disconnected, using GATT fallback");
            coc = None;
        }
        if let Some(coc) = &mut coc {
            coc.pump(host, device, udp)?;
        } else if device.acl_output_is_drained(connection_handle) {
            if let Some(packet) = udp.pending.pop_front() {
                let Some(rx_handle) = gatt.rx_handle else {
                    return Err("Gattlink CoC closed and no RX characteristic is available".into());
                };
                if packet.len() > gatt.max_packet_size {
                    eprintln!(
                        "!!! Dropping {}-byte UDP datagram: negotiated GATT fallback limit is {}",
                        packet.len(),
                        gatt.max_packet_size
                    );
                } else if !device.send_att_on_handle(
                    host,
                    connection_handle,
                    &AttPdu::WriteCommand {
                        attribute_handle: rx_handle,
                        attribute_value: packet,
                    },
                ) {
                    return Err("failed to write Gattlink RX characteristic".into());
                } else {
                    println!(">>> [GATT RX]");
                }
            }
        }

        for pdu in device.take_inbox_on_handle(connection_handle) {
            if let AttPdu::HandleValueNotification {
                attribute_handle,
                attribute_value,
            } = &pdu
            {
                gatt.client
                    .on_notification(&pdu)
                    .map_err(|error| error.to_string())?;
                if Some(*attribute_handle) == gatt.tx_handle {
                    println!("<<< [GATT TX]: {} bytes", attribute_value.len());
                    udp.send_packet(attribute_value)
                        .map_err(|error| error.to_string())?;
                } else if Some(*attribute_handle) == gatt.psm_handle {
                    if let Some(bytes) = attribute_value
                        .get(..2)
                        .and_then(|value| <[u8; 2]>::try_from(value).ok())
                    {
                        let psm = u16::from_le_bytes(bytes);
                        gatt.psm = Some(psm);
                        if coc.is_none() {
                            coc = Some(open_coc(host, device, connection_handle, psm)?);
                        }
                    }
                }
            }
        }
        for (handle, error) in device.take_le_credit_errors() {
            eprintln!("!!! L2CAP error on handle {handle:#06x}: {error}");
            coc = None;
        }
        if !device.is_connected_on_handle(connection_handle) {
            return Err("Bluetooth connection ended".into());
        }
        if !wait_tick(host)? {
            return Ok(());
        }
    }
}

fn run(args: Args) -> Result<(), String> {
    let local_address = Address::parse(&args.device_address, AddressType::RANDOM_DEVICE)
        .map_err(|error| error.to_string())?;
    let node = args.role_or_peer_address == "node";
    let node_gatt = node.then(build_node_gatt).transpose()?;
    println!("<<< connecting to HCI...");
    let transport = open_split_transport(&args.hci_transport).map_err(|error| error.to_string())?;
    let mut host = ExternalHost::new(transport);
    let mut device = match &node_gatt {
        Some(gatt) => Device::with_server(0, gatt.server.clone()),
        None => Device::new(0),
    };
    host.initialize_device(&mut device, COMMAND_TIMEOUT)
        .map_err(|error| error.to_string())?;
    command(
        &mut host,
        Command::LeSetRandomAddress {
            random_address: local_address,
        },
        "setting local random address",
    )?;
    println!("<<< connected");
    let mut udp = UdpEndpoint::open(&args)?;
    println!(
        "### UDP receive {} -> Bluetooth -> UDP send {}",
        udp.receiver
            .local_addr()
            .map_err(|error| error.to_string())?,
        udp.sender.peer_addr().map_err(|error| error.to_string())?
    );
    match node_gatt {
        Some(gatt) => run_node(
            &mut host,
            &mut device,
            gatt.tx_handle,
            &gatt.rx_packets,
            &mut udp,
        ),
        None => run_hub(&mut host, &mut device, &args.role_or_peer_address, &mut udp),
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

    fn address(value: &str) -> Address {
        Address::parse(value, AddressType::RANDOM_DEVICE).unwrap()
    }

    #[test]
    fn parses_upstream_cli_with_short_and_long_udp_options() {
        let args = parse_args(
            [
                "gg-bridge",
                "usb:0",
                "C4:F2:17:1A:1D:AA",
                "node",
                "-sh",
                "192.0.2.1",
                "--send-port=7001",
                "-rh=0.0.0.0",
                "--receive-port",
                "7000",
            ]
            .map(str::to_string),
        )
        .unwrap();
        assert_eq!(args.hci_transport, "usb:0");
        assert_eq!(args.device_address, "C4:F2:17:1A:1D:AA");
        assert_eq!(args.role_or_peer_address, "node");
        assert_eq!(args.send_host, "192.0.2.1");
        assert_eq!(args.send_port, 7001);
        assert_eq!(args.receive_host, "0.0.0.0");
        assert_eq!(args.receive_port, 7000);
        assert!(parse_args(["gg-bridge", "transport"].map(str::to_string)).is_err());
        assert!(parse_args(
            ["gg-bridge", "a", "b", "node", "--send-port", "70000"].map(str::to_string)
        )
        .is_err());
    }

    #[test]
    fn packet_framing_handles_fragmented_and_coalesced_sdus() {
        let first = frame_packet(b"one").unwrap();
        let second = frame_packet(b"second").unwrap();
        let mut stream = first;
        stream.extend(second);
        let mut decoder = GattlinkPacketDecoder::default();
        assert!(decoder.push_sdu(&stream[..2]).is_empty());
        assert_eq!(
            decoder.push_sdu(&stream[2..]),
            [b"one".to_vec(), b"second".to_vec()]
        );
        assert_eq!(frame_packet(&vec![0xAA; 256]).unwrap()[0], 255);
        assert!(frame_packet(&[]).is_err());
        assert!(frame_packet(&vec![0; 257]).is_err());
    }

    #[test]
    fn node_gatt_database_supports_discovery_writes_psm_and_subscription() {
        let node = build_node_gatt().unwrap();
        let NodeGatt {
            mut server,
            tx_handle,
            rx_packets,
        } = node;
        let mut client = GattClient::new();
        let service = client
            .discover_services(&mut server)
            .unwrap()
            .into_iter()
            .find(|service| service.uuid == uuid(GG_GATTLINK_SERVICE_UUID))
            .unwrap();
        let characteristics = client
            .discover_characteristics(&mut server, &service)
            .unwrap();
        let rx = characteristics
            .iter()
            .find(|characteristic| characteristic.uuid == uuid(GG_GATTLINK_RX_CHARACTERISTIC_UUID))
            .unwrap();
        let tx = characteristics
            .iter()
            .find(|characteristic| characteristic.uuid == uuid(GG_GATTLINK_TX_CHARACTERISTIC_UUID))
            .unwrap();
        let psm = characteristics
            .iter()
            .find(|characteristic| {
                characteristic.uuid == uuid(GG_GATTLINK_L2CAP_CHANNEL_PSM_CHARACTERISTIC_UUID)
            })
            .unwrap();
        assert_eq!(tx.handle, tx_handle);
        client
            .write_value(&mut server, rx.handle, b"GATT packet".to_vec(), false)
            .unwrap();
        assert_eq!(
            take_dynamic_packets(&rx_packets).unwrap(),
            [b"GATT packet".to_vec()]
        );
        assert_eq!(
            client.read_value(&mut server, psm.handle, false).unwrap(),
            GG_L2CAP_PSM.to_le_bytes()
        );
        let cccd = client
            .discover_descriptors(&mut server, tx)
            .unwrap()
            .into_iter()
            .find(|descriptor| {
                descriptor.uuid == Uuid::from_16_bits(GATT_CLIENT_CHARACTERISTIC_CONFIGURATION_UUID)
            })
            .unwrap();
        client
            .subscribe(&mut server, tx.handle, cccd.handle, false)
            .unwrap();
        assert_eq!(gg_advertising_data().len(), 29);
    }

    #[test]
    fn udp_endpoint_bridges_both_directions_over_live_gattlink_coc() {
        let mut link = ControllerLocalLink::new();
        let hub_id = link.add_controller(Controller::new("hub", address("00:00:00:00:00:01")));
        let node_id = link.add_controller(Controller::new("node", address("00:00:00:00:00:02")));
        let mut devices = [Device::new(hub_id), Device::new(node_id)];
        devices[1]
            .register_le_credit_server(coc_spec(GG_L2CAP_PSM))
            .unwrap();
        let hub_address = address("C4:F2:17:1A:1D:AA");
        let node_address = address("C4:F2:17:1A:1D:BB");
        devices[0].set_random_address(&mut link, hub_address);
        devices[1].set_random_address(&mut link, node_address.clone());
        assert!(devices[1].start_advertising(&mut link, &[]));
        devices[0].connect_le(&mut link, node_address);
        pump(&mut link, &mut devices);
        let hub_handle = devices[0].connection_handle().unwrap();
        let node_handle = devices[1].connection_handle().unwrap();
        let hub_cid = devices[0]
            .connect_le_credit_channel(&mut link, hub_handle, GG_L2CAP_PSM, coc_spec(GG_L2CAP_PSM))
            .unwrap();
        pump(&mut link, &mut devices);
        let node_cid = devices[1]
            .take_accepted_le_credit_channels(node_handle)
            .into_iter()
            .next()
            .unwrap();

        let udp_target = UdpSocket::bind("127.0.0.1:0").unwrap();
        udp_target
            .set_read_timeout(Some(Duration::from_secs(1)))
            .unwrap();
        let bridge_sender = UdpSocket::bind("127.0.0.1:0").unwrap();
        bridge_sender
            .connect(udp_target.local_addr().unwrap())
            .unwrap();
        let bridge_receiver = UdpSocket::bind("127.0.0.1:0").unwrap();
        let udp_peer = UdpSocket::bind("127.0.0.1:0").unwrap();
        udp_peer
            .connect(bridge_receiver.local_addr().unwrap())
            .unwrap();
        let mut udp = UdpEndpoint::from_sockets(bridge_receiver, bridge_sender).unwrap();
        let mut bridge = CocBridge::new(hub_handle, hub_cid);

        udp_peer.send(b"UDP to Bluetooth").unwrap();
        let deadline = Instant::now() + Duration::from_secs(1);
        let udp_to_bluetooth = loop {
            udp.poll_receive().unwrap();
            bridge.pump(&mut link, &mut devices[0], &mut udp).unwrap();
            pump(&mut link, &mut devices);
            let sdus = devices[1].take_le_credit_sdus(node_handle, node_cid);
            if !sdus.is_empty() {
                let mut decoder = GattlinkPacketDecoder::default();
                break sdus
                    .into_iter()
                    .flat_map(|sdu| decoder.push_sdu(&sdu))
                    .collect::<Vec<_>>();
            }
            assert!(Instant::now() < deadline, "UDP packet was not bridged");
            std::thread::yield_now();
        };
        assert_eq!(udp_to_bluetooth, [b"UDP to Bluetooth".to_vec()]);

        devices[1]
            .send_le_credit_sdu(
                &mut link,
                node_handle,
                node_cid,
                &frame_packet(b"Bluetooth to UDP").unwrap(),
            )
            .unwrap();
        pump(&mut link, &mut devices);
        bridge.pump(&mut link, &mut devices[0], &mut udp).unwrap();
        let mut received = [0; 16];
        let (size, _) = udp_target.recv_from(&mut received).unwrap();
        assert_eq!(&received[..size], b"Bluetooth to UDP");
        assert!(devices[0].take_le_credit_errors().is_empty());
        assert!(devices[1].take_le_credit_errors().is_empty());
    }
}
