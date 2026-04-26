/// Routes for provider config management.
///
/// GET    /api/providers          — list user's configs (api_key masked)
/// POST   /api/providers          — add config (201)
/// PATCH  /api/providers/:id      — partial update
/// DELETE /api/providers/:id      — delete (local providers return 409)
/// POST   /api/providers/:id/test — test connectivity
use crate::{
    auth::middleware::UserCtx,
    error::AppError,
    repositories::provider_configs::{
        self as repo, CreateProviderRequest, PatchProviderRequest, ProviderConfig,
    },
    services::providers::encrypt,
    state::AppState,
};
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde_json::{json, Value};
use uuid::Uuid;

// ─── Helpers ──────────────────────────────────────────────────────────────────

/// Mask a ProviderConfig for API responses — replaces `enc_api_key` in config
/// with `api_key_hint` (last 4 chars of the plaintext key).
fn mask_config(mut config: ProviderConfig, app_secret: &str) -> Value {
    let api_key_hint = if let Some(enc) = config.config.get("enc_api_key").and_then(|v| v.as_str())
    {
        match encrypt::decrypt_api_key(enc, app_secret) {
            Ok(key) => Some(encrypt::api_key_hint(&key)),
            Err(_) => Some("****".to_string()),
        }
    } else {
        None
    };

    // Remove enc_api_key from the response; replace with hint
    if let Some(obj) = config.config.as_object_mut() {
        obj.remove("enc_api_key");
        if let Some(hint) = api_key_hint {
            obj.insert("api_key_hint".to_string(), json!(hint));
        }
    }

    json!({
        "id": config.id,
        "service": config.service,
        "provider": config.provider,
        "priority": config.priority,
        "config": config.config,
        "enabled": config.enabled,
        "created_at": config.created_at
    })
}

// ─── GET /api/providers ───────────────────────────────────────────────────────

pub async fn list_providers(
    State(state): State<AppState>,
    ctx: UserCtx,
) -> Result<impl IntoResponse, AppError> {
    let configs = repo::list_for_user(&state.db, ctx.user_id).await?;
    let masked: Vec<Value> = configs
        .into_iter()
        .map(|c| mask_config(c, &state.config.session_secret))
        .collect();
    Ok(Json(json!({ "providers": masked })))
}

// ─── POST /api/providers ──────────────────────────────────────────────────────

pub async fn create_provider(
    State(state): State<AppState>,
    ctx: UserCtx,
    Json(body): Json<CreateProviderRequest>,
) -> Result<impl IntoResponse, AppError> {
    // Validate service value
    let valid_services = ["transcription", "llm", "embeddings"];
    if !valid_services.contains(&body.service.as_str()) {
        return Err(AppError::Validation(format!(
            "service must be one of: {}",
            valid_services.join(", ")
        )));
    }

    // Encrypt api_key if provided in config
    let config = encrypt_config_key(
        body.config.unwrap_or(serde_json::Value::Object(Default::default())),
        &state.config.session_secret,
    )?;

    let priority = body.priority.unwrap_or(10);

    let created = repo::create(&state.db, ctx.user_id, &body.service, &body.provider, priority, config).await?;

    let masked = mask_config(created, &state.config.session_secret);
    Ok((StatusCode::CREATED, Json(masked)))
}

// ─── PATCH /api/providers/:id ────────────────────────────────────────────────

pub async fn patch_provider(
    State(state): State<AppState>,
    ctx: UserCtx,
    Path(id): Path<Uuid>,
    Json(body): Json<PatchProviderRequest>,
) -> Result<impl IntoResponse, AppError> {
    // Encrypt api_key in config patch if provided
    let config = if let Some(cfg) = body.config {
        Some(encrypt_config_key(cfg, &state.config.session_secret)?)
    } else {
        None
    };

    let fields = repo::UpdateFields {
        priority: body.priority,
        config,
        enabled: body.enabled,
    };

    let updated = repo::update(&state.db, id, ctx.user_id, fields).await?;
    let masked = mask_config(updated, &state.config.session_secret);
    Ok(Json(masked))
}

