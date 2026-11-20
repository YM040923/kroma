//! 115's own request cipher, ported from the Go implementation in faststrm
//! (MIT) that 115play carries.
//!
//! The algorithm is not standard in either direction:
//!
//!   encrypt: xor[8d,a5,a5,8d] → reverse → xor[12 bytes] → prepend 16 zero bytes
//!            → PKCS#1 v1.5 with *zero* padding bytes → RSA → base64
//!   decrypt: base64 → RSA → strip to the first zero byte → first 16 bytes seed
//!            a XOR key → xor → reverse → xor[8d,a5,a5,8d]
//!
//! Two details are load-bearing and look like bugs:
//!
//!   - the 16-byte prefix is zeros, not random. The upstream TypeScript leaves
//!     the buffer unfilled and the server accepts it, so randomising it would
//!     make every signature wrong;
//!   - the PKCS#1 padding bytes are all `0x02`, not random. That is what makes
//!     the cipher deterministic, which is what lets the test vectors below pin
//!     the implementation byte for byte.
//!
//! The vectors come from the upstream implementation. They are the only honest
//! test: a wrong step still decrypts with itself, and only the server rejecting
//! the request would say so.

use base64::Engine;
use num_bigint::BigUint;
use num_traits::Zero;

const G_KTS: [u8; 144] = [
    0xf0, 0xe5, 0x69, 0xae, 0xbf, 0xdc, 0xbf, 0x8a, 0x1a, 0x45, 0xe8, 0xbe, 0x7d, 0xa6, 0x73, 0xb8,
    0xde, 0x8f, 0xe7, 0xc4, 0x45, 0xda, 0x86, 0xc4, 0x9b, 0x64, 0x8b, 0x14, 0x6a, 0xb4, 0xf1, 0xaa,
    0x38, 0x01, 0x35, 0x9e, 0x26, 0x69, 0x2c, 0x86, 0x00, 0x6b, 0x4f, 0xa5, 0x36, 0x34, 0x62, 0xa6,
    0x2a, 0x96, 0x68, 0x18, 0xf2, 0x4a, 0xfd, 0xbd, 0x6b, 0x97, 0x8f, 0x4d, 0x8f, 0x89, 0x13, 0xb7,
    0x6c, 0x8e, 0x93, 0xed, 0x0e, 0x0d, 0x48, 0x3e, 0xd7, 0x2f, 0x88, 0xd8, 0xfe, 0xfe, 0x7e, 0x86,
    0x50, 0x95, 0x4f, 0xd1, 0xeb, 0x83, 0x26, 0x34, 0xdb, 0x66, 0x7b, 0x9c, 0x7e, 0x9d, 0x7a, 0x81,
    0x32, 0xea, 0xb6, 0x33, 0xde, 0x3a, 0xa9, 0x59, 0x34, 0x66, 0x3b, 0xaa, 0xba, 0x81, 0x60, 0x48,
    0xb9, 0xd5, 0x81, 0x9c, 0xf8, 0x6c, 0x84, 0x77, 0xff, 0x54, 0x78, 0x26, 0x5f, 0xbe, 0xe8, 0x1e,
    0x36, 0x9f, 0x34, 0x80, 0x5c, 0x45, 0x2c, 0x9b, 0x76, 0xd5, 0x1b, 0x8f, 0xcc, 0xc3, 0xb8, 0xf5,
];

/// 115 calls this `RSAe`, but it is the modulus; `RSAn` is the exponent.
const MODULUS_HEX: &str = "8686980c0f5a24c4b9d43020cd2c22703ff3f450756529058b1cf88f09b8602136477198a6e2683149659bd122c33592fdb5ad47944ad1ea4d36c6b172aad6338c3bb6ac6227502d010993ac967d1aef00f0c8e038de2e4d3bc2ec368af2e9f10a6f1eda4f7262f136420c07c331b871bf139f74f3010e3c4fe57df3afb71683";
const EXPONENT: u32 = 0x10001;

const XOR_KEY_4: [u8; 4] = [0x8d, 0xa5, 0xa5, 0x8d];
const XOR_KEY_12: [u8; 12] = [
    0x78, 0x06, 0xad, 0x4c, 0x33, 0x86, 0x5d, 0x18, 0x4c, 0x01, 0x3f, 0x46,
];

