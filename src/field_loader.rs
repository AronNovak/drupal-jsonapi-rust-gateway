use crate::error::AppError;
use crate::types::*;
use indexmap::IndexMap;
use serde_json::Value;
use sqlx::mysql::MySqlPool;
use sqlx::Row;

pub async fn load_field_data(
    pool: &MySqlPool,
    resource_type: &ResourceType,
    entities: &mut [EntityData],
) -> Result<(), AppError> {
    if entities.is_empty() {
        return Ok(());
    }

    let entity_ids: Vec<i64> = entities.iter().map(|e| e.entity_id).collect();

    // Run all field storage queries concurrently
    let futures: Vec<_> = resource_type.field_storages.iter().map(|fs| {
        let table_name = format!("{}__{}", resource_type.entity_type, fs.field_name);
        let placeholders: Vec<&str> = entity_ids.iter().map(|_| "?").collect();
        let sql = format!(
            "SELECT * FROM `{}` WHERE entity_id IN ({}) AND deleted = 0 ORDER BY entity_id, delta",
            table_name,
            placeholders.join(", ")
        );
        let ids = entity_ids.clone();
        let pool = pool.clone();
        let field_name = fs.field_name.clone();
        async move {
            let mut db_query = sqlx::query(&sql);
            for id in &ids {
                db_query = db_query.bind(id);
            }
            match db_query.fetch_all(&pool).await {
                Ok(rows) => (field_name, Some(rows)),
                Err(e) => {
                    tracing::warn!("Failed to load field {} from {}: {}", field_name, table_name, e);
                    (field_name, None)
                }
            }
        }
    }).collect();

    let results = futures::future::join_all(futures).await;

    for (i, (_field_name, rows_opt)) in results.into_iter().enumerate() {
        let fs = &resource_type.field_storages[i];
        let rows = match rows_opt {
            Some(r) => r,
            None => continue,
        };

        let mut by_entity: IndexMap<i64, Vec<&sqlx::mysql::MySqlRow>> = IndexMap::new();
        for row in &rows {
            let eid: i64 = row
                .try_get::<i64, _>("entity_id")
                .or_else(|_| row.try_get::<u32, _>("entity_id").map(|v| v as i64))
                .unwrap_or(0);
            by_entity.entry(eid).or_default().push(row);
        }

        for entity in entities.iter_mut() {
            let field_value = if let Some(field_rows) = by_entity.get(&entity.entity_id) {
                build_field_value(fs, field_rows)
            } else if is_reference_field(&fs.field_type) {
                if fs.cardinality == 1 {
                    Value::Null
                } else {
                    Value::Array(vec![])
                }
            } else {
                Value::Null
            };

            entity.field_values.insert(fs.field_name.clone(), field_value);
        }
    }

    // Load comment statistics for comment fields
    load_comment_statistics(pool, resource_type, entities).await?;

    Ok(())
}

