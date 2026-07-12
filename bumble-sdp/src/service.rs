//! The SDP service runtime — a synchronous port of upstream's asyncio
//! `Client` and `Server`.
//!
//! **Slice 20.** The crate root is the codec (`DataElement`, `ServiceAttribute`,
//! `SdpPdu`); this module is the request/response machinery on top: a
//! [`SdpServer`] that answers queries against a service-record database, and a
//! [`SdpClient`] that issues them and reassembles multi-packet answers across
//! SDP's continuation-state protocol.
//!
//! # Transport-agnostic by design
//!
//! Upstream drives SDP over an `asyncio` L2CAP `ClassicChannel`. This port has
//! no live Classic L2CAP connection-oriented channel to route over, so the
//! runtime is transport-agnostic, mirroring the [`bumble_gatt`] client's
//! `AttTransport`: the server answers one request at a time via
//! [`SdpRequestHandler::handle_request`], and the client drives requests through
//! an [`SdpTransport`]. A blanket impl makes every handler a transport, so an
//! [`SdpClient`] can wrap an [`SdpServer`] directly and the pair talks to itself
//! in-process. The two-party integration test does exactly that.
//!
//! [`bumble_gatt`]: https://docs.rs/bumble-gatt
//!
//! # Continuation state
//!
//! An SDP answer that does not fit in one response PDU is split into chunks.
//! The server serializes the whole answer once, hands out `maximum_size`-byte
//! slices, and signals "more to come" with a fixed 2-byte continuation state
//! (`01 00`); the final slice carries the 1-byte terminator (`00`). The client
//! loops, echoing the server's continuation state back verbatim and
//! accumulating the payload, until it sees the terminator. Both directions are
//! ported here and exercised across multiple round-trips in the tests.

use bumble::Uuid;

use crate::{error_code, DataElement, SdpPdu, ServiceAttribute};

/// The non-terminal continuation state the server emits when more response data
/// remains: a 1-byte length (`0x01`) and a single opaque state byte (`0x00`).
/// Matches upstream's `Server.CONTINUATION_STATE`.
const SERVER_CONTINUATION_STATE: [u8; 2] = [0x01, 0x00];

/// The client's continuation watchdog: the maximum number of continuation
/// round-trips before it gives up (upstream's `SDP_CONTINUATION_WATCHDOG`).
const CONTINUATION_WATCHDOG: usize = 64;

/// An attribute selector in a request: a single attribute id, or an inclusive
/// range. Ranges are encoded on the wire as a 32-bit `(start << 16) | end`
/// integer and single ids as a 16-bit integer, which is how the server tells
/// them apart (by value size).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AttributeId {
    /// A single attribute id.
    Id(u16),
    /// An inclusive `[start, end]` range.
    Range(u16, u16),
}

impl AttributeId {
    fn to_data_element(self) -> DataElement {
        match self {
            AttributeId::Id(id) => DataElement::unsigned_integer_16(id),
            AttributeId::Range(start, end) => {
                DataElement::unsigned_integer_32(((start as u32) << 16) | end as u32)
            }
        }
    }
}

/// Build the `service_search_pattern` sequence from a list of UUIDs.
fn search_pattern(uuids: &[Uuid]) -> DataElement {
    DataElement::sequence(uuids.iter().map(|u| DataElement::uuid(u.clone())))
}

/// Build the `attribute_id_list` sequence from a list of selectors.
fn attribute_id_list(ids: &[AttributeId]) -> DataElement {
    DataElement::sequence(ids.iter().map(|id| id.to_data_element()))
}

/// Does `value` contain `uuid`, directly or by recursing into sequences?
/// Mirrors upstream `ServiceAttribute.is_uuid_in_value` (which recurses into
/// sequences but not alternatives).
fn is_uuid_in_value(uuid: &Uuid, value: &DataElement) -> bool {
    match value {
        DataElement::Uuid(u) => u == uuid,
        DataElement::Sequence(elements) => elements.iter().any(|e| is_uuid_in_value(uuid, e)),
        _ => false,
    }
}

// -----------------------------------------------------------------------------
// Transport plumbing (mirrors bumble-gatt's AttTransport)
// -----------------------------------------------------------------------------

