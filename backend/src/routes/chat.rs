use crate::{
    auth::middleware::UserCtx,
    error::AppError,
    repositories::{chats as chat_repo, eras::Era, personas::Persona, style_profiles},
    services::{
        embedder::Embedder,
        idempotency,
        llm::CompletionRequest,
        prompt::{build_persona_prompt, has_ai_leakage},
        retriever::{self, RetrievalQuery, RetrievedChunk},
    },
    state::AppState,
};
use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse,
    },
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    convert::Infallible,
    sync::{atomic::Ordering, Arc},
    time::Duration,
};
use tokio_stream::wrappers::UnboundedReceiverStream;
use uuid::Uuid;

const DEFAULT_MODEL_ID: &str = "qwen2.5-7b-instruct-q4_k_m";
const MAX_CONTENT_BYTES: usize = 20_480; // 20 KB
const USER_CONCURRENCY_CAP: u8 = 2;
const SEMAPHORE_TIMEOUT_SECS: u64 = 20;

// ─── Create session ───────────────────────────────────────────────────────────

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CreateSessionRequest {
    pub era_id: Option<Uuid>,
    pub model_id: Option<String>,
    pub temperature: Option<f32>,
    pub top_p: Option<f32>,
}

pub async fn create_session(
    ctx: UserCtx,
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(persona_id): Path<Uuid>,
    Json(body): Json<CreateSessionRequest>,
) -> Result<impl IntoResponse, AppError> {
    let user_id = ctx.user_id;
    let idempotency_key = headers
        .get("idempotency-key")
        .and_then(|v| v.to_str().ok())
        .ok_or(AppError::Validation(
            "Idempotency-Key header required.".into(),
        ))?;
    Uuid::parse_str(idempotency_key)
        .map_err(|_| AppError::Validation("Idempotency-Key must be a UUID.".into()))?;

    // Verify persona ownership
    fetch_persona(&state.db, user_id, persona_id).await?;

    let hash = idempotency::body_hash(&body);
    if let Some(hit) =
        idempotency::check(&state.db, idempotency_key, user_id, "create_session", &hash).await?
    {
        return Ok((
            StatusCode::from_u16(hit.status as u16).unwrap_or(StatusCode::OK),
            Json(hit.body),
        )
            .into_response());
    }

    let temperature = body.temperature.unwrap_or(0.7).clamp(0.0, 2.0);
    let top_p = body.top_p.unwrap_or(0.9).clamp(0.0, 1.0);
    let model_id = body
        .model_id
        .as_deref()
        .unwrap_or(DEFAULT_MODEL_ID)
        .to_string();

    let session = chat_repo::create_session(
        &state.db,
        user_id,
        persona_id,
        body.era_id,
        &model_id,
        temperature,
        top_p,
    )
    .await?;

    let resp = Json(json!({
        "id": session.id,
        "persona_id": session.persona_id,
        "era_id": session.era_id,
        "model_id": session.model_id,
        "temperature": session.temperature,
        "top_p": session.top_p,
        "title": session.title,
        "created_at": session.created_at,
        "updated_at": session.updated_at,
    }));

    idempotency::store(
        &state.db,
        idempotency_key,
        user_id,
        "create_session",
        &hash,
        201,
        resp.0.clone(),
    )
    .await?;

    Ok((StatusCode::CREATED, resp).into_response())
}

// ─── List sessions ────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct ListSessionsQuery {
    pub cursor: Option<String>,
    pub limit: Option<i64>,
}

pub async fn list_sessions(
    ctx: UserCtx,
    State(state): State<AppState>,
    Path(persona_id): Path<Uuid>,
    Query(q): Query<ListSessionsQuery>,
) -> Result<impl IntoResponse, AppError> {
    let user_id = ctx.user_id;
    fetch_persona(&state.db, user_id, persona_id).await?;

    let limit = q.limit.unwrap_or(20).clamp(1, 100);
    let (items, next_cursor) =
        chat_repo::list_sessions(&state.db, user_id, persona_id, q.cursor.as_deref(), limit)
            .await?;

    Ok(Json(json!({
        "items": items,
        "next_cursor": next_cursor,
    })))
}

// ─── Get session ──────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct GetSessionQuery {
    pub cursor: Option<String>,
    pub limit: Option<i64>,
}

