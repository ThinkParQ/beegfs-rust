-- TODO: Indexes

CREATE TABLE entities (
    uid INTEGER PRIMARY KEY AUTOINCREMENT,
    entity_type TEXT NOT NULL
        CHECK(entity_type IN ("node", "target", "buddy_group")),

    UNIQUE(uid, entity_type)
);

CREATE TABLE nodes (
    node_uid INTEGER PRIMARY KEY,

    alias TEXT UNIQUE NOT NULL
        CHECK(LENGTH(alias) > 0),
    port INTEGER NOT NULL
        CHECK(port BETWEEN 0 AND 0xFFFF),
    last_contact TEXT NOT NULL,

    node_type TEXT NOT NULL
        CHECK (node_type IN ("meta", "storage", "client")),
    entity_type TEXT GENERATED ALWAYS AS ("node"),

    UNIQUE(node_uid, node_type),
    FOREIGN KEY (node_uid, entity_type) REFERENCES entities (uid, entity_type) ON DELETE RESTRICT
);

CREATE TABLE meta_nodes (
    node_id INTEGER PRIMARY KEY
        CHECK(node_id BETWEEN 1 AND 0xFFFF),
    node_uid INTEGER NOT NULL
        REFERENCES nodes (node_uid) ON DELETE CASCADE,

    node_type TEXT GENERATED ALWAYS AS ("meta"),
    FOREIGN KEY (node_uid, node_type) REFERENCES nodes (node_uid, node_type)
);

CREATE TABLE storage_nodes (
    node_id INTEGER PRIMARY KEY
        CHECK(node_id BETWEEN 1 AND 0xFFFF),
    node_uid INTEGER NOT NULL
        REFERENCES nodes (node_uid) ON DELETE CASCADE,

    node_type TEXT GENERATED ALWAYS AS ("storage"),
    FOREIGN KEY (node_uid, node_type) REFERENCES nodes (node_uid, node_type)
);

CREATE TABLE client_nodes (
    node_id INTEGER PRIMARY KEY
        CHECK(node_id BETWEEN 1 AND 0xFFFF),
    node_uid INTEGER NOT NULL
        REFERENCES nodes (node_uid) ON DELETE CASCADE,

    node_type TEXT GENERATED ALWAYS AS ("client"),
    FOREIGN KEY (node_uid, node_type) REFERENCES nodes (node_uid, node_type)
);

CREATE TABLE node_nics (
    nic_uid INTEGER PRIMARY KEY,
    node_uid INTEGER NOT NULL
        REFERENCES nodes (node_uid) ON DELETE CASCADE,
    nic_type TEXT NOT NULL
        CHECK(nic_type in ("ethernet", "sdp", "rdma")),
    addr BLOB NOT NULL,
    name TEXT NOT NULL
);

CREATE TABLE targets (
    target_uid INTEGER PRIMARY KEY,

    alias TEXT UNIQUE NOT NULL
        CHECK(LENGTH(alias) > 0),
    total_space INTEGER
        CHECK(total_space >= 0),
    total_inodes INTEGER
        CHECK(total_inodes >= 0),
    free_space INTEGER
        CHECK(free_space >= 0),
    free_inodes INTEGER
        CHECK(free_inodes >= 0),
    consistency TEXT NOT NULL DEFAULT "good"
        CHECK(consistency IN ("good", "needs_resync", "bad")),

    node_type TEXT NOT NULL
        CHECK (node_type IN ("meta", "storage")),
    entity_type TEXT GENERATED ALWAYS AS ("target"),

    UNIQUE(target_uid, node_type),
    FOREIGN KEY (target_uid, entity_type) REFERENCES entities (uid, entity_type) ON DELETE RESTRICT
);

CREATE TABLE meta_targets (
    target_id INTEGER PRIMARY KEY
        -- BeeGFS does technically support meta targets. Usually they actually refer to meta nodes,
        -- and respective IDs are used interchangably. Therefore, here, we them to exactly one per
        -- node and enforce it to have that same ID.
        REFERENCES meta_nodes (node_id) ON DELETE RESTRICT
        CHECK(target_id BETWEEN 1 AND 0xFFFF),
    target_uid INTEGER UNIQUE NOT NULL
        REFERENCES targets (target_uid) ON DELETE CASCADE,

    node_id INTEGER NOT NULL
        REFERENCES meta_nodes (node_id) ON DELETE RESTRICT,

    node_type TEXT GENERATED ALWAYS AS ("meta"),
    FOREIGN KEY (target_uid, node_type) REFERENCES targets (target_uid, node_type)
);

CREATE TABLE storage_targets (
    target_id INTEGER PRIMARY KEY
        CHECK(target_id BETWEEN 1 AND 0xFFFF),
    target_uid INTEGER UNIQUE NOT NULL
        REFERENCES targets (target_uid) ON DELETE CASCADE,

    -- NULL means the target is "unmapped", meaning it is not assigned to a node
    node_id INTEGER
        REFERENCES storage_nodes (node_id) ON DELETE RESTRICT,
    pool_id INTEGER NOT NULL DEFAULT 1
        REFERENCES storage_pools (pool_id) ON DELETE RESTRICT,

    node_type TEXT GENERATED ALWAYS AS ("storage"),
    FOREIGN KEY (target_uid, node_type) REFERENCES targets (target_uid, node_type)
);

