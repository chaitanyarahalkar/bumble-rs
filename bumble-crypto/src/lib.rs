//! bumble-crypto — a Rust port of the SMP cryptographic toolbox from
//! [`google/bumble`](https://github.com/google/bumble) (Vol 3, Part H - 2.2).
//!
//! **Slice 6** of the incremental port: the security functions used by the
//! Security Manager Protocol — the block function `e`, `aes_cmac` (RFC 4493
//! AES-CMAC), the LE Legacy functions `c1`/`s1`/`ah`, and the LE Secure
//! Connections functions `f4`/`f5`/`f6`/`g2`/`h6`/`h7`.
//!
//! AES-128 comes from the audited `aes` crate; CMAC and everything above it are
//! implemented here. Correctness is pinned to the published Bluetooth-spec and
//! RFC 4493 test vectors.
//!
//! Byte-order note (matching Bumble): `e` and the legacy functions work in
//! little-endian and swap internally; `aes_cmac` works in big-endian; the
//! Secure-Connections functions `reverse()` their inputs/outputs around
//! `aes_cmac`.
//!
//! P-256 ECC key agreement ([`EccKey`], added in slice 19 for LE Secure
//! Connections) wraps the audited RustCrypto `p256` crate; it mirrors upstream
//! `crypto.EccKey` (big-endian `x`/`y`, and a `dh` that returns the shared
//! secret's X coordinate).

use aes::cipher::generic_array::GenericArray;
use aes::cipher::{BlockEncrypt, KeyInit};
use aes::Aes128;

/// AES-128 encryption of a single 16-byte block.
fn aes_encrypt_block(key: &[u8; 16], block: &[u8; 16]) -> [u8; 16] {
    let cipher = Aes128::new(GenericArray::from_slice(key));
    let mut b = *GenericArray::from_slice(block);
    cipher.encrypt_block(&mut b);
    let mut out = [0u8; 16];
    out.copy_from_slice(&b);
    out
}

/// XOR two equal-length byte slices.
fn xor(x: &[u8], y: &[u8]) -> Vec<u8> {
    assert_eq!(x.len(), y.len(), "xor: length mismatch");
    x.iter().zip(y).map(|(a, b)| a ^ b).collect()
}

/// Bytes in reversed order (endianness swap).
fn reverse(input: &[u8]) -> Vec<u8> {
    input.iter().rev().copied().collect()
}

fn to_array16(data: &[u8]) -> [u8; 16] {
    let mut out = [0u8; 16];
    out.copy_from_slice(data);
    out
}

/// The security function `e`: AES-128 with byte-swapped key, input, and output
/// (Vol 3, Part H - 2.2.1). Both `key` and `data` must be 16 bytes.
pub fn e(key: &[u8], data: &[u8]) -> Vec<u8> {
    let rk = to_array16(&reverse(key));
    let rd = to_array16(&reverse(data));
    reverse(&aes_encrypt_block(&rk, &rd))
}

/// Left-shift a 128-bit big-endian value by one bit, applying the CMAC
/// polynomial reduction (Rb = 0x87) when the high bit was set.
fn cmac_double(input: [u8; 16]) -> [u8; 16] {
    let msb = input[0] >> 7;
    let mut out = [0u8; 16];
    let mut carry = 0u8;
    for i in (0..16).rev() {
        out[i] = (input[i] << 1) | carry;
        carry = input[i] >> 7;
    }
    if msb == 1 {
        out[15] ^= 0x87;
    }
    out
}

/// AES-CMAC (RFC 4493 / Vol 3, Part H - 2.2.5). Big-endian input and output.
/// `k` must be 16 bytes.
pub fn aes_cmac(m: &[u8], k: &[u8]) -> [u8; 16] {
    let key = to_array16(k);

    // Subkey generation.
    let l = aes_encrypt_block(&key, &[0u8; 16]);
    let k1 = cmac_double(l);
    let k2 = cmac_double(k1);

    let n = m.len().div_ceil(16).max(1);
    let last_is_complete = !m.is_empty() && m.len().is_multiple_of(16);

    // Process all but the last block.
    let mut x = [0u8; 16];
    for i in 0..n - 1 {
        let block = &m[i * 16..i * 16 + 16];
        for j in 0..16 {
            x[j] ^= block[j];
        }
        x = aes_encrypt_block(&key, &x);
    }

    // The last block, padded and XORed with the appropriate subkey.
    let start = (n - 1) * 16;
    let rem = &m[start..];
    let mut last = [0u8; 16];
    if last_is_complete {
        for j in 0..16 {
            last[j] = rem[j] ^ k1[j];
        }
    } else {
        last[..rem.len()].copy_from_slice(rem);
        last[rem.len()] = 0x80;
        for j in 0..16 {
            last[j] ^= k2[j];
        }
    }

    for j in 0..16 {
        x[j] ^= last[j];
    }
    aes_encrypt_block(&key, &x)
}

