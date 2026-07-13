//! bumble-att — a Rust port of the ATT (Attribute Protocol) PDU codec from
//! [`google/bumble`](https://github.com/google/bumble).
//!
//! **Slice 5** of the incremental port: the ATT PDU framing
//! (`[op_code, payload…]`) and a representative set of request/response/command
//! PDUs. Depends on the `bumble` crate for [`bumble::Uuid`].
//!
//! ## Scope
//!
//! Implemented: Error_Response, Exchange_MTU_Request/Response, Read_Request/
//! Response, Read_Blob_Request/Response, Read_Multiple and
//! Read_Multiple_Variable, Read_By_Type_Request/Response,
//! Read_By_Group_Type_Request/Response, Find_Information_Request/Response,
//! Find_By_Type_Value_Request/Response, Write_Request/Response, Write_Command,
//! Signed_Write_Command, Prepare/Execute_Write,
//! Handle_Value_Notification, and Handle_Value_Indication/Confirmation, with an
//! [`AttPdu::Generic`] fallback. This is the set the GATT client (slice 18)
//! drives for discovery, long reads, writes, and subscriptions.
//!
//! Every PDU subclass registered by upstream `att.py` is represented.

use bumble::Uuid;
use core::fmt;

/// ATT PDU op codes (Vol 3, Part F - 3.4).
pub mod codes {
    pub const ATT_ERROR_RESPONSE: u8 = 0x01;
    pub const ATT_EXCHANGE_MTU_REQUEST: u8 = 0x02;
    pub const ATT_EXCHANGE_MTU_RESPONSE: u8 = 0x03;
    pub const ATT_FIND_INFORMATION_REQUEST: u8 = 0x04;
    pub const ATT_FIND_INFORMATION_RESPONSE: u8 = 0x05;
    pub const ATT_FIND_BY_TYPE_VALUE_REQUEST: u8 = 0x06;
    pub const ATT_FIND_BY_TYPE_VALUE_RESPONSE: u8 = 0x07;
    pub const ATT_READ_BY_TYPE_REQUEST: u8 = 0x08;
    pub const ATT_READ_BY_TYPE_RESPONSE: u8 = 0x09;
    pub const ATT_READ_REQUEST: u8 = 0x0A;
    pub const ATT_READ_RESPONSE: u8 = 0x0B;
    pub const ATT_READ_BLOB_REQUEST: u8 = 0x0C;
    pub const ATT_READ_BLOB_RESPONSE: u8 = 0x0D;
    pub const ATT_READ_MULTIPLE_REQUEST: u8 = 0x0E;
    pub const ATT_READ_MULTIPLE_RESPONSE: u8 = 0x0F;
    pub const ATT_READ_BY_GROUP_TYPE_REQUEST: u8 = 0x10;
    pub const ATT_READ_BY_GROUP_TYPE_RESPONSE: u8 = 0x11;
    pub const ATT_WRITE_REQUEST: u8 = 0x12;
    pub const ATT_WRITE_RESPONSE: u8 = 0x13;
    pub const ATT_PREPARE_WRITE_REQUEST: u8 = 0x16;
    pub const ATT_PREPARE_WRITE_RESPONSE: u8 = 0x17;
    pub const ATT_EXECUTE_WRITE_REQUEST: u8 = 0x18;
    pub const ATT_EXECUTE_WRITE_RESPONSE: u8 = 0x19;
    pub const ATT_WRITE_COMMAND: u8 = 0x52;
    pub const ATT_SIGNED_WRITE_COMMAND: u8 = 0xD2;
    pub const ATT_READ_MULTIPLE_VARIABLE_REQUEST: u8 = 0x20;
    pub const ATT_READ_MULTIPLE_VARIABLE_RESPONSE: u8 = 0x21;
    pub const ATT_HANDLE_VALUE_NOTIFICATION: u8 = 0x1B;
    pub const ATT_HANDLE_VALUE_INDICATION: u8 = 0x1D;
    pub const ATT_HANDLE_VALUE_CONFIRMATION: u8 = 0x1E;
}

