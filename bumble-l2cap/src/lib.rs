//! bumble-l2cap — a Rust port of the L2CAP frame codec from
//! [`google/bumble`](https://github.com/google/bumble).
//!
//! **Slice 4** of the incremental port: the L2CAP data-packet frame
//! ([`L2capPdu`]), the signaling [`ControlFrame`]s, the variable-length PSM
//! encoding, and the frame-check-sequence CRC. std-only — the L2CAP frame
//! format is independent of HCI and addresses.
//!
//! ## Scope
//!
//! Implemented: the basic L2CAP PDU with optional FCS, PSM (de)serialization,
//! and the Connection_Request plus the four credit-based signaling frames,
//! with a [`ControlFrame::Generic`] fallback for other signaling codes.
//!
//! Deferred to later slices: the full signaling command set, configuration
//! option encoding, enhanced-retransmission control fields, and the channel
//! manager / fragmentation-and-reassembly logic.

use core::fmt;

/// L2CAP signaling command codes (Vol 3, Part A - 4).
pub mod codes {
    pub const COMMAND_REJECT: u8 = 0x01;
    pub const CONNECTION_REQUEST: u8 = 0x02;
    pub const CONNECTION_RESPONSE: u8 = 0x03;
    pub const CONFIGURE_REQUEST: u8 = 0x04;
    pub const CONFIGURE_RESPONSE: u8 = 0x05;
    pub const DISCONNECTION_REQUEST: u8 = 0x06;
    pub const DISCONNECTION_RESPONSE: u8 = 0x07;
    pub const ECHO_REQUEST: u8 = 0x08;
    pub const ECHO_RESPONSE: u8 = 0x09;
    pub const INFORMATION_REQUEST: u8 = 0x0A;
    pub const INFORMATION_RESPONSE: u8 = 0x0B;
    pub const CONNECTION_PARAMETER_UPDATE_REQUEST: u8 = 0x12;
    pub const CONNECTION_PARAMETER_UPDATE_RESPONSE: u8 = 0x13;
    pub const LE_CREDIT_BASED_CONNECTION_REQUEST: u8 = 0x14;
    pub const LE_CREDIT_BASED_CONNECTION_RESPONSE: u8 = 0x15;
    pub const LE_FLOW_CONTROL_CREDIT: u8 = 0x16;
    pub const CREDIT_BASED_CONNECTION_REQUEST: u8 = 0x17;
    pub const CREDIT_BASED_CONNECTION_RESPONSE: u8 = 0x18;
    pub const CREDIT_BASED_RECONFIGURE_REQUEST: u8 = 0x19;
    pub const CREDIT_BASED_RECONFIGURE_RESPONSE: u8 = 0x1A;
}

/// The signaling channel identifier used for BR/EDR L2CAP.
pub const L2CAP_SIGNALING_CID: u16 = 0x0001;
/// The signaling channel identifier used for LE L2CAP.
pub const L2CAP_LE_SIGNALING_CID: u16 = 0x0005;

/// Errors produced while parsing L2CAP frames.
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

/// CRC-16-IBM (reversed polynomial `0xA001`, initial value `0x0000`) — the
/// L2CAP Frame Check Sequence (Vol 3, Part A - 3.3.5).
pub fn crc_16(data: &[u8]) -> u16 {
    let mut crc: u16 = 0x0000;
    for &byte in data {
        crc ^= byte as u16;
        for _ in 0..8 {
            if crc & 0x0001 != 0 {
                crc = (crc >> 1) ^ 0xA001;
            } else {
                crc >>= 1;
            }
        }
    }
    crc
}

/// Serialize a PSM: the low 16 bits little-endian, then one byte per remaining
/// 8 bits (Vol 3, Part A - 4.2).
pub fn serialize_psm(psm: u32) -> Vec<u8> {
    let mut out = ((psm & 0xFFFF) as u16).to_le_bytes().to_vec();
    let mut rest = psm >> 16;
    while rest != 0 {
        out.push((rest & 0xFF) as u8);
        rest >>= 8;
    }
    out
}

/// Parse a PSM starting at `offset`. The field is at least 2 bytes and extends
/// while the most-recently-read byte is odd. Returns `(next_offset, psm)`.
pub fn parse_psm(data: &[u8], offset: usize) -> Result<(usize, u32)> {
    if offset + 2 > data.len() {
        return Err(Error::InvalidPacket("not enough data for PSM".into()));
    }
    let mut psm = data[offset] as u32 | ((data[offset + 1] as u32) << 8);
    let mut psm_length = 2usize;
    while data[offset + psm_length - 1] % 2 == 1 {
        if offset + psm_length >= data.len() {
            return Err(Error::InvalidPacket("truncated PSM".into()));
        }
        psm |= (data[offset + psm_length] as u32) << (8 * psm_length);
        psm_length += 1;
    }
    Ok((offset + psm_length, psm))
}

