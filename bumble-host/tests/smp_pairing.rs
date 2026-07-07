//! Slice-14 (real) acceptance: an LE Legacy (JustWorks) pairing handshake run
//! over the connection through the `Device` API. Two independent peers exchange
//! Pairing Request/Response/Confirm/Random on the SMP channel (CID 0x0006),
//! each verifies the peer's confirm by recomputing `c1` with the *received*
//! random, and both derive the same Short Term Key from `s1` — a genuine
//! two-party agreement, not a self-comparison.

use bumble::{Address, AddressType};
use bumble_controller::{Controller, LocalLink};
use bumble_hci::Command;
use bumble_host::{pump, Device};
use bumble_smp::{legacy_confirm, legacy_stk, PairingFeatures, SmpPdu, SMP_CID};

fn addr(s: &str) -> Address {
    Address::parse(s, AddressType::RANDOM_DEVICE).unwrap()
}

const CENTRAL_ADDR: &str = "C4:F2:17:1A:1D:AA";
const PERIPHERAL_ADDR: &str = "C4:F2:17:1A:1D:BB";

fn connect(link: &mut LocalLink, central: usize, peripheral: usize) {
    link.handle_command(
        peripheral,
        Command::LeSetRandomAddress {
            random_address: addr(PERIPHERAL_ADDR),
        },
    );
    link.handle_command(
        peripheral,
        Command::LeSetAdvertisingEnable {
            advertising_enable: 1,
        },
    );
    link.handle_command(
        central,
        Command::LeSetRandomAddress {
            random_address: addr(CENTRAL_ADDR),
        },
    );
    link.handle_command(
        central,
        Command::LeCreateConnection {
            le_scan_interval: 16,
            le_scan_window: 16,
            initiator_filter_policy: 0,
            peer_address_type: 1,
            peer_address: addr(PERIPHERAL_ADDR),
            own_address_type: 1,
            connection_interval_min: 24,
            connection_interval_max: 40,
            max_latency: 0,
            supervision_timeout: 42,
            min_ce_length: 0,
            max_ce_length: 0,
        },
    );
    link.establish_connections();
}

/// Send an SMP PDU and pump; return the single SMP PDU the peer received.
fn exchange(
    link: &mut LocalLink,
    devices: &mut [Device],
    from: usize,
    to: usize,
    pdu: SmpPdu,
) -> SmpPdu {
    assert!(devices[from].send_l2cap(link, SMP_CID, &pdu.to_bytes()));
    pump(link, devices);
    let mut received = devices[to].take_l2cap(SMP_CID);
    assert_eq!(received.len(), 1, "expected one SMP PDU");
    SmpPdu::from_bytes(&received.pop().unwrap()).unwrap()
}

#[test]
fn le_legacy_pairing_handshake_derives_matching_stk() {
    let mut link = LocalLink::new();
    let central = link.add_controller(Controller::new("C", addr("00:00:00:00:00:01")));
    let peripheral = link.add_controller(Controller::new("P", addr("00:00:00:00:00:02")));
    let mut devices = [Device::new(central), Device::new(peripheral)];
    connect(&mut link, central, peripheral);
    pump(&mut link, &mut devices);

    // Addresses used in the confirm computation (both random, type 1).
    let ia = addr(CENTRAL_ADDR);
    let ra = addr(PERIPHERAL_ADDR);
    let (iat, rat) = (1u8, 1u8);
    let tk = [0u8; 16]; // JustWorks: TK = 0

    let features = PairingFeatures {
        io_capability: 0x03,
        oob_data_flag: 0,
        auth_req: 0x01,
        maximum_encryption_key_size: 16,
        initiator_key_distribution: 0x07,
        responder_key_distribution: 0x07,
    };

    // 1. Feature exchange: central → Pairing Request, peripheral → Pairing Response.
    let req_on_peripheral = exchange(
        &mut link,
        &mut devices,
        central,
        peripheral,
        SmpPdu::PairingRequest(features),
    );
    let resp_on_central = exchange(
        &mut link,
        &mut devices,
        peripheral,
        central,
        SmpPdu::PairingResponse(features),
    );

    // Each side now holds both PDUs (its own + the one it received on the wire).
    let preq = SmpPdu::PairingRequest(features).to_bytes();
    let pres = SmpPdu::PairingResponse(features).to_bytes();
    assert_eq!(req_on_peripheral.to_bytes(), preq);
    assert_eq!(resp_on_central.to_bytes(), pres);

    // 2. Each side picks a random and sends its confirm.
    let mrand = [0x11u8; 16];
    let srand = [0x22u8; 16];
    let mconfirm = legacy_confirm(&tk, &mrand, &preq, &pres, &ia, iat, &ra, rat);
    let sconfirm = legacy_confirm(&tk, &srand, &preq, &pres, &ia, iat, &ra, rat);

    let peer_mconfirm = match exchange(
        &mut link,
        &mut devices,
        central,
        peripheral,
        SmpPdu::PairingConfirm {
            confirm_value: mconfirm,
        },
    ) {
        SmpPdu::PairingConfirm { confirm_value } => confirm_value,
        other => panic!("expected confirm, got {other:?}"),
    };
    let peer_sconfirm = match exchange(
        &mut link,
        &mut devices,
        peripheral,
        central,
        SmpPdu::PairingConfirm {
            confirm_value: sconfirm,
        },
    ) {
        SmpPdu::PairingConfirm { confirm_value } => confirm_value,
        other => panic!("expected confirm, got {other:?}"),
    };

    // 3. Exchange the randoms; each side verifies the peer's confirm by
    //    recomputing c1 with the received random.
    let mrand_recv = match exchange(
        &mut link,
        &mut devices,
        central,
        peripheral,
        SmpPdu::PairingRandom {
            random_value: mrand,
        },
    ) {
        SmpPdu::PairingRandom { random_value } => random_value,
        other => panic!("expected random, got {other:?}"),
    };
    // Peripheral verifies the central's confirm.
    assert_eq!(
        legacy_confirm(&tk, &mrand_recv, &preq, &pres, &ia, iat, &ra, rat),
        peer_mconfirm,
        "peripheral must verify the central's confirm"
    );

    let srand_recv = match exchange(
        &mut link,
        &mut devices,
        peripheral,
        central,
        SmpPdu::PairingRandom {
            random_value: srand,
        },
    ) {
        SmpPdu::PairingRandom { random_value } => random_value,
        other => panic!("expected random, got {other:?}"),
    };
    // Central verifies the peripheral's confirm.
    assert_eq!(
        legacy_confirm(&tk, &srand_recv, &preq, &pres, &ia, iat, &ra, rat),
        peer_sconfirm,
        "central must verify the peripheral's confirm"
    );

    // 4. Each side independently derives the STK: the central from the srand it
    //    received + its own mrand; the peripheral from its own srand + the mrand
    //    it received. They must agree.
    let central_stk = legacy_stk(&tk, &srand_recv, &mrand);
    let peripheral_stk = legacy_stk(&tk, &srand, &mrand_recv);
    assert_eq!(
        central_stk, peripheral_stk,
        "both peers derive the same STK"
    );
}
