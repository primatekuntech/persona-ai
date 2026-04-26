/// Era repository. All functions scope by both persona_id and user_id.
use crate::error::AppError;
use serde::Serialize;
use sqlx::{FromRow, PgPool};
use time::{Date, OffsetDateTime};
use uuid::Uuid;

#[allow(dead_code)]
mod date_opt_serde {
    use serde::{Deserialize, Deserializer, Serializer};
    use time::{macros::format_description, Date};

    pub fn serialize<S: Serializer>(val: &Option<Date>, s: S) -> Result<S::Ok, S::Error> {
        match val {
            None => s.serialize_none(),
            Some(d) => {
                let fmt = format_description!("[year]-[month]-[day]");
                let str = d.format(fmt).map_err(serde::ser::Error::custom)?;
                s.serialize_str(&str)
            }
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Date>, D::Error> {
        let s: Option<String> = Deserialize::deserialize(d)?;
        match s {
            None => Ok(None),
            Some(ref str_val) => {
                let fmt = format_description!("[year]-[month]-[day]");
                Date::parse(str_val, fmt)
                    .map(Some)
                    .map_err(serde::de::Error::custom)
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct Era {
    pub id: Uuid,
    pub persona_id: Uuid,
    pub user_id: Uuid,
    pub label: String,
    #[serde(with = "date_opt_serde")]
    pub start_date: Option<Date>,
    #[serde(with = "date_opt_serde")]
    pub end_date: Option<Date>,
    pub description: Option<String>,
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
}

pub async fn create(
    pool: &PgPool,
    persona_id: Uuid,
    user_id: Uuid,
    label: &str,
    start_date: Option<Date>,
    end_date: Option<Date>,
    description: Option<&str>,
) -> Result<Era, AppError> {
    // Verify persona belongs to user
    let exists: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM personas WHERE id = $1 AND user_id = $2)")
            .bind(persona_id)
            .bind(user_id)
            .fetch_one(pool)
            .await
            .map_err(AppError::Database)?;

    if !exists {
        return Err(AppError::NotFound);
    }

    sqlx::query_as(
        r#"INSERT INTO eras (persona_id, user_id, label, start_date, end_date, description)
           VALUES ($1, $2, $3, $4, $5, $6)
           RETURNING id, persona_id, user_id, label, start_date, end_date, description, created_at, updated_at"#,
    )
    .bind(persona_id)
    .bind(user_id)
    .bind(label)
    .bind(start_date)
    .bind(end_date)
    .bind(description)
    .fetch_one(pool)
    .await
    .map_err(|e| crate::error::pg_unique_to_conflict(e, "label_taken"))
}

pub async fn find_by_id(
    pool: &PgPool,
    id: Uuid,
    persona_id: Uuid,
    user_id: Uuid,
) -> Result<Option<Era>, AppError> {
    sqlx::query_as(
        "SELECT id, persona_id, user_id, label, start_date, end_date, description, created_at, updated_at
         FROM eras WHERE id = $1 AND persona_id = $2 AND user_id = $3",
    )
    .bind(id)
    .bind(persona_id)
    .bind(user_id)
    .fetch_optional(pool)
    .await
    .map_err(AppError::Database)
}

pub async fn list(pool: &PgPool, persona_id: Uuid, user_id: Uuid) -> Result<Vec<Era>, AppError> {
    sqlx::query_as(
        r#"SELECT id, persona_id, user_id, label, start_date, end_date, description, created_at, updated_at
           FROM eras
           WHERE persona_id = $1 AND user_id = $2
           ORDER BY start_date ASC NULLS LAST, created_at ASC"#,
    )
    .bind(persona_id)
    .bind(user_id)
    .fetch_all(pool)
    .await
    .map_err(AppError::Database)
}

#[allow(clippy::too_many_arguments)] // all args are semantically distinct; a struct would add noise
pub async fn update(
    pool: &PgPool,
    id: Uuid,
    persona_id: Uuid,
    user_id: Uuid,
    label: Option<&str>,
    start_date: Option<Option<Date>>,
    end_date: Option<Option<Date>>,
    description: Option<Option<&str>>,
) -> Result<Option<Era>, AppError> {
    let mut builder =
        sqlx::QueryBuilder::<sqlx::Postgres>::new("UPDATE eras SET updated_at = now()");

    if let Some(l) = label {
        builder.push(", label = ");
        builder.push_bind(l);
    }
    if let Some(sd) = start_date {
        builder.push(", start_date = ");
        builder.push_bind(sd);
    }
    if let Some(ed) = end_date {
        builder.push(", end_date = ");
        builder.push_bind(ed);
    }
    if let Some(desc) = description {
        builder.push(", description = ");
        builder.push_bind(desc);
    }

    builder.push(" WHERE id = ");
    builder.push_bind(id);
    builder.push(" AND persona_id = ");
    builder.push_bind(persona_id);
    builder.push(" AND user_id = ");
    builder.push_bind(user_id);
    builder.push(
        " RETURNING id, persona_id, user_id, label, start_date, end_date, description, created_at, updated_at",
    );

    builder
        .build_query_as::<Era>()
        .fetch_optional(pool)
        .await
        .map_err(|e| crate::error::pg_unique_to_conflict(e, "label_taken"))
}

pub async fn delete(
    pool: &PgPool,
    id: Uuid,
    persona_id: Uuid,
    user_id: Uuid,
) -> Result<bool, AppError> {
    let result = sqlx::query("DELETE FROM eras WHERE id = $1 AND persona_id = $2 AND user_id = $3")
        .bind(id)
        .bind(persona_id)
        .bind(user_id)
        .execute(pool)
        .await
        .map_err(AppError::Database)?;
    Ok(result.rows_affected() > 0)
}
