//! Heart Rate Service.

use crate::{discover_profile, find_characteristic, require_characteristic, uuid, Error, Result};
use bumble_gatt::{
    permissions, properties, AccessContext, AdapterError, AttTransport, ByteOrder,
    ByteSerializable, CharacteristicDefinition, CharacteristicProxy, CharacteristicProxyAdapter,
    DelegatedCodec, DynamicValue, EnumCodec, GattClient, GattServer, IntConvertible,
    SerializableCodec, ServiceDefinition, ServiceProxy,
};
use std::sync::Arc;

pub const HEART_RATE_SERVICE: u16 = 0x180D;
pub const HEART_RATE_MEASUREMENT_CHARACTERISTIC: u16 = 0x2A37;
pub const BODY_SENSOR_LOCATION_CHARACTERISTIC: u16 = 0x2A38;
pub const HEART_RATE_CONTROL_POINT_CHARACTERISTIC: u16 = 0x2A39;
pub const CONTROL_POINT_NOT_SUPPORTED: u8 = 0x80;
pub const RESET_ENERGY_EXPENDED: u8 = 0x01;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BodySensorLocation(pub u8);

impl BodySensorLocation {
    pub const OTHER: Self = Self(0);
    pub const CHEST: Self = Self(1);
    pub const WRIST: Self = Self(2);
    pub const FINGER: Self = Self(3);
    pub const HAND: Self = Self(4);
    pub const EAR_LOBE: Self = Self(5);
    pub const FOOT: Self = Self(6);
}

impl IntConvertible for BodySensorLocation {
    fn to_u64(&self) -> u64 {
        u64::from(self.0)
    }

    fn from_u64(value: u64) -> core::result::Result<Self, String> {
        u8::try_from(value)
            .map(Self)
            .map_err(|_| format!("body sensor location {value} does not fit in one byte"))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct HeartRateMeasurement {
    pub heart_rate: u16,
    pub sensor_contact_detected: Option<bool>,
    pub energy_expended: Option<u16>,
    pub rr_intervals: Option<Vec<f32>>,
}

impl HeartRateMeasurement {
    const INT16_HEART_RATE: u8 = 1 << 0;
    const SENSOR_CONTACT_DETECTED: u8 = 1 << 1;
    const SENSOR_CONTACT_SUPPORTED: u8 = 1 << 2;
    const ENERGY_EXPENDED_STATUS: u8 = 1 << 3;
    const RR_INTERVAL: u8 = 1 << 4;

    pub fn new(heart_rate: u16) -> Self {
        Self {
            heart_rate,
            sensor_contact_detected: None,
            energy_expended: None,
            rr_intervals: None,
        }
    }

    pub fn try_new(
        heart_rate: u32,
        sensor_contact_detected: Option<bool>,
        energy_expended: Option<u32>,
        rr_intervals: Option<Vec<f32>>,
    ) -> Result<Self> {
        let heart_rate = u16::try_from(heart_rate)
            .map_err(|_| Error::InvalidValue("heart_rate out of range".into()))?;
        let energy_expended = energy_expended
            .map(|value| {
                u16::try_from(value)
                    .map_err(|_| Error::InvalidValue("energy_expended out of range".into()))
            })
            .transpose()?;
        if let Some(intervals) = &rr_intervals {
            if intervals.iter().any(|interval| {
                !interval.is_finite() || *interval < 0.0 || *interval * 1024.0 > 65535.0
            }) {
                return Err(Error::InvalidValue("rr_intervals out of range".into()));
            }
        }
        Ok(Self {
            heart_rate,
            sensor_contact_detected,
            energy_expended,
            rr_intervals,
        })
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut flags = 0u8;
        let mut data = Vec::new();
        if self.heart_rate < 256 {
            data.push(self.heart_rate as u8);
        } else {
            flags |= Self::INT16_HEART_RATE;
            data.extend_from_slice(&self.heart_rate.to_le_bytes());
        }
        if let Some(detected) = self.sensor_contact_detected {
            flags |= Self::SENSOR_CONTACT_SUPPORTED;
            if detected {
                flags |= Self::SENSOR_CONTACT_DETECTED;
            }
        }
        if let Some(energy) = self.energy_expended {
            flags |= Self::ENERGY_EXPENDED_STATUS;
            data.extend_from_slice(&energy.to_le_bytes());
        }
        if let Some(intervals) = &self.rr_intervals {
            flags |= Self::RR_INTERVAL;
            for interval in intervals {
                data.extend_from_slice(&((*interval * 1024.0) as u16).to_le_bytes());
            }
        }
        let mut encoded = Vec::with_capacity(1 + data.len());
        encoded.push(flags);
        encoded.extend_from_slice(&data);
        encoded
    }

    pub fn decode(data: &[u8]) -> Result<Self> {
        let (&flags, mut data) = data
            .split_first()
            .ok_or_else(|| Error::InvalidValue("heart-rate measurement is empty".into()))?;
        let heart_rate = if flags & Self::INT16_HEART_RATE != 0 {
            let bytes: [u8; 2] = take(&mut data, 2)?.try_into().expect("two-byte heart rate");
            u16::from_le_bytes(bytes)
        } else {
            u16::from(take(&mut data, 1)?[0])
        };
        let sensor_contact_detected = (flags & Self::SENSOR_CONTACT_SUPPORTED != 0)
            .then_some(flags & Self::SENSOR_CONTACT_DETECTED != 0);
        let energy_expended = if flags & Self::ENERGY_EXPENDED_STATUS != 0 {
            let bytes: [u8; 2] = take(&mut data, 2)?.try_into().expect("two-byte energy");
            Some(u16::from_le_bytes(bytes))
        } else {
            None
        };
        let rr_intervals = if flags & Self::RR_INTERVAL != 0 {
            if !data.len().is_multiple_of(2) {
                return Err(Error::InvalidValue(
                    "heart-rate RR interval payload has odd length".into(),
                ));
            }
            Some(
                data.chunks_exact(2)
                    .map(|bytes| u16::from_le_bytes([bytes[0], bytes[1]]) as f32 / 1024.0)
                    .collect(),
            )
        } else {
            None
        };
        Ok(Self {
            heart_rate,
            sensor_contact_detected,
            energy_expended,
            rr_intervals,
        })
    }
}

fn take<'a>(data: &mut &'a [u8], length: usize) -> Result<&'a [u8]> {
    if data.len() < length {
        return Err(Error::InvalidValue(
            "heart-rate measurement is truncated".into(),
        ));
    }
    let (value, remaining) = data.split_at(length);
    *data = remaining;
    Ok(value)
}

