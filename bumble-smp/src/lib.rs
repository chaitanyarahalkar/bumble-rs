//! bumble-smp — the Security Manager Protocol layer of the
//! [`google/bumble`](https://github.com/google/bumble) port.
//!
//! **Slice 14** of the incremental port: the SMP PDU codec plus the LE Legacy
//! pairing confirm/key computation. This is the slice that wires the
//! (previously standalone) `bumble-crypto` toolbox into a real protocol
//! exchange — the last crate to join the composition.
//!
//! ## Scope
//!
//! - [`SmpPdu`]: the SMP signaling PDUs (`[code, payload…]` over L2CAP CID
//!   `0x0006`) — Pairing Request/Response/Confirm/Random/Failed, with a
//!   `Generic` fallback.
//! - [`legacy_confirm`] / [`legacy_stk`]: the LE Legacy pairing `c1`/`s1`
//!   computations, wrapping `bumble_crypto`.
//!
//! Deferred: the full pairing state machine, LE Secure Connections (public key
//! / DHKey exchange, `f4`/`f5`/`f6`/`g2` — the crypto is present), key
//! distribution, and bonding storage.

use bumble::Address;
use core::fmt;

/// The L2CAP channel id for the LE Security Manager Protocol.
pub const SMP_CID: u16 = 0x0006;

/// SMP command codes (Vol 3, Part H - 3.3).
pub mod codes {
    pub const SMP_PAIRING_REQUEST: u8 = 0x01;
    pub const SMP_PAIRING_RESPONSE: u8 = 0x02;
    pub const SMP_PAIRING_CONFIRM: u8 = 0x03;
    pub const SMP_PAIRING_RANDOM: u8 = 0x04;
    pub const SMP_PAIRING_FAILED: u8 = 0x05;
}

/// Errors produced while parsing SMP PDUs.
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

/// The pairing feature-exchange fields, shared by Pairing Request and Response.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PairingFeatures {
    pub io_capability: u8,
    pub oob_data_flag: u8,
    pub auth_req: u8,
    pub maximum_encryption_key_size: u8,
    pub initiator_key_distribution: u8,
    pub responder_key_distribution: u8,
}

impl PairingFeatures {
    fn to_payload(self) -> [u8; 6] {
        [
            self.io_capability,
            self.oob_data_flag,
            self.auth_req,
            self.maximum_encryption_key_size,
            self.initiator_key_distribution,
            self.responder_key_distribution,
        ]
    }

    fn from_payload(p: &[u8]) -> Result<PairingFeatures> {
        if p.len() < 6 {
            return Err(Error::InvalidPacket("truncated pairing features".into()));
        }
        Ok(PairingFeatures {
            io_capability: p[0],
            oob_data_flag: p[1],
            auth_req: p[2],
            maximum_encryption_key_size: p[3],
            initiator_key_distribution: p[4],
            responder_key_distribution: p[5],
        })
    }
}

/// An SMP protocol PDU.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SmpPdu {
    PairingRequest(PairingFeatures),
    PairingResponse(PairingFeatures),
    PairingConfirm { confirm_value: [u8; 16] },
    PairingRandom { random_value: [u8; 16] },
    PairingFailed { reason: u8 },
    Generic { code: u8, payload: Vec<u8> },
}

impl SmpPdu {
    pub fn code(&self) -> u8 {
        match self {
            SmpPdu::PairingRequest(_) => codes::SMP_PAIRING_REQUEST,
            SmpPdu::PairingResponse(_) => codes::SMP_PAIRING_RESPONSE,
            SmpPdu::PairingConfirm { .. } => codes::SMP_PAIRING_CONFIRM,
            SmpPdu::PairingRandom { .. } => codes::SMP_PAIRING_RANDOM,
            SmpPdu::PairingFailed { .. } => codes::SMP_PAIRING_FAILED,
            SmpPdu::Generic { code, .. } => *code,
        }
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = vec![self.code()];
        match self {
            SmpPdu::PairingRequest(f) | SmpPdu::PairingResponse(f) => {
                out.extend_from_slice(&f.to_payload())
            }
            SmpPdu::PairingConfirm { confirm_value } => out.extend_from_slice(confirm_value),
            SmpPdu::PairingRandom { random_value } => out.extend_from_slice(random_value),
            SmpPdu::PairingFailed { reason } => out.push(*reason),
            SmpPdu::Generic { payload, .. } => out.extend_from_slice(payload),
        }
        out
    }

    pub fn from_bytes(pdu: &[u8]) -> Result<SmpPdu> {
        let code = *pdu
            .first()
            .ok_or_else(|| Error::InvalidPacket("empty SMP PDU".into()))?;
        let payload = &pdu[1..];
        let array16 = |p: &[u8]| -> Result<[u8; 16]> {
            p.try_into()
                .map_err(|_| Error::InvalidPacket("expected 16-byte value".into()))
        };
        Ok(match code {
            codes::SMP_PAIRING_REQUEST => {
                SmpPdu::PairingRequest(PairingFeatures::from_payload(payload)?)
            }
            codes::SMP_PAIRING_RESPONSE => {
                SmpPdu::PairingResponse(PairingFeatures::from_payload(payload)?)
            }
            codes::SMP_PAIRING_CONFIRM => SmpPdu::PairingConfirm {
                confirm_value: array16(payload)?,
            },
            codes::SMP_PAIRING_RANDOM => SmpPdu::PairingRandom {
                random_value: array16(payload)?,
            },
            codes::SMP_PAIRING_FAILED => SmpPdu::PairingFailed {
                reason: *payload
                    .first()
                    .ok_or_else(|| Error::InvalidPacket("truncated Pairing Failed".into()))?,
            },
            _ => SmpPdu::Generic {
                code,
                payload: payload.to_vec(),
            },
        })
    }
}

