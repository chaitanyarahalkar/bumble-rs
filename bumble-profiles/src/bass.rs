//! Broadcast Audio Scan Service (BASS) wire models and GATT runtime.

use crate::{discover_profile, require_characteristic, uuid, Error, Result};
use bumble::{Address, AddressType};
use bumble_gatt::{
    permissions, properties, AttTransport, CharacteristicDefinition, CharacteristicProxy,
    DynamicValue, GattClient, GattServer, ServiceDefinition, ServiceProxy,
};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

pub const BROADCAST_AUDIO_SCAN_SERVICE: u16 = 0x184F;
pub const BROADCAST_AUDIO_SCAN_CONTROL_POINT_CHARACTERISTIC: u16 = 0x2BC7;
pub const BROADCAST_RECEIVE_STATE_CHARACTERISTIC: u16 = 0x2BC8;

const INVALID_ATTRIBUTE_VALUE_LENGTH: u8 = 0x0D;
const UNLIKELY_ERROR: u8 = 0x0E;

pub mod application_error {
    pub const OPCODE_NOT_SUPPORTED: u8 = 0x80;
    pub const INVALID_SOURCE_ID: u8 = 0x81;
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PeriodicAdvertisingSyncParams(pub u8);

impl PeriodicAdvertisingSyncParams {
    pub const DO_NOT_SYNCHRONIZE_TO_PA: Self = Self(0x00);
    pub const SYNCHRONIZE_TO_PA_PAST_AVAILABLE: Self = Self(0x01);
    pub const SYNCHRONIZE_TO_PA_PAST_NOT_AVAILABLE: Self = Self(0x02);
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SubgroupInfo {
    pub bis_sync: u32,
    pub metadata: Vec<u8>,
}

impl SubgroupInfo {
    pub const ANY_BIS: u32 = 0xFFFF_FFFF;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ControlPointOperation {
    RemoteScanStopped,
    RemoteScanStarted,
    AddSource {
        advertiser_address: Address,
        advertising_sid: u8,
        broadcast_id: u32,
        pa_sync: PeriodicAdvertisingSyncParams,
        pa_interval: u16,
        subgroups: Vec<SubgroupInfo>,
    },
    ModifySource {
        source_id: u8,
        pa_sync: PeriodicAdvertisingSyncParams,
        pa_interval: u16,
        subgroups: Vec<SubgroupInfo>,
    },
    SetBroadcastCode {
        source_id: u8,
        broadcast_code: [u8; 16],
    },
    RemoveSource {
        source_id: u8,
    },
}

impl ControlPointOperation {
    pub fn opcode(&self) -> u8 {
        match self {
            Self::RemoteScanStopped => 0x00,
            Self::RemoteScanStarted => 0x01,
            Self::AddSource { .. } => 0x02,
            Self::ModifySource { .. } => 0x03,
            Self::SetBroadcastCode { .. } => 0x04,
            Self::RemoveSource { .. } => 0x05,
        }
    }

