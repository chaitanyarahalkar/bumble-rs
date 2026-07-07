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
//! Response, Read_By_Type_Request/Response, Read_By_Group_Type_Request/Response,
//! Write_Request/Response, and Handle_Value_Notification, with an
//! [`AttPdu::Generic`] fallback.
//!
//! Deferred: the remaining ATT PDUs (Find_Information, prepared/queued writes,
//! signed writes, indications).

use bumble::Uuid;
use core::fmt;

/// ATT PDU op codes (Vol 3, Part F - 3.4).
pub mod codes {
    pub const ATT_ERROR_RESPONSE: u8 = 0x01;
    pub const ATT_EXCHANGE_MTU_REQUEST: u8 = 0x02;
    pub const ATT_EXCHANGE_MTU_RESPONSE: u8 = 0x03;
    pub const ATT_READ_BY_TYPE_REQUEST: u8 = 0x08;
    pub const ATT_READ_BY_TYPE_RESPONSE: u8 = 0x09;
    pub const ATT_READ_REQUEST: u8 = 0x0A;
    pub const ATT_READ_RESPONSE: u8 = 0x0B;
    pub const ATT_READ_BY_GROUP_TYPE_REQUEST: u8 = 0x10;
    pub const ATT_READ_BY_GROUP_TYPE_RESPONSE: u8 = 0x11;
    pub const ATT_WRITE_REQUEST: u8 = 0x12;
    pub const ATT_WRITE_RESPONSE: u8 = 0x13;
    pub const ATT_HANDLE_VALUE_NOTIFICATION: u8 = 0x1B;
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
    ReadRequest {
        attribute_handle: u16,
    },
    ReadResponse {
        attribute_value: Vec<u8>,
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
    WriteRequest {
        attribute_handle: u16,
        attribute_value: Vec<u8>,
    },
    WriteResponse,
    HandleValueNotification {
        attribute_handle: u16,
        attribute_value: Vec<u8>,
    },
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

impl AttPdu {
    /// The 1-byte op code.
    pub fn op_code(&self) -> u8 {
        match self {
            AttPdu::ErrorResponse { .. } => codes::ATT_ERROR_RESPONSE,
            AttPdu::ExchangeMtuRequest { .. } => codes::ATT_EXCHANGE_MTU_REQUEST,
            AttPdu::ExchangeMtuResponse { .. } => codes::ATT_EXCHANGE_MTU_RESPONSE,
            AttPdu::ReadRequest { .. } => codes::ATT_READ_REQUEST,
            AttPdu::ReadResponse { .. } => codes::ATT_READ_RESPONSE,
            AttPdu::ReadByTypeRequest { .. } => codes::ATT_READ_BY_TYPE_REQUEST,
            AttPdu::ReadByTypeResponse { .. } => codes::ATT_READ_BY_TYPE_RESPONSE,
            AttPdu::ReadByGroupTypeRequest { .. } => codes::ATT_READ_BY_GROUP_TYPE_REQUEST,
            AttPdu::ReadByGroupTypeResponse { .. } => codes::ATT_READ_BY_GROUP_TYPE_RESPONSE,
            AttPdu::WriteRequest { .. } => codes::ATT_WRITE_REQUEST,
            AttPdu::WriteResponse => codes::ATT_WRITE_RESPONSE,
            AttPdu::HandleValueNotification { .. } => codes::ATT_HANDLE_VALUE_NOTIFICATION,
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
            AttPdu::ReadRequest { attribute_handle } => push_u16(&mut p, *attribute_handle),
            AttPdu::ReadResponse { attribute_value } => p.extend_from_slice(attribute_value),
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
            AttPdu::WriteRequest {
                attribute_handle,
                attribute_value,
            } => {
                push_u16(&mut p, *attribute_handle);
                p.extend_from_slice(attribute_value);
            }
            AttPdu::WriteResponse => {}
            AttPdu::HandleValueNotification {
                attribute_handle,
                attribute_value,
            } => {
                push_u16(&mut p, *attribute_handle);
                p.extend_from_slice(attribute_value);
            }
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
            codes::ATT_READ_REQUEST => AttPdu::ReadRequest {
                attribute_handle: le16(payload, 0)?,
            },
            codes::ATT_READ_RESPONSE => AttPdu::ReadResponse {
                attribute_value: payload.to_vec(),
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
            codes::ATT_WRITE_REQUEST => AttPdu::WriteRequest {
                attribute_handle: le16(payload, 0)?,
                attribute_value: payload.get(2..).unwrap_or(&[]).to_vec(),
            },
            codes::ATT_WRITE_RESPONSE => AttPdu::WriteResponse,
            codes::ATT_HANDLE_VALUE_NOTIFICATION => AttPdu::HandleValueNotification {
                attribute_handle: le16(payload, 0)?,
                attribute_value: payload.get(2..).unwrap_or(&[]).to_vec(),
            },
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
