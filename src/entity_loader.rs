use crate::error::AppError;
use crate::query::*;
use crate::types::*;
use indexmap::IndexMap;
use serde_json::Value;
use sqlx::mysql::MySqlPool;
use sqlx::Row;

struct QueryContext {
    has_revision_join: bool,
    has_term_revision_join: bool,
}

fn build_base_select(et_info: &EntityTypeInfo) -> (String, QueryContext) {
    let data_table = et_info
        .data_table
        .as_deref()
        .unwrap_or(&et_info.base_table);

    let has_revision_join = et_info.revision_table.is_some() && et_info.entity_type == "node";
    let has_term_revision_join = et_info.revision_table.is_some() && et_info.entity_type == "taxonomy_term";

    // Select only the columns we need instead of dt.*
    let mut columns: Vec<String> = Vec::new();
    // id column
    columns.push(format!("dt.`{}`", et_info.id_column));
    // revision id column
    if let Some(rev_col) = &et_info.revision_id_column {
        columns.push(format!("dt.`{}`", rev_col));
    }
    // base field columns (skip ones that come from joined revision tables)
    let revision_join_cols: &[&str] = if has_revision_join {
        &["revision_uid", "revision_timestamp", "revision_log"]
    } else if has_term_revision_join {
        &["revision_user", "revision_created"]
    } else {
        &[]
    };
    for bf in &et_info.base_fields {
        if revision_join_cols.contains(&bf.column.as_str()) {
            continue;
        }
        let col = format!("dt.`{}`", bf.column);
        if !columns.contains(&col) {
            columns.push(col);
        }
    }
    columns.push("bt.uuid".to_string());
    if has_revision_join {
        columns.push("rt.revision_timestamp".to_string());
        columns.push("rt.revision_uid".to_string());
        columns.push("rt.revision_log".to_string());
    }
    if has_term_revision_join {
        columns.push("trt.revision_created".to_string());
        columns.push("trt.revision_user".to_string());
    }

    let mut sql = format!("SELECT {}", columns.join(", "));

    sql.push_str(&format!(" FROM `{}` AS dt", data_table));
    sql.push_str(&format!(
        " JOIN `{}` AS bt ON bt.`{}` = dt.`{}`",
        et_info.base_table, et_info.id_column, et_info.id_column
    ));

    if has_revision_join {
        sql.push_str(&format!(
            " JOIN `{}` AS rt ON rt.vid = dt.vid",
            et_info.revision_table.as_ref().unwrap()
        ));
    }
    if has_term_revision_join {
        sql.push_str(&format!(
            " JOIN `{}` AS trt ON trt.revision_id = dt.revision_id",
            et_info.revision_table.as_ref().unwrap()
        ));
    }

    (sql, QueryContext { has_revision_join, has_term_revision_join })
}

