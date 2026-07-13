//! The GATT client (slice 18) — a synchronous port of the discovery, read,
//! write, and subscription logic from upstream `gatt_client.py`.
//!
//! The client is transport-agnostic: it emits ATT requests through an
//! [`AttTransport`] and interprets the responses. A blanket impl makes any
//! [`crate::AttRequestHandler`] (a bare [`crate::AttServer`] or a full
//! [`crate::GattServer`]) usable as a transport, so a client and server can be
//! wired directly for testing. In a real stack the transport carries the PDUs
//! over L2CAP/ACL instead.

use std::collections::BTreeMap;

use bumble::Uuid;
use bumble_att::{AttPdu, SignedWriteSigner};

use crate::{AttRequestHandler, GATT_CHARACTERISTIC_UUID, GATT_PRIMARY_SERVICE_UUID};

/// ATT error: attribute not found (ends a discovery iteration).
const ATT_ATTRIBUTE_NOT_FOUND_ERROR: u8 = 0x0A;
/// ATT error: attribute value cannot be read with Read Blob.
const ATT_ATTRIBUTE_NOT_LONG_ERROR: u8 = 0x0B;
/// ATT error: the Read Blob offset is past the end of the value.
const ATT_INVALID_OFFSET_ERROR: u8 = 0x07;

/// CCCD value enabling notifications (Vol 3, Part G - 3.3.3.3).
pub const CCCD_NOTIFICATION: u16 = 0x0001;
/// CCCD value enabling indications.
pub const CCCD_INDICATION: u16 = 0x0002;

/// A transport that carries an ATT request to the peer and returns its
/// response PDU.
pub trait AttTransport {
    fn request(&mut self, request: &AttPdu) -> AttPdu;
}

/// Any request-handling server is usable as a transport: the request is
/// answered in-process. This is what lets the client talk to a
/// [`crate::GattServer`] directly.
impl<H: AttRequestHandler> AttTransport for H {
    fn request(&mut self, request: &AttPdu) -> AttPdu {
        self.handle_request(request)
    }
}

/// An error surfaced by the client.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GattError {
    /// The peer returned an ATT Error Response.
    Att {
        request_opcode: u8,
        attribute_handle: u16,
        error_code: u8,
    },
    /// The peer's response did not match the request or was malformed.
    Protocol(String),
}

impl std::fmt::Display for GattError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GattError::Att {
                request_opcode,
                attribute_handle,
                error_code,
            } => write!(
                f,
                "ATT error {error_code:#04x} for opcode {request_opcode:#04x} at handle {attribute_handle:#06x}"
            ),
            GattError::Protocol(m) => write!(f, "protocol error: {m}"),
        }
    }
}

impl std::error::Error for GattError {}

type Result<T> = core::result::Result<T, GattError>;

/// A discovered primary service.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServiceProxy {
    pub handle: u16,
    pub end_group_handle: u16,
    pub uuid: Uuid,
}

/// A discovered characteristic. `handle` is the value handle (where reads and
/// writes are addressed); `declaration_handle` is the declaration attribute.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CharacteristicProxy {
    pub declaration_handle: u16,
    pub handle: u16,
    pub end_group_handle: u16,
    pub properties: u8,
    pub uuid: Uuid,
}

/// A discovered characteristic descriptor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DescriptorProxy {
    pub handle: u16,
    pub uuid: Uuid,
}

/// A synchronous GATT client.
#[derive(Debug, Default)]
pub struct GattClient {
    mtu: u16,
    cached_values: BTreeMap<u16, Vec<u8>>,
    notification_subscribers: BTreeMap<u16, ()>,
    indication_subscribers: BTreeMap<u16, ()>,
}

/// Turn an ATT Error Response into a [`GattError`]; any other PDU passes
/// through unchanged.
fn as_error(pdu: &AttPdu) -> Option<GattError> {
    match pdu {
        AttPdu::ErrorResponse {
            request_opcode_in_error,
            attribute_handle_in_error,
            error_code,
        } => Some(GattError::Att {
            request_opcode: *request_opcode_in_error,
            attribute_handle: *attribute_handle_in_error,
            error_code: *error_code,
        }),
        _ => None,
    }
}

impl GattClient {
    pub fn new() -> GattClient {
        GattClient {
            mtu: crate::ATT_DEFAULT_MTU,
            cached_values: BTreeMap::new(),
            notification_subscribers: BTreeMap::new(),
            indication_subscribers: BTreeMap::new(),
        }
    }

