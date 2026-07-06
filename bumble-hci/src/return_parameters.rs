//! HCI Command Complete return parameters.
//!
//! Ported from `bumble.hci` return-parameter classes. All typed return
//! parameters begin with a status byte; per `HCI_StatusReturnParameters`, when
//! the status is not SUCCESS the controller returns only the status and the
//! remaining fields are absent — so parsing falls back to [`ReturnParameters::Status`].

use crate::codes::*;
use crate::{Reader, Result};
use bumble::{Address, AddressType};

/// Decode a null-terminated UTF-8 string (mirrors
/// `bumble.hci.map_null_terminated_utf8_string`). Invalid UTF-8 is returned
/// lossily.
pub fn map_null_terminated_utf8_string(bytes: &[u8]) -> String {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..end]).into_owned()
}

/// The return parameters carried by an HCI Command Complete event. Typed
/// variants decode known commands; [`ReturnParameters::Raw`] preserves the raw
/// bytes for commands this slice does not model.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReturnParameters {
    /// Status-only (an error response, or a command whose only return
    /// parameter is a status).
    Status {
        status: u8,
    },
    LeReadBufferSize {
        status: u8,
        le_acl_data_packet_length: u16,
        total_num_le_acl_data_packets: u8,
    },
    ReadBdAddr {
        status: u8,
        bd_addr: Address,
    },
    ReadLocalName {
        status: u8,
        /// The fixed 248-byte local name field (see
        /// [`map_null_terminated_utf8_string`]).
        local_name: Vec<u8>,
    },
    ReadLocalSupportedCodecs {
        status: u8,
        standard_codec_ids: Vec<u8>,
        vendor_specific_codec_ids: Vec<u32>,
    },
    ReadLocalSupportedCodecsV2 {
        status: u8,
        standard_codec_ids: Vec<u8>,
        standard_codec_transports: Vec<u8>,
        vendor_specific_codec_ids: Vec<u32>,
        vendor_specific_codec_transports: Vec<u8>,
    },
    Raw {
        data: Vec<u8>,
    },
}

impl ReturnParameters {
    /// The status byte (0 = SUCCESS), or `None` for [`ReturnParameters::Raw`].
    pub fn status(&self) -> Option<u8> {
        Some(match self {
            ReturnParameters::Status { status }
            | ReturnParameters::LeReadBufferSize { status, .. }
            | ReturnParameters::ReadBdAddr { status, .. }
            | ReturnParameters::ReadLocalName { status, .. }
            | ReturnParameters::ReadLocalSupportedCodecs { status, .. }
            | ReturnParameters::ReadLocalSupportedCodecsV2 { status, .. } => *status,
            ReturnParameters::Raw { .. } => return None,
        })
    }