const BLOCK: usize = 128;
const CHUNK: usize = 117;

fn modulus() -> BigUint {
    BigUint::parse_bytes(MODULUS_HEX.as_bytes(), 16).expect("the modulus is valid hex")
}

/// Encrypts a request body the way 115's app API expects it, as base64.
pub fn encrypt(data: &[u8]) -> Result<String, String> {
    if data.is_empty() {
        return Err("empty data".into());
    }

    let inner = xor(data, &XOR_KEY_4);
    let mut reversed = inner;
    reversed.reverse();
    let payload = xor(&reversed, &XOR_KEY_12);

    // A 16-byte zero prefix, not a random key: the upstream leaves this buffer
    // unfilled and the server accepts the result. Randomising it breaks every
    // signature.
    let mut text = vec![0u8; 16];
    text.extend_from_slice(&payload);

    let n = modulus();
    let mut cipher = Vec::with_capacity(text.len().div_ceil(CHUNK) * BLOCK);
    for start in (0..text.len()).step_by(CHUNK) {
        let end = (start + CHUNK).min(text.len());
        let padded = pad_pkcs1(&text[start..end]).ok_or("message too long for a block")?;
        let m = BigUint::from_bytes_be(&padded);
        let c = m.modpow(&BigUint::from(EXPONENT), &n);
        let bytes = c.to_bytes_be();
        // Left-pad to a whole block: the ciphertext of a small m is shorter.
        let mut block = vec![0u8; BLOCK - bytes.len()];
        block.extend_from_slice(&bytes);
        cipher.extend_from_slice(&block);
    }

    Ok(base64::engine::general_purpose::STANDARD.encode(cipher))
}

/// Decrypts a response body from 115, as UTF-8.
pub fn decrypt(cipher_base64: &str) -> Result<String, String> {
    let cipher = base64::engine::general_purpose::STANDARD
        .decode(cipher_base64.trim())
        .map_err(|e| format!("invalid base64: {e}"))?;
    if cipher.is_empty() || cipher.len() % BLOCK != 0 {
        return Err("invalid cipher length".into());
    }

    let n = modulus();
    let mut out = Vec::with_capacity(cipher.len());
    for block in cipher.chunks(BLOCK) {
        let c = BigUint::from_bytes_be(block);
        let m = c.modpow(&BigUint::from(EXPONENT), &n);
        if m.is_zero() {
            return Err("invalid PKCS#1 padding (empty block)".into());
        }
        // `to_bytes_be` drops the leading 0x00 the padding starts with, so the
        // first zero byte is the separator before the message.
        let bytes = m.to_bytes_be();
        let zero = bytes
            .iter()
            .position(|&b| b == 0)
            .ok_or("invalid PKCS#1 padding (no 0x00 separator)")?;
        out.extend_from_slice(&bytes[zero + 1..]);
    }

    if out.len() < 16 {
        return Err("decrypted data too short (missing seed)".into());
    }
    let key = gen_key(&out[..16], 12);
    let mut tmp = xor(&out[16..], &key);
    tmp.reverse();
    let plain = xor(&tmp, &XOR_KEY_4);
    String::from_utf8(plain).map_err(|e| format!("decrypted data is not UTF-8: {e}"))
}

/// The per-response XOR key 115 derives from the 16-byte seed. Ported exactly:
/// the index arithmetic is the upstream's, not a cleaner reformulation. The
/// running index is signed because its last decrement lands on zero and the
/// upstream simply lets it go negative unwritten.
fn gen_key(seed: &[u8], len: usize) -> Vec<u8> {
    let mut key = vec![0u8; len];
    let mut length = (len * (len - 1)) as isize;
    let mut index = 0usize;
    for i in 0..len {
        let x = seed[i].wrapping_add(G_KTS[index]);
        key[i] = G_KTS[length as usize] ^ x;
        length -= len as isize;
        index += len;
    }
    key
}

/// PKCS#1 v1.5, type 2, with `0x02` fill instead of random bytes — the
/// upstream's shape, and what makes the cipher deterministic.
fn pad_pkcs1(msg: &[u8]) -> Option<Vec<u8>> {
    if msg.len() > BLOCK - 11 {
        return None;
    }
    let mut buf = vec![0u8; BLOCK];
    let fill_end = (BLOCK - 1) - msg.len();
    for byte in buf.iter_mut().take(fill_end).skip(1) {
        *byte = 0x02;
    }
    buf[BLOCK - msg.len()..].copy_from_slice(msg);
    Some(buf)
}

