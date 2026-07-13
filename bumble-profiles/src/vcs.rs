//! Volume Control Service (VCS).

use crate::{discover_profile, require_characteristic, uuid, Error, Result};
use bumble_gatt::{
    permissions, properties, AttTransport, CharacteristicDefinition, CharacteristicProxy,
    DynamicValue, GattClient, GattServer, ServiceDefinition, ServiceProxy,
};
use std::sync::{Arc, Mutex};

pub const VOLUME_CONTROL_SERVICE: u16 = 0x1844;
pub const VOLUME_STATE_CHARACTERISTIC: u16 = 0x2B7D;
pub const VOLUME_CONTROL_POINT_CHARACTERISTIC: u16 = 0x2B7E;
pub const VOLUME_FLAGS_CHARACTERISTIC: u16 = 0x2B7F;
pub const MIN_VOLUME: u8 = 0;
pub const MAX_VOLUME: u8 = 255;

pub mod error_code {
    pub const INVALID_CHANGE_COUNTER: u8 = 0x80;
    pub const OPCODE_NOT_SUPPORTED: u8 = 0x81;
}

const INVALID_ATTRIBUTE_VALUE_LENGTH: u8 = 0x0D;
const UNLIKELY_ERROR: u8 = 0x0E;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct VolumeFlags(pub u8);

impl VolumeFlags {
    pub const VOLUME_SETTING_PERSISTED: Self = Self(0x01);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VolumeControlPointOpcode(pub u8);

impl VolumeControlPointOpcode {
    pub const RELATIVE_VOLUME_DOWN: Self = Self(0x00);
    pub const RELATIVE_VOLUME_UP: Self = Self(0x01);
    pub const UNMUTE_RELATIVE_VOLUME_DOWN: Self = Self(0x02);
    pub const UNMUTE_RELATIVE_VOLUME_UP: Self = Self(0x03);
    pub const SET_ABSOLUTE_VOLUME: Self = Self(0x04);
    pub const UNMUTE: Self = Self(0x05);
    pub const MUTE: Self = Self(0x06);
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct VolumeState {
    pub volume_setting: u8,
    pub mute: u8,
    pub change_counter: u8,
}

impl VolumeState {
    pub fn encode(self) -> [u8; 3] {
        [self.volume_setting, self.mute, self.change_counter]
    }

    pub fn decode(value: &[u8]) -> Result<Self> {
        if value.len() != 3 {
            return Err(Error::InvalidValue(format!(
                "volume state has length {}, expected 3",
                value.len()
            )));
        }
        Ok(Self {
            volume_setting: value[0],
            mute: value[1],
            change_counter: value[2],
        })
    }
}

#[derive(Clone, Debug)]
pub struct VolumeControlService {
    step_size: u8,
    state: Arc<Mutex<VolumeState>>,
    volume_flags: VolumeFlags,
    included_services: Vec<usize>,
}

impl Default for VolumeControlService {
    fn default() -> Self {
        Self::new()
    }
}

impl VolumeControlService {
    pub fn new() -> Self {
        Self {
            step_size: 16,
            state: Arc::new(Mutex::new(VolumeState::default())),
            volume_flags: VolumeFlags::default(),
            included_services: Vec::new(),
        }
    }

    pub fn step_size(mut self, step_size: u8) -> Self {
        self.step_size = step_size;
        self
    }

    pub fn initial_state(mut self, state: VolumeState) -> Self {
        self.state = Arc::new(Mutex::new(state));
        self
    }

    pub fn volume_flags(mut self, flags: VolumeFlags) -> Self {
        self.volume_flags = flags;
        self
    }

    pub fn included_services(mut self, service_indices: impl Into<Vec<usize>>) -> Self {
        self.included_services = service_indices.into();
        self
    }

    pub fn state(&self) -> Result<VolumeState> {
        self.state
            .lock()
            .map(|state| *state)
            .map_err(|_| Error::InvalidValue("VCS state lock is poisoned".into()))
    }

    pub fn definition(&self) -> ServiceDefinition {
        ServiceDefinition {
            uuid: uuid(VOLUME_CONTROL_SERVICE),
            primary: true,
            included_services: self.included_services.clone(),
            characteristics: vec![
                CharacteristicDefinition {
                    uuid: uuid(VOLUME_STATE_CHARACTERISTIC),
                    properties: properties::READ | properties::NOTIFY,
                    permissions: permissions::READ_REQUIRES_ENCRYPTION,
                    value: self.state().unwrap_or_default().encode().to_vec(),
                    descriptors: Vec::new(),
                },
                CharacteristicDefinition {
                    uuid: uuid(VOLUME_CONTROL_POINT_CHARACTERISTIC),
                    properties: properties::WRITE,
                    permissions: permissions::WRITE_REQUIRES_ENCRYPTION,
                    value: Vec::new(),
                    descriptors: Vec::new(),
                },
                CharacteristicDefinition {
                    uuid: uuid(VOLUME_FLAGS_CHARACTERISTIC),
                    properties: properties::READ,
                    permissions: permissions::READ_REQUIRES_ENCRYPTION,
                    value: vec![self.volume_flags.0],
                    descriptors: Vec::new(),
                },
            ],
        }
    }

