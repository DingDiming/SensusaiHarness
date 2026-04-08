use axum::{
    extract::State,
    http::{Request, Uri},
    body::Body,
    response::Response,
};

use crate::AppState;
use crate::auth::validate_token;

pub async fn proxy_handler(
    State(state): State<AppState>,
    mut req: Request<Body>,
) -> Response<Body> {
    let path = req.uri().path().to_string();

    // Only proxy /api/app/* requests
    if !path.starts_with("/api/app/") {
        return Response::builder()
            .status(404)
            .body(Body::from("Not Found"))
            .unwrap();
    }

    // Skip auth for login and health endpoints
    let skip_auth = path == "/api/app/auth/login" || path == "/api/app/health" || path.ends_with("/health");

    if !skip_auth {
        // Validate JWT and inject X-User-Id
        let token = req.headers()
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .map(|s| s.to_string());

        match token.and_then(|t| validate_token(&t, &state.config.jwt_secret)) {
            Some(user_id) => {
                req.headers_mut().insert(
                    "x-user-id",
                    user_id.parse().unwrap(),
                );
            }
            None => {
                return Response::builder()
                    .status(401)
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"detail":"Unauthorized"}"#))
                    .unwrap();
            }
        }
    }

    // Forward to Python upstream — rewrite /api/app/* → /api/*
    let upstream = &state.config.python_upstream;
    let pq = req.uri().path_and_query().map(|pq| pq.as_str().to_string()).unwrap_or(path.clone());
    let rewritten = pq.replacen("/api/app/", "/api/", 1);
    let uri: Uri = format!("{}{}", upstream, rewritten).parse().unwrap();

    *req.uri_mut() = uri;

    // Use hyper client to forward
    let client = hyper_util::client::legacy::Client::builder(hyper_util::rt::TokioExecutor::new())
        .build_http();

    match client.request(req).await {
        Ok(resp) => resp.map(Body::new),
        Err(e) => {
            tracing::error!("Proxy error: {}", e);
            Response::builder()
                .status(502)
                .body(Body::from("Bad Gateway"))
                .unwrap()
        }
    }
}
