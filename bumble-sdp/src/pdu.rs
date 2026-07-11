//! The seven SDP Protocol Data Units (Vol 3, Part B - 4.4–4.7).
//!
//! Every PDU shares the framing
//! `[pdu-id: u8][transaction-id: u16 BE][parameter-length: u16 BE][parameters…]`
//! and every request/response ends with a `continuation_state` byte string
//! (empty when the whole answer fit in one PDU), carried here verbatim.

use super::{pdu_id, truncated, DataElement, Error, Result};

/// An SDP Protocol Data Unit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SdpPdu {
    /// Error Response (4.4.1).
    ErrorResponse {
        transaction_id: u16,
        error_code: u16,
    },
    /// Service Search Request (4.5.1).
    ServiceSearchRequest {
        transaction_id: u16,
        service_search_pattern: DataElement,
        maximum_service_record_count: u16,
        continuation_state: Vec<u8>,
    },
    /// Service Search Response (4.5.2).
    ServiceSearchResponse {
        transaction_id: u16,
        total_service_record_count: u16,
        service_record_handle_list: Vec<u32>,
        continuation_state: Vec<u8>,
    },
    /// Service Attribute Request (4.6.1).
    ServiceAttributeRequest {
        transaction_id: u16,
        service_record_handle: u32,
        maximum_attribute_byte_count: u16,
        attribute_id_list: DataElement,
        continuation_state: Vec<u8>,
    },
    /// Service Attribute Response (4.6.2).
    ServiceAttributeResponse {
        transaction_id: u16,
        attribute_list: Vec<u8>,
        continuation_state: Vec<u8>,
    },
    /// Service Search Attribute Request (4.7.1).
    ServiceSearchAttributeRequest {
        transaction_id: u16,
        service_search_pattern: DataElement,
        maximum_attribute_byte_count: u16,
        attribute_id_list: DataElement,
        continuation_state: Vec<u8>,
    },
    /// Service Search Attribute Response (4.7.2).
    ServiceSearchAttributeResponse {
        transaction_id: u16,
        attribute_lists: Vec<u8>,
        continuation_state: Vec<u8>,
    },
}

impl SdpPdu {
    /// The PDU identifier byte.
    pub fn pdu_id(&self) -> u8 {
        match self {
            SdpPdu::ErrorResponse { .. } => pdu_id::SDP_ERROR_RESPONSE,
            SdpPdu::ServiceSearchRequest { .. } => pdu_id::SDP_SERVICE_SEARCH_REQUEST,
            SdpPdu::ServiceSearchResponse { .. } => pdu_id::SDP_SERVICE_SEARCH_RESPONSE,
            SdpPdu::ServiceAttributeRequest { .. } => pdu_id::SDP_SERVICE_ATTRIBUTE_REQUEST,
            SdpPdu::ServiceAttributeResponse { .. } => pdu_id::SDP_SERVICE_ATTRIBUTE_RESPONSE,
            SdpPdu::ServiceSearchAttributeRequest { .. } => {
                pdu_id::SDP_SERVICE_SEARCH_ATTRIBUTE_REQUEST
            }
            SdpPdu::ServiceSearchAttributeResponse { .. } => {
                pdu_id::SDP_SERVICE_SEARCH_ATTRIBUTE_RESPONSE
            }
        }
    }

    /// The PDU's transaction id.
    pub fn transaction_id(&self) -> u16 {
        match self {
            SdpPdu::ErrorResponse { transaction_id, .. }
            | SdpPdu::ServiceSearchRequest { transaction_id, .. }
            | SdpPdu::ServiceSearchResponse { transaction_id, .. }
            | SdpPdu::ServiceAttributeRequest { transaction_id, .. }
            | SdpPdu::ServiceAttributeResponse { transaction_id, .. }
            | SdpPdu::ServiceSearchAttributeRequest { transaction_id, .. }
            | SdpPdu::ServiceSearchAttributeResponse { transaction_id, .. } => *transaction_id,
        }
    }

