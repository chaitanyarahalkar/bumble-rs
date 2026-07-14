//! Portable PCM audio input and output.
//!
//! This crate ports `bumble.audio.io`: PCM format parsing, non-blocking stream
//! and file output, subprocess output, raw stream and file input, and looping
//! 16-bit WAVE input. The optional `sound-device` feature adds live device
//! enumeration, float32 output, and int16 input through CPAL.

use std::fmt;
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::str::FromStr;
use std::sync::mpsc::{self, Sender};
use std::thread::{self, JoinHandle};

#[cfg(feature = "sound-device")]
mod sound_device;

#[cfg(feature = "sound-device")]
pub use sound_device::{SoundDeviceAudioInput, SoundDeviceAudioOutput};

/// Byte ordering of PCM samples.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Endianness {
    Little,
    Big,
}

/// Representation of one PCM sample.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SampleType {
    Float32,
    Int16,
}

/// PCM stream description.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PcmFormat {
    pub endianness: Endianness,
    pub sample_type: SampleType,
    pub sample_rate: u32,
    pub channels: u16,
}

impl PcmFormat {
    pub const fn new(
        endianness: Endianness,
        sample_type: SampleType,
        sample_rate: u32,
        channels: u16,
    ) -> Self {
        Self {
            endianness,
            sample_type,
            sample_rate,
            channels,
        }
    }

    pub const fn bytes_per_sample(self) -> usize {
        match self.sample_type {
            SampleType::Int16 => 2,
            SampleType::Float32 => 4,
        }
    }

    pub fn bytes_per_frame(self) -> Result<usize> {
        usize::from(self.channels)
            .checked_mul(self.bytes_per_sample())
            .ok_or(Error::ValueTooLarge)
    }

    fn validate(self) -> Result<()> {
        if self.sample_rate == 0 {
            return Err(Error::InvalidFormat("sample rate must be nonzero".into()));
        }
        if self.channels == 0 {
            return Err(Error::InvalidFormat("channel count must be nonzero".into()));
        }
        Ok(())
    }
}

impl FromStr for PcmFormat {
    type Err = Error;

    fn from_str(value: &str) -> Result<Self> {
        let mut fields = value.split(',');
        let sample_type = match fields.next() {
            Some("int16le") => SampleType::Int16,
            Some("float32le") => SampleType::Float32,
            Some(other) => {
                return Err(Error::InvalidFormat(format!(
                    "sample type {other} not supported"
                )))
            }
            None => return Err(Error::InvalidFormat("missing sample type".into())),
        };
        let sample_rate = fields
            .next()
            .ok_or_else(|| Error::InvalidFormat("missing sample rate".into()))?
            .parse()
            .map_err(|_| Error::InvalidFormat("invalid sample rate".into()))?;
        let channels = fields
            .next()
            .ok_or_else(|| Error::InvalidFormat("missing channel count".into()))?
            .parse()
            .map_err(|_| Error::InvalidFormat("invalid channel count".into()))?;
        if fields.next().is_some() {
            return Err(Error::InvalidFormat("too many PCM format fields".into()));
        }
        let format = Self::new(Endianness::Little, sample_type, sample_rate, channels);
        format.validate()?;
        Ok(format)
    }
}

impl fmt::Display for PcmFormat {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let sample_type = match (self.sample_type, self.endianness) {
            (SampleType::Int16, Endianness::Little) => "int16le",
            (SampleType::Float32, Endianness::Little) => "float32le",
            (SampleType::Int16, Endianness::Big) => "int16be",
            (SampleType::Float32, Endianness::Big) => "float32be",
        };
        write!(
            formatter,
            "{sample_type},{},{}",
            self.sample_rate, self.channels
        )
    }
}

