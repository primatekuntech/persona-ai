/// Ingest job orchestrator: parse/transcribe → chunk → embed → enqueue profile job.
use crate::{
    config::AppConfig,
    error::AppError,
    repositories::documents as doc_repo,
    services::{broadcast, broadcast::IngestEvent},
};
use sqlx::PgPool;
use uuid::Uuid;

fn emit(
    tx: &broadcast::Sender,
    user_id: Uuid,
    persona_id: Uuid,
    document_id: Uuid,
    status: &str,
    progress_pct: Option<i16>,
    error: Option<String>,
) {
    let event = IngestEvent {
        user_id,
        persona_id,
        document_id,
        status: status.to_string(),
        progress_pct,
        error,
    };
    // Ignore send error (no subscribers is OK).
    let _ = tx.send(event);
}

/// Run the full ingest pipeline for a single document.
/// This function is called from within a background worker task.
pub async fn run_ingest(
    pool: &PgPool,
    config: &AppConfig,
    document_id: Uuid,
    ingest_tx: &broadcast::Sender,
) -> Result<(), AppError> {
    // Fetch document row
    let doc = sqlx::query_as::<_, doc_repo::Document>(
        "SELECT id, persona_id, era_id, user_id, kind, mime_type, original_path, \
         transcript_path, content_hash, size_bytes, title, source, word_count, duration_sec, \
         progress_pct, status, error, created_at, ingested_at \
         FROM documents WHERE id = $1",
    )
    .bind(document_id)
    .fetch_optional(pool)
    .await
    .map_err(AppError::Database)?
    .ok_or(AppError::NotFound)?;

    let user_id = doc.user_id;
    let persona_id = doc.persona_id;

    emit(
        ingest_tx,
        user_id,
        persona_id,
        document_id,
        &doc.status,
        doc.progress_pct,
        doc.error.clone(),
    );

    // Step 1: Parse/transcribe
    let (text, transcript_path, duration_sec) = if doc.kind == "audio" {
        // Update status to 'transcribing'
        doc_repo::update_status(pool, document_id, "transcribing", Some(0), None).await?;
        emit(
            ingest_tx,
            user_id,
            persona_id,
            document_id,
            "transcribing",
            Some(0),
            None,
        );

        let original_path = doc.original_path.clone();
        let data_dir = config.data_dir.clone();
        let model_dir = config.model_dir.clone();
        let pool_clone = pool.clone();
        let ingest_tx_clone = ingest_tx.clone();

        let result = tokio::task::spawn_blocking(move || {
            let transcriber = crate::services::transcriber::Transcriber::new(
                &model_dir.join("whisper/ggml-small.en.bin"),
            )
            .map_err(crate::services::transcriber::TranscriberError::Other)?;

            // Join data_dir with the relative original_path stored in the DB.
            let audio_path = data_dir.join(&original_path);
            let progress_pool = pool_clone.clone();
            let progress_tx = ingest_tx_clone.clone();

            let (transcript_text, dur) = transcriber.transcribe(&audio_path, move |pct| {
                // Update progress in DB and emit SSE (best-effort, ignore errors)
                let rt = tokio::runtime::Handle::try_current();
                if let Ok(handle) = rt {
                    let pool2 = progress_pool.clone();
                    let tx2 = progress_tx.clone();
                    handle.spawn(async move {
                        let _ = doc_repo::update_status(
                            &pool2,
                            document_id,
                            "transcribing",
                            Some(pct),
                            None,
                        )
                        .await;
                        emit(
                            &tx2,
                            user_id,
                            persona_id,
                            document_id,
                            "transcribing",
                            Some(pct),
                            None,
                        );
                    });
                }
            })?;

            // Save transcript; store relative path so handlers can join with data_dir.
            let transcript_dir = data_dir.join("transcripts");
            std::fs::create_dir_all(&transcript_dir)
                .map_err(|e| crate::services::transcriber::TranscriberError::Other(e.into()))?;
            let tpath_abs = transcript_dir.join(format!("{document_id}.txt"));
            std::fs::write(&tpath_abs, &transcript_text)
                .map_err(|e| crate::services::transcriber::TranscriberError::Other(e.into()))?;
            let tpath_rel = format!("transcripts/{document_id}.txt");

            Ok::<_, crate::services::transcriber::TranscriberError>((
                transcript_text,
                tpath_rel,
                dur,
            ))
        })
        .await
        .map_err(|e| AppError::IngestFailed {
            reason: format!("spawn_blocking join error: {e}"),
        })?
        .map_err(|e| match e {
            crate::services::transcriber::TranscriberError::AudioTooLong => AppError::AudioTooLong,
            crate::services::transcriber::TranscriberError::Other(inner) => {
                AppError::IngestFailed {
                    reason: inner.to_string(),
                }
            }
        })?;

        (result.0, Some(result.1), Some(result.2))
    } else {
        // Text document: parse
        doc_repo::update_status(pool, document_id, "parsing", None, None).await?;
        emit(
            ingest_tx,
            user_id,
            persona_id,
            document_id,
            "parsing",
            None,
            None,
        );

        let original_path = doc.original_path.clone();
        let mime_type = doc.mime_type.clone();
        let data_dir_text = config.data_dir.clone();

        let text = tokio::task::spawn_blocking(move || {
            let full_path = data_dir_text.join(&original_path);
            crate::services::parser::parse_to_text(&full_path, &mime_type)
        })
        .await
        .map_err(|e| AppError::IngestFailed {
            reason: format!("spawn_blocking join error: {e}"),
        })??;

        (text, None, None)
    };

    // Step 2: Chunk
    doc_repo::update_status(pool, document_id, "chunking", None, None).await?;
    emit(
        ingest_tx,
        user_id,
        persona_id,
        document_id,
        "chunking",
        None,
        None,
    );

    let model_dir = config.model_dir.clone();
    let text_clone = text.clone();
    let chunks = tokio::task::spawn_blocking(move || {
        let chunker = crate::services::chunker::Chunker::new(&model_dir)?;
        Ok::<_, AppError>(chunker.chunk_text(&text_clone))
    })
    .await
    .map_err(|e| AppError::IngestFailed {
        reason: format!("spawn_blocking join error: {e}"),
    })??;

    let word_count: i32 = text.split_whitespace().count() as i32;

    // Insert chunks in batches of 500 (embedding=NULL initially)
    let era_id = doc.era_id;
    for batch in chunks.chunks(500) {
        let mut tx = pool.begin().await.map_err(AppError::Database)?;
        for chunk in batch {
            sqlx::query(
                "INSERT INTO chunks (document_id, persona_id, era_id, user_id, chunk_index, text, token_count)
                 VALUES ($1, $2, $3, $4, $5, $6, $7)",
            )
            .bind(document_id)
            .bind(persona_id)
            .bind(era_id)
            .bind(user_id)
            .bind(chunk.chunk_index)
            .bind(&chunk.text)
            .bind(chunk.token_count)
            .execute(&mut *tx)
            .await
            .map_err(AppError::Database)?;
        }
        tx.commit().await.map_err(AppError::Database)?;
    }

    // Step 3: Embed
    doc_repo::update_status(pool, document_id, "embedding", None, None).await?;
    emit(
        ingest_tx,
        user_id,
        persona_id,
        document_id,
        "embedding",
        None,
        None,
    );

    let embed_model_dir = config.model_dir.clone();

    // Fetch un-embedded chunks for this document
    let chunk_rows: Vec<(Uuid, String)> = sqlx::query_as(
        "SELECT id, text FROM chunks WHERE document_id = $1 AND embedding IS NULL ORDER BY chunk_index",
    )
    .bind(document_id)
    .fetch_all(pool)
    .await
    .map_err(AppError::Database)?;

    // Create embedder once; keep in Arc so spawn_blocking batches can borrow it.
    let embedder = tokio::task::spawn_blocking(move || {
        crate::services::embedder::Embedder::new(&embed_model_dir)
    })
    .await
    .map_err(|e| AppError::IngestFailed {
        reason: format!("embed init spawn error: {e}"),
    })?
    .map_err(|e| AppError::IngestFailed {
        reason: format!("embed init error: {e}"),
    })?;
    let embedder = std::sync::Arc::new(embedder);

    // Embed in batches of 16 (idempotent: only fetches embedding IS NULL chunks)
    for batch in chunk_rows.chunks(16) {
        let texts: Vec<String> = batch.iter().map(|(_, t)| t.clone()).collect();
        let ids: Vec<Uuid> = batch.iter().map(|(id, _)| *id).collect();
        let embedder_clone = embedder.clone();

        let embeddings = tokio::task::spawn_blocking(move || {
            let text_refs: Vec<&str> = texts.iter().map(|s| s.as_str()).collect();
            embedder_clone.embed(&text_refs)
        })
        .await
        .map_err(|e| AppError::IngestFailed {
            reason: format!("embed spawn error: {e}"),
        })?
        .map_err(|e| AppError::IngestFailed {
            reason: format!("embed error: {e}"),
        })?;

        for (chunk_id, embedding) in ids.iter().zip(embeddings.iter()) {
            let vec = pgvector::Vector::from(embedding.clone());
            sqlx::query("UPDATE chunks SET embedding = $1 WHERE id = $2 AND embedding IS NULL")
                .bind(vec)
                .bind(chunk_id)
                .execute(pool)
                .await
                .map_err(AppError::Database)?;
        }
    }

    // Step 4: Enqueue recompute_profile job for the persona (coalesced via unique partial index)
    sqlx::query(
        "INSERT INTO jobs (kind, user_id, persona_id, payload)
         VALUES ('recompute_profile', $1, $2, $3)
         ON CONFLICT DO NOTHING",
    )
    .bind(user_id)
    .bind(persona_id)
    .bind(serde_json::json!({ "persona_id": persona_id.to_string(), "era_id": serde_json::Value::Null }))
    .execute(pool)
    .await
    .map_err(AppError::Database)?;

    // Step 5: Mark done
    doc_repo::update_ingested(
        pool,
        document_id,
        Some(word_count),
        transcript_path.as_deref(),
    )
    .await?;

    // Also update duration_sec if audio
    if let Some(dur) = duration_sec {
        sqlx::query("UPDATE documents SET duration_sec = $1 WHERE id = $2")
            .bind(dur)
            .bind(document_id)
            .execute(pool)
            .await
            .map_err(AppError::Database)?;
    }

    emit(
        ingest_tx,
        user_id,
        persona_id,
        document_id,
        "done",
        None,
        None,
    );

    Ok(())
}
