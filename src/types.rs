use indexmap::IndexMap;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct EntityTypeInfo {
    pub entity_type: String,
    pub base_table: String,
    pub data_table: Option<String>,
    pub revision_table: Option<String>,
    pub id_column: String,
    pub revision_id_column: Option<String>,
    pub uuid_column: String,
    pub bundle_column: Option<String>,
    pub label_column: Option<String>,
    pub base_fields: Vec<BaseFieldDef>,
}

#[derive(Debug, Clone)]
pub struct BaseFieldDef {
    pub name: String,
    pub column: String,
    pub field_type: BaseFieldType,
    pub is_relationship: bool,
    pub target_type: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum BaseFieldType {
    Int,
    String,
    Bool,
    Timestamp,
    Langcode,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct FieldStorageDef {
    pub entity_type: String,
    pub field_name: String,
    pub field_type: String,
    pub cardinality: i32,
    pub target_type: Option<String>,
    pub columns: Vec<FieldColumn>,
}

#[derive(Debug, Clone)]
pub struct FieldColumn {
    pub property: String,
    pub column_name: String,
}

#[derive(Debug, Clone)]
pub struct ResourceType {
    pub entity_type: String,
    pub bundle: String,
    pub jsonapi_type: String,
    pub path: String,
    pub base_fields: Vec<BaseFieldDef>,
    pub field_storages: Vec<FieldStorageDef>,
    pub entity_type_info: Arc<EntityTypeInfo>,
}

#[allow(dead_code)]
impl ResourceType {
    pub fn is_relationship_field(&self, field_name: &str) -> bool {
        // Check base fields
        for bf in &self.base_fields {
            if bf.name == field_name {
                return bf.is_relationship;
            }
        }
        // Check field storages
        for fs in &self.field_storages {
            if fs.field_name == field_name {
                return matches!(
                    fs.field_type.as_str(),
                    "entity_reference" | "entity_reference_revisions" | "image" | "file"
                );
            }
        }
        false
    }

    pub fn get_field_storage(&self, field_name: &str) -> Option<&FieldStorageDef> {
        self.field_storages.iter().find(|f| f.field_name == field_name)
    }

    pub fn get_target_type(&self, field_name: &str) -> Option<&str> {
        for bf in &self.base_fields {
            if bf.name == field_name {
                return bf.target_type.as_deref();
            }
        }
        for fs in &self.field_storages {
            if fs.field_name == field_name {
                return fs.target_type.as_deref();
            }
        }
        None
    }
}

#[derive(Debug, Clone)]
pub struct ResourceTypeMap {
    pub by_type: HashMap<String, Arc<ResourceType>>,
    pub by_path: HashMap<String, Arc<ResourceType>>,
    pub entity_types: HashMap<String, Arc<EntityTypeInfo>>,
}

impl ResourceTypeMap {
    pub fn get_by_type(&self, jsonapi_type: &str) -> Option<&Arc<ResourceType>> {
        self.by_type.get(jsonapi_type)
    }

    pub fn get_by_path(&self, entity_type: &str, bundle: &str) -> Option<&Arc<ResourceType>> {
        let path = format!("{}/{}", entity_type, bundle);
        self.by_path.get(&path)
    }

    pub fn all_types(&self) -> impl Iterator<Item = &Arc<ResourceType>> {
        self.by_type.values()
    }
}

#[derive(Debug, Clone)]
pub struct EntityData {
    pub entity_type: String,
    pub bundle: String,
    pub uuid: String,
    pub entity_id: i64,
    pub revision_id: Option<i64>,
    pub base_field_values: IndexMap<String, Value>,
    pub field_values: IndexMap<String, Value>,
}
