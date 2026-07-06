//! HCI Command packets (Vol 2, Part E - 5.4.1).
//!
//! Wire form: `[0x01, op_code_lo, op_code_hi, param_len, parameters…]`.
//! Ported from `bumble.hci.HCI_Command` and the specific command classes.

use crate::codes::*;
use crate::{Error, Reader, Result};
use bumble::{Address, AddressType};

/// An HCI command. Typed variants carry decoded fields; [`Command::Generic`]
/// preserves the raw parameters for op codes this slice does not yet decode.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    Reset,
    Disconnect {
        connection_handle: u16,
        reason: u8,
    },
    SetEventMask {
        event_mask: [u8; 8],
    },
    LeSetEventMask {
        le_event_mask: [u8; 8],
    },
    LeSetRandomAddress {
        random_address: Address,
    },
    LeSetScanEnable {
        le_scan_enable: u8,
        filter_duplicates: u8,
    },
    ReadLocalVersionInformation,
    ReadLocalSupportedCommands,
    ReadLocalSupportedFeatures,
    /// Any command not decoded by this slice: raw op code + parameters.
    Generic {
        op_code: u16,
        parameters: Vec<u8>,
    },
}

impl Command {
    /// The 16-bit op code for this command.
    pub fn op_code(&self) -> u16 {
        match self {
            Command::Reset => HCI_RESET_COMMAND,
            Command::Disconnect { .. } => HCI_DISCONNECT_COMMAND,
            Command::SetEventMask { .. } => HCI_SET_EVENT_MASK_COMMAND,
            Command::LeSetEventMask { .. } => HCI_LE_SET_EVENT_MASK_COMMAND,
            Command::LeSetRandomAddress { .. } => HCI_LE_SET_RANDOM_ADDRESS_COMMAND,
            Command::LeSetScanEnable { .. } => HCI_LE_SET_SCAN_ENABLE_COMMAND,
            Command::ReadLocalVersionInformation => HCI_READ_LOCAL_VERSION_INFORMATION_COMMAND,
            Command::ReadLocalSupportedCommands => HCI_READ_LOCAL_SUPPORTED_COMMANDS_COMMAND,
            Command::ReadLocalSupportedFeatures => HCI_READ_LOCAL_SUPPORTED_FEATURES_COMMAND,
            Command::Generic { op_code, .. } => *op_code,
        }
    }

    /// The serialized command parameters (without the packet/op-code header).
    pub fn parameters(&self) -> Vec<u8> {
        match self {
            Command::Reset
            | Command::ReadLocalVersionInformation
            | Command::ReadLocalSupportedCommands
            | Command::ReadLocalSupportedFeatures => Vec::new(),
            Command::Disconnect {
                connection_handle,
                reason,
            } => {
                let mut p = Vec::with_capacity(3);
                p.extend_from_slice(&connection_handle.to_le_bytes());
                p.push(*reason);
                p
            }
            Command::SetEventMask { event_mask } => event_mask.to_vec(),
            Command::LeSetEventMask { le_event_mask } => le_event_mask.to_vec(),
            Command::LeSetRandomAddress { random_address } => {
                random_address.address_bytes().to_vec()
            }
            Command::LeSetScanEnable {
                le_scan_enable,
                filter_duplicates,
            } => vec![*le_scan_enable, *filter_duplicates],
            Command::Generic { parameters, .. } => parameters.clone(),
        }
    }

    /// Serialize to the full wire packet.
    pub fn to_bytes(&self) -> Vec<u8> {
        let params = self.parameters();
        let mut out = Vec::with_capacity(4 + params.len());
        out.push(HCI_COMMAND_PACKET);
        out.extend_from_slice(&self.op_code().to_le_bytes());
        out.push(params.len() as u8);
        out.extend_from_slice(&params);
        out
    }

    /// Parse a complete command packet (including the leading type byte).
    pub fn from_bytes(packet: &[u8]) -> Result<Command> {
        let mut r = Reader::new(packet, 1);
        let op_code = r.u16_le()?;
        let length = r.u8()? as usize;
        let parameters = r
            .take(length)
            .map_err(|_| Error::InvalidPacket("invalid packet length".into()))?;
        Command::from_parameters(op_code, parameters)
    }

    /// Build a typed command from its op code and raw parameters.
    pub fn from_parameters(op_code: u16, parameters: &[u8]) -> Result<Command> {
        let mut r = Reader::new(parameters, 0);
        Ok(match op_code {
            HCI_RESET_COMMAND => Command::Reset,
            HCI_READ_LOCAL_VERSION_INFORMATION_COMMAND => Command::ReadLocalVersionInformation,
            HCI_READ_LOCAL_SUPPORTED_COMMANDS_COMMAND => Command::ReadLocalSupportedCommands,
            HCI_READ_LOCAL_SUPPORTED_FEATURES_COMMAND => Command::ReadLocalSupportedFeatures,
            HCI_DISCONNECT_COMMAND => Command::Disconnect {
                connection_handle: r.u16_le()?,
                reason: r.u8()?,
            },
            HCI_SET_EVENT_MASK_COMMAND => Command::SetEventMask {
                event_mask: r.array::<8>()?,
            },
            HCI_LE_SET_EVENT_MASK_COMMAND => Command::LeSetEventMask {
                le_event_mask: r.array::<8>()?,
            },
            HCI_LE_SET_RANDOM_ADDRESS_COMMAND => Command::LeSetRandomAddress {
                // The wire form carries only the 6 address bytes; the type is
                // not transmitted for this field, so reconstruct as a random
                // device address (the value used when the command is built).
                random_address: Address::from_bytes(r.array::<6>()?, AddressType::RANDOM_DEVICE),
            },
            HCI_LE_SET_SCAN_ENABLE_COMMAND => Command::LeSetScanEnable {
                le_scan_enable: r.u8()?,
                filter_duplicates: r.u8()?,
            },
            _ => Command::Generic {
                op_code,
                parameters: parameters.to_vec(),
            },
        })
    }
}
