//! Media Control Service (MCS) and Generic Media Control Service (GMCS).

use crate::{discover_profile, find_characteristic, uuid, Error, Result};
use bumble_gatt::{
    permissions, properties, AttTransport, CharacteristicDefinition, CharacteristicProxy,
    DynamicValue, GattClient, GattServer, ServiceDefinition, ServiceProxy,
};
use std::collections::VecDeque;
use std::ops::{BitOr, BitOrAssign, Deref};
use std::sync::{Arc, Mutex};

pub const MEDIA_CONTROL_SERVICE: u16 = 0x1848;
pub const GENERIC_MEDIA_CONTROL_SERVICE: u16 = 0x1849;

pub const MEDIA_PLAYER_NAME_CHARACTERISTIC: u16 = 0x2B93;
pub const MEDIA_PLAYER_ICON_OBJECT_ID_CHARACTERISTIC: u16 = 0x2B94;
pub const MEDIA_PLAYER_ICON_URL_CHARACTERISTIC: u16 = 0x2B95;
pub const TRACK_CHANGED_CHARACTERISTIC: u16 = 0x2B96;
pub const TRACK_TITLE_CHARACTERISTIC: u16 = 0x2B97;
pub const TRACK_DURATION_CHARACTERISTIC: u16 = 0x2B98;
pub const TRACK_POSITION_CHARACTERISTIC: u16 = 0x2B99;
pub const PLAYBACK_SPEED_CHARACTERISTIC: u16 = 0x2B9A;
pub const SEEKING_SPEED_CHARACTERISTIC: u16 = 0x2B9B;
pub const CURRENT_TRACK_SEGMENTS_OBJECT_ID_CHARACTERISTIC: u16 = 0x2B9C;
pub const CURRENT_TRACK_OBJECT_ID_CHARACTERISTIC: u16 = 0x2B9D;
pub const NEXT_TRACK_OBJECT_ID_CHARACTERISTIC: u16 = 0x2B9E;
pub const PARENT_GROUP_OBJECT_ID_CHARACTERISTIC: u16 = 0x2B9F;
pub const CURRENT_GROUP_OBJECT_ID_CHARACTERISTIC: u16 = 0x2BA0;
pub const PLAYING_ORDER_CHARACTERISTIC: u16 = 0x2BA1;
pub const PLAYING_ORDERS_SUPPORTED_CHARACTERISTIC: u16 = 0x2BA2;
pub const MEDIA_STATE_CHARACTERISTIC: u16 = 0x2BA3;
pub const MEDIA_CONTROL_POINT_CHARACTERISTIC: u16 = 0x2BA4;
pub const MEDIA_CONTROL_POINT_OPCODES_SUPPORTED_CHARACTERISTIC: u16 = 0x2BA5;
pub const SEARCH_RESULTS_OBJECT_ID_CHARACTERISTIC: u16 = 0x2BA6;
pub const SEARCH_CONTROL_POINT_CHARACTERISTIC: u16 = 0x2BA7;
pub const CONTENT_CONTROL_ID_CHARACTERISTIC: u16 = 0x2BBA;

const INVALID_ATTRIBUTE_VALUE_LENGTH: u8 = 0x0D;
const UNLIKELY_ERROR: u8 = 0x0E;

macro_rules! open_u8 {
    ($name:ident { $($constant:ident = $value:expr),+ $(,)? }) => {
        #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
        pub struct $name(pub u8);

        impl $name {
            $(pub const $constant: Self = Self($value);)+
        }
    };
}

open_u8!(PlayingOrder {
    SINGLE_ONCE = 0x01,
    SINGLE_REPEAT = 0x02,
    IN_ORDER_ONCE = 0x03,
    IN_ORDER_REPEAT = 0x04,
    OLDEST_ONCE = 0x05,
    OLDEST_REPEAT = 0x06,
    NEWEST_ONCE = 0x07,
    NEWEST_REPEAT = 0x08,
    SHUFFLE_ONCE = 0x09,
    SHUFFLE_REPEAT = 0x0A,
});

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PlayingOrderSupported(pub u16);

impl PlayingOrderSupported {
    pub const SINGLE_ONCE: Self = Self(0x0001);
    pub const SINGLE_REPEAT: Self = Self(0x0002);
    pub const IN_ORDER_ONCE: Self = Self(0x0004);
    pub const IN_ORDER_REPEAT: Self = Self(0x0008);
    pub const OLDEST_ONCE: Self = Self(0x0010);
    pub const OLDEST_REPEAT: Self = Self(0x0020);
    pub const NEWEST_ONCE: Self = Self(0x0040);
    pub const NEWEST_REPEAT: Self = Self(0x0080);
    pub const SHUFFLE_ONCE: Self = Self(0x0100);
    pub const SHUFFLE_REPEAT: Self = Self(0x0200);
}

