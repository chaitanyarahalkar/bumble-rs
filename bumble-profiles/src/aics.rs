//! Audio Input Control Service (AICS).

use crate::{discover_secondary_profile, require_characteristic, uuid, Error, Result};
use bumble_gatt::{
    permissions, properties, AttTransport, CharacteristicDefinition, CharacteristicProxy,
    DynamicValue, GattClient, GattServer, ServiceDefinition, ServiceProxy,
};
use std::sync::{Arc, Mutex};

pub const AUDIO_INPUT_CONTROL_SERVICE: u16 = 0x1843;
pub const AUDIO_INPUT_STATE_CHARACTERISTIC: u16 = 0x2B77;
pub const GAIN_SETTINGS_ATTRIBUTE_CHARACTERISTIC: u16 = 0x2B78;
pub const AUDIO_INPUT_TYPE_CHARACTERISTIC: u16 = 0x2B79;
pub const AUDIO_INPUT_STATUS_CHARACTERISTIC: u16 = 0x2B7A;
pub const AUDIO_INPUT_CONTROL_POINT_CHARACTERISTIC: u16 = 0x2B7B;
pub const AUDIO_INPUT_DESCRIPTION_CHARACTERISTIC: u16 = 0x2B7C;

pub mod error_code {
    pub const INVALID_CHANGE_COUNTER: u8 = 0x80;
    pub const OPCODE_NOT_SUPPORTED: u8 = 0x81;
    pub const MUTE_DISABLED: u8 = 0x82;
    pub const VALUE_OUT_OF_RANGE: u8 = 0x83;
    pub const GAIN_MODE_CHANGE_NOT_ALLOWED: u8 = 0x84;
}

const INVALID_ATTRIBUTE_VALUE_LENGTH: u8 = 0x0D;
const UNLIKELY_ERROR: u8 = 0x0E;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Mute(pub u8);

impl Mute {
    pub const NOT_MUTED: Self = Self(0x00);
    pub const MUTED: Self = Self(0x01);
    pub const DISABLED: Self = Self(0x02);
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GainMode(pub u8);

impl GainMode {
    pub const MANUAL_ONLY: Self = Self(0x00);
    pub const AUTOMATIC_ONLY: Self = Self(0x01);
    pub const MANUAL: Self = Self(0x02);
    pub const AUTOMATIC: Self = Self(0x03);
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AudioInputStatus(pub u8);

impl AudioInputStatus {
    pub const INACTIVE: Self = Self(0x00);
    pub const ACTIVE: Self = Self(0x01);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AudioInputControlPointOpcode(pub u8);

impl AudioInputControlPointOpcode {
    pub const SET_GAIN_SETTING: Self = Self(0x01);
    pub const UNMUTE: Self = Self(0x02);
    pub const MUTE: Self = Self(0x03);
    pub const SET_MANUAL_GAIN_MODE: Self = Self(0x04);
    pub const SET_AUTOMATIC_GAIN_MODE: Self = Self(0x05);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AudioInputState {
    pub gain_settings: u8,
    pub mute: Mute,
    pub gain_mode: GainMode,
    pub change_counter: u8,
}

impl Default for AudioInputState {
    fn default() -> Self {
        Self {
            gain_settings: 0,
            mute: Mute::NOT_MUTED,
            gain_mode: GainMode::MANUAL,
            change_counter: 0,
        }
    }
}

impl AudioInputState {
    pub fn encode(self) -> [u8; 4] {
        [
            self.gain_settings,
            self.mute.0,
            self.gain_mode.0,
            self.change_counter,
        ]
    }

