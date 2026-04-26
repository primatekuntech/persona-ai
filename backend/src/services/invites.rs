/// Invite creation service. Encapsulates the business rules around invite tokens.
use crate::{
    error::AppError,
    repositories::{
        invites::{create_invite, generate_token, hash_token},
        users::find_by_email,
    },
};
use sqlx::PgPool;
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

pub const INVITE_TTL_DAYS: i64 = 7;

pub struct CreatedInvite {
    pub token_hash: String,
    pub invite_url: String,
}

/// Create an invite for `email` with `role`, sent by `created_by`.
///
/// Rejects with:
/// - `Conflict { "user_exists" }` if an active user with that email exists.
/// - `Conflict { "invite_pending" }` if an active unexpired invite already exists
///   (enforced by the DB partial unique index).
pub async fn create(
    pool: &PgPool,
    email: &str,
    role: &str,
    created_by: Uuid,
    app_base_url: &str,
) -> Result<CreatedInvite, AppError> {
    // Reject if email already has a live user
    if let Some(existing) = find_by_email(pool, email).await? {
        tracing::info!(email = %email, user_id = %existing.id, "invite rejected: user_exists");
        return Err(AppError::Conflict {
            code: "user_exists",
        });
    }

    let plaintext = generate_token();
    let token_hash = hash_token(&plaintext);
    let expires_at = OffsetDateTime::now_utc() + Duration::days(INVITE_TTL_DAYS);

    create_invite(pool, &token_hash, email, role, created_by, expires_at).await?;

    let invite_url = format!("{app_base_url}/accept-invite?token={plaintext}");

    Ok(CreatedInvite {
        token_hash,
        invite_url,
    })
}
