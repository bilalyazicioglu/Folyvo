CREATE TABLE items (
    id            TEXT PRIMARY KEY,
    display_name  TEXT    NOT NULL,
    created_at    INTEGER NOT NULL,
    missing       INTEGER NOT NULL DEFAULT 0 CHECK (missing IN (0, 1))
);

CREATE TABLE locations (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    item_id       TEXT    NOT NULL REFERENCES items(id),
    path          TEXT    NOT NULL,
    canonical_key TEXT    NOT NULL,
    observed_at   INTEGER NOT NULL,
    is_current    INTEGER NOT NULL CHECK (is_current IN (0, 1))
);
CREATE UNIQUE INDEX one_current_location_per_item ON locations(item_id) WHERE is_current = 1;
CREATE UNIQUE INDEX one_current_item_per_path ON locations(canonical_key) WHERE is_current = 1;

CREATE TABLE objects (
    content_hash  TEXT PRIMARY KEY,
    size          INTEGER NOT NULL,
    stored_at     INTEGER NOT NULL
);

CREATE TABLE versions (
    id            TEXT PRIMARY KEY,
    item_id       TEXT    NOT NULL REFERENCES items(id),
    seq           INTEGER NOT NULL,
    content_hash  TEXT    NOT NULL REFERENCES objects(content_hash),
    size          INTEGER NOT NULL,
    created_at    INTEGER NOT NULL,
    origin        TEXT    NOT NULL CHECK (origin IN ('initial', 'auto', 'restore')),
    restored_from TEXT    REFERENCES versions(id),
    note          TEXT,
    UNIQUE (item_id, seq),
    UNIQUE (id, item_id)
);

CREATE TABLE share_events (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    item_id     TEXT    NOT NULL REFERENCES items(id),
    version_id  TEXT    NOT NULL,
    kind        TEXT    NOT NULL CHECK (kind IN ('exported', 'handed')),
    service     TEXT,
    target_path TEXT,
    target_key  TEXT,
    at          INTEGER NOT NULL,
    CHECK ((kind = 'handed') = (service IS NOT NULL)),
    FOREIGN KEY (version_id, item_id) REFERENCES versions(id, item_id)
);

CREATE INDEX versions_by_hash ON versions(content_hash);

CREATE TABLE watched_roots (
    id       INTEGER PRIMARY KEY AUTOINCREMENT,
    path     TEXT    NOT NULL UNIQUE,
    added_at INTEGER NOT NULL
);
