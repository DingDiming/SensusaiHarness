use axum::{
    extract::{Path, State, Json},
};
use serde::Deserialize;

use crate::AppState;

#[derive(Deserialize)]
pub struct IngestPayload {
    pub event_type: String,
    pub data: serde_json::Value,
}

pub async fn ingest_event(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
    Json(payload): Json<IngestPayload>,
) -> Json<serde_json::Value> {
    let event_id = state.bus.publish(&run_id, payload.event_type, payload.data).await;
    Json(serde_json::json!({ "event_id": event_id }))
}
