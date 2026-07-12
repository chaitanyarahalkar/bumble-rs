//! The RFCOMM session runtime — a synchronous, sans-I/O port of upstream's
//! asyncio [`Multiplexer`] and [`DLC`].
//!
//! **Slice 20.** The frame codec in the crate root is the wire format; this
//! module is the state machine that drives a live session over it: opening the
//! multiplexer session on DLCI 0, negotiating and opening per-channel data link
//! connections (DLCs), and the credit-based flow control that paces data.
//!
//! # Transport-agnostic by design
//!
//! Upstream drives everything through an `asyncio` L2CAP `ClassicChannel`. This
//! port has no live Classic L2CAP connection-oriented channel to route over
//! (only the signaling codec is ported), so the state machine is *sans-I/O*: it
//! never touches a socket. Incoming frames are fed to [`Multiplexer::on_pdu`],
//! and every frame the machine wants to send is appended to an outbox drained
//! with [`Multiplexer::drain_outgoing`]. The caller relays those bytes over
//! whatever transport it has. The two-party integration test relays them
//! peer-to-peer in memory.
//!
//! Because the emitted frames are a deterministic function of the inputs, the
//! open-handshake frames (SABM/UA on DLCI 0, the PN parameter negotiation with
//! its credit/frame-size choices, and the MSC modem-status exchange) are pinned
//! byte-for-byte to captures from the real upstream state machine; see the
//! crate tests. The credit-flow arithmetic is verified behaviorally by driving
//! the transmit buffer past the credit boundary and asserting the sender blocks
//! until the peer grants more credits.
//!
//! # Structural note
//!
//! Upstream models a `DLC` as an object holding a back-reference to its
//! `Multiplexer` (calling `self.multiplexer.send_frame(...)`). Rust's ownership
//! rules make a child-holds-parent cycle awkward, so this port flattens the two
//! into a single owner: [`Multiplexer`] owns plain-data [`Dlc`] records and all
//! behavior lives on the multiplexer or on frame-producing `Dlc` methods that
//! append to a borrowed outbox and return an [`upcall`](DlcUpcall) for the
//! session-level state changes the multiplexer must apply. The observable wire
//! behavior is identical.
//!
//! [`Multiplexer`]: https://github.com/google/bumble/blob/main/bumble/rfcomm.py
//! [`DLC`]: https://github.com/google/bumble/blob/main/bumble/rfcomm.py

use std::collections::BTreeMap;

use crate::{
    make_mcc, parse_mcc, Error, FrameType, MccType, Result, RfcommFrame, RfcommMccMsc, RfcommMccPn,
    RFCOMM_DEFAULT_MAX_CREDITS,
};

/// The credit threshold at or below which a DLC replenishes the peer's credits
/// (`RFCOMM_DEFAULT_MAX_CREDITS / 2` upstream).
pub const RFCOMM_DEFAULT_CREDIT_THRESHOLD: u16 = (RFCOMM_DEFAULT_MAX_CREDITS / 2) as u16;

/// Fixed overhead subtracted from the L2CAP MTU to size an RFCOMM frame:
/// address, control, a 2-octet length indicator, and the FCS.
const FRAME_OVERHEAD: u16 = 4 + 1;

/// The role a multiplexer plays in a session. Determines the command/response
/// bit on frames and which side may actively open the session and DLCs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// The side that opens the session (a `Client` upstream).
    Initiator,
    /// The side that accepts an incoming session (a `Server` upstream).
    Responder,
}

/// Multiplexer session state (DLCI 0), mirroring upstream's `Multiplexer.State`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MultiplexerState {
    /// Not yet connected.
    Init,
    /// SABM sent on DLCI 0, awaiting UA (initiator).
    Connecting,
    /// Session established.
    Connected,
    /// A DLC open is in progress (PN sent, awaiting response).
    Opening,
    /// DISC sent on DLCI 0, awaiting UA.
    Disconnecting,
    /// Session torn down.
    Disconnected,
}

