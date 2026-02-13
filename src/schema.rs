use crate::php_unserialize::php_unserialize;
use crate::types::*;
use sqlx::mysql::MySqlPool;
use std::collections::HashMap;
use std::sync::Arc;
use tracing::info;

pub async fn discover_schema(pool: &MySqlPool) -> Result<ResourceTypeMap, Box<dyn std::error::Error + Send + Sync>> {
    let entity_types = build_entity_type_infos();
    let field_storages = discover_field_storages(pool).await?;
    let bundle_fields = discover_bundle_fields(pool).await?;
    let bundles = discover_bundles(pool).await?;

    let mut by_type = HashMap::new();
    let mut by_path = HashMap::new();
    let mut entity_type_map = HashMap::new();

    for (et_name, et_info) in &entity_types {
        let et_arc = Arc::new(et_info.clone());
        entity_type_map.insert(et_name.clone(), et_arc.clone());

        let et_bundles = bundles
            .get(et_name.as_str())
            .cloned()
            .unwrap_or_default();

        // If no bundles discovered, use entity_type as bundle (e.g., user--user, file--file)
        let bundle_list = if et_bundles.is_empty() {
            vec![et_name.clone()]
        } else {
            et_bundles
        };

        for bundle in &bundle_list {
            let jsonapi_type = format!("{}--{}", et_name, bundle);
            let path = format!("{}/{}", et_name, bundle);

            let mut rt_field_storages = Vec::new();
            let bundle_key = format!("{}.{}", et_name, bundle);

            if let Some(field_names) = bundle_fields.get(&bundle_key) {
                for field_name in field_names {
                    let storage_key = format!("{}.{}", et_name, field_name);
                    if let Some(fs) = field_storages.get(&storage_key) {
                        rt_field_storages.push(fs.clone());
                    }
                }
            }

            let rt = ResourceType {
                entity_type: et_name.clone(),
                bundle: bundle.clone(),
                jsonapi_type: jsonapi_type.clone(),
                path: path.clone(),
                base_fields: et_info.base_fields.clone(),
                field_storages: rt_field_storages,
                entity_type_info: et_arc.clone(),
            };

            let rt_arc = Arc::new(rt);
            by_type.insert(jsonapi_type, rt_arc.clone());
            by_path.insert(path, rt_arc);
        }
    }

    info!(
        "Discovered {} resource types across {} entity types",
        by_type.len(),
        entity_type_map.len()
    );

    Ok(ResourceTypeMap {
        by_type,
        by_path,
        entity_types: entity_type_map,
    })
}

async fn discover_field_storages(
    pool: &MySqlPool,
) -> Result<HashMap<String, FieldStorageDef>, Box<dyn std::error::Error + Send + Sync>> {
    let rows: Vec<(String, Vec<u8>)> = sqlx::query_as(
        "SELECT name, data FROM config WHERE name LIKE 'field.storage.%'"
    )
    .fetch_all(pool)
    .await?;

    let mut map = HashMap::new();

    for (name, data) in rows {
        let php_val = match php_unserialize(&data) {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!("Failed to parse config {}: {}", name, e);
                continue;
            }
        };

        let entity_type = match php_val.get_str("entity_type") {
            Some(v) => v.to_string(),
            None => continue,
        };
        let field_name = match php_val.get_str("field_name") {
            Some(v) => v.to_string(),
            None => continue,
        };
        let field_type = match php_val.get_str("type") {
            Some(v) => v.to_string(),
            None => continue,
        };
        let cardinality = php_val.get_i64("cardinality").unwrap_or(1) as i32;

        let target_type = php_val
            .get("settings")
            .and_then(|s| s.get_str("target_type"))
            .map(|s| s.to_string());

        let columns = build_field_columns(&field_name, &field_type);

        let key = format!("{}.{}", entity_type, field_name);
        map.insert(
            key,
            FieldStorageDef {
                entity_type,
                field_name,
                field_type,
                cardinality,
                target_type,
                columns,
            },
        );
    }

    info!("Discovered {} field storages", map.len());
    Ok(map)
}

