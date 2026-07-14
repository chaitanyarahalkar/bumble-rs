//! Link-Layer (LL) control PDUs exchanged between controllers over the
//! [`LocalLink`](crate::LocalLink) — a port of the subset of `bumble.ll` that
//! the software controller drives.
//!
//! As with [`AdvertisingPdu`](crate::AdvertisingPdu), the link is in-process, so
//! these are plain Rust structs rather than serialized LL PDUs; the *exchange
//! behavior* (who sends what in response to what) mirrors upstream `ll.py` /
//! `controller.py`, which is what these model.

/// Complete LL control-opcode catalog from upstream `ControlPdu.Opcode`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum ControlOpcode {
    ConnectionUpdateInd = 0x00,
    ChannelMapInd = 0x01,
    TerminateInd = 0x02,
    EncReq = 0x03,
    EncRsp = 0x04,
    StartEncReq = 0x05,
    StartEncRsp = 0x06,
    UnknownRsp = 0x07,
    FeatureReq = 0x08,
    FeatureRsp = 0x09,
    PauseEncReq = 0x0A,
    PauseEncRsp = 0x0B,
    VersionInd = 0x0C,
    RejectInd = 0x0D,
    PeripheralFeatureReq = 0x0E,
    ConnectionParamReq = 0x0F,
    ConnectionParamRsp = 0x10,
    RejectExtInd = 0x11,
    PingReq = 0x12,
    PingRsp = 0x13,
    LengthReq = 0x14,
    LengthRsp = 0x15,
    PhyReq = 0x16,
    PhyRsp = 0x17,
    PhyUpdateInd = 0x18,
    MinUsedChannelsInd = 0x19,
    CteReq = 0x1A,
    CteRsp = 0x1B,
    PeriodicSyncInd = 0x1C,
    ClockAccuracyReq = 0x1D,
    ClockAccuracyRsp = 0x1E,
    CisReq = 0x1F,
    CisRsp = 0x20,
    CisInd = 0x21,
    CisTerminateInd = 0x22,
    PowerControlReq = 0x23,
    PowerControlRsp = 0x24,
    PowerChangeInd = 0x25,
    SubrateReq = 0x26,
    SubrateInd = 0x27,
    ChannelReportingInd = 0x28,
    ChannelStatusInd = 0x29,
    PeriodicSyncWrInd = 0x2A,
    FeatureExtReq = 0x2B,
    FeatureExtRsp = 0x2C,
    CsSecRsp = 0x2D,
    CsCapabilitiesReq = 0x2E,
    CsCapabilitiesRsp = 0x2F,
    CsConfigReq = 0x30,
    CsConfigRsp = 0x31,
    CsReq = 0x32,
    CsRsp = 0x33,
    CsInd = 0x34,
    CsTerminateReq = 0x35,
    CsFaeReq = 0x36,
    CsFaeRsp = 0x37,
    CsChannelMapInd = 0x38,
    CsSecReq = 0x39,
    CsTerminateRsp = 0x3A,
    FrameSpaceReq = 0x3B,
    FrameSpaceRsp = 0x3C,
}

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
    /// Extended rejection of a connected-isochronous-stream request.
    CisReject {
        cig_id: u8,
        cis_id: u8,
        error_code: u8,
    },
    /// Connected-isochronous-stream termination indication.
    CisTerminateInd {
        cig_id: u8,
        cis_id: u8,
        error_code: u8,
    },
    /// Connection termination.
    TerminateInd { error_code: u8 },
}

impl ControlPdu {
    /// Opcode associated with this modeled control PDU.
    pub const fn opcode(&self) -> ControlOpcode {
        match self {
            Self::EncReq { .. } => ControlOpcode::EncReq,
            Self::FeatureReq { .. } => ControlOpcode::FeatureReq,
            Self::PeripheralFeatureReq { .. } => ControlOpcode::PeripheralFeatureReq,
            Self::FeatureRsp { .. } => ControlOpcode::FeatureRsp,
            Self::CisReq { .. } => ControlOpcode::CisReq,
            Self::CisRsp { .. } => ControlOpcode::CisRsp,
            Self::CisInd { .. } => ControlOpcode::CisInd,
            Self::CisReject { .. } => ControlOpcode::RejectExtInd,
            Self::CisTerminateInd { .. } => ControlOpcode::CisTerminateInd,
            Self::TerminateInd { .. } => ControlOpcode::TerminateInd,
        }
    }
}
