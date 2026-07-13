//! Audio Streaming for Hearing Aid (ASHA) service.

use crate::{discover_profile, Error, Result};
use bumble::{advertising_data::Type as AdvertisingType, AdvertisingData, Uuid};
use bumble_gatt::{
    permissions, properties, AttTransport, CharacteristicDefinition, CharacteristicProxy,
    DynamicValue, GattClient, GattServer, ServiceDefinition, ServiceProxy,
};
use std::sync::{Arc, Mutex};

pub const ASHA_SERVICE: u16 = 0xFDF0;
pub const READ_ONLY_PROPERTIES_CHARACTERISTIC: &str = "6333651e-c481-4a3e-9169-7c902aad37bb";
pub const AUDIO_CONTROL_POINT_CHARACTERISTIC: &str = "f0d4de7e-4a88-476c-9d9f-1937b0996cc0";
pub const AUDIO_STATUS_CHARACTERISTIC: &str = "38663f1a-e711-4cac-b641-326b56404837";
pub const VOLUME_CHARACTERISTIC: &str = "00e4ca9e-ab14-41e4-8823-f9e70c7e91df";
pub const LE_PSM_OUT_CHARACTERISTIC: &str = "2d410339-82b6-42aa-b34e-e2e01df8cc1a";

pub mod device_capabilities {
    pub const IS_RIGHT: u8 = 0x01;
    pub const IS_DUAL: u8 = 0x02;
    pub const CSIS_SUPPORTED: u8 = 0x04;
}

pub mod feature_map {
    pub const LE_COC_AUDIO_OUTPUT_STREAMING_SUPPORTED: u8 = 0x01;
}

pub mod opcode {
    pub const START: u8 = 1;
    pub const STOP: u8 = 2;
    pub const STATUS: u8 = 3;
}

pub mod codec {
    pub const G_722_16KHZ: u8 = 1;
}

pub mod audio_type {
    pub const UNKNOWN: u8 = 0;
    pub const RINGTONE: u8 = 1;
    pub const PHONE_CALL: u8 = 2;
    pub const MEDIA: u8 = 3;
}

pub const AUDIO_STATUS_OK: u8 = 0;
const INVALID_ATTRIBUTE_VALUE_LENGTH: u8 = 0x0D;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AshaState {
    pub active_codec: Option<u8>,
    pub audio_type: Option<u8>,
    pub volume: Option<u8>,
    pub other_state: Option<u8>,
    pub peripheral_status: Option<u8>,
    pub starts: u64,
    pub stops: u64,
    pub volume_changes: u64,
}

type AudioSink = dyn Fn(&[u8]) + Send + Sync + 'static;

#[derive(Clone)]
pub struct AshaService {
    capability: u8,
    hisyncid: Vec<u8>,
    feature_map: u8,
    protocol_version: u8,
    render_delay_milliseconds: u16,
    supported_codecs: u16,
    psm: u16,
    state: Arc<Mutex<AshaState>>,
    audio_sink: Option<Arc<AudioSink>>,
}

impl core::fmt::Debug for AshaService {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter
            .debug_struct("AshaService")
            .field("capability", &self.capability)
            .field("hisyncid", &self.hisyncid)
            .field("psm", &self.psm)
            .finish_non_exhaustive()
    }
}

impl AshaService {
    pub fn new(capability: u8, hisyncid: impl Into<Vec<u8>>, psm: u16) -> Self {
        Self {
            capability,
            hisyncid: hisyncid.into(),
            feature_map: feature_map::LE_COC_AUDIO_OUTPUT_STREAMING_SUPPORTED,
            protocol_version: 1,
            render_delay_milliseconds: 0,
            supported_codecs: 1 << codec::G_722_16KHZ,
            psm,
            state: Arc::new(Mutex::new(AshaState::default())),
            audio_sink: None,
        }
    }

    pub fn feature_map(mut self, feature_map: u8) -> Self {
        self.feature_map = feature_map;
        self
    }

    pub fn protocol_version(mut self, protocol_version: u8) -> Self {
        self.protocol_version = protocol_version;
        self
    }