/// A common ATT error code.
pub const ATT_ATTRIBUTE_NOT_FOUND_ERROR: u8 = 0x0A;

/// Errors produced while parsing ATT PDUs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    InvalidPacket(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::InvalidPacket(m) => write!(f, "invalid packet: {m}"),
        }
    }
}

impl std::error::Error for Error {}

pub type Result<T> = core::result::Result<T, Error>;

/// An ATT protocol PDU. Typed variants carry decoded fields;
/// [`AttPdu::Generic`] preserves any op code not decoded by this slice.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AttPdu {
    ErrorResponse {
        request_opcode_in_error: u8,
        attribute_handle_in_error: u16,
        error_code: u8,
    },
    ExchangeMtuRequest {
        client_rx_mtu: u16,
    },
    ExchangeMtuResponse {
        server_rx_mtu: u16,
    },
    FindInformationRequest {
        starting_handle: u16,
        ending_handle: u16,
    },
    /// `format` is 1 for 16-bit UUIDs, 2 for 128-bit. `information_data` is a
    /// sequence of `[handle(2), uuid(2 or 16)]` entries.
    FindInformationResponse {
        format: u8,
        information_data: Vec<u8>,
    },
    /// `attribute_type` is always a 16-bit UUID on the wire (Vol 3, Part F -
    /// 3.4.3.3).
    FindByTypeValueRequest {
        starting_handle: u16,
        ending_handle: u16,
        attribute_type: Uuid,
        attribute_value: Vec<u8>,
    },
    /// `handles_information_list` is a sequence of `[found_handle(2),
    /// group_end_handle(2)]` entries.
    FindByTypeValueResponse {
        handles_information_list: Vec<u8>,
    },
    ReadRequest {
        attribute_handle: u16,
    },
    ReadResponse {
        attribute_value: Vec<u8>,
    },
    ReadBlobRequest {
        attribute_handle: u16,
        value_offset: u16,
    },
    ReadBlobResponse {
        part_attribute_value: Vec<u8>,
    },
    ReadMultipleRequest {
        set_of_handles: Vec<u16>,
    },
    ReadMultipleResponse {
        set_of_values: Vec<u8>,
    },
    ReadByTypeRequest {
        starting_handle: u16,
        ending_handle: u16,
        attribute_type: Uuid,
    },
    /// `attribute_data_list` is a sequence of `length`-byte entries, each
    /// `[handle(2), value(length-2)]`.
    ReadByTypeResponse {
        length: u8,
        attribute_data_list: Vec<u8>,
    },
    ReadByGroupTypeRequest {
        starting_handle: u16,
        ending_handle: u16,
        attribute_group_type: Uuid,
    },
    /// `attribute_data_list` is a sequence of `length`-byte entries, each
    /// `[handle(2), end_group_handle(2), value(length-4)]`.
    ReadByGroupTypeResponse {
        length: u8,
        attribute_data_list: Vec<u8>,
    },
    ReadMultipleVariableRequest {
        set_of_handles: Vec<u16>,
    },
    ReadMultipleVariableResponse {
        length_value_tuples: Vec<(u16, Vec<u8>)>,
    },
    WriteRequest {
        attribute_handle: u16,
        attribute_value: Vec<u8>,
    },
    WriteResponse,
    /// Write without a response (op code has the command bit set).
    WriteCommand {
        attribute_handle: u16,
        attribute_value: Vec<u8>,
    },
    SignedWriteCommand {
        attribute_handle: u16,
        attribute_value: Vec<u8>,
    },
    PrepareWriteRequest {
        attribute_handle: u16,
        value_offset: u16,
        part_attribute_value: Vec<u8>,
    },
    PrepareWriteResponse {
        attribute_handle: u16,
        value_offset: u16,
        part_attribute_value: Vec<u8>,
    },
    ExecuteWriteRequest {
        flags: u8,
    },
    ExecuteWriteResponse,
    HandleValueNotification {
        attribute_handle: u16,
        attribute_value: Vec<u8>,
    },
    HandleValueIndication {
        attribute_handle: u16,
        attribute_value: Vec<u8>,
    },
    HandleValueConfirmation,
    /// Any op code not decoded by this slice.
    Generic {
        op_code: u8,
        payload: Vec<u8>,
    },
}