impl BitOr for PlayingOrderSupported {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self::Output {
        Self(self.0 | rhs.0)
    }
}

impl BitOrAssign for PlayingOrderSupported {
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}

open_u8!(MediaState {
    INACTIVE = 0x00,
    PLAYING = 0x01,
    PAUSED = 0x02,
    SEEKING = 0x03,
});

open_u8!(MediaControlPointOpcode {
    PLAY = 0x01,
    PAUSE = 0x02,
    FAST_REWIND = 0x03,
    FAST_FORWARD = 0x04,
    STOP = 0x05,
    MOVE_RELATIVE = 0x10,
    PREVIOUS_SEGMENT = 0x20,
    NEXT_SEGMENT = 0x21,
    FIRST_SEGMENT = 0x22,
    LAST_SEGMENT = 0x23,
    GOTO_SEGMENT = 0x24,
    PREVIOUS_TRACK = 0x30,
    NEXT_TRACK = 0x31,
    FIRST_TRACK = 0x32,
    LAST_TRACK = 0x33,
    GOTO_TRACK = 0x34,
    PREVIOUS_GROUP = 0x40,
    NEXT_GROUP = 0x41,
    FIRST_GROUP = 0x42,
    LAST_GROUP = 0x43,
    GOTO_GROUP = 0x44,
});

open_u8!(MediaControlPointResultCode {
    SUCCESS = 0x01,
    OPCODE_NOT_SUPPORTED = 0x02,
    MEDIA_PLAYER_INACTIVE = 0x03,
    COMMAND_CANNOT_BE_COMPLETED = 0x04,
});

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MediaControlPointOpcodeSupported(pub u32);

impl MediaControlPointOpcodeSupported {
    pub const PLAY: Self = Self(0x0000_0001);
    pub const PAUSE: Self = Self(0x0000_0002);
    pub const FAST_REWIND: Self = Self(0x0000_0004);
    pub const FAST_FORWARD: Self = Self(0x0000_0008);
    pub const STOP: Self = Self(0x0000_0010);
    pub const MOVE_RELATIVE: Self = Self(0x0000_0020);
    pub const PREVIOUS_SEGMENT: Self = Self(0x0000_0040);
    pub const NEXT_SEGMENT: Self = Self(0x0000_0080);
    pub const FIRST_SEGMENT: Self = Self(0x0000_0100);
    pub const LAST_SEGMENT: Self = Self(0x0000_0200);
    pub const GOTO_SEGMENT: Self = Self(0x0000_0400);
    pub const PREVIOUS_TRACK: Self = Self(0x0000_0800);
    pub const NEXT_TRACK: Self = Self(0x0000_1000);
    pub const FIRST_TRACK: Self = Self(0x0000_2000);
    pub const LAST_TRACK: Self = Self(0x0000_4000);
    pub const GOTO_TRACK: Self = Self(0x0000_8000);
    pub const PREVIOUS_GROUP: Self = Self(0x0001_0000);
    pub const NEXT_GROUP: Self = Self(0x0002_0000);
    pub const FIRST_GROUP: Self = Self(0x0004_0000);
    pub const LAST_GROUP: Self = Self(0x0008_0000);
    pub const GOTO_GROUP: Self = Self(0x0010_0000);
}

impl BitOr for MediaControlPointOpcodeSupported {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self::Output {
        Self(self.0 | rhs.0)
    }
}

impl BitOrAssign for MediaControlPointOpcodeSupported {
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}

open_u8!(SearchControlPointItemType {
    TRACK_NAME = 0x01,
    ARTIST_NAME = 0x02,
    ALBUM_NAME = 0x03,
    GROUP_NAME = 0x04,
    EARLIEST_YEAR = 0x05,
    LATEST_YEAR = 0x06,
    GENRE = 0x07,
    ONLY_TRACKS = 0x08,
    ONLY_GROUPS = 0x09,
});

open_u8!(ObjectType {
    TASK = 0x00,
    GROUP = 0x01,
});

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ObjectId(pub u64);

impl ObjectId {
    pub const MAX: u64 = (1u64 << 48) - 1;