/// The upstream's XOR, ported statement for statement. Its pattern restarts at
/// the first chunk boundary rather than running modulo the key length, so a
/// "cleaner" rewrite would diverge for inputs whose length is not a multiple of
/// the key length.
fn xor(src: &[u8], key: &[u8]) -> Vec<u8> {
    if src.is_empty() || key.is_empty() {
        return src.to_vec();
    }
    let mut out = vec![0u8; src.len()];
    let head = src.len() & 3;
    for i in 0..head {
        out[i] = src[i] ^ key[i % key.len()];
    }
    let mut i = head;
    while i < src.len() {
        let end = (i + key.len()).min(src.len());
        for j in i..end {
            out[j] = src[j] ^ key[j - i];
        }
        i += key.len();
    }
    out
}

// ---------------------------------------------------------------------------
// Upload handshake (`uplb.115.com/4.0/initupload.php`). The wire formats are
// 115's; this is a clean reimplementation, no reference code is linked.
//
// AES-CBC with a fixed key and IV; the padding is only the short form (a full
// 16-byte block is never appended), and a response body is AES followed by LZ4
// chunks, each prefixed with its little-endian length.

use aes::Aes128;
use cbc::cipher::{block_padding::NoPadding, BlockDecryptMut, BlockEncryptMut, KeyIvInit};
use md5::Md5;
use sha1::Sha1;
use sha1::Digest as _;

const UP_AES_KEY: [u8; 16] = [
    0xfb, 0x1a, 0x19, 0xd6, 0x52, 0xf5, 0xaa, 0xf7, 0xbc, 0x65, 0x1d, 0x0f, 0x69, 0xbf, 0x42, 0x2f,
];
const UP_AES_IV: [u8; 16] = [
    0x69, 0xbf, 0x42, 0x2f, 0x49, 0x96, 0x05, 0x50, 0xa0, 0xad, 0x44, 0xec, 0x34, 0x46, 0xcb, 0x4c,
];
const UP_PUBKEY: [u8; 30] = [
    0x1d, 0x03, 0x0e, 0x80, 0xa1, 0x78, 0xdc, 0xee, 0xce, 0xcd, 0xa3, 0x77, 0xde, 0x12, 0x8d, 0x8e,
    0xd9, 0xdd, 0xcf, 0x55, 0xae, 0x61, 0xed, 0x46, 0xea, 0x12, 0x1a, 0x1c, 0xfc, 0x81,
];
const CRC_SALT: &[u8] = b"^j>WD3Kr?J2gLFjD4W2y@";
const MD5_SALT: &[u8] = b"Qclm8MGWUv59TnrR0XPg";

fn upload_pad(data: &[u8]) -> Vec<u8> {
    let short = (16 - data.len() % 16) % 16;
    let mut out = data.to_vec();
    out.extend(std::iter::repeat(short as u8).take(short));
    out
}

fn upload_unpad(mut data: Vec<u8>) -> Vec<u8> {
    if let Some(&last) = data.last() {
        let size = last as usize;
        if (1..16).contains(&size)
            && data.len() >= size
            && data[data.len() - size..].iter().all(|byte| *byte == last)
        {
            data.truncate(data.len() - size);
        }
    }
    data
}

type Aes128CbcEnc = cbc::Encryptor<Aes128>;
type Aes128CbcDec = cbc::Decryptor<Aes128>;

pub(crate) fn aes_encrypt(data: &[u8]) -> Vec<u8> {
    Aes128CbcEnc::new((&UP_AES_KEY).into(), (&UP_AES_IV).into())
        .encrypt_padded_vec_mut::<NoPadding>(&upload_pad(data))
}

pub(crate) fn aes_decrypt(data: &[u8]) -> Vec<u8> {
    let whole = data.len() & !15;
    Aes128CbcDec::new((&UP_AES_KEY).into(), (&UP_AES_IV).into())
        .decrypt_padded_vec_mut::<NoPadding>(&data[..whole])
        .map(upload_unpad)
        .unwrap_or_default()
}

