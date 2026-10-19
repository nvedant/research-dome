use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;
use serde_json::json;

use super::{no_content, AppState, ValidJson};
use crate::error::{AppError, AppResult};
use crate::models::{CategoryIn, ProjectIn};
use crate::repo;

pub async fn health(State(state): State<AppState>) -> Json<serde_json::Value> {
    Json(json!({ "status": "ok", "storage": state.storage.root().display().to_string() }))
}

pub async fn list_projects(State(state): State<AppState>) -> AppResult<Response> {
    let projects = state.db.call(|c| repo::list_projects(c)).await?;
    Ok(Json(projects).into_response())
}

pub async fn create_project(State(state): State<AppState>, ValidJson(body): ValidJson<ProjectIn>) -> AppResult<Response> {
    let name = body.name.trim().to_string();
    if name.is_empty() {
        return Err(AppError::unprocessable("Project name is required"));
    }
    if name.chars().count() > 200 {
        return Err(AppError::unprocessable("Project name must be 200 characters or fewer"));
    }
    let description = body.description.trim().to_string();
    let project = state.db.call(move |c| repo::create_project(c, &name, &description)).await?;
    Ok((StatusCode::CREATED, Json(project)).into_response())
}

pub async fn get_project(State(state): State<AppState>, Path(project_id): Path<i64>) -> AppResult<Response> {
    let project = state.db.call(move |c| repo::require_project(c, project_id)).await?;
    Ok(Json(project).into_response())
}

/// Permanently remove one project and everything it owns, including local files.
pub async fn delete_project(State(state): State<AppState>, Path(project_id): Path<i64>) -> AppResult<Response> {
    let files = state
        .db
        .call(move |c| {
            repo::require_project(c, project_id)?;
            let files = repo::project_files(c, project_id)?;
            repo::delete_project(c, project_id)?;
            Ok(files)
        })
        .await?;
    for path in &files.papers {
        state.storage.remove_file(path);
    }
    for path in &files.images {
        state.storage.remove_file(path);
    }
    for dir in &files.latex_dirs {
        state.storage.remove_dir(dir);
    }
    state.storage.remove_abs_dir(&state.storage.papers_dir(project_id));
    state.storage.remove_abs_dir(&state.storage.images_dir(project_id));
    state.storage.remove_abs_dir(&state.storage.latex_dir(project_id));
    Ok(no_content())
}

pub async fn list_categories(State(state): State<AppState>, Path(project_id): Path<i64>) -> AppResult<Response> {
    let categories = state
        .db
        .call(move |c| {
            repo::require_project(c, project_id)?;
            repo::list_categories(c, project_id)
        })
        .await?;
    Ok(Json(categories).into_response())
}

pub async fn create_category(
    State(state): State<AppState>,
    Path(project_id): Path<i64>,
    ValidJson(body): ValidJson<CategoryIn>,
) -> AppResult<Response> {
    let name = body.name.trim().to_string();
    let description = body.description.trim().to_string();
    let category = state
        .db
        .call(move |c| {
            repo::require_project(c, project_id)?;
            if name.is_empty() {
                return Err(AppError::unprocessable("Category name is required"));
            }
            if name.chars().count() > 160 {
                return Err(AppError::unprocessable("Category name must be 160 characters or fewer"));
            }
            if repo::category_name_exists(c, project_id, &name)? {
                return Err(AppError::Conflict("A category with this name already exists in this project".into()));
            }
            repo::create_category(c, project_id, &name, &description)
        })
        .await?;
    Ok((StatusCode::CREATED, Json(category)).into_response())
}

#[derive(Deserialize)]
pub struct DeleteCategoryQuery {
    #[serde(default)]
    delete_papers: bool,
}

pub async fn delete_category(
    State(state): State<AppState>,
    Path(category_id): Path<i64>,
    Query(query): Query<DeleteCategoryQuery>,
) -> AppResult<Response> {
    let delete_papers = query.delete_papers;
    let removed = state
        .db
        .call(move |c| {
            if repo::get_category(c, category_id)?.is_none() {
                return Err(AppError::not_found("Category not found"));
            }
            repo::delete_category(c, category_id, delete_papers)
        })
        .await?;
    for path in &removed {
        state.storage.remove_file(path);
    }
    Ok(no_content())
}
