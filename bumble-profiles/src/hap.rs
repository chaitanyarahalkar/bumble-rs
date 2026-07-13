//! Hearing Access Profile (HAP) features, presets, and live GATT service.

use crate::{discover_profile, require_characteristic, uuid, Error, Result};
use bumble_gatt::{
    permissions, properties, AttTransport, CharacteristicDefinition, CharacteristicProxy,
    DynamicValue, GattClient, GattServer, ServiceDefinition, ServiceProxy,
};
use std::collections::{BTreeMap, VecDeque};
use std::sync::{Arc, Mutex};

pub const HEARING_ACCESS_SERVICE: u16 = 0x1854;
pub const HEARING_AID_FEATURES_CHARACTERISTIC: u16 = 0x2BDA;
pub const HEARING_AID_PRESET_CONTROL_POINT_CHARACTERISTIC: u16 = 0x2BDB;
pub const ACTIVE_PRESET_INDEX_CHARACTERISTIC: u16 = 0x2BDC;

const PROCEDURE_ALREADY_IN_PROGRESS: u8 = 0xFE;
const OUT_OF_RANGE: u8 = 0xFF;
const UNLIKELY_ERROR: u8 = 0x0E;

pub mod error_code {
    pub const INVALID_OPCODE: u8 = 0x80;
    pub const WRITE_NAME_NOT_ALLOWED: u8 = 0x81;
    pub const PRESET_SYNCHRONIZATION_NOT_SUPPORTED: u8 = 0x82;
    pub const PRESET_OPERATION_NOT_POSSIBLE: u8 = 0x83;
    pub const INVALID_PARAMETERS_LENGTH: u8 = 0x84;
}

macro_rules! open_u8 {
    ($name:ident { $($constant:ident = $value:expr),+ $(,)? }) => {
        #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
        pub struct $name(pub u8);

        impl $name {
            $(pub const $constant: Self = Self($value);)+
        }
    };
}

open_u8!(HearingAidType {
    BINAURAL_HEARING_AID = 0b00,
    MONAURAL_HEARING_AID = 0b01,
    BANDED_HEARING_AID = 0b10,
});

open_u8!(PresetSynchronizationSupport {
    PRESET_SYNCHRONIZATION_IS_NOT_SUPPORTED = 0,
    PRESET_SYNCHRONIZATION_IS_SUPPORTED = 1,
});

open_u8!(IndependentPresets {
    IDENTICAL_PRESET_RECORD = 0,
    DIFFERENT_PRESET_RECORD = 1,
});

open_u8!(DynamicPresets {
    PRESET_RECORDS_DOES_NOT_CHANGE = 0,
    PRESET_RECORDS_MAY_CHANGE = 1,
});

open_u8!(WritablePresetsSupport {
    WRITABLE_PRESET_RECORDS_NOT_SUPPORTED = 0,
    WRITABLE_PRESET_RECORDS_SUPPORTED = 1,
});

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HearingAidFeatures {
    pub hearing_aid_type: HearingAidType,
    pub preset_synchronization_support: PresetSynchronizationSupport,
    pub independent_presets: IndependentPresets,
    pub dynamic_presets: DynamicPresets,
    pub writable_presets_support: WritablePresetsSupport,
}

impl HearingAidFeatures {
    pub fn from_byte(value: u8) -> Self {
        Self {
            hearing_aid_type: HearingAidType(value & 0b11),
            preset_synchronization_support: PresetSynchronizationSupport((value >> 2) & 1),
            independent_presets: IndependentPresets((value >> 3) & 1),
            dynamic_presets: DynamicPresets((value >> 4) & 1),
            writable_presets_support: WritablePresetsSupport((value >> 5) & 1),
        }
    }

    pub fn to_byte(self) -> u8 {
        self.hearing_aid_type.0
            | (self.preset_synchronization_support.0 << 2)
            | (self.independent_presets.0 << 3)
            | (self.dynamic_presets.0 << 4)
            | (self.writable_presets_support.0 << 5)
    }
}

open_u8!(PresetWritable {
    CANNOT_BE_WRITTEN = 0,
    CAN_BE_WRITTEN = 1,
});

open_u8!(PresetAvailability {
    IS_UNAVAILABLE = 0,
    IS_AVAILABLE = 1,
});

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PresetProperties {
    pub writable: PresetWritable,
    pub is_available: PresetAvailability,
}