fn build_field_columns(field_name: &str, field_type: &str) -> Vec<FieldColumn> {
    match field_type {
        "text" | "text_long" | "text_with_summary" => vec![
            FieldColumn {
                property: "value".to_string(),
                column_name: format!("{}_value", field_name),
            },
            FieldColumn {
                property: "format".to_string(),
                column_name: format!("{}_format", field_name),
            },
        ],
        "entity_reference" | "entity_reference_revisions" => vec![FieldColumn {
            property: "target_id".to_string(),
            column_name: format!("{}_target_id", field_name),
        }],
        "image" => vec![
            FieldColumn {
                property: "target_id".to_string(),
                column_name: format!("{}_target_id", field_name),
            },
            FieldColumn {
                property: "alt".to_string(),
                column_name: format!("{}_alt", field_name),
            },
            FieldColumn {
                property: "title".to_string(),
                column_name: format!("{}_title", field_name),
            },
            FieldColumn {
                property: "width".to_string(),
                column_name: format!("{}_width", field_name),
            },
            FieldColumn {
                property: "height".to_string(),
                column_name: format!("{}_height", field_name),
            },
        ],
        "file" => vec![
            FieldColumn {
                property: "target_id".to_string(),
                column_name: format!("{}_target_id", field_name),
            },
            FieldColumn {
                property: "display".to_string(),
                column_name: format!("{}_display", field_name),
            },
            FieldColumn {
                property: "description".to_string(),
                column_name: format!("{}_description", field_name),
            },
        ],
        "link" => vec![
            FieldColumn {
                property: "uri".to_string(),
                column_name: format!("{}_uri", field_name),
            },
            FieldColumn {
                property: "title".to_string(),
                column_name: format!("{}_title", field_name),
            },
            FieldColumn {
                property: "options".to_string(),
                column_name: format!("{}_options", field_name),
            },
        ],
        "comment" => vec![
            FieldColumn {
                property: "status".to_string(),
                column_name: format!("{}_status", field_name),
            },
        ],
        // Simple value fields
        _ => vec![FieldColumn {
            property: "value".to_string(),
            column_name: format!("{}_value", field_name),
        }],
    }
}

async fn discover_bundle_fields(
    pool: &MySqlPool,
) -> Result<HashMap<String, Vec<String>>, Box<dyn std::error::Error + Send + Sync>> {
    let rows: Vec<(String, Vec<u8>)> = sqlx::query_as(
        "SELECT name, data FROM config WHERE name LIKE 'field.field.%'"
    )
    .fetch_all(pool)
    .await?;

    let mut map: HashMap<String, Vec<String>> = HashMap::new();

    for (name, data) in rows {
        let php_val = match php_unserialize(&data) {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!("Failed to parse config {}: {}", name, e);
                continue;
            }
        };

        let entity_type = match php_val.get_str("entity_type") {
            Some(v) => v.to_string(),
            None => continue,
        };
        let field_name = match php_val.get_str("field_name") {
            Some(v) => v.to_string(),
            None => continue,
        };
        let bundle = match php_val.get_str("bundle") {
            Some(v) => v.to_string(),
            None => continue,
        };

        let key = format!("{}.{}", entity_type, bundle);
        map.entry(key).or_default().push(field_name);
    }

    info!("Discovered bundle-field mappings for {} bundles", map.len());
    Ok(map)
}

