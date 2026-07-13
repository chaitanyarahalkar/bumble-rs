//! Apple Media Service (AMS) protocol, GATT service, proxy, and client state.

use crate::{Error, Result};
use bumble::Uuid;
use bumble_gatt::{
    permissions, properties, AttTransport, CharacteristicDefinition, CharacteristicProxy,
    DynamicValue, GattClient, GattServer, ServiceDefinition, ServiceProxy,
};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::{Arc, Mutex};

pub const AMS_SERVICE_UUID: &str = "89D3502B-0F36-433A-8EF4-C502AD55F8DC";
pub const AMS_REMOTE_COMMAND_CHARACTERISTIC_UUID: &str = "9B3C81D8-57B1-4A8A-B8DF-0E56F7CA51C2";
pub const AMS_ENTITY_UPDATE_CHARACTERISTIC_UUID: &str = "2F7CABCE-808D-411F-9A0C-BB92BA96C102";
pub const AMS_ENTITY_ATTRIBUTE_CHARACTERISTIC_UUID: &str = "C6B2F38C-23AB-46D8-A6AB-A3A870BBD5D7";

const INVALID_ATTRIBUTE_VALUE_LENGTH: u8 = 0x0D;
const UNLIKELY_ERROR: u8 = 0x0E;

macro_rules! open_u8 {
    ($name:ident { $($constant:ident = $value:expr),+ $(,)? }) => {
        #[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
        pub struct $name(pub u8);

        impl $name {
            $(pub const $constant: Self = Self($value);)+
        }
    };
}

open_u8!(RemoteCommandId {
    PLAY = 0,
    PAUSE = 1,
    TOGGLE_PLAY_PAUSE = 2,
    NEXT_TRACK = 3,
    PREVIOUS_TRACK = 4,
    VOLUME_UP = 5,
    VOLUME_DOWN = 6,
    ADVANCE_REPEAT_MODE = 7,
    ADVANCE_SHUFFLE_MODE = 8,
    SKIP_FORWARD = 9,
    SKIP_BACKWARD = 10,
    LIKE_TRACK = 11,
    DISLIKE_TRACK = 12,
    BOOKMARK_TRACK = 13,
});

open_u8!(EntityId {
    PLAYER = 0,
    QUEUE = 1,
    TRACK = 2,
});

open_u8!(ActionId {
    POSITIVE = 0,
    NEGATIVE = 1,
});

open_u8!(PlayerAttributeId {
    NAME = 0,
    PLAYBACK_INFO = 1,
    VOLUME = 2,
});

open_u8!(QueueAttributeId {
    INDEX = 0,
    COUNT = 1,
    SHUFFLE_MODE = 2,
    REPEAT_MODE = 3,
});

open_u8!(ShuffleMode {
    OFF = 0,
    ONE = 1,
    ALL = 2,
});

open_u8!(RepeatMode {
    OFF = 0,
    ONE = 1,
    ALL = 2,
});

open_u8!(TrackAttributeId {
    ARTIST = 0,
    ALBUM = 1,
    TITLE = 2,
    DURATION = 3,
});

