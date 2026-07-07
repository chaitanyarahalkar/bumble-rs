//! Classic (BR/EDR) Link Manager Protocol PDUs exchanged between controllers
//! over the [`LocalLink`](crate::LocalLink) — a simplified, in-process port of
//! the subset of `bumble.lmp` the software controller drives.
//!
//! As with the LE [`ll`](crate::ll) PDUs these are plain Rust structs, not
//! serialized LMP PDUs. The classic connection handshake is simplified relative
//! to upstream (no role-switch / authentication sub-dance): a host-connection
//! request is answered with an acceptance, which is enough to reproduce the HCI
//! event sequence a host observes (`Connection Request` → `Connection Complete`).

/// A classic LMP PDU (the subset modelled by the software controller).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ClassicPdu {
    /// Host-initiated connection request (`LmpHostConnectionReq`).
    HostConnectionReq,
    /// Connection accepted (`LmpAccepted` for `LMP_HOST_CONNECTION_REQ`).
    Accepted,
    /// Remote-name request (`LmpNameReq`).
    NameReq,
    /// Remote-name response (`LmpNameRes`); carries the 248-byte name field.
    NameRes { name: Vec<u8> },
    /// Features request (`LmpFeaturesReq`).
    FeaturesReq,
    /// Features response (`LmpFeaturesRes`).
    FeaturesRes { features: [u8; 8] },
    /// Detach / disconnect (`LmpDetach`).
    Detach { error_code: u8 },
}
