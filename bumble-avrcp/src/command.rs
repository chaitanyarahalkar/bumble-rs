use crate::{Error, PacketType, PduId, Result, VendorPdu};

macro_rules! open_integer {
    ($name:ident, $type:ty { $($constant:ident = $value:expr),+ $(,)? }) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub struct $name(pub $type);
        impl $name { $(pub const $constant: Self = Self($value);)+ }
    };
}

open_integer!(CapabilityId, u8 {
    COMPANY_ID = 0x02,
    EVENTS_SUPPORTED = 0x03,
});

open_integer!(ApplicationSettingAttributeId, u8 {
    EQUALIZER_ON_OFF = 0x01,
    REPEAT_MODE = 0x02,
    SHUFFLE_ON_OFF = 0x03,
    SCAN_ON_OFF = 0x04,
});

open_integer!(ApplicationSettingValue, u8 {
    OFF = 0x01,
    ON_OR_SINGLE_TRACK = 0x02,
    ALL_TRACKS = 0x03,
    GROUP = 0x04,
    EQUALIZER_OFF = 0x01,
    EQUALIZER_ON = 0x02,
    REPEAT_OFF = 0x01,
    SINGLE_TRACK_REPEAT = 0x02,
    ALL_TRACK_REPEAT = 0x03,
    GROUP_REPEAT = 0x04,
    SHUFFLE_OFF = 0x01,
    ALL_TRACKS_SHUFFLE = 0x02,
    GROUP_SHUFFLE = 0x03,
    SCAN_OFF = 0x01,
    ALL_TRACKS_SCAN = 0x02,
    GROUP_SCAN = 0x03,
});

open_integer!(CharacterSetId, u16 {
    UTF_8 = 0x006A,
});

open_integer!(MediaAttributeId, u32 {
    TITLE = 0x01,
    ARTIST_NAME = 0x02,
    ALBUM_NAME = 0x03,
    TRACK_NUMBER = 0x04,
    TOTAL_NUMBER_OF_TRACKS = 0x05,
    GENRE = 0x06,
    PLAYING_TIME = 0x07,
    DEFAULT_COVER_ART = 0x08,
});

open_integer!(BatteryStatus, u8 {
    NORMAL = 0x00,
    WARNING = 0x01,
    CRITICAL = 0x02,
    EXTERNAL = 0x03,
    FULL_CHARGE = 0x04,
});

open_integer!(EventId, u8 {
    PLAYBACK_STATUS_CHANGED = 0x01,
    TRACK_CHANGED = 0x02,
    TRACK_REACHED_END = 0x03,
    TRACK_REACHED_START = 0x04,
    PLAYBACK_POS_CHANGED = 0x05,
    BATT_STATUS_CHANGED = 0x06,
    SYSTEM_STATUS_CHANGED = 0x07,
    PLAYER_APPLICATION_SETTING_CHANGED = 0x08,
    NOW_PLAYING_CONTENT_CHANGED = 0x09,
    AVAILABLE_PLAYERS_CHANGED = 0x0A,
    ADDRESSED_PLAYER_CHANGED = 0x0B,
    UIDS_CHANGED = 0x0C,
    VOLUME_CHANGED = 0x0D,
});

open_integer!(Scope, u8 {
    MEDIA_PLAYER_LIST = 0x00,
    MEDIA_PLAYER_VIRTUAL_FILESYSTEM = 0x01,
    SEARCH = 0x02,
    NOW_PLAYING = 0x03,
});

open_integer!(Direction, u8 {
    UP = 0,
    DOWN = 1,
});

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlayerApplicationSetting {
    pub attribute: ApplicationSettingAttributeId,
    pub value: ApplicationSettingValue,
}

