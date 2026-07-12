//! Hands-Free Profile service-level connection state machines.
//!
//! This slice ports the normative HFP feature exchange and SLC initialization
//! sequence onto the incremental parsers in `bumble-at`. Both roles are
//! synchronous and sans-I/O: callers feed RFCOMM application bytes and drain
//! the bytes each role wants to send.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use bumble_at::{AtCommand, AtResponse, CommandStream, CommandSubCode, Parameter, ResponseStream};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    At(bumble_at::Error),
    InvalidState(String),
    Protocol(String),
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Error::At(error) => write!(f, "AT: {error}"),
            Error::InvalidState(message) => write!(f, "invalid HFP state: {message}"),
            Error::Protocol(message) => write!(f, "HFP protocol error: {message}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<bumble_at::Error> for Error {
    fn from(value: bumble_at::Error) -> Self {
        Error::At(value)
    }
}

pub type Result<T> = core::result::Result<T, Error>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct HfFeatures(pub u16);

impl HfFeatures {
    pub const EC_NR: Self = Self(0x001);
    pub const THREE_WAY_CALLING: Self = Self(0x002);
    pub const CLI_PRESENTATION_CAPABILITY: Self = Self(0x004);
    pub const VOICE_RECOGNITION_ACTIVATION: Self = Self(0x008);
    pub const REMOTE_VOLUME_CONTROL: Self = Self(0x010);
    pub const ENHANCED_CALL_STATUS: Self = Self(0x020);
    pub const ENHANCED_CALL_CONTROL: Self = Self(0x040);
    pub const CODEC_NEGOTIATION: Self = Self(0x080);
    pub const HF_INDICATORS: Self = Self(0x100);
    pub const ESCO_S4_SETTINGS_SUPPORTED: Self = Self(0x200);
    pub const ENHANCED_VOICE_RECOGNITION_STATUS: Self = Self(0x400);
    pub const VOICE_RECOGNITION_TEXT: Self = Self(0x800);

    pub fn contains(self, feature: Self) -> bool {
        self.0 & feature.0 != 0
    }
}

