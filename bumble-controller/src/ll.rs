//! Link-Layer (LL) control PDUs exchanged between controllers over the
//! [`LocalLink`](crate::LocalLink) — a port of the subset of `bumble.ll` that
//! the software controller drives.
//!
//! As with [`AdvertisingPdu`](crate::AdvertisingPdu), the link is in-process, so
//! these are plain Rust structs rather than serialized LL PDUs; the *exchange
//! behavior* (who sends what in response to what) mirrors upstream `ll.py` /
//! `controller.py`, which is what these model.

/// An LL control PDU (`ll.ControlPdu` in upstream). Only the variants the
/// software controller exchanges are modelled.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ControlPdu {
    /// Start-encryption request (central → peripheral).
    EncReq {
        rand: [u8; 8],
        ediv: u16,
        ltk: [u8; 16],
    },
    /// Feature-exchange request from a central.
    FeatureReq { feature_set: [u8; 8] },
    /// Feature-exchange request from a peripheral.
    PeripheralFeatureReq { feature_set: [u8; 8] },
    /// Feature-exchange response.
    FeatureRsp { feature_set: [u8; 8] },
    /// Connected-isochronous-stream request (central → peripheral).
    CisReq { cig_id: u8, cis_id: u8 },
    /// Connected-isochronous-stream response (peripheral → central).
    CisRsp { cig_id: u8, cis_id: u8 },
    /// Connected-isochronous-stream indication (central → peripheral).
    CisInd { cig_id: u8, cis_id: u8 },
    /// Connection termination.
    TerminateInd { error_code: u8 },
}