open_u8!(PlaybackState {
    PAUSED = 0,
    PLAYING = 1,
    REWINDING = 2,
    FAST_FORWARDING = 3,
});

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PlaybackInfo {
    pub playback_state: PlaybackState,
    pub playback_rate: f32,
    pub elapsed_time: f32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EntityUpdate {
    pub entity: EntityId,
    pub attribute_id: u8,
    pub truncated: bool,
    pub value: Vec<u8>,
}

impl EntityUpdate {
    pub fn from_bytes(data: &[u8]) -> Result<Self> {
        if data.len() < 3 {
            return Err(Error::InvalidValue(format!(
                "AMS entity update has length {}, expected at least 3",
                data.len()
            )));
        }
        Ok(Self {
            entity: EntityId(data[0]),
            attribute_id: data[1],
            truncated: data[2] & 1 != 0,
            value: data[3..].to_vec(),
        })
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut value = vec![self.entity.0, self.attribute_id, u8::from(self.truncated)];
        value.extend_from_slice(&self.value);
        value
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum PendingNotification {
    RemoteCommands(Vec<u8>),
    EntityUpdate(Vec<u8>),
}

#[derive(Clone, Debug, Default)]
struct AmsState {
    commands: VecDeque<RemoteCommandId>,
    observations: VecDeque<(EntityId, Vec<u8>)>,
    attributes: BTreeMap<(EntityId, u8), Vec<u8>>,
    selected_attribute: Option<(EntityId, u8)>,
    pending: VecDeque<PendingNotification>,
}

#[derive(Clone, Debug, Default)]
pub struct AmsService {
    state: Arc<Mutex<AmsState>>,
}

impl AmsService {
    pub fn definition(&self) -> ServiceDefinition {
        ServiceDefinition {
            uuid: ams_service_uuid(),
            primary: true,
            included_services: Vec::new(),
            characteristics: vec![
                CharacteristicDefinition {
                    uuid: remote_command_uuid(),
                    properties: properties::NOTIFY | properties::WRITE_WITHOUT_RESPONSE,
                    permissions: permissions::WRITEABLE,
                    value: Vec::new(),
                    descriptors: Vec::new(),
                },
                CharacteristicDefinition {
                    uuid: entity_update_uuid(),
                    properties: properties::NOTIFY | properties::WRITE,
                    permissions: permissions::WRITEABLE,
                    value: Vec::new(),
                    descriptors: Vec::new(),
                },
                CharacteristicDefinition {
                    uuid: entity_attribute_uuid(),
                    properties: properties::READ | properties::WRITE_WITHOUT_RESPONSE,
                    permissions: permissions::READABLE | permissions::WRITEABLE,
                    value: Vec::new(),
                    descriptors: Vec::new(),
                },
            ],
        }
    }

    pub fn bind(&self, server: &mut GattServer) -> Result<AmsHandles> {
        let remote_command = required_handle(server, &remote_command_uuid())?;
        let entity_update = required_handle(server, &entity_update_uuid())?;
        let entity_attribute = required_handle(server, &entity_attribute_uuid())?;

        let state = Arc::clone(&self.state);
        server.set_dynamic_value(
            remote_command,
            DynamicValue::write_only(move |_, value| {
                let [command]: [u8; 1] = value
                    .try_into()
                    .map_err(|_| INVALID_ATTRIBUTE_VALUE_LENGTH)?;
                state
                    .lock()
                    .map_err(|_| UNLIKELY_ERROR)?
                    .commands
                    .push_back(RemoteCommandId(command));
                Ok(())
            }),
        )?;

        let state = Arc::clone(&self.state);
        server.set_dynamic_value(
            entity_update,
            DynamicValue::write_only(move |_, value| {
                let (&entity, attributes) =
                    value.split_first().ok_or(INVALID_ATTRIBUTE_VALUE_LENGTH)?;
                state
                    .lock()
                    .map_err(|_| UNLIKELY_ERROR)?
                    .observations
                    .push_back((EntityId(entity), attributes.to_vec()));
                Ok(())
            }),
        )?;

        let read_state = Arc::clone(&self.state);
        let write_state = Arc::clone(&self.state);
        server.set_dynamic_value(
            entity_attribute,
            DynamicValue::read_write(
                move |_| {
                    let state = read_state.lock().map_err(|_| UNLIKELY_ERROR)?;
                    Ok(state
                        .selected_attribute
                        .and_then(|selector| state.attributes.get(&selector).cloned())
                        .unwrap_or_default())
                },
                move |_, value| {
                    let [entity, attribute]: [u8; 2] = value
                        .try_into()
                        .map_err(|_| INVALID_ATTRIBUTE_VALUE_LENGTH)?;
                    write_state
                        .lock()
                        .map_err(|_| UNLIKELY_ERROR)?
                        .selected_attribute = Some((EntityId(entity), attribute));
                    Ok(())
                },
            ),
        )?;
        Ok(AmsHandles {
            remote_command,
            entity_update,
            entity_attribute,
        })
    }

    pub fn take_command(&self) -> Result<Option<RemoteCommandId>> {
        self.state
            .lock()
            .map(|mut state| state.commands.pop_front())
            .map_err(|_| Error::InvalidValue("AMS state lock is poisoned".into()))
    }

    pub fn take_observation(&self) -> Result<Option<(EntityId, Vec<u8>)>> {
        self.state
            .lock()
            .map(|mut state| state.observations.pop_front())
            .map_err(|_| Error::InvalidValue("AMS state lock is poisoned".into()))
    }

    pub fn set_supported_commands(&self, commands: &[RemoteCommandId]) -> Result<()> {
        self.state
            .lock()
            .map_err(|_| Error::InvalidValue("AMS state lock is poisoned".into()))?
            .pending
            .push_back(PendingNotification::RemoteCommands(
                commands.iter().map(|command| command.0).collect(),
            ));
        Ok(())
    }

    pub fn update_entity(
        &self,
        entity: EntityId,
        attribute_id: u8,
        value: impl Into<Vec<u8>>,
        notification_limit: Option<usize>,
    ) -> Result<()> {
        let value = value.into();
        let (truncated, notification_value) = notification_limit
            .filter(|limit| *limit < value.len())
            .map(|limit| (true, value[..limit].to_vec()))
            .unwrap_or_else(|| (false, value.clone()));
        let update = EntityUpdate {
            entity,
            attribute_id,
            truncated,
            value: notification_value,
        };
        let mut state = self
            .state
            .lock()
            .map_err(|_| Error::InvalidValue("AMS state lock is poisoned".into()))?;
        state.attributes.insert((entity, attribute_id), value);
        state
            .pending
            .push_back(PendingNotification::EntityUpdate(update.to_bytes()));
        Ok(())
    }

    pub fn take_pending_notifications(&self, handles: AmsHandles) -> Result<Vec<(u16, Vec<u8>)>> {
        let notifications = self
            .state
            .lock()
            .map_err(|_| Error::InvalidValue("AMS state lock is poisoned".into()))?
            .pending
            .drain(..)
            .map(|notification| match notification {
                PendingNotification::RemoteCommands(value) => (handles.remote_command, value),
                PendingNotification::EntityUpdate(value) => (handles.entity_update, value),
            })
            .collect();
        Ok(notifications)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AmsHandles {
    pub remote_command: u16,
    pub entity_update: u16,
    pub entity_attribute: u16,
}

#[derive(Clone, Debug)]
pub struct AmsServiceProxy {
    pub service: ServiceProxy,
    pub remote_command: CharacteristicProxy,
    pub entity_update: CharacteristicProxy,
    pub entity_attribute: CharacteristicProxy,
}

impl AmsServiceProxy {
    pub fn from_parts(
        service: ServiceProxy,
        characteristics: &[CharacteristicProxy],
    ) -> Result<Self> {
        Ok(Self {
            service,
            remote_command: required_characteristic(characteristics, &remote_command_uuid())?,
            entity_update: required_characteristic(characteristics, &entity_update_uuid())?,
            entity_attribute: required_characteristic(characteristics, &entity_attribute_uuid())?,
        })
    }

    pub fn discover(
        client: &mut GattClient,
        transport: &mut impl AttTransport,
    ) -> Result<Option<Self>> {
        let mut services = client.discover_service_by_uuid(transport, &ams_service_uuid())?;
        let Some(service) = services.drain(..).next() else {
            return Ok(None);
        };
        let characteristics = client.discover_characteristics(transport, &service)?;
        Self::from_parts(service, &characteristics).map(Some)
    }

    pub fn start(&self, client: &mut GattClient, transport: &mut impl AttTransport) -> Result<()> {
        for characteristic in [&self.remote_command, &self.entity_update] {
            let cccd = client
                .discover_descriptors(transport, characteristic)?
                .into_iter()
                .find(|descriptor| descriptor.uuid == Uuid::from_16_bits(0x2902))
                .ok_or_else(|| {
                    Error::InvalidValue(format!(
                        "AMS notification characteristic {:?} has no CCCD",
                        characteristic.uuid
                    ))
                })?;
            client.subscribe(transport, characteristic.handle, cccd.handle, false)?;
        }
        Ok(())
    }

    pub fn stop(&self, client: &mut GattClient, transport: &mut impl AttTransport) -> Result<()> {
        for characteristic in [&self.remote_command, &self.entity_update] {
            let cccd = client
                .discover_descriptors(transport, characteristic)?
                .into_iter()
                .find(|descriptor| descriptor.uuid == Uuid::from_16_bits(0x2902))
                .ok_or_else(|| Error::InvalidValue("AMS characteristic has no CCCD".into()))?;
            client.unsubscribe(transport, characteristic.handle, cccd.handle)?;
        }
        Ok(())
    }

    pub fn observe(
        &self,
        client: &mut GattClient,
        transport: &mut impl AttTransport,
        entity: EntityId,
        attributes: &[u8],
    ) -> Result<()> {
        let mut value = vec![entity.0];
        value.extend_from_slice(attributes);
        client.write_value(transport, self.entity_update.handle, value, true)?;
        Ok(())
    }

    pub fn command(
        &self,
        client: &mut GattClient,
        transport: &mut impl AttTransport,
        command: RemoteCommandId,
    ) -> Result<()> {
        client.write_value(transport, self.remote_command.handle, vec![command.0], true)?;
        Ok(())
    }

    pub fn read_entity_attribute(
        &self,
        client: &mut GattClient,
        transport: &mut impl AttTransport,
        entity: EntityId,
        attribute_id: u8,
    ) -> Result<Vec<u8>> {
        client.write_value(
            transport,
            self.entity_attribute.handle,
            vec![entity.0, attribute_id],
            false,
        )?;
        Ok(client.read_value(transport, self.entity_attribute.handle, false)?)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum AmsEvent {
    SupportedCommands(BTreeSet<RemoteCommandId>),
    PlayerName(String),
    PlayerPlaybackInfo(PlaybackInfo),
    PlayerVolume(f32),
    QueueCount(u32),
    QueueIndex(u32),
    QueueShuffleMode(ShuffleMode),
    QueueRepeatMode(RepeatMode),
    TrackArtist(String),
    TrackAlbum(String),
    TrackTitle(String),
    TrackDuration(f32),
}

#[derive(Clone, Debug)]
pub struct AmsClient {
    pub supported_commands: BTreeSet<RemoteCommandId>,
    pub player_name: String,
    pub player_playback_info: PlaybackInfo,
    pub player_volume: f32,
    pub queue_count: u32,
    pub queue_index: u32,
    pub queue_shuffle_mode: ShuffleMode,
    pub queue_repeat_mode: RepeatMode,
    pub track_artist: String,
    pub track_album: String,
    pub track_title: String,
    pub track_duration: f32,
}

impl Default for AmsClient {
    fn default() -> Self {
        Self {
            supported_commands: BTreeSet::new(),
            player_name: String::new(),
            player_playback_info: PlaybackInfo {
                playback_state: PlaybackState::PAUSED,
                playback_rate: 0.0,
                elapsed_time: 0.0,
            },
            player_volume: 1.0,
            queue_count: 0,
            queue_index: 0,
            queue_shuffle_mode: ShuffleMode::OFF,
            queue_repeat_mode: RepeatMode::OFF,
            track_artist: String::new(),
            track_album: String::new(),
            track_title: String::new(),
            track_duration: 0.0,
        }
    }
}

impl AmsClient {
    pub fn on_remote_command_notification(&mut self, data: &[u8]) -> AmsEvent {
        self.supported_commands
            .extend(data.iter().copied().map(RemoteCommandId));
        AmsEvent::SupportedCommands(self.supported_commands.clone())
    }

    pub fn on_entity_update_notification(
        &mut self,
        proxy: &AmsServiceProxy,
        client: &mut GattClient,
        transport: &mut impl AttTransport,
        data: &[u8],
    ) -> Result<AmsEvent> {
        let mut update = EntityUpdate::from_bytes(data)?;
        if update.truncated {
            update.value = proxy.read_entity_attribute(
                client,
                transport,
                update.entity,
                update.attribute_id,
            )?;
        }
        self.apply_entity_update(update)
    }

    pub fn apply_entity_update(&mut self, update: EntityUpdate) -> Result<AmsEvent> {
        match (update.entity, update.attribute_id) {
            (EntityId::PLAYER, 0) => {
                self.player_name = text(&update.value, "player name")?;
                Ok(AmsEvent::PlayerName(self.player_name.clone()))
            }
            (EntityId::PLAYER, 1) => {
                let value = text(&update.value, "playback info")?;
                let mut fields = value.split(',');
                let playback_state = fields
                    .next()
                    .ok_or_else(|| Error::InvalidValue("playback info omits state".into()))?
                    .parse::<u8>()
                    .map(PlaybackState)
                    .map_err(|error| {
                        Error::InvalidValue(format!("invalid playback state: {error}"))
                    })?;
                let playback_rate = parse_float(fields.next(), "playback rate")?;
                let elapsed_time = parse_float(fields.next(), "elapsed time")?;
                if fields.next().is_some() {
                    return Err(Error::InvalidValue("playback info has extra fields".into()));
                }
                self.player_playback_info = PlaybackInfo {
                    playback_state,
                    playback_rate,
                    elapsed_time,
                };
                Ok(AmsEvent::PlayerPlaybackInfo(self.player_playback_info))
            }
            (EntityId::PLAYER, 2) => {
                self.player_volume = text(&update.value, "player volume")?
                    .parse()
                    .map_err(|error| Error::InvalidValue(format!("invalid volume: {error}")))?;
                Ok(AmsEvent::PlayerVolume(self.player_volume))
            }
            (EntityId::QUEUE, 0) => {
                self.queue_index = parse_u32(&update.value, "queue index")?;
                Ok(AmsEvent::QueueIndex(self.queue_index))
            }
            (EntityId::QUEUE, 1) => {
                self.queue_count = parse_u32(&update.value, "queue count")?;
                Ok(AmsEvent::QueueCount(self.queue_count))
            }
            (EntityId::QUEUE, 2) => {
                self.queue_shuffle_mode = ShuffleMode(parse_u8(&update.value, "shuffle mode")?);
                Ok(AmsEvent::QueueShuffleMode(self.queue_shuffle_mode))
            }
            (EntityId::QUEUE, 3) => {
                self.queue_repeat_mode = RepeatMode(parse_u8(&update.value, "repeat mode")?);
                Ok(AmsEvent::QueueRepeatMode(self.queue_repeat_mode))
            }
            (EntityId::TRACK, 0) => {
                self.track_artist = text(&update.value, "track artist")?;
                Ok(AmsEvent::TrackArtist(self.track_artist.clone()))
            }
            (EntityId::TRACK, 1) => {
                self.track_album = text(&update.value, "track album")?;
                Ok(AmsEvent::TrackAlbum(self.track_album.clone()))
            }
            (EntityId::TRACK, 2) => {
                self.track_title = text(&update.value, "track title")?;
                Ok(AmsEvent::TrackTitle(self.track_title.clone()))
            }
            (EntityId::TRACK, 3) => {
                self.track_duration = text(&update.value, "track duration")?
                    .parse()
                    .map_err(|error| Error::InvalidValue(format!("invalid duration: {error}")))?;
                Ok(AmsEvent::TrackDuration(self.track_duration))
            }
            (entity, attribute) => Err(Error::InvalidValue(format!(
                "unknown AMS entity/attribute {}/{}",
                entity.0, attribute
            ))),
        }
    }
}

fn parse_float(value: Option<&str>, name: &str) -> Result<f32> {
    value
        .ok_or_else(|| Error::InvalidValue(format!("playback info omits {name}")))?
        .parse()
        .map_err(|error| Error::InvalidValue(format!("invalid {name}: {error}")))
}

fn parse_u32(value: &[u8], name: &str) -> Result<u32> {
    text(value, name)?
        .parse()
        .map_err(|error| Error::InvalidValue(format!("invalid {name}: {error}")))
}

fn parse_u8(value: &[u8], name: &str) -> Result<u8> {
    text(value, name)?
        .parse()
        .map_err(|error| Error::InvalidValue(format!("invalid {name}: {error}")))
}

fn text(value: &[u8], name: &str) -> Result<String> {
    String::from_utf8(value.to_vec())
        .map_err(|error| Error::InvalidValue(format!("invalid {name} UTF-8: {error}")))
}

fn required_handle(server: &GattServer, characteristic_uuid: &Uuid) -> Result<u16> {
    server
        .handles_by_uuid(characteristic_uuid)
        .into_iter()
        .next()
        .ok_or_else(|| {
            Error::InvalidValue(format!(
                "required AMS characteristic {:?} is missing",
                characteristic_uuid
            ))
        })
}

fn required_characteristic(
    characteristics: &[CharacteristicProxy],
    characteristic_uuid: &Uuid,
) -> Result<CharacteristicProxy> {
    characteristics
        .iter()
        .find(|characteristic| characteristic.uuid == *characteristic_uuid)
        .cloned()
        .ok_or_else(|| {
            Error::InvalidValue(format!(
                "required AMS characteristic {:?} is missing",
                characteristic_uuid
            ))
        })
}

fn ams_service_uuid() -> Uuid {
    Uuid::parse(AMS_SERVICE_UUID).expect("valid AMS service UUID")
}

fn remote_command_uuid() -> Uuid {
    Uuid::parse(AMS_REMOTE_COMMAND_CHARACTERISTIC_UUID).expect("valid AMS command UUID")
}

fn entity_update_uuid() -> Uuid {
    Uuid::parse(AMS_ENTITY_UPDATE_CHARACTERISTIC_UUID).expect("valid AMS update UUID")
}

fn entity_attribute_uuid() -> Uuid {
    Uuid::parse(AMS_ENTITY_ATTRIBUTE_CHARACTERISTIC_UUID).expect("valid AMS attribute UUID")
}
