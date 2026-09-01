-- Long term BeeMsg identities and their pre-shared X25519 public keys.
--
-- An identity is a principal allowed to open an authenticated BeeMsg connection. It usually
-- corresponds to one node, but not necessarily - beegfs-ctl has no node entry. How the keys get
-- here is out of scope for now, the tables are filled by hand.
CREATE TABLE identities (
    identity_id INTEGER PRIMARY KEY,
    name TEXT UNIQUE NOT NULL
        CHECK(LENGTH(name) > 0)
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

-- An identity may hold several keys, so one can be rotated without a window in which the peer
-- cannot connect. Order by key_id for "newest".
--
-- key is UNIQUE because it is the identity selector the initiator sends in the clear: the responder
-- looks it up and that lookup is the authentication decision, so two identities sharing a key would
-- make the authenticated principal ambiguous.
--
-- Deliberately no length constraint. This column is meant to hold other credential material later
-- as well, for example a password hash for username/password authentication, which would derive an
-- ephemeral keypair for the actual messaging. Adding a constraint now would be hard to take out
-- again, so the reader decides what it can use.
CREATE TABLE keys (
    key_id INTEGER PRIMARY KEY AUTOINCREMENT,
    key BLOB NOT NULL UNIQUE,
    identity_id INTEGER NOT NULL
        REFERENCES identities (identity_id) ON DELETE CASCADE
) STRICT;
CREATE INDEX index_keys_1 ON keys(identity_id);
