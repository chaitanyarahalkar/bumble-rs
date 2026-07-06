//! bumble-gatt — a minimal ATT attribute server, the GATT-layer starting point
//! of the [`google/bumble`](https://github.com/google/bumble) port.
//!
//! **Slice 9** of the incremental port: an [`AttServer`] holding an attribute
//! table (handle → value) that turns an incoming ATT request
//! ([`bumble_att::AttPdu`]) into the correct ATT response. This is the piece
//! that makes a real characteristic read/write between two virtual devices
//! work end-to-end (see the crate's integration test, which drives the server
//! over the full HCI/L2CAP/ACL stack).
//!
//! ## Scope
//!
//! Handled requests: Exchange_MTU, Read_Request, Write_Request — with
//! Error_Response for missing attributes and an unsupported-request fallback.
//!
//! Deferred: the full attribute grouping/discovery requests
//! (Find_Information, Read_By_Type, Read_By_Group_Type), notifications /
//! indications, prepared writes, the service/characteristic declaration model,
//! and the GATT client.

use std::collections::BTreeMap;

use bumble_att::{codes, AttPdu};

/// ATT error: the attribute handle was not found.
pub const ATT_ATTRIBUTE_NOT_FOUND_ERROR: u8 = 0x0A;
/// ATT error: the request op code is not supported.
pub const ATT_REQUEST_NOT_SUPPORTED_ERROR: u8 = 0x06;

/// The default ATT MTU (Vol 3, Part F - 3.2.8).
pub const ATT_DEFAULT_MTU: u16 = 23;

/// A minimal ATT server: an attribute table plus request handling.
#[derive(Debug, Clone)]
pub struct AttServer {
    attributes: BTreeMap<u16, Vec<u8>>,
    mtu: u16,
}

impl Default for AttServer {
    fn default() -> Self {
        AttServer {
            attributes: BTreeMap::new(),
            mtu: ATT_DEFAULT_MTU,
        }
    }
}

impl AttServer {
    pub fn new() -> AttServer {
        AttServer::default()
    }

    /// Insert or replace the value at `handle`.
    pub fn set_attribute(&mut self, handle: u16, value: Vec<u8>) {
        self.attributes.insert(handle, value);
    }

    /// The value at `handle`, if present.
    pub fn attribute(&self, handle: u16) -> Option<&[u8]> {
        self.attributes.get(&handle).map(Vec::as_slice)
    }

    /// Turn an incoming ATT request into the appropriate ATT response.
    pub fn on_request(&mut self, request: &AttPdu) -> AttPdu {
        match request {
            AttPdu::ExchangeMtuRequest { .. } => AttPdu::ExchangeMtuResponse {
                server_rx_mtu: self.mtu,
            },
            AttPdu::ReadRequest { attribute_handle } => match self.attributes.get(attribute_handle)
            {
                Some(value) => AttPdu::ReadResponse {
                    attribute_value: value.clone(),
                },
                None => error(
                    codes::ATT_READ_REQUEST,
                    *attribute_handle,
                    ATT_ATTRIBUTE_NOT_FOUND_ERROR,
                ),
            },
            AttPdu::WriteRequest {
                attribute_handle,
                attribute_value,
            } => {
                if let Some(slot) = self.attributes.get_mut(attribute_handle) {
                    *slot = attribute_value.clone();
                    AttPdu::WriteResponse
                } else {
                    error(
                        codes::ATT_WRITE_REQUEST,
                        *attribute_handle,
                        ATT_ATTRIBUTE_NOT_FOUND_ERROR,
                    )
                }
            }
            other => error(other.op_code(), 0, ATT_REQUEST_NOT_SUPPORTED_ERROR),
        }
    }
}

fn error(request_opcode_in_error: u8, attribute_handle_in_error: u16, error_code: u8) -> AttPdu {
    AttPdu::ErrorResponse {
        request_opcode_in_error,
        attribute_handle_in_error,
        error_code,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_existing_and_missing() {
        let mut server = AttServer::new();
        server.set_attribute(0x0025, vec![0xAA, 0xBB]);

        assert_eq!(
            server.on_request(&AttPdu::ReadRequest {
                attribute_handle: 0x0025
            }),
            AttPdu::ReadResponse {
                attribute_value: vec![0xAA, 0xBB]
            }
        );

        assert_eq!(
            server.on_request(&AttPdu::ReadRequest {
                attribute_handle: 0x0099
            }),
            AttPdu::ErrorResponse {
                request_opcode_in_error: codes::ATT_READ_REQUEST,
                attribute_handle_in_error: 0x0099,
                error_code: ATT_ATTRIBUTE_NOT_FOUND_ERROR,
            }
        );
    }

    #[test]
    fn write_existing_updates_value() {
        let mut server = AttServer::new();
        server.set_attribute(0x0025, vec![0x00]);

        assert_eq!(
            server.on_request(&AttPdu::WriteRequest {
                attribute_handle: 0x0025,
                attribute_value: vec![0x11, 0x22],
            }),
            AttPdu::WriteResponse
        );
        assert_eq!(server.attribute(0x0025), Some(&[0x11, 0x22][..]));
    }

    #[test]
    fn write_missing_is_an_error() {
        let mut server = AttServer::new();
        assert_eq!(
            server.on_request(&AttPdu::WriteRequest {
                attribute_handle: 0x0099,
                attribute_value: vec![0x11],
            }),
            AttPdu::ErrorResponse {
                request_opcode_in_error: codes::ATT_WRITE_REQUEST,
                attribute_handle_in_error: 0x0099,
                error_code: ATT_ATTRIBUTE_NOT_FOUND_ERROR,
            }
        );
    }

    #[test]
    fn exchange_mtu_and_unsupported() {
        let mut server = AttServer::new();
        assert_eq!(
            server.on_request(&AttPdu::ExchangeMtuRequest { client_rx_mtu: 517 }),
            AttPdu::ExchangeMtuResponse {
                server_rx_mtu: ATT_DEFAULT_MTU
            }
        );

        // A Handle Value Notification is not a request the server answers.
        let resp = server.on_request(&AttPdu::HandleValueNotification {
            attribute_handle: 1,
            attribute_value: vec![],
        });
        assert!(matches!(
            resp,
            AttPdu::ErrorResponse {
                error_code: ATT_REQUEST_NOT_SUPPORTED_ERROR,
                ..
            }
        ));
    }
}
