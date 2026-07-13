//! Common LE Audio metadata models.

use crate::bap::ContextType;
use crate::{Error, Result};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AudioActiveState(pub u8);

impl AudioActiveState {
    pub const NO_AUDIO_DATA_TRANSMITTED: Self = Self(0x00);
    pub const AUDIO_DATA_TRANSMITTED: Self = Self(0x01);
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AssistedListeningStream(pub u8);

impl AssistedListeningStream {
    pub const UNSPECIFIED_AUDIO_ENHANCEMENT: Self = Self(0x00);
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MetadataTag(pub u8);

impl MetadataTag {
    pub const PREFERRED_AUDIO_CONTEXTS: Self = Self(0x01);
    pub const STREAMING_AUDIO_CONTEXTS: Self = Self(0x02);
    pub const PROGRAM_INFO: Self = Self(0x03);
    pub const LANGUAGE: Self = Self(0x04);
    pub const CCID_LIST: Self = Self(0x05);
    pub const PARENTAL_RATING: Self = Self(0x06);
    pub const PROGRAM_INFO_URI: Self = Self(0x07);
    pub const AUDIO_ACTIVE_STATE: Self = Self(0x08);
    pub const BROADCAST_AUDIO_IMMEDIATE_RENDERING_FLAG: Self = Self(0x09);
    pub const ASSISTED_LISTENING_STREAM: Self = Self(0x0A);
    pub const BROADCAST_NAME: Self = Self(0x0B);
    pub const EXTENDED_METADATA: Self = Self(0xFE);
    pub const VENDOR_SPECIFIC: Self = Self(0xFF);