/// Every typed command registered by upstream `bumble.avrcp.Command`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    GetCapabilities {
        capability_id: CapabilityId,
    },
    ListPlayerApplicationSettingAttributes,
    ListPlayerApplicationSettingValues {
        attribute: ApplicationSettingAttributeId,
    },
    GetCurrentPlayerApplicationSettingValue {
        attributes: Vec<ApplicationSettingAttributeId>,
    },
    SetPlayerApplicationSettingValue {
        settings: Vec<PlayerApplicationSetting>,
    },
    GetPlayerApplicationSettingAttributeText {
        attributes: Vec<ApplicationSettingAttributeId>,
    },
    GetPlayerApplicationSettingValueText {
        attribute: ApplicationSettingAttributeId,
        values: Vec<ApplicationSettingValue>,
    },
    InformDisplayableCharacterSet {
        character_set_ids: Vec<CharacterSetId>,
    },
    InformBatteryStatusOfCt {
        battery_status: BatteryStatus,
    },
    GetElementAttributes {
        identifier: u64,
        attribute_ids: Vec<MediaAttributeId>,
    },
    GetPlayStatus,
    RegisterNotification {
        event_id: EventId,
        playback_interval: u32,
    },
    SetAbsoluteVolume {
        volume: u8,
    },
    SetAddressedPlayer {
        player_id: u16,
    },
    SetBrowsedPlayer {
        player_id: u16,
    },
    GetFolderItems {
        scope: Scope,
        start_item: u32,
        end_item: u32,
        attributes: Vec<MediaAttributeId>,
    },
    ChangePath {
        uid_counter: u16,
        direction: Direction,
        folder_uid: u64,
    },
    GetItemAttributes {
        scope: Scope,
        uid: u64,
        uid_counter: u16,
        attributes: Vec<MediaAttributeId>,
    },
    PlayItem {
        scope: Scope,
        uid: u64,
        uid_counter: u16,
    },
    GetTotalNumberOfItems {
        scope: Scope,
    },
    Search {
        character_set_id: CharacterSetId,
        search_string: String,
    },
    AddToNowPlaying {
        scope: Scope,
        uid: u64,
        uid_counter: u16,
    },
    Unknown {
        pdu_id: PduId,
        parameters: Vec<u8>,
    },
}

impl Command {
    pub fn pdu_id(&self) -> PduId {
        match self {
            Self::GetCapabilities { .. } => PduId::GET_CAPABILITIES,
            Self::ListPlayerApplicationSettingAttributes => {
                PduId::LIST_PLAYER_APPLICATION_SETTING_ATTRIBUTES
            }
            Self::ListPlayerApplicationSettingValues { .. } => {
                PduId::LIST_PLAYER_APPLICATION_SETTING_VALUES
            }
            Self::GetCurrentPlayerApplicationSettingValue { .. } => {
                PduId::GET_CURRENT_PLAYER_APPLICATION_SETTING_VALUE
            }
            Self::SetPlayerApplicationSettingValue { .. } => {
                PduId::SET_PLAYER_APPLICATION_SETTING_VALUE
            }
            Self::GetPlayerApplicationSettingAttributeText { .. } => {
                PduId::GET_PLAYER_APPLICATION_SETTING_ATTRIBUTE_TEXT
            }
            Self::GetPlayerApplicationSettingValueText { .. } => {
                PduId::GET_PLAYER_APPLICATION_SETTING_VALUE_TEXT
            }
            Self::InformDisplayableCharacterSet { .. } => PduId::INFORM_DISPLAYABLE_CHARACTER_SET,
            Self::InformBatteryStatusOfCt { .. } => PduId::INFORM_BATTERY_STATUS_OF_CT,
            Self::GetElementAttributes { .. } => PduId::GET_ELEMENT_ATTRIBUTES,
            Self::GetPlayStatus => PduId::GET_PLAY_STATUS,
            Self::RegisterNotification { .. } => PduId::REGISTER_NOTIFICATION,
            Self::SetAbsoluteVolume { .. } => PduId::SET_ABSOLUTE_VOLUME,
            Self::SetAddressedPlayer { .. } => PduId::SET_ADDRESSED_PLAYER,
            Self::SetBrowsedPlayer { .. } => PduId::SET_BROWSED_PLAYER,
            Self::GetFolderItems { .. } => PduId::GET_FOLDER_ITEMS,
            Self::ChangePath { .. } => PduId::CHANGE_PATH,
            Self::GetItemAttributes { .. } => PduId::GET_ITEM_ATTRIBUTES,
            Self::PlayItem { .. } => PduId::PLAY_ITEM,
            Self::GetTotalNumberOfItems { .. } => PduId::GET_TOTAL_NUMBER_OF_ITEMS,
            Self::Search { .. } => PduId::SEARCH,
            Self::AddToNowPlaying { .. } => PduId::ADD_TO_NOW_PLAYING,
            Self::Unknown { pdu_id, .. } => *pdu_id,
        }
    }