    pub fn from_bytes(data: &[u8]) -> Result<Self> {
        let mut reader = Reader::new(data);
        let opcode = reader.u8("BASS opcode")?;
        let operation = match opcode {
            0x00 => Self::RemoteScanStopped,
            0x01 => Self::RemoteScanStarted,
            0x02 => Self::AddSource {
                advertiser_address: reader.address("advertiser address")?,
                advertising_sid: reader.u8("advertising SID")?,
                broadcast_id: reader.u24("broadcast ID")?,
                pa_sync: PeriodicAdvertisingSyncParams(reader.u8("PA sync")?),
                pa_interval: reader.u16("PA interval")?,
                subgroups: reader.subgroups()?,
            },
            0x03 => Self::ModifySource {
                source_id: reader.u8("source ID")?,
                pa_sync: PeriodicAdvertisingSyncParams(reader.u8("PA sync")?),
                pa_interval: reader.u16("PA interval")?,
                subgroups: reader.subgroups()?,
            },
            0x04 => Self::SetBroadcastCode {
                source_id: reader.u8("source ID")?,
                broadcast_code: reader
                    .take(16, "broadcast code")?
                    .try_into()
                    .expect("sixteen-byte reader slice"),
            },
            0x05 => Self::RemoveSource {
                source_id: reader.u8("source ID")?,
            },
            _ => {
                return Err(Error::InvalidValue(format!(
                    "unknown BASS opcode 0x{opcode:02X}"
                )))
            }
        };
        reader.finish("BASS control-point operation")?;
        Ok(operation)
    }

    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        let mut value = vec![self.opcode()];
        match self {
            Self::RemoteScanStopped | Self::RemoteScanStarted => {}
            Self::AddSource {
                advertiser_address,
                advertising_sid,
                broadcast_id,
                pa_sync,
                pa_interval,
                subgroups,
            } => {
                require_u24("broadcast ID", *broadcast_id)?;
                value.push(advertiser_address.address_type().0);
                value.extend_from_slice(advertiser_address.address_bytes());
                value.push(*advertising_sid);
                value.extend_from_slice(&u24_bytes(*broadcast_id));
                value.push(pa_sync.0);
                value.extend_from_slice(&pa_interval.to_le_bytes());
                encode_subgroups(&mut value, subgroups)?;
            }
            Self::ModifySource {
                source_id,
                pa_sync,
                pa_interval,
                subgroups,
            } => {
                value.extend_from_slice(&[*source_id, pa_sync.0]);
                value.extend_from_slice(&pa_interval.to_le_bytes());
                encode_subgroups(&mut value, subgroups)?;
            }
            Self::SetBroadcastCode {
                source_id,
                broadcast_code,
            } => {
                value.push(*source_id);
                value.extend_from_slice(broadcast_code);
            }
            Self::RemoveSource { source_id } => value.push(*source_id),
        }
        Ok(value)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PeriodicAdvertisingSyncState(pub u8);

impl PeriodicAdvertisingSyncState {
    pub const NOT_SYNCHRONIZED_TO_PA: Self = Self(0x00);
    pub const SYNCINFO_REQUEST: Self = Self(0x01);
    pub const SYNCHRONIZED_TO_PA: Self = Self(0x02);
    pub const FAILED_TO_SYNCHRONIZE_TO_PA: Self = Self(0x03);
    pub const NO_PAST: Self = Self(0x04);
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BigEncryption(pub u8);

impl BigEncryption {
    pub const NOT_ENCRYPTED: Self = Self(0x00);
    pub const BROADCAST_CODE_REQUIRED: Self = Self(0x01);
    pub const DECRYPTING: Self = Self(0x02);
    pub const BAD_CODE: Self = Self(0x03);
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BroadcastReceiveState {
    pub source_id: u8,
    pub source_address: Address,
    pub source_adv_sid: u8,
    pub broadcast_id: u32,
    pub pa_sync_state: PeriodicAdvertisingSyncState,
    pub big_encryption: BigEncryption,
    pub bad_code: Option<[u8; 16]>,
    pub subgroups: Vec<SubgroupInfo>,
}

impl BroadcastReceiveState {
    pub fn from_bytes(data: &[u8]) -> Result<Self> {
        let mut reader = Reader::new(data);
        let source_id = reader.u8("source ID")?;
        let source_address = reader.address("source address")?;
        let source_adv_sid = reader.u8("source advertising SID")?;
        let broadcast_id = reader.u24("broadcast ID")?;
        let pa_sync_state = PeriodicAdvertisingSyncState(reader.u8("PA sync state")?);
        let big_encryption = BigEncryption(reader.u8("BIG encryption")?);
        let bad_code = if big_encryption == BigEncryption::BAD_CODE {
            Some(
                reader
                    .take(16, "bad broadcast code")?
                    .try_into()
                    .expect("sixteen-byte reader slice"),
            )
        } else {
            None
        };
        let subgroups = reader.subgroups()?;
        reader.finish("broadcast receive state")?;
        Ok(Self {
            source_id,
            source_address,
            source_adv_sid,
            broadcast_id,
            pa_sync_state,
            big_encryption,
            bad_code,
            subgroups,
        })
    }

    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        require_u24("broadcast ID", self.broadcast_id)?;
        if self.big_encryption == BigEncryption::BAD_CODE && self.bad_code.is_none() {
            return Err(Error::InvalidValue(
                "BAD_CODE receive state omits its 16-byte code".into(),
            ));
        }
        if self.big_encryption != BigEncryption::BAD_CODE && self.bad_code.is_some() {
            return Err(Error::InvalidValue(
                "broadcast code is only valid for BAD_CODE receive state".into(),
            ));
        }
        let mut value = vec![self.source_id, self.source_address.address_type().0];
        value.extend_from_slice(self.source_address.address_bytes());
        value.push(self.source_adv_sid);
        value.extend_from_slice(&u24_bytes(self.broadcast_id));
        value.extend_from_slice(&[self.pa_sync_state.0, self.big_encryption.0]);
        if let Some(bad_code) = self.bad_code {
            value.extend_from_slice(&bad_code);
        }
        encode_subgroups(&mut value, &self.subgroups)?;
        Ok(value)
    }
}

type PendingStateNotifications = VecDeque<(usize, Vec<u8>)>;

#[derive(Clone, Debug)]
pub struct BroadcastAudioScanService {
    states: Arc<Mutex<Vec<Option<BroadcastReceiveState>>>>,
    operations: Arc<Mutex<VecDeque<ControlPointOperation>>>,
    pending_notifications: Arc<Mutex<PendingStateNotifications>>,
}

impl BroadcastAudioScanService {
    pub fn new(receive_state_slots: usize) -> Result<Self> {
        if receive_state_slots > 255 {
            return Err(Error::InvalidValue(
                "BASS cannot publish over 255 receive-state slots".into(),
            ));
        }
        Ok(Self {
            states: Arc::new(Mutex::new(vec![None; receive_state_slots])),
            operations: Arc::new(Mutex::new(VecDeque::new())),
            pending_notifications: Arc::new(Mutex::new(VecDeque::new())),
        })
    }

