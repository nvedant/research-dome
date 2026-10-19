use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::Value;

use super::{no_content, AppState, ValidJson};
use crate::error::{AppError, AppResult};
use crate::models::AnnotationIn;
use crate::repo;
use crate::util::is_hex_color;

pub async fn list_annotations(State(state): State<AppState>, Path(paper_id): Path<i64>) -> AppResult<Response> {
    let items = state
        .db
        .call(move |c| {
            repo::get_paper(c, paper_id)?;
            repo::list_annotations(c, paper_id)
        })
        .await?;
    Ok(Json(items).into_response())
}

fn validate(payload: &AnnotationIn) -> AppResult<()> {
    if payload.page_number < 1 {
        return Err(AppError::unprocessable("page_number must be 1 or greater"));
    }
    if payload.annotation_type != "sentence" && payload.annotation_type != "rectangle" {
        return Err(AppError::unprocessable("annotation_type must be 'sentence' or 'rectangle'"));
    }
    if payload.rects.is_empty() {
        return Err(AppError::unprocessable("Select text or draw a rectangle first"));
    }
    if payload.annotation_type == "sentence" && payload.text.trim().is_empty() {
        return Err(AppError::unprocessable("Selected sentence text is required"));
    }
    if !is_hex_color(&payload.color) {
        return Err(AppError::unprocessable("Color must be a six-digit hex color"));
    }
    for rect in &payload.rects {
        for key in ["x", "y", "width", "height"] {
            let value = rect.get(key).and_then(Value::as_f64).ok_or_else(|| {
                AppError::unprocessable("Each rectangle needs numeric x, y, width and height values")
            })?;
            if !(0.0..=1.0).contains(&value) {
                return Err(AppError::unprocessable("Rectangle coordinates must be normalized between 0 and 1"));
            }
        }
    }
    Ok(())
}

pub async fn create_annotation(
    State(state): State<AppState>,
    Path(paper_id): Path<i64>,
    ValidJson(payload): ValidJson<AnnotationIn>,
) -> AppResult<Response> {
    if payload.page_number < 1 || (payload.annotation_type != "sentence" && payload.annotation_type != "rectangle") {
        validate(&payload)?;
    }
    let annotation = state
        .db
        .call(move |c| {
            repo::get_paper(c, paper_id)?;
            validate(&payload)?;
            repo::insert_annotation(
                c,
                paper_id,
                payload.page_number,
                &payload.annotation_type,
                payload.text.trim(),
                &payload.color,
                &payload.rects,
            )
        })
        .await?;
    Ok((StatusCode::CREATED, Json(annotation)).into_response())
}

pub async fn delete_annotation(State(state): State<AppState>, Path(annotation_id): Path<i64>) -> AppResult<Response> {
    let removed = state.db.call(move |c| repo::delete_annotation(c, annotation_id)).await?;
    if !removed {
        return Err(AppError::not_found("Annotation not found"));
    }
    Ok(no_content())
}

pub async fn important_sentences(State(state): State<AppState>, Path(project_id): Path<i64>) -> AppResult<Response> {
    let sentences = state
        .db
        .call(move |c| {
            repo::require_project(c, project_id)?;
            repo::list_sentences(c, project_id)
        })
        .await?;
    Ok(Json(sentences).into_response())
}
