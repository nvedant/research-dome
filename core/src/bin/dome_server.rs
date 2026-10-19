//! Headless API server for development and automated testing:
//!
//!   cargo run -p research-dome-core --bin dome-server
//!
//! The desktop app embeds the same server; this binary just runs it standalone so
//! the UI can be developed in a normal browser (`npm run dev`).

use research_dome_core::{init_logging, start, Config};

#[tokio::main]
async fn main() {
    let default_dir = std::env::temp_dir().join("research-dome-dev");
    let config = Config::resolve(default_dir);
    let _guard = init_logging(&config);
    let port: u16 = std::env::var("DOME_PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(8000);
    let token = std::env::var("DOME_TOKEN").unwrap_or_else(|_| "dev-token".to_string());
    match start(&config, Some(port), Some(token.clone())).await {
        Ok(handle) => {
            println!("Research Dome API  : {}", handle.api_base());
            println!("Data directory     : {}", config.data_dir.display());
            println!("Token (X-Dome-Token): {token}");
            let _ = tokio::signal::ctrl_c().await;
            handle.shutdown().await;
        }
        Err(err) => {
            eprintln!("Could not start: {err}");
            std::process::exit(1);
        }
    }
}
