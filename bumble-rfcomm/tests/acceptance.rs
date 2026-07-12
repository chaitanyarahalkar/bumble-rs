//! Oracle-pinned acceptance tests for the RFCOMM frame + MCC codec.
//!
//! Every hex literal was captured from upstream Python Bumble (`bytes(x).hex()`)
//! at commit `1d26b99865f96a3e7359009424c0ddf2934acd0b`, mirroring the frame
//! round-trip that upstream `tests/rfcomm_test.py::basic_frame_check` performs
//! (`bytes(from_bytes(serialized)) == serialized`) and adding the MCC message
//! bodies, the length-indicator boundaries, and the parse-error cases a codec
//! needs.

use bumble_rfcomm::{
    compute_fcs, make_mcc, parse_mcc, FrameType, MccType, RfcommFrame, RfcommMccMsc, RfcommMccPn,
};

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Assert a (non-credit) frame serializes to `oracle` and round-trips
/// byte-for-byte — the check upstream's `basic_frame_check` runs.
fn check_frame(frame: RfcommFrame, oracle: &str) {
    let bytes = frame.to_bytes().expect("serialize");
    assert_eq!(hex(&bytes), oracle, "serialization mismatch for {frame:?}");
    let parsed = RfcommFrame::from_bytes(&bytes).expect("parse");
    assert_eq!(parsed, frame, "round-trip mismatch for {frame:?}");
    assert_eq!(
        hex(&parsed.to_bytes().expect("re-serialize")),
        oracle,
        "re-serialize mismatch for {frame:?}"
    );
}

// --- FCS ---------------------------------------------------------------------

/// Pin `compute_fcs` directly so a single transcription error in the 256-byte
/// table fails loudly here rather than as a distant frame-check failure.
#[test]
fn fcs_canary() {
    assert_eq!(compute_fcs(b""), 0x00);
    assert_eq!(compute_fcs(&[0xFF]), 0xFF);
    assert_eq!(compute_fcs(&[0x03, 0x3F, 0x01]), 0x1C);
}

// --- Control frames ----------------------------------------------------------

#[test]
fn frame_sabm() {
    check_frame(RfcommFrame::sabm(true, 0), "033f011c");
    // Non-zero DLCI exercises the address computation.
    check_frame(RfcommFrame::sabm(false, 62), "f93f01da");
}

#[test]
fn frame_ua() {
    check_frame(RfcommFrame::ua(true, 0), "037301d7");
}

#[test]
fn frame_dm() {
    check_frame(RfcommFrame::dm(true, 2), "0b1f0173");
}

#[test]
fn frame_disc() {
    check_frame(RfcommFrame::disc(true, 2), "0b5301b8");
}

// --- UIH information frames --------------------------------------------------

#[test]
fn frame_uih_no_credits() {
    // FCS covers only address+control for UIH, so it matches the empty-info case.
    check_frame(
        RfcommFrame::uih(true, 0, *b"hello", false),
        "03ef0b68656c6c6f70",
    );
}

/// A credit-bearing UIH frame: the leading octet (0x07) is a credit count
/// excluded from the length field, so the length is 5 (`0x0b`) not 6.
///
/// Upstream `from_bytes` reconstructs without the credit flag, so this frame is
/// byte-lossy through a parse round-trip in upstream too — we pin serialization
/// and field recovery instead of re-serialization.
#[test]
fn frame_uih_with_credits() {
    let mut info = vec![0x07];
    info.extend_from_slice(b"hello");
    let frame = RfcommFrame::uih(false, 2, info, true);
    assert_eq!(hex(&frame.to_bytes().unwrap()), "09ff0b0768656c6c6f5c");

    let parsed = RfcommFrame::from_bytes(&frame.to_bytes().unwrap()).unwrap();
    assert_eq!(parsed.frame_type, FrameType::Uih);
    assert!(parsed.p_f);
    assert!(!parsed.c_r);
    assert_eq!(parsed.dlci, 2);
    // The credit octet is recovered as part of the information field.
    assert_eq!(parsed.information, vec![0x07, 0x68, 0x65, 0x6c, 0x6c, 0x6f]);
}

