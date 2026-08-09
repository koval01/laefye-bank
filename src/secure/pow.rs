//! Hashcash-style PoW for channel open (SHA-256 leading zero bits).

use sha2::{Digest, Sha256};

pub fn verify(challenge: &[u8], nonce: u64, difficulty: u32) -> bool {
    let mut hasher = Sha256::new();
    hasher.update(challenge);
    hasher.update(nonce.to_le_bytes());
    let dig = hasher.finalize();
    leading_zero_bits(&dig) >= difficulty
}

pub fn leading_zero_bits(bytes: &[u8]) -> u32 {
    let mut n = 0u32;
    for b in bytes {
        if *b == 0 {
            n += 8;
            continue;
        }
        n += b.leading_zeros();
        break;
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pow_roundtrip() {
        let challenge = b"test-challenge-bytes";
        let difficulty = 12;
        let mut nonce = 0u64;
        loop {
            if verify(challenge, nonce, difficulty) {
                break;
            }
            nonce += 1;
            assert!(nonce < 5_000_000, "pow too hard for test");
        }
        assert!(verify(challenge, nonce, difficulty));
    }
}
