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
//! Read Multiple/Variable, CSRK-authenticated signed commands with replay
//! protection, and atomic Prepare/Execute queued writes are supported. The remaining architectural difference is the async
//! bearer/event convenience layer.

use std::collections::BTreeMap;
use std::sync::Arc;

use bumble::Uuid;
use bumble_att::{codes, AttPdu, SignedWriteVerifier};

mod adapters;
pub use adapters::*;
mod client;
pub use client::{
    AttTransport, CharacteristicProxy, DescriptorProxy, GattClient, GattError, ServiceProxy,
};

/// GATT Primary Service declaration attribute type.
pub const GATT_PRIMARY_SERVICE_UUID: u16 = 0x2800;
/// GATT Secondary Service declaration attribute type.
pub const GATT_SECONDARY_SERVICE_UUID: u16 = 0x2801;
/// GATT Include declaration attribute type.
pub const GATT_INCLUDE_UUID: u16 = 0x2802;
/// GATT Characteristic declaration attribute type.
pub const GATT_CHARACTERISTIC_UUID: u16 = 0x2803;
/// GATT Client Characteristic Configuration descriptor (CCCD) attribute type.
pub const GATT_CLIENT_CHARACTERISTIC_CONFIGURATION_UUID: u16 = 0x2902;

/// Characteristic property bits (Vol 3, Part G - 3.3.1.1).
pub mod properties {
    pub const BROADCAST: u8 = 0x01;
    pub const READ: u8 = 0x02;
    pub const WRITE_WITHOUT_RESPONSE: u8 = 0x04;
    pub const WRITE: u8 = 0x08;
    pub const NOTIFY: u8 = 0x10;
    pub const INDICATE: u8 = 0x20;
    pub const AUTHENTICATED_SIGNED_WRITES: u8 = 0x40;
    pub const EXTENDED_PROPERTIES: u8 = 0x80;
}

