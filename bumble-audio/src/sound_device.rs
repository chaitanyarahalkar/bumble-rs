use std::collections::VecDeque;
use std::fmt;
use std::sync::mpsc::{self, Receiver, Sender, SyncSender, TryRecvError};
use std::thread::{self, JoinHandle};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{BufferSize, Device, Host, SampleFormat, StreamConfig};

use crate::{
    AudioDeviceInfo, AudioDeviceSelector, AudioInput, AudioOutput, Endianness, Error, PcmFormat,
    Result, SampleType,
};

#[derive(Clone, Copy)]
enum Direction {
    Input,
    Output,
}

fn backend_error(error: impl fmt::Display) -> Error {
    Error::Backend(error.to_string())
}

fn default_device(host: &Host, direction: Direction) -> Option<Device> {
    match direction {
        Direction::Input => host.default_input_device(),
        Direction::Output => host.default_output_device(),
    }
}

fn max_channels(device: &Device, direction: Direction) -> u16 {
    match direction {
        Direction::Input => device
            .supported_input_configs()
            .ok()
            .and_then(|configs| configs.map(|config| config.channels()).max()),
        Direction::Output => device
            .supported_output_configs()
            .ok()
            .and_then(|configs| configs.map(|config| config.channels()).max()),
    }
    .unwrap_or(0)
}

fn list_devices(direction: Direction) -> Result<Vec<AudioDeviceInfo>> {
    let host = cpal::default_host();
    let default_id = default_device(&host, direction)
        .and_then(|device| device.id().ok())
        .map(|id| id.to_string());
    let devices = host.devices().map_err(backend_error)?;
    let mut result = Vec::new();

    for (index, device) in devices.enumerate() {
        let max_channels = max_channels(&device, direction);
        if max_channels == 0 {
            continue;
        }
        let id = device
            .id()
            .map(|id| id.to_string())
            .unwrap_or_else(|_| format!("index:{index}"));
        result.push(AudioDeviceInfo {
            is_default: default_id.as_ref().is_some_and(|default| default == &id),
            id,
            index,
            name: device.to_string(),
            max_channels,
        });
    }

    Ok(result)
}

pub(crate) fn list_output_devices() -> Result<Vec<AudioDeviceInfo>> {
    list_devices(Direction::Output)
}

pub(crate) fn list_input_devices() -> Result<Vec<AudioDeviceInfo>> {
    list_devices(Direction::Input)
}

fn select_device(direction: Direction, index: Option<usize>) -> Result<Device> {
    let host = cpal::default_host();
    let device = match index {
        Some(index) => host
            .devices()
            .map_err(backend_error)?
            .nth(index)
            .ok_or_else(|| Error::Backend(format!("no audio device at index {index}")))?,
        None => default_device(&host, direction)
            .ok_or_else(|| Error::Backend("no default audio device".into()))?,
    };
    let channels = max_channels(&device, direction);
    if channels == 0 {
        let direction = match direction {
            Direction::Input => "input",
            Direction::Output => "output",
        };
        return Err(Error::Backend(format!(
            "device {} ({device}) does not support audio {direction}",
            index.map_or_else(|| "default".into(), |index| index.to_string())
        )));
    }
    Ok(device)
}

pub(crate) fn check_output(selector: AudioDeviceSelector) -> Result<bool> {
    match selector {
        AudioDeviceSelector::Default => Ok(true),
        AudioDeviceSelector::Index(index) => {
            select_device(Direction::Output, Some(index))?;
            Ok(true)
        }
        AudioDeviceSelector::List => {
            println!("Audio Devices:");
            for device in list_output_devices()? {
                println!(
                    "{}: {}{}",
                    device.index,
                    device.name,
                    if device.is_default { " [default]" } else { "" }
                );
            }
            Ok(false)
        }
    }
}

