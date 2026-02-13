use serde_json::{json, Value};

pub fn build_collection_document(
    data: Vec<Value>,
    included: Vec<Value>,
    self_url: &str,
    next_url: Option<&str>,
) -> Value {
    let mut doc = json!({
        "jsonapi": {
            "version": "1.1",
            "meta": {
                "links": {
                    "self": {
                        "href": "http://jsonapi.org/format/1.1/"
                    }
                }
            }
        },
        "data": data,
    });

    if !included.is_empty() {
        doc["included"] = Value::Array(included);
    }

    let mut links = serde_json::Map::new();
    links.insert("self".to_string(), json!({ "href": self_url }));
    if let Some(next) = next_url {
        links.insert("next".to_string(), json!({ "href": next }));
    }
    doc["links"] = Value::Object(links);

    doc
}

pub fn build_individual_document(
    data: Value,
    included: Vec<Value>,
    self_url: &str,
) -> Value {
    let mut doc = json!({
        "jsonapi": {
            "version": "1.1",
            "meta": {
                "links": {
                    "self": {
                        "href": "http://jsonapi.org/format/1.1/"
                    }
                }
            }
        },
        "data": data,
    });

    if !included.is_empty() {
        doc["included"] = Value::Array(included);
    }

    doc["links"] = json!({ "self": { "href": self_url } });

    doc
}

pub fn build_entrypoint_document(
    resource_links: Vec<(String, String)>,
) -> Value {
    let mut links = serde_json::Map::new();
    for (type_name, href) in resource_links {
        links.insert(type_name, json!({ "href": href }));
    }

    json!({
        "jsonapi": {
            "version": "1.1",
            "meta": {
                "links": {
                    "self": {
                        "href": "http://jsonapi.org/format/1.1/"
                    }
                }
            }
        },
        "data": [],
        "links": links,
    })
}
