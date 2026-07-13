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
//!   `0x0006`) — Pairing Request/Response/Confirm/Random/Failed, the LE Secure
//!   Connections PDUs (Security Request, Pairing Public Key, Pairing DHKey
//!   Check, Keypress Notification) and the key-distribution PDUs (Encryption
//!   Information, Master Identification, Identity Information, Identity Address
//!   Information, Signing Information), with a `Generic` fallback.
//! - [`legacy_confirm`] / [`legacy_stk`]: the LE Legacy pairing `c1`/`s1`
//!   computations, wrapping `bumble_crypto`.
//! - [`sc`]: the **LE Secure Connections** JustWorks derivation (slice 19) —
//!   composing `bumble_crypto`'s ECDH (`EccKey`) and `f4`/`f5`/`f6`/`g2` into
//!   the confirm value, MacKey/LTK, DHKey checks (`Ea`/`Eb`), and numeric
//!   comparison value, following upstream's argument construction.
//!
//! [`pairing`] adds the full I/O capability decision matrix, authentication and
//! key-distribution policy, SC/Legacy OOB contexts and AD interchange, pairing
//! configuration, and CTKD key derivation. Deferred: the live pairing state
//! machine, user-delegate actions, key exchange, and bonding storage.

use bumble::Address;
use core::fmt;

pub mod pairing;

pub use pairing::{
    derive_link_key, derive_ltk, select_pairing_method, select_pairing_method_with_oob, AuthReq,
    IdentityAddressType, IoCapability, KeyDistribution, OobConfig, OobContext, OobData,
    OobLegacyContext, OobSharedData, PairingCapabilities, PairingConfig, PairingMethod,
    PairingMethodSelection,
};

/// The L2CAP channel id for the LE Security Manager Protocol.
pub const SMP_CID: u16 = 0x0006;

/// SMP command codes (Vol 3, Part H - 3.3).
pub mod codes {
    pub const SMP_PAIRING_REQUEST: u8 = 0x01;
    pub const SMP_PAIRING_RESPONSE: u8 = 0x02;
    pub const SMP_PAIRING_CONFIRM: u8 = 0x03;
    pub const SMP_PAIRING_RANDOM: u8 = 0x04;
    pub const SMP_PAIRING_FAILED: u8 = 0x05;
    pub const SMP_ENCRYPTION_INFORMATION: u8 = 0x06;
    pub const SMP_MASTER_IDENTIFICATION: u8 = 0x07;
    pub const SMP_IDENTITY_INFORMATION: u8 = 0x08;
    pub const SMP_IDENTITY_ADDRESS_INFORMATION: u8 = 0x09;
    pub const SMP_SIGNING_INFORMATION: u8 = 0x0A;
    pub const SMP_SECURITY_REQUEST: u8 = 0x0B;
    pub const SMP_PAIRING_PUBLIC_KEY: u8 = 0x0C;
    pub const SMP_PAIRING_DHKEY_CHECK: u8 = 0x0D;
    pub const SMP_PAIRING_KEYPRESS_NOTIFICATION: u8 = 0x0E;
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
    PairingConfirm {
        confirm_value: [u8; 16],
    },
    PairingRandom {
        random_value: [u8; 16],
    },
    PairingFailed {
        reason: u8,
    },
    /// LE Secure Connections public key: X and Y coordinates, little-endian on
    /// the wire (Vol 3, Part H - 3.5.6).
    PairingPublicKey {
        public_key_x: [u8; 32],
        public_key_y: [u8; 32],
    },
    PairingDhKeyCheck {
        dhkey_check: [u8; 16],
    },
    KeypressNotification {
        notification_type: u8,
    },
    EncryptionInformation {
        long_term_key: [u8; 16],
    },
    MasterIdentification {
        ediv: u16,
        rand: [u8; 8],
    },
    IdentityInformation {
        identity_resolving_key: [u8; 16],
    },
    IdentityAddressInformation {
        addr_type: u8,
        bd_addr: [u8; 6],
    },
    SigningInformation {
        signature_key: [u8; 16],
    },
    SecurityRequest {
        auth_req: u8,
    },
    Generic {
        code: u8,
        payload: Vec<u8>,
    },
}

