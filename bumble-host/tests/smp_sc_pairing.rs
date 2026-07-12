//! Slice-19 acceptance: an LE Secure Connections (JustWorks) pairing handshake
//! run over the connection through the `Device` API. Two independent peers each
//! own a P-256 key pair, exchange public keys and nonces on the SMP channel
//! (CID 0x0006), and derive the Long Term Key from `f5` after the responder's
//! `f4` confirm and the `f6` DHKey checks are cross-verified — a genuine
//! two-party agreement, not a self-comparison. Each side computes its DHKey
//! from the *peer's* transmitted public key and keys off the *received* nonce.

use bumble::{Address, AddressType};
use bumble_controller::{Controller, LocalLink};
use bumble_crypto::EccKey;
use bumble_hci::Command;
use bumble_host::{pump, Device};
use bumble_smp::{sc, PairingFeatures, SmpPdu, SMP_CID};

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

/// The little-endian X coordinate as f4/f5/f6/g2 (and the wire) use it.
fn x_le(key: &EccKey) -> [u8; 32] {
    let mut x = key.public_x();
    x.reverse();
    x
}

/// The DHKey shared secret, byte-reversed to little-endian, computed from the
/// peer's little-endian public key coordinates as they arrive on the wire.
fn dh_key_le(own: &EccKey, peer_x_le: &[u8; 32], peer_y_le: &[u8; 32]) -> [u8; 32] {
    let mut peer_x_be = *peer_x_le;
    peer_x_be.reverse();
    let mut peer_y_be = *peer_y_le;
    peer_y_be.reverse();
    let mut dh = own.dh(&peer_x_be, &peer_y_be).unwrap();
    dh.reverse();
    dh
}