/// Something that answers a single SDP request with a single response PDU.
/// Implemented by [`SdpServer`].
pub trait SdpRequestHandler {
    /// Answer one request.
    fn handle_request(&mut self, request: &SdpPdu) -> SdpPdu;
}

/// The transport an [`SdpClient`] issues requests through. Every
/// [`SdpRequestHandler`] is one (blanket impl below), so a client can wrap a
/// server directly.
pub trait SdpTransport {
    /// Send a request and return the response.
    fn request(&mut self, request: &SdpPdu) -> SdpPdu;
}

impl<H: SdpRequestHandler> SdpTransport for H {
    fn request(&mut self, request: &SdpPdu) -> SdpPdu {
        self.handle_request(request)
    }
}

// -----------------------------------------------------------------------------
// Server
// -----------------------------------------------------------------------------

/// One service record: a handle and the attributes registered under it.
#[derive(Clone, Debug)]
struct ServiceRecord {
    handle: u32,
    attributes: Vec<ServiceAttribute>,
}

/// The in-flight response a continuation is draining. Attribute responses carry
/// serialized bytes; a service search carries the (total, remaining) handles,
/// matching upstream's `current_response: None | bytes | tuple[int, list[int]]`.
#[derive(Clone, Debug)]
enum CurrentResponse {
    Bytes(Vec<u8>),
    Handles { total: u16, remaining: Vec<u32> },
}

/// The outcome of the continuation check at the top of every handler.
enum Continuation {
    /// Bail with this error code.
    Error(u16),
    /// Valid continuation: keep draining the in-flight response.
    Continue,
    /// A fresh request: (re)compute the response.
    Fresh,
}

/// An SDP server: answers queries against a service-record database, chunking
/// long answers across continuation state. Records are held in insertion order
/// (as upstream's `dict` is), which fixes the order of matched services in a
/// response.
#[derive(Debug, Default)]
pub struct SdpServer {
    records: Vec<ServiceRecord>,
    /// The response currently being drained by a continuation, if any.
    current_response: Option<CurrentResponse>,
    /// The peer L2CAP MTU, which bounds each response PDU (upstream reads it
    /// from the channel). Answers larger than this are split.
    mtu: u16,
}

impl SdpServer {
    /// Create a server with the given peer MTU (bounds each response PDU;
    /// upstream reads it from the L2CAP channel).
    pub fn new(mtu: u16) -> Self {
        SdpServer {
            records: Vec::new(),
            current_response: None,
            mtu,
        }
    }

    /// Register a service record under `handle`. Records keep their insertion
    /// order, which determines the order matched services appear in a response.
    pub fn add_service(&mut self, handle: u32, attributes: Vec<ServiceAttribute>) {
        self.records.push(ServiceRecord { handle, attributes });
    }

    /// Find the services matching a search pattern: a service matches if any one
    /// of the pattern's UUIDs appears in any of its attribute values. Mirrors
    /// upstream `match_services` (an OR match despite the "subset" comment), and
    /// preserves record insertion order.
    fn match_services(&self, pattern: &DataElement) -> Vec<(u32, &Vec<ServiceAttribute>)> {
        let uuids: Vec<&Uuid> = match pattern {
            DataElement::Sequence(elements) => elements
                .iter()
                .filter_map(|e| match e {
                    DataElement::Uuid(u) => Some(u),
                    _ => None,
                })
                .collect(),
            _ => Vec::new(),
        };
        let mut matching = Vec::new();
        for record in &self.records {
            for &uuid in &uuids {
                if record
                    .attributes
                    .iter()
                    .any(|a| is_uuid_in_value(uuid, &a.value))
                {
                    matching.push((record.handle, &record.attributes));
                    break;
                }
            }
        }
        matching
    }

