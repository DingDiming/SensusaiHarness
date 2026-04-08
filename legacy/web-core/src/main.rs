use std::net::SocketAddr;

mod auth;
mod config;
mod event_bus;
mod ingest;
mod proxy;
mod stream;

use axum::{
    Router,
    routing::{get, post},
    Json,
};
use serde_json::json;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,sensusai_core=debug".into()),
        )
        .init();

    let cfg = config::AppConfig::from_env();
    let bus = event_bus::EventBus::new(1000);
    let state = AppState {
        config: cfg.clone(),
        bus: bus.clone(),
    };

    let app = Router::new()
        // Core routes (Rust handles directly)
        .route("/api/core/health", get(health))
        .route("/api/core/runs/{run_id}/stream", get(stream::sse_handler))
        // Internal: Python → Rust event push
        .route("/internal/runs/{run_id}/events", post(ingest::ingest_event))
        // Proxy: /api/app/* → Python
        .fallback(proxy::proxy_handler)
        .layer(CorsLayer::very_permissive())
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    let addr = SocketAddr::from(([0, 0, 0, 0], cfg.port));
    tracing::info!("Rust Core listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

async fn health() -> Json<serde_json::Value> {
    Json(json!({
        "status": "ok",
        "version": "2.0.0",
        "time": chrono::Utc::now().to_rfc3339()
    }))
}

#[derive(Clone)]
pub struct AppState {
    pub config: config::AppConfig,
    pub bus: event_bus::EventBus,
}
