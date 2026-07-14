use bumble_audio::{
    check_audio_input, check_audio_output, create_audio_input, create_audio_output, AudioInput,
    AudioOutput, Endianness, Error, PcmFormat, SampleType, StreamAudioInput, StreamAudioOutput,
    SubprocessAudioOutput, WaveAudioInput,
};
use std::fs;
use std::io::{Cursor, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

static NEXT_FILE_ID: AtomicU64 = AtomicU64::new(0);

fn temporary_file(suffix: &str) -> PathBuf {
    let id = NEXT_FILE_ID.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("bumble-audio-{}-{id}.{suffix}", std::process::id()))
}

fn pcm_format() -> PcmFormat {
    PcmFormat::new(Endianness::Little, SampleType::Int16, 48_000, 2)
}

fn wave_file(channels: u16, sample_rate: u32, sample_bits: u16, samples: &[u8]) -> Vec<u8> {
    let mut chunks = Vec::new();
    chunks.extend_from_slice(b"fmt ");
    chunks.extend_from_slice(&16u32.to_le_bytes());
    chunks.extend_from_slice(&1u16.to_le_bytes());
    chunks.extend_from_slice(&channels.to_le_bytes());
    chunks.extend_from_slice(&sample_rate.to_le_bytes());
    let block_align = channels * (sample_bits / 8);
    chunks.extend_from_slice(&(sample_rate * u32::from(block_align)).to_le_bytes());
    chunks.extend_from_slice(&block_align.to_le_bytes());
    chunks.extend_from_slice(&sample_bits.to_le_bytes());
    chunks.extend_from_slice(b"JUNK");
    chunks.extend_from_slice(&3u32.to_le_bytes());
    chunks.extend_from_slice(&[1, 2, 3, 0]);
    chunks.extend_from_slice(b"data");
    chunks.extend_from_slice(&(samples.len() as u32).to_le_bytes());
    chunks.extend_from_slice(samples);
    if !samples.len().is_multiple_of(2) {
        chunks.push(0);
    }

    let mut wave = b"RIFF".to_vec();
    wave.extend_from_slice(&(4 + chunks.len() as u32).to_le_bytes());
    wave.extend_from_slice(b"WAVE");
    wave.extend_from_slice(&chunks);
    wave
}

#[test]
fn pcm_format_matches_upstream_string_contract() {
    let int16: PcmFormat = "int16le,48000,2".parse().unwrap();
    assert_eq!(int16, pcm_format());
    assert_eq!(int16.bytes_per_sample(), 2);
    assert_eq!(int16.bytes_per_frame().unwrap(), 4);
    assert_eq!(int16.to_string(), "int16le,48000,2");

    let float: PcmFormat = "float32le,44100,1".parse().unwrap();
    assert_eq!(float.sample_type, SampleType::Float32);
    assert_eq!(float.bytes_per_sample(), 4);
    assert!(matches!(
        "int24le,48000,2".parse::<PcmFormat>(),
        Err(Error::InvalidFormat(_))
    ));
    assert!("int16le,0,2".parse::<PcmFormat>().is_err());
    assert!("int16le,48000,2,extra".parse::<PcmFormat>().is_err());
}

#[test]
fn raw_stream_input_uses_frames_not_bytes() {
    let mut input = StreamAudioInput::new(Cursor::new(vec![0, 1, 2, 3, 4, 5]), pcm_format());
    assert_eq!(input.open().unwrap(), pcm_format());
    assert_eq!(input.read_frame(1).unwrap().unwrap(), vec![0, 1, 2, 3]);
    assert_eq!(input.read_frame(1).unwrap().unwrap(), vec![4, 5]);
    assert_eq!(input.read_frame(1).unwrap(), None);
    assert!(input.read_frame(0).is_err());
}

#[derive(Clone, Debug)]
struct SharedWriter(Arc<Mutex<Vec<u8>>>);