    pub fn definition(&self) -> Result<ServiceDefinition> {
        let states = self
            .states
            .lock()
            .map_err(|_| Error::InvalidValue("BASS state lock is poisoned".into()))?;
        let mut characteristics = vec![CharacteristicDefinition {
            uuid: uuid(BROADCAST_AUDIO_SCAN_CONTROL_POINT_CHARACTERISTIC),
            properties: properties::WRITE | properties::WRITE_WITHOUT_RESPONSE,
            permissions: permissions::WRITEABLE,
            value: Vec::new(),
            descriptors: Vec::new(),
        }];
        for state in states.iter() {
            characteristics.push(CharacteristicDefinition {
                uuid: uuid(BROADCAST_RECEIVE_STATE_CHARACTERISTIC),
                properties: properties::READ | properties::NOTIFY,
                permissions: permissions::READ_REQUIRES_ENCRYPTION,
                value: state
                    .as_ref()
                    .map(BroadcastReceiveState::to_bytes)
                    .transpose()?
                    .unwrap_or_default(),
                descriptors: Vec::new(),
            });
        }
        Ok(ServiceDefinition {
            uuid: uuid(BROADCAST_AUDIO_SCAN_SERVICE),
            primary: true,
            included_services: Vec::new(),
            characteristics,
        })
    }

