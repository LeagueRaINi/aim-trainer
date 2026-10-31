//! Release signatures: every `aim_trainer.exe.zst` is signed offline with the maintainer's
//! Ed25519 key, so a release published by anyone else (a leaked token, a compromised build) is
//! never installed. The private key never touches CI; `src/bin/release.rs` signs with it.
//!
//! Shared by the app (`update.rs`) and the release tool, which includes this file by path.

use ed25519_dalek::{Signature, VerifyingKey};

/// The public half of the release signing key.
pub const PUBLIC_KEY: [u8; 32] = [
    0x56, 0x70, 0xd8, 0xcf, 0xdf, 0xe5, 0xa4, 0x57, 0xa5, 0x36, 0x79, 0xdd, 0x15, 0x8a, 0x36, 0x0b, 0x54, 0x99, 0x90,
    0x18, 0xc4, 0x2d, 0x60, 0x06, 0x39, 0x6a, 0x15, 0x78, 0xe4, 0x40, 0x66, 0x37,
];

/// Whether `signature` (the `.sig` asset: 128 hex chars, optionally followed by a newline) is the
/// release key's signature of `data`.
pub fn verify(data: &[u8], signature: &str) -> bool {
    let hex = signature.strip_suffix('\n').unwrap_or(signature);
    let hex = hex.strip_suffix('\r').unwrap_or(hex);
    let Some(signature) = decode_hex::<64>(hex) else { return false };
    let Ok(key) = VerifyingKey::from_bytes(&PUBLIC_KEY) else { return false };
    // Strict: also rejects malleable and weak-key signatures, which a real release never has.
    key.verify_strict(data, &Signature::from_bytes(&signature)).is_ok()
}

/// Exactly `2 * N` hex digits (either case) as bytes.
pub fn decode_hex<const N: usize>(hex: &str) -> Option<[u8; N]> {
    if hex.len() != 2 * N || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let mut bytes = [0; N];
    for (byte, pair) in bytes.iter_mut().zip(hex.as_bytes().as_chunks::<2>().0) {
        *byte = u8::from_str_radix(std::str::from_utf8(pair).ok()?, 16).ok()?;
    }
    Some(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The release key's signature of the 4 bytes `test`.
    const TEST_SIGNATURE: &str = "ba5e89840eff04bc88b1a1f661e4858c8aca48380d95d6f62398db5f3db8c99e0464fd5b6b732a2062450bf9b0de7e330a61f720bc65784e637341afa9bbb601";

    #[test]
    fn accepts_only_the_release_keys_signature() {
        assert!(verify(b"test", TEST_SIGNATURE));
        assert!(verify(b"test", &format!("{TEST_SIGNATURE}\n")));
        assert!(verify(b"test", &format!("{TEST_SIGNATURE}\r\n")));
        // Other data, a flipped bit, a truncated or padded signature, or no hex at all.
        assert!(!verify(b"tesT", TEST_SIGNATURE));
        let mut flipped = decode_hex::<64>(TEST_SIGNATURE).unwrap();
        flipped[10] ^= 1;
        let flipped: String = flipped.iter().map(|byte| format!("{byte:02x}")).collect();
        assert!(!verify(b"test", &flipped));
        assert!(!verify(b"test", &TEST_SIGNATURE[..126]));
        assert!(!verify(b"test", &format!(" {TEST_SIGNATURE}")));
        assert!(!verify(b"test", ""));
        assert!(!verify(b"test", &"zz".repeat(64)));
    }

    #[test]
    fn decodes_exact_length_hex_only() {
        assert_eq!(decode_hex::<2>("0aFf"), Some([0x0a, 0xff]));
        assert_eq!(decode_hex::<2>("0aF"), None);
        assert_eq!(decode_hex::<2>("0aFf0"), None);
        // `from_str_radix` alone would take the sign.
        assert_eq!(decode_hex::<2>("+aff"), None);
    }
}