    /// Serialize the PDU (framing header + parameters).
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        let parameters = self.parameters()?;
        let mut out = Vec::with_capacity(5 + parameters.len());
        out.push(self.pdu_id());
        out.extend_from_slice(&self.transaction_id().to_be_bytes());
        out.extend_from_slice(&(parameters.len() as u16).to_be_bytes());
        out.extend_from_slice(&parameters);
        Ok(out)
    }

    /// The serialized parameter block (everything after the 5-byte header).
    fn parameters(&self) -> Result<Vec<u8>> {
        let mut p = Vec::new();
        match self {
            SdpPdu::ErrorResponse { error_code, .. } => {
                // NB: little-endian. Upstream declares error_code with the HCI
                // default u16 encoding (`type_metadata(2)`), which is
                // little-endian — unlike the other SDP integer fields, which
                // are explicitly big-endian (`'>2'`/`'>4'`). The oracle caught
                // this: error_code 0x0102 serializes to `0201`, not `0102`.
                p.extend_from_slice(&error_code.to_le_bytes());
            }
            SdpPdu::ServiceSearchRequest {
                service_search_pattern,
                maximum_service_record_count,
                continuation_state,
                ..
            } => {
                p.extend_from_slice(&service_search_pattern.to_bytes()?);
                p.extend_from_slice(&maximum_service_record_count.to_be_bytes());
                p.extend_from_slice(continuation_state);
            }
            SdpPdu::ServiceSearchResponse {
                total_service_record_count,
                service_record_handle_list,
                continuation_state,
                ..
            } => {
                p.extend_from_slice(&total_service_record_count.to_be_bytes());
                p.extend_from_slice(&(service_record_handle_list.len() as u16).to_be_bytes());
                for handle in service_record_handle_list {
                    p.extend_from_slice(&handle.to_be_bytes());
                }
                p.extend_from_slice(continuation_state);
            }
            SdpPdu::ServiceAttributeRequest {
                service_record_handle,
                maximum_attribute_byte_count,
                attribute_id_list,
                continuation_state,
                ..
            } => {
                p.extend_from_slice(&service_record_handle.to_be_bytes());
                p.extend_from_slice(&maximum_attribute_byte_count.to_be_bytes());
                p.extend_from_slice(&attribute_id_list.to_bytes()?);
                p.extend_from_slice(continuation_state);
            }
            SdpPdu::ServiceAttributeResponse {
                attribute_list,
                continuation_state,
                ..
            } => {
                p.extend_from_slice(&(attribute_list.len() as u16).to_be_bytes());
                p.extend_from_slice(attribute_list);
                p.extend_from_slice(continuation_state);
            }
            SdpPdu::ServiceSearchAttributeRequest {
                service_search_pattern,
                maximum_attribute_byte_count,
                attribute_id_list,
                continuation_state,
                ..
            } => {
                p.extend_from_slice(&service_search_pattern.to_bytes()?);
                p.extend_from_slice(&maximum_attribute_byte_count.to_be_bytes());
                p.extend_from_slice(&attribute_id_list.to_bytes()?);
                p.extend_from_slice(continuation_state);
            }
            SdpPdu::ServiceSearchAttributeResponse {
                attribute_lists,
                continuation_state,
                ..
            } => {
                p.extend_from_slice(&(attribute_lists.len() as u16).to_be_bytes());
                p.extend_from_slice(attribute_lists);
                p.extend_from_slice(continuation_state);
            }
        }
        Ok(p)
    }

    /// Parse a complete SDP PDU (framing header + parameters).
    pub fn from_bytes(pdu: &[u8]) -> Result<SdpPdu> {
        if pdu.len() < 5 {
            return Err(truncated("SDP PDU header"));
        }
        let id = pdu[0];
        let transaction_id = u16::from_be_bytes([pdu[1], pdu[2]]);
        let parameter_length = u16::from_be_bytes([pdu[3], pdu[4]]) as usize;
        let params = pdu
            .get(5..5 + parameter_length)
            .ok_or_else(|| truncated("SDP PDU parameters"))?;
        let mut c = Cursor::new(params);

        let result = match id {
            pdu_id::SDP_ERROR_RESPONSE => SdpPdu::ErrorResponse {
                transaction_id,
                error_code: c.u16_le()?,
            },
            pdu_id::SDP_SERVICE_SEARCH_REQUEST => SdpPdu::ServiceSearchRequest {
                transaction_id,
                service_search_pattern: c.data_element()?,
                maximum_service_record_count: c.u16()?,
                continuation_state: c.rest(),
            },
            pdu_id::SDP_SERVICE_SEARCH_RESPONSE => {
                let total_service_record_count = c.u16()?;
                let count = c.u16()? as usize;
                let mut service_record_handle_list = Vec::with_capacity(count);
                for _ in 0..count {
                    service_record_handle_list.push(c.u32()?);
                }
                SdpPdu::ServiceSearchResponse {
                    transaction_id,
                    total_service_record_count,
                    service_record_handle_list,
                    continuation_state: c.rest(),
                }
            }
            pdu_id::SDP_SERVICE_ATTRIBUTE_REQUEST => SdpPdu::ServiceAttributeRequest {
                transaction_id,
                service_record_handle: c.u32()?,
                maximum_attribute_byte_count: c.u16()?,
                attribute_id_list: c.data_element()?,
                continuation_state: c.rest(),
            },
            pdu_id::SDP_SERVICE_ATTRIBUTE_RESPONSE => SdpPdu::ServiceAttributeResponse {
                transaction_id,
                attribute_list: c.length_prefixed_bytes()?,
                continuation_state: c.rest(),
            },
            pdu_id::SDP_SERVICE_SEARCH_ATTRIBUTE_REQUEST => SdpPdu::ServiceSearchAttributeRequest {
                transaction_id,
                service_search_pattern: c.data_element()?,
                maximum_attribute_byte_count: c.u16()?,
                attribute_id_list: c.data_element()?,
                continuation_state: c.rest(),
            },
            pdu_id::SDP_SERVICE_SEARCH_ATTRIBUTE_RESPONSE => {
                SdpPdu::ServiceSearchAttributeResponse {
                    transaction_id,
                    attribute_lists: c.length_prefixed_bytes()?,
                    continuation_state: c.rest(),
                }
            }
            other => {
                return Err(Error::InvalidPacket(format!(
                    "unknown SDP PDU id {other:#04x}"
                )))
            }
        };
        Ok(result)
    }
}