impl Default for PresetProperties {
    fn default() -> Self {
        Self {
            writable: PresetWritable::CAN_BE_WRITTEN,
            is_available: PresetAvailability::IS_AVAILABLE,
        }
    }
}

impl PresetProperties {
    pub fn from_byte(value: u8) -> Self {
        Self {
            writable: PresetWritable(value & 1),
            is_available: PresetAvailability((value >> 1) & 1),
        }
    }

    pub fn to_byte(self) -> u8 {
        self.writable.0 | (self.is_available.0 << 1)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PresetRecord {
    pub index: u8,
    pub name: String,
    pub properties: PresetProperties,
}

impl PresetRecord {
    pub fn new(index: u8, name: impl Into<String>) -> Self {
        Self {
            index,
            name: name.into(),
            properties: PresetProperties::default(),
        }
    }

    pub fn from_bytes(data: &[u8]) -> Result<Self> {
        if data.len() < 3 {
            return Err(Error::InvalidValue(format!(
                "preset record has length {}, expected at least 3",
                data.len()
            )));
        }
        let name = String::from_utf8(data[2..].to_vec())
            .map_err(|error| Error::InvalidValue(format!("invalid preset name: {error}")))?;
        validate_name(&name)?;
        Ok(Self {
            index: data[0],
            properties: PresetProperties::from_byte(data[1]),
            name,
        })
    }

    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        validate_name(&self.name)?;
        let mut value = vec![self.index, self.properties.to_byte()];
        value.extend_from_slice(self.name.as_bytes());
        Ok(value)
    }

    pub fn is_available(&self) -> bool {
        self.properties.is_available == PresetAvailability::IS_AVAILABLE
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PresetChangedOperation {
    GenericUpdate {
        previous_index: u8,
        preset_record: PresetRecord,
    },
    PresetRecordDeleted(u8),
    PresetRecordAvailable(u8),
    PresetRecordUnavailable(u8),
}

impl PresetChangedOperation {
    pub fn to_bytes(&self, is_last: bool) -> Result<Vec<u8>> {
        let mut value = vec![0x03, self.change_id(), u8::from(is_last)];
        match self {
            Self::GenericUpdate {
                previous_index,
                preset_record,
            } => {
                value.push(*previous_index);
                value.extend_from_slice(&preset_record.to_bytes()?);
            }
            Self::PresetRecordDeleted(index)
            | Self::PresetRecordAvailable(index)
            | Self::PresetRecordUnavailable(index) => value.push(*index),
        }
        Ok(value)
    }

    pub fn from_bytes(data: &[u8]) -> Result<(Self, bool)> {
        if data.len() < 4 || data[0] != 0x03 {
            return Err(Error::InvalidValue(
                "invalid Preset Changed operation".into(),
            ));
        }
        let operation = match data[1] {
            0x00 => {
                if data.len() < 6 {
                    return Err(Error::InvalidValue(
                        "generic Preset Changed operation is truncated".into(),
                    ));
                }
                Self::GenericUpdate {
                    previous_index: data[3],
                    preset_record: PresetRecord::from_bytes(&data[4..])?,
                }
            }
            0x01 if data.len() == 4 => Self::PresetRecordDeleted(data[3]),
            0x02 if data.len() == 4 => Self::PresetRecordAvailable(data[3]),
            0x03 if data.len() == 4 => Self::PresetRecordUnavailable(data[3]),
            change_id => {
                return Err(Error::InvalidValue(format!(
                    "invalid Preset Changed ID 0x{change_id:02X} or length {}",
                    data.len()
                )))
            }
        };
        Ok((operation, data[2] != 0))
    }

    fn change_id(&self) -> u8 {
        match self {
            Self::GenericUpdate { .. } => 0x00,
            Self::PresetRecordDeleted(_) => 0x01,
            Self::PresetRecordAvailable(_) => 0x02,
            Self::PresetRecordUnavailable(_) => 0x03,
        }
    }

    fn index(&self) -> u8 {
        match self {
            Self::GenericUpdate { previous_index, .. } => *previous_index,
            Self::PresetRecordDeleted(index)
            | Self::PresetRecordAvailable(index)
            | Self::PresetRecordUnavailable(index) => *index,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PresetControlPointOperation {
    ReadPresets { start_index: u8, count: u8 },
    WritePresetName { index: u8, name: String },
    SetActivePreset(u8),
    SetNextPreset,
    SetPreviousPreset,
    SetActivePresetSynchronizedLocally(u8),
    SetNextPresetSynchronizedLocally,
    SetPreviousPresetSynchronizedLocally,
}

impl PresetControlPointOperation {
    pub fn from_bytes(data: &[u8]) -> Result<Self> {
        let (&opcode, parameters) = data
            .split_first()
            .ok_or_else(|| Error::InvalidValue("empty HAP control operation".into()))?;
        match (opcode, parameters) {
            (0x01, [start_index, count]) => Ok(Self::ReadPresets {
                start_index: *start_index,
                count: *count,
            }),
            (0x04, [index, name @ ..]) if !name.is_empty() => {
                let name = String::from_utf8(name.to_vec()).map_err(|error| {
                    Error::InvalidValue(format!("invalid preset name: {error}"))
                })?;
                Ok(Self::WritePresetName {
                    index: *index,
                    name,
                })
            }
            (0x05, [index]) => Ok(Self::SetActivePreset(*index)),
            (0x06, []) => Ok(Self::SetNextPreset),
            (0x07, []) => Ok(Self::SetPreviousPreset),
            (0x08, [index]) => Ok(Self::SetActivePresetSynchronizedLocally(*index)),
            (0x09, []) => Ok(Self::SetNextPresetSynchronizedLocally),
            (0x0A, []) => Ok(Self::SetPreviousPresetSynchronizedLocally),
            _ => Err(Error::InvalidValue(format!(
                "invalid HAP opcode 0x{opcode:02X} or parameter length {}",
                parameters.len()
            ))),
        }
    }

    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        let value = match self {
            Self::ReadPresets { start_index, count } => vec![0x01, *start_index, *count],
            Self::WritePresetName { index, name } => {
                validate_name(name)?;
                let mut value = vec![0x04, *index];
                value.extend_from_slice(name.as_bytes());
                value
            }
            Self::SetActivePreset(index) => vec![0x05, *index],
            Self::SetNextPreset => vec![0x06],
            Self::SetPreviousPreset => vec![0x07],
            Self::SetActivePresetSynchronizedLocally(index) => vec![0x08, *index],
            Self::SetNextPresetSynchronizedLocally => vec![0x09],
            Self::SetPreviousPresetSynchronizedLocally => vec![0x0A],
        };
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PresetControlPointEvent {
    ReadPresetResponse {
        is_last: bool,
        preset_record: PresetRecord,
    },
    PresetChanged {
        is_last: bool,
        operation: PresetChangedOperation,
    },
}

impl PresetControlPointEvent {
    pub fn from_bytes(data: &[u8]) -> Result<Self> {
        match data.first().copied() {
            Some(0x02) if data.len() >= 5 => Ok(Self::ReadPresetResponse {
                is_last: data[1] != 0,
                preset_record: PresetRecord::from_bytes(&data[2..])?,
            }),
            Some(0x03) => {
                let (operation, is_last) = PresetChangedOperation::from_bytes(data)?;
                Ok(Self::PresetChanged { is_last, operation })
            }
            _ => Err(Error::InvalidValue(
                "invalid HAP control-point event".into(),
            )),
        }
    }
}

fn validate_name(name: &str) -> Result<()> {
    if !(1..=40).contains(&name.len()) {
        return Err(Error::InvalidValue(format!(
            "preset name has {} bytes, expected 1..=40",
            name.len()
        )));
    }
    Ok(())
}

#[derive(Clone, Debug)]
enum PendingEvent {
    ControlPoint(Vec<u8>),
    ActivePreset(u8),
}

#[derive(Clone, Debug)]
struct HearingAccessState {
    features: HearingAidFeatures,
    presets: BTreeMap<u8, PresetRecord>,
    active_preset_index: u8,
    read_presets_request_in_progress: bool,
    pending_events: VecDeque<PendingEvent>,
}

#[derive(Clone, Debug)]
pub struct HearingAccessService {
    state: Arc<Mutex<HearingAccessState>>,
    other_server: Arc<Mutex<Option<Arc<Mutex<HearingAccessState>>>>>,
}

impl HearingAccessService {
    pub fn new(features: HearingAidFeatures, presets: Vec<PresetRecord>) -> Result<Self> {
        if presets.is_empty() {
            return Err(Error::InvalidValue(
                "Hearing Access Service requires at least one preset".into(),
            ));
        }
        let mut records = BTreeMap::new();
        for preset in presets {
            validate_name(&preset.name)?;
            if preset.index == 0 {
                return Err(Error::InvalidValue("preset index zero is reserved".into()));
            }
            if records.insert(preset.index, preset).is_some() {
                return Err(Error::InvalidValue("duplicate preset index".into()));
            }
        }
        let active_preset_index = *records.keys().next().expect("non-empty preset map");
        Ok(Self {
            state: Arc::new(Mutex::new(HearingAccessState {
                features,
                presets: records,
                active_preset_index,
                read_presets_request_in_progress: false,
                pending_events: VecDeque::new(),
            })),
            other_server: Arc::new(Mutex::new(None)),
        })
    }

    pub fn set_other_server_in_binaural_set(&self, other: &Self) -> Result<()> {
        *self
            .other_server
            .lock()
            .map_err(|_| Error::InvalidValue("HAP peer-server lock is poisoned".into()))? =
            Some(Arc::clone(&other.state));
        Ok(())
    }

    pub fn definition(&self) -> Result<ServiceDefinition> {
        let state = self
            .state
            .lock()
            .map_err(|_| Error::InvalidValue("HAP state lock is poisoned".into()))?;
        Ok(ServiceDefinition {
            uuid: uuid(HEARING_ACCESS_SERVICE),
            primary: true,
            included_services: Vec::new(),
            characteristics: vec![
                CharacteristicDefinition {
                    uuid: uuid(HEARING_AID_FEATURES_CHARACTERISTIC),
                    properties: properties::READ,
                    permissions: permissions::READ_REQUIRES_ENCRYPTION,
                    value: vec![state.features.to_byte()],
                    descriptors: Vec::new(),
                },
                CharacteristicDefinition {
                    uuid: uuid(HEARING_AID_PRESET_CONTROL_POINT_CHARACTERISTIC),
                    properties: properties::WRITE | properties::INDICATE,
                    permissions: permissions::WRITE_REQUIRES_ENCRYPTION,
                    value: Vec::new(),
                    descriptors: Vec::new(),
                },
                CharacteristicDefinition {
                    uuid: uuid(ACTIVE_PRESET_INDEX_CHARACTERISTIC),
                    properties: properties::READ | properties::NOTIFY,
                    permissions: permissions::READ_REQUIRES_ENCRYPTION,
                    value: vec![state.active_preset_index],
                    descriptors: Vec::new(),
                },
            ],
        })
    }

    pub fn bind(&self, server: &mut GattServer) -> Result<HearingAccessHandles> {
        let features = required_handle(server, HEARING_AID_FEATURES_CHARACTERISTIC)?;
        let control_point =
            required_handle(server, HEARING_AID_PRESET_CONTROL_POINT_CHARACTERISTIC)?;
        let active_preset_index = required_handle(server, ACTIVE_PRESET_INDEX_CHARACTERISTIC)?;

        let state = Arc::clone(&self.state);
        server.set_dynamic_value(
            active_preset_index,
            DynamicValue::read_only(move |_| {
                state
                    .lock()
                    .map(|state| vec![state.active_preset_index])
                    .map_err(|_| UNLIKELY_ERROR)
            }),
        )?;
        let state = Arc::clone(&self.state);
        let other_server = Arc::clone(&self.other_server);
        server.set_dynamic_value(
            control_point,
            DynamicValue::write_only(move |_, value| {
                let opcode = value
                    .first()
                    .copied()
                    .ok_or(error_code::INVALID_PARAMETERS_LENGTH)?;
                if !(0x01..=0x0A).contains(&opcode) || matches!(opcode, 0x02 | 0x03) {
                    return Err(error_code::INVALID_OPCODE);
                }
                let operation = PresetControlPointOperation::from_bytes(value)
                    .map_err(|_| error_code::INVALID_PARAMETERS_LENGTH)?;
                {
                    let mut state = state.lock().map_err(|_| UNLIKELY_ERROR)?;
                    process_control_operation(&mut state, &operation)?;
                }
                let peer_operation = match operation {
                    PresetControlPointOperation::SetActivePresetSynchronizedLocally(index) => {
                        Some(PresetControlPointOperation::SetActivePreset(index))
                    }
                    PresetControlPointOperation::SetNextPresetSynchronizedLocally => {
                        Some(PresetControlPointOperation::SetNextPreset)
                    }
                    PresetControlPointOperation::SetPreviousPresetSynchronizedLocally => {
                        Some(PresetControlPointOperation::SetPreviousPreset)
                    }
                    _ => None,
                };
                if let Some(peer_operation) = peer_operation {
                    let peer = other_server.lock().map_err(|_| UNLIKELY_ERROR)?.clone();
                    if let Some(peer) = peer {
                        let mut peer_state = peer.lock().map_err(|_| UNLIKELY_ERROR)?;
                        process_control_operation(&mut peer_state, &peer_operation)?;
                    }
                }
                Ok(())
            }),
        )?;
        Ok(HearingAccessHandles {
            features,
            control_point,
            active_preset_index,
        })
    }

    pub fn active_preset_index(&self) -> Result<u8> {
        self.state
            .lock()
            .map(|state| state.active_preset_index)
            .map_err(|_| Error::InvalidValue("HAP state lock is poisoned".into()))
    }

    pub fn presets(&self) -> Result<Vec<PresetRecord>> {
        self.state
            .lock()
            .map(|state| state.presets.values().cloned().collect())
            .map_err(|_| Error::InvalidValue("HAP state lock is poisoned".into()))
    }

    pub fn generic_update(&self, operation: PresetChangedOperation) -> Result<()> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| Error::InvalidValue("HAP state lock is poisoned".into()))?;
        queue_preset_changes(&mut state, vec![operation])
    }

    pub fn delete_preset(&self, index: u8) -> Result<()> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| Error::InvalidValue("HAP state lock is poisoned".into()))?;
        if index == state.active_preset_index {
            return Err(Error::InvalidValue(
                "cannot delete the active preset".into(),
            ));
        }
        state
            .presets
            .remove(&index)
            .ok_or_else(|| Error::InvalidValue(format!("preset {index} does not exist")))?;
        queue_preset_changes(
            &mut state,
            vec![PresetChangedOperation::PresetRecordDeleted(index)],
        )
    }

    pub fn set_preset_available(&self, index: u8, available: bool) -> Result<()> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| Error::InvalidValue("HAP state lock is poisoned".into()))?;
        if !available && index == state.active_preset_index {
            return Err(Error::InvalidValue(
                "cannot make the active preset unavailable".into(),
            ));
        }
        let preset = state
            .presets
            .get_mut(&index)
            .ok_or_else(|| Error::InvalidValue(format!("preset {index} does not exist")))?;
        preset.properties.is_available = if available {
            PresetAvailability::IS_AVAILABLE
        } else {
            PresetAvailability::IS_UNAVAILABLE
        };
        let operation = if available {
            PresetChangedOperation::PresetRecordAvailable(index)
        } else {
            PresetChangedOperation::PresetRecordUnavailable(index)
        };
        queue_preset_changes(&mut state, vec![operation])
    }

    pub fn take_pending_events(
        &self,
        handles: HearingAccessHandles,
    ) -> Result<Vec<HearingAccessNotification>> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| Error::InvalidValue("HAP state lock is poisoned".into()))?;
        let events = state
            .pending_events
            .drain(..)
            .map(|event| match event {
                PendingEvent::ControlPoint(value) => HearingAccessNotification {
                    handle: handles.control_point,
                    value,
                    indicate: true,
                },
                PendingEvent::ActivePreset(index) => HearingAccessNotification {
                    handle: handles.active_preset_index,
                    value: vec![index],
                    indicate: false,
                },
            })
            .collect::<Vec<_>>();
        if events.iter().any(|event| {
            event.handle == handles.control_point && event.value.get(0..2) == Some(&[0x02, 0x01])
        }) {
            state.read_presets_request_in_progress = false;
        }
        Ok(events)
    }
}

