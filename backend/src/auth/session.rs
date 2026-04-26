/// Session management helpers.
///
/// tower-sessions owns the `tower_sessions` table. We maintain a shadow
/// `session_index` table for "log out everywhere" functionality.
/// The session_id_hash stored in session_index is sha3_256(tower_sessions.id)
/// encoded as lowercase hex.
use sha3::{Digest, Sha3_256};
use sqlx::PgPool;
use std::net::IpAddr;
use time::OffsetDateTime;
use tower_sessions::Session;
use uuid::Uuid;

pub const SESSION_USER_ID_KEY: &str = "user_id";

/// Hash a tower-sessions session ID using SHA3-256.
pub fn hash_session_id(session_id: &str) -> String {
    let mut hasher = Sha3_256::new();
    hasher.update(session_id.as_bytes());
    hex::encode(hasher.finalize())
}

/// Record a new session in `session_index` after login.
/// Both the raw session ID (needed for revoke-all) and its SHA3-256 hash are stored.
pub async fn record_session(
    pool: &PgPool,
    session: &Session,
    user_id: Uuid,
    ip: Option<IpAddr>,
    user_agent: Option<&str>,
    expires_at: OffsetDateTime,
) -> Result<(), sqlx::Error> {
    let session_id = session.id().map(|id| id.to_string()).unwrap_or_default();
    let hash = hash_session_id(&session_id);
    let ip_pg = ip.map(|a| a.to_string());

    sqlx::query(
        r#"
        INSERT INTO session_index (session_id_hash, session_id, user_id, ip, user_agent, expires_at)
        VALUES ($1, $2, $3, $4::inet, $5, $6)
        ON CONFLICT (session_id_hash) DO UPDATE
            SET last_seen_at = now(), expires_at = $6
        "#,
    )
    .bind(&hash)
    .bind(&session_id)
    .bind(user_id)
    .bind(ip_pg.as_deref())
    .bind(user_agent)
    .bind(expires_at)
    .execute(pool)
    .await?;

    Ok(())
}

/// Remove a single session from `session_index` on logout.
pub async fn remove_session(pool: &PgPool, session: &Session) -> Result<(), sqlx::Error> {
    let hash = hash_session_id(&session.id().map(|id| id.to_string()).unwrap_or_default());
    sqlx::query("DELETE FROM session_index WHERE session_id_hash = $1")
        .bind(&hash)
        .execute(pool)
        .await?;
    Ok(())
}

/// Revoke all sessions for a user.
///
/// Uses the `session_id` column stored in `session_index` to delete directly from
/// `tower_sessions` in O(user-sessions) rather than scanning the entire sessions table.
pub async fn revoke_all_sessions(pool: &PgPool, user_id: Uuid) -> Result<(), sqlx::Error> {
    // Fetch only this user's raw session IDs — no full-table scan needed.
    let session_ids: Vec<String> =
        sqlx::query_scalar("SELECT session_id FROM session_index WHERE user_id = $1")
            .bind(user_id)
            .fetch_all(pool)
            .await?;

    if !session_ids.is_empty() {
        sqlx::query("DELETE FROM tower_sessions WHERE id = ANY($1)")
            .bind(&session_ids)
            .execute(pool)
            .await?;
    }

    // Remove our projection rows
    sqlx::query("DELETE FROM session_index WHERE user_id = $1")
        .bind(user_id)
        .execute(pool)
        .await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_session_id_is_deterministic() {
        let id = "550e8400-e29b-41d4-a716-446655440000";
        let h1 = hash_session_id(id);
        let h2 = hash_session_id(id);
        assert_eq!(h1, h2);
        assert_eq!(h1.len(), 64); // sha3_256 -> 32 bytes -> 64 hex chars
    }

    #[test]
    fn different_ids_produce_different_hashes() {
        let h1 = hash_session_id("session-id-one");
        let h2 = hash_session_id("session-id-two");
        assert_ne!(h1, h2);
    }
}
