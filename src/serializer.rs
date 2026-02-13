use crate::types::*;
use indexmap::IndexMap;
use serde_json::{json, Map, Value};

pub fn serialize_entity(
    entity: &EntityData,
    resource_type: &ResourceType,
    base_url: &str,
    sparse_fields: Option<&Vec<String>>,
) -> Value {
    let jsonapi_type = &resource_type.jsonapi_type;
    let mut attributes = Map::new();
    let mut relationships = Map::new();

    // Base fields - collect compound fields (e.g. description__value, description__format)
    let mut compound_parts: IndexMap<String, Map<String, Value>> = IndexMap::new();

    for bf in &resource_type.base_fields {
        if bf.is_relationship {
            let rel = serialize_base_relationship(entity, bf, resource_type, base_url);
            if should_include_field(&bf.name, sparse_fields) {
                relationships.insert(bf.name.clone(), rel);
            }
            continue;
        }

        // Check for compound field pattern (e.g. description__value)
        if let Some((parent, property)) = bf.name.split_once("__") {
            if let Some(value) = entity.base_field_values.get(&bf.name) {
                compound_parts
                    .entry(parent.to_string())
                    .or_default()
                    .insert(property.to_string(), value.clone());
            }
            continue;
        }

        if !should_include_field(&bf.name, sparse_fields) {
            continue;
        }

        let attr_name = drupal_internal_prefix(&bf.name, &resource_type.entity_type);
        if let Some(value) = entity.base_field_values.get(&bf.name) {
            attributes.insert(attr_name, value.clone());
        }
    }

    // Serialize compound fields
    for (parent, parts) in &compound_parts {
        if !should_include_field(parent, sparse_fields) {
            continue;
        }
        let all_null = parts.values().all(|v| v.is_null());
        if all_null {
            attributes.insert(parent.clone(), Value::Null);
        } else {
            let mut obj = parts.clone();
            // Add "processed" for text-like compound fields
            if let Some(val) = obj.get("value") {
                obj.insert("processed".to_string(), val.clone());
            }
            attributes.insert(parent.clone(), Value::Object(obj));
        }
    }

    // revision_timestamp / revision_created from joined data
    if let Some(v) = entity.base_field_values.get("revision_timestamp") {
        if should_include_field("revision_timestamp", sparse_fields) {
            attributes.insert("revision_timestamp".to_string(), v.clone());
        }
    }
    if let Some(v) = entity.base_field_values.get("revision_created") {
        if should_include_field("revision_created", sparse_fields) {
            attributes.insert("revision_created".to_string(), v.clone());
        }
    }

    // display_name for user entities
    if entity.entity_type == "user" && should_include_field("display_name", sparse_fields) {
        let display_name = entity.base_field_values.get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        attributes.insert("display_name".to_string(), Value::String(display_name));
    }

    // path attribute (always null/empty for now)
    if should_include_field("path", sparse_fields) {
        attributes.insert(
            "path".to_string(),
            json!({
                "alias": null,
                "pid": null,
                "langcode": entity.base_field_values.get("langcode").and_then(|v| v.as_str()).unwrap_or("en")
            }),
        );
    }

    // Configurable fields
    for fs in &resource_type.field_storages {
        if !should_include_field(&fs.field_name, sparse_fields) {
            continue;
        }

        if is_reference_field_type(&fs.field_type) {
            let rel = serialize_field_relationship(entity, fs, resource_type, base_url);
            relationships.insert(fs.field_name.clone(), rel);
        } else {
            if let Some(value) = entity.field_values.get(&fs.field_name) {
                attributes.insert(fs.field_name.clone(), value.clone());
            }
        }
    }

    let mut self_href = format!("{}/jsonapi/{}/{}/{}", base_url, entity.entity_type, entity.bundle, entity.uuid);
    if let Some(vid) = entity.revision_id {
        self_href.push_str(&format!("?resourceVersion=id%3A{}", vid));
    }

    let mut resource = json!({
        "type": jsonapi_type,
        "id": entity.uuid,
        "links": {
            "self": { "href": self_href }
        },
        "attributes": attributes,
        "relationships": relationships,
    });

    // Add working-copy link for revisionable entities
    if entity.revision_id.is_some() {
        let wc_href = format!(
            "{}/jsonapi/{}/{}/{}?resourceVersion=rel%3Aworking-copy",
            base_url, entity.entity_type, entity.bundle, entity.uuid
        );
        resource["links"]["working-copy"] = json!({ "href": wc_href });
    }

    resource
}