    pub fn to_parameters(&self) -> Result<Vec<u8>> {
        let mut bytes = Vec::new();
        match self {
            Self::GetCapabilities { capability_id } => bytes.push(capability_id.0),
            Self::ListPlayerApplicationSettingAttributes | Self::GetPlayStatus => {}
            Self::ListPlayerApplicationSettingValues { attribute } => bytes.push(attribute.0),
            Self::GetCurrentPlayerApplicationSettingValue { attributes }
            | Self::GetPlayerApplicationSettingAttributeText { attributes } => {
                push_count(&mut bytes, attributes.len())?;
                bytes.extend(attributes.iter().map(|attribute| attribute.0));
            }
            Self::SetPlayerApplicationSettingValue { settings } => {
                push_count(&mut bytes, settings.len())?;
                for setting in settings {
                    bytes.extend_from_slice(&[setting.attribute.0, setting.value.0]);
                }
            }
            Self::GetPlayerApplicationSettingValueText { attribute, values } => {
                bytes.push(attribute.0);
                push_count(&mut bytes, values.len())?;
                bytes.extend(values.iter().map(|value| value.0));
            }
            Self::InformDisplayableCharacterSet { character_set_ids } => {
                push_count(&mut bytes, character_set_ids.len())?;
                for character_set_id in character_set_ids {
                    bytes.extend_from_slice(&character_set_id.0.to_be_bytes());
                }
            }
            Self::InformBatteryStatusOfCt { battery_status } => bytes.push(battery_status.0),
            Self::GetElementAttributes {
                identifier,
                attribute_ids,
            } => {
                bytes.extend_from_slice(&identifier.to_be_bytes());
                push_u32_list(&mut bytes, attribute_ids, true)?;
            }
            Self::RegisterNotification {
                event_id,
                playback_interval,
            } => {
                bytes.push(event_id.0);
                bytes.extend_from_slice(&playback_interval.to_be_bytes());
            }
            Self::SetAbsoluteVolume { volume } => bytes.push(*volume),
            Self::SetAddressedPlayer { player_id } | Self::SetBrowsedPlayer { player_id } => {
                bytes.extend_from_slice(&player_id.to_be_bytes());
            }
            Self::GetFolderItems {
                scope,
                start_item,
                end_item,
                attributes,
            } => {
                bytes.push(scope.0);
                bytes.extend_from_slice(&start_item.to_be_bytes());
                bytes.extend_from_slice(&end_item.to_be_bytes());
                push_u32_list(&mut bytes, attributes, true)?;
            }
            Self::ChangePath {
                uid_counter,
                direction,
                folder_uid,
            } => {
                bytes.extend_from_slice(&uid_counter.to_be_bytes());
                bytes.push(direction.0);
                bytes.extend_from_slice(&folder_uid.to_be_bytes());
            }
            Self::GetItemAttributes {
                scope,
                uid,
                uid_counter,
                attributes,
            } => {
                bytes.push(scope.0);
                bytes.extend_from_slice(&uid.to_be_bytes());
                bytes.extend_from_slice(&uid_counter.to_be_bytes());
                // This intentionally matches upstream Bumble's field metadata,
                // whose GetItemAttributes list uses the default little endian.
                push_u32_list(&mut bytes, attributes, false)?;
            }
            Self::PlayItem {
                scope,
                uid,
                uid_counter,
            }
            | Self::AddToNowPlaying {
                scope,
                uid,
                uid_counter,
            } => {
                bytes.push(scope.0);
                bytes.extend_from_slice(&uid.to_be_bytes());
                bytes.extend_from_slice(&uid_counter.to_be_bytes());
            }
            Self::GetTotalNumberOfItems { scope } => bytes.push(scope.0),
            Self::Search {
                character_set_id,
                search_string,
            } => {
                bytes.extend_from_slice(&character_set_id.0.to_be_bytes());
                let encoded = search_string.as_bytes();
                let length = u16::try_from(encoded.len())
                    .map_err(|_| Error::InvalidField("AVRCP search string"))?;
                bytes.extend_from_slice(&length.to_be_bytes());
                bytes.extend_from_slice(encoded);
            }
            Self::Unknown { parameters, .. } => bytes.extend_from_slice(parameters),
        }
        Ok(bytes)
    }