    pub fn new(value: u64) -> Result<Self> {
        if value > Self::MAX {
            return Err(Error::InvalidValue(format!(
                "object ID 0x{value:X} exceeds 48 bits"
            )));
        }
        Ok(Self(value))
    }

    pub fn decode(value: &[u8]) -> Result<Self> {
        if value.len() != 6 {
            return Err(Error::InvalidValue(format!(
                "object ID has length {}, expected 6",
                value.len()
            )));
        }
        let mut bytes = [0; 8];
        bytes[..6].copy_from_slice(value);
        Ok(Self(u64::from_le_bytes(bytes)))
    }

    pub fn encode(self) -> Result<[u8; 6]> {
        Self::new(self.0)?;
        let bytes = self.0.to_le_bytes();
        Ok(bytes[..6].try_into().expect("six-byte object ID"))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GroupObjectType {
    pub object_type: ObjectType,
    pub object_id: ObjectId,
}

impl GroupObjectType {
    pub fn decode(value: &[u8]) -> Result<Self> {
        if value.len() != 7 {
            return Err(Error::InvalidValue(format!(
                "group object has length {}, expected 7",
                value.len()
            )));
        }
        Ok(Self {
            object_type: ObjectType(value[0]),
            object_id: ObjectId::decode(&value[1..])?,
        })
    }

    pub fn encode(self) -> Result<[u8; 7]> {
        let mut value = [0; 7];
        value[0] = self.object_type.0;
        value[1..].copy_from_slice(&self.object_id.encode()?);
        Ok(value)
    }
}

#[derive(Clone)]
pub struct MediaControlService {
    service_uuid: u16,
    media_player_name: String,
    pending_control_responses: Arc<Mutex<VecDeque<[u8; 2]>>>,
}

impl core::fmt::Debug for MediaControlService {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter
            .debug_struct("MediaControlService")
            .field("service_uuid", &format_args!("0x{:04X}", self.service_uuid))
            .field("media_player_name", &self.media_player_name)
            .finish_non_exhaustive()
    }
}

impl Default for MediaControlService {
    fn default() -> Self {
        Self::new(None)
    }
}

impl MediaControlService {
    pub fn new(media_player_name: Option<&str>) -> Self {
        Self {
            service_uuid: MEDIA_CONTROL_SERVICE,
            media_player_name: media_player_name.unwrap_or("Bumble Player").into(),
            pending_control_responses: Arc::new(Mutex::new(VecDeque::new())),
        }
    }

    pub fn generic(media_player_name: Option<&str>) -> Self {
        Self {
            service_uuid: GENERIC_MEDIA_CONTROL_SERVICE,
            ..Self::new(media_player_name)
        }
    }

    pub fn definition(&self) -> ServiceDefinition {
        let read_encrypted = permissions::READ_REQUIRES_ENCRYPTION;
        let write_encrypted = permissions::WRITE_REQUIRES_ENCRYPTION;
        ServiceDefinition {
            uuid: uuid(self.service_uuid),
            primary: true,
            included_services: Vec::new(),
            characteristics: vec![
                characteristic(
                    MEDIA_PLAYER_NAME_CHARACTERISTIC,
                    properties::READ | properties::NOTIFY,
                    read_encrypted,
                    self.media_player_name.as_bytes().to_vec(),
                ),
                characteristic(
                    TRACK_CHANGED_CHARACTERISTIC,
                    properties::NOTIFY,
                    read_encrypted,
                    Vec::new(),
                ),
                characteristic(
                    TRACK_TITLE_CHARACTERISTIC,
                    properties::READ | properties::NOTIFY,
                    read_encrypted,
                    Vec::new(),
                ),
                characteristic(
                    TRACK_DURATION_CHARACTERISTIC,
                    properties::READ | properties::NOTIFY,
                    read_encrypted,
                    Vec::new(),
                ),
                characteristic(
                    TRACK_POSITION_CHARACTERISTIC,
                    properties::READ
                        | properties::WRITE
                        | properties::WRITE_WITHOUT_RESPONSE
                        | properties::NOTIFY,
                    read_encrypted | write_encrypted,
                    Vec::new(),
                ),
                characteristic(
                    MEDIA_STATE_CHARACTERISTIC,
                    properties::READ | properties::NOTIFY,
                    read_encrypted,
                    Vec::new(),
                ),
                characteristic(
                    MEDIA_CONTROL_POINT_CHARACTERISTIC,
                    properties::WRITE | properties::WRITE_WITHOUT_RESPONSE | properties::NOTIFY,
                    read_encrypted | write_encrypted,
                    Vec::new(),
                ),
                characteristic(
                    MEDIA_CONTROL_POINT_OPCODES_SUPPORTED_CHARACTERISTIC,
                    properties::READ | properties::NOTIFY,
                    read_encrypted,
                    Vec::new(),
                ),
                characteristic(
                    CONTENT_CONTROL_ID_CHARACTERISTIC,
                    properties::READ,
                    read_encrypted,
                    Vec::new(),
                ),
            ],
        }
    }