/// An L2CAP data-packet PDU: a channel id plus a payload (Vol 3, Part A - 3).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct L2capPdu {
    pub cid: u16,
    pub payload: Vec<u8>,
}

impl L2capPdu {
    pub fn new(cid: u16, payload: Vec<u8>) -> L2capPdu {
        L2capPdu { cid, payload }
    }

    /// Parse a PDU. The 2-byte length prefixes the 2-byte CID; the payload is
    /// the following `length` bytes.
    pub fn from_bytes(data: &[u8]) -> Result<L2capPdu> {
        if data.len() < 4 {
            return Err(Error::InvalidPacket(
                "not enough data for L2CAP header".into(),
            ));
        }
        let length = u16::from_le_bytes([data[0], data[1]]) as usize;
        let cid = u16::from_le_bytes([data[2], data[3]]);
        let end = (4 + length).min(data.len());
        Ok(L2capPdu {
            cid,
            payload: data[4..end].to_vec(),
        })
    }

    /// Serialize. When `with_fcs` is set, the length field includes the 2-byte
    /// FCS, and the FCS (CRC-16 over the whole frame so far) is appended.
    pub fn to_bytes(&self, with_fcs: bool) -> Vec<u8> {
        let mut length = self.payload.len();
        if with_fcs {
            length += 2;
        }
        let mut body = Vec::with_capacity(4 + length);
        body.extend_from_slice(&(length as u16).to_le_bytes());
        body.extend_from_slice(&self.cid.to_le_bytes());
        body.extend_from_slice(&self.payload);
        if with_fcs {
            body.extend_from_slice(&crc_16(&body).to_le_bytes());
        }
        body
    }
}

/// An L2CAP signaling (control) frame (Vol 3, Part A - 4). Typed variants carry
/// decoded fields; [`ControlFrame::Generic`] preserves any other signaling code.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ControlFrame {
    ConnectionRequest {
        identifier: u8,
        psm: u32,
        source_cid: u16,
    },
    CreditBasedConnectionRequest {
        identifier: u8,
        spsm: u16,
        mtu: u16,
        mps: u16,
        initial_credits: u16,
        source_cid: Vec<u16>,
    },
    CreditBasedConnectionResponse {
        identifier: u8,
        mtu: u16,
        mps: u16,
        initial_credits: u16,
        result: u16,
        destination_cid: Vec<u16>,
    },
    CreditBasedReconfigureRequest {
        identifier: u8,
        mtu: u16,
        mps: u16,
        destination_cid: Vec<u16>,
    },
    CreditBasedReconfigureResponse {
        identifier: u8,
        result: u16,
    },
    /// Any signaling code not decoded by this slice.
    Generic {
        code: u8,
        identifier: u8,
        payload: Vec<u8>,
    },
}

fn push_u16(p: &mut Vec<u8>, v: u16) {
    p.extend_from_slice(&v.to_le_bytes());
}

fn read_cid_list(data: &[u8]) -> Vec<u16> {
    data.chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect()
}

impl ControlFrame {
    pub fn code(&self) -> u8 {
        match self {
            ControlFrame::ConnectionRequest { .. } => codes::CONNECTION_REQUEST,
            ControlFrame::CreditBasedConnectionRequest { .. } => {
                codes::CREDIT_BASED_CONNECTION_REQUEST
            }
            ControlFrame::CreditBasedConnectionResponse { .. } => {
                codes::CREDIT_BASED_CONNECTION_RESPONSE
            }
            ControlFrame::CreditBasedReconfigureRequest { .. } => {
                codes::CREDIT_BASED_RECONFIGURE_REQUEST
            }
            ControlFrame::CreditBasedReconfigureResponse { .. } => {
                codes::CREDIT_BASED_RECONFIGURE_RESPONSE
            }
            ControlFrame::Generic { code, .. } => *code,
        }
    }

    pub fn identifier(&self) -> u8 {
        match self {
            ControlFrame::ConnectionRequest { identifier, .. }
            | ControlFrame::CreditBasedConnectionRequest { identifier, .. }
            | ControlFrame::CreditBasedConnectionResponse { identifier, .. }
            | ControlFrame::CreditBasedReconfigureRequest { identifier, .. }
            | ControlFrame::CreditBasedReconfigureResponse { identifier, .. }
            | ControlFrame::Generic { identifier, .. } => *identifier,
        }
    }

