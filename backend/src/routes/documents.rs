/// Document route handlers (sprint 3).
use crate::{
    auth::middleware::UserCtx,
    error::AppError,
    repositories::{documents as doc_repo, personas as persona_repo},
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
use futures::stream::StreamExt;
use serde::Deserialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::convert::Infallible;
use tokio::io::AsyncWriteExt;
use tokio_stream::wrappers::BroadcastStream;
use uuid::Uuid;

// ─── MIME allow-lists ─────────────────────────────────────────────────────────

const TEXT_MIMES: &[&str] = &[
    "text/plain",
    "text/markdown",
    "application/pdf",
    "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
];
const AUDIO_MIMES: &[&str] = &[
    "audio/mpeg",
    "audio/wav",
    "audio/x-wav",
    "audio/mp4",
    "audio/x-m4a",
];

const TEXT_MAX_BYTES: u64 = 25 * 1024 * 1024;
const AUDIO_MAX_BYTES: u64 = 500 * 1024 * 1024;

fn mime_to_ext(mime: &str) -> &'static str {
    match mime {
        "text/plain" => ".txt",
        "text/markdown" => ".md",
        "application/pdf" => ".pdf",
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document" => ".docx",
        "audio/mpeg" => ".mp3",
        "audio/wav" | "audio/x-wav" => ".wav",
        "audio/mp4" | "audio/x-m4a" => ".m4a",
        _ => "",
    }
}

fn kind_for_mime(mime: &str) -> Option<&'static str> {
    if TEXT_MIMES.contains(&mime) {
        Some("text")
    } else if AUDIO_MIMES.contains(&mime) {
        Some("audio")
    } else {
        None
    }
}

// ─── Query params ─────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct ListDocumentsParams {
    pub limit: Option<i64>,
    pub cursor: Option<String>,
    pub era_id: Option<Uuid>,
    pub kind: Option<String>,
    /// Multi-value: `?status=done&status=failed`
    pub status: Option<Vec<String>>,
}

// ─── Handlers ─────────────────────────────────────────────────────────────────

