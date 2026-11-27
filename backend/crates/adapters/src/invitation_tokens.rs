//! Production adapters for invitation secrets and time (ADR 008, ADR 017).
//!
//! Hashing and secure randomness live here, behind application-owned ports, because the
//! domain and application crates must stay free of external dependencies. `ring` is already
//! part of the dependency graph (rustls/jsonwebtoken) and provides both a CSPRNG backed by the
//! operating system and SHA-256.
//!
//! None of these types log or print their inputs or outputs.

use std::time::{SystemTime, UNIX_EPOCH};

use einkaufio_application::{
    Clock, InvitationTokenGenerator, InvitationTokenHasher, TokenGenerationError,
};
use einkaufio_domain::{InvitationToken, InvitationTokenHash, Timestamp};
use ring::digest::{SHA256, digest};
use ring::rand::{SecureRandom, SystemRandom};

/// Random bytes per token: 32 bytes = 256 bits of entropy.
pub const TOKEN_BYTES: usize = 32;

// Compile-time guarantee of the >= 256-bit requirement (docs/security.md).
const _: () = assert!(TOKEN_BYTES * 8 >= 256);

/// Generates 256-bit tokens from the operating system CSPRNG, encoded as unpadded base64url
/// (43 URL-safe characters, no escaping needed in an HTTPS path or query).
pub struct SecureInvitationTokenGenerator {
    random: SystemRandom,
}

impl SecureInvitationTokenGenerator {
    pub fn new() -> Self {
        Self {
            random: SystemRandom::new(),
        }
    }
}

impl Default for SecureInvitationTokenGenerator {
    fn default() -> Self {
        Self::new()
    }
}

impl InvitationTokenGenerator for SecureInvitationTokenGenerator {
    fn generate(&self) -> Result<InvitationToken, TokenGenerationError> {
        let mut bytes = [0_u8; TOKEN_BYTES];
        self.random
            .fill(&mut bytes)
            .map_err(|_| TokenGenerationError)?;
        InvitationToken::try_new(base64url(&bytes)).map_err(|_| TokenGenerationError)
    }
}

/// SHA-256 over the token's UTF-8 bytes. Tokens carry 256 bits of entropy, so an unsalted,
/// fast hash is sufficient: the stored hash cannot be brute-forced back to a token, and a
/// deterministic hash allows a direct key lookup (ADR 017).
#[derive(Clone, Copy, Debug, Default)]
pub struct Sha256InvitationTokenHasher;

impl InvitationTokenHasher for Sha256InvitationTokenHasher {
    fn hash(&self, token: &InvitationToken) -> InvitationTokenHash {
        let mut bytes = [0_u8; 32];
        bytes.copy_from_slice(digest(&SHA256, token.expose_secret().as_bytes()).as_ref());
        InvitationTokenHash::from_bytes(bytes)
    }
}

/// Wall-clock time from the operating system. A host clock before the Unix epoch (a
/// misconfiguration) is reported as the epoch; expiry still never exceeds the lifetime
/// relative to the time of creation.
#[derive(Clone, Copy, Debug, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Timestamp {
        let seconds = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_secs());
        Timestamp::from_unix_seconds(seconds)
    }
}

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

/// Unpadded base64url (RFC 4648 §5).
fn base64url(bytes: &[u8]) -> String {
    let mut encoded = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let triple = chunk.iter().enumerate().fold(0_u32, |acc, (index, byte)| {
            acc | (u32::from(*byte) << (16 - 8 * index))
        });
        let symbols = chunk.len() + 1;
        for index in 0..symbols {
            let sextet = (triple >> (18 - 6 * index)) & 0x3f;
            encoded.push(char::from(ALPHABET[sextet as usize]));
        }
    }
    encoded
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn base64url_matches_rfc_4648_vectors_without_padding() {
        assert_eq!(base64url(b""), "");
        assert_eq!(base64url(b"f"), "Zg");
        assert_eq!(base64url(b"fo"), "Zm8");
        assert_eq!(base64url(b"foo"), "Zm9v");
        assert_eq!(base64url(b"foobar"), "Zm9vYmFy");
        assert_eq!(base64url(&[0xfb, 0xff]), "-_8");
    }

    #[test]
    fn generated_tokens_carry_256_bits_and_are_url_safe() {
        let generator = SecureInvitationTokenGenerator::new();
        let token = generator.generate().unwrap();
        // 32 random bytes → 43 base64url symbols, 6 bits each (the last carries 4 bits).
        assert_eq!(token.expose_secret().len(), 43);
        assert!(
            token
                .expose_secret()
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
        );
    }

    #[test]
    fn generated_tokens_do_not_repeat() {
        let generator = SecureInvitationTokenGenerator::new();
        let tokens: HashSet<String> = (0..1_000)
            .map(|_| generator.generate().unwrap().expose_secret().to_owned())
            .collect();
        assert_eq!(tokens.len(), 1_000);
    }

    #[test]
    fn hash_is_sha256_of_the_token() {
        // SHA-256("aaa…a" × 43) computed independently (`printf 'a%.0s' {1..43} | sha256sum`).
        let token = InvitationToken::try_new("a".repeat(43)).unwrap();
        let hash = Sha256InvitationTokenHasher.hash(&token);
        let hex: String = hash
            .as_bytes()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        assert_eq!(hex, EXPECTED_SHA256_OF_43_A);
        assert_eq!(Sha256InvitationTokenHasher.hash(&token), hash);
        assert_ne!(
            Sha256InvitationTokenHasher.hash(&InvitationToken::try_new("b".repeat(43)).unwrap()),
            hash
        );
        assert!(!format!("{hash:?}").contains(&hex[..8]));
    }

    const EXPECTED_SHA256_OF_43_A: &str =
        "66d34fba71f8f450f7e45598853e53bfc23bbd129027cbb131a2f4ffd7878cd0";

    #[test]
    fn system_clock_is_after_2025() {
        assert!(SystemClock.now().as_unix_seconds() > 1_735_689_600);
    }
}
