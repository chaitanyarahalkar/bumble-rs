//! Slice-20 acceptance: a two-party RFCOMM session driven peer-to-peer over an
//! in-memory frame relay.
//!
//! Two [`Multiplexer`]s — an initiator and a responder — exchange frames
//! through [`pump`], which drains each side's outbox and feeds it to the other
//! until both fall quiet. Nothing here touches a socket: the state machines are
//! sans-I/O (see `bumble_rfcomm::mux`), and this test *is* the transport.
//!
//! Two properties are checked:
//!
//! 1. **The open handshake is byte-pinned.** The frames each side emits while
//!    opening the session (SABM/UA on DLCI 0) and a data link connection (PN
//!    parameter negotiation, then SABM/UA and the MSC modem-status exchange)
//!    are asserted against hex captured from the *real* upstream Python
//!    `Multiplexer`/`DLC` driven over the same in-memory relay. This pins the
//!    field-value choices the state machine makes — the `0xF0`/`0xE0`
//!    convergence layers, the credit and frame-size negotiation, the MSC
//!    signals — not just the frame codec (already pinned in slice 17).
//!
//! 2. **Credit-based flow control blocks and resumes.** A write larger than the
//!    sender's transmit credits is shown to stall with data still buffered, and
//!    to drain to completion only once the peer grants more credits — the one
//!    subtle path in the runtime.

use bumble_rfcomm::mux::{DlcState, Multiplexer, MultiplexerState, Role};
use bumble_rfcomm::RfcommFrame;

/// The negotiated L2CAP MTU the oracle capture used.
const PEER_MTU: u16 = 512;

