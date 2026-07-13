use crate::{Error, H4Transport, PacketSink, PacketSource, Result};
use bumble_hci::HciPacket;
use serialport::{FlowControl, SerialPort};
use std::thread;
use std::time::Duration;

pub const DEFAULT_SERIAL_SPEED: u32 = 1_000_000;
pub const DEFAULT_POST_OPEN_DELAY: Duration = Duration::from_millis(500);

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
        if config.dsrdtr {
            return Err(Error::Unsupported("DSR/DTR hardware flow control".into()));
        }
        let mut port = serialport::new(&config.device, config.speed)
            .flow_control(if config.rtscts {
                FlowControl::Hardware
            } else {
                FlowControl::None
            })
            .timeout(Duration::from_secs(24 * 60 * 60))
            .open()?;
        let _ = port.write_data_terminal_ready(true);
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
}

impl PacketSource for SerialTransport {
    fn read_packet(&mut self) -> Result<Option<HciPacket>> {
        self.inner.read_packet()
    }
}

impl PacketSink for SerialTransport {
    fn write_packet(&mut self, packet: &HciPacket) -> Result<()> {
        self.inner.write_packet(packet)
    }

    fn flush(&mut self) -> Result<()> {
        self.inner.flush()
    }
}
