/// Persona and era route handlers (sprint 2).
use crate::{
    audit,
    auth::middleware::UserCtx,
    error::AppError,
    repositories::{eras as era_repo, personas as persona_repo},
    services::idempotency,
    state::AppState,
};
use axum::{
    body::Body,
    extract::{Multipart, Path, Query, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use time::Date;
use tokio::fs;
use uuid::Uuid;

// ─── Shared response types ───────────────────────────────────────────────────

#[derive(Serialize)]
pub struct Paginated<T: Serialize> {
    pub items: Vec<T>,
    pub next_cursor: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_estimate: Option<i64>,
}

// ─── Validation ──────────────────────────────────────────────────────────────

fn validate_relation(rel: &str) -> bool {
    matches!(rel, "self" | "family" | "friend" | "other")
}

fn validate_persona_fields(
    name: Option<&str>,
    relation: Option<Option<&str>>,
    birth_year: Option<Option<i32>>,
) -> Result<(), AppError> {
    let mut fields: HashMap<String, String> = HashMap::new();

    if let Some(n) = name {
        if n.is_empty() || n.len() > 80 {
            fields.insert("name".to_string(), "Must be 1–80 characters.".to_string());
        }
    }

    if let Some(Some(rel)) = relation {
        if !validate_relation(rel) {
            fields.insert(
                "relation".to_string(),
                "Must be one of: self, family, friend, other.".to_string(),
            );
        }
    }

    if let Some(Some(year)) = birth_year {
        let max_year = time::OffsetDateTime::now_utc().year();
        if year < 1900 || year > max_year {
            fields.insert(
                "birth_year".to_string(),
                format!("Must be between 1900 and {max_year}."),
            );
        }
    }

    if fields.is_empty() {
        Ok(())
    } else {
        Err(AppError::ValidationFields(fields))
    }
}

fn parse_date(s: &str, field: &str) -> Result<Date, AppError> {
    let fmt = time::macros::format_description!("[year]-[month]-[day]");
    Date::parse(s, fmt).map_err(|_| {
        AppError::ValidationFields(HashMap::from([(
            field.to_string(),
            "Invalid date format. Use YYYY-MM-DD.".to_string(),
        )]))
    })
}

// ─── Cursor query param ───────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct CursorParams {
    pub limit: Option<i64>,
    pub cursor: Option<String>,
}

// ─── Persona handlers ─────────────────────────────────────────────────────────

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CreatePersonaRequest {
    pub name: String,
    pub relation: Option<String>,
    pub description: Option<String>,
    pub birth_year: Option<i32>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PatchPersonaRequest {
    pub name: Option<String>,
    pub relation: Option<Option<String>>,
    pub description: Option<Option<String>>,
    pub birth_year: Option<Option<i32>>,
}

pub async fn list_personas(
    State(state): State<AppState>,
    ctx: UserCtx,
    Query(params): Query<CursorParams>,
) -> Result<impl IntoResponse, AppError> {
    let limit = params.limit.unwrap_or(50);
    let cursor = params
        .cursor
        .as_deref()
        .map(persona_repo::decode_cursor)
        .transpose()?;

    let (items, next_cursor) = persona_repo::list(&state.db, ctx.user_id, limit, cursor).await?;

    Ok(Json(Paginated {
        total_estimate: None,
        items,
        next_cursor,
    }))
}

pub async fn create_persona(
    State(state): State<AppState>,
    ctx: UserCtx,
    headers: HeaderMap,
    Json(body): Json<CreatePersonaRequest>,
) -> Result<Response, AppError> {
    validate_persona_fields(
        Some(&body.name),
        Some(body.relation.as_deref()),
        Some(body.birth_year),
    )?;

    let idem_key = headers
        .get("idempotency-key")
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);

    let body_hash = idempotency::body_hash(&body);

    if let Some(ref key) = idem_key {
        if let Some(hit) =
            idempotency::check(&state.db, key, ctx.user_id, "/api/personas", &body_hash).await?
        {
            let status = StatusCode::from_u16(hit.status as u16).unwrap_or(StatusCode::OK);
            return Ok((status, Json(hit.body)).into_response());
        }
    }

    let persona = persona_repo::create(
        &state.db,
        ctx.user_id,
        &body.name,
        body.relation.as_deref(),
        body.description.as_deref(),
        body.birth_year,
    )
    .await?;

    audit::log(
        &state.db,
        Some(ctx.user_id),
        "persona.created",
        Some("persona"),
        Some(&persona.id.to_string()),
        None,
        None,
    )
    .await
    .ok();

    if let Some(ref key) = idem_key {
        let persona_json = serde_json::to_value(&persona).unwrap_or(serde_json::Value::Null);
        idempotency::store(
            &state.db,
            key,
            ctx.user_id,
            "/api/personas",
            &body_hash,
            201,
            persona_json,
        )
        .await
        .ok();
    }

    Ok((StatusCode::CREATED, Json(persona)).into_response())
}

pub async fn get_persona(
    State(state): State<AppState>,
    ctx: UserCtx,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    let persona = persona_repo::find_by_id(&state.db, id, ctx.user_id)
        .await?
        .ok_or(AppError::NotFound)?;
    Ok(Json(persona))
}

pub async fn patch_persona(
    State(state): State<AppState>,
    ctx: UserCtx,
    Path(id): Path<Uuid>,
    Json(body): Json<PatchPersonaRequest>,
) -> Result<impl IntoResponse, AppError> {
    validate_persona_fields(
        body.name.as_deref(),
        body.relation.as_ref().map(|r| r.as_deref()),
        body.birth_year.as_ref().copied(),
    )?;

    let persona = persona_repo::update(
        &state.db,
        id,
        ctx.user_id,
        body.name.as_deref(),
        body.relation.as_ref().map(|r| r.as_deref()),
        body.description.as_ref().map(|d| d.as_deref()),
        body.birth_year,
    )
    .await?
    .ok_or(AppError::NotFound)?;

    audit::log(
        &state.db,
        Some(ctx.user_id),
        "persona.updated",
        Some("persona"),
        Some(&persona.id.to_string()),
        None,
        None,
    )
    .await
    .ok();

    Ok(Json(persona))
}

pub async fn delete_persona(
    State(state): State<AppState>,
    ctx: UserCtx,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    let info = persona_repo::delete(&state.db, id, ctx.user_id)
        .await?
        .ok_or(AppError::NotFound)?;

    let doc_count = info.document_ids.len();

    audit::log(
        &state.db,
        Some(ctx.user_id),
        "persona.deleted",
        Some("persona"),
        Some(&id.to_string()),
        None,
        Some(json!({ "documents_removed": doc_count })),
    )
    .await
    .ok();

    let data_dir = state.config.data_dir.clone();
    let avatar_path = info.avatar_path.clone();
    let document_ids = info.document_ids.clone();
    tokio::spawn(async move {
        // Remove uploads directory for this persona
        let uploads_dir = data_dir.join("uploads").join(id.to_string());
        if let Err(e) = fs::remove_dir_all(&uploads_dir).await {
            tracing::warn!(path = %uploads_dir.display(), error = %e, "cleanup: remove uploads dir failed");
        }

        // Remove transcripts for each document
        for doc_id in &document_ids {
            let transcript = data_dir.join("transcripts").join(format!("{doc_id}.txt"));
            if let Err(e) = fs::remove_file(&transcript).await {
                if e.kind() != std::io::ErrorKind::NotFound {
                    tracing::warn!(path = %transcript.display(), error = %e, "cleanup: remove transcript failed");
                }
            }
        }

        // Remove avatar
        if let Some(rel_path) = avatar_path {
            let avatar = data_dir.join(&rel_path);
            if let Err(e) = fs::remove_file(&avatar).await {
                if e.kind() != std::io::ErrorKind::NotFound {
                    tracing::warn!(path = %avatar.display(), error = %e, "cleanup: remove avatar failed");
                }
            }
        }
    });

    Ok(StatusCode::NO_CONTENT)
}

// ─── Avatar handlers ──────────────────────────────────────────────────────────

const AVATAR_MAX_BYTES: usize = 2 * 1024 * 1024; // 2 MB

fn process_avatar(bytes: &[u8]) -> Result<Vec<u8>, AppError> {
    let img = image::load_from_memory(bytes)
        .map_err(|e| AppError::Internal(anyhow::anyhow!("image decode: {e}")))?;

    let resized = img.thumbnail(512, 512);

    let mut buf = std::io::Cursor::new(Vec::<u8>::new());
    resized
        .write_to(&mut buf, image::ImageFormat::WebP)
        .map_err(|e| AppError::Internal(anyhow::anyhow!("webp encode: {e}")))?;

    Ok(buf.into_inner())
}

pub async fn upload_avatar(
    State(state): State<AppState>,
    ctx: UserCtx,
    Path(id): Path<Uuid>,
    mut multipart: Multipart,
) -> Result<impl IntoResponse, AppError> {
    // Verify persona exists and belongs to this user
    let _ = persona_repo::find_by_id(&state.db, id, ctx.user_id)
        .await?
        .ok_or(AppError::NotFound)?;

    let mut file_bytes: Option<Vec<u8>> = None;

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| AppError::Validation(format!("Multipart error: {e}")))?
    {
        if matches!(field.name(), Some("avatar" | "file")) {
            let bytes = field
                .bytes()
                .await
                .map_err(|e| AppError::Validation(format!("Field read error: {e}")))?;
            file_bytes = Some(bytes.to_vec());
            break;
        }
    }

    let bytes = file_bytes.ok_or_else(|| AppError::Validation("Missing 'avatar' field.".into()))?;

    if bytes.len() > AVATAR_MAX_BYTES {
        return Err(AppError::PayloadTooLarge);
    }

    // Detect MIME from magic bytes (not Content-Type header)
    let mime = infer::get(&bytes).map(|t| t.mime_type());
    match mime {
        Some("image/jpeg") | Some("image/png") | Some("image/webp") => {}
        _ => return Err(AppError::UnsupportedMediaType),
    }

    let webp_bytes = tokio::task::spawn_blocking(move || process_avatar(&bytes))
        .await
        .map_err(|e| AppError::Internal(anyhow::anyhow!("spawn blocking: {e}")))??;

    // Persist to /data/avatars/<id>.webp
    let avatars_dir = state.config.data_dir.join("avatars");
    fs::create_dir_all(&avatars_dir)
        .await
        .map_err(|e| AppError::Internal(anyhow::anyhow!("create avatars dir: {e}")))?;

    let filename = format!("{id}.webp");
    let file_path = avatars_dir.join(&filename);
    fs::write(&file_path, &webp_bytes)
        .await
        .map_err(|e| AppError::Internal(anyhow::anyhow!("write avatar: {e}")))?;

    let rel_path = format!("avatars/{filename}");
    let updated = persona_repo::set_avatar_path(&state.db, id, ctx.user_id, &rel_path).await?;
    if !updated {
        // Persona was deleted concurrently between ownership check and write — clean up orphan.
        if let Err(e) = fs::remove_file(&file_path).await {
            tracing::warn!(path = %file_path.display(), error = %e, "cleanup: remove orphaned avatar failed");
        }
        return Err(AppError::NotFound);
    }

    Ok(Json(json!({ "avatar_path": rel_path })))
}

