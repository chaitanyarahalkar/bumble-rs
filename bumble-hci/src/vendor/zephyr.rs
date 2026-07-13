//! Zephyr RTOS vendor-specific transmit-power HCI commands.

use super::{command, exact_length, vendor_command_op_code};
use crate::{Command, Reader, Result};

pub const HCI_WRITE_TX_POWER_LEVEL_COMMAND: u16 = vendor_command_op_code(0x000E);
pub const HCI_READ_TX_POWER_LEVEL_COMMAND: u16 = vendor_command_op_code(0x000F);

pub const TX_POWER_HANDLE_TYPE_ADV: u8 = 0x00;
pub const TX_POWER_HANDLE_TYPE_SCAN: u8 = 0x01;
pub const TX_POWER_HANDLE_TYPE_CONN: u8 = 0x02;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WriteTxPowerLevelCommand {
    pub handle_type: u8,
    pub connection_handle: u16,
    pub tx_power_level: i8,
}

impl WriteTxPowerLevelCommand {
    pub fn to_command(self) -> Command {
        let mut parameters = Vec::with_capacity(4);
        parameters.push(self.handle_type);
        parameters.extend_from_slice(&self.connection_handle.to_le_bytes());
        parameters.push(self.tx_power_level as u8);
        command(HCI_WRITE_TX_POWER_LEVEL_COMMAND, parameters)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WriteTxPowerLevelReturnParameters {
    pub status: u8,
    pub handle_type: u8,
    pub connection_handle: u16,
    pub selected_tx_power_level: i8,
}

impl WriteTxPowerLevelReturnParameters {
    pub fn parse(data: &[u8]) -> Result<Self> {
        exact_length(data, 5, "Zephyr write TX power return parameters")?;
        let mut reader = Reader::new(data, 0);
        Ok(Self {
            status: reader.u8()?,
            handle_type: reader.u8()?,
            connection_handle: reader.u16_le()?,
            selected_tx_power_level: reader.u8()? as i8,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReadTxPowerLevelCommand {
    pub handle_type: u8,
    pub connection_handle: u16,
}

impl ReadTxPowerLevelCommand {
    pub fn to_command(self) -> Command {
        let mut parameters = Vec::with_capacity(3);
        parameters.push(self.handle_type);
        parameters.extend_from_slice(&self.connection_handle.to_le_bytes());
        command(HCI_READ_TX_POWER_LEVEL_COMMAND, parameters)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReadTxPowerLevelReturnParameters {
    pub status: u8,
    pub handle_type: u8,
    pub connection_handle: u16,
    pub tx_power_level: i8,
}

impl ReadTxPowerLevelReturnParameters {
    pub fn parse(data: &[u8]) -> Result<Self> {
        exact_length(data, 5, "Zephyr read TX power return parameters")?;
        let mut reader = Reader::new(data, 0);
        Ok(Self {
            status: reader.u8()?,
            handle_type: reader.u8()?,
            connection_handle: reader.u16_le()?,
            tx_power_level: reader.u8()? as i8,
        })
    }
}