    /// The negotiated ATT MTU.
    pub fn mtu(&self) -> u16 {
        self.mtu
    }

    /// Exchange MTUs, adopting the smaller of ours and the server's.
    pub fn exchange_mtu(&mut self, t: &mut impl AttTransport, client_rx_mtu: u16) -> Result<u16> {
        let response = t.request(&AttPdu::ExchangeMtuRequest { client_rx_mtu });
        if let Some(e) = as_error(&response) {
            return Err(e);
        }
        match response {
            AttPdu::ExchangeMtuResponse { server_rx_mtu } => {
                self.mtu = client_rx_mtu.min(server_rx_mtu).max(crate::ATT_DEFAULT_MTU);
                Ok(self.mtu)
            }
            other => Err(GattError::Protocol(format!(
                "expected Exchange MTU Response, got {other:?}"
            ))),
        }
    }

    /// Discover all primary services (Vol 3, Part G - 4.4.1).
    pub fn discover_services(&mut self, t: &mut impl AttTransport) -> Result<Vec<ServiceProxy>> {
        let mut services = Vec::new();
        let mut starting_handle: u16 = 0x0001;
        loop {
            let response = t.request(&AttPdu::ReadByGroupTypeRequest {
                starting_handle,
                ending_handle: 0xFFFF,
                attribute_group_type: Uuid::from_16_bits(GATT_PRIMARY_SERVICE_UUID),
            });
            match response {
                AttPdu::ErrorResponse { error_code, .. }
                    if error_code == ATT_ATTRIBUTE_NOT_FOUND_ERROR =>
                {
                    break
                }
                AttPdu::ErrorResponse { .. } => return Err(as_error(&response).unwrap()),
                AttPdu::ReadByGroupTypeResponse {
                    length,
                    attribute_data_list,
                } => {
                    let entries = parse_group_entries(length, &attribute_data_list)?;
                    if entries.is_empty() {
                        break;
                    }
                    let mut last_end = 0u16;
                    for (handle, end_group_handle, value) in entries {
                        if handle < starting_handle || end_group_handle < handle {
                            return Err(GattError::Protocol(format!(
                                "bogus service handles {handle:#06x}/{end_group_handle:#06x}"
                            )));
                        }
                        let uuid = Uuid::from_bytes(&value)
                            .map_err(|e| GattError::Protocol(format!("bad service UUID: {e}")))?;
                        services.push(ServiceProxy {
                            handle,
                            end_group_handle,
                            uuid,
                        });
                        last_end = end_group_handle;
                    }
                    if last_end == 0xFFFF {
                        break;
                    }
                    starting_handle = last_end + 1;
                }
                other => {
                    return Err(GattError::Protocol(format!(
                        "unexpected response to Read By Group Type: {other:?}"
                    )))
                }
            }
        }
        Ok(services)
    }

    /// Discover a primary service by UUID (Vol 3, Part G - 4.4.2), using Find
    /// By Type Value.
    pub fn discover_service_by_uuid(
        &mut self,
        t: &mut impl AttTransport,
        uuid: &Uuid,
    ) -> Result<Vec<ServiceProxy>> {
        let mut services = Vec::new();
        let mut starting_handle: u16 = 0x0001;
        loop {
            let response = t.request(&AttPdu::FindByTypeValueRequest {
                starting_handle,
                ending_handle: 0xFFFF,
                attribute_type: Uuid::from_16_bits(GATT_PRIMARY_SERVICE_UUID),
                attribute_value: uuid.to_bytes(false),
            });
            match response {
                AttPdu::ErrorResponse { error_code, .. }
                    if error_code == ATT_ATTRIBUTE_NOT_FOUND_ERROR =>
                {
                    break
                }
                AttPdu::ErrorResponse { .. } => return Err(as_error(&response).unwrap()),
                AttPdu::FindByTypeValueResponse {
                    handles_information_list,
                } => {
                    if !handles_information_list.len().is_multiple_of(4)
                        || handles_information_list.is_empty()
                    {
                        break;
                    }
                    let mut last_end = 0u16;
                    for chunk in handles_information_list.chunks_exact(4) {
                        let handle = u16::from_le_bytes([chunk[0], chunk[1]]);
                        let end_group_handle = u16::from_le_bytes([chunk[2], chunk[3]]);
                        services.push(ServiceProxy {
                            handle,
                            end_group_handle,
                            uuid: uuid.clone(),
                        });
                        last_end = end_group_handle;
                    }
                    if last_end == 0xFFFF {
                        break;
                    }
                    starting_handle = last_end + 1;
                }
                other => {
                    return Err(GattError::Protocol(format!(
                        "unexpected response to Find By Type Value: {other:?}"
                    )))
                }
            }
        }
        Ok(services)
    }