/// 127 information octets still fit a 1-byte length indicator (`0xff`); 128
/// tips over into a 2-byte indicator (`0x0001`); 200 gives `0x9001`.
#[test]
fn frame_uih_length_indicator_boundary() {
    let f127 = RfcommFrame::uih(true, 0, vec![b'C'; 127], false);
    let b127 = f127.to_bytes().unwrap();
    assert_eq!(&b127[..3], &[0x03, 0xef, 0xff]);
    assert_eq!(RfcommFrame::from_bytes(&b127).unwrap(), f127);

    let f128 = RfcommFrame::uih(true, 0, vec![b'B'; 128], false);
    let b128 = f128.to_bytes().unwrap();
    assert_eq!(&b128[..4], &[0x03, 0xef, 0x00, 0x01]);
    assert_eq!(RfcommFrame::from_bytes(&b128).unwrap(), f128);

    let f200 = RfcommFrame::uih(true, 0, vec![b'A'; 200], false);
    let mut expected = vec![0x03, 0xef, 0x90, 0x01];
    expected.extend_from_slice(&[b'A'; 200]);
    expected.push(0x70);
    assert_eq!(f200.to_bytes().unwrap(), expected);
    assert_eq!(RfcommFrame::from_bytes(&expected).unwrap(), f200);
}

// --- MCC messages ------------------------------------------------------------

#[test]
fn mcc_pn_roundtrip() {
    let pn = RfcommMccPn {
        dlci: 4,
        cl: 0xF0,
        priority: 7,
        ack_timer: 0,
        max_frame_size: 1000,
        max_retransmissions: 0,
        initial_credits: 7,
    };
    assert_eq!(hex(&pn.to_bytes()), "04f00700e8030007");
    assert_eq!(RfcommMccPn::from_bytes(&pn.to_bytes()).unwrap(), pn);

    // Wrapped as a command / response MCC (type/length header differs by c/r).
    assert_eq!(
        hex(&make_mcc(MccType::Pn, true, &pn.to_bytes())),
        "831104f00700e8030007"
    );
    assert_eq!(
        hex(&make_mcc(MccType::Pn, false, &pn.to_bytes())),
        "811104f00700e8030007"
    );
}

#[test]
fn mcc_msc_roundtrip() {
    let msc = RfcommMccMsc {
        dlci: 4,
        fc: false,
        rtc: true,
        rtr: true,
        ic: false,
        dv: true,
    };
    assert_eq!(hex(&msc.to_bytes()), "138d");
    assert_eq!(RfcommMccMsc::from_bytes(&msc.to_bytes()).unwrap(), msc);
    assert_eq!(
        hex(&make_mcc(MccType::Msc, true, &msc.to_bytes())),
        "e305138d"
    );
}

#[test]
fn frame_uih_carrying_pn_command() {
    let pn = RfcommMccPn {
        dlci: 4,
        cl: 0xF0,
        priority: 7,
        ack_timer: 0,
        max_frame_size: 1000,
        max_retransmissions: 0,
        initial_credits: 7,
    };
    let mcc = make_mcc(MccType::Pn, true, &pn.to_bytes());
    let frame = RfcommFrame::uih(true, 0, mcc.clone(), false);
    check_frame(frame, "03ef15831104f00700e803000770");

    // The MCC header parses back to (type, command, value).
    let (mcc_type, c_r, value) = parse_mcc(&mcc).unwrap();
    assert_eq!(mcc_type, MccType::Pn.value());
    assert!(c_r);
    assert_eq!(hex(&value), "04f00700e8030007");
    assert_eq!(RfcommMccPn::from_bytes(&value).unwrap(), pn);
}

// --- Parse-error behavior ----------------------------------------------------

#[test]
fn parse_rejects_truncated_frame() {
    assert!(RfcommFrame::from_bytes(&[0x03, 0x3f]).is_err());
    assert!(RfcommFrame::from_bytes(&[0x03, 0x3f, 0x01]).is_err());
    // Even length byte (2-byte indicator) with no room for the second octet.
    assert!(RfcommFrame::from_bytes(&[0x03, 0xef, 0x00, 0x70]).is_err());
}

#[test]
fn parse_rejects_unknown_frame_type() {
    // Control byte 0x20 masks to 0x20, which is not one of the six frame types.
    assert!(RfcommFrame::from_bytes(&[0x03, 0x20, 0x01, 0x00]).is_err());
}

#[test]
fn parse_rejects_fcs_mismatch() {
    // Valid SABM with the FCS octet flipped.
    assert!(RfcommFrame::from_bytes(&[0x03, 0x3f, 0x01, 0x1d]).is_err());
}
