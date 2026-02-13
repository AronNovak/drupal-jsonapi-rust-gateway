# Drupal JSON:API Gateway (Rust)

A read-only JSON:API gateway in Rust that sits in front of a Drupal 12 site. It reads directly from Drupal's MySQL database and serves spec-identical JSON:API 1.1 responses, while proxying write operations (POST/PATCH/DELETE) to Drupal's PHP backend.

## Requirements

- Rust 1.75+
- A running Drupal 12 instance with an accessible MySQL/MariaDB database

## Configuration

Copy and edit `config.toml`:

```toml
[server]
host = "0.0.0.0"
port = 3000

[database]
url = "mysql://db:db@127.0.0.1:33001/db"

[drupal]
base_url = "https://drupal.ddev.site"
```

## Usage

```bash
cargo run            # dev
cargo run --release  # optimized
```

The gateway auto-discovers entity types, bundles, and field definitions from Drupal's config table at startup.

## Supported features

- Collection and individual entity endpoints
- `?include=` for related entities
- `?filter[field]=value` with all JSON:API filter operators
- `?sort=` ascending/descending
- `?page[limit]=&page[offset]=` pagination
- Entity types: node, taxonomy_term, user, file, comment, block_content
- All core field types: text, boolean, integer, decimal, datetime, entity_reference, image, file, link, comment, list_string