    pub fn bind(&self, server: &mut GattServer) -> Result<BroadcastAudioScanHandles> {
        let control_point = server
            .handles_by_uuid(&uuid(BROADCAST_AUDIO_SCAN_CONTROL_POINT_CHARACTERISTIC))
            .into_iter()
            .next()
            .ok_or(Error::MissingCharacteristic(
                BROADCAST_AUDIO_SCAN_CONTROL_POINT_CHARACTERISTIC,
            ))?;
        let receive_states = server.handles_by_uuid(&uuid(BROADCAST_RECEIVE_STATE_CHARACTERISTIC));
        let state_count = self
            .states
            .lock()
            .map_err(|_| Error::InvalidValue("BASS state lock is poisoned".into()))?
            .len();
        if receive_states.len() != state_count {
            return Err(Error::InvalidValue(format!(
                "BASS has {} receive-state handles for {state_count} slots",
                receive_states.len()
            )));
        }
        for (index, handle) in receive_states.iter().copied().enumerate() {
            let states = Arc::clone(&self.states);
            server.set_dynamic_value(
                handle,
                DynamicValue::read_only(move |_| {
                    states
                        .lock()
                        .map_err(|_| UNLIKELY_ERROR)?
                        .get(index)
                        .ok_or(UNLIKELY_ERROR)?
                        .as_ref()
                        .map(BroadcastReceiveState::to_bytes)
                        .transpose()
                        .map_err(|_| UNLIKELY_ERROR)
                        .map(Option::unwrap_or_default)
                }),
            )?;
        }
        let operations = Arc::clone(&self.operations);
        server.set_dynamic_value(
            control_point,
            DynamicValue::write_only(move |_, value| {
                let operation = ControlPointOperation::from_bytes(value)
                    .map_err(|_| INVALID_ATTRIBUTE_VALUE_LENGTH)?;
                operations
                    .lock()
                    .map_err(|_| UNLIKELY_ERROR)?
                    .push_back(operation);
                Ok(())
            }),
        )?;
        Ok(BroadcastAudioScanHandles {
            control_point,
            receive_states,
        })
    }

    pub fn take_operation(&self) -> Result<Option<ControlPointOperation>> {
        self.operations
            .lock()
            .map(|mut operations| operations.pop_front())
            .map_err(|_| Error::InvalidValue("BASS operation lock is poisoned".into()))
    }

    pub fn set_receive_state(
        &self,
        index: usize,
        state: Option<BroadcastReceiveState>,
    ) -> Result<()> {
        let value = state
            .as_ref()
            .map(BroadcastReceiveState::to_bytes)
            .transpose()?
            .unwrap_or_default();
        let mut states = self
            .states
            .lock()
            .map_err(|_| Error::InvalidValue("BASS state lock is poisoned".into()))?;
        let slot = states.get_mut(index).ok_or_else(|| {
            Error::InvalidValue(format!("BASS receive-state slot {index} does not exist"))
        })?;
        *slot = state;
        drop(states);
        self.pending_notifications
            .lock()
            .map_err(|_| Error::InvalidValue("BASS notification lock is poisoned".into()))?
            .push_back((index, value));
        Ok(())
    }