    pub fn decode(value: &[u8]) -> Result<Self> {
        if value.len() != 4 {
            return Err(Error::InvalidValue(format!(
                "audio input state has length {}, expected 4",
                value.len()
            )));
        }
        Ok(Self {
            gain_settings: value[0],
            mute: Mute(value[1]),
            gain_mode: GainMode(value[2]),
            change_counter: value[3],
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GainSettingsProperties {
    pub gain_settings_unit: u8,
    pub gain_settings_minimum: u8,
    pub gain_settings_maximum: u8,
}

impl Default for GainSettingsProperties {
    fn default() -> Self {
        Self {
            gain_settings_unit: 1,
            gain_settings_minimum: 0,
            gain_settings_maximum: 255,
        }
    }
}

impl GainSettingsProperties {
    pub fn encode(self) -> [u8; 3] {
        [
            self.gain_settings_unit,
            self.gain_settings_minimum,
            self.gain_settings_maximum,
        ]
    }

    pub fn decode(value: &[u8]) -> Result<Self> {
        if value.len() != 3 {
            return Err(Error::InvalidValue(format!(
                "gain settings properties have length {}, expected 3",
                value.len()
            )));
        }
        Ok(Self {
            gain_settings_unit: value[0],
            gain_settings_minimum: value[1],
            gain_settings_maximum: value[2],
        })
    }
}

#[derive(Clone, Debug)]
pub struct AudioInputControlService {
    state: Arc<Mutex<AudioInputState>>,
    gain_settings_properties: GainSettingsProperties,
    audio_input_type: String,
    audio_input_status: AudioInputStatus,
    audio_input_description: Arc<Mutex<String>>,
}

impl Default for AudioInputControlService {
    fn default() -> Self {
        Self::new()
    }
}

impl AudioInputControlService {
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(AudioInputState::default())),
            gain_settings_properties: GainSettingsProperties::default(),
            audio_input_type: "local".into(),
            audio_input_status: AudioInputStatus::ACTIVE,
            audio_input_description: Arc::new(Mutex::new("Bluetooth".into())),
        }
    }

    pub fn initial_state(mut self, state: AudioInputState) -> Self {
        self.state = Arc::new(Mutex::new(state));
        self
    }

    pub fn gain_settings_properties(mut self, properties: GainSettingsProperties) -> Self {
        self.gain_settings_properties = properties;
        self
    }

    pub fn audio_input_type(mut self, input_type: impl Into<String>) -> Self {
        self.audio_input_type = input_type.into();
        self
    }

    pub fn audio_input_status(mut self, status: AudioInputStatus) -> Self {
        self.audio_input_status = status;
        self
    }

    pub fn audio_input_description(mut self, description: impl Into<String>) -> Self {
        self.audio_input_description = Arc::new(Mutex::new(description.into()));
        self
    }

    pub fn state(&self) -> Result<AudioInputState> {
        self.state
            .lock()
            .map(|state| *state)
            .map_err(|_| Error::InvalidValue("AICS state lock is poisoned".into()))
    }

    pub fn definition(&self) -> ServiceDefinition {
        let read_encrypted = permissions::READ_REQUIRES_ENCRYPTION;
        let write_encrypted = permissions::WRITE_REQUIRES_ENCRYPTION;
        ServiceDefinition {
            uuid: uuid(AUDIO_INPUT_CONTROL_SERVICE),
            primary: false,
            included_services: Vec::new(),
            characteristics: vec![
                characteristic(
                    AUDIO_INPUT_STATE_CHARACTERISTIC,
                    properties::READ | properties::NOTIFY,
                    read_encrypted,
                    self.state().unwrap_or_default().encode().to_vec(),
                ),
                characteristic(
                    GAIN_SETTINGS_ATTRIBUTE_CHARACTERISTIC,
                    properties::READ,
                    read_encrypted,
                    self.gain_settings_properties.encode().to_vec(),
                ),
                characteristic(
                    AUDIO_INPUT_TYPE_CHARACTERISTIC,
                    properties::READ,
                    read_encrypted,
                    self.audio_input_type.as_bytes().to_vec(),
                ),
                characteristic(
                    AUDIO_INPUT_STATUS_CHARACTERISTIC,
                    properties::READ,
                    read_encrypted,
                    vec![self.audio_input_status.0],
                ),
                characteristic(
                    AUDIO_INPUT_CONTROL_POINT_CHARACTERISTIC,
                    properties::WRITE,
                    write_encrypted,
                    Vec::new(),
                ),
                characteristic(
                    AUDIO_INPUT_DESCRIPTION_CHARACTERISTIC,
                    properties::READ | properties::NOTIFY | properties::WRITE_WITHOUT_RESPONSE,
                    read_encrypted | write_encrypted,
                    self.description_or_default().into_bytes(),
                ),
            ],
        }
    }

    fn description_or_default(&self) -> String {
        self.audio_input_description
            .lock()
            .map(|description| description.clone())
            .unwrap_or_default()
    }

    pub fn bind(&self, server: &mut GattServer) -> Result<AudioInputControlHandles> {
        let state_handle = required_handle(server, AUDIO_INPUT_STATE_CHARACTERISTIC)?;
        let control_handle = required_handle(server, AUDIO_INPUT_CONTROL_POINT_CHARACTERISTIC)?;
        let description_handle = required_handle(server, AUDIO_INPUT_DESCRIPTION_CHARACTERISTIC)?;

        let read_state = Arc::clone(&self.state);
        server.set_dynamic_value(
            state_handle,
            DynamicValue::read_only(move |_| {
                read_state
                    .lock()
                    .map(|state| state.encode().to_vec())
                    .map_err(|_| UNLIKELY_ERROR)
            }),
        )?;

        let write_state = Arc::clone(&self.state);
        let properties = self.gain_settings_properties;
        server.set_dynamic_value(
            control_handle,
            DynamicValue::write_only(move |_, value| {
                update_audio_input(&write_state, properties, value)
            }),
        )?;

        let read_description = Arc::clone(&self.audio_input_description);
        let write_description = Arc::clone(&self.audio_input_description);
        server.set_dynamic_value(
            description_handle,
            DynamicValue::read_write(
                move |_| {
                    read_description
                        .lock()
                        .map(|description| description.as_bytes().to_vec())
                        .map_err(|_| UNLIKELY_ERROR)
                },
                move |_, value| {
                    let description = core::str::from_utf8(value)
                        .map_err(|_| INVALID_ATTRIBUTE_VALUE_LENGTH)?
                        .to_owned();
                    *write_description.lock().map_err(|_| UNLIKELY_ERROR)? = description;
                    Ok(())
                },
            ),
        )?;

        Ok(AudioInputControlHandles {
            audio_input_state: state_handle,
            audio_input_control_point: control_handle,
            audio_input_description: description_handle,
        })
    }
}

fn characteristic(
    characteristic_uuid: u16,
    characteristic_properties: u8,
    characteristic_permissions: u8,
    value: Vec<u8>,
) -> CharacteristicDefinition {
    CharacteristicDefinition {
        uuid: uuid(characteristic_uuid),
        properties: characteristic_properties,
        permissions: characteristic_permissions,
        value,
        descriptors: Vec::new(),
    }
}

fn required_handle(server: &GattServer, characteristic_uuid: u16) -> Result<u16> {
    server
        .handles_by_uuid(&uuid(characteristic_uuid))
        .into_iter()
        .next()
        .ok_or(Error::MissingCharacteristic(characteristic_uuid))
}

fn update_audio_input(
    state: &Mutex<AudioInputState>,
    properties: GainSettingsProperties,
    value: &[u8],
) -> core::result::Result<(), u8> {
    let Some(&opcode) = value.first() else {
        return Err(INVALID_ATTRIBUTE_VALUE_LENGTH);
    };
    let mut state = state.lock().map_err(|_| UNLIKELY_ERROR)?;
    match AudioInputControlPointOpcode(opcode) {
        AudioInputControlPointOpcode::SET_GAIN_SETTING => {
            let setting = value
                .get(2)
                .copied()
                .ok_or(INVALID_ATTRIBUTE_VALUE_LENGTH)?;
            if state.gain_mode != GainMode::MANUAL && state.gain_mode != GainMode::MANUAL_ONLY {
                return Ok(());
            }
            if !(properties.gain_settings_minimum..=properties.gain_settings_maximum)
                .contains(&setting)
            {
                return Err(error_code::VALUE_OUT_OF_RANGE);
            }
            state.gain_settings = setting;
        }
        AudioInputControlPointOpcode::UNMUTE => {
            if state.mute == Mute::DISABLED {
                return Err(error_code::MUTE_DISABLED);
            }
            if state.mute != Mute::NOT_MUTED {
                state.mute = Mute::NOT_MUTED;
                state.change_counter = state.change_counter.wrapping_add(1);
            }
        }
        AudioInputControlPointOpcode::MUTE => {
            if state.mute == Mute::DISABLED {
                return Err(error_code::MUTE_DISABLED);
            }
            let counter = value
                .get(1)
                .copied()
                .ok_or(INVALID_ATTRIBUTE_VALUE_LENGTH)?;
            if counter != state.change_counter {
                return Err(error_code::INVALID_CHANGE_COUNTER);
            }
            if state.mute != Mute::MUTED {
                state.mute = Mute::MUTED;
                state.change_counter = state.change_counter.wrapping_add(1);
            }
        }
        AudioInputControlPointOpcode::SET_MANUAL_GAIN_MODE => {
            if state.gain_mode == GainMode::AUTOMATIC_ONLY
                || state.gain_mode == GainMode::MANUAL_ONLY
            {
                return Err(error_code::GAIN_MODE_CHANGE_NOT_ALLOWED);
            }
            if state.gain_mode != GainMode::MANUAL {
                state.gain_mode = GainMode::MANUAL;
                state.change_counter = state.change_counter.wrapping_add(1);
            }
        }
        AudioInputControlPointOpcode::SET_AUTOMATIC_GAIN_MODE => {
            if state.gain_mode == GainMode::AUTOMATIC_ONLY
                || state.gain_mode == GainMode::MANUAL_ONLY
            {
                return Err(error_code::GAIN_MODE_CHANGE_NOT_ALLOWED);
            }
            if state.gain_mode != GainMode::AUTOMATIC {
                state.gain_mode = GainMode::AUTOMATIC;
                state.change_counter = state.change_counter.wrapping_add(1);
            }
        }
        _ => return Err(error_code::OPCODE_NOT_SUPPORTED),
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AudioInputControlHandles {
    pub audio_input_state: u16,
    pub audio_input_control_point: u16,
    pub audio_input_description: u16,
}

#[derive(Clone, Debug)]
pub struct AudioInputControlServiceProxy {
    pub service: ServiceProxy,
    pub audio_input_state: CharacteristicProxy,
    pub gain_settings_properties: CharacteristicProxy,
    pub audio_input_type: CharacteristicProxy,
    pub audio_input_status: CharacteristicProxy,
    pub audio_input_control_point: CharacteristicProxy,
    pub audio_input_description: CharacteristicProxy,
}

impl AudioInputControlServiceProxy {
    pub fn from_parts(
        service: ServiceProxy,
        characteristics: &[CharacteristicProxy],
    ) -> Result<Self> {
        Ok(Self {
            service,
            audio_input_state: require_characteristic(
                characteristics,
                AUDIO_INPUT_STATE_CHARACTERISTIC,
            )?,
            gain_settings_properties: require_characteristic(
                characteristics,
                GAIN_SETTINGS_ATTRIBUTE_CHARACTERISTIC,
            )?,
            audio_input_type: require_characteristic(
                characteristics,
                AUDIO_INPUT_TYPE_CHARACTERISTIC,
            )?,
            audio_input_status: require_characteristic(
                characteristics,
                AUDIO_INPUT_STATUS_CHARACTERISTIC,
            )?,
            audio_input_control_point: require_characteristic(
                characteristics,
                AUDIO_INPUT_CONTROL_POINT_CHARACTERISTIC,
            )?,
            audio_input_description: require_characteristic(
                characteristics,
                AUDIO_INPUT_DESCRIPTION_CHARACTERISTIC,
            )?,
        })
    }

    pub fn discover(
        client: &mut GattClient,
        transport: &mut impl AttTransport,
    ) -> Result<Option<Self>> {
        let Some((service, characteristics)) =
            discover_secondary_profile(client, transport, AUDIO_INPUT_CONTROL_SERVICE)?
        else {
            return Ok(None);
        };
        Self::from_parts(service, &characteristics).map(Some)
    }

    pub fn read_audio_input_state(
        &self,
        client: &mut GattClient,
        transport: &mut impl AttTransport,
    ) -> Result<AudioInputState> {
        AudioInputState::decode(&client.read_value(
            transport,
            self.audio_input_state.handle,
            false,
        )?)
    }

    pub fn read_gain_settings_properties(
        &self,
        client: &mut GattClient,
        transport: &mut impl AttTransport,
    ) -> Result<GainSettingsProperties> {
        GainSettingsProperties::decode(&client.read_value(
            transport,
            self.gain_settings_properties.handle,
            false,
        )?)
    }

    pub fn read_audio_input_status(
        &self,
        client: &mut GattClient,
        transport: &mut impl AttTransport,
    ) -> Result<AudioInputStatus> {
        let value = client.read_value(transport, self.audio_input_status.handle, false)?;
        value
            .first()
            .copied()
            .map(AudioInputStatus)
            .ok_or_else(|| Error::InvalidValue("audio input status is empty".into()))
    }

    pub fn read_audio_input_type(
        &self,
        client: &mut GattClient,
        transport: &mut impl AttTransport,
    ) -> Result<String> {
        let value = client.read_value(transport, self.audio_input_type.handle, false)?;
        String::from_utf8(value)
            .map_err(|error| Error::InvalidValue(format!("invalid audio input type: {error}")))
    }

    pub fn write_control_point(
        &self,
        client: &mut GattClient,
        transport: &mut impl AttTransport,
        value: Vec<u8>,
    ) -> Result<()> {
        client.write_value(
            transport,
            self.audio_input_control_point.handle,
            value,
            true,
        )?;
        Ok(())
    }

    pub fn read_audio_input_description(
        &self,
        client: &mut GattClient,
        transport: &mut impl AttTransport,
    ) -> Result<String> {
        let value = client.read_value(transport, self.audio_input_description.handle, false)?;
        String::from_utf8(value)
            .map_err(|error| Error::InvalidValue(format!("invalid UTF-8 description: {error}")))
    }

    pub fn write_audio_input_description(
        &self,
        client: &mut GattClient,
        transport: &mut impl AttTransport,
        description: &str,
    ) -> Result<()> {
        client.write_value(
            transport,
            self.audio_input_description.handle,
            description.as_bytes().to_vec(),
            false,
        )?;
        Ok(())
    }
}
