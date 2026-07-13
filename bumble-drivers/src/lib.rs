//! Vendor-specific Bluetooth controller initialization.
//!
//! This crate ports `bumble.drivers`: driver selection remains separate from
//! transport discovery, while [`DriverHost`] abstracts the synchronous command
//! and vendor-event operations needed during cold-start firmware loading.

use bumble_hci::Command;
use std::collections::BTreeMap;
use std::fmt;

pub mod intel;

/// Metadata attached to an opened HCI transport.
pub type HciMetadata = BTreeMap<String, String>;

/// A completed HCI command transaction.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandResponse {
    pub num_hci_command_packets: u8,
    /// Raw command-complete return parameters, including the status byte.
    pub return_parameters: Vec<u8>,
}

impl CommandResponse {
    pub fn status(&self) -> Option<u8> {
        self.return_parameters.first().copied()
    }
}

/// Host operations used by firmware drivers.
///
/// `transact_batch` may be overridden by an external-host implementation to
/// honor the controller's command-credit window. Its default preserves the
/// same wire order with one command in flight.
pub trait DriverHost {
    fn metadata(&self) -> &HciMetadata;
    fn transact(&mut self, command: Command) -> Result<CommandResponse>;

    fn transact_batch(&mut self, commands: Vec<Command>) -> Result<Vec<CommandResponse>> {
        commands
            .into_iter()
            .map(|command| self.transact(command))
            .collect()
    }

    /// Send a command whose completion is reported by a vendor event.
    fn send_without_response(&mut self, command: Command) -> Result<()>;

    /// Wait for a vendor event with the requested first parameter byte.
    fn wait_vendor_event(&mut self, event_type: u8) -> Result<Vec<u8>>;
}

/// Firmware and configuration blob lookup.
pub trait FirmwareProvider {
    /// Return `Ok(None)` when the named optional blob is unavailable.
    fn load(&self, file_name: &str) -> Result<Option<Vec<u8>>>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    InvalidMetadata(String),
    InvalidFirmware(String),
    InvalidResponse(String),
    MissingFirmware(String),
    Unsupported(String),
    Host(String),
    Io(String),
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidMetadata(message) => {
                write!(formatter, "invalid driver metadata: {message}")
            }
            Self::InvalidFirmware(message) => write!(formatter, "invalid firmware: {message}"),
            Self::InvalidResponse(message) => write!(formatter, "invalid HCI response: {message}"),
            Self::MissingFirmware(name) => write!(formatter, "firmware file not found: {name}"),
            Self::Unsupported(message) => write!(formatter, "unsupported controller: {message}"),
            Self::Host(message) => write!(formatter, "driver host error: {message}"),
            Self::Io(message) => write!(formatter, "driver I/O error: {message}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error.to_string())
    }
}

pub type Result<T> = std::result::Result<T, Error>;

pub(crate) fn metadata_u16(metadata: &HciMetadata, name: &str) -> Option<u16> {
    let value = metadata.get(name)?;
    let value = value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
        .unwrap_or(value);
    u16::from_str_radix(value, 16).ok()
}

pub(crate) fn require_success(response: &CommandResponse, operation: &str) -> Result<()> {
    match response.status() {
        Some(0) => Ok(()),
        Some(status) => Err(Error::InvalidResponse(format!(
            "{operation} returned status 0x{status:02X}"
        ))),
        None => Err(Error::InvalidResponse(format!(
            "{operation} returned no status"
        ))),
    }
}
