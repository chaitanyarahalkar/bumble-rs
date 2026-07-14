//! Bluetooth transport benchmark compatible with upstream `apps/bench.py`.

use bumble::{Address, AddressType, Uuid};
use bumble_att::AttPdu;
use bumble_gatt::{
    permissions, properties, CharacteristicDefinition, DescriptorDefinition, DynamicValue,
    GattClient, GattServer, ServiceDefinition, GATT_CLIENT_CHARACTERISTIC_CONFIGURATION_UUID,
};
use bumble_hci::Command as HciCommand;
use bumble_host::Device;
use bumble_l2cap::{ClassicChannelSpec, ClassicChannelState, LeCreditBasedChannelSpec};
use bumble_rfcomm::mux::{DlcState, Multiplexer, MultiplexerState, Role as RfcommRole};
use bumble_rfcomm::{
    RfcommFrame, RFCOMM_DEFAULT_INITIAL_CREDITS, RFCOMM_DEFAULT_MAX_CREDITS,
    RFCOMM_DEFAULT_MAX_FRAME_SIZE, RFCOMM_DYNAMIC_CHANNEL_NUMBER_START, RFCOMM_PSM,
};
use bumble_sdp::service::{
    AttributeId, SdpClient, SdpRequestHandler, SdpServer, SdpTransport, TransportError,
};
use bumble_sdp::{public_browse_root, DataElement, SdpPdu, ServiceAttribute, SDP_PSM};
use bumble_smp::PairingConfig;
use bumble_transport::{
    open_split_transport, ClassicPairingSession, CommandResponse, ExternalAttTransport,
    ExternalHost, ExternalHostActivity,
};
use std::collections::{BTreeMap, VecDeque};
use std::process::ExitCode;
use std::sync::atomic::{AtomicU16, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const DEFAULT_CENTRAL_ADDRESS: &str = "F0:F0:F0:F0:F0:F0";
const DEFAULT_PERIPHERAL_ADDRESS: &str = "F1:F1:F1:F1:F1:F1";
const DEFAULT_RFCOMM_UUID: &str = "E6D55659-C8B4-4B85-96BB-B1143AF6D3AE";
const DEFAULT_L2CAP_PSM: u16 = 128;
const DEFAULT_L2CAP_MAX_CREDITS: u16 = 128;
const DEFAULT_L2CAP_MTU: u16 = 1024;
const DEFAULT_L2CAP_MPS: u16 = 1024;
const DEFAULT_RFCOMM_CHANNEL: u8 = 8;
const DEFAULT_RFCOMM_MTU: u16 = 2048;
const DEFAULT_ISO_MAX_SDU_C_TO_P: u16 = 251;
const DEFAULT_ISO_MAX_SDU_P_TO_C: u16 = 251;
const DEFAULT_ISO_SDU_INTERVAL_C_TO_P: u32 = 10_000;
const DEFAULT_ISO_SDU_INTERVAL_P_TO_C: u32 = 10_000;
const DEFAULT_ISO_MAX_TRANSPORT_LATENCY_C_TO_P: u16 = 35;
const DEFAULT_ISO_MAX_TRANSPORT_LATENCY_P_TO_C: u16 = 35;
const DEFAULT_ISO_RTN_C_TO_P: u8 = 3;
const DEFAULT_ISO_RTN_P_TO_C: u8 = 3;
const SPEED_SERVICE_UUID: &str = "50DB505C-8AC4-4738-8448-3B1D9CC09CC5";
const SPEED_TX_UUID: &str = "E789C754-41A1-45F4-A948-A0A1A90DBA53";
const SPEED_RX_UUID: &str = "016A2CC7-E14B-4819-935F-1F56EAE4098D";
const COMMAND_TIMEOUT: Duration = Duration::from_secs(5);
const PROCEDURE_TIMEOUT: Duration = Duration::from_secs(30);
const POLL_INTERVAL: Duration = Duration::from_millis(2);
const ATT_INVALID_ATTRIBUTE_VALUE_LENGTH_ERROR: u8 = 0x0D;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AppRole {
    Central,
    Peripheral,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Scenario {
    Send,
    Receive,
    Ping,
    Pong,
}

impl Scenario {
    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "send" => Ok(Self::Send),
            "receive" => Ok(Self::Receive),
            "ping" => Ok(Self::Ping),
            "pong" => Ok(Self::Pong),
            _ => Err(format!("invalid scenario {value:?}")),
        }
    }

    const fn is_sender(self) -> bool {
        matches!(self, Self::Send | Self::Ping)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    GattClient,
    GattServer,
    L2capClient,
    L2capServer,
    RfcommClient,
    RfcommServer,
    IsoClient,
    IsoServer,
}

impl Mode {
    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "gatt-client" => Ok(Self::GattClient),
            "gatt-server" => Ok(Self::GattServer),
            "l2cap-client" => Ok(Self::L2capClient),
            "l2cap-server" => Ok(Self::L2capServer),
            "rfcomm-client" => Ok(Self::RfcommClient),
            "rfcomm-server" => Ok(Self::RfcommServer),
            "iso-client" => Ok(Self::IsoClient),
            "iso-server" => Ok(Self::IsoServer),
            _ => Err(format!("invalid mode {value:?}")),
        }
    }

    const fn is_classic(self) -> bool {
        matches!(self, Self::RfcommClient | Self::RfcommServer)
    }

    const fn is_iso(self) -> bool {
        matches!(self, Self::IsoClient | Self::IsoServer)
    }

    const fn overhead(self) -> usize {
        if matches!(self, Self::GattClient | Self::GattServer) {
            0
        } else {
            2
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phy {
    OneM,
    TwoM,
    Coded,
}

impl Phy {
    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "1m" => Ok(Self::OneM),
            "2m" => Ok(Self::TwoM),
            "coded" => Ok(Self::Coded),
            _ => Err(format!("invalid PHY {value:?}")),
        }
    }

    const fn hci(self) -> u8 {
        match self {
            Self::OneM => 1,
            Self::TwoM => 2,
            Self::Coded => 3,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
struct Args {
    role: AppRole,
    transport: String,
    peer: String,
    scenario: Scenario,
    mode: Mode,
    device_config: Option<String>,
    att_mtu: u16,
    extended_data_length: Option<(u16, u16)>,
    role_switch: Option<AppRole>,
    le_scan: Option<(f64, f64)>,
    le_advertise: Option<f64>,
    classic_page_scan: bool,
    classic_inquiry_scan: bool,
    rfcomm_channel: u8,
    rfcomm_uuid: Uuid,
    rfcomm_l2cap_mtu: Option<u16>,
    rfcomm_max_frame_size: Option<u16>,
    rfcomm_initial_credits: Option<u8>,
    rfcomm_max_credits: Option<u8>,
    rfcomm_credits_threshold: Option<u8>,
    l2cap_psm: u16,
    l2cap_mtu: u16,
    l2cap_mps: u16,
    l2cap_max_credits: u16,
    packet_size: usize,
    packet_count: u32,
    start_delay: Duration,
    repeat: u32,
    repeat_delay: Duration,
    pace: Duration,
    linger: bool,
    connection_interval: Option<u16>,
    phy: Option<Phy>,
    authenticate: bool,
    encrypt: bool,
    iso_sdu_interval_c_to_p: Option<u32>,
    iso_sdu_interval_p_to_c: Option<u32>,
    iso_max_sdu_c_to_p: Option<u16>,
    iso_max_sdu_p_to_c: Option<u16>,
    iso_max_transport_latency_c_to_p: Option<u16>,
    iso_max_transport_latency_p_to_c: Option<u16>,
    iso_rtn_c_to_p: Option<u8>,
    iso_rtn_p_to_c: Option<u8>,
}

fn usage() -> &'static str {
    "usage: bumble-bench [OPTIONS] <central TRANSPORT [CENTRAL-OPTIONS] | peripheral TRANSPORT>\n\nscenarios: send, receive, ping, pong\nmodes: gatt-client, gatt-server, l2cap-client, l2cap-server, rfcomm-client, rfcomm-server, iso-client, iso-server"
}

fn value(arguments: &[String], index: &mut usize, option: &str) -> Result<String, String> {
    *index += 1;
    arguments
        .get(*index)
        .cloned()
        .ok_or_else(|| format!("{option} requires a value"))
}

fn number<T: std::str::FromStr>(value: &str, option: &str) -> Result<T, String> {
    value
        .parse()
        .map_err(|_| format!("invalid value {value:?} for {option}"))
}

fn pair<T: std::str::FromStr>(value: &str, option: &str) -> Result<(T, T), String> {
    let (first, second) = value
        .split_once('/')
        .ok_or_else(|| format!("{option} must be FIRST/SECOND"))?;
    Ok((number(first, option)?, number(second, option)?))
}

fn parse_args(
    arguments: impl IntoIterator<Item = impl Into<String>>,
) -> Result<Option<Args>, String> {
    let mut values = arguments.into_iter().map(Into::into).collect::<Vec<_>>();
    if values.is_empty() {
        return Err("missing executable name".into());
    }
    values.remove(0);
    if values.is_empty()
        || values
            .iter()
            .any(|value| matches!(value.as_str(), "-h" | "--help"))
    {
        return Ok(None);
    }

    let mut role = None;
    let mut transport = None;
    let mut peer = DEFAULT_PERIPHERAL_ADDRESS.to_string();
    let mut scenario = None;
    let mut mode = None;
    let mut device_config = None;
    let mut att_mtu = 517u16;
    let mut extended_data_length = None;
    let mut role_switch = None;
    let mut le_scan: Option<(f64, f64)> = None;
    let mut le_advertise: Option<f64> = None;
    let mut classic_page_scan = false;
    let mut classic_inquiry_scan = false;
    let mut rfcomm_channel = DEFAULT_RFCOMM_CHANNEL;
    let mut rfcomm_uuid = Uuid::parse(DEFAULT_RFCOMM_UUID).map_err(|error| error.to_string())?;
    let mut rfcomm_l2cap_mtu = None;
    let mut rfcomm_max_frame_size = None;
    let mut rfcomm_initial_credits = None;
    let mut rfcomm_max_credits = None;
    let mut rfcomm_credits_threshold = None;
    let mut l2cap_psm = DEFAULT_L2CAP_PSM;
    let mut l2cap_mtu = DEFAULT_L2CAP_MTU;
    let mut l2cap_mps = DEFAULT_L2CAP_MPS;
    let mut l2cap_max_credits = DEFAULT_L2CAP_MAX_CREDITS;
    let mut packet_size = 500usize;
    let mut packet_count = 10u32;
    let mut start_delay = Duration::from_secs(1);
    let mut repeat = 0u32;
    let mut repeat_delay = Duration::from_secs(1);
    let mut pace = Duration::ZERO;
    let mut linger = false;
    let mut connection_interval = None;
    let mut phy = None;
    let mut authenticate = false;
    let mut encrypt = false;
    let mut iso_sdu_interval_c_to_p = None;
    let mut iso_sdu_interval_p_to_c = None;
    let mut iso_max_sdu_c_to_p = None;
    let mut iso_max_sdu_p_to_c = None;
    let mut iso_max_transport_latency_c_to_p = None;
    let mut iso_max_transport_latency_p_to_c = None;
    let mut iso_rtn_c_to_p = None;
    let mut iso_rtn_p_to_c = None;
    let mut index = 0;
    while index < values.len() {
        let argument = values[index].as_str();
        match argument {
            "central" | "peripheral" => {
                if role.is_some() {
                    return Err("central/peripheral specified more than once".into());
                }
                role = Some(if argument == "central" {
                    AppRole::Central
                } else {
                    AppRole::Peripheral
                });
                transport = Some(value(&values, &mut index, argument)?);
            }
            "--device-config" => device_config = Some(value(&values, &mut index, argument)?),
            "--scenario" => {
                scenario = Some(Scenario::parse(&value(&values, &mut index, argument)?)?)
            }
            "--mode" => mode = Some(Mode::parse(&value(&values, &mut index, argument)?)?),
            "--att-mtu" => att_mtu = number(&value(&values, &mut index, argument)?, argument)?,
            "--extended-data-length" => {
                extended_data_length = Some(pair(&value(&values, &mut index, argument)?, argument)?)
            }
            "--role-switch" => {
                role_switch = Some(match value(&values, &mut index, argument)?.as_str() {
                    "central" => AppRole::Central,
                    "peripheral" => AppRole::Peripheral,
                    value => return Err(format!("invalid role {value:?}")),
                })
            }
            "--le-scan" => le_scan = Some(pair(&value(&values, &mut index, argument)?, argument)?),
            "--le-advertise" => {
                le_advertise = Some(number(&value(&values, &mut index, argument)?, argument)?)
            }
            "--classic-page-scan" => classic_page_scan = true,
            "--classic-inquiry-scan" => classic_inquiry_scan = true,
            "--rfcomm-channel" => {
                rfcomm_channel = number(&value(&values, &mut index, argument)?, argument)?
            }
            "--rfcomm-uuid" => {
                rfcomm_uuid = Uuid::parse(&value(&values, &mut index, argument)?)
                    .map_err(|error| error.to_string())?
            }
            "--rfcomm-l2cap-mtu" => {
                rfcomm_l2cap_mtu = Some(number(&value(&values, &mut index, argument)?, argument)?)
            }
            "--rfcomm-max-frame-size" => {
                rfcomm_max_frame_size =
                    Some(number(&value(&values, &mut index, argument)?, argument)?)
            }
            "--rfcomm-initial-credits" => {
                rfcomm_initial_credits =
                    Some(number(&value(&values, &mut index, argument)?, argument)?)
            }
            "--rfcomm-max-credits" => {
                rfcomm_max_credits = Some(number(&value(&values, &mut index, argument)?, argument)?)
            }
            "--rfcomm-credits-threshold" => {
                rfcomm_credits_threshold =
                    Some(number(&value(&values, &mut index, argument)?, argument)?)
            }
            "--l2cap-psm" => l2cap_psm = number(&value(&values, &mut index, argument)?, argument)?,
            "--l2cap-mtu" => l2cap_mtu = number(&value(&values, &mut index, argument)?, argument)?,
            "--l2cap-mps" => l2cap_mps = number(&value(&values, &mut index, argument)?, argument)?,
            "--l2cap-max-credits" => {
                l2cap_max_credits = number(&value(&values, &mut index, argument)?, argument)?
            }
            "--packet-size" | "-s" => {
                packet_size = number(&value(&values, &mut index, argument)?, argument)?
            }
            "--packet-count" | "-c" => {
                packet_count = number(&value(&values, &mut index, argument)?, argument)?
            }
            "--start-delay" | "-sd" => {
                start_delay =
                    Duration::from_secs(number(&value(&values, &mut index, argument)?, argument)?)
            }
            "--repeat" => repeat = number(&value(&values, &mut index, argument)?, argument)?,
            "--repeat-delay" => {
                repeat_delay =
                    Duration::from_secs(number(&value(&values, &mut index, argument)?, argument)?)
            }
            "--pace" => {
                pace =
                    Duration::from_millis(number(&value(&values, &mut index, argument)?, argument)?)
            }
            "--linger" => linger = true,
            "--peripheral" => peer = value(&values, &mut index, argument)?,
            "--connection-interval" | "--ci" => {
                connection_interval =
                    Some(number(&value(&values, &mut index, argument)?, argument)?)
            }
            "--phy" => phy = Some(Phy::parse(&value(&values, &mut index, argument)?)?),
            "--authenticate" => authenticate = true,
            "--encrypt" => encrypt = true,
            "--iso-sdu-interval-c-to-p" => {
                iso_sdu_interval_c_to_p =
                    Some(number(&value(&values, &mut index, argument)?, argument)?)
            }
            "--iso-sdu-interval-p-to-c" => {
                iso_sdu_interval_p_to_c =
                    Some(number(&value(&values, &mut index, argument)?, argument)?)
            }
            "--iso-max-sdu-c-to-p" => {
                iso_max_sdu_c_to_p = Some(number(&value(&values, &mut index, argument)?, argument)?)
            }
            "--iso-max-sdu-p-to-c" => {
                iso_max_sdu_p_to_c = Some(number(&value(&values, &mut index, argument)?, argument)?)
            }
            "--iso-max-transport-latency-c-to-p" => {
                iso_max_transport_latency_c_to_p =
                    Some(number(&value(&values, &mut index, argument)?, argument)?)
            }
            "--iso-max-transport-latency-p-to-c" => {
                iso_max_transport_latency_p_to_c =
                    Some(number(&value(&values, &mut index, argument)?, argument)?)
            }
            "--iso-rtn-c-to-p" => {
                iso_rtn_c_to_p = Some(number(&value(&values, &mut index, argument)?, argument)?)
            }
            "--iso-rtn-p-to-c" => {
                iso_rtn_p_to_c = Some(number(&value(&values, &mut index, argument)?, argument)?)
            }
            option if option.starts_with('-') => return Err(format!("unknown option {option}")),
            value => return Err(format!("unexpected positional argument {value:?}")),
        }
        index += 1;
    }
    let role = role.ok_or_else(|| "missing central or peripheral command".to_string())?;
    let mode = mode.unwrap_or(if role == AppRole::Central {
        Mode::GattClient
    } else {
        Mode::GattServer
    });
    let scenario = scenario.unwrap_or(if role == AppRole::Central {
        Scenario::Send
    } else {
        Scenario::Receive
    });
    if mode.is_iso() && matches!(scenario, Scenario::Ping | Scenario::Pong) {
        return Err("ping and pong are not supported with ISO mode".into());
    }
    if !(23..=517).contains(&att_mtu) {
        return Err("--att-mtu must be in 23..=517".into());
    }
    if !(10..=8192).contains(&packet_size) || packet_size < 10 + mode.overhead() {
        return Err(format!(
            "packet size is too small or exceeds 8192 for {mode:?}"
        ));
    }
    if packet_count == 0 {
        return Err("packet count must be nonzero".into());
    }
    if rfcomm_channel > 30 {
        return Err("RFCOMM channel must be in 0..=30".into());
    }
    if let Some((window, interval)) = le_scan {
        if !window.is_finite() || !interval.is_finite() || window <= 0.0 || interval < window {
            return Err(
                "LE scan requires finite positive WINDOW/INTERVAL with WINDOW <= INTERVAL".into(),
            );
        }
    }
    if le_advertise.is_some_and(|interval| !interval.is_finite() || interval <= 0.0) {
        return Err("LE advertising interval must be finite and positive".into());
    }
    let receive_credit_max = rfcomm_max_credits.unwrap_or(RFCOMM_DEFAULT_MAX_CREDITS);
    if receive_credit_max == 0
        || rfcomm_credits_threshold.is_some_and(|threshold| threshold > receive_credit_max)
    {
        return Err("RFCOMM receive credits require 0 <= threshold <= max and max > 0".into());
    }
    if l2cap_psm == 0 || l2cap_mtu == 0 || l2cap_mps == 0 || l2cap_max_credits == 0 {
        return Err("L2CAP PSM, MTU, MPS, and credits must be nonzero".into());
    }
    Ok(Some(Args {
        role,
        transport: transport.expect("role parser records transport"),
        peer,
        scenario,
        mode,
        device_config,
        att_mtu,
        extended_data_length,
        role_switch,
        le_scan,
        le_advertise,
        classic_page_scan,
        classic_inquiry_scan,
        rfcomm_channel,
        rfcomm_uuid,
        rfcomm_l2cap_mtu,
        rfcomm_max_frame_size,
        rfcomm_initial_credits,
        rfcomm_max_credits,
        rfcomm_credits_threshold,
        l2cap_psm,
        l2cap_mtu,
        l2cap_mps,
        l2cap_max_credits,
        packet_size,
        packet_count,
        start_delay,
        repeat,
        repeat_delay,
        pace,
        linger,
        connection_interval,
        phy,
        authenticate,
        encrypt: encrypt || authenticate,
        iso_sdu_interval_c_to_p,
        iso_sdu_interval_p_to_c,
        iso_max_sdu_c_to_p,
        iso_max_sdu_p_to_c,
        iso_max_transport_latency_c_to_p,
        iso_max_transport_latency_p_to_c,
        iso_rtn_c_to_p,
        iso_rtn_p_to_c,
    }))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PacketType {
    Reset = 0,
    Sequence = 1,
    Ack = 2,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Packet {
    packet_type: PacketType,
    last: bool,
    sequence: u32,
    timestamp_us: u32,
    payload: Vec<u8>,
}

impl Packet {
    fn reset() -> Self {
        Self {
            packet_type: PacketType::Reset,
            last: false,
            sequence: 0,
            timestamp_us: 0,
            payload: Vec::new(),
        }
    }

    fn ack(sequence: u32, last: bool) -> Self {
        Self {
            packet_type: PacketType::Ack,
            last,
            sequence,
            timestamp_us: 0,
            payload: Vec::new(),
        }
    }

    fn sequence(sequence: u32, last: bool, timestamp_us: u32, size: usize) -> Result<Self, String> {
        if size < 10 {
            return Err("sequence packet size must be at least 10".into());
        }
        Ok(Self {
            packet_type: PacketType::Sequence,
            last,
            sequence,
            timestamp_us,
            payload: vec![0; size - 10],
        })
    }

    fn from_bytes(data: &[u8]) -> Result<Self, String> {
        let packet_type = match data.first().copied() {
            Some(0) => PacketType::Reset,
            Some(1) => PacketType::Sequence,
            Some(2) => PacketType::Ack,
            Some(value) => return Err(format!("invalid benchmark packet type 0x{value:02X}")),
            None => return Err("benchmark packet is empty".into()),
        };
        if packet_type == PacketType::Reset {
            if data.len() != 1 {
                return Err("RESET packet has trailing bytes".into());
            }
            return Ok(Self::reset());
        }
        if data.len() < 6 {
            return Err("benchmark ACK/SEQUENCE packet is shorter than six bytes".into());
        }
        let last = data[1] & 1 != 0;
        let sequence = u32::from_le_bytes(data[2..6].try_into().expect("four-byte slice"));
        if packet_type == PacketType::Ack {
            if data.len() != 6 {
                return Err("ACK packet must contain exactly six bytes".into());
            }
            return Ok(Self::ack(sequence, last));
        }
        if data.len() < 10 {
            return Err("SEQUENCE packet is shorter than ten bytes".into());
        }
        Ok(Self {
            packet_type,
            last,
            sequence,
            timestamp_us: u32::from_le_bytes(data[6..10].try_into().expect("four-byte slice")),
            payload: data[10..].to_vec(),
        })
    }

    fn to_bytes(&self) -> Vec<u8> {
        if self.packet_type == PacketType::Reset {
            return vec![0];
        }
        let mut data = vec![self.packet_type as u8, u8::from(self.last)];
        data.extend_from_slice(&self.sequence.to_le_bytes());
        if self.packet_type == PacketType::Sequence {
            data.extend_from_slice(&self.timestamp_us.to_le_bytes());
            data.extend_from_slice(&self.payload);
        }
        data
    }
}

#[derive(Debug, Default)]
struct StreamFramer {
    buffer: Vec<u8>,
}

impl StreamFramer {
    fn frame(packet: &[u8]) -> Result<Vec<u8>, String> {
        let length =
            u16::try_from(packet.len()).map_err(|_| "stream packet exceeds 65535 bytes")?;
        let mut framed = length.to_be_bytes().to_vec();
        framed.extend_from_slice(packet);
        Ok(framed)
    }

    fn push(&mut self, data: &[u8]) -> Result<Vec<Vec<u8>>, String> {
        self.buffer.extend_from_slice(data);
        let mut packets = Vec::new();
        loop {
            if self.buffer.len() < 2 {
                break;
            }
            let length = usize::from(u16::from_be_bytes([self.buffer[0], self.buffer[1]]));
            if length == 0 {
                return Err("zero-length benchmark stream frame".into());
            }
            if self.buffer.len() < length + 2 {
                break;
            }
            packets.push(self.buffer[2..length + 2].to_vec());
            self.buffer.drain(..length + 2);
        }
        Ok(packets)
    }
}

fn stats(values: &[f64]) -> Option<(f64, f64, f64, f64)> {
    if values.is_empty() {
        return None;
    }
    let min = values.iter().copied().fold(f64::INFINITY, f64::min);
    let max = values.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let mean = values.iter().sum::<f64>() / values.len() as f64;
    let deviation = if values.len() < 2 {
        0.0
    } else {
        (values
            .iter()
            .map(|value| (value - mean).powi(2))
            .sum::<f64>()
            / (values.len() - 1) as f64)
            .sqrt()
    };
    Some((min, max, mean, deviation))
}

fn require_success(response: CommandResponse, context: &str) -> Result<(), String> {
    if response.status() == Some(0) {
        Ok(())
    } else {
        Err(format!(
            "{context} failed with HCI status {:?}",
            response.status()
        ))
    }
}

fn command(host: &mut ExternalHost, command: HciCommand, context: &str) -> Result<(), String> {
    require_success(
        host.send_command(command, COMMAND_TIMEOUT)
            .map_err(|error| error.to_string())?,
        context,
    )
}

fn configured_address(path: Option<&str>, fallback: &str) -> Result<Address, String> {
    let value = if let Some(path) = path {
        let bytes = std::fs::read(path)
            .map_err(|error| format!("failed to read device config {path:?}: {error}"))?;
        let json: serde_json::Value = serde_json::from_slice(&bytes)
            .map_err(|error| format!("invalid device config JSON: {error}"))?;
        json.get("address")
            .and_then(serde_json::Value::as_str)
            .unwrap_or(fallback)
            .to_string()
    } else {
        fallback.to_string()
    };
    Address::parse(&value, AddressType::RANDOM_DEVICE).map_err(|error| error.to_string())
}

struct PreparedDevice {
    device: Device,
    gatt_queue: Option<Arc<Mutex<VecDeque<Vec<u8>>>>>,
    gatt_rx_handle: Option<u16>,
    gatt_cccd: Option<Arc<AtomicU16>>,
}

fn prepare_device(args: &Args) -> Result<PreparedDevice, String> {
    if args.mode == Mode::GattServer {
        let service_uuid = Uuid::parse(SPEED_SERVICE_UUID).map_err(|error| error.to_string())?;
        let tx_uuid = Uuid::parse(SPEED_TX_UUID).map_err(|error| error.to_string())?;
        let rx_uuid = Uuid::parse(SPEED_RX_UUID).map_err(|error| error.to_string())?;
        let mut server = GattServer::from_definitions(vec![ServiceDefinition {
            uuid: service_uuid,
            primary: true,
            included_services: Vec::new(),
            characteristics: vec![
                CharacteristicDefinition {
                    uuid: tx_uuid.clone(),
                    properties: properties::WRITE,
                    permissions: permissions::WRITEABLE,
                    value: Vec::new(),
                    descriptors: Vec::new(),
                },
                CharacteristicDefinition {
                    uuid: rx_uuid.clone(),
                    properties: properties::NOTIFY,
                    permissions: 0,
                    value: Vec::new(),
                    descriptors: vec![DescriptorDefinition {
                        uuid: Uuid::from_16_bits(GATT_CLIENT_CHARACTERISTIC_CONFIGURATION_UUID),
                        permissions: permissions::READABLE | permissions::WRITEABLE,
                        value: vec![0, 0],
                    }],
                },
            ],
        }])
        .map_err(|error| error.to_string())?;
        let tx_handle = *server
            .handles_by_uuid(&tx_uuid)
            .first()
            .ok_or_else(|| "Speed TX handle missing".to_string())?;
        let rx_handle = *server
            .handles_by_uuid(&rx_uuid)
            .first()
            .ok_or_else(|| "Speed RX handle missing".to_string())?;
        let cccd_handle = *server
            .handles_by_uuid(&Uuid::from_16_bits(
                GATT_CLIENT_CHARACTERISTIC_CONFIGURATION_UUID,
            ))
            .first()
            .ok_or_else(|| "Speed RX CCCD handle missing".to_string())?;
        let queue = Arc::new(Mutex::new(VecDeque::new()));
        let writes = Arc::clone(&queue);
        server
            .set_dynamic_value(
                tx_handle,
                DynamicValue::write_only(move |_, value| {
                    writes
                        .lock()
                        .expect("GATT benchmark queue")
                        .push_back(value.to_vec());
                    Ok(())
                }),
            )
            .map_err(|error| error.to_string())?;
        let cccd = Arc::new(AtomicU16::new(0));
        let cccd_reader = Arc::clone(&cccd);
        let cccd_writer = Arc::clone(&cccd);
        server
            .set_dynamic_value(
                cccd_handle,
                DynamicValue::read_write(
                    move |_| Ok(cccd_reader.load(Ordering::SeqCst).to_le_bytes().to_vec()),
                    move |_, value| {
                        let value: [u8; 2] = value
                            .try_into()
                            .map_err(|_| ATT_INVALID_ATTRIBUTE_VALUE_LENGTH_ERROR)?;
                        cccd_writer.store(u16::from_le_bytes(value), Ordering::SeqCst);
                        Ok(())
                    },
                ),
            )
            .map_err(|error| error.to_string())?;
        return Ok(PreparedDevice {
            device: Device::with_server(0, server),
            gatt_queue: Some(queue),
            gatt_rx_handle: Some(rx_handle),
            gatt_cccd: Some(cccd),
        });
    }
    Ok(PreparedDevice {
        device: Device::new(0),
        gatt_queue: None,
        gatt_rx_handle: None,
        gatt_cccd: None,
    })
}

fn open_device(args: &Args) -> Result<(ExternalHost, PreparedDevice, Address), String> {
    let fallback = if args.role == AppRole::Central {
        DEFAULT_CENTRAL_ADDRESS
    } else {
        DEFAULT_PERIPHERAL_ADDRESS
    };
    let local_address = configured_address(args.device_config.as_deref(), fallback)?;
    let transport = open_split_transport(&args.transport).map_err(|error| error.to_string())?;
    let mut host = ExternalHost::new(transport);
    let mut prepared = prepare_device(args)?;
    host.initialize_device(&mut prepared.device, COMMAND_TIMEOUT)
        .map_err(|error| error.to_string())?;
    command(
        &mut host,
        HciCommand::LeSetRandomAddress {
            random_address: local_address.clone(),
        },
        "setting random address",
    )?;
    Ok((host, prepared, local_address))
}

fn wait_tick(host: &mut ExternalHost, timeout: Duration) -> Result<bool, String> {
    match host
        .wait_for_activity(timeout)
        .map_err(|error| error.to_string())?
    {
        ExternalHostActivity::Packet | ExternalHostActivity::Timeout => Ok(true),
        ExternalHostActivity::Ended => Ok(false),
    }
}

fn wait_for_le_connection(
    host: &mut ExternalHost,
    device: &mut Device,
    peer: Option<&Address>,
) -> Result<u16, String> {
    let deadline = Instant::now() + PROCEDURE_TIMEOUT;
    loop {
        device.poll(host);
        let handle = peer
            .and_then(|peer| device.connection_handle_for_peer(peer))
            .or_else(|| peer.is_none().then(|| device.connection_handle()).flatten());
        if let Some(handle) = handle {
            return Ok(handle);
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err("timed out waiting for LE connection".into());
        }
        if !wait_tick(host, remaining)? {
            return Err("transport ended while waiting for LE connection".into());
        }
    }
}

fn connect_le(host: &mut ExternalHost, device: &mut Device, args: &Args) -> Result<u16, String> {
    let peer = Address::parse(&args.peer, AddressType::RANDOM_DEVICE)
        .map_err(|error| error.to_string())?;
    let interval = args.connection_interval.unwrap_or(30);
    let interval_units = (u32::from(interval) * 4 / 5).clamp(6, 3200) as u16;
    command(
        host,
        HciCommand::LeCreateConnection {
            le_scan_interval: 0x0010,
            le_scan_window: 0x0010,
            initiator_filter_policy: 0,
            peer_address_type: u8::from(!peer.is_public()),
            peer_address: peer.clone(),
            own_address_type: 1,
            connection_interval_min: interval_units,
            connection_interval_max: interval_units,
            max_latency: 0,
            supervision_timeout: 42,
            min_ce_length: 0,
            max_ce_length: 0,
        },
        "creating LE connection",
    )?;
    wait_for_le_connection(host, device, Some(&peer))
}

fn start_legacy_advertising(host: &mut ExternalHost, interval_ms: f64) -> Result<(), String> {
    if !interval_ms.is_finite() || interval_ms <= 0.0 {
        return Err("LE advertising interval must be positive".into());
    }
    let interval = (interval_ms / 0.625)
        .round()
        .clamp(0x20 as f64, 0x4000 as f64) as u16;
    command(
        host,
        HciCommand::LeSetAdvertisingParameters {
            advertising_interval_min: interval,
            advertising_interval_max: interval,
            advertising_type: 0,
            own_address_type: 1,
            peer_address_type: 0,
            peer_address: Address::from_bytes([0; 6], AddressType::PUBLIC_DEVICE),
            advertising_channel_map: 7,
            advertising_filter_policy: 0,
        },
        "setting LE advertising parameters",
    )?;
    command(
        host,
        HciCommand::LeSetAdvertisingData {
            advertising_data: vec![2, 0x01, 0x06],
        },
        "setting LE advertising data",
    )?;
    command(
        host,
        HciCommand::LeSetAdvertisingEnable {
            advertising_enable: 1,
        },
        "enabling LE advertising",
    )
}

fn advertise_le(host: &mut ExternalHost, device: &mut Device, args: &Args) -> Result<u16, String> {
    start_legacy_advertising(host, args.le_advertise.unwrap_or(100.0))?;
    wait_for_le_connection(host, device, None)
}

fn wait_for_classic_connection(
    host: &mut ExternalHost,
    device: &mut Device,
    peer: Option<&Address>,
) -> Result<u16, String> {
    let deadline = Instant::now() + PROCEDURE_TIMEOUT;
    loop {
        device.poll(host);
        let handle = peer
            .and_then(|peer| device.classic_connection_handle_for_peer(peer))
            .or_else(|| {
                peer.is_none()
                    .then(|| device.classic_connection_handle())
                    .flatten()
            });
        if let Some(handle) = handle {
            return Ok(handle);
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err("timed out waiting for Classic connection".into());
        }
        if !wait_tick(host, remaining)? {
            return Err("transport ended while waiting for Classic connection".into());
        }
    }
}

fn connect_classic(
    host: &mut ExternalHost,
    device: &mut Device,
    args: &Args,
) -> Result<u16, String> {
    let peer = Address::parse(&args.peer, AddressType::PUBLIC_DEVICE)
        .map_err(|error| error.to_string())?;
    device.connect_classic(host, peer.clone());
    wait_for_classic_connection(host, device, Some(&peer))
}

fn accept_classic(host: &mut ExternalHost, device: &mut Device) -> Result<u16, String> {
    command(
        host,
        HciCommand::WriteScanEnable { scan_enable: 0x03 },
        "enabling Classic page/inquiry scan",
    )?;
    let deadline = Instant::now() + PROCEDURE_TIMEOUT;
    loop {
        device.poll(host);
        if let Some(peer) = device.take_classic_connection_requests().into_iter().next() {
            device.accept_classic(host, peer);
        }
        if let Some(handle) = device.classic_connection_handle() {
            return Ok(handle);
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err("timed out waiting for Classic connection".into());
        }
        if !wait_tick(host, remaining)? {
            return Err("transport ended while waiting for Classic connection".into());
        }
    }
}

fn establish_connection(
    host: &mut ExternalHost,
    device: &mut Device,
    args: &Args,
) -> Result<u16, String> {
    if let Some((window, interval)) = args.le_scan {
        let window_units = (window / 0.625).round().clamp(4.0, 65_535.0) as u16;
        let interval_units = (interval / 0.625).round().clamp(4.0, 65_535.0) as u16;
        command(
            host,
            HciCommand::LeSetScanParameters {
                le_scan_type: 1,
                le_scan_interval: interval_units,
                le_scan_window: window_units,
                own_address_type: 1,
                scanning_filter_policy: 0,
            },
            "setting LE scan parameters",
        )?;
        command(
            host,
            HciCommand::LeSetScanEnable {
                le_scan_enable: 1,
                filter_duplicates: 0,
            },
            "enabling LE scan",
        )?;
    }
    let classic_peripheral = args.role == AppRole::Peripheral && args.mode.is_classic();
    let inquiry_scan = args.classic_inquiry_scan || classic_peripheral;
    let page_scan = args.classic_page_scan || classic_peripheral;
    if page_scan || inquiry_scan {
        let scan_enable = u8::from(inquiry_scan) | (u8::from(page_scan) << 1);
        command(
            host,
            HciCommand::WriteScanEnable { scan_enable },
            "configuring Classic scan enable",
        )?;
    }
    let handle = match (args.role, args.mode.is_classic()) {
        (AppRole::Central, false) => connect_le(host, device, args)?,
        (AppRole::Peripheral, false) => advertise_le(host, device, args)?,
        (AppRole::Central, true) => connect_classic(host, device, args)?,
        (AppRole::Peripheral, true) => accept_classic(host, device)?,
    };
    if let Some(role) = args.role_switch {
        if args.role == AppRole::Central && args.mode.is_classic() {
            let peer = device
                .classic_connection(handle)
                .ok_or_else(|| "Classic connection disappeared before role switch".to_string())?
                .peer_address
                .clone();
            let target_role = u8::from(role == AppRole::Peripheral);
            device.send_hci_command(
                host,
                HciCommand::SwitchRole {
                    bd_addr: peer,
                    role: target_role,
                },
            );
            let deadline = Instant::now() + PROCEDURE_TIMEOUT;
            loop {
                device.poll(host);
                if device
                    .classic_connection(handle)
                    .is_some_and(|connection| connection.role == target_role)
                {
                    break;
                }
                let remaining = deadline.saturating_duration_since(Instant::now());
                if remaining.is_zero() {
                    return Err("timed out waiting for Classic role switch".into());
                }
                if !wait_tick(host, remaining)? {
                    return Err("transport ended before Classic role switch completed".into());
                }
            }
        }
    }
    if args.role == AppRole::Central {
        std::thread::sleep(Duration::from_secs(1));
    }
    if let Some((octets, time)) = args.extended_data_length {
        if !args.mode.is_classic() {
            device.send_hci_command(
                host,
                HciCommand::LeSetDataLength {
                    connection_handle: handle,
                    tx_octets: octets,
                    tx_time: time,
                },
            );
        }
    }
    if args.mode.is_classic() && (args.authenticate || args.encrypt) {
        let mut pairing = ClassicPairingSession::accept_all(
            device,
            handle,
            PairingConfig {
                bonding: false,
                ..PairingConfig::default()
            },
            None,
        )
        .map_err(|error| error.to_string())?;
        pairing
            .pair(host, device, PROCEDURE_TIMEOUT)
            .map_err(|error| error.to_string())?;
        if args.encrypt {
            encrypt_classic(host, device, handle)?;
        }
    }
    if let Some(phy) = args.phy {
        if !args.mode.is_classic() {
            let bit = 1 << (phy.hci() - 1);
            device.send_hci_command(
                host,
                HciCommand::LeSetPhy {
                    connection_handle: handle,
                    all_phys: 0,
                    tx_phys: bit,
                    rx_phys: bit,
                    phy_options: 0,
                },
            );
        }
    }
    Ok(handle)
}

fn encrypt_classic(
    host: &mut ExternalHost,
    device: &mut Device,
    connection_handle: u16,
) -> Result<(), String> {
    if device.is_classic_encrypted_on_handle(connection_handle) {
        return Ok(());
    }
    if !device.set_classic_encryption_on_handle(host, connection_handle, true) {
        return Err("failed to request Classic encryption".into());
    }
    let deadline = Instant::now() + PROCEDURE_TIMEOUT;
    loop {
        device.poll(host);
        if device.is_classic_encrypted_on_handle(connection_handle) {
            return Ok(());
        }
        if device.classic_connection(connection_handle).is_none() {
            return Err("Classic connection ended before encryption completed".into());
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err("timed out waiting for Classic encryption".into());
        }
        if !wait_tick(host, remaining)? {
            return Err("transport ended before Classic encryption completed".into());
        }
    }
}

struct RfcommRuntime {
    connection_handle: u16,
    source_cid: u16,
    multiplexer: Multiplexer,
    dlci: u8,
}

fn service_record(uuid: Uuid, channel: u8) -> Vec<ServiceAttribute> {
    vec![
        ServiceAttribute {
            id: 0x0000,
            value: DataElement::unsigned_integer_32(0x0001_0001),
        },
        ServiceAttribute {
            id: 0x0005,
            value: DataElement::sequence([DataElement::uuid(public_browse_root())]),
        },
        ServiceAttribute {
            id: 0x0004,
            value: DataElement::sequence([
                DataElement::sequence([DataElement::uuid(Uuid::from_16_bits(0x0100))]),
                DataElement::sequence([
                    DataElement::uuid(Uuid::from_16_bits(RFCOMM_PSM)),
                    DataElement::unsigned_integer_8(channel),
                ]),
            ]),
        },
        ServiceAttribute {
            id: 0x0001,
            value: DataElement::sequence([DataElement::uuid(uuid)]),
        },
    ]
}

fn rfcomm_channel(attributes: &[ServiceAttribute]) -> Option<u8> {
    let DataElement::Sequence(protocols) = ServiceAttribute::find(attributes, 0x0004)? else {
        return None;
    };
    let DataElement::Sequence(rfcomm) = protocols.get(1)? else {
        return None;
    };
    if rfcomm.first()? != &DataElement::uuid(Uuid::from_16_bits(RFCOMM_PSM)) {
        return None;
    }
    match rfcomm.get(1)? {
        DataElement::UnsignedInteger { value, .. } => u8::try_from(*value).ok(),
        _ => None,
    }
}

struct ExternalSdpTransport<'a> {
    host: &'a mut ExternalHost,
    device: &'a mut Device,
    connection_handle: u16,
    source_cid: u16,
}

impl SdpTransport for ExternalSdpTransport<'_> {
    fn request(&mut self, request: &SdpPdu) -> Result<SdpPdu, TransportError> {
        let bytes = request
            .to_bytes()
            .map_err(|error| TransportError(error.to_string()))?;
        self.device
            .send_classic_channel_sdu(self.host, self.connection_handle, self.source_cid, &bytes)
            .map_err(|error| TransportError(error.to_string()))?;
        let deadline = Instant::now() + PROCEDURE_TIMEOUT;
        loop {
            self.device.poll(self.host);
            if let Some(bytes) = self
                .device
                .take_classic_channel_sdus(self.connection_handle, self.source_cid)
                .into_iter()
                .next()
            {
                return SdpPdu::from_bytes(&bytes)
                    .map_err(|error| TransportError(error.to_string()));
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(TransportError("timed out waiting for SDP response".into()));
            }
            match self
                .host
                .wait_for_activity(remaining)
                .map_err(|error| TransportError(error.to_string()))?
            {
                ExternalHostActivity::Packet => {}
                ExternalHostActivity::Timeout => {
                    return Err(TransportError("timed out waiting for SDP response".into()))
                }
                ExternalHostActivity::Ended => {
                    return Err(TransportError(
                        "transport ended while waiting for SDP response".into(),
                    ))
                }
            }
        }
    }
}

fn resolve_rfcomm_channel(
    host: &mut ExternalHost,
    device: &mut Device,
    connection_handle: u16,
    uuid: &Uuid,
) -> Result<u8, String> {
    let source_cid = device
        .connect_classic_channel(
            host,
            connection_handle,
            u32::from(SDP_PSM),
            ClassicChannelSpec {
                mtu: DEFAULT_RFCOMM_MTU,
            },
        )
        .map_err(|error| error.to_string())?;
    wait_for_classic_channel(host, device, connection_handle, source_cid)?;
    let services = SdpClient::new(ExternalSdpTransport {
        host,
        device,
        connection_handle,
        source_cid,
    })
    .service_search_attribute(
        std::slice::from_ref(uuid),
        &[AttributeId::Range(0x0000, 0xFFFF)],
    )
    .map_err(|error| error.to_string())?;
    device
        .disconnect_classic_channel(host, connection_handle, source_cid)
        .map_err(|error| error.to_string())?;
    services
        .iter()
        .find_map(|attributes| rfcomm_channel(attributes))
        .ok_or_else(|| format!("RFCOMM channel with UUID {uuid:?} not found"))
}

struct SdpEndpoint {
    source_cid: u16,
    server: SdpServer,
}

impl SdpEndpoint {
    fn new(source_cid: u16, peer_mtu: u16, uuid: Uuid, channel: u8) -> Self {
        let mut server = SdpServer::new(peer_mtu);
        server.add_service(0x0001_0001, service_record(uuid, channel));
        Self { source_cid, server }
    }

    fn poll(
        &mut self,
        host: &mut ExternalHost,
        device: &mut Device,
        connection_handle: u16,
    ) -> Result<(), String> {
        for bytes in device.take_classic_channel_sdus(connection_handle, self.source_cid) {
            let request = SdpPdu::from_bytes(&bytes).map_err(|error| error.to_string())?;
            let response = self.server.handle_request(&request);
            device
                .send_classic_channel_sdu(
                    host,
                    connection_handle,
                    self.source_cid,
                    &response.to_bytes().map_err(|error| error.to_string())?,
                )
                .map_err(|error| error.to_string())?;
        }
        Ok(())
    }
}

impl RfcommRuntime {
    fn flush(&mut self, host: &mut ExternalHost, device: &mut Device) -> Result<(), String> {
        for frame in self.multiplexer.drain_outgoing() {
            device
                .send_classic_channel_sdu(
                    host,
                    self.connection_handle,
                    self.source_cid,
                    &frame.to_bytes().map_err(|error| error.to_string())?,
                )
                .map_err(|error| error.to_string())?;
        }
        Ok(())
    }

    fn poll(
        &mut self,
        host: &mut ExternalHost,
        device: &mut Device,
    ) -> Result<Vec<Vec<u8>>, String> {
        for bytes in device.take_classic_channel_sdus(self.connection_handle, self.source_cid) {
            self.multiplexer
                .on_pdu(&RfcommFrame::from_bytes(&bytes).map_err(|error| error.to_string())?);
        }
        self.flush(host, device)?;
        Ok(self.multiplexer.take_rx(self.dlci))
    }
}

enum Endpoint {
    GattClient {
        connection_handle: u16,
        tx_handle: u16,
        rx_handle: u16,
        client: GattClient,
        unsolicited: VecDeque<AttPdu>,
    },
    GattServer {
        connection_handle: u16,
        rx_handle: u16,
        queue: Arc<Mutex<VecDeque<Vec<u8>>>>,
    },
    L2cap {
        connection_handle: u16,
        source_cid: u16,
        framer: StreamFramer,
    },
    Rfcomm {
        runtime: RfcommRuntime,
        framer: StreamFramer,
    },
    Iso {
        cis_handle: u16,
        framer: StreamFramer,
    },
}

fn wait_for_classic_channel(
    host: &mut ExternalHost,
    device: &mut Device,
    connection_handle: u16,
    source_cid: u16,
) -> Result<(), String> {
    let deadline = Instant::now() + PROCEDURE_TIMEOUT;
    loop {
        device.poll(host);
        if let Some(channel) = device.classic_channel(connection_handle, source_cid) {
            match channel.state {
                ClassicChannelState::Open => return Ok(()),
                ClassicChannelState::Closed => {
                    return Err("Classic L2CAP channel was refused".into())
                }
                _ => {}
            }
        }
        if let Some((_, error)) = device.take_classic_channel_errors().into_iter().next() {
            return Err(error);
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err("timed out opening Classic L2CAP channel".into());
        }
        if !wait_tick(host, remaining)? {
            return Err("transport ended while opening Classic L2CAP channel".into());
        }
    }
}

fn poll_rfcomm_until<F>(
    host: &mut ExternalHost,
    device: &mut Device,
    runtime: &mut RfcommRuntime,
    mut ready: F,
    description: &str,
) -> Result<(), String>
where
    F: FnMut(&mut RfcommRuntime) -> bool,
{
    let deadline = Instant::now() + PROCEDURE_TIMEOUT;
    loop {
        device.poll(host);
        runtime.poll(host, device)?;
        if ready(runtime) {
            return Ok(());
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(format!("timed out {description}"));
        }
        if !wait_tick(host, remaining)? {
            return Err(format!("transport ended while {description}"));
        }
    }
}

fn setup_rfcomm(
    host: &mut ExternalHost,
    device: &mut Device,
    connection_handle: u16,
    args: &Args,
    mut passive_pairing: Option<&mut ClassicPairingSession>,
) -> Result<Endpoint, String> {
    let channel = if args.rfcomm_channel == 0 && args.mode == Mode::RfcommClient {
        resolve_rfcomm_channel(host, device, connection_handle, &args.rfcomm_uuid)?
    } else if args.rfcomm_channel == 0 {
        RFCOMM_DYNAMIC_CHANNEL_NUMBER_START
    } else {
        args.rfcomm_channel
    };
    let max_frame_size = args
        .rfcomm_max_frame_size
        .unwrap_or(RFCOMM_DEFAULT_MAX_FRAME_SIZE);
    let initial_credits = u16::from(
        args.rfcomm_initial_credits
            .unwrap_or(RFCOMM_DEFAULT_INITIAL_CREDITS),
    );
    let source_cid = if args.mode == Mode::RfcommClient {
        let source_cid = device
            .connect_classic_channel(
                host,
                connection_handle,
                u32::from(RFCOMM_PSM),
                ClassicChannelSpec {
                    mtu: args.rfcomm_l2cap_mtu.unwrap_or(DEFAULT_RFCOMM_MTU),
                },
            )
            .map_err(|error| error.to_string())?;
        wait_for_classic_channel(host, device, connection_handle, source_cid)?;
        source_cid
    } else {
        let deadline = Instant::now() + PROCEDURE_TIMEOUT;
        let mut sdp = None;
        loop {
            device.poll(host);
            if let Some(pairing) = passive_pairing.as_deref_mut() {
                let _ = pairing
                    .drive_once(host, device)
                    .map_err(|error| error.to_string())?;
            }
            let accepted = device.take_accepted_classic_channels(connection_handle);
            let mut rfcomm = None;
            for source_cid in accepted {
                let info = device
                    .classic_channel(connection_handle, source_cid)
                    .ok_or_else(|| "accepted Classic channel disappeared".to_string())?;
                if info.psm == u32::from(SDP_PSM) {
                    sdp = Some(SdpEndpoint::new(
                        source_cid,
                        info.peer_mtu,
                        args.rfcomm_uuid.clone(),
                        channel,
                    ));
                } else if info.psm == u32::from(RFCOMM_PSM) {
                    rfcomm = Some(source_cid);
                }
            }
            if let Some(source_cid) = rfcomm {
                break source_cid;
            }
            if let Some(sdp) = &mut sdp {
                sdp.poll(host, device, connection_handle)?;
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err("timed out waiting for RFCOMM L2CAP channel".into());
            }
            if !wait_tick(host, remaining)? {
                return Err("transport ended while waiting for RFCOMM L2CAP channel".into());
            }
        }
    };
    let peer_mtu = device
        .classic_channel(connection_handle, source_cid)
        .ok_or_else(|| "RFCOMM L2CAP channel disappeared".to_string())?
        .peer_mtu;
    let role = if args.mode == Mode::RfcommClient {
        RfcommRole::Initiator
    } else {
        RfcommRole::Responder
    };
    let mut runtime = RfcommRuntime {
        connection_handle,
        source_cid,
        multiplexer: Multiplexer::new(role, peer_mtu),
        dlci: channel << 1,
    };
    if args.mode == Mode::RfcommClient {
        runtime
            .multiplexer
            .connect()
            .map_err(|error| error.to_string())?;
        runtime.flush(host, device)?;
        poll_rfcomm_until(
            host,
            device,
            &mut runtime,
            |runtime| runtime.multiplexer.state() == MultiplexerState::Connected,
            "opening RFCOMM multiplexer",
        )?;
        runtime
            .multiplexer
            .open_dlc(channel, max_frame_size, initial_credits)
            .map_err(|error| error.to_string())?;
        runtime.flush(host, device)?;
    } else {
        runtime
            .multiplexer
            .listen(channel, max_frame_size, initial_credits);
    }
    let dlci = runtime.dlci;
    poll_rfcomm_until(
        host,
        device,
        &mut runtime,
        |runtime| runtime.multiplexer.dlc_state(dlci) == Some(DlcState::Connected),
        "opening RFCOMM DLC",
    )?;
    if args.rfcomm_max_credits.is_some() || args.rfcomm_credits_threshold.is_some() {
        let max = u16::from(
            args.rfcomm_max_credits
                .unwrap_or(RFCOMM_DEFAULT_MAX_CREDITS),
        );
        let threshold = u16::from(args.rfcomm_credits_threshold.unwrap_or((max / 2) as u8));
        runtime
            .multiplexer
            .set_dlc_receive_credits(runtime.dlci, max, threshold)
            .map_err(|error| error.to_string())?;
    }
    Ok(Endpoint::Rfcomm {
        runtime,
        framer: StreamFramer::default(),
    })
}

fn setup_l2cap(
    host: &mut ExternalHost,
    device: &mut Device,
    connection_handle: u16,
    args: &Args,
) -> Result<Endpoint, String> {
    let spec = LeCreditBasedChannelSpec {
        psm: Some(args.l2cap_psm),
        mtu: args.l2cap_mtu,
        mps: args.l2cap_mps,
        max_credits: args.l2cap_max_credits,
    };
    let source_cid = if args.mode == Mode::L2capClient {
        let source_cid = device
            .connect_le_credit_channel(host, connection_handle, args.l2cap_psm, spec)
            .map_err(|error| error.to_string())?;
        let deadline = Instant::now() + PROCEDURE_TIMEOUT;
        loop {
            device.poll(host);
            if device
                .le_credit_channel(connection_handle, source_cid)
                .is_some()
            {
                break source_cid;
            }
            if let Some(result) = device.le_credit_connection_result(connection_handle, source_cid)
            {
                if result != 0 {
                    return Err(format!(
                        "LE credit channel failed with result 0x{result:04X}"
                    ));
                }
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err("timed out opening LE credit channel".into());
            }
            if !wait_tick(host, remaining)? {
                return Err("transport ended while opening LE credit channel".into());
            }
        }
    } else {
        let deadline = Instant::now() + PROCEDURE_TIMEOUT;
        loop {
            device.poll(host);
            if let Some(source_cid) = device
                .take_accepted_le_credit_channels(connection_handle)
                .into_iter()
                .next()
            {
                break source_cid;
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err("timed out waiting for LE credit channel".into());
            }
            if !wait_tick(host, remaining)? {
                return Err("transport ended while waiting for LE credit channel".into());
            }
        }
    };
    Ok(Endpoint::L2cap {
        connection_handle,
        source_cid,
        framer: StreamFramer::default(),
    })
}

fn setup_gatt_client(
    host: &mut ExternalHost,
    device: &mut Device,
    connection_handle: u16,
    att_mtu: u16,
) -> Result<Endpoint, String> {
    let mut transport =
        ExternalAttTransport::new(host, device, connection_handle, PROCEDURE_TIMEOUT)
            .map_err(|error| error.to_string())?;
    let mut client = GattClient::new();
    client
        .exchange_mtu(&mut transport, att_mtu)
        .map_err(|error| error.to_string())?;
    let service_uuid = Uuid::parse(SPEED_SERVICE_UUID).map_err(|error| error.to_string())?;
    let tx_uuid = Uuid::parse(SPEED_TX_UUID).map_err(|error| error.to_string())?;
    let rx_uuid = Uuid::parse(SPEED_RX_UUID).map_err(|error| error.to_string())?;
    let service = client
        .discover_services(&mut transport)
        .map_err(|error| error.to_string())?
        .into_iter()
        .find(|service| service.uuid == service_uuid)
        .ok_or_else(|| "Speed Service not found".to_string())?;
    let characteristics = client
        .discover_characteristics(&mut transport, &service)
        .map_err(|error| error.to_string())?;
    let tx = characteristics
        .iter()
        .find(|characteristic| characteristic.uuid == tx_uuid)
        .ok_or_else(|| "Speed TX characteristic not found".to_string())?;
    let rx = characteristics
        .iter()
        .find(|characteristic| characteristic.uuid == rx_uuid)
        .ok_or_else(|| "Speed RX characteristic not found".to_string())?;
    let cccd = client
        .discover_descriptors(&mut transport, rx)
        .map_err(|error| error.to_string())?
        .into_iter()
        .find(|descriptor| descriptor.uuid == Uuid::from_16_bits(0x2902))
        .ok_or_else(|| "Speed RX CCCD not found".to_string())?;
    client
        .subscribe(&mut transport, rx.handle, cccd.handle, false)
        .map_err(|error| error.to_string())?;
    let unsolicited = transport.take_unsolicited().into();
    Ok(Endpoint::GattClient {
        connection_handle,
        tx_handle: tx.handle,
        rx_handle: rx.handle,
        client,
        unsolicited,
    })
}

fn setup_iso(
    host: &mut ExternalHost,
    device: &mut Device,
    acl_handle: u16,
    args: &Args,
) -> Result<Endpoint, String> {
    let cis_handle = if args.role == AppRole::Central {
        let sender = args.scenario.is_sender();
        let sdu_interval_c_to_p = args.iso_sdu_interval_c_to_p.unwrap_or(if sender {
            DEFAULT_ISO_SDU_INTERVAL_C_TO_P
        } else {
            0
        });
        let sdu_interval_p_to_c = args.iso_sdu_interval_p_to_c.unwrap_or(if sender {
            0
        } else {
            DEFAULT_ISO_SDU_INTERVAL_P_TO_C
        });
        let max_sdu_c_to_p = args.iso_max_sdu_c_to_p.unwrap_or(if sender {
            DEFAULT_ISO_MAX_SDU_C_TO_P
        } else {
            0
        });
        let max_sdu_p_to_c = args.iso_max_sdu_p_to_c.unwrap_or(if sender {
            0
        } else {
            DEFAULT_ISO_MAX_SDU_P_TO_C
        });
        device.send_hci_command(
            host,
            HciCommand::LeSetCigParameters {
                cig_id: 1,
                sdu_interval_c_to_p,
                sdu_interval_p_to_c,
                worst_case_sca: 0,
                packing: 0,
                framing: 0,
                max_transport_latency_c_to_p: args.iso_max_transport_latency_c_to_p.unwrap_or(
                    if sender {
                        DEFAULT_ISO_MAX_TRANSPORT_LATENCY_C_TO_P
                    } else {
                        0
                    },
                ),
                max_transport_latency_p_to_c: args.iso_max_transport_latency_p_to_c.unwrap_or(
                    if sender {
                        0
                    } else {
                        DEFAULT_ISO_MAX_TRANSPORT_LATENCY_P_TO_C
                    },
                ),
                cis_id: vec![2],
                max_sdu_c_to_p: vec![max_sdu_c_to_p],
                max_sdu_p_to_c: vec![max_sdu_p_to_c],
                phy_c_to_p: vec![1],
                phy_p_to_c: vec![1],
                rtn_c_to_p: vec![args.iso_rtn_c_to_p.unwrap_or(if sender {
                    DEFAULT_ISO_RTN_C_TO_P
                } else {
                    0
                })],
                rtn_p_to_c: vec![args.iso_rtn_p_to_c.unwrap_or(if sender {
                    0
                } else {
                    DEFAULT_ISO_RTN_P_TO_C
                })],
            },
        );
        let deadline = Instant::now() + PROCEDURE_TIMEOUT;
        let configured = loop {
            device.poll(host);
            if let Some(handle) = device.take_configured_cis_handles().into_iter().next() {
                break handle;
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err("timed out configuring CIG".into());
            }
            if !wait_tick(host, remaining)? {
                return Err("transport ended while configuring CIG".into());
            }
        };
        if !device.create_cis_on_handle(host, acl_handle, configured) {
            return Err("failed to create CIS".into());
        }
        configured
    } else {
        let deadline = Instant::now() + PROCEDURE_TIMEOUT;
        loop {
            device.poll(host);
            if let Some(request) = device.take_cis_requests().into_iter().next() {
                device.accept_cis(host, request.cis_connection_handle);
                break request.cis_connection_handle;
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err("timed out waiting for CIS request".into());
            }
            if !wait_tick(host, remaining)? {
                return Err("transport ended while waiting for CIS request".into());
            }
        }
    };
    let deadline = Instant::now() + PROCEDURE_TIMEOUT;
    loop {
        device.poll(host);
        if device
            .established_cis_handles()
            .any(|handle| handle == cis_handle)
        {
            break;
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err("timed out establishing CIS".into());
        }
        if !wait_tick(host, remaining)? {
            return Err("transport ended while establishing CIS".into());
        }
    }
    let direction = if args.scenario.is_sender() { 0 } else { 1 };
    if !device.setup_iso_data_path(host, cis_handle, direction) {
        return Err("failed to set up CIS data path".into());
    }
    Ok(Endpoint::Iso {
        cis_handle,
        framer: StreamFramer::default(),
    })
}

fn setup_endpoint(
    host: &mut ExternalHost,
    prepared: &mut PreparedDevice,
    connection_handle: u16,
    args: &Args,
    passive_pairing: Option<&mut ClassicPairingSession>,
) -> Result<Endpoint, String> {
    match args.mode {
        Mode::GattClient => {
            setup_gatt_client(host, &mut prepared.device, connection_handle, args.att_mtu)
        }
        Mode::GattServer => {
            if args.scenario.is_sender() {
                let cccd = prepared
                    .gatt_cccd
                    .as_ref()
                    .expect("GATT server prepared CCCD");
                let deadline = Instant::now() + PROCEDURE_TIMEOUT;
                while cccd.load(Ordering::SeqCst) & 1 == 0 {
                    prepared.device.poll(host);
                    let remaining = deadline.saturating_duration_since(Instant::now());
                    if remaining.is_zero() {
                        return Err("timed out waiting for GATT subscription".into());
                    }
                    if !wait_tick(host, remaining)? {
                        return Err("transport ended while waiting for GATT subscription".into());
                    }
                }
            }
            Ok(Endpoint::GattServer {
                connection_handle,
                rx_handle: prepared
                    .gatt_rx_handle
                    .expect("GATT server prepared RX handle"),
                queue: Arc::clone(
                    prepared
                        .gatt_queue
                        .as_ref()
                        .expect("GATT server prepared queue"),
                ),
            })
        }
        Mode::L2capClient | Mode::L2capServer => {
            setup_l2cap(host, &mut prepared.device, connection_handle, args)
        }
        Mode::RfcommClient | Mode::RfcommServer => setup_rfcomm(
            host,
            &mut prepared.device,
            connection_handle,
            args,
            passive_pairing,
        ),
        Mode::IsoClient | Mode::IsoServer => {
            setup_iso(host, &mut prepared.device, connection_handle, args)
        }
    }
}

fn drain_gatt_notifications(
    client: &mut GattClient,
    rx_handle: u16,
    unsolicited: &mut VecDeque<AttPdu>,
) -> Vec<Vec<u8>> {
    let mut packets = Vec::new();
    for pdu in unsolicited.drain(..) {
        if let AttPdu::HandleValueNotification {
            attribute_handle,
            attribute_value,
        } = &pdu
        {
            if *attribute_handle == rx_handle {
                packets.push(attribute_value.clone());
            }
        }
        let _ = client.on_notification(&pdu);
    }
    packets
}

impl Endpoint {
    fn send(
        &mut self,
        host: &mut ExternalHost,
        device: &mut Device,
        packet: &[u8],
    ) -> Result<(), String> {
        match self {
            Self::GattClient {
                connection_handle,
                tx_handle,
                client,
                unsolicited,
                ..
            } => {
                let mut transport =
                    ExternalAttTransport::new(host, device, *connection_handle, PROCEDURE_TIMEOUT)
                        .map_err(|error| error.to_string())?;
                let result = client
                    .write_value(&mut transport, *tx_handle, packet.to_vec(), true)
                    .map_err(|error| error.to_string());
                unsolicited.extend(transport.take_unsolicited());
                result
            }
            Self::GattServer {
                connection_handle,
                rx_handle,
                ..
            } => {
                if device.notify_on_handle(host, *connection_handle, *rx_handle, packet.to_vec()) {
                    Ok(())
                } else {
                    Err("failed to send GATT notification".into())
                }
            }
            Self::L2cap {
                connection_handle,
                source_cid,
                ..
            } => device
                .send_le_credit_sdu(
                    host,
                    *connection_handle,
                    *source_cid,
                    &StreamFramer::frame(packet)?,
                )
                .map_err(|error| error.to_string()),
            Self::Rfcomm { runtime, .. } => {
                runtime
                    .multiplexer
                    .write(runtime.dlci, &StreamFramer::frame(packet)?)
                    .map_err(|error| error.to_string())?;
                runtime.flush(host, device)
            }
            Self::Iso { cis_handle, .. } => {
                if device.send_iso_sdu(host, *cis_handle, &StreamFramer::frame(packet)?) {
                    Ok(())
                } else {
                    Err("failed to send CIS SDU".into())
                }
            }
        }
    }

    fn poll(
        &mut self,
        host: &mut ExternalHost,
        device: &mut Device,
    ) -> Result<Vec<Vec<u8>>, String> {
        device.poll(host);
        match self {
            Self::GattClient {
                connection_handle,
                rx_handle,
                client,
                unsolicited,
                ..
            } => {
                unsolicited.extend(device.take_inbox_on_handle(*connection_handle));
                Ok(drain_gatt_notifications(client, *rx_handle, unsolicited))
            }
            Self::GattServer { queue, .. } => {
                let mut queue = queue.lock().map_err(|_| "GATT benchmark queue poisoned")?;
                Ok(queue.drain(..).collect())
            }
            Self::L2cap {
                connection_handle,
                source_cid,
                framer,
            } => {
                let mut packets = Vec::new();
                for sdu in device.take_le_credit_sdus(*connection_handle, *source_cid) {
                    packets.extend(framer.push(&sdu)?);
                }
                if let Some((_, error)) = device.take_le_credit_errors().into_iter().next() {
                    return Err(error);
                }
                Ok(packets)
            }
            Self::Rfcomm { runtime, framer } => {
                let mut packets = Vec::new();
                for data in runtime.poll(host, device)? {
                    packets.extend(framer.push(&data)?);
                }
                Ok(packets)
            }
            Self::Iso { cis_handle, framer } => {
                let mut packets = Vec::new();
                for sdu in device.take_iso_sdus(*cis_handle) {
                    if sdu.packet_status_flag == 0 {
                        packets.extend(framer.push(&sdu.data)?);
                    }
                }
                Ok(packets)
            }
        }
    }

    fn is_drained(&self, device: &Device) -> bool {
        match self {
            Self::L2cap {
                connection_handle,
                source_cid,
                ..
            } => device.le_credit_output_is_drained(*connection_handle, *source_cid),
            Self::Rfcomm { runtime, .. } => {
                runtime
                    .multiplexer
                    .dlc_pending_tx(runtime.dlci)
                    .is_some_and(|pending| pending == 0)
                    && device.classic_channel_output_is_drained(runtime.connection_handle)
            }
            _ => true,
        }
    }

    fn can_receive(&self) -> bool {
        !matches!(self, Self::Iso { .. })
    }
}

fn wait_until_drained(
    endpoint: &mut Endpoint,
    host: &mut ExternalHost,
    device: &mut Device,
) -> Result<(), String> {
    while !endpoint.is_drained(device) {
        let _ = endpoint.poll(host, device)?;
        if !wait_tick(host, POLL_INTERVAL)? {
            return Err("transport ended while draining benchmark output".into());
        }
    }
    Ok(())
}

fn print_stats(title: &str, values: &[f64], unit: &str) {
    if let Some((min, max, mean, deviation)) = stats(values) {
        println!(
            "### {title}: min={min:.3}{unit}, max={max:.3}{unit}, average={mean:.3}{unit}, stdev={deviation:.3}{unit}"
        );
    }
}

fn wait_for_packets(
    endpoint: &mut Endpoint,
    host: &mut ExternalHost,
    device: &mut Device,
    deadline: Option<Instant>,
) -> Result<Vec<Vec<u8>>, String> {
    loop {
        let packets = endpoint.poll(host, device)?;
        if !packets.is_empty() {
            return Ok(packets);
        }
        if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
            return Err("timed out waiting for benchmark packet".into());
        }
        if !wait_tick(host, POLL_INTERVAL)? {
            return Err("transport ended while waiting for benchmark packet".into());
        }
    }
}

fn run_send(
    args: &Args,
    endpoint: &mut Endpoint,
    host: &mut ExternalHost,
    device: &mut Device,
) -> Result<(), String> {
    let mut run_speeds = Vec::new();
    for run in 0..=args.repeat {
        if run > 0 && !args.repeat_delay.is_zero() {
            std::thread::sleep(args.repeat_delay);
        }
        if !args.start_delay.is_zero() {
            std::thread::sleep(args.start_delay);
        }
        println!("=== Sending RESET");
        endpoint.send(host, device, &Packet::reset().to_bytes())?;
        let start = Instant::now();
        let mut bytes_sent = 0usize;
        for sequence in 0..args.packet_count {
            if !args.pace.is_zero() {
                let target = start + args.pace * sequence;
                let delay = target.saturating_duration_since(Instant::now());
                if !delay.is_zero() {
                    std::thread::sleep(delay);
                }
            } else {
                wait_until_drained(endpoint, host, device)?;
            }
            let packet = Packet::sequence(
                sequence,
                sequence + 1 == args.packet_count,
                u32::try_from(start.elapsed().as_micros()).unwrap_or(u32::MAX),
                args.packet_size - args.mode.overhead(),
            )?
            .to_bytes();
            bytes_sent += packet.len();
            println!("Sending packet {sequence}: {} bytes", args.packet_size);
            endpoint.send(host, device, &packet)?;
        }
        if endpoint.can_receive() {
            let deadline = Instant::now() + PROCEDURE_TIMEOUT;
            let elapsed = loop {
                let mut final_ack = false;
                for bytes in wait_for_packets(endpoint, host, device, Some(deadline))? {
                    let packet = Packet::from_bytes(&bytes)?;
                    if packet.packet_type == PacketType::Ack && packet.last {
                        final_ack = true;
                        break;
                    }
                }
                if final_ack {
                    break start.elapsed();
                }
                if Instant::now() >= deadline {
                    return Err("timed out waiting for final ACK".into());
                }
            };
            let speed = bytes_sent as f64 / elapsed.as_secs_f64().max(f64::EPSILON);
            run_speeds.push(speed);
            println!(
                "@@@ Received ACK. Speed: average={speed:.4} ({bytes_sent} bytes in {:.2} seconds)",
                elapsed.as_secs_f64()
            );
        }
        println!("=== [{} of {}] Done!", run + 1, args.repeat + 1);
    }
    if args.repeat > 0 {
        print_stats("Run throughput", &run_speeds, " B/s");
    }
    Ok(())
}

#[derive(Default)]
struct ReceiveStats {
    expected: u32,
    started_at: Option<Instant>,
    first: Option<(Instant, u32)>,
    last: Option<Instant>,
    total_bytes: usize,
    jitter: Vec<f64>,
    measurements: VecDeque<(Instant, usize)>,
}

impl ReceiveStats {
    fn reset(&mut self) {
        *self = Self::default();
        let now = Instant::now();
        self.started_at = Some(now);
        self.measurements.push_back((now, 0));
    }

    fn record(&mut self, packet: &Packet, bytes: usize, overhead: usize) {
        let now = Instant::now();
        let started_at = *self.started_at.get_or_insert(now);
        if self.measurements.is_empty() {
            self.measurements.push_back((started_at, 0));
        }
        let (first_time, first_timestamp) = *self.first.get_or_insert((now, packet.timestamp_us));
        let expected = first_time
            + Duration::from_micros(u64::from(packet.timestamp_us.wrapping_sub(first_timestamp)));
        self.jitter.push(if now >= expected {
            now.duration_since(expected).as_secs_f64()
        } else {
            -expected.duration_since(now).as_secs_f64()
        });
        self.total_bytes += bytes;
        self.measurements.push_back((now, bytes));
        while self.measurements.len() > 64 {
            self.measurements.pop_front();
        }
        let instant = self
            .last
            .map(|last| bytes as f64 / now.duration_since(last).as_secs_f64().max(f64::EPSILON))
            .unwrap_or(0.0);
        let average = self.total_bytes as f64
            / now
                .duration_since(started_at)
                .as_secs_f64()
                .max(f64::EPSILON);
        let windowed = self
            .measurements
            .front()
            .map(|(start, _)| {
                self.measurements
                    .iter()
                    .skip(1)
                    .map(|(_, bytes)| *bytes)
                    .sum::<usize>() as f64
                    / now.duration_since(*start).as_secs_f64().max(f64::EPSILON)
            })
            .unwrap_or(0.0);
        println!(
            "<<< Received packet {}: last={}, {} bytes | speed instant={instant:.2}, windowed={windowed:.2}, average={average:.2}",
            packet.sequence,
            packet.last,
            bytes + overhead
        );
        if packet.sequence != self.expected {
            eprintln!(
                "!!! Unexpected packet, expected {} but received {}",
                self.expected, packet.sequence
            );
        }
        self.expected = packet.sequence.wrapping_add(1);
        self.last = Some(now);
    }

    fn print_jitter(&self) {
        if self.jitter.len() < 3 {
            return;
        }
        let mean = self.jitter.iter().sum::<f64>() / self.jitter.len() as f64;
        let adjusted = self
            .jitter
            .iter()
            .map(|jitter| (jitter - mean) * 1_000.0)
            .collect::<Vec<_>>();
        print_stats("Jitter signed", &adjusted, " ms");
        print_stats(
            "Jitter absolute",
            &adjusted.iter().map(|value| value.abs()).collect::<Vec<_>>(),
            " ms",
        );
    }
}

fn run_receive(
    args: &Args,
    endpoint: &mut Endpoint,
    host: &mut ExternalHost,
    device: &mut Device,
) -> Result<(), String> {
    let mut receive = ReceiveStats::default();
    loop {
        for bytes in wait_for_packets(endpoint, host, device, None)? {
            let packet = Packet::from_bytes(&bytes)?;
            if packet.packet_type == PacketType::Reset {
                println!("=== Received RESET");
                receive.reset();
                continue;
            }
            if packet.packet_type != PacketType::Sequence {
                continue;
            }
            receive.record(&packet, bytes.len(), args.mode.overhead());
            if packet.last {
                endpoint.send(host, device, &Packet::ack(packet.sequence, true).to_bytes())?;
                receive.print_jitter();
                if !args.linger {
                    println!("=== Done!");
                    return Ok(());
                }
            }
        }
    }
}

fn run_ping(
    args: &Args,
    endpoint: &mut Endpoint,
    host: &mut ExternalHost,
    device: &mut Device,
) -> Result<(), String> {
    let mut minimums = Vec::new();
    let mut maximums = Vec::new();
    let mut averages = Vec::new();
    for run in 0..=args.repeat {
        if run > 0 && !args.repeat_delay.is_zero() {
            std::thread::sleep(args.repeat_delay);
        }
        if !args.start_delay.is_zero() {
            std::thread::sleep(args.start_delay);
        }
        endpoint.send(host, device, &Packet::reset().to_bytes())?;
        let start = Instant::now();
        let mut sent = BTreeMap::new();
        for sequence in 0..args.packet_count {
            let target = start + args.pace * sequence;
            let delay = target.saturating_duration_since(Instant::now());
            if !delay.is_zero() {
                std::thread::sleep(delay);
            }
            let now = Instant::now();
            let packet = Packet::sequence(
                sequence,
                sequence + 1 == args.packet_count,
                u32::try_from(start.elapsed().as_micros()).unwrap_or(u32::MAX),
                args.packet_size,
            )?;
            sent.insert(sequence, now);
            endpoint.send(host, device, &packet.to_bytes())?;
        }
        let deadline = Instant::now() + PROCEDURE_TIMEOUT;
        let mut rtts = Vec::new();
        let mut final_ack = false;
        while !final_ack {
            for bytes in wait_for_packets(endpoint, host, device, Some(deadline))? {
                let packet = Packet::from_bytes(&bytes)?;
                if packet.packet_type != PacketType::Ack {
                    continue;
                }
                let sent_at = sent
                    .get(&packet.sequence)
                    .ok_or_else(|| format!("ACK for unsent packet {}", packet.sequence))?;
                let rtt = sent_at.elapsed().as_secs_f64() * 1_000.0;
                println!("<<< Received ACK [{}], RTT={rtt:.2}ms", packet.sequence);
                rtts.push(rtt);
                final_ack |= packet.last;
            }
        }
        let (min, max, average, deviation) = stats(&rtts).ok_or("no RTT samples")?;
        println!(
            "@@@ RTTs: min={min:.2}ms, max={max:.2}ms, average={average:.2}ms, stdev={deviation:.2}ms"
        );
        minimums.push(min);
        maximums.push(max);
        averages.push(average);
        println!("=== [{} of {}] Done!", run + 1, args.repeat + 1);
    }
    if args.repeat > 0 {
        print_stats("Min RTT", &minimums, " ms");
        print_stats("Max RTT", &maximums, " ms");
        print_stats("Average RTT", &averages, " ms");
    }
    Ok(())
}

fn run_pong(
    args: &Args,
    endpoint: &mut Endpoint,
    host: &mut ExternalHost,
    device: &mut Device,
) -> Result<(), String> {
    let mut receive = ReceiveStats::default();
    loop {
        for bytes in wait_for_packets(endpoint, host, device, None)? {
            let packet = Packet::from_bytes(&bytes)?;
            if packet.packet_type == PacketType::Reset {
                receive.reset();
                continue;
            }
            if packet.packet_type != PacketType::Sequence {
                continue;
            }
            receive.record(&packet, bytes.len(), 0);
            endpoint.send(
                host,
                device,
                &Packet::ack(packet.sequence, packet.last).to_bytes(),
            )?;
            if packet.last {
                receive.print_jitter();
                if !args.linger {
                    println!("=== Done!");
                    return Ok(());
                }
            }
        }
    }
}

fn run(args: Args) -> Result<(), String> {
    let (mut host, mut prepared, _) = open_device(&args)?;
    if args.mode == Mode::L2capServer {
        prepared
            .device
            .register_le_credit_server(LeCreditBasedChannelSpec {
                psm: Some(args.l2cap_psm),
                mtu: args.l2cap_mtu,
                mps: args.l2cap_mps,
                max_credits: args.l2cap_max_credits,
            })
            .map_err(|error| error.to_string())?;
    }
    if args.mode == Mode::RfcommServer {
        prepared
            .device
            .register_classic_channel_server(
                Some(u32::from(RFCOMM_PSM)),
                ClassicChannelSpec {
                    mtu: args.rfcomm_l2cap_mtu.unwrap_or(DEFAULT_RFCOMM_MTU),
                },
            )
            .map_err(|error| error.to_string())?;
        prepared
            .device
            .register_classic_channel_server(
                Some(u32::from(SDP_PSM)),
                ClassicChannelSpec {
                    mtu: DEFAULT_RFCOMM_MTU,
                },
            )
            .map_err(|error| error.to_string())?;
    }
    if args.role == AppRole::Central {
        if let Some(interval) = args.le_advertise {
            start_legacy_advertising(&mut host, interval)?;
        }
    }
    let connection_handle = establish_connection(&mut host, &mut prepared.device, &args)?;
    println!("### Connected on handle 0x{connection_handle:04X}");
    let mut passive_pairing = if args.role == AppRole::Peripheral && args.mode.is_classic() {
        let mut pairing = ClassicPairingSession::accept_all(
            &prepared.device,
            connection_handle,
            PairingConfig {
                bonding: false,
                ..PairingConfig::default()
            },
            None,
        )
        .map_err(|error| error.to_string())?;
        pairing
            .listen(&prepared.device)
            .map_err(|error| error.to_string())?;
        Some(pairing)
    } else {
        None
    };
    let mut endpoint = setup_endpoint(
        &mut host,
        &mut prepared,
        connection_handle,
        &args,
        passive_pairing.as_mut(),
    )?;
    println!("--- Go! mode={:?}, scenario={:?}", args.mode, args.scenario);
    match args.scenario {
        Scenario::Send => run_send(&args, &mut endpoint, &mut host, &mut prepared.device)?,
        Scenario::Receive => run_receive(&args, &mut endpoint, &mut host, &mut prepared.device)?,
        Scenario::Ping => run_ping(&args, &mut endpoint, &mut host, &mut prepared.device)?,
        Scenario::Pong => run_pong(&args, &mut endpoint, &mut host, &mut prepared.device)?,
    }
    std::thread::sleep(Duration::from_secs(1));
    prepared
        .device
        .disconnect_handle(&mut host, connection_handle, 0x13);
    Ok(())
}

fn main() -> ExitCode {
    match parse_args(std::env::args()) {
        Ok(None) => {
            println!("{}", usage());
            ExitCode::SUCCESS
        }
        Ok(Some(args)) => match run(args) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("{error}");
                ExitCode::FAILURE
            }
        },
        Err(error) => {
            eprintln!("{error}\n{}", usage());
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(arguments: &[&str]) -> Result<Option<Args>, String> {
        parse_args(arguments.iter().copied())
    }

    #[test]
    fn packet_wire_format_matches_upstream() {
        assert_eq!(Packet::reset().to_bytes(), vec![0]);
        assert_eq!(
            Packet::ack(0x0403_0201, true).to_bytes(),
            vec![2, 1, 1, 2, 3, 4]
        );
        let sequence = Packet::sequence(0x0403_0201, true, 0x0807_0605, 12).unwrap();
        assert_eq!(
            sequence.to_bytes(),
            vec![1, 1, 1, 2, 3, 4, 5, 6, 7, 8, 0, 0]
        );
        assert_eq!(Packet::from_bytes(&sequence.to_bytes()).unwrap(), sequence);
    }

    #[test]
    fn packet_decoder_rejects_malformed_inputs() {
        for malformed in [
            Vec::new(),
            vec![3],
            vec![0, 0],
            vec![2, 0, 0, 0, 0],
            vec![2, 0, 0, 0, 0, 0, 0],
            vec![1, 0, 0, 0, 0, 0, 0, 0, 0],
        ] {
            assert!(Packet::from_bytes(&malformed).is_err(), "{malformed:02X?}");
        }
        assert!(Packet::sequence(0, false, 0, 9).is_err());
    }

    #[test]
    fn stream_framer_handles_fragmentation_and_coalescing() {
        let first = StreamFramer::frame(&[1, 2, 3]).unwrap();
        let second = StreamFramer::frame(&[4, 5]).unwrap();
        assert_eq!(first, vec![0, 3, 1, 2, 3]);
        let mut framer = StreamFramer::default();
        assert!(framer.push(&first[..1]).unwrap().is_empty());
        assert!(framer.push(&first[1..4]).unwrap().is_empty());
        let mut tail = first[4..].to_vec();
        tail.extend_from_slice(&second);
        assert_eq!(framer.push(&tail).unwrap(), vec![vec![1, 2, 3], vec![4, 5]]);
        assert!(StreamFramer::default().push(&[0, 0]).is_err());
        assert!(StreamFramer::frame(&vec![0; 65_536]).is_err());
    }

    #[test]
    fn gatt_notifications_retained_during_writes_are_delivered() {
        let mut client = GattClient::new();
        let mut unsolicited = VecDeque::from([
            AttPdu::HandleValueNotification {
                attribute_handle: 7,
                attribute_value: vec![1, 2, 3],
            },
            AttPdu::HandleValueNotification {
                attribute_handle: 8,
                attribute_value: vec![4, 5],
            },
            AttPdu::WriteResponse,
        ]);
        assert_eq!(
            drain_gatt_notifications(&mut client, 7, &mut unsolicited),
            vec![vec![1, 2, 3]]
        );
        assert!(unsolicited.is_empty());
    }

    #[test]
    fn sample_statistics_match_upstream_calculation() {
        assert_eq!(stats(&[]), None);
        assert_eq!(stats(&[4.0]), Some((4.0, 4.0, 4.0, 0.0)));
        let (min, max, mean, deviation) = stats(&[1.0, 2.0, 3.0]).unwrap();
        assert_eq!((min, max, mean), (1.0, 3.0, 2.0));
        assert!((deviation - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn cli_defaults_follow_role() {
        let central = parsed(&["bench", "central", "tcp-client:127.0.0.1:1234"])
            .unwrap()
            .unwrap();
        assert_eq!(central.role, AppRole::Central);
        assert_eq!(central.mode, Mode::GattClient);
        assert_eq!(central.scenario, Scenario::Send);
        assert_eq!(central.peer, DEFAULT_PERIPHERAL_ADDRESS);
        assert_eq!(central.packet_size, 500);
        assert_eq!(central.packet_count, 10);

        let peripheral = parsed(&["bench", "peripheral", "tcp-server:1234"])
            .unwrap()
            .unwrap();
        assert_eq!(peripheral.role, AppRole::Peripheral);
        assert_eq!(peripheral.mode, Mode::GattServer);
        assert_eq!(peripheral.scenario, Scenario::Receive);
    }

    #[test]
    fn cli_accepts_every_transport_mode() {
        for mode in [
            "gatt-client",
            "gatt-server",
            "l2cap-client",
            "l2cap-server",
            "rfcomm-client",
            "rfcomm-server",
            "iso-client",
            "iso-server",
        ] {
            let args = parsed(&[
                "bench",
                "--mode",
                mode,
                "--scenario",
                "send",
                "central",
                "tcp-client:127.0.0.1:1234",
            ])
            .unwrap()
            .unwrap();
            assert_eq!(args.mode, Mode::parse(mode).unwrap());
        }
    }

    #[test]
    fn cli_parses_upstream_tuning_surface() {
        let args = parsed(&[
            "bench",
            "--device-config",
            "device.json",
            "--scenario",
            "pong",
            "--mode",
            "rfcomm-client",
            "--att-mtu",
            "247",
            "--extended-data-length",
            "251/2120",
            "--role-switch",
            "peripheral",
            "--le-scan",
            "10.0/20.0",
            "--le-advertise",
            "30.0",
            "--classic-page-scan",
            "--classic-inquiry-scan",
            "--rfcomm-channel",
            "0",
            "--rfcomm-uuid",
            DEFAULT_RFCOMM_UUID,
            "--rfcomm-l2cap-mtu",
            "1024",
            "--rfcomm-max-frame-size",
            "512",
            "--rfcomm-initial-credits",
            "7",
            "--rfcomm-max-credits",
            "20",
            "--rfcomm-credits-threshold",
            "10",
            "--l2cap-psm",
            "129",
            "--l2cap-mtu",
            "1000",
            "--l2cap-mps",
            "500",
            "--l2cap-max-credits",
            "100",
            "--packet-size",
            "1000",
            "--packet-count",
            "20",
            "--start-delay",
            "2",
            "--repeat",
            "3",
            "--repeat-delay",
            "4",
            "--pace",
            "5",
            "--linger",
            "central",
            "tcp-client:127.0.0.1:1234",
            "--peripheral",
            "F2:F2:F2:F2:F2:F2",
            "--connection-interval",
            "24",
            "--phy",
            "2m",
            "--authenticate",
            "--encrypt",
            "--iso-sdu-interval-c-to-p",
            "10000",
            "--iso-sdu-interval-p-to-c",
            "11000",
            "--iso-max-sdu-c-to-p",
            "250",
            "--iso-max-sdu-p-to-c",
            "240",
            "--iso-max-transport-latency-c-to-p",
            "30",
            "--iso-max-transport-latency-p-to-c",
            "31",
            "--iso-rtn-c-to-p",
            "2",
            "--iso-rtn-p-to-c",
            "3",
        ])
        .unwrap()
        .unwrap();
        assert_eq!(args.scenario, Scenario::Pong);
        assert_eq!(args.extended_data_length, Some((251, 2120)));
        assert_eq!(args.le_scan, Some((10.0, 20.0)));
        assert_eq!(args.rfcomm_channel, 0);
        assert_eq!(args.rfcomm_max_credits, Some(20));
        assert_eq!(args.packet_size, 1000);
        assert_eq!(args.repeat, 3);
        assert_eq!(args.phy, Some(Phy::TwoM));
        assert!(args.authenticate && args.encrypt);
        assert_eq!(args.iso_rtn_p_to_c, Some(3));
    }

    #[test]
    fn cli_rejects_invalid_mode_combinations_and_ranges() {
        assert!(parsed(&[
            "bench",
            "--mode",
            "iso-client",
            "--scenario",
            "ping",
            "central",
            "serial:/dev/null",
        ])
        .is_err());
        assert!(parsed(&["bench", "--le-scan", "20/10", "central", "serial:/dev/null",]).is_err());
        assert!(parsed(&[
            "bench",
            "--rfcomm-max-credits",
            "4",
            "--rfcomm-credits-threshold",
            "5",
            "central",
            "serial:/dev/null",
        ])
        .is_err());
    }
}