    pub fn bind(&self, server: &mut GattServer) -> Result<MediaControlHandles> {
        let control_point = required_handle(server, MEDIA_CONTROL_POINT_CHARACTERISTIC)?;
        let responses = Arc::clone(&self.pending_control_responses);
        server.set_dynamic_value(
            control_point,
            DynamicValue::write_only(move |_, value| {
                let opcode = value
                    .first()
                    .copied()
                    .ok_or(INVALID_ATTRIBUTE_VALUE_LENGTH)?;
                responses
                    .lock()
                    .map_err(|_| UNLIKELY_ERROR)?
                    .push_back([opcode, MediaControlPointResultCode::SUCCESS.0]);
                Ok(())
            }),
        )?;
        Ok(MediaControlHandles {
            media_player_name: required_handle(server, MEDIA_PLAYER_NAME_CHARACTERISTIC)?,
            track_changed: required_handle(server, TRACK_CHANGED_CHARACTERISTIC)?,
            track_title: required_handle(server, TRACK_TITLE_CHARACTERISTIC)?,
            track_duration: required_handle(server, TRACK_DURATION_CHARACTERISTIC)?,
            track_position: required_handle(server, TRACK_POSITION_CHARACTERISTIC)?,
            media_state: required_handle(server, MEDIA_STATE_CHARACTERISTIC)?,
            media_control_point: control_point,
        })
    }

    pub fn take_control_response(&self) -> Result<Option<[u8; 2]>> {
        self.pending_control_responses
            .lock()
            .map(|mut responses| responses.pop_front())
            .map_err(|_| Error::InvalidValue("MCP response queue lock is poisoned".into()))
    }
}

#[derive(Clone, Debug)]
pub struct GenericMediaControlService(MediaControlService);

impl Default for GenericMediaControlService {
    fn default() -> Self {
        Self::new(None)
    }
}

impl GenericMediaControlService {
    pub fn new(media_player_name: Option<&str>) -> Self {
        Self(MediaControlService::generic(media_player_name))
    }
}

impl Deref for GenericMediaControlService {
    type Target = MediaControlService;

