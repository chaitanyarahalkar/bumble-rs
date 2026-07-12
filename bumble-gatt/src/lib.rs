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
//! Two servers are provided:
//! - [`AttServer`] — a bare `handle → value` attribute table.
//! - [`GattServer`] — builds a proper attribute database (Primary Service and
//!   Characteristic declarations plus value attributes) from a set of
//!   [`Service`]s, and answers primary discovery (Read_By_Group_Type for
//!   services, Read_By_Type for characteristics) as well as reads and writes.
//!
//! Both implement [`AttRequestHandler`] so the host layer can drive either.
//! The [`GattServer`] also answers Find_Information / Find_By_Type_Value
//! discovery, applies Write_Command, exposes a CCCD descriptor per
//! notify/indicate characteristic, and can emit server-initiated
//! notifications/indications ([`GattServer::notify`] / [`GattServer::indicate`]).
//!
//! **Slice 18** adds the client side: [`GattClient`] drives service /
//! characteristic / descriptor discovery, reads (with long-read via Read_Blob),
//! writes (with and without response), and subscriptions (CCCD write plus
//! notification/indication handling) over an [`AttTransport`]. A blanket impl
//! makes any [`AttRequestHandler`] usable as a transport, so a client and server
//! talk directly in the crate's `client` integration test.
//!
//! Deferred: prepared/queued writes (Prepare/Execute), Read_Multiple, signed
//! writes, and the full async bearer.

use std::collections::BTreeMap;

use bumble::Uuid;
use bumble_att::{codes, AttPdu};

mod client;
pub use client::{
    AttTransport, CharacteristicProxy, DescriptorProxy, GattClient, GattError, ServiceProxy,
};

/// GATT Primary Service declaration attribute type.
pub const GATT_PRIMARY_SERVICE_UUID: u16 = 0x2800;
/// GATT Characteristic declaration attribute type.
pub const GATT_CHARACTERISTIC_UUID: u16 = 0x2803;
/// GATT Client Characteristic Configuration descriptor (CCCD) attribute type.
pub const GATT_CLIENT_CHARACTERISTIC_CONFIGURATION_UUID: u16 = 0x2902;

/// Characteristic property bits (Vol 3, Part G - 3.3.1.1).
pub mod properties {
    pub const READ: u8 = 0x02;
    pub const WRITE_WITHOUT_RESPONSE: u8 = 0x04;
    pub const WRITE: u8 = 0x08;
    pub const NOTIFY: u8 = 0x10;
    pub const INDICATE: u8 = 0x20;
}

/// Something that answers ATT requests. Lets the host layer hold any server
/// (a bare [`AttServer`] or a full [`GattServer`]) behind one interface.
pub trait AttRequestHandler {
    fn handle_request(&mut self, request: &AttPdu) -> AttPdu;
}

impl AttRequestHandler for AttServer {
    fn handle_request(&mut self, request: &AttPdu) -> AttPdu {
        self.on_request(request)
    }
}

impl AttRequestHandler for GattServer {
    fn handle_request(&mut self, request: &AttPdu) -> AttPdu {
        self.on_request(request)
    }
}

/// ATT error: the attribute handle was not found.
pub const ATT_ATTRIBUTE_NOT_FOUND_ERROR: u8 = 0x0A;
/// ATT error: the request op code is not supported.
pub const ATT_REQUEST_NOT_SUPPORTED_ERROR: u8 = 0x06;
/// ATT error: the Read Blob offset is past the end of the attribute value.
pub const ATT_INVALID_OFFSET_ERROR: u8 = 0x07;

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

/// The first `max` bytes of `value` (all of it when shorter).
fn truncate(value: &[u8], max: usize) -> Vec<u8> {
    value[..value.len().min(max)].to_vec()
}

/// A GATT characteristic to expose on a [`GattServer`].
#[derive(Clone, Debug)]
pub struct Characteristic {
    pub uuid: Uuid,
    pub properties: u8,
    pub value: Vec<u8>,
}

/// A GATT primary service and its characteristics.
#[derive(Clone, Debug)]
pub struct Service {
    pub uuid: Uuid,
    pub characteristics: Vec<Characteristic>,
}

/// One entry in the flat attribute database a [`GattServer`] builds from its
/// services.
#[derive(Clone, Debug)]
struct Attribute {
    handle: u16,
    type_uuid: Uuid,
    /// For a service declaration, the last handle of the service group;
    /// otherwise the attribute's own handle.
    end_group_handle: u16,
    value: Vec<u8>,
}