    /// Discover all characteristics of a service (Vol 3, Part G - 4.6.1).
    pub fn discover_characteristics(
        &mut self,
        t: &mut impl AttTransport,
        service: &ServiceProxy,
    ) -> Result<Vec<CharacteristicProxy>> {
        let mut characteristics: Vec<CharacteristicProxy> = Vec::new();
        let mut starting_handle = service.handle;
        let ending_handle = service.end_group_handle;
        while starting_handle <= ending_handle {
            let response = t.request(&AttPdu::ReadByTypeRequest {
                starting_handle,
                ending_handle,
                attribute_type: Uuid::from_16_bits(GATT_CHARACTERISTIC_UUID),
            });
            match response {
                AttPdu::ErrorResponse { error_code, .. }
                    if error_code == ATT_ATTRIBUTE_NOT_FOUND_ERROR =>
                {
                    break
                }
                AttPdu::ErrorResponse { .. } => return Err(as_error(&response).unwrap()),
                AttPdu::ReadByTypeResponse {
                    length,
                    attribute_data_list,
                } => {
                    let entries = parse_type_entries(length, &attribute_data_list)?;
                    if entries.is_empty() {
                        break;
                    }
                    let mut last_decl = 0u16;
                    for (declaration_handle, value) in entries {
                        if declaration_handle < starting_handle {
                            return Err(GattError::Protocol(format!(
                                "bogus characteristic handle {declaration_handle:#06x}"
                            )));
                        }
                        // Declaration value: [properties(1), value_handle(2), uuid(2|16)].
                        if value.len() < 3 {
                            return Err(GattError::Protocol(
                                "truncated characteristic declaration".into(),
                            ));
                        }
                        let properties = value[0];
                        let handle = u16::from_le_bytes([value[1], value[2]]);
                        let uuid = Uuid::from_bytes(&value[3..]).map_err(|e| {
                            GattError::Protocol(format!("bad characteristic UUID: {e}"))
                        })?;
                        // The previous characteristic ends just before this one.
                        if let Some(prev) = characteristics.last_mut() {
                            prev.end_group_handle = declaration_handle - 1;
                        }
                        characteristics.push(CharacteristicProxy {
                            declaration_handle,
                            handle,
                            end_group_handle: ending_handle,
                            properties,
                            uuid,
                        });
                        last_decl = declaration_handle;
                    }
                    if last_decl >= ending_handle {
                        break;
                    }
                    starting_handle = last_decl + 1;
                }
                other => {
                    return Err(GattError::Protocol(format!(
                        "unexpected response to Read By Type: {other:?}"
                    )))
                }
            }
        }
        // The final characteristic extends to the service's end.
        if let Some(last) = characteristics.last_mut() {
            last.end_group_handle = service.end_group_handle;
        }
        Ok(characteristics)
    }

    /// Discover a characteristic's descriptors (Vol 3, Part G - 4.7.1), using
    /// Find Information over the handles after the value attribute.
    pub fn discover_descriptors(
        &mut self,
        t: &mut impl AttTransport,
        characteristic: &CharacteristicProxy,
    ) -> Result<Vec<DescriptorProxy>> {
        let mut descriptors = Vec::new();
        if characteristic.handle >= characteristic.end_group_handle {
            return Ok(descriptors);
        }
        let mut starting_handle = characteristic.handle + 1;
        let ending_handle = characteristic.end_group_handle;
        while starting_handle <= ending_handle {
            let response = t.request(&AttPdu::FindInformationRequest {
                starting_handle,
                ending_handle,
            });
            match response {
                AttPdu::ErrorResponse { error_code, .. }
                    if error_code == ATT_ATTRIBUTE_NOT_FOUND_ERROR =>
                {
                    break
                }
                AttPdu::ErrorResponse { .. } => return Err(as_error(&response).unwrap()),
                AttPdu::FindInformationResponse {
                    format,
                    information_data,
                } => {
                    let entries = parse_information_entries(format, &information_data)?;
                    if entries.is_empty() {
                        break;
                    }
                    let mut last = 0u16;
                    for (handle, uuid) in entries {
                        descriptors.push(DescriptorProxy { handle, uuid });
                        last = handle;
                    }
                    if last >= ending_handle {
                        break;
                    }
                    starting_handle = last + 1;
                }
                other => {
                    return Err(GattError::Protocol(format!(
                        "unexpected response to Find Information: {other:?}"
                    )))
                }
            }
        }
        Ok(descriptors)
    }