pub async fn load_entity_collection(
    pool: &MySqlPool,
    resource_type: &ResourceType,
    query: &JsonApiQuery,
) -> Result<Vec<EntityData>, AppError> {
    let et_info = &resource_type.entity_type_info;
    let (mut sql, ctx) = build_base_select(et_info);

    let mut conditions = Vec::new();
    let mut bind_values: Vec<String> = Vec::new();

    if let Some(bundle_col) = &et_info.bundle_column {
        conditions.push(format!("dt.`{}` = ?", bundle_col));
        bind_values.push(resource_type.bundle.clone());
    }
    if et_info.data_table.is_some() {
        conditions.push("dt.default_langcode = 1".to_string());
    }

    for filter in &query.filters {
        let (col, table_alias) = resolve_filter_path(&filter.path, resource_type, et_info);
        match &filter.operator {
            FilterOperator::Equal => {
                if let Some(FilterValue::Single(v)) = &filter.value {
                    conditions.push(format!("{}.`{}` = ?", table_alias, col));
                    bind_values.push(v.clone());
                }
            }
            FilterOperator::NotEqual => {
                if let Some(FilterValue::Single(v)) = &filter.value {
                    conditions.push(format!("{}.`{}` <> ?", table_alias, col));
                    bind_values.push(v.clone());
                }
            }
            FilterOperator::GreaterThan => {
                if let Some(FilterValue::Single(v)) = &filter.value {
                    conditions.push(format!("{}.`{}` > ?", table_alias, col));
                    bind_values.push(v.clone());
                }
            }
            FilterOperator::GreaterThanOrEqual => {
                if let Some(FilterValue::Single(v)) = &filter.value {
                    conditions.push(format!("{}.`{}` >= ?", table_alias, col));
                    bind_values.push(v.clone());
                }
            }
            FilterOperator::LessThan => {
                if let Some(FilterValue::Single(v)) = &filter.value {
                    conditions.push(format!("{}.`{}` < ?", table_alias, col));
                    bind_values.push(v.clone());
                }
            }
            FilterOperator::LessThanOrEqual => {
                if let Some(FilterValue::Single(v)) = &filter.value {
                    conditions.push(format!("{}.`{}` <= ?", table_alias, col));
                    bind_values.push(v.clone());
                }
            }
            FilterOperator::Contains => {
                if let Some(FilterValue::Single(v)) = &filter.value {
                    conditions.push(format!("{}.`{}` LIKE ?", table_alias, col));
                    bind_values.push(format!("%{}%", v));
                }
            }
            FilterOperator::StartsWith => {
                if let Some(FilterValue::Single(v)) = &filter.value {
                    conditions.push(format!("{}.`{}` LIKE ?", table_alias, col));
                    bind_values.push(format!("{}%", v));
                }
            }
            FilterOperator::EndsWith => {
                if let Some(FilterValue::Single(v)) = &filter.value {
                    conditions.push(format!("{}.`{}` LIKE ?", table_alias, col));
                    bind_values.push(format!("%{}", v));
                }
            }
            FilterOperator::In => {
                if let Some(FilterValue::Multiple(vals)) = &filter.value {
                    let placeholders: Vec<&str> = vals.iter().map(|_| "?").collect();
                    conditions.push(format!(
                        "{}.`{}` IN ({})",
                        table_alias, col, placeholders.join(", ")
                    ));
                    bind_values.extend(vals.clone());
                }
            }
            FilterOperator::NotIn => {
                if let Some(FilterValue::Multiple(vals)) = &filter.value {
                    let placeholders: Vec<&str> = vals.iter().map(|_| "?").collect();
                    conditions.push(format!(
                        "{}.`{}` NOT IN ({})",
                        table_alias, col, placeholders.join(", ")
                    ));
                    bind_values.extend(vals.clone());
                }
            }
            FilterOperator::IsNull => {
                conditions.push(format!("{}.`{}` IS NULL", table_alias, col));
            }
            FilterOperator::IsNotNull => {
                conditions.push(format!("{}.`{}` IS NOT NULL", table_alias, col));
            }
            FilterOperator::Between => {
                if let Some(FilterValue::Multiple(vals)) = &filter.value {
                    if vals.len() == 2 {
                        conditions.push(format!("{}.`{}` BETWEEN ? AND ?", table_alias, col));
                        bind_values.push(vals[0].clone());
                        bind_values.push(vals[1].clone());
                    }
                }
            }
        }
    }

    if !conditions.is_empty() {
        sql.push_str(" WHERE ");
        sql.push_str(&conditions.join(" AND "));
    }

    if !query.sorts.is_empty() {
        let mut order_parts = Vec::new();
        for sort in &query.sorts {
            let (col, table_alias) = resolve_filter_path(&sort.path, resource_type, et_info);
            let dir = match sort.direction {
                SortDirection::Asc => "ASC",
                SortDirection::Desc => "DESC",
            };
            order_parts.push(format!("{}.`{}` {}", table_alias, col, dir));
        }
        sql.push_str(" ORDER BY ");
        sql.push_str(&order_parts.join(", "));
    } else {
        sql.push_str(&format!(" ORDER BY dt.`{}` ASC", et_info.id_column));
    }

    sql.push_str(&format!(" LIMIT {} OFFSET {}", query.page_limit, query.page_offset));

    let mut db_query = sqlx::query(&sql);
    for val in &bind_values {
        db_query = db_query.bind(val);
    }

    let rows = db_query.fetch_all(pool).await?;
    let mut entities = Vec::with_capacity(rows.len());
    for row in &rows {
        entities.push(row_to_entity_data(row, resource_type, &ctx)?);
    }

    Ok(entities)
}