/// Attribute access and security requirement bits, matching
/// `bumble.att.Attribute.Permissions`.
pub mod permissions {
    pub const READABLE: u8 = 0x01;
    pub const WRITEABLE: u8 = 0x02;
    pub const READ_REQUIRES_ENCRYPTION: u8 = 0x04;
    pub const WRITE_REQUIRES_ENCRYPTION: u8 = 0x08;
    pub const READ_REQUIRES_AUTHENTICATION: u8 = 0x10;
    pub const WRITE_REQUIRES_AUTHENTICATION: u8 = 0x20;
    pub const READ_REQUIRES_AUTHORIZATION: u8 = 0x40;
    pub const WRITE_REQUIRES_AUTHORIZATION: u8 = 0x80;
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
/// ATT error: the request fields are invalid for this opcode.
pub const ATT_INVALID_PDU_ERROR: u8 = 0x04;
/// ATT error: this attribute does not permit reads.
pub const ATT_READ_NOT_PERMITTED_ERROR: u8 = 0x02;
/// ATT error: this attribute does not permit writes.
pub const ATT_WRITE_NOT_PERMITTED_ERROR: u8 = 0x03;
/// ATT error: the connection is not authenticated.
pub const ATT_INSUFFICIENT_AUTHENTICATION_ERROR: u8 = 0x05;
/// ATT error: the client is not authorized.
pub const ATT_INSUFFICIENT_AUTHORIZATION_ERROR: u8 = 0x08;
/// ATT error: the connection is not encrypted.
pub const ATT_INSUFFICIENT_ENCRYPTION_ERROR: u8 = 0x0F;
/// ATT error: queued writes are not supported for this attribute.
pub const ATT_ATTRIBUTE_NOT_LONG_ERROR: u8 = 0x0B;

/// The default ATT MTU (Vol 3, Part F - 3.2.8).
pub const ATT_DEFAULT_MTU: u16 = 23;

/// A minimal ATT server: an attribute table plus request handling.
#[derive(Debug, Clone)]
pub struct AttServer {
    attributes: BTreeMap<u16, Vec<u8>>,
    mtu: u16,
    prepared_writes: Vec<(u16, u16, Vec<u8>)>,
    signed_write_verifier: Option<SignedWriteVerifier>,
}

impl Default for AttServer {
    fn default() -> Self {
        AttServer {
            attributes: BTreeMap::new(),
            mtu: ATT_DEFAULT_MTU,
            prepared_writes: Vec::new(),
            signed_write_verifier: None,
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

    pub fn prepared_write_count(&self) -> usize {
        self.prepared_writes.len()
    }

    pub fn set_signed_write_key(&mut self, csrk: [u8; 16], last_counter: Option<u32>) {
        self.signed_write_verifier = Some(SignedWriteVerifier::new(csrk, last_counter));
    }

    pub fn signed_write_counter(&self) -> Option<u32> {
        self.signed_write_verifier
            .as_ref()
            .and_then(SignedWriteVerifier::last_counter)
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
            AttPdu::ReadMultipleRequest { set_of_handles } => {
                self.read_multiple(set_of_handles, false)
            }
            AttPdu::ReadMultipleVariableRequest { set_of_handles } => {
                self.read_multiple(set_of_handles, true)
            }
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
            AttPdu::WriteCommand {
                attribute_handle,
                attribute_value,
            } => {
                if let Some(slot) = self.attributes.get_mut(attribute_handle) {
                    *slot = attribute_value.clone();
                }
                AttPdu::HandleValueConfirmation
            }
            AttPdu::SignedWriteCommand {
                attribute_handle,
                attribute_value,
                ..
            } => {
                let verified = self
                    .signed_write_verifier
                    .as_mut()
                    .is_some_and(|verifier| verifier.verify(request));
                if verified {
                    if let Some(slot) = self.attributes.get_mut(attribute_handle) {
                        *slot = attribute_value.clone();
                    }
                }
                AttPdu::HandleValueConfirmation
            }
            AttPdu::PrepareWriteRequest {
                attribute_handle,
                value_offset,
                part_attribute_value,
            } => {
                if !self.attributes.contains_key(attribute_handle) {
                    error(
                        codes::ATT_PREPARE_WRITE_REQUEST,
                        *attribute_handle,
                        ATT_ATTRIBUTE_NOT_FOUND_ERROR,
                    )
                } else {
                    self.prepared_writes.push((
                        *attribute_handle,
                        *value_offset,
                        part_attribute_value.clone(),
                    ));
                    AttPdu::PrepareWriteResponse {
                        attribute_handle: *attribute_handle,
                        value_offset: *value_offset,
                        part_attribute_value: part_attribute_value.clone(),
                    }
                }
            }
            AttPdu::ExecuteWriteRequest { flags } => self.execute_writes(*flags),
            other => error(other.op_code(), 0, ATT_REQUEST_NOT_SUPPORTED_ERROR),
        }
    }

    fn read_multiple(&self, handles: &[u16], variable: bool) -> AttPdu {
        let mut remaining = usize::from(self.mtu - 1);
        if variable {
            let mut tuples = Vec::new();
            for handle in handles {
                let Some(value) = self.attributes.get(handle) else {
                    return error(
                        codes::ATT_READ_MULTIPLE_VARIABLE_REQUEST,
                        *handle,
                        ATT_ATTRIBUTE_NOT_FOUND_ERROR,
                    );
                };
                let part = truncate(value, usize::from(self.mtu - 3).min(251));
                if part.len() + 2 > remaining {
                    break;
                }
                tuples.push((value.len().min(u16::MAX as usize) as u16, part));
                remaining -= tuples.last().expect("tuple inserted").1.len() + 2;
            }
            AttPdu::ReadMultipleVariableResponse {
                length_value_tuples: tuples,
            }
        } else {
            let mut values = Vec::new();
            for handle in handles {
                let Some(value) = self.attributes.get(handle) else {
                    return error(
                        codes::ATT_READ_MULTIPLE_REQUEST,
                        *handle,
                        ATT_ATTRIBUTE_NOT_FOUND_ERROR,
                    );
                };
                let part = truncate(value, usize::from(self.mtu - 1).min(251));
                if part.len() > remaining {
                    break;
                }
                remaining -= part.len();
                values.extend_from_slice(&part);
            }
            AttPdu::ReadMultipleResponse {
                set_of_values: values,
            }
        }
    }

    fn execute_writes(&mut self, flags: u8) -> AttPdu {
        if flags == 0 {
            self.prepared_writes.clear();
            return AttPdu::ExecuteWriteResponse;
        }
        if flags != 1 {
            return error(codes::ATT_EXECUTE_WRITE_REQUEST, 0, ATT_INVALID_PDU_ERROR);
        }
        let prepared_writes = core::mem::take(&mut self.prepared_writes);
        let mut staged = self.attributes.clone();
        for (handle, offset, part) in &prepared_writes {
            let Some(value) = staged.get_mut(handle) else {
                return error(
                    codes::ATT_EXECUTE_WRITE_REQUEST,
                    *handle,
                    ATT_ATTRIBUTE_NOT_FOUND_ERROR,
                );
            };
            let offset = usize::from(*offset);
            if offset > value.len() {
                return error(
                    codes::ATT_EXECUTE_WRITE_REQUEST,
                    *handle,
                    ATT_INVALID_OFFSET_ERROR,
                );
            }
            let end = offset + part.len();
            if end > value.len() {
                value.resize(end, 0);
            }
            value[offset..end].copy_from_slice(part);
        }
        self.attributes = staged;
        AttPdu::ExecuteWriteResponse
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

/// Security state associated with the ATT bearer handling a request.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AccessContext {
    /// Stable caller-supplied identity for per-bearer values such as CCCDs.
    pub bearer_id: u64,
    pub encrypted: bool,
    pub authenticated: bool,
    pub authorized: bool,
}

/// A synchronous dynamic attribute read callback.
pub type ReadCallback = dyn Fn(AccessContext) -> Result<Vec<u8>, u8> + Send + Sync + 'static;
/// A synchronous dynamic attribute write callback.
pub type WriteCallback = dyn Fn(AccessContext, &[u8]) -> Result<(), u8> + Send + Sync + 'static;

/// Read/write callbacks backing a dynamic attribute value.
#[derive(Clone, Default)]
pub struct DynamicValue {
    read: Option<Arc<ReadCallback>>,
    write: Option<Arc<WriteCallback>>,
}

impl core::fmt::Debug for DynamicValue {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("DynamicValue")
            .field("read", &self.read.is_some())
            .field("write", &self.write.is_some())
            .finish()
    }
}

impl DynamicValue {
    pub fn read_only<F>(read: F) -> Self
    where
        F: Fn(AccessContext) -> Result<Vec<u8>, u8> + Send + Sync + 'static,
    {
        Self {
            read: Some(Arc::new(read)),
            write: None,
        }
    }

