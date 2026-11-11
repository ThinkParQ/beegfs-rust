CREATE TABLE identity_tokens (
    token_id INTEGER PRIMARY KEY AUTOINCREMENT,
    secret_hash TEXT NOT NULL,
    scope TEXT NOT NULL
        CHECK(scope IN ('client', 'admin')),
    reusable INTEGER NOT NULL
        CHECK(reusable IN (0, 1)),
    comment TEXT,
    created_at TEXT NOT NULL DEFAULT (DATETIME('now')),
    used_at TEXT,
    revoked INTEGER NOT NULL DEFAULT 0
        CHECK(revoked IN (0, 1))
) STRICT;

CREATE TABLE identities (
    identity_id INTEGER PRIMARY KEY,
    name TEXT
        CHECK(LENGTH(name) > 0),
    admin INTEGER NOT NULL DEFAULT 0
        CHECK(admin IN (0, 1)),
    revoked INTEGER NOT NULL DEFAULT 0
        CHECK(revoked IN (0, 1)),
    token_id INTEGER
        REFERENCES identity_tokens (token_id) ON DELETE SET NULL
) STRICT;

CREATE TABLE pending_keys (
    key TEXT PRIMARY KEY,
    category TEXT NOT NULL
        CHECK(category IN ('client', 'server')),
    node_type INTEGER
        REFERENCES node_types (node_type) ON DELETE RESTRICT,
    node_id INTEGER,
    name TEXT,
    addr TEXT NOT NULL,
    rejected INTEGER NOT NULL DEFAULT 0
        CHECK(rejected IN (0, 1)),
    first_seen TEXT NOT NULL DEFAULT (DATETIME('now')),
    last_seen TEXT NOT NULL DEFAULT (DATETIME('now'))
) STRICT;

CREATE TABLE keys (
    key_id INTEGER PRIMARY KEY AUTOINCREMENT,
    key TEXT NOT NULL UNIQUE,
    identity_id INTEGER NOT NULL
        REFERENCES identities (identity_id) ON DELETE CASCADE
) STRICT;
CREATE INDEX index_keys_1 ON keys(identity_id);

ALTER TABLE nodes ADD COLUMN identity_id INTEGER
    REFERENCES identities (identity_id) ON DELETE SET NULL;
CREATE UNIQUE INDEX index_nodes_identity_id ON nodes(identity_id);

INSERT INTO config (key, value) VALUES ('identities_version', '0');
