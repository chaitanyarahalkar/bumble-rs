//! Advanced Audio Distribution Profile codec capability models.

use core::fmt;

use bumble_avdtp::{MediaType, ServiceCapabilities};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    Truncated(&'static str),
    Invalid(&'static str),
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for Error {}

pub type Result<T> = core::result::Result<T, Error>;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct CodecType(pub u8);

impl CodecType {
    pub const SBC: Self = Self(0x00);
    pub const MPEG_1_2_AUDIO: Self = Self(0x01);
    pub const MPEG_2_4_AAC: Self = Self(0x02);
    pub const ATRAC_FAMILY: Self = Self(0x03);
    pub const NON_A2DP: Self = Self(0xFF);
}

macro_rules! flags {
    ($name:ident, $storage:ty, { $($constant:ident = $value:expr),+ $(,)? }) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub struct $name(pub $storage);

        impl $name {
            $(pub const $constant: Self = Self($value);)+
        }

        impl core::ops::BitOr for $name {
            type Output = Self;
            fn bitor(self, rhs: Self) -> Self { Self(self.0 | rhs.0) }
        }
    };
}

flags!(SbcSamplingFrequency, u8, {
    SF_16000 = 1 << 3,
    SF_32000 = 1 << 2,
    SF_44100 = 1 << 1,
    SF_48000 = 1,
});
flags!(SbcChannelMode, u8, {
    MONO = 1 << 3,
    DUAL_CHANNEL = 1 << 2,
    STEREO = 1 << 1,
    JOINT_STEREO = 1,
});
flags!(SbcBlockLength, u8, {
    BL_4 = 1 << 3,
    BL_8 = 1 << 2,
    BL_12 = 1 << 1,
    BL_16 = 1,
});
flags!(SbcSubbands, u8, {
    S_4 = 1 << 1,
    S_8 = 1,
});
flags!(SbcAllocationMethod, u8, {
    SNR = 1 << 1,
    LOUDNESS = 1,
});

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SbcMediaCodecInformation {
    pub sampling_frequency: SbcSamplingFrequency,
    pub channel_mode: SbcChannelMode,
    pub block_length: SbcBlockLength,
    pub subbands: SbcSubbands,
    pub allocation_method: SbcAllocationMethod,
    pub minimum_bitpool_value: u8,
    pub maximum_bitpool_value: u8,
}

impl SbcMediaCodecInformation {
    pub fn from_bytes(data: &[u8]) -> Result<Self> {
        if data.len() < 4 {
            return Err(Error::Truncated("SBC codec information"));
        }
        Ok(Self {
            sampling_frequency: SbcSamplingFrequency(data[0] >> 4),
            channel_mode: SbcChannelMode(data[0] & 0x0F),
            block_length: SbcBlockLength(data[1] >> 4),
            subbands: SbcSubbands((data[1] >> 2) & 0x03),
            allocation_method: SbcAllocationMethod(data[1] & 0x03),
            minimum_bitpool_value: data[2],
            maximum_bitpool_value: data[3],
        })
    }

    pub fn to_bytes(self) -> [u8; 4] {
        [
            (self.sampling_frequency.0 << 4) | self.channel_mode.0,
            (self.block_length.0 << 4) | (self.subbands.0 << 2) | self.allocation_method.0,
            self.minimum_bitpool_value,
            self.maximum_bitpool_value,
        ]
    }
}

flags!(AacObjectType, u8, {
    MPEG_2_AAC_LC = 1 << 7,
    MPEG_4_AAC_LC = 1 << 6,
    MPEG_4_AAC_LTP = 1 << 5,
    MPEG_4_AAC_SCALABLE = 1 << 4,
});
flags!(AacSamplingFrequency, u16, {
    SF_8000 = 1 << 11,
    SF_11025 = 1 << 10,
    SF_12000 = 1 << 9,
    SF_16000 = 1 << 8,
    SF_22050 = 1 << 7,
    SF_24000 = 1 << 6,
    SF_32000 = 1 << 5,
    SF_44100 = 1 << 4,
    SF_48000 = 1 << 3,
    SF_64000 = 1 << 2,
    SF_88200 = 1 << 1,
    SF_96000 = 1,
});
flags!(AacChannels, u8, {
    MONO = 1 << 1,
    STEREO = 1,
});

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AacMediaCodecInformation {
    pub object_type: AacObjectType,
    pub sampling_frequency: AacSamplingFrequency,
    pub channels: AacChannels,
    pub vbr: bool,
    pub bitrate: u32,
}

impl AacMediaCodecInformation {
    pub fn from_bytes(data: &[u8]) -> Result<Self> {
        if data.len() < 6 {
            return Err(Error::Truncated("AAC codec information"));
        }
        Ok(Self {
            object_type: AacObjectType(data[0]),
            sampling_frequency: AacSamplingFrequency(
                (u16::from(data[1]) << 4) | u16::from(data[2] >> 4),
            ),
            channels: AacChannels((data[2] >> 2) & 0x03),
            vbr: data[3] & 0x80 != 0,
            bitrate: (u32::from(data[3] & 0x7F) << 16)
                | (u32::from(data[4]) << 8)
                | u32::from(data[5]),
        })
    }

    pub fn to_bytes(self) -> Result<[u8; 6]> {
        if self.bitrate > 0x7F_FFFF {
            return Err(Error::Invalid("AAC bitrate exceeds 23 bits"));
        }
        Ok([
            self.object_type.0,
            (self.sampling_frequency.0 >> 4) as u8,
            ((self.sampling_frequency.0 as u8 & 0x0F) << 4) | (self.channels.0 << 2),
            (u8::from(self.vbr) << 7) | ((self.bitrate >> 16) as u8 & 0x7F),
            (self.bitrate >> 8) as u8,
            self.bitrate as u8,
        ])
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VendorSpecificMediaCodecInformation {
    pub vendor_id: u32,
    pub codec_id: u16,
    pub value: Vec<u8>,
}

impl VendorSpecificMediaCodecInformation {
    pub fn from_bytes(data: &[u8]) -> Result<Self> {
        if data.len() < 6 {
            return Err(Error::Truncated("vendor codec information"));
        }
        Ok(Self {
            vendor_id: u32::from_le_bytes(data[0..4].try_into().expect("four bytes")),
            codec_id: u16::from_le_bytes(data[4..6].try_into().expect("two bytes")),
            value: data[6..].to_vec(),
        })
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut data = self.vendor_id.to_le_bytes().to_vec();
        data.extend_from_slice(&self.codec_id.to_le_bytes());
        data.extend_from_slice(&self.value);
        data
    }
}

flags!(OpusChannelMode, u8, {
    MONO = 1,
    STEREO = 1 << 1,
    DUAL_MONO = 1 << 2,
});
flags!(OpusFrameSize, u8, {
    FS_10MS = 1,
    FS_20MS = 1 << 1,
});
flags!(OpusSamplingFrequency, u8, {
    SF_48000 = 1,
});

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OpusMediaCodecInformation {
    pub channel_mode: OpusChannelMode,
    pub frame_size: OpusFrameSize,
    pub sampling_frequency: OpusSamplingFrequency,
}

impl OpusMediaCodecInformation {
    pub const VENDOR_ID: u32 = 0x0000_00E0;
    pub const CODEC_ID: u16 = 0x0001;

    pub fn from_value(value: &[u8]) -> Result<Self> {
        let value = *value
            .first()
            .ok_or(Error::Truncated("Opus codec information"))?;
        Ok(Self {
            channel_mode: OpusChannelMode(value & 0x07),
            frame_size: OpusFrameSize((value >> 3) & 0x03),
            sampling_frequency: OpusSamplingFrequency((value >> 7) & 0x01),
        })
    }

    pub fn value(self) -> u8 {
        self.channel_mode.0 | (self.frame_size.0 << 3) | (self.sampling_frequency.0 << 7)
    }

    pub fn from_vendor(data: &[u8]) -> Result<Self> {
        let vendor = VendorSpecificMediaCodecInformation::from_bytes(data)?;
        if vendor.vendor_id != Self::VENDOR_ID || vendor.codec_id != Self::CODEC_ID {
            return Err(Error::Invalid("not the A2DP Opus vendor codec"));
        }
        Self::from_value(&vendor.value)
    }

    pub fn to_vendor(self) -> VendorSpecificMediaCodecInformation {
        VendorSpecificMediaCodecInformation {
            vendor_id: Self::VENDOR_ID,
            codec_id: Self::CODEC_ID,
            value: vec![self.value()],
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MediaCodecInformation {
    Sbc(SbcMediaCodecInformation),
    Aac(AacMediaCodecInformation),
    Opus(OpusMediaCodecInformation),
    Vendor(VendorSpecificMediaCodecInformation),
    Other(Vec<u8>),
}

impl MediaCodecInformation {
    pub fn parse(codec_type: CodecType, data: &[u8]) -> Result<Self> {
        match codec_type {
            CodecType::SBC => Ok(Self::Sbc(SbcMediaCodecInformation::from_bytes(data)?)),
            CodecType::MPEG_2_4_AAC => Ok(Self::Aac(AacMediaCodecInformation::from_bytes(data)?)),
            CodecType::NON_A2DP => {
                let vendor = VendorSpecificMediaCodecInformation::from_bytes(data)?;
                if vendor.vendor_id == OpusMediaCodecInformation::VENDOR_ID
                    && vendor.codec_id == OpusMediaCodecInformation::CODEC_ID
                {
                    Ok(Self::Opus(OpusMediaCodecInformation::from_value(
                        &vendor.value,
                    )?))
                } else {
                    Ok(Self::Vendor(vendor))
                }
            }
            _ => Ok(Self::Other(data.to_vec())),
        }
    }

    pub fn codec_type(&self) -> CodecType {
        match self {
            Self::Sbc(_) => CodecType::SBC,
            Self::Aac(_) => CodecType::MPEG_2_4_AAC,
            Self::Opus(_) | Self::Vendor(_) => CodecType::NON_A2DP,
            Self::Other(_) => CodecType(0xFE),
        }
    }

    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        Ok(match self {
            Self::Sbc(info) => info.to_bytes().to_vec(),
            Self::Aac(info) => info.to_bytes()?.to_vec(),
            Self::Opus(info) => info.to_vendor().to_bytes(),
            Self::Vendor(info) => info.to_bytes(),
            Self::Other(data) => data.clone(),
        })
    }

    pub fn to_avdtp_capability(&self) -> Result<ServiceCapabilities> {
        Ok(ServiceCapabilities::MediaCodec {
            media_type: MediaType::AUDIO,
            media_codec_type: self.codec_type().0,
            media_codec_information: self.to_bytes()?,
        })
    }
}