pub async fn load_entity_by_uuid(
    pool: &MySqlPool,
    resource_type: &ResourceType,
    uuid: &str,
) -> Result<Option<EntityData>, AppError> {
    let et_info = &resource_type.entity_type_info;
    let (mut sql, ctx) = build_base_select(et_info);

    sql.push_str(" WHERE bt.uuid = ?");
    if et_info.data_table.is_some() {
        sql.push_str(" AND dt.default_langcode = 1");
    }
    if let Some(bundle_col) = &et_info.bundle_column {
        sql.push_str(&format!(" AND dt.`{}` = ?", bundle_col));
    }

    let mut db_query = sqlx::query(&sql).bind(uuid);
    if et_info.bundle_column.is_some() {
        db_query = db_query.bind(&resource_type.bundle);
    }

    let row = db_query.fetch_optional(pool).await?;
    match row {
        Some(row) => Ok(Some(row_to_entity_data(&row, resource_type, &ctx)?)),
        None => Ok(None),
    }
}

pub async fn load_entities_by_ids(
    pool: &MySqlPool,
    resource_type: &ResourceType,
    entity_ids: &[i64],
) -> Result<Vec<EntityData>, AppError> {
    if entity_ids.is_empty() {
        return Ok(Vec::new());
    }

    let et_info = &resource_type.entity_type_info;
    let (mut sql, ctx) = build_base_select(et_info);

    let placeholders: Vec<&str> = entity_ids.iter().map(|_| "?").collect();
    sql.push_str(&format!(
        " WHERE dt.`{}` IN ({})",
        et_info.id_column,
        placeholders.join(", ")
    ));
    if et_info.data_table.is_some() {
        sql.push_str(" AND dt.default_langcode = 1");
    }

    let mut db_query = sqlx::query(&sql);
    for id in entity_ids {
        db_query = db_query.bind(id);
    }

    let rows = db_query.fetch_all(pool).await?;
    let mut entities = Vec::with_capacity(rows.len());
    for row in &rows {
        entities.push(row_to_entity_data(row, resource_type, &ctx)?);
    }

    Ok(entities)
}

