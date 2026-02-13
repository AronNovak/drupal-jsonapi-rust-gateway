use axum::{
    body::Body,
    extract::{Request, State},
    http::StatusCode,
    response::Response,
};
use std::sync::Arc;

use crate::handlers::AppState;

pub async fn proxy_handler(
    State(state): State<Arc<AppState>>,
    request: Request,
) -> Result<Response, StatusCode> {
    let backend_url = &state.config.drupal.backend_url;
    let path = request.uri().path();
    let query = request.uri().query().map(|q| format!("?{}", q)).unwrap_or_default();
    let url = format!("{}{}{}", backend_url, path, query);

    let method = request.method().clone();
    let headers = request.headers().clone();

    let client = reqwest::Client::new();
    let mut req_builder = client.request(method, &url);

    for (name, value) in headers.iter() {
        if name == "host" {
            continue;
        }
        req_builder = req_builder.header(name, value);
    }

    let body_bytes = axum::body::to_bytes(request.into_body(), 10_000_000)
        .await
        .map_err(|_| StatusCode::BAD_REQUEST)?;

    if !body_bytes.is_empty() {
        req_builder = req_builder.body(body_bytes.to_vec());
    }

    let resp = req_builder
        .send()
        .await
        .map_err(|_| StatusCode::BAD_GATEWAY)?;

    let status = StatusCode::from_u16(resp.status().as_u16())
        .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    let resp_headers = resp.headers().clone();
    let resp_body = resp.bytes().await.map_err(|_| StatusCode::BAD_GATEWAY)?;

    let mut response = Response::builder().status(status);
    for (name, value) in resp_headers.iter() {
        if name == "transfer-encoding" {
            continue;
        }
        response = response.header(name, value);
    }

    response
        .body(Body::from(resp_body))
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}