async fn discover_bundles(
    pool: &MySqlPool,
) -> Result<HashMap<String, Vec<String>>, Box<dyn std::error::Error + Send + Sync>> {
    let mut map: HashMap<String, Vec<String>> = HashMap::new();

    // Node types
    let rows: Vec<(String,)> = sqlx::query_as(
        "SELECT name FROM config WHERE name LIKE 'node.type.%'"
    )
    .fetch_all(pool)
    .await?;
    for (name,) in rows {
        let bundle = name.strip_prefix("node.type.").unwrap_or("").to_string();
        if !bundle.is_empty() {
            map.entry("node".to_string()).or_default().push(bundle);
        }
    }

    // Taxonomy vocabularies
    let rows: Vec<(String,)> = sqlx::query_as(
        "SELECT name FROM config WHERE name LIKE 'taxonomy.vocabulary.%'"
    )
    .fetch_all(pool)
    .await?;
    for (name,) in rows {
        let bundle = name.strip_prefix("taxonomy.vocabulary.").unwrap_or("").to_string();
        if !bundle.is_empty() {
            map.entry("taxonomy_term".to_string())
                .or_default()
                .push(bundle);
        }
    }

    // Media types
    let rows: Vec<(String,)> = sqlx::query_as(
        "SELECT name FROM config WHERE name LIKE 'media.type.%'"
    )
    .fetch_all(pool)
    .await?;
    for (name,) in rows {
        let bundle = name.strip_prefix("media.type.").unwrap_or("").to_string();
        if !bundle.is_empty() {
            map.entry("media".to_string()).or_default().push(bundle);
        }
    }

    // Block content types
    let rows: Vec<(String,)> = sqlx::query_as(
        "SELECT name FROM config WHERE name LIKE 'block_content.type.%'"
    )
    .fetch_all(pool)
    .await?;
    for (name,) in rows {
        let bundle = name.strip_prefix("block_content.type.").unwrap_or("").to_string();
        if !bundle.is_empty() {
            map.entry("block_content".to_string())
                .or_default()
                .push(bundle);
        }
    }

    // Comment types
    let rows: Vec<(String,)> = sqlx::query_as(
        "SELECT name FROM config WHERE name LIKE 'comment.type.%'"
    )
    .fetch_all(pool)
    .await?;
    for (name,) in rows {
        let bundle = name.strip_prefix("comment.type.").unwrap_or("").to_string();
        if !bundle.is_empty() {
            map.entry("comment".to_string()).or_default().push(bundle);
        }
    }

    info!("Discovered bundles: {:?}", map.keys().collect::<Vec<_>>());
    Ok(map)
}

