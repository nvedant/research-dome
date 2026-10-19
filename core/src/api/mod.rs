//! The private loopback HTTP API used by the desktop UI. It exposes exactly the
//! routes the original FastAPI service did, protected by a per-launch secret.

mod annotations;
mod images;
mod latex_docs;
mod papers;
mod projects;

use std::sync::Arc;

use axum::extract::{DefaultBodyLimit, FromRequest, Request, State};
use axum::http::{header, HeaderMap, HeaderValue, Method, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::Router;
use axum::body::Bytes;
use serde::de::DeserializeOwned;
use tower_http::cors::{AllowOrigin, CorsLayer};

use crate::db::Db;
use crate::error::AppError;
use crate::storage::Storage;

/// Header carrying the per-launch secret (file/image URLs use `?token=` instead).
pub const TOKEN_HEADER: &str = "x-dome-token";

#[derive(Clone)]
pub struct AppState {
    pub db: Db,
    pub storage: Storage,
    /// Serialises LaTeX builds so two builds never fight over one workspace.
    pub build_lock: Arc<tokio::sync::Mutex<()>>,
    /// When set, every request must present this secret.
    pub token: Option<Arc<str>>,
}

impl AppState {
    pub fn new(db: Db, storage: Storage, token: Option<String>) -> Self {
        Self {
            db,
            storage,
            build_lock: Arc::new(tokio::sync::Mutex::new(())),
            token: token.map(|t| Arc::from(t.as_str())),
        }
    }
}

/// JSON body extractor that reports malformed bodies as `422 {"detail": …}`.
pub struct ValidJson<T>(pub T);

impl<S, T> FromRequest<S> for ValidJson<T>
where
    T: DeserializeOwned,
    S: Send + Sync,
{
    type Rejection = AppError;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        let bytes = Bytes::from_request(req, state)
            .await
            .map_err(|e| AppError::bad_request(format!("Could not read the request body: {e}")))?;
        serde_json::from_slice(&bytes)
            .map(ValidJson)
            .map_err(|e| AppError::unprocessable(format!("Invalid request body: {e}")))
    }
}

impl From<axum::extract::multipart::MultipartError> for AppError {
    fn from(err: axum::extract::multipart::MultipartError) -> Self {
        if err.status() == StatusCode::PAYLOAD_TOO_LARGE {
            AppError::TooLarge("The upload is too large".into())
        } else {
            AppError::bad_request(format!("Invalid upload: {err}"))
        }
    }
}

/// Build a file download / inline response.
pub fn file_response(bytes: Vec<u8>, content_type: &str, filename: &str, disposition: &str) -> Response {
    let clean: String = filename
        .chars()
        .map(|c| if c.is_ascii_graphic() && c != '"' && c != '\\' { c } else { '_' })
        .collect();
    let mut headers = HeaderMap::new();
    if let Ok(value) = HeaderValue::from_str(content_type) {
        headers.insert(header::CONTENT_TYPE, value);
    }
    if let Ok(value) = HeaderValue::from_str(&format!("{disposition}; filename=\"{clean}\"")) {
        headers.insert(header::CONTENT_DISPOSITION, value);
    }
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    (StatusCode::OK, headers, bytes).into_response()
}

pub fn no_content() -> Response {
    StatusCode::NO_CONTENT.into_response()
}

/// Reject requests that do not carry the per-launch secret.
async fn require_token(State(state): State<AppState>, req: Request, next: Next) -> Response {
    if let Some(expected) = &state.token {
        let from_header = req
            .headers()
            .get(TOKEN_HEADER)
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned);
        let from_query = req.uri().query().and_then(|q| {
            q.split('&')
                .find_map(|pair| pair.strip_prefix("token=").map(str::to_owned))
        });
        let presented = from_header.or(from_query);
        if presented.as_deref() != Some(expected.as_ref()) {
            return AppError::Unauthorized("Unauthorized".into()).into_response();
        }
    }
    next.run(req).await
}

const ALLOWED_ORIGINS: &[&str] = &[
    "tauri://localhost",
    "http://tauri.localhost",
    "https://tauri.localhost",
    "http://localhost:1420",
    "http://127.0.0.1:1420",
];

pub fn build_router(state: AppState) -> Router {
    let origins: Vec<HeaderValue> = ALLOWED_ORIGINS.iter().filter_map(|o| HeaderValue::from_str(o).ok()).collect();
    let cors = CorsLayer::new()
        .allow_origin(AllowOrigin::list(origins))
        .allow_methods([Method::GET, Method::POST, Method::PUT, Method::PATCH, Method::DELETE, Method::OPTIONS])
        .allow_headers([header::CONTENT_TYPE, header::HeaderName::from_static(TOKEN_HEADER)])
        .expose_headers([header::CONTENT_DISPOSITION]);

    let api = Router::new()
        .route("/health", get(projects::health))
        // projects
        .route("/projects", get(projects::list_projects).post(projects::create_project))
        .route("/projects/{project_id}", get(projects::get_project).delete(projects::delete_project))
        // categories
        .route(
            "/projects/{project_id}/categories",
            get(projects::list_categories).post(projects::create_category),
        )
        .route("/categories/{category_id}", delete(projects::delete_category))
        // papers
        .route("/projects/{project_id}/papers", get(papers::list_papers))
        .route("/papers", post(papers::upload_paper))
        .route(
            "/papers/{paper_id}",
            get(papers::get_paper).patch(papers::update_paper).delete(papers::delete_paper),
        )
        .route("/papers/{paper_id}/file", get(papers::paper_file))
        .route("/papers/{paper_id}/download", get(papers::download_original))
        .route("/papers/{paper_id}/annotated-file", get(papers::download_annotated))
        // annotations + important sentences
        .route(
            "/papers/{paper_id}/annotations",
            get(annotations::list_annotations).post(annotations::create_annotation),
        )
        .route("/annotations/{annotation_id}", delete(annotations::delete_annotation))
        .route("/projects/{project_id}/important-sentences", get(annotations::important_sentences))
        // important images
        .route("/projects/{project_id}/images", get(images::list_images))
        .route("/images", post(images::create_image))
        .route("/images/{image_id}/file", get(images::image_file))
        .route("/images/{image_id}", delete(images::delete_image))
        // LaTeX studio
        .route(
            "/projects/{project_id}/latex-documents",
            get(latex_docs::list_documents).post(latex_docs::create_document),
        )
        .route(
            "/latex-documents/{document_id}",
            get(latex_docs::get_document).put(latex_docs::save_document).delete(latex_docs::delete_document),
        )
        .route("/latex-documents/{document_id}/files", get(latex_docs::list_files).post(latex_docs::upload_file))
        .route(
            "/latex-documents/{document_id}/file",
            get(latex_docs::get_file).put(latex_docs::save_file).delete(latex_docs::delete_file),
        )
        .route("/latex-documents/{document_id}/asset", get(latex_docs::get_asset))
        .route("/latex-documents/{document_id}/compile", post(latex_docs::compile_document))
        .route("/latex-documents/{document_id}/pdf", get(latex_docs::preview_pdf))
        .route("/latex-documents/{document_id}/download", get(latex_docs::download_pdf))
        .route("/latex-documents/{document_id}/source", get(latex_docs::download_source));

    Router::new()
        .nest("/api", api)
        .layer(middleware::from_fn_with_state(state.clone(), require_token))
        .layer(DefaultBodyLimit::max(200 * 1024 * 1024))
        .layer(cors)
        .with_state(state)
}