#[derive(Debug)]
pub enum Error {
    Io(io::Error),
    Backend(String),
    InvalidFormat(String),
    Unsupported(String),
    NotOpen,
    Closed,
    ValueTooLarge,
    WorkerPanicked,
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "audio I/O error: {error}"),
            Self::Backend(message) => write!(formatter, "audio backend error: {message}"),
            Self::InvalidFormat(message) => write!(formatter, "invalid audio format: {message}"),
            Self::Unsupported(message) => write!(formatter, "unsupported audio I/O: {message}"),
            Self::NotOpen => formatter.write_str("audio input or output is not open"),
            Self::Closed => formatter.write_str("audio input or output is closed"),
            Self::ValueTooLarge => formatter.write_str("audio size is too large"),
            Self::WorkerPanicked => formatter.write_str("audio output worker panicked"),
        }
    }
}

/// One live audio device exposed by the optional platform backend.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AudioDeviceInfo {
    /// Stable backend identifier when the platform supplies one.
    pub id: String,
    /// Index accepted by Bumble's `device:INDEX` syntax.
    pub index: usize,
    /// Human-readable platform name.
    pub name: String,
    /// Maximum channel count advertised for the selected direction.
    pub max_channels: u16,
    /// Whether this is the platform's default device for that direction.
    pub is_default: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AudioDeviceSelector {
    Default,
    List,
    Index(usize),
}

fn parse_audio_device_selector(specification: &str) -> Result<AudioDeviceSelector> {
    if specification == "device" {
        return Ok(AudioDeviceSelector::Default);
    }
    let selector = specification
        .strip_prefix("device:")
        .ok_or_else(|| Error::Unsupported("audio device specification".into()))?;
    if selector == "?" {
        return Ok(AudioDeviceSelector::List);
    }
    selector
        .parse()
        .map(AudioDeviceSelector::Index)
        .map_err(|_| Error::InvalidFormat("audio device index must be an integer".into()))
}

/// Enumerate devices with at least one output channel.
pub fn list_audio_output_devices() -> Result<Vec<AudioDeviceInfo>> {
    #[cfg(feature = "sound-device")]
    {
        sound_device::list_output_devices()
    }
    #[cfg(not(feature = "sound-device"))]
    Err(Error::Unsupported(
        "sound-device output requires the 'sound-device' feature".into(),
    ))
}

/// Enumerate devices with at least one input channel.
pub fn list_audio_input_devices() -> Result<Vec<AudioDeviceInfo>> {
    #[cfg(feature = "sound-device")]
    {
        sound_device::list_input_devices()
    }
    #[cfg(not(feature = "sound-device"))]
    Err(Error::Unsupported(
        "sound-device input requires the 'sound-device' feature".into(),
    ))
}

/// Validate Bumble's output syntax and print output devices for `device:?`.
///
/// The return value matches upstream: listing devices returns `false`; a usable
/// specification returns `true`.
pub fn check_audio_output(specification: &str) -> Result<bool> {
    if specification != "device" && !specification.starts_with("device:") {
        return Ok(true);
    }
    let selector = parse_audio_device_selector(specification)?;
    #[cfg(feature = "sound-device")]
    {
        sound_device::check_output(selector)
    }
    #[cfg(not(feature = "sound-device"))]
    {
        let _ = selector;
        Err(Error::Unsupported(
            "sound-device output requires the 'sound-device' feature".into(),
        ))
    }
}

/// Validate Bumble's input syntax and print input devices for `device:?`.
///
/// The return value matches upstream: listing devices returns `false`; a usable
/// specification returns `true`.
pub fn check_audio_input(specification: &str) -> Result<bool> {
    if specification != "device" && !specification.starts_with("device:") {
        return Ok(true);
    }
    let selector = parse_audio_device_selector(specification)?;
    #[cfg(feature = "sound-device")]
    {
        sound_device::check_input(selector)
    }
    #[cfg(not(feature = "sound-device"))]
    {
        let _ = selector;
        Err(Error::Unsupported(
            "sound-device input requires the 'sound-device' feature".into(),
        ))
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            _ => None,
        }
    }
}

