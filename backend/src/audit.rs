/// Writes a row to `audit_log`. Non-fatal: errors are logged but not propagated
/// to the caller (an audit failure should not break the business operation).
use sqlx::PgPool;
use std::net::IpAddr;
use uuid::Uuid;

#[allow(clippy::too_many_arguments)]
pub async fn log(
    pool: &PgPool,
    user_id: Option<Uuid>,
    action: &str,
    resource_type: Option<&str>,
    resource_id: Option<&str>,
    ip: Option<IpAddr>,
    metadata: Option<serde_json::Value>,
) -> Result<(), crate::error::AppError> {
    let ip_str = ip.map(|a| a.to_string());
    sqlx::query(
        r#"INSERT INTO audit_log (user_id, action, resource_type, resource_id, ip, metadata)
           VALUES ($1, $2, $3, $4, $5::inet, $6)"#,
    )
    .bind(user_id)
    .bind(action)
    .bind(resource_type)
    .bind(resource_id)
    .bind(ip_str.as_deref())
    .bind(metadata)
    .execute(pool)
    .await
    .map_err(|e| {
        tracing::warn!(error = %e, action = %action, "audit log write failed (non-fatal)");
        crate::error::AppError::Database(e)
    })?;
    Ok(())
}