async fn load_comment_statistics(
    pool: &MySqlPool,
    resource_type: &ResourceType,
    entities: &mut [EntityData],
) -> Result<(), AppError> {
    let comment_fields: Vec<&FieldStorageDef> = resource_type
        .field_storages
        .iter()
        .filter(|fs| fs.field_type == "comment")
        .collect();

    if comment_fields.is_empty() || entities.is_empty() {
        return Ok(());
    }

    let entity_ids: Vec<i64> = entities.iter().map(|e| e.entity_id).collect();
    let placeholders: Vec<&str> = entity_ids.iter().map(|_| "?").collect();

    for cf in &comment_fields {
        let sql = format!(
            "SELECT entity_id, cid, last_comment_timestamp, last_comment_name, last_comment_uid, comment_count \
             FROM comment_entity_statistics \
             WHERE entity_type = ? AND field_name = ? AND entity_id IN ({})",
            placeholders.join(", ")
        );

        let mut db_query = sqlx::query(&sql)
            .bind(&resource_type.entity_type)
            .bind(&cf.field_name);
        for id in &entity_ids {
            db_query = db_query.bind(id);
        }

        let rows = match db_query.fetch_all(pool).await {
            Ok(rows) => rows,
            Err(e) => {
                tracing::warn!("Failed to load comment statistics for {}: {}", cf.field_name, e);
                continue;
            }
        };

        let mut stats_by_entity: IndexMap<i64, &sqlx::mysql::MySqlRow> = IndexMap::new();
        for row in &rows {
            let eid: i64 = row
                .try_get::<i64, _>("entity_id")
                .or_else(|_| row.try_get::<u32, _>("entity_id").map(|v| v as i64))
                .unwrap_or(0);
            stats_by_entity.insert(eid, row);
        }

        for entity in entities.iter_mut() {
            if let Some(stats_row) = stats_by_entity.get(&entity.entity_id) {
                if let Some(Value::Object(obj)) = entity.field_values.get_mut(&cf.field_name) {
                    let cid = stats_row.try_get::<i64, _>("cid")
                        .or_else(|_| stats_row.try_get::<i32, _>("cid").map(|v| v as i64))
                        .map(Value::from)
                        .unwrap_or(Value::from(0));
                    let last_ts = stats_row.try_get::<i64, _>("last_comment_timestamp")
                        .or_else(|_| stats_row.try_get::<i32, _>("last_comment_timestamp").map(|v| v as i64))
                        .map(Value::from)
                        .unwrap_or(Value::from(0));
                    let last_name = stats_row.try_get::<Option<String>, _>("last_comment_name")
                        .ok()
                        .flatten()
                        .map(Value::String)
                        .unwrap_or(Value::Null);
                    let last_uid = stats_row.try_get::<i64, _>("last_comment_uid")
                        .or_else(|_| stats_row.try_get::<u32, _>("last_comment_uid").map(|v| v as i64))
                        .map(Value::from)
                        .unwrap_or(Value::from(0));
                    let count = stats_row.try_get::<i64, _>("comment_count")
                        .or_else(|_| stats_row.try_get::<u32, _>("comment_count").map(|v| v as i64))
                        .map(Value::from)
                        .unwrap_or(Value::from(0));

                    obj.insert("cid".to_string(), cid);
                    obj.insert("last_comment_timestamp".to_string(), last_ts);
                    obj.insert("last_comment_name".to_string(), last_name);
                    obj.insert("last_comment_uid".to_string(), last_uid);
                    obj.insert("comment_count".to_string(), count);
                }
            }
        }
    }

    Ok(())
}

fn is_reference_field(field_type: &str) -> bool {
    matches!(
        field_type,
        "entity_reference" | "entity_reference_revisions" | "image" | "file"
    )
}

fn build_field_value(fs: &FieldStorageDef, rows: &[&sqlx::mysql::MySqlRow]) -> Value {
    let values: Vec<Value> = rows
        .iter()
        .map(|row| build_single_field_value(fs, row))
        .collect();

    if fs.cardinality == 1 {
        values.into_iter().next().unwrap_or(Value::Null)
    } else {
        Value::Array(values)
    }
}