impl From<io::Error> for Error {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

pub type Result<T> = std::result::Result<T, Error>;

/// A destination for PCM samples.
pub trait AudioOutput: Send {
    fn open(&mut self, pcm_format: PcmFormat) -> Result<()>;
    fn write(&mut self, pcm_samples: &[u8]) -> Result<()>;
    fn close(&mut self) -> Result<()>;
}

enum WriterMessage {
    Samples(Vec<u8>),
    Close,
}

/// PCM output backed by a dedicated writer thread.
///
/// `write` only copies samples into an unbounded queue; blocking writes and
/// flushes happen on the worker. `close` drains the queue and reports any
/// writer error.
pub struct StreamAudioOutput {
    sender: Option<Sender<WriterMessage>>,
    worker: Option<JoinHandle<io::Result<()>>>,
}

impl fmt::Debug for StreamAudioOutput {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("StreamAudioOutput")
            .field("closed", &self.sender.is_none())
            .finish()
    }
}

impl StreamAudioOutput {
    pub fn new<W>(mut writer: W) -> Self
    where
        W: Write + Send + 'static,
    {
        let (sender, receiver) = mpsc::channel();
        let worker = thread::spawn(move || {
            while let Ok(message) = receiver.recv() {
                match message {
                    WriterMessage::Samples(samples) => {
                        writer.write_all(&samples)?;
                        writer.flush()?;
                    }
                    WriterMessage::Close => break,
                }
            }
            writer.flush()
        });
        Self {
            sender: Some(sender),
            worker: Some(worker),
        }
    }
}

impl AudioOutput for StreamAudioOutput {
    fn open(&mut self, pcm_format: PcmFormat) -> Result<()> {
        pcm_format.validate()
    }

    fn write(&mut self, pcm_samples: &[u8]) -> Result<()> {
        self.sender
            .as_ref()
            .ok_or(Error::Closed)?
            .send(WriterMessage::Samples(pcm_samples.to_vec()))
            .map_err(|_| Error::Closed)
    }

    fn close(&mut self) -> Result<()> {
        if let Some(sender) = self.sender.take() {
            let _ = sender.send(WriterMessage::Close);
        }
        if let Some(worker) = self.worker.take() {
            worker.join().map_err(|_| Error::WorkerPanicked)??;
        }
        Ok(())
    }
}

impl Drop for StreamAudioOutput {
    fn drop(&mut self) {
        let _ = self.close();
    }
}

/// Non-blocking PCM output to a raw file.
#[derive(Debug)]
pub struct FileAudioOutput(StreamAudioOutput);

impl FileAudioOutput {
    pub fn create(path: impl AsRef<Path>) -> Result<Self> {
        Ok(Self(StreamAudioOutput::new(File::create(path)?)))
    }
}

impl AudioOutput for FileAudioOutput {
    fn open(&mut self, pcm_format: PcmFormat) -> Result<()> {
        self.0.open(pcm_format)
    }

    fn write(&mut self, pcm_samples: &[u8]) -> Result<()> {
        self.0.write(pcm_samples)
    }

    fn close(&mut self) -> Result<()> {
        self.0.close()
    }
}

/// PCM output to the standard input of a subprocess.
#[derive(Debug)]
pub struct SubprocessAudioOutput {
    command: String,
    child: Option<Child>,
    input: Option<StreamAudioOutput>,
}

impl SubprocessAudioOutput {
    pub fn new(command: impl Into<String>) -> Self {
        Self {
            command: command.into(),
            child: None,
            input: None,
        }
    }

    fn expanded_command(&self, pcm_format: PcmFormat) -> Result<String> {
        let channel_layout = match pcm_format.channels {
            1 => "mono",
            2 => "stereo",
            channels => {
                return Err(Error::Unsupported(format!(
                    "{channels} channels are not supported by subprocess output"
                )))
            }
        };
        Ok(self
            .command
            .replace("{sample_rate}", &pcm_format.sample_rate.to_string())
            .replace("{channel_layout}", channel_layout))
    }
}