    pub fn write_only<F>(write: F) -> Self
    where
        F: Fn(AccessContext, &[u8]) -> Result<(), u8> + Send + Sync + 'static,
    {
        Self {
            read: None,
            write: Some(Arc::new(write)),
        }
    }

    pub fn read_write<R, W>(read: R, write: W) -> Self
    where
        R: Fn(AccessContext) -> Result<Vec<u8>, u8> + Send + Sync + 'static,
        W: Fn(AccessContext, &[u8]) -> Result<(), u8> + Send + Sync + 'static,
    {
        Self {
            read: Some(Arc::new(read)),
            write: Some(Arc::new(write)),
        }
    }

    fn read(&self, context: AccessContext) -> Result<Vec<u8>, u8> {
        self.read.as_ref().ok_or(ATT_READ_NOT_PERMITTED_ERROR)?(context)
    }

    fn write(&self, context: AccessContext, value: &[u8]) -> Result<(), u8> {
        self.write.as_ref().ok_or(ATT_WRITE_NOT_PERMITTED_ERROR)?(context, value)
    }
}

/// A descriptor in the complete GATT database-definition API.
#[derive(Clone, Debug)]
pub struct DescriptorDefinition {
    pub uuid: Uuid,
    pub permissions: u8,
    pub value: Vec<u8>,
}

/// A characteristic in the complete GATT database-definition API.
#[derive(Clone, Debug)]
pub struct CharacteristicDefinition {
    pub uuid: Uuid,
    pub properties: u8,
    pub permissions: u8,
    pub value: Vec<u8>,
    pub descriptors: Vec<DescriptorDefinition>,
}

/// A primary or secondary service. Included services are indices into the
/// complete definitions slice passed to [`GattServer::from_definitions`].
#[derive(Clone, Debug)]
pub struct ServiceDefinition {
    pub uuid: Uuid,
    pub primary: bool,
    pub included_services: Vec<usize>,
    pub characteristics: Vec<CharacteristicDefinition>,
}

/// An invalid GATT database definition.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DatabaseError {
    InvalidIncludedService { service: usize, included: usize },
    UnknownAttribute(u16),
    TooManyAttributes,
}

impl core::fmt::Display for DatabaseError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::InvalidIncludedService { service, included } => write!(
                f,
                "service {service} includes missing service index {included}"
            ),
            Self::UnknownAttribute(handle) => {
                write!(f, "attribute handle 0x{handle:04X} does not exist")
            }
            Self::TooManyAttributes => f.write_str("GATT database exceeds 65535 attributes"),
        }
    }
}

impl std::error::Error for DatabaseError {}

/// One entry in the flat attribute database a [`GattServer`] builds from its
/// services.
#[derive(Clone, Debug)]
struct Attribute {
    handle: u16,
    type_uuid: Uuid,
    /// For a service declaration, the last handle of the service group;
    /// otherwise the attribute's own handle.
    end_group_handle: u16,
    permissions: u8,
    value: Vec<u8>,
    dynamic_value: Option<DynamicValue>,
}

impl Attribute {
    fn read_value(&self, context: AccessContext) -> Result<Vec<u8>, u8> {
        match &self.dynamic_value {
            Some(dynamic) => dynamic.read(context),
            None => Ok(self.value.clone()),
        }
    }

    fn write_value(&mut self, context: AccessContext, value: &[u8]) -> Result<(), u8> {
        match &self.dynamic_value {
            Some(dynamic) => dynamic.write(context, value),
            None => {
                self.value = value.to_vec();
                Ok(())
            }
        }
    }
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
    prepared_writes: Vec<(u16, u16, Vec<u8>)>,
    signed_write_verifier: Option<SignedWriteVerifier>,
}

impl GattServer {
    /// Build the attribute database for the given services. Handles are
    /// assigned sequentially from 1, with the standard layout: a Primary
    /// Service declaration, then per characteristic a declaration followed by
    /// its value attribute.
    pub fn new(services: Vec<Service>) -> GattServer {
        let definitions = services
            .into_iter()
            .map(|service| ServiceDefinition {
                uuid: service.uuid,
                primary: true,
                included_services: Vec::new(),
                characteristics: service
                    .characteristics
                    .into_iter()
                    .map(|characteristic| {
                        CharacteristicDefinition {
                            uuid: characteristic.uuid,
                            properties: characteristic.properties,
                            // The original compact API predates permissions and
                            // accepted both operations. Keep that behavior;
                            // explicit definitions opt into enforcement.
                            permissions: permissions::READABLE | permissions::WRITEABLE,
                            value: characteristic.value,
                            descriptors: Vec::new(),
                        }
                    })
                    .collect(),
            })
            .collect();
        Self::from_definitions(definitions).expect("legacy GATT database fits in handle space")
    }