/// LE Legacy pairing confirm value `Cx = c1(tk, rand, preq, pres, iat, rat, ia, ra)`
/// (Vol 3, Part H - 2.3.5.5), wrapping [`bumble_crypto::c1`].
///
/// `preq`/`pres` are the serialized Pairing Request/Response PDUs; `ia`/`ra`
/// are the initiating/responding device addresses.
#[allow(clippy::too_many_arguments)]
pub fn legacy_confirm(
    tk: &[u8],
    rand: &[u8],
    preq: &[u8],
    pres: &[u8],
    ia: &Address,
    iat: u8,
    ra: &Address,
    rat: u8,
) -> [u8; 16] {
    let confirm = bumble_crypto::c1(
        tk,
        rand,
        preq,
        pres,
        iat,
        rat,
        ia.address_bytes(),
        ra.address_bytes(),
    );
    let mut out = [0u8; 16];
    out.copy_from_slice(&confirm);
    out
}

/// LE Legacy pairing Short Term Key `STK = s1(tk, srand, mrand)`
/// (Vol 3, Part H - 2.3.5.5), wrapping [`bumble_crypto::s1`].
pub fn legacy_stk(tk: &[u8], srand: &[u8], mrand: &[u8]) -> [u8; 16] {
    let stk = bumble_crypto::s1(tk, srand, mrand);
    let mut out = [0u8; 16];
    out.copy_from_slice(&stk);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn features() -> PairingFeatures {
        PairingFeatures {
            io_capability: 0x03,
            oob_data_flag: 0,
            auth_req: 0x01,
            maximum_encryption_key_size: 16,
            initiator_key_distribution: 0x07,
            responder_key_distribution: 0x07,
        }
    }

    fn check(pdu: SmpPdu, expected_hex: &str) {
        let bytes = pdu.to_bytes();
        let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(hex, expected_hex, "serialization vs Python oracle");
        assert_eq!(SmpPdu::from_bytes(&bytes).unwrap(), pdu, "round-trip");
    }

    #[test]
    fn smp_pdu_codec() {
        check(SmpPdu::PairingRequest(features()), "01030001100707");
        check(SmpPdu::PairingResponse(features()), "02030001100707");
        check(
            SmpPdu::PairingConfirm {
                confirm_value: unhex16("1e1e3fef878988ead2a74dc5bef13b86"),
            },
            "031e1e3fef878988ead2a74dc5bef13b86",
        );
        check(
            SmpPdu::PairingRandom {
                random_value: unhex16("5783d52156ad6f0e6388274ec6702ee0"),
            },
            "045783d52156ad6f0e6388274ec6702ee0",
        );
        check(SmpPdu::PairingFailed { reason: 0x03 }, "0503");
    }

    fn unhex16(s: &str) -> [u8; 16] {
        let v: Vec<u8> = (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
            .collect();
        v.try_into().unwrap()
    }
    fn rhex16(s: &str) -> [u8; 16] {
        let mut a = unhex16(s);
        a.reverse();
        a
    }

    /// The confirm computation matches the published Bluetooth-spec `c1` vector
    /// (see bumble-crypto). (A genuine two-party pairing handshake, where the
    /// matching STK is derived by independent peers, lives in the bumble-host
    /// integration test.)
    #[test]
    fn legacy_confirm_matches_spec_vector() {
        let tk = [0u8; 16];
        let mrand = rhex16("5783D52156AD6F0E6388274EC6702EE0");
        // preq/pres are the reversed 7-byte values from the c1 vector.
        let preq = reversed(&unhex_n("07071000000101"));
        let pres = reversed(&unhex_n("05000800000302"));
        let ia =
            bumble::Address::from_bytes(rhex6("A1A2A3A4A5A6"), bumble::AddressType::PUBLIC_DEVICE);
        let ra =
            bumble::Address::from_bytes(rhex6("B1B2B3B4B5B6"), bumble::AddressType::PUBLIC_DEVICE);

        let mconfirm = legacy_confirm(&tk, &mrand, &preq, &pres, &ia, 1, &ra, 0);
        assert_eq!(mconfirm, rhex16("1e1e3fef878988ead2a74dc5bef13b86"));
    }

    fn unhex_n(s: &str) -> Vec<u8> {
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
            .collect()
    }
    fn reversed(v: &[u8]) -> Vec<u8> {
        v.iter().rev().copied().collect()
    }
    fn rhex6(s: &str) -> [u8; 6] {
        let mut v = unhex_n(s);
        v.reverse();
        v.try_into().unwrap()
    }
}