    /// Collect the attributes of `service` selected by `attribute_ids`, sorted
    /// by id, as the flat `[id, value, …]` sequence a response carries. Mirrors
    /// upstream's static `get_service_attributes`: a 4-byte selector is an
    /// inclusive range, anything else a single id.
    fn get_service_attributes(
        service: &[ServiceAttribute],
        attribute_ids: &[DataElement],
    ) -> DataElement {
        let mut selected: Vec<&ServiceAttribute> = Vec::new();
        for selector in attribute_ids {
            if let DataElement::UnsignedInteger { value, size } = selector {
                let (start, end) = if *size == 4 {
                    ((*value >> 16) as u16, (*value & 0xFFFF) as u16)
                } else {
                    (*value as u16, *value as u16)
                };
                for attribute in service {
                    if attribute.id >= start && attribute.id <= end {
                        selected.push(attribute);
                    }
                }
            }
        }
        // Stable sort by id, matching upstream's `list.sort(key=...)`.
        selected.sort_by_key(|a| a.id);
        let mut elements = Vec::with_capacity(selected.len() * 2);
        for attribute in selected {
            elements.push(DataElement::unsigned_integer_16(attribute.id));
            elements.push(attribute.value.clone());
        }
        DataElement::Sequence(elements)
    }

    fn check_continuation(&mut self, continuation_state: &[u8]) -> Continuation {
        if continuation_state.len() > 1 {
            if self.current_response.is_none() || continuation_state != SERVER_CONTINUATION_STATE {
                return Continuation::Error(error_code::INVALID_CONTINUATION_STATE);
            }
            Continuation::Continue
        } else {
            // Fresh request: drop any leftover partial response.
            self.current_response = None;
            Continuation::Fresh
        }
    }

    /// Slice the next chunk of a byte-valued `current_response`, returning
    /// `(payload, continuation_state)`. Mirrors `get_next_response_payload`.
    fn next_byte_payload(&mut self, maximum_size: usize) -> (Vec<u8>, Vec<u8>) {
        let buffer = match self.current_response.take() {
            Some(CurrentResponse::Bytes(b)) => b,
            other => {
                // Not a byte response (shouldn't happen); restore and end.
                self.current_response = other;
                return (Vec::new(), vec![0]);
            }
        };
        if buffer.len() > maximum_size {
            let payload = buffer[..maximum_size].to_vec();
            self.current_response = Some(CurrentResponse::Bytes(buffer[maximum_size..].to_vec()));
            (payload, SERVER_CONTINUATION_STATE.to_vec())
        } else {
            // Final chunk: current_response was taken and stays cleared.
            (buffer, vec![0])
        }
    }

    /// The per-PDU byte cap for attribute responses: the smaller of the client's
    /// request and `mtu - 9` (upstream's header allowance).
    fn attribute_byte_cap(&self, requested: u16) -> usize {
        (self.mtu.saturating_sub(9) as usize).min(requested as usize)
    }

    fn on_service_search_attribute(
        &mut self,
        transaction_id: u16,
        pattern: &DataElement,
        maximum_attribute_byte_count: u16,
        attribute_ids: &DataElement,
        continuation_state: &[u8],
    ) -> SdpPdu {
        match self.check_continuation(continuation_state) {
            Continuation::Error(code) => {
                return SdpPdu::ErrorResponse {
                    transaction_id,
                    error_code: code,
                }
            }
            Continuation::Fresh => {
                let ids = sequence_elements(attribute_ids);
                let serialized = {
                    let matching = self.match_services(pattern);
                    let mut lists = Vec::new();
                    for (_handle, attributes) in &matching {
                        let list = Self::get_service_attributes(attributes, ids);
                        if matches!(&list, DataElement::Sequence(v) if !v.is_empty()) {
                            lists.push(list);
                        }
                    }
                    DataElement::Sequence(lists).to_bytes().unwrap_or_default()
                };
                self.current_response = Some(CurrentResponse::Bytes(serialized));
            }
            Continuation::Continue => {}
        }
        let cap = self.attribute_byte_cap(maximum_attribute_byte_count);
        let (attribute_lists, continuation) = self.next_byte_payload(cap);
        SdpPdu::ServiceSearchAttributeResponse {
            transaction_id,
            attribute_lists,
            continuation_state: continuation,
        }
    }