fn row_to_entity_data(
    row: &sqlx::mysql::MySqlRow,
    resource_type: &ResourceType,
    ctx: &QueryContext,
) -> Result<EntityData, AppError> {
    let et_info = &resource_type.entity_type_info;
    let uuid: String = row.try_get("uuid").unwrap_or_default();
    let id_col: &str = &et_info.id_column;
    let entity_id: i64 = row
        .try_get::<i64, _>(id_col)
        .or_else(|_| row.try_get::<u32, _>(id_col).map(|v| v as i64))
        .or_else(|_| row.try_get::<i32, _>(id_col).map(|v| v as i64))
        .unwrap_or(0);

    let revision_id: Option<i64> = et_info.revision_id_column.as_deref().and_then(|col| {
        row.try_get::<i64, _>(col)
            .or_else(|_| row.try_get::<u32, _>(col).map(|v| v as i64))
            .or_else(|_| row.try_get::<i32, _>(col).map(|v| v as i64))
            .ok()
    });

    let mut base_field_values = IndexMap::new();

    for bf in &resource_type.base_fields {
        if bf.is_relationship {
            // Skip relationships that are in revision tables (handled below)
            if (bf.name == "revision_uid" && ctx.has_revision_join)
                || (bf.name == "revision_user" && ctx.has_term_revision_join)
            {
                continue;
            }
            let value = read_base_field_value(row, bf);
            base_field_values.insert(bf.column.clone(), value);
        } else {
            let value = read_base_field_value(row, bf);
            base_field_values.insert(bf.name.clone(), value);
        }
    }

    // Revision data from joined tables
    if ctx.has_revision_join {
        if let Ok(ts) = row.try_get::<i64, _>("revision_timestamp")
            .or_else(|_| row.try_get::<i32, _>("revision_timestamp").map(|v| v as i64))
        {
            let dt = chrono::DateTime::from_timestamp(ts, 0)
                .unwrap_or_default()
                .format("%Y-%m-%dT%H:%M:%S+00:00")
                .to_string();
            base_field_values.insert("revision_timestamp".to_string(), Value::String(dt));
        }
        if let Ok(ruid) = row.try_get::<i64, _>("revision_uid")
            .or_else(|_| row.try_get::<u32, _>("revision_uid").map(|v| v as i64))
            .or_else(|_| row.try_get::<i32, _>("revision_uid").map(|v| v as i64))
        {
            base_field_values.insert("revision_uid".to_string(), Value::from(ruid));
        }
    }
    if ctx.has_term_revision_join {
        if let Ok(ts) = row.try_get::<i64, _>("revision_created")
            .or_else(|_| row.try_get::<i32, _>("revision_created").map(|v| v as i64))
        {
            let dt = chrono::DateTime::from_timestamp(ts, 0)
                .unwrap_or_default()
                .format("%Y-%m-%dT%H:%M:%S+00:00")
                .to_string();
            base_field_values.insert("revision_created".to_string(), Value::String(dt));
        }
        if let Ok(ruid) = row.try_get::<i64, _>("revision_user")
            .or_else(|_| row.try_get::<u32, _>("revision_user").map(|v| v as i64))
            .or_else(|_| row.try_get::<i32, _>("revision_user").map(|v| v as i64))
        {
            base_field_values.insert("revision_user".to_string(), Value::from(ruid));
        }
    }

    Ok(EntityData {
        entity_type: resource_type.entity_type.clone(),
        bundle: resource_type.bundle.clone(),
        uuid,
        entity_id,
        revision_id,
        base_field_values,
        field_values: IndexMap::new(),
    })
}

fn read_base_field_value(row: &sqlx::mysql::MySqlRow, bf: &BaseFieldDef) -> Value {
    let col: &str = &bf.column;
    match bf.field_type {
        BaseFieldType::Int => {
            row.try_get::<i64, _>(col)
                .or_else(|_| row.try_get::<u32, _>(col).map(|v| v as i64))
                .or_else(|_| row.try_get::<i32, _>(col).map(|v| v as i64))
                .map(Value::from)
                .unwrap_or(Value::Null)
        }
        BaseFieldType::String | BaseFieldType::Langcode => {
            row.try_get::<String, _>(col)
                .map(Value::String)
                .unwrap_or(Value::Null)
        }
        BaseFieldType::Bool => {
            row.try_get::<i8, _>(col)
                .map(|v| Value::Bool(v != 0))
                .or_else(|_| {
                    row.try_get::<Option<i8>, _>(col)
                        .map(|v| v.map(|b| Value::Bool(b != 0)).unwrap_or(Value::Null))
                })
                .unwrap_or(Value::Null)
        }
        BaseFieldType::Timestamp => {
            row.try_get::<i64, _>(col)
                .or_else(|_| row.try_get::<i32, _>(col).map(|v| v as i64))
                .map(|ts| {
                    let dt = chrono::DateTime::from_timestamp(ts, 0)
                        .unwrap_or_default()
                        .format("%Y-%m-%dT%H:%M:%S+00:00")
                        .to_string();
                    Value::String(dt)
                })
                .unwrap_or(Value::Null)
        }
    }
}

fn resolve_filter_path(
    path: &str,
    _resource_type: &ResourceType,
    et_info: &EntityTypeInfo,
) -> (String, String) {
    let field = if let Some(stripped) = path.strip_prefix("drupal_internal__") {
        stripped
    } else {
        path
    };

    for bf in &et_info.base_fields {
        if bf.name == field {
            return (bf.column.clone(), "dt".to_string());
        }
    }

    (field.to_string(), "dt".to_string())
}