pub(crate) fn check_input(selector: AudioDeviceSelector) -> Result<bool> {
    match selector {
        AudioDeviceSelector::Default => Ok(true),
        AudioDeviceSelector::Index(index) => {
            select_device(Direction::Input, Some(index))?;
            Ok(true)
        }
        AudioDeviceSelector::List => {
            println!("Audio Devices:");
            for device in list_input_devices()? {
                println!(
                    "{}: {} [{}]{}",
                    device.index,
                    device.name,
                    if device.max_channels == 1 {
                        "mono"
                    } else {
                        "stereo"
                    },
                    if device.is_default { " [default]" } else { "" }
                );
            }
            Ok(false)
        }
    }
}

fn stream_config(format: PcmFormat) -> StreamConfig {
    StreamConfig {
        channels: format.channels,
        sample_rate: format.sample_rate,
        buffer_size: BufferSize::Default,
    }
}

fn fill_output_buffer(output: &mut [u8], samples: &Receiver<Vec<u8>>, pending: &mut VecDeque<u8>) {
    output.fill(0);
    let mut offset = 0;
    while offset < output.len() {
        while offset < output.len() {
            let Some(sample) = pending.pop_front() else {
                break;
            };
            output[offset] = sample;
            offset += 1;
        }
        if offset == output.len() {
            break;
        }
        match samples.try_recv() {
            Ok(samples) => pending.extend(samples),
            Err(TryRecvError::Empty | TryRecvError::Disconnected) => break,
        }
    }
}

fn run_output_worker(
    index: Option<usize>,
    format: PcmFormat,
    ready: SyncSender<std::result::Result<(), String>>,
    control: Receiver<()>,
    samples: Receiver<Vec<u8>>,
    errors: Sender<String>,
) {
    let result = (|| -> std::result::Result<cpal::Stream, String> {
        let device = select_device(Direction::Output, index).map_err(|error| error.to_string())?;
        let mut pending = VecDeque::new();
        let stream = device
            .build_output_stream_raw(
                stream_config(format),
                SampleFormat::F32,
                move |data, _| fill_output_buffer(data.bytes_mut(), &samples, &mut pending),
                move |error| {
                    let _ = errors.send(error.to_string());
                },
                None,
            )
            .map_err(|error| error.to_string())?;
        stream.play().map_err(|error| error.to_string())?;
        Ok(stream)
    })();

    match result {
        Ok(stream) => {
            if ready.send(Ok(())).is_ok() {
                let _ = control.recv();
            }
            drop(stream);
        }
        Err(error) => {
            let _ = ready.send(Err(error));
        }
    }
}

/// Non-blocking float32 PCM output to a platform sound device.
///
/// This matches upstream `SoundDeviceAudioOutput`: the requested rate and
/// channel count configure the stream, while the device sample type is always
/// float32. `write` queues bytes without waiting for the hardware callback.
pub struct SoundDeviceAudioOutput {
    device_index: Option<usize>,
    samples: Option<Sender<Vec<u8>>>,
    errors: Option<Receiver<String>>,
    control: Option<Sender<()>>,
    worker: Option<JoinHandle<()>>,
}

impl SoundDeviceAudioOutput {
    pub fn new(device_index: Option<usize>) -> Self {
        Self {
            device_index,
            samples: None,
            errors: None,
            control: None,
            worker: None,
        }
    }

    fn take_stream_error(&self) -> Option<Error> {
        self.errors
            .as_ref()
            .and_then(|errors| errors.try_recv().ok())
            .map(Error::Backend)
    }
}

impl fmt::Debug for SoundDeviceAudioOutput {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SoundDeviceAudioOutput")
            .field("device_index", &self.device_index)
            .field("open", &self.worker.is_some())
            .finish()
    }
}

