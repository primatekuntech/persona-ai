/// `require_auth` and `require_admin` axum extractors.
///
/// Role is NOT cached in session data. Every request refetches `users.role`
/// and `users.status` from the database so that admin demotion or account
/// disable takes effect immediately without forcing re-login.
use crate::{error::AppError, state::AppState};
use axum::{
    async_trait,
    extract::FromRequestParts,
    http::request::Parts,
};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use tower_sessions::Session;
use uuid::Uuid;

use super::SESSION_USER_ID_KEY;

/// User role. Serialized as snake_case strings matching the DB constraint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "text", rename_all = "lowercase")]
pub enum Role {
    Admin,
    User,
}

/// Account status.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "text", rename_all = "lowercase")]
pub enum UserStatus {
    Active,
    Disabled,
}

/// Injected into every authenticated handler by `require_auth`.
#[derive(Debug, Clone)]
pub struct UserCtx {
    pub user_id: Uuid,
    pub role: Role,
}

#[derive(Debug, FromRow)]
struct UserRow {
    id: Uuid,
    role: String,
    status: String,
}

/// Extractor that loads the session, refetches `users(role, status)` from DB,
/// and injects `UserCtx`. Returns 401 if no session; 403 if account disabled.
#[async_trait]
impl FromRequestParts<AppState> for UserCtx {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let session = Session::from_request_parts(parts, state)
            .await
            .map_err(|_| AppError::Unauthorized)?;

        let user_id: Uuid = session
            .get::<Uuid>(SESSION_USER_ID_KEY)
            .await
            .map_err(|_| AppError::Unauthorized)?
            .ok_or(AppError::Unauthorized)?;

        let row: Option<UserRow> =
            sqlx::query_as("SELECT id, role, status FROM users WHERE id = $1")
                .bind(user_id)
                .fetch_optional(&state.db)
                .await
                .map_err(AppError::Database)?;

        let row = row.ok_or(AppError::Unauthorized)?;

        let role = match row.role.as_str() {
            "admin" => Role::Admin,
            _ => Role::User,
        };
        let status = match row.status.as_str() {
            "active" => UserStatus::Active,
            _ => UserStatus::Disabled,
        };

        if status == UserStatus::Disabled {
            return Err(AppError::Forbidden {
                code: "account_disabled",
            });
        }

        Ok(UserCtx {
            user_id: row.id,
            role,
        })
    }
}

/// Extractor that requires admin role. Returns 403 for non-admin authenticated users.
pub struct AdminCtx(pub UserCtx);

#[async_trait]
impl FromRequestParts<AppState> for AdminCtx {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let ctx = UserCtx::from_request_parts(parts, state).await?;
        if ctx.role != Role::Admin {
            return Err(AppError::Forbidden { code: "forbidden" });
        }
        Ok(AdminCtx(ctx))
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn role_equality() {
        assert_eq!(Role::Admin, Role::Admin);
        assert_ne!(Role::Admin, Role::User);
    }

    #[test]
    fn status_equality() {
        assert_eq!(UserStatus::Active, UserStatus::Active);
        assert_ne!(UserStatus::Active, UserStatus::Disabled);
    }
}