fn process_control_operation(
    state: &mut HearingAccessState,
    operation: &PresetControlPointOperation,
) -> core::result::Result<(), u8> {
    match operation {
        PresetControlPointOperation::ReadPresets { start_index, count } => {
            if state.read_presets_request_in_progress {
                return Err(PROCEDURE_ALREADY_IN_PROGRESS);
            }
            if *start_index == 0 || *count == 0 {
                return Err(OUT_OF_RANGE);
            }
            let presets = state
                .presets
                .range(*start_index..)
                .take(usize::from(*count))
                .map(|(_, preset)| preset.clone())
                .collect::<Vec<_>>();
            if presets.is_empty() {
                return Err(OUT_OF_RANGE);
            }
            state.read_presets_request_in_progress = true;
            let preset_count = presets.len();
            for (index, preset) in presets.into_iter().enumerate() {
                let mut value = vec![0x02, u8::from(index + 1 == preset_count)];
                value.extend_from_slice(&preset.to_bytes().map_err(|_| UNLIKELY_ERROR)?);
                state
                    .pending_events
                    .push_back(PendingEvent::ControlPoint(value));
            }
        }
        PresetControlPointOperation::WritePresetName { index, name } => {
            if state.read_presets_request_in_progress {
                return Err(PROCEDURE_ALREADY_IN_PROGRESS);
            }
            if validate_name(name).is_err() {
                return Err(error_code::INVALID_PARAMETERS_LENGTH);
            }
            let preset = state
                .presets
                .get_mut(index)
                .filter(|preset| preset.properties.writable == PresetWritable::CAN_BE_WRITTEN)
                .ok_or(error_code::WRITE_NAME_NOT_ALLOWED)?;
            preset.name.clone_from(name);
            let operation = PresetChangedOperation::GenericUpdate {
                previous_index: *index,
                preset_record: preset.clone(),
            };
            queue_preset_changes(state, vec![operation]).map_err(|_| UNLIKELY_ERROR)?;
        }
        PresetControlPointOperation::SetActivePreset(index) => set_active(state, *index)?,
        PresetControlPointOperation::SetNextPreset => set_next_or_previous(state, false)?,
        PresetControlPointOperation::SetPreviousPreset => set_next_or_previous(state, true)?,
        PresetControlPointOperation::SetActivePresetSynchronizedLocally(index) => {
            require_synchronization(state)?;
            set_active(state, *index)?;
        }
        PresetControlPointOperation::SetNextPresetSynchronizedLocally => {
            require_synchronization(state)?;
            set_next_or_previous(state, false)?;
        }
        PresetControlPointOperation::SetPreviousPresetSynchronizedLocally => {
            require_synchronization(state)?;
            set_next_or_previous(state, true)?;
        }
    }
    Ok(())
}