pub async fn get_avatar(
    State(state): State<AppState>,
    ctx: UserCtx,
    Path(id): Path<Uuid>,
) -> Result<Response, AppError> {
    let persona = persona_repo::find_by_id(&state.db, id, ctx.user_id)
        .await?
        .ok_or(AppError::NotFound)?;

    let rel_path = persona.avatar_path.ok_or(AppError::NotFound)?;
    let file_path = state.config.data_dir.join(&rel_path);

    let bytes = fs::read(&file_path).await.map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            AppError::NotFound
        } else {
            AppError::Internal(anyhow::anyhow!("read avatar: {e}"))
        }
    })?;

    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "image/webp")
        .header(header::CACHE_CONTROL, "private, max-age=3600")
        .body(Body::from(bytes))
        .map_err(|e| AppError::Internal(anyhow::anyhow!("build response: {e}")))
}

pub async fn delete_avatar(
    State(state): State<AppState>,
    ctx: UserCtx,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    let persona = persona_repo::find_by_id(&state.db, id, ctx.user_id)
        .await?
        .ok_or(AppError::NotFound)?;

    let rel_path = persona.avatar_path.ok_or(AppError::NotFound)?;
    let file_path = state.config.data_dir.join(&rel_path);

    persona_repo::clear_avatar_path(&state.db, id, ctx.user_id).await?;

    tokio::spawn(async move {
        if let Err(e) = fs::remove_file(&file_path).await {
            if e.kind() != std::io::ErrorKind::NotFound {
                tracing::warn!(path = %file_path.display(), error = %e, "cleanup: remove avatar failed");
            }
        }
    });

    Ok(StatusCode::NO_CONTENT)
}

