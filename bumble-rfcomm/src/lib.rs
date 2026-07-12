//! bumble-rfcomm — a Rust port of the RFCOMM frame + MCC codec from
//! [`google/bumble`](https://github.com/google/bumble).
//!
//! **Slice 17** of the incremental port, and the second piece of Classic
//! Bluetooth (BR/EDR) infrastructure after [`bumble-sdp`]. RFCOMM (TS 07.10 /
//! Bluetooth RFCOMM spec) is the serial-cable emulation that runs over L2CAP
//! and carries the Serial Port Profile — the transport that SPP, HFP and many
//! other classic profiles are built on. A device finds an RFCOMM server
//! channel through an SDP service record (see [`bumble-sdp`]) and then speaks
//! this framing to it.
//!
//! [`bumble-sdp`]: https://docs.rs/bumble-sdp
//!
//! ## Scope
//!
//! Implemented, byte-for-byte against upstream:
//!
//! - [`RfcommFrame`] — the TS 07.10 UIH/SABM/UA/DM/DISC frame: the
//!   `[address, control, length, information…, fcs]` layout, the 1- and
//!   2-byte length indicators (EA bit), the credit-based flow-control variant
//!   of UIH, and the frame-check-sequence.
//! - [`compute_fcs`] — the CRC-8 frame check sequence over the FCS table.
//! - [`RfcommMccPn`] / [`RfcommMccMsc`] — the two Multiplexer Control Channel
//!   messages this port needs (Parameter Negotiation and Modem Status
//!   Command), plus [`make_mcc`]/[`parse_mcc`] for the MCC type/length header.
//!
//! The session runtime that drives this codec lives in [`mux`] (**slice 20**):
//! a synchronous, sans-I/O port of upstream's asyncio `Multiplexer` and `DLC`,
//! including the open handshake (SABM/UA, PN, MSC) and credit-based flow
//! control. [`l2cap`] (**slice 22**) binds that runtime to a live Classic L2CAP
//! channel. Still deferred: retransmission (upstream sets `max_retransmissions
//! = 0` too), aggregate flow control, and socket/async convenience APIs.
//!
//! ## Oracle
//!
//! Every serialization in the tests is pinned to a hex literal captured from
//! upstream Python Bumble at commit
//! `1d26b99865f96a3e7359009424c0ddf2934acd0b`, via `bytes(frame)` /
//! `bytes(mcc)`. The FCS table is hand-transcribed from upstream, so
//! [`compute_fcs`] is pinned directly against captured values to catch a
//! single-nibble transcription error locally rather than as a distant frame
//! failure.
//!
//! Undefined **frame-type** codes are rejected on parse (mirroring upstream's
//! `FrameType(...)` `ValueError` and this port's SDP unknown-type handling).
//! Credit-bearing UIH frames are byte-lossy through a parse round-trip in
//! upstream too — its `from_bytes` reconstructs them without the credit flag —
//! so those are pinned by serialization plus field recovery, not by
//! re-serialization; see the tests.

use core::fmt;

pub mod l2cap;
pub mod mux;

/// RFCOMM's fixed L2CAP PSM (Protocol/Service Multiplexer).
pub const RFCOMM_PSM: u16 = 0x0003;

/// Default number of credits granted at DLC open.
pub const RFCOMM_DEFAULT_INITIAL_CREDITS: u8 = 7;
/// Default maximum outstanding credits.
pub const RFCOMM_DEFAULT_MAX_CREDITS: u8 = 32;
/// Default maximum RFCOMM information frame size.
pub const RFCOMM_DEFAULT_MAX_FRAME_SIZE: u16 = 1000;
/// First dynamically-assignable RFCOMM server channel.
pub const RFCOMM_DYNAMIC_CHANNEL_NUMBER_START: u8 = 1;
/// Last dynamically-assignable RFCOMM server channel.
pub const RFCOMM_DYNAMIC_CHANNEL_NUMBER_END: u8 = 30;

/// Errors produced while parsing or serializing RFCOMM frames and MCC messages.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// The bytes are malformed, truncated, use an unknown frame type, or fail
    /// the frame-check-sequence.
    InvalidPacket(String),
    /// A value cannot be serialized as requested (e.g. a credit-bearing frame
    /// with no information octet to carry the credit count).
    InvalidArgument(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::InvalidPacket(m) => write!(f, "invalid packet: {m}"),
            Error::InvalidArgument(m) => write!(f, "invalid argument: {m}"),
        }
    }
}