// ─── DELETE /api/providers/:id ───────────────────────────────────────────────

pub async fn delete_provider(
    State(state): State<AppState>,
    ctx: UserCtx,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    repo::delete(&state.db, id, ctx.user_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

// ─── POST /api/providers/:id/test ────────────────────────────────────────────

pub async fn test_provider(
    State(state): State<AppState>,
    ctx: UserCtx,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    let config = repo::find(&state.db, id, ctx.user_id)
        .await?
        .ok_or(AppError::NotFound)?;

    let result = run_provider_test(&config, &state.config.session_secret).await;

    match result {
        Ok(()) => Ok((StatusCode::OK, Json(json!({ "ok": true })))),
        Err(e) => Ok((
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(json!({ "ok": false, "error": e })),
        )),
    }
}

// ─── Internal helpers ─────────────────────────────────────────────────────────

/// If the config contains a plaintext `api_key`, encrypt it and store as `enc_api_key`.
fn encrypt_config_key(
    mut config: Value,
    app_secret: &str,
) -> Result<Value, AppError> {
    if let Some(obj) = config.as_object_mut() {
        if let Some(raw_key) = obj.remove("api_key") {
            if let Some(key_str) = raw_key.as_str() {
                if !key_str.is_empty() {
                    let enc = encrypt::encrypt_api_key(key_str, app_secret)
                        .map_err(AppError::Internal)?;
                    obj.insert("enc_api_key".to_string(), json!(enc));
                }
            }
        }
    }
    Ok(config)
}

/// Run a minimal connectivity probe for a provider.
async fn run_provider_test(config: &ProviderConfig, app_secret: &str) -> Result<(), String> {
    match config.provider.as_str() {
        "local_whisper" | "local_llama" | "local_bge" => {
            // Local providers are always "up" from a connectivity standpoint
            Ok(())
        }
        "openai_compat" => {
            let enc = config.config["enc_api_key"]
                .as_str()
                .ok_or_else(|| "no api_key configured".to_string())?;
            let api_key = encrypt::decrypt_api_key(enc, app_secret)
                .map_err(|e| format!("key decrypt failed: {e}"))?;
            let endpoint = config.config["endpoint"]
                .as_str()
                .unwrap_or("https://api.openai.com");
            let model = config.config["model"]
                .as_str()
                .unwrap_or("gpt-4o-mini");

            let url = format!("{}/v1/chat/completions", endpoint.trim_end_matches('/'));
            let payload = json!({
                "model": model,
                "messages": [{"role": "user", "content": "ping"}],
                "max_tokens": 1
            });

            let client = reqwest::Client::new();
            let resp = client
                .post(&url)
                .bearer_auth(&api_key)
                .json(&payload)
                .send()
                .await
                .map_err(|e| format!("request failed: {e}"))?;

            if resp.status().is_success() {
                Ok(())
            } else {
                let status = resp.status();
                let body = resp.text().await.unwrap_or_default();
                Err(format!("API returned {status}: {body}"))
            }
        }
        "google_speech" => {
            let enc = config.config["enc_api_key"]
                .as_str()
                .ok_or_else(|| "no api_key configured".to_string())?;
            let api_key = encrypt::decrypt_api_key(enc, app_secret)
                .map_err(|e| format!("key decrypt failed: {e}"))?;

            // Minimal probe: list speech models (lightweight GET endpoint)
            let url = format!(
                "https://speech.googleapis.com/v1/operations?key={}",
                api_key
            );
            let client = reqwest::Client::new();
            let resp = client
                .get(&url)
                .send()
                .await
                .map_err(|e| format!("request failed: {e}"))?;

            if resp.status().is_success() || resp.status().as_u16() == 200 {
                Ok(())
            } else {
                let status = resp.status();
                let body = resp.text().await.unwrap_or_default();
                Err(format!("Google Speech API returned {status}: {body}"))
            }
        }
        other => Err(format!("unknown provider type '{other}' — cannot test")),
    }
}
