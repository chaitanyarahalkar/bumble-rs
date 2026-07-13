//! Classic (BR/EDR) Link Manager Protocol PDUs exchanged between controllers
//! over the [`LocalLink`](crate::LocalLink) — a simplified, in-process port of
//! the subset of `bumble.lmp` the software controller drives.
//!
//! As with the LE [`ll`](crate::ll) PDUs these are plain Rust structs, not
//! serialized LMP PDUs. They preserve the state transitions visible to the host,
//! including role switching during and after Classic connection establishment.

/// A classic LMP PDU (the subset modelled by the software controller).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ClassicPdu {
    /// Host-initiated connection request (`LmpHostConnectionReq`).
    HostConnectionReq,
    /// Connection accepted (`LmpAccepted` for `LMP_HOST_CONNECTION_REQ`).
    Accepted,
    /// Connection rejected (`LmpNotAccepted` for `LMP_HOST_CONNECTION_REQ`).
    Rejected { reason: u8 },
    /// Request that the two controllers exchange their Central/Peripheral roles.
    SwitchReq,
    /// Accept a pending role-switch request.
    SwitchAccepted,
    /// Reject a pending role-switch request.
    SwitchRejected { reason: u8 },
    /// Remote-name request (`LmpNameReq`).
    NameReq,
    /// Remote-name response (`LmpNameRes`); carries the 248-byte name field.
    NameRes { name: Vec<u8> },
    /// Features request (`LmpFeaturesReq`).
    FeaturesReq,
    /// Features response (`LmpFeaturesRes`).
    FeaturesRes { features: [u8; 8] },
    /// Enable or disable encryption on an established Classic ACL.
    EncryptionModeReq { enable: bool },
    /// Request an SCO/eSCO logical link over an established Classic ACL.
    SynchronousConnectionReq { link_type: u8, air_mode: u8 },
    /// Accept a pending SCO/eSCO logical link.
    SynchronousConnectionAccepted { link_type: u8, air_mode: u8 },
    /// Reject a pending SCO/eSCO logical link.
    SynchronousConnectionRejected { reason: u8 },
    /// Disconnect an established SCO/eSCO logical link without dropping ACL.
    SynchronousDetach { error_code: u8 },
    /// Detach / disconnect (`LmpDetach`).
    Detach { error_code: u8 },
}