    /// Read a characteristic value (Vol 3, Part G - 4.8.1). When the value
    /// fills the MTU, continues with Read Blob until a short (or no-more) read,
    /// unless `no_long_read` is set.
    pub fn read_value(
        &mut self,
        t: &mut impl AttTransport,
        attribute_handle: u16,
        no_long_read: bool,
    ) -> Result<Vec<u8>> {
        let response = t.request(&AttPdu::ReadRequest { attribute_handle });
        if let Some(e) = as_error(&response) {
            return Err(e);
        }
        let AttPdu::ReadResponse { attribute_value } = response else {
            return Err(GattError::Protocol(format!(
                "expected Read Response, got {response:?}"
            )));
        };
        let mut value = attribute_value;

        let chunk = (self.mtu - 1) as usize;
        if !no_long_read && value.len() == chunk {
            let mut offset = value.len() as u16;
            loop {
                let response = t.request(&AttPdu::ReadBlobRequest {
                    attribute_handle,
                    value_offset: offset,
                });
                match response {
                    AttPdu::ErrorResponse { error_code, .. }
                        if error_code == ATT_ATTRIBUTE_NOT_LONG_ERROR
                            || error_code == ATT_INVALID_OFFSET_ERROR =>
                    {
                        break
                    }
                    AttPdu::ErrorResponse { .. } => return Err(as_error(&response).unwrap()),
                    AttPdu::ReadBlobResponse {
                        part_attribute_value,
                    } => {
                        let part_len = part_attribute_value.len();
                        value.extend_from_slice(&part_attribute_value);
                        if part_len < chunk {
                            break;
                        }
                        offset += part_len as u16;
                    }
                    other => {
                        return Err(GattError::Protocol(format!(
                            "expected Read Blob Response, got {other:?}"
                        )))
                    }
                }
            }
        }

        self.cached_values.insert(attribute_handle, value.clone());
        Ok(value)
    }

    /// Write a characteristic value (Vol 3, Part G - 4.9.3/4.9.1). With
    /// `with_response`, uses Write Request and awaits Write Response; otherwise
    /// sends a Write Command (no response).
    pub fn write_value(
        &mut self,
        t: &mut impl AttTransport,
        attribute_handle: u16,
        value: Vec<u8>,
        with_response: bool,
    ) -> Result<()> {
        if with_response {
            let response = t.request(&AttPdu::WriteRequest {
                attribute_handle,
                attribute_value: value,
            });
            if let Some(e) = as_error(&response) {
                return Err(e);
            }
            match response {
                AttPdu::WriteResponse => Ok(()),
                other => Err(GattError::Protocol(format!(
                    "expected Write Response, got {other:?}"
                ))),
            }
        } else {
            // A command has no response; the transport discards the returned PDU.
            let _ = t.request(&AttPdu::WriteCommand {
                attribute_handle,
                attribute_value: value,
            });
            Ok(())
        }
    }

    /// Send an authenticated Signed Write Command using the local CSRK state.
    pub fn write_signed_value(
        &mut self,
        t: &mut impl AttTransport,
        signer: &mut SignedWriteSigner,
        attribute_handle: u16,
        value: Vec<u8>,
    ) -> Result<()> {
        let command = signer
            .sign(attribute_handle, value)
            .ok_or_else(|| GattError::Protocol("signed-write counter exhausted".into()))?;
        let _ = t.request(&command);
        Ok(())
    }

    /// Subscribe to a characteristic by writing its CCCD (Vol 3, Part G - 4.10)
    /// and recording the subscription so later notifications/indications for
    /// `value_handle` are accepted.
    pub fn subscribe(
        &mut self,
        t: &mut impl AttTransport,
        value_handle: u16,
        cccd_handle: u16,
        indicate: bool,
    ) -> Result<()> {
        let bits = if indicate {
            CCCD_INDICATION
        } else {
            CCCD_NOTIFICATION
        };
        self.write_value(t, cccd_handle, bits.to_le_bytes().to_vec(), true)?;
        if indicate {
            self.indication_subscribers.insert(value_handle, ());
        } else {
            self.notification_subscribers.insert(value_handle, ());
        }
        Ok(())
    }

