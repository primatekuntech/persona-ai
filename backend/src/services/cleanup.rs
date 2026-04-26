/// Daily cleanup task. Runs at 03:00 UTC, pruning old rows and orphan files.
use crate::state::AppState;
use std::time::Duration;

pub fn start_cleanup(state: AppState) {
    tokio::spawn(async move {
        loop {
            // Sleep until next 03:00 UTC
            let now = time::OffsetDateTime::now_utc();
            let target_hour = 3u8;
            let secs_until = {
                let mut t = now.replace_time(
                    time::Time::from_hms(target_hour, 0, 0).expect("valid time constant"),
                );
                if t <= now {
                    t += time::Duration::days(1);
                }
                (t - now).whole_seconds().max(0) as u64
            };
            tokio::time::sleep(Duration::from_secs(secs_until)).await;
            run_cleanup(&state).await;
        }
    });
}

async fn run_cleanup(state: &AppState) {
    tracing::info!("cleanup: starting daily pass");

    // 1. Prune errors older than 30 days
    match sqlx::query("DELETE FROM errors WHERE created_at < now() - interval '30 days'")
        .execute(&state.db)
        .await
    {
        Ok(r) => tracing::info!(pruned=%r.rows_affected(), "cleanup: pruned old errors"),
        Err(e) => tracing::error!(error=%e, "cleanup: prune errors failed"),
    }

    // 2. Prune audit_log older than 180 days
    match sqlx::query("DELETE FROM audit_log WHERE created_at < now() - interval '180 days'")
        .execute(&state.db)
        .await
    {
        Ok(r) => tracing::info!(pruned=%r.rows_affected(), "cleanup: pruned old audit_log"),
        Err(e) => tracing::error!(error=%e, "cleanup: prune audit_log failed"),
    }

    // 3. Prune done/failed jobs older than 30 days
    match sqlx::query(
        "DELETE FROM jobs WHERE status IN ('done','failed') \
         AND finished_at < now() - interval '30 days'",
    )
    .execute(&state.db)
    .await
    {
        Ok(r) => tracing::info!(pruned=%r.rows_affected(), "cleanup: pruned old jobs"),
        Err(e) => tracing::error!(error=%e, "cleanup: prune jobs failed"),
    }

    // 4. Prune idempotency_keys older than 24 h
    match sqlx::query("DELETE FROM idempotency_keys WHERE created_at < now() - interval '24 hours'")
        .execute(&state.db)
        .await
    {
        Ok(r) => tracing::info!(pruned=%r.rows_affected(), "cleanup: pruned idempotency_keys"),
        Err(e) => tracing::error!(error=%e, "cleanup: prune idempotency_keys failed"),
    }

    // 5. Prune login_attempts older than 24 h
    match sqlx::query("DELETE FROM login_attempts WHERE attempted_at < now() - interval '24 hours'")
        .execute(&state.db)
        .await
    {
        Ok(r) => tracing::info!(pruned=%r.rows_affected(), "cleanup: pruned login_attempts"),
        Err(e) => tracing::error!(error=%e, "cleanup: prune login_attempts failed"),
    }

    // 6. Prune password_resets older than 7 days
    match sqlx::query("DELETE FROM password_resets WHERE expires_at < now() - interval '7 days'")
        .execute(&state.db)
        .await
    {
        Ok(r) => tracing::info!(pruned=%r.rows_affected(), "cleanup: pruned password_resets"),
        Err(e) => tracing::error!(error=%e, "cleanup: prune password_resets failed"),
    }

    // 7. Prune invite_tokens past expires_at + 7 days
    match sqlx::query("DELETE FROM invite_tokens WHERE expires_at < now() - interval '7 days'")
        .execute(&state.db)
        .await
    {
        Ok(r) => tracing::info!(pruned=%r.rows_affected(), "cleanup: pruned old invite_tokens"),
        Err(e) => tracing::error!(error=%e, "cleanup: prune invite_tokens failed"),
    }

    // 8. Prune export files older than 7 days + mark those jobs expired in DB
    let exports_dir = state.config.data_dir.join("exports");
    if exports_dir.exists() {
        let cutoff = std::time::SystemTime::now() - std::time::Duration::from_secs(7 * 86400);
        if let Ok(rd) = std::fs::read_dir(&exports_dir) {
            for user_dir in rd.flatten() {
                if let Ok(files) = std::fs::read_dir(user_dir.path()) {
                    for f in files.flatten() {
                        if let Ok(meta) = f.metadata() {
                            if let Ok(modified) = meta.modified() {
                                if modified < cutoff {
                                    // Extract job_id from filename stem
                                    if let Some(stem) = f
                                        .path()
                                        .file_stem()
                                        .and_then(|s| s.to_str())
                                        .and_then(|s| uuid::Uuid::parse_str(s).ok())
                                    {
                                        let _ = sqlx::query(
                                            "UPDATE jobs SET payload = payload || '{\"expired\":true}'::jsonb \
                                             WHERE id=$1 AND kind='user_export' AND status='done'",
                                        )
                                        .bind(stem)
                                        .execute(&state.db)
                                        .await;
                                    }
                                    let _ = std::fs::remove_file(f.path());
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // 9. Prune .tmp upload files older than 1 h
    let tmp_dir = state.config.data_dir.join("uploads").join(".tmp");
    if tmp_dir.exists() {
        let tmp_cutoff = std::time::SystemTime::now() - std::time::Duration::from_secs(3600);
        if let Ok(rd) = std::fs::read_dir(&tmp_dir) {
            let mut pruned = 0u64;
            for f in rd.flatten() {
                if let Ok(meta) = f.metadata() {
                    if let Ok(modified) = meta.modified() {
                        if modified < tmp_cutoff && std::fs::remove_file(f.path()).is_ok() {
                            pruned += 1;
                        }
                    }
                }
            }
            if pruned > 0 {
                tracing::info!(pruned=%pruned, "cleanup: pruned stale .tmp uploads");
            }
        }
    }

    // 10. Orphan file check: uploads
    cleanup_orphan_uploads(state).await;

    tracing::info!("cleanup: daily pass complete");
}

async fn cleanup_orphan_uploads(state: &AppState) {
    let uploads_dir = state.config.data_dir.join("uploads");
    if !uploads_dir.exists() {
        return;
    }
    use walkdir::WalkDir;
    for entry in WalkDir::new(&uploads_dir).min_depth(2).max_depth(3) {
        let Ok(entry) = entry else {
            continue;
        };
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        // Extract doc_id from filename (UUID prefix before first '.')
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        let Ok(doc_id) = uuid::Uuid::parse_str(stem.split('.').next().unwrap_or("")) else {
            continue;
        };
        let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM documents WHERE id=$1)")
            .bind(doc_id)
            .fetch_one(&state.db)
            .await
            .unwrap_or(true); // default to true (safe: don't delete if unsure)
        if !exists {
            tracing::warn!(
                path=%path.display(),
                doc_id=%doc_id,
                "cleanup: orphan upload file deleted"
            );
            let _ = std::fs::remove_file(path);
        }
    }
}