impl core::ops::BitOr for HfFeatures {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self::Output {
        Self(self.0 | rhs.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AgFeatures(pub u16);

impl AgFeatures {
    pub const THREE_WAY_CALLING: Self = Self(0x001);
    pub const EC_NR: Self = Self(0x002);
    pub const VOICE_RECOGNITION_FUNCTION: Self = Self(0x004);
    pub const IN_BAND_RING_TONE_CAPABILITY: Self = Self(0x008);
    pub const VOICE_TAG: Self = Self(0x010);
    pub const REJECT_CALL: Self = Self(0x020);
    pub const ENHANCED_CALL_STATUS: Self = Self(0x040);
    pub const ENHANCED_CALL_CONTROL: Self = Self(0x080);
    pub const EXTENDED_ERROR_RESULT_CODES: Self = Self(0x100);
    pub const CODEC_NEGOTIATION: Self = Self(0x200);
    pub const HF_INDICATORS: Self = Self(0x400);
    pub const ESCO_S4_SETTINGS_SUPPORTED: Self = Self(0x800);
    pub const ENHANCED_VOICE_RECOGNITION_STATUS: Self = Self(0x1000);
    pub const VOICE_RECOGNITION_TEXT: Self = Self(0x2000);

    pub fn contains(self, feature: Self) -> bool {
        self.0 & feature.0 != 0
    }
}

impl core::ops::BitOr for AgFeatures {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self::Output {
        Self(self.0 | rhs.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum AudioCodec {
    Cvsd,
    Msbc,
    Lc3Swb,
    Other(u8),
}

impl AudioCodec {
    pub fn value(self) -> u8 {
        match self {
            AudioCodec::Cvsd => 1,
            AudioCodec::Msbc => 2,
            AudioCodec::Lc3Swb => 3,
            AudioCodec::Other(value) => value,
        }
    }

    pub fn from_value(value: u8) -> Self {
        match value {
            1 => AudioCodec::Cvsd,
            2 => AudioCodec::Msbc,
            3 => AudioCodec::Lc3Swb,
            value => AudioCodec::Other(value),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum HfIndicator {
    EnhancedSafety,
    BatteryLevel,
    Other(u16),
}

impl HfIndicator {
    pub fn value(self) -> u16 {
        match self {
            HfIndicator::EnhancedSafety => 1,
            HfIndicator::BatteryLevel => 2,
            HfIndicator::Other(value) => value,
        }
    }

    pub fn from_value(value: u16) -> Self {
        match value {
            1 => HfIndicator::EnhancedSafety,
            2 => HfIndicator::BatteryLevel,
            value => HfIndicator::Other(value),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgIndicator {
    Service,
    Call,
    CallSetup,
    CallHeld,
    Signal,
    Roam,
    BatteryCharge,
}

impl AgIndicator {
    pub fn name(self) -> &'static str {
        match self {
            AgIndicator::Service => "service",
            AgIndicator::Call => "call",
            AgIndicator::CallSetup => "callsetup",
            AgIndicator::CallHeld => "callheld",
            AgIndicator::Signal => "signal",
            AgIndicator::Roam => "roam",
            AgIndicator::BatteryCharge => "battchg",
        }
    }

    pub fn parse(name: &[u8]) -> Option<Self> {
        Some(match name {
            b"service" => AgIndicator::Service,
            b"call" => AgIndicator::Call,
            b"callsetup" => AgIndicator::CallSetup,
            b"callheld" => AgIndicator::CallHeld,
            b"signal" => AgIndicator::Signal,
            b"roam" => AgIndicator::Roam,
            b"battchg" => AgIndicator::BatteryCharge,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgIndicatorState {
    pub indicator: AgIndicator,
    pub supported_values: BTreeSet<i32>,
    pub current_status: i32,
    pub enabled: bool,
}

impl AgIndicatorState {
    pub fn new(
        indicator: AgIndicator,
        supported_values: impl IntoIterator<Item = i32>,
        current_status: i32,
    ) -> Self {
        Self {
            indicator,
            supported_values: supported_values.into_iter().collect(),
            current_status,
            enabled: true,
        }
    }

    pub fn call() -> Self {
        Self::new(AgIndicator::Call, [0, 1], 0)
    }

    pub fn service() -> Self {
        Self::new(AgIndicator::Service, [0, 1], 0)
    }

    pub fn call_setup() -> Self {
        Self::new(AgIndicator::CallSetup, [0, 1, 2, 3], 0)
    }

    pub fn signal() -> Self {
        Self::new(AgIndicator::Signal, [0, 1, 2, 3, 4, 5], 0)
    }

    pub fn on_test_text(&self) -> String {
        let min = self.supported_values.first().copied().unwrap_or_default();
        let max = self.supported_values.last().copied().unwrap_or_default();
        let contiguous = self.supported_values.len() == (max - min + 1).max(0) as usize;
        let values = if contiguous {
            format!("{min}-{max}")
        } else {
            self.supported_values
                .iter()
                .map(i32::to_string)
                .collect::<Vec<_>>()
                .join(",")
        };
        format!("(\"{}\",({values}))", self.indicator.name())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HfIndicatorState {
    pub indicator: HfIndicator,
    pub supported: bool,
    pub enabled: bool,
    pub current_status: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum CallHoldOperation {
    ReleaseAllHeld,
    ReleaseAllActive,
    ReleaseSpecific,
    HoldAllActive,
    HoldAllExcept,
    AddHeld,
    ConnectTwo,
}

impl CallHoldOperation {
    pub fn code(self) -> &'static str {
        match self {
            CallHoldOperation::ReleaseAllHeld => "0",
            CallHoldOperation::ReleaseAllActive => "1",
            CallHoldOperation::ReleaseSpecific => "1x",
            CallHoldOperation::HoldAllActive => "2",
            CallHoldOperation::HoldAllExcept => "2x",
            CallHoldOperation::AddHeld => "3",
            CallHoldOperation::ConnectTwo => "4",
        }
    }

    fn parse(code: &[u8]) -> Option<Self> {
        Some(match code {
            b"0" => CallHoldOperation::ReleaseAllHeld,
            b"1" => CallHoldOperation::ReleaseAllActive,
            b"1x" => CallHoldOperation::ReleaseSpecific,
            b"2" => CallHoldOperation::HoldAllActive,
            b"2x" => CallHoldOperation::HoldAllExcept,
            b"3" => CallHoldOperation::AddHeld,
            b"4" => CallHoldOperation::ConnectTwo,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone)]
pub struct HfConfiguration {
    pub features: HfFeatures,
    pub indicators: Vec<HfIndicator>,
    pub codecs: Vec<AudioCodec>,
}

#[derive(Debug, Clone)]
pub struct AgConfiguration {
    pub features: AgFeatures,
    pub indicators: Vec<AgIndicatorState>,
    pub hf_indicators: BTreeSet<HfIndicator>,
    pub call_hold_operations: BTreeSet<CallHoldOperation>,
    pub codecs: Vec<AudioCodec>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PendingKind {
    Brsf,
    Bac,
    CindTest,
    CindRead,
    Cmer,
    ChldTest,
    BindSet,
    BindTest,
    BindRead,
}

#[derive(Debug)]
struct PendingCommand {
    kind: PendingKind,
    responses: Vec<AtResponse>,
}

#[derive(Debug)]
pub struct HfProtocol {
    configuration: HfConfiguration,
    response_stream: ResponseStream,
    outbox: VecDeque<Vec<u8>>,
    pending: Option<PendingCommand>,
    pub supported_ag_features: AgFeatures,
    pub ag_indicators: Vec<AgIndicatorState>,
    pub hf_indicators: BTreeMap<HfIndicator, HfIndicatorState>,
    pub supported_ag_call_hold_operations: BTreeSet<CallHoldOperation>,
    pub slc_complete: bool,
}

impl HfProtocol {
    pub fn new(configuration: HfConfiguration) -> Self {
        let hf_indicators = configuration
            .indicators
            .iter()
            .copied()
            .map(|indicator| {
                (
                    indicator,
                    HfIndicatorState {
                        indicator,
                        supported: false,
                        enabled: false,
                        current_status: 0,
                    },
                )
            })
            .collect();
        Self {
            configuration,
            response_stream: ResponseStream::default(),
            outbox: VecDeque::new(),
            pending: None,
            supported_ag_features: AgFeatures::default(),
            ag_indicators: Vec::new(),
            hf_indicators,
            supported_ag_call_hold_operations: BTreeSet::new(),
            slc_complete: false,
        }
    }

    pub fn start_slc(&mut self) -> Result<()> {
        if self.pending.is_some() || self.slc_complete {
            return Err(Error::InvalidState("SLC already started".into()));
        }
        self.send_command(
            PendingKind::Brsf,
            format!("AT+BRSF={}", self.configuration.features.0),
        )
    }

    pub fn feed(&mut self, bytes: &[u8]) -> Result<()> {
        for response in self.response_stream.push(bytes)? {
            if is_status(&response.code) {
                if response.code != "OK" {
                    return Err(Error::Protocol(response.code));
                }
                let pending = self
                    .pending
                    .take()
                    .ok_or_else(|| Error::Protocol("unexpected OK".into()))?;
                self.on_command_complete(pending.kind, pending.responses)?;
            } else {
                self.pending
                    .as_mut()
                    .ok_or_else(|| Error::Protocol("unsolicited response during SLC".into()))?
                    .responses
                    .push(response);
            }
        }
        Ok(())
    }

    pub fn drain_outgoing(&mut self) -> Vec<Vec<u8>> {
        self.outbox.drain(..).collect()
    }

    fn send_command(&mut self, kind: PendingKind, command: String) -> Result<()> {
        if self.pending.is_some() {
            return Err(Error::InvalidState("command already pending".into()));
        }
        let mut bytes = command.into_bytes();
        bytes.push(b'\r');
        self.outbox.push_back(bytes);
        self.pending = Some(PendingCommand {
            kind,
            responses: Vec::new(),
        });
        Ok(())
    }

    fn on_command_complete(&mut self, kind: PendingKind, responses: Vec<AtResponse>) -> Result<()> {
        match kind {
            PendingKind::Brsf => {
                let response = single_response(&responses, "+BRSF")?;
                self.supported_ag_features = AgFeatures(parse_u16(value_at(response, 0)?)?);
                if self
                    .configuration
                    .features
                    .contains(HfFeatures::CODEC_NEGOTIATION)
                    && self
                        .supported_ag_features
                        .contains(AgFeatures::CODEC_NEGOTIATION)
                {
                    let codecs = self
                        .configuration
                        .codecs
                        .iter()
                        .map(|codec| codec.value().to_string())
                        .collect::<Vec<_>>()
                        .join(",");
                    self.send_command(PendingKind::Bac, format!("AT+BAC={codecs}"))?;
                } else {
                    self.send_command(PendingKind::CindTest, "AT+CIND=?".into())?;
                }
            }
            PendingKind::Bac => {
                self.send_command(PendingKind::CindTest, "AT+CIND=?".into())?;
            }
            PendingKind::CindTest => {
                let response = single_response(&responses, "+CIND")?;
                self.ag_indicators = parse_ag_indicator_descriptions(&response.parameters)?;
                self.send_command(PendingKind::CindRead, "AT+CIND?".into())?;
            }
            PendingKind::CindRead => {
                let response = single_response(&responses, "+CIND")?;
                if response.parameters.len() != self.ag_indicators.len() {
                    return Err(Error::Protocol("CIND status count mismatch".into()));
                }
                for (state, parameter) in self.ag_indicators.iter_mut().zip(&response.parameters) {
                    state.current_status = parse_i32(parameter_value(parameter)?)?;
                }
                self.send_command(PendingKind::Cmer, "AT+CMER=3,,,1".into())?;
            }
            PendingKind::Cmer => self.after_cmer()?,
            PendingKind::ChldTest => {
                let response = single_response(&responses, "+CHLD")?;
                let operations = list_at(response, 0)?;
                self.supported_ag_call_hold_operations = operations
                    .iter()
                    .map(|operation| {
                        CallHoldOperation::parse(parameter_value(operation)?)
                            .ok_or_else(|| Error::Protocol("unknown CHLD operation".into()))
                    })
                    .collect::<Result<_>>()?;
                self.after_chld()?;
            }
            PendingKind::BindSet => {
                self.send_command(PendingKind::BindTest, "AT+BIND=?".into())?;
            }
            PendingKind::BindTest => {
                let response = single_response(&responses, "+BIND")?;
                for parameter in list_at(response, 0)? {
                    let indicator =
                        HfIndicator::from_value(parse_u16(parameter_value(parameter)?)?);
                    if let Some(state) = self.hf_indicators.get_mut(&indicator) {
                        state.supported = true;
                    }
                }
                self.send_command(PendingKind::BindRead, "AT+BIND?".into())?;
            }
            PendingKind::BindRead => {
                for response in &responses {
                    if response.code != "+BIND" || response.parameters.len() < 2 {
                        return Err(Error::Protocol("unexpected BIND response".into()));
                    }
                    let indicator = HfIndicator::from_value(parse_u16(value_at(response, 0)?)?);
                    let enabled = parse_u16(value_at(response, 1)?)? != 0;
                    if let Some(state) = self.hf_indicators.get_mut(&indicator) {
                        state.enabled = enabled;
                    }
                }
                self.slc_complete = true;
            }
        }
        Ok(())
    }

    fn after_cmer(&mut self) -> Result<()> {
        if self
            .configuration
            .features
            .contains(HfFeatures::THREE_WAY_CALLING)
            && self
                .supported_ag_features
                .contains(AgFeatures::THREE_WAY_CALLING)
        {
            self.send_command(PendingKind::ChldTest, "AT+CHLD=?".into())
        } else {
            self.after_chld()
        }
    }

    fn after_chld(&mut self) -> Result<()> {
        if self
            .configuration
            .features
            .contains(HfFeatures::HF_INDICATORS)
            && self
                .supported_ag_features
                .contains(AgFeatures::HF_INDICATORS)
        {
            let indicators = self
                .configuration
                .indicators
                .iter()
                .map(|indicator| indicator.value().to_string())
                .collect::<Vec<_>>()
                .join(",");
            self.send_command(PendingKind::BindSet, format!("AT+BIND={indicators}"))
        } else {
            self.slc_complete = true;
            Ok(())
        }
    }
}

#[derive(Debug)]
pub struct AgProtocol {
    configuration: AgConfiguration,
    command_stream: CommandStream,
    outbox: VecDeque<Vec<u8>>,
    pub supported_hf_features: HfFeatures,
    pub supported_audio_codecs: Vec<AudioCodec>,
    pub hf_indicators: BTreeMap<HfIndicator, HfIndicatorState>,
    pub indicator_report_enabled: bool,
    pub slc_complete: bool,
    core_setup_complete: bool,
    chld_complete: bool,
    bind_complete: bool,
}

impl AgProtocol {
    pub fn new(configuration: AgConfiguration) -> Self {
        Self {
            configuration,
            command_stream: CommandStream::default(),
            outbox: VecDeque::new(),
            supported_hf_features: HfFeatures::default(),
            supported_audio_codecs: Vec::new(),
            hf_indicators: BTreeMap::new(),
            indicator_report_enabled: false,
            slc_complete: false,
            core_setup_complete: false,
            chld_complete: false,
            bind_complete: false,
        }
    }

    pub fn feed(&mut self, bytes: &[u8]) -> Result<()> {
        for command in self.command_stream.push(bytes)? {
            self.on_command(command)?;
        }
        Ok(())
    }

    pub fn drain_outgoing(&mut self) -> Vec<Vec<u8>> {
        self.outbox.drain(..).collect()
    }

    pub fn configuration(&self) -> &AgConfiguration {
        &self.configuration
    }

    fn on_command(&mut self, command: AtCommand) -> Result<()> {
        match (command.code.as_str(), command.sub_code) {
            ("BRSF", CommandSubCode::Set) => self.on_brsf(&command.parameters),
            ("BAC", CommandSubCode::Set) => self.on_bac(&command.parameters),
            ("CIND", CommandSubCode::Test) => self.on_cind_test(),
            ("CIND", CommandSubCode::Read) => self.on_cind_read(),
            ("CMER", CommandSubCode::Set) => self.on_cmer(&command.parameters),
            ("CHLD", CommandSubCode::Test) => self.on_chld_test(),
            ("BIND", CommandSubCode::Set) => self.on_bind(&command.parameters),
            ("BIND", CommandSubCode::Test) => self.on_bind_test(),
            ("BIND", CommandSubCode::Read) => self.on_bind_read(),
            _ => {
                self.send_response("ERROR");
                Ok(())
            }
        }
    }

    fn on_brsf(&mut self, parameters: &[Parameter]) -> Result<()> {
        self.supported_hf_features = HfFeatures(parse_u16(first_value(parameters)?)?);
        self.send_response(&format!("+BRSF: {}", self.configuration.features.0));
        self.send_ok();
        Ok(())
    }

    fn on_bac(&mut self, parameters: &[Parameter]) -> Result<()> {
        self.supported_audio_codecs = parameters
            .iter()
            .map(|parameter| parse_u8(parameter_value(parameter)?).map(AudioCodec::from_value))
            .collect::<Result<_>>()?;
        self.send_ok();
        Ok(())
    }

    fn on_cind_test(&mut self) -> Result<()> {
        let indicators = self
            .configuration
            .indicators
            .iter()
            .map(AgIndicatorState::on_test_text)
            .collect::<Vec<_>>()
            .join(",");
        self.send_response(&format!("+CIND: {indicators}"));
        self.send_ok();
        Ok(())
    }

    fn on_cind_read(&mut self) -> Result<()> {
        let values = self
            .configuration
            .indicators
            .iter()
            .map(|indicator| indicator.current_status.to_string())
            .collect::<Vec<_>>()
            .join(",");
        self.send_response(&format!("+CIND: {values}"));
        self.send_ok();
        Ok(())
    }

    fn on_cmer(&mut self, parameters: &[Parameter]) -> Result<()> {
        if parameters.len() < 4
            || parameter_value(&parameters[0])? != b"3"
            || parameter_value(&parameters[3])? != b"1"
        {
            self.send_response("ERROR");
            return Ok(());
        }
        self.indicator_report_enabled = true;
        self.core_setup_complete = true;
        self.send_ok();
        self.update_slc_complete();
        Ok(())
    }

    fn on_chld_test(&mut self) -> Result<()> {
        if !self
            .configuration
            .features
            .contains(AgFeatures::THREE_WAY_CALLING)
        {
            self.send_response("ERROR");
            return Ok(());
        }
        let operations = self
            .configuration
            .call_hold_operations
            .iter()
            .map(|operation| operation.code())
            .collect::<Vec<_>>()
            .join(",");
        self.send_response(&format!("+CHLD: ({operations})"));
        self.send_ok();
        self.chld_complete = true;
        self.update_slc_complete();
        Ok(())
    }

    fn on_bind(&mut self, parameters: &[Parameter]) -> Result<()> {
        if !self
            .configuration
            .features
            .contains(AgFeatures::HF_INDICATORS)
        {
            self.send_response("ERROR");
            return Ok(());
        }
        self.hf_indicators.clear();
        for parameter in parameters {
            let indicator = HfIndicator::from_value(parse_u16(parameter_value(parameter)?)?);
            if self.configuration.hf_indicators.contains(&indicator) {
                self.hf_indicators.insert(
                    indicator,
                    HfIndicatorState {
                        indicator,
                        supported: true,
                        enabled: true,
                        current_status: 0,
                    },
                );
            }
        }
        self.send_ok();
        Ok(())
    }

    fn on_bind_test(&mut self) -> Result<()> {
        let indicators = self
            .configuration
            .hf_indicators
            .iter()
            .map(|indicator| indicator.value().to_string())
            .collect::<Vec<_>>()
            .join(",");
        self.send_response(&format!("+BIND: ({indicators})"));
        self.send_ok();
        Ok(())
    }

    fn on_bind_read(&mut self) -> Result<()> {
        let indicators: Vec<_> = self.hf_indicators.keys().copied().collect();
        for indicator in indicators {
            self.send_response(&format!("+BIND: {},1", indicator.value()));
        }
        self.send_ok();
        self.bind_complete = true;
        self.update_slc_complete();
        Ok(())
    }

    fn update_slc_complete(&mut self) {
        let needs_chld = self
            .supported_hf_features
            .contains(HfFeatures::THREE_WAY_CALLING)
            && self
                .configuration
                .features
                .contains(AgFeatures::THREE_WAY_CALLING);
        let needs_bind = self
            .supported_hf_features
            .contains(HfFeatures::HF_INDICATORS)
            && self
                .configuration
                .features
                .contains(AgFeatures::HF_INDICATORS);
        self.slc_complete = self.core_setup_complete
            && (!needs_chld || self.chld_complete)
            && (!needs_bind || self.bind_complete);
    }

    fn send_response(&mut self, response: &str) {
        self.outbox
            .push_back(format!("\r\n{response}\r\n").into_bytes());
    }

    fn send_ok(&mut self) {
        self.send_response("OK");
    }
}

fn is_status(code: &str) -> bool {
    matches!(
        code,
        "+CME ERROR"
            | "BLACKLISTED"
            | "BUSY"
            | "DELAYED"
            | "ERROR"
            | "NO ANSWER"
            | "NO CARRIER"
            | "OK"
    )
}

fn single_response<'a>(responses: &'a [AtResponse], code: &str) -> Result<&'a AtResponse> {
    if responses.len() != 1 || responses[0].code != code {
        return Err(Error::Protocol(format!("expected one {code} response")));
    }
    Ok(&responses[0])
}

fn parameter_value(parameter: &Parameter) -> Result<&[u8]> {
    match parameter {
        Parameter::Value(value) => Ok(value),
        Parameter::List(_) => Err(Error::Protocol("expected scalar parameter".into())),
    }
}

fn first_value(parameters: &[Parameter]) -> Result<&[u8]> {
    parameters
        .first()
        .ok_or_else(|| Error::Protocol("missing parameter".into()))
        .and_then(parameter_value)
}

fn value_at(response: &AtResponse, index: usize) -> Result<&[u8]> {
    response
        .parameters
        .get(index)
        .ok_or_else(|| Error::Protocol("missing response parameter".into()))
        .and_then(parameter_value)
}

fn list_at(response: &AtResponse, index: usize) -> Result<&[Parameter]> {
    match response.parameters.get(index) {
        Some(Parameter::List(values)) => Ok(values),
        _ => Err(Error::Protocol("expected list response parameter".into())),
    }
}

fn parse_u8(value: &[u8]) -> Result<u8> {
    core::str::from_utf8(value)
        .ok()
        .and_then(|text| text.parse().ok())
        .ok_or_else(|| Error::Protocol("expected u8 parameter".into()))
}

fn parse_u16(value: &[u8]) -> Result<u16> {
    core::str::from_utf8(value)
        .ok()
        .and_then(|text| text.parse().ok())
        .ok_or_else(|| Error::Protocol("expected u16 parameter".into()))
}

fn parse_i32(value: &[u8]) -> Result<i32> {
    core::str::from_utf8(value)
        .ok()
        .and_then(|text| text.parse().ok())
        .ok_or_else(|| Error::Protocol("expected integer parameter".into()))
}

fn parse_ag_indicator_descriptions(parameters: &[Parameter]) -> Result<Vec<AgIndicatorState>> {
    parameters
        .iter()
        .map(|parameter| {
            let Parameter::List(description) = parameter else {
                return Err(Error::Protocol("expected CIND description list".into()));
            };
            if description.len() != 2 {
                return Err(Error::Protocol("invalid CIND description".into()));
            }
            let indicator = AgIndicator::parse(parameter_value(&description[0])?)
                .ok_or_else(|| Error::Protocol("unknown AG indicator".into()))?;
            let Parameter::List(values) = &description[1] else {
                return Err(Error::Protocol("expected CIND value list".into()));
            };
            let mut supported = BTreeSet::new();
            for value in values {
                let bytes = parameter_value(value)?;
                if let Some(separator) = bytes.iter().position(|byte| *byte == b'-') {
                    let min = parse_i32(&bytes[..separator])?;
                    let max = parse_i32(&bytes[separator + 1..])?;
                    supported.extend(min..=max);
                } else {
                    supported.insert(parse_i32(bytes)?);
                }
            }
            Ok(AgIndicatorState::new(indicator, supported, 0))
        })
        .collect()
}
