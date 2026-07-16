use crate::{Error, H4Transport, PacketSink, PacketSource, Result};
use bumble_hci::HciPacket;
use serialport::{FlowControl, SerialPort};
use std::io;
use std::thread;
use std::time::{Duration, Instant};

pub const DEFAULT_SERIAL_SPEED: u32 = 1_000_000;
pub const DEFAULT_POST_OPEN_DELAY: Duration = Duration::from_millis(500);
const DSR_POLL_INTERVAL: Duration = Duration::from_millis(1);

fn wait_for_dsr_with<ReadDsr, Sleep>(
    timeout: Duration,
    mut read_dsr: ReadDsr,
    mut sleep: Sleep,
) -> Result<()>
where
    ReadDsr: FnMut() -> serialport::Result<bool>,
    Sleep: FnMut(Duration),
{
    let deadline = Instant::now() + timeout;
    loop {
        if read_dsr()? {
            return Ok(());
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "timed out waiting for serial DSR",
            )
            .into());
        }
        sleep(remaining.min(DSR_POLL_INTERVAL));
    }
}

/// Parsed Bumble serial transport parameters.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SerialConfig {
    pub device: String,
    pub speed: u32,
    pub rtscts: bool,
    pub dsrdtr: bool,
    pub post_open_delay: Duration,
}

impl SerialConfig {
    pub fn parse(spec: &str) -> Result<Self> {
        let mut parts = spec.split(',');
        let device = parts.next().unwrap_or_default();
        if device.is_empty() {
            return Err(Error::InvalidSpec("serial device path is empty".into()));
        }
        let mut config = Self {
            device: device.into(),
            speed: DEFAULT_SERIAL_SPEED,
            rtscts: false,
            dsrdtr: false,
            post_open_delay: Duration::ZERO,
        };
        for part in parts {
            match part {
                "rtscts" => config.rtscts = true,
                "dsrdtr" => config.dsrdtr = true,
                "delay" => config.post_open_delay = DEFAULT_POST_OPEN_DELAY,
                value if value.bytes().all(|byte| byte.is_ascii_digit()) && !value.is_empty() => {
                    config.speed = value
                        .parse()
                        .map_err(|_| Error::InvalidSpec(format!("invalid serial speed {value}")))?;
                }
                _ => {}
            }
        }
        Ok(config)
    }
}

pub struct SerialTransport {
    inner: H4Transport<Box<dyn SerialPort>>,
    config: SerialConfig,
}

impl SerialTransport {
    pub fn open(spec: &str) -> Result<Self> {
        Self::open_config(SerialConfig::parse(spec)?)
    }

    pub fn open_config(config: SerialConfig) -> Result<Self> {
        let mut port = serialport::new(&config.device, config.speed)
            .flow_control(if config.rtscts {
                FlowControl::Hardware
            } else {
                FlowControl::None
            })
            .timeout(Duration::from_secs(24 * 60 * 60))
            .open()?;
        if config.dsrdtr {
            port.write_data_terminal_ready(true)?;
        } else {
            let _ = port.write_data_terminal_ready(true);
        }
        if !config.post_open_delay.is_zero() {
            thread::sleep(config.post_open_delay);
        }
        Ok(Self {
            inner: H4Transport::new(port),
            config,
        })
    }

    pub fn config(&self) -> &SerialConfig {
        &self.config
    }

    pub fn port(&self) -> &dyn SerialPort {
        self.inner.get_ref().as_ref()
    }

    pub fn port_mut(&mut self) -> &mut dyn SerialPort {
        self.inner.get_mut().as_mut()
    }

    pub fn try_split(self) -> Result<(Self, Self)> {
        let source = Self {
            inner: H4Transport::new(self.inner.get_ref().try_clone()?),
            config: self.config.clone(),
        };
        Ok((source, self))
    }
}

impl PacketSource for SerialTransport {
    fn read_packet(&mut self) -> Result<Option<HciPacket>> {
        self.inner.read_packet()
    }
}

impl PacketSink for SerialTransport {
    fn write_packet(&mut self, packet: &HciPacket) -> Result<()> {
        if self.config.dsrdtr {
            let timeout = self.inner.get_ref().timeout();
            let port = self.inner.get_mut().as_mut();
            wait_for_dsr_with(timeout, || port.read_data_set_ready(), thread::sleep)?;
        }
        self.inner.write_packet(packet)
    }

    fn flush(&mut self) -> Result<()> {
        self.inner.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    #[test]
    fn dsr_gate_waits_until_the_peer_is_ready() {
        let mut samples = VecDeque::from([false, false, true]);
        let mut sleeps = Vec::new();

        wait_for_dsr_with(
            Duration::from_secs(1),
            || Ok(samples.pop_front().unwrap()),
            |duration| sleeps.push(duration),
        )
        .unwrap();

        assert!(samples.is_empty());
        assert_eq!(sleeps, vec![DSR_POLL_INTERVAL, DSR_POLL_INTERVAL]);
    }

    #[test]
    fn dsr_gate_times_out_without_sleeping_past_the_deadline() {
        let mut sleeps = Vec::new();

        let error = wait_for_dsr_with(
            Duration::ZERO,
            || Ok(false),
            |duration| sleeps.push(duration),
        )
        .unwrap_err();

        assert!(matches!(
            error,
            Error::Io(error) if error.kind() == io::ErrorKind::TimedOut
        ));
        assert!(sleeps.is_empty());
    }

    #[test]
    fn dsr_gate_preserves_serial_pin_errors() {
        let error = wait_for_dsr_with(
            Duration::from_secs(1),
            || {
                Err(serialport::Error::new(
                    serialport::ErrorKind::Unknown,
                    "DSR pin unavailable",
                ))
            },
            |_| panic!("a pin-read error must not sleep"),
        )
        .unwrap_err();

        assert!(
            matches!(error, Error::Serial(error) if error.description == "DSR pin unavailable")
        );
    }
}