/// Per-DLC state, mirroring upstream's `DLC.State`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DlcState {
    /// Not yet negotiated.
    Init,
    /// SABM exchanged, awaiting UA (or accepted, awaiting SABM).
    Connecting,
    /// Open and ready to carry data.
    Connected,
    /// DISC sent, awaiting UA.
    Disconnecting,
    /// Closed.
    Disconnected,
}

/// A session-level state change a [`Dlc`] method asks the owning
/// [`Multiplexer`] to apply once the DLC borrow has ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DlcUpcall {
    /// Nothing for the multiplexer to do.
    None,
    /// The initiator's DLC finished opening (UA received): the session returns
    /// to `Connected` and the DLC is now open.
    OpenComplete,
    /// The responder's DLC finished opening (SABM received): the DLC is now
    /// open (upstream's `EVENT_DLC`).
    Opened,
    /// The DLC finished disconnecting: drop it.
    Disconnected,
}

/// A single data link connection: pure state, driven by [`Multiplexer`].
#[derive(Debug)]
struct Dlc {
    dlci: u8,
    /// Command/response bit: `true` for the initiator, `false` for the responder.
    c_r: bool,
    state: DlcState,
    rx_max_credits: u16,
    rx_credits: u16,
    rx_credits_threshold: u16,
    tx_credits: u16,
    tx_buffer: Vec<u8>,
    /// The largest information payload a single frame may carry.
    mtu: usize,
    /// Application data delivered to this DLC, in arrival order (upstream's sink
    /// / receive queue).
    rx_packets: Vec<Vec<u8>>,
}

impl Dlc {
    fn new(
        role: Role,
        dlci: u8,
        tx_max_frame_size: u16,
        tx_initial_credits: u16,
        rx_initial_credits: u16,
        peer_mtu: u16,
    ) -> Self {
        let mtu = tx_max_frame_size.min(peer_mtu.saturating_sub(FRAME_OVERHEAD)) as usize;
        Dlc {
            dlci,
            c_r: matches!(role, Role::Initiator),
            state: DlcState::Init,
            rx_max_credits: RFCOMM_DEFAULT_MAX_CREDITS as u16,
            rx_credits: rx_initial_credits,
            rx_credits_threshold: RFCOMM_DEFAULT_CREDIT_THRESHOLD,
            tx_credits: tx_initial_credits,
            tx_buffer: Vec::new(),
            mtu,
            rx_packets: Vec::new(),
        }
    }

    /// Build and enqueue the MSC (modem status) command both sides send as soon
    /// as the DLC's SABM/UA completes.
    fn send_msc_command(&self, outbox: &mut Vec<RfcommFrame>) {
        let msc = RfcommMccMsc {
            dlci: self.dlci,
            fc: false,
            rtc: true,
            rtr: true,
            ic: false,
            dv: true,
        };
        let mcc = make_mcc(MccType::Msc, true, &msc.to_bytes());
        outbox.push(RfcommFrame::uih(self.c_r, 0, mcc, false));
    }

    /// Active open (initiator): send the DLC's SABM.
    fn connect(&mut self, outbox: &mut Vec<RfcommFrame>) {
        self.state = DlcState::Connecting;
        outbox.push(RfcommFrame::sabm(self.c_r, self.dlci));
    }

    /// Passive open (responder): send the PN response and await the SABM.
    fn accept(
        &mut self,
        rx_max_frame_size: u16,
        rx_initial_credits: u16,
        outbox: &mut Vec<RfcommFrame>,
    ) {
        let pn = RfcommMccPn {
            dlci: self.dlci,
            cl: 0xE0,
            priority: 7,
            ack_timer: 0,
            max_frame_size: rx_max_frame_size,
            max_retransmissions: 0,
            initial_credits: rx_initial_credits as u8,
        };
        let mcc = make_mcc(MccType::Pn, false, &pn.to_bytes());
        outbox.push(RfcommFrame::uih(self.c_r, 0, mcc, false));
        self.state = DlcState::Connecting;
    }