fn push_u16(p: &mut Vec<u8>, v: u16) {
    p.extend_from_slice(&v.to_le_bytes());
}

fn le16(data: &[u8], offset: usize) -> Result<u16> {
    if offset + 2 > data.len() {
        return Err(Error::InvalidPacket("truncated u16 field".into()));
    }
    Ok(u16::from_le_bytes([data[offset], data[offset + 1]]))
}

fn parse_handles(data: &[u8]) -> Result<Vec<u16>> {
    if !data.len().is_multiple_of(2) {
        return Err(Error::InvalidPacket("odd-length ATT handle set".into()));
    }
    Ok(data
        .chunks_exact(2)
        .map(|bytes| u16::from_le_bytes([bytes[0], bytes[1]]))
        .collect())
}

fn parse_length_value_tuples(data: &[u8]) -> Result<Vec<(u16, Vec<u8>)>> {
    let mut tuples = Vec::new();
    let mut offset = 0;
    while offset < data.len() {
        let length = usize::from(le16(data, offset)?);
        offset += 2;
        let end = offset
            .checked_add(length)
            .ok_or_else(|| Error::InvalidPacket("ATT tuple length overflow".into()))?;
        let value = data
            .get(offset..end)
            .ok_or_else(|| Error::InvalidPacket("truncated ATT length/value tuple".into()))?;
        tuples.push((length as u16, value.to_vec()));
        offset = end;
    }
    Ok(tuples)
}

impl AttPdu {
    /// The 1-byte op code.
    pub fn op_code(&self) -> u8 {
        match self {
            AttPdu::ErrorResponse { .. } => codes::ATT_ERROR_RESPONSE,
            AttPdu::ExchangeMtuRequest { .. } => codes::ATT_EXCHANGE_MTU_REQUEST,
            AttPdu::ExchangeMtuResponse { .. } => codes::ATT_EXCHANGE_MTU_RESPONSE,
            AttPdu::FindInformationRequest { .. } => codes::ATT_FIND_INFORMATION_REQUEST,
            AttPdu::FindInformationResponse { .. } => codes::ATT_FIND_INFORMATION_RESPONSE,
            AttPdu::FindByTypeValueRequest { .. } => codes::ATT_FIND_BY_TYPE_VALUE_REQUEST,
            AttPdu::FindByTypeValueResponse { .. } => codes::ATT_FIND_BY_TYPE_VALUE_RESPONSE,
            AttPdu::ReadRequest { .. } => codes::ATT_READ_REQUEST,
            AttPdu::ReadResponse { .. } => codes::ATT_READ_RESPONSE,
            AttPdu::ReadBlobRequest { .. } => codes::ATT_READ_BLOB_REQUEST,
            AttPdu::ReadBlobResponse { .. } => codes::ATT_READ_BLOB_RESPONSE,
            AttPdu::ReadMultipleRequest { .. } => codes::ATT_READ_MULTIPLE_REQUEST,
            AttPdu::ReadMultipleResponse { .. } => codes::ATT_READ_MULTIPLE_RESPONSE,
            AttPdu::ReadByTypeRequest { .. } => codes::ATT_READ_BY_TYPE_REQUEST,
            AttPdu::ReadByTypeResponse { .. } => codes::ATT_READ_BY_TYPE_RESPONSE,
            AttPdu::ReadByGroupTypeRequest { .. } => codes::ATT_READ_BY_GROUP_TYPE_REQUEST,
            AttPdu::ReadByGroupTypeResponse { .. } => codes::ATT_READ_BY_GROUP_TYPE_RESPONSE,
            AttPdu::ReadMultipleVariableRequest { .. } => codes::ATT_READ_MULTIPLE_VARIABLE_REQUEST,
            AttPdu::ReadMultipleVariableResponse { .. } => {
                codes::ATT_READ_MULTIPLE_VARIABLE_RESPONSE
            }
            AttPdu::WriteRequest { .. } => codes::ATT_WRITE_REQUEST,
            AttPdu::WriteResponse => codes::ATT_WRITE_RESPONSE,
            AttPdu::WriteCommand { .. } => codes::ATT_WRITE_COMMAND,
            AttPdu::SignedWriteCommand { .. } => codes::ATT_SIGNED_WRITE_COMMAND,
            AttPdu::PrepareWriteRequest { .. } => codes::ATT_PREPARE_WRITE_REQUEST,
            AttPdu::PrepareWriteResponse { .. } => codes::ATT_PREPARE_WRITE_RESPONSE,
            AttPdu::ExecuteWriteRequest { .. } => codes::ATT_EXECUTE_WRITE_REQUEST,
            AttPdu::ExecuteWriteResponse => codes::ATT_EXECUTE_WRITE_RESPONSE,
            AttPdu::HandleValueNotification { .. } => codes::ATT_HANDLE_VALUE_NOTIFICATION,
            AttPdu::HandleValueIndication { .. } => codes::ATT_HANDLE_VALUE_INDICATION,
            AttPdu::HandleValueConfirmation => codes::ATT_HANDLE_VALUE_CONFIRMATION,
            AttPdu::Generic { op_code, .. } => *op_code,
        }
    }