impl ByteSerializable for HeartRateMeasurement {
    fn to_bytes(&self) -> Vec<u8> {
        self.encode()
    }

    fn from_bytes(bytes: &[u8]) -> core::result::Result<Self, String> {
        Self::decode(bytes).map_err(|error| error.to_string())
    }
}

type MeasurementReader = dyn Fn(AccessContext) -> HeartRateMeasurement + Send + Sync + 'static;
type EnergyReset = dyn Fn(AccessContext) + Send + Sync + 'static;

#[derive(Clone)]
pub struct HeartRateService {
    read_measurement: Arc<MeasurementReader>,
    body_sensor_location: Option<BodySensorLocation>,
    reset_energy_expended: Option<Arc<EnergyReset>>,
}

impl core::fmt::Debug for HeartRateService {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter
            .debug_struct("HeartRateService")
            .field("body_sensor_location", &self.body_sensor_location)
            .field("has_energy_reset", &self.reset_energy_expended.is_some())
            .finish_non_exhaustive()
    }
}

impl HeartRateService {
    pub fn new(
        read_measurement: impl Fn(AccessContext) -> HeartRateMeasurement + Send + Sync + 'static,
    ) -> Self {
        Self {
            read_measurement: Arc::new(read_measurement),
            body_sensor_location: None,
            reset_energy_expended: None,
        }
    }

    pub fn body_sensor_location(mut self, location: BodySensorLocation) -> Self {
        self.body_sensor_location = Some(location);
        self
    }

    pub fn reset_energy_expended(
        mut self,
        reset: impl Fn(AccessContext) + Send + Sync + 'static,
    ) -> Self {
        self.reset_energy_expended = Some(Arc::new(reset));
        self
    }

    pub fn definition(&self) -> ServiceDefinition {
        let mut characteristics = vec![CharacteristicDefinition {
            uuid: uuid(HEART_RATE_MEASUREMENT_CHARACTERISTIC),
            properties: properties::NOTIFY,
            // Bumble permits direct reads of its dynamic callback in tests.
            permissions: permissions::READABLE,
            value: HeartRateMeasurement::new(0).encode(),
            descriptors: Vec::new(),
        }];
        if let Some(location) = self.body_sensor_location {
            characteristics.push(CharacteristicDefinition {
                uuid: uuid(BODY_SENSOR_LOCATION_CHARACTERISTIC),
                properties: properties::READ,
                permissions: permissions::READABLE,
                value: vec![location.0],
                descriptors: Vec::new(),
            });
        }
        if self.reset_energy_expended.is_some() {
            characteristics.push(CharacteristicDefinition {
                uuid: uuid(HEART_RATE_CONTROL_POINT_CHARACTERISTIC),
                properties: properties::WRITE,
                permissions: permissions::WRITEABLE,
                value: Vec::new(),
                descriptors: Vec::new(),
            });
        }
        ServiceDefinition {
            uuid: uuid(HEART_RATE_SERVICE),
            primary: true,
            included_services: Vec::new(),
            characteristics,
        }
    }