fn build_single_field_value(fs: &FieldStorageDef, row: &sqlx::mysql::MySqlRow) -> Value {
    match fs.field_type.as_str() {
        "entity_reference" | "entity_reference_revisions" => {
            let target_id_col = format!("{}_target_id", fs.field_name);
            let target_id = row.try_get::<i64, _>(target_id_col.as_str())
                .or_else(|_| row.try_get::<u32, _>(target_id_col.as_str()).map(|v| v as i64))
                .unwrap_or(0);
            let mut obj = serde_json::Map::new();
            obj.insert("target_id".to_string(), Value::from(target_id));
            Value::Object(obj)
        }
        "image" => {
            let mut obj = serde_json::Map::new();
            let target_id_col = format!("{}_target_id", fs.field_name);
            if let Ok(tid) = row.try_get::<i64, _>(target_id_col.as_str())
                .or_else(|_| row.try_get::<u32, _>(target_id_col.as_str()).map(|v| v as i64))
            {
                obj.insert("target_id".to_string(), Value::from(tid));
            }
            for col in &fs.columns {
                if col.property == "target_id" {
                    continue;
                }
                let val = read_column_value(row, &col.column_name);
                obj.insert(col.property.clone(), val);
            }
            Value::Object(obj)
        }
        "file" => {
            let mut obj = serde_json::Map::new();
            let target_id_col = format!("{}_target_id", fs.field_name);
            if let Ok(tid) = row.try_get::<i64, _>(target_id_col.as_str())
                .or_else(|_| row.try_get::<u32, _>(target_id_col.as_str()).map(|v| v as i64))
            {
                obj.insert("target_id".to_string(), Value::from(tid));
            }
            for col in &fs.columns {
                if col.property == "target_id" {
                    continue;
                }
                let val = read_column_value(row, &col.column_name);
                obj.insert(col.property.clone(), val);
            }
            Value::Object(obj)
        }
        "text" | "text_long" | "text_with_summary" => {
            let mut obj = serde_json::Map::new();
            for col in &fs.columns {
                let val = read_column_value(row, &col.column_name);
                obj.insert(col.property.clone(), val);
            }
            // Add "processed" field (same as value for now)
            if let Some(v) = obj.get("value") {
                obj.insert("processed".to_string(), v.clone());
            }
            Value::Object(obj)
        }
        "comment" => {
            let mut obj = serde_json::Map::new();
            for col in &fs.columns {
                let val = read_column_value_typed(row, &col.column_name, &col.property);
                obj.insert(col.property.clone(), val);
            }
            Value::Object(obj)
        }
        "link" => {
            let mut obj = serde_json::Map::new();
            for col in &fs.columns {
                if col.property == "options" {
                    // Options is a serialized blob, output as empty object
                    obj.insert(col.property.clone(), Value::Object(serde_json::Map::new()));
                } else {
                    let val = read_column_value(row, &col.column_name);
                    obj.insert(col.property.clone(), val);
                }
            }
            Value::Object(obj)
        }
        "boolean" => {
            let val_col = format!("{}_value", fs.field_name);
            row.try_get::<i8, _>(val_col.as_str())
                .map(|v| Value::Bool(v != 0))
                .unwrap_or(Value::Null)
        }
        "integer" | "list_integer" => {
            let val_col = format!("{}_value", fs.field_name);
            row.try_get::<i64, _>(val_col.as_str())
                .or_else(|_| row.try_get::<i32, _>(val_col.as_str()).map(|v| v as i64))
                .map(Value::from)
                .unwrap_or(Value::Null)
        }
        "datetime" => {
            let val_col = format!("{}_value", fs.field_name);
            row.try_get::<Option<String>, _>(val_col.as_str())
                .ok()
                .flatten()
                .map(|v| {
                    // Drupal appends +00:00 timezone to datetime values
                    if v.contains('T') && !v.contains('+') && !v.ends_with('Z') {
                        Value::String(format!("{}+00:00", v))
                    } else {
                        Value::String(v)
                    }
                })
                .unwrap_or(Value::Null)
        }
        "decimal" | "float" | "list_float" => {
            let val_col = format!("{}_value", fs.field_name);
            row.try_get::<f64, _>(val_col.as_str())
                .map(|v| Value::from(v))
                .unwrap_or(Value::Null)
        }
        _ => {
            // Default: single value column
            let val_col = format!("{}_value", fs.field_name);
            read_column_value(row, &val_col)
        }
    }
}

fn read_column_value(row: &sqlx::mysql::MySqlRow, col: &str) -> Value {
    // Try string first as it's the most common
    if let Ok(v) = row.try_get::<Option<String>, _>(col) {
        return v.map(Value::String).unwrap_or(Value::Null);
    }
    if let Ok(v) = row.try_get::<Option<i64>, _>(col) {
        return v.map(Value::from).unwrap_or(Value::Null);
    }
    Value::Null
}

fn read_column_value_typed(row: &sqlx::mysql::MySqlRow, col: &str, property: &str) -> Value {
    match property {
        "status" | "cid" | "comment_count" | "last_comment_uid" => {
            row.try_get::<Option<i64>, _>(col)
                .or_else(|_| row.try_get::<Option<i32>, _>(col).map(|v| v.map(|x| x as i64)))
                .ok()
                .flatten()
                .map(Value::from)
                .unwrap_or(Value::Null)
        }
        "last_comment_timestamp" => {
            row.try_get::<Option<i64>, _>(col)
                .or_else(|_| row.try_get::<Option<i32>, _>(col).map(|v| v.map(|x| x as i64)))
                .ok()
                .flatten()
                .map(Value::from)
                .unwrap_or(Value::Null)
        }
        _ => read_column_value(row, col),
    }
}