    /// The PDU payload (the bytes after the op-code byte).
    pub fn payload(&self) -> Vec<u8> {
        let mut p = Vec::new();
        match self {
            AttPdu::ErrorResponse {
                request_opcode_in_error,
                attribute_handle_in_error,
                error_code,
            } => {
                p.push(*request_opcode_in_error);
                push_u16(&mut p, *attribute_handle_in_error);
                p.push(*error_code);
            }
            AttPdu::ExchangeMtuRequest { client_rx_mtu } => push_u16(&mut p, *client_rx_mtu),
            AttPdu::ExchangeMtuResponse { server_rx_mtu } => push_u16(&mut p, *server_rx_mtu),
            AttPdu::FindInformationRequest {
                starting_handle,
                ending_handle,
            } => {
                push_u16(&mut p, *starting_handle);
                push_u16(&mut p, *ending_handle);
            }
            AttPdu::FindInformationResponse {
                format,
                information_data,
            } => {
                p.push(*format);
                p.extend_from_slice(information_data);
            }
            AttPdu::FindByTypeValueRequest {
                starting_handle,
                ending_handle,
                attribute_type,
                attribute_value,
            } => {
                push_u16(&mut p, *starting_handle);
                push_u16(&mut p, *ending_handle);
                p.extend_from_slice(&attribute_type.to_bytes(false));
                p.extend_from_slice(attribute_value);
            }
            AttPdu::FindByTypeValueResponse {
                handles_information_list,
            } => p.extend_from_slice(handles_information_list),
            AttPdu::ReadRequest { attribute_handle } => push_u16(&mut p, *attribute_handle),
            AttPdu::ReadResponse { attribute_value } => p.extend_from_slice(attribute_value),
            AttPdu::ReadBlobRequest {
                attribute_handle,
                value_offset,
            } => {
                push_u16(&mut p, *attribute_handle);
                push_u16(&mut p, *value_offset);
            }
            AttPdu::ReadBlobResponse {
                part_attribute_value,
            } => p.extend_from_slice(part_attribute_value),
            AttPdu::ReadMultipleRequest { set_of_handles }
            | AttPdu::ReadMultipleVariableRequest { set_of_handles } => {
                for handle in set_of_handles {
                    push_u16(&mut p, *handle);
                }
            }
            AttPdu::ReadMultipleResponse { set_of_values } => p.extend_from_slice(set_of_values),
            AttPdu::ReadByTypeRequest {
                starting_handle,
                ending_handle,
                attribute_type,
            } => {
                push_u16(&mut p, *starting_handle);
                push_u16(&mut p, *ending_handle);
                p.extend_from_slice(&attribute_type.to_bytes(false));
            }
            AttPdu::ReadByTypeResponse {
                length,
                attribute_data_list,
            } => {
                p.push(*length);
                p.extend_from_slice(attribute_data_list);
            }
            AttPdu::ReadByGroupTypeRequest {
                starting_handle,
                ending_handle,
                attribute_group_type,
            } => {
                push_u16(&mut p, *starting_handle);
                push_u16(&mut p, *ending_handle);
                p.extend_from_slice(&attribute_group_type.to_bytes(false));
            }
            AttPdu::ReadByGroupTypeResponse {
                length,
                attribute_data_list,
            } => {
                p.push(*length);
                p.extend_from_slice(attribute_data_list);
            }
            AttPdu::ReadMultipleVariableResponse {
                length_value_tuples,
            } => {
                for (length, value) in length_value_tuples {
                    push_u16(&mut p, *length);
                    p.extend_from_slice(value);
                }
            }
            AttPdu::WriteRequest {
                attribute_handle,
                attribute_value,
            } => {
                push_u16(&mut p, *attribute_handle);
                p.extend_from_slice(attribute_value);
            }
            AttPdu::WriteResponse => {}
            AttPdu::WriteCommand {
                attribute_handle,
                attribute_value,
            }
            | AttPdu::SignedWriteCommand {
                attribute_handle,
                attribute_value,
            } => {
                push_u16(&mut p, *attribute_handle);
                p.extend_from_slice(attribute_value);
            }
            AttPdu::PrepareWriteRequest {
                attribute_handle,
                value_offset,
                part_attribute_value,
            }
            | AttPdu::PrepareWriteResponse {
                attribute_handle,
                value_offset,
                part_attribute_value,
            } => {
                push_u16(&mut p, *attribute_handle);
                push_u16(&mut p, *value_offset);
                p.extend_from_slice(part_attribute_value);
            }
            AttPdu::ExecuteWriteRequest { flags } => p.push(*flags),
            AttPdu::ExecuteWriteResponse => {}
            AttPdu::HandleValueNotification {
                attribute_handle,
                attribute_value,
            } => {
                push_u16(&mut p, *attribute_handle);
                p.extend_from_slice(attribute_value);
            }
            AttPdu::HandleValueIndication {
                attribute_handle,
                attribute_value,
            } => {
                push_u16(&mut p, *attribute_handle);
                p.extend_from_slice(attribute_value);
            }
            AttPdu::HandleValueConfirmation => {}
            AttPdu::Generic { payload, .. } => p.extend_from_slice(payload),
        }
        p
    }