fn serialize_base_relationship(
    entity: &EntityData,
    bf: &BaseFieldDef,
    _resource_type: &ResourceType,
    base_url: &str,
) -> Value {
    let entity_url = format!(
        "{}/jsonapi/{}/{}/{}",
        base_url, entity.entity_type, entity.bundle, entity.uuid
    );
    let version_suffix = entity.revision_id.map(|vid| format!("?resourceVersion=id%3A{}", vid)).unwrap_or_default();

    let mut links = Map::new();
    links.insert(
        "related".to_string(),
        json!({ "href": format!("{}/{}{}", entity_url, bf.name, version_suffix) }),
    );
    links.insert(
        "self".to_string(),
        json!({ "href": format!("{}/relationships/{}{}", entity_url, bf.name, version_suffix) }),
    );

    let data = if let Some(target_type) = &bf.target_type {
        if target_type == "node_type" {
            // Config entity relationship - bundle type
            json!({
                "type": "node_type--node_type",
                "id": entity.base_field_values.get("node_type_uuid")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown"),
                "meta": {
                    "drupal_internal__target_id": entity.bundle,
                }
            })
        } else if target_type == "taxonomy_vocabulary" {
            json!({
                "type": "taxonomy_vocabulary--taxonomy_vocabulary",
                "id": entity.base_field_values.get("vid_uuid")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown"),
                "meta": {
                    "drupal_internal__target_id": entity.bundle,
                }
            })
        } else {
            // User or other entity reference
            let target_id = entity
                .base_field_values
                .get(&format!("{}_target_id", bf.name))
                .or_else(|| {
                    // For uid/revision_uid, the value is stored directly
                    entity.base_field_values.get(&bf.column)
                })
                .cloned()
                .unwrap_or(Value::Null);

            let target_uuid = entity
                .base_field_values
                .get(&format!("{}_uuid", bf.name))
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");

            let target_entity_type = target_type.as_str();
            let target_bundle = entity
                .base_field_values
                .get(&format!("{}_bundle", bf.name))
                .and_then(|v| v.as_str())
                .unwrap_or(target_entity_type);
            let target_jsonapi_type = format!("{}--{}", target_entity_type, target_bundle);

            let mut data = json!({
                "type": target_jsonapi_type,
                "id": target_uuid,
            });
            if let Some(tid) = target_id.as_i64() {
                data["meta"] = json!({
                    "drupal_internal__target_id": tid,
                });
            }
            data
        }
    } else {
        Value::Null
    };

    json!({
        "data": data,
        "links": links,
    })
}