/// Random Address Hash function `ah` (Vol 3, Part H - 2.2.2). Returns 3 bytes.
pub fn ah(k: &[u8], r: &[u8]) -> Vec<u8> {
    let mut r_prime = r.to_vec();
    r_prime.resize(16, 0);
    e(k, &r_prime)[..3].to_vec()
}

/// LE Legacy confirm value function `c1` (Vol 3, Part H - 2.2.3).
#[allow(clippy::too_many_arguments)]
pub fn c1(
    k: &[u8],
    r: &[u8],
    preq: &[u8],
    pres: &[u8],
    iat: u8,
    rat: u8,
    ia: &[u8],
    ra: &[u8],
) -> Vec<u8> {
    let mut p1 = vec![iat, rat];
    p1.extend_from_slice(preq);
    p1.extend_from_slice(pres);

    let mut p2 = ra.to_vec();
    p2.extend_from_slice(ia);
    p2.extend_from_slice(&[0, 0, 0, 0]);

    e(k, &xor(&e(k, &xor(r, &p1)), &p2))
}

/// LE Legacy key generation function `s1` (Vol 3, Part H - 2.2.4).
pub fn s1(k: &[u8], r1: &[u8], r2: &[u8]) -> Vec<u8> {
    let mut data = r2[..8].to_vec();
    data.extend_from_slice(&r1[..8]);
    e(k, &data)
}

/// LE Secure Connections confirm value function `f4` (Vol 3, Part H - 2.2.6).
pub fn f4(u: &[u8], v: &[u8], x: &[u8], z: u8) -> Vec<u8> {
    let mut m = reverse(u);
    m.extend_from_slice(&reverse(v));
    m.push(z);
    reverse(&aes_cmac(&m, &reverse(x)))
}

/// LE Secure Connections key generation function `f5` (Vol 3, Part H - 2.2.7).
/// Returns `(MacKey, LTK)` in little-endian byte order.
pub fn f5(w: &[u8], n1: &[u8], n2: &[u8], a1: &[u8], a2: &[u8]) -> (Vec<u8>, Vec<u8>) {
    const SALT: [u8; 16] = [
        0x6C, 0x88, 0x83, 0x91, 0xAA, 0xF5, 0xA5, 0x38, 0x60, 0x37, 0x0B, 0xDB, 0x5A, 0x60, 0x83,
        0xBE,
    ];
    let t = aes_cmac(&reverse(w), &SALT);
    let key_id = [0x62u8, 0x74, 0x6C, 0x65];

    let build = |counter: u8| -> Vec<u8> {
        let mut m = vec![counter];
        m.extend_from_slice(&key_id);
        m.extend_from_slice(&reverse(n1));
        m.extend_from_slice(&reverse(n2));
        m.extend_from_slice(&reverse(a1));
        m.extend_from_slice(&reverse(a2));
        m.extend_from_slice(&[1, 0]);
        reverse(&aes_cmac(&m, &t))
    };

    (build(0), build(1))
}

/// LE Secure Connections check value function `f6` (Vol 3, Part H - 2.2.8).
#[allow(clippy::too_many_arguments)]
pub fn f6(
    w: &[u8],
    n1: &[u8],
    n2: &[u8],
    r: &[u8],
    io_cap: &[u8],
    a1: &[u8],
    a2: &[u8],
) -> Vec<u8> {
    let mut m = reverse(n1);
    m.extend_from_slice(&reverse(n2));
    m.extend_from_slice(&reverse(r));
    m.extend_from_slice(&reverse(io_cap));
    m.extend_from_slice(&reverse(a1));
    m.extend_from_slice(&reverse(a2));
    reverse(&aes_cmac(&m, &reverse(w)))
}

