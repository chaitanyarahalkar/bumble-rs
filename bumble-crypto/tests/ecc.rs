//! P-256 ECC key-agreement acceptance suite (slice 19). The public-key
//! coordinates and the Diffie-Hellman shared secret are pinned to ground-truth
//! values captured from upstream Python Bumble's `crypto.EccKey`
//! (`from_private_key_bytes`, `.x`/`.y`, `.dh`).

use bumble_crypto::EccKey;

fn unhex(s: &str) -> Vec<u8> {
    let s: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}

/// Deterministic private scalars 0x01..=0x20 and 0x21..=0x40 (both well below
/// the curve order), matching the oracle capture.
fn key_a() -> EccKey {
    EccKey::from_private_key_bytes(&(1u8..=32).collect::<Vec<u8>>()).unwrap()
}
fn key_b() -> EccKey {
    EccKey::from_private_key_bytes(&(33u8..=64).collect::<Vec<u8>>()).unwrap()
}

#[test]
fn ecc_public_keys_match_oracle() {
    let ka = key_a();
    assert_eq!(
        ka.public_x().to_vec(),
        unhex("515c3d6eb9e396b904d3feca7f54fdcd0cc1e997bf375dca515ad0a6c3b4035f")
    );
    assert_eq!(
        ka.public_y().to_vec(),
        unhex("4536be3a50f318fbf9a5475902a221502bef0d57e08c53b2cc0a56f17d9f9354")
    );

    let kb = key_b();
    assert_eq!(
        kb.public_x().to_vec(),
        unhex("1f140146bfb1b251f84f4ddbe0d4cdcfd77afd984a9520e35794021f8312bb9e")
    );
    assert_eq!(
        kb.public_y().to_vec(),
        unhex("ec995a08b1fa7704df3dcc0b50a9665263fb7711f95f9f8a449c5096e47c892b")
    );
}

#[test]
fn ecdh_shared_secret_matches_oracle() {
    let ka = key_a();
    let kb = key_b();

    // `EccKey::dh` returns the shared X coordinate big-endian; upstream stores
    // it little-endian (`ecc_key.dh(...)[::-1]`), so reverse to compare.
    let mut dh_a = ka.dh(&kb.public_x(), &kb.public_y()).unwrap().to_vec();
    dh_a.reverse();
    let mut dh_b = kb.dh(&ka.public_x(), &ka.public_y()).unwrap().to_vec();
    dh_b.reverse();

    assert_eq!(
        dh_a,
        unhex("0a1545129049c607555792865d22c308d96e2e823895a6c2a18a378f9043e24f")
    );
    // Both peers derive the same secret — the whole point of the exchange.
    assert_eq!(dh_a, dh_b);
}

#[test]
fn generated_keys_agree() {
    // A freshly generated pair still produces a shared secret both sides agree
    // on (exercises `generate` + `dh` without pinning to a fixed vector).
    let ka = EccKey::generate();
    let kb = EccKey::generate();
    let ab = ka.dh(&kb.public_x(), &kb.public_y()).unwrap();
    let ba = kb.dh(&ka.public_x(), &ka.public_y()).unwrap();
    assert_eq!(ab, ba);
}

#[test]
fn rejects_bad_peer_coordinates() {
    let ka = key_a();
    // A point not on the curve must be rejected, not silently accepted.
    assert!(ka.dh(&[0u8; 32], &[0u8; 32]).is_err());
    assert!(ka.dh(&[1u8; 16], &[2u8; 16]).is_err());
}
