use crate::entity_loader;
use crate::error::AppError;
use crate::field_loader;
use crate::serializer;
use crate::types::*;
use serde_json::Value;
use sqlx::mysql::MySqlPool;
use sqlx::Row;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

pub async fn resolve_uuids(
    pool: &MySqlPool,
    resource_type_map: &ResourceTypeMap,
    entities: &mut [EntityData],
    resource_type: &ResourceType,
    config_entity_uuids: &HashMap<String, String>,
) -> Result<(), AppError> {
    if entities.is_empty() {
        return Ok(());
    }
    resolve_reference_uuids(pool, resource_type_map, entities, resource_type, config_entity_uuids).await
}

pub async fn resolve_includes(
    pool: &MySqlPool,
    resource_type_map: &ResourceTypeMap,
    entities: &mut [EntityData],
    resource_type: &ResourceType,
    include_paths: &[String],
    base_url: &str,
    config_entity_uuids: &HashMap<String, String>,
) -> Result<Vec<Value>, AppError> {
    if include_paths.is_empty() || entities.is_empty() {
        return Ok(Vec::new());
    }

    let mut included_resources: Vec<Value> = Vec::new();
    let mut seen: HashSet<(String, String)> = HashSet::new();

    for path in include_paths {
        let parts: Vec<&str> = path.split('.').collect();
        resolve_include_path(
            pool,
            resource_type_map,
            entities,
            resource_type,
            &parts,
            base_url,
            &mut included_resources,
            &mut seen,
            config_entity_uuids,
        )
        .await?;
    }

    Ok(included_resources)
}

async fn resolve_reference_uuids(
    pool: &MySqlPool,
    resource_type_map: &ResourceTypeMap,
    entities: &mut [EntityData],
    resource_type: &ResourceType,
    config_entity_uuids: &HashMap<String, String>,
) -> Result<(), AppError> {
    // Collect all target_ids per target entity type
    let mut target_ids_by_type: HashMap<String, Vec<i64>> = HashMap::new();

    // From base field relationships
    for bf in &resource_type.base_fields {
        if !bf.is_relationship {
            continue;
        }
        if let Some(target_type) = &bf.target_type {
            if target_type == "node_type" || target_type == "taxonomy_vocabulary" {
                continue; // Config entities handled differently
            }
            for entity in entities.iter() {
                // For uid/revision_uid base fields, the value is the column value
                if let Some(val) = entity.base_field_values.get(&bf.column) {
                    if let Some(tid) = val.as_i64() {
                        target_ids_by_type
                            .entry(target_type.clone())
                            .or_default()
                            .push(tid);
                    }
                }
            }
        }
    }

    // From field storages (entity_reference, image, file)
    for fs in &resource_type.field_storages {
        let target_type = match &fs.target_type {
            Some(t) => t.clone(),
            None => continue,
        };
        if !matches!(
            fs.field_type.as_str(),
            "entity_reference" | "entity_reference_revisions" | "image" | "file"
        ) {
            continue;
        }

        for entity in entities.iter() {
            if let Some(field_val) = entity.field_values.get(&fs.field_name) {
                collect_target_ids(field_val, &mut target_ids_by_type.entry(target_type.clone()).or_default());
            }
        }
    }

    // Batch-load UUIDs (and bundles) for all target entities concurrently
    let uuid_futures: Vec<_> = target_ids_by_type.iter().map(|(target_type, ids)| {
        let unique_ids: Vec<i64> = ids.iter().copied().collect::<HashSet<_>>().into_iter().collect();
        let target_type = target_type.clone();
        let pool = pool.clone();
        async move {
            if unique_ids.is_empty() {
                return Ok((target_type, HashMap::new()));
            }
            let uuid_map = load_uuids(&pool, resource_type_map, &target_type, &unique_ids).await?;
            Ok::<_, AppError>((target_type, uuid_map))
        }
    }).collect();

    let uuid_results = futures::future::join_all(uuid_futures).await;

    let mut uuid_maps: HashMap<String, HashMap<i64, (String, String)>> = HashMap::new();
    for result in uuid_results {
        let (target_type, uuid_map) = result?;
        if !uuid_map.is_empty() {
            uuid_maps.insert(target_type, uuid_map);
        }
    }

    // Now update entities with resolved UUIDs
    for entity in entities.iter_mut() {
        // Base field relationships
        for bf in &resource_type.base_fields {
            if !bf.is_relationship {
                continue;
            }
            if let Some(target_type) = &bf.target_type {
                if target_type == "node_type" {
                    if let Some(uuid) = config_entity_uuids.get(&format!("node_type:{}", entity.bundle)) {
                        entity.base_field_values.insert("node_type_uuid".to_string(), Value::String(uuid.clone()));
                    }
                } else if target_type == "taxonomy_vocabulary" {
                    if let Some(uuid) = config_entity_uuids.get(&format!("taxonomy_vocabulary:{}", entity.bundle)) {
                        entity.base_field_values.insert("vid_uuid".to_string(), Value::String(uuid.clone()));
                    }
                } else if let Some(uuid_map) = uuid_maps.get(target_type.as_str()) {
                    if let Some(val) = entity.base_field_values.get(&bf.column) {
                        if let Some(tid) = val.as_i64() {
                            if let Some((uuid, bundle)) = uuid_map.get(&tid) {
                                entity.base_field_values.insert(
                                    format!("{}_uuid", bf.name),
                                    Value::String(uuid.clone()),
                                );
                                entity.base_field_values.insert(
                                    format!("{}_bundle", bf.name),
                                    Value::String(bundle.clone()),
                                );
                            }
                        }
                    }
                }
            }
        }

        // Field storage relationships
        for fs in &resource_type.field_storages {
            let target_type = match &fs.target_type {
                Some(t) => t,
                None => continue,
            };
            if let Some(uuid_map) = uuid_maps.get(target_type.as_str()) {
                if let Some(field_val) = entity.field_values.get_mut(&fs.field_name) {
                    update_reference_uuids(field_val, uuid_map);
                }
            }
        }
    }

    Ok(())
}