/// A GATT server: builds a proper attribute database (service and
/// characteristic declarations plus characteristic values) from a set of
/// [`Service`]s, and answers reads, writes, and the primary discovery requests.
#[derive(Clone, Debug)]
pub struct GattServer {
    attributes: Vec<Attribute>,
    /// The MTU this server can receive (returned in Exchange MTU Response).
    mtu: u16,
    /// The negotiated MTU (min of both peers), used to size read responses.
    negotiated_mtu: u16,
}

impl GattServer {
    /// Build the attribute database for the given services. Handles are
    /// assigned sequentially from 1, with the standard layout: a Primary
    /// Service declaration, then per characteristic a declaration followed by
    /// its value attribute.
    pub fn new(services: Vec<Service>) -> GattServer {
        let mut attributes: Vec<Attribute> = Vec::new();
        let mut handle: u16 = 1;

        for service in services {
            let service_index = attributes.len();
            let service_handle = handle;
            handle += 1;
            attributes.push(Attribute {
                handle: service_handle,
                type_uuid: Uuid::from_16_bits(GATT_PRIMARY_SERVICE_UUID),
                end_group_handle: service_handle,
                value: service.uuid.to_bytes(false),
            });

            for ch in service.characteristics {
                let decl_handle = handle;
                handle += 1;
                let value_handle = handle;
                handle += 1;

                let mut declaration = Vec::with_capacity(3 + 16);
                declaration.push(ch.properties);
                declaration.extend_from_slice(&value_handle.to_le_bytes());
                declaration.extend_from_slice(&ch.uuid.to_bytes(false));
                attributes.push(Attribute {
                    handle: decl_handle,
                    type_uuid: Uuid::from_16_bits(GATT_CHARACTERISTIC_UUID),
                    end_group_handle: decl_handle,
                    value: declaration,
                });
                attributes.push(Attribute {
                    handle: value_handle,
                    type_uuid: ch.uuid.clone(),
                    end_group_handle: value_handle,
                    value: ch.value,
                });

                // A notify/indicate characteristic gets a Client Characteristic
                // Configuration descriptor, initialised to "disabled".
                if ch.properties & (properties::NOTIFY | properties::INDICATE) != 0 {
                    let cccd_handle = handle;
                    handle += 1;
                    attributes.push(Attribute {
                        handle: cccd_handle,
                        type_uuid: Uuid::from_16_bits(
                            GATT_CLIENT_CHARACTERISTIC_CONFIGURATION_UUID,
                        ),
                        end_group_handle: cccd_handle,
                        value: vec![0x00, 0x00],
                    });
                }
            }

            attributes[service_index].end_group_handle = handle - 1;
        }

        GattServer {
            attributes,
            mtu: ATT_DEFAULT_MTU,
            negotiated_mtu: ATT_DEFAULT_MTU,
        }
    }

    fn find(&self, handle: u16) -> Option<&Attribute> {
        self.attributes.iter().find(|a| a.handle == handle)
    }

    fn find_mut(&mut self, handle: u16) -> Option<&mut Attribute> {
        self.attributes.iter_mut().find(|a| a.handle == handle)
    }