    fn on_service_attribute(
        &mut self,
        transaction_id: u16,
        service_record_handle: u32,
        maximum_attribute_byte_count: u16,
        attribute_ids: &DataElement,
        continuation_state: &[u8],
    ) -> SdpPdu {
        match self.check_continuation(continuation_state) {
            Continuation::Error(code) => {
                return SdpPdu::ErrorResponse {
                    transaction_id,
                    error_code: code,
                }
            }
            Continuation::Fresh => {
                let ids = sequence_elements(attribute_ids);
                let serialized = match self
                    .records
                    .iter()
                    .find(|r| r.handle == service_record_handle)
                {
                    Some(record) => Self::get_service_attributes(&record.attributes, ids)
                        .to_bytes()
                        .unwrap_or_default(),
                    None => {
                        return SdpPdu::ErrorResponse {
                            transaction_id,
                            error_code: error_code::INVALID_SERVICE_RECORD_HANDLE,
                        }
                    }
                };
                self.current_response = Some(CurrentResponse::Bytes(serialized));
            }
            Continuation::Continue => {}
        }
        let cap = self.attribute_byte_cap(maximum_attribute_byte_count);
        let (attribute_list, continuation) = self.next_byte_payload(cap);
        SdpPdu::ServiceAttributeResponse {
            transaction_id,
            attribute_list,
            continuation_state: continuation,
        }
    }

    fn on_service_search(
        &mut self,
        transaction_id: u16,
        pattern: &DataElement,
        maximum_service_record_count: u16,
        continuation_state: &[u8],
    ) -> SdpPdu {
        match self.check_continuation(continuation_state) {
            Continuation::Error(code) => {
                return SdpPdu::ErrorResponse {
                    transaction_id,
                    error_code: code,
                }
            }
            Continuation::Fresh => {
                let handles: Vec<u32> = self
                    .match_services(pattern)
                    .iter()
                    .map(|(h, _)| *h)
                    .collect();
                let total = handles.len() as u16;
                let subset: Vec<u32> = handles
                    .into_iter()
                    .take(maximum_service_record_count as usize)
                    .collect();
                self.current_response = Some(CurrentResponse::Handles {
                    total,
                    remaining: subset,
                });
            }
            Continuation::Continue => {}
        }

        let (total, remaining) = match self.current_response.take() {
            Some(CurrentResponse::Handles { total, remaining }) => (total, remaining),
            other => {
                self.current_response = other;
                return SdpPdu::ErrorResponse {
                    transaction_id,
                    error_code: error_code::INVALID_CONTINUATION_STATE,
                };
            }
        };
        // Chunk handles by how many 4-byte handles fit under the MTU.
        let max_count = (self.mtu.saturating_sub(11) / 4) as usize;
        let this_chunk: Vec<u32> = remaining.iter().copied().take(max_count).collect();
        let remaining_after: Vec<u32> = remaining.iter().copied().skip(max_count).collect();
        let continuation = if remaining_after.is_empty() {
            vec![0]
        } else {
            SERVER_CONTINUATION_STATE.to_vec()
        };
        // Upstream keeps the (total, remaining) tuple even when empty.
        self.current_response = Some(CurrentResponse::Handles {
            total,
            remaining: remaining_after,
        });
        SdpPdu::ServiceSearchResponse {
            transaction_id,
            total_service_record_count: total,
            service_record_handle_list: this_chunk,
            continuation_state: continuation,
        }
    }
}

impl SdpRequestHandler for SdpServer {
    fn handle_request(&mut self, request: &SdpPdu) -> SdpPdu {
        match request {
            SdpPdu::ServiceSearchAttributeRequest {
                transaction_id,
                service_search_pattern,
                maximum_attribute_byte_count,
                attribute_id_list,
                continuation_state,
            } => self.on_service_search_attribute(
                *transaction_id,
                service_search_pattern,
                *maximum_attribute_byte_count,
                attribute_id_list,
                continuation_state,
            ),
            SdpPdu::ServiceAttributeRequest {
                transaction_id,
                service_record_handle,
                maximum_attribute_byte_count,
                attribute_id_list,
                continuation_state,
            } => self.on_service_attribute(
                *transaction_id,
                *service_record_handle,
                *maximum_attribute_byte_count,
                attribute_id_list,
                continuation_state,
            ),
            SdpPdu::ServiceSearchRequest {
                transaction_id,
                service_search_pattern,
                maximum_service_record_count,
                continuation_state,
            } => self.on_service_search(
                *transaction_id,
                service_search_pattern,
                *maximum_service_record_count,
                continuation_state,
            ),
            // Responses and error PDUs are not requests the server answers.
            other => SdpPdu::ErrorResponse {
                transaction_id: other.transaction_id(),
                error_code: error_code::INVALID_REQUEST_SYNTAX,
            },
        }
    }
}