    fn on_frame(&mut self, frame: &RfcommFrame, outbox: &mut Vec<RfcommFrame>) -> DlcUpcall {
        match frame.frame_type {
            FrameType::Sabm => self.on_sabm(outbox),
            FrameType::Ua => self.on_ua(outbox),
            FrameType::Disc => {
                outbox.push(RfcommFrame::ua(!self.c_r, self.dlci));
                DlcUpcall::None
            }
            FrameType::Uih => {
                self.on_uih(frame, outbox);
                DlcUpcall::None
            }
            // DM and UI are no-ops at the DLC level upstream.
            FrameType::Dm | FrameType::Ui => DlcUpcall::None,
        }
    }

    fn on_sabm(&mut self, outbox: &mut Vec<RfcommFrame>) -> DlcUpcall {
        if self.state != DlcState::Connecting {
            return DlcUpcall::None;
        }
        outbox.push(RfcommFrame::ua(!self.c_r, self.dlci));
        self.send_msc_command(outbox);
        self.state = DlcState::Connected;
        DlcUpcall::Opened
    }

    fn on_ua(&mut self, outbox: &mut Vec<RfcommFrame>) -> DlcUpcall {
        match self.state {
            DlcState::Connecting => {
                self.send_msc_command(outbox);
                self.state = DlcState::Connected;
                DlcUpcall::OpenComplete
            }
            DlcState::Disconnecting => {
                self.state = DlcState::Disconnected;
                DlcUpcall::Disconnected
            }
            _ => DlcUpcall::None,
        }
    }

    fn on_uih(&mut self, frame: &RfcommFrame, outbox: &mut Vec<RfcommFrame>) {
        let data: &[u8] = if frame.p_f {
            // Credit-bearing: the first octet is a credit grant, not data.
            self.tx_credits += frame.information[0] as u16;
            &frame.information[1..]
        } else {
            &frame.information
        };

        if !data.is_empty() {
            self.rx_packets.push(data.to_vec());
            if self.rx_credits > 0 {
                self.rx_credits -= 1;
            }
        }

        // Flush anything we can now send (data and/or a credit replenishment).
        self.process_tx(outbox);
    }

    /// Handle an inbound MSC on this DLC: acknowledge a command with a response.
    fn on_mcc_msc(&self, command: bool, outbox: &mut Vec<RfcommFrame>) {
        if command {
            let msc = RfcommMccMsc {
                dlci: self.dlci,
                fc: false,
                rtc: true,
                rtr: true,
                ic: false,
                dv: true,
            };
            let mcc = make_mcc(MccType::Msc, false, &msc.to_bytes());
            outbox.push(RfcommFrame::uih(self.c_r, 0, mcc, false));
        }
    }

    /// How many credits to grant the peer, or 0 if it still has enough.
    fn rx_credits_needed(&self) -> u16 {
        if self.rx_credits <= self.rx_credits_threshold {
            self.rx_max_credits - self.rx_credits
        } else {
            0
        }
    }

    /// The transmit engine (upstream `DLC.process_tx`): drain the transmit
    /// buffer while credits allow, prepending a credit grant to the first frame
    /// when the peer needs replenishing. A pure credit grant costs no transmit
    /// credit; a frame carrying data spends one.
    fn process_tx(&mut self, outbox: &mut Vec<RfcommFrame>) {
        let mut rx_credits_needed = self.rx_credits_needed();
        while (!self.tx_buffer.is_empty() && self.tx_credits > 0) || rx_credits_needed > 0 {
            let granting = rx_credits_needed > 0;
            let (chunk, tx_credit_spent) = if granting {
                let mut chunk = vec![rx_credits_needed as u8];
                self.rx_credits += rx_credits_needed;
                if !self.tx_buffer.is_empty() && self.tx_credits > 0 {
                    let take = (self.mtu - 1).min(self.tx_buffer.len());
                    chunk.extend_from_slice(&self.tx_buffer[..take]);
                    self.tx_buffer.drain(..take);
                    (chunk, true)
                } else {
                    (chunk, false)
                }
            } else {
                let take = self.mtu.min(self.tx_buffer.len());
                let chunk = self.tx_buffer[..take].to_vec();
                self.tx_buffer.drain(..take);
                (chunk, true)
            };

            if tx_credit_spent {
                self.tx_credits -= 1;
            }

            outbox.push(RfcommFrame::uih(self.c_r, self.dlci, chunk, granting));
            rx_credits_needed = 0;
        }
    }