    /// Turn an incoming ATT request into a response.
    pub fn on_request(&mut self, request: &AttPdu) -> AttPdu {
        match request {
            AttPdu::ExchangeMtuRequest { client_rx_mtu } => {
                self.negotiated_mtu = (*client_rx_mtu).min(self.mtu).max(ATT_DEFAULT_MTU);
                AttPdu::ExchangeMtuResponse {
                    server_rx_mtu: self.mtu,
                }
            }
            AttPdu::ReadRequest { attribute_handle } => match self.find(*attribute_handle) {
                Some(a) => AttPdu::ReadResponse {
                    // A Read Response carries at most MTU-1 bytes; longer
                    // values are fetched with Read Blob.
                    attribute_value: truncate(&a.value, (self.negotiated_mtu - 1) as usize),
                },
                None => error(
                    codes::ATT_READ_REQUEST,
                    *attribute_handle,
                    ATT_ATTRIBUTE_NOT_FOUND_ERROR,
                ),
            },
            AttPdu::ReadBlobRequest {
                attribute_handle,
                value_offset,
            } => match self.find(*attribute_handle) {
                Some(a) => {
                    let offset = *value_offset as usize;
                    if offset > a.value.len() {
                        error(
                            codes::ATT_READ_BLOB_REQUEST,
                            *attribute_handle,
                            ATT_INVALID_OFFSET_ERROR,
                        )
                    } else {
                        let end = (offset + (self.negotiated_mtu - 1) as usize).min(a.value.len());
                        AttPdu::ReadBlobResponse {
                            part_attribute_value: a.value[offset..end].to_vec(),
                        }
                    }
                }
                None => error(
                    codes::ATT_READ_BLOB_REQUEST,
                    *attribute_handle,
                    ATT_ATTRIBUTE_NOT_FOUND_ERROR,
                ),
            },
            AttPdu::WriteRequest {
                attribute_handle,
                attribute_value,
            } => match self.find_mut(*attribute_handle) {
                Some(a) => {
                    a.value = attribute_value.clone();
                    AttPdu::WriteResponse
                }
                None => error(
                    codes::ATT_WRITE_REQUEST,
                    *attribute_handle,
                    ATT_ATTRIBUTE_NOT_FOUND_ERROR,
                ),
            },
            AttPdu::ReadByGroupTypeRequest {
                starting_handle,
                ending_handle,
                attribute_group_type,
            } => self.read_by_group_type(*starting_handle, *ending_handle, attribute_group_type),
            AttPdu::ReadByTypeRequest {
                starting_handle,
                ending_handle,
                attribute_type,
            } => self.read_by_type(*starting_handle, *ending_handle, attribute_type),
            AttPdu::FindInformationRequest {
                starting_handle,
                ending_handle,
            } => self.find_information(*starting_handle, *ending_handle),
            AttPdu::FindByTypeValueRequest {
                starting_handle,
                ending_handle,
                attribute_type,
                attribute_value,
            } => self.find_by_type_value(
                *starting_handle,
                *ending_handle,
                attribute_type,
                attribute_value,
            ),
            AttPdu::WriteCommand {
                attribute_handle,
                attribute_value,
            } => {
                // A command has no response; apply it best-effort.
                if let Some(a) = self.find_mut(*attribute_handle) {
                    a.value = attribute_value.clone();
                }
                // Callers ignore the returned PDU for commands; surface a no-op.
                AttPdu::HandleValueConfirmation
            }
            other => error(other.op_code(), 0, ATT_REQUEST_NOT_SUPPORTED_ERROR),
        }
    }

    /// Build a server-initiated Handle Value Notification for `value_handle`.
    pub fn notify(&self, value_handle: u16, value: Vec<u8>) -> AttPdu {
        AttPdu::HandleValueNotification {
            attribute_handle: value_handle,
            attribute_value: value,
        }
    }

    /// Build a server-initiated Handle Value Indication for `value_handle`.
    pub fn indicate(&self, value_handle: u16, value: Vec<u8>) -> AttPdu {
        AttPdu::HandleValueIndication {
            attribute_handle: value_handle,
            attribute_value: value,
        }
    }

    /// Find Information: return the `(handle, type)` pairs in range. `format`
    /// is 1 when every type is a 16-bit UUID, else 2 (Vol 3, Part F - 3.4.3.1).
    fn find_information(&self, start: u16, end: u16) -> AttPdu {
        let matches: Vec<&Attribute> = self
            .attributes
            .iter()
            .filter(|a| a.handle >= start && a.handle <= end)
            .collect();
        let Some(first) = matches.first() else {
            return error(
                codes::ATT_FIND_INFORMATION_REQUEST,
                start,
                ATT_ATTRIBUTE_NOT_FOUND_ERROR,
            );
        };

        // A response groups only entries whose UUID width matches the first.
        let uuid_len = first.type_uuid.to_bytes(false).len();
        let format = if uuid_len == 2 { 1u8 } else { 2u8 };
        let mut data = Vec::new();
        for a in matches
            .iter()
            .take_while(|a| a.type_uuid.to_bytes(false).len() == uuid_len)
        {
            data.extend_from_slice(&a.handle.to_le_bytes());
            data.extend_from_slice(&a.type_uuid.to_bytes(false));
        }
        AttPdu::FindInformationResponse {
            format,
            information_data: data,
        }
    }

    /// Find By Type Value: return the `(handle, group_end)` pairs whose
    /// attribute type and value match (Vol 3, Part F - 3.4.3.3).
    fn find_by_type_value(
        &self,
        start: u16,
        end: u16,
        attribute_type: &Uuid,
        attribute_value: &[u8],
    ) -> AttPdu {
        let mut list = Vec::new();
        for a in self.attributes.iter().filter(|a| {
            a.handle >= start
                && a.handle <= end
                && a.type_uuid == *attribute_type
                && a.value == attribute_value
        }) {
            list.extend_from_slice(&a.handle.to_le_bytes());
            list.extend_from_slice(&a.end_group_handle.to_le_bytes());
        }
        if list.is_empty() {
            return error(
                codes::ATT_FIND_BY_TYPE_VALUE_REQUEST,
                start,
                ATT_ATTRIBUTE_NOT_FOUND_ERROR,
            );
        }
        AttPdu::FindByTypeValueResponse {
            handles_information_list: list,
        }
    }