impl std::error::Error for Error {}

pub type Result<T> = core::result::Result<T, Error>;

/// Build an `InvalidPacket` error for a truncated field.
pub(crate) fn truncated(what: &str) -> Error {
    Error::InvalidPacket(format!("truncated: expected more bytes for {what}"))
}

// -----------------------------------------------------------------------------
// Frame check sequence (CRC-8)
// -----------------------------------------------------------------------------

/// The RFCOMM FCS lookup table (TS 07.10 Annex B), transcribed from upstream.
#[rustfmt::skip]
const CRC_TABLE: [u8; 256] = [
    0x00, 0x91, 0xE3, 0x72, 0x07, 0x96, 0xE4, 0x75,
    0x0E, 0x9F, 0xED, 0x7C, 0x09, 0x98, 0xEA, 0x7B,
    0x1C, 0x8D, 0xFF, 0x6E, 0x1B, 0x8A, 0xF8, 0x69,
    0x12, 0x83, 0xF1, 0x60, 0x15, 0x84, 0xF6, 0x67,
    0x38, 0xA9, 0xDB, 0x4A, 0x3F, 0xAE, 0xDC, 0x4D,
    0x36, 0xA7, 0xD5, 0x44, 0x31, 0xA0, 0xD2, 0x43,
    0x24, 0xB5, 0xC7, 0x56, 0x23, 0xB2, 0xC0, 0x51,
    0x2A, 0xBB, 0xC9, 0x58, 0x2D, 0xBC, 0xCE, 0x5F,
    0x70, 0xE1, 0x93, 0x02, 0x77, 0xE6, 0x94, 0x05,
    0x7E, 0xEF, 0x9D, 0x0C, 0x79, 0xE8, 0x9A, 0x0B,
    0x6C, 0xFD, 0x8F, 0x1E, 0x6B, 0xFA, 0x88, 0x19,
    0x62, 0xF3, 0x81, 0x10, 0x65, 0xF4, 0x86, 0x17,
    0x48, 0xD9, 0xAB, 0x3A, 0x4F, 0xDE, 0xAC, 0x3D,
    0x46, 0xD7, 0xA5, 0x34, 0x41, 0xD0, 0xA2, 0x33,
    0x54, 0xC5, 0xB7, 0x26, 0x53, 0xC2, 0xB0, 0x21,
    0x5A, 0xCB, 0xB9, 0x28, 0x5D, 0xCC, 0xBE, 0x2F,
    0xE0, 0x71, 0x03, 0x92, 0xE7, 0x76, 0x04, 0x95,
    0xEE, 0x7F, 0x0D, 0x9C, 0xE9, 0x78, 0x0A, 0x9B,
    0xFC, 0x6D, 0x1F, 0x8E, 0xFB, 0x6A, 0x18, 0x89,
    0xF2, 0x63, 0x11, 0x80, 0xF5, 0x64, 0x16, 0x87,
    0xD8, 0x49, 0x3B, 0xAA, 0xDF, 0x4E, 0x3C, 0xAD,
    0xD6, 0x47, 0x35, 0xA4, 0xD1, 0x40, 0x32, 0xA3,
    0xC4, 0x55, 0x27, 0xB6, 0xC3, 0x52, 0x20, 0xB1,
    0xCA, 0x5B, 0x29, 0xB8, 0xCD, 0x5C, 0x2E, 0xBF,
    0x90, 0x01, 0x73, 0xE2, 0x97, 0x06, 0x74, 0xE5,
    0x9E, 0x0F, 0x7D, 0xEC, 0x99, 0x08, 0x7A, 0xEB,
    0x8C, 0x1D, 0x6F, 0xFE, 0x8B, 0x1A, 0x68, 0xF9,
    0x82, 0x13, 0x61, 0xF0, 0x85, 0x14, 0x66, 0xF7,
    0xA8, 0x39, 0x4B, 0xDA, 0xAF, 0x3E, 0x4C, 0xDD,
    0xA6, 0x37, 0x45, 0xD4, 0xA1, 0x30, 0x42, 0xD3,
    0xB4, 0x25, 0x57, 0xC6, 0xB3, 0x22, 0x50, 0xC1,
    0xBA, 0x2B, 0x59, 0xC8, 0xBD, 0x2C, 0x5E, 0xCF,
];

/// Compute the RFCOMM frame-check-sequence over `buffer` (TS 07.10 Annex B).
pub fn compute_fcs(buffer: &[u8]) -> u8 {
    let mut result: u8 = 0xFF;
    for &byte in buffer {
        result = CRC_TABLE[(result ^ byte) as usize];
    }
    0xFF - result
}