#[test]
fn le_sc_just_works_handshake_derives_matching_ltk() {
    let mut link = LocalLink::new();
    let central = link.add_controller(Controller::new("C", addr("00:00:00:00:00:01")));
    let peripheral = link.add_controller(Controller::new("P", addr("00:00:00:00:00:02")));
    let mut devices = [Device::new(central), Device::new(peripheral)];
    connect(&mut link, central, peripheral);
    pump(&mut link, &mut devices);

    // Deterministic key pairs so the run is reproducible; each side keeps its
    // secret and only its public key ever crosses the wire.
    let ka = EccKey::from_private_key_bytes(&(1u8..=32).collect::<Vec<u8>>()).unwrap();
    let kb = EccKey::from_private_key_bytes(&(33u8..=64).collect::<Vec<u8>>()).unwrap();

    // Addresses used in the f6 checks (both random, type 1), little-endian
    // exactly as upstream feeds `connection.self_address.address_bytes`.
    let ia = *addr(CENTRAL_ADDR).address_bytes();
    let ra = *addr(PERIPHERAL_ADDR).address_bytes();
    let (iat, rat) = (1u8, 1u8);

    // Secure Connections capable, MITM + bonding requested (auth_req = 0x0d).
    let features = PairingFeatures {
        io_capability: 0x03,
        oob_data_flag: 0,
        auth_req: 0x0d,
        maximum_encryption_key_size: 16,
        initiator_key_distribution: 0x07,
        responder_key_distribution: 0x07,
    };
    let mut resp_features = features;
    resp_features.io_capability = 0x04;

    // 1. Feature exchange.
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
        SmpPdu::PairingResponse(resp_features),
    );
    let preq = SmpPdu::PairingRequest(features).to_bytes();
    let pres = SmpPdu::PairingResponse(resp_features).to_bytes();
    assert_eq!(req_on_peripheral.to_bytes(), preq);
    assert_eq!(resp_on_central.to_bytes(), pres);
    let io_cap_a = sc::io_cap(&preq).unwrap();
    let io_cap_b = sc::io_cap(&pres).unwrap();

    // 2. Public-key exchange (little-endian on the wire). Each side derives the
    //    DHKey from the coordinates it *received*, never from the peer's secret.
    let pka_x = x_le(&ka);
    let pkb_x = x_le(&kb);
    let mut pka_y = ka.public_y();
    pka_y.reverse();
    let mut pkb_y = kb.public_y();
    pkb_y.reverse();

    let (peer_pka_x, peer_pka_y) = match exchange(
        &mut link,
        &mut devices,
        central,
        peripheral,
        SmpPdu::PairingPublicKey {
            public_key_x: pka_x,
            public_key_y: pka_y,
        },
    ) {
        SmpPdu::PairingPublicKey {
            public_key_x,
            public_key_y,
        } => (public_key_x, public_key_y),
        other => panic!("expected public key, got {other:?}"),
    };
    let (peer_pkb_x, peer_pkb_y) = match exchange(
        &mut link,
        &mut devices,
        peripheral,
        central,
        SmpPdu::PairingPublicKey {
            public_key_x: pkb_x,
            public_key_y: pkb_y,
        },
    ) {
        SmpPdu::PairingPublicKey {
            public_key_x,
            public_key_y,
        } => (public_key_x, public_key_y),
        other => panic!("expected public key, got {other:?}"),
    };

    // Central holds kb's public key; peripheral holds ka's. Independent DHKeys.
    let dh_central = dh_key_le(&ka, &peer_pkb_x, &peer_pkb_y);
    let dh_peripheral = dh_key_le(&kb, &peer_pka_x, &peer_pka_y);
    assert_eq!(
        dh_central, dh_peripheral,
        "both peers derive the same DHKey"
    );

    // 3. Responder confirm Cb = f4(PKb, PKa, Nb, 0), sent before the nonces.
    let na = [0xA0u8; 16];
    let nb = [0xB0u8; 16];
    let cb = sc::confirm_value(&pkb_x, &pka_x, &nb);
    let peer_cb = match exchange(
        &mut link,
        &mut devices,
        peripheral,
        central,
        SmpPdu::PairingConfirm { confirm_value: cb },
    ) {
        SmpPdu::PairingConfirm { confirm_value } => confirm_value,
        other => panic!("expected confirm, got {other:?}"),
    };

    // 4. Nonce exchange.
    let na_recv = match exchange(
        &mut link,
        &mut devices,
        central,
        peripheral,
        SmpPdu::PairingRandom { random_value: na },
    ) {
        SmpPdu::PairingRandom { random_value } => random_value,
        other => panic!("expected random, got {other:?}"),
    };
    let nb_recv = match exchange(
        &mut link,
        &mut devices,
        peripheral,
        central,
        SmpPdu::PairingRandom { random_value: nb },
    ) {
        SmpPdu::PairingRandom { random_value } => random_value,
        other => panic!("expected random, got {other:?}"),
    };

    // Central verifies the responder's confirm against the received Nb and the
    // received PKb — this is what defends against a swapped public key.
    assert_eq!(
        sc::confirm_value(&peer_pkb_x, &pka_x, &nb_recv),
        peer_cb,
        "central must verify the responder's f4 confirm"
    );

    // 5. Each side derives the keys from the values it received off the wire.
    let central_keys = sc::just_works_keys(
        &dh_central,
        &na,
        &nb_recv,
        &ia,
        iat,
        &ra,
        rat,
        &io_cap_a,
        &io_cap_b,
        &pka_x,
        &peer_pkb_x,
    );
    let peripheral_keys = sc::just_works_keys(
        &dh_peripheral,
        &na_recv,
        &nb,
        &ia,
        iat,
        &ra,
        rat,
        &io_cap_a,
        &io_cap_b,
        &peer_pka_x,
        &pkb_x,
    );

    // 6. DHKey-check exchange: initiator sends Ea, responder sends Eb; each
    //    verifies the other's against its own computation.
    let peer_ea = match exchange(
        &mut link,
        &mut devices,
        central,
        peripheral,
        SmpPdu::PairingDhKeyCheck {
            dhkey_check: central_keys.ea,
        },
    ) {
        SmpPdu::PairingDhKeyCheck { dhkey_check } => dhkey_check,
        other => panic!("expected DHKey check, got {other:?}"),
    };
    assert_eq!(
        peer_ea, peripheral_keys.ea,
        "peripheral must verify the initiator's Ea"
    );
    let peer_eb = match exchange(
        &mut link,
        &mut devices,
        peripheral,
        central,
        SmpPdu::PairingDhKeyCheck {
            dhkey_check: peripheral_keys.eb,
        },
    ) {
        SmpPdu::PairingDhKeyCheck { dhkey_check } => dhkey_check,
        other => panic!("expected DHKey check, got {other:?}"),
    };
    assert_eq!(
        peer_eb, central_keys.eb,
        "central must verify the responder's Eb"
    );

    // 7. The whole point: both peers hold the same Long Term Key, and it agrees
    //    with the numeric-comparison value both would display.
    assert_eq!(
        central_keys.ltk, peripheral_keys.ltk,
        "both peers derive the same LTK"
    );
    assert_eq!(central_keys.mac_key, peripheral_keys.mac_key);
    assert_eq!(central_keys.numeric_check, peripheral_keys.numeric_check);
    assert!(central_keys.numeric_check < 1_000_000);
}
