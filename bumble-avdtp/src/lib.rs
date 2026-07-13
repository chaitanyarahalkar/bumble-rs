//! Audio/Video Distribution Transport Protocol signaling primitives.
//!
//! This module ports the message and fragmentation boundary from Bumble's
//! `avdtp.py`. It is synchronous and transport-neutral so it can run over the
//! existing Classic L2CAP channel runtime.

use core::fmt;

pub mod host;
pub mod l2cap;
pub mod session;

pub const AVDTP_PSM: u16 = 0x0019;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    Truncated(&'static str),
    Invalid(&'static str),
    CapabilityLength,
    PayloadTooLong,
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for Error {}

pub type Result<T> = core::result::Result<T, Error>;

macro_rules! open_u8 {
    ($name:ident { $($constant:ident = $value:expr),+ $(,)? }) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(pub u8);

        impl $name {
            $(pub const $constant: Self = Self($value);)+
        }

        impl From<u8> for $name {
            fn from(value: u8) -> Self { Self(value) }
        }

        impl From<$name> for u8 {
            fn from(value: $name) -> Self { value.0 }
        }
    };
}

open_u8!(SignalIdentifier {
    DISCOVER = 0x01,
    GET_CAPABILITIES = 0x02,
    SET_CONFIGURATION = 0x03,
    GET_CONFIGURATION = 0x04,
    RECONFIGURE = 0x05,
    OPEN = 0x06,
    START = 0x07,
    CLOSE = 0x08,
    SUSPEND = 0x09,
    ABORT = 0x0A,
    SECURITY_CONTROL = 0x0B,
    GET_ALL_CAPABILITIES = 0x0C,
    DELAY_REPORT = 0x0D,
});

open_u8!(ErrorCode {
    BAD_HEADER_FORMAT = 0x01,
    BAD_LENGTH = 0x11,
    BAD_ACP_SEID = 0x12,
    SEP_IN_USE = 0x13,
    SEP_NOT_IN_USE = 0x14,
    BAD_SERVICE_CATEGORY = 0x17,
    BAD_PAYLOAD_FORMAT = 0x18,
    NOT_SUPPORTED_COMMAND = 0x19,
    INVALID_CAPABILITIES = 0x1A,
    BAD_RECOVERY_TYPE = 0x22,
    BAD_MEDIA_TRANSPORT_FORMAT = 0x23,
    BAD_RECOVERY_FORMAT = 0x25,
    BAD_ROHC_FORMAT = 0x26,
    BAD_CP_FORMAT = 0x27,
    BAD_MULTIPLEXING_FORMAT = 0x28,
    UNSUPPORTED_CONFIGURATION = 0x29,
    BAD_STATE = 0x31,
});

open_u8!(MediaType {
    AUDIO = 0x00,
    VIDEO = 0x01,
    MULTIMEDIA = 0x02,
});

open_u8!(StreamEndpointType {
    SOURCE = 0x00,
    SINK = 0x01,
});

open_u8!(ServiceCategory {
    MEDIA_TRANSPORT = 0x01,
    REPORTING = 0x02,
    RECOVERY = 0x03,
    CONTENT_PROTECTION = 0x04,
    HEADER_COMPRESSION = 0x05,
    MULTIPLEXING = 0x06,
    MEDIA_CODEC = 0x07,
    DELAY_REPORTING = 0x08,
});

open_u8!(State {
    IDLE = 0x00,
    CONFIGURED = 0x01,
    OPEN = 0x02,
    STREAMING = 0x03,
    CLOSING = 0x04,
    ABORTING = 0x05,
});

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum MessageType {
    Command = 0,
    GeneralReject = 1,
    ResponseAccept = 2,
    ResponseReject = 3,
}

impl MessageType {
    fn from_bits(value: u8) -> Self {
        match value & 3 {
            0 => Self::Command,
            1 => Self::GeneralReject,
            2 => Self::ResponseAccept,
            _ => Self::ResponseReject,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum PacketType {
    Single = 0,
    Start = 1,
    Continue = 2,
    End = 3,
}

impl PacketType {
    fn from_bits(value: u8) -> Self {
        match value & 3 {
            0 => Self::Single,
            1 => Self::Start,
            2 => Self::Continue,
            _ => Self::End,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EndpointInfo {
    pub seid: u8,
    pub in_use: bool,
    pub media_type: MediaType,
    pub endpoint_type: StreamEndpointType,
}

impl EndpointInfo {
    pub fn to_bytes(&self) -> [u8; 2] {
        [
            (self.seid << 2) | (u8::from(self.in_use) << 1),
            (self.media_type.0 << 4) | (self.endpoint_type.0 << 3),
        ]
    }

    pub fn from_bytes(payload: &[u8]) -> Result<Self> {
        if payload.len() < 2 {
            return Err(Error::Truncated("endpoint descriptor"));
        }
        Ok(Self {
            seid: payload[0] >> 2,
            in_use: payload[0] & 0x02 != 0,
            media_type: MediaType(payload[1] >> 4),
            endpoint_type: StreamEndpointType((payload[1] >> 3) & 1),
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ServiceCapabilities {
    MediaCodec {
        media_type: MediaType,
        media_codec_type: u8,
        media_codec_information: Vec<u8>,
    },
    Other {
        category: ServiceCategory,
        data: Vec<u8>,
    },
}

impl ServiceCapabilities {
    pub fn empty(category: ServiceCategory) -> Self {
        Self::Other {
            category,
            data: Vec::new(),
        }
    }

    pub fn category(&self) -> ServiceCategory {
        match self {
            Self::MediaCodec { .. } => ServiceCategory::MEDIA_CODEC,
            Self::Other { category, .. } => *category,
        }
    }

    pub fn data(&self) -> Vec<u8> {
        match self {
            Self::MediaCodec {
                media_type,
                media_codec_type,
                media_codec_information,
            } => {
                let mut data = vec![media_type.0, *media_codec_type];
                data.extend_from_slice(media_codec_information);
                data
            }
            Self::Other { data, .. } => data.clone(),
        }
    }

    pub fn parse_all(payload: &[u8]) -> Result<Vec<Self>> {
        let mut capabilities = Vec::new();
        let mut offset = 0;
        while offset < payload.len() {
            if payload.len() - offset < 2 {
                return Err(Error::Truncated("capability header"));
            }
            let category = ServiceCategory(payload[offset]);
            let length = payload[offset + 1] as usize;
            offset += 2;
            let end = offset.checked_add(length).ok_or(Error::CapabilityLength)?;
            let data = payload.get(offset..end).ok_or(Error::CapabilityLength)?;
            let capability = if category == ServiceCategory::MEDIA_CODEC {
                if data.len() < 2 {
                    return Err(Error::Truncated("media codec capability"));
                }
                Self::MediaCodec {
                    media_type: MediaType(data[0]),
                    media_codec_type: data[1],
                    media_codec_information: data[2..].to_vec(),
                }
            } else {
                Self::Other {
                    category,
                    data: data.to_vec(),
                }
            };
            capabilities.push(capability);
            offset = end;
        }
        Ok(capabilities)
    }

    pub fn serialize_all(capabilities: &[Self]) -> Result<Vec<u8>> {
        let mut payload = Vec::new();
        for capability in capabilities {
            let data = capability.data();
            let length = u8::try_from(data.len()).map_err(|_| Error::CapabilityLength)?;
            payload.extend_from_slice(&[capability.category().0, length]);
            payload.extend_from_slice(&data);
        }
        Ok(payload)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Message {
    DiscoverCommand,
    DiscoverResponse {
        endpoints: Vec<EndpointInfo>,
    },
    GetCapabilitiesCommand {
        acp_seid: u8,
    },
    GetCapabilitiesResponse {
        capabilities: Vec<ServiceCapabilities>,
    },
    GetCapabilitiesReject {
        error_code: ErrorCode,
    },
    GetAllCapabilitiesCommand {
        acp_seid: u8,
    },
    GetAllCapabilitiesResponse {
        capabilities: Vec<ServiceCapabilities>,
    },
    GetAllCapabilitiesReject {
        error_code: ErrorCode,
    },
    SetConfigurationCommand {
        acp_seid: u8,
        int_seid: u8,
        capabilities: Vec<ServiceCapabilities>,
    },
    SetConfigurationResponse,
    SetConfigurationReject {
        service_category: ServiceCategory,
        error_code: ErrorCode,
    },
    GetConfigurationCommand {
        acp_seid: u8,
    },
    GetConfigurationResponse {
        capabilities: Vec<ServiceCapabilities>,
    },
    GetConfigurationReject {
        error_code: ErrorCode,
    },
    ReconfigureCommand {
        acp_seid: u8,
        capabilities: Vec<ServiceCapabilities>,
    },
    ReconfigureResponse,
    ReconfigureReject {
        service_category: ServiceCategory,
        error_code: ErrorCode,
    },
    OpenCommand {
        acp_seid: u8,
    },
    OpenResponse,
    OpenReject {
        error_code: ErrorCode,
    },
    StartCommand {
        acp_seids: Vec<u8>,
    },
    StartResponse,
    StartReject {
        acp_seid: u8,
        error_code: ErrorCode,
    },
    CloseCommand {
        acp_seid: u8,
    },
    CloseResponse,
    CloseReject {
        error_code: ErrorCode,
    },
    SuspendCommand {
        acp_seids: Vec<u8>,
    },
    SuspendResponse,
    SuspendReject {
        acp_seid: u8,
        error_code: ErrorCode,
    },
    AbortCommand {
        acp_seid: u8,
    },
    AbortResponse,
    SecurityControlCommand {
        acp_seid: u8,
        data: Vec<u8>,
    },
    SecurityControlResponse,
    SecurityControlReject {
        error_code: ErrorCode,
    },
    GeneralReject,
    DelayReportCommand {
        acp_seid: u8,
        delay: u16,
    },
    DelayReportResponse,
    DelayReportReject {
        error_code: ErrorCode,
    },
    Unknown {
        signal_identifier: SignalIdentifier,
        message_type: MessageType,
        payload: Vec<u8>,
    },
}

fn seid(seid: u8) -> u8 {
    seid << 2
}

fn parse_seid(payload: &[u8]) -> Result<u8> {
    payload
        .first()
        .map(|value| value >> 2)
        .ok_or(Error::Truncated("SEID"))
}

fn parse_error(payload: &[u8]) -> Result<ErrorCode> {
    payload
        .first()
        .copied()
        .map(ErrorCode)
        .ok_or(Error::Truncated("error code"))
}

impl Message {
    pub fn signal_identifier(&self) -> SignalIdentifier {
        use Message::*;
        match self {
            DiscoverCommand | DiscoverResponse { .. } => SignalIdentifier::DISCOVER,
            GetCapabilitiesCommand { .. }
            | GetCapabilitiesResponse { .. }
            | GetCapabilitiesReject { .. } => SignalIdentifier::GET_CAPABILITIES,
            GetAllCapabilitiesCommand { .. }
            | GetAllCapabilitiesResponse { .. }
            | GetAllCapabilitiesReject { .. } => SignalIdentifier::GET_ALL_CAPABILITIES,
            SetConfigurationCommand { .. }
            | SetConfigurationResponse
            | SetConfigurationReject { .. } => SignalIdentifier::SET_CONFIGURATION,
            GetConfigurationCommand { .. }
            | GetConfigurationResponse { .. }
            | GetConfigurationReject { .. } => SignalIdentifier::GET_CONFIGURATION,
            ReconfigureCommand { .. } | ReconfigureResponse | ReconfigureReject { .. } => {
                SignalIdentifier::RECONFIGURE
            }
            OpenCommand { .. } | OpenResponse | OpenReject { .. } => SignalIdentifier::OPEN,
            StartCommand { .. } | StartResponse | StartReject { .. } => SignalIdentifier::START,
            CloseCommand { .. } | CloseResponse | CloseReject { .. } => SignalIdentifier::CLOSE,
            SuspendCommand { .. } | SuspendResponse | SuspendReject { .. } => {
                SignalIdentifier::SUSPEND
            }
            AbortCommand { .. } | AbortResponse => SignalIdentifier::ABORT,
            SecurityControlCommand { .. }
            | SecurityControlResponse
            | SecurityControlReject { .. } => SignalIdentifier::SECURITY_CONTROL,
            GeneralReject => SignalIdentifier(0),
            DelayReportCommand { .. } | DelayReportResponse | DelayReportReject { .. } => {
                SignalIdentifier::DELAY_REPORT
            }
            Unknown {
                signal_identifier, ..
            } => *signal_identifier,
        }
    }

    pub fn message_type(&self) -> MessageType {
        use Message::*;
        match self {
            DiscoverCommand
            | GetCapabilitiesCommand { .. }
            | GetAllCapabilitiesCommand { .. }
            | SetConfigurationCommand { .. }
            | GetConfigurationCommand { .. }
            | ReconfigureCommand { .. }
            | OpenCommand { .. }
            | StartCommand { .. }
            | CloseCommand { .. }
            | SuspendCommand { .. }
            | AbortCommand { .. }
            | SecurityControlCommand { .. }
            | DelayReportCommand { .. } => MessageType::Command,
            GeneralReject => MessageType::GeneralReject,
            DiscoverResponse { .. }
            | GetCapabilitiesResponse { .. }
            | GetAllCapabilitiesResponse { .. }
            | SetConfigurationResponse
            | GetConfigurationResponse { .. }
            | ReconfigureResponse
            | OpenResponse
            | StartResponse
            | CloseResponse
            | SuspendResponse
            | AbortResponse
            | SecurityControlResponse
            | DelayReportResponse => MessageType::ResponseAccept,
            GetCapabilitiesReject { .. }
            | GetAllCapabilitiesReject { .. }
            | SetConfigurationReject { .. }
            | GetConfigurationReject { .. }
            | ReconfigureReject { .. }
            | OpenReject { .. }
            | StartReject { .. }
            | CloseReject { .. }
            | SuspendReject { .. }
            | SecurityControlReject { .. }
            | DelayReportReject { .. } => MessageType::ResponseReject,
            Unknown { message_type, .. } => *message_type,
        }
    }

    pub fn payload(&self) -> Result<Vec<u8>> {
        use Message::*;
        let payload = match self {
            DiscoverCommand
            | SetConfigurationResponse
            | ReconfigureResponse
            | OpenResponse
            | StartResponse
            | CloseResponse
            | SuspendResponse
            | AbortResponse
            | SecurityControlResponse
            | GeneralReject
            | DelayReportResponse => Vec::new(),
            DiscoverResponse { endpoints } => {
                endpoints.iter().flat_map(EndpointInfo::to_bytes).collect()
            }
            GetCapabilitiesCommand { acp_seid }
            | GetAllCapabilitiesCommand { acp_seid }
            | GetConfigurationCommand { acp_seid }
            | OpenCommand { acp_seid }
            | CloseCommand { acp_seid }
            | AbortCommand { acp_seid } => vec![seid(*acp_seid)],
            GetCapabilitiesResponse { capabilities }
            | GetAllCapabilitiesResponse { capabilities }
            | GetConfigurationResponse { capabilities } => {
                ServiceCapabilities::serialize_all(capabilities)?
            }
            GetCapabilitiesReject { error_code }
            | GetAllCapabilitiesReject { error_code }
            | GetConfigurationReject { error_code }
            | OpenReject { error_code }
            | CloseReject { error_code }
            | SecurityControlReject { error_code }
            | DelayReportReject { error_code } => vec![error_code.0],
            SetConfigurationCommand {
                acp_seid,
                int_seid,
                capabilities,
            } => {
                let mut bytes = vec![seid(*acp_seid), seid(*int_seid)];
                bytes.extend(ServiceCapabilities::serialize_all(capabilities)?);
                bytes
            }
            SetConfigurationReject {
                service_category,
                error_code,
            }
            | ReconfigureReject {
                service_category,
                error_code,
            } => vec![service_category.0, error_code.0],
            ReconfigureCommand {
                acp_seid,
                capabilities,
            } => {
                let mut bytes = vec![seid(*acp_seid)];
                bytes.extend(ServiceCapabilities::serialize_all(capabilities)?);
                bytes
            }
            StartCommand { acp_seids } | SuspendCommand { acp_seids } => {
                acp_seids.iter().map(|value| seid(*value)).collect()
            }
            StartReject {
                acp_seid,
                error_code,
            }
            | SuspendReject {
                acp_seid,
                error_code,
            } => vec![seid(*acp_seid), error_code.0],
            SecurityControlCommand { acp_seid, data } => {
                let mut bytes = vec![seid(*acp_seid)];
                bytes.extend_from_slice(data);
                bytes
            }
            DelayReportCommand { acp_seid, delay } => {
                vec![seid(*acp_seid), (delay >> 8) as u8, *delay as u8]
            }
            Unknown { payload, .. } => payload.clone(),
        };
        Ok(payload)
    }

    pub fn parse(
        signal: SignalIdentifier,
        message_type: MessageType,
        payload: &[u8],
    ) -> Result<Self> {
        use Message::*;
        let capabilities = || ServiceCapabilities::parse_all(payload);
        let message = match (signal, message_type) {
            (SignalIdentifier::DISCOVER, MessageType::Command) => DiscoverCommand,
            (SignalIdentifier::DISCOVER, MessageType::ResponseAccept) => {
                if !payload.len().is_multiple_of(2) {
                    return Err(Error::Invalid("endpoint list length"));
                }
                DiscoverResponse {
                    endpoints: payload
                        .chunks_exact(2)
                        .map(EndpointInfo::from_bytes)
                        .collect::<Result<_>>()?,
                }
            }
            (SignalIdentifier::GET_CAPABILITIES, MessageType::Command) => GetCapabilitiesCommand {
                acp_seid: parse_seid(payload)?,
            },
            (SignalIdentifier::GET_CAPABILITIES, MessageType::ResponseAccept) => {
                GetCapabilitiesResponse {
                    capabilities: capabilities()?,
                }
            }
            (SignalIdentifier::GET_CAPABILITIES, MessageType::ResponseReject) => {
                GetCapabilitiesReject {
                    error_code: parse_error(payload)?,
                }
            }
            (SignalIdentifier::GET_ALL_CAPABILITIES, MessageType::Command) => {
                GetAllCapabilitiesCommand {
                    acp_seid: parse_seid(payload)?,
                }
            }
            (SignalIdentifier::GET_ALL_CAPABILITIES, MessageType::ResponseAccept) => {
                GetAllCapabilitiesResponse {
                    capabilities: capabilities()?,
                }
            }
            (SignalIdentifier::GET_ALL_CAPABILITIES, MessageType::ResponseReject) => {
                GetAllCapabilitiesReject {
                    error_code: parse_error(payload)?,
                }
            }
            (SignalIdentifier::SET_CONFIGURATION, MessageType::Command) => {
                if payload.len() < 2 {
                    return Err(Error::Truncated("set configuration"));
                }
                SetConfigurationCommand {
                    acp_seid: payload[0] >> 2,
                    int_seid: payload[1] >> 2,
                    capabilities: ServiceCapabilities::parse_all(&payload[2..])?,
                }
            }
            (SignalIdentifier::SET_CONFIGURATION, MessageType::ResponseAccept) => {
                SetConfigurationResponse
            }
            (SignalIdentifier::SET_CONFIGURATION, MessageType::ResponseReject) => {
                if payload.len() < 2 {
                    return Err(Error::Truncated("set configuration reject"));
                }
                SetConfigurationReject {
                    service_category: ServiceCategory(payload[0]),
                    error_code: ErrorCode(payload[1]),
                }
            }
            (SignalIdentifier::GET_CONFIGURATION, MessageType::Command) => {
                GetConfigurationCommand {
                    acp_seid: parse_seid(payload)?,
                }
            }
            (SignalIdentifier::GET_CONFIGURATION, MessageType::ResponseAccept) => {
                GetConfigurationResponse {
                    capabilities: capabilities()?,
                }
            }
            (SignalIdentifier::GET_CONFIGURATION, MessageType::ResponseReject) => {
                GetConfigurationReject {
                    error_code: parse_error(payload)?,
                }
            }
            (SignalIdentifier::RECONFIGURE, MessageType::Command) => {
                let acp_seid = parse_seid(payload)?;
                ReconfigureCommand {
                    acp_seid,
                    capabilities: ServiceCapabilities::parse_all(&payload[1..])?,
                }
            }
            (SignalIdentifier::RECONFIGURE, MessageType::ResponseAccept) => ReconfigureResponse,
            (SignalIdentifier::RECONFIGURE, MessageType::ResponseReject) => {
                if payload.len() < 2 {
                    return Err(Error::Truncated("reconfigure reject"));
                }
                ReconfigureReject {
                    service_category: ServiceCategory(payload[0]),
                    error_code: ErrorCode(payload[1]),
                }
            }
            (SignalIdentifier::OPEN, MessageType::Command) => OpenCommand {
                acp_seid: parse_seid(payload)?,
            },
            (SignalIdentifier::OPEN, MessageType::ResponseAccept) => OpenResponse,
            (SignalIdentifier::OPEN, MessageType::ResponseReject) => OpenReject {
                error_code: parse_error(payload)?,
            },
            (SignalIdentifier::START, MessageType::Command) => StartCommand {
                acp_seids: payload.iter().map(|value| value >> 2).collect(),
            },
            (SignalIdentifier::START, MessageType::ResponseAccept) => StartResponse,
            (SignalIdentifier::START, MessageType::ResponseReject) => {
                if payload.len() < 2 {
                    return Err(Error::Truncated("start reject"));
                }
                StartReject {
                    acp_seid: payload[0] >> 2,
                    error_code: ErrorCode(payload[1]),
                }
            }
            (SignalIdentifier::CLOSE, MessageType::Command) => CloseCommand {
                acp_seid: parse_seid(payload)?,
            },
            (SignalIdentifier::CLOSE, MessageType::ResponseAccept) => CloseResponse,
            (SignalIdentifier::CLOSE, MessageType::ResponseReject) => CloseReject {
                error_code: parse_error(payload)?,
            },
            (SignalIdentifier::SUSPEND, MessageType::Command) => SuspendCommand {
                acp_seids: payload.iter().map(|value| value >> 2).collect(),
            },
            (SignalIdentifier::SUSPEND, MessageType::ResponseAccept) => SuspendResponse,
            (SignalIdentifier::SUSPEND, MessageType::ResponseReject) => {
                if payload.len() < 2 {
                    return Err(Error::Truncated("suspend reject"));
                }
                SuspendReject {
                    acp_seid: payload[0] >> 2,
                    error_code: ErrorCode(payload[1]),
                }
            }
            (SignalIdentifier::ABORT, MessageType::Command) => AbortCommand {
                acp_seid: parse_seid(payload)?,
            },
            (SignalIdentifier::ABORT, MessageType::ResponseAccept) => AbortResponse,
            (SignalIdentifier::SECURITY_CONTROL, MessageType::Command) => {
                let acp_seid = parse_seid(payload)?;
                SecurityControlCommand {
                    acp_seid,
                    data: payload[1..].to_vec(),
                }
            }
            (SignalIdentifier::SECURITY_CONTROL, MessageType::ResponseAccept) => {
                SecurityControlResponse
            }
            (SignalIdentifier::SECURITY_CONTROL, MessageType::ResponseReject) => {
                SecurityControlReject {
                    error_code: parse_error(payload)?,
                }
            }
            (SignalIdentifier::DELAY_REPORT, MessageType::Command) => {
                if payload.len() < 3 {
                    return Err(Error::Truncated("delay report"));
                }
                DelayReportCommand {
                    acp_seid: payload[0] >> 2,
                    delay: u16::from_be_bytes([payload[1], payload[2]]),
                }
            }
            (SignalIdentifier::DELAY_REPORT, MessageType::ResponseAccept) => DelayReportResponse,
            (SignalIdentifier::DELAY_REPORT, MessageType::ResponseReject) => DelayReportReject {
                error_code: parse_error(payload)?,
            },
            (_, MessageType::GeneralReject) => GeneralReject,
            _ => Unknown {
                signal_identifier: signal,
                message_type,
                payload: payload.to_vec(),
            },
        };
        Ok(message)
    }

    pub fn encode_pdus(&self, transaction_label: u8, mtu: usize) -> Result<Vec<Vec<u8>>> {
        if transaction_label > 0x0F || mtu < 3 {
            return Err(Error::Invalid("transaction label or MTU"));
        }
        let payload = self.payload()?;
        let message_type = self.message_type() as u8;
        let signal = self.signal_identifier().0 & 0x3F;
        if payload.len() + 2 <= mtu {
            let mut pdu = vec![(transaction_label << 4) | message_type, signal];
            pdu.extend(payload);
            return Ok(vec![pdu]);
        }
        let start_capacity = mtu - 3;
        let continuation_capacity = mtu - 1;
        if start_capacity == 0 || continuation_capacity == 0 {
            return Err(Error::Invalid("MTU"));
        }
        let remainder = payload.len() - start_capacity;
        let continuation_count = remainder.div_ceil(continuation_capacity);
        let packet_count = 1usize + continuation_count;
        let packet_count_u8 = u8::try_from(packet_count).map_err(|_| Error::PayloadTooLong)?;
        let mut pdus = Vec::with_capacity(packet_count);
        let mut start = vec![
            (transaction_label << 4) | ((PacketType::Start as u8) << 2) | message_type,
            signal,
            packet_count_u8,
        ];
        start.extend_from_slice(&payload[..start_capacity]);
        pdus.push(start);
        let chunks: Vec<_> = payload[start_capacity..]
            .chunks(continuation_capacity)
            .collect();
        for (index, chunk) in chunks.iter().enumerate() {
            let packet_type = if index + 1 == chunks.len() {
                PacketType::End
            } else {
                PacketType::Continue
            };
            let mut pdu =
                vec![(transaction_label << 4) | ((packet_type as u8) << 2) | message_type];
            pdu.extend_from_slice(chunk);
            pdus.push(pdu);
        }
        Ok(pdus)
    }
}

#[derive(Debug, Default)]
pub struct MessageAssembler {
    pending: Option<PendingMessage>,
}

#[derive(Debug)]
struct PendingMessage {
    transaction_label: u8,
    message_type: MessageType,
    signal_identifier: SignalIdentifier,
    expected_packets: u8,
    received_packets: u8,
    payload: Vec<u8>,
}

impl MessageAssembler {
    pub fn push(&mut self, pdu: &[u8]) -> Result<Option<(u8, Message)>> {
        let Some(header) = pdu.first().copied() else {
            return Ok(None);
        };
        let transaction_label = header >> 4;
        let packet_type = PacketType::from_bits(header >> 2);
        let message_type = MessageType::from_bits(header);
        match packet_type {
            PacketType::Single => {
                if pdu.len() < 2 {
                    return Ok(None);
                }
                self.pending = None;
                let signal = SignalIdentifier(pdu[1] & 0x3F);
                Ok(Some((
                    transaction_label,
                    Message::parse(signal, message_type, &pdu[2..])?,
                )))
            }
            PacketType::Start => {
                if pdu.len() < 3 || pdu[2] < 2 {
                    return Ok(None);
                }
                self.pending = Some(PendingMessage {
                    transaction_label,
                    message_type,
                    signal_identifier: SignalIdentifier(pdu[1] & 0x3F),
                    expected_packets: pdu[2],
                    received_packets: 1,
                    payload: pdu[3..].to_vec(),
                });
                Ok(None)
            }
            PacketType::Continue | PacketType::End => {
                let Some(pending) = self.pending.as_mut() else {
                    return Ok(None);
                };
                if pending.transaction_label != transaction_label
                    || pending.message_type != message_type
                {
                    return Ok(None);
                }
                pending.received_packets = pending.received_packets.saturating_add(1);
                pending.payload.extend_from_slice(&pdu[1..]);
                if pending.received_packets > pending.expected_packets {
                    self.pending = None;
                    return Ok(None);
                }
                if packet_type == PacketType::End {
                    let pending = self.pending.take().expect("pending message exists");
                    if pending.received_packets != pending.expected_packets {
                        return Ok(None);
                    }
                    let message = Message::parse(
                        pending.signal_identifier,
                        pending.message_type,
                        &pending.payload,
                    )?;
                    Ok(Some((transaction_label, message)))
                } else {
                    Ok(None)
                }
            }
        }
    }
}