    /// Queue application data and flush what credits allow.
    fn write(&mut self, data: &[u8], outbox: &mut Vec<RfcommFrame>) {
        self.tx_buffer.extend_from_slice(data);
        self.process_tx(outbox);
    }
}

/// The RFCOMM multiplexer: owns the session on DLCI 0 and its DLCs, and drives
/// them as a synchronous state machine. See the [module docs](self) for the
/// sans-I/O contract.
#[derive(Debug)]
pub struct Multiplexer {
    role: Role,
    state: MultiplexerState,
    dlcs: BTreeMap<u8, Dlc>,
    /// The PN this side sent while opening a DLC, kept so the DLC can be created
    /// from our own parameters when the response arrives.
    open_pn: Option<RfcommMccPn>,
    /// Responder acceptors: channel number -> (rx max frame size, rx initial
    /// credits) offered when a peer opens that channel.
    acceptor: BTreeMap<u8, (u16, u16)>,
    peer_mtu: u16,
    outbox: Vec<RfcommFrame>,
    /// DLCIs that finished opening since the last [`take_opened`](Self::take_opened).
    opened: Vec<u8>,
}

impl Multiplexer {
    /// Create a multiplexer with the given role and the negotiated L2CAP MTU
    /// (used to size DLC frames).
    pub fn new(role: Role, peer_mtu: u16) -> Self {
        Multiplexer {
            role,
            state: MultiplexerState::Init,
            dlcs: BTreeMap::new(),
            open_pn: None,
            acceptor: BTreeMap::new(),
            peer_mtu,
            outbox: Vec::new(),
            opened: Vec::new(),
        }
    }

    /// The multiplexer's role.
    pub fn role(&self) -> Role {
        self.role
    }

    /// The current session state.
    pub fn state(&self) -> MultiplexerState {
        self.state
    }

    /// Register a channel this (responder) multiplexer will accept, with the rx
    /// parameters it offers. Mirrors `Server.listen`.
    pub fn listen(&mut self, channel: u8, rx_max_frame_size: u16, rx_initial_credits: u16) {
        self.acceptor
            .insert(channel, (rx_max_frame_size, rx_initial_credits));
    }

    /// Actively open the session: send SABM on DLCI 0. Must be in `Init`.
    pub fn connect(&mut self) -> Result<()> {
        if self.state != MultiplexerState::Init {
            return Err(Error::InvalidArgument(
                "multiplexer not in INIT state".into(),
            ));
        }
        self.state = MultiplexerState::Connecting;
        // Upstream sends the DLCI-0 SABM with c_r = 1 unconditionally.
        self.outbox.push(RfcommFrame::sabm(true, 0));
        Ok(())
    }

