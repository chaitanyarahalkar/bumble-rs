use crate::{
    ApplicationSettingAttributeId, ApplicationSettingValue, CapabilityId, CharacterSetId, Error,
    Event, EventId, MediaAttributeId, PacketType, PduId, PlayStatus, PlayerApplicationSetting,
    Result, VendorPdu,
};

macro_rules! open_integer {
    ($name:ident, $type:ty { $($constant:ident = $value:expr),+ $(,)? }) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub struct $name(pub $type);
        impl $name { $(pub const $constant: Self = Self($value);)+ }
    };
}

open_integer!(StatusCode, u8 {
    INVALID_COMMAND = 0x00,
    INVALID_PARAMETER = 0x01,
    PARAMETER_CONTENT_ERROR = 0x02,
    INTERNAL_ERROR = 0x03,
    OPERATION_COMPLETED = 0x04,
    UID_CHANGED = 0x05,
    INVALID_DIRECTION = 0x07,
    NOT_A_DIRECTORY = 0x08,
    DOES_NOT_EXIST = 0x09,
    INVALID_SCOPE = 0x0A,
    RANGE_OUT_OF_BOUNDS = 0x0B,
    FOLDER_ITEM_IS_NOT_PLAYABLE = 0x0C,
    MEDIA_IN_USE = 0x0D,
    NOW_PLAYING_LIST_FULL = 0x0E,
    SEARCH_NOT_SUPPORTED = 0x0F,
    SEARCH_IN_PROGRESS = 0x10,
    INVALID_PLAYER_ID = 0x11,
    PLAYER_NOT_BROWSABLE = 0x12,
    PLAYER_NOT_ADDRESSED = 0x13,
    NO_VALID_SEARCH_RESULTS = 0x14,
    NO_AVAILABLE_PLAYERS = 0x15,
    ADDRESSED_PLAYER_CHANGED = 0x16,
});

open_integer!(BrowseableItemType, u8 {
    MEDIA_PLAYER = 0x01,
    FOLDER = 0x02,
    MEDIA_ELEMENT = 0x03,
});

open_integer!(MajorPlayerType, u8 {
    AUDIO = 0x01,
    VIDEO = 0x02,
    BROADCASTING_AUDIO = 0x04,
    BROADCASTING_VIDEO = 0x08,
});

open_integer!(PlayerSubType, u32 {
    AUDIO_BOOK = 0x01,
    PODCAST = 0x02,
});

open_integer!(PlayerFeatures, u128 {
    SELECT = 1 << 0,
    UP = 1 << 1,
    DOWN = 1 << 2,
    LEFT = 1 << 3,
    RIGHT = 1 << 4,
    RIGHT_UP = 1 << 5,
    RIGHT_DOWN = 1 << 6,
    LEFT_UP = 1 << 7,
    LEFT_DOWN = 1 << 8,
    ROOT_MENU = 1 << 9,
    SETUP_MENU = 1 << 10,
    CONTENTS_MENU = 1 << 11,
    FAVORITE_MENU = 1 << 12,
    EXIT = 1 << 13,
    NUM_0 = 1 << 14,
    NUM_1 = 1 << 15,
    NUM_2 = 1 << 16,
    NUM_3 = 1 << 17,
    NUM_4 = 1 << 18,
    NUM_5 = 1 << 19,
    NUM_6 = 1 << 20,
    NUM_7 = 1 << 21,
    NUM_8 = 1 << 22,
    NUM_9 = 1 << 23,
    DOT = 1 << 24,
    ENTER = 1 << 25,
    CLEAR = 1 << 26,
    CHANNEL_UP = 1 << 27,
    CHANNEL_DOWN = 1 << 28,
    PREVIOUS_CHANNEL = 1 << 29,
    SOUND_SELECT = 1 << 30,
    INPUT_SELECT = 1 << 31,
    DISPLAY_INFORMATION = 1 << 32,
    HELP = 1 << 33,
    PAGE_UP = 1 << 34,
    PAGE_DOWN = 1 << 35,
    POWER = 1 << 36,
    VOLUME_UP = 1 << 37,
    VOLUME_DOWN = 1 << 38,
    MUTE = 1 << 39,
    PLAY = 1 << 40,
    STOP = 1 << 41,
    PAUSE = 1 << 42,
    RECORD = 1 << 43,
    REWIND = 1 << 44,
    FAST_FORWARD = 1 << 45,
    EJECT = 1 << 46,
    FORWARD = 1 << 47,
    BACKWARD = 1 << 48,
    ANGLE = 1 << 49,
    SUBPICTURE = 1 << 50,
    F1 = 1 << 51,
    F2 = 1 << 52,
    F3 = 1 << 53,
    F4 = 1 << 54,
    F5 = 1 << 55,
    VENDOR_UNIQUE = 1 << 56,
    BASIC_GROUP_NAVIGATION = 1 << 57,
    ADVANCED_CONTROL_PLAYER = 1 << 58,
    BROWSING = 1 << 59,
    SEARCHING = 1 << 60,
    ADD_TO_NOW_PLAYING = 1 << 61,
    UIDS_UNIQUE_IN_PLAYER_BROWSE_TREE = 1 << 62,
    UI_DS_UNIQUE_IN_PLAYER_BROWSE_TREE = 1 << 62,
    ONLY_BROWSABLE_WHEN_ADDRESSED = 1 << 63,
    ONLY_SEARCHABLE_WHEN_ADDRESSED = 1 << 64,
    NOW_PLAYING = 1 << 65,
    UID_PERSISTENCY = 1 << 66,
    NUMBER_OF_ITEMS = 1 << 67,
    COVER_ART = 1 << 68,
});