    /// The signaling payload (the bytes after the 4-byte code/id/length header).
    pub fn payload(&self) -> Vec<u8> {
        let mut p = Vec::new();
        match self {
            ControlFrame::ConnectionRequest {
                psm, source_cid, ..
            } => {
                p.extend_from_slice(&serialize_psm(*psm));
                push_u16(&mut p, *source_cid);
            }
            ControlFrame::CreditBasedConnectionRequest {
                spsm,
                mtu,
                mps,
                initial_credits,
                source_cid,
                ..
            } => {
                push_u16(&mut p, *spsm);
                push_u16(&mut p, *mtu);
                push_u16(&mut p, *mps);
                push_u16(&mut p, *initial_credits);
                for cid in source_cid {
                    push_u16(&mut p, *cid);
                }
            }
            ControlFrame::CreditBasedConnectionResponse {
                mtu,
                mps,
                initial_credits,
                result,
                destination_cid,
                ..
            } => {
                push_u16(&mut p, *mtu);
                push_u16(&mut p, *mps);
                push_u16(&mut p, *initial_credits);
                push_u16(&mut p, *result);
                for cid in destination_cid {
                    push_u16(&mut p, *cid);
                }
            }
            ControlFrame::CreditBasedReconfigureRequest {
                mtu,
                mps,
                destination_cid,
                ..
            } => {
                push_u16(&mut p, *mtu);
                push_u16(&mut p, *mps);
                for cid in destination_cid {
                    push_u16(&mut p, *cid);
                }
            }
            ControlFrame::CreditBasedReconfigureResponse { result, .. } => {
                push_u16(&mut p, *result);
            }
            ControlFrame::Generic { payload, .. } => p.extend_from_slice(payload),
        }
        p
    }

    /// Serialize to the full signaling frame.
    pub fn to_bytes(&self) -> Vec<u8> {
        let payload = self.payload();
        let mut out = Vec::with_capacity(4 + payload.len());
        out.push(self.code());
        out.push(self.identifier());
        out.extend_from_slice(&(payload.len() as u16).to_le_bytes());
        out.extend_from_slice(&payload);
        out
    }

    /// Parse a signaling frame from its wire bytes.
    pub fn from_bytes(pdu: &[u8]) -> Result<ControlFrame> {
        if pdu.len() < 4 {
            return Err(Error::InvalidPacket(
                "not enough data for control frame".into(),
            ));
        }
        let code = pdu[0];
        let identifier = pdu[1];
        let length = u16::from_le_bytes([pdu[2], pdu[3]]) as usize;
        let end = (4 + length).min(pdu.len());
        let payload = &pdu[4..end];

        Ok(match code {
            codes::CONNECTION_REQUEST => {
                let (offset, psm) = parse_psm(payload, 0)?;
                if offset + 2 > payload.len() {
                    return Err(Error::InvalidPacket("truncated Connection_Request".into()));
                }
                let source_cid = u16::from_le_bytes([payload[offset], payload[offset + 1]]);
                ControlFrame::ConnectionRequest {
                    identifier,
                    psm,
                    source_cid,
                }
            }
            codes::CREDIT_BASED_CONNECTION_REQUEST => {
                need(payload, 8)?;
                ControlFrame::CreditBasedConnectionRequest {
                    identifier,
                    spsm: le16(payload, 0),
                    mtu: le16(payload, 2),
                    mps: le16(payload, 4),
                    initial_credits: le16(payload, 6),
                    source_cid: read_cid_list(&payload[8..]),
                }
            }
            codes::CREDIT_BASED_CONNECTION_RESPONSE => {
                need(payload, 8)?;
                ControlFrame::CreditBasedConnectionResponse {
                    identifier,
                    mtu: le16(payload, 0),
                    mps: le16(payload, 2),
                    initial_credits: le16(payload, 4),
                    result: le16(payload, 6),
                    destination_cid: read_cid_list(&payload[8..]),
                }
            }
            codes::CREDIT_BASED_RECONFIGURE_REQUEST => {
                need(payload, 4)?;
                ControlFrame::CreditBasedReconfigureRequest {
                    identifier,
                    mtu: le16(payload, 0),
                    mps: le16(payload, 2),
                    destination_cid: read_cid_list(&payload[4..]),
                }
            }
            codes::CREDIT_BASED_RECONFIGURE_RESPONSE => {
                need(payload, 2)?;
                ControlFrame::CreditBasedReconfigureResponse {
                    identifier,
                    result: le16(payload, 0),
                }
            }
            _ => ControlFrame::Generic {
                code,
                identifier,
                payload: payload.to_vec(),
            },
        })
    }
}

fn need(payload: &[u8], n: usize) -> Result<()> {
    if payload.len() < n {
        Err(Error::InvalidPacket(format!(
            "control frame payload too short: need {n}, have {}",
            payload.len()
        )))
    } else {
        Ok(())
    }
}

fn le16(data: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([data[offset], data[offset + 1]])
}