// ─── Era handlers ─────────────────────────────────────────────────────────────

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateEraRequest {
    pub label: String,
    pub start_date: Option<String>,
    pub end_date: Option<String>,
    pub description: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PatchEraRequest {
    pub label: Option<String>,
    pub start_date: Option<Option<String>>,
    pub end_date: Option<Option<String>>,
    pub description: Option<Option<String>>,
}

fn validate_era_dates(start_date: Option<Date>, end_date: Option<Date>) -> Result<(), AppError> {
    if let (Some(s), Some(e)) = (start_date, end_date) {
        if e < s {
            return Err(AppError::ValidationFields(HashMap::from([(
                "end_date".to_string(),
                "end_date must not be before start_date.".to_string(),
            )])));
        }
    }
    Ok(())
}

fn validate_era_label(label: &str) -> Result<(), AppError> {
    if label.is_empty() || label.len() > 40 {
        return Err(AppError::ValidationFields(HashMap::from([(
            "label".to_string(),
            "Must be 1–40 characters.".to_string(),
        )])));
    }
    Ok(())
}

pub async fn list_eras(
    State(state): State<AppState>,
    ctx: UserCtx,
    Path(persona_id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    // Verify persona belongs to user
    let _ = persona_repo::find_by_id(&state.db, persona_id, ctx.user_id)
        .await?
        .ok_or(AppError::NotFound)?;

    let eras = era_repo::list(&state.db, persona_id, ctx.user_id).await?;
    Ok(Json(eras))
}

pub async fn create_era(
    State(state): State<AppState>,
    ctx: UserCtx,
    Path(persona_id): Path<Uuid>,
    Json(body): Json<CreateEraRequest>,
) -> Result<impl IntoResponse, AppError> {
    validate_era_label(&body.label)?;

    let start = body
        .start_date
        .as_deref()
        .map(|s| parse_date(s, "start_date"))
        .transpose()?;
    let end = body
        .end_date
        .as_deref()
        .map(|s| parse_date(s, "end_date"))
        .transpose()?;

    validate_era_dates(start, end)?;

    let era = era_repo::create(
        &state.db,
        persona_id,
        ctx.user_id,
        &body.label,
        start,
        end,
        body.description.as_deref(),
    )
    .await?;

    audit::log(
        &state.db,
        Some(ctx.user_id),
        "era.created",
        Some("era"),
        Some(&era.id.to_string()),
        None,
        Some(json!({ "persona_id": persona_id })),
    )
    .await
    .ok();

    Ok((StatusCode::CREATED, Json(era)))
}

pub async fn patch_era(
    State(state): State<AppState>,
    ctx: UserCtx,
    Path((persona_id, era_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<PatchEraRequest>,
) -> Result<impl IntoResponse, AppError> {
    if let Some(ref label) = body.label {
        validate_era_label(label)?;
    }

    let start = body
        .start_date
        .as_ref()
        .map(|opt_s| {
            opt_s
                .as_deref()
                .map(|s| parse_date(s, "start_date"))
                .transpose()
        })
        .transpose()?;

    let end = body
        .end_date
        .as_ref()
        .map(|opt_s| {
            opt_s
                .as_deref()
                .map(|s| parse_date(s, "end_date"))
                .transpose()
        })
        .transpose()?;

    // Fetch existing era once if we need the complementary date for cross-validation.
    let setting_start = start.as_ref().map(|s| s.is_some()).unwrap_or(false);
    let setting_end = end.as_ref().map(|e| e.is_some()).unwrap_or(false);
    let need_existing = (setting_start && end.is_none()) || (setting_end && start.is_none());

    let existing = if need_existing {
        era_repo::find_by_id(&state.db, era_id, persona_id, ctx.user_id).await?
    } else {
        None
    };

    let resolved_start = match start {
        Some(s) => s,
        None => existing.as_ref().and_then(|e| e.start_date),
    };
    let resolved_end = match end {
        Some(e) => e,
        None => existing.as_ref().and_then(|e| e.end_date),
    };

    validate_era_dates(resolved_start, resolved_end)?;

    let era = era_repo::update(
        &state.db,
        era_id,
        persona_id,
        ctx.user_id,
        body.label.as_deref(),
        start,
        end,
        body.description.as_ref().map(|d| d.as_deref()),
    )
    .await?
    .ok_or(AppError::NotFound)?;

    audit::log(
        &state.db,
        Some(ctx.user_id),
        "era.updated",
        Some("era"),
        Some(&era_id.to_string()),
        None,
        Some(json!({ "persona_id": persona_id })),
    )
    .await
    .ok();

    Ok(Json(era))
}

pub async fn delete_era(
    State(state): State<AppState>,
    ctx: UserCtx,
    Path((persona_id, era_id)): Path<(Uuid, Uuid)>,
) -> Result<impl IntoResponse, AppError> {
    // Verify persona belongs to user first
    let _ = persona_repo::find_by_id(&state.db, persona_id, ctx.user_id)
        .await?
        .ok_or(AppError::NotFound)?;

    let deleted = era_repo::delete(&state.db, era_id, persona_id, ctx.user_id).await?;
    if !deleted {
        return Err(AppError::NotFound);
    }

    audit::log(
        &state.db,
        Some(ctx.user_id),
        "era.deleted",
        Some("era"),
        Some(&era_id.to_string()),
        None,
        Some(json!({ "persona_id": persona_id })),
    )
    .await
    .ok();

    Ok(StatusCode::NO_CONTENT)
}
