use axum::extract::{Multipart, Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;

use super::{file_response, no_content, AppState};
use crate::error::{AppError, AppResult};
use crate::repo;
use crate::util::{extension_lower, head_chars, safe_filename};

const MAX_IMAGE_BYTES: usize = 20 * 1024 * 1024;

pub async fn list_images(State(state): State<AppState>, Path(project_id): Path<i64>) -> AppResult<Response> {
    let images = state
        .db
        .call(move |c| {
            repo::require_project(c, project_id)?;
            repo::list_images(c, project_id)
        })
        .await?;
    Ok(Json(images).into_response())
}

fn optional_int(name: &str, value: &str) -> AppResult<Option<i64>> {
    let value = value.trim();
    if value.is_empty() {
        return Ok(None);
    }
    value
        .parse::<i64>()
        .map(Some)
        .map_err(|_| AppError::unprocessable(format!("{name} must be a whole number")))
}

pub async fn create_image(State(state): State<AppState>, mut multipart: Multipart) -> AppResult<Response> {
    let mut project_id: Option<i64> = None;
    let mut title = String::from("Research clipping");
    let mut paper_id: Option<i64> = None;
    let mut page_number: Option<i64> = None;
    let mut file: Option<(Option<String>, Vec<u8>)> = None;

    while let Some(field) = multipart.next_field().await? {
        let name = field.name().unwrap_or("").to_string();
        match name.as_str() {
            "project_id" => {
                project_id = optional_int("project_id", &field.text().await?)?;
            }
            "title" => title = field.text().await?,
            "paper_id" => paper_id = optional_int("paper_id", &field.text().await?)?,
            "page_number" => page_number = optional_int("page_number", &field.text().await?)?,
            "file" => {
                let content_type = field.content_type().map(str::to_owned);
                let data = field.bytes().await?;
                file = Some((content_type, data.to_vec()));
            }
            _ => {}
        }
    }
    let project_id = project_id.ok_or_else(|| AppError::unprocessable("Field required: project_id"))?;
    let (content_type, data) = file.ok_or_else(|| AppError::unprocessable("Field required: file"))?;

    state
        .db
        .call(move |c| {
            repo::require_project(c, project_id)?;
            if let Some(paper_id) = paper_id {
                match repo::get_paper(c, paper_id) {
                    Ok(paper) if paper.out.project_id == project_id => {}
                    _ => return Err(AppError::bad_request("Selected paper must belong to this project")),
                }
            }
            Ok(())
        })
        .await?;

    let content_type = content_type
        .unwrap_or_default()
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_lowercase();
    let extension = match content_type.as_str() {
        "image/png" => ".png",
        "image/jpeg" => ".jpg",
        "image/webp" => ".webp",
        _ => return Err(AppError::UnsupportedMedia("Image clippings must be PNG, JPEG, or WebP".into())),
    };
    if data.len() > MAX_IMAGE_BYTES {
        return Err(AppError::TooLarge("Image clipping exceeds 20 MB".into()));
    }
    if data.is_empty() {
        return Err(AppError::bad_request("Empty image upload"));
    }

    let dir = state.storage.images_dir(project_id);
    tokio::fs::create_dir_all(&dir).await?;
    let target = dir.join(format!("{}{}", uuid::Uuid::new_v4().simple(), extension));
    tokio::fs::write(&target, &data).await?;
    let relative = state.storage.relative(&target)?;

    let clean_title = {
        let t = head_chars(title.trim(), 250);
        if t.is_empty() { "Research clipping".to_string() } else { t }
    };
    let inserted = {
        let relative = relative.clone();
        state
            .db
            .call(move |c| repo::insert_image(c, project_id, paper_id, &clean_title, page_number, &relative))
            .await
    };
    match inserted {
        Ok(image) => Ok((StatusCode::CREATED, Json(image)).into_response()),
        Err(err) => {
            state.storage.remove_file(&relative);
            Err(err)
        }
    }
}

pub async fn image_file(State(state): State<AppState>, Path(image_id): Path<i64>) -> AppResult<Response> {
    let record = state.db.call(move |c| repo::get_image(c, image_id)).await?;
    let path = state.storage.resolve(&record.storage_path)?;
    let bytes = tokio::fs::read(&path)
        .await
        .map_err(|_| AppError::not_found("Image file is missing"))?;
    let ext = extension_lower(&record.storage_path);
    let content_type = match ext.as_str() {
        ".png" => "image/png",
        ".jpg" | ".jpeg" => "image/jpeg",
        ".webp" => "image/webp",
        _ => "application/octet-stream",
    };
    let filename = format!("{}{}", safe_filename(&record.out.title), ext);
    Ok(file_response(bytes, content_type, &filename, "inline"))
}

pub async fn delete_image(State(state): State<AppState>, Path(image_id): Path<i64>) -> AppResult<Response> {
    let record = state
        .db
        .call(move |c| {
            let record = repo::get_image(c, image_id)?;
            repo::delete_image(c, image_id)?;
            Ok(record)
        })
        .await?;
    state.storage.remove_file(&record.storage_path);
    Ok(no_content())
}
