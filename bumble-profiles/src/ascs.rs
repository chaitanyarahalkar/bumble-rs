//! Audio Stream Control Service (ASCS) operations and ASE state machines.

use crate::bap::CodecSpecificConfiguration;
use crate::le_audio::Metadata;
use crate::{discover_profile, require_characteristic, uuid, Error, Result};
use bumble_gatt::{
    permissions, properties, AttTransport, CharacteristicDefinition, CharacteristicProxy,
    DynamicValue, GattClient, GattServer, ServiceDefinition, ServiceProxy,
};
use bumble_hci::CodingFormat;
use std::collections::{BTreeMap, VecDeque};
use std::sync::{Arc, Mutex};

pub const AUDIO_STREAM_CONTROL_SERVICE: u16 = 0x184E;
pub const SINK_ASE_CHARACTERISTIC: u16 = 0x2BC4;
pub const SOURCE_ASE_CHARACTERISTIC: u16 = 0x2BC5;
pub const ASE_CONTROL_POINT_CHARACTERISTIC: u16 = 0x2BC6;

const INVALID_ATTRIBUTE_VALUE_LENGTH: u8 = 0x0D;
const UNLIKELY_ERROR: u8 = 0x0E;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum AseOpcode {
    ConfigCodec = 0x01,
    ConfigQos = 0x02,
    Enable = 0x03,
    ReceiverStartReady = 0x04,
    Disable = 0x05,
    ReceiverStopReady = 0x06,
    UpdateMetadata = 0x07,
    Release = 0x08,
}

impl TryFrom<u8> for AseOpcode {
    type Error = Error;