// -----------------------------------------------------------------------------
// Frame and MCC types
// -----------------------------------------------------------------------------

/// RFCOMM frame type, encoded in the low bits of the control field.
///
/// The stored value is the control byte with the P/F bit cleared.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameType {
    /// Set Asynchronous Balanced Mode (connect a DLC).
    Sabm,
    /// Unnumbered Acknowledgement.
    Ua,
    /// Disconnected Mode.
    Dm,
    /// Disconnect.
    Disc,
    /// Unnumbered Information with Header check — carries data and MCC.
    Uih,
    /// Unnumbered Information.
    Ui,
}

impl FrameType {
    /// The frame type's control-field code (P/F bit cleared).
    pub fn value(self) -> u8 {
        match self {
            FrameType::Sabm => 0x2F,
            FrameType::Ua => 0x63,
            FrameType::Dm => 0x0F,
            FrameType::Disc => 0x43,
            FrameType::Uih => 0xEF,
            FrameType::Ui => 0x03,
        }
    }

    /// Decode a frame type from its control-field code (P/F bit already
    /// cleared by the caller). Unknown codes are rejected, mirroring upstream's
    /// `FrameType(...)` raising `ValueError`.
    fn from_code(code: u8) -> Result<Self> {
        Ok(match code {
            0x2F => FrameType::Sabm,
            0x63 => FrameType::Ua,
            0x0F => FrameType::Dm,
            0x43 => FrameType::Disc,
            0xEF => FrameType::Uih,
            0x03 => FrameType::Ui,
            other => {
                return Err(Error::InvalidPacket(format!(
                    "unknown RFCOMM frame type 0x{other:02X}"
                )))
            }
        })
    }
}

/// Multiplexer Control Channel message type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MccType {
    /// DLC Parameter Negotiation.
    Pn,
    /// Modem Status Command.
    Msc,
}

impl MccType {
    /// The MCC message type's code.
    pub fn value(self) -> u8 {
        match self {
            MccType::Pn => 0x20,
            MccType::Msc => 0x38,
        }
    }
}

// -----------------------------------------------------------------------------
// RFCOMM frame
// -----------------------------------------------------------------------------

/// An RFCOMM frame (TS 07.10 5.2.1).
///
/// The address, control, length and FCS bytes are derived from these fields on
/// [`to_bytes`](RfcommFrame::to_bytes) rather than stored, so a value compares
/// equal to itself after a round-trip. The one subtlety is [`with_credits`]:
/// upstream's `from_bytes` always reconstructs frames with it cleared, so it is
/// deliberately excluded from equality (see the manual [`PartialEq`] impl).
///
/// [`with_credits`]: RfcommFrame::with_credits
#[derive(Debug, Clone)]
pub struct RfcommFrame {
    /// The frame type.
    pub frame_type: FrameType,
    /// Command/response bit.
    pub c_r: bool,
    /// Data Link Connection Identifier (0 addresses the multiplexer itself).
    pub dlci: u8,
    /// Poll/Final bit. For UIH data frames this doubles as the credit flag.
    pub p_f: bool,
    /// The information field. For a credit-bearing UIH frame the first octet is
    /// the credit count and is not counted in the on-wire length.
    pub information: Vec<u8>,
    /// Whether the first information octet is a credit count excluded from the
    /// length field. Set for UIH frames sent with `p_f = true`.
    pub with_credits: bool,
}

/// Equality ignores [`with_credits`](RfcommFrame::with_credits): upstream's
/// `from_bytes` reconstructs every frame with it cleared, so including it would
/// make a credit-bearing frame unequal to its own parse.
impl PartialEq for RfcommFrame {
    fn eq(&self, other: &Self) -> bool {
        self.frame_type == other.frame_type
            && self.c_r == other.c_r
            && self.dlci == other.dlci
            && self.p_f == other.p_f
            && self.information == other.information
    }
}

impl Eq for RfcommFrame {}

impl RfcommFrame {
    /// A SABM command frame (P/F set, no information).
    pub fn sabm(c_r: bool, dlci: u8) -> Self {
        Self::control_frame(FrameType::Sabm, c_r, dlci)
    }

    /// A UA response frame (P/F set, no information).
    pub fn ua(c_r: bool, dlci: u8) -> Self {
        Self::control_frame(FrameType::Ua, c_r, dlci)
    }

