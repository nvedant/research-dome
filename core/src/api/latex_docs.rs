use axum::extract::{Multipart, Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;
use serde_json::json;

use super::{file_response, no_content, AppState, ValidJson};
use crate::error::{AppError, AppResult};
use crate::latex::{self, content_type_for, is_text_ext, STARTER_SOURCE};
use crate::models::{LatexCreate, LatexFileSave, LatexRecord, LatexSave};
use crate::repo;
use crate::util::{extension_lower, safe_filename};

async fn load(state: &AppState, document_id: i64) -> AppResult<LatexRecord> {
    state.db.call(move |c| repo::get_latex(c, document_id)).await
}

fn workspace_root(state: &AppState, record: &LatexRecord) -> AppResult<std::path::PathBuf> {
    state.storage.resolve(&record.storage_dir)
}

pub async fn list_documents(State(state): State<AppState>, Path(project_id): Path<i64>) -> AppResult<Response> {
    let docs = state
        .db
        .call(move |c| {
            repo::require_project(c, project_id)?;
            repo::list_latex(c, project_id)
        })
        .await?;
    Ok(Json(docs).into_response())
}

pub async fn create_document(
    State(state): State<AppState>,
    Path(project_id): Path<i64>,
    ValidJson(body): ValidJson<LatexCreate>,
) -> AppResult<Response> {
    let title = body.title.trim().to_string();
    if title.is_empty() {
        return Err(AppError::unprocessable("Workspace name is required"));
    }
    if title.chars().count() > 250 {
        return Err(AppError::unprocessable("Workspace name must be 250 characters or fewer"));
    }
    state.db.call(move |c| repo::require_project(c, project_id).map(|_| ())).await?;

    let work_dir = state.storage.latex_dir(project_id).join(uuid::Uuid::new_v4().simple().to_string());
    tokio::fs::create_dir_all(&work_dir).await?;
    let starter = if body.source.is_empty() { STARTER_SOURCE.to_string() } else { body.source };
    tokio::fs::write(work_dir.join("main.tex"), &starter).await?;
    let relative = state.storage.relative(&work_dir)?;

    let inserted = {
        let relative = relative.clone();
        state.db.call(move |c| repo::insert_latex(c, project_id, &title, &relative)).await
    };
    match inserted {
        Ok(record) => Ok((
            StatusCode::CREATED,
            Json(json!({
                "id": record.out.id,
                "project_id": record.out.project_id,
                "title": record.out.title,
                "source": starter,
                "created_at": record.out.created_at,
                "updated_at": record.out.updated_at,
                "compiled": false,
                "last_compile_log": "",
            })),
        )
            .into_response()),
        Err(err) => {
            state.storage.remove_abs_dir(&work_dir);
            Err(err)
        }
    }
}

pub async fn get_document(State(state): State<AppState>, Path(document_id): Path<i64>) -> AppResult<Response> {
    let record = load(&state, document_id).await?;
    let root = workspace_root(&state, &record)?;
    let source = tokio::fs::read(root.join("main.tex"))
        .await
        .map(|b| String::from_utf8_lossy(&b).into_owned())
        .unwrap_or_default();
    Ok(Json(json!({
        "id": record.out.id,
        "project_id": record.out.project_id,
        "title": record.out.title,
        "source": source,
        "created_at": record.out.created_at,
        "updated_at": record.out.updated_at,
        "compiled": record.out.compiled,
        "last_compile_log": record.out.last_compile_log,
    }))
    .into_response())
}

pub async fn delete_document(State(state): State<AppState>, Path(document_id): Path<i64>) -> AppResult<Response> {
    let record = load(&state, document_id).await?;
    state
        .db
        .call(move |c| repo::delete_latex(c, document_id))
        .await?;
    state.storage.remove_dir(&record.storage_dir);
    Ok(no_content())
}

pub async fn save_document(
    State(state): State<AppState>,
    Path(document_id): Path<i64>,
    ValidJson(body): ValidJson<LatexSave>,
) -> AppResult<Response> {
    let record = load(&state, document_id).await?;
    let root = workspace_root(&state, &record)?;
    tokio::fs::create_dir_all(&root).await?;
    tokio::fs::write(root.join("main.tex"), body.source).await?;
    let updated_at = state.db.call(move |c| repo::touch_latex(c, document_id)).await?;
    Ok(Json(json!({
        "id": record.out.id,
        "project_id": record.out.project_id,
        "title": record.out.title,
        "updated_at": updated_at,
        "saved": true,
    }))
    .into_response())
}

pub async fn list_files(State(state): State<AppState>, Path(document_id): Path<i64>) -> AppResult<Response> {
    let record = load(&state, document_id).await?;
    let root = workspace_root(&state, &record)?;
    tokio::fs::create_dir_all(&root).await?;
    let files = tokio::task::spawn_blocking(move || latex::list_files(&root)).await?;
    Ok(Json(json!({ "files": files })).into_response())
}

#[derive(Deserialize)]
pub struct PathQuery {
    path: String,
}

fn relative_of(root: &std::path::Path, target: &std::path::Path) -> String {
    target
        .strip_prefix(root)
        .map(|p| p.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect::<Vec<_>>().join("/"))
        .unwrap_or_default()
}

pub async fn get_file(
    State(state): State<AppState>,
    Path(document_id): Path<i64>,
    Query(query): Query<PathQuery>,
) -> AppResult<Response> {
    let record = load(&state, document_id).await?;
    let root = workspace_root(&state, &record)?;
    let target = latex::workspace_file(&root, &query.path, false)?;
    if !target.is_file() {
        return Err(AppError::not_found("LaTeX project file not found"));
    }
    if !is_text_ext(&extension_lower(&query.path)) {
        return Err(AppError::UnsupportedMedia("This file is an asset, not an editable text file".into()));
    }
    let bytes = tokio::fs::read(&target).await?;
    Ok(Json(json!({
        "path": relative_of(&root, &target),
        "content": String::from_utf8_lossy(&bytes),
        "kind": "text",
    }))
    .into_response())
}

pub async fn save_file(
    State(state): State<AppState>,
    Path(document_id): Path<i64>,
    Query(query): Query<PathQuery>,
    ValidJson(body): ValidJson<LatexFileSave>,
) -> AppResult<Response> {
    let record = load(&state, document_id).await?;
    let root = workspace_root(&state, &record)?;
    let target = latex::workspace_file(&root, &query.path, true)?;
    if !is_text_ext(&extension_lower(&query.path)) {
        return Err(AppError::UnsupportedMedia("Use the asset upload tool for images and other binary files".into()));
    }
    if let Some(parent) = target.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    tokio::fs::write(&target, body.content).await?;
    let updated_at = state.db.call(move |c| repo::touch_latex(c, document_id)).await?;
    Ok(Json(json!({ "saved": true, "path": relative_of(&root, &target), "updated_at": updated_at })).into_response())
}

pub async fn delete_file(
    State(state): State<AppState>,
    Path(document_id): Path<i64>,
    Query(query): Query<PathQuery>,
) -> AppResult<Response> {
    let record = load(&state, document_id).await?;
    let relative = query.path.replace('\\', "/");
    if relative.trim().eq_ignore_ascii_case("main.tex") {
        return Err(AppError::bad_request("main.tex is the required root document and cannot be deleted"));
    }
    let root = workspace_root(&state, &record)?;
    let target = latex::workspace_file(&root, &relative, true)?;
    if !target.is_file() {
        return Err(AppError::not_found("LaTeX project file not found"));
    }
    tokio::fs::remove_file(&target).await?;
    state.db.call(move |c| repo::touch_latex(c, document_id)).await?;
    Ok(no_content())
}

pub async fn upload_file(
    State(state): State<AppState>,
    Path(document_id): Path<i64>,
    mut multipart: Multipart,
) -> AppResult<Response> {
    let mut path_field = String::new();
    let mut file: Option<(Option<String>, Vec<u8>)> = None;
    while let Some(field) = multipart.next_field().await? {
        match field.name().unwrap_or("") {
            "path" => path_field = field.text().await?,
            "file" => {
                let name = field.file_name().map(str::to_owned);
                file = Some((name, field.bytes().await?.to_vec()));
            }
            _ => {}
        }
    }
    let (upload_name, content) = file.ok_or_else(|| AppError::unprocessable("Field required: file"))?;

    let record = load(&state, document_id).await?;
    let root = workspace_root(&state, &record)?;
    let wanted = {
        let p = path_field.trim();
        if !p.is_empty() {
            p.to_string()
        } else {
            upload_name.filter(|n| !n.is_empty()).unwrap_or_else(|| "asset".to_string())
        }
    };
    let target = latex::workspace_file(&root, &wanted.replace('\\', "/"), true)?;
    if content.len() > latex::MAX_ASSET_BYTES {
        return Err(AppError::TooLarge("LaTeX project assets are limited to 30 MB per file".into()));
    }
    if let Some(parent) = target.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    tokio::fs::write(&target, &content).await?;
    state.db.call(move |c| repo::touch_latex(c, document_id)).await?;
    Ok((
        StatusCode::CREATED,
        Json(json!({ "uploaded": true, "path": relative_of(&root, &target), "size": content.len() })),
    )
        .into_response())
}

pub async fn get_asset(
    State(state): State<AppState>,
    Path(document_id): Path<i64>,
    Query(query): Query<PathQuery>,
) -> AppResult<Response> {
    let record = load(&state, document_id).await?;
    let root = workspace_root(&state, &record)?;
    let target = latex::workspace_file(&root, &query.path, false)?;
    if !target.is_file() {
        return Err(AppError::not_found("LaTeX asset not found"));
    }
    let ext = extension_lower(&query.path);
    if is_text_ext(&ext) {
        return Err(AppError::UnsupportedMedia("Text source files should be opened in the editor".into()));
    }
    let bytes = tokio::fs::read(&target).await?;
    let name = target.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    Ok(file_response(bytes, content_type_for(&ext), &name, "inline"))
}

pub async fn compile_document(State(state): State<AppState>, Path(document_id): Path<i64>) -> AppResult<Response> {
    let record = load(&state, document_id).await?;
    let root = workspace_root(&state, &record)?;
    let outcome = {
        let _guard = state.build_lock.lock().await;
        latex::compile(&root).await
    };
    let pdf_exists = root.join("main.pdf").exists();
    {
        let log = outcome.log.clone();
        let ok = outcome.ok;
        state.db.call(move |c| repo::set_latex_build(c, document_id, ok, &log)).await?;
    }
    let compiled = outcome.ok && pdf_exists;
    Ok(Json(json!({
        "success": outcome.ok,
        "compiled": compiled,
        "log": outcome.log,
        "pdf_url": if compiled { Some(format!("/api/latex-documents/{document_id}/pdf")) } else { None },
    }))
    .into_response())
}

async fn pdf_response(
    state: &AppState,
    document_id: i64,
    disposition: &str,
    missing: &str,
) -> AppResult<Response> {
    let record = load(state, document_id).await?;
    let root = workspace_root(state, &record)?;
    let bytes = tokio::fs::read(root.join("main.pdf"))
        .await
        .map_err(|_| AppError::not_found(missing))?;
    Ok(file_response(bytes, "application/pdf", &format!("{}.pdf", safe_filename(&record.out.title)), disposition))
}

pub async fn preview_pdf(State(state): State<AppState>, Path(document_id): Path<i64>) -> AppResult<Response> {
    pdf_response(&state, document_id, "inline", "Compile the LaTeX document to generate a preview PDF").await
}

pub async fn download_pdf(State(state): State<AppState>, Path(document_id): Path<i64>) -> AppResult<Response> {
    pdf_response(&state, document_id, "attachment", "Compile the LaTeX document before downloading its PDF").await
}

pub async fn download_source(State(state): State<AppState>, Path(document_id): Path<i64>) -> AppResult<Response> {
    let record = load(&state, document_id).await?;
    let root = workspace_root(&state, &record)?;
    let bytes = tokio::fs::read(root.join("main.tex"))
        .await
        .map_err(|_| AppError::not_found("LaTeX source is missing"))?;
    Ok(file_response(bytes, "application/x-tex", &format!("{}.tex", safe_filename(&record.out.title)), "attachment"))
}