impl Write for SharedWriter {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buffer);
        Ok(buffer.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[test]
fn threaded_stream_output_drains_before_close() {
    let bytes = Arc::new(Mutex::new(Vec::new()));
    let mut output = StreamAudioOutput::new(SharedWriter(bytes.clone()));
    output.open(pcm_format()).unwrap();
    output.write(&[1, 2]).unwrap();
    output.write(&[3, 4]).unwrap();
    output.close().unwrap();
    assert_eq!(*bytes.lock().unwrap(), vec![1, 2, 3, 4]);
    assert!(matches!(output.write(&[5]), Err(Error::Closed)));
}

#[test]
fn wave_input_reads_format_and_loops_after_eof() {
    let path = temporary_file("wav");
    let samples = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11];
    fs::write(&path, wave_file(2, 48_000, 16, &samples)).unwrap();

    let mut input = WaveAudioInput::new(&path);
    assert_eq!(input.open().unwrap(), pcm_format());
    assert_eq!(input.read_frame(2).unwrap().unwrap(), samples[..8]);
    assert_eq!(input.read_frame(2).unwrap().unwrap(), samples[8..]);
    assert_eq!(input.read_frame(1).unwrap().unwrap(), samples[..4]);
    input.close().unwrap();
    assert!(matches!(input.read_frame(1), Err(Error::NotOpen)));
    fs::remove_file(path).unwrap();
}

#[test]
fn wave_input_rejects_unsupported_sample_width() {
    let path = temporary_file("wav");
    fs::write(&path, wave_file(1, 16_000, 8, &[1, 2])).unwrap();
    let error = WaveAudioInput::new(&path).open().unwrap_err();
    assert!(matches!(error, Error::Unsupported(_)));
    fs::remove_file(path).unwrap();
}

#[test]
fn factories_support_implicit_wave_and_raw_file_paths() {
    let wave_path = temporary_file("wav");
    fs::write(&wave_path, wave_file(1, 16_000, 16, &[1, 2])).unwrap();
    let mut wave = create_audio_input(wave_path.to_str().unwrap(), "auto").unwrap();
    assert_eq!(wave.open().unwrap().sample_rate, 16_000);

    let raw_path = temporary_file("pcm");
    fs::write(&raw_path, [1, 2, 3, 4]).unwrap();
    let mut raw = create_audio_input(raw_path.to_str().unwrap(), "int16le,16000,1").unwrap();
    assert_eq!(raw.open().unwrap().channels, 1);
    assert_eq!(raw.read_frame(2).unwrap().unwrap(), vec![1, 2, 3, 4]);
    assert!(create_audio_input(raw_path.to_str().unwrap(), "auto").is_err());

    fs::remove_file(wave_path).unwrap();
    fs::remove_file(raw_path).unwrap();
}

#[test]
fn device_factory_syntax_matches_upstream_contract() {
    assert!(check_audio_output("file:unused").unwrap());
    assert!(check_audio_input("stdin").unwrap());
    assert!(matches!(
        check_audio_output("device:not-an-index"),
        Err(Error::InvalidFormat(_))
    ));
    assert!(matches!(
        check_audio_input("device:"),
        Err(Error::InvalidFormat(_))
    ));
    assert!(matches!(
        create_audio_output("device:?"),
        Err(Error::InvalidFormat(_))
    ));
    assert!(matches!(
        create_audio_input("device", "auto"),
        Err(Error::InvalidFormat(_))
    ));
}

#[cfg(unix)]
#[test]
fn subprocess_output_expands_format_and_delivers_stdin() {
    let path = temporary_file("pcm");
    let command = format!(
        "test {{sample_rate}} = 48000 && test {{channel_layout}} = stereo && cat > {}",
        path.display()
    );
    let mut output = SubprocessAudioOutput::new(command);
    output.open(pcm_format()).unwrap();
    output.write(&[1, 2, 3, 4]).unwrap();
    output.close().unwrap();
    assert_eq!(fs::read(&path).unwrap(), vec![1, 2, 3, 4]);
    fs::remove_file(path).unwrap();
}
