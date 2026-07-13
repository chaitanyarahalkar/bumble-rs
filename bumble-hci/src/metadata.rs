//! Symbolic controller-capability metadata from the Bluetooth specification.
//!
//! Bumble exposes names for the values returned by controller-information
//! commands. The large version, feature, codec, and Supported Commands tables
//! are generated from upstream; the compact voice-setting bitfield is modeled
//! directly here.

mod tables {
    include!("metadata_tables.rs");
}

/// Return Bumble's symbolic name for an HCI/LMP specification version.
pub fn specification_version_name(value: u8) -> Option<&'static str> {
    tables::SPECIFICATION_VERSION_NAMES
        .iter()
        .find_map(|(candidate, name)| (*candidate == value).then_some(*name))
}

/// Return Bumble's symbolic name for a standard controller codec ID.
pub fn codec_id_name(value: u8) -> Option<&'static str> {
    tables::CODEC_ID_NAMES
        .iter()
        .find_map(|(candidate, name)| (*candidate == value).then_some(*name))
}

/// Decode the set bits in an LE feature bitmap, in specification order.
pub fn le_feature_names(features: &[u8]) -> Vec<&'static str> {
    tables::LE_FEATURE_NAMES
        .iter()
        .filter_map(|(bit, name)| {
            let bit = usize::from(*bit);
            features
                .get(bit / 8)
                .is_some_and(|byte| byte & (1 << (bit % 8)) != 0)
                .then_some(*name)
        })
        .collect()
}

/// Decode the set bits in the 64-byte HCI Supported Commands bitmap.
pub fn supported_command_names(supported_commands: &[u8]) -> Vec<&'static str> {
    tables::SUPPORTED_COMMAND_NAMES
        .iter()
        .filter_map(|(byte_index, mask, name)| {
            supported_commands
                .get(*byte_index)
                .is_some_and(|byte| byte & mask != 0)
                .then_some(*name)
        })
        .collect()
}

/// Decode the bit flags used by Read Local Supported Codecs V2.
pub fn codec_transport_names(transport: u8) -> Vec<&'static str> {
    const TRANSPORTS: &[(u8, &str)] = &[
        (1 << 0, "BR_EDR_ACL"),
        (1 << 1, "BR_EDR_SCO"),
        (1 << 2, "LE_CIS"),
        (1 << 3, "LE_BIS"),
    ];
    TRANSPORTS
        .iter()
        .filter_map(|(mask, name)| (transport & mask != 0).then_some(*name))
        .collect()
}

/// Voice Setting air coding format (Core Vol 2, Part E, 7.3.29).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VoiceAirCodingFormat {
    Cvsd,
    ULaw,
    ALaw,
    TransparentData,
}

impl VoiceAirCodingFormat {
    pub fn name(self) -> &'static str {
        match self {
            VoiceAirCodingFormat::Cvsd => "CVSD",
            VoiceAirCodingFormat::ULaw => "U_LAW",
            VoiceAirCodingFormat::ALaw => "A_LAW",
            VoiceAirCodingFormat::TransparentData => "TRANSPARENT_DATA",
        }
    }

    const fn bits(self) -> u16 {
        match self {
            VoiceAirCodingFormat::Cvsd => 0,
            VoiceAirCodingFormat::ULaw => 1,
            VoiceAirCodingFormat::ALaw => 2,
            VoiceAirCodingFormat::TransparentData => 3,
        }
    }
}

/// Voice Setting input sample width.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VoiceInputSampleSize {
    Bits8,
    Bits16,
}

impl VoiceInputSampleSize {
    pub fn name(self) -> &'static str {
        match self {
            VoiceInputSampleSize::Bits8 => "SIZE_8_BITS",
            VoiceInputSampleSize::Bits16 => "SIZE_16_BITS",
        }
    }

    const fn bits(self) -> u16 {
        match self {
            VoiceInputSampleSize::Bits8 => 0,
            VoiceInputSampleSize::Bits16 => 1,
        }
    }
}

/// Voice Setting input data representation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VoiceInputDataFormat {
    OnesComplement,
    TwosComplement,
    SignAndMagnitude,
    Unsigned,
}

impl VoiceInputDataFormat {
    pub fn name(self) -> &'static str {
        match self {
            VoiceInputDataFormat::OnesComplement => "ONES_COMPLEMENT",
            VoiceInputDataFormat::TwosComplement => "TWOS_COMPLEMENT",
            VoiceInputDataFormat::SignAndMagnitude => "SIGN_AND_MAGNITUDE",
            VoiceInputDataFormat::Unsigned => "UNSIGNED",
        }
    }

    const fn bits(self) -> u16 {
        match self {
            VoiceInputDataFormat::OnesComplement => 0,
            VoiceInputDataFormat::TwosComplement => 1,
            VoiceInputDataFormat::SignAndMagnitude => 2,
            VoiceInputDataFormat::Unsigned => 3,
        }
    }
}

/// Voice Setting input coding format.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VoiceInputCodingFormat {
    Linear,
    ULaw,
    ALaw,
    Reserved,
}

impl VoiceInputCodingFormat {
    pub fn name(self) -> &'static str {
        match self {
            VoiceInputCodingFormat::Linear => "LINEAR",
            VoiceInputCodingFormat::ULaw => "U_LAW",
            VoiceInputCodingFormat::ALaw => "A_LAW",
            VoiceInputCodingFormat::Reserved => "RESERVED",
        }
    }

    const fn bits(self) -> u16 {
        match self {
            VoiceInputCodingFormat::Linear => 0,
            VoiceInputCodingFormat::ULaw => 1,
            VoiceInputCodingFormat::ALaw => 2,
            VoiceInputCodingFormat::Reserved => 3,
        }
    }
}

/// Typed view of the 10 defined bits in the HCI Voice Setting field.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VoiceSetting {
    pub air_coding_format: VoiceAirCodingFormat,
    pub linear_pcm_bit_position: u8,
    pub input_sample_size: VoiceInputSampleSize,
    pub input_data_format: VoiceInputDataFormat,
    pub input_coding_format: VoiceInputCodingFormat,
}

impl VoiceSetting {
    pub fn from_bits(value: u16) -> Self {
        Self {
            air_coding_format: match value & 0b11 {
                0 => VoiceAirCodingFormat::Cvsd,
                1 => VoiceAirCodingFormat::ULaw,
                2 => VoiceAirCodingFormat::ALaw,
                _ => VoiceAirCodingFormat::TransparentData,
            },
            linear_pcm_bit_position: ((value >> 2) & 0b111) as u8,
            input_sample_size: if value & (1 << 5) == 0 {
                VoiceInputSampleSize::Bits8
            } else {
                VoiceInputSampleSize::Bits16
            },
            input_data_format: match (value >> 6) & 0b11 {
                0 => VoiceInputDataFormat::OnesComplement,
                1 => VoiceInputDataFormat::TwosComplement,
                2 => VoiceInputDataFormat::SignAndMagnitude,
                _ => VoiceInputDataFormat::Unsigned,
            },
            input_coding_format: match (value >> 8) & 0b11 {
                0 => VoiceInputCodingFormat::Linear,
                1 => VoiceInputCodingFormat::ULaw,
                2 => VoiceInputCodingFormat::ALaw,
                _ => VoiceInputCodingFormat::Reserved,
            },
        }
    }

    pub fn to_bits(self) -> u16 {
        self.air_coding_format.bits()
            | (u16::from(self.linear_pcm_bit_position & 0b111) << 2)
            | (self.input_sample_size.bits() << 5)
            | (self.input_data_format.bits() << 6)
            | (self.input_coding_format.bits() << 8)
    }
}