fn require_synchronization(state: &HearingAccessState) -> core::result::Result<(), u8> {
    if state.features.preset_synchronization_support
        == PresetSynchronizationSupport::PRESET_SYNCHRONIZATION_IS_NOT_SUPPORTED
    {
        Err(error_code::PRESET_SYNCHRONIZATION_NOT_SUPPORTED)
    } else {
        Ok(())
    }
}

fn set_active(state: &mut HearingAccessState, index: u8) -> core::result::Result<(), u8> {
    if !state
        .presets
        .get(&index)
        .is_some_and(PresetRecord::is_available)
    {
        return Err(error_code::PRESET_OPERATION_NOT_POSSIBLE);
    }
    if state.active_preset_index != index {
        state.active_preset_index = index;
        state
            .pending_events
            .push_back(PendingEvent::ActivePreset(index));
    }
    Ok(())
}

fn set_next_or_previous(
    state: &mut HearingAccessState,
    previous: bool,
) -> core::result::Result<(), u8> {
    let presets = state
        .presets
        .values()
        .filter(|preset| preset.is_available())
        .map(|preset| preset.index)
        .collect::<Vec<_>>();
    if presets.len() < 2 {
        return Err(error_code::PRESET_OPERATION_NOT_POSSIBLE);
    }
    let position = presets
        .iter()
        .position(|index| *index == state.active_preset_index)
        .ok_or(error_code::PRESET_OPERATION_NOT_POSSIBLE)?;
    let next_position = if previous {
        (position + presets.len() - 1) % presets.len()
    } else {
        (position + 1) % presets.len()
    };
    set_active(state, presets[next_position])
}

