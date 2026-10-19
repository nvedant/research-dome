//! Research Dome desktop shell.
//!
//! * starts the embedded Rust + SQLite service on a private loopback port,
//! * hands the UI its address and per-launch secret,
//! * provides native "Save as…" downloads and "Open data folder".

use std::path::PathBuf;

use axum::body::Body;
use axum::http::Request;
use http_body_util::BodyExt;
use research_dome_core::{api::TOKEN_HEADER, Config, ServerHandle};
use serde::Serialize;
use tauri::{AppHandle, Manager, State};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_opener::OpenerExt;
use tower::ServiceExt;

/// Everything the UI needs to talk to the embedded service.
struct Backend {
    server: ServerHandle,
    data_dir: PathBuf,
    // Dropping the guard flushes the log file, so keep it for the process lifetime.
    _log_guard: Option<tracing_appender_guard::Guard>,
}

mod tracing_appender_guard {
    /// Type-erased holder for the logging worker guard.
    pub struct Guard(#[allow(dead_code)] pub Box<dyn std::any::Any + Send + Sync>);
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct BackendInfo {
    api_base: String,
    token: String,
    data_dir: String,
}

/// Called by the UI once at start-up.
#[tauri::command]
fn backend_info(state: State<'_, Backend>) -> BackendInfo {
    BackendInfo {
        api_base: state.server.api_base(),
        token: state.server.token.clone(),
        data_dir: state.data_dir.display().to_string(),
    }
}

/// "Save as…": fetch an API download in-process, ask the user where to put it,
/// and write it there. Returns the chosen path, or `None` if the dialog was cancelled.
#[tauri::command]
async fn save_download(
    app: AppHandle,
    state: State<'_, Backend>,
    path: String,
    suggested_name: String,
) -> Result<Option<String>, String> {
    if !path.starts_with('/') || path.contains("..") {
        return Err("Invalid download path".into());
    }
    let request = Request::builder()
        .method("GET")
        .uri(format!("/api{path}"))
        .header(TOKEN_HEADER, state.server.token.as_str())
        .body(Body::empty())
        .map_err(|e| e.to_string())?;
    let response = state
        .server
        .router
        .clone()
        .oneshot(request)
        .await
        .map_err(|e| e.to_string())?;
    let status = response.status();
    let bytes = response
        .into_body()
        .collect()
        .await
        .map_err(|e| e.to_string())?
        .to_bytes();
    if !status.is_success() {
        let detail = serde_json::from_slice::<serde_json::Value>(&bytes)
            .ok()
            .and_then(|v| v.get("detail").and_then(|d| d.as_str().map(str::to_owned)))
            .unwrap_or_else(|| format!("Download failed ({status})"));
        return Err(detail);
    }

    let dialog_app = app.clone();
    let chosen = tauri::async_runtime::spawn_blocking(move || {
        dialog_app
            .dialog()
            .file()
            .set_file_name(&suggested_name)
            .blocking_save_file()
    })
    .await
    .map_err(|e| e.to_string())?;

    let Some(file_path) = chosen else { return Ok(None) };
    let target = file_path.into_path().map_err(|e| e.to_string())?;
    std::fs::write(&target, &bytes).map_err(|e| format!("Could not save the file: {e}"))?;
    Ok(Some(target.display().to_string()))
}

/// Reveal the folder that holds the library (database, PDFs, figures, LaTeX).
#[tauri::command]
fn open_data_folder(app: AppHandle, state: State<'_, Backend>) -> Result<(), String> {
    app.opener()
        .open_path(state.data_dir.display().to_string(), None::<&str>)
        .map_err(|e| e.to_string())
}

pub fn run() {
    tauri::Builder::default()
        // Must be first: a second launch focuses the running window instead of
        // opening another process that would fight over the same database.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.unminimize();
                let _ = window.show();
                let _ = window.set_focus();
            }
        }))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let default_dir = app
                .path()
                .app_data_dir()
                .map_err(|e| format!("Could not locate the application data folder: {e}"))?;
            let config = Config::resolve(default_dir);
            let guard = research_dome_core::init_logging(&config)
                .map(|g| tracing_appender_guard::Guard(Box::new(g)));
            let server = tauri::async_runtime::block_on(research_dome_core::start(&config, None, None))
                .map_err(|e| format!("Research Dome could not open its library: {e}"))?;
            app.manage(Backend { server, data_dir: config.data_dir.clone(), _log_guard: guard });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![backend_info, save_download, open_data_folder])
        .run(tauri::generate_context!())
        .expect("error while running Research Dome");
}