pub async fn get_session(
    ctx: UserCtx,
    State(state): State<AppState>,
    Path(session_id): Path<Uuid>,
    Query(q): Query<GetSessionQuery>,
) -> Result<impl IntoResponse, AppError> {
    let user_id = ctx.user_id;
    let session = chat_repo::get_session(&state.db, user_id, session_id)
        .await?
        .ok_or(AppError::NotFound)?;

    let limit = q.limit.unwrap_or(50).clamp(1, 200);
    let (messages, next_cursor) =
        chat_repo::list_messages(&state.db, user_id, session_id, q.cursor.as_deref(), limit)
            .await?;

    // Messages are returned oldest-first for display
    let mut msgs = messages;
    msgs.reverse();

    Ok(Json(json!({
        "session": session,
        "messages": msgs,
        "next_cursor": next_cursor,
    })))
}

// ─── Delete session ───────────────────────────────────────────────────────────

pub async fn delete_session(
    ctx: UserCtx,
    State(state): State<AppState>,
    Path(session_id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    let user_id = ctx.user_id;
    let deleted = chat_repo::delete_session(&state.db, user_id, session_id).await?;
    if !deleted {
        return Err(AppError::NotFound);
    }
    Ok(StatusCode::NO_CONTENT)
}

// ─── Post message (SSE) ───────────────────────────────────────────────────────

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PostMessageRequest {
    pub content: String,
}

pub async fn post_message(
    ctx: UserCtx,
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(session_id): Path<Uuid>,
    Json(body): Json<PostMessageRequest>,
) -> Result<Sse<impl futures::Stream<Item = Result<Event, Infallible>>>, AppError> {
    let user_id = ctx.user_id;
    // Validate Idempotency-Key
    let idempotency_key = headers
        .get("idempotency-key")
        .and_then(|v| v.to_str().ok())
        .ok_or(AppError::Validation(
            "Idempotency-Key header required.".into(),
        ))?
        .to_string();
    Uuid::parse_str(&idempotency_key)
        .map_err(|_| AppError::Validation("Idempotency-Key must be a UUID.".into()))?;

    // Content length cap
    if body.content.len() > MAX_CONTENT_BYTES {
        return Err(AppError::PayloadTooLarge);
    }
    if body.content.trim().is_empty() {
        return Err(AppError::Validation("content must not be empty.".into()));
    }

    // Verify session ownership
    let session = chat_repo::get_session(&state.db, user_id, session_id)
        .await?
        .ok_or(AppError::NotFound)?;

    // Idempotency check — if this key was used before, replay the stored assistant message
    let body_hash = idempotency::body_hash(&body.content);
    if let Some(hit) = idempotency::check(
        &state.db,
        &idempotency_key,
        user_id,
        "post_message",
        &body_hash,
    )
    .await?
    {
        // Replay: stream the stored assistant message
        let assistant_msg_id = hit.body["assistant_message_id"]
            .as_str()
            .and_then(|s| Uuid::parse_str(s).ok())
            .unwrap_or(Uuid::nil());
        let stored_content = hit.body["content"].as_str().unwrap_or("").to_string();
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<Result<Event, Infallible>>();
        tokio::spawn(async move {
            let meta = json!({"assistant_message_id": assistant_msg_id, "retrieved_chunk_ids": [], "replay": true});
            let _ = tx.send(Ok(Event::default().event("meta").data(meta.to_string())));
            for word in stored_content.split(' ') {
                let ev = Event::default()
                    .event("token")
                    .data(json!({"t": format!("{word} ")}).to_string());
                let _ = tx.send(Ok(ev));
            }
            let done = json!({"assistant_message_id": assistant_msg_id, "tokens_in": 0, "tokens_out": 0, "finish_reason": "replay"});
            let _ = tx.send(Ok(Event::default().event("done").data(done.to_string())));
        });
        let stream = UnboundedReceiverStream::new(rx);
        return Ok(Sse::new(stream).keep_alive(
            KeepAlive::new()
                .interval(Duration::from_secs(15))
                .text("keep-alive"),
        ));
    }

    // Empty corpus guard
    let chunk_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM chunks WHERE user_id = $1 AND persona_id = $2
         AND ($3::uuid IS NULL OR era_id = $3) AND embedding IS NOT NULL",
    )
    .bind(user_id)
    .bind(session.persona_id)
    .bind(session.era_id)
    .fetch_one(&state.db)
    .await
    .map_err(AppError::Database)?;

    if chunk_count == 0 {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<Result<Event, Infallible>>();
        let db = state.db.clone();
        let uid = user_id;
        let sid = session_id;
        let content_clone = body.content.clone();
        let ik = idempotency_key.clone();
        let bh = body_hash.clone();
        tokio::spawn(async move {
            // Persist user message
            let _ = chat_repo::create_message(&db, uid, sid, "user", &content_clone).await;
            let asst_msg = chat_repo::create_message(
                &db,
                uid,
                sid,
                "assistant",
                "I don't have any writing to draw from yet for this persona. Upload some documents under this era and I'll sound like them.",
            )
            .await;
            let asst_id = asst_msg.as_ref().map(|m| m.id).unwrap_or(Uuid::nil());

            let meta = json!({"assistant_message_id": asst_id, "retrieved_chunk_ids": [], "synthetic": true});
            let _ = tx.send(Ok(Event::default().event("meta").data(meta.to_string())));

            for token in &[
                "I don't have any writing to draw from yet for this persona. ",
                "Upload some documents under this era and I'll sound like them.",
            ] {
                let ev = Event::default()
                    .event("token")
                    .data(json!({"t": token}).to_string());
                let _ = tx.send(Ok(ev));
            }
            let done = json!({"assistant_message_id": asst_id, "tokens_in": 0, "tokens_out": 0, "finish_reason": "synthetic"});
            let _ = tx.send(Ok(Event::default().event("done").data(done.to_string())));

            // Store idempotency record
            let _ = idempotency::store(
                &db,
                &ik,
                uid,
                "post_message",
                &bh,
                200,
                json!({"assistant_message_id": asst_id, "content": "I don't have any writing to draw from yet for this persona. Upload some documents under this era and I'll sound like them."}),
            )
            .await;
        });
        let stream = UnboundedReceiverStream::new(rx);
        return Ok(Sse::new(stream).keep_alive(
            KeepAlive::new()
                .interval(Duration::from_secs(15))
                .text("keep-alive"),
        ));
    }

    // Acquire server-wide generation semaphore (timeout 20s)
    let permit = tokio::time::timeout(
        Duration::from_secs(SEMAPHORE_TIMEOUT_SECS),
        Arc::clone(&state.generation_semaphore).acquire_owned(),
    )
    .await
    .map_err(|_| AppError::ServerBusy)?
    .map_err(|_| AppError::ServerBusy)?;

    // Acquire per-user concurrency slot
    let counter = state
        .user_generation_counts
        .entry(user_id)
        .or_insert_with(|| Arc::new(std::sync::atomic::AtomicU8::new(0)))
        .clone();
    let prev = counter.fetch_update(Ordering::SeqCst, Ordering::SeqCst, |v| {
        if v < USER_CONCURRENCY_CAP {
            Some(v + 1)
        } else {
            None
        }
    });
    if prev.is_err() {
        drop(permit);
        return Err(AppError::GenerationConcurrencyExceeded);
    }

    // Persist user message + set session title
    let user_msg =
        chat_repo::create_message(&state.db, user_id, session_id, "user", &body.content).await?;
    let title_snippet: String = body.content.chars().take(50).collect();
    let _ = chat_repo::update_session_title(&state.db, session_id, &title_snippet).await;

    // Fetch persona + era + style profile for prompt building
    let persona = fetch_persona(&state.db, user_id, session.persona_id).await?;
    let era = if let Some(era_id) = session.era_id {
        fetch_era(&state.db, user_id, era_id).await.ok().flatten()
    } else {
        None
    };
    let profile = style_profiles::find(&state.db, session.persona_id, session.era_id, user_id)
        .await
        .ok()
        .flatten();
    let profile_json = profile.as_ref().map(|p| &p.profile);

    // Retrieve chunks
    let embedder = Embedder::new(&state.config.model_dir)
        .map_err(|e| AppError::Internal(anyhow::anyhow!("embedder init: {e}")))?;

    let retrieved = retriever::retrieve(
        &state.db,
        &embedder,
        &RetrievalQuery {
            user_id,
            persona_id: session.persona_id,
            era_id: session.era_id,
            query_text: &body.content,
            k: 8,
        },
    )
    .await?;

    // Prepend profile exemplars
    let exemplar_ids: Vec<Uuid> = profile_json
        .and_then(|p| p["exemplars"].as_array())
        .map(|arr| {
            arr.iter()
                .take(3)
                .filter_map(|e| e["chunk_id"].as_str())
                .filter_map(|s| Uuid::parse_str(s).ok())
                .collect()
        })
        .unwrap_or_default();

    let exemplars = if exemplar_ids.is_empty() {
        vec![]
    } else {
        fetch_chunks_by_ids(&state.db, &exemplar_ids, user_id, session.persona_id).await?
    };

    // Build prompt
    let system_prompt =
        build_persona_prompt(&persona, era.as_ref(), profile_json, &exemplars, &retrieved);

    // Fetch prior messages for context (last 20)
    let (prior_msgs, _) =
        chat_repo::list_messages(&state.db, user_id, session_id, None, 20).await?;
    // prior_msgs is newest-first; we want oldest-first for context
    let mut history: Vec<(crate::services::llm::Role, String)> = prior_msgs
        .into_iter()
        .rev()
        .filter(|m| m.id != user_msg.id && (m.role == "user" || m.role == "assistant"))
        .map(|m| {
            let role = if m.role == "user" {
                crate::services::llm::Role::User
            } else {
                crate::services::llm::Role::Assistant
            };
            (role, m.content)
        })
        .collect();
    history.push((crate::services::llm::Role::User, body.content.clone()));

    let llm_req = CompletionRequest {
        system: system_prompt,
        messages: history,
        temperature: session.temperature,
        top_p: session.top_p,
        max_tokens: 512,
    };

    let retrieved_ids: Vec<Uuid> = retrieved.iter().map(|c| c.id).collect();
    let llm = state.llm.clone();
    let db = state.db.clone();
    let ik = idempotency_key.clone();
    let bh = body_hash.clone();

    let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<Result<Event, Infallible>>();
    let tx2 = tx.clone();

    tokio::spawn(async move {
        // _permit and counter are dropped when this task ends (auto-release)
        let _permit = permit;
        let _counter_guard = CounterGuard { counter };

        // Pre-persist assistant message (empty, will be updated)
        let asst_msg =
            match chat_repo::create_message(&db, user_id, session_id, "assistant", "").await {
                Ok(m) => m,
                Err(e) => {
                    let ev = Event::default()
                        .event("error")
                        .data(json!({"code": "internal", "message": e.to_string()}).to_string());
                    let _ = tx2.send(Ok(ev));
                    return;
                }
            };
        let asst_id = asst_msg.id;

        // Emit meta
        let meta = json!({
            "assistant_message_id": asst_id,
            "retrieved_chunk_ids": retrieved_ids,
        });
        let _ = tx2.send(Ok(Event::default().event("meta").data(meta.to_string())));

        // Generate or use stub
        let generation_result = match llm {
            Some(ref llm_arc) => {
                let llm_clone = Arc::clone(llm_arc);
                let req_clone = llm_req;
                tokio::task::spawn_blocking(move || llm_clone.generate(&req_clone, 3))
                    .await
                    .ok()
                    .and_then(|r| r.ok())
            }
            None => {
                // Model not loaded — synthetic response
                Some(crate::services::llm::GenerationResult {
                    tokens: vec![
                        "I".to_string(),
                        " can't".to_string(),
                        " respond".to_string(),
                        " right".to_string(),
                        " now".to_string(),
                        " —".to_string(),
                        " the".to_string(),
                        " model".to_string(),
                        " isn't".to_string(),
                        " loaded.".to_string(),
                        " Ask".to_string(),
                        " the".to_string(),
                        " administrator".to_string(),
                        " to".to_string(),
                        " place".to_string(),
                        " the".to_string(),
                        " GGUF".to_string(),
                        " file".to_string(),
                        " in".to_string(),
                        " /data/models/llm/.".to_string(),
                    ],
                    tokens_in: 0,
                    tokens_out: 0,
                    finish_reason: "model_not_loaded".to_string(),
                })
            }
        };

        let (final_content, tokens_in, tokens_out, finish_reason) = match generation_result {
            Some(result) => {
                // Check post-filter (only for real LLM output, not stub)
                let full_text: String = result.tokens.join("");
                let finish = result.finish_reason.clone();
                let (content, fr) = if finish != "model_not_loaded" && has_ai_leakage(&full_text) {
                    (
                        "Could not stay in voice — try a different prompt.".to_string(),
                        "voice_break".to_string(),
                    )
                } else {
                    (full_text, finish)
                };
                (content, result.tokens_in, result.tokens_out, fr)
            }
            None => (
                "Generation failed. Please try again.".to_string(),
                0,
                0,
                "error".to_string(),
            ),
        };

        // Stream the final content token-by-token (from buffer)
        for word in final_content.split(' ') {
            let t = format!("{word} ");
            let ev = Event::default()
                .event("token")
                .data(json!({"t": t}).to_string());
            if tx2.send(Ok(ev)).is_err() {
                break; // client disconnected — continue generating to persist
            }
        }

        // Update assistant message with full content + metadata
        let _ = sqlx::query("UPDATE messages SET content = $1 WHERE id = $2")
            .bind(&final_content)
            .bind(asst_id)
            .execute(&db)
            .await;

        let _ = chat_repo::update_message_metadata(
            &db,
            asst_id,
            &retrieved_ids.to_vec(),
            tokens_in as i32,
            tokens_out as i32,
        )
        .await;

        // Store idempotency
        let _ = idempotency::store(
            &db,
            &ik,
            user_id,
            "post_message",
            &bh,
            200,
            json!({
                "assistant_message_id": asst_id,
                "content": final_content,
            }),
        )
        .await;

        // Done event
        let done = json!({
            "assistant_message_id": asst_id,
            "tokens_in": tokens_in,
            "tokens_out": tokens_out,
            "finish_reason": finish_reason,
        });
        let _ = tx2.send(Ok(Event::default().event("done").data(done.to_string())));
    });

    let stream = UnboundedReceiverStream::new(rx);
    Ok(Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("keep-alive"),
    ))
}

