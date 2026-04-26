/// Text extraction from various document types.
/// All heavy parsing runs inside `spawn_blocking` in the caller.
use crate::error::AppError;
use std::path::Path;

const MAX_EXTRACTED_BYTES: usize = 10 * 1024 * 1024; // 10 MB extracted text limit
const MAX_PDF_PAGES: u32 = 500;
const MAX_DOCX_PARAGRAPHS: usize = 5000;
const ZIP_BOMB_RATIO: usize = 100;
const ZIP_BOMB_SIZE_THRESHOLD: usize = 50 * 1024 * 1024; // 50 MB uncompressed

/// Extract plain text from a file given its MIME type.
/// Returns `AppError::IngestFailed` on parse failures.
pub fn parse_to_text(file_path: &Path, mime_type: &str) -> Result<String, AppError> {
    match mime_type {
        "text/plain" | "text/markdown" => parse_plain_text(file_path),
        "application/pdf" => parse_pdf(file_path),
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document" => {
            parse_docx(file_path)
        }
        other => Err(AppError::IngestFailed {
            reason: format!("Unsupported MIME type for parsing: {other}"),
        }),
    }
}

fn parse_plain_text(path: &Path) -> Result<String, AppError> {
    let bytes = std::fs::read(path).map_err(|e| AppError::IngestFailed {
        reason: format!("Failed to read file: {e}"),
    })?;

    // Count non-printable bytes (binary detection)
    let non_printable = bytes
        .iter()
        .filter(|&&b| b < 0x20 && b != b'\n' && b != b'\r' && b != b'\t')
        .count();
    if !bytes.is_empty() && non_printable * 100 / bytes.len() > 40 {
        return Err(AppError::IngestFailed {
            reason: "File appears to be binary (> 40% non-printable bytes).".into(),
        });
    }

    // Detect BOM-based encoding first, fall back to UTF-8
    let encoding = encoding_rs::Encoding::for_bom(&bytes)
        .map(|(enc, _bom_len)| enc)
        .unwrap_or(encoding_rs::UTF_8);

    let (text, _, had_errors) = encoding.decode(&bytes);
    if had_errors {
        // Fall back: interpret as UTF-8 with replacement
        let text = String::from_utf8_lossy(&bytes).into_owned();
        return Ok(text);
    }

    Ok(text.into_owned())
}

fn parse_pdf(path: &Path) -> Result<String, AppError> {
    let doc = lopdf::Document::load(path).map_err(|e| AppError::IngestFailed {
        reason: format!("PDF parse error: {e}"),
    })?;

    // Reject encrypted PDFs
    if doc.is_encrypted() {
        return Err(AppError::IngestFailed {
            reason: "Encrypted PDFs are not supported.".into(),
        });
    }

    // Page count check
    let page_count = doc.get_pages().len() as u32;
    if page_count > MAX_PDF_PAGES {
        return Err(AppError::IngestFailed {
            reason: format!("PDF has {page_count} pages; limit is {MAX_PDF_PAGES}."),
        });
    }

    let mut text = String::new();
    let pages: Vec<u32> = doc.get_pages().keys().copied().collect();

    for page_num in pages {
        let page_text = doc.extract_text(&[page_num]).unwrap_or_default();
        text.push_str(&page_text);
        text.push('\n');

        if text.len() > MAX_EXTRACTED_BYTES {
            return Err(AppError::IngestFailed {
                reason: format!(
                    "Extracted PDF text exceeds {:.0} MB limit.",
                    MAX_EXTRACTED_BYTES as f64 / 1_048_576.0
                ),
            });
        }
    }

    Ok(text)
}

fn parse_docx(path: &Path) -> Result<String, AppError> {
    // Zip-bomb detection: check compressed vs uncompressed sizes
    let file = std::fs::File::open(path).map_err(|e| AppError::IngestFailed {
        reason: format!("Failed to open docx: {e}"),
    })?;

    {
        let mut zip = zip::ZipArchive::new(&file).map_err(|e| AppError::IngestFailed {
            reason: format!("Failed to open docx as zip: {e}"),
        })?;

        let mut total_uncompressed: usize = 0;
        let mut total_compressed: usize = 0;

        for i in 0..zip.len() {
            let entry = zip.by_index(i).map_err(|e| AppError::IngestFailed {
                reason: format!("Zip entry error: {e}"),
            })?;
            total_uncompressed += entry.size() as usize;
            total_compressed += entry.compressed_size() as usize;
        }

        if total_uncompressed > ZIP_BOMB_SIZE_THRESHOLD
            && total_compressed > 0
            && total_uncompressed / total_compressed > ZIP_BOMB_RATIO
        {
            return Err(AppError::IngestFailed {
                reason: "Zip bomb detected: uncompressed/compressed ratio exceeds 100:1.".into(),
            });
        }
    }

    // Parse docx using docx-rs
    let bytes = std::fs::read(path).map_err(|e| AppError::IngestFailed {
        reason: format!("Failed to read docx: {e}"),
    })?;

    let docx = docx_rs::read_docx(&bytes).map_err(|e| AppError::IngestFailed {
        reason: format!("Failed to parse docx: {e}"),
    })?;

    let mut text = String::new();
    let mut para_count = 0usize;

    for child in &docx.document.children {
        if let docx_rs::DocumentChild::Paragraph(para) = child {
            para_count += 1;
            if para_count > MAX_DOCX_PARAGRAPHS {
                break;
            }

            for run in &para.children {
                if let docx_rs::ParagraphChild::Run(r) = run {
                    for rc in &r.children {
                        if let docx_rs::RunChild::Text(t) = rc {
                            text.push_str(&t.text);
                        }
                    }
                }
            }
            text.push('\n');

            if text.len() > MAX_EXTRACTED_BYTES {
                return Err(AppError::IngestFailed {
                    reason: format!(
                        "Extracted docx text exceeds {:.0} MB limit.",
                        MAX_EXTRACTED_BYTES as f64 / 1_048_576.0
                    ),
                });
            }
        }
    }

    Ok(text)
}
