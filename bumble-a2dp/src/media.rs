//! A2DP media frame parsing and RTP packetization.

use bumble_rtp::MediaPacket;

use crate::{Error, Result};

pub const SBC_SYNC_WORD: u8 = 0x9C;
pub const SBC_MAX_FRAMES_IN_RTP_PAYLOAD: usize = 15;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SbcFrame {
    pub sampling_frequency: u32,
    pub block_count: u8,
    pub channel_mode: u8,
    pub allocation_method: u8,
    pub subband_count: u8,
    pub bitpool: u8,
    pub payload: Vec<u8>,
}

impl SbcFrame {
    pub fn sample_count(&self) -> u32 {
        u32::from(self.subband_count) * u32::from(self.block_count)
    }

    pub fn bitrate(&self) -> u32 {
        8 * (self.payload.len() as u32 * self.sampling_frequency / self.sample_count())
    }

    pub fn duration_seconds(&self) -> f64 {
        f64::from(self.sample_count()) / f64::from(self.sampling_frequency)
    }

    /// Parse exactly one SBC frame and return it with the number of bytes used.
    pub fn parse(data: &[u8]) -> Result<(Self, usize)> {
        if data.len() < 4 {
            return Err(Error::Truncated("SBC frame header"));
        }
        if data[0] != SBC_SYNC_WORD {
            return Err(Error::Invalid("SBC sync word"));
        }
        let sampling_frequency = [16_000, 32_000, 44_100, 48_000][usize::from(data[1] >> 6)];
        let block_count = 4 * (1 + ((data[1] >> 4) & 3));
        let channel_mode = (data[1] >> 2) & 3;
        let channels: u8 = if channel_mode == 0 { 1 } else { 2 };
        let allocation_method = (data[1] >> 1) & 1;
        let subband_count: u8 = if data[1] & 1 != 0 { 8 } else { 4 };
        let bitpool = data[2];
        let scale_factors = (4 * usize::from(subband_count) * usize::from(channels)).div_ceil(8);
        let audio_bits = if matches!(channel_mode, 0 | 1) {
            usize::from(block_count) * usize::from(channels) * usize::from(bitpool)
        } else {
            usize::from(channel_mode == 3) * usize::from(subband_count)
                + usize::from(block_count) * usize::from(bitpool)
        };
        let frame_length = 4 + scale_factors + audio_bits.div_ceil(8);
        let payload = data
            .get(..frame_length)
            .ok_or(Error::Truncated("SBC frame payload"))?
            .to_vec();
        Ok((
            Self {
                sampling_frequency,
                block_count,
                channel_mode,
                allocation_method,
                subband_count,
                bitpool,
                payload,
            },
            frame_length,
        ))
    }

    pub fn parse_stream(mut data: &[u8]) -> Result<Vec<Self>> {
        let mut frames = Vec::new();
        while !data.is_empty() {
            let (frame, length) = Self::parse(data)?;
            frames.push(frame);
            data = &data[length..];
        }
        Ok(frames)
    }
}

/// Aggregate complete SBC frames into RTP packets without frame fragmentation.
pub fn packetize_sbc(frames: &[SbcFrame], mtu: usize) -> Result<Vec<MediaPacket>> {
    let max_payload = mtu
        .checked_sub(13)
        .ok_or(Error::Invalid("MTU is too small for RTP/SBC"))?;
    let mut packets = Vec::new();
    let mut batch = Vec::<&SbcFrame>::new();
    let mut batch_size = 0usize;
    let mut sequence_number = 0u16;
    let mut sample_count = 0u32;

    let flush = |batch: &mut Vec<&SbcFrame>,
                 batch_size: &mut usize,
                 packets: &mut Vec<MediaPacket>,
                 sequence_number: &mut u16,
                 sample_count: &mut u32| {
        if batch.is_empty() {
            return;
        }
        let sampling_frequency = batch[0].sampling_frequency;
        let mut payload = Vec::with_capacity(1 + *batch_size);
        payload.push(batch.len() as u8);
        for frame in batch.iter() {
            payload.extend_from_slice(&frame.payload);
        }
        packets.push(MediaPacket::new(
            96,
            *sequence_number,
            *sample_count,
            0,
            payload,
        ));
        *sequence_number = sequence_number.wrapping_add(1);
        *sample_count =
            sample_count.wrapping_add(batch.iter().map(|frame| frame.sample_count()).sum::<u32>());
        debug_assert!(sampling_frequency != 0);
        batch.clear();
        *batch_size = 0;
    };

    for frame in frames {
        if frame.payload.len() > max_payload {
            return Err(Error::Invalid("SBC frame exceeds RTP payload MTU"));
        }
        if !batch.is_empty()
            && (batch_size + frame.payload.len() > max_payload
                || batch.len() == SBC_MAX_FRAMES_IN_RTP_PAYLOAD)
        {
            flush(
                &mut batch,
                &mut batch_size,
                &mut packets,
                &mut sequence_number,
                &mut sample_count,
            );
        }
        batch.push(frame);
        batch_size += frame.payload.len();
    }
    flush(
        &mut batch,
        &mut batch_size,
        &mut packets,
        &mut sequence_number,
        &mut sample_count,
    );
    Ok(packets)
}