    pub fn to_vendor_pdu(&self) -> Result<VendorPdu> {
        Ok(VendorPdu {
            pdu_id: self.pdu_id(),
            packet_type: PacketType::Single,
            parameters: self.to_parameters()?,
        })
    }

    pub fn from_vendor_pdu(pdu: &VendorPdu) -> Result<Self> {
        Self::from_parameters(pdu.pdu_id, &pdu.parameters)
    }

    pub fn from_parameters(pdu_id: PduId, parameters: &[u8]) -> Result<Self> {
        let mut cursor = Cursor::new(parameters);
        let command = match pdu_id {
            PduId::GET_CAPABILITIES => Self::GetCapabilities {
                capability_id: CapabilityId(cursor.u8()?),
            },
            PduId::LIST_PLAYER_APPLICATION_SETTING_ATTRIBUTES => {
                Self::ListPlayerApplicationSettingAttributes
            }
            PduId::LIST_PLAYER_APPLICATION_SETTING_VALUES => {
                Self::ListPlayerApplicationSettingValues {
                    attribute: ApplicationSettingAttributeId(cursor.u8()?),
                }
            }
            PduId::GET_CURRENT_PLAYER_APPLICATION_SETTING_VALUE => {
                Self::GetCurrentPlayerApplicationSettingValue {
                    attributes: cursor
                        .counted_u8s()?
                        .into_iter()
                        .map(ApplicationSettingAttributeId)
                        .collect(),
                }
            }
            PduId::SET_PLAYER_APPLICATION_SETTING_VALUE => {
                let count = usize::from(cursor.u8()?);
                let mut settings = Vec::with_capacity(count);
                for _ in 0..count {
                    settings.push(PlayerApplicationSetting {
                        attribute: ApplicationSettingAttributeId(cursor.u8()?),
                        value: ApplicationSettingValue(cursor.u8()?),
                    });
                }
                Self::SetPlayerApplicationSettingValue { settings }
            }
            PduId::GET_PLAYER_APPLICATION_SETTING_ATTRIBUTE_TEXT => {
                Self::GetPlayerApplicationSettingAttributeText {
                    attributes: cursor
                        .counted_u8s()?
                        .into_iter()
                        .map(ApplicationSettingAttributeId)
                        .collect(),
                }
            }
            PduId::GET_PLAYER_APPLICATION_SETTING_VALUE_TEXT => {
                Self::GetPlayerApplicationSettingValueText {
                    attribute: ApplicationSettingAttributeId(cursor.u8()?),
                    values: cursor
                        .counted_u8s()?
                        .into_iter()
                        .map(ApplicationSettingValue)
                        .collect(),
                }
            }
            PduId::INFORM_DISPLAYABLE_CHARACTER_SET => {
                let count = usize::from(cursor.u8()?);
                let mut character_set_ids = Vec::with_capacity(count);
                for _ in 0..count {
                    character_set_ids.push(CharacterSetId(cursor.u16_be()?));
                }
                Self::InformDisplayableCharacterSet { character_set_ids }
            }
            PduId::INFORM_BATTERY_STATUS_OF_CT => Self::InformBatteryStatusOfCt {
                battery_status: BatteryStatus(cursor.u8()?),
            },
            PduId::GET_ELEMENT_ATTRIBUTES => Self::GetElementAttributes {
                identifier: cursor.u64_be()?,
                attribute_ids: cursor.counted_u32s(true)?,
            },
            PduId::GET_PLAY_STATUS => Self::GetPlayStatus,
            PduId::REGISTER_NOTIFICATION => Self::RegisterNotification {
                event_id: EventId(cursor.u8()?),
                playback_interval: cursor.u32_be()?,
            },
            PduId::SET_ABSOLUTE_VOLUME => Self::SetAbsoluteVolume {
                volume: cursor.u8()?,
            },
            PduId::SET_ADDRESSED_PLAYER => Self::SetAddressedPlayer {
                player_id: cursor.u16_be()?,
            },
            PduId::SET_BROWSED_PLAYER => Self::SetBrowsedPlayer {
                player_id: cursor.u16_be()?,
            },
            PduId::GET_FOLDER_ITEMS => Self::GetFolderItems {
                scope: Scope(cursor.u8()?),
                start_item: cursor.u32_be()?,
                end_item: cursor.u32_be()?,
                attributes: cursor.counted_u32s(true)?,
            },
            PduId::CHANGE_PATH => Self::ChangePath {
                uid_counter: cursor.u16_be()?,
                direction: Direction(cursor.u8()?),
                folder_uid: cursor.u64_be()?,
            },
            PduId::GET_ITEM_ATTRIBUTES => Self::GetItemAttributes {
                scope: Scope(cursor.u8()?),
                uid: cursor.u64_be()?,
                uid_counter: cursor.u16_be()?,
                attributes: cursor.counted_u32s(false)?,
            },
            PduId::PLAY_ITEM => Self::PlayItem {
                scope: Scope(cursor.u8()?),
                uid: cursor.u64_be()?,
                uid_counter: cursor.u16_be()?,
            },
            PduId::GET_TOTAL_NUMBER_OF_ITEMS => Self::GetTotalNumberOfItems {
                scope: Scope(cursor.u8()?),
            },
            PduId::SEARCH => Self::Search {
                character_set_id: CharacterSetId(cursor.u16_be()?),
                search_string: cursor.string_u16()?,
            },
            PduId::ADD_TO_NOW_PLAYING => Self::AddToNowPlaying {
                scope: Scope(cursor.u8()?),
                uid: cursor.u64_be()?,
                uid_counter: cursor.u16_be()?,
            },
            _ => {
                return Ok(Self::Unknown {
                    pdu_id,
                    parameters: parameters.to_vec(),
                });
            }
        };
        cursor.finish()?;
        Ok(command)
    }
}

