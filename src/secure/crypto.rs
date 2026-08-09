//! ECDH P-256 + HKDF + AES-GCM + HMAC session keys.

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use hkdf::Hkdf;
use hmac::{Hmac, Mac};
use p256::ecdh::EphemeralSecret;
use p256::elliptic_curve::sec1::{FromEncodedPoint, ToEncodedPoint};
use p256::{EncodedPoint, PublicKey};
use rand::rngs::OsRng;
use rand::RngCore;
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

pub struct SessionKeys {
    pub enc: [u8; 32],
    pub mac: [u8; 32],
}

pub fn generate_server_ephemeral() -> (EphemeralSecret, Vec<u8>) {
    let secret = EphemeralSecret::random(&mut OsRng);
    let pub_bytes = PublicKey::from(&secret)
        .to_encoded_point(false)
        .as_bytes()
        .to_vec();
    (secret, pub_bytes)
}

pub fn parse_client_public(bytes: &[u8]) -> Result<PublicKey, String> {
    let point = EncodedPoint::from_bytes(bytes).map_err(|_| "bad client public key".to_string())?;
    Option::<PublicKey>::from(PublicKey::from_encoded_point(&point))
        .ok_or_else(|| "invalid client public key".into())
}

pub fn derive_session_keys(
    server_secret: EphemeralSecret,
    client_public: &PublicKey,
    channel_id: &str,
    client_binding: &[u8],
) -> SessionKeys {
    let shared = server_secret.diffie_hellman(client_public);
    let hk = Hkdf::<Sha256>::new(Some(channel_id.as_bytes()), shared.raw_secret_bytes());
    let mut info = Vec::with_capacity(client_binding.len() + 16);
    info.extend_from_slice(client_binding);
    info.extend_from_slice(b"laefye-secure-v1");
    let mut okm = [0u8; 64];
    hk.expand(&info, &mut okm).expect("hkdf");
    let mut enc = [0u8; 32];
    let mut mac = [0u8; 32];
    enc.copy_from_slice(&okm[..32]);
    mac.copy_from_slice(&okm[32..]);
    SessionKeys { enc, mac }
}

pub fn encrypt(enc_key: &[u8; 32], plaintext: &[u8]) -> Result<(Vec<u8>, Vec<u8>), String> {
    let cipher = Aes256Gcm::new_from_slice(enc_key).map_err(|e| e.to_string())?;
    let mut nonce_bytes = [0u8; 12];
    OsRng.fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ct = cipher
        .encrypt(nonce, plaintext)
        .map_err(|_| "encrypt failed".to_string())?;
    Ok((nonce_bytes.to_vec(), ct))
}

pub fn decrypt(enc_key: &[u8; 32], nonce: &[u8], ciphertext: &[u8]) -> Result<Vec<u8>, String> {
    if nonce.len() != 12 {
        return Err("bad nonce".into());
    }
    let cipher = Aes256Gcm::new_from_slice(enc_key).map_err(|e| e.to_string())?;
    let nonce = Nonce::from_slice(nonce);
    cipher
        .decrypt(nonce, ciphertext)
        .map_err(|_| "decrypt failed".to_string())
}

pub fn make_proof(
    mac_key: &[u8; 32],
    channel_id: &str,
    counter: u64,
    ts_ms: i64,
    nonce: &[u8],
    ciphertext: &[u8],
) -> Vec<u8> {
    let mut mac = <HmacSha256 as Mac>::new_from_slice(mac_key).expect("hmac key");
    mac.update(channel_id.as_bytes());
    mac.update(&counter.to_le_bytes());
    mac.update(&ts_ms.to_le_bytes());
    mac.update(nonce);
    mac.update(ciphertext);
    mac.finalize().into_bytes().to_vec()
}

pub fn verify_proof(
    mac_key: &[u8; 32],
    channel_id: &str,
    counter: u64,
    ts_ms: i64,
    nonce: &[u8],
    ciphertext: &[u8],
    given: &[u8],
) -> bool {
    let expected = make_proof(mac_key, channel_id, counter, ts_ms, nonce, ciphertext);
    expected.len() == given.len() && constant_eq(&expected, given)
}

fn constant_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut d = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        d |= x ^ y;
    }
    d == 0
}

pub fn random_id() -> String {
    let mut b = [0u8; 16];
    OsRng.fill_bytes(&mut b);
    hex::encode(b)
}