/// AES then the chunked LZ4 the upload endpoint answers with.
pub(crate) fn upload_response(data: &[u8]) -> Vec<u8> {
    let plain = aes_decrypt(data);
    let mut src = plain.as_slice();
    let mut out: Vec<u8> = Vec::new();
    while src.len() > 2 {
        let length = u16::from_le_bytes([src[0], src[1]]) as usize;
        if length == 0 || src.len() < length + 2 {
            break;
        }
        let mut chunk = vec![0u8; 0x2000];
        if let Ok(written) = lz4_flex::block::decompress_into(&src[2..2 + length], &mut chunk) {
            out.extend_from_slice(&chunk[..written]);
        }
        src = &src[2 + length..];
    }
    out
}

/// The `k_ec` query value: 115's public key around the timestamp, with a CRC.
pub(crate) fn ec_token(timestamp: u32) -> String {
    let mut token: Vec<u8> = Vec::with_capacity(46);
    token.extend_from_slice(&UP_PUBKEY[..15]);
    token.extend_from_slice(&[0x00, 0x73, 0x00, 0x00, 0x00]);
    token.extend_from_slice(&timestamp.to_le_bytes());
    token.extend_from_slice(&UP_PUBKEY[15..]);
    token.extend_from_slice(&[0x00, 0x01, 0x00, 0x00, 0x00]);
    let crc = crc32fast::hash(&[CRC_SALT, token.as_slice()].concat());
    token.extend_from_slice(&crc.to_le_bytes());
    base64::engine::general_purpose::STANDARD.encode(token)
}

pub(crate) struct UploadInit<'a> {
    pub userkey: &'a str,
    pub userid: &'a str,
    pub fileid: &'a str,
    pub filename: &'a str,
    pub filesize: u64,
    pub target: &'a str,
    pub sign_key: &'a str,
    pub sign_val: &'a str,
    pub appversion: &'a str,
}