    pub fn render_delay_milliseconds(mut self, delay: u16) -> Self {
        self.render_delay_milliseconds = delay;
        self
    }

    pub fn supported_codecs(mut self, codecs: u16) -> Self {
        self.supported_codecs = codecs;
        self
    }

    pub fn audio_sink(mut self, sink: impl Fn(&[u8]) + Send + Sync + 'static) -> Self {
        self.audio_sink = Some(Arc::new(sink));
        self
    }

    pub fn state(&self) -> Result<AshaState> {
        self.state
            .lock()
            .map(|state| state.clone())
            .map_err(|_| Error::InvalidValue("ASHA state lock is poisoned".into()))
    }

    pub fn read_only_properties(&self) -> Vec<u8> {
        let mut hisyncid = [0u8; 8];
        let count = self.hisyncid.len().min(hisyncid.len());
        hisyncid[..count].copy_from_slice(&self.hisyncid[..count]);
        let mut value = Vec::with_capacity(17);
        value.push(self.protocol_version);
        value.push(self.capability);
        value.extend_from_slice(&hisyncid);
        value.push(self.feature_map);
        value.extend_from_slice(&self.render_delay_milliseconds.to_le_bytes());
        value.extend_from_slice(&[0, 0]);
        value.extend_from_slice(&self.supported_codecs.to_le_bytes());
        value
    }

    pub fn definition(&self) -> ServiceDefinition {
        ServiceDefinition {
            uuid: Uuid::from_16_bits(ASHA_SERVICE),
            primary: true,
            included_services: Vec::new(),
            characteristics: vec![
                characteristic(
                    READ_ONLY_PROPERTIES_CHARACTERISTIC,
                    properties::READ,
                    permissions::READABLE,
                    self.read_only_properties(),
                ),
                characteristic(
                    AUDIO_CONTROL_POINT_CHARACTERISTIC,
                    properties::WRITE | properties::WRITE_WITHOUT_RESPONSE,
                    permissions::WRITEABLE,
                    Vec::new(),
                ),
                characteristic(
                    AUDIO_STATUS_CHARACTERISTIC,
                    properties::READ | properties::NOTIFY,
                    permissions::READABLE,
                    vec![AUDIO_STATUS_OK],
                ),
                characteristic(
                    VOLUME_CHARACTERISTIC,
                    properties::WRITE_WITHOUT_RESPONSE,
                    permissions::WRITEABLE,
                    Vec::new(),
                ),
                characteristic(
                    LE_PSM_OUT_CHARACTERISTIC,
                    properties::READ,
                    permissions::READABLE,
                    self.psm.to_le_bytes().to_vec(),
                ),
            ],
        }
    }

    pub fn bind(&self, server: &mut GattServer) -> Result<AshaHandles> {
        let control = required_handle(server, AUDIO_CONTROL_POINT_CHARACTERISTIC)?;
        let volume = required_handle(server, VOLUME_CHARACTERISTIC)?;
        let status = required_handle(server, AUDIO_STATUS_CHARACTERISTIC)?;
        let control_state = Arc::clone(&self.state);
        server.set_dynamic_value(
            control,
            DynamicValue::write_only(move |_, value| update_control(&control_state, value)),
        )?;
        let volume_state = Arc::clone(&self.state);
        server.set_dynamic_value(
            volume,
            DynamicValue::write_only(move |_, value| update_volume(&volume_state, value)),
        )?;
        Ok(AshaHandles {
            audio_control_point: control,
            audio_status: status,
            volume,
        })
    }

    pub fn receive_audio(&self, data: &[u8]) {
        if let Some(sink) = &self.audio_sink {
            sink(data);
        }
    }

    pub fn advertising_data(&self) -> Vec<u8> {
        let mut value = Uuid::from_16_bits(ASHA_SERVICE).to_bytes(false);
        value.push(self.protocol_version);
        value.push(self.capability);
        value.extend_from_slice(&self.hisyncid[..self.hisyncid.len().min(4)]);
        AdvertisingData {
            ad_structures: vec![(AdvertisingType(0x16), value)],
        }
        .to_bytes()
    }
}

