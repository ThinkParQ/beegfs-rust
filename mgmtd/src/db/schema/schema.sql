-- TODO: Indexes

CREATE TABLE entities (
    uid INTEGER PRIMARY KEY AUTOINCREMENT,
    entity_type TEXT NOT NULL
        CHECK(entity_type IN ("node", "target", "buddy_group", "storage_pool")),
    alias TEXT UNIQUE NOT NULL
        CHECK(LENGTH(alias) > 0),

    UNIQUE(uid, entity_type)
) STRICT;

CREATE TABLE nodes (
    node_uid INTEGER PRIMARY KEY,
    node_type TEXT NOT NULL
        CHECK (node_type IN ("meta", "storage", "client")),

    port INTEGER NOT NULL
        CHECK(port BETWEEN 0 AND 0xFFFF),
    last_contact TEXT NOT NULL,

    entity_type TEXT GENERATED ALWAYS AS ("node"),

    UNIQUE(node_uid, node_type),
    FOREIGN KEY (node_uid, entity_type) REFERENCES entities (uid, entity_type) ON DELETE CASCADE
) STRICT;

CREATE TRIGGER "Auto delete entity after node delete" AFTER DELETE ON nodes
FOR EACH ROW
BEGIN
    DELETE FROM entities WHERE uid = OLD.node_uid;
END;

CREATE TABLE meta_nodes (
    node_id INTEGER PRIMARY KEY
        CHECK(node_id BETWEEN 1 AND 0xFFFF),
    node_uid INTEGER UNIQUE NOT NULL,

    node_type TEXT GENERATED ALWAYS AS ("meta"),

    FOREIGN KEY (node_uid, node_type) REFERENCES nodes (node_uid, node_type) ON DELETE CASCADE
) STRICT;

CREATE TRIGGER "Auto delete node after meta delete" AFTER DELETE ON meta_nodes
FOR EACH ROW
BEGIN
    DELETE FROM nodes WHERE node_uid = OLD.node_uid;
END;

CREATE TABLE storage_nodes (
    node_id INTEGER PRIMARY KEY
        CHECK(node_id BETWEEN 1 AND 0xFFFF),
    node_uid INTEGER UNIQUE NOT NULL,

    node_type TEXT GENERATED ALWAYS AS ("storage"),

    FOREIGN KEY (node_uid, node_type) REFERENCES nodes (node_uid, node_type) ON DELETE CASCADE
) STRICT;

CREATE TRIGGER "Auto delete node after storage delete" AFTER DELETE ON storage_nodes
FOR EACH ROW
BEGIN
    DELETE FROM nodes WHERE node_uid = OLD.node_uid;
END;

CREATE TABLE client_nodes (
    node_id INTEGER PRIMARY KEY
        CHECK(node_id BETWEEN 1 AND 0xFFFF),
    node_uid INTEGER UNIQUE NOT NULL,

    node_type TEXT GENERATED ALWAYS AS ("client"),

    FOREIGN KEY (node_uid, node_type) REFERENCES nodes (node_uid, node_type) ON DELETE CASCADE
) STRICT;

CREATE TRIGGER "Auto delete node after client delete" AFTER DELETE ON client_nodes
FOR EACH ROW
BEGIN
    DELETE FROM nodes WHERE node_uid = OLD.node_uid;
END;

CREATE TABLE node_nics (
    nic_uid INTEGER PRIMARY KEY,
    node_uid INTEGER NOT NULL
        REFERENCES nodes (node_uid) ON DELETE CASCADE,
    nic_type TEXT NOT NULL
        CHECK(nic_type in ("ethernet", "sdp", "rdma")),
    addr BLOB NOT NULL
        CHECK(LENGTH(addr) = 4),
    name TEXT NOT NULL
) STRICT;

CREATE TABLE targets (
    target_uid INTEGER PRIMARY KEY,
    node_type TEXT NOT NULL
        CHECK (node_type IN ("meta", "storage")),

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

    entity_type TEXT GENERATED ALWAYS AS ("target"),

    UNIQUE(target_uid, node_type),
    FOREIGN KEY (target_uid, entity_type) REFERENCES entities (uid, entity_type) ON DELETE CASCADE
) STRICT;

CREATE TRIGGER "Auto delete entity after target delete" AFTER DELETE ON targets
FOR EACH ROW
BEGIN
    DELETE FROM entities WHERE uid = OLD.target_uid;
END;