/// LE Secure Connections numeric comparison value function `g2`
/// (Vol 3, Part H - 2.2.9). Returns the low 32 bits (big-endian) of the MAC.
pub fn g2(u: &[u8], v: &[u8], x: &[u8], y: &[u8]) -> u32 {
    let mut m = reverse(u);
    m.extend_from_slice(&reverse(v));
    m.extend_from_slice(&reverse(y));
    let mac = aes_cmac(&m, &reverse(x));
    u32::from_be_bytes([mac[12], mac[13], mac[14], mac[15]])
}

/// Link key conversion function `h6` (Vol 3, Part H - 2.2.10).
pub fn h6(w: &[u8], key_id: &[u8]) -> Vec<u8> {
    reverse(&aes_cmac(key_id, &reverse(w)))
}

/// Link key conversion function `h7` (Vol 3, Part H - 2.2.11).
pub fn h7(salt: &[u8], w: &[u8]) -> Vec<u8> {
    reverse(&aes_cmac(&reverse(w), salt))
}

use p256::elliptic_curve::sec1::{FromEncodedPoint, ToEncodedPoint};
use p256::{EncodedPoint, PublicKey, SecretKey};

/// Errors from ECC key operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    InvalidKey(String),
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Error::InvalidKey(m) => write!(f, "invalid ECC key: {m}"),
        }
    }
}

impl std::error::Error for Error {}

/// A P-256 (secp256r1) key pair for LE Secure Connections ECDH, mirroring
/// upstream `crypto.EccKey`.
///
/// Coordinates and the DH result are **big-endian**, matching upstream (SMP
/// then byte-swaps them to little-endian for the wire and for `f4`/`f5`/`f6`).
pub struct EccKey {
    secret: SecretKey,
}

impl EccKey {
    /// Generate a fresh random key pair from the OS RNG.
    pub fn generate() -> EccKey {
        EccKey {
            secret: SecretKey::random(&mut rand_core::OsRng),
        }
    }

    /// Build a key pair from a 32-byte big-endian private scalar.
    pub fn from_private_key_bytes(d_be: &[u8]) -> core::result::Result<EccKey, Error> {
        let secret = SecretKey::from_slice(d_be)
            .map_err(|e| Error::InvalidKey(format!("private scalar: {e}")))?;
        Ok(EccKey { secret })
    }

    fn encoded_public(&self) -> EncodedPoint {
        self.secret.public_key().to_encoded_point(false)
    }

    /// The public key's X coordinate, big-endian (32 bytes).
    pub fn public_x(&self) -> [u8; 32] {
        let ep = self.encoded_public();
        let mut out = [0u8; 32];
        out.copy_from_slice(ep.x().expect("uncompressed point has an X").as_slice());
        out
    }

    /// The public key's Y coordinate, big-endian (32 bytes).
    pub fn public_y(&self) -> [u8; 32] {
        let ep = self.encoded_public();
        let mut out = [0u8; 32];
        out.copy_from_slice(ep.y().expect("uncompressed point has a Y").as_slice());
        out
    }

    /// ECDH: the shared secret's X coordinate (big-endian, 32 bytes), from a
    /// peer public key given as big-endian X and Y (Vol 3, Part H - 2.3.5.6).
    pub fn dh(&self, peer_x_be: &[u8], peer_y_be: &[u8]) -> core::result::Result<[u8; 32], Error> {
        if peer_x_be.len() != 32 || peer_y_be.len() != 32 {
            return Err(Error::InvalidKey(
                "peer coordinates must be 32 bytes".into(),
            ));
        }
        let x = p256::FieldBytes::clone_from_slice(peer_x_be);
        let y = p256::FieldBytes::clone_from_slice(peer_y_be);
        let ep = EncodedPoint::from_affine_coordinates(&x, &y, false);
        let peer = Option::<PublicKey>::from(PublicKey::from_encoded_point(&ep))
            .ok_or_else(|| Error::InvalidKey("peer point not on curve".into()))?;
        let shared = p256::ecdh::diffie_hellman(self.secret.to_nonzero_scalar(), peer.as_affine());
        let mut out = [0u8; 32];
        out.copy_from_slice(shared.raw_secret_bytes().as_slice());
        Ok(out)
    }
}
