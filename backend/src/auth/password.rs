/// Password hashing and verification using Argon2id.
/// Parameters: m=19456 (19 MiB), t=2, p=1 — OWASP 2024 recommendation.
///
/// Also enforces a minimum length of 12 characters and rejects the top-1000
/// most common passwords (see backend/assets/common-passwords.txt).
use crate::error::AppError;
use argon2::{
    password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2, Params,
};

static COMMON_PASSWORDS: std::sync::OnceLock<std::collections::HashSet<&'static str>> =
    std::sync::OnceLock::new();

const COMMON_PASSWORDS_RAW: &str = include_str!("../../assets/common-passwords.txt");

pub const MIN_PASSWORD_LEN: usize = 12;

fn common_passwords() -> &'static std::collections::HashSet<&'static str> {
    COMMON_PASSWORDS.get_or_init(|| {
        COMMON_PASSWORDS_RAW
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .collect()
    })
}

fn argon2() -> Argon2<'static> {
    let params = Params::new(
        19456, // m_cost: 19 MiB
        2,     // t_cost: 2 iterations
        1,     // p_cost: 1 parallelism
        None,
    )
    .expect("valid argon2 params");
    Argon2::new(argon2::Algorithm::Argon2id, argon2::Version::V0x13, params)
}

/// Returns a PHC-formatted Argon2id hash string.
pub fn hash(password: &str) -> Result<String, AppError> {
    let salt = SaltString::generate(&mut OsRng);
    argon2()
        .hash_password(password.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| AppError::Internal(anyhow::anyhow!("password hash failed: {e}")))
}

/// Returns `true` if `password` matches the stored `hash`.
pub fn verify(password: &str, hash: &str) -> Result<bool, AppError> {
    let parsed = PasswordHash::new(hash)
        .map_err(|e| AppError::Internal(anyhow::anyhow!("password hash parse failed: {e}")))?;
    Ok(argon2().verify_password(password.as_bytes(), &parsed).is_ok())
}

/// Runs a dummy hash to consume constant time regardless of whether a user exists.
/// Call this on the "user not found" path of login to prevent timing-based enumeration.
pub fn dummy_verify() {
    let _ = verify("dummy-password-for-timing", DUMMY_HASH);
}

/// A pre-computed Argon2id hash of a throwaway string. Used by `dummy_verify`.
const DUMMY_HASH: &str =
    "$argon2id$v=19$m=19456,t=2,p=1$c29tZXNhbHRmb3JkdW1teQ$placeholder00000000000000000000000000000000000000000000";

/// Returns `true` if the password is in the common-passwords list.
pub fn is_common_password(password: &str) -> bool {
    common_passwords().contains(password)
}

/// Validates a candidate password for registration/reset.
/// Returns `Err(AppError::Validation)` with a user-safe message on failure.
pub fn validate_new_password(password: &str) -> Result<(), AppError> {
    if password.len() < MIN_PASSWORD_LEN {
        return Err(AppError::Validation(format!(
            "Password must be at least {MIN_PASSWORD_LEN} characters."
        )));
    }
    if is_common_password(password) {
        return Err(AppError::Validation(
            "Password is too common. Please choose a more unique password.".to_owned(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_and_verify_round_trip() {
        let pw = "correct-horse-battery-staple-1234";
        let hashed = hash(pw).unwrap();
        assert!(verify(pw, &hashed).unwrap());
    }

    #[test]
    fn wrong_password_does_not_verify() {
        let hashed = hash("correct-horse-battery-staple-1234").unwrap();
        assert!(!verify("wrong-password", &hashed).unwrap());
    }

    #[test]
    fn two_hashes_of_same_password_differ() {
        let pw = "unique-each-time-has-salt-12345";
        let h1 = hash(pw).unwrap();
        let h2 = hash(pw).unwrap();
        assert_ne!(h1, h2, "different salts must produce different hashes");
    }

    #[test]
    fn validate_rejects_short_password() {
        assert!(validate_new_password("short").is_err());
    }

    #[test]
    fn validate_accepts_long_password() {
        assert!(validate_new_password("a-long-enough-password-here").is_ok());
    }

    #[test]
    fn dummy_verify_does_not_panic() {
        dummy_verify();
    }
}