CREATE TABLE meta_targets (
    target_id INTEGER PRIMARY KEY
        -- BeeGFS does technically support meta targets. Usually they actually refer to meta nodes,
        -- and respective IDs are used interchangably. Therefore, here, we them to exactly one per
        -- node and enforce it to have that same ID.
        REFERENCES meta_nodes (node_id) ON DELETE RESTRICT
        CHECK(target_id BETWEEN 1 AND 0xFFFF),
    target_uid INTEGER UNIQUE NOT NULL,

    node_id INTEGER NOT NULL
        REFERENCES meta_nodes (node_id) ON DELETE RESTRICT,

    node_type TEXT GENERATED ALWAYS AS ("meta"),

    FOREIGN KEY (target_uid, node_type) REFERENCES targets (target_uid, node_type) ON DELETE CASCADE
) STRICT;

CREATE TRIGGER "Auto delete target after meta delete" AFTER DELETE ON meta_targets
FOR EACH ROW
BEGIN
    DELETE FROM targets WHERE target_uid = OLD.target_uid;
END;

CREATE TABLE storage_targets (
    target_id INTEGER PRIMARY KEY
        CHECK(target_id BETWEEN 1 AND 0xFFFF),
    target_uid INTEGER UNIQUE NOT NULL,

    -- NULL means the target is "unmapped", meaning it is not assigned to a node
    node_id INTEGER
        REFERENCES storage_nodes (node_id) ON DELETE RESTRICT,
    pool_id INTEGER NOT NULL DEFAULT 1
        REFERENCES storage_pools (pool_id) ON DELETE RESTRICT,

    node_type TEXT GENERATED ALWAYS AS ("storage"),

    FOREIGN KEY (target_uid, node_type) REFERENCES targets (target_uid, node_type) ON DELETE CASCADE
) STRICT;

CREATE TRIGGER "Auto delete target after storage delete" AFTER DELETE ON storage_targets
FOR EACH ROW
BEGIN
    DELETE FROM targets WHERE target_uid = OLD.target_uid;
END;

CREATE TABLE storage_pools (
    pool_id INTEGER PRIMARY KEY
        CHECK(pool_id BETWEEN 1 AND 0xFFFF),
    pool_uid INTEGER UNIQUE NOT NULL,

    entity_type TEXT GENERATED ALWAYS AS ("storage_pool"),

    FOREIGN KEY (pool_uid, entity_type) REFERENCES entities (uid, entity_type) ON DELETE CASCADE
) STRICT;

CREATE TRIGGER "Auto delete entity after pool delete" AFTER DELETE ON storage_pools
FOR EACH ROW
BEGIN
    DELETE FROM entities WHERE uid = OLD.pool_uid;
END;


-- Default storage pool
INSERT INTO entities VALUES (1, "storage_pool", "storage_pool_default");
INSERT INTO storage_pools (pool_id, pool_uid) VALUES (1, 1);

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
    FOREIGN KEY (buddy_group_uid, entity_type) REFERENCES entities (uid, entity_type) ON DELETE CASCADE
) STRICT;

CREATE TRIGGER "Auto delete entity after buddy group delete" AFTER DELETE ON buddy_groups
FOR EACH ROW
BEGIN
    DELETE FROM entities WHERE uid = OLD.buddy_group_uid;
END;

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
) STRICT;

CREATE TRIGGER "Auto delete buddy group after meta delete" AFTER DELETE ON meta_buddy_groups
FOR EACH ROW
BEGIN
    DELETE FROM buddy_groups WHERE buddy_group_uid = OLD.buddy_group_uid;
END;

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
) STRICT;

CREATE TRIGGER "Auto delete buddy group after storage delete" AFTER DELETE ON storage_buddy_groups
FOR EACH ROW
BEGIN
    DELETE FROM buddy_groups WHERE buddy_group_uid = OLD.buddy_group_uid;
END;

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
) STRICT;

CREATE TABLE quota_default_limits (
    id_type TEXT NOT NULL
        CHECK(id_type IN ("user", "group")),
    quota_type TEXT NOT NULL
        CHECK(quota_type IN ("space", "inodes")),
    pool_id INTEGER NOT NULL
        REFERENCES storage_pools (pool_id) ON DELETE CASCADE,
    value INTEGER NOT NULL,

    PRIMARY KEY (id_type, quota_type, pool_id)
) STRICT;

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
) STRICT;

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
) STRICT;

CREATE TABLE config (
    key TEXT PRIMARY KEY NOT NULL,
    value TEXT NOT NULL
) STRICT, WITHOUT ROWID;