fn characteristic(
    characteristic_uuid: &str,
    characteristic_properties: u8,
    characteristic_permissions: u8,
    value: Vec<u8>,
) -> CharacteristicDefinition {
    CharacteristicDefinition {
        uuid: Uuid::parse(characteristic_uuid).expect("valid ASHA UUID"),
        properties: characteristic_properties,
        permissions: characteristic_permissions,
        value,
        descriptors: Vec::new(),
    }
}

fn required_handle(server: &GattServer, characteristic_uuid: &str) -> Result<u16> {
    server
        .handles_by_uuid(&Uuid::parse(characteristic_uuid).expect("valid ASHA UUID"))
        .into_iter()
        .next()
        .ok_or_else(|| {
            Error::InvalidValue(format!("missing ASHA characteristic {characteristic_uuid}"))
        })
}

fn update_control(state: &Mutex<AshaState>, value: &[u8]) -> core::result::Result<(), u8> {
    let Some(&command) = value.first() else {
        return Err(INVALID_ATTRIBUTE_VALUE_LENGTH);
    };
    let mut state = state.lock().map_err(|_| 0x0E)?;
    match command {
        opcode::START => {
            if value.len() < 5 {
                return Err(INVALID_ATTRIBUTE_VALUE_LENGTH);
            }
            state.active_codec = Some(value[1]);
            state.audio_type = Some(value[2]);
            state.volume = Some(value[3]);
            state.other_state = Some(value[4]);
            state.starts += 1;
        }
        opcode::STOP => {
            state.active_codec = None;
            state.audio_type = None;
            state.volume = None;
            state.other_state = None;
            state.stops += 1;
        }
        opcode::STATUS => {
            if value.len() < 2 {
                return Err(INVALID_ATTRIBUTE_VALUE_LENGTH);
            }
            state.peripheral_status = Some(value[1]);
        }
        _ => {}
    }
    Ok(())
}

fn update_volume(state: &Mutex<AshaState>, value: &[u8]) -> core::result::Result<(), u8> {
    let Some(&volume) = value.first() else {
        return Err(INVALID_ATTRIBUTE_VALUE_LENGTH);
    };
    let mut state = state.lock().map_err(|_| 0x0E)?;
    state.volume = Some(volume);
    state.volume_changes += 1;
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AshaHandles {
    pub audio_control_point: u16,
    pub audio_status: u16,
    pub volume: u16,
}

#[derive(Clone, Debug)]
pub struct AshaServiceProxy {
    pub service: ServiceProxy,
    pub read_only_properties_characteristic: CharacteristicProxy,
    pub audio_control_point_characteristic: CharacteristicProxy,
    pub audio_status_characteristic: CharacteristicProxy,
    pub volume_characteristic: CharacteristicProxy,
    pub psm_characteristic: CharacteristicProxy,
}

impl AshaServiceProxy {
    pub fn from_parts(
        service: ServiceProxy,
        characteristics: &[CharacteristicProxy],
    ) -> Result<Self> {
        let required = |uuid_text: &str| {
            let expected = Uuid::parse(uuid_text).expect("valid ASHA UUID");
            characteristics
                .iter()
                .find(|characteristic| characteristic.uuid == expected)
                .cloned()
                .ok_or_else(|| {
                    Error::InvalidValue(format!("missing ASHA characteristic {uuid_text}"))
                })
        };
        Ok(Self {
            service,
            read_only_properties_characteristic: required(READ_ONLY_PROPERTIES_CHARACTERISTIC)?,
            audio_control_point_characteristic: required(AUDIO_CONTROL_POINT_CHARACTERISTIC)?,
            audio_status_characteristic: required(AUDIO_STATUS_CHARACTERISTIC)?,
            volume_characteristic: required(VOLUME_CHARACTERISTIC)?,
            psm_characteristic: required(LE_PSM_OUT_CHARACTERISTIC)?,
        })
    }

    pub fn discover(
        client: &mut GattClient,
        transport: &mut impl AttTransport,
    ) -> Result<Option<Self>> {
        let Some((service, characteristics)) = discover_profile(client, transport, ASHA_SERVICE)?
        else {
            return Ok(None);
        };
        Self::from_parts(service, &characteristics).map(Some)
    }
}