fn hex(frame: &RfcommFrame) -> String {
    frame
        .to_bytes()
        .unwrap()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Relay frames between two multiplexers until neither has anything to send.
/// Frames are delivered in strict per-direction FIFO order, matching how a real
/// transport (and the oracle's event loop) delivers them.
fn pump(a: &mut Multiplexer, b: &mut Multiplexer) {
    for _ in 0..1000 {
        let from_a = a.drain_outgoing();
        let from_b = b.drain_outgoing();
        if from_a.is_empty() && from_b.is_empty() {
            return;
        }
        for f in &from_a {
            b.on_pdu(f);
        }
        for f in &from_b {
            a.on_pdu(f);
        }
    }
    panic!("relay did not reach quiescence");
}

/// Like [`pump`], but records the hex of every frame each side emits.
fn pump_collect(
    a: &mut Multiplexer,
    b: &mut Multiplexer,
    a_sent: &mut Vec<String>,
    b_sent: &mut Vec<String>,
) {
    for _ in 0..1000 {
        let from_a = a.drain_outgoing();
        let from_b = b.drain_outgoing();
        if from_a.is_empty() && from_b.is_empty() {
            return;
        }
        for f in &from_a {
            a_sent.push(hex(f));
            b.on_pdu(f);
        }
        for f in &from_b {
            b_sent.push(hex(f));
            a.on_pdu(f);
        }
    }
    panic!("relay did not reach quiescence");
}

#[test]
fn open_handshake_matches_upstream_bytes() {
    let mut a = Multiplexer::new(Role::Initiator, PEER_MTU);
    let mut b = Multiplexer::new(Role::Responder, PEER_MTU);
    // Responder accepts channel 1, offering 64-byte frames and 5 initial credits.
    b.listen(1, 64, 5);

    let mut a_sent = Vec::new();
    let mut b_sent = Vec::new();

    // Session open on DLCI 0.
    a.connect().unwrap();
    pump_collect(&mut a, &mut b, &mut a_sent, &mut b_sent);
    assert_eq!(a.state(), MultiplexerState::Connected);
    assert_eq!(b.state(), MultiplexerState::Connected);

    // Open a DLC on channel 1 with the initiator's own parameters (48-byte
    // frames, 4 initial credits).
    a.open_dlc(1, 48, 4).unwrap();
    pump_collect(&mut a, &mut b, &mut a_sent, &mut b_sent);

    // Ground truth captured from upstream Python Bumble
    // (commit 1d26b99865f96a3e7359009424c0ddf2934acd0b) driven over the same
    // in-memory relay: see scratchpad/rfcomm_oracle.py.
    assert_eq!(
        a_sent,
        vec![
            "033f011c",                     // SABM, DLCI 0 (c/r = 1)
            "03ef15831102f007003000000470", // UIH, PN command (cl=0xF0, mfs=48, credits=4)
            "0b3f0159",                     // SABM, DLCI 2
            "03ef09e3050b8d70",             // UIH, MSC command
            "03ef09e1050b8d70",             // UIH, MSC response
        ],
        "initiator handshake frames"
    );
    assert_eq!(
        b_sent,
        vec![
            "037301d7",                     // UA, DLCI 0
            "01ef15811102e0070040000005aa", // UIH, PN response (cl=0xE0, mfs=64, credits=5)
            "0b730192",                     // UA, DLCI 2
            "01ef09e3050b8daa",             // UIH, MSC command
            "01ef09e1050b8daa",             // UIH, MSC response
        ],
        "responder handshake frames"
    );

    // The DLC is open on both sides, with parameters cross-assigned: each
    // side's transmit credits are what the peer offered.
    let dlci = 2;
    assert_eq!(a.dlc_state(dlci), Some(DlcState::Connected));
    assert_eq!(b.dlc_state(dlci), Some(DlcState::Connected));
    assert_eq!(a.dlc_tx_credits(dlci), Some(5)); // responder offered 5
    assert_eq!(a.dlc_rx_credits(dlci), Some(4)); // initiator asked for 4
    assert_eq!(b.dlc_tx_credits(dlci), Some(4));
    assert_eq!(b.dlc_rx_credits(dlci), Some(5));

    // A single small write is byte-pinned too: it carries the initiator's first
    // credit grant (28 = 32 - 4) piggybacked on the payload, and the responder
    // answers with a credit-only grant of its own.
    a.write(dlci, b"hello").unwrap();
    let a_frames = a.drain_outgoing();
    assert_eq!(a_frames.len(), 1);
    assert_eq!(hex(&a_frames[0]), "0bff0b1c68656c6c6f86");
    for f in &a_frames {
        b.on_pdu(f);
    }
    let b_frames = b.drain_outgoing();
    assert_eq!(b_frames.len(), 1);
    assert_eq!(hex(&b_frames[0]), "09ff011c5c");
    for f in &b_frames {
        a.on_pdu(f);
    }

    assert_eq!(b.take_rx(dlci), vec![b"hello".to_vec()]);
    assert_eq!(a.dlc_tx_credits(dlci), Some(32)); // replenished by the peer's grant
    assert_eq!(b.dlc_rx_credits(dlci), Some(32));
}

/// Open a session and a single DLC on `channel`, leaving both peers connected.
fn open_dlc(
    a: &mut Multiplexer,
    b: &mut Multiplexer,
    channel: u8,
    a_max_frame_size: u16,
    a_initial_credits: u16,
) {
    a.connect().unwrap();
    pump(a, b);
    a.open_dlc(channel, a_max_frame_size, a_initial_credits)
        .unwrap();
    pump(a, b);
}

#[test]
fn credit_flow_blocks_then_resumes() {
    let mut a = Multiplexer::new(Role::Initiator, PEER_MTU);
    let mut b = Multiplexer::new(Role::Responder, PEER_MTU);
    // Responder offers 8-byte frames and only 2 initial credits: those 2
    // credits become the initiator's transmit budget.
    b.listen(1, 8, 2);
    open_dlc(&mut a, &mut b, 1, 8, 7);

    let dlci = 2;
    assert_eq!(a.dlc_state(dlci), Some(DlcState::Connected));
    assert_eq!(b.dlc_state(dlci), Some(DlcState::Connected));
    assert_eq!(
        a.dlc_tx_credits(dlci),
        Some(2),
        "budget = responder's offer"
    );

    // Write more than fits in the transmit budget: 20 bytes across an 8-byte
    // MTU needs more than 2 frames.
    let data: Vec<u8> = (0u8..20).collect();
    a.write(dlci, &data).unwrap();

    // The sender emits exactly its 2 credits' worth of frames, then stalls with
    // the remainder still buffered.
    let first = a.drain_outgoing();
    assert_eq!(first.len(), 2, "sender spends exactly its transmit credits");
    assert_eq!(
        a.dlc_tx_credits(dlci),
        Some(0),
        "transmit credits exhausted"
    );
    assert!(
        a.dlc_pending_tx(dlci).unwrap() > 0,
        "unsent data is blocked awaiting credits"
    );

    // Deliver the two frames; the peer consumes them and grants fresh credits,
    // which unblocks the backlog and drains it to completion.
    for f in &first {
        b.on_pdu(f);
    }
    pump(&mut a, &mut b);

    let received: Vec<u8> = b.take_rx(dlci).concat();
    assert_eq!(
        received, data,
        "all bytes delivered in order after replenishment"
    );
    assert_eq!(
        a.dlc_pending_tx(dlci),
        Some(0),
        "transmit buffer fully drained"
    );
    assert!(
        a.dlc_tx_credits(dlci).unwrap() > 0,
        "credits replenished by the peer's grant"
    );
}

#[test]
fn paused_reading_withholds_rfcomm_credits_until_the_sink_resumes() {
    let mut a = Multiplexer::new(Role::Initiator, PEER_MTU);
    let mut b = Multiplexer::new(Role::Responder, PEER_MTU);
    b.listen(1, 8, 2);
    open_dlc(&mut a, &mut b, 1, 8, 7);
    let dlci = 2;

    b.set_dlc_reading_paused(dlci, true).unwrap();
    assert_eq!(b.dlc_is_reading_paused(dlci), Some(true));
    let data: Vec<u8> = (0..20).collect();
    a.write(dlci, &data).unwrap();
    for frame in a.drain_outgoing() {
        b.on_pdu(&frame);
    }
    assert!(b.drain_outgoing().is_empty());
    assert_eq!(a.dlc_tx_credits(dlci), Some(0));
    assert!(a.dlc_pending_tx(dlci).unwrap() > 0);

    b.set_dlc_reading_paused(dlci, false).unwrap();
    assert_eq!(b.dlc_is_reading_paused(dlci), Some(false));
    pump(&mut a, &mut b);
    assert_eq!(b.take_rx(dlci).concat(), data);
    assert_eq!(a.dlc_pending_tx(dlci), Some(0));
}

#[test]
fn disconnecting_one_dlc_keeps_the_session_reusable() {
    let mut a = Multiplexer::new(Role::Initiator, PEER_MTU);
    let mut b = Multiplexer::new(Role::Responder, PEER_MTU);
    b.listen(1, 64, 5);
    open_dlc(&mut a, &mut b, 1, 48, 4);
    let dlci = 2;

    a.disconnect_dlc(dlci).unwrap();
    pump(&mut a, &mut b);
    assert_eq!(a.state(), MultiplexerState::Connected);
    assert_eq!(b.state(), MultiplexerState::Connected);
    assert_eq!(a.dlc_state(dlci), None);

    a.open_dlc(1, 48, 4).unwrap();
    pump(&mut a, &mut b);
    assert_eq!(a.dlc_state(dlci), Some(DlcState::Connected));
    assert_eq!(b.dlc_state(dlci), Some(DlcState::Connected));
}