    /// Serialize the return parameters.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut p = Vec::new();
        match self {
            ReturnParameters::Status { status } => p.push(*status),
            ReturnParameters::LeReadBufferSize {
                status,
                le_acl_data_packet_length,
                total_num_le_acl_data_packets,
            } => {
                p.push(*status);
                p.extend_from_slice(&le_acl_data_packet_length.to_le_bytes());
                p.push(*total_num_le_acl_data_packets);
            }
            ReturnParameters::ReadBdAddr { status, bd_addr } => {
                p.push(*status);
                p.extend_from_slice(bd_addr.address_bytes());
            }
            ReturnParameters::ReadLocalName { status, local_name } => {
                p.push(*status);
                p.extend_from_slice(local_name);
            }
            ReturnParameters::ReadLocalSupportedCodecs {
                status,
                standard_codec_ids,
                vendor_specific_codec_ids,
            } => {
                p.push(*status);
                p.push(standard_codec_ids.len() as u8);
                p.extend_from_slice(standard_codec_ids);
                p.push(vendor_specific_codec_ids.len() as u8);
                for v in vendor_specific_codec_ids {
                    p.extend_from_slice(&v.to_le_bytes());
                }
            }
            ReturnParameters::ReadLocalSupportedCodecsV2 {
                status,
                standard_codec_ids,
                standard_codec_transports,
                vendor_specific_codec_ids,
                vendor_specific_codec_transports,
            } => {
                p.push(*status);
                p.push(standard_codec_ids.len() as u8);
                for i in 0..standard_codec_ids.len() {
                    p.push(standard_codec_ids[i]);
                    p.push(standard_codec_transports[i]);
                }
                p.push(vendor_specific_codec_ids.len() as u8);
                for i in 0..vendor_specific_codec_ids.len() {
                    p.extend_from_slice(&vendor_specific_codec_ids[i].to_le_bytes());
                    p.push(vendor_specific_codec_transports[i]);
                }
            }
            ReturnParameters::Raw { data } => p.extend_from_slice(data),
        }
        p
    }

    /// Parse return parameters for a given command op code.
    ///
    /// All typed parameters share the status-based fallback: a non-SUCCESS
    /// status yields [`ReturnParameters::Status`] without decoding further.
    pub fn parse(command_opcode: u16, data: &[u8]) -> Result<ReturnParameters> {
        // Commands whose only return parameter is a status.
        if command_opcode == HCI_RESET_COMMAND {
            return Ok(ReturnParameters::Status {
                status: first_status(data),
            });
        }

        let is_typed = matches!(
            command_opcode,
            HCI_LE_READ_BUFFER_SIZE_COMMAND
                | HCI_READ_BD_ADDR_COMMAND
                | HCI_READ_LOCAL_NAME_COMMAND
                | HCI_READ_LOCAL_SUPPORTED_CODECS_COMMAND
                | HCI_READ_LOCAL_SUPPORTED_CODECS_V2_COMMAND
        );
        if !is_typed {
            return Ok(ReturnParameters::Raw {
                data: data.to_vec(),
            });
        }

        // Typed: on a non-SUCCESS status the extra fields are absent.
        let status = first_status(data);
        if status != HCI_SUCCESS {
            return Ok(ReturnParameters::Status { status });
        }

        let mut r = Reader::new(data, 0);
        let status = r.u8()?;
        Ok(match command_opcode {
            HCI_LE_READ_BUFFER_SIZE_COMMAND => ReturnParameters::LeReadBufferSize {
                status,
                le_acl_data_packet_length: r.u16_le()?,
                total_num_le_acl_data_packets: r.u8()?,
            },
            HCI_READ_BD_ADDR_COMMAND => ReturnParameters::ReadBdAddr {
                status,
                bd_addr: Address::from_bytes(r.array::<6>()?, AddressType::PUBLIC_DEVICE),
            },
            HCI_READ_LOCAL_NAME_COMMAND => ReturnParameters::ReadLocalName {
                status,
                local_name: r.take(248)?.to_vec(),
            },
            HCI_READ_LOCAL_SUPPORTED_CODECS_COMMAND => {
                let n_std = r.u8()? as usize;
                let standard_codec_ids = (0..n_std).map(|_| r.u8()).collect::<Result<Vec<_>>>()?;
                let n_vendor = r.u8()? as usize;
                let vendor_specific_codec_ids = (0..n_vendor)
                    .map(|_| r.u32_le())
                    .collect::<Result<Vec<_>>>()?;
                ReturnParameters::ReadLocalSupportedCodecs {
                    status,
                    standard_codec_ids,
                    vendor_specific_codec_ids,
                }
            }
            HCI_READ_LOCAL_SUPPORTED_CODECS_V2_COMMAND => {
                let n_std = r.u8()? as usize;
                let mut standard_codec_ids = Vec::with_capacity(n_std);
                let mut standard_codec_transports = Vec::with_capacity(n_std);
                for _ in 0..n_std {
                    standard_codec_ids.push(r.u8()?);
                    standard_codec_transports.push(r.u8()?);
                }
                let n_vendor = r.u8()? as usize;
                let mut vendor_specific_codec_ids = Vec::with_capacity(n_vendor);
                let mut vendor_specific_codec_transports = Vec::with_capacity(n_vendor);
                for _ in 0..n_vendor {
                    vendor_specific_codec_ids.push(r.u32_le()?);
                    vendor_specific_codec_transports.push(r.u8()?);
                }
                ReturnParameters::ReadLocalSupportedCodecsV2 {
                    status,
                    standard_codec_ids,
                    standard_codec_transports,
                    vendor_specific_codec_ids,
                    vendor_specific_codec_transports,
                }
            }
            _ => unreachable!("guarded by is_typed"),
        })
    }
}

/// The leading status byte, or SUCCESS if the buffer is empty.
fn first_status(data: &[u8]) -> u8 {
    data.first().copied().unwrap_or(HCI_SUCCESS)
}