    /// Build a complete GATT database with secondary services, include
    /// declarations, arbitrary descriptors, and explicit access permissions.
    pub fn from_definitions(services: Vec<ServiceDefinition>) -> Result<GattServer, DatabaseError> {
        #[derive(Clone, Copy)]
        struct Layout {
            start: u16,
            end: u16,
        }

        let mut layouts = Vec::with_capacity(services.len());
        let mut next_handle = 1usize;
        for service in &services {
            let mut count = 1usize + service.included_services.len();
            for characteristic in &service.characteristics {
                count = count
                    .checked_add(2 + characteristic.descriptors.len())
                    .ok_or(DatabaseError::TooManyAttributes)?;
                let has_cccd = characteristic.descriptors.iter().any(|descriptor| {
                    descriptor.uuid
                        == Uuid::from_16_bits(GATT_CLIENT_CHARACTERISTIC_CONFIGURATION_UUID)
                });
                if characteristic.properties & (properties::NOTIFY | properties::INDICATE) != 0
                    && !has_cccd
                {
                    count = count
                        .checked_add(1)
                        .ok_or(DatabaseError::TooManyAttributes)?;
                }
            }
            let end = next_handle
                .checked_add(count - 1)
                .ok_or(DatabaseError::TooManyAttributes)?;
            if end > u16::MAX as usize {
                return Err(DatabaseError::TooManyAttributes);
            }
            layouts.push(Layout {
                start: next_handle as u16,
                end: end as u16,
            });
            next_handle = end + 1;
        }

        for (service_index, service) in services.iter().enumerate() {
            for &included in &service.included_services {
                if included >= services.len() {
                    return Err(DatabaseError::InvalidIncludedService {
                        service: service_index,
                        included,
                    });
                }
            }
        }
        let service_uuids: Vec<Uuid> = services
            .iter()
            .map(|service| service.uuid.clone())
            .collect();

        let mut attributes: Vec<Attribute> = Vec::new();
        let mut handle = 1u32;

        for (service_index, service) in services.into_iter().enumerate() {
            let service_layout = layouts[service_index];
            attributes.push(Attribute {
                handle: handle as u16,
                type_uuid: Uuid::from_16_bits(if service.primary {
                    GATT_PRIMARY_SERVICE_UUID
                } else {
                    GATT_SECONDARY_SERVICE_UUID
                }),
                end_group_handle: service_layout.end,
                permissions: permissions::READABLE,
                value: service.uuid.to_bytes(false),
                dynamic_value: None,
            });
            handle += 1;

            for included_index in service.included_services {
                let included_layout = layouts[included_index];
                let mut value = Vec::with_capacity(6);
                value.extend_from_slice(&included_layout.start.to_le_bytes());
                value.extend_from_slice(&included_layout.end.to_le_bytes());
                let included_uuid = service_uuids[included_index].to_bytes(false);
                if included_uuid.len() == 2 {
                    value.extend_from_slice(&included_uuid);
                }
                attributes.push(Attribute {
                    handle: handle as u16,
                    type_uuid: Uuid::from_16_bits(GATT_INCLUDE_UUID),
                    end_group_handle: handle as u16,
                    permissions: permissions::READABLE,
                    value,
                    dynamic_value: None,
                });
                handle += 1;
            }

            for ch in service.characteristics {
                let decl_handle = handle;
                handle += 1;
                let value_handle = handle;
                handle += 1;

                let mut declaration = Vec::with_capacity(3 + 16);
                declaration.push(ch.properties);
                declaration.extend_from_slice(&(value_handle as u16).to_le_bytes());
                declaration.extend_from_slice(&ch.uuid.to_bytes(false));
                attributes.push(Attribute {
                    handle: decl_handle as u16,
                    type_uuid: Uuid::from_16_bits(GATT_CHARACTERISTIC_UUID),
                    end_group_handle: decl_handle as u16,
                    permissions: permissions::READABLE,
                    value: declaration,
                    dynamic_value: None,
                });
                attributes.push(Attribute {
                    handle: value_handle as u16,
                    type_uuid: ch.uuid.clone(),
                    end_group_handle: value_handle as u16,
                    permissions: ch.permissions,
                    value: ch.value,
                    dynamic_value: None,
                });

                let has_cccd = ch.descriptors.iter().any(|descriptor| {
                    descriptor.uuid
                        == Uuid::from_16_bits(GATT_CLIENT_CHARACTERISTIC_CONFIGURATION_UUID)
                });
                for descriptor in ch.descriptors {
                    let descriptor_handle = handle;
                    handle += 1;
                    attributes.push(Attribute {
                        handle: descriptor_handle as u16,
                        type_uuid: descriptor.uuid,
                        end_group_handle: descriptor_handle as u16,
                        permissions: descriptor.permissions,
                        value: descriptor.value,
                        dynamic_value: None,
                    });
                }

                // A notify/indicate characteristic gets a Client Characteristic
                // Configuration descriptor, initialised to "disabled".
                if ch.properties & (properties::NOTIFY | properties::INDICATE) != 0 && !has_cccd {
                    let cccd_handle = handle;
                    handle += 1;
                    attributes.push(Attribute {
                        handle: cccd_handle as u16,
                        type_uuid: Uuid::from_16_bits(
                            GATT_CLIENT_CHARACTERISTIC_CONFIGURATION_UUID,
                        ),
                        end_group_handle: cccd_handle as u16,
                        permissions: permissions::READABLE | permissions::WRITEABLE,
                        value: vec![0x00, 0x00],
                        dynamic_value: None,
                    });
                }
            }
            debug_assert_eq!((handle - 1) as u16, service_layout.end);
        }

        Ok(GattServer {
            attributes,
            mtu: ATT_DEFAULT_MTU,
            negotiated_mtu: ATT_DEFAULT_MTU,
            prepared_writes: Vec::new(),
            signed_write_verifier: None,
        })
    }