/// POST /api/personas/:id/documents
pub async fn upload_document(
    State(state): State<AppState>,
    ctx: UserCtx,
    Path(persona_id): Path<Uuid>,
    headers: HeaderMap,
    mut multipart: Multipart,
) -> Result<Response, AppError> {
    // Require Idempotency-Key per spec §3.1; validate as UUID.
    let idem_key = headers
        .get("idempotency-key")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| AppError::Validation("Missing Idempotency-Key header.".into()))?
        .to_string();
    Uuid::parse_str(&idem_key)
        .map_err(|_| AppError::Validation("Idempotency-Key must be a valid UUID.".into()))?;

    // Verify persona ownership (404 if not found)
    let _ = persona_repo::find_by_id(&state.db, persona_id, ctx.user_id)
        .await?
        .ok_or(AppError::NotFound)?;

    // Collect multipart fields
    let mut file_bytes_head: Vec<u8> = Vec::new(); // first 512 bytes for sniff
    let mut tmp_path: Option<std::path::PathBuf> = None;
    let mut total_bytes: u64 = 0;
    let mut sha = Sha256::new();
    let mut era_id: Option<Uuid> = None;
    let mut title: Option<String> = None;
    let mut source: Option<String> = None;
    let mut original_filename: Option<String> = None;

    let uploads_tmp = state.config.data_dir.join("uploads").join(".tmp");
    tokio::fs::create_dir_all(&uploads_tmp)
        .await
        .map_err(|e| AppError::Internal(anyhow::anyhow!("create tmp dir: {e}")))?;

    let tmp_id = Uuid::now_v7();
    let tmp_file_path = uploads_tmp.join(tmp_id.to_string());

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| AppError::Validation(format!("Multipart error: {e}")))?
    {
        let name = field.name().unwrap_or("").to_string();
        match name.as_str() {
            "file" => {
                original_filename = field.file_name().map(|s| s.to_string());
                let mut file = tokio::fs::File::create(&tmp_file_path)
                    .await
                    .map_err(|e| AppError::Internal(anyhow::anyhow!("create tmp file: {e}")))?;

                // Stream file to disk while accumulating hash and byte count
                let mut field_stream = field;
                // Pessimistic per-kind size limit: tightened to TEXT_MAX_BYTES once MIME is sniffed.
                let mut stream_limit = AUDIO_MAX_BYTES;
                let mut sniff_done = false;
                loop {
                    let chunk = match field_stream.chunk().await {
                        Ok(Some(c)) => c,
                        Ok(None) => break,
                        Err(e) => {
                            let _ = tokio::fs::remove_file(&tmp_file_path).await;
                            return Err(AppError::Validation(format!("File read error: {e}")));
                        }
                    };

                    total_bytes += chunk.len() as u64;
                    sha.update(&chunk);

                    // Capture first 512 bytes for MIME sniff
                    if file_bytes_head.len() < 512 {
                        let need = (512 - file_bytes_head.len()).min(chunk.len());
                        file_bytes_head.extend_from_slice(&chunk[..need]);
                    }

                    // Once we have 512 bytes, sniff MIME and tighten limit for text files.
                    if !sniff_done && file_bytes_head.len() >= 512 {
                        sniff_done = true;
                        let early_mime = infer::get(&file_bytes_head)
                            .map(|t| t.mime_type())
                            .unwrap_or("application/octet-stream");
                        if TEXT_MIMES.contains(&early_mime) {
                            stream_limit = TEXT_MAX_BYTES;
                        }
                    }

                    if total_bytes > stream_limit {
                        let _ = tokio::fs::remove_file(&tmp_file_path).await;
                        return Err(AppError::PayloadTooLarge);
                    }

                    file.write_all(&chunk)
                        .await
                        .map_err(|e| AppError::Internal(anyhow::anyhow!("write tmp: {e}")))?;
                }
                file.flush()
                    .await
                    .map_err(|e| AppError::Internal(anyhow::anyhow!("flush tmp: {e}")))?;
                tmp_path = Some(tmp_file_path.clone());
            }
            "era_id" => {
                if let Ok(bytes) = field.bytes().await {
                    if let Ok(s) = String::from_utf8(bytes.to_vec()) {
                        era_id = Uuid::parse_str(s.trim()).ok();
                    }
                }
            }
            "title" => {
                if let Ok(bytes) = field.bytes().await {
                    title = String::from_utf8(bytes.to_vec())
                        .ok()
                        .filter(|s| !s.is_empty());
                }
            }
            "source" => {
                if let Ok(bytes) = field.bytes().await {
                    source = String::from_utf8(bytes.to_vec())
                        .ok()
                        .filter(|s| !s.is_empty());
                }
            }
            _ => {
                // Drain unknown fields
                let _ = field.bytes().await;
            }
        }
    }

    let tmp_path = tmp_path.ok_or_else(|| AppError::Validation("Missing 'file' field.".into()))?;

    // MIME sniff from magic bytes (ignore Content-Type header)
    let detected = infer::get(&file_bytes_head);
    let mime_type = detected
        .map(|t| t.mime_type())
        .unwrap_or("application/octet-stream");

    // Validate against allow-list
    let kind = kind_for_mime(mime_type).ok_or_else(|| {
        tokio::task::spawn(async move {
            let _ = tokio::fs::remove_file(&tmp_path).await;
        });
        AppError::UnsupportedMediaType
    })?;

    // Size limit per kind
    let max_bytes = if kind == "text" {
        TEXT_MAX_BYTES
    } else {
        AUDIO_MAX_BYTES
    };
    if total_bytes > max_bytes {
        tokio::fs::remove_file(&tmp_file_path).await.ok();
        return Err(AppError::PayloadTooLarge);
    }

    let content_hash = hex::encode(sha.finalize());

    // Idempotency check (body hash is content_hash + persona_id)
    let body_hash = format!("{persona_id}:{content_hash}");
    if let Some(hit) = idempotency::check(
        &state.db,
        &idem_key,
        ctx.user_id,
        &format!("/api/personas/{persona_id}/documents"),
        &body_hash,
    )
    .await?
    {
        tokio::fs::remove_file(&tmp_file_path).await.ok();
        let status = StatusCode::from_u16(hit.status as u16).unwrap_or(StatusCode::OK);
        return Ok((status, Json(hit.body)).into_response());
    }

    // Duplicate content check
    if let Some(existing_id) =
        doc_repo::find_by_content_hash(&state.db, persona_id, &content_hash).await?
    {
        tokio::fs::remove_file(&tmp_file_path).await.ok();
        let request_id = Uuid::now_v7().to_string();
        return Ok((
            StatusCode::CONFLICT,
            Json(json!({
                "error": { "code": "duplicate", "message": "This document is already uploaded." },
                "document_id": existing_id,
                "request_id": request_id
            })),
        )
            .into_response());
    }

    // Atomic quota check + increment
    let mut tx = state.db.begin().await.map_err(AppError::Database)?;

    let quota_ok =
        doc_repo::atomic_quota_check_and_increment(&mut tx, ctx.user_id, total_bytes as i64)
            .await?;

    if !quota_ok {
        tx.rollback().await.ok();
        tokio::fs::remove_file(&tmp_file_path).await.ok();
        return Err(AppError::QuotaExceeded);
    }
    tx.commit().await.map_err(AppError::Database)?;

    // Move temp file to final path
    let doc_id = Uuid::now_v7();
    let ext = mime_to_ext(mime_type);
    let persona_dir = state
        .config
        .data_dir
        .join("uploads")
        .join(persona_id.to_string());
    tokio::fs::create_dir_all(&persona_dir)
        .await
        .map_err(|e| AppError::Internal(anyhow::anyhow!("create persona dir: {e}")))?;

    let final_filename = format!("{doc_id}{ext}");
    let final_path = persona_dir.join(&final_filename);
    let original_path = format!("uploads/{persona_id}/{final_filename}");

    if let Err(e) = tokio::fs::rename(&tmp_file_path, &final_path).await {
        // rename across filesystems may fail; fallback to copy+remove
        if let Err(e2) = tokio::fs::copy(&tmp_file_path, &final_path).await {
            // Decrement quota since file didn't land
            doc_repo::decrement_quota(&state.db, ctx.user_id, total_bytes as i64)
                .await
                .ok();
            return Err(AppError::Internal(anyhow::anyhow!("move file: {e} / {e2}")));
        }
        tokio::fs::remove_file(&tmp_file_path).await.ok();
    }

    // Use original filename as title fallback
    let doc_title = title.or_else(|| original_filename.clone());

    // Insert document row
    let doc = match doc_repo::create(
        &state.db,
        persona_id,
        ctx.user_id,
        era_id,
        kind,
        mime_type,
        &original_path,
        &content_hash,
        total_bytes as i64,
        doc_title.as_deref(),
        source.as_deref(),
    )
    .await
    {
        Ok(d) => d,
        Err(e) => {
            // Undo: remove file, decrement quota
            tokio::fs::remove_file(&final_path).await.ok();
            doc_repo::decrement_quota(&state.db, ctx.user_id, total_bytes as i64)
                .await
                .ok();
            return Err(e);
        }
    };

    // Enqueue ingest job
    if let Err(e) = sqlx::query(
        "INSERT INTO jobs (kind, user_id, persona_id, payload) VALUES ('ingest_document', $1, $2, $3)",
    )
    .bind(ctx.user_id)
    .bind(persona_id)
    .bind(json!({ "document_id": doc.id }))
    .execute(&state.db)
    .await
    {
        tracing::error!(doc_id=%doc.id, error=%e, "failed to enqueue ingest job");
        // Non-fatal: document row exists, ingest can be triggered manually via reingest
    }

    // Store idempotency record
    let doc_json = serde_json::to_value(&doc)
        .map_err(|e| AppError::Internal(anyhow::anyhow!("serialize document: {e}")))?;
    idempotency::store(
        &state.db,
        &idem_key,
        ctx.user_id,
        &format!("/api/personas/{persona_id}/documents"),
        &body_hash,
        201,
        doc_json.clone(),
    )
    .await
    .ok();

    Ok((StatusCode::CREATED, Json(doc_json)).into_response())
}