impl AudioOutput for SoundDeviceAudioOutput {
    fn open(&mut self, pcm_format: PcmFormat) -> Result<()> {
        pcm_format.validate()?;
        if self.worker.is_some() {
            return Err(Error::InvalidFormat(
                "sound-device output is already open".into(),
            ));
        }

        let (sample_sender, sample_receiver) = mpsc::channel();
        let (error_sender, error_receiver) = mpsc::channel();
        let (control_sender, control_receiver) = mpsc::channel();
        let (ready_sender, ready_receiver) = mpsc::sync_channel(1);
        let index = self.device_index;
        let worker = thread::spawn(move || {
            run_output_worker(
                index,
                pcm_format,
                ready_sender,
                control_receiver,
                sample_receiver,
                error_sender,
            )
        });

        match ready_receiver.recv() {
            Ok(Ok(())) => {
                self.samples = Some(sample_sender);
                self.errors = Some(error_receiver);
                self.control = Some(control_sender);
                self.worker = Some(worker);
                Ok(())
            }
            Ok(Err(error)) => {
                worker.join().map_err(|_| Error::WorkerPanicked)?;
                Err(Error::Backend(error))
            }
            Err(_) => {
                worker.join().map_err(|_| Error::WorkerPanicked)?;
                Err(Error::WorkerPanicked)
            }
        }
    }

    fn write(&mut self, pcm_samples: &[u8]) -> Result<()> {
        if let Some(error) = self.take_stream_error() {
            return Err(error);
        }
        self.samples
            .as_ref()
            .ok_or(Error::NotOpen)?
            .send(pcm_samples.to_vec())
            .map_err(|_| Error::Closed)
    }

    fn close(&mut self) -> Result<()> {
        self.samples.take();
        if let Some(control) = self.control.take() {
            let _ = control.send(());
        }
        if let Some(worker) = self.worker.take() {
            worker.join().map_err(|_| Error::WorkerPanicked)?;
        }
        if let Some(error) = self.take_stream_error() {
            self.errors = None;
            return Err(error);
        }
        self.errors = None;
        Ok(())
    }
}

impl Drop for SoundDeviceAudioOutput {
    fn drop(&mut self) {
        let _ = self.close();
    }
}

enum InputEvent {
    Samples(Vec<u8>),
    Error(String),
}

fn run_input_worker(
    index: Option<usize>,
    format: PcmFormat,
    ready: SyncSender<std::result::Result<PcmFormat, String>>,
    control: Receiver<()>,
    events: Sender<InputEvent>,
) {
    let event_errors = events.clone();
    let result = (|| -> std::result::Result<cpal::Stream, String> {
        let device = select_device(Direction::Input, index).map_err(|error| error.to_string())?;
        let stream = device
            .build_input_stream_raw(
                stream_config(format),
                SampleFormat::I16,
                move |data, _| {
                    let _ = events.send(InputEvent::Samples(data.bytes().to_vec()));
                },
                move |error| {
                    let _ = event_errors.send(InputEvent::Error(error.to_string()));
                },
                None,
            )
            .map_err(|error| error.to_string())?;
        stream.play().map_err(|error| error.to_string())?;
        Ok(stream)
    })();

    match result {
        Ok(stream) => {
            let output_format =
                PcmFormat::new(Endianness::Little, SampleType::Int16, format.sample_rate, 2);
            if ready.send(Ok(output_format)).is_ok() {
                let _ = control.recv();
            }
            drop(stream);
        }
        Err(error) => {
            let _ = ready.send(Err(error));
        }
    }
}

fn duplicate_mono_samples(samples: &[u8]) -> Vec<u8> {
    let mut stereo = Vec::with_capacity(samples.len().saturating_mul(2));
    for sample in samples.chunks_exact(2) {
        stereo.extend_from_slice(sample);
        stereo.extend_from_slice(sample);
    }
    stereo
}

/// Blocking int16 PCM input from a platform sound device.
///
/// Like upstream, mono input is duplicated into stereo and `open` reports a
/// two-channel int16 format at the requested sample rate.
pub struct SoundDeviceAudioInput {
    device_index: Option<usize>,
    requested_format: PcmFormat,
    events: Option<Receiver<InputEvent>>,
    pending: VecDeque<u8>,
    control: Option<Sender<()>>,
    worker: Option<JoinHandle<()>>,
}

impl SoundDeviceAudioInput {
    pub fn new(device_index: Option<usize>, requested_format: PcmFormat) -> Self {
        Self {
            device_index,
            requested_format,
            events: None,
            pending: VecDeque::new(),
            control: None,
            worker: None,
        }
    }
}