    fn find(&self, handle: u16) -> Option<&Attribute> {
        self.attributes.iter().find(|a| a.handle == handle)
    }

    fn find_mut(&mut self, handle: u16) -> Option<&mut Attribute> {
        self.attributes.iter_mut().find(|a| a.handle == handle)
    }

    /// Replace an attribute's static value with synchronous read/write
    /// callbacks. The callbacks remain shared when the server is cloned.
    pub fn set_dynamic_value(
        &mut self,
        handle: u16,
        value: DynamicValue,
    ) -> Result<(), DatabaseError> {
        let attribute = self
            .find_mut(handle)
            .ok_or(DatabaseError::UnknownAttribute(handle))?;
        attribute.dynamic_value = Some(value);
        Ok(())
    }

    /// Restore the retained static value (the most recent value from before
    /// the dynamic binding was installed).
    pub fn clear_dynamic_value(&mut self, handle: u16) -> Result<(), DatabaseError> {
        let attribute = self
            .find_mut(handle)
            .ok_or(DatabaseError::UnknownAttribute(handle))?;
        attribute.dynamic_value = None;
        Ok(())
    }

    pub fn prepared_write_count(&self) -> usize {
        self.prepared_writes.len()
    }

    pub fn set_signed_write_key(&mut self, csrk: [u8; 16], last_counter: Option<u32>) {
        self.signed_write_verifier = Some(SignedWriteVerifier::new(csrk, last_counter));
    }

    pub fn signed_write_counter(&self) -> Option<u32> {
        self.signed_write_verifier
            .as_ref()
            .and_then(SignedWriteVerifier::last_counter)
    }

    /// Turn an incoming ATT request into a response.
    pub fn on_request(&mut self, request: &AttPdu) -> AttPdu {
        self.on_request_with_context(request, AccessContext::default())
    }