/// GET /api/personas/:id/documents
pub async fn list_documents(
    State(state): State<AppState>,
    ctx: UserCtx,
    Path(persona_id): Path<Uuid>,
    Query(params): Query<ListDocumentsParams>,
) -> Result<impl IntoResponse, AppError> {
    // Verify persona ownership
    let _ = persona_repo::find_by_id(&state.db, persona_id, ctx.user_id)
        .await?
        .ok_or(AppError::NotFound)?;

    let limit = params.limit.unwrap_or(50);
    let cursor = params
        .cursor
        .as_deref()
        .map(doc_repo::decode_cursor)
        .transpose()?;

    let kind_ref = params.kind.as_deref();
    let status_ref = params.status.as_deref();

    let mut rows = doc_repo::list(
        &state.db,
        persona_id,
        ctx.user_id,
        limit,
        cursor,
        params.era_id,
        kind_ref,
        status_ref,
    )
    .await?;

    let has_more = rows.len() as i64 > limit;
    if has_more {
        rows.pop();
    }

    let next_cursor = if has_more {
        rows.last()
            .map(|d| doc_repo::encode_cursor(d.created_at, d.id))
    } else {
        None
    };

    Ok(Json(json!({
        "items": rows,
        "next_cursor": next_cursor
    })))
}