fn collect_target_ids(value: &Value, ids: &mut Vec<i64>) {
    match value {
        Value::Number(n) => {
            if let Some(id) = n.as_i64() {
                ids.push(id);
            }
        }
        Value::Array(arr) => {
            for item in arr {
                collect_target_ids(item, ids);
            }
        }
        Value::Object(obj) => {
            if let Some(tid) = obj.get("target_id").and_then(|v| v.as_i64()) {
                ids.push(tid);
            } else if let Some(tid) = obj.get("_target_id").and_then(|v| v.as_i64()) {
                ids.push(tid);
            }
        }
        _ => {}
    }
}

fn update_reference_uuids(value: &mut Value, uuid_map: &HashMap<i64, (String, String)>) {
    match value {
        Value::Number(_) => {}
        Value::Array(arr) => {
            for item in arr.iter_mut() {
                update_reference_uuids(item, uuid_map);
            }
        }
        Value::Object(obj) => {
            if let Some(tid) = obj.get("target_id").and_then(|v| v.as_i64()) {
                if let Some((uuid, bundle)) = uuid_map.get(&tid) {
                    obj.insert("uuid".to_string(), Value::String(uuid.clone()));
                    obj.insert("bundle".to_string(), Value::String(bundle.clone()));
                }
            }
        }
        _ => {}
    }
}

/// Returns (uuid, bundle) for each entity id
async fn load_uuids(
    pool: &MySqlPool,
    resource_type_map: &ResourceTypeMap,
    entity_type: &str,
    ids: &[i64],
) -> Result<HashMap<i64, (String, String)>, AppError> {
    if ids.is_empty() {
        return Ok(HashMap::new());
    }

    let et_info = match resource_type_map.entity_types.get(entity_type) {
        Some(info) => info,
        None => return Ok(HashMap::new()),
    };

    let placeholders: Vec<&str> = ids.iter().map(|_| "?").collect();
    // Include bundle column if present
    let bundle_select = if let Some(bundle_col) = &et_info.bundle_column {
        format!(", `{}`", bundle_col)
    } else {
        String::new()
    };
    let sql = format!(
        "SELECT `{}`, uuid{} FROM `{}` WHERE `{}` IN ({})",
        et_info.id_column,
        bundle_select,
        et_info.base_table,
        et_info.id_column,
        placeholders.join(", ")
    );

    let mut db_query = sqlx::query(&sql);
    for id in ids {
        db_query = db_query.bind(id);
    }

    let rows = db_query.fetch_all(pool).await?;
    let mut map = HashMap::new();
    let id_col: &str = &et_info.id_column;
    for row in rows {
        let id: i64 = row
            .try_get::<i64, _>(id_col)
            .or_else(|_| row.try_get::<u32, _>(id_col).map(|v| v as i64))
            .unwrap_or(0);
        let uuid: String = row.try_get("uuid").unwrap_or_default();
        let bundle: String = if let Some(bundle_col) = &et_info.bundle_column {
            row.try_get::<String, _>(bundle_col.as_str()).unwrap_or_else(|_| entity_type.to_string())
        } else {
            entity_type.to_string()
        };
        map.insert(id, (uuid, bundle));
    }

    Ok(map)
}