/// The signed, encrypted request: `(k_ec, body)`.
pub(crate) fn upload_payload(init: &UploadInit<'_>) -> (String, Vec<u8>) {
    let t = now_secs().to_string();
    let filesize = init.filesize.to_string();
    let mut sig = Sha1::new();
    sig.update(init.userkey.as_bytes());
    sig.update(
        hex::encode(Sha1::digest(
            format!("{}{}{}0", init.userid, init.fileid, init.target).as_bytes(),
        ))
        .as_bytes(),
    );
    sig.update(b"000000");
    let signature = hex::encode(sig.finalize()).to_uppercase();

    let mut token = Md5::new();
    token.update(MD5_SALT);
    token.update(
        format!(
            "{}{}{}{}{}{}",
            init.fileid, filesize, init.sign_key, init.sign_val, init.userid, t
        )
        .as_bytes(),
    );
    token.update(hex::encode(Md5::digest(init.userid.as_bytes())).as_bytes());
    token.update(init.appversion.as_bytes());
    let token = hex::encode(token.finalize());

    // Empty fields (and the numeric 0 appid) are omitted, then sorted by key.
    let mut fields: Vec<(String, String)> = vec![
        ("appversion".into(), init.appversion.into()),
        ("topupload".into(), "true".into()),
        ("target".into(), init.target.into()),
        ("sign_key".into(), init.sign_key.into()),
        ("sign_val".into(), init.sign_val.into()),
        ("fileid".into(), init.fileid.into()),
        ("filename".into(), init.filename.into()),
        ("filesize".into(), filesize),
        ("path".into(), String::new()),
        ("userid".into(), init.userid.into()),
        ("userkey".into(), init.userkey.into()),
        ("t".into(), t),
        ("sig".into(), signature),
        ("token".into(), token),
    ];
    fields.retain(|(_, value)| !value.is_empty() && value != "0");
    fields.sort_by(|a, b| a.0.cmp(&b.0));
    let body = fields
        .iter()
        .map(|(key, value)| {
            format!(
                "{}={}",
                crate::client::form_encode(key),
                crate::client::form_encode(value)
            )
        })
        .collect::<Vec<_>>()
        .join("&");
    (ec_token(now_secs() as u32), aes_encrypt(body.as_bytes()))
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|since| since.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Outputs the upstream implementation produced for the same input. Any
    /// step in the wrong order still round-trips with itself, so matching these
    /// byte for byte is the only test that means anything.
    const VECTORS: &[(&str, &str)] = &[
        (
            "ping",
            "NoAw8uf5gu9No+UqEBEJhP2AR1WjBlRso2nWEKVOcvu04zcxl40vK+kzEpQfDlpuLPa9ajfnTj3KgU30BKB93D3DM7P7tF5DQD3TvLfxoy218qPLzt3IwsR8cQ0BQdcoB1a1csXzvVGCe1RmtpEVOHiAEVZ+pevwEjTJFESiLAQ=",
        ),
        (
            "hello 115",
            "CYKPukbISRZ6edZsO4u4mIkBgeekuVgx3T8t1486hoKhs1nl5ZTxx86eg1sdyfSIA30V795TX6jMQS4iJstgiQVu87G5XyjbbuKnMvNcAuNIGYov8EQgd0YGsmRdAIB91/G1H9kJsnyxfpjSKh97rQFSH3ziAgcmsRnChW5SyAQ=",
        ),
        (
            r#"{"pick_code":"abcdefg"}"#,
            "D53UO9AcvuIjiurn5w6EKfpHoBu8mKkhKfzk6bxOtaz/f/4dIsR3plk8ioALTyzE1k8BD3Iee1jRl+rxbYQpzFwrID/r4VkJ/M2Jf+FuxDXi/S07KCGkuYCV0D8HnCH+79t7HjZkjkVf+GlErwT/tkraW/rBsWFwXH8GZl8m3EA=",
        ),
    ];

    #[test]
    fn encrypt_matches_the_upstream_bytes() {
        for (plain, want) in VECTORS {
            let got = encrypt(plain.as_bytes()).expect("encrypt");
            assert_eq!(&got, want, "ciphertext differs for {plain:?}");
        }
    }

    #[test]
    fn ciphertext_is_whole_rsa_blocks() {
        for len in [1usize, 100, 117, 118, 200, 300] {
            let data: Vec<u8> = (1..=len).map(|i| i as u8).collect();
            let enc = encrypt(&data).expect("encrypt");
            let raw = base64::engine::general_purpose::STANDARD
                .decode(&enc)
                .expect("base64");
            let text_len = 16 + len;
            let chunks = text_len.div_ceil(CHUNK);
            assert_eq!(raw.len(), chunks * BLOCK, "len={len}");
        }
    }

    #[test]
    fn xor_is_its_own_inverse() {
        let src = b"hello world, 115";
        let once = xor(src, &XOR_KEY_4);
        let twice = xor(&once, &XOR_KEY_4);
        assert_eq!(&twice[..], src);
    }

    #[test]
    fn gen_key_is_deterministic_and_twelve_bytes() {
        let seed: Vec<u8> = (0..16).collect();
        let a = gen_key(&seed, 12);
        let b = gen_key(&seed, 12);
        assert_eq!(a, b);
        assert_eq!(a.len(), 12);
    }

    #[test]
    fn padding_puts_the_separator_right_before_the_message() {
        let buf = pad_pkcs1(b"abc").expect("pad");
        assert_eq!(buf.len(), BLOCK);
        assert_eq!(buf[0], 0x00);
        assert_eq!(buf[1], 0x02);
        assert_eq!(&buf[BLOCK - 3..], b"abc");
        assert_eq!(buf[BLOCK - 4], 0x00, "the separator sits before the message");
    }

    /// What can be tested without the drive: that the shared primitives the
    /// two directions rely on are the upstream's. A round trip through our own
    /// ciphertext proves nothing — 115 encrypts responses with its private
    /// exponent, so applying the public one twice is not the identity, and a
    /// "round trip" test would be checking a property the drive does not have.
    /// Decrypt is exercised by real responses only.
    ///
    /// The expected key is derived by hand from the upstream's index arithmetic
    /// (length starts at `len*(len-1)` and walks down by `len` while the table
    /// index walks up), so it pins this port against a "tidier" rewrite of that
    /// loop. It cannot prove decrypt is right; only a real response can.
    #[test]
    fn the_shared_primitives_stay_pinned() {
        let seed: Vec<u8> = (0..16).collect();
        let key = gen_key(&seed, 12);
        assert_eq!(
            key,
            vec![0xac, 0x81, 0x27, 0x1b, 0xc5, 0x43, 0x52, 0xd8, 0x1c, 0x58, 0x74, 0x97],
            "the derived key moved"
        );
    }
}