    fn try_from(value: u8) -> Result<Self> {
        match value {
            0x01 => Ok(Self::ConfigCodec),
            0x02 => Ok(Self::ConfigQos),
            0x03 => Ok(Self::Enable),
            0x04 => Ok(Self::ReceiverStartReady),
            0x05 => Ok(Self::Disable),
            0x06 => Ok(Self::ReceiverStopReady),
            0x07 => Ok(Self::UpdateMetadata),
            0x08 => Ok(Self::Release),
            _ => Err(Error::InvalidValue(format!(
                "unknown ASE opcode 0x{value:02X}"
            ))),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConfigCodecParameters {
    pub ase_id: u8,
    pub target_latency: u8,
    pub target_phy: u8,
    pub codec_id: CodingFormat,
    pub codec_specific_configuration: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConfigQosParameters {
    pub ase_id: u8,
    pub cig_id: u8,
    pub cis_id: u8,
    pub sdu_interval: u32,
    pub framing: u8,
    pub phy: u8,
    pub max_sdu: u16,
    pub retransmission_number: u8,
    pub max_transport_latency: u16,
    pub presentation_delay: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AseMetadataParameters {
    pub ase_id: u8,
    pub metadata: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AseOperation {
    ConfigCodec(Vec<ConfigCodecParameters>),
    ConfigQos(Vec<ConfigQosParameters>),
    Enable(Vec<AseMetadataParameters>),
    ReceiverStartReady(Vec<u8>),
    Disable(Vec<u8>),
    ReceiverStopReady(Vec<u8>),
    UpdateMetadata(Vec<AseMetadataParameters>),
    Release(Vec<u8>),
}

impl AseOperation {
    pub fn opcode(&self) -> AseOpcode {
        match self {
            Self::ConfigCodec(_) => AseOpcode::ConfigCodec,
            Self::ConfigQos(_) => AseOpcode::ConfigQos,
            Self::Enable(_) => AseOpcode::Enable,
            Self::ReceiverStartReady(_) => AseOpcode::ReceiverStartReady,
            Self::Disable(_) => AseOpcode::Disable,
            Self::ReceiverStopReady(_) => AseOpcode::ReceiverStopReady,
            Self::UpdateMetadata(_) => AseOpcode::UpdateMetadata,
            Self::Release(_) => AseOpcode::Release,
        }
    }

    pub fn from_bytes(data: &[u8]) -> Result<Self> {
        let mut reader = Reader::new(data);
        let opcode = AseOpcode::try_from(reader.u8("ASE opcode")?)?;
        let count = reader.u8("ASE count")?;
        let operation = match opcode {
            AseOpcode::ConfigCodec => {
                let mut parameters = Vec::with_capacity(usize::from(count));
                for _ in 0..count {
                    parameters.push(ConfigCodecParameters {
                        ase_id: reader.u8("ASE ID")?,
                        target_latency: reader.u8("target latency")?,
                        target_phy: reader.u8("target PHY")?,
                        codec_id: reader.coding_format()?,
                        codec_specific_configuration: reader
                            .length_prefixed("codec-specific configuration")?
                            .to_vec(),
                    });
                }
                Self::ConfigCodec(parameters)
            }
            AseOpcode::ConfigQos => {
                let mut parameters = Vec::with_capacity(usize::from(count));
                for _ in 0..count {
                    parameters.push(ConfigQosParameters {
                        ase_id: reader.u8("ASE ID")?,
                        cig_id: reader.u8("CIG ID")?,
                        cis_id: reader.u8("CIS ID")?,
                        sdu_interval: reader.u24("SDU interval")?,
                        framing: reader.u8("framing")?,
                        phy: reader.u8("PHY")?,
                        max_sdu: reader.u16("maximum SDU")?,
                        retransmission_number: reader.u8("retransmission number")?,
                        max_transport_latency: reader.u16("maximum transport latency")?,
                        presentation_delay: reader.u24("presentation delay")?,
                    });
                }
                Self::ConfigQos(parameters)
            }
            AseOpcode::Enable | AseOpcode::UpdateMetadata => {
                let mut parameters = Vec::with_capacity(usize::from(count));
                for _ in 0..count {
                    parameters.push(AseMetadataParameters {
                        ase_id: reader.u8("ASE ID")?,
                        metadata: reader.length_prefixed("ASE metadata")?.to_vec(),
                    });
                }
                if opcode == AseOpcode::Enable {
                    Self::Enable(parameters)
                } else {
                    Self::UpdateMetadata(parameters)
                }
            }
            AseOpcode::ReceiverStartReady
            | AseOpcode::Disable
            | AseOpcode::ReceiverStopReady
            | AseOpcode::Release => {
                let mut ase_ids = Vec::with_capacity(usize::from(count));
                for _ in 0..count {
                    ase_ids.push(reader.u8("ASE ID")?);
                }
                match opcode {
                    AseOpcode::ReceiverStartReady => Self::ReceiverStartReady(ase_ids),
                    AseOpcode::Disable => Self::Disable(ase_ids),
                    AseOpcode::ReceiverStopReady => Self::ReceiverStopReady(ase_ids),
                    AseOpcode::Release => Self::Release(ase_ids),
                    _ => unreachable!(),
                }
            }
        };
        reader.finish("ASE operation")?;
        Ok(operation)
    }

    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        let count = match self {
            Self::ConfigCodec(parameters) => parameters.len(),
            Self::ConfigQos(parameters) => parameters.len(),
            Self::Enable(parameters) | Self::UpdateMetadata(parameters) => parameters.len(),
            Self::ReceiverStartReady(ase_ids)
            | Self::Disable(ase_ids)
            | Self::ReceiverStopReady(ase_ids)
            | Self::Release(ase_ids) => ase_ids.len(),
        };
        let count = u8::try_from(count)
            .map_err(|_| Error::InvalidValue("ASE operation has over 255 entries".into()))?;
        let mut value = vec![self.opcode() as u8, count];
        match self {
            Self::ConfigCodec(parameters) => {
                for parameter in parameters {
                    value.extend_from_slice(&[
                        parameter.ase_id,
                        parameter.target_latency,
                        parameter.target_phy,
                    ]);
                    value.extend_from_slice(&parameter.codec_id.to_bytes());
                    push_length_prefixed(
                        &mut value,
                        &parameter.codec_specific_configuration,
                        "codec-specific configuration",
                    )?;
                }
            }
            Self::ConfigQos(parameters) => {
                for parameter in parameters {
                    require_u24("SDU interval", parameter.sdu_interval)?;
                    require_u24("presentation delay", parameter.presentation_delay)?;
                    value.extend_from_slice(&[
                        parameter.ase_id,
                        parameter.cig_id,
                        parameter.cis_id,
                    ]);
                    value.extend_from_slice(&u24_bytes(parameter.sdu_interval));
                    value.extend_from_slice(&[parameter.framing, parameter.phy]);
                    value.extend_from_slice(&parameter.max_sdu.to_le_bytes());
                    value.push(parameter.retransmission_number);
                    value.extend_from_slice(&parameter.max_transport_latency.to_le_bytes());
                    value.extend_from_slice(&u24_bytes(parameter.presentation_delay));
                }
            }
            Self::Enable(parameters) | Self::UpdateMetadata(parameters) => {
                for parameter in parameters {
                    value.push(parameter.ase_id);
                    push_length_prefixed(&mut value, &parameter.metadata, "ASE metadata")?;
                }
            }
            Self::ReceiverStartReady(ase_ids)
            | Self::Disable(ase_ids)
            | Self::ReceiverStopReady(ase_ids)
            | Self::Release(ase_ids) => value.extend_from_slice(ase_ids),
        }
        Ok(value)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AseResponseCode(pub u8);

impl AseResponseCode {
    pub const SUCCESS: Self = Self(0x00);
    pub const UNSUPPORTED_OPCODE: Self = Self(0x01);
    pub const INVALID_LENGTH: Self = Self(0x02);
    pub const INVALID_ASE_ID: Self = Self(0x03);
    pub const INVALID_ASE_STATE_MACHINE_TRANSITION: Self = Self(0x04);
    pub const INVALID_ASE_DIRECTION: Self = Self(0x05);
    pub const UNSUPPORTED_AUDIO_CAPABILITIES: Self = Self(0x06);
    pub const UNSUPPORTED_CONFIGURATION_PARAMETER_VALUE: Self = Self(0x07);
    pub const REJECTED_CONFIGURATION_PARAMETER_VALUE: Self = Self(0x08);
    pub const INVALID_CONFIGURATION_PARAMETER_VALUE: Self = Self(0x09);
    pub const UNSUPPORTED_METADATA: Self = Self(0x0A);
    pub const REJECTED_METADATA: Self = Self(0x0B);
    pub const INVALID_METADATA: Self = Self(0x0C);
    pub const INSUFFICIENT_RESOURCES: Self = Self(0x0D);
    pub const UNSPECIFIED_ERROR: Self = Self(0x0E);
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AseReasonCode(pub u8);

impl AseReasonCode {
    pub const NONE: Self = Self(0x00);
    pub const CODEC_ID: Self = Self(0x01);
    pub const CODEC_SPECIFIC_CONFIGURATION: Self = Self(0x02);
    pub const SDU_INTERVAL: Self = Self(0x03);
    pub const FRAMING: Self = Self(0x04);
    pub const PHY: Self = Self(0x05);
    pub const MAXIMUM_SDU_SIZE: Self = Self(0x06);
    pub const RETRANSMISSION_NUMBER: Self = Self(0x07);
    pub const MAX_TRANSPORT_LATENCY: Self = Self(0x08);
    pub const PRESENTATION_DELAY: Self = Self(0x09);
    pub const INVALID_ASE_CIS_MAPPING: Self = Self(0x0A);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AseResponse {
    pub ase_id: u8,
    pub code: AseResponseCode,
    pub reason: AseReasonCode,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AseControlResponse {
    pub opcode: AseOpcode,
    pub responses: Vec<AseResponse>,
}

impl AseControlResponse {
    pub fn from_bytes(data: &[u8]) -> Result<Self> {
        let mut reader = Reader::new(data);
        let opcode = AseOpcode::try_from(reader.u8("response opcode")?)?;
        let count = reader.u8("response count")?;
        let mut responses = Vec::with_capacity(usize::from(count));
        for _ in 0..count {
            responses.push(AseResponse {
                ase_id: reader.u8("response ASE ID")?,
                code: AseResponseCode(reader.u8("response code")?),
                reason: AseReasonCode(reader.u8("response reason")?),
            });
        }
        reader.finish("ASE control response")?;
        Ok(Self { opcode, responses })
    }

    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        let count = u8::try_from(self.responses.len())
            .map_err(|_| Error::InvalidValue("ASE response has over 255 entries".into()))?;
        let mut value = vec![self.opcode as u8, count];
        for response in &self.responses {
            value.extend_from_slice(&[response.ase_id, response.code.0, response.reason.0]);
        }
        Ok(value)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AudioRole {
    Sink,
    Source,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AseState(pub u8);

impl AseState {
    pub const IDLE: Self = Self(0x00);
    pub const CODEC_CONFIGURED: Self = Self(0x01);
    pub const QOS_CONFIGURED: Self = Self(0x02);
    pub const ENABLING: Self = Self(0x03);
    pub const STREAMING: Self = Self(0x04);
    pub const DISABLING: Self = Self(0x05);
    pub const RELEASING: Self = Self(0x06);
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AseQosConfiguration {
    pub cig_id: u8,
    pub cis_id: u8,
    pub sdu_interval: u32,
    pub framing: u8,
    pub phy: u8,
    pub max_sdu: u16,
    pub retransmission_number: u8,
    pub max_transport_latency: u16,
    pub presentation_delay: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AseEndpoint {
    pub ase_id: u8,
    pub role: AudioRole,
    pub state: AseState,
    pub preferred_framing: u8,
    pub preferred_phy: u8,
    pub preferred_retransmission_number: u8,
    pub preferred_max_transport_latency: u16,
    pub supported_presentation_delay_min: u32,
    pub supported_presentation_delay_max: u32,
    pub preferred_presentation_delay_min: u32,
    pub preferred_presentation_delay_max: u32,
    pub codec_id: CodingFormat,
    pub codec_specific_configuration: Vec<u8>,
    pub qos: AseQosConfiguration,
    pub metadata: Metadata,
}

impl AseEndpoint {
    pub fn new(ase_id: u8, role: AudioRole) -> Self {
        Self {
            ase_id,
            role,
            state: AseState::IDLE,
            preferred_framing: 0,
            preferred_phy: 0,
            preferred_retransmission_number: 13,
            preferred_max_transport_latency: 100,
            supported_presentation_delay_min: 0,
            supported_presentation_delay_max: 0,
            preferred_presentation_delay_min: 0,
            preferred_presentation_delay_max: 0,
            codec_id: CodingFormat {
                coding_format: 0x06,
                company_id: 0,
                vendor_specific_codec_id: 0,
            },
            codec_specific_configuration: Vec::new(),
            qos: AseQosConfiguration::default(),
            metadata: Metadata::default(),
        }
    }

    pub fn value(&self) -> Result<Vec<u8>> {
        let mut value = vec![self.ase_id, self.state.0];
        match self.state {
            AseState::CODEC_CONFIGURED => {
                for (name, delay) in [
                    (
                        "supported presentation delay minimum",
                        self.supported_presentation_delay_min,
                    ),
                    (
                        "supported presentation delay maximum",
                        self.supported_presentation_delay_max,
                    ),
                    (
                        "preferred presentation delay minimum",
                        self.preferred_presentation_delay_min,
                    ),
                    (
                        "preferred presentation delay maximum",
                        self.preferred_presentation_delay_max,
                    ),
                ] {
                    require_u24(name, delay)?;
                }
                value.extend_from_slice(&[
                    self.preferred_framing,
                    self.preferred_phy,
                    self.preferred_retransmission_number,
                ]);
                value.extend_from_slice(&self.preferred_max_transport_latency.to_le_bytes());
                value.extend_from_slice(&u24_bytes(self.supported_presentation_delay_min));
                value.extend_from_slice(&u24_bytes(self.supported_presentation_delay_max));
                value.extend_from_slice(&u24_bytes(self.preferred_presentation_delay_min));
                value.extend_from_slice(&u24_bytes(self.preferred_presentation_delay_max));
                value.extend_from_slice(&self.codec_id.to_bytes());
                push_length_prefixed(
                    &mut value,
                    &self.codec_specific_configuration,
                    "ASE codec configuration",
                )?;
            }
            AseState::QOS_CONFIGURED => {
                require_u24("SDU interval", self.qos.sdu_interval)?;
                require_u24("presentation delay", self.qos.presentation_delay)?;
                value.extend_from_slice(&[self.qos.cig_id, self.qos.cis_id]);
                value.extend_from_slice(&u24_bytes(self.qos.sdu_interval));
                value.extend_from_slice(&[self.qos.framing, self.qos.phy]);
                value.extend_from_slice(&self.qos.max_sdu.to_le_bytes());
                value.push(self.qos.retransmission_number);
                value.extend_from_slice(&self.qos.max_transport_latency.to_le_bytes());
                value.extend_from_slice(&u24_bytes(self.qos.presentation_delay));
            }
            AseState::ENABLING | AseState::STREAMING | AseState::DISABLING => {
                value.extend_from_slice(&[self.qos.cig_id, self.qos.cis_id]);
                push_length_prefixed(&mut value, &self.metadata.to_bytes()?, "ASE metadata")?;
            }
            AseState::IDLE | AseState::RELEASING => {}
            _ => {
                return Err(Error::InvalidValue(format!(
                    "unknown ASE state 0x{:02X}",
                    self.state.0
                )))
            }
        }
        Ok(value)
    }

    fn invalid_transition() -> (AseResponseCode, AseReasonCode) {
        (
            AseResponseCode::INVALID_ASE_STATE_MACHINE_TRANSITION,
            AseReasonCode::NONE,
        )
    }

    fn config_codec(
        &mut self,
        parameters: &ConfigCodecParameters,
    ) -> (AseResponseCode, AseReasonCode) {
        if ![
            AseState::IDLE,
            AseState::CODEC_CONFIGURED,
            AseState::QOS_CONFIGURED,
        ]
        .contains(&self.state)
        {
            return Self::invalid_transition();
        }
        if parameters.codec_id.coding_format != 0xFF
            && CodecSpecificConfiguration::from_bytes(&parameters.codec_specific_configuration)
                .is_err()
        {
            return (
                AseResponseCode::INVALID_CONFIGURATION_PARAMETER_VALUE,
                AseReasonCode::CODEC_SPECIFIC_CONFIGURATION,
            );
        }
        self.qos.max_transport_latency = u16::from(parameters.target_latency);
        self.qos.phy = parameters.target_phy;
        self.codec_id = parameters.codec_id;
        self.codec_specific_configuration = parameters.codec_specific_configuration.clone();
        self.state = AseState::CODEC_CONFIGURED;
        (AseResponseCode::SUCCESS, AseReasonCode::NONE)
    }

    fn config_qos(&mut self, parameters: &ConfigQosParameters) -> (AseResponseCode, AseReasonCode) {
        if ![AseState::CODEC_CONFIGURED, AseState::QOS_CONFIGURED].contains(&self.state) {
            return Self::invalid_transition();
        }
        self.qos = AseQosConfiguration {
            cig_id: parameters.cig_id,
            cis_id: parameters.cis_id,
            sdu_interval: parameters.sdu_interval,
            framing: parameters.framing,
            phy: parameters.phy,
            max_sdu: parameters.max_sdu,
            retransmission_number: parameters.retransmission_number,
            max_transport_latency: parameters.max_transport_latency,
            presentation_delay: parameters.presentation_delay,
        };
        self.state = AseState::QOS_CONFIGURED;
        (AseResponseCode::SUCCESS, AseReasonCode::NONE)
    }

    fn enable(&mut self, metadata: &[u8]) -> (AseResponseCode, AseReasonCode) {
        if self.state != AseState::QOS_CONFIGURED {
            return Self::invalid_transition();
        }
        let Ok(metadata) = Metadata::from_bytes(metadata) else {
            return (AseResponseCode::INVALID_METADATA, AseReasonCode::NONE);
        };
        self.metadata = metadata;
        self.state = AseState::ENABLING;
        (AseResponseCode::SUCCESS, AseReasonCode::NONE)
    }

    fn receiver_start_ready(&mut self) -> (AseResponseCode, AseReasonCode) {
        if self.state != AseState::ENABLING {
            return Self::invalid_transition();
        }
        self.state = AseState::STREAMING;
        (AseResponseCode::SUCCESS, AseReasonCode::NONE)
    }

    fn disable(&mut self) -> (AseResponseCode, AseReasonCode) {
        if ![AseState::ENABLING, AseState::STREAMING].contains(&self.state) {
            return Self::invalid_transition();
        }
        self.state = match self.role {
            AudioRole::Sink => AseState::QOS_CONFIGURED,
            AudioRole::Source => AseState::DISABLING,
        };
        (AseResponseCode::SUCCESS, AseReasonCode::NONE)
    }

    fn receiver_stop_ready(&mut self) -> (AseResponseCode, AseReasonCode) {
        if self.role != AudioRole::Source || self.state != AseState::DISABLING {
            return Self::invalid_transition();
        }
        self.state = AseState::QOS_CONFIGURED;
        (AseResponseCode::SUCCESS, AseReasonCode::NONE)
    }

    fn update_metadata(&mut self, metadata: &[u8]) -> (AseResponseCode, AseReasonCode) {
        if ![AseState::ENABLING, AseState::STREAMING].contains(&self.state) {
            return Self::invalid_transition();
        }
        let Ok(metadata) = Metadata::from_bytes(metadata) else {
            return (AseResponseCode::INVALID_METADATA, AseReasonCode::NONE);
        };
        self.metadata = metadata;
        (AseResponseCode::SUCCESS, AseReasonCode::NONE)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AseStatusDetails {
    None,
    CodecConfigured {
        preferred_framing: u8,
        preferred_phy: u8,
        preferred_retransmission_number: u8,
        preferred_max_transport_latency: u16,
        supported_presentation_delay_min: u32,
        supported_presentation_delay_max: u32,
        preferred_presentation_delay_min: u32,
        preferred_presentation_delay_max: u32,
        codec_id: CodingFormat,
        codec_specific_configuration: Vec<u8>,
    },
    QosConfigured(AseQosConfiguration),
    Metadata {
        cig_id: u8,
        cis_id: u8,
        metadata: Metadata,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AseStatus {
    pub ase_id: u8,
    pub state: AseState,
    pub details: AseStatusDetails,
}

impl AseStatus {
    pub fn from_bytes(data: &[u8]) -> Result<Self> {
        let mut reader = Reader::new(data);
        let ase_id = reader.u8("ASE ID")?;
        let state = AseState(reader.u8("ASE state")?);
        let details = match state {
            AseState::IDLE | AseState::RELEASING => AseStatusDetails::None,
            AseState::CODEC_CONFIGURED => AseStatusDetails::CodecConfigured {
                preferred_framing: reader.u8("preferred framing")?,
                preferred_phy: reader.u8("preferred PHY")?,
                preferred_retransmission_number: reader.u8("preferred retransmission number")?,
                preferred_max_transport_latency: reader
                    .u16("preferred maximum transport latency")?,
                supported_presentation_delay_min: reader
                    .u24("supported presentation delay minimum")?,
                supported_presentation_delay_max: reader
                    .u24("supported presentation delay maximum")?,
                preferred_presentation_delay_min: reader
                    .u24("preferred presentation delay minimum")?,
                preferred_presentation_delay_max: reader
                    .u24("preferred presentation delay maximum")?,
                codec_id: reader.coding_format()?,
                codec_specific_configuration: reader
                    .length_prefixed("ASE codec configuration")?
                    .to_vec(),
            },
            AseState::QOS_CONFIGURED => AseStatusDetails::QosConfigured(AseQosConfiguration {
                cig_id: reader.u8("CIG ID")?,
                cis_id: reader.u8("CIS ID")?,
                sdu_interval: reader.u24("SDU interval")?,
                framing: reader.u8("framing")?,
                phy: reader.u8("PHY")?,
                max_sdu: reader.u16("maximum SDU")?,
                retransmission_number: reader.u8("retransmission number")?,
                max_transport_latency: reader.u16("maximum transport latency")?,
                presentation_delay: reader.u24("presentation delay")?,
            }),
            AseState::ENABLING | AseState::STREAMING | AseState::DISABLING => {
                AseStatusDetails::Metadata {
                    cig_id: reader.u8("CIG ID")?,
                    cis_id: reader.u8("CIS ID")?,
                    metadata: Metadata::from_bytes(reader.length_prefixed("ASE metadata")?)?,
                }
            }
            _ => {
                return Err(Error::InvalidValue(format!(
                    "unknown ASE state 0x{:02X}",
                    state.0
                )))
            }
        };
        reader.finish("ASE status")?;
        Ok(Self {
            ase_id,
            state,
            details,
        })
    }
}

#[derive(Clone, Debug)]
enum PendingNotification {
    ControlPoint(Vec<u8>),
    Ase { ase_id: u8, value: Vec<u8> },
}

#[derive(Clone, Debug)]
pub struct AudioStreamControlService {
    endpoints: Arc<Mutex<BTreeMap<u8, AseEndpoint>>>,
    pending_notifications: Arc<Mutex<VecDeque<PendingNotification>>>,
}

impl AudioStreamControlService {
    pub fn new(sink_ase_ids: &[u8], source_ase_ids: &[u8]) -> Result<Self> {
        let mut endpoints = BTreeMap::new();
        for (role, ids) in [
            (AudioRole::Sink, sink_ase_ids),
            (AudioRole::Source, source_ase_ids),
        ] {
            for ase_id in ids {
                if *ase_id == 0 {
                    return Err(Error::InvalidValue("ASE ID zero is reserved".into()));
                }
                if endpoints
                    .insert(*ase_id, AseEndpoint::new(*ase_id, role))
                    .is_some()
                {
                    return Err(Error::InvalidValue(format!("duplicate ASE ID {ase_id}")));
                }
            }
        }
        Ok(Self {
            endpoints: Arc::new(Mutex::new(endpoints)),
            pending_notifications: Arc::new(Mutex::new(VecDeque::new())),
        })
    }

    pub fn definition(&self) -> Result<ServiceDefinition> {
        let endpoints = self
            .endpoints
            .lock()
            .map_err(|_| Error::InvalidValue("ASCS endpoint lock is poisoned".into()))?;
        let mut characteristics = vec![CharacteristicDefinition {
            uuid: uuid(ASE_CONTROL_POINT_CHARACTERISTIC),
            properties: properties::WRITE | properties::WRITE_WITHOUT_RESPONSE | properties::NOTIFY,
            permissions: permissions::WRITEABLE,
            value: Vec::new(),
            descriptors: Vec::new(),
        }];
        for endpoint in endpoints.values() {
            characteristics.push(CharacteristicDefinition {
                uuid: uuid(match endpoint.role {
                    AudioRole::Sink => SINK_ASE_CHARACTERISTIC,
                    AudioRole::Source => SOURCE_ASE_CHARACTERISTIC,
                }),
                properties: properties::READ | properties::NOTIFY,
                permissions: permissions::READABLE,
                value: endpoint.value()?,
                descriptors: Vec::new(),
            });
        }
        Ok(ServiceDefinition {
            uuid: uuid(AUDIO_STREAM_CONTROL_SERVICE),
            primary: true,
            included_services: Vec::new(),
            characteristics,
        })
    }

    pub fn bind(&self, server: &mut GattServer) -> Result<AudioStreamControlHandles> {
        let control_point = server
            .handles_by_uuid(&uuid(ASE_CONTROL_POINT_CHARACTERISTIC))
            .into_iter()
            .next()
            .ok_or(Error::MissingCharacteristic(
                ASE_CONTROL_POINT_CHARACTERISTIC,
            ))?;

        let endpoints = self
            .endpoints
            .lock()
            .map_err(|_| Error::InvalidValue("ASCS endpoint lock is poisoned".into()))?;
        let sink_ids = endpoints
            .values()
            .filter(|endpoint| endpoint.role == AudioRole::Sink)
            .map(|endpoint| endpoint.ase_id)
            .collect::<Vec<_>>();
        let source_ids = endpoints
            .values()
            .filter(|endpoint| endpoint.role == AudioRole::Source)
            .map(|endpoint| endpoint.ase_id)
            .collect::<Vec<_>>();
        drop(endpoints);

        let sink_handles = server.handles_by_uuid(&uuid(SINK_ASE_CHARACTERISTIC));
        let source_handles = server.handles_by_uuid(&uuid(SOURCE_ASE_CHARACTERISTIC));
        if sink_handles.len() != sink_ids.len() || source_handles.len() != source_ids.len() {
            return Err(Error::InvalidValue(format!(
                "ASCS endpoint handle counts do not match: {} sink/{} source handles for {} sink/{} source endpoints",
                sink_handles.len(),
                source_handles.len(),
                sink_ids.len(),
                source_ids.len()
            )));
        }

        let mut ase_handles = BTreeMap::new();
        for (ase_id, handle) in sink_ids
            .into_iter()
            .zip(sink_handles)
            .chain(source_ids.into_iter().zip(source_handles))
        {
            ase_handles.insert(ase_id, handle);
            let endpoints = Arc::clone(&self.endpoints);
            server.set_dynamic_value(
                handle,
                DynamicValue::read_only(move |_| {
                    endpoints
                        .lock()
                        .map_err(|_| UNLIKELY_ERROR)?
                        .get(&ase_id)
                        .ok_or(UNLIKELY_ERROR)?
                        .value()
                        .map_err(|_| UNLIKELY_ERROR)
                }),
            )?;
        }

        let endpoints = Arc::clone(&self.endpoints);
        let pending = Arc::clone(&self.pending_notifications);
        server.set_dynamic_value(
            control_point,
            DynamicValue::write_only(move |_, data| {
                let operation =
                    AseOperation::from_bytes(data).map_err(|_| INVALID_ATTRIBUTE_VALUE_LENGTH)?;
                let mut endpoints = endpoints.lock().map_err(|_| UNLIKELY_ERROR)?;
                let (response, notifications) =
                    process_operation(&mut endpoints, &operation).map_err(|_| UNLIKELY_ERROR)?;
                let mut pending = pending.lock().map_err(|_| UNLIKELY_ERROR)?;
                pending.push_back(PendingNotification::ControlPoint(
                    response.to_bytes().map_err(|_| UNLIKELY_ERROR)?,
                ));
                pending.extend(
                    notifications
                        .into_iter()
                        .map(|(ase_id, value)| PendingNotification::Ase { ase_id, value }),
                );
                Ok(())
            }),
        )?;

        Ok(AudioStreamControlHandles {
            control_point,
            ase: ase_handles,
        })
    }

    pub fn take_pending_notifications(
        &self,
        handles: &AudioStreamControlHandles,
    ) -> Result<Vec<(u16, Vec<u8>)>> {
        let mut pending = self
            .pending_notifications
            .lock()
            .map_err(|_| Error::InvalidValue("ASCS notification lock is poisoned".into()))?;
        pending
            .drain(..)
            .map(|notification| match notification {
                PendingNotification::ControlPoint(value) => Ok((handles.control_point, value)),
                PendingNotification::Ase { ase_id, value } => handles
                    .ase
                    .get(&ase_id)
                    .copied()
                    .map(|handle| (handle, value))
                    .ok_or_else(|| {
                        Error::InvalidValue(format!("ASE ID {ase_id} has no bound handle"))
                    }),
            })
            .collect()
    }

    /// Model successful CIS establishment. Sink ASEs enter Streaming; source
    /// ASEs remain Enabling until Receiver Start Ready, matching upstream.
    pub fn establish_cis(&self, cig_id: u8, cis_id: u8) -> Result<Vec<u8>> {
        let mut endpoints = self
            .endpoints
            .lock()
            .map_err(|_| Error::InvalidValue("ASCS endpoint lock is poisoned".into()))?;
        let mut changed = Vec::new();
        let mut notifications = Vec::new();
        for endpoint in endpoints.values_mut().filter(|endpoint| {
            endpoint.qos.cig_id == cig_id
                && endpoint.qos.cis_id == cis_id
                && endpoint.state == AseState::ENABLING
        }) {
            if endpoint.role == AudioRole::Sink {
                endpoint.state = AseState::STREAMING;
            }
            changed.push(endpoint.ase_id);
            notifications.push(PendingNotification::Ase {
                ase_id: endpoint.ase_id,
                value: endpoint.value()?,
            });
        }
        drop(endpoints);
        self.pending_notifications
            .lock()
            .map_err(|_| Error::InvalidValue("ASCS notification lock is poisoned".into()))?
            .extend(notifications);
        Ok(changed)
    }

    pub fn reset(&self) -> Result<()> {
        let mut endpoints = self
            .endpoints
            .lock()
            .map_err(|_| Error::InvalidValue("ASCS endpoint lock is poisoned".into()))?;
        for endpoint in endpoints.values_mut() {
            endpoint.state = AseState::IDLE;
        }
        Ok(())
    }

    pub fn endpoint(&self, ase_id: u8) -> Result<Option<AseEndpoint>> {
        self.endpoints
            .lock()
            .map(|endpoints| endpoints.get(&ase_id).cloned())
            .map_err(|_| Error::InvalidValue("ASCS endpoint lock is poisoned".into()))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AudioStreamControlHandles {
    pub control_point: u16,
    pub ase: BTreeMap<u8, u16>,
}

type AseNotifications = Vec<(u8, Vec<u8>)>;

fn process_operation(
    endpoints: &mut BTreeMap<u8, AseEndpoint>,
    operation: &AseOperation,
) -> Result<(AseControlResponse, AseNotifications)> {
    let mut responses = Vec::new();
    let mut notifications = Vec::new();
    match operation {
        AseOperation::ConfigCodec(parameters) => {
            for parameter in parameters {
                apply_and_snapshot(
                    endpoints,
                    parameter.ase_id,
                    &mut responses,
                    &mut notifications,
                    false,
                    |endpoint| endpoint.config_codec(parameter),
                )?;
            }
        }
        AseOperation::ConfigQos(parameters) => {
            for parameter in parameters {
                apply_and_snapshot(
                    endpoints,
                    parameter.ase_id,
                    &mut responses,
                    &mut notifications,
                    false,
                    |endpoint| endpoint.config_qos(parameter),
                )?;
            }
        }
        AseOperation::Enable(parameters) => {
            for parameter in parameters {
                apply_and_snapshot(
                    endpoints,
                    parameter.ase_id,
                    &mut responses,
                    &mut notifications,
                    false,
                    |endpoint| endpoint.enable(&parameter.metadata),
                )?;
            }
        }
        AseOperation::ReceiverStartReady(ase_ids) => {
            for ase_id in ase_ids {
                apply_and_snapshot(
                    endpoints,
                    *ase_id,
                    &mut responses,
                    &mut notifications,
                    false,
                    AseEndpoint::receiver_start_ready,
                )?;
            }
        }
        AseOperation::Disable(ase_ids) => {
            for ase_id in ase_ids {
                apply_and_snapshot(
                    endpoints,
                    *ase_id,
                    &mut responses,
                    &mut notifications,
                    false,
                    AseEndpoint::disable,
                )?;
            }
        }
        AseOperation::ReceiverStopReady(ase_ids) => {
            for ase_id in ase_ids {
                apply_and_snapshot(
                    endpoints,
                    *ase_id,
                    &mut responses,
                    &mut notifications,
                    false,
                    AseEndpoint::receiver_stop_ready,
                )?;
            }
        }
        AseOperation::UpdateMetadata(parameters) => {
            for parameter in parameters {
                apply_and_snapshot(
                    endpoints,
                    parameter.ase_id,
                    &mut responses,
                    &mut notifications,
                    false,
                    |endpoint| endpoint.update_metadata(&parameter.metadata),
                )?;
            }
        }
        AseOperation::Release(ase_ids) => {
            for ase_id in ase_ids {
                apply_and_snapshot(
                    endpoints,
                    *ase_id,
                    &mut responses,
                    &mut notifications,
                    true,
                    |endpoint| {
                        if endpoint.state == AseState::IDLE {
                            AseEndpoint::invalid_transition()
                        } else {
                            endpoint.state = AseState::RELEASING;
                            (AseResponseCode::SUCCESS, AseReasonCode::NONE)
                        }
                    },
                )?;
            }
        }
    }
    Ok((
        AseControlResponse {
            opcode: operation.opcode(),
            responses,
        },
        notifications,
    ))
}

fn apply_and_snapshot<F>(
    endpoints: &mut BTreeMap<u8, AseEndpoint>,
    ase_id: u8,
    responses: &mut Vec<AseResponse>,
    notifications: &mut Vec<(u8, Vec<u8>)>,
    return_to_idle: bool,
    apply: F,
) -> Result<()>
where
    F: FnOnce(&mut AseEndpoint) -> (AseResponseCode, AseReasonCode),
{
    let Some(endpoint) = endpoints.get_mut(&ase_id) else {
        responses.push(AseResponse {
            ase_id,
            code: AseResponseCode::INVALID_ASE_ID,
            reason: AseReasonCode::NONE,
        });
        return Ok(());
    };
    let (code, reason) = apply(endpoint);
    responses.push(AseResponse {
        ase_id,
        code,
        reason,
    });
    notifications.push((ase_id, endpoint.value()?));
    if return_to_idle && code == AseResponseCode::SUCCESS {
        endpoint.state = AseState::IDLE;
        notifications.push((ase_id, endpoint.value()?));
    }
    Ok(())
}

#[derive(Clone, Debug)]
pub struct AudioStreamControlServiceProxy {
    pub service: ServiceProxy,
    pub sink_ase: Vec<CharacteristicProxy>,
    pub source_ase: Vec<CharacteristicProxy>,
    pub ase_control_point: CharacteristicProxy,
}

impl AudioStreamControlServiceProxy {
    pub fn from_parts(
        service: ServiceProxy,
        characteristics: &[CharacteristicProxy],
    ) -> Result<Self> {
        let sink_uuid = uuid(SINK_ASE_CHARACTERISTIC);
        let source_uuid = uuid(SOURCE_ASE_CHARACTERISTIC);
        Ok(Self {
            service,
            sink_ase: characteristics
                .iter()
                .filter(|characteristic| characteristic.uuid == sink_uuid)
                .cloned()
                .collect(),
            source_ase: characteristics
                .iter()
                .filter(|characteristic| characteristic.uuid == source_uuid)
                .cloned()
                .collect(),
            ase_control_point: require_characteristic(
                characteristics,
                ASE_CONTROL_POINT_CHARACTERISTIC,
            )?,
        })
    }

    pub fn discover(
        client: &mut GattClient,
        transport: &mut impl AttTransport,
    ) -> Result<Option<Self>> {
        let Some((service, characteristics)) =
            discover_profile(client, transport, AUDIO_STREAM_CONTROL_SERVICE)?
        else {
            return Ok(None);
        };
        Self::from_parts(service, &characteristics).map(Some)
    }

    pub fn subscribe_all(
        &self,
        client: &mut GattClient,
        transport: &mut impl AttTransport,
    ) -> Result<()> {
        for characteristic in self
            .sink_ase
            .iter()
            .chain(&self.source_ase)
            .chain(core::iter::once(&self.ase_control_point))
        {
            let cccd = client
                .discover_descriptors(transport, characteristic)?
                .into_iter()
                .find(|descriptor| descriptor.uuid == uuid(0x2902))
                .ok_or_else(|| {
                    Error::InvalidValue(format!(
                        "ASCS notification characteristic {:?} has no CCCD",
                        characteristic.uuid
                    ))
                })?;
            client.subscribe(transport, characteristic.handle, cccd.handle, false)?;
        }
        Ok(())
    }

    pub fn write_operation(
        &self,
        client: &mut GattClient,
        transport: &mut impl AttTransport,
        operation: &AseOperation,
    ) -> Result<()> {
        client.write_value(
            transport,
            self.ase_control_point.handle,
            operation.to_bytes()?,
            false,
        )?;
        Ok(())
    }

    pub fn read_ase(
        characteristic: &CharacteristicProxy,
        client: &mut GattClient,
        transport: &mut impl AttTransport,
    ) -> Result<AseStatus> {
        AseStatus::from_bytes(&client.read_value(transport, characteristic.handle, false)?)
    }

    pub fn event_from_notification(&self, handle: u16, value: &[u8]) -> Result<AseEvent> {
        if handle == self.ase_control_point.handle {
            return AseControlResponse::from_bytes(value).map(AseEvent::ControlPoint);
        }
        if self
            .sink_ase
            .iter()
            .chain(&self.source_ase)
            .any(|characteristic| characteristic.handle == handle)
        {
            return AseStatus::from_bytes(value).map(AseEvent::Status);
        }
        Err(Error::InvalidValue(format!(
            "notification handle 0x{handle:04X} does not belong to ASCS"
        )))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AseEvent {
    ControlPoint(AseControlResponse),
    Status(AseStatus),
}

fn push_length_prefixed(target: &mut Vec<u8>, data: &[u8], name: &str) -> Result<()> {
    let length = u8::try_from(data.len())
        .map_err(|_| Error::InvalidValue(format!("{name} exceeds 255 bytes")))?;
    target.push(length);
    target.extend_from_slice(data);
    Ok(())
}

fn require_u24(name: &str, value: u32) -> Result<()> {
    if value > 0x00FF_FFFF {
        return Err(Error::InvalidValue(format!(
            "{name} 0x{value:08X} exceeds 24 bits"
        )));
    }
    Ok(())
}

fn u24_bytes(value: u32) -> [u8; 3] {
    let bytes = value.to_le_bytes();
    [bytes[0], bytes[1], bytes[2]]
}

struct Reader<'a> {
    data: &'a [u8],
    offset: usize,
}

impl<'a> Reader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, offset: 0 }
    }

    fn take(&mut self, length: usize, name: &str) -> Result<&'a [u8]> {
        let end = self
            .offset
            .checked_add(length)
            .ok_or_else(|| Error::InvalidValue(format!("{name} length overflow")))?;
        let value = self.data.get(self.offset..end).ok_or_else(|| {
            Error::InvalidValue(format!(
                "truncated {name} at offset {}: need {length} bytes",
                self.offset
            ))
        })?;
        self.offset = end;
        Ok(value)
    }

    fn u8(&mut self, name: &str) -> Result<u8> {
        Ok(self.take(1, name)?[0])
    }

    fn u16(&mut self, name: &str) -> Result<u16> {
        let value: [u8; 2] = self
            .take(2, name)?
            .try_into()
            .expect("two-byte reader slice");
        Ok(u16::from_le_bytes(value))
    }

    fn u24(&mut self, name: &str) -> Result<u32> {
        let value = self.take(3, name)?;
        Ok(u32::from_le_bytes([value[0], value[1], value[2], 0]))
    }

    fn coding_format(&mut self) -> Result<CodingFormat> {
        CodingFormat::from_bytes(self.take(5, "coding format")?)
            .map_err(|error| Error::InvalidValue(error.to_string()))
    }

    fn length_prefixed(&mut self, name: &str) -> Result<&'a [u8]> {
        let length = usize::from(self.u8(&format!("{name} length"))?);
        self.take(length, name)
    }

    fn finish(self, name: &str) -> Result<()> {
        if self.offset != self.data.len() {
            return Err(Error::InvalidValue(format!(
                "{name} has {} trailing bytes",
                self.data.len() - self.offset
            )));
        }
        Ok(())
    }
}