impl AudioOutput for SubprocessAudioOutput {
    fn open(&mut self, pcm_format: PcmFormat) -> Result<()> {
        pcm_format.validate()?;
        if self.child.is_some() {
            return Err(Error::InvalidFormat(
                "subprocess output is already open".into(),
            ));
        }
        let command = self.expanded_command(pcm_format)?;
        #[cfg(unix)]
        let mut child = Command::new("sh")
            .args(["-c", &command])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;
        #[cfg(windows)]
        let mut child = Command::new("cmd")
            .args(["/C", &command])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;
        let input = child.stdin.take().ok_or_else(|| {
            Error::Io(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "subprocess stdin unavailable",
            ))
        })?;
        self.input = Some(StreamAudioOutput::new(input));
        self.child = Some(child);
        Ok(())
    }

    fn write(&mut self, pcm_samples: &[u8]) -> Result<()> {
        self.input
            .as_mut()
            .ok_or(Error::NotOpen)?
            .write(pcm_samples)
    }

    fn close(&mut self) -> Result<()> {
        if let Some(mut input) = self.input.take() {
            input.close()?;
        }
        if let Some(mut child) = self.child.take() {
            child.wait()?;
        }
        Ok(())
    }
}

impl Drop for SubprocessAudioOutput {
    fn drop(&mut self) {
        let _ = self.close();
    }
}

/// Create one of the portable output implementations from Bumble's output
/// syntax (`stdout`, `ffplay`, or `file:PATH`).
pub fn create_audio_output(specification: &str) -> Result<Box<dyn AudioOutput>> {
    if specification == "stdout" {
        return Ok(Box::new(StreamAudioOutput::new(io::stdout())));
    }
    if specification == "ffplay" {
        return Ok(Box::new(SubprocessAudioOutput::new(
            "ffplay -probesize 32 -fflags nobuffer -analyzeduration 0 \
             -ar {sample_rate} -ch_layout {channel_layout} -f f32le pipe:0",
        )));
    }
    if let Some(path) = specification.strip_prefix("file:") {
        return Ok(Box::new(FileAudioOutput::create(path)?));
    }
    if specification == "device" || specification.starts_with("device:") {
        let selector = parse_audio_device_selector(specification)?;
        let device_index = match selector {
            AudioDeviceSelector::Default => None,
            AudioDeviceSelector::Index(index) => Some(index),
            AudioDeviceSelector::List => {
                return Err(Error::InvalidFormat(
                    "device:? lists outputs and cannot be opened".into(),
                ))
            }
        };
        #[cfg(feature = "sound-device")]
        {
            return Ok(Box::new(SoundDeviceAudioOutput::new(device_index)));
        }
        #[cfg(not(feature = "sound-device"))]
        {
            let _ = device_index;
            return Err(Error::Unsupported(
                "sound-device output requires the 'sound-device' feature".into(),
            ));
        }
    }
    Err(Error::Unsupported("audio output specification".into()))
}

/// A source of PCM samples.
pub trait AudioInput: Send {
    fn open(&mut self) -> Result<PcmFormat>;
    fn read_frame(&mut self, frame_size: usize) -> Result<Option<Vec<u8>>>;
    fn close(&mut self) -> Result<()>;
}

/// PCM input read from a raw byte stream.
pub struct StreamAudioInput {
    stream: Box<dyn Read + Send>,
    pcm_format: PcmFormat,
}

impl fmt::Debug for StreamAudioInput {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("StreamAudioInput")
            .field("pcm_format", &self.pcm_format)
            .finish_non_exhaustive()
    }
}

impl StreamAudioInput {
    pub fn new<R>(stream: R, pcm_format: PcmFormat) -> Self
    where
        R: Read + Send + 'static,
    {
        Self {
            stream: Box::new(stream),
            pcm_format,
        }
    }
}

impl AudioInput for StreamAudioInput {
    fn open(&mut self) -> Result<PcmFormat> {
        self.pcm_format.validate()?;
        Ok(self.pcm_format)
    }

    fn read_frame(&mut self, frame_size: usize) -> Result<Option<Vec<u8>>> {
        if frame_size == 0 {
            return Err(Error::InvalidFormat("frame size must be nonzero".into()));
        }
        let byte_count = frame_size
            .checked_mul(self.pcm_format.bytes_per_frame()?)
            .ok_or(Error::ValueTooLarge)?;
        let mut samples = vec![0; byte_count];
        let bytes_read = self.stream.read(&mut samples)?;
        if bytes_read == 0 {
            return Ok(None);
        }
        samples.truncate(bytes_read);
        Ok(Some(samples))
    }

