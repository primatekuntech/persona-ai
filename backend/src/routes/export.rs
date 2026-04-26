use crate::{
    auth::middleware::UserCtx,
    error::AppError,
    repositories::{chats as chat_repo, eras, personas},
    state::AppState,
};
use axum::{
    extract::{Path, Query, State},
    http::{header, HeaderValue},
    response::Response,
};
use docx_rs::{
    AlignmentType, Docx, LineSpacing, LineSpacingType, PageMargin, Paragraph, Run, RunFonts,
};
use serde::Deserialize;
use std::io::{Read, Seek, SeekFrom};
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Deserialize)]
pub struct ExportQuery {
    pub format: Option<String>,
    pub message_ids: Option<String>,
    pub title: Option<String>,
}

fn slugify(s: &str) -> String {
    let slug: String = s
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    slug.split('-')
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

pub async fn export_session(
    ctx: UserCtx,
    State(state): State<AppState>,
    Path(session_id): Path<Uuid>,
    Query(q): Query<ExportQuery>,
) -> Result<Response, AppError> {
    let user_id = ctx.user_id;
    let session = chat_repo::get_session(&state.db, user_id, session_id)
        .await?
        .ok_or(AppError::NotFound)?;

    // Collect all messages (paginate through all)
    let mut all_messages = Vec::new();
    let mut cursor: Option<String> = None;
    loop {
        let (batch, next) =
            chat_repo::list_messages(&state.db, user_id, session_id, cursor.as_deref(), 200)
                .await?;
        all_messages.extend(batch);
        cursor = next;
        if cursor.is_none() {
            break;
        }
    }
    // list_messages is newest-first; reverse to chronological
    all_messages.reverse();

    // Only assistant messages with non-empty content
    let mut asst_messages: Vec<_> = all_messages
        .into_iter()
        .filter(|m| m.role == "assistant" && !m.content.is_empty())
        .collect();

    // Filter to requested IDs if provided
    if let Some(ref ids_str) = q.message_ids {
        let requested: Vec<Uuid> = ids_str
            .split(',')
            .filter_map(|s| Uuid::parse_str(s.trim()).ok())
            .collect();
        if !requested.is_empty() {
            asst_messages.retain(|m| requested.contains(&m.id));
        }
    }

    if asst_messages.is_empty() {
        return Err(AppError::Validation(
            "No messages to export. Select at least one assistant message.".into(),
        ));
    }

    let persona = sqlx::query_as::<_, personas::Persona>(
        "SELECT id, user_id, name, relation, description, avatar_path, birth_year,
                created_at, updated_at
         FROM personas WHERE id = $1 AND user_id = $2",
    )
    .bind(session.persona_id)
    .bind(user_id)
    .fetch_optional(&state.db)
    .await
    .map_err(AppError::Database)?
    .ok_or(AppError::NotFound)?;

    let era = if let Some(era_id) = session.era_id {
        eras::find_by_id(&state.db, era_id, session.persona_id, user_id).await?
    } else {
        None
    };

    let title = q.title.clone().unwrap_or_else(|| {
        session
            .title
            .as_deref()
            .map(|t| t.chars().take(60).collect())
            .unwrap_or_else(|| format!("{} — Chat", persona.name))
    });

    let now = OffsetDateTime::now_utc();
    let ts = format!(
        "{}{:02}{:02}-{:02}{:02}",
        now.year(),
        now.month() as u8,
        now.day(),
        now.hour(),
        now.minute()
    );
    let persona_slug = slugify(&persona.name);
    let era_slug = era
        .as_ref()
        .map(|e| slugify(&e.label))
        .unwrap_or_else(|| "all".to_string());
    let format = q.format.as_deref().unwrap_or("md").to_lowercase();
    let filename = format!("{persona_slug}_{era_slug}_{ts}.{format}");

    match format.as_str() {
        "docx" => {
            let bytes = build_docx(&title, &persona.name, era.as_ref(), &asst_messages, &now)?;

            save_export(
                &state.config.data_dir,
                user_id,
                session_id,
                &ts,
                "docx",
                &bytes,
            );

            let mut resp = Response::new(axum::body::Body::from(bytes));
            resp.headers_mut().insert(
                header::CONTENT_TYPE,
                HeaderValue::from_static(
                    "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
                ),
            );
            resp.headers_mut().insert(
                header::CONTENT_DISPOSITION,
                HeaderValue::from_str(&format!("attachment; filename=\"{filename}\""))
                    .unwrap_or(HeaderValue::from_static("attachment")),
            );
            Ok(resp)
        }
        _ => {
            let md = build_markdown(&title, &persona.name, era.as_ref(), &asst_messages);
            let bytes = md.into_bytes();

            save_export(
                &state.config.data_dir,
                user_id,
                session_id,
                &ts,
                "md",
                &bytes,
            );

            let mut resp = Response::new(axum::body::Body::from(bytes));
            resp.headers_mut().insert(
                header::CONTENT_TYPE,
                HeaderValue::from_static("text/markdown; charset=utf-8"),
            );
            resp.headers_mut().insert(
                header::CONTENT_DISPOSITION,
                HeaderValue::from_str(&format!("attachment; filename=\"{filename}\""))
                    .unwrap_or(HeaderValue::from_static("attachment")),
            );
            Ok(resp)
        }
    }
}

fn build_markdown(
    title: &str,
    persona_name: &str,
    era: Option<&eras::Era>,
    messages: &[chat_repo::Message],
) -> String {
    let mut md = String::new();

    md.push_str(&format!("# {title}\n\n"));

    if let Some(e) = era {
        md.push_str(&format!("_{persona_name}, {}_\n\n", e.label));
    } else {
        md.push_str(&format!("_{persona_name}_\n\n"));
    }
    md.push_str("---\n\n");

    for msg in messages {
        md.push_str(&msg.content);
        md.push_str("\n\n---\n\n");
    }

    md.push_str(&format!(
        "_Generated from {} source(s) in your own corpus._\n",
        messages.len()
    ));

    md
}

fn build_docx(
    title: &str,
    persona_name: &str,
    era: Option<&eras::Era>,
    messages: &[chat_repo::Message],
    now: &OffsetDateTime,
) -> Result<Vec<u8>, AppError> {
    let calibri = RunFonts::new().ascii("Calibri").hi_ansi("Calibri");

    let title_para = Paragraph::new()
        .add_run(
            Run::new()
                .add_text(title)
                .bold()
                .size(36)
                .fonts(calibri.clone()),
        )
        .align(AlignmentType::Center);

    let subtitle_text = era
        .map(|e| format!("{persona_name}, {}", e.label))
        .unwrap_or_else(|| persona_name.to_string());
    let subtitle_para = Paragraph::new()
        .add_run(
            Run::new()
                .add_text(subtitle_text)
                .italic()
                .size(22)
                .color("808080")
                .fonts(calibri.clone()),
        )
        .align(AlignmentType::Center);

    let hr_para = Paragraph::new()
        .add_run(
            Run::new()
                .add_text("─────────────────────────────────────")
                .size(22)
                .color("cccccc")
                .fonts(calibri.clone()),
        )
        .align(AlignmentType::Center);

    let body_spacing = LineSpacing::new()
        .line(288)
        .line_rule(LineSpacingType::Auto);

    let mut docx = Docx::new()
        .page_size(11906, 16838)
        .page_margin(PageMargin {
            top: 1440,
            left: 1440,
            bottom: 1440,
            right: 1440,
            header: 720,
            footer: 720,
            gutter: 0,
        })
        .add_paragraph(title_para)
        .add_paragraph(subtitle_para)
        .add_paragraph(hr_para);

    for msg in messages {
        for para_text in msg.content.split("\n\n") {
            let para_text = para_text.trim();
            if para_text.is_empty() {
                continue;
            }
            // Strip leading markdown heading markers
            let para_text = para_text.trim_start_matches('#').trim();
            docx = docx.add_paragraph(
                Paragraph::new()
                    .add_run(
                        Run::new()
                            .add_text(para_text)
                            .size(22)
                            .fonts(calibri.clone()),
                    )
                    .line_spacing(body_spacing.clone()),
            );
        }
        // Empty paragraph between messages
        docx = docx.add_paragraph(Paragraph::new());
    }

    let footer_text = format!(
        "Generated {}-{:02}-{:02} · {persona_name} · {}",
        now.year(),
        now.month() as u8,
        now.day(),
        era.map(|e| e.label.as_str()).unwrap_or("all eras"),
    );
    docx = docx.add_paragraph(
        Paragraph::new()
            .add_run(
                Run::new()
                    .add_text(footer_text)
                    .size(18)
                    .color("999999")
                    .fonts(calibri),
            )
            .align(AlignmentType::Center),
    );

    // Write to a temp file, then read back (docx-rs pack() consumes the writer)
    let mut tmpfile =
        tempfile::tempfile().map_err(|e| AppError::Internal(anyhow::anyhow!("tmpfile: {e}")))?;
    docx.build()
        .pack(&mut tmpfile)
        .map_err(|e| AppError::Internal(anyhow::anyhow!("docx pack: {e}")))?;
    tmpfile
        .seek(SeekFrom::Start(0))
        .map_err(|e| AppError::Internal(anyhow::anyhow!("seek: {e}")))?;
    let mut bytes = Vec::new();
    tmpfile
        .read_to_end(&mut bytes)
        .map_err(|e| AppError::Internal(anyhow::anyhow!("read: {e}")))?;
    Ok(bytes)
}

fn save_export(
    data_dir: &std::path::Path,
    user_id: Uuid,
    session_id: Uuid,
    ts: &str,
    ext: &str,
    bytes: &[u8],
) {
    let dir = data_dir.join("exports").join(user_id.to_string());
    if std::fs::create_dir_all(&dir).is_ok() {
        let path = dir.join(format!("{session_id}-{ts}.{ext}"));
        if let Err(e) = std::fs::write(&path, bytes) {
            tracing::warn!(path = %path.display(), error = %e, "export save failed");
        }
    }
}
