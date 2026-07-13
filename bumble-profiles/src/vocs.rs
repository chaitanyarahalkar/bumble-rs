//! Volume Offset Control Service (VOCS).

use crate::{discover_secondary_profile, require_characteristic, uuid, Error, Result};
use bumble_gatt::{
    permissions, properties, AttTransport, CharacteristicDefinition, CharacteristicProxy,
    DynamicValue, GattClient, GattServer, ServiceDefinition, ServiceProxy,
};
use std::ops::{BitOr, BitOrAssign};
use std::sync::{Arc, Mutex};

pub const VOLUME_OFFSET_CONTROL_SERVICE: u16 = 0x1845;
pub const VOLUME_OFFSET_STATE_CHARACTERISTIC: u16 = 0x2B80;
pub const AUDIO_LOCATION_CHARACTERISTIC: u16 = 0x2B81;
pub const VOLUME_OFFSET_CONTROL_POINT_CHARACTERISTIC: u16 = 0x2B82;
pub const AUDIO_OUTPUT_DESCRIPTION_CHARACTERISTIC: u16 = 0x2B83;
pub const MIN_VOLUME_OFFSET: i16 = -255;
pub const MAX_VOLUME_OFFSET: i16 = 255;

pub mod error_code {
    pub const INVALID_CHANGE_COUNTER: u8 = 0x80;
    pub const OPCODE_NOT_SUPPORTED: u8 = 0x81;
    pub const VALUE_OUT_OF_RANGE: u8 = 0x82;
}

pub const SET_VOLUME_OFFSET_OPCODE: u8 = 0x01;
const INVALID_ATTRIBUTE_VALUE_LENGTH: u8 = 0x0D;
const UNLIKELY_ERROR: u8 = 0x0E;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AudioLocation(pub u32);

impl AudioLocation {
    pub const NOT_ALLOWED: Self = Self(0x0000_0000);
    pub const FRONT_LEFT: Self = Self(0x0000_0001);
    pub const FRONT_RIGHT: Self = Self(0x0000_0002);
    pub const FRONT_CENTER: Self = Self(0x0000_0004);
    pub const LOW_FREQUENCY_EFFECTS_1: Self = Self(0x0000_0008);
    pub const BACK_LEFT: Self = Self(0x0000_0010);
    pub const BACK_RIGHT: Self = Self(0x0000_0020);
    pub const FRONT_LEFT_OF_CENTER: Self = Self(0x0000_0040);
    pub const FRONT_RIGHT_OF_CENTER: Self = Self(0x0000_0080);
    pub const BACK_CENTER: Self = Self(0x0000_0100);
    pub const LOW_FREQUENCY_EFFECTS_2: Self = Self(0x0000_0200);
    pub const SIDE_LEFT: Self = Self(0x0000_0400);
    pub const SIDE_RIGHT: Self = Self(0x0000_0800);
    pub const TOP_FRONT_LEFT: Self = Self(0x0000_1000);
    pub const TOP_FRONT_RIGHT: Self = Self(0x0000_2000);
    pub const TOP_FRONT_CENTER: Self = Self(0x0000_4000);
    pub const TOP_CENTER: Self = Self(0x0000_8000);
    pub const TOP_BACK_LEFT: Self = Self(0x0001_0000);
    pub const TOP_BACK_RIGHT: Self = Self(0x0002_0000);
    pub const TOP_SIDE_LEFT: Self = Self(0x0004_0000);
    pub const TOP_SIDE_RIGHT: Self = Self(0x0008_0000);
    pub const TOP_BACK_CENTER: Self = Self(0x0010_0000);
    pub const BOTTOM_FRONT_CENTER: Self = Self(0x0020_0000);
    pub const BOTTOM_FRONT_LEFT: Self = Self(0x0040_0000);
    pub const BOTTOM_FRONT_RIGHT: Self = Self(0x0080_0000);
    pub const FRONT_LEFT_WIDE: Self = Self(0x0100_0000);
    pub const FRONT_RIGHT_WIDE: Self = Self(0x0200_0000);
    pub const LEFT_SURROUND: Self = Self(0x0400_0000);
    pub const RIGHT_SURROUND: Self = Self(0x0800_0000);

    pub fn channel_count(self) -> u32 {
        self.0.count_ones()
    }
}

impl BitOr for AudioLocation {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self::Output {
        Self(self.0 | rhs.0)
    }
}

impl BitOrAssign for AudioLocation {
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct VolumeOffsetState {
    pub volume_offset: i16,
    pub change_counter: u8,
}

impl VolumeOffsetState {
    pub fn encode(self) -> [u8; 3] {
        let offset = self.volume_offset.to_le_bytes();
        [offset[0], offset[1], self.change_counter]
    }

