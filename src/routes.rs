use crate::handlers::{self, AppState};
use crate::proxy;
use axum::{routing::get, Router};
use std::sync::Arc;

pub fn build_router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/jsonapi", get(handlers::entrypoint_handler))
        .route(
            "/jsonapi/:entity_type/:bundle",
            get(handlers::collection_handler)
                .post(proxy::proxy_handler),
        )
        .route(
            "/jsonapi/:entity_type/:bundle/:uuid",
            get(handlers::individual_handler)
                .patch(proxy::proxy_handler)
                .delete(proxy::proxy_handler),
        )
        .route(
            "/jsonapi/:entity_type/:bundle/:uuid/:field",
            get(proxy::proxy_handler),
        )
        .route(
            "/jsonapi/:entity_type/:bundle/:uuid/relationships/:field",
            get(proxy::proxy_handler)
                .post(proxy::proxy_handler)
                .patch(proxy::proxy_handler)
                .delete(proxy::proxy_handler),
        )
        .with_state(state)
}
