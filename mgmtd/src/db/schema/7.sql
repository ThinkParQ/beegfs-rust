CREATE TABLE identities (
    identity_id INTEGER PRIMARY KEY,
    name TEXT UNIQUE NOT NULL
) STRICT;
CREATE INDEX index_identities_1 ON identities(name);

CREATE TABLE identity_to_node (
    identity_id INTEGER NOT NULL
        REFERENCES identities (identity_id) ON DELETE CASCADE,
    node_type INTEGER NOT NULL
        REFERENCES node_types (node_type) ON DELETE RESTRICT,
    node_id INTEGER NOT NULL,

    PRIMARY KEY (identity_id, node_type, node_id),
    FOREIGN KEY (node_type, node_id) REFERENCES nodes (node_type, node_id) ON DELETE CASCADE
) STRICT;

CREATE TABLE keys (
    key_id INTEGER PRIMARY KEY AUTOINCREMENT,
    key BLOB NOT NULL,
    identity INTEGER NOT NULL
        REFERENCES identities (identity_id) ON DELETE CASCADE
) STRICT;
