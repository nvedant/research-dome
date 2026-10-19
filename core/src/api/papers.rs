use axum::extract::{Multipart, Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;
use serde_json::Value;

use super::{file_response, no_content, AppState, ValidJson};
use crate::error::{AppError, AppResult};
use crate::models::PaperRecord;
use crate::pdf_export;
use crate::repo::{self, NewPaper};
use crate::util::{extension_lower, file_stem, head_chars, safe_filename};

const MAX_PDF_BYTES: usize = 100 * 1024 * 1024;

#[derive(Deserialize)]
pub struct ListPapersQuery {
    category_id: Option<i64>,
    q: Option<String>,
}

pub async fn list_papers(
    State(state): State<AppState>,
    Path(project_id): Path<i64>,
    Query(query): Query<ListPapersQuery>,
) -> AppResult<Response> {
    let papers = state
        .db
        .call(move |c| {
            repo::require_project(c, project_id)?;
            repo::list_papers(c, project_id, query.category_id, query.q.as_deref())
        })
        .await?;
    Ok(Json(papers).into_response())
}

fn parse_int(name: &str, value: &str) -> AppResult<i64> {
    value
        .trim()
        .parse::<i64>()
        .map_err(|_| AppError::unprocessable(format!("{name} must be a whole number")))
}

pub async fn upload_paper(State(state): State<AppState>, mut multipart: Multipart) -> AppResult<Response> {
    let mut project_id: Option<i64> = None;
    let mut category_id: Option<i64> = None;
    let mut title: Option<String> = None;
    let mut file: Option<(Option<String>, Option<String>, Vec<u8>)> = None;

    while let Some(field) = multipart.next_field().await? {
        let name = field.name().unwrap_or("").to_string();
        match name.as_str() {
            "project_id" => project_id = Some(parse_int("project_id", &field.text().await?)?),
            "category_id" => category_id = Some(parse_int("category_id", &field.text().await?)?),
            "title" => title = Some(field.text().await?),
            "file" => {
                let filename = field.file_name().map(str::to_owned);
                let content_type = field.content_type().map(str::to_owned);
                let data = field.bytes().await?;
                file = Some((filename, content_type, data.to_vec()));
            }
            _ => {}
        }
    }
    let project_id = project_id.ok_or_else(|| AppError::unprocessable("Field required: project_id"))?;
    let category_id = category_id.ok_or_else(|| AppError::unprocessable("Field required: category_id"))?;
    let (filename, content_type, data) = file.ok_or_else(|| AppError::unprocessable("Field required: file"))?;

    // Project / category checks come first, like the original service.
    state
        .db
        .call(move |c| {
            repo::require_project(c, project_id)?;
            match repo::get_category(c, category_id)? {
                Some(category) if category.project_id == project_id => Ok(()),
                _ => Err(AppError::bad_request("Choose a category that belongs to this project")),
            }
        })
        .await?;

    let original_name = safe_filename(filename.as_deref().filter(|n| !n.is_empty()).unwrap_or("research-paper.pdf"));
    let content_type_ok = match content_type.as_deref() {
        None => true,
        Some(ct) => {
            let ct = ct.split(';').next().unwrap_or("").trim().to_lowercase();
            ct == "application/pdf" || ct == "application/octet-stream"
        }
    };
    if extension_lower(&original_name) != ".pdf" || !content_type_ok {
        return Err(AppError::UnsupportedMedia("Only PDF research papers are supported".into()));
    }
    if data.len() > MAX_PDF_BYTES {
        return Err(AppError::TooLarge("PDF exceeds the 100 MB upload limit".into()));
    }
    if !data.starts_with(b"%PDF-") {
        return Err(AppError::bad_request("The uploaded file does not appear to be a valid PDF"));
    }
    if let Err(reason) = pdf_export::probe(&data) {
        // Some real-world PDFs have damaged cross-reference tables that browsers'
        // PDF.js still opens. Accept those when they clearly contain PDF objects.
        let looks_like_pdf = data.windows(6).any(|w| w == b"endobj");
        if !looks_like_pdf {
            return Err(AppError::bad_request(format!("Unable to read this PDF: {reason}")));
        }
        tracing::warn!(%reason, "accepting PDF with a non-standard structure");
    }

    let stored_name = format!("{}_{}", uuid::Uuid::new_v4().simple(), original_name);
    let target_dir = state.storage.papers_dir(project_id);
    tokio::fs::create_dir_all(&target_dir).await?;
    let target = target_dir.join(&stored_name);
    tokio::fs::write(&target, &data).await?;
    let relative = state.storage.relative(&target)?;

    let paper_title = {
        let raw = title.unwrap_or_default();
        let raw = raw.trim();
        let chosen = if raw.is_empty() { file_stem(&original_name) } else { raw.to_string() };
        head_chars(chosen.trim(), 300)
    };
    let file_size = data.len() as i64;
    let inserted = {
        let relative = relative.clone();
        let original_name = original_name.clone();
        state
            .db
            .call(move |c| {
                let id = repo::insert_paper(
                    c,
                    &NewPaper {
                        project_id,
                        category_id,
                        title: &paper_title,
                        original_filename: &original_name,
                        storage_path: &relative,
                        file_size,
                    },
                )?;
                Ok(repo::get_paper(c, id)?.out)
            })
            .await
    };
    match inserted {
        Ok(paper) => Ok((StatusCode::CREATED, Json(paper)).into_response()),
        Err(err) => {
            state.storage.remove_file(&relative);
            Err(err)
        }
    }
}

pub async fn get_paper(State(state): State<AppState>, Path(paper_id): Path<i64>) -> AppResult<Response> {
    let record = state.db.call(move |c| repo::get_paper(c, paper_id)).await?;
    Ok(Json(record.out).into_response())
}

pub async fn update_paper(
    State(state): State<AppState>,
    Path(paper_id): Path<i64>,
    ValidJson(payload): ValidJson<Value>,
) -> AppResult<Response> {
    let record = state
        .db
        .call(move |c| {
            let current = repo::get_paper(c, paper_id)?;
            if let Some(title) = payload.get("title") {
                let text = match title {
                    Value::String(s) => s.clone(),
                    other => other.to_string(),
                };
                let text = text.trim();
                if text.is_empty() {
                    return Err(AppError::unprocessable("Paper title cannot be empty"));
                }
                repo::update_paper_title(c, paper_id, &head_chars(text, 300))?;
            }
            if let Some(requested) = payload.get("category_id") {
                let invalid = || AppError::bad_request("Choose a valid category in the same project or select Uncategorized");
                let target: Option<i64> = match requested {
                    Value::Null => None,
                    Value::String(s) if matches!(s.trim().to_lowercase().as_str(), "" | "none" | "null" | "uncategorized") => None,
                    Value::String(s) => Some(s.trim().parse::<i64>().map_err(|_| invalid())?),
                    Value::Number(n) => Some(n.as_i64().ok_or_else(invalid)?),
                    _ => return Err(invalid()),
                };
                if let Some(category_id) = target {
                    match repo::get_category(c, category_id)? {
                        Some(category) if category.project_id == current.out.project_id => {}
                        _ => return Err(AppError::bad_request("Choose a category in the same project")),
                    }
                }
                repo::update_paper_category(c, paper_id, target)?;
            }
            repo::get_paper(c, paper_id)
        })
        .await?;
    Ok(Json(record.out).into_response())
}

pub async fn delete_paper(State(state): State<AppState>, Path(paper_id): Path<i64>) -> AppResult<Response> {
    let record = state
        .db
        .call(move |c| {
            let record = repo::get_paper(c, paper_id)?;
            repo::delete_paper(c, paper_id)?;
            Ok(record)
        })
        .await?;
    state.storage.remove_file(&record.storage_path);
    Ok(no_content())
}

async fn read_paper_bytes(state: &AppState, record: &PaperRecord) -> AppResult<Vec<u8>> {
    let path = state.storage.resolve(&record.storage_path)?;
    tokio::fs::read(&path)
        .await
        .map_err(|_| AppError::not_found("Stored PDF file is missing"))
}

pub async fn paper_file(State(state): State<AppState>, Path(paper_id): Path<i64>) -> AppResult<Response> {
    let record = state.db.call(move |c| repo::get_paper(c, paper_id)).await?;
    let bytes = read_paper_bytes(&state, &record).await?;
    Ok(file_response(bytes, "application/pdf", &record.out.original_filename, "inline"))
}

pub async fn download_original(State(state): State<AppState>, Path(paper_id): Path<i64>) -> AppResult<Response> {
    let record = state.db.call(move |c| repo::get_paper(c, paper_id)).await?;
    let bytes = read_paper_bytes(&state, &record).await?;
    Ok(file_response(bytes, "application/pdf", &record.out.original_filename, "attachment"))
}

pub async fn download_annotated(State(state): State<AppState>, Path(paper_id): Path<i64>) -> AppResult<Response> {
    let (record, annotations) = state
        .db
        .call(move |c| Ok((repo::get_paper(c, paper_id)?, repo::list_annotations(c, paper_id)?)))
        .await?;
    let source = read_paper_bytes(&state, &record).await?;
    let filename = format!("{}_annotated.pdf", file_stem(&record.out.original_filename));
    if annotations.is_empty() {
        return Ok(file_response(source, "application/pdf", &filename, "attachment"));
    }
    let output = tokio::task::spawn_blocking(move || pdf_export::build_annotated_pdf(&source, &annotations))
        .await?
        .map_err(|e| AppError::internal(format!("Could not create annotated PDF: {e}")))?;
    Ok(file_response(output, "application/pdf", &filename, "attachment"))
}