    fn read_by_group_type(&self, start: u16, end: u16, group_type: &Uuid) -> AttPdu {
        // Grouping attributes (services) in range with the matching type.
        let matches: Vec<&Attribute> = self
            .attributes
            .iter()
            .filter(|a| a.handle >= start && a.handle <= end && a.type_uuid == *group_type)
            .collect();
        let Some(first) = matches.first() else {
            return error(
                codes::ATT_READ_BY_GROUP_TYPE_REQUEST,
                start,
                ATT_ATTRIBUTE_NOT_FOUND_ERROR,
            );
        };

        // A response groups only entries whose value has the same length.
        let value_len = first.value.len();
        let mut adl = Vec::new();
        for a in matches.iter().take_while(|a| a.value.len() == value_len) {
            adl.extend_from_slice(&a.handle.to_le_bytes());
            adl.extend_from_slice(&a.end_group_handle.to_le_bytes());
            adl.extend_from_slice(&a.value);
        }
        AttPdu::ReadByGroupTypeResponse {
            length: (4 + value_len) as u8,
            attribute_data_list: adl,
        }
    }

    fn read_by_type(&self, start: u16, end: u16, attribute_type: &Uuid) -> AttPdu {
        let matches: Vec<&Attribute> = self
            .attributes
            .iter()
            .filter(|a| a.handle >= start && a.handle <= end && a.type_uuid == *attribute_type)
            .collect();
        let Some(first) = matches.first() else {
            return error(
                codes::ATT_READ_BY_TYPE_REQUEST,
                start,
                ATT_ATTRIBUTE_NOT_FOUND_ERROR,
            );
        };

        let value_len = first.value.len();
        let mut adl = Vec::new();
        for a in matches.iter().take_while(|a| a.value.len() == value_len) {
            adl.extend_from_slice(&a.handle.to_le_bytes());
            adl.extend_from_slice(&a.value);
        }
        AttPdu::ReadByTypeResponse {
            length: (2 + value_len) as u8,
            attribute_data_list: adl,
        }
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

    fn sample_gatt_server() -> GattServer {
        // Device Information service (0x180A) with a Device Name char (0x2A00).
        GattServer::new(vec![Service {
            uuid: Uuid::from_16_bits(0x180A),
            characteristics: vec![Characteristic {
                uuid: Uuid::from_16_bits(0x2A00),
                properties: 0x02, // READ
                value: b"Hi".to_vec(),
            }],
        }])
    }

    #[test]
    fn gatt_discover_services() {
        let mut server = sample_gatt_server();
        // Service decl=1, char decl=2, char value=3 → service group is 1..=3.
        let resp = server.on_request(&AttPdu::ReadByGroupTypeRequest {
            starting_handle: 0x0001,
            ending_handle: 0xFFFF,
            attribute_group_type: Uuid::from_16_bits(GATT_PRIMARY_SERVICE_UUID),
        });
        assert_eq!(
            resp,
            AttPdu::ReadByGroupTypeResponse {
                length: 6, // handle(2) + end_group(2) + 16-bit uuid(2)
                // handle=0x0001, end_group=0x0003, service uuid=0x180A
                attribute_data_list: vec![0x01, 0x00, 0x03, 0x00, 0x0A, 0x18],
            }
        );
    }

    #[test]
    fn gatt_discover_characteristics_then_read() {
        let mut server = sample_gatt_server();
        let resp = server.on_request(&AttPdu::ReadByTypeRequest {
            starting_handle: 0x0001,
            ending_handle: 0xFFFF,
            attribute_type: Uuid::from_16_bits(GATT_CHARACTERISTIC_UUID),
        });
        // char decl handle=0x0002, value = [props=0x02, value_handle=0x0003, uuid=0x2A00]
        assert_eq!(
            resp,
            AttPdu::ReadByTypeResponse {
                length: 7,
                attribute_data_list: vec![0x02, 0x00, 0x02, 0x03, 0x00, 0x00, 0x2A],
            }
        );

        // The value handle (0x0003) reads back the characteristic value.
        assert_eq!(
            server.on_request(&AttPdu::ReadRequest {
                attribute_handle: 0x0003
            }),
            AttPdu::ReadResponse {
                attribute_value: b"Hi".to_vec()
            }
        );
    }

    #[test]
    fn gatt_discovery_empty_range_is_error() {
        let mut server = sample_gatt_server();
        let resp = server.on_request(&AttPdu::ReadByGroupTypeRequest {
            starting_handle: 0x0010,
            ending_handle: 0xFFFF,
            attribute_group_type: Uuid::from_16_bits(GATT_PRIMARY_SERVICE_UUID),
        });
        assert!(matches!(resp, AttPdu::ErrorResponse { .. }));
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