    pub fn take_pending_notifications(
        &self,
        handles: &BroadcastAudioScanHandles,
    ) -> Result<Vec<(u16, Vec<u8>)>> {
        self.pending_notifications
            .lock()
            .map_err(|_| Error::InvalidValue("BASS notification lock is poisoned".into()))?
            .drain(..)
            .map(|(index, value)| {
                handles
                    .receive_states
                    .get(index)
                    .copied()
                    .map(|handle| (handle, value))
                    .ok_or_else(|| {
                        Error::InvalidValue(format!(
                            "BASS receive-state slot {index} has no bound handle"
                        ))
                    })
            })
            .collect()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BroadcastAudioScanHandles {
    pub control_point: u16,
    pub receive_states: Vec<u16>,
}

#[derive(Clone, Debug)]
pub struct BroadcastAudioScanServiceProxy {
    pub service: ServiceProxy,
    pub broadcast_audio_scan_control_point: CharacteristicProxy,
    pub broadcast_receive_states: Vec<CharacteristicProxy>,
}

impl BroadcastAudioScanServiceProxy {
    pub fn from_parts(
        service: ServiceProxy,
        characteristics: &[CharacteristicProxy],
    ) -> Result<Self> {
        let receive_state_uuid = uuid(BROADCAST_RECEIVE_STATE_CHARACTERISTIC);
        Ok(Self {
            service,
            broadcast_audio_scan_control_point: require_characteristic(
                characteristics,
                BROADCAST_AUDIO_SCAN_CONTROL_POINT_CHARACTERISTIC,
            )?,
            broadcast_receive_states: characteristics
                .iter()
                .filter(|characteristic| characteristic.uuid == receive_state_uuid)
                .cloned()
                .collect(),
        })
    }

    pub fn discover(
        client: &mut GattClient,
        transport: &mut impl AttTransport,
    ) -> Result<Option<Self>> {
        let Some((service, characteristics)) =
            discover_profile(client, transport, BROADCAST_AUDIO_SCAN_SERVICE)?
        else {
            return Ok(None);
        };
        Self::from_parts(service, &characteristics).map(Some)
    }

    pub fn send_control_point_operation(
        &self,
        client: &mut GattClient,
        transport: &mut impl AttTransport,
        operation: &ControlPointOperation,
    ) -> Result<()> {
        client.write_value(
            transport,
            self.broadcast_audio_scan_control_point.handle,
            operation.to_bytes()?,
            true,
        )?;
        Ok(())
    }

    pub fn read_receive_state(
        characteristic: &CharacteristicProxy,
        client: &mut GattClient,
        transport: &mut impl AttTransport,
    ) -> Result<Option<BroadcastReceiveState>> {
        let value = client.read_value(transport, characteristic.handle, false)?;
        if value.is_empty() {
            Ok(None)
        } else {
            BroadcastReceiveState::from_bytes(&value).map(Some)
        }
    }

    pub fn subscribe_receive_states(
        &self,
        client: &mut GattClient,
        transport: &mut impl AttTransport,
    ) -> Result<()> {
        for characteristic in &self.broadcast_receive_states {
            let cccd = client
                .discover_descriptors(transport, characteristic)?
                .into_iter()
                .find(|descriptor| descriptor.uuid == uuid(0x2902))
                .ok_or_else(|| {
                    Error::InvalidValue(format!(
                        "BASS receive-state characteristic {:?} has no CCCD",
                        characteristic.uuid
                    ))
                })?;
            client.subscribe(transport, characteristic.handle, cccd.handle, false)?;
        }
        Ok(())
    }

    pub fn state_from_notification(
        &self,
        handle: u16,
        value: &[u8],
    ) -> Result<Option<BroadcastReceiveState>> {
        if !self
            .broadcast_receive_states
            .iter()
            .any(|characteristic| characteristic.handle == handle)
        {
            return Err(Error::InvalidValue(format!(
                "notification handle 0x{handle:04X} does not belong to BASS"
            )));
        }
        if value.is_empty() {
            Ok(None)
        } else {
            BroadcastReceiveState::from_bytes(value).map(Some)
        }
    }
}

fn encode_subgroups(target: &mut Vec<u8>, subgroups: &[SubgroupInfo]) -> Result<()> {
    let count = u8::try_from(subgroups.len())
        .map_err(|_| Error::InvalidValue("BASS has over 255 subgroups".into()))?;
    target.push(count);
    for subgroup in subgroups {
        let metadata_length = u8::try_from(subgroup.metadata.len())
            .map_err(|_| Error::InvalidValue("BASS subgroup metadata exceeds 255 bytes".into()))?;
        target.extend_from_slice(&subgroup.bis_sync.to_le_bytes());
        target.push(metadata_length);
        target.extend_from_slice(&subgroup.metadata);
    }
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
        let value = self.take(2, name)?;
        Ok(u16::from_le_bytes([value[0], value[1]]))
    }

    fn u24(&mut self, name: &str) -> Result<u32> {
        let value = self.take(3, name)?;
        Ok(u32::from_le_bytes([value[0], value[1], value[2], 0]))
    }

    fn address(&mut self, name: &str) -> Result<Address> {
        let address_type = AddressType(self.u8(&format!("{name} type"))?);
        let bytes: [u8; 6] = self
            .take(6, name)?
            .try_into()
            .expect("six-byte reader slice");
        Ok(Address::from_bytes(bytes, address_type))
    }

    fn subgroups(&mut self) -> Result<Vec<SubgroupInfo>> {
        let count = self.u8("subgroup count")?;
        let mut subgroups = Vec::with_capacity(usize::from(count));
        for _ in 0..count {
            let bis_sync = {
                let bytes = self.take(4, "BIS sync")?;
                u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
            };
            let metadata_length = usize::from(self.u8("subgroup metadata length")?);
            let metadata = self.take(metadata_length, "subgroup metadata")?.to_vec();
            subgroups.push(SubgroupInfo { bis_sync, metadata });
        }
        Ok(subgroups)
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