/// A forward-only reader over a PDU's parameter bytes.
struct Cursor<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    fn new(data: &'a [u8]) -> Self {
        Cursor { data, pos: 0 }
    }

    fn u16(&mut self) -> Result<u16> {
        let s = self
            .data
            .get(self.pos..self.pos + 2)
            .ok_or_else(|| truncated("u16"))?;
        self.pos += 2;
        Ok(u16::from_be_bytes([s[0], s[1]]))
    }

    /// A little-endian u16 (used only by the error_code field; see the note in
    /// [`SdpPdu::parameters`]).
    fn u16_le(&mut self) -> Result<u16> {
        let s = self
            .data
            .get(self.pos..self.pos + 2)
            .ok_or_else(|| truncated("u16"))?;
        self.pos += 2;
        Ok(u16::from_le_bytes([s[0], s[1]]))
    }

    fn u32(&mut self) -> Result<u32> {
        let s = self
            .data
            .get(self.pos..self.pos + 4)
            .ok_or_else(|| truncated("u32"))?;
        self.pos += 4;
        Ok(u32::from_be_bytes([s[0], s[1], s[2], s[3]]))
    }

    fn data_element(&mut self) -> Result<DataElement> {
        let (next, element) = DataElement::parse_from_bytes(self.data, self.pos)?;
        self.pos = next;
        Ok(element)
    }

    /// A `[length: u16 BE][bytes]` block.
    fn length_prefixed_bytes(&mut self) -> Result<Vec<u8>> {
        let length = self.u16()? as usize;
        let bytes = self
            .data
            .get(self.pos..self.pos + length)
            .ok_or_else(|| truncated("length-prefixed bytes"))?
            .to_vec();
        self.pos += length;
        Ok(bytes)
    }

    /// Everything left (the continuation state).
    fn rest(&mut self) -> Vec<u8> {
        let out = self.data[self.pos..].to_vec();
        self.pos = self.data.len();
        out
    }
}
