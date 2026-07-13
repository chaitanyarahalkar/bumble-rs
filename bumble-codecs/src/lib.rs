//! Common media bitstream codecs from `google/bumble`.

pub mod g722;
pub mod lc3;

use core::fmt;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    InvalidArgument(&'static str),
    InvalidPacket(&'static str),
    Unsupported(&'static str),
    ValueTooLarge,
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for Error {}

pub type Result<T> = core::result::Result<T, Error>;

#[derive(Clone, Debug)]
pub struct BitReader<'a> {
    data: &'a [u8],
    bit_position: usize,
}

impl<'a> BitReader<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self {
            data,
            bit_position: 0,
        }
    }

    pub fn read(&mut self, bits: usize) -> Result<u32> {
        if bits > 32 {
            return Err(Error::InvalidArgument("maximum read size is 32"));
        }
        if bits > self.bits_left() {
            return Err(Error::InvalidArgument("trying to read past the data"));
        }
        let mut value = 0u32;
        for _ in 0..bits {
            let byte = self.data[self.bit_position / 8];
            let shift = 7 - (self.bit_position % 8);
            value = (value << 1) | u32::from((byte >> shift) & 1);
            self.bit_position += 1;
        }
        Ok(value)
    }

    pub fn read_bytes(&mut self, count: usize) -> Result<Vec<u8>> {
        let bits = count.checked_mul(8).ok_or(Error::ValueTooLarge)?;
        if bits > self.bits_left() {
            return Err(Error::InvalidArgument("not enough data"));
        }
        if self.bit_position.is_multiple_of(8) {
            let offset = self.bit_position / 8;
            self.bit_position += bits;
            return Ok(self.data[offset..offset + count].to_vec());
        }
        (0..count)
            .map(|_| self.read(8).map(|value| value as u8))
            .collect()
    }

    pub fn bits_left(&self) -> usize {
        self.data.len() * 8 - self.bit_position
    }

    pub fn bit_position(&self) -> usize {
        self.bit_position
    }

    pub fn skip(&mut self, mut bits: usize) -> Result<()> {
        while bits != 0 {
            let count = bits.min(32);
            self.read(count)?;
            bits -= count;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BitWriter {
    bytes: Vec<u8>,
    bit_count: usize,
}

impl BitWriter {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn write(&mut self, value: u32, bit_count: usize) -> Result<()> {
        if bit_count > 32 {
            return Err(Error::InvalidArgument("maximum write size is 32"));
        }
        if bit_count < 32 && value >= (1u32 << bit_count) {
            return Err(Error::InvalidArgument("value does not fit bit count"));
        }
        for shift in (0..bit_count).rev() {
            if self.bit_count.is_multiple_of(8) {
                self.bytes.push(0);
            }
            let bit = ((value >> shift) & 1) as u8;
            let index = self.bytes.len() - 1;
            self.bytes[index] |= bit << (7 - (self.bit_count % 8));
            self.bit_count += 1;
        }
        Ok(())
    }

    pub fn write_bytes(&mut self, data: &[u8]) -> Result<()> {
        for byte in data {
            self.write(u32::from(*byte), 8)?;
        }
        Ok(())
    }

    pub fn bit_count(&self) -> usize {
        self.bit_count
    }

    pub fn into_bytes(self) -> Vec<u8> {
        self.bytes
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GaSpecificConfig {
    pub audio_object_type: u8,
}

impl GaSpecificConfig {
    fn from_bits(
        reader: &mut BitReader<'_>,
        channel_configuration: u8,
        audio_object_type: u8,
    ) -> Result<Self> {
        reader.read(1)?;
        if reader.read(1)? != 0 {
            reader.read(14)?;
        }
        let extension_flag = reader.read(1)? != 0;
        if channel_configuration == 0 {
            return Err(Error::Unsupported("program_config_element"));
        }
        if matches!(audio_object_type, 6 | 20) {
            reader.read(3)?;
        }
        if extension_flag {
            if audio_object_type == 22 {
                reader.read(5)?;
            }
            reader.read(11)?;
            if matches!(audio_object_type, 17 | 19 | 20 | 23) {
                reader.read(1)?;
                reader.read(1)?;
                reader.read(1)?;
            }
            if reader.read(1)? != 0 {
                return Err(Error::Unsupported("extensionFlag3"));
            }
        }
        Ok(Self { audio_object_type })
    }

    fn to_bits(&self, writer: &mut BitWriter) -> Result<()> {
        if !matches!(self.audio_object_type, 1 | 2) {
            return Err(Error::Unsupported("GA audio object type"));
        }
        writer.write(0, 1)?;
        writer.write(0, 1)?;
        writer.write(0, 1)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AudioSpecificConfig {
    pub audio_object_type: u8,
    pub sampling_frequency_index: u8,
    pub sampling_frequency: u32,
    pub channel_configuration: u8,
    pub ga_specific_config: GaSpecificConfig,
    pub sbr_present_flag: bool,
    pub ps_present_flag: bool,
    pub extension_audio_object_type: u8,
    pub extension_sampling_frequency_index: u8,
    pub extension_sampling_frequency: u32,
    pub extension_channel_configuration: u8,
}

impl AudioSpecificConfig {
    pub const SAMPLING_FREQUENCIES: [u32; 13] = [
        96_000, 88_200, 64_000, 48_000, 44_100, 32_000, 24_000, 22_050, 16_000, 12_000, 11_025,
        8_000, 7_350,
    ];

    pub fn for_simple_aac(
        audio_object_type: u8,
        sampling_frequency: u32,
        channel_configuration: u8,
    ) -> Result<Self> {
        let index = Self::SAMPLING_FREQUENCIES
            .iter()
            .position(|frequency| *frequency == sampling_frequency)
            .ok_or(Error::InvalidArgument("invalid sampling frequency"))?;
        Ok(Self {
            audio_object_type,
            sampling_frequency_index: index as u8,
            sampling_frequency,
            channel_configuration,
            ga_specific_config: GaSpecificConfig { audio_object_type },
            sbr_present_flag: false,
            ps_present_flag: false,
            extension_audio_object_type: 0,
            extension_sampling_frequency_index: 0,
            extension_sampling_frequency: 0,
            extension_channel_configuration: 0,
        })
    }

    fn from_bits(reader: &mut BitReader<'_>) -> Result<Self> {
        let mut audio_object_type = read_audio_object_type(reader)?;
        let sampling_frequency_index = reader.read(4)? as u8;
        let sampling_frequency = read_sampling_frequency(reader, sampling_frequency_index)?;
        let channel_configuration = reader.read(4)? as u8;
        let mut sbr_present_flag = false;
        let mut ps_present_flag = false;
        let mut extension_audio_object_type = 0;
        let mut extension_sampling_frequency_index = 0;
        let mut extension_sampling_frequency = 0;
        let mut extension_channel_configuration = 0;
        if matches!(audio_object_type, 5 | 29) {
            extension_audio_object_type = 5;
            sbr_present_flag = true;
            ps_present_flag = audio_object_type == 29;
            extension_sampling_frequency_index = reader.read(4)? as u8;
            extension_sampling_frequency =
                read_sampling_frequency(reader, extension_sampling_frequency_index)?;
            audio_object_type = read_audio_object_type(reader)?;
            if audio_object_type == 22 {
                extension_channel_configuration = reader.read(4)? as u8;
            }
        }
        if !matches!(
            audio_object_type,
            1 | 2 | 3 | 4 | 6 | 7 | 17 | 19 | 20 | 21 | 22 | 23
        ) {
            return Err(Error::Unsupported("audio object type"));
        }
        let ga_specific_config =
            GaSpecificConfig::from_bits(reader, channel_configuration, audio_object_type)?;
        Ok(Self {
            audio_object_type,
            sampling_frequency_index,
            sampling_frequency,
            channel_configuration,
            ga_specific_config,
            sbr_present_flag,
            ps_present_flag,
            extension_audio_object_type,
            extension_sampling_frequency_index,
            extension_sampling_frequency,
            extension_channel_configuration,
        })
    }

    fn to_bits(&self, writer: &mut BitWriter) -> Result<()> {
        if self.sampling_frequency_index >= 15 {
            return Err(Error::Unsupported("sampling frequency index"));
        }
        if !matches!(self.audio_object_type, 1 | 2) {
            return Err(Error::Unsupported("audio object type"));
        }
        writer.write(u32::from(self.audio_object_type), 5)?;
        writer.write(u32::from(self.sampling_frequency_index), 4)?;
        writer.write(u32::from(self.channel_configuration), 4)?;
        self.ga_specific_config.to_bits(writer)
    }
}

fn read_audio_object_type(reader: &mut BitReader<'_>) -> Result<u8> {
    let value = reader.read(5)? as u8;
    if value == 31 {
        Ok(32 + reader.read(6)? as u8)
    } else {
        Ok(value)
    }
}

fn read_sampling_frequency(reader: &mut BitReader<'_>, index: u8) -> Result<u32> {
    if index == 0x0F {
        reader.read(24)
    } else {
        AudioSpecificConfig::SAMPLING_FREQUENCIES
            .get(usize::from(index))
            .copied()
            .ok_or(Error::InvalidPacket("sampling frequency index"))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StreamMuxConfig {
    pub other_data_present: bool,
    pub other_data_len_bits: usize,
    pub audio_specific_config: AudioSpecificConfig,
}

impl StreamMuxConfig {
    fn from_bits(reader: &mut BitReader<'_>) -> Result<Self> {
        let audio_mux_version = reader.read(1)?;
        let audio_mux_version_a = if audio_mux_version == 1 {
            reader.read(1)?
        } else {
            0
        };
        if audio_mux_version_a != 0 {
            return Err(Error::Unsupported("audioMuxVersionA"));
        }
        if audio_mux_version == 1 {
            read_latm_value(reader)?;
        }
        reader.read(1)?;
        reader.read(6)?;
        if reader.read(4)? != 0 {
            return Err(Error::Unsupported("num_program"));
        }
        if reader.read(3)? != 0 {
            return Err(Error::Unsupported("num_layer"));
        }
        let audio_specific_config = if audio_mux_version == 0 {
            AudioSpecificConfig::from_bits(reader)?
        } else {
            let mut asc_len =
                usize::try_from(read_latm_value(reader)?).map_err(|_| Error::ValueTooLarge)?;
            let marker = reader.bit_position();
            let config = AudioSpecificConfig::from_bits(reader)?;
            let consumed = reader.bit_position() - marker;
            if asc_len < consumed {
                return Err(Error::InvalidPacket("audio specific config length"));
            }
            asc_len -= consumed;
            reader.skip(asc_len)?;
            config
        };
        match reader.read(3)? {
            0 => {
                reader.read(8)?;
            }
            1 => {
                reader.read(9)?;
            }
            _ => return Err(Error::Unsupported("frame length type")),
        }
        let other_data_present = reader.read(1)? != 0;
        let mut other_data_len_bits = 0usize;
        if other_data_present {
            if audio_mux_version == 1 {
                other_data_len_bits =
                    usize::try_from(read_latm_value(reader)?).map_err(|_| Error::ValueTooLarge)?;
            } else {
                loop {
                    other_data_len_bits = other_data_len_bits
                        .checked_mul(256)
                        .ok_or(Error::ValueTooLarge)?;
                    let more = reader.read(1)? != 0;
                    other_data_len_bits = other_data_len_bits
                        .checked_add(reader.read(8)? as usize)
                        .ok_or(Error::ValueTooLarge)?;
                    if !more {
                        break;
                    }
                }
            }
        }
        if reader.read(1)? != 0 {
            reader.read(8)?;
        }
        Ok(Self {
            other_data_present,
            other_data_len_bits,
            audio_specific_config,
        })
    }

    fn to_bits(&self, writer: &mut BitWriter) -> Result<()> {
        writer.write(0, 1)?;
        writer.write(1, 1)?;
        writer.write(0, 6)?;
        writer.write(0, 4)?;
        writer.write(0, 3)?;
        self.audio_specific_config.to_bits(writer)?;
        writer.write(0, 3)?;
        writer.write(0, 8)?;
        writer.write(0, 1)?;
        writer.write(0, 1)
    }
}

fn read_latm_value(reader: &mut BitReader<'_>) -> Result<u32> {
    let bytes_for_value = reader.read(2)?;
    let mut value = 0u32;
    for _ in 0..=bytes_for_value {
        let byte = reader.read(8)?;
        value = value
            .checked_mul(256)
            .and_then(|value| value.checked_add(byte))
            .ok_or(Error::ValueTooLarge)?;
    }
    Ok(value)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AudioMuxElement {
    pub stream_mux_config: StreamMuxConfig,
    pub payload: Vec<u8>,
}

impl AudioMuxElement {
    fn from_bits(reader: &mut BitReader<'_>) -> Result<Self> {
        if reader.read(1)? != 0 {
            return Err(Error::Unsupported("useSameStreamMux"));
        }
        let stream_mux_config = StreamMuxConfig::from_bits(reader)?;
        let mut mux_slot_length_bytes = 0usize;
        loop {
            let value = reader.read(8)? as usize;
            mux_slot_length_bytes = mux_slot_length_bytes
                .checked_add(value)
                .ok_or(Error::ValueTooLarge)?;
            if value != 255 {
                break;
            }
        }
        let payload = reader.read_bytes(mux_slot_length_bytes)?;
        if stream_mux_config.other_data_present {
            reader.skip(stream_mux_config.other_data_len_bits)?;
        }
        while !reader.bit_position().is_multiple_of(8) {
            reader.read(1)?;
        }
        Ok(Self {
            stream_mux_config,
            payload,
        })
    }

    fn to_bits(&self, writer: &mut BitWriter) -> Result<()> {
        writer.write(0, 1)?;
        self.stream_mux_config.to_bits(writer)?;
        let mut length = self.payload.len();
        while length > 255 {
            writer.write(255, 8)?;
            length -= 255;
        }
        writer.write(length as u32, 8)?;
        if length == 255 {
            writer.write(0, 8)?;
        }
        writer.write_bytes(&self.payload)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AacAudioRtpPacket {
    pub audio_mux_element: AudioMuxElement,
}

impl AacAudioRtpPacket {
    pub fn from_bytes(data: &[u8]) -> Result<Self> {
        let mut reader = BitReader::new(data);
        Ok(Self {
            audio_mux_element: AudioMuxElement::from_bits(&mut reader)?,
        })
    }

    pub fn for_simple_aac(
        sampling_frequency: u32,
        channel_configuration: u8,
        payload: Vec<u8>,
    ) -> Result<Self> {
        let audio_specific_config =
            AudioSpecificConfig::for_simple_aac(2, sampling_frequency, channel_configuration)?;
        Ok(Self {
            audio_mux_element: AudioMuxElement {
                stream_mux_config: StreamMuxConfig {
                    other_data_present: false,
                    other_data_len_bits: 0,
                    audio_specific_config,
                },
                payload,
            },
        })
    }

    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        let mut writer = BitWriter::new();
        self.audio_mux_element.to_bits(&mut writer)?;
        Ok(writer.into_bytes())
    }

    pub fn to_adts(&self) -> Result<Vec<u8>> {
        let config = &self
            .audio_mux_element
            .stream_mux_config
            .audio_specific_config;
        let frame_size = self
            .audio_mux_element
            .payload
            .len()
            .checked_add(7)
            .ok_or(Error::ValueTooLarge)?;
        if frame_size > 0x1FFF {
            return Err(Error::ValueTooLarge);
        }
        let channel = config.channel_configuration;
        let mut bytes = vec![
            0xFF,
            0xF1,
            0x40 | (config.sampling_frequency_index << 2) | (channel >> 2),
            ((channel & 3) << 6) | ((frame_size >> 11) as u8),
            ((frame_size >> 3) & 0xFF) as u8,
            (((frame_size << 5) & 0xFF) as u8) | 0x1F,
            0xFC,
        ];
        bytes.extend_from_slice(&self.audio_mux_element.payload);
        Ok(bytes)
    }
}