    /// A DM response frame (P/F set, no information).
    pub fn dm(c_r: bool, dlci: u8) -> Self {
        Self::control_frame(FrameType::Dm, c_r, dlci)
    }

    /// A DISC command frame (P/F set, no information).
    pub fn disc(c_r: bool, dlci: u8) -> Self {
        Self::control_frame(FrameType::Disc, c_r, dlci)
    }

    fn control_frame(frame_type: FrameType, c_r: bool, dlci: u8) -> Self {
        RfcommFrame {
            frame_type,
            c_r,
            dlci,
            p_f: true,
            information: Vec::new(),
            with_credits: false,
        }
    }

    /// A UIH information frame. When `p_f` is set the first `information` octet
    /// is a credit count excluded from the on-wire length field.
    pub fn uih(c_r: bool, dlci: u8, information: impl Into<Vec<u8>>, p_f: bool) -> Self {
        RfcommFrame {
            frame_type: FrameType::Uih,
            c_r,
            dlci,
            p_f,
            information: information.into(),
            with_credits: p_f,
        }
    }

    /// The address octet: `[dlci(6) | c_r(1) | ea=1]`.
    pub fn address(&self) -> u8 {
        (self.dlci << 2) | ((self.c_r as u8) << 1) | 1
    }

    /// The control octet: frame type with the P/F bit merged in.
    pub fn control(&self) -> u8 {
        self.frame_type.value() | ((self.p_f as u8) << 4)
    }

    /// The 1- or 2-octet length indicator.
    fn length_field(&self) -> Result<Vec<u8>> {
        let mut length = self.information.len();
        if self.with_credits {
            // The leading credit octet is not part of the RFCOMM length.
            length = length.checked_sub(1).ok_or_else(|| {
                Error::InvalidArgument(
                    "credit-bearing frame needs an information octet for the credit count".into(),
                )
            })?;
        }
        Ok(if length > 0x7F {
            // 2-octet length, EA bit clear in the first octet.
            vec![((length & 0x7F) << 1) as u8, ((length >> 7) & 0xFF) as u8]
        } else {
            // 1-octet length, EA bit set.
            vec![((length << 1) | 1) as u8]
        })
    }

    /// The frame-check-sequence: over address+control for UIH, and additionally
    /// the length field for every other frame type (TS 07.10 5.1.1).
    pub fn fcs(&self) -> Result<u8> {
        let mut buffer = vec![self.address(), self.control()];
        if self.frame_type != FrameType::Uih {
            buffer.extend_from_slice(&self.length_field()?);
        }
        Ok(compute_fcs(&buffer))
    }

    /// Serialize the frame to its on-wire bytes.
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        let mut out = vec![self.address(), self.control()];
        out.extend_from_slice(&self.length_field()?);
        out.extend_from_slice(&self.information);
        out.push(self.fcs()?);
        Ok(out)
    }

    /// Parse a frame from bytes, validating the frame-check-sequence.
    ///
    /// Mirrors upstream: the information field is sliced positionally (from
    /// after the length indicator to before the FCS), the declared length value
    /// is not otherwise used, and the frame is reconstructed with the credit
    /// flag cleared.
    pub fn from_bytes(data: &[u8]) -> Result<Self> {
        // Minimum frame: address, control, 1-byte length, FCS.
        if data.len() < 4 {
            return Err(truncated("rfcomm frame"));
        }
        let dlci = (data[0] >> 2) & 0x3F;
        let c_r = (data[0] >> 1) & 0x01 != 0;
        let frame_type = FrameType::from_code(data[1] & 0xEF)?;
        let p_f = (data[1] >> 4) & 0x01 != 0;

        let length_byte = data[2];
        let information = if length_byte & 0x01 != 0 {
            // 1-octet length indicator.
            data[3..data.len() - 1].to_vec()
        } else {
            // 2-octet length indicator: the info field starts one octet later.
            if data.len() < 5 {
                return Err(truncated("rfcomm frame 2-byte length"));
            }
            data[4..data.len() - 1].to_vec()
        };
        let fcs = data[data.len() - 1];

        let frame = RfcommFrame {
            frame_type,
            c_r,
            dlci,
            p_f,
            information,
            with_credits: false,
        };
        let expected = frame.fcs()?;
        if expected != fcs {
            return Err(Error::InvalidPacket(format!(
                "fcs mismatch: got 0x{fcs:02X}, expected 0x{expected:02X}"
            )));
        }
        Ok(frame)
    }
}