fn resolve_include_path<'a>(
    pool: &'a MySqlPool,
    resource_type_map: &'a ResourceTypeMap,
    entities: &'a [EntityData],
    resource_type: &'a ResourceType,
    path_parts: &'a [&'a str],
    base_url: &'a str,
    included: &'a mut Vec<Value>,
    seen: &'a mut HashSet<(String, String)>,
    config_entity_uuids: &'a HashMap<String, String>,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), AppError>> + Send + 'a>> {
    Box::pin(async move {
    if path_parts.is_empty() || entities.is_empty() {
        return Ok(());
    }

    let field_name = path_parts[0];
    let remaining = &path_parts[1..];

    // Find target entity type for this relationship
    let target_entity_type = resource_type.get_target_type(field_name);
    let target_entity_type = match target_entity_type {
        Some(t) => t.to_string(),
        None => return Ok(()),
    };

    // Skip config entity references (node_type, taxonomy_vocabulary)
    if target_entity_type == "node_type" || target_entity_type == "taxonomy_vocabulary" {
        return Ok(());
    }

    // Collect target entity IDs
    let mut target_ids: Vec<i64> = Vec::new();
    for entity in entities {
        // Check base fields
        for bf in &resource_type.base_fields {
            if bf.name == field_name && bf.is_relationship {
                if let Some(val) = entity.base_field_values.get(&bf.column) {
                    if let Some(id) = val.as_i64() {
                        target_ids.push(id);
                    }
                }
            }
        }
        // Check field storages
        if let Some(field_val) = entity.field_values.get(field_name) {
            collect_target_ids(field_val, &mut target_ids);
        }
    }

    let unique_ids: Vec<i64> = target_ids
        .into_iter()
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();

    if unique_ids.is_empty() {
        return Ok(());
    }

    // Find the resource type for the target
    // For user, file, etc. the bundle == entity_type
    let target_rt = find_target_resource_type(resource_type_map, &target_entity_type, entities, field_name);

    if let Some(target_rt) = target_rt {
        let mut target_entities =
            entity_loader::load_entities_by_ids(pool, &target_rt, &unique_ids).await?;
        field_loader::load_field_data(pool, &target_rt, &mut target_entities).await?;

        // Resolve UUIDs for the included entities' relationships too
        resolve_reference_uuids(pool, resource_type_map, &mut target_entities, &target_rt, config_entity_uuids).await?;

        for entity in &target_entities {
            let key = (entity.entity_type.clone(), entity.uuid.clone());
            if seen.contains(&key) {
                continue;
            }
            seen.insert(key);

            let serialized = serializer::serialize_entity(entity, &target_rt, base_url, None);
            included.push(serialized);
        }

        // Handle nested includes
        if !remaining.is_empty() {
            resolve_include_path(
                pool,
                resource_type_map,
                &target_entities,
                &target_rt,
                remaining,
                base_url,
                included,
                seen,
                config_entity_uuids,
            )
            .await?;
        }
    }

    Ok(())
    })
}

fn find_target_resource_type<'a>(
    resource_type_map: &'a ResourceTypeMap,
    target_entity_type: &str,
    _entities: &[EntityData],
    _field_name: &str,
) -> Option<Arc<ResourceType>> {
    // For entity types where bundle == entity_type (user, file)
    let key = format!("{}--{}", target_entity_type, target_entity_type);
    if let Some(rt) = resource_type_map.get_by_type(&key) {
        return Some(rt.clone());
    }

    // Try to find any resource type with this entity type
    for rt in resource_type_map.all_types() {
        if rt.entity_type == target_entity_type {
            return Some(rt.clone());
        }
    }

    None
}
