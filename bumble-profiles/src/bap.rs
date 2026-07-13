//! Shared Basic Audio Profile models.
//!
//! The service/session portions are added with the BAP runtime slice; these
//! assigned-number and codec LTV models are shared by PACS and LE Audio.

use crate::{Error, Result};
use bumble::{advertising_data::Type as AdvertisingType, AdvertisingData, Uuid};
use std::ops::{BitOr, BitOrAssign};

pub use crate::vocs::AudioLocation;

pub const AUDIO_STREAM_CONTROL_SERVICE: u16 = 0x184E;

macro_rules! open_u8 {
    ($name:ident { $($constant:ident = $value:expr),+ $(,)? }) => {
        #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
        pub struct $name(pub u8);

        impl $name {
            $(pub const $constant: Self = Self($value);)+
        }
    };
}

open_u8!(AudioInputType {
    UNSPECIFIED = 0x00,
    BLUETOOTH = 0x01,
    MICROPHONE = 0x02,
    ANALOG = 0x03,
    DIGITAL = 0x04,
    RADIO = 0x05,
    STREAMING = 0x06,
    AMBIENT = 0x07,
});

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ContextType(pub u16);

impl ContextType {
    pub const PROHIBITED: Self = Self(0x0000);
    pub const UNSPECIFIED: Self = Self(0x0001);
    pub const CONVERSATIONAL: Self = Self(0x0002);
    pub const MEDIA: Self = Self(0x0004);
    pub const GAME: Self = Self(0x0008);
    pub const INSTRUCTIONAL: Self = Self(0x0010);
    pub const VOICE_ASSISTANTS: Self = Self(0x0020);
    pub const LIVE: Self = Self(0x0040);
    pub const SOUND_EFFECTS: Self = Self(0x0080);
    pub const NOTIFICATIONS: Self = Self(0x0100);
    pub const RINGTONE: Self = Self(0x0200);
    pub const ALERTS: Self = Self(0x0400);
    pub const EMERGENCY_ALARM: Self = Self(0x0800);
}

impl BitOr for ContextType {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self::Output {
        Self(self.0 | rhs.0)
    }
}

impl BitOrAssign for ContextType {
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}

open_u8!(SamplingFrequency {
    FREQ_8000 = 0x01,
    FREQ_11025 = 0x02,
    FREQ_16000 = 0x03,
    FREQ_22050 = 0x04,
    FREQ_24000 = 0x05,
    FREQ_32000 = 0x06,
    FREQ_44100 = 0x07,
    FREQ_48000 = 0x08,
    FREQ_88200 = 0x09,
    FREQ_96000 = 0x0A,
    FREQ_176400 = 0x0B,
    FREQ_192000 = 0x0C,
    FREQ_384000 = 0x0D,
});

impl SamplingFrequency {
    pub fn from_hz(frequency: u32) -> Result<Self> {
        const FREQUENCIES: [u32; 13] = [
            8_000, 11_025, 16_000, 22_050, 24_000, 32_000, 44_100, 48_000, 88_200, 96_000, 176_400,
            192_000, 384_000,
        ];
        FREQUENCIES
            .iter()
            .position(|candidate| *candidate == frequency)
            .map(|index| Self(index as u8 + 1))
            .ok_or_else(|| Error::InvalidValue(format!("unsupported sampling rate {frequency} Hz")))
    }

