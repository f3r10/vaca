mod config;
mod db;
mod error;
mod http;
mod lock;
mod mint;
mod pot;

use std::sync::atomic::AtomicUsize;
use std::sync::{Arc, Mutex};

use http::AppState;

use crate::http::SimpleCache;

#[tokio::main]
async fn main() {
    let config = match config::Config::from_env() {
        Ok(config) => config,
        Err(message) => {
            eprintln!("{message}");
            std::process::exit(1);
        }
    };
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    tracing_subscriber::fmt().with_env_filter(filter).init();

    let key = config.server_key_status();
    tracing::info!(
        bind = %config.bind,
        experimental = config.experimental,
        mint_url_set = !config.mint_url.is_empty(),
        server_key_path_set = key.path_set,
        server_key_file_present = key.file_present,
        server_pubkey_set = config.server_pubkey.is_some(),
        max_pot_sats = config.max_pot_sats,
        grace_seconds = config.grace_seconds,
        db_path = %config.db_path.display(),
        "vaca server starting"
    );
    if !config.bind.ip().is_loopback() {
        tracing::warn!(bind = %config.bind, "bind address is not a loopback address");
    }

    let bind = config.bind;
    let connection = match db::open(&config.db_path) {
        Ok(connection) => connection,
        Err(message) => {
            tracing::error!(error = %message, "could not open the pot store");
            std::process::exit(1);
        }
    };
    match db::remember_missing_mint(&connection, &config.mint_url) {
        Ok(0) => {}
        Ok(filled) => tracing::info!(pots = filled, "stored the mint on pots that had none"),
        Err(message) => {
            tracing::error!(error = %message, "could not store the mint on existing pots");
            std::process::exit(1);
        }
    }
    let quote_cache = SimpleCache::new();
    let state = AppState {
        config: Arc::new(config),
        db: Arc::new(Mutex::new(connection)),
        mint: mint::MintBackend::Live,
        quote_cache,
        melt_attempts: Arc::new(AtomicUsize::new(0)),
    };
    let app = http::router(state);
    let listener = match tokio::net::TcpListener::bind(bind).await {
        Ok(listener) => listener,
        Err(err) => {
            tracing::error!(error = %err, %bind, "could not bind");
            std::process::exit(1);
        }
    };
    if let Err(err) = axum::serve(listener, app)
        .with_graceful_shutdown(shutdown())
        .await
    {
        tracing::error!(error = %err, "server stopped");
        std::process::exit(1);
    }
}

async fn shutdown() {
    let _ = tokio::signal::ctrl_c().await;
    tracing::info!("shutting down");
}