    pub fn bind(&self, server: &mut GattServer) -> Result<VolumeControlHandles> {
        let state_handle = required_handle(server, VOLUME_STATE_CHARACTERISTIC)?;
        let control_handle = required_handle(server, VOLUME_CONTROL_POINT_CHARACTERISTIC)?;
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
        let step_size = self.step_size;
        server.set_dynamic_value(
            control_handle,
            DynamicValue::write_only(move |_, value| update_volume(&write_state, step_size, value)),
        )?;
        Ok(VolumeControlHandles {
            volume_state: state_handle,
            volume_control_point: control_handle,
        })
    }
}

fn required_handle(server: &GattServer, characteristic_uuid: u16) -> Result<u16> {
    server
        .handles_by_uuid(&uuid(characteristic_uuid))
        .into_iter()
        .next()
        .ok_or(Error::MissingCharacteristic(characteristic_uuid))
}

fn update_volume(
    state: &Mutex<VolumeState>,
    step_size: u8,
    value: &[u8],
) -> core::result::Result<(), u8> {
    if value.len() < 2 {
        return Err(INVALID_ATTRIBUTE_VALUE_LENGTH);
    }
    let opcode = VolumeControlPointOpcode(value[0]);
    let mut state = state.lock().map_err(|_| UNLIKELY_ERROR)?;
    if value[1] != state.change_counter {
        return Err(error_code::INVALID_CHANGE_COUNTER);
    }
    let old = (state.volume_setting, state.mute);
    match opcode {
        VolumeControlPointOpcode::RELATIVE_VOLUME_DOWN => {
            state.volume_setting = state.volume_setting.saturating_sub(step_size);
        }
        VolumeControlPointOpcode::RELATIVE_VOLUME_UP => {
            state.volume_setting = state.volume_setting.saturating_add(step_size);
        }
        VolumeControlPointOpcode::UNMUTE_RELATIVE_VOLUME_DOWN => {
            state.volume_setting = state.volume_setting.saturating_sub(step_size);
            state.mute = 0;
        }
        VolumeControlPointOpcode::UNMUTE_RELATIVE_VOLUME_UP => {
            state.volume_setting = state.volume_setting.saturating_add(step_size);
            state.mute = 0;
        }
        VolumeControlPointOpcode::SET_ABSOLUTE_VOLUME => {
            let setting = value
                .get(2)
                .copied()
                .ok_or(INVALID_ATTRIBUTE_VALUE_LENGTH)?;
            state.volume_setting = setting;
        }
        VolumeControlPointOpcode::UNMUTE => state.mute = 0,
        VolumeControlPointOpcode::MUTE => state.mute = 1,
        _ => return Err(error_code::OPCODE_NOT_SUPPORTED),
    }
    if (state.volume_setting, state.mute) != old {
        state.change_counter = state.change_counter.wrapping_add(1);
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VolumeControlHandles {
    pub volume_state: u16,
    pub volume_control_point: u16,
}

#[derive(Clone, Debug)]
pub struct VolumeControlServiceProxy {
    pub service: ServiceProxy,
    pub volume_state: CharacteristicProxy,
    pub volume_control_point: CharacteristicProxy,
    pub volume_flags: CharacteristicProxy,
}

impl VolumeControlServiceProxy {
    pub fn from_parts(
        service: ServiceProxy,
        characteristics: &[CharacteristicProxy],
    ) -> Result<Self> {
        Ok(Self {
            service,
            volume_state: require_characteristic(characteristics, VOLUME_STATE_CHARACTERISTIC)?,
            volume_control_point: require_characteristic(
                characteristics,
                VOLUME_CONTROL_POINT_CHARACTERISTIC,
            )?,
            volume_flags: require_characteristic(characteristics, VOLUME_FLAGS_CHARACTERISTIC)?,
        })
    }

    pub fn discover(
        client: &mut GattClient,
        transport: &mut impl AttTransport,
    ) -> Result<Option<Self>> {
        let Some((service, characteristics)) =
            discover_profile(client, transport, VOLUME_CONTROL_SERVICE)?
        else {
            return Ok(None);
        };
        Self::from_parts(service, &characteristics).map(Some)
    }

    pub fn read_volume_state(
        &self,
        client: &mut GattClient,
        transport: &mut impl AttTransport,
    ) -> Result<VolumeState> {
        VolumeState::decode(&client.read_value(transport, self.volume_state.handle, false)?)
    }

    pub fn read_volume_flags(
        &self,
        client: &mut GattClient,
        transport: &mut impl AttTransport,
    ) -> Result<VolumeFlags> {
        let value = client.read_value(transport, self.volume_flags.handle, false)?;
        let flag = value
            .first()
            .copied()
            .ok_or_else(|| Error::InvalidValue("volume flags value is empty".into()))?;
        Ok(VolumeFlags(flag))
    }

    pub fn write_control_point(
        &self,
        client: &mut GattClient,
        transport: &mut impl AttTransport,
        opcode: VolumeControlPointOpcode,
        change_counter: u8,
        operand: Option<u8>,
    ) -> Result<()> {
        let mut value = vec![opcode.0, change_counter];
        if let Some(operand) = operand {
            value.push(operand);
        }
        client.write_value(transport, self.volume_control_point.handle, value, true)?;
        Ok(())
    }
}
