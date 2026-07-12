use crate::{
    ApplicationSettingAttributeId, ApplicationSettingValue, Error, EventId,
    PlayerApplicationSetting, Result,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PlayStatus(pub u8);

impl PlayStatus {
    pub const STOPPED: Self = Self(0x00);
    pub const PLAYING: Self = Self(0x01);
    pub const PAUSED: Self = Self(0x02);
    pub const FWD_SEEK: Self = Self(0x03);
    pub const REV_SEEK: Self = Self(0x04);
    pub const ERROR: Self = Self(0xFF);
}

/// Every notification event class registered by upstream Bumble.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    PlaybackStatusChanged {
        play_status: PlayStatus,
    },
    PlaybackPositionChanged {
        playback_position: u32,
    },
    TrackChanged {
        uid: u64,
    },
    PlayerApplicationSettingChanged {
        settings: Vec<PlayerApplicationSetting>,
    },
    NowPlayingContentChanged,
    AvailablePlayersChanged,
    AddressedPlayerChanged {
        player_id: u16,
        uid_counter: u16,
    },
    UidsChanged {
        uid_counter: u16,
    },
    VolumeChanged {
        volume: u8,
    },
    Unknown {
        event_id: EventId,
        data: Vec<u8>,
    },
}

impl Event {
    pub const NO_TRACK: u64 = u64::MAX;

    pub fn event_id(&self) -> EventId {
        match self {
            Self::PlaybackStatusChanged { .. } => EventId::PLAYBACK_STATUS_CHANGED,
            Self::PlaybackPositionChanged { .. } => EventId::PLAYBACK_POS_CHANGED,
            Self::TrackChanged { .. } => EventId::TRACK_CHANGED,
            Self::PlayerApplicationSettingChanged { .. } => {
                EventId::PLAYER_APPLICATION_SETTING_CHANGED
            }
            Self::NowPlayingContentChanged => EventId::NOW_PLAYING_CONTENT_CHANGED,
            Self::AvailablePlayersChanged => EventId::AVAILABLE_PLAYERS_CHANGED,
            Self::AddressedPlayerChanged { .. } => EventId::ADDRESSED_PLAYER_CHANGED,
            Self::UidsChanged { .. } => EventId::UIDS_CHANGED,
            Self::VolumeChanged { .. } => EventId::VOLUME_CHANGED,
            Self::Unknown { event_id, .. } => *event_id,
        }
    }

    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        let mut bytes = vec![self.event_id().0];
        match self {
            Self::PlaybackStatusChanged { play_status } => bytes.push(play_status.0),
            Self::PlaybackPositionChanged { playback_position } => {
                bytes.extend_from_slice(&playback_position.to_be_bytes());
            }
            Self::TrackChanged { uid } => bytes.extend_from_slice(&uid.to_be_bytes()),
            Self::PlayerApplicationSettingChanged { settings } => {
                bytes.push(
                    u8::try_from(settings.len())
                        .map_err(|_| Error::InvalidField("AVRCP event setting count"))?,
                );
                for setting in settings {
                    bytes.extend_from_slice(&[setting.attribute.0, setting.value.0]);
                }
            }
            Self::NowPlayingContentChanged | Self::AvailablePlayersChanged => {}
            Self::AddressedPlayerChanged {
                player_id,
                uid_counter,
            } => {
                bytes.extend_from_slice(&player_id.to_be_bytes());
                bytes.extend_from_slice(&uid_counter.to_be_bytes());
            }
            Self::UidsChanged { uid_counter } => {
                bytes.extend_from_slice(&uid_counter.to_be_bytes());
            }
            Self::VolumeChanged { volume } => bytes.push(*volume),
            Self::Unknown { data, .. } => bytes.extend_from_slice(data),
        }
        Ok(bytes)
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        let (&event_id, data) = bytes
            .split_first()
            .ok_or(Error::Truncated("AVRCP event ID"))?;
        let event_id = EventId(event_id);
        let event = match event_id {
            EventId::PLAYBACK_STATUS_CHANGED => Self::PlaybackStatusChanged {
                play_status: PlayStatus(one(data)?),
            },
            EventId::PLAYBACK_POS_CHANGED => Self::PlaybackPositionChanged {
                playback_position: u32::from_be_bytes(exact(data, 4)?.try_into().unwrap()),
            },
            EventId::TRACK_CHANGED => Self::TrackChanged {
                uid: u64::from_be_bytes(exact(data, 8)?.try_into().unwrap()),
            },
            EventId::PLAYER_APPLICATION_SETTING_CHANGED => {
                let (&count, entries) = data
                    .split_first()
                    .ok_or(Error::Truncated("AVRCP event setting count"))?;
                let expected = usize::from(count) * 2;
                let entries = exact(entries, expected)?;
                let settings = entries
                    .chunks_exact(2)
                    .map(|entry| PlayerApplicationSetting {
                        attribute: ApplicationSettingAttributeId(entry[0]),
                        value: ApplicationSettingValue(entry[1]),
                    })
                    .collect();
                Self::PlayerApplicationSettingChanged { settings }
            }
            EventId::NOW_PLAYING_CONTENT_CHANGED => {
                exact(data, 0)?;
                Self::NowPlayingContentChanged
            }
            EventId::AVAILABLE_PLAYERS_CHANGED => {
                exact(data, 0)?;
                Self::AvailablePlayersChanged
            }
            EventId::ADDRESSED_PLAYER_CHANGED => {
                let data = exact(data, 4)?;
                Self::AddressedPlayerChanged {
                    player_id: u16::from_be_bytes(data[..2].try_into().unwrap()),
                    uid_counter: u16::from_be_bytes(data[2..].try_into().unwrap()),
                }
            }
            EventId::UIDS_CHANGED => Self::UidsChanged {
                uid_counter: u16::from_be_bytes(exact(data, 2)?.try_into().unwrap()),
            },
            EventId::VOLUME_CHANGED => Self::VolumeChanged { volume: one(data)? },
            _ => Self::Unknown {
                event_id,
                data: data.to_vec(),
            },
        };
        Ok(event)
    }
}

fn one(data: &[u8]) -> Result<u8> {
    Ok(exact(data, 1)?[0])
}

fn exact(data: &[u8], length: usize) -> Result<&[u8]> {
    if data.len() < length {
        Err(Error::Truncated("AVRCP event parameters"))
    } else if data.len() > length {
        Err(Error::TrailingBytes(data.len() - length))
    } else {
        Ok(data)
    }
}
