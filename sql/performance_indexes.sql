-- Performance indexes for the Rust JSON:API gateway.
-- These complement Drupal's default indexes to optimize queries
-- the sidecar generates (bundle + filter + ORDER BY id LIMIT N).

-- Covers: filter by type + created range, sorted by nid (default sort).
-- Without this, MySQL picks a PK walk + row filter for ORDER BY nid LIMIT 50,
-- which degrades badly on selective created-range filters.
CREATE INDEX IF NOT EXISTS idx_type_lang_created_nid
  ON node_field_data (type, default_langcode, created, nid);

-- Covers: ORDER BY title with bundle filter.
CREATE INDEX IF NOT EXISTS idx_type_lang_title
  ON node_field_data (type, default_langcode, title(191));