    fn deref(&self) -> &Self::Target {
        &self.0
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MediaControlHandles {
    pub media_player_name: u16,
    pub track_changed: u16,
    pub track_title: u16,
    pub track_duration: u16,
    pub track_position: u16,
    pub media_state: u16,
    pub media_control_point: u16,
}

#[derive(Clone, Debug)]
pub struct MediaControlServiceProxy {
    pub service: ServiceProxy,
    pub media_player_name: Option<CharacteristicProxy>,
    pub media_player_icon_object_id: Option<CharacteristicProxy>,
    pub media_player_icon_url: Option<CharacteristicProxy>,
    pub track_changed: Option<CharacteristicProxy>,
    pub track_title: Option<CharacteristicProxy>,
    pub track_duration: Option<CharacteristicProxy>,
    pub track_position: Option<CharacteristicProxy>,
    pub playback_speed: Option<CharacteristicProxy>,
    pub seeking_speed: Option<CharacteristicProxy>,
    pub current_track_segments_object_id: Option<CharacteristicProxy>,
    pub current_track_object_id: Option<CharacteristicProxy>,
    pub next_track_object_id: Option<CharacteristicProxy>,
    pub parent_group_object_id: Option<CharacteristicProxy>,
    pub current_group_object_id: Option<CharacteristicProxy>,
    pub playing_order: Option<CharacteristicProxy>,
    pub playing_orders_supported: Option<CharacteristicProxy>,
    pub media_state: Option<CharacteristicProxy>,
    pub media_control_point: Option<CharacteristicProxy>,
    pub media_control_point_opcodes_supported: Option<CharacteristicProxy>,
    pub search_control_point: Option<CharacteristicProxy>,
    pub search_results_object_id: Option<CharacteristicProxy>,
    pub content_control_id: Option<CharacteristicProxy>,
}

impl MediaControlServiceProxy {
    pub fn from_parts(service: ServiceProxy, characteristics: &[CharacteristicProxy]) -> Self {
        Self {
            service,
            media_player_name: find_characteristic(
                characteristics,
                MEDIA_PLAYER_NAME_CHARACTERISTIC,
            ),
            media_player_icon_object_id: find_characteristic(
                characteristics,
                MEDIA_PLAYER_ICON_OBJECT_ID_CHARACTERISTIC,
            ),
            media_player_icon_url: find_characteristic(
                characteristics,
                MEDIA_PLAYER_ICON_URL_CHARACTERISTIC,
            ),
            track_changed: find_characteristic(characteristics, TRACK_CHANGED_CHARACTERISTIC),
            track_title: find_characteristic(characteristics, TRACK_TITLE_CHARACTERISTIC),
            track_duration: find_characteristic(characteristics, TRACK_DURATION_CHARACTERISTIC),
            track_position: find_characteristic(characteristics, TRACK_POSITION_CHARACTERISTIC),
            playback_speed: find_characteristic(characteristics, PLAYBACK_SPEED_CHARACTERISTIC),
            seeking_speed: find_characteristic(characteristics, SEEKING_SPEED_CHARACTERISTIC),
            current_track_segments_object_id: find_characteristic(
                characteristics,
                CURRENT_TRACK_SEGMENTS_OBJECT_ID_CHARACTERISTIC,
            ),
            current_track_object_id: find_characteristic(
                characteristics,
                CURRENT_TRACK_OBJECT_ID_CHARACTERISTIC,
            ),
            next_track_object_id: find_characteristic(
                characteristics,
                NEXT_TRACK_OBJECT_ID_CHARACTERISTIC,
            ),
            parent_group_object_id: find_characteristic(
                characteristics,
                PARENT_GROUP_OBJECT_ID_CHARACTERISTIC,
            ),
            current_group_object_id: find_characteristic(
                characteristics,
                CURRENT_GROUP_OBJECT_ID_CHARACTERISTIC,
            ),
            playing_order: find_characteristic(characteristics, PLAYING_ORDER_CHARACTERISTIC),
            playing_orders_supported: find_characteristic(
                characteristics,
                PLAYING_ORDERS_SUPPORTED_CHARACTERISTIC,
            ),
            media_state: find_characteristic(characteristics, MEDIA_STATE_CHARACTERISTIC),
            media_control_point: find_characteristic(
                characteristics,
                MEDIA_CONTROL_POINT_CHARACTERISTIC,
            ),
            media_control_point_opcodes_supported: find_characteristic(
                characteristics,
                MEDIA_CONTROL_POINT_OPCODES_SUPPORTED_CHARACTERISTIC,
            ),
            search_control_point: find_characteristic(
                characteristics,
                SEARCH_CONTROL_POINT_CHARACTERISTIC,
            ),
            search_results_object_id: find_characteristic(
                characteristics,
                SEARCH_RESULTS_OBJECT_ID_CHARACTERISTIC,
            ),
            content_control_id: find_characteristic(
                characteristics,
                CONTENT_CONTROL_ID_CHARACTERISTIC,
            ),
        }
    }

    pub fn discover(
        client: &mut GattClient,
        transport: &mut impl AttTransport,
    ) -> Result<Option<Self>> {
        Self::discover_by_uuid(client, transport, MEDIA_CONTROL_SERVICE)
    }

    pub fn discover_generic(
        client: &mut GattClient,
        transport: &mut impl AttTransport,
    ) -> Result<Option<Self>> {
        Self::discover_by_uuid(client, transport, GENERIC_MEDIA_CONTROL_SERVICE)
    }

    fn discover_by_uuid(
        client: &mut GattClient,
        transport: &mut impl AttTransport,
        service_uuid: u16,
    ) -> Result<Option<Self>> {
        let Some((service, characteristics)) = discover_profile(client, transport, service_uuid)?
        else {
            return Ok(None);
        };
        Ok(Some(Self::from_parts(service, &characteristics)))
    }

    pub fn subscribe_characteristics(
        &self,
        client: &mut GattClient,
        transport: &mut impl AttTransport,
    ) -> Result<()> {
        for characteristic in [
            self.media_control_point.as_ref(),
            self.media_state.as_ref(),
            self.track_changed.as_ref(),
            self.track_title.as_ref(),
            self.track_duration.as_ref(),
            self.track_position.as_ref(),
        ]
        .into_iter()
        .flatten()
        {
            let cccd = client
                .discover_descriptors(transport, characteristic)?
                .into_iter()
                .find(|descriptor| descriptor.uuid == uuid(0x2902))
                .ok_or_else(|| {
                    Error::InvalidValue(format!(
                        "notification characteristic {:?} has no CCCD",
                        characteristic.uuid
                    ))
                })?;
            client.subscribe(transport, characteristic.handle, cccd.handle, false)?;
        }
        Ok(())
    }

    pub fn write_control_point(
        &self,
        client: &mut GattClient,
        transport: &mut impl AttTransport,
        opcode: MediaControlPointOpcode,
    ) -> Result<()> {
        let characteristic = self.media_control_point.as_ref().ok_or_else(|| {
            Error::InvalidValue("peer does not have a media control point".into())
        })?;
        client.write_value(transport, characteristic.handle, vec![opcode.0], false)?;
        Ok(())
    }

    pub fn event_from_notification(&self, handle: u16, value: &[u8]) -> Result<MediaControlEvent> {
        if self
            .media_control_point
            .as_ref()
            .is_some_and(|characteristic| characteristic.handle == handle)
        {
            if value.len() != 2 {
                return Err(Error::InvalidValue(format!(
                    "media control response has length {}, expected 2",
                    value.len()
                )));
            }
            return Ok(MediaControlEvent::ControlPoint {
                opcode: MediaControlPointOpcode(value[0]),
                result: MediaControlPointResultCode(value[1]),
            });
        }
        if self
            .media_state
            .as_ref()
            .is_some_and(|characteristic| characteristic.handle == handle)
        {
            let state =
                value.first().copied().map(MediaState).ok_or_else(|| {
                    Error::InvalidValue("media state notification is empty".into())
                })?;
            return Ok(MediaControlEvent::MediaState(state));
        }
        if self
            .track_changed
            .as_ref()
            .is_some_and(|characteristic| characteristic.handle == handle)
        {
            return Ok(MediaControlEvent::TrackChanged);
        }
        if self
            .track_title
            .as_ref()
            .is_some_and(|characteristic| characteristic.handle == handle)
        {
            return String::from_utf8(value.to_vec())
                .map(MediaControlEvent::TrackTitle)
                .map_err(|error| Error::InvalidValue(format!("invalid track title: {error}")));
        }
        if self
            .track_duration
            .as_ref()
            .is_some_and(|characteristic| characteristic.handle == handle)
        {
            return decode_i32(value).map(MediaControlEvent::TrackDuration);
        }
        if self
            .track_position
            .as_ref()
            .is_some_and(|characteristic| characteristic.handle == handle)
        {
            return decode_i32(value).map(MediaControlEvent::TrackPosition);
        }
        Err(Error::InvalidValue(format!(
            "notification handle 0x{handle:04X} is not an MCP event characteristic"
        )))
    }

    pub fn control_result(
        &self,
        expected_opcode: MediaControlPointOpcode,
        event: MediaControlEvent,
    ) -> Result<MediaControlPointResultCode> {
        let MediaControlEvent::ControlPoint { opcode, result } = event else {
            return Err(Error::InvalidValue(
                "expected a media control point notification".into(),
            ));
        };
        if opcode != expected_opcode {
            return Err(Error::InvalidValue(format!(
                "expected media opcode 0x{:02X}, received 0x{:02X}",
                expected_opcode.0, opcode.0
            )));
        }
        Ok(result)
    }
}

fn decode_i32(value: &[u8]) -> Result<i32> {
    let bytes: [u8; 4] = value.try_into().map_err(|_| {
        Error::InvalidValue(format!(
            "signed media value has length {}, expected 4",
            value.len()
        ))
    })?;
    Ok(i32::from_le_bytes(bytes))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MediaControlEvent {
    ControlPoint {
        opcode: MediaControlPointOpcode,
        result: MediaControlPointResultCode,
    },
    MediaState(MediaState),
    TrackChanged,
    TrackTitle(String),
    TrackDuration(i32),
    TrackPosition(i32),
}

#[derive(Clone, Debug)]
pub struct GenericMediaControlServiceProxy(MediaControlServiceProxy);

impl GenericMediaControlServiceProxy {
    pub fn discover(
        client: &mut GattClient,
        transport: &mut impl AttTransport,
    ) -> Result<Option<Self>> {
        MediaControlServiceProxy::discover_generic(client, transport).map(|proxy| proxy.map(Self))
    }
}

impl Deref for GenericMediaControlServiceProxy {
    type Target = MediaControlServiceProxy;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
