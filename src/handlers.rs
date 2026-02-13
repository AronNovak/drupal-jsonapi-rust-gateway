use crate::config::AppConfig;
use crate::document;
use crate::entity_loader;
use crate::error::AppError;
use crate::field_loader;
use crate::include_resolver;
use crate::query;
use crate::serializer;
use crate::types::ResourceTypeMap;
use axum::{
    extract::{Path, RawQuery, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde_json::Value;
use sqlx::mysql::MySqlPool;
use std::sync::Arc;

pub struct AppState {
    pub pool: MySqlPool,
    pub resource_type_map: ResourceTypeMap,
    pub config: AppConfig,
}

fn jsonapi_response(body: Value) -> Response {
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "application/vnd.api+json")],
        Json(body),
    )
        .into_response()
}

pub async fn entrypoint_handler(
    State(state): State<Arc<AppState>>,
) -> Result<Response, AppError> {
    let base_url = &state.config.server.base_url;
    let mut links: Vec<(String, String)> = state
        .resource_type_map
        .all_types()
        .map(|rt| {
            (
                rt.jsonapi_type.clone(),
                format!("{}/jsonapi/{}", base_url, rt.path),
            )
        })
        .collect();
    links.sort_by(|a, b| a.0.cmp(&b.0));

    let doc = document::build_entrypoint_document(links);
    Ok(jsonapi_response(doc))
}

pub async fn collection_handler(
    State(state): State<Arc<AppState>>,
    Path((entity_type, bundle)): Path<(String, String)>,
    RawQuery(raw_query): RawQuery,
) -> Result<Response, AppError> {
    let resource_type = state
        .resource_type_map
        .get_by_path(&entity_type, &bundle)
        .ok_or_else(|| {
            AppError::NotFound(format!(
                "Resource type {}/{} not found",
                entity_type, bundle
            ))
        })?
        .clone();

    let qs = raw_query.as_deref().unwrap_or("");
    let parsed_query = query::parse_query(
        qs,
        state.config.pagination.default_limit,
        state.config.pagination.max_limit,
    );

    let mut entities =
        entity_loader::load_entity_collection(&state.pool, &resource_type, &parsed_query).await?;

    field_loader::load_field_data(&state.pool, &resource_type, &mut entities).await?;

    include_resolver::resolve_uuids(
        &state.pool,
        &state.resource_type_map,
        &mut entities,
        &resource_type,
    )
    .await?;

    let included = include_resolver::resolve_includes(
        &state.pool,
        &state.resource_type_map,
        &mut entities,
        &resource_type,
        &parsed_query.include,
        &state.config.server.base_url,
    )
    .await?;

    let base_url = &state.config.server.base_url;
    let sparse = parsed_query.fields.get(&resource_type.jsonapi_type);

    let data: Vec<Value> = entities
        .iter()
        .map(|e| serializer::serialize_entity(e, &resource_type, base_url, sparse))
        .collect();

    let self_url = if qs.is_empty() {
        format!("{}/jsonapi/{}", base_url, resource_type.path)
    } else {
        format!("{}/jsonapi/{}?{}", base_url, resource_type.path, qs)
    };

    let next_url = if entities.len() as u64 >= parsed_query.page_limit {
        let next_offset = parsed_query.page_offset + parsed_query.page_limit;
        Some(format!(
            "{}/jsonapi/{}?page%5Boffset%5D={}&page%5Blimit%5D={}",
            base_url, resource_type.path, next_offset, parsed_query.page_limit
        ))
    } else {
        None
    };

    let doc = document::build_collection_document(
        data,
        included,
        &self_url,
        next_url.as_deref(),
    );
    Ok(jsonapi_response(doc))
}

pub async fn individual_handler(
    State(state): State<Arc<AppState>>,
    Path((entity_type, bundle, uuid)): Path<(String, String, String)>,
    RawQuery(raw_query): RawQuery,
) -> Result<Response, AppError> {
    let resource_type = state
        .resource_type_map
        .get_by_path(&entity_type, &bundle)
        .ok_or_else(|| {
            AppError::NotFound(format!(
                "Resource type {}/{} not found",
                entity_type, bundle
            ))
        })?
        .clone();

    let qs = raw_query.as_deref().unwrap_or("");
    let parsed_query = query::parse_query(
        qs,
        state.config.pagination.default_limit,
        state.config.pagination.max_limit,
    );

    let entity = entity_loader::load_entity_by_uuid(&state.pool, &resource_type, &uuid).await?;

    let mut entity = entity.ok_or_else(|| {
        AppError::NotFound("The resource does not exist.".to_string())
    })?;

    // Load field data
    let mut entities = vec![entity];
    field_loader::load_field_data(&state.pool, &resource_type, &mut entities).await?;

    include_resolver::resolve_uuids(
        &state.pool,
        &state.resource_type_map,
        &mut entities,
        &resource_type,
    )
    .await?;

    let included = include_resolver::resolve_includes(
        &state.pool,
        &state.resource_type_map,
        &mut entities,
        &resource_type,
        &parsed_query.include,
        &state.config.server.base_url,
    )
    .await?;

    entity = entities.into_iter().next().unwrap();

    let base_url = &state.config.server.base_url;
    let sparse = parsed_query.fields.get(&resource_type.jsonapi_type);

    let data = serializer::serialize_entity(&entity, &resource_type, base_url, sparse);

    let self_url = format!(
        "{}/jsonapi/{}/{}",
        base_url, resource_type.path, uuid
    );

    let doc = document::build_individual_document(data, included, &self_url);
    Ok(jsonapi_response(doc))
}
