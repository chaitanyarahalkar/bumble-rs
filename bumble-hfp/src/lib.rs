//! Hands-Free Profile service-level connection state machines.
//!
//! The normative HFP feature exchange, SLC initialization, post-SLC control,
//! codec negotiation, SDP records, and SCO/eSCO parameter surface run on the
//! incremental parsers in `bumble-at`. Both roles are synchronous and sans-I/O:
//! callers feed RFCOMM application bytes and drain the bytes each role wants to
//! send. Upstream `hfp.py` negotiates codecs and links but does not encode CVSD
//! or mSBC media, so codec payload conversion is intentionally outside this
//! crate too.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use bumble_at::{AtCommand, AtResponse, CommandStream, CommandSubCode, Parameter, ResponseStream};

pub mod audio;
pub mod sdp;

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

    pub fn call_held() -> Self {
        Self::new(AgIndicator::CallHeld, [0, 1, 2], 0)
    }

    pub fn signal() -> Self {
        Self::new(AgIndicator::Signal, [0, 1, 2, 3, 4, 5], 0)
    }

    /// Match upstream's current `roam()` factory, including its `CALL`
    /// indicator selection rather than `ROAM`.
    pub fn roam() -> Self {
        Self::new(AgIndicator::Call, [0, 1], 0)
    }

    pub fn battery_charge() -> Self {
        Self::new(AgIndicator::BatteryCharge, [0, 1, 2, 3, 4, 5], 0)
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResponseHoldStatus {
    IncomingCallHeld,
    HeldCallAccepted,
    HeldCallRejected,
    Other(u8),
}

impl ResponseHoldStatus {
    pub fn from_value(value: u8) -> Self {
        match value {
            0 => Self::IncomingCallHeld,
            1 => Self::HeldCallAccepted,
            2 => Self::HeldCallRejected,
            value => Self::Other(value),
        }
    }

    pub fn value(self) -> u8 {
        match self {
            Self::IncomingCallHeld => 0,
            Self::HeldCallAccepted => 1,
            Self::HeldCallRejected => 2,
            Self::Other(value) => value,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallSetupIndicator {
    None,
    Incoming,
    Outgoing,
    RemoteAlerted,
    Other(u8),
}

impl CallSetupIndicator {
    pub fn from_value(value: u8) -> Self {
        match value {
            0 => Self::None,
            1 => Self::Incoming,
            2 => Self::Outgoing,
            3 => Self::RemoteAlerted,
            value => Self::Other(value),
        }
    }

    pub fn value(self) -> u8 {
        match self {
            Self::None => 0,
            Self::Incoming => 1,
            Self::Outgoing => 2,
            Self::RemoteAlerted => 3,
            Self::Other(value) => value,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallHeldIndicator {
    None,
    HeldAndActive,
    HeldOnly,
    Other(u8),
}

pub type CallSetupAgIndicator = CallSetupIndicator;
pub type CallHeldAgIndicator = CallHeldIndicator;

impl CallHeldIndicator {
    pub fn from_value(value: u8) -> Self {
        match value {
            0 => Self::None,
            1 => Self::HeldAndActive,
            2 => Self::HeldOnly,
            value => Self::Other(value),
        }
    }

    pub fn value(self) -> u8 {
        match self {
            Self::None => 0,
            Self::HeldAndActive => 1,
            Self::HeldOnly => 2,
            Self::Other(value) => value,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VoiceRecognitionState {
    Disabled,
    Enabled,
    EnhancedReady,
    Other(u8),
}

impl VoiceRecognitionState {
    pub fn from_value(value: u8) -> Self {
        match value {
            0 => Self::Disabled,
            1 => Self::Enabled,
            2 => Self::EnhancedReady,
            value => Self::Other(value),
        }
    }

    pub fn value(self) -> u8 {
        match self {
            Self::Disabled => 0,
            Self::Enabled => 1,
            Self::EnhancedReady => 2,
            Self::Other(value) => value,
        }
    }
}

impl From<u8> for VoiceRecognitionState {
    fn from(value: u8) -> Self {
        Self::from_value(value)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CmeError {
    PhoneFailure,
    OperationNotAllowed,
    OperationNotSupported,
    MemoryFull,
    InvalidIndex,
    NotFound,
    Other(u16),
}

impl CmeError {
    pub fn from_value(value: u16) -> Self {
        match value {
            0 => Self::PhoneFailure,
            3 => Self::OperationNotAllowed,
            4 => Self::OperationNotSupported,
            20 => Self::MemoryFull,
            21 => Self::InvalidIndex,
            22 => Self::NotFound,
            value => Self::Other(value),
        }
    }

    pub fn value(self) -> u16 {
        match self {
            Self::PhoneFailure => 0,
            Self::OperationNotAllowed => 3,
            Self::OperationNotSupported => 4,
            Self::MemoryFull => 20,
            Self::InvalidIndex => 21,
            Self::NotFound => 22,
            Self::Other(value) => value,
        }
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
pub enum ResponseExpectation {
    None,
    Single,
    Multiple,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandResult {
    pub id: u64,
    pub responses: Vec<AtResponse>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallDirection {
    MobileOriginated,
    MobileTerminated,
    Other(u8),
}

impl CallDirection {
    fn value(self) -> u8 {
        match self {
            CallDirection::MobileOriginated => 0,
            CallDirection::MobileTerminated => 1,
            CallDirection::Other(value) => value,
        }
    }

    fn from_value(value: u8) -> Self {
        match value {
            0 => CallDirection::MobileOriginated,
            1 => CallDirection::MobileTerminated,
            value => CallDirection::Other(value),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallStatus {
    Active,
    Held,
    Dialing,
    Alerting,
    Incoming,
    Waiting,
    Other(u8),
}

impl CallStatus {
    fn value(self) -> u8 {
        match self {
            CallStatus::Active => 0,
            CallStatus::Held => 1,
            CallStatus::Dialing => 2,
            CallStatus::Alerting => 3,
            CallStatus::Incoming => 4,
            CallStatus::Waiting => 5,
            CallStatus::Other(value) => value,
        }
    }

    fn from_value(value: u8) -> Self {
        match value {
            0 => CallStatus::Active,
            1 => CallStatus::Held,
            2 => CallStatus::Dialing,
            3 => CallStatus::Alerting,
            4 => CallStatus::Incoming,
            5 => CallStatus::Waiting,
            value => CallStatus::Other(value),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallMode {
    Voice,
    Data,
    Fax,
    Unknown,
    Other(u8),
}

impl CallMode {
    fn value(self) -> u8 {
        match self {
            CallMode::Voice => 0,
            CallMode::Data => 1,
            CallMode::Fax => 2,
            CallMode::Unknown => 9,
            CallMode::Other(value) => value,
        }
    }

    fn from_value(value: u8) -> Self {
        match value {
            0 => CallMode::Voice,
            1 => CallMode::Data,
            2 => CallMode::Fax,
            9 => CallMode::Unknown,
            value => CallMode::Other(value),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallMultiParty {
    NotInConference,
    InConference,
    Other(u8),
}

impl CallMultiParty {
    fn value(self) -> u8 {
        match self {
            CallMultiParty::NotInConference => 0,
            CallMultiParty::InConference => 1,
            CallMultiParty::Other(value) => value,
        }
    }

    fn from_value(value: u8) -> Self {
        match value {
            0 => CallMultiParty::NotInConference,
            1 => CallMultiParty::InConference,
            value => CallMultiParty::Other(value),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallInfo {
    pub index: u8,
    pub direction: CallDirection,
    pub status: CallStatus,
    pub mode: CallMode,
    pub multi_party: CallMultiParty,
    pub number: Option<String>,
    pub number_type: Option<u16>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallLineIdentification {
    pub number: String,
    pub number_type: u16,
    pub subaddress: Option<String>,
    pub subaddress_type: Option<u16>,
    pub alpha: Option<String>,
    pub validity: Option<u8>,
}

impl CallLineIdentification {
    pub fn new(number: impl Into<String>, number_type: u16) -> Self {
        Self {
            number: number.into(),
            number_type,
            subaddress: None,
            subaddress_type: None,
            alpha: None,
            validity: None,
        }
    }

    pub fn parse(response: &AtResponse) -> Result<Self> {
        Ok(Self {
            number: String::from_utf8_lossy(value_at(response, 0)?).into_owned(),
            number_type: parse_u16(value_at(response, 1)?)?,
            subaddress: response
                .parameters
                .get(2)
                .map(parameter_value)
                .transpose()?
                .map(|value| String::from_utf8_lossy(value).into_owned()),
            subaddress_type: optional_u16(response, 3)?,
            alpha: response
                .parameters
                .get(4)
                .map(parameter_value)
                .transpose()?
                .map(|value| String::from_utf8_lossy(value).into_owned()),
            validity: optional_u16(response, 5)?.map(|value| value as u8),
        })
    }

    pub fn to_clip_parameters(&self) -> String {
        let quote = |value: &str| format!("\"{}\"", value.replace('"', ""));
        let mut parameters = vec![quote(&self.number), self.number_type.to_string()];
        if self.subaddress.is_some()
            || self.subaddress_type.is_some()
            || self.alpha.is_some()
            || self.validity.is_some()
        {
            parameters.push(self.subaddress.as_deref().map(quote).unwrap_or_default());
            parameters.push(
                self.subaddress_type
                    .map(|value| value.to_string())
                    .unwrap_or_default(),
            );
            parameters.push(self.alpha.as_deref().map(quote).unwrap_or_default());
            parameters.push(
                self.validity
                    .map(|value| value.to_string())
                    .unwrap_or_default(),
            );
        }
        parameters.join(",")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HfEvent {
    AgIndicatorChanged { indicator: AgIndicator, value: i32 },
    Ring,
    SpeakerVolume(u8),
    MicrophoneVolume(u8),
    CodecProposal(AudioCodec),
    VoiceRecognition(VoiceRecognitionState),
    CallerId(CallLineIdentification),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgEvent {
    Answer,
    Dial(String),
    HangUp,
    CallHold {
        operation: CallHoldOperation,
        call_index: Option<u8>,
    },
    HfIndicatorChanged {
        indicator: HfIndicator,
        value: i32,
    },
    CodecSelected(AudioCodec),
    CodecConnectionRequest,
    VoiceRecognition(VoiceRecognitionState),
    SpeakerVolume(u8),
    MicrophoneVolume(u8),
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PendingPurpose {
    Slc(PendingKind),
    User {
        id: u64,
        expectation: ResponseExpectation,
    },
}

#[derive(Debug)]
struct PendingCommand {
    purpose: PendingPurpose,
    responses: Vec<AtResponse>,
}

#[derive(Debug)]
pub struct HfProtocol {
    configuration: HfConfiguration,
    response_stream: ResponseStream,
    outbox: VecDeque<Vec<u8>>,
    pending: Option<PendingCommand>,
    completed_commands: VecDeque<CommandResult>,
    events: VecDeque<HfEvent>,
    next_command_id: u64,
    pending_codec_selection: Option<(u64, AudioCodec)>,
    pub supported_ag_features: AgFeatures,
    pub ag_indicators: Vec<AgIndicatorState>,
    pub hf_indicators: BTreeMap<HfIndicator, HfIndicatorState>,
    pub supported_ag_call_hold_operations: BTreeSet<CallHoldOperation>,
    pub slc_complete: bool,
    pub active_codec: AudioCodec,
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
            completed_commands: VecDeque::new(),
            events: VecDeque::new(),
            next_command_id: 0,
            pending_codec_selection: None,
            supported_ag_features: AgFeatures::default(),
            ag_indicators: Vec::new(),
            hf_indicators,
            supported_ag_call_hold_operations: BTreeSet::new(),
            slc_complete: false,
            active_codec: AudioCodec::Cvsd,
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
                    if let Some(PendingCommand {
                        purpose: PendingPurpose::User { id, .. },
                        ..
                    }) = self.pending.take()
                    {
                        if self
                            .pending_codec_selection
                            .is_some_and(|(codec_id, _)| codec_id == id)
                        {
                            self.pending_codec_selection = None;
                        }
                    }
                    return Err(Error::Protocol(response.code));
                }
                let Some(pending) = self.pending.take() else {
                    self.on_unsolicited(response)?;
                    continue;
                };
                match pending.purpose {
                    PendingPurpose::Slc(kind) => {
                        self.on_slc_command_complete(kind, pending.responses)?
                    }
                    PendingPurpose::User { id, expectation } => {
                        validate_response_count(expectation, &pending.responses)?;
                        if let Some((codec_id, codec)) = self.pending_codec_selection {
                            if codec_id == id {
                                self.active_codec = codec;
                                self.pending_codec_selection = None;
                            }
                        }
                        self.completed_commands.push_back(CommandResult {
                            id,
                            responses: pending.responses,
                        });
                    }
                }
            } else if let Some(pending) = self.pending.as_mut() {
                pending.responses.push(response);
            } else {
                self.on_unsolicited(response)?;
            }
        }
        Ok(())
    }

    pub fn drain_outgoing(&mut self) -> Vec<Vec<u8>> {
        self.outbox.drain(..).collect()
    }

    /// Queue one post-SLC AT command. Only one command may be pending, matching
    /// upstream's serialized command lock.
    pub fn execute_command(
        &mut self,
        command: impl Into<String>,
        expectation: ResponseExpectation,
    ) -> Result<u64> {
        if !self.slc_complete {
            return Err(Error::InvalidState("SLC is not complete".into()));
        }
        if self.pending.is_some() {
            return Err(Error::InvalidState("command already pending".into()));
        }
        let id = self.next_command_id;
        self.next_command_id = self.next_command_id.wrapping_add(1);
        self.queue_command(PendingPurpose::User { id, expectation }, command.into())?;
        Ok(id)
    }

    pub fn answer(&mut self) -> Result<u64> {
        self.execute_command("ATA", ResponseExpectation::None)
    }

    pub fn hang_up(&mut self) -> Result<u64> {
        self.execute_command("AT+CHUP", ResponseExpectation::None)
    }

    pub fn reject_incoming_call(&mut self) -> Result<u64> {
        self.hang_up()
    }

    pub fn terminate_call(&mut self) -> Result<u64> {
        self.hang_up()
    }

    pub fn request_audio_connection(&mut self) -> Result<u64> {
        self.execute_command("AT+BCC", ResponseExpectation::None)
    }

    pub fn setup_audio_connection(&mut self) -> Result<u64> {
        self.request_audio_connection()
    }

    pub fn dial(&mut self, number: &str) -> Result<u64> {
        self.execute_command(format!("ATD{number}"), ResponseExpectation::None)
    }

    pub fn query_current_calls(&mut self) -> Result<u64> {
        self.execute_command("AT+CLCC", ResponseExpectation::Multiple)
    }

    pub fn hold_call(&mut self, operation: CallHoldOperation, index: Option<u8>) -> Result<u64> {
        let indexed = matches!(
            operation,
            CallHoldOperation::ReleaseSpecific | CallHoldOperation::HoldAllExcept
        );
        if indexed != index.is_some() {
            return Err(Error::InvalidState(
                "call index must be supplied only for indexed CHLD operations".into(),
            ));
        }
        let code = match (operation, index) {
            (CallHoldOperation::ReleaseSpecific, Some(index)) => format!("1{index}"),
            (CallHoldOperation::HoldAllExcept, Some(index)) => format!("2{index}"),
            _ => operation.code().to_owned(),
        };
        self.execute_command(format!("AT+CHLD={code}"), ResponseExpectation::None)
    }

    pub fn report_hf_indicator(&mut self, indicator: HfIndicator, value: i32) -> Result<u64> {
        self.execute_command(
            format!("AT+BIEV={},{}", indicator.value(), value),
            ResponseExpectation::None,
        )
    }

    pub fn select_codec(&mut self, codec: AudioCodec) -> Result<u64> {
        let id = self.execute_command(
            format!("AT+BCS={}", codec.value()),
            ResponseExpectation::None,
        )?;
        self.pending_codec_selection = Some((id, codec));
        Ok(id)
    }

    pub fn take_completed_commands(&mut self) -> Vec<CommandResult> {
        self.completed_commands.drain(..).collect()
    }

    pub fn take_events(&mut self) -> Vec<HfEvent> {
        self.events.drain(..).collect()
    }

    pub fn parse_current_calls(result: &CommandResult) -> Result<Vec<CallInfo>> {
        result.responses.iter().map(parse_call_info).collect()
    }

    fn send_command(&mut self, kind: PendingKind, command: String) -> Result<()> {
        self.queue_command(PendingPurpose::Slc(kind), command)
    }

    fn queue_command(&mut self, purpose: PendingPurpose, command: String) -> Result<()> {
        if self.pending.is_some() {
            return Err(Error::InvalidState("command already pending".into()));
        }
        let mut bytes = command.into_bytes();
        bytes.push(b'\r');
        self.outbox.push_back(bytes);
        self.pending = Some(PendingCommand {
            purpose,
            responses: Vec::new(),
        });
        Ok(())
    }

    fn on_slc_command_complete(
        &mut self,
        kind: PendingKind,
        responses: Vec<AtResponse>,
    ) -> Result<()> {
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

    fn on_unsolicited(&mut self, response: AtResponse) -> Result<()> {
        match response.code.as_str() {
            "+CIEV" => {
                let index = parse_u16(value_at(&response, 0)?)? as usize;
                let value = parse_i32(value_at(&response, 1)?)?;
                let state = index
                    .checked_sub(1)
                    .and_then(|index| self.ag_indicators.get_mut(index))
                    .ok_or_else(|| Error::Protocol("invalid CIEV indicator index".into()))?;
                state.current_status = value;
                self.events.push_back(HfEvent::AgIndicatorChanged {
                    indicator: state.indicator,
                    value,
                });
            }
            "RING" => self.events.push_back(HfEvent::Ring),
            "+VGS" => self
                .events
                .push_back(HfEvent::SpeakerVolume(parse_u8(value_at(&response, 0)?)?)),
            "+VGM" => self
                .events
                .push_back(HfEvent::MicrophoneVolume(parse_u8(value_at(
                    &response, 0,
                )?)?)),
            "+BCS" => {
                let codec = AudioCodec::from_value(parse_u8(value_at(&response, 0)?)?);
                self.events.push_back(HfEvent::CodecProposal(codec));
            }
            "+BVRA" => self.events.push_back(HfEvent::VoiceRecognition(
                VoiceRecognitionState::from_value(parse_u8(value_at(&response, 0)?)?),
            )),
            "+CLIP" => self
                .events
                .push_back(HfEvent::CallerId(CallLineIdentification::parse(&response)?)),
            // Upstream logs and ignores unknown unsolicited result codes. This
            // matters for optional extensions such as +BSIR that may be sent
            // even when the HF has no dedicated event for them.
            _ => {}
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
    events: VecDeque<AgEvent>,
    pub supported_hf_features: HfFeatures,
    pub supported_audio_codecs: Vec<AudioCodec>,
    pub hf_indicators: BTreeMap<HfIndicator, HfIndicatorState>,
    pub indicator_report_enabled: bool,
    pub inband_ringtone_enabled: bool,
    pub cme_error_enabled: bool,
    pub cli_notification_enabled: bool,
    pub call_waiting_enabled: bool,
    pub slc_complete: bool,
    pub calls: Vec<CallInfo>,
    pub active_codec: AudioCodec,
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
            events: VecDeque::new(),
            supported_hf_features: HfFeatures::default(),
            supported_audio_codecs: Vec::new(),
            hf_indicators: BTreeMap::new(),
            indicator_report_enabled: false,
            inband_ringtone_enabled: true,
            cme_error_enabled: false,
            cli_notification_enabled: false,
            call_waiting_enabled: false,
            slc_complete: false,
            calls: Vec::new(),
            active_codec: AudioCodec::Cvsd,
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

    pub fn take_events(&mut self) -> Vec<AgEvent> {
        self.events.drain(..).collect()
    }

    pub fn update_ag_indicator(&mut self, indicator: AgIndicator, value: i32) -> Result<()> {
        let (index, state) = self
            .configuration
            .indicators
            .iter_mut()
            .enumerate()
            .find(|(_, state)| state.indicator == indicator)
            .ok_or_else(|| Error::Protocol("AG indicator is not supported".into()))?;
        if !state.supported_values.contains(&value) {
            return Err(Error::Protocol(
                "AG indicator value is not supported".into(),
            ));
        }
        state.current_status = value;
        if self.indicator_report_enabled && state.enabled {
            self.send_response(&format!("+CIEV: {},{value}", index + 1));
        }
        Ok(())
    }

    pub fn send_ring(&mut self) {
        self.send_response("RING");
    }

    pub fn send_cme_error(&mut self, error: CmeError) {
        if self.cme_error_enabled {
            self.send_response(&format!("+CME ERROR: {}", error.value()));
        } else {
            self.send_response("ERROR");
        }
    }

    pub fn set_inband_ringtone_enabled(&mut self, enabled: bool) {
        self.inband_ringtone_enabled = enabled;
        self.send_response(&format!("+BSIR: {}", u8::from(enabled)));
    }

    pub fn set_speaker_volume(&mut self, level: u8) {
        self.send_response(&format!("+VGS: {level}"));
    }

    pub fn set_microphone_volume(&mut self, level: u8) {
        self.send_response(&format!("+VGM: {level}"));
    }

    pub fn send_caller_id(&mut self, number: &str, number_type: u16) {
        self.send_cli_notification(&CallLineIdentification::new(number, number_type));
    }

    pub fn send_cli_notification(&mut self, identification: &CallLineIdentification) {
        self.send_response(&format!("+CLIP: {}", identification.to_clip_parameters()));
    }

    pub fn send_voice_recognition(&mut self, state: impl Into<VoiceRecognitionState>) {
        self.send_response(&format!("+BVRA: {}", state.into().value()));
    }

    pub fn propose_codec(&mut self, codec: AudioCodec) -> Result<()> {
        if !self.supported_audio_codecs.contains(&codec) {
            return Err(Error::Protocol("codec is not supported by HF".into()));
        }
        self.send_response(&format!("+BCS: {}", codec.value()));
        Ok(())
    }

    fn on_command(&mut self, command: AtCommand) -> Result<()> {
        match (command.code.as_str(), command.sub_code) {
            ("BRSF", CommandSubCode::Set) => self.on_brsf(&command.parameters),
            ("BAC", CommandSubCode::Set) => self.on_bac(&command.parameters),
            ("CIND", CommandSubCode::Test) => self.on_cind_test(),
            ("CIND", CommandSubCode::Read) => self.on_cind_read(),
            ("CMER", CommandSubCode::Set) => self.on_cmer(&command.parameters),
            ("CMEE", CommandSubCode::Set) => self.on_cmee(&command.parameters),
            ("CCWA", CommandSubCode::Set) => self.on_ccwa(&command.parameters),
            ("CHLD", CommandSubCode::Test) => self.on_chld_test(),
            ("BIND", CommandSubCode::Set) => self.on_bind(&command.parameters),
            ("BIND", CommandSubCode::Test) => self.on_bind_test(),
            ("BIND", CommandSubCode::Read) => self.on_bind_read(),
            ("BIEV", CommandSubCode::Set) => self.on_biev(&command.parameters),
            ("BIA", CommandSubCode::Set) => self.on_bia(&command.parameters),
            ("BCS", CommandSubCode::Set) => self.on_bcs(&command.parameters),
            ("BCC", CommandSubCode::None) => self.on_bcc(),
            ("BVRA", CommandSubCode::Set) => self.on_bvra(&command.parameters),
            ("CHLD", CommandSubCode::Set) => self.on_chld(&command.parameters),
            ("CHUP", CommandSubCode::None) => self.on_chup(),
            ("CLCC", CommandSubCode::None) => self.on_clcc(),
            ("CLIP", CommandSubCode::Set) => self.on_clip(&command.parameters),
            ("VGS", CommandSubCode::Set) => self.on_vgs(&command.parameters),
            ("VGM", CommandSubCode::Set) => self.on_vgm(&command.parameters),
            ("A", CommandSubCode::None) => self.on_answer(),
            ("D", CommandSubCode::None) => self.on_dial(&command.parameters),
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
        if self.configuration.indicators.is_empty() {
            self.send_cme_error(CmeError::NotFound);
            return Ok(());
        }
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
        if self.configuration.indicators.is_empty() {
            self.send_cme_error(CmeError::NotFound);
            return Ok(());
        }
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
        let valid_optional_zero = |parameter: &Parameter| -> Result<bool> {
            let value = parameter_value(parameter)?;
            Ok(value.is_empty() || value == b"0")
        };
        let indicator = parameters.get(3).map(parameter_value).transpose()?;
        if parameters.first().map(parameter_value).transpose()? != Some(b"3".as_slice())
            || parameters
                .get(1)
                .is_some_and(|parameter| !valid_optional_zero(parameter).unwrap_or(false))
            || parameters
                .get(2)
                .is_some_and(|parameter| !valid_optional_zero(parameter).unwrap_or(false))
            || !matches!(indicator, Some(b"0" | b"1"))
        {
            self.send_cme_error(CmeError::InvalidIndex);
            return Ok(());
        }
        self.indicator_report_enabled = indicator == Some(b"1");
        self.core_setup_complete = true;
        self.send_ok();
        self.update_slc_complete();
        Ok(())
    }

    fn on_cmee(&mut self, parameters: &[Parameter]) -> Result<()> {
        self.cme_error_enabled = parse_u8(first_value(parameters)?)? != 0;
        self.send_ok();
        Ok(())
    }

    fn on_ccwa(&mut self, parameters: &[Parameter]) -> Result<()> {
        self.call_waiting_enabled = parse_u8(first_value(parameters)?)? != 0;
        self.send_ok();
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

    fn on_biev(&mut self, parameters: &[Parameter]) -> Result<()> {
        let indicator = HfIndicator::from_value(parse_u16(first_value(parameters)?)?);
        let value = parameters
            .get(1)
            .ok_or_else(|| Error::Protocol("missing BIEV value".into()))
            .and_then(parameter_value)
            .and_then(parse_i32)?;
        let Some(state) = self.hf_indicators.get_mut(&indicator) else {
            self.send_response("ERROR");
            return Ok(());
        };
        state.current_status = value;
        self.events
            .push_back(AgEvent::HfIndicatorChanged { indicator, value });
        self.send_ok();
        Ok(())
    }

    fn on_bia(&mut self, parameters: &[Parameter]) -> Result<()> {
        for (parameter, indicator) in parameters
            .iter()
            .zip(self.configuration.indicators.iter_mut())
        {
            let value = parameter_value(parameter)?;
            if !value.is_empty() {
                indicator.enabled = parse_u8(value)? != 0;
            }
        }
        self.send_ok();
        Ok(())
    }

    fn on_bcs(&mut self, parameters: &[Parameter]) -> Result<()> {
        let codec = AudioCodec::from_value(parse_u8(first_value(parameters)?)?);
        if !self.supported_audio_codecs.contains(&codec) {
            self.send_response("ERROR");
            return Ok(());
        }
        self.active_codec = codec;
        self.events.push_back(AgEvent::CodecSelected(codec));
        self.send_ok();
        Ok(())
    }

    fn on_bcc(&mut self) -> Result<()> {
        self.events.push_back(AgEvent::CodecConnectionRequest);
        self.send_ok();
        Ok(())
    }

    fn on_bvra(&mut self, parameters: &[Parameter]) -> Result<()> {
        let state = VoiceRecognitionState::from_value(parse_u8(first_value(parameters)?)?);
        self.events.push_back(AgEvent::VoiceRecognition(state));
        self.send_ok();
        Ok(())
    }

    fn on_chld(&mut self, parameters: &[Parameter]) -> Result<()> {
        let code = first_value(parameters)?;
        let (normalized, call_index) = if code.len() > 1 {
            let prefix = code[0];
            let index = parse_u8(&code[1..])?;
            (vec![prefix, b'x'], Some(index))
        } else {
            (code.to_vec(), None)
        };
        let Some(operation) = CallHoldOperation::parse(&normalized) else {
            self.send_cme_error(CmeError::OperationNotSupported);
            return Ok(());
        };
        if !self.configuration.call_hold_operations.contains(&operation) {
            self.send_cme_error(CmeError::OperationNotSupported);
            return Ok(());
        }
        if call_index.is_some_and(|index| !self.calls.iter().any(|call| call.index == index)) {
            self.send_cme_error(CmeError::InvalidIndex);
            return Ok(());
        }
        self.events.push_back(AgEvent::CallHold {
            operation,
            call_index,
        });
        self.send_ok();
        Ok(())
    }

    fn on_chup(&mut self) -> Result<()> {
        self.events.push_back(AgEvent::HangUp);
        self.send_ok();
        Ok(())
    }

    fn on_clcc(&mut self) -> Result<()> {
        let calls = self.calls.clone();
        for call in calls {
            let mut response = format!(
                "+CLCC: {},{},{},{},{}",
                call.index,
                call.direction.value(),
                call.status.value(),
                call.mode.value(),
                call.multi_party.value()
            );
            if let Some(number) = &call.number {
                response.push_str(&format!(",\"{number}\""));
            }
            if let Some(number_type) = call.number_type {
                response.push_str(&format!(",{number_type}"));
            }
            self.send_response(&response);
        }
        self.send_ok();
        Ok(())
    }

    fn on_clip(&mut self, parameters: &[Parameter]) -> Result<()> {
        self.cli_notification_enabled = parse_u8(first_value(parameters)?)? != 0;
        self.send_ok();
        Ok(())
    }

    fn on_vgs(&mut self, parameters: &[Parameter]) -> Result<()> {
        self.events
            .push_back(AgEvent::SpeakerVolume(parse_u8(first_value(parameters)?)?));
        self.send_ok();
        Ok(())
    }

    fn on_vgm(&mut self, parameters: &[Parameter]) -> Result<()> {
        self.events
            .push_back(AgEvent::MicrophoneVolume(parse_u8(first_value(
                parameters,
            )?)?));
        self.send_ok();
        Ok(())
    }

    fn on_answer(&mut self) -> Result<()> {
        self.events.push_back(AgEvent::Answer);
        self.send_ok();
        Ok(())
    }

    fn on_dial(&mut self, parameters: &[Parameter]) -> Result<()> {
        let number = String::from_utf8_lossy(first_value(parameters)?).into_owned();
        self.events.push_back(AgEvent::Dial(number));
        self.send_ok();
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

fn validate_response_count(
    expectation: ResponseExpectation,
    responses: &[AtResponse],
) -> Result<()> {
    let valid = match expectation {
        ResponseExpectation::None => responses.is_empty(),
        ResponseExpectation::Single => responses.len() == 1,
        ResponseExpectation::Multiple => true,
    };
    if valid {
        Ok(())
    } else {
        Err(Error::Protocol(format!(
            "unexpected response count {} for {expectation:?}",
            responses.len()
        )))
    }
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

fn optional_u16(response: &AtResponse, index: usize) -> Result<Option<u16>> {
    let Some(parameter) = response.parameters.get(index) else {
        return Ok(None);
    };
    let value = parameter_value(parameter)?;
    if value.is_empty() {
        Ok(None)
    } else {
        parse_u16(value).map(Some)
    }
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

fn parse_call_info(response: &AtResponse) -> Result<CallInfo> {
    if response.code != "+CLCC" || response.parameters.len() < 5 {
        return Err(Error::Protocol("invalid CLCC response".into()));
    }
    Ok(CallInfo {
        index: parse_u8(value_at(response, 0)?)?,
        direction: CallDirection::from_value(parse_u8(value_at(response, 1)?)?),
        status: CallStatus::from_value(parse_u8(value_at(response, 2)?)?),
        mode: CallMode::from_value(parse_u8(value_at(response, 3)?)?),
        multi_party: CallMultiParty::from_value(parse_u8(value_at(response, 4)?)?),
        number: response
            .parameters
            .get(5)
            .map(parameter_value)
            .transpose()?
            .map(|number| String::from_utf8_lossy(number).into_owned()),
        number_type: response
            .parameters
            .get(6)
            .map(parameter_value)
            .transpose()?
            .map(parse_u16)
            .transpose()?,
    })
}