    /// Actively open a DLC on `channel`: send the PN command. Must be in
    /// `Connected`. Mirrors `Multiplexer.open_dlc`.
    pub fn open_dlc(
        &mut self,
        channel: u8,
        max_frame_size: u16,
        initial_credits: u16,
    ) -> Result<()> {
        if self.state != MultiplexerState::Connected {
            return Err(Error::InvalidArgument("multiplexer not connected".into()));
        }
        let pn = RfcommMccPn {
            dlci: channel << 1,
            cl: 0xF0,
            priority: 7,
            ack_timer: 0,
            max_frame_size,
            max_retransmissions: 0,
            initial_credits: initial_credits as u8,
        };
        let mcc = make_mcc(MccType::Pn, true, &pn.to_bytes());
        self.open_pn = Some(pn);
        self.state = MultiplexerState::Opening;
        let c_r = matches!(self.role, Role::Initiator);
        self.outbox.push(RfcommFrame::uih(c_r, 0, mcc, false));
        Ok(())
    }

    /// Disconnect the session: send DISC on DLCI 0. Must be in `Connected`.
    pub fn disconnect(&mut self) -> Result<()> {
        if self.state != MultiplexerState::Connected {
            return Err(Error::InvalidArgument("multiplexer not connected".into()));
        }
        self.state = MultiplexerState::Disconnecting;
        let c_r = matches!(self.role, Role::Initiator);
        self.outbox.push(RfcommFrame::disc(c_r, 0));
        Ok(())
    }

    /// Queue application data on an open DLC and flush what credits allow.
    pub fn write(&mut self, dlci: u8, data: &[u8]) -> Result<()> {
        let dlc = self
            .dlcs
            .get_mut(&dlci)
            .ok_or_else(|| Error::InvalidArgument(format!("no DLC for DLCI {dlci}")))?;
        dlc.write(data, &mut self.outbox);
        Ok(())
    }

    /// Feed one received frame into the state machine.
    pub fn on_pdu(&mut self, frame: &RfcommFrame) {
        if frame.dlci == 0 {
            self.on_control_frame(frame);
        } else if frame.frame_type == FrameType::Dm {
            // DM for a DLCI is handled at the multiplexer: the DLC may not exist
            // yet (it is created only on the PN response).
            self.on_dm();
        } else {
            let dlci = frame.dlci;
            let upcall = match self.dlcs.get_mut(&dlci) {
                Some(dlc) => dlc.on_frame(frame, &mut self.outbox),
                None => return,
            };
            self.apply_upcall(dlci, upcall);
        }
    }

    /// Take the frames the machine wants to send since the last drain.
    pub fn drain_outgoing(&mut self) -> Vec<RfcommFrame> {
        std::mem::take(&mut self.outbox)
    }