// -----------------------------------------------------------------------------
// Multiplexer Control Channel framing
// -----------------------------------------------------------------------------

/// Wrap MCC `data` in a type/length header (`make_mcc` upstream). The length is
/// always encoded as a single EA-terminated octet, matching upstream, which is
/// valid because MCC payloads are far below the 128-octet 1-byte limit.
pub fn make_mcc(mcc_type: MccType, c_r: bool, data: &[u8]) -> Vec<u8> {
    let mut out = vec![
        (mcc_type.value() << 2) | ((c_r as u8) << 1) | 1,
        (((data.len() as u8) & 0x7F) << 1) | 1,
    ];
    out.extend_from_slice(data);
    out
}

/// Parse an MCC type/length header, returning `(type_code, command, value)`.
///
/// As upstream, the returned `value` is everything after the 2-octet header
/// (the 1-byte-length case does not slice to the declared length), and the
/// `type_code` is returned raw rather than mapped, so unknown MCC types are the
/// caller's concern.
pub fn parse_mcc(data: &[u8]) -> Result<(u8, bool, Vec<u8>)> {
    if data.len() < 2 {
        return Err(truncated("mcc header"));
    }
    let mcc_type = data[0] >> 2;
    let c_r = (data[0] >> 1) & 1 != 0;
    Ok((mcc_type, c_r, data[2..].to_vec()))
}

/// RFCOMM MCC DLC Parameter Negotiation message (TS 07.10 5.5.3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RfcommMccPn {
    /// DLCI the parameters apply to.
    pub dlci: u8,
    /// Convergence layer / frame type field.
    pub cl: u8,
    /// Priority.
    pub priority: u8,
    /// Acknowledgement timer (T1), in units defined by the spec.
    pub ack_timer: u8,
    /// Maximum frame size (little-endian on the wire).
    pub max_frame_size: u16,
    /// Maximum number of retransmissions (N2).
    pub max_retransmissions: u8,
    /// Initial credits (only the low 3 bits are meaningful).
    pub initial_credits: u8,
}

impl RfcommMccPn {
    /// Serialize to the 8-octet PN body.
    pub fn to_bytes(&self) -> [u8; 8] {
        [
            self.dlci,
            self.cl,
            self.priority,
            self.ack_timer,
            (self.max_frame_size & 0xFF) as u8,
            ((self.max_frame_size >> 8) & 0xFF) as u8,
            self.max_retransmissions,
            self.initial_credits & 0x07,
        ]
    }

    /// Parse the 8-octet PN body.
    pub fn from_bytes(data: &[u8]) -> Result<Self> {
        if data.len() < 8 {
            return Err(truncated("mcc pn"));
        }
        Ok(RfcommMccPn {
            dlci: data[0],
            cl: data[1],
            priority: data[2],
            ack_timer: data[3],
            max_frame_size: (data[4] as u16) | ((data[5] as u16) << 8),
            max_retransmissions: data[6],
            initial_credits: data[7] & 0x07,
        })
    }
}

/// RFCOMM MCC Modem Status Command message (TS 07.10 5.5.11).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RfcommMccMsc {
    /// DLCI the status applies to.
    pub dlci: u8,
    /// Flow control bit.
    pub fc: bool,
    /// Ready to communicate.
    pub rtc: bool,
    /// Ready to receive.
    pub rtr: bool,
    /// Incoming call indicator.
    pub ic: bool,
    /// Data valid.
    pub dv: bool,
}

impl RfcommMccMsc {
    /// Serialize to the 2-octet MSC body.
    pub fn to_bytes(&self) -> [u8; 2] {
        [
            (self.dlci << 2) | 3,
            1 | ((self.fc as u8) << 1)
                | ((self.rtc as u8) << 2)
                | ((self.rtr as u8) << 3)
                | ((self.ic as u8) << 6)
                | ((self.dv as u8) << 7),
        ]
    }

    /// Parse the 2-octet MSC body.
    pub fn from_bytes(data: &[u8]) -> Result<Self> {
        if data.len() < 2 {
            return Err(truncated("mcc msc"));
        }
        Ok(RfcommMccMsc {
            dlci: data[0] >> 2,
            fc: (data[1] >> 1) & 1 != 0,
            rtc: (data[1] >> 2) & 1 != 0,
            rtr: (data[1] >> 3) & 1 != 0,
            ic: (data[1] >> 6) & 1 != 0,
            dv: (data[1] >> 7) & 1 != 0,
        })
    }
}