const ADTS_AAC_SAMPLING_FREQUENCIES: [u32; 16] = [
    96_000, 88_200, 64_000, 48_000, 44_100, 32_000, 24_000, 22_050, 16_000, 12_000, 11_025, 8_000,
    7_350, 0, 0, 0,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AacProfile {
    Main,
    LowComplexity,
    ScalableSampleRate,
    LongTermPrediction,
}

impl AacProfile {
    fn from_bits(value: u8) -> Self {
        match value & 3 {
            0 => Self::Main,
            1 => Self::LowComplexity,
            2 => Self::ScalableSampleRate,
            _ => Self::LongTermPrediction,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AacFrame {
    pub profile: AacProfile,
    pub sampling_frequency: u32,
    pub channel_configuration: u8,
    /// Raw AAC access unit without the ADTS header.
    pub payload: Vec<u8>,
}

impl AacFrame {
    pub const SAMPLE_COUNT: u32 = 1024;

    pub fn duration_seconds(&self) -> f64 {
        f64::from(Self::SAMPLE_COUNT) / f64::from(self.sampling_frequency)
    }

    pub fn parse(data: &[u8]) -> Result<(Self, usize)> {
        if data.len() < 7 {
            return Err(Error::Truncated("ADTS header"));
        }
        if data[0] != 0xFF || data[1] >> 4 != 0x0F {
            return Err(Error::Invalid("ADTS sync word"));
        }
        if (data[1] >> 1) & 3 != 0 {
            return Err(Error::Invalid("ADTS layer"));
        }
        let frequency_index = usize::from((data[2] >> 2) & 0x0F);
        let sampling_frequency = ADTS_AAC_SAMPLING_FREQUENCIES[frequency_index];
        if sampling_frequency == 0 {
            return Err(Error::Invalid("ADTS sampling frequency"));
        }
        let channel_configuration = ((data[2] & 1) << 2) | (data[3] >> 6);
        let frame_length = (usize::from(data[3] & 3) << 11)
            | (usize::from(data[4]) << 3)
            | usize::from(data[5] >> 5);
        if frame_length < 7 {
            return Err(Error::Invalid("ADTS frame length"));
        }
        let frame = data
            .get(..frame_length)
            .ok_or(Error::Truncated("ADTS frame payload"))?;
        Ok((
            Self {
                profile: AacProfile::from_bits(data[2] >> 6),
                sampling_frequency,
                channel_configuration,
                payload: frame[7..].to_vec(),
            },
            frame_length,
        ))
    }

    pub fn parse_stream(mut data: &[u8]) -> Result<Vec<Self>> {
        let mut frames = Vec::new();
        while !data.is_empty() {
            let (frame, length) = Self::parse(data)?;
            frames.push(frame);
            data = &data[length..];
        }
        Ok(frames)
    }
}

#[derive(Default)]
struct BitWriter {
    bytes: Vec<u8>,
    bit_len: usize,
}

impl BitWriter {
    fn write(&mut self, value: u32, bit_count: usize) {
        for shift in (0..bit_count).rev() {
            if self.bit_len.is_multiple_of(8) {
                self.bytes.push(0);
            }
            let bit = ((value >> shift) & 1) as u8;
            let byte = self.bit_len / 8;
            self.bytes[byte] |= bit << (7 - self.bit_len % 8);
            self.bit_len += 1;
        }
    }

    fn write_bytes(&mut self, data: &[u8]) {
        for byte in data {
            self.write(u32::from(*byte), 8);
        }
    }
}

/// Build the LATM AudioMuxElement used by upstream's simple AAC RTP source.
pub fn simple_aac_latm(frame: &AacFrame) -> Result<Vec<u8>> {
    let frequency_index = ADTS_AAC_SAMPLING_FREQUENCIES
        .iter()
        .position(|frequency| *frequency == frame.sampling_frequency)
        .ok_or(Error::Invalid("AAC sampling frequency"))?;
    // Upstream's `for_simple_aac` always signals AAC-LC in LATM, even when
    // the source ADTS header advertises a different profile.
    let audio_object_type = 2;
    let mut writer = BitWriter::default();
    writer.write(0, 1); // useSameStreamMux
    writer.write(0, 1); // audioMuxVersion
    writer.write(1, 1); // allStreamsSameTimeFraming
    writer.write(0, 6); // numSubFrames
    writer.write(0, 4); // numProgram
    writer.write(0, 3); // numLayer
    writer.write(audio_object_type, 5);
    writer.write(frequency_index as u32, 4);
    writer.write(u32::from(frame.channel_configuration), 4);
    writer.write(0, 1); // frameLengthFlag
    writer.write(0, 1); // dependsOnCoreCoder
    writer.write(0, 1); // extensionFlag
    writer.write(0, 3); // frameLengthType
    writer.write(0, 8); // latmBufferFullness
    writer.write(0, 1); // otherDataPresent
    writer.write(0, 1); // crcCheckPresent
    let mut remaining = frame.payload.len();
    while remaining > 255 {
        writer.write(255, 8);
        remaining -= 255;
    }
    writer.write(remaining as u32, 8);
    if remaining == 255 {
        writer.write(0, 8);
    }
    writer.write_bytes(&frame.payload);
    Ok(writer.bytes)
}

pub fn packetize_aac(frames: &[AacFrame]) -> Result<Vec<MediaPacket>> {
    let mut packets = Vec::with_capacity(frames.len());
    let mut sequence_number = 0u16;
    let mut sample_count = 0u32;
    for frame in frames {
        packets.push(MediaPacket::new(
            96,
            sequence_number,
            sample_count,
            0,
            simple_aac_latm(frame)?,
        ));
        sequence_number = sequence_number.wrapping_add(1);
        sample_count = sample_count.wrapping_add(AacFrame::SAMPLE_COUNT);
    }
    Ok(packets)
}
