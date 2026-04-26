/// User repository. Every function takes `user_id: Uuid` and filters by it
/// (authorization invariant from docs/02-data-model.md and docs/08-security.md).
use crate::error::AppError;
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgPool};
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct User {
    pub id: Uuid,
    pub email: String,
    #[serde(skip_serializing)]
    pub password_hash: String,
    pub role: String,
    pub status: String,
    pub display_name: Option<String>,
    pub created_at: OffsetDateTime,
    pub last_login_at: Option<OffsetDateTime>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct UserSummary {
    pub id: Uuid,
    pub email: String,
    pub role: String,
    pub status: String,
    pub display_name: Option<String>,
    pub created_at: OffsetDateTime,
    pub last_login_at: Option<OffsetDateTime>,
}

/// Find a user by email (case-insensitive via CITEXT). No user_id filter —
/// used during login/invite-accept before a session exists.
pub async fn find_by_email(pool: &PgPool, email: &str) -> Result<Option<User>, AppError> {
    sqlx::query_as::<_, User>(
        "SELECT id, email, password_hash, role, status, display_name, created_at, last_login_at
         FROM users WHERE email = $1",
    )
    .bind(email)
    .fetch_optional(pool)
    .await
    .map_err(AppError::Database)
}

/// Find a user by ID. Used in `require_auth` to refetch role+status per request.
pub async fn find_by_id(pool: &PgPool, id: Uuid) -> Result<Option<User>, AppError> {
    sqlx::query_as::<_, User>(
        "SELECT id, email, password_hash, role, status, display_name, created_at, last_login_at
         FROM users WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(AppError::Database)
}

/// Create a new user. Called from invite acceptance and admin bootstrap.
pub async fn create(
    pool: &PgPool,
    email: &str,
    password_hash: &str,
    role: &str,
    display_name: Option<&str>,
) -> Result<User, AppError> {
    sqlx::query_as::<_, User>(
        r#"INSERT INTO users (email, password_hash, role, display_name)
           VALUES ($1, $2, $3, $4)
           RETURNING id, email, password_hash, role, status, display_name, created_at, last_login_at"#,
    )
    .bind(email)
    .bind(password_hash)
    .bind(role)
    .bind(display_name)
    .fetch_one(pool)
    .await
    .map_err(AppError::Database)
}

/// Check whether any admin user exists (for bootstrap guard).
pub async fn admin_exists(pool: &PgPool) -> Result<bool, AppError> {
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users WHERE role = 'admin'")
        .fetch_one(pool)
        .await
        .map_err(AppError::Database)?;
    Ok(count > 0)
}

/// Update last_login_at timestamp after successful authentication.
pub async fn touch_login(pool: &PgPool, user_id: Uuid) -> Result<(), AppError> {
    sqlx::query("UPDATE users SET last_login_at = now() WHERE id = $1")
        .bind(user_id)
        .execute(pool)
        .await
        .map_err(AppError::Database)?;
    Ok(())
}

/// Update password hash. Called after password reset.
#[allow(dead_code)]
pub async fn set_password(
    pool: &PgPool,
    user_id: Uuid,
    new_hash: &str,
) -> Result<(), AppError> {
    sqlx::query("UPDATE users SET password_hash = $1 WHERE id = $2")
        .bind(new_hash)
        .bind(user_id)
        .execute(pool)
        .await
        .map_err(AppError::Database)?;
    Ok(())
}

/// List users (admin only). Cursor-based pagination on (created_at DESC, id DESC).
pub async fn list(
    pool: &PgPool,
    limit: i64,
    cursor_ts: Option<OffsetDateTime>,
    cursor_id: Option<Uuid>,
) -> Result<Vec<UserSummary>, AppError> {
    let rows = if let (Some(ts), Some(id)) = (cursor_ts, cursor_id) {
        sqlx::query_as::<_, UserSummary>(
            r#"SELECT id, email, role, status, display_name, created_at, last_login_at
               FROM users
               WHERE (created_at, id) < ($1, $2)
               ORDER BY created_at DESC, id DESC
               LIMIT $3"#,
        )
        .bind(ts)
        .bind(id)
        .bind(limit + 1)
        .fetch_all(pool)
        .await
    } else {
        sqlx::query_as::<_, UserSummary>(
            r#"SELECT id, email, role, status, display_name, created_at, last_login_at
               FROM users
               ORDER BY created_at DESC, id DESC
               LIMIT $1"#,
        )
        .bind(limit + 1)
        .fetch_all(pool)
        .await
    };
    rows.map_err(AppError::Database)
}

/// Patch a user's role and/or status (admin action).
pub async fn update_role_status(
    pool: &PgPool,
    target_id: Uuid,
    role: Option<&str>,
    status: Option<&str>,
) -> Result<(), AppError> {
    if let Some(r) = role {
        sqlx::query("UPDATE users SET role = $1 WHERE id = $2")
            .bind(r)
            .bind(target_id)
            .execute(pool)
            .await
            .map_err(AppError::Database)?;
    }
    if let Some(s) = status {
        sqlx::query("UPDATE users SET status = $1 WHERE id = $2")
            .bind(s)
            .bind(target_id)
            .execute(pool)
            .await
            .map_err(AppError::Database)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    // Integration tests that require a live DB are in backend/tests/repositories_users.rs
    // using the sqlx::test macro.
}