    pub fn hz(self) -> Result<u32> {
        const FREQUENCIES: [u32; 13] = [
            8_000, 11_025, 16_000, 22_050, 24_000, 32_000, 44_100, 48_000, 88_200, 96_000, 176_400,
            192_000, 384_000,
        ];
        self.0
            .checked_sub(1)
            .and_then(|index| FREQUENCIES.get(index as usize))
            .copied()
            .ok_or_else(|| {
                Error::InvalidValue(format!("unknown sampling-frequency value 0x{:02X}", self.0))
            })
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SupportedSamplingFrequency(pub u16);

impl SupportedSamplingFrequency {
    pub const FREQ_8000: Self = Self(1 << 0);
    pub const FREQ_11025: Self = Self(1 << 1);
    pub const FREQ_16000: Self = Self(1 << 2);
    pub const FREQ_22050: Self = Self(1 << 3);
    pub const FREQ_24000: Self = Self(1 << 4);
    pub const FREQ_32000: Self = Self(1 << 5);
    pub const FREQ_44100: Self = Self(1 << 6);
    pub const FREQ_48000: Self = Self(1 << 7);
    pub const FREQ_88200: Self = Self(1 << 8);
    pub const FREQ_96000: Self = Self(1 << 9);
    pub const FREQ_176400: Self = Self(1 << 10);
    pub const FREQ_192000: Self = Self(1 << 11);
    pub const FREQ_384000: Self = Self(1 << 12);

    pub fn from_hz(frequencies: &[u32]) -> Result<Self> {
        let mut bits = 0u16;
        for frequency in frequencies {
            let assigned = SamplingFrequency::from_hz(*frequency)?;
            bits |= 1 << (assigned.0 - 1);
        }
        Ok(Self(bits))
    }
}

impl BitOr for SupportedSamplingFrequency {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self::Output {
        Self(self.0 | rhs.0)
    }
}

impl BitOrAssign for SupportedSamplingFrequency {
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}

open_u8!(FrameDuration {
    DURATION_7500_US = 0x00,
    DURATION_10000_US = 0x01,
});

impl FrameDuration {
    pub fn microseconds(self) -> Result<u32> {
        match self {
            Self::DURATION_7500_US => Ok(7_500),
            Self::DURATION_10000_US => Ok(10_000),
            _ => Err(Error::InvalidValue(format!(
                "unknown frame-duration value 0x{:02X}",
                self.0
            ))),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SupportedFrameDuration(pub u8);

impl SupportedFrameDuration {
    pub const DURATION_7500_US_SUPPORTED: Self = Self(0b0001);
    pub const DURATION_10000_US_SUPPORTED: Self = Self(0b0010);
    pub const DURATION_7500_US_PREFERRED: Self = Self(0b0001);
    pub const DURATION_10000_US_PREFERRED: Self = Self(0b0010);
}

impl BitOr for SupportedFrameDuration {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self::Output {
        Self(self.0 | rhs.0)
    }
}

open_u8!(AnnouncementType {
    GENERAL = 0x00,
    TARGETED = 0x01,
});

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnicastServerAdvertisingData {
    pub announcement_type: AnnouncementType,
    pub available_audio_contexts: ContextType,
    pub metadata: Vec<u8>,
}

impl Default for UnicastServerAdvertisingData {
    fn default() -> Self {
        Self {
            announcement_type: AnnouncementType::TARGETED,
            available_audio_contexts: ContextType::MEDIA,
            metadata: Vec::new(),
        }
    }
}

impl UnicastServerAdvertisingData {
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        let metadata_length = u8::try_from(self.metadata.len())
            .map_err(|_| Error::InvalidValue("unicast metadata exceeds 255 bytes".into()))?;
        let mut value = Uuid::from_16_bits(AUDIO_STREAM_CONTROL_SERVICE).to_bytes(false);
        value.push(self.announcement_type.0);
        value.extend_from_slice(&u32::from(self.available_audio_contexts.0).to_le_bytes());
        value.push(metadata_length);
        value.extend_from_slice(&self.metadata);
        Ok(AdvertisingData {
            ad_structures: vec![(AdvertisingType(0x16), value)],
        }
        .to_bytes())
    }
}

pub fn bits_to_channel_counts(mut bits: u32) -> Vec<u8> {
    let mut position = 0u8;
    let mut counts = Vec::new();
    while bits != 0 {
        position += 1;
        if bits & 1 != 0 {
            counts.push(position);
        }
        bits >>= 1;
    }
    counts
}

pub fn channel_counts_to_bits(counts: &[u8]) -> Result<u32> {
    let mut bits = 0u32;
    for count in counts {
        if !(1..=32).contains(count) {
            return Err(Error::InvalidValue(format!(
                "audio channel count {count} is outside 1..=32"
            )));
        }
        bits |= 1 << (count - 1);
    }
    Ok(bits)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CodecSpecificCapabilities {
    pub supported_sampling_frequencies: SupportedSamplingFrequency,
    pub supported_frame_durations: SupportedFrameDuration,
    pub supported_audio_channel_count: Vec<u8>,
    pub min_octets_per_codec_frame: u16,
    pub max_octets_per_codec_frame: u16,
    pub supported_max_codec_frames_per_sdu: u8,
}

impl CodecSpecificCapabilities {
    pub fn from_bytes(data: &[u8]) -> Result<Self> {
        let mut sampling = None;
        let mut duration = None;
        let mut channel_counts = vec![1];
        let mut min_octets = None;
        let mut max_octets = None;
        let mut frames_per_sdu = 1;
        for (tag, value) in parse_ltv(data)? {
            match tag {
                0x01 => {
                    require_ltv_length(tag, value, 2)?;
                    sampling = Some(SupportedSamplingFrequency(u16::from_le_bytes([
                        value[0], value[1],
                    ])));
                }
                0x02 => {
                    require_ltv_length(tag, value, 1)?;
                    duration = Some(SupportedFrameDuration(value[0]));
                }
                0x03 => {
                    require_ltv_length(tag, value, 1)?;
                    channel_counts = bits_to_channel_counts(u32::from(value[0]));
                }
                0x04 => {
                    require_ltv_length(tag, value, 4)?;
                    min_octets = Some(u16::from_le_bytes([value[0], value[1]]));
                    max_octets = Some(u16::from_le_bytes([value[2], value[3]]));
                }
                0x05 => {
                    require_ltv_length(tag, value, 1)?;
                    frames_per_sdu = value[0];
                }
                _ => {}
            }
        }
        Ok(Self {
            supported_sampling_frequencies: sampling.ok_or_else(|| {
                Error::InvalidValue("codec capabilities omit sampling frequencies".into())
            })?,
            supported_frame_durations: duration.ok_or_else(|| {
                Error::InvalidValue("codec capabilities omit frame durations".into())
            })?,
            supported_audio_channel_count: channel_counts,
            min_octets_per_codec_frame: min_octets.ok_or_else(|| {
                Error::InvalidValue("codec capabilities omit minimum octets per frame".into())
            })?,
            max_octets_per_codec_frame: max_octets.ok_or_else(|| {
                Error::InvalidValue("codec capabilities omit maximum octets per frame".into())
            })?,
            supported_max_codec_frames_per_sdu: frames_per_sdu,
        })
    }

    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        let channel_bits = channel_counts_to_bits(&self.supported_audio_channel_count)?;
        let channel_bits = u8::try_from(channel_bits).map_err(|_| {
            Error::InvalidValue("codec capability channel counts exceed eight bits".into())
        })?;
        let mut value = Vec::with_capacity(19);
        value.extend_from_slice(&[3, 0x01]);
        value.extend_from_slice(&self.supported_sampling_frequencies.0.to_le_bytes());
        value.extend_from_slice(&[2, 0x02, self.supported_frame_durations.0]);
        value.extend_from_slice(&[2, 0x03, channel_bits]);
        value.extend_from_slice(&[5, 0x04]);
        value.extend_from_slice(&self.min_octets_per_codec_frame.to_le_bytes());
        value.extend_from_slice(&self.max_octets_per_codec_frame.to_le_bytes());
        value.extend_from_slice(&[2, 0x05, self.supported_max_codec_frames_per_sdu]);
        Ok(value)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CodecSpecificConfiguration {
    pub sampling_frequency: Option<SamplingFrequency>,
    pub frame_duration: Option<FrameDuration>,
    pub audio_channel_allocation: Option<AudioLocation>,
    pub octets_per_codec_frame: Option<u16>,
    pub codec_frames_per_sdu: Option<u8>,
}

impl CodecSpecificConfiguration {
    pub fn from_bytes(data: &[u8]) -> Result<Self> {
        let mut configuration = Self::default();
        for (tag, value) in parse_ltv(data)? {
            match tag {
                0x01 => {
                    require_ltv_length(tag, value, 1)?;
                    configuration.sampling_frequency = Some(SamplingFrequency(value[0]));
                }
                0x02 => {
                    require_ltv_length(tag, value, 1)?;
                    configuration.frame_duration = Some(FrameDuration(value[0]));
                }
                0x03 => {
                    require_ltv_length(tag, value, 4)?;
                    configuration.audio_channel_allocation = Some(AudioLocation(
                        u32::from_le_bytes(value.try_into().expect("four-byte allocation")),
                    ));
                }
                0x04 => {
                    require_ltv_length(tag, value, 2)?;
                    configuration.octets_per_codec_frame =
                        Some(u16::from_le_bytes([value[0], value[1]]));
                }
                0x05 => {
                    require_ltv_length(tag, value, 1)?;
                    configuration.codec_frames_per_sdu = Some(value[0]);
                }
                _ => {}
            }
        }
        Ok(configuration)
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut value = Vec::new();
        if let Some(frequency) = self.sampling_frequency {
            value.extend_from_slice(&[2, 0x01, frequency.0]);
        }
        if let Some(duration) = self.frame_duration {
            value.extend_from_slice(&[2, 0x02, duration.0]);
        }
        if let Some(location) = self.audio_channel_allocation {
            value.extend_from_slice(&[5, 0x03]);
            value.extend_from_slice(&location.0.to_le_bytes());
        }
        if let Some(octets) = self.octets_per_codec_frame {
            value.extend_from_slice(&[3, 0x04]);
            value.extend_from_slice(&octets.to_le_bytes());
        }
        if let Some(frames) = self.codec_frames_per_sdu {
            value.extend_from_slice(&[2, 0x05, frames]);
        }
        value
    }
}

fn parse_ltv(data: &[u8]) -> Result<Vec<(u8, &[u8])>> {
    let mut entries = Vec::new();
    let mut offset = 0;
    while offset < data.len() {
        let length = usize::from(data[offset]);
        if length == 0 {
            return Err(Error::InvalidValue("zero-length LTV entry".into()));
        }
        let end = offset
            .checked_add(1 + length)
            .ok_or_else(|| Error::InvalidValue("LTV length overflow".into()))?;
        if end > data.len() {
            return Err(Error::InvalidValue(format!(
                "truncated LTV entry at offset {offset}"
            )));
        }
        entries.push((data[offset + 1], &data[offset + 2..end]));
        offset = end;
    }
    Ok(entries)
}

fn require_ltv_length(tag: u8, value: &[u8], expected: usize) -> Result<()> {
    if value.len() != expected {
        return Err(Error::InvalidValue(format!(
            "LTV tag 0x{tag:02X} has {} value bytes, expected {expected}",
            value.len()
        )));
    }
    Ok(())
}
