/// AES-256-GCM encryption for provider API keys.
///
/// Key is derived from the app's `session_secret` via HKDF-SHA256 with
/// salt `"provider-key-v1"`. Nonce is random per encrypt, prepended to
/// ciphertext: `nonce(12) || ciphertext`, then base64-encoded.
///
/// Rotating `session_secret` invalidates all stored API keys.
use aes_gcm::{
    aead::{Aead, AeadCore, KeyInit, OsRng},
    Aes256Gcm, Key, Nonce,
};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use hkdf::Hkdf;
use sha2::Sha256;

const SALT: &[u8] = b"provider-key-v1";

/// Derive a 32-byte AES key from the app secret using HKDF-SHA256.
fn derive_key(app_secret: &str) -> Result<[u8; 32], anyhow::Error> {
    let hk = Hkdf::<Sha256>::new(Some(SALT), app_secret.as_bytes());
    let mut okm = [0u8; 32];
    hk.expand(&[], &mut okm)
        .map_err(|_| anyhow::anyhow!("HKDF expand failed"))?;
    Ok(okm)
}

/// Encrypt a plaintext API key. Returns base64-encoded `nonce || ciphertext`.
pub fn encrypt_api_key(plaintext: &str, app_secret: &str) -> Result<String, anyhow::Error> {
    let key_bytes = derive_key(app_secret)?;
    let key = Key::<Aes256Gcm>::from_slice(&key_bytes);
    let cipher = Aes256Gcm::new(key);

    let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
    let ciphertext = cipher
        .encrypt(&nonce, plaintext.as_bytes())
        .map_err(|_| anyhow::anyhow!("AES-GCM encrypt failed"))?;

    // Prepend nonce (12 bytes) to ciphertext
    let mut combined = Vec::with_capacity(12 + ciphertext.len());
    combined.extend_from_slice(&nonce);
    combined.extend_from_slice(&ciphertext);

    Ok(B64.encode(&combined))
}

/// Decrypt a base64-encoded `nonce || ciphertext`. Returns the plaintext.
pub fn decrypt_api_key(ciphertext_b64: &str, app_secret: &str) -> Result<String, anyhow::Error> {
    let combined = B64
        .decode(ciphertext_b64)
        .map_err(|e| anyhow::anyhow!("base64 decode failed: {e}"))?;

    if combined.len() < 12 {
        return Err(anyhow::anyhow!("ciphertext too short"));
    }

    let (nonce_bytes, ct) = combined.split_at(12);
    let nonce = Nonce::from_slice(nonce_bytes);

    let key_bytes = derive_key(app_secret)?;
    let key = Key::<Aes256Gcm>::from_slice(&key_bytes);
    let cipher = Aes256Gcm::new(key);

    let plaintext_bytes = cipher
        .decrypt(nonce, ct)
        .map_err(|_| anyhow::anyhow!("AES-GCM decrypt failed — key mismatch or tampered data"))?;

    String::from_utf8(plaintext_bytes)
        .map_err(|e| anyhow::anyhow!("decrypted bytes not valid UTF-8: {e}"))
}

/// Return the last 4 characters of a key as a hint (masked display).
pub fn api_key_hint(plaintext: &str) -> String {
    let chars: Vec<char> = plaintext.chars().collect();
    if chars.len() <= 4 {
        return "****".to_string();
    }
    let prefix: String = chars[..chars.len().min(3)].iter().collect();
    let suffix: String = chars[chars.len() - 4..].iter().collect();
    format!("{prefix}...{suffix}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let secret = "test_secret_for_testing_only_do_not_use";
        let plaintext = "sk-proj-abc123xyz";
        let enc = encrypt_api_key(plaintext, secret).unwrap();
        let dec = decrypt_api_key(&enc, secret).unwrap();
        assert_eq!(dec, plaintext);
    }

    #[test]
    fn different_nonces() {
        let secret = "test_secret";
        let plaintext = "sk-test";
        let enc1 = encrypt_api_key(plaintext, secret).unwrap();
        let enc2 = encrypt_api_key(plaintext, secret).unwrap();
        // Different nonces → different ciphertexts
        assert_ne!(enc1, enc2);
    }

    #[test]
    fn wrong_key_fails() {
        let plaintext = "sk-test-key";
        let enc = encrypt_api_key(plaintext, "correct_secret").unwrap();
        let result = decrypt_api_key(&enc, "wrong_secret");
        assert!(result.is_err());
    }

    #[test]
    fn hint_format() {
        assert_eq!(api_key_hint("sk-proj-abc123"), "sk-...c123");
        assert_eq!(api_key_hint("ab"), "****");
    }
}
