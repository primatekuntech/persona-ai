/// Invite token repository.
/// token_hash = sha3_256(plaintext_token) hex-encoded.
use crate::error::AppError;
use serde::{Deserialize, Serialize};
use sha3::{Digest, Sha3_256};
use sqlx::{FromRow, PgPool};
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct InviteToken {
    pub token_hash: String,
    pub email: String,
    pub role: String,
    pub created_by: Uuid,
    pub expires_at: OffsetDateTime,
    pub used_at: Option<OffsetDateTime>,
    pub used_by: Option<Uuid>,
    pub created_at: OffsetDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct PasswordReset {
    pub token_hash: String,
    pub user_id: Uuid,
    pub expires_at: OffsetDateTime,
    pub used_at: Option<OffsetDateTime>,
}

pub fn hash_token(plaintext: &str) -> String {
    let mut h = Sha3_256::new();
    h.update(plaintext.as_bytes());
    hex::encode(h.finalize())
}

/// Generate a 32-byte random token and return its hex-encoded plaintext.
pub fn generate_token() -> String {
    use rand::RngCore;
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    hex::encode(bytes)
}

// ─── Invite tokens ─────────────────────────────────────────────────────────

/// Insert a new invite token. Returns `Conflict { "invite_pending" }` if an
/// active invite for that email already exists (via the partial unique index).
pub async fn create_invite(
    pool: &PgPool,
    token_hash: &str,
    email: &str,
    role: &str,
    created_by: Uuid,
    expires_at: OffsetDateTime,
) -> Result<(), AppError> {
    sqlx::query(
        r#"INSERT INTO invite_tokens (token_hash, email, role, created_by, expires_at)
           VALUES ($1, $2, $3, $4, $5)"#,
    )
    .bind(token_hash)
    .bind(email)
    .bind(role)
    .bind(created_by)
    .bind(expires_at)
    .execute(pool)
    .await
    .map_err(|e| crate::error::pg_unique_to_conflict(e, "invite_pending"))?;
    Ok(())
}

/// Find an active (unused, unexpired) invite by token hash.
/// Returns `None` for unknown, expired, or used tokens.
pub async fn find_active_invite(
    pool: &PgPool,
    token_hash: &str,
) -> Result<Option<InviteToken>, AppError> {
    sqlx::query_as::<_, InviteToken>(
        r#"SELECT token_hash, email, role, created_by, expires_at, used_at, used_by, created_at
           FROM invite_tokens
           WHERE token_hash = $1 AND used_at IS NULL AND expires_at > now()"#,
    )
    .bind(token_hash)
    .fetch_optional(pool)
    .await
    .map_err(AppError::Database)
}

/// Mark an invite as used inside a transaction (prevents race-condition double-use).
///
/// Returns `Some((email, role))` when the token is valid and has now been locked;
/// returns `None` if the token is unknown, expired, or already used.
/// The caller must INSERT the new user and then UPDATE `used_by` within the same
/// transaction before committing, so that `used_by` is never left as NULL on commit.
pub async fn use_invite(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    token_hash: &str,
) -> Result<Option<(String, String)>, AppError> {
    // Lock the row for this transaction — concurrent acceptances block here.
    let row: Option<(String, String)> = sqlx::query_as(
        r#"SELECT email, role FROM invite_tokens
           WHERE token_hash = $1 AND used_at IS NULL AND expires_at > now()
           FOR UPDATE"#,
    )
    .bind(token_hash)
    .fetch_optional(&mut **tx)
    .await
    .map_err(AppError::Database)?;

    if row.is_none() {
        return Ok(None);
    }

    // Mark the token used immediately; used_by is set by the caller after user insert.
    sqlx::query(
        "UPDATE invite_tokens SET used_at = now() WHERE token_hash = $1",
    )
    .bind(token_hash)
    .execute(&mut **tx)
    .await
    .map_err(AppError::Database)?;

    Ok(row)
}

/// List all invite tokens (admin view, paginated).
pub async fn list_invites(
    pool: &PgPool,
    limit: i64,
    cursor_ts: Option<OffsetDateTime>,
    cursor_id: Option<String>,
) -> Result<Vec<InviteToken>, AppError> {
    let rows = if let (Some(ts), Some(id)) = (cursor_ts, cursor_id) {
        sqlx::query_as::<_, InviteToken>(
            r#"SELECT token_hash, email, role, created_by, expires_at, used_at, used_by, created_at
               FROM invite_tokens
               WHERE (created_at, token_hash) < ($1, $2)
               ORDER BY created_at DESC, token_hash DESC
               LIMIT $3"#,
        )
        .bind(ts)
        .bind(id)
        .bind(limit + 1)
        .fetch_all(pool)
        .await
    } else {
        sqlx::query_as::<_, InviteToken>(
            r#"SELECT token_hash, email, role, created_by, expires_at, used_at, used_by, created_at
               FROM invite_tokens
               ORDER BY created_at DESC, token_hash DESC
               LIMIT $1"#,
        )
        .bind(limit + 1)
        .fetch_all(pool)
        .await
    };
    rows.map_err(AppError::Database)
}

/// Revoke (delete) an invite by hash. Admin action.
pub async fn revoke_invite(pool: &PgPool, token_hash: &str) -> Result<bool, AppError> {
    let res = sqlx::query("DELETE FROM invite_tokens WHERE token_hash = $1")
        .bind(token_hash)
        .execute(pool)
        .await
        .map_err(AppError::Database)?;
    Ok(res.rows_affected() > 0)
}

// ─── Password reset tokens ─────────────────────────────────────────────────

pub async fn create_password_reset(
    pool: &PgPool,
    token_hash: &str,
    user_id: Uuid,
    expires_at: OffsetDateTime,
) -> Result<(), AppError> {
    sqlx::query(
        r#"INSERT INTO password_resets (token_hash, user_id, expires_at)
           VALUES ($1, $2, $3)"#,
    )
    .bind(token_hash)
    .bind(user_id)
    .bind(expires_at)
    .execute(pool)
    .await
    .map_err(AppError::Database)?;
    Ok(())
}

pub async fn find_active_reset(
    pool: &PgPool,
    token_hash: &str,
) -> Result<Option<PasswordReset>, AppError> {
    sqlx::query_as::<_, PasswordReset>(
        r#"SELECT token_hash, user_id, expires_at, used_at
           FROM password_resets
           WHERE token_hash = $1 AND used_at IS NULL AND expires_at > now()"#,
    )
    .bind(token_hash)
    .fetch_optional(pool)
    .await
    .map_err(AppError::Database)
}

/// Mark a password-reset token as used inside a transaction (FOR UPDATE prevents races).
///
/// Returns the `PasswordReset` row if the token was active and is now locked; `None`
/// if it is unknown, expired, or already used.  The caller must update the user's
/// password_hash within the same transaction before committing.
pub async fn use_reset_token(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    token_hash: &str,
) -> Result<Option<PasswordReset>, AppError> {
    let row: Option<PasswordReset> = sqlx::query_as(
        r#"SELECT token_hash, user_id, expires_at, used_at
           FROM password_resets
           WHERE token_hash = $1 AND used_at IS NULL AND expires_at > now()
           FOR UPDATE"#,
    )
    .bind(token_hash)
    .fetch_optional(&mut **tx)
    .await
    .map_err(AppError::Database)?;

    if row.is_some() {
        sqlx::query(
            "UPDATE password_resets SET used_at = now() WHERE token_hash = $1",
        )
        .bind(token_hash)
        .execute(&mut **tx)
        .await
        .map_err(AppError::Database)?;
    }

    Ok(row)
}