open_integer!(FolderType, u8 {
    MIXED = 0x00,
    TITLES = 0x01,
    ALBUMS = 0x02,
    ARTISTS = 0x03,
    GENRES = 0x04,
    PLAYLISTS = 0x05,
    YEARS = 0x06,
});

open_integer!(Playable, u8 {
    NOT_PLAYABLE = 0x00,
    PLAYABLE = 0x01,
});

open_integer!(MediaType, u8 {
    AUDIO = 0x00,
    VIDEO = 0x01,
});

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Capability {
    CompanyId(u32),
    Event(EventId),
    Raw(Vec<u8>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MediaAttribute {
    pub attribute_id: MediaAttributeId,
    pub character_set_id: CharacterSetId,
    pub value: String,
}

pub type AttributeValueEntry = MediaAttribute;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AttributeText {
    pub attribute: ApplicationSettingAttributeId,
    pub character_set_id: CharacterSetId,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ValueText {
    pub value: ApplicationSettingValue,
    pub character_set_id: CharacterSetId,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BrowseableItem {
    MediaPlayer {
        player_id: u16,
        major_player_type: MajorPlayerType,
        player_sub_type: PlayerSubType,
        play_status: PlayStatus,
        feature_bitmask: PlayerFeatures,
        character_set_id: CharacterSetId,
        displayable_name: String,
    },
    Folder {
        folder_uid: u64,
        folder_type: FolderType,
        is_playable: Playable,
        character_set_id: CharacterSetId,
        displayable_name: String,
    },
    MediaElement {
        media_element_uid: u64,
        media_type: MediaType,
        character_set_id: CharacterSetId,
        displayable_name: String,
        attributes: Vec<AttributeValueEntry>,
    },
    Unknown {
        item_type: BrowseableItemType,
        data: Vec<u8>,
    },
}

impl BrowseableItem {
    pub fn item_type(&self) -> BrowseableItemType {
        match self {
            Self::MediaPlayer { .. } => BrowseableItemType::MEDIA_PLAYER,
            Self::Folder { .. } => BrowseableItemType::FOLDER,
            Self::MediaElement { .. } => BrowseableItemType::MEDIA_ELEMENT,
            Self::Unknown { item_type, .. } => *item_type,
        }
    }

    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        let mut payload = Vec::new();
        match self {
            Self::MediaPlayer {
                player_id,
                major_player_type,
                player_sub_type,
                play_status,
                feature_bitmask,
                character_set_id,
                displayable_name,
            } => {
                payload.extend_from_slice(&player_id.to_be_bytes());
                payload.push(major_player_type.0);
                payload.extend_from_slice(&player_sub_type.0.to_le_bytes());
                payload.push(play_status.0);
                payload.extend_from_slice(&feature_bitmask.0.to_le_bytes());
                payload.extend_from_slice(&character_set_id.0.to_be_bytes());
                push_string_u16(&mut payload, displayable_name)?;
            }
            Self::Folder {
                folder_uid,
                folder_type,
                is_playable,
                character_set_id,
                displayable_name,
            } => {
                payload.extend_from_slice(&folder_uid.to_be_bytes());
                payload.extend_from_slice(&[folder_type.0, is_playable.0]);
                payload.extend_from_slice(&character_set_id.0.to_be_bytes());
                push_string_u16(&mut payload, displayable_name)?;
            }
            Self::MediaElement {
                media_element_uid,
                media_type,
                character_set_id,
                displayable_name,
                attributes,
            } => {
                payload.extend_from_slice(&media_element_uid.to_be_bytes());
                payload.push(media_type.0);
                payload.extend_from_slice(&character_set_id.0.to_be_bytes());
                push_string_u16(&mut payload, displayable_name)?;
                push_count_u8(
                    &mut payload,
                    attributes.len(),
                    "AVRCP media attribute count",
                )?;
                for attribute in attributes {
                    encode_media_attribute(&mut payload, attribute)?;
                }
            }
            Self::Unknown { data, .. } => payload.extend_from_slice(data),
        }
        let length = u16::try_from(payload.len())
            .map_err(|_| Error::InvalidField("AVRCP browseable item length"))?;
        let mut bytes = vec![self.item_type().0];
        bytes.extend_from_slice(&length.to_be_bytes());
        bytes.extend_from_slice(&payload);
        Ok(bytes)
    }

    fn parse(reader: &mut Reader<'_>) -> Result<Self> {
        let item_type = BrowseableItemType(reader.u8()?);
        let length = usize::from(reader.u16_be()?);
        let mut payload = Reader::new(reader.take(length)?);
        let item = match item_type {
            BrowseableItemType::MEDIA_PLAYER => Self::MediaPlayer {
                player_id: payload.u16_be()?,
                major_player_type: MajorPlayerType(payload.u8()?),
                player_sub_type: PlayerSubType(payload.u32_le()?),
                play_status: PlayStatus(payload.u8()?),
                feature_bitmask: PlayerFeatures(payload.u128_le()?),
                character_set_id: CharacterSetId(payload.u16_be()?),
                displayable_name: payload.string_u16()?,
            },
            BrowseableItemType::FOLDER => Self::Folder {
                folder_uid: payload.u64_be()?,
                folder_type: FolderType(payload.u8()?),
                is_playable: Playable(payload.u8()?),
                character_set_id: CharacterSetId(payload.u16_be()?),
                displayable_name: payload.string_u16()?,
            },
            BrowseableItemType::MEDIA_ELEMENT => {
                let media_element_uid = payload.u64_be()?;
                let media_type = MediaType(payload.u8()?);
                let character_set_id = CharacterSetId(payload.u16_be()?);
                let displayable_name = payload.string_u16()?;
                let count = usize::from(payload.u8()?);
                let mut attributes = Vec::with_capacity(count);
                for _ in 0..count {
                    attributes.push(parse_media_attribute(&mut payload)?);
                }
                Self::MediaElement {
                    media_element_uid,
                    media_type,
                    character_set_id,
                    displayable_name,
                    attributes,
                }
            }
            _ => {
                let data = payload.rest().to_vec();
                payload.consume_rest();
                Self::Unknown { item_type, data }
            }
        };
        payload.finish()?;
        Ok(item)
    }
}

/// Every implemented response class registered by upstream Bumble, plus its
/// AV/C rejection/not-implemented fallbacks.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Response {
    Rejected {
        pdu_id: PduId,
        status: StatusCode,
    },
    NotImplemented {
        pdu_id: PduId,
        parameters: Vec<u8>,
    },
    GetCapabilities {
        capability_id: CapabilityId,
        capabilities: Vec<Capability>,
    },
    ListPlayerApplicationSettingAttributes {
        attributes: Vec<ApplicationSettingAttributeId>,
    },
    ListPlayerApplicationSettingValues {
        values: Vec<ApplicationSettingValue>,
    },
    GetCurrentPlayerApplicationSettingValue {
        settings: Vec<PlayerApplicationSetting>,
    },
    SetPlayerApplicationSettingValue,
    GetPlayerApplicationSettingAttributeText {
        entries: Vec<AttributeText>,
    },
    GetPlayerApplicationSettingValueText {
        entries: Vec<ValueText>,
    },
    InformDisplayableCharacterSet,
    InformBatteryStatusOfCt,
    GetPlayStatus {
        song_length: u32,
        song_position: u32,
        play_status: PlayStatus,
    },
    GetElementAttributes {
        attributes: Vec<MediaAttribute>,
    },
    SetAbsoluteVolume {
        volume: u8,
    },
    RegisterNotification {
        event: Event,
    },
    SetAddressedPlayer {
        status: StatusCode,
    },
    SetBrowsedPlayer {
        status: StatusCode,
        uid_counter: u16,
        number_of_items: u32,
        character_set_id: CharacterSetId,
        folder_names: Vec<String>,
    },
    GetFolderItems {
        status: StatusCode,
        uid_counter: u16,
        items: Vec<BrowseableItem>,
    },
    ChangePath {
        status: StatusCode,
        number_of_items: u32,
    },
    GetItemAttributes {
        status: StatusCode,
        attributes: Vec<AttributeValueEntry>,
    },
    GetTotalNumberOfItems {
        status: StatusCode,
        uid_counter: u16,
        number_of_items: u32,
    },
    Search {
        status: StatusCode,
        uid_counter: u16,
        number_of_items: u32,
    },
    PlayItem {
        status: StatusCode,
    },
    AddToNowPlaying {
        status: StatusCode,
    },
    Unknown {
        pdu_id: PduId,
        parameters: Vec<u8>,
    },
}

impl Response {
    pub const UNAVAILABLE: u32 = u32::MAX;

    pub fn pdu_id(&self) -> PduId {
        match self {
            Self::Rejected { pdu_id, .. }
            | Self::NotImplemented { pdu_id, .. }
            | Self::Unknown { pdu_id, .. } => *pdu_id,
            Self::GetCapabilities { .. } => PduId::GET_CAPABILITIES,
            Self::ListPlayerApplicationSettingAttributes { .. } => {
                PduId::LIST_PLAYER_APPLICATION_SETTING_ATTRIBUTES
            }
            Self::ListPlayerApplicationSettingValues { .. } => {
                PduId::LIST_PLAYER_APPLICATION_SETTING_VALUES
            }
            Self::GetCurrentPlayerApplicationSettingValue { .. } => {
                PduId::GET_CURRENT_PLAYER_APPLICATION_SETTING_VALUE
            }
            Self::SetPlayerApplicationSettingValue => PduId::SET_PLAYER_APPLICATION_SETTING_VALUE,
            Self::GetPlayerApplicationSettingAttributeText { .. } => {
                PduId::GET_PLAYER_APPLICATION_SETTING_ATTRIBUTE_TEXT
            }
            Self::GetPlayerApplicationSettingValueText { .. } => {
                PduId::GET_PLAYER_APPLICATION_SETTING_VALUE_TEXT
            }
            Self::InformDisplayableCharacterSet => PduId::INFORM_DISPLAYABLE_CHARACTER_SET,
            Self::InformBatteryStatusOfCt => PduId::INFORM_BATTERY_STATUS_OF_CT,
            Self::GetPlayStatus { .. } => PduId::GET_PLAY_STATUS,
            Self::GetElementAttributes { .. } => PduId::GET_ELEMENT_ATTRIBUTES,
            Self::SetAbsoluteVolume { .. } => PduId::SET_ABSOLUTE_VOLUME,
            Self::RegisterNotification { .. } => PduId::REGISTER_NOTIFICATION,
            Self::SetAddressedPlayer { .. } => PduId::SET_ADDRESSED_PLAYER,
            Self::SetBrowsedPlayer { .. } => PduId::SET_BROWSED_PLAYER,
            Self::GetFolderItems { .. } => PduId::GET_FOLDER_ITEMS,
            Self::ChangePath { .. } => PduId::CHANGE_PATH,
            Self::GetItemAttributes { .. } => PduId::GET_ITEM_ATTRIBUTES,
            Self::GetTotalNumberOfItems { .. } => PduId::GET_TOTAL_NUMBER_OF_ITEMS,
            Self::Search { .. } => PduId::SEARCH,
            Self::PlayItem { .. } => PduId::PLAY_ITEM,
            Self::AddToNowPlaying { .. } => PduId::ADD_TO_NOW_PLAYING,
        }
    }

    pub fn to_parameters(&self) -> Result<Vec<u8>> {
        let mut bytes = Vec::new();
        match self {
            Self::Rejected { status, .. } => bytes.push(status.0),
            Self::NotImplemented { parameters, .. } | Self::Unknown { parameters, .. } => {
                bytes.extend_from_slice(parameters)
            }
            Self::GetCapabilities {
                capability_id,
                capabilities,
            } => {
                bytes.push(capability_id.0);
                push_count_u8(&mut bytes, capabilities.len(), "AVRCP capability count")?;
                for capability in capabilities {
                    match capability {
                        Capability::CompanyId(company_id) => {
                            if *company_id > 0xFF_FFFF {
                                return Err(Error::InvalidField("AVRCP company ID"));
                            }
                            bytes.extend_from_slice(&company_id.to_be_bytes()[1..]);
                        }
                        Capability::Event(event_id) => bytes.push(event_id.0),
                        Capability::Raw(raw) => bytes.extend_from_slice(raw),
                    }
                }
            }
            Self::ListPlayerApplicationSettingAttributes { attributes } => {
                push_count_u8(&mut bytes, attributes.len(), "AVRCP attribute count")?;
                bytes.extend(attributes.iter().map(|value| value.0));
            }
            Self::ListPlayerApplicationSettingValues { values } => {
                push_count_u8(&mut bytes, values.len(), "AVRCP value count")?;
                bytes.extend(values.iter().map(|value| value.0));
            }
            Self::GetCurrentPlayerApplicationSettingValue { settings } => {
                push_count_u8(&mut bytes, settings.len(), "AVRCP setting count")?;
                for setting in settings {
                    bytes.extend_from_slice(&[setting.attribute.0, setting.value.0]);
                }
            }
            Self::SetPlayerApplicationSettingValue
            | Self::InformDisplayableCharacterSet
            | Self::InformBatteryStatusOfCt => {}
            Self::GetPlayerApplicationSettingAttributeText { entries } => {
                push_count_u8(&mut bytes, entries.len(), "AVRCP attribute text count")?;
                for entry in entries {
                    bytes.push(entry.attribute.0);
                    bytes.extend_from_slice(&entry.character_set_id.0.to_be_bytes());
                    push_string_u8(&mut bytes, &entry.text)?;
                }
            }
            Self::GetPlayerApplicationSettingValueText { entries } => {
                push_count_u8(&mut bytes, entries.len(), "AVRCP value text count")?;
                for entry in entries {
                    bytes.push(entry.value.0);
                    bytes.extend_from_slice(&entry.character_set_id.0.to_be_bytes());
                    push_string_u8(&mut bytes, &entry.text)?;
                }
            }
            Self::GetPlayStatus {
                song_length,
                song_position,
                play_status,
            } => {
                bytes.extend_from_slice(&song_length.to_be_bytes());
                bytes.extend_from_slice(&song_position.to_be_bytes());
                bytes.push(play_status.0);
            }
            Self::GetElementAttributes { attributes } => {
                push_count_u8(&mut bytes, attributes.len(), "AVRCP media attribute count")?;
                for attribute in attributes {
                    encode_media_attribute(&mut bytes, attribute)?;
                }
            }
            Self::SetAbsoluteVolume { volume } => bytes.push(*volume),
            Self::RegisterNotification { event } => bytes.extend_from_slice(&event.to_bytes()?),
            Self::SetAddressedPlayer { status }
            | Self::PlayItem { status }
            | Self::AddToNowPlaying { status } => bytes.push(status.0),
            Self::SetBrowsedPlayer {
                status,
                uid_counter,
                number_of_items,
                character_set_id,
                folder_names,
            } => {
                bytes.push(status.0);
                bytes.extend_from_slice(&uid_counter.to_be_bytes());
                bytes.extend_from_slice(&number_of_items.to_be_bytes());
                bytes.extend_from_slice(&character_set_id.0.to_be_bytes());
                push_count_u8(&mut bytes, folder_names.len(), "AVRCP folder count")?;
                for name in folder_names {
                    push_string_u16(&mut bytes, name)?;
                }
            }
            Self::GetFolderItems {
                status,
                uid_counter,
                items,
            } => {
                bytes.push(status.0);
                bytes.extend_from_slice(&uid_counter.to_be_bytes());
                let count = u16::try_from(items.len())
                    .map_err(|_| Error::InvalidField("AVRCP item count"))?;
                bytes.extend_from_slice(&count.to_be_bytes());
                for item in items {
                    bytes.extend_from_slice(&item.to_bytes()?);
                }
            }
            Self::ChangePath {
                status,
                number_of_items,
            } => {
                bytes.push(status.0);
                bytes.extend_from_slice(&number_of_items.to_be_bytes());
            }
            Self::GetItemAttributes { status, attributes } => {
                bytes.push(status.0);
                push_count_u8(&mut bytes, attributes.len(), "AVRCP attribute count")?;
                for attribute in attributes {
                    encode_media_attribute(&mut bytes, attribute)?;
                }
            }
            Self::GetTotalNumberOfItems {
                status,
                uid_counter,
                number_of_items,
            }
            | Self::Search {
                status,
                uid_counter,
                number_of_items,
            } => {
                bytes.push(status.0);
                bytes.extend_from_slice(&uid_counter.to_be_bytes());
                bytes.extend_from_slice(&number_of_items.to_be_bytes());
            }
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
        let mut reader = Reader::new(parameters);
        let response = match pdu_id {
            PduId::GET_CAPABILITIES => {
                let capability_id = CapabilityId(reader.u8()?);
                let count = usize::from(reader.u8()?);
                let remaining = reader.rest();
                let capabilities = if capability_id == CapabilityId::EVENTS_SUPPORTED {
                    if remaining.len() != count {
                        return Err(length_error(remaining.len(), count));
                    }
                    remaining
                        .iter()
                        .copied()
                        .map(|value| Capability::Event(EventId(value)))
                        .collect()
                } else if count == 0 {
                    if !remaining.is_empty() {
                        return Err(Error::TrailingBytes(remaining.len()));
                    }
                    Vec::new()
                } else {
                    if !remaining.len().is_multiple_of(count) {
                        return Err(Error::InvalidField("AVRCP capability size"));
                    }
                    let size = remaining.len() / count;
                    if size == 0 {
                        return Err(Error::InvalidField("AVRCP capability size"));
                    }
                    remaining
                        .chunks_exact(size)
                        .map(|chunk| {
                            if capability_id == CapabilityId::COMPANY_ID && size == 3 {
                                Capability::CompanyId(u32::from_be_bytes([
                                    0, chunk[0], chunk[1], chunk[2],
                                ]))
                            } else {
                                Capability::Raw(chunk.to_vec())
                            }
                        })
                        .collect()
                };
                reader.consume_rest();
                Self::GetCapabilities {
                    capability_id,
                    capabilities,
                }
            }
            PduId::LIST_PLAYER_APPLICATION_SETTING_ATTRIBUTES => {
                Self::ListPlayerApplicationSettingAttributes {
                    attributes: reader
                        .counted_u8s()?
                        .into_iter()
                        .map(ApplicationSettingAttributeId)
                        .collect(),
                }
            }
            PduId::LIST_PLAYER_APPLICATION_SETTING_VALUES => {
                Self::ListPlayerApplicationSettingValues {
                    values: reader
                        .counted_u8s()?
                        .into_iter()
                        .map(ApplicationSettingValue)
                        .collect(),
                }
            }
            PduId::GET_CURRENT_PLAYER_APPLICATION_SETTING_VALUE => {
                let count = usize::from(reader.u8()?);
                let mut settings = Vec::with_capacity(count);
                for _ in 0..count {
                    settings.push(PlayerApplicationSetting {
                        attribute: ApplicationSettingAttributeId(reader.u8()?),
                        value: ApplicationSettingValue(reader.u8()?),
                    });
                }
                Self::GetCurrentPlayerApplicationSettingValue { settings }
            }
            PduId::SET_PLAYER_APPLICATION_SETTING_VALUE => Self::SetPlayerApplicationSettingValue,
            PduId::GET_PLAYER_APPLICATION_SETTING_ATTRIBUTE_TEXT => {
                let count = usize::from(reader.u8()?);
                let mut entries = Vec::with_capacity(count);
                for _ in 0..count {
                    entries.push(AttributeText {
                        attribute: ApplicationSettingAttributeId(reader.u8()?),
                        character_set_id: CharacterSetId(reader.u16_be()?),
                        text: reader.string_u8()?,
                    });
                }
                Self::GetPlayerApplicationSettingAttributeText { entries }
            }
            PduId::GET_PLAYER_APPLICATION_SETTING_VALUE_TEXT => {
                let count = usize::from(reader.u8()?);
                let mut entries = Vec::with_capacity(count);
                for _ in 0..count {
                    entries.push(ValueText {
                        value: ApplicationSettingValue(reader.u8()?),
                        character_set_id: CharacterSetId(reader.u16_be()?),
                        text: reader.string_u8()?,
                    });
                }
                Self::GetPlayerApplicationSettingValueText { entries }
            }
            PduId::INFORM_DISPLAYABLE_CHARACTER_SET => Self::InformDisplayableCharacterSet,
            PduId::INFORM_BATTERY_STATUS_OF_CT => Self::InformBatteryStatusOfCt,
            PduId::GET_PLAY_STATUS => Self::GetPlayStatus {
                song_length: reader.u32_be()?,
                song_position: reader.u32_be()?,
                play_status: PlayStatus(reader.u8()?),
            },
            PduId::GET_ELEMENT_ATTRIBUTES => {
                let count = usize::from(reader.u8()?);
                let mut attributes = Vec::with_capacity(count);
                for _ in 0..count {
                    attributes.push(parse_media_attribute(&mut reader)?);
                }
                Self::GetElementAttributes { attributes }
            }
            PduId::SET_ABSOLUTE_VOLUME => Self::SetAbsoluteVolume {
                volume: reader.u8()?,
            },
            PduId::REGISTER_NOTIFICATION => {
                let event = Event::from_bytes(reader.rest())?;
                reader.consume_rest();
                Self::RegisterNotification { event }
            }
            PduId::SET_ADDRESSED_PLAYER => Self::SetAddressedPlayer {
                status: StatusCode(reader.u8()?),
            },
            PduId::SET_BROWSED_PLAYER => {
                let status = StatusCode(reader.u8()?);
                let uid_counter = reader.u16_be()?;
                let number_of_items = reader.u32_be()?;
                let character_set_id = CharacterSetId(reader.u16_be()?);
                let count = usize::from(reader.u8()?);
                let mut folder_names = Vec::with_capacity(count);
                for _ in 0..count {
                    folder_names.push(reader.string_u16()?);
                }
                Self::SetBrowsedPlayer {
                    status,
                    uid_counter,
                    number_of_items,
                    character_set_id,
                    folder_names,
                }
            }
            PduId::GET_FOLDER_ITEMS => {
                let status = StatusCode(reader.u8()?);
                let uid_counter = reader.u16_be()?;
                let count = usize::from(reader.u16_be()?);
                let mut items = Vec::with_capacity(count);
                for _ in 0..count {
                    items.push(BrowseableItem::parse(&mut reader)?);
                }
                Self::GetFolderItems {
                    status,
                    uid_counter,
                    items,
                }
            }
            PduId::CHANGE_PATH => Self::ChangePath {
                status: StatusCode(reader.u8()?),
                number_of_items: reader.u32_be()?,
            },
            PduId::GET_ITEM_ATTRIBUTES => {
                let status = StatusCode(reader.u8()?);
                let count = usize::from(reader.u8()?);
                let mut attributes = Vec::with_capacity(count);
                for _ in 0..count {
                    attributes.push(parse_media_attribute(&mut reader)?);
                }
                Self::GetItemAttributes { status, attributes }
            }
            PduId::GET_TOTAL_NUMBER_OF_ITEMS => Self::GetTotalNumberOfItems {
                status: StatusCode(reader.u8()?),
                uid_counter: reader.u16_be()?,
                number_of_items: reader.u32_be()?,
            },
            PduId::SEARCH => Self::Search {
                status: StatusCode(reader.u8()?),
                uid_counter: reader.u16_be()?,
                number_of_items: reader.u32_be()?,
            },
            PduId::PLAY_ITEM => Self::PlayItem {
                status: StatusCode(reader.u8()?),
            },
            PduId::ADD_TO_NOW_PLAYING => Self::AddToNowPlaying {
                status: StatusCode(reader.u8()?),
            },
            _ => {
                return Ok(Self::Unknown {
                    pdu_id,
                    parameters: parameters.to_vec(),
                })
            }
        };
        reader.finish()?;
        Ok(response)
    }
}

fn encode_media_attribute(bytes: &mut Vec<u8>, attribute: &MediaAttribute) -> Result<()> {
    bytes.extend_from_slice(&attribute.attribute_id.0.to_be_bytes());
    bytes.extend_from_slice(&attribute.character_set_id.0.to_be_bytes());
    push_string_u16(bytes, &attribute.value)
}

fn parse_media_attribute(reader: &mut Reader<'_>) -> Result<MediaAttribute> {
    Ok(MediaAttribute {
        attribute_id: MediaAttributeId(reader.u32_be()?),
        character_set_id: CharacterSetId(reader.u16_be()?),
        value: reader.string_u16()?,
    })
}

fn push_count_u8(bytes: &mut Vec<u8>, count: usize, field: &'static str) -> Result<()> {
    bytes.push(u8::try_from(count).map_err(|_| Error::InvalidField(field))?);
    Ok(())
}

fn push_string_u8(bytes: &mut Vec<u8>, value: &str) -> Result<()> {
    push_count_u8(bytes, value.len(), "AVRCP one-byte string length")?;
    bytes.extend_from_slice(value.as_bytes());
    Ok(())
}

fn push_string_u16(bytes: &mut Vec<u8>, value: &str) -> Result<()> {
    let length =
        u16::try_from(value.len()).map_err(|_| Error::InvalidField("AVRCP string length"))?;
    bytes.extend_from_slice(&length.to_be_bytes());
    bytes.extend_from_slice(value.as_bytes());
    Ok(())
}

fn length_error(actual: usize, declared: usize) -> Error {
    Error::LengthMismatch { declared, actual }
}

struct Reader<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }
    fn take(&mut self, count: usize) -> Result<&'a [u8]> {
        let end = self
            .offset
            .checked_add(count)
            .ok_or(Error::InvalidField("AVRCP response offset"))?;
        let value = self
            .bytes
            .get(self.offset..end)
            .ok_or(Error::Truncated("AVRCP response parameters"))?;
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
    fn u32_le(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn u64_be(&mut self) -> Result<u64> {
        Ok(u64::from_be_bytes(self.take(8)?.try_into().unwrap()))
    }
    fn u128_le(&mut self) -> Result<u128> {
        Ok(u128::from_le_bytes(self.take(16)?.try_into().unwrap()))
    }
    fn counted_u8s(&mut self) -> Result<Vec<u8>> {
        let count = usize::from(self.u8()?);
        Ok(self.take(count)?.to_vec())
    }
    fn string_u8(&mut self) -> Result<String> {
        let length = usize::from(self.u8()?);
        self.string(length)
    }
    fn string_u16(&mut self) -> Result<String> {
        let length = usize::from(self.u16_be()?);
        self.string(length)
    }
    fn string(&mut self, length: usize) -> Result<String> {
        String::from_utf8(self.take(length)?.to_vec())
            .map_err(|_| Error::InvalidField("AVRCP UTF-8 string"))
    }
    fn rest(&self) -> &'a [u8] {
        &self.bytes[self.offset..]
    }
    fn consume_rest(&mut self) {
        self.offset = self.bytes.len();
    }
    fn finish(self) -> Result<()> {
        if self.offset == self.bytes.len() {
            Ok(())
        } else {
            Err(Error::TrailingBytes(self.bytes.len() - self.offset))
        }
    }
}