fn serialize_field_relationship(
    entity: &EntityData,
    fs: &FieldStorageDef,
    _resource_type: &ResourceType,
    base_url: &str,
) -> Value {
    let entity_url = format!(
        "{}/jsonapi/{}/{}/{}",
        base_url, entity.entity_type, entity.bundle, entity.uuid
    );
    let version_suffix = entity.revision_id.map(|vid| format!("?resourceVersion=id%3A{}", vid)).unwrap_or_default();

    let mut links = Map::new();
    links.insert(
        "related".to_string(),
        json!({ "href": format!("{}/{}{}", entity_url, fs.field_name, version_suffix) }),
    );
    links.insert(
        "self".to_string(),
        json!({ "href": format!("{}/relationships/{}{}", entity_url, fs.field_name, version_suffix) }),
    );

    let field_value = entity.field_values.get(&fs.field_name);

    let data = match field_value {
        None | Some(Value::Null) => {
            if fs.cardinality == 1 {
                Value::Null
            } else {
                Value::Array(vec![])
            }
        }
        Some(Value::Array(items)) => {
            let refs: Vec<Value> = items
                .iter()
                .filter_map(|item| make_reference_data(item, fs))
                .collect();
            Value::Array(refs)
        }
        Some(item) => {
            if fs.cardinality == 1 {
                make_reference_data(item, fs).unwrap_or(Value::Null)
            } else {
                let refs: Vec<Value> = make_reference_data(item, fs).into_iter().collect();
                Value::Array(refs)
            }
        }
    };

    json!({
        "data": data,
        "links": links,
    })
}

fn make_reference_data(item: &Value, fs: &FieldStorageDef) -> Option<Value> {
    let target_type = fs.target_type.as_deref().unwrap_or("unknown");

    match &fs.field_type {
        t if t == "entity_reference" || t == "entity_reference_revisions" => {
            let obj = item.as_object()?;
            let target_id = obj.get("target_id")?.as_i64()?;
            let target_uuid = obj
                .get("uuid")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            let target_bundle = obj
                .get("bundle")
                .and_then(|v| v.as_str())
                .unwrap_or(target_type);

            Some(json!({
                "type": format!("{}--{}", target_type, target_bundle),
                "id": target_uuid,
                "meta": {
                    "drupal_internal__target_id": target_id,
                },
            }))
        }
        t if t == "image" || t == "file" => {
            let obj = item.as_object()?;
            let target_id = obj.get("target_id")?.as_i64()?;
            let target_uuid = obj
                .get("uuid")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            let target_bundle = obj
                .get("bundle")
                .and_then(|v| v.as_str())
                .unwrap_or(target_type);

            let mut meta = Map::new();
            meta.insert(
                "drupal_internal__target_id".to_string(),
                Value::from(target_id),
            );
            for (k, v) in obj {
                match k.as_str() {
                    "target_id" | "uuid" | "bundle" => continue,
                    _ => { meta.insert(k.clone(), v.clone()); }
                }
            }

            Some(json!({
                "type": format!("{}--{}", target_type, target_bundle),
                "id": target_uuid,
                "meta": meta,
            }))
        }
        _ => None,
    }
}

fn drupal_internal_prefix(field_name: &str, entity_type: &str) -> String {
    match field_name {
        "nid" | "vid" | "uid" | "tid" | "fid" | "mid" | "cid" | "id" => {
            match (entity_type, field_name) {
                ("node", "vid") => "drupal_internal__vid".to_string(),
                ("node", "nid") => "drupal_internal__nid".to_string(),
                ("user", "uid") => "drupal_internal__uid".to_string(),
                ("taxonomy_term", "tid") => "drupal_internal__tid".to_string(),
                ("taxonomy_term", "revision_id") => "drupal_internal__revision_id".to_string(),
                ("file", "fid") => "drupal_internal__fid".to_string(),
                ("comment", "cid") => "drupal_internal__cid".to_string(),
                ("block_content", "id") => "drupal_internal__id".to_string(),
                _ => format!("drupal_internal__{}", field_name),
            }
        }
        "revision_id" if entity_type == "taxonomy_term" => "drupal_internal__revision_id".to_string(),
        _ => field_name.to_string(),
    }
}

fn is_reference_field_type(field_type: &str) -> bool {
    matches!(
        field_type,
        "entity_reference" | "entity_reference_revisions" | "image" | "file"
    )
}

fn should_include_field(field_name: &str, sparse_fields: Option<&Vec<String>>) -> bool {
    match sparse_fields {
        None => true,
        Some(fields) => fields.iter().any(|f| f == field_name),
    }
}