    /// Serialize to the full PDU (`[op_code, payload…]`).
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = vec![self.op_code()];
        out.extend_from_slice(&self.payload());
        out
    }

    /// Parse a PDU from its wire bytes.
    pub fn from_bytes(pdu: &[u8]) -> Result<AttPdu> {
        let op_code = *pdu
            .first()
            .ok_or_else(|| Error::InvalidPacket("empty ATT PDU".into()))?;
        let payload = &pdu[1..];

        Ok(match op_code {
            codes::ATT_ERROR_RESPONSE => AttPdu::ErrorResponse {
                request_opcode_in_error: *payload
                    .first()
                    .ok_or_else(|| Error::InvalidPacket("truncated Error_Response".into()))?,
                attribute_handle_in_error: le16(payload, 1)?,
                error_code: *payload
                    .get(3)
                    .ok_or_else(|| Error::InvalidPacket("truncated Error_Response".into()))?,
            },
            codes::ATT_EXCHANGE_MTU_REQUEST => AttPdu::ExchangeMtuRequest {
                client_rx_mtu: le16(payload, 0)?,
            },
            codes::ATT_EXCHANGE_MTU_RESPONSE => AttPdu::ExchangeMtuResponse {
                server_rx_mtu: le16(payload, 0)?,
            },
            codes::ATT_FIND_INFORMATION_REQUEST => AttPdu::FindInformationRequest {
                starting_handle: le16(payload, 0)?,
                ending_handle: le16(payload, 2)?,
            },
            codes::ATT_FIND_INFORMATION_RESPONSE => AttPdu::FindInformationResponse {
                format: *payload.first().ok_or_else(|| {
                    Error::InvalidPacket("truncated Find_Information_Response".into())
                })?,
                information_data: payload.get(1..).unwrap_or(&[]).to_vec(),
            },
            codes::ATT_FIND_BY_TYPE_VALUE_REQUEST => {
                let attribute_type = Uuid::from_bytes(payload.get(4..6).ok_or_else(|| {
                    Error::InvalidPacket("truncated Find_By_Type_Value_Request".into())
                })?)
                .map_err(|e| Error::InvalidPacket(format!("bad type UUID: {e}")))?;
                AttPdu::FindByTypeValueRequest {
                    starting_handle: le16(payload, 0)?,
                    ending_handle: le16(payload, 2)?,
                    attribute_type,
                    attribute_value: payload.get(6..).unwrap_or(&[]).to_vec(),
                }
            }
            codes::ATT_FIND_BY_TYPE_VALUE_RESPONSE => AttPdu::FindByTypeValueResponse {
                handles_information_list: payload.to_vec(),
            },
            codes::ATT_READ_REQUEST => AttPdu::ReadRequest {
                attribute_handle: le16(payload, 0)?,
            },
            codes::ATT_READ_RESPONSE => AttPdu::ReadResponse {
                attribute_value: payload.to_vec(),
            },
            codes::ATT_READ_BLOB_REQUEST => AttPdu::ReadBlobRequest {
                attribute_handle: le16(payload, 0)?,
                value_offset: le16(payload, 2)?,
            },
            codes::ATT_READ_BLOB_RESPONSE => AttPdu::ReadBlobResponse {
                part_attribute_value: payload.to_vec(),
            },
            codes::ATT_READ_MULTIPLE_REQUEST => AttPdu::ReadMultipleRequest {
                set_of_handles: parse_handles(payload)?,
            },
            codes::ATT_READ_MULTIPLE_RESPONSE => AttPdu::ReadMultipleResponse {
                set_of_values: payload.to_vec(),
            },
            codes::ATT_READ_BY_TYPE_REQUEST => {
                let attribute_type = Uuid::from_bytes(payload.get(4..).unwrap_or(&[]))
                    .map_err(|e| Error::InvalidPacket(format!("bad type UUID: {e}")))?;
                AttPdu::ReadByTypeRequest {
                    starting_handle: le16(payload, 0)?,
                    ending_handle: le16(payload, 2)?,
                    attribute_type,
                }
            }
            codes::ATT_READ_BY_TYPE_RESPONSE => AttPdu::ReadByTypeResponse {
                length: *payload.first().ok_or_else(|| {
                    Error::InvalidPacket("truncated Read_By_Type_Response".into())
                })?,
                attribute_data_list: payload.get(1..).unwrap_or(&[]).to_vec(),
            },
            codes::ATT_READ_BY_GROUP_TYPE_REQUEST => {
                let attribute_group_type = Uuid::from_bytes(payload.get(4..).unwrap_or(&[]))
                    .map_err(|e| Error::InvalidPacket(format!("bad group type UUID: {e}")))?;
                AttPdu::ReadByGroupTypeRequest {
                    starting_handle: le16(payload, 0)?,
                    ending_handle: le16(payload, 2)?,
                    attribute_group_type,
                }
            }
            codes::ATT_READ_BY_GROUP_TYPE_RESPONSE => AttPdu::ReadByGroupTypeResponse {
                length: *payload.first().ok_or_else(|| {
                    Error::InvalidPacket("truncated Read_By_Group_Type_Response".into())
                })?,
                attribute_data_list: payload.get(1..).unwrap_or(&[]).to_vec(),
            },
            codes::ATT_READ_MULTIPLE_VARIABLE_REQUEST => AttPdu::ReadMultipleVariableRequest {
                set_of_handles: parse_handles(payload)?,
            },
            codes::ATT_READ_MULTIPLE_VARIABLE_RESPONSE => AttPdu::ReadMultipleVariableResponse {
                length_value_tuples: parse_length_value_tuples(payload)?,
            },
            codes::ATT_WRITE_REQUEST => AttPdu::WriteRequest {
                attribute_handle: le16(payload, 0)?,
                attribute_value: payload.get(2..).unwrap_or(&[]).to_vec(),
            },
            codes::ATT_WRITE_RESPONSE => AttPdu::WriteResponse,
            codes::ATT_WRITE_COMMAND => AttPdu::WriteCommand {
                attribute_handle: le16(payload, 0)?,
                attribute_value: payload.get(2..).unwrap_or(&[]).to_vec(),
            },
            codes::ATT_SIGNED_WRITE_COMMAND => AttPdu::SignedWriteCommand {
                attribute_handle: le16(payload, 0)?,
                attribute_value: payload.get(2..).unwrap_or(&[]).to_vec(),
            },
            codes::ATT_PREPARE_WRITE_REQUEST => AttPdu::PrepareWriteRequest {
                attribute_handle: le16(payload, 0)?,
                value_offset: le16(payload, 2)?,
                part_attribute_value: payload.get(4..).unwrap_or(&[]).to_vec(),
            },
            codes::ATT_PREPARE_WRITE_RESPONSE => AttPdu::PrepareWriteResponse {
                attribute_handle: le16(payload, 0)?,
                value_offset: le16(payload, 2)?,
                part_attribute_value: payload.get(4..).unwrap_or(&[]).to_vec(),
            },
            codes::ATT_EXECUTE_WRITE_REQUEST => AttPdu::ExecuteWriteRequest {
                flags: *payload.first().ok_or_else(|| {
                    Error::InvalidPacket("truncated Execute_Write_Request".into())
                })?,
            },
            codes::ATT_EXECUTE_WRITE_RESPONSE => AttPdu::ExecuteWriteResponse,
            codes::ATT_HANDLE_VALUE_NOTIFICATION => AttPdu::HandleValueNotification {
                attribute_handle: le16(payload, 0)?,
                attribute_value: payload.get(2..).unwrap_or(&[]).to_vec(),
            },
            codes::ATT_HANDLE_VALUE_INDICATION => AttPdu::HandleValueIndication {
                attribute_handle: le16(payload, 0)?,
                attribute_value: payload.get(2..).unwrap_or(&[]).to_vec(),
            },
            codes::ATT_HANDLE_VALUE_CONFIRMATION => AttPdu::HandleValueConfirmation,
            _ => AttPdu::Generic {
                op_code,
                payload: payload.to_vec(),
            },
        })
    }

    /// `true` if the op code's "command" bit (bit 6) is set.
    pub fn is_command(&self) -> bool {
        (self.op_code() >> 6) & 1 == 1
    }

    /// `true` if the op code's "authentication signature" bit (bit 7) is set.
    pub fn is_signed(&self) -> bool {
        (self.op_code() >> 7) & 1 == 1
    }
}