    pub fn bind(&self, server: &mut GattServer) -> Result<HeartRateHandles> {
        let measurement = server
            .handles_by_uuid(&uuid(HEART_RATE_MEASUREMENT_CHARACTERISTIC))
            .into_iter()
            .next()
            .ok_or(Error::MissingCharacteristic(
                HEART_RATE_MEASUREMENT_CHARACTERISTIC,
            ))?;
        let reader = Arc::clone(&self.read_measurement);
        server.set_dynamic_value(
            measurement,
            DynamicValue::read_only(move |context| Ok(reader(context).encode())),
        )?;

        let control_point = if let Some(reset) = &self.reset_energy_expended {
            let handle = server
                .handles_by_uuid(&uuid(HEART_RATE_CONTROL_POINT_CHARACTERISTIC))
                .into_iter()
                .next()
                .ok_or(Error::MissingCharacteristic(
                    HEART_RATE_CONTROL_POINT_CHARACTERISTIC,
                ))?;
            let reset = Arc::clone(reset);
            server.set_dynamic_value(
                handle,
                DynamicValue::write_only(move |context, value| {
                    if value == [RESET_ENERGY_EXPENDED] {
                        reset(context);
                        Ok(())
                    } else {
                        Err(CONTROL_POINT_NOT_SUPPORTED)
                    }
                }),
            )?;
            Some(handle)
        } else {
            None
        };
        Ok(HeartRateHandles {
            measurement,
            control_point,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HeartRateHandles {
    pub measurement: u16,
    pub control_point: Option<u16>,
}

pub type MeasurementProxy = CharacteristicProxyAdapter<SerializableCodec<HeartRateMeasurement>>;
pub type BodyLocationProxy = CharacteristicProxyAdapter<EnumCodec<BodySensorLocation>>;
pub type ControlPointProxy = CharacteristicProxyAdapter<DelegatedCodec<u8>>;

#[derive(Clone, Debug)]
pub struct HeartRateServiceProxy {
    pub service: ServiceProxy,
    pub heart_rate_measurement: MeasurementProxy,
    pub body_sensor_location: Option<BodyLocationProxy>,
    pub heart_rate_control_point: Option<ControlPointProxy>,
}

impl HeartRateServiceProxy {
    pub fn from_parts(
        service: ServiceProxy,
        characteristics: &[CharacteristicProxy],
    ) -> Result<Self> {
        let measurement =
            require_characteristic(characteristics, HEART_RATE_MEASUREMENT_CHARACTERISTIC)?;
        let body_sensor_location =
            find_characteristic(characteristics, BODY_SENSOR_LOCATION_CHARACTERISTIC)
                .map(|proxy| {
                    EnumCodec::new(1, ByteOrder::Little)
                        .map(|codec| BodyLocationProxy::new(proxy, codec))
                })
                .transpose()?;
        let heart_rate_control_point =
            find_characteristic(characteristics, HEART_RATE_CONTROL_POINT_CHARACTERISTIC).map(
                |proxy| {
                    ControlPointProxy::new(
                        proxy,
                        DelegatedCodec::new(
                            |value: &u8| Ok(vec![*value]),
                            |bytes| match bytes {
                                [value] => Ok(*value),
                                _ => Err(AdapterError::InvalidValue(format!(
                                    "heart-rate control point needs 1 byte, got {}",
                                    bytes.len()
                                ))),
                            },
                        ),
                    )
                },
            );
        Ok(Self {
            service,
            heart_rate_measurement: MeasurementProxy::new(
                measurement,
                SerializableCodec::default(),
            ),
            body_sensor_location,
            heart_rate_control_point,
        })
    }

    pub fn discover(
        client: &mut GattClient,
        transport: &mut impl AttTransport,
    ) -> Result<Option<Self>> {
        let Some((service, characteristics)) =
            discover_profile(client, transport, HEART_RATE_SERVICE)?
        else {
            return Ok(None);
        };
        Self::from_parts(service, &characteristics).map(Some)
    }

    pub fn reset_energy_expended(
        &self,
        client: &mut GattClient,
        transport: &mut impl AttTransport,
    ) -> Result<()> {
        let control_point =
            self.heart_rate_control_point
                .as_ref()
                .ok_or(Error::MissingCharacteristic(
                    HEART_RATE_CONTROL_POINT_CHARACTERISTIC,
                ))?;
        control_point.write_value(client, transport, &RESET_ENERGY_EXPENDED, true)?;
        Ok(())
    }
}