    /// Turn an incoming ATT request into a response using the security state
    /// associated with its bearer.
    pub fn on_request_with_context(&mut self, request: &AttPdu, context: AccessContext) -> AttPdu {
        match request {
            AttPdu::ExchangeMtuRequest { client_rx_mtu } => {
                self.negotiated_mtu = (*client_rx_mtu).min(self.mtu).max(ATT_DEFAULT_MTU);
                AttPdu::ExchangeMtuResponse {
                    server_rx_mtu: self.mtu,
                }
            }
            AttPdu::ReadRequest { attribute_handle } => match self.find(*attribute_handle) {
                Some(a) => match check_read_access(a.permissions, context) {
                    Ok(()) => match a.read_value(context) {
                        Ok(value) => AttPdu::ReadResponse {
                            // A Read Response carries at most MTU-1 bytes; longer
                            // values are fetched with Read Blob.
                            attribute_value: truncate(&value, (self.negotiated_mtu - 1) as usize),
                        },
                        Err(code) => error(codes::ATT_READ_REQUEST, *attribute_handle, code),
                    },
                    Err(code) => error(codes::ATT_READ_REQUEST, *attribute_handle, code),
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
                    if let Err(code) = check_read_access(a.permissions, context) {
                        return error(codes::ATT_READ_BLOB_REQUEST, *attribute_handle, code);
                    }
                    let value = match a.read_value(context) {
                        Ok(value) => value,
                        Err(code) => {
                            return error(codes::ATT_READ_BLOB_REQUEST, *attribute_handle, code);
                        }
                    };
                    let offset = *value_offset as usize;
                    if offset > value.len() {
                        error(
                            codes::ATT_READ_BLOB_REQUEST,
                            *attribute_handle,
                            ATT_INVALID_OFFSET_ERROR,
                        )
                    } else {
                        let end = (offset + (self.negotiated_mtu - 1) as usize).min(value.len());
                        AttPdu::ReadBlobResponse {
                            part_attribute_value: value[offset..end].to_vec(),
                        }
                    }
                }
                None => error(
                    codes::ATT_READ_BLOB_REQUEST,
                    *attribute_handle,
                    ATT_ATTRIBUTE_NOT_FOUND_ERROR,
                ),
            },
            AttPdu::ReadMultipleRequest { set_of_handles } => {
                self.read_multiple(set_of_handles, false, context)
            }
            AttPdu::ReadMultipleVariableRequest { set_of_handles } => {
                self.read_multiple(set_of_handles, true, context)
            }
            AttPdu::WriteRequest {
                attribute_handle,
                attribute_value,
            } => match self.find(*attribute_handle) {
                Some(a) => match check_write_access(a.permissions, context) {
                    Ok(()) => match self
                        .find_mut(*attribute_handle)
                        .expect("attribute was just found")
                        .write_value(context, attribute_value)
                    {
                        Ok(()) => AttPdu::WriteResponse,
                        Err(code) => error(codes::ATT_WRITE_REQUEST, *attribute_handle, code),
                    },
                    Err(code) => error(codes::ATT_WRITE_REQUEST, *attribute_handle, code),
                },
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
            } => self.read_by_group_type(
                *starting_handle,
                *ending_handle,
                attribute_group_type,
                context,
            ),
            AttPdu::ReadByTypeRequest {
                starting_handle,
                ending_handle,
                attribute_type,
            } => self.read_by_type(*starting_handle, *ending_handle, attribute_type, context),
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
                context,
            ),
            AttPdu::WriteCommand {
                attribute_handle,
                attribute_value,
            } => {
                // A command has no response; apply it best-effort.
                if let Some(a) = self.find(*attribute_handle) {
                    if check_write_access(a.permissions, context).is_ok() {
                        let _ = self
                            .find_mut(*attribute_handle)
                            .expect("attribute was just found")
                            .write_value(context, attribute_value);
                    }
                }
                // Callers ignore the returned PDU for commands; surface a no-op.
                AttPdu::HandleValueConfirmation
            }
            AttPdu::SignedWriteCommand {
                attribute_handle,
                attribute_value,
                ..
            } => {
                let verified = self
                    .signed_write_verifier
                    .as_mut()
                    .is_some_and(|verifier| verifier.verify(request));
                if verified {
                    let signed_context = AccessContext {
                        authenticated: true,
                        ..context
                    };
                    if let Some(attribute) = self.find(*attribute_handle) {
                        if check_write_access(attribute.permissions, signed_context).is_ok() {
                            let _ = self
                                .find_mut(*attribute_handle)
                                .expect("attribute was just found")
                                .write_value(signed_context, attribute_value);
                        }
                    }
                }
                AttPdu::HandleValueConfirmation
            }
            AttPdu::PrepareWriteRequest {
                attribute_handle,
                value_offset,
                part_attribute_value,
            } => match self.find(*attribute_handle) {
                None => error(
                    codes::ATT_PREPARE_WRITE_REQUEST,
                    *attribute_handle,
                    ATT_ATTRIBUTE_NOT_FOUND_ERROR,
                ),
                Some(attribute) => match check_write_access(attribute.permissions, context) {
                    Err(code) => error(codes::ATT_PREPARE_WRITE_REQUEST, *attribute_handle, code),
                    Ok(()) if attribute.dynamic_value.is_some() => error(
                        codes::ATT_PREPARE_WRITE_REQUEST,
                        *attribute_handle,
                        ATT_ATTRIBUTE_NOT_LONG_ERROR,
                    ),
                    Ok(()) => {
                        self.prepared_writes.push((
                            *attribute_handle,
                            *value_offset,
                            part_attribute_value.clone(),
                        ));
                        AttPdu::PrepareWriteResponse {
                            attribute_handle: *attribute_handle,
                            value_offset: *value_offset,
                            part_attribute_value: part_attribute_value.clone(),
                        }
                    }
                },
            },
            AttPdu::ExecuteWriteRequest { flags } => self.execute_writes(*flags, context),
            other => error(other.op_code(), 0, ATT_REQUEST_NOT_SUPPORTED_ERROR),
        }
    }

    fn read_multiple(&self, handles: &[u16], variable: bool, context: AccessContext) -> AttPdu {
        let mut remaining = usize::from(self.negotiated_mtu - 1);
        if variable {
            let mut tuples = Vec::new();
            for handle in handles {
                let Some(attribute) = self.find(*handle) else {
                    return error(
                        codes::ATT_READ_MULTIPLE_VARIABLE_REQUEST,
                        *handle,
                        ATT_ATTRIBUTE_NOT_FOUND_ERROR,
                    );
                };
                if let Err(code) = check_read_access(attribute.permissions, context) {
                    return error(codes::ATT_READ_MULTIPLE_VARIABLE_REQUEST, *handle, code);
                }
                let value = match attribute.read_value(context) {
                    Ok(value) => value,
                    Err(code) => {
                        return error(codes::ATT_READ_MULTIPLE_VARIABLE_REQUEST, *handle, code);
                    }
                };
                let part = truncate(&value, usize::from(self.negotiated_mtu - 3).min(251));
                if part.len() + 2 > remaining {
                    break;
                }
                remaining -= part.len() + 2;
                tuples.push((value.len().min(u16::MAX as usize) as u16, part));
            }
            AttPdu::ReadMultipleVariableResponse {
                length_value_tuples: tuples,
            }
        } else {
            let mut values = Vec::new();
            for handle in handles {
                let Some(attribute) = self.find(*handle) else {
                    return error(
                        codes::ATT_READ_MULTIPLE_REQUEST,
                        *handle,
                        ATT_ATTRIBUTE_NOT_FOUND_ERROR,
                    );
                };
                if let Err(code) = check_read_access(attribute.permissions, context) {
                    return error(codes::ATT_READ_MULTIPLE_REQUEST, *handle, code);
                }
                let value = match attribute.read_value(context) {
                    Ok(value) => value,
                    Err(code) => return error(codes::ATT_READ_MULTIPLE_REQUEST, *handle, code),
                };
                let part = truncate(&value, usize::from(self.negotiated_mtu - 1).min(251));
                if part.len() > remaining {
                    break;
                }
                remaining -= part.len();
                values.extend_from_slice(&part);
            }
            AttPdu::ReadMultipleResponse {
                set_of_values: values,
            }
        }
    }

    fn execute_writes(&mut self, flags: u8, context: AccessContext) -> AttPdu {
        if flags == 0 {
            self.prepared_writes.clear();
            return AttPdu::ExecuteWriteResponse;
        }
        if flags != 1 {
            return error(codes::ATT_EXECUTE_WRITE_REQUEST, 0, ATT_INVALID_PDU_ERROR);
        }
        let prepared_writes = core::mem::take(&mut self.prepared_writes);
        let mut staged: BTreeMap<u16, Vec<u8>> = self
            .attributes
            .iter()
            .map(|attribute| (attribute.handle, attribute.value.clone()))
            .collect();
        for (handle, offset, part) in &prepared_writes {
            let Some(attribute) = self.find(*handle) else {
                return error(
                    codes::ATT_EXECUTE_WRITE_REQUEST,
                    *handle,
                    ATT_ATTRIBUTE_NOT_FOUND_ERROR,
                );
            };
            if let Err(code) = check_write_access(attribute.permissions, context) {
                return error(codes::ATT_EXECUTE_WRITE_REQUEST, *handle, code);
            }
            if attribute.dynamic_value.is_some() {
                return error(
                    codes::ATT_EXECUTE_WRITE_REQUEST,
                    *handle,
                    ATT_ATTRIBUTE_NOT_LONG_ERROR,
                );
            }
            let Some(value) = staged.get_mut(handle) else {
                return error(
                    codes::ATT_EXECUTE_WRITE_REQUEST,
                    *handle,
                    ATT_ATTRIBUTE_NOT_FOUND_ERROR,
                );
            };
            let offset = usize::from(*offset);
            if offset > value.len() {
                return error(
                    codes::ATT_EXECUTE_WRITE_REQUEST,
                    *handle,
                    ATT_INVALID_OFFSET_ERROR,
                );
            }
            let end = offset + part.len();
            if end > value.len() {
                value.resize(end, 0);
            }
            value[offset..end].copy_from_slice(part);
        }
        for attribute in &mut self.attributes {
            if let Some(value) = staged.remove(&attribute.handle) {
                attribute.value = value;
            }
        }
        AttPdu::ExecuteWriteResponse
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
        context: AccessContext,
    ) -> AttPdu {
        let mut list = Vec::new();
        for a in self
            .attributes
            .iter()
            .filter(|a| a.handle >= start && a.handle <= end && a.type_uuid == *attribute_type)
        {
            if let Err(code) = check_read_access(a.permissions, context) {
                return error(codes::ATT_FIND_BY_TYPE_VALUE_REQUEST, a.handle, code);
            }
            match a.read_value(context) {
                Ok(value) if value == attribute_value => {
                    list.extend_from_slice(&a.handle.to_le_bytes());
                    list.extend_from_slice(&a.end_group_handle.to_le_bytes());
                }
                Ok(_) => {}
                Err(code) => {
                    return error(codes::ATT_FIND_BY_TYPE_VALUE_REQUEST, a.handle, code);
                }
            }
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

    fn read_by_group_type(
        &self,
        start: u16,
        end: u16,
        group_type: &Uuid,
        context: AccessContext,
    ) -> AttPdu {
        // Grouping attributes (services) in range with the matching type.
        let matches: Vec<&Attribute> = self
            .attributes
            .iter()
            .filter(|a| a.handle >= start && a.handle <= end && a.type_uuid == *group_type)
            .collect();
        if matches.is_empty() {
            return error(
                codes::ATT_READ_BY_GROUP_TYPE_REQUEST,
                start,
                ATT_ATTRIBUTE_NOT_FOUND_ERROR,
            );
        }

        // A response groups only entries whose value has the same length.
        let mut selected = Vec::new();
        for attribute in matches {
            if let Err(code) = check_read_access(attribute.permissions, context) {
                return error(
                    codes::ATT_READ_BY_GROUP_TYPE_REQUEST,
                    attribute.handle,
                    code,
                );
            }
            let value = match attribute.read_value(context) {
                Ok(value) => value,
                Err(code) => {
                    return error(
                        codes::ATT_READ_BY_GROUP_TYPE_REQUEST,
                        attribute.handle,
                        code,
                    );
                }
            };
            if selected
                .first()
                .is_some_and(|(_, _, first_value): &(u16, u16, Vec<u8>)| {
                    first_value.len() != value.len()
                })
            {
                break;
            }
            selected.push((attribute.handle, attribute.end_group_handle, value));
        }
        let value_len = selected[0].2.len();
        let mut adl = Vec::new();
        for (handle, end_group_handle, value) in selected {
            adl.extend_from_slice(&handle.to_le_bytes());
            adl.extend_from_slice(&end_group_handle.to_le_bytes());
            adl.extend_from_slice(&value);
        }
        AttPdu::ReadByGroupTypeResponse {
            length: (4 + value_len) as u8,
            attribute_data_list: adl,
        }
    }

    fn read_by_type(
        &self,
        start: u16,
        end: u16,
        attribute_type: &Uuid,
        context: AccessContext,
    ) -> AttPdu {
        let matches: Vec<&Attribute> = self
            .attributes
            .iter()
            .filter(|a| a.handle >= start && a.handle <= end && a.type_uuid == *attribute_type)
            .collect();
        if matches.is_empty() {
            return error(
                codes::ATT_READ_BY_TYPE_REQUEST,
                start,
                ATT_ATTRIBUTE_NOT_FOUND_ERROR,
            );
        }

        let mut selected = Vec::new();
        for attribute in matches {
            if let Err(code) = check_read_access(attribute.permissions, context) {
                return error(codes::ATT_READ_BY_TYPE_REQUEST, attribute.handle, code);
            }
            let value = match attribute.read_value(context) {
                Ok(value) => value,
                Err(code) => return error(codes::ATT_READ_BY_TYPE_REQUEST, attribute.handle, code),
            };
            if selected
                .first()
                .is_some_and(|(_, first_value): &(u16, Vec<u8>)| first_value.len() != value.len())
            {
                break;
            }
            selected.push((attribute.handle, value));
        }
        let value_len = selected[0].1.len();
        let mut adl = Vec::new();
        for (handle, value) in selected {
            adl.extend_from_slice(&handle.to_le_bytes());
            adl.extend_from_slice(&value);
        }
        AttPdu::ReadByTypeResponse {
            length: (2 + value_len) as u8,
            attribute_data_list: adl,
        }
    }
}

fn check_read_access(attribute_permissions: u8, context: AccessContext) -> Result<(), u8> {
    let read_bits = permissions::READABLE
        | permissions::READ_REQUIRES_ENCRYPTION
        | permissions::READ_REQUIRES_AUTHENTICATION
        | permissions::READ_REQUIRES_AUTHORIZATION;
    if attribute_permissions & read_bits == 0 {
        return Err(ATT_READ_NOT_PERMITTED_ERROR);
    }
    if attribute_permissions & permissions::READ_REQUIRES_ENCRYPTION != 0 && !context.encrypted {
        return Err(ATT_INSUFFICIENT_ENCRYPTION_ERROR);
    }
    if attribute_permissions & permissions::READ_REQUIRES_AUTHENTICATION != 0
        && !context.authenticated
    {
        return Err(ATT_INSUFFICIENT_AUTHENTICATION_ERROR);
    }
    if attribute_permissions & permissions::READ_REQUIRES_AUTHORIZATION != 0 && !context.authorized
    {
        return Err(ATT_INSUFFICIENT_AUTHORIZATION_ERROR);
    }
    Ok(())
}

fn check_write_access(attribute_permissions: u8, context: AccessContext) -> Result<(), u8> {
    let write_bits = permissions::WRITEABLE
        | permissions::WRITE_REQUIRES_ENCRYPTION
        | permissions::WRITE_REQUIRES_AUTHENTICATION
        | permissions::WRITE_REQUIRES_AUTHORIZATION;
    if attribute_permissions & write_bits == 0 {
        return Err(ATT_WRITE_NOT_PERMITTED_ERROR);
    }
    if attribute_permissions & permissions::WRITE_REQUIRES_ENCRYPTION != 0 && !context.encrypted {
        return Err(ATT_INSUFFICIENT_ENCRYPTION_ERROR);
    }
    if attribute_permissions & permissions::WRITE_REQUIRES_AUTHENTICATION != 0
        && !context.authenticated
    {
        return Err(ATT_INSUFFICIENT_AUTHENTICATION_ERROR);
    }
    if attribute_permissions & permissions::WRITE_REQUIRES_AUTHORIZATION != 0 && !context.authorized
    {
        return Err(ATT_INSUFFICIENT_AUTHORIZATION_ERROR);
    }
    Ok(())
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