    /// Unsubscribe by clearing the CCCD.
    pub fn unsubscribe(
        &mut self,
        t: &mut impl AttTransport,
        value_handle: u16,
        cccd_handle: u16,
    ) -> Result<()> {
        self.write_value(t, cccd_handle, 0u16.to_le_bytes().to_vec(), true)?;
        self.notification_subscribers.remove(&value_handle);
        self.indication_subscribers.remove(&value_handle);
        Ok(())
    }

    /// Handle an incoming Handle Value Notification: cache the value. Returns
    /// `true` if there was a matching subscription.
    pub fn on_notification(&mut self, pdu: &AttPdu) -> Result<bool> {
        match pdu {
            AttPdu::HandleValueNotification {
                attribute_handle,
                attribute_value,
            } => {
                let subscribed = self.notification_subscribers.contains_key(attribute_handle);
                self.cached_values
                    .insert(*attribute_handle, attribute_value.clone());
                Ok(subscribed)
            }
            other => Err(GattError::Protocol(format!(
                "expected Handle Value Notification, got {other:?}"
            ))),
        }
    }

    /// Handle an incoming Handle Value Indication: cache the value and return
    /// the Handle Value Confirmation the client must send back.
    pub fn on_indication(&mut self, pdu: &AttPdu) -> Result<AttPdu> {
        match pdu {
            AttPdu::HandleValueIndication {
                attribute_handle,
                attribute_value,
            } => {
                self.cached_values
                    .insert(*attribute_handle, attribute_value.clone());
                Ok(AttPdu::HandleValueConfirmation)
            }
            other => Err(GattError::Protocol(format!(
                "expected Handle Value Indication, got {other:?}"
            ))),
        }
    }

    /// The last cached value for a handle (from a read, notification, or
    /// indication).
    pub fn cached_value(&self, handle: u16) -> Option<&[u8]> {
        self.cached_values.get(&handle).map(Vec::as_slice)
    }
}

/// Parse a Read By Group Type Response data list: `length`-byte entries, each
/// `[handle(2), end_group_handle(2), value(length-4)]`.
fn parse_group_entries(length: u8, data: &[u8]) -> Result<Vec<(u16, u16, Vec<u8>)>> {
    let length = length as usize;
    if length < 4 {
        return Err(GattError::Protocol(format!(
            "Read By Group Type entry length {length} < 4"
        )));
    }
    if !data.len().is_multiple_of(length) {
        return Err(GattError::Protocol(
            "Read By Group Type data list not a multiple of entry length".into(),
        ));
    }
    let mut out = Vec::new();
    for chunk in data.chunks_exact(length) {
        let handle = u16::from_le_bytes([chunk[0], chunk[1]]);
        let end_group = u16::from_le_bytes([chunk[2], chunk[3]]);
        out.push((handle, end_group, chunk[4..].to_vec()));
    }
    Ok(out)
}

/// Parse a Read By Type Response data list: `length`-byte entries, each
/// `[handle(2), value(length-2)]`.
fn parse_type_entries(length: u8, data: &[u8]) -> Result<Vec<(u16, Vec<u8>)>> {
    let length = length as usize;
    if length < 2 {
        return Err(GattError::Protocol(format!(
            "Read By Type entry length {length} < 2"
        )));
    }
    if !data.len().is_multiple_of(length) {
        return Err(GattError::Protocol(
            "Read By Type data list not a multiple of entry length".into(),
        ));
    }
    let mut out = Vec::new();
    for chunk in data.chunks_exact(length) {
        let handle = u16::from_le_bytes([chunk[0], chunk[1]]);
        out.push((handle, chunk[2..].to_vec()));
    }
    Ok(out)
}

/// Parse a Find Information Response: `format` 1 → 16-bit UUIDs, 2 → 128-bit;
/// entries are `[handle(2), uuid(2|16)]`.
fn parse_information_entries(format: u8, data: &[u8]) -> Result<Vec<(u16, Uuid)>> {
    let uuid_size = match format {
        1 => 2usize,
        2 => 16usize,
        other => {
            return Err(GattError::Protocol(format!(
                "invalid Find Information format {other}"
            )))
        }
    };
    let entry = 2 + uuid_size;
    if !data.len().is_multiple_of(entry) {
        return Err(GattError::Protocol(
            "Find Information data not a multiple of entry size".into(),
        ));
    }
    let mut out = Vec::new();
    for chunk in data.chunks_exact(entry) {
        let handle = u16::from_le_bytes([chunk[0], chunk[1]]);
        let uuid = Uuid::from_bytes(&chunk[2..])
            .map_err(|e| GattError::Protocol(format!("bad descriptor UUID: {e}")))?;
        out.push((handle, uuid));
    }
    Ok(out)
}