/// Borrow the elements of a sequence element, or an empty slice for anything
/// else.
fn sequence_elements(element: &DataElement) -> &[DataElement] {
    match element {
        DataElement::Sequence(elements) => elements,
        _ => &[],
    }
}

// -----------------------------------------------------------------------------
// Client
// -----------------------------------------------------------------------------

/// Errors from an SDP client request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClientError {
    /// The server returned an SDP error response.
    Protocol(u16),
    /// The server returned a PDU that did not match the request.
    Unexpected,
    /// A response payload could not be parsed.
    Parse(crate::Error),
}

impl core::fmt::Display for ClientError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            ClientError::Protocol(code) => write!(f, "SDP error response: {code:#06x}"),
            ClientError::Unexpected => write!(f, "unexpected SDP response PDU"),
            ClientError::Parse(e) => write!(f, "failed to parse SDP response: {e}"),
        }
    }
}

impl std::error::Error for ClientError {}

/// An SDP client: issues queries through a [`SdpTransport`] and reassembles
/// answers across continuation state.
#[derive(Debug)]
pub struct SdpClient<T: SdpTransport> {
    transport: T,
    next_transaction_id: u16,
}

impl<T: SdpTransport> SdpClient<T> {
    /// Wrap a transport.
    pub fn new(transport: T) -> Self {
        SdpClient {
            transport,
            next_transaction_id: 0,
        }
    }

    /// Borrow the underlying transport (e.g. to inspect a wrapped server).
    pub fn transport_mut(&mut self) -> &mut T {
        &mut self.transport
    }

    /// Recover the underlying transport.
    pub fn into_transport(self) -> T {
        self.transport
    }

    fn make_transaction_id(&mut self) -> u16 {
        let id = self.next_transaction_id;
        // Wrapping is exactly upstream's `& 0xFFFF` for a u16.
        self.next_transaction_id = self.next_transaction_id.wrapping_add(1);
        id
    }

    /// Service Search Attribute: find the services matching `uuids` and return,
    /// per matching service, the requested attributes. Runs the continuation
    /// loop and parses the accumulated `SEQUENCE`-of-`SEQUENCE`s answer.
    pub fn service_search_attribute(
        &mut self,
        uuids: &[Uuid],
        attribute_ids: &[AttributeId],
    ) -> Result<Vec<Vec<ServiceAttribute>>, ClientError> {
        let pattern = search_pattern(uuids);
        let ids = attribute_id_list(attribute_ids);
        let mut accumulator = Vec::new();
        let mut continuation_state = vec![0u8];
        for _ in 0..CONTINUATION_WATCHDOG {
            let request = SdpPdu::ServiceSearchAttributeRequest {
                transaction_id: self.make_transaction_id(),
                service_search_pattern: pattern.clone(),
                maximum_attribute_byte_count: 0xFFFF,
                attribute_id_list: ids.clone(),
                continuation_state: continuation_state.clone(),
            };
            match self.transport.request(&request) {
                SdpPdu::ServiceSearchAttributeResponse {
                    attribute_lists,
                    continuation_state: cs,
                    ..
                } => {
                    accumulator.extend_from_slice(&attribute_lists);
                    if is_terminal(&cs) {
                        return parse_attribute_lists(&accumulator).map_err(ClientError::Parse);
                    }
                    continuation_state = cs;
                }
                SdpPdu::ErrorResponse { error_code, .. } => {
                    return Err(ClientError::Protocol(error_code))
                }
                _ => return Err(ClientError::Unexpected),
            }
        }
        // Watchdog exhausted: return what we have.
        parse_attribute_lists(&accumulator).map_err(ClientError::Parse)
    }