    /// Take the DLCIs that finished opening since the last call.
    pub fn take_opened(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.opened)
    }

    /// The state of a DLC, if it exists.
    pub fn dlc_state(&self, dlci: u8) -> Option<DlcState> {
        self.dlcs.get(&dlci).map(|d| d.state)
    }

    /// The DLC's current transmit credits (how many frames it may still send).
    pub fn dlc_tx_credits(&self, dlci: u8) -> Option<u16> {
        self.dlcs.get(&dlci).map(|d| d.tx_credits)
    }

    /// The DLC's current receive credits (how many frames the peer may send).
    pub fn dlc_rx_credits(&self, dlci: u8) -> Option<u16> {
        self.dlcs.get(&dlci).map(|d| d.rx_credits)
    }

    /// How many bytes are queued for transmit but not yet sent (blocked on
    /// credits).
    pub fn dlc_pending_tx(&self, dlci: u8) -> Option<usize> {
        self.dlcs.get(&dlci).map(|d| d.tx_buffer.len())
    }

    /// Take the application data delivered to a DLC since the last call.
    pub fn take_rx(&mut self, dlci: u8) -> Vec<Vec<u8>> {
        self.dlcs
            .get_mut(&dlci)
            .map(|d| std::mem::take(&mut d.rx_packets))
            .unwrap_or_default()
    }

    fn on_control_frame(&mut self, frame: &RfcommFrame) {
        match frame.frame_type {
            FrameType::Sabm => {
                // Incoming session open (responder side).
                if self.state == MultiplexerState::Init {
                    self.state = MultiplexerState::Connected;
                    self.outbox.push(RfcommFrame::ua(true, 0));
                }
            }
            FrameType::Ua => match self.state {
                MultiplexerState::Connecting => self.state = MultiplexerState::Connected,
                MultiplexerState::Disconnecting => self.state = MultiplexerState::Disconnected,
                _ => {}
            },
            FrameType::Disc => {
                self.state = MultiplexerState::Disconnected;
                let c_r = !matches!(self.role, Role::Initiator);
                self.outbox.push(RfcommFrame::ua(c_r, 0));
            }
            FrameType::Dm => self.on_dm(),
            FrameType::Uih => self.on_uih_control(frame),
            FrameType::Ui => {}
        }
    }

    fn on_dm(&mut self) {
        // A DM in response to our DLC open request refuses it; return to
        // Connected so another open can be attempted.
        if self.state == MultiplexerState::Opening {
            self.state = MultiplexerState::Connected;
            self.open_pn = None;
        }
    }

    fn on_uih_control(&mut self, frame: &RfcommFrame) {
        let Ok((mcc_type, command, value)) = parse_mcc(&frame.information) else {
            return;
        };
        if mcc_type == MccType::Pn.value() {
            if let Ok(pn) = RfcommMccPn::from_bytes(&value) {
                self.on_mcc_pn(command, pn);
            }
        } else if mcc_type == MccType::Msc.value() {
            if let Ok(msc) = RfcommMccMsc::from_bytes(&value) {
                let dlci = msc.dlci;
                if let Some(dlc) = self.dlcs.get_mut(&dlci) {
                    dlc.on_mcc_msc(command, &mut self.outbox);
                }
            }
        }
    }

    fn on_mcc_pn(&mut self, command: bool, pn: RfcommMccPn) {
        if command {
            // Responder: a peer wants to open a channel.
            if pn.dlci & 1 != 0 {
                // Odd DLCI is not expected from an initiator; ignore.
                return;
            }
            let channel = pn.dlci >> 1;
            match self.acceptor.get(&channel).copied() {
                Some((rx_max_frame_size, rx_initial_credits)) => {
                    let dlc = Dlc::new(
                        self.role,
                        pn.dlci,
                        pn.max_frame_size,
                        pn.initial_credits as u16,
                        rx_initial_credits,
                        self.peer_mtu,
                    );
                    let dlci = pn.dlci;
                    self.dlcs.insert(dlci, dlc);
                    let dlc = self.dlcs.get_mut(&dlci).expect("just inserted");
                    dlc.accept(rx_max_frame_size, rx_initial_credits, &mut self.outbox);
                }
                None => {
                    // No acceptor: refuse with DM.
                    self.outbox.push(RfcommFrame::dm(true, pn.dlci));
                }
            }
        } else {
            // Initiator: the peer accepted our open; create the DLC from our own
            // parameters (rx) and theirs (tx), then send its SABM.
            if self.state == MultiplexerState::Opening {
                let open_pn = match self.open_pn.take() {
                    Some(pn) => pn,
                    None => return,
                };
                let dlc = Dlc::new(
                    self.role,
                    pn.dlci,
                    pn.max_frame_size,
                    pn.initial_credits as u16,
                    open_pn.initial_credits as u16,
                    self.peer_mtu,
                );
                let dlci = pn.dlci;
                self.dlcs.insert(dlci, dlc);
                let dlc = self.dlcs.get_mut(&dlci).expect("just inserted");
                dlc.connect(&mut self.outbox);
            }
        }
    }

    fn apply_upcall(&mut self, dlci: u8, upcall: DlcUpcall) {
        match upcall {
            DlcUpcall::OpenComplete => {
                self.state = MultiplexerState::Connected;
                self.opened.push(dlci);
            }
            DlcUpcall::Opened => self.opened.push(dlci),
            DlcUpcall::Disconnected => {
                self.dlcs.remove(&dlci);
            }
            DlcUpcall::None => {}
        }
    }
}