// ─── RAII guard to decrement per-user counter on drop ────────────────────────

struct CounterGuard {
    counter: Arc<std::sync::atomic::AtomicU8>,
}

impl Drop for CounterGuard {
    fn drop(&mut self) {
        self.counter.fetch_sub(1, Ordering::SeqCst);
    }
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

async fn fetch_persona(
    pool: &sqlx::PgPool,
    user_id: Uuid,
    persona_id: Uuid,
) -> Result<Persona, AppError> {
    sqlx::query_as::<_, Persona>(
        "SELECT id, user_id, name, relation, description, avatar_path, birth_year, created_at, updated_at
         FROM personas WHERE id = $1 AND user_id = $2",
    )
    .bind(persona_id)
    .bind(user_id)
    .fetch_optional(pool)
    .await
    .map_err(AppError::Database)?
    .ok_or(AppError::NotFound)
}

async fn fetch_era(
    pool: &sqlx::PgPool,
    user_id: Uuid,
    era_id: Uuid,
) -> Result<Option<Era>, AppError> {
    sqlx::query_as::<_, Era>(
        "SELECT id, persona_id, user_id, label, start_date, end_date, description, created_at, updated_at
         FROM eras WHERE id = $1 AND user_id = $2",
    )
    .bind(era_id)
    .bind(user_id)
    .fetch_optional(pool)
    .await
    .map_err(AppError::Database)
}

async fn fetch_chunks_by_ids(
    pool: &sqlx::PgPool,
    ids: &[Uuid],
    user_id: Uuid,
    persona_id: Uuid,
) -> Result<Vec<RetrievedChunk>, AppError> {
    if ids.is_empty() {
        return Ok(vec![]);
    }
    type Row = (Uuid, String, Uuid, Option<String>);
    let rows: Vec<Row> = sqlx::query_as(
        "SELECT c.id, c.text, c.document_id, d.title
         FROM chunks c
         LEFT JOIN documents d ON d.id = c.document_id
         WHERE c.id = ANY($1) AND c.user_id = $2 AND c.persona_id = $3",
    )
    .bind(ids)
    .bind(user_id)
    .bind(persona_id)
    .fetch_all(pool)
    .await
    .map_err(AppError::Database)?;

    Ok(rows
        .into_iter()
        .map(|(id, text, document_id, doc_title)| RetrievedChunk {
            id,
            text,
            document_id,
            doc_title,
        })
        .collect())
}