impl fmt::Debug for SoundDeviceAudioInput {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SoundDeviceAudioInput")
            .field("device_index", &self.device_index)
            .field("requested_format", &self.requested_format)
            .field("open", &self.worker.is_some())
            .finish()
    }
}

impl AudioInput for SoundDeviceAudioInput {
    fn open(&mut self) -> Result<PcmFormat> {
        self.requested_format.validate()?;
        if self.worker.is_some() {
            return Err(Error::InvalidFormat(
                "sound-device input is already open".into(),
            ));
        }

        let (event_sender, event_receiver) = mpsc::channel();
        let (control_sender, control_receiver) = mpsc::channel();
        let (ready_sender, ready_receiver) = mpsc::sync_channel(1);
        let index = self.device_index;
        let format = self.requested_format;
        let worker = thread::spawn(move || {
            run_input_worker(index, format, ready_sender, control_receiver, event_sender)
        });

        match ready_receiver.recv() {
            Ok(Ok(output_format)) => {
                self.events = Some(event_receiver);
                self.control = Some(control_sender);
                self.worker = Some(worker);
                self.pending.clear();
                Ok(output_format)
            }
            Ok(Err(error)) => {
                worker.join().map_err(|_| Error::WorkerPanicked)?;
                Err(Error::Backend(error))
            }
            Err(_) => {
                worker.join().map_err(|_| Error::WorkerPanicked)?;
                Err(Error::WorkerPanicked)
            }
        }
    }

    fn read_frame(&mut self, frame_size: usize) -> Result<Option<Vec<u8>>> {
        if frame_size == 0 {
            return Err(Error::InvalidFormat("frame size must be nonzero".into()));
        }
        let byte_count = frame_size
            .checked_mul(usize::from(self.requested_format.channels))
            .and_then(|samples| samples.checked_mul(2))
            .ok_or(Error::ValueTooLarge)?;
        let events = self.events.as_ref().ok_or(Error::NotOpen)?;

        while self.pending.len() < byte_count {
            match events.recv() {
                Ok(InputEvent::Samples(samples)) => self.pending.extend(samples),
                Ok(InputEvent::Error(error)) => return Err(Error::Backend(error)),
                Err(_) if self.pending.is_empty() => return Ok(None),
                Err(_) => break,
            }
        }

        let bytes_to_take = byte_count.min(self.pending.len());
        let samples: Vec<_> = self.pending.drain(..bytes_to_take).collect();
        if self.requested_format.channels == 1 {
            Ok(Some(duplicate_mono_samples(&samples)))
        } else {
            Ok(Some(samples))
        }
    }

    fn close(&mut self) -> Result<()> {
        if let Some(control) = self.control.take() {
            let _ = control.send(());
        }
        if let Some(worker) = self.worker.take() {
            worker.join().map_err(|_| Error::WorkerPanicked)?;
        }
        self.events = None;
        self.pending.clear();
        Ok(())
    }
}

impl Drop for SoundDeviceAudioInput {
    fn drop(&mut self) {
        let _ = self.close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_callback_preserves_queue_order_and_silences_underflow() {
        let (sender, receiver) = mpsc::channel();
        sender.send(vec![1, 2, 3]).unwrap();
        sender.send(vec![4, 5]).unwrap();
        let mut pending = VecDeque::new();
        let mut output = [0xFF; 7];

        fill_output_buffer(&mut output, &receiver, &mut pending);

        assert_eq!(output, [1, 2, 3, 4, 5, 0, 0]);
        let mut empty_output = [0xFF; 3];
        fill_output_buffer(&mut empty_output, &receiver, &mut pending);
        assert_eq!(empty_output, [0, 0, 0]);
    }

    #[test]
    fn mono_int16_samples_are_duplicated_into_stereo() {
        assert_eq!(
            duplicate_mono_samples(&[0x01, 0x02, 0x03, 0x04]),
            [0x01, 0x02, 0x01, 0x02, 0x03, 0x04, 0x03, 0x04]
        );
    }
}