/// GET /api/personas/:id/documents/:doc_id
pub async fn get_document(
    State(state): State<AppState>,
    ctx: UserCtx,
    Path((persona_id, doc_id)): Path<(Uuid, Uuid)>,
) -> Result<impl IntoResponse, AppError> {
    let doc = doc_repo::find_by_id(&state.db, doc_id, persona_id, ctx.user_id)
        .await?
        .ok_or(AppError::NotFound)?;
    Ok(Json(doc))
}

/// DELETE /api/personas/:id/documents/:doc_id
pub async fn delete_document(
    State(state): State<AppState>,
    ctx: UserCtx,
    Path((persona_id, doc_id)): Path<(Uuid, Uuid)>,
) -> Result<impl IntoResponse, AppError> {
    // Fetch document first to get paths for cleanup
    let doc = doc_repo::find_by_id(&state.db, doc_id, persona_id, ctx.user_id)
        .await?
        .ok_or(AppError::NotFound)?;

    let original_path = state.config.data_dir.join(&doc.original_path);
    let transcript_path = doc
        .transcript_path
        .as_ref()
        .map(|p| state.config.data_dir.join(p));

    // Delete row + decrement quota atomically in one transaction
    doc_repo::delete_with_quota(&state.db, doc_id, ctx.user_id)
        .await?
        .ok_or(AppError::NotFound)?;

    // Async filesystem cleanup
    tokio::spawn(async move {
        if let Err(e) = tokio::fs::remove_file(&original_path).await {
            if e.kind() != std::io::ErrorKind::NotFound {
                tracing::warn!(path=%original_path.display(), error=%e, "cleanup: remove document file failed");
            }
        }
        if let Some(tp) = transcript_path {
            if let Err(e) = tokio::fs::remove_file(&tp).await {
                if e.kind() != std::io::ErrorKind::NotFound {
                    tracing::warn!(path=%tp.display(), error=%e, "cleanup: remove transcript failed");
                }
            }
        }
    });

    Ok(StatusCode::NO_CONTENT)
}