    pub fn decode(value: &[u8]) -> Result<Self> {
        if value.len() != 3 {
            return Err(Error::InvalidValue(format!(
                "volume-offset state has length {}, expected 3",
                value.len()
            )));
        }
        Ok(Self {
            volume_offset: i16::from_le_bytes([value[0], value[1]]),
            change_counter: value[2],
        })
    }
}

#[derive(Clone, Debug)]
pub struct VolumeOffsetControlService {
    state: Arc<Mutex<VolumeOffsetState>>,
    audio_location: Arc<Mutex<AudioLocation>>,
    audio_output_description: Arc<Mutex<String>>,
}

impl Default for VolumeOffsetControlService {
    fn default() -> Self {
        Self::new()
    }
}

impl VolumeOffsetControlService {
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(VolumeOffsetState::default())),
            audio_location: Arc::new(Mutex::new(AudioLocation::NOT_ALLOWED)),
            audio_output_description: Arc::new(Mutex::new(String::new())),
        }
    }

    pub fn initial_state(mut self, state: VolumeOffsetState) -> Self {
        self.state = Arc::new(Mutex::new(state));
        self
    }

    pub fn audio_location(mut self, location: AudioLocation) -> Self {
        self.audio_location = Arc::new(Mutex::new(location));
        self
    }

    pub fn audio_output_description(mut self, description: impl Into<String>) -> Self {
        self.audio_output_description = Arc::new(Mutex::new(description.into()));
        self
    }

    pub fn state(&self) -> Result<VolumeOffsetState> {
        self.state
            .lock()
            .map(|state| *state)
            .map_err(|_| Error::InvalidValue("VOCS state lock is poisoned".into()))
    }

    pub fn definition(&self) -> ServiceDefinition {
        let read_encrypted = permissions::READ_REQUIRES_ENCRYPTION;
        let write_encrypted = permissions::WRITE_REQUIRES_ENCRYPTION;
        ServiceDefinition {
            uuid: uuid(VOLUME_OFFSET_CONTROL_SERVICE),
            primary: false,
            included_services: Vec::new(),
            characteristics: vec![
                characteristic(
                    VOLUME_OFFSET_STATE_CHARACTERISTIC,
                    properties::READ | properties::NOTIFY,
                    read_encrypted,
                    self.state().unwrap_or_default().encode().to_vec(),
                ),
                characteristic(
                    AUDIO_LOCATION_CHARACTERISTIC,
                    properties::READ | properties::NOTIFY | properties::WRITE_WITHOUT_RESPONSE,
                    read_encrypted | write_encrypted,
                    self.location_or_default().0.to_le_bytes().to_vec(),
                ),
                characteristic(
                    VOLUME_OFFSET_CONTROL_POINT_CHARACTERISTIC,
                    properties::WRITE,
                    write_encrypted,
                    Vec::new(),
                ),
                characteristic(
                    AUDIO_OUTPUT_DESCRIPTION_CHARACTERISTIC,
                    properties::READ | properties::NOTIFY | properties::WRITE_WITHOUT_RESPONSE,
                    read_encrypted | write_encrypted,
                    self.description_or_default().into_bytes(),
                ),
            ],
        }
    }

    fn location_or_default(&self) -> AudioLocation {
        self.audio_location
            .lock()
            .map(|location| *location)
            .unwrap_or_default()
    }

    fn description_or_default(&self) -> String {
        self.audio_output_description
            .lock()
            .map(|description| description.clone())
            .unwrap_or_default()
    }

    pub fn bind(&self, server: &mut GattServer) -> Result<VolumeOffsetControlHandles> {
        let state_handle = required_handle(server, VOLUME_OFFSET_STATE_CHARACTERISTIC)?;
        let location_handle = required_handle(server, AUDIO_LOCATION_CHARACTERISTIC)?;
        let control_handle = required_handle(server, VOLUME_OFFSET_CONTROL_POINT_CHARACTERISTIC)?;
        let description_handle = required_handle(server, AUDIO_OUTPUT_DESCRIPTION_CHARACTERISTIC)?;

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

        let read_location = Arc::clone(&self.audio_location);
        let write_location = Arc::clone(&self.audio_location);
        server.set_dynamic_value(
            location_handle,
            DynamicValue::read_write(
                move |_| {
                    read_location
                        .lock()
                        .map(|location| location.0.to_le_bytes().to_vec())
                        .map_err(|_| UNLIKELY_ERROR)
                },
                move |_, value| {
                    if value.len() != 4 {
                        return Err(INVALID_ATTRIBUTE_VALUE_LENGTH);
                    }
                    let location = u32::from_le_bytes(value.try_into().expect("four-byte value"));
                    *write_location.lock().map_err(|_| UNLIKELY_ERROR)? = AudioLocation(location);
                    Ok(())
                },
            ),
        )?;

        let write_state = Arc::clone(&self.state);
        server.set_dynamic_value(
            control_handle,
            DynamicValue::write_only(move |_, value| update_volume_offset(&write_state, value)),
        )?;

        let read_description = Arc::clone(&self.audio_output_description);
        let write_description = Arc::clone(&self.audio_output_description);
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

        Ok(VolumeOffsetControlHandles {
            volume_offset_state: state_handle,
            audio_location: location_handle,
            volume_offset_control_point: control_handle,
            audio_output_description: description_handle,
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

fn update_volume_offset(
    state: &Mutex<VolumeOffsetState>,
    value: &[u8],
) -> core::result::Result<(), u8> {
    let Some(&opcode) = value.first() else {
        return Err(INVALID_ATTRIBUTE_VALUE_LENGTH);
    };
    if opcode != SET_VOLUME_OFFSET_OPCODE {
        return Err(error_code::OPCODE_NOT_SUPPORTED);
    }
    if value.len() != 4 {
        return Err(INVALID_ATTRIBUTE_VALUE_LENGTH);
    }
    let mut state = state.lock().map_err(|_| UNLIKELY_ERROR)?;
    if value[1] != state.change_counter {
        return Err(error_code::INVALID_CHANGE_COUNTER);
    }
    let offset = i16::from_le_bytes([value[2], value[3]]);
    if !(MIN_VOLUME_OFFSET..=MAX_VOLUME_OFFSET).contains(&offset) {
        return Err(error_code::VALUE_OUT_OF_RANGE);
    }
    state.volume_offset = offset;
    state.change_counter = state.change_counter.wrapping_add(1);
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VolumeOffsetControlHandles {
    pub volume_offset_state: u16,
    pub audio_location: u16,
    pub volume_offset_control_point: u16,
    pub audio_output_description: u16,
}

#[derive(Clone, Debug)]
pub struct VolumeOffsetControlServiceProxy {
    pub service: ServiceProxy,
    pub volume_offset_state: CharacteristicProxy,
    pub audio_location: CharacteristicProxy,
    pub volume_offset_control_point: CharacteristicProxy,
    pub audio_output_description: CharacteristicProxy,
}

impl VolumeOffsetControlServiceProxy {
    pub fn from_parts(
        service: ServiceProxy,
        characteristics: &[CharacteristicProxy],
    ) -> Result<Self> {
        Ok(Self {
            service,
            volume_offset_state: require_characteristic(
                characteristics,
                VOLUME_OFFSET_STATE_CHARACTERISTIC,
            )?,
            audio_location: require_characteristic(characteristics, AUDIO_LOCATION_CHARACTERISTIC)?,
            volume_offset_control_point: require_characteristic(
                characteristics,
                VOLUME_OFFSET_CONTROL_POINT_CHARACTERISTIC,
            )?,
            audio_output_description: require_characteristic(
                characteristics,
                AUDIO_OUTPUT_DESCRIPTION_CHARACTERISTIC,
            )?,
        })
    }

    pub fn discover(
        client: &mut GattClient,
        transport: &mut impl AttTransport,
    ) -> Result<Option<Self>> {
        let Some((service, characteristics)) =
            discover_secondary_profile(client, transport, VOLUME_OFFSET_CONTROL_SERVICE)?
        else {
            return Ok(None);
        };
        Self::from_parts(service, &characteristics).map(Some)
    }

    pub fn read_volume_offset_state(
        &self,
        client: &mut GattClient,
        transport: &mut impl AttTransport,
    ) -> Result<VolumeOffsetState> {
        VolumeOffsetState::decode(&client.read_value(
            transport,
            self.volume_offset_state.handle,
            false,
        )?)
    }

    pub fn read_audio_location(
        &self,
        client: &mut GattClient,
        transport: &mut impl AttTransport,
    ) -> Result<AudioLocation> {
        let value = client.read_value(transport, self.audio_location.handle, false)?;
        let bytes: [u8; 4] = value.try_into().map_err(|value: Vec<u8>| {
            Error::InvalidValue(format!(
                "audio location has length {}, expected 4",
                value.len()
            ))
        })?;
        Ok(AudioLocation(u32::from_le_bytes(bytes)))
    }

    pub fn write_audio_location(
        &self,
        client: &mut GattClient,
        transport: &mut impl AttTransport,
        location: AudioLocation,
    ) -> Result<()> {
        client.write_value(
            transport,
            self.audio_location.handle,
            location.0.to_le_bytes().to_vec(),
            false,
        )?;
        Ok(())
    }

    pub fn set_volume_offset(
        &self,
        client: &mut GattClient,
        transport: &mut impl AttTransport,
        change_counter: u8,
        offset: i16,
    ) -> Result<()> {
        let mut value = vec![SET_VOLUME_OFFSET_OPCODE, change_counter];
        value.extend_from_slice(&offset.to_le_bytes());
        client.write_value(
            transport,
            self.volume_offset_control_point.handle,
            value,
            true,
        )?;
        Ok(())
    }

    pub fn read_audio_output_description(
        &self,
        client: &mut GattClient,
        transport: &mut impl AttTransport,
    ) -> Result<String> {
        let value = client.read_value(transport, self.audio_output_description.handle, false)?;
        String::from_utf8(value)
            .map_err(|error| Error::InvalidValue(format!("invalid UTF-8 description: {error}")))
    }

    pub fn write_audio_output_description(
        &self,
        client: &mut GattClient,
        transport: &mut impl AttTransport,
        description: &str,
    ) -> Result<()> {
        client.write_value(
            transport,
            self.audio_output_description.handle,
            description.as_bytes().to_vec(),
            false,
        )?;
        Ok(())
    }
}
