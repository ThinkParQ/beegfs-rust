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

-- Long-term public keys used to authenticate BeeMsg connections.
--
-- `key` is a raw 32 byte X25519 public key. It is UNIQUE because the key itself identifies the peer
-- in the key exchange: the initiator sends its public key and the responder looks it up here, so one
-- key shared by two identities would make the authenticated identity ambiguous. The length CHECK
-- catches malformed keys at insert time, which matters while the list is maintained by hand.
CREATE TABLE keys (
    key_id INTEGER PRIMARY KEY AUTOINCREMENT,
    key BLOB NOT NULL UNIQUE CHECK (length(key) = 32),
    identity_id INTEGER NOT NULL
        REFERENCES identities (identity_id) ON DELETE CASCADE
) STRICT;