    pub fn name(self) -> &'static str {
        match self {
            Self::PREFERRED_AUDIO_CONTEXTS => "PREFERRED_AUDIO_CONTEXTS",
            Self::STREAMING_AUDIO_CONTEXTS => "STREAMING_AUDIO_CONTEXTS",
            Self::PROGRAM_INFO => "PROGRAM_INFO",
            Self::LANGUAGE => "LANGUAGE",
            Self::CCID_LIST => "CCID_LIST",
            Self::PARENTAL_RATING => "PARENTAL_RATING",
            Self::PROGRAM_INFO_URI => "PROGRAM_INFO_URI",
            Self::AUDIO_ACTIVE_STATE => "AUDIO_ACTIVE_STATE",
            Self::BROADCAST_AUDIO_IMMEDIATE_RENDERING_FLAG => {
                "BROADCAST_AUDIO_IMMEDIATE_RENDERING_FLAG"
            }
            Self::ASSISTED_LISTENING_STREAM => "ASSISTED_LISTENING_STREAM",
            Self::BROADCAST_NAME => "BROADCAST_NAME",
            Self::EXTENDED_METADATA => "EXTENDED_METADATA",
            Self::VENDOR_SPECIFIC => "VENDOR_SPECIFIC",
            _ => "UNKNOWN",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MetadataEntry {
    pub tag: MetadataTag,
    pub data: Vec<u8>,
}

impl MetadataEntry {
    pub fn new(tag: MetadataTag, data: impl Into<Vec<u8>>) -> Self {
        Self {
            tag,
            data: data.into(),
        }
    }

    pub fn from_value_bytes(value: &[u8]) -> Result<Self> {
        let (&tag, data) = value
            .split_first()
            .ok_or_else(|| Error::InvalidValue("metadata entry omits its tag".into()))?;
        Ok(Self::new(MetadataTag(tag), data))
    }

    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        let length = self
            .data
            .len()
            .checked_add(1)
            .and_then(|length| u8::try_from(length).ok())
            .ok_or_else(|| Error::InvalidValue("metadata entry exceeds 254 data bytes".into()))?;
        let mut value = Vec::with_capacity(self.data.len() + 2);
        value.extend_from_slice(&[length, self.tag.0]);
        value.extend_from_slice(&self.data);
        Ok(value)
    }

    pub fn decode(&self) -> Result<MetadataValue> {
        match self.tag {
            MetadataTag::PREFERRED_AUDIO_CONTEXTS | MetadataTag::STREAMING_AUDIO_CONTEXTS => {
                let bytes: [u8; 2] = self.data.as_slice().try_into().map_err(|_| {
                    Error::InvalidValue(format!(
                        "audio-context metadata has length {}, expected 2",
                        self.data.len()
                    ))
                })?;
                Ok(MetadataValue::Context(ContextType(u16::from_le_bytes(
                    bytes,
                ))))
            }
            MetadataTag::PROGRAM_INFO
            | MetadataTag::PROGRAM_INFO_URI
            | MetadataTag::BROADCAST_NAME => String::from_utf8(self.data.clone())
                .map(MetadataValue::Text)
                .map_err(|error| {
                    Error::InvalidValue(format!("metadata text is not UTF-8: {error}"))
                }),
            MetadataTag::LANGUAGE => {
                if !self.data.is_ascii() {
                    return Err(Error::InvalidValue("metadata language is not ASCII".into()));
                }
                Ok(MetadataValue::Language(
                    String::from_utf8(self.data.clone()).expect("ASCII is UTF-8"),
                ))
            }
            MetadataTag::CCID_LIST => Ok(MetadataValue::CcidList(self.data.clone())),
            MetadataTag::PARENTAL_RATING => {
                first_byte(&self.data).map(MetadataValue::ParentalRating)
            }
            MetadataTag::AUDIO_ACTIVE_STATE => first_byte(&self.data)
                .map(|value| MetadataValue::AudioActiveState(AudioActiveState(value))),
            MetadataTag::ASSISTED_LISTENING_STREAM => first_byte(&self.data).map(|value| {
                MetadataValue::AssistedListeningStream(AssistedListeningStream(value))
            }),
            _ => Ok(MetadataValue::Bytes(self.data.clone())),
        }
    }
}

fn first_byte(data: &[u8]) -> Result<u8> {
    data.first()
        .copied()
        .ok_or_else(|| Error::InvalidValue("metadata value is empty".into()))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MetadataValue {
    Context(ContextType),
    Text(String),
    Language(String),
    CcidList(Vec<u8>),
    ParentalRating(u8),
    AudioActiveState(AudioActiveState),
    AssistedListeningStream(AssistedListeningStream),
    Bytes(Vec<u8>),
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Metadata {
    pub entries: Vec<MetadataEntry>,
}

impl Metadata {
    pub fn new(entries: impl Into<Vec<MetadataEntry>>) -> Self {
        Self {
            entries: entries.into(),
        }
    }

    pub fn from_bytes(data: &[u8]) -> Result<Self> {
        let mut offset = 0;
        let mut entries = Vec::new();
        while offset < data.len() {
            let entry_length = usize::from(data[offset]);
            if entry_length == 0 {
                return Err(Error::InvalidValue(format!(
                    "zero-length metadata entry at offset {offset}"
                )));
            }
            let end = offset
                .checked_add(entry_length + 1)
                .ok_or_else(|| Error::InvalidValue("metadata length overflow".into()))?;
            if end > data.len() {
                return Err(Error::InvalidValue(format!(
                    "truncated metadata entry at offset {offset}"
                )));
            }
            entries.push(MetadataEntry::from_value_bytes(&data[offset + 1..end])?);
            offset = end;
        }
        Ok(Self { entries })
    }

    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        let mut value = Vec::new();
        for entry in &self.entries {
            value.extend_from_slice(&entry.to_bytes()?);
        }
        Ok(value)
    }

    pub fn pretty_print(&self, indent: &str) -> Result<String> {
        let width = self
            .entries
            .iter()
            .map(|entry| entry.tag.name().len())
            .max()
            .unwrap_or(0);
        self.entries
            .iter()
            .map(|entry| {
                let name = entry.tag.name();
                Ok(format!(
                    "{indent}{name}: {}{}",
                    " ".repeat(width - name.len()),
                    metadata_value_text(&entry.decode()?)
                ))
            })
            .collect::<Result<Vec<_>>>()
            .map(|lines| lines.join("\n"))
    }
}

fn metadata_value_text(value: &MetadataValue) -> String {
    match value {
        MetadataValue::Context(value) => format!("0x{:04X}", value.0),
        MetadataValue::Text(value) | MetadataValue::Language(value) => value.clone(),
        MetadataValue::CcidList(value) => format!("{value:?}"),
        MetadataValue::ParentalRating(value) => value.to_string(),
        MetadataValue::AudioActiveState(value) => format!("0x{:02X}", value.0),
        MetadataValue::AssistedListeningStream(value) => format!("0x{:02X}", value.0),
        MetadataValue::Bytes(value) => value
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>(),
    }
}

impl core::fmt::Display for Metadata {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(formatter, "Metadata(entries=")?;
        for (index, entry) in self.entries.iter().enumerate() {
            if index != 0 {
                write!(formatter, ", ")?;
            }
            let value = entry
                .decode()
                .map(|value| metadata_value_text(&value))
                .unwrap_or_else(|_| "<invalid>".into());
            write!(formatter, "{}: {value}", entry.tag.name())?;
        }
        write!(formatter, ")")
    }
}