fn build_entity_type_infos() -> HashMap<String, EntityTypeInfo> {
    let mut map = HashMap::new();

    // Node
    map.insert(
        "node".to_string(),
        EntityTypeInfo {
            entity_type: "node".to_string(),
            base_table: "node".to_string(),
            data_table: Some("node_field_data".to_string()),
            revision_table: Some("node_revision".to_string()),
            id_column: "nid".to_string(),
            revision_id_column: Some("vid".to_string()),
            uuid_column: "uuid".to_string(),
            bundle_column: Some("type".to_string()),
            label_column: Some("title".to_string()),
            base_fields: vec![
                BaseFieldDef { name: "nid".to_string(), column: "nid".to_string(), field_type: BaseFieldType::Int, is_relationship: false, target_type: None },
                BaseFieldDef { name: "vid".to_string(), column: "vid".to_string(), field_type: BaseFieldType::Int, is_relationship: false, target_type: None },
                BaseFieldDef { name: "langcode".to_string(), column: "langcode".to_string(), field_type: BaseFieldType::Langcode, is_relationship: false, target_type: None },
                BaseFieldDef { name: "title".to_string(), column: "title".to_string(), field_type: BaseFieldType::String, is_relationship: false, target_type: None },
                BaseFieldDef { name: "status".to_string(), column: "status".to_string(), field_type: BaseFieldType::Bool, is_relationship: false, target_type: None },
                BaseFieldDef { name: "created".to_string(), column: "created".to_string(), field_type: BaseFieldType::Timestamp, is_relationship: false, target_type: None },
                BaseFieldDef { name: "changed".to_string(), column: "changed".to_string(), field_type: BaseFieldType::Timestamp, is_relationship: false, target_type: None },
                BaseFieldDef { name: "promote".to_string(), column: "promote".to_string(), field_type: BaseFieldType::Bool, is_relationship: false, target_type: None },
                BaseFieldDef { name: "sticky".to_string(), column: "sticky".to_string(), field_type: BaseFieldType::Bool, is_relationship: false, target_type: None },
                BaseFieldDef { name: "default_langcode".to_string(), column: "default_langcode".to_string(), field_type: BaseFieldType::Bool, is_relationship: false, target_type: None },
                BaseFieldDef { name: "revision_translation_affected".to_string(), column: "revision_translation_affected".to_string(), field_type: BaseFieldType::Bool, is_relationship: false, target_type: None },
                BaseFieldDef { name: "uid".to_string(), column: "uid".to_string(), field_type: BaseFieldType::Int, is_relationship: true, target_type: Some("user".to_string()) },
                BaseFieldDef { name: "revision_uid".to_string(), column: "revision_uid".to_string(), field_type: BaseFieldType::Int, is_relationship: true, target_type: Some("user".to_string()) },
                BaseFieldDef { name: "node_type".to_string(), column: "type".to_string(), field_type: BaseFieldType::String, is_relationship: true, target_type: Some("node_type".to_string()) },
            ],
        },
    );

    // User
    map.insert(
        "user".to_string(),
        EntityTypeInfo {
            entity_type: "user".to_string(),
            base_table: "users".to_string(),
            data_table: Some("users_field_data".to_string()),
            revision_table: None,
            id_column: "uid".to_string(),
            revision_id_column: None,
            uuid_column: "uuid".to_string(),
            bundle_column: None,
            label_column: Some("name".to_string()),
            base_fields: vec![
                BaseFieldDef { name: "uid".to_string(), column: "uid".to_string(), field_type: BaseFieldType::Int, is_relationship: false, target_type: None },
                BaseFieldDef { name: "langcode".to_string(), column: "langcode".to_string(), field_type: BaseFieldType::Langcode, is_relationship: false, target_type: None },
                BaseFieldDef { name: "name".to_string(), column: "name".to_string(), field_type: BaseFieldType::String, is_relationship: false, target_type: None },
                BaseFieldDef { name: "status".to_string(), column: "status".to_string(), field_type: BaseFieldType::Bool, is_relationship: false, target_type: None },
                BaseFieldDef { name: "created".to_string(), column: "created".to_string(), field_type: BaseFieldType::Timestamp, is_relationship: false, target_type: None },
                BaseFieldDef { name: "changed".to_string(), column: "changed".to_string(), field_type: BaseFieldType::Timestamp, is_relationship: false, target_type: None },
                BaseFieldDef { name: "default_langcode".to_string(), column: "default_langcode".to_string(), field_type: BaseFieldType::Bool, is_relationship: false, target_type: None },
            ],
        },
    );

    // Taxonomy term
    map.insert(
        "taxonomy_term".to_string(),
        EntityTypeInfo {
            entity_type: "taxonomy_term".to_string(),
            base_table: "taxonomy_term_data".to_string(),
            data_table: Some("taxonomy_term_field_data".to_string()),
            revision_table: Some("taxonomy_term_revision".to_string()),
            id_column: "tid".to_string(),
            revision_id_column: Some("revision_id".to_string()),
            uuid_column: "uuid".to_string(),
            bundle_column: Some("vid".to_string()),
            label_column: Some("name".to_string()),
            base_fields: vec![
                BaseFieldDef { name: "tid".to_string(), column: "tid".to_string(), field_type: BaseFieldType::Int, is_relationship: false, target_type: None },
                BaseFieldDef { name: "revision_id".to_string(), column: "revision_id".to_string(), field_type: BaseFieldType::Int, is_relationship: false, target_type: None },
                BaseFieldDef { name: "langcode".to_string(), column: "langcode".to_string(), field_type: BaseFieldType::Langcode, is_relationship: false, target_type: None },
                BaseFieldDef { name: "name".to_string(), column: "name".to_string(), field_type: BaseFieldType::String, is_relationship: false, target_type: None },
                BaseFieldDef { name: "status".to_string(), column: "status".to_string(), field_type: BaseFieldType::Bool, is_relationship: false, target_type: None },
                BaseFieldDef { name: "description__value".to_string(), column: "description__value".to_string(), field_type: BaseFieldType::String, is_relationship: false, target_type: None },
                BaseFieldDef { name: "description__format".to_string(), column: "description__format".to_string(), field_type: BaseFieldType::String, is_relationship: false, target_type: None },
                BaseFieldDef { name: "weight".to_string(), column: "weight".to_string(), field_type: BaseFieldType::Int, is_relationship: false, target_type: None },
                BaseFieldDef { name: "changed".to_string(), column: "changed".to_string(), field_type: BaseFieldType::Timestamp, is_relationship: false, target_type: None },
                BaseFieldDef { name: "default_langcode".to_string(), column: "default_langcode".to_string(), field_type: BaseFieldType::Bool, is_relationship: false, target_type: None },
                BaseFieldDef { name: "revision_translation_affected".to_string(), column: "revision_translation_affected".to_string(), field_type: BaseFieldType::Bool, is_relationship: false, target_type: None },
                BaseFieldDef { name: "revision_created".to_string(), column: "revision_created".to_string(), field_type: BaseFieldType::Timestamp, is_relationship: false, target_type: None },
                BaseFieldDef { name: "vid".to_string(), column: "vid".to_string(), field_type: BaseFieldType::String, is_relationship: true, target_type: Some("taxonomy_vocabulary".to_string()) },
                BaseFieldDef { name: "revision_user".to_string(), column: "revision_user".to_string(), field_type: BaseFieldType::Int, is_relationship: true, target_type: Some("user".to_string()) },
            ],
        },
    );

    // File
    map.insert(
        "file".to_string(),
        EntityTypeInfo {
            entity_type: "file".to_string(),
            base_table: "file_managed".to_string(),
            data_table: None,
            revision_table: None,
            id_column: "fid".to_string(),
            revision_id_column: None,
            uuid_column: "uuid".to_string(),
            bundle_column: None,
            label_column: Some("filename".to_string()),
            base_fields: vec![
                BaseFieldDef { name: "fid".to_string(), column: "fid".to_string(), field_type: BaseFieldType::Int, is_relationship: false, target_type: None },
                BaseFieldDef { name: "langcode".to_string(), column: "langcode".to_string(), field_type: BaseFieldType::Langcode, is_relationship: false, target_type: None },
                BaseFieldDef { name: "filename".to_string(), column: "filename".to_string(), field_type: BaseFieldType::String, is_relationship: false, target_type: None },
                BaseFieldDef { name: "uri".to_string(), column: "uri".to_string(), field_type: BaseFieldType::String, is_relationship: false, target_type: None },
                BaseFieldDef { name: "filemime".to_string(), column: "filemime".to_string(), field_type: BaseFieldType::String, is_relationship: false, target_type: None },
                BaseFieldDef { name: "filesize".to_string(), column: "filesize".to_string(), field_type: BaseFieldType::Int, is_relationship: false, target_type: None },
                BaseFieldDef { name: "status".to_string(), column: "status".to_string(), field_type: BaseFieldType::Bool, is_relationship: false, target_type: None },
                BaseFieldDef { name: "created".to_string(), column: "created".to_string(), field_type: BaseFieldType::Timestamp, is_relationship: false, target_type: None },
                BaseFieldDef { name: "changed".to_string(), column: "changed".to_string(), field_type: BaseFieldType::Timestamp, is_relationship: false, target_type: None },
                BaseFieldDef { name: "uid".to_string(), column: "uid".to_string(), field_type: BaseFieldType::Int, is_relationship: true, target_type: Some("user".to_string()) },
            ],
        },
    );

    // Block content
    map.insert(
        "block_content".to_string(),
        EntityTypeInfo {
            entity_type: "block_content".to_string(),
            base_table: "block_content".to_string(),
            data_table: Some("block_content_field_data".to_string()),
            revision_table: Some("block_content_revision".to_string()),
            id_column: "id".to_string(),
            revision_id_column: Some("revision_id".to_string()),
            uuid_column: "uuid".to_string(),
            bundle_column: Some("type".to_string()),
            label_column: Some("info".to_string()),
            base_fields: vec![
                BaseFieldDef { name: "id".to_string(), column: "id".to_string(), field_type: BaseFieldType::Int, is_relationship: false, target_type: None },
                BaseFieldDef { name: "revision_id".to_string(), column: "revision_id".to_string(), field_type: BaseFieldType::Int, is_relationship: false, target_type: None },
                BaseFieldDef { name: "langcode".to_string(), column: "langcode".to_string(), field_type: BaseFieldType::Langcode, is_relationship: false, target_type: None },
                BaseFieldDef { name: "info".to_string(), column: "info".to_string(), field_type: BaseFieldType::String, is_relationship: false, target_type: None },
                BaseFieldDef { name: "status".to_string(), column: "status".to_string(), field_type: BaseFieldType::Bool, is_relationship: false, target_type: None },
                BaseFieldDef { name: "changed".to_string(), column: "changed".to_string(), field_type: BaseFieldType::Timestamp, is_relationship: false, target_type: None },
                BaseFieldDef { name: "default_langcode".to_string(), column: "default_langcode".to_string(), field_type: BaseFieldType::Bool, is_relationship: false, target_type: None },
                BaseFieldDef { name: "revision_translation_affected".to_string(), column: "revision_translation_affected".to_string(), field_type: BaseFieldType::Bool, is_relationship: false, target_type: None },
            ],
        },
    );

    // Comment
    map.insert(
        "comment".to_string(),
        EntityTypeInfo {
            entity_type: "comment".to_string(),
            base_table: "comment".to_string(),
            data_table: Some("comment_field_data".to_string()),
            revision_table: None,
            id_column: "cid".to_string(),
            revision_id_column: None,
            uuid_column: "uuid".to_string(),
            bundle_column: Some("comment_type".to_string()),
            label_column: Some("subject".to_string()),
            base_fields: vec![
                BaseFieldDef { name: "cid".to_string(), column: "cid".to_string(), field_type: BaseFieldType::Int, is_relationship: false, target_type: None },
                BaseFieldDef { name: "langcode".to_string(), column: "langcode".to_string(), field_type: BaseFieldType::Langcode, is_relationship: false, target_type: None },
                BaseFieldDef { name: "subject".to_string(), column: "subject".to_string(), field_type: BaseFieldType::String, is_relationship: false, target_type: None },
                BaseFieldDef { name: "status".to_string(), column: "status".to_string(), field_type: BaseFieldType::Bool, is_relationship: false, target_type: None },
                BaseFieldDef { name: "created".to_string(), column: "created".to_string(), field_type: BaseFieldType::Timestamp, is_relationship: false, target_type: None },
                BaseFieldDef { name: "changed".to_string(), column: "changed".to_string(), field_type: BaseFieldType::Timestamp, is_relationship: false, target_type: None },
                BaseFieldDef { name: "default_langcode".to_string(), column: "default_langcode".to_string(), field_type: BaseFieldType::Bool, is_relationship: false, target_type: None },
                BaseFieldDef { name: "uid".to_string(), column: "uid".to_string(), field_type: BaseFieldType::Int, is_relationship: true, target_type: Some("user".to_string()) },
            ],
        },
    );

    map
}
