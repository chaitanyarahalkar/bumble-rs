//! Owned, thread-backed LC3 encoder and decoder.
//!
//! The underlying pure-Rust codec borrows its working storage. Keeping the
//! codec and that storage together on a worker thread gives callers a safe,
//! owned interface while preserving codec state between SDUs.

use lc3_codec::common::{
    complex::Complex,
    config::{FrameDuration, Lc3Config, SamplingFrequency},
};
use lc3_codec::decoder::lc3_decoder::Lc3Decoder as CodecDecoder;
use lc3_codec::encoder::lc3_encoder::Lc3Encoder as CodecEncoder;
use std::fmt;
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::thread::{self, JoinHandle};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lc3FrameDuration {
    SevenPointFiveMs,
    TenMs,
}

impl Lc3FrameDuration {
    pub const fn microseconds(self) -> u32 {
        match self {
            Self::SevenPointFiveMs => 7_500,
            Self::TenMs => 10_000,
        }
    }

    const fn codec(self) -> FrameDuration {
        match self {
            Self::SevenPointFiveMs => FrameDuration::SevenPointFiveMs,
            Self::TenMs => FrameDuration::TenMs,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Lc3StreamConfig {
    pub sampling_frequency: u32,
    pub frame_duration: Lc3FrameDuration,
    pub channels: usize,
    pub octets_per_codec_frame: usize,
    pub codec_frames_per_sdu: usize,
}

impl Lc3StreamConfig {
    pub fn validate(self) -> Result<Self> {
        self.codec_sampling_frequency()?;
        if self.channels == 0 {
            return Err(Lc3Error::InvalidConfiguration(
                "channel count must be nonzero",
            ));
        }
        if self.octets_per_codec_frame == 0 {
            return Err(Lc3Error::InvalidConfiguration(
                "octets per codec frame must be nonzero",
            ));
        }
        if self.codec_frames_per_sdu == 0 {
            return Err(Lc3Error::InvalidConfiguration(
                "codec frames per SDU must be nonzero",
            ));
        }
        self.frame_samples()
            .checked_mul(self.channels)
            .and_then(|value| value.checked_mul(self.codec_frames_per_sdu))
            .ok_or(Lc3Error::ValueTooLarge)?
            .checked_mul(std::mem::size_of::<i16>())
            .ok_or(Lc3Error::ValueTooLarge)?;
        self.octets_per_codec_frame
            .checked_mul(self.channels)
            .and_then(|value| value.checked_mul(self.codec_frames_per_sdu))
            .ok_or(Lc3Error::ValueTooLarge)?;
        Ok(self)
    }

    pub fn frame_samples(self) -> usize {
        Lc3Config::new(
            self.codec_sampling_frequency_unchecked(),
            self.frame_duration.codec(),
        )
        .nf
    }

    pub fn pcm_samples_per_sdu(self) -> usize {
        self.frame_samples()
            .checked_mul(self.channels)
            .and_then(|value| value.checked_mul(self.codec_frames_per_sdu))
            .unwrap_or(usize::MAX)
    }

    pub fn encoded_sdu_len(self) -> usize {
        self.octets_per_codec_frame
            .checked_mul(self.channels)
            .and_then(|value| value.checked_mul(self.codec_frames_per_sdu))
            .unwrap_or(usize::MAX)
    }

    fn codec_sampling_frequency(self) -> Result<SamplingFrequency> {
        match self.sampling_frequency {
            8_000 => Ok(SamplingFrequency::Hz8000),
            16_000 => Ok(SamplingFrequency::Hz16000),
            24_000 => Ok(SamplingFrequency::Hz24000),
            32_000 => Ok(SamplingFrequency::Hz32000),
            44_100 => Ok(SamplingFrequency::Hz44100),
            48_000 => Ok(SamplingFrequency::Hz48000),
            _ => Err(Lc3Error::InvalidConfiguration(
                "unsupported LC3 sampling frequency",
            )),
        }
    }

    fn codec_sampling_frequency_unchecked(self) -> SamplingFrequency {
        self.codec_sampling_frequency()
            .expect("validated LC3 sampling frequency")
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Lc3Error {
    InvalidConfiguration(&'static str),
    InvalidPcmLength { expected: usize, actual: usize },
    InvalidSduLength { expected: usize, actual: usize },
    Codec(String),
    WorkerClosed,
    WorkerPanicked,
    ValueTooLarge,
}

impl fmt::Display for Lc3Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidConfiguration(message) => {
                write!(formatter, "invalid LC3 configuration: {message}")
            }
            Self::InvalidPcmLength { expected, actual } => write!(
                formatter,
                "LC3 PCM input has {actual} samples, expected {expected}"
            ),
            Self::InvalidSduLength { expected, actual } => write!(
                formatter,
                "LC3 SDU has {actual} octets, expected {expected}"
            ),
            Self::Codec(message) => write!(formatter, "LC3 codec error: {message}"),
            Self::WorkerClosed => formatter.write_str("LC3 worker closed"),
            Self::WorkerPanicked => formatter.write_str("LC3 worker panicked"),
            Self::ValueTooLarge => formatter.write_str("LC3 buffer size is too large"),
        }
    }
}

impl std::error::Error for Lc3Error {}

pub type Result<T> = std::result::Result<T, Lc3Error>;

type Response<T> = SyncSender<Result<T>>;

enum EncoderRequest {
    Encode(Vec<i16>, Response<Vec<u8>>),
    Stop,
}

pub struct Lc3Encoder {
    config: Lc3StreamConfig,
    sender: SyncSender<EncoderRequest>,
    worker: Option<JoinHandle<()>>,
}

impl fmt::Debug for Lc3Encoder {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Lc3Encoder")
            .field("config", &self.config)
            .finish_non_exhaustive()
    }
}

impl Lc3Encoder {
    pub fn new(config: Lc3StreamConfig) -> Result<Self> {
        let config = config.validate()?;
        let (sender, receiver) = mpsc::sync_channel(1);
        let worker = thread::Builder::new()
            .name("bumble-lc3-encoder".into())
            .spawn(move || run_encoder(config, receiver))
            .map_err(|error| Lc3Error::Codec(error.to_string()))?;
        Ok(Self {
            config,
            sender,
            worker: Some(worker),
        })
    }

    pub const fn config(&self) -> Lc3StreamConfig {
        self.config
    }

    /// Encode one SDU from interleaved signed 16-bit PCM samples.
    pub fn encode_sdu(&self, samples: &[i16]) -> Result<Vec<u8>> {
        let expected = self.config.pcm_samples_per_sdu();
        if samples.len() != expected {
            return Err(Lc3Error::InvalidPcmLength {
                expected,
                actual: samples.len(),
            });
        }
        let (sender, receiver) = mpsc::sync_channel(0);
        self.sender
            .send(EncoderRequest::Encode(samples.to_vec(), sender))
            .map_err(|_| Lc3Error::WorkerClosed)?;
        receiver.recv().map_err(|_| Lc3Error::WorkerClosed)?
    }
}

impl Drop for Lc3Encoder {
    fn drop(&mut self) {
        let _ = self.sender.send(EncoderRequest::Stop);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn run_encoder(config: Lc3StreamConfig, receiver: Receiver<EncoderRequest>) {
    let frame_duration = config.frame_duration.codec();
    let sampling_frequency = config.codec_sampling_frequency_unchecked();
    let (integer_len, scaler_len, complex_len) = CodecEncoder::calc_working_buffer_lengths(
        config.channels,
        frame_duration,
        sampling_frequency,
    );
    let mut integer = vec![0; integer_len];
    let mut scaler = vec![0.0; scaler_len];
    let mut complex = vec![Complex::default(); complex_len];
    let mut encoder = CodecEncoder::new(
        config.channels,
        frame_duration,
        sampling_frequency,
        &mut integer,
        &mut scaler,
        &mut complex,
    );
    while let Ok(request) = receiver.recv() {
        match request {
            EncoderRequest::Encode(samples, response) => {
                let _ = response.send(encode_sdu(&mut encoder, config, &samples));
            }
            EncoderRequest::Stop => break,
        }
    }
}

fn encode_sdu(
    encoder: &mut CodecEncoder<'_>,
    config: Lc3StreamConfig,
    samples: &[i16],
) -> Result<Vec<u8>> {
    let frame_samples = config.frame_samples();
    let mut encoded = vec![0; config.encoded_sdu_len()];
    let mut channel_samples = vec![0; frame_samples];
    for frame in 0..config.codec_frames_per_sdu {
        for channel in 0..config.channels {
            let frame_start = frame * frame_samples * config.channels;
            for (sample_index, sample) in channel_samples.iter_mut().enumerate() {
                *sample = samples[frame_start + sample_index * config.channels + channel];
            }
            let encoded_start = (frame * config.channels + channel) * config.octets_per_codec_frame;
            encoder
                .encode_frame(
                    channel,
                    &channel_samples,
                    &mut encoded[encoded_start..encoded_start + config.octets_per_codec_frame],
                )
                .map_err(|error| Lc3Error::Codec(format!("{error:?}")))?;
        }
    }
    Ok(encoded)
}

enum DecoderRequest {
    Decode(Vec<u8>, Response<Vec<i16>>),
    Stop,
}

pub struct Lc3Decoder {
    config: Lc3StreamConfig,
    sender: SyncSender<DecoderRequest>,
    worker: Option<JoinHandle<()>>,
}

impl fmt::Debug for Lc3Decoder {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Lc3Decoder")
            .field("config", &self.config)
            .finish_non_exhaustive()
    }
}

impl Lc3Decoder {
    pub fn new(config: Lc3StreamConfig) -> Result<Self> {
        let config = config.validate()?;
        let (sender, receiver) = mpsc::sync_channel(1);
        let worker = thread::Builder::new()
            .name("bumble-lc3-decoder".into())
            .spawn(move || run_decoder(config, receiver))
            .map_err(|error| Lc3Error::Codec(error.to_string()))?;
        Ok(Self {
            config,
            sender,
            worker: Some(worker),
        })
    }

    pub const fn config(&self) -> Lc3StreamConfig {
        self.config
    }

    /// Decode one LC3 SDU into interleaved signed 16-bit PCM samples.
    pub fn decode_sdu(&self, sdu: &[u8]) -> Result<Vec<i16>> {
        let expected = self.config.encoded_sdu_len();
        if sdu.len() != expected {
            return Err(Lc3Error::InvalidSduLength {
                expected,
                actual: sdu.len(),
            });
        }
        let (sender, receiver) = mpsc::sync_channel(0);
        self.sender
            .send(DecoderRequest::Decode(sdu.to_vec(), sender))
            .map_err(|_| Lc3Error::WorkerClosed)?;
        receiver.recv().map_err(|_| Lc3Error::WorkerClosed)?
    }
}

impl Drop for Lc3Decoder {
    fn drop(&mut self) {
        let _ = self.sender.send(DecoderRequest::Stop);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn run_decoder(config: Lc3StreamConfig, receiver: Receiver<DecoderRequest>) {
    let frame_duration = config.frame_duration.codec();
    let sampling_frequency = config.codec_sampling_frequency_unchecked();
    let (scaler_len, complex_len) = CodecDecoder::calc_working_buffer_lengths(
        config.channels,
        frame_duration,
        sampling_frequency,
    );
    let mut scaler = vec![0.0; scaler_len];
    let mut complex = vec![Complex::default(); complex_len];
    let mut decoder = CodecDecoder::new(
        config.channels,
        frame_duration,
        sampling_frequency,
        &mut scaler,
        &mut complex,
    );
    while let Ok(request) = receiver.recv() {
        match request {
            DecoderRequest::Decode(sdu, response) => {
                let _ = response.send(decode_sdu(&mut decoder, config, &sdu));
            }
            DecoderRequest::Stop => break,
        }
    }
}

fn decode_sdu(
    decoder: &mut CodecDecoder<'_>,
    config: Lc3StreamConfig,
    sdu: &[u8],
) -> Result<Vec<i16>> {
    let frame_samples = config.frame_samples();
    let mut samples = vec![0; config.pcm_samples_per_sdu()];
    let mut channel_samples = vec![0; frame_samples];
    for frame in 0..config.codec_frames_per_sdu {
        for channel in 0..config.channels {
            let encoded_start = (frame * config.channels + channel) * config.octets_per_codec_frame;
            decoder
                .decode_frame(
                    16,
                    channel,
                    &sdu[encoded_start..encoded_start + config.octets_per_codec_frame],
                    &mut channel_samples,
                )
                .map_err(|error| Lc3Error::Codec(format!("{error:?}")))?;
            let frame_start = frame * frame_samples * config.channels;
            for (sample_index, sample) in channel_samples.iter().enumerate() {
                samples[frame_start + sample_index * config.channels + channel] = *sample;
            }
        }
    }
    Ok(samples)
}