    fn close(&mut self) -> Result<()> {
        Ok(())
    }
}

/// PCM input read from a raw file.
#[derive(Debug)]
pub struct FileAudioInput(StreamAudioInput);

impl FileAudioInput {
    pub fn open_file(path: impl AsRef<Path>, pcm_format: PcmFormat) -> Result<Self> {
        Ok(Self(StreamAudioInput::new(File::open(path)?, pcm_format)))
    }
}

impl AudioInput for FileAudioInput {
    fn open(&mut self) -> Result<PcmFormat> {
        self.0.open()
    }

    fn read_frame(&mut self, frame_size: usize) -> Result<Option<Vec<u8>>> {
        self.0.read_frame(frame_size)
    }

    fn close(&mut self) -> Result<()> {
        self.0.close()
    }
}

#[derive(Debug)]
struct WaveState {
    file: File,
    format: PcmFormat,
    data_offset: u64,
    data_length: u64,
    position: u64,
    bytes_read: u64,
}

/// Looping 16-bit PCM input from a RIFF/WAVE file.
#[derive(Debug)]
pub struct WaveAudioInput {
    path: PathBuf,
    state: Option<WaveState>,
}

impl WaveAudioInput {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            state: None,
        }
    }

    fn parse(mut file: File) -> Result<WaveState> {
        let mut riff = [0; 12];
        file.read_exact(&mut riff)?;
        if &riff[..4] != b"RIFF" || &riff[8..] != b"WAVE" {
            return Err(Error::InvalidFormat("not a RIFF/WAVE file".into()));
        }

        let mut format = None;
        let mut data = None;
        loop {
            let mut header = [0; 8];
            match file.read_exact(&mut header) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => break,
                Err(error) => return Err(error.into()),
            }
            let chunk_length = u64::from(u32::from_le_bytes([
                header[4], header[5], header[6], header[7],
            ]));
            let chunk_start = file.stream_position()?;
            match &header[..4] {
                b"fmt " => {
                    if chunk_length < 16 {
                        return Err(Error::InvalidFormat("truncated WAVE fmt chunk".into()));
                    }
                    let mut fmt = [0; 16];
                    file.read_exact(&mut fmt)?;
                    let encoding = u16::from_le_bytes([fmt[0], fmt[1]]);
                    let channels = u16::from_le_bytes([fmt[2], fmt[3]]);
                    let sample_rate = u32::from_le_bytes([fmt[4], fmt[5], fmt[6], fmt[7]]);
                    let block_align = u16::from_le_bytes([fmt[12], fmt[13]]);
                    let sample_bits = u16::from_le_bytes([fmt[14], fmt[15]]);
                    if encoding != 1 {
                        return Err(Error::Unsupported("compressed WAVE input".into()));
                    }
                    if sample_bits != 16 {
                        return Err(Error::Unsupported("WAVE sample width".into()));
                    }
                    let pcm = PcmFormat::new(
                        Endianness::Little,
                        SampleType::Int16,
                        sample_rate,
                        channels,
                    );
                    pcm.validate()?;
                    if usize::from(block_align) != pcm.bytes_per_frame()? {
                        return Err(Error::InvalidFormat("invalid WAVE block alignment".into()));
                    }
                    format = Some(pcm);
                }
                b"data" => data = Some((chunk_start, chunk_length)),
                _ => {}
            }
            let next = chunk_start
                .checked_add(chunk_length)
                .and_then(|position| position.checked_add(chunk_length & 1))
                .ok_or(Error::ValueTooLarge)?;
            file.seek(SeekFrom::Start(next))?;
            if format.is_some() && data.is_some() {
                break;
            }
        }

        let format = format.ok_or_else(|| Error::InvalidFormat("missing WAVE fmt chunk".into()))?;
        let (data_offset, data_length) =
            data.ok_or_else(|| Error::InvalidFormat("missing WAVE data chunk".into()))?;
        file.seek(SeekFrom::Start(data_offset))?;
        Ok(WaveState {
            file,
            format,
            data_offset,
            data_length,
            position: 0,
            bytes_read: 0,
        })
    }
}