CREATE TABLE storage_pools (
    pool_id INTEGER PRIMARY KEY
        CHECK(pool_id BETWEEN 1 AND 0xFFFF),
    alias TEXT NOT NULL
);

-- Default storage pool
INSERT INTO storage_pools (pool_id, alias) VALUES (1, "Default");

CREATE TRIGGER "Prevent default pool deletion" BEFORE DELETE ON storage_pools
FOR EACH ROW WHEN OLD.pool_id == 1
BEGIN
    SELECT RAISE (ABORT, "Deleting the default pool is not allowed");
END;

CREATE TABLE buddy_groups (
    buddy_group_uid INTEGER PRIMARY KEY,

    node_type TEXT NOT NULL
        CHECK(node_type IN ("meta", "storage")),
    entity_type TEXT GENERATED ALWAYS AS ("buddy_group"),

    UNIQUE(buddy_group_uid, node_type),
    FOREIGN KEY (buddy_group_uid, entity_type) REFERENCES entities (uid, entity_type) ON DELETE RESTRICT
);

CREATE TABLE meta_buddy_groups (
    buddy_group_id INTEGER PRIMARY KEY
        CHECK(buddy_group_id BETWEEN 1 AND 0xFFFF),
    buddy_group_uid INTEGER UNIQUE NOT NULL
        REFERENCES buddy_groups (buddy_group_uid) ON DELETE CASCADE,

    -- TODO add trigger to ensure uniqueness over both columns
    primary_target_id INTEGER UNIQUE NOT NULL
        REFERENCES meta_targets (target_id) ON DELETE RESTRICT,
    secondary_target_id INTEGER UNIQUE NOT NULL
        REFERENCES meta_targets (target_id) ON DELETE RESTRICT,

    node_type TEXT GENERATED ALWAYS AS ("meta"),
    FOREIGN KEY (buddy_group_uid, node_type) REFERENCES buddy_groups (buddy_group_uid, node_type)
);


CREATE TABLE storage_buddy_groups (
    buddy_group_id INTEGER PRIMARY KEY
        CHECK(buddy_group_id BETWEEN 1 AND 0xFFFF),
    buddy_group_uid INTEGER UNIQUE NOT NULL
        REFERENCES buddy_groups (buddy_group_uid) ON DELETE CASCADE,

    -- TODO add trigger to ensure uniqueness over both columns
    primary_target_id INTEGER UNIQUE NOT NULL
        REFERENCES storage_targets (target_id) ON DELETE RESTRICT,
    secondary_target_id INTEGER UNIQUE NOT NULL
        REFERENCES storage_targets (target_id) ON DELETE RESTRICT,

    pool_id INTEGER NOT NULL
        REFERENCES storage_pools (pool_id) ON DELETE RESTRICT,

    node_type TEXT GENERATED ALWAYS AS ("storage"),
    FOREIGN KEY (buddy_group_uid, node_type) REFERENCES buddy_groups (buddy_group_uid, node_type)
);

CREATE TABLE root_inode (
    _only_one_row INTEGER PRIMARY KEY DEFAULT 1
        CHECK(_only_one_row = 1),

    target_id INTEGER
        REFERENCES meta_targets (target_id) ON DELETE RESTRICT,
    buddy_group_id INTEGER
        REFERENCES meta_buddy_groups (buddy_group_id) ON DELETE RESTRICT,

    -- Ensure that one and only one of target_id or buddy_group_id is set
    CHECK (target_id IS NOT NULL OR buddy_group_id IS NOT NULL),
    CHECK (target_id IS NULL OR buddy_group_id IS NULL)
);

CREATE TABLE quota_default_limits (
    id_type TEXT NOT NULL
        CHECK(id_type IN ("user", "group")),
    quota_type TEXT NOT NULL
        CHECK(quota_type IN ("space", "inodes")),
    pool_id INTEGER NOT NULL
        REFERENCES storage_pools (pool_id) ON DELETE CASCADE,
    value INTEGER NOT NULL,

    PRIMARY KEY (id_type, quota_type, pool_id)
);

CREATE TABLE quota_limits (
    quota_id INTEGER NOT NULL,
    id_type TEXT NOT NULL
        CHECK(id_type IN ("user", "group")),
    quota_type TEXT NOT NULL
        CHECK(quota_type IN ("space", "inodes")),
    pool_id INTEGER NOT NULL
        REFERENCES storage_pools (pool_id) ON DELETE CASCADE,
    value INTEGER NOT NULL,

    PRIMARY KEY (quota_id, id_type, quota_type, pool_id)
);

CREATE TABLE quota_entries (
    quota_id INTEGER NOT NULL,
    id_type TEXT NOT NULL
        CHECK(id_type IN ("user", "group")),
    quota_type TEXT NOT NULL
        CHECK(quota_type IN ("space", "inodes")),
    target_id INTEGER NOT NULL
        REFERENCES storage_targets (target_id) ON DELETE CASCADE,
    value INTEGER NOT NULL,

    PRIMARY KEY (quota_id, id_type, quota_type, target_id)
);

CREATE TABLE config (
    key TEXT PRIMARY KEY NOT NULL,
    value TEXT NOT NULL
) WITHOUT ROWID;
