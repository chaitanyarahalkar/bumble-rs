//! Vendor-specific HCI command and event codecs.

pub mod android;
pub mod zephyr;

use crate::{Command, Error, Result};

pub(crate) const fn vendor_command_op_code(command_field: u16) -> u16 {
    (0x3F << 10) | (command_field & 0x03FF)
}

pub(crate) fn command(op_code: u16, parameters: Vec<u8>) -> Command {
    Command::Generic {
        op_code,
        parameters,
    }
}

pub(crate) fn exact_length(data: &[u8], expected: usize, name: &str) -> Result<()> {
    if data.len() != expected {
        return Err(Error::InvalidPacket(format!(
            "{name} has length {}, expected {expected}",
            data.len()
        )));
    }
    Ok(())
}
