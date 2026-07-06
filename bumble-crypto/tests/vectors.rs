//! SMP crypto acceptance suite. Ported 1:1 from google/bumble
//! `tests/smp_test.py`, using the published Bluetooth-spec and RFC 4493 test
//! vectors. `rhex` mirrors the test's `reversed_hex` (spec vectors are
//! big-endian; Bumble works little-endian).

use bumble_crypto::{aes_cmac, ah, c1, f4, f5, f6, g2, h6, h7, s1};

fn unhex(s: &str) -> Vec<u8> {
    let s: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}

/// `reversed_hex` from the upstream tests: hex → bytes, reversed.
fn rhex(s: &str) -> Vec<u8> {
    let mut v = unhex(s);
    v.reverse();
    v
}

// smp_test.py::test_aes_cmac (RFC 4493 vectors)
#[test]
fn test_aes_cmac() {
    let k = unhex("2b7e1516 28aed2a6 abf71588 09cf4f3c");

    assert_eq!(
        aes_cmac(&[], &k).to_vec(),
        unhex("bb1d6929 e9593728 7fa37d12 9b756746")
    );

    assert_eq!(
        aes_cmac(&unhex("6bc1bee2 2e409f96 e93d7e11 7393172a"), &k).to_vec(),
        unhex("070a16b4 6b4d4144 f79bdd9d d04a287c")
    );

    assert_eq!(
        aes_cmac(
            &unhex("6bc1bee2 2e409f96 e93d7e11 7393172a ae2d8a57 1e03ac9c 9eb76fac 45af8e51 30c81c46 a35ce411"),
            &k
        )
        .to_vec(),
        unhex("dfa66747 de9ae630 30ca3261 1497c827")
    );

    assert_eq!(
        aes_cmac(
            &unhex(
                "6bc1bee2 2e409f96 e93d7e11 7393172a ae2d8a57 1e03ac9c 9eb76fac 45af8e51 \
                 30c81c46 a35ce411 e5fbc119 1a0a52ef f69f2445 df4f9b17 ad2b417b e66c3710"
            ),
            &k
        )
        .to_vec(),
        unhex("51f0bebf 7e3b9d92 fc497417 79363cfe")
    );
}

// smp_test.py::test_c1
#[test]
fn test_c1() {
    let k = vec![0u8; 16];
    let r = rhex("5783D52156AD6F0E6388274EC6702EE0");
    let pres = rhex("05000800000302");
    let preq = rhex("07071000000101");
    let ia = rhex("A1A2A3A4A5A6");
    let ra = rhex("B1B2B3B4B5B6");
    let result = c1(&k, &r, &preq, &pres, 1, 0, &ia, &ra);
    assert_eq!(result, rhex("1e1e3fef878988ead2a74dc5bef13b86"));
}

// smp_test.py::test_s1
#[test]
fn test_s1() {
    let k = vec![0u8; 16];
    let r1 = rhex("000F0E0D0C0B0A091122334455667788");
    let r2 = rhex("010203040506070899AABBCCDDEEFF00");
    assert_eq!(s1(&k, &r1, &r2), rhex("9a1fe1f0e8b0f49b5b4216ae796da062"));
}

// smp_test.py::test_f4
#[test]
fn test_f4() {
    let u = rhex("20b003d2 f297be2c 5e2c83a7 e9f9a5b9 eff49111 acf4fddb cc030148 0e359de6");
    let v = rhex("55188b3d 32f6bb9a 900afcfb eed4e72a 59cb9ac2 f19d7cfb 6b4fdd49 f47fc5fd");
    let x = rhex("d5cb8454 d177733e ffffb2ec 712baeab");
    assert_eq!(
        f4(&u, &v, &x, 0),
        rhex("f2c916f1 07a9bd1c f1eda1be a974872d")
    );
}

// smp_test.py::test_f5
#[test]
fn test_f5() {
    let w = rhex("ec0234a3 57c8ad05 341010a6 0a397d9b 99796b13 b4f866f1 868d34f3 73bfa698");
    let n1 = rhex("d5cb8454 d177733e ffffb2ec 712baeab");
    let n2 = rhex("a6e8e7cc 25a75f6e 216583f7 ff3dc4cf");
    let a1 = rhex("00561237 37bfce");
    let a2 = rhex("00a71370 2dcfc1");
    let (mac_key, ltk) = f5(&w, &n1, &n2, &a1, &a2);
    assert_eq!(mac_key, rhex("2965f176 a1084a02 fd3f6a20 ce636e20"));
    assert_eq!(ltk, rhex("69867911 69d7cd23 980522b5 94750a38"));
}

// smp_test.py::test_f6
#[test]
fn test_f6() {
    let n1 = rhex("d5cb8454 d177733e ffffb2ec 712baeab");
    let n2 = rhex("a6e8e7cc 25a75f6e 216583f7 ff3dc4cf");
    let mac_key = rhex("2965f176 a1084a02 fd3f6a20 ce636e20");
    let r = rhex("12a3343b b453bb54 08da42d2 0c2d0fc8");
    let io_cap = rhex("010102");
    let a1 = rhex("00561237 37bfce");
    let a2 = rhex("00a71370 2dcfc1");
    assert_eq!(
        f6(&mac_key, &n1, &n2, &r, &io_cap, &a1, &a2),
        rhex("e3c47398 9cd0e8c5 d26c0b09 da958f61")
    );
}

// smp_test.py::test_g2
#[test]
fn test_g2() {
    let u = rhex("20b003d2 f297be2c 5e2c83a7 e9f9a5b9 eff49111 acf4fddb cc030148 0e359de6");
    let v = rhex("55188b3d 32f6bb9a 900afcfb eed4e72a 59cb9ac2 f19d7cfb 6b4fdd49 f47fc5fd");
    let x = rhex("d5cb8454 d177733e ffffb2ec 712baeab");
    let y = rhex("a6e8e7cc 25a75f6e 216583f7 ff3dc4cf");
    assert_eq!(g2(&u, &v, &x, &y), 0x2F9ED5BA);
}

// smp_test.py::test_h6
#[test]
fn test_h6() {
    let key = rhex("ec0234a3 57c8ad05 341010a6 0a397d9b");
    let key_id = unhex("6c656272");
    assert_eq!(
        h6(&key, &key_id),
        rhex("2d9ae102 e76dc91c e8d3a9e2 80b16399")
    );
}

// smp_test.py::test_h7
#[test]
fn test_h7() {
    let key = rhex("ec0234a3 57c8ad05 341010a6 0a397d9b");
    let salt = unhex("00000000 00000000 00000000 746D7031");
    assert_eq!(h7(&salt, &key), rhex("fb173597 c6a3c0ec d2998c2a 75a57011"));
}

// smp_test.py::test_ah
#[test]
fn test_ah() {
    let irk = rhex("ec0234a3 57c8ad05 341010a6 0a397d9b");
    let prand = rhex("708194");
    assert_eq!(ah(&irk, &prand), rhex("0dfbaa"));
}