fn push_count(bytes: &mut Vec<u8>, count: usize) -> Result<()> {
    bytes.push(u8::try_from(count).map_err(|_| Error::InvalidField("AVRCP list count"))?);
    Ok(())
}

fn push_u32_list(bytes: &mut Vec<u8>, values: &[MediaAttributeId], big_endian: bool) -> Result<()> {
    push_count(bytes, values.len())?;
    for value in values {
        let encoded = if big_endian {
            value.0.to_be_bytes()
        } else {
            value.0.to_le_bytes()
        };
        bytes.extend_from_slice(&encoded);
    }
    Ok(())
}

struct Cursor<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Cursor<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn take(&mut self, count: usize) -> Result<&'a [u8]> {
        let end = self
            .offset
            .checked_add(count)
            .ok_or(Error::InvalidField("AVRCP parameter offset"))?;
        let value = self
            .bytes
            .get(self.offset..end)
            .ok_or(Error::Truncated("AVRCP command parameters"))?;
        self.offset = end;
        Ok(value)
    }

    fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }

    fn u16_be(&mut self) -> Result<u16> {
        Ok(u16::from_be_bytes(self.take(2)?.try_into().unwrap()))
    }

    fn u32_be(&mut self) -> Result<u32> {
        Ok(u32::from_be_bytes(self.take(4)?.try_into().unwrap()))
    }

    fn u64_be(&mut self) -> Result<u64> {
        Ok(u64::from_be_bytes(self.take(8)?.try_into().unwrap()))
    }

    fn counted_u8s(&mut self) -> Result<Vec<u8>> {
        let count = usize::from(self.u8()?);
        Ok(self.take(count)?.to_vec())
    }

    fn counted_u32s(&mut self, big_endian: bool) -> Result<Vec<MediaAttributeId>> {
        let count = usize::from(self.u8()?);
        let mut values = Vec::with_capacity(count);
        for _ in 0..count {
            let bytes: [u8; 4] = self.take(4)?.try_into().unwrap();
            values.push(MediaAttributeId(if big_endian {
                u32::from_be_bytes(bytes)
            } else {
                u32::from_le_bytes(bytes)
            }));
        }
        Ok(values)
    }

    fn string_u16(&mut self) -> Result<String> {
        let length = usize::from(self.u16_be()?);
        String::from_utf8(self.take(length)?.to_vec())
            .map_err(|_| Error::InvalidField("AVRCP UTF-8 string"))
    }

    fn finish(self) -> Result<()> {
        if self.offset == self.bytes.len() {
            Ok(())
        } else {
            Err(Error::TrailingBytes(self.bytes.len() - self.offset))
        }
    }
}