    /// Service Attribute: fetch the requested attributes of a single service
    /// record by handle.
    pub fn get_attributes(
        &mut self,
        service_record_handle: u32,
        attribute_ids: &[AttributeId],
    ) -> Result<Vec<ServiceAttribute>, ClientError> {
        let ids = attribute_id_list(attribute_ids);
        let mut accumulator = Vec::new();
        let mut continuation_state = vec![0u8];
        for _ in 0..CONTINUATION_WATCHDOG {
            let request = SdpPdu::ServiceAttributeRequest {
                transaction_id: self.make_transaction_id(),
                service_record_handle,
                maximum_attribute_byte_count: 0xFFFF,
                attribute_id_list: ids.clone(),
                continuation_state: continuation_state.clone(),
            };
            match self.transport.request(&request) {
                SdpPdu::ServiceAttributeResponse {
                    attribute_list,
                    continuation_state: cs,
                    ..
                } => {
                    accumulator.extend_from_slice(&attribute_list);
                    if is_terminal(&cs) {
                        return parse_attribute_list(&accumulator).map_err(ClientError::Parse);
                    }
                    continuation_state = cs;
                }
                SdpPdu::ErrorResponse { error_code, .. } => {
                    return Err(ClientError::Protocol(error_code))
                }
                _ => return Err(ClientError::Unexpected),
            }
        }
        parse_attribute_list(&accumulator).map_err(ClientError::Parse)
    }

    /// Service Search: return the handles of the services matching `uuids`.
    pub fn search_services(&mut self, uuids: &[Uuid]) -> Result<Vec<u32>, ClientError> {
        let pattern = search_pattern(uuids);
        let mut handles = Vec::new();
        let mut continuation_state = vec![0u8];
        for _ in 0..CONTINUATION_WATCHDOG {
            let request = SdpPdu::ServiceSearchRequest {
                transaction_id: self.make_transaction_id(),
                service_search_pattern: pattern.clone(),
                maximum_service_record_count: 0xFFFF,
                continuation_state: continuation_state.clone(),
            };
            match self.transport.request(&request) {
                SdpPdu::ServiceSearchResponse {
                    service_record_handle_list,
                    continuation_state: cs,
                    ..
                } => {
                    handles.extend_from_slice(&service_record_handle_list);
                    if is_terminal(&cs) {
                        return Ok(handles);
                    }
                    continuation_state = cs;
                }
                SdpPdu::ErrorResponse { error_code, .. } => {
                    return Err(ClientError::Protocol(error_code))
                }
                _ => return Err(ClientError::Unexpected),
            }
        }
        Ok(handles)
    }
}

/// The continuation state is terminal when it is a single zero byte.
fn is_terminal(continuation_state: &[u8]) -> bool {
    continuation_state.len() == 1 && continuation_state[0] == 0
}

/// Parse an accumulated Service-Search-Attribute answer: a `SEQUENCE` of
/// per-service `SEQUENCE`s, each a flat attribute list.
fn parse_attribute_lists(bytes: &[u8]) -> crate::Result<Vec<Vec<ServiceAttribute>>> {
    if bytes.is_empty() {
        return Ok(Vec::new());
    }
    let outer = DataElement::from_bytes(bytes)?;
    let DataElement::Sequence(services) = outer else {
        return Ok(Vec::new());
    };
    Ok(services
        .into_iter()
        .filter_map(|service| match service {
            DataElement::Sequence(elements) => {
                Some(ServiceAttribute::list_from_data_elements(&elements))
            }
            _ => None,
        })
        .collect())
}

/// Parse an accumulated Service-Attribute answer: a single flat attribute-list
/// `SEQUENCE`.
fn parse_attribute_list(bytes: &[u8]) -> crate::Result<Vec<ServiceAttribute>> {
    if bytes.is_empty() {
        return Ok(Vec::new());
    }
    match DataElement::from_bytes(bytes)? {
        DataElement::Sequence(elements) => Ok(ServiceAttribute::list_from_data_elements(&elements)),
        _ => Ok(Vec::new()),
    }
}