fn queue_preset_changes(
    state: &mut HearingAccessState,
    mut operations: Vec<PresetChangedOperation>,
) -> Result<()> {
    operations.sort_by_key(PresetChangedOperation::index);
    let count = operations.len();
    for (index, operation) in operations.into_iter().enumerate() {
        state.pending_events.push_back(PendingEvent::ControlPoint(
            operation.to_bytes(index + 1 == count)?,
        ));
    }
    Ok(())
}

fn required_handle(server: &GattServer, characteristic_uuid: u16) -> Result<u16> {
    server
        .handles_by_uuid(&uuid(characteristic_uuid))
        .into_iter()
        .next()
        .ok_or(Error::MissingCharacteristic(characteristic_uuid))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HearingAccessHandles {
    pub features: u16,
    pub control_point: u16,
    pub active_preset_index: u16,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HearingAccessNotification {
    pub handle: u16,
    pub value: Vec<u8>,
    pub indicate: bool,
}

#[derive(Clone, Debug)]
pub struct HearingAccessServiceProxy {
    pub service: ServiceProxy,
    pub server_features: CharacteristicProxy,
    pub hearing_aid_preset_control_point: CharacteristicProxy,
    pub active_preset_index: CharacteristicProxy,
}

impl HearingAccessServiceProxy {
    pub fn from_parts(
        service: ServiceProxy,
        characteristics: &[CharacteristicProxy],
    ) -> Result<Self> {
        Ok(Self {
            service,
            server_features: require_characteristic(
                characteristics,
                HEARING_AID_FEATURES_CHARACTERISTIC,
            )?,
            hearing_aid_preset_control_point: require_characteristic(
                characteristics,
                HEARING_AID_PRESET_CONTROL_POINT_CHARACTERISTIC,
            )?,
            active_preset_index: require_characteristic(
                characteristics,
                ACTIVE_PRESET_INDEX_CHARACTERISTIC,
            )?,
        })
    }

    pub fn discover(
        client: &mut GattClient,
        transport: &mut impl AttTransport,
    ) -> Result<Option<Self>> {
        let Some((service, characteristics)) =
            discover_profile(client, transport, HEARING_ACCESS_SERVICE)?
        else {
            return Ok(None);
        };
        Self::from_parts(service, &characteristics).map(Some)
    }

    pub fn setup_subscription(
        &self,
        client: &mut GattClient,
        transport: &mut impl AttTransport,
    ) -> Result<()> {
        for (characteristic, indications) in [
            (&self.hearing_aid_preset_control_point, true),
            (&self.active_preset_index, false),
        ] {
            let cccd = client
                .discover_descriptors(transport, characteristic)?
                .into_iter()
                .find(|descriptor| descriptor.uuid == uuid(0x2902))
                .ok_or_else(|| {
                    Error::InvalidValue(format!(
                        "HAP characteristic {:?} has no CCCD",
                        characteristic.uuid
                    ))
                })?;
            client.subscribe(transport, characteristic.handle, cccd.handle, indications)?;
        }
        Ok(())
    }

    pub fn read_features(
        &self,
        client: &mut GattClient,
        transport: &mut impl AttTransport,
    ) -> Result<HearingAidFeatures> {
        let value = client.read_value(transport, self.server_features.handle, false)?;
        let [value]: [u8; 1] = value.try_into().map_err(|value: Vec<u8>| {
            Error::InvalidValue(format!(
                "Hearing Aid Features has length {}, expected 1",
                value.len()
            ))
        })?;
        Ok(HearingAidFeatures::from_byte(value))
    }

    pub fn read_active_preset_index(
        &self,
        client: &mut GattClient,
        transport: &mut impl AttTransport,
    ) -> Result<u8> {
        let value = client.read_value(transport, self.active_preset_index.handle, false)?;
        let [index]: [u8; 1] = value.try_into().map_err(|value: Vec<u8>| {
            Error::InvalidValue(format!(
                "Active Preset Index has length {}, expected 1",
                value.len()
            ))
        })?;
        Ok(index)
    }

    pub fn write_control_point(
        &self,
        client: &mut GattClient,
        transport: &mut impl AttTransport,
        operation: &PresetControlPointOperation,
    ) -> Result<()> {
        client.write_value(
            transport,
            self.hearing_aid_preset_control_point.handle,
            operation.to_bytes()?,
            true,
        )?;
        Ok(())
    }

    pub fn event_from_indication(
        &self,
        handle: u16,
        value: &[u8],
    ) -> Result<PresetControlPointEvent> {
        if handle != self.hearing_aid_preset_control_point.handle {
            return Err(Error::InvalidValue(format!(
                "indication handle 0x{handle:04X} is not the HAP control point"
            )));
        }
        PresetControlPointEvent::from_bytes(value)
    }

    pub fn active_index_from_notification(&self, handle: u16, value: &[u8]) -> Result<u8> {
        if handle != self.active_preset_index.handle || value.len() != 1 {
            return Err(Error::InvalidValue(
                "invalid Active Preset Index notification".into(),
            ));
        }
        Ok(value[0])
    }
}
