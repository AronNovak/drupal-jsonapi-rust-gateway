use std::collections::HashMap;

#[derive(Debug, Clone, Default)]
pub struct JsonApiQuery {
    pub filters: Vec<FilterCondition>,
    pub filter_groups: Vec<FilterGroup>,
    pub sorts: Vec<SortField>,
    pub page_offset: u64,
    pub page_limit: u64,
    pub include: Vec<String>,
    pub fields: HashMap<String, Vec<String>>,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct FilterCondition {
    pub path: String,
    pub value: Option<FilterValue>,
    pub operator: FilterOperator,
    pub member_of: Option<String>,
}

#[derive(Debug, Clone)]
pub enum FilterValue {
    Single(String),
    Multiple(Vec<String>),
}

#[derive(Debug, Clone, PartialEq)]
pub enum FilterOperator {
    Equal,
    NotEqual,
    GreaterThan,
    GreaterThanOrEqual,
    LessThan,
    LessThanOrEqual,
    In,
    NotIn,
    Between,
    Contains,
    StartsWith,
    EndsWith,
    IsNull,
    IsNotNull,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct FilterGroup {
    pub name: String,
    pub conjunction: Conjunction,
}

#[derive(Debug, Clone)]
pub enum Conjunction {
    And,
    Or,
}

#[derive(Debug, Clone)]
pub struct SortField {
    pub path: String,
    pub direction: SortDirection,
}

#[derive(Debug, Clone)]
pub enum SortDirection {
    Asc,
    Desc,
}

pub fn parse_query(query_string: &str, default_limit: u64, max_limit: u64) -> JsonApiQuery {
    let params = parse_query_string(query_string);
    let mut query = JsonApiQuery {
        page_limit: default_limit,
        ..Default::default()
    };

    parse_filters(&params, &mut query);
    parse_sorts(&params, &mut query);
    parse_pagination(&params, &mut query, max_limit);
    parse_include(&params, &mut query);
    parse_fields(&params, &mut query);

    query
}

fn parse_query_string(qs: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    let qs = if qs.starts_with('?') { &qs[1..] } else { qs };
    for pair in qs.split('&') {
        if pair.is_empty() {
            continue;
        }
        let mut parts = pair.splitn(2, '=');
        let key = urlencoding_decode(parts.next().unwrap_or(""));
        let value = urlencoding_decode(parts.next().unwrap_or(""));
        map.insert(key, value);
    }
    map
}

fn urlencoding_decode(s: &str) -> String {
    let s = s.replace('+', " ");
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '%' {
            let hex: String = chars.by_ref().take(2).collect();
            if let Ok(byte) = u8::from_str_radix(&hex, 16) {
                result.push(byte as char);
            }
        } else {
            result.push(c);
        }
    }
    result
}

fn parse_filters(params: &HashMap<String, String>, query: &mut JsonApiQuery) {
    // Collect all filter-related params
    let mut conditions: HashMap<String, HashMap<String, String>> = HashMap::new();
    let mut groups: HashMap<String, HashMap<String, String>> = HashMap::new();

    for (key, value) in params {
        if !key.starts_with("filter[") {
            continue;
        }

        let inner = &key[7..key.len() - 1];
        let parts: Vec<&str> = inner.split("][").collect();

        if parts.len() == 1 {
            // Simple filter: filter[status]=1
            query.filters.push(FilterCondition {
                path: parts[0].to_string(),
                value: Some(FilterValue::Single(value.clone())),
                operator: FilterOperator::Equal,
                member_of: None,
            });
        } else if parts.len() >= 3 && parts[1] == "condition" {
            conditions
                .entry(parts[0].to_string())
                .or_default()
                .insert(parts[2].to_string(), value.clone());
        } else if parts.len() >= 3 && parts[1] == "group" {
            groups
                .entry(parts[0].to_string())
                .or_default()
                .insert(parts[2].to_string(), value.clone());
        }
    }

    for (name, props) in &groups {
        let conjunction = match props.get("conjunction").map(|s| s.as_str()) {
            Some("OR") => Conjunction::Or,
            _ => Conjunction::And,
        };
        query.filter_groups.push(FilterGroup {
            name: name.clone(),
            conjunction,
        });
    }

    for (_name, props) in conditions {
        let path = match props.get("path") {
            Some(p) => p.clone(),
            None => continue,
        };
        let operator = match props.get("operator").map(|s| s.as_str()) {
            Some("<>") => FilterOperator::NotEqual,
            Some(">") => FilterOperator::GreaterThan,
            Some(">=") => FilterOperator::GreaterThanOrEqual,
            Some("<") => FilterOperator::LessThan,
            Some("<=") => FilterOperator::LessThanOrEqual,
            Some("IN") => FilterOperator::In,
            Some("NOT IN") => FilterOperator::NotIn,
            Some("BETWEEN") => FilterOperator::Between,
            Some("CONTAINS") => FilterOperator::Contains,
            Some("STARTS_WITH") => FilterOperator::StartsWith,
            Some("ENDS_WITH") => FilterOperator::EndsWith,
            Some("IS NULL") => FilterOperator::IsNull,
            Some("IS NOT NULL") => FilterOperator::IsNotNull,
            _ => FilterOperator::Equal,
        };

        let value = if operator == FilterOperator::IsNull || operator == FilterOperator::IsNotNull {
            None
        } else if operator == FilterOperator::In || operator == FilterOperator::NotIn {
            props.get("value").map(|v| {
                FilterValue::Multiple(v.split(',').map(|s| s.to_string()).collect())
            })
        } else {
            props.get("value").map(|v| FilterValue::Single(v.clone()))
        };

        let member_of = props.get("memberOf").cloned();

        query.filters.push(FilterCondition {
            path,
            value,
            operator,
            member_of,
        });
    }
}

fn parse_sorts(params: &HashMap<String, String>, query: &mut JsonApiQuery) {
    if let Some(sort_str) = params.get("sort") {
        for part in sort_str.split(',') {
            let part = part.trim();
            if part.is_empty() {
                continue;
            }
            if let Some(field) = part.strip_prefix('-') {
                query.sorts.push(SortField {
                    path: field.to_string(),
                    direction: SortDirection::Desc,
                });
            } else {
                query.sorts.push(SortField {
                    path: part.to_string(),
                    direction: SortDirection::Asc,
                });
            }
        }
    }

    // Also handle structured sort params
    let mut sort_items: HashMap<String, HashMap<String, String>> = HashMap::new();
    for (key, value) in params {
        if !key.starts_with("sort[") {
            continue;
        }
        let inner = &key[5..key.len() - 1];
        let parts: Vec<&str> = inner.split("][").collect();
        if parts.len() == 2 {
            sort_items
                .entry(parts[0].to_string())
                .or_default()
                .insert(parts[1].to_string(), value.clone());
        }
    }

    for (_name, props) in sort_items {
        if let Some(path) = props.get("path") {
            let direction = match props.get("direction").map(|s| s.as_str()) {
                Some("DESC") => SortDirection::Desc,
                _ => SortDirection::Asc,
            };
            query.sorts.push(SortField {
                path: path.clone(),
                direction,
            });
        }
    }
}

fn parse_pagination(params: &HashMap<String, String>, query: &mut JsonApiQuery, max_limit: u64) {
    if let Some(offset) = params.get("page[offset]") {
        query.page_offset = offset.parse().unwrap_or(0);
    }
    if let Some(limit) = params.get("page[limit]") {
        let l: u64 = limit.parse().unwrap_or(query.page_limit);
        query.page_limit = l.min(max_limit);
    }
}

fn parse_include(params: &HashMap<String, String>, query: &mut JsonApiQuery) {
    if let Some(include_str) = params.get("include") {
        query.include = include_str
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
    }
}

fn parse_fields(params: &HashMap<String, String>, query: &mut JsonApiQuery) {
    for (key, value) in params {
        if !key.starts_with("fields[") {
            continue;
        }
        let type_name = &key[7..key.len() - 1];
        let fields: Vec<String> = value
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        query.fields.insert(type_name.to_string(), fields);
    }
}