/// POST /api/personas/:id/documents/:doc_id/reingest
pub async fn reingest_document(
    State(state): State<AppState>,
    ctx: UserCtx,
    Path((persona_id, doc_id)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, AppError> {
    // Require Idempotency-Key
    let idem_key = headers
        .get("idempotency-key")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| AppError::Validation("Missing Idempotency-Key header.".into()))?
        .to_string();

    let doc = doc_repo::find_by_id(&state.db, doc_id, persona_id, ctx.user_id)
        .await?
        .ok_or(AppError::NotFound)?;

    let body_hash = format!("reingest:{doc_id}");
    if let Some(hit) = idempotency::check(
        &state.db,
        &idem_key,
        ctx.user_id,
        &format!("/api/personas/{persona_id}/documents/{doc_id}/reingest"),
        &body_hash,
    )
    .await?
    {
        let status = StatusCode::from_u16(hit.status as u16).unwrap_or(StatusCode::OK);
        return Ok((status, Json(hit.body)).into_response());
    }

    // Guard: reject if an ingest job is currently running for this document
    let running: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM jobs WHERE kind='ingest_document' AND status='running' \
         AND payload->>'document_id' = $1",
    )
    .bind(doc_id.to_string())
    .fetch_one(&state.db)
    .await
    .map_err(AppError::Database)?;

    if running > 0 {
        return Err(AppError::Conflict {
            code: "ingest_running",
        });
    }

    // Delete existing chunks
    sqlx::query("DELETE FROM chunks WHERE document_id = $1")
        .bind(doc_id)
        .execute(&state.db)
        .await
        .map_err(AppError::Database)?;

    // Cancel queued/failed ingest jobs for this document
    sqlx::query(
        "DELETE FROM jobs WHERE kind='ingest_document' AND payload->>'document_id'=$1 AND status IN ('queued','failed')",
    )
    .bind(doc_id.to_string())
    .execute(&state.db)
    .await
    .map_err(AppError::Database)?;

    // Reset document status
    doc_repo::update_status(&state.db, doc_id, "pending", None, None).await?;

    // Enqueue new ingest job
    sqlx::query(
        "INSERT INTO jobs (kind, user_id, persona_id, payload) VALUES ('ingest_document', $1, $2, $3)",
    )
    .bind(ctx.user_id)
    .bind(persona_id)
    .bind(json!({ "document_id": doc_id }))
    .execute(&state.db)
    .await
    .map_err(AppError::Database)?;

    let doc_json = serde_json::to_value(&doc)
        .map_err(|e| AppError::Internal(anyhow::anyhow!("serialize document: {e}")))?;
    idempotency::store(
        &state.db,
        &idem_key,
        ctx.user_id,
        &format!("/api/personas/{persona_id}/documents/{doc_id}/reingest"),
        &body_hash,
        200,
        doc_json.clone(),
    )
    .await
    .ok();

    Ok((StatusCode::OK, Json(doc_json)).into_response())
}

/// GET /api/personas/:id/documents/:doc_id/transcript
pub async fn get_transcript(
    State(state): State<AppState>,
    ctx: UserCtx,
    Path((persona_id, doc_id)): Path<(Uuid, Uuid)>,
) -> Result<Response, AppError> {
    let doc = doc_repo::find_by_id(&state.db, doc_id, persona_id, ctx.user_id)
        .await?
        .ok_or(AppError::NotFound)?;

    let transcript_rel = doc.transcript_path.ok_or(AppError::NotFound)?;
    let transcript_path = state.config.data_dir.join(&transcript_rel);

    let content = tokio::fs::read(&transcript_path).await.map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            AppError::NotFound
        } else {
            AppError::Internal(anyhow::anyhow!("read transcript: {e}"))
        }
    })?;

    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/plain; charset=utf-8")
        .body(Body::from(content))
        .map_err(|e| AppError::Internal(anyhow::anyhow!("build response: {e}")))
}

/// GET /api/personas/:id/documents/events — SSE stream for ingest progress
pub async fn document_events(
    State(state): State<AppState>,
    Path(persona_id): Path<Uuid>,
    ctx: UserCtx,
) -> impl IntoResponse {
    let rx = state.ingest_tx.subscribe();
    let user_id = ctx.user_id;

    let stream = BroadcastStream::new(rx).filter_map(move |result| async move {
        let event = result.ok()?;
        if event.user_id != user_id || event.persona_id != persona_id {
            return None;
        }
        let data = serde_json::to_string(&event).ok()?;
        Some(Ok::<_, Infallible>(
            axum::response::sse::Event::default().data(data),
        ))
    });

    axum::response::sse::Sse::new(stream).keep_alive(
        axum::response::sse::KeepAlive::new()
            .interval(std::time::Duration::from_secs(15))
            .text("keep-alive"),
    )
}
