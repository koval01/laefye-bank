//! Deterministic TRON address derivation from a master seed.

use hmac::{Hmac, Mac};
use secp256k1::{PublicKey, Secp256k1, SecretKey};
use sha2::{Digest, Sha256};
use sha3::Keccak256;
use sha3::Digest as KeccakDigest;
use thiserror::Error;

type HmacSha512 = Hmac<Sha512>;
use sha2::Sha512;

#[derive(Debug, Error)]
pub enum AddressError {
    #[error("invalid seed")]
    InvalidSeed,
    #[error("secp256k1: {0}")]
    Secp(String),
}

#[derive(Debug, Clone)]
pub struct TronKey {
    pub address: String,
    pub private_key: [u8; 32],
}

/// Hot wallet = derivation index 0.
pub fn hot_wallet_address(seed: &[u8; 32]) -> Result<TronKey, AddressError> {
    derive_deposit_address(seed, 0)
}

/// Derive a TRON account from HMAC-SHA512(seed, "laefye/tron/{index}").
/// Not BIP-44, but deterministic and good enough for custodial deposit addresses.
pub fn derive_deposit_address(seed: &[u8; 32], index: u32) -> Result<TronKey, AddressError> {
    let mut mac =
        HmacSha512::new_from_slice(seed).map_err(|_| AddressError::InvalidSeed)?;
    mac.update(format!("laefye/tron/{index}").as_bytes());
    let result = mac.finalize().into_bytes();
    let mut sk_bytes = [0u8; 32];
    sk_bytes.copy_from_slice(&result[..32]);

    // Ensure valid secp256k1 scalar (retry with hash if needed — extremely rare).
    let secp = Secp256k1::new();
    let mut sk = SecretKey::from_slice(&sk_bytes);
    let mut n = 0u32;
    while sk.is_err() && n < 8 {
        let mut h = Sha256::new();
        h.update(sk_bytes);
        h.update(n.to_le_bytes());
        sk_bytes = h.finalize().into();
        sk = SecretKey::from_slice(&sk_bytes);
        n += 1;
    }
    let sk = sk.map_err(|e| AddressError::Secp(e.to_string()))?;
    let pk = PublicKey::from_secret_key(&secp, &sk);
    let address = pubkey_to_tron_address(&pk);
    Ok(TronKey {
        address,
        private_key: sk.secret_bytes(),
    })
}

fn pubkey_to_tron_address(pk: &PublicKey) -> String {
    // Uncompressed pubkey without 0x04 prefix → keccak → last 20 bytes → 0x41 + base58check
    let uncompressed = pk.serialize_uncompressed();
    let mut hasher = Keccak256::new();
    hasher.update(&uncompressed[1..]);
    let hash = KeccakDigest::finalize(hasher);
    let mut raw = [0u8; 21];
    raw[0] = 0x41;
    raw[1..].copy_from_slice(&hash[12..]);
    base58check_encode(&raw)
}

fn base58check_encode(payload: &[u8]) -> String {
    let first = Sha256::digest(payload);
    let second = Sha256::digest(&first);
    let mut with_checksum = Vec::with_capacity(payload.len() + 4);
    with_checksum.extend_from_slice(payload);
    with_checksum.extend_from_slice(&second[..4]);
    bs58::encode(with_checksum).into_string()
}

pub fn is_valid_tron_address(addr: &str) -> bool {
    if !(addr.starts_with('T') && addr.len() >= 30 && addr.len() <= 36) {
        return false;
    }
    let Ok(bytes) = bs58::decode(addr).into_vec() else {
        return false;
    };
    if bytes.len() != 25 || bytes[0] != 0x41 {
        return false;
    }
    let (payload, checksum) = bytes.split_at(21);
    let first = Sha256::digest(payload);
    let second = Sha256::digest(&first);
    checksum == &second[..4]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derives_stable_t_address() {
        let seed = [7u8; 32];
        let a = derive_deposit_address(&seed, 1).unwrap();
        let b = derive_deposit_address(&seed, 1).unwrap();
        assert_eq!(a.address, b.address);
        assert!(a.address.starts_with('T'));
        assert!(is_valid_tron_address(&a.address));
        let c = derive_deposit_address(&seed, 2).unwrap();
        assert_ne!(a.address, c.address);
    }
}