impl AudioInput for WaveAudioInput {
    fn open(&mut self) -> Result<PcmFormat> {
        let state = Self::parse(File::open(&self.path)?)?;
        let format = state.format;
        self.state = Some(state);
        Ok(format)
    }

    fn read_frame(&mut self, frame_size: usize) -> Result<Option<Vec<u8>>> {
        if frame_size == 0 {
            return Err(Error::InvalidFormat("frame size must be nonzero".into()));
        }
        let state = self.state.as_mut().ok_or(Error::NotOpen)?;
        if state.position == state.data_length {
            if state.bytes_read == 0 {
                return Ok(None);
            }
            state.file.seek(SeekFrom::Start(state.data_offset))?;
            state.position = 0;
            state.bytes_read = 0;
        }
        let requested = frame_size
            .checked_mul(state.format.bytes_per_frame()?)
            .ok_or(Error::ValueTooLarge)?;
        let available = usize::try_from(state.data_length - state.position)
            .unwrap_or(usize::MAX)
            .min(requested);
        if available == 0 {
            return Ok(None);
        }
        let mut samples = vec![0; available];
        let bytes_read = state.file.read(&mut samples)?;
        if bytes_read == 0 {
            return Err(Error::InvalidFormat("truncated WAVE data chunk".into()));
        }
        samples.truncate(bytes_read);
        let bytes_read = bytes_read as u64;
        state.position += bytes_read;
        state.bytes_read += bytes_read;
        Ok(Some(samples))
    }

    fn close(&mut self) -> Result<()> {
        self.state = None;
        Ok(())
    }
}

/// Create a portable input from Bumble's input syntax. `format` is either
/// `auto` or a PCM format such as `int16le,48000,2`.
pub fn create_audio_input(specification: &str, format: &str) -> Result<Box<dyn AudioInput>> {
    let pcm_format = if format == "auto" {
        None
    } else {
        Some(format.parse()?)
    };

    if specification == "stdin" {
        let format = pcm_format.ok_or_else(|| {
            Error::InvalidFormat("input format details required for stdin".into())
        })?;
        return Ok(Box::new(StreamAudioInput::new(io::stdin(), format)));
    }
    if specification == "device" || specification.starts_with("device:") {
        let format = pcm_format.ok_or_else(|| {
            Error::InvalidFormat("input format details required for device".into())
        })?;
        let selector = parse_audio_device_selector(specification)?;
        let device_index = match selector {
            AudioDeviceSelector::Default => None,
            AudioDeviceSelector::Index(index) => Some(index),
            AudioDeviceSelector::List => {
                return Err(Error::InvalidFormat(
                    "device:? lists inputs and cannot be opened".into(),
                ))
            }
        };
        #[cfg(feature = "sound-device")]
        {
            return Ok(Box::new(SoundDeviceAudioInput::new(device_index, format)));
        }
        #[cfg(not(feature = "sound-device"))]
        {
            let _ = (device_index, format);
            return Err(Error::Unsupported(
                "sound-device input requires the 'sound-device' feature".into(),
            ));
        }
    }

    let mut path = specification.strip_prefix("file:").map(PathBuf::from);
    if path.is_none() && Path::new(specification).is_file() {
        path = Some(PathBuf::from(specification));
    }
    if let Some(path) = path {
        if path
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("wav"))
        {
            if pcm_format.is_some() {
                return Err(Error::InvalidFormat(
                    ".wav file only supported with 'auto' format".into(),
                ));
            }
            return Ok(Box::new(WaveAudioInput::new(path)));
        }
        let format = pcm_format.ok_or_else(|| {
            Error::InvalidFormat("input format details required for raw PCM files".into())
        })?;
        return Ok(Box::new(FileAudioInput::open_file(path, format)?));
    }
    Err(Error::Unsupported("audio input specification".into()))
}