impl SmpPdu {
    pub fn code(&self) -> u8 {
        match self {
            SmpPdu::PairingRequest(_) => codes::SMP_PAIRING_REQUEST,
            SmpPdu::PairingResponse(_) => codes::SMP_PAIRING_RESPONSE,
            SmpPdu::PairingConfirm { .. } => codes::SMP_PAIRING_CONFIRM,
            SmpPdu::PairingRandom { .. } => codes::SMP_PAIRING_RANDOM,
            SmpPdu::PairingFailed { .. } => codes::SMP_PAIRING_FAILED,
            SmpPdu::PairingPublicKey { .. } => codes::SMP_PAIRING_PUBLIC_KEY,
            SmpPdu::PairingDhKeyCheck { .. } => codes::SMP_PAIRING_DHKEY_CHECK,
            SmpPdu::KeypressNotification { .. } => codes::SMP_PAIRING_KEYPRESS_NOTIFICATION,
            SmpPdu::EncryptionInformation { .. } => codes::SMP_ENCRYPTION_INFORMATION,
            SmpPdu::MasterIdentification { .. } => codes::SMP_MASTER_IDENTIFICATION,
            SmpPdu::IdentityInformation { .. } => codes::SMP_IDENTITY_INFORMATION,
            SmpPdu::IdentityAddressInformation { .. } => codes::SMP_IDENTITY_ADDRESS_INFORMATION,
            SmpPdu::SigningInformation { .. } => codes::SMP_SIGNING_INFORMATION,
            SmpPdu::SecurityRequest { .. } => codes::SMP_SECURITY_REQUEST,
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
            SmpPdu::PairingPublicKey {
                public_key_x,
                public_key_y,
            } => {
                out.extend_from_slice(public_key_x);
                out.extend_from_slice(public_key_y);
            }
            SmpPdu::PairingDhKeyCheck { dhkey_check } => out.extend_from_slice(dhkey_check),
            SmpPdu::KeypressNotification { notification_type } => out.push(*notification_type),
            SmpPdu::EncryptionInformation { long_term_key } => out.extend_from_slice(long_term_key),
            SmpPdu::MasterIdentification { ediv, rand } => {
                out.extend_from_slice(&ediv.to_le_bytes());
                out.extend_from_slice(rand);
            }
            SmpPdu::IdentityInformation {
                identity_resolving_key,
            } => out.extend_from_slice(identity_resolving_key),
            SmpPdu::IdentityAddressInformation { addr_type, bd_addr } => {
                out.push(*addr_type);
                out.extend_from_slice(bd_addr);
            }
            SmpPdu::SigningInformation { signature_key } => out.extend_from_slice(signature_key),
            SmpPdu::SecurityRequest { auth_req } => out.push(*auth_req),
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
        // Take exactly `N` bytes from the front of `p`, erroring if short.
        fn take<const N: usize>(p: &[u8], what: &str) -> Result<[u8; N]> {
            p.get(..N)
                .and_then(|s| s.try_into().ok())
                .ok_or_else(|| Error::InvalidPacket(format!("truncated {what}")))
        }
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
            codes::SMP_PAIRING_PUBLIC_KEY => SmpPdu::PairingPublicKey {
                public_key_x: take::<32>(payload, "Pairing Public Key X")?,
                public_key_y: take::<32>(payload.get(32..).unwrap_or(&[]), "Pairing Public Key Y")?,
            },
            codes::SMP_PAIRING_DHKEY_CHECK => SmpPdu::PairingDhKeyCheck {
                dhkey_check: array16(payload)?,
            },
            codes::SMP_PAIRING_KEYPRESS_NOTIFICATION => SmpPdu::KeypressNotification {
                notification_type: *payload.first().ok_or_else(|| {
                    Error::InvalidPacket("truncated Keypress Notification".into())
                })?,
            },
            codes::SMP_ENCRYPTION_INFORMATION => SmpPdu::EncryptionInformation {
                long_term_key: array16(payload)?,
            },
            codes::SMP_MASTER_IDENTIFICATION => SmpPdu::MasterIdentification {
                ediv: u16::from_le_bytes(take::<2>(payload, "Master Identification EDIV")?),
                rand: take::<8>(
                    payload.get(2..).unwrap_or(&[]),
                    "Master Identification Rand",
                )?,
            },
            codes::SMP_IDENTITY_INFORMATION => SmpPdu::IdentityInformation {
                identity_resolving_key: array16(payload)?,
            },
            codes::SMP_IDENTITY_ADDRESS_INFORMATION => SmpPdu::IdentityAddressInformation {
                addr_type: *payload.first().ok_or_else(|| {
                    Error::InvalidPacket("truncated Identity Address Information".into())
                })?,
                bd_addr: take::<6>(
                    payload.get(1..).unwrap_or(&[]),
                    "Identity Address Information address",
                )?,
            },
            codes::SMP_SIGNING_INFORMATION => SmpPdu::SigningInformation {
                signature_key: array16(payload)?,
            },
            codes::SMP_SECURITY_REQUEST => SmpPdu::SecurityRequest {
                auth_req: *payload
                    .first()
                    .ok_or_else(|| Error::InvalidPacket("truncated Security Request".into()))?,
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

/// LE Secure Connections pairing derivation.
///
/// This composes `bumble_crypto`'s ECDH ([`bumble_crypto::EccKey`]) and
/// `f4`/`f5`/`f6`/`g2` into the JustWorks / Numeric Comparison key agreement,
/// following upstream `smp.py`'s exact argument construction. Byte order
/// matches upstream: public-key X coordinates and the DH key are **little
/// endian** here (SMP byte-swaps the big-endian [`bumble_crypto::EccKey`]
/// output before feeding these functions).
///
/// The passkey and OOB variants (non-zero `ra`/`rb`) and the async state
/// machine are deferred.
pub mod sc {
    use bumble_crypto::{f4, f5, f6, g2};

    /// The keys and checks produced at the end of an LE Secure Connections
    /// JustWorks / Numeric Comparison exchange.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct ScKeys {
        /// The MacKey used to compute the DHKey checks.
        pub mac_key: [u8; 16],
        /// The Long Term Key both peers derive.
        pub ltk: [u8; 16],
        /// The initiator's DHKey check value (`Ea`).
        pub ea: [u8; 16],
        /// The responder's DHKey check value (`Eb`).
        pub eb: [u8; 16],
        /// The 6-digit numeric comparison value shown to the user.
        pub numeric_check: u32,
    }

    fn to16(v: &[u8]) -> [u8; 16] {
        let mut out = [0u8; 16];
        out.copy_from_slice(v);
        out
    }

    /// The 3-byte `IOcap` field (`io_capability, oob_data_flag, auth_req`) as it
    /// appears at offset 1 of a serialized Pairing Request/Response PDU — the
    /// slice upstream passes to `f6` as `preq[1:4]` / `pres[1:4]`.
    pub fn io_cap(pairing_pdu: &[u8]) -> Option<[u8; 3]> {
        pairing_pdu.get(1..4).and_then(|s| s.try_into().ok())
    }

    /// The LE Secure Connections confirm value
    /// `f4(own_pk_x, peer_pk_x, own_nonce, 0)` — the responder's `Cb` in the
    /// JustWorks / Numeric Comparison flows (Vol 3, Part H - 2.3.5.6.2).
    pub fn confirm_value(
        own_pk_x_le: &[u8; 32],
        peer_pk_x_le: &[u8; 32],
        own_nonce: &[u8; 16],
    ) -> [u8; 16] {
        to16(&f4(own_pk_x_le, peer_pk_x_le, own_nonce, 0))
    }

    /// Derive the JustWorks / Numeric Comparison keys and DHKey checks
    /// (Vol 3, Part H - 2.3.5.6.5), composing `f5`/`f6`/`g2` exactly as upstream
    /// does. `dh_key_le` is the shared secret from
    /// [`bumble_crypto::EccKey::dh`], byte-reversed to little-endian.
    #[allow(clippy::too_many_arguments)]
    pub fn just_works_keys(
        dh_key_le: &[u8; 32],
        na: &[u8; 16],
        nb: &[u8; 16],
        ia: &[u8; 6],
        iat: u8,
        ra: &[u8; 6],
        rat: u8,
        io_cap_a: &[u8; 3],
        io_cap_b: &[u8; 3],
        pka_x_le: &[u8; 32],
        pkb_x_le: &[u8; 32],
    ) -> ScKeys {
        let mut a = [0u8; 7];
        a[..6].copy_from_slice(ia);
        a[6] = iat;
        let mut b = [0u8; 7];
        b[..6].copy_from_slice(ra);
        b[6] = rat;

        let (mac_key, ltk) = f5(dh_key_le, na, nb, &a, &b);
        // JustWorks / Numeric Comparison use all-zero r values in the checks.
        let r0 = [0u8; 16];
        let ea = f6(&mac_key, na, nb, &r0, io_cap_a, &a, &b);
        let eb = f6(&mac_key, nb, na, &r0, io_cap_b, &b, &a);
        let numeric_check = g2(pka_x_le, pkb_x_le, na, nb) % 1_000_000;

        ScKeys {
            mac_key: to16(&mac_key),
            ltk: to16(&ltk),
            ea: to16(&ea),
            eb: to16(&eb),
            numeric_check,
        }
    }
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

    /// Sequential bytes `start, start+1, …` — matches the oracle capture's
    /// range-filled fields.
    fn seq<const N: usize>(start: u8) -> [u8; N] {
        let mut a = [0u8; N];
        for (i, b) in a.iter_mut().enumerate() {
            *b = start.wrapping_add(i as u8);
        }
        a
    }

    #[test]
    fn smp_sc_pdu_codec() {
        check(SmpPdu::SecurityRequest { auth_req: 0x0d }, "0b0d");
        check(
            SmpPdu::PairingPublicKey {
                public_key_x: seq::<32>(0x01),
                public_key_y: seq::<32>(0x21),
            },
            "0c0102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f\
             202122232425262728292a2b2c2d2e2f303132333435363738393a3b3c3d3e3f40",
        );
        check(
            SmpPdu::PairingDhKeyCheck {
                dhkey_check: seq::<16>(0x10),
            },
            "0d101112131415161718191a1b1c1d1e1f",
        );
        check(
            SmpPdu::KeypressNotification {
                notification_type: 0x02,
            },
            "0e02",
        );
        check(
            SmpPdu::EncryptionInformation {
                long_term_key: seq::<16>(0x20),
            },
            "06202122232425262728292a2b2c2d2e2f",
        );
        check(
            SmpPdu::MasterIdentification {
                ediv: 0x1234,
                rand: seq::<8>(0x00),
            },
            "0734120001020304050607",
        );
        check(
            SmpPdu::IdentityInformation {
                identity_resolving_key: seq::<16>(0x30),
            },
            "08303132333435363738393a3b3c3d3e3f",
        );
        check(
            SmpPdu::IdentityAddressInformation {
                addr_type: 0x01,
                bd_addr: [0xff, 0xee, 0xdd, 0xcc, 0xbb, 0xaa],
            },
            "0901ffeeddccbbaa",
        );
        check(
            SmpPdu::SigningInformation {
                signature_key: seq::<16>(0x40),
            },
            "0a404142434445464748494a4b4c4d4e4f",
        );
    }

    /// The LE Secure Connections JustWorks derivation, pinned end-to-end to
    /// values captured from upstream Python Bumble (`crypto.EccKey` + `f4`/`f5`/
    /// `f6`/`g2`) for fixed private keys, nonces, and addresses.
    #[test]
    fn sc_just_works_matches_oracle() {
        use bumble_crypto::EccKey;

        let ka = EccKey::from_private_key_bytes(&(1u8..=32).collect::<Vec<u8>>()).unwrap();
        let kb = EccKey::from_private_key_bytes(&(33u8..=64).collect::<Vec<u8>>()).unwrap();

        // Public-key X coordinates as f4/f5/f6/g2 receive them: little-endian.
        let mut pka_x = ka.public_x();
        pka_x.reverse();
        let mut pkb_x = kb.public_x();
        pkb_x.reverse();

        // Shared secret, little-endian (upstream stores `ecc_key.dh(...)[::-1]`).
        let mut dh_key = ka.dh(&kb.public_x(), &kb.public_y()).unwrap();
        dh_key.reverse();

        let na: [u8; 16] = seq::<16>(0xA0);
        let nb: [u8; 16] = seq::<16>(0xB0);
        let ia = [0x06, 0x05, 0x04, 0x03, 0x02, 0x01];
        let ra = [0x0c, 0x0b, 0x0a, 0x09, 0x08, 0x07];

        // Responder confirm Cb = f4(PKb, PKa, Nb, 0).
        assert_eq!(
            hex16(&sc::confirm_value(&pkb_x, &pka_x, &nb)),
            "d0decd97e767a76a879285ab5d72637f"
        );

        let preq = SmpPdu::PairingRequest(PairingFeatures {
            io_capability: 0x03,
            oob_data_flag: 0,
            auth_req: 0x0d,
            maximum_encryption_key_size: 16,
            initiator_key_distribution: 0x07,
            responder_key_distribution: 0x07,
        })
        .to_bytes();
        let pres = SmpPdu::PairingResponse(PairingFeatures {
            io_capability: 0x04,
            oob_data_flag: 0,
            auth_req: 0x0d,
            maximum_encryption_key_size: 16,
            initiator_key_distribution: 0x07,
            responder_key_distribution: 0x07,
        })
        .to_bytes();
        let io_cap_a = sc::io_cap(&preq).unwrap();
        let io_cap_b = sc::io_cap(&pres).unwrap();

        let keys = sc::just_works_keys(
            &dh_key, &na, &nb, &ia, 1, &ra, 1, &io_cap_a, &io_cap_b, &pka_x, &pkb_x,
        );
        assert_eq!(hex16(&keys.mac_key), "85ab8a333649442a7888ebc8023d5470");
        assert_eq!(hex16(&keys.ltk), "2e4ef0edc5dbe52252849a6c574c7f03");
        assert_eq!(hex16(&keys.ea), "4c46feac6c7c62fe30f683e2d501c8cb");
        assert_eq!(hex16(&keys.eb), "fc0cebf2719aa3786a827e8ecf83ff74");
        assert_eq!(keys.numeric_check, 753306);
    }

    fn hex16(b: &[u8; 16]) -> String {
        b.iter().map(|x| format!("{x:02x}")).collect()
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
