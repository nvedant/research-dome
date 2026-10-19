//! Starts / stops the private loopback API.

use std::net::SocketAddr;

use axum::Router;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

use crate::api::{build_router, AppState};
use crate::config::Config;
use crate::db::Db;
use crate::error::{AppError, AppResult};
use crate::storage::Storage;

pub struct ServerHandle {
    pub addr: SocketAddr,
    /// Per-launch secret the UI must present with every request.
    pub token: String,
    /// The router, kept so in-process callers (e.g. "save as") can invoke routes
    /// without going through the network.
    pub router: Router,
    pub storage: Storage,
    shutdown: Option<oneshot::Sender<()>>,
    join: Option<JoinHandle<()>>,
}

impl ServerHandle {
    pub fn api_base(&self) -> String {
        format!("http://127.0.0.1:{}/api", self.addr.port())
    }

    pub async fn shutdown(mut self) {
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
        if let Some(join) = self.join.take() {
            let _ = join.await;
        }
    }
}

pub fn generate_token() -> String {
    format!("{}{}", uuid::Uuid::new_v4().simple(), uuid::Uuid::new_v4().simple())
}

/// Open the library, then serve the API on `127.0.0.1` (random port unless `port`
/// is given). Only the local machine can connect.
pub async fn start(config: &Config, port: Option<u16>, token: Option<String>) -> AppResult<ServerHandle> {
    std::fs::create_dir_all(&config.data_dir)?;
    let storage = Storage::new(config.storage_root())?;
    let db = Db::open(&config.db_path())?;
    let token = token.unwrap_or_else(generate_token);
    let state = AppState::new(db, storage.clone(), Some(token.clone()));
    let router = build_router(state);

    let listener = tokio::net::TcpListener::bind(("127.0.0.1", port.unwrap_or(0)))
        .await
        .map_err(|e| AppError::internal(format!("Could not open a local port: {e}")))?;
    let addr = listener.local_addr()?;
    let (tx, rx) = oneshot::channel::<()>();
    let serve_router = router.clone();
    let join = tokio::spawn(async move {
        let result = axum::serve(listener, serve_router)
            .with_graceful_shutdown(async {
                let _ = rx.await;
            })
            .await;
        if let Err(err) = result {
            tracing::error!(%err, "API server stopped unexpectedly");
        }
    });
    tracing::info!(%addr, data_dir = %config.data_dir.display(), "Research Dome API listening");
    Ok(ServerHandle { addr, token, router, storage, shutdown: Some(tx), join: Some(join) })
}

/// Initialise file + console logging. Keep the returned guard alive for the
/// lifetime of the process so buffered log lines are flushed.
pub fn init_logging(config: &Config) -> Option<tracing_appender::non_blocking::WorkerGuard> {
    use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info,hyper=warn"));
    let _ = std::fs::create_dir_all(config.log_dir());
    let appender = tracing_appender::rolling::daily(config.log_dir(), "research-dome.log");
    let (writer, guard) = tracing_appender::non_blocking(appender);
    let result = tracing_subscriber::registry()
        .with(filter)
        .with(fmt::layer().with_writer(std::io::stderr))
        .with(fmt::layer().with_ansi(false).with_writer(writer))
        .try_init();
    result.ok().map(|_| guard)
}
