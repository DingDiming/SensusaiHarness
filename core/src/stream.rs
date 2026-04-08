use axum::{
    extract::{Path, State, Query},
    response::{sse::{Event, Sse}, IntoResponse},
};
use std::convert::Infallible;

use crate::AppState;
use crate::auth::validate_token;

#[derive(serde::Deserialize)]
pub struct StreamParams {
    pub token: Option<String>,
    #[serde(rename = "lastEventId")]
    pub last_event_id: Option<u64>,
}

pub async fn sse_handler(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
    Query(params): Query<StreamParams>,
    headers: axum::http::HeaderMap,
) -> Result<impl IntoResponse, axum::http::StatusCode> {
    // Auth: check query param token OR Authorization header
    let token = params.token.or_else(|| {
        headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .map(|s| s.to_string())
    });

    let token = token.ok_or(axum::http::StatusCode::UNAUTHORIZED)?;
    validate_token(&token, &state.config.jwt_secret)
        .ok_or(axum::http::StatusCode::UNAUTHORIZED)?;

    // Get last event ID from query param or header
    let last_id = params.last_event_id.or_else(|| {
        headers
            .get("last-event-id")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse().ok())
    });

    let (replay, mut rx) = state.bus.subscribe(&run_id, last_id).await;

    let stream = async_stream::stream! {
        // Replay buffered events
        for event in replay {
            yield Ok::<Event, Infallible>(Event::default()
                .id(event.event_id.to_string())
                .event(&event.event_type)
                .json_data(&event.data)
                .unwrap_or_else(|_| Event::default()));
        }

        // Live events
        loop {
            match rx.recv().await {
                Ok(event) => {
                    yield Ok::<Event, Infallible>(Event::default()
                        .id(event.event_id.to_string())
                        .event(&event.event_type)
                        .json_data(&event.data)
                        .unwrap_or_else(|_| Event::default()));
                }
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    };

    use tokio::sync::broadcast;

    Ok(Sse::new(Box::pin(stream))
        .keep_alive(axum::response::sse::KeepAlive::default()))
}
