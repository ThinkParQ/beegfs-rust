CREATE TABLE entity_types (
    entity_type INTEGER PRIMARY KEY,
    name TEXT UNIQUE NOT NULL
) STRICT;
INSERT INTO entity_types VALUES
    (1, "node"),
    (2, "target"),
    (3, "pool"),
    (4, "buddy_group")
;

CREATE TABLE node_types (
    node_type INTEGER PRIMARY KEY,
    name TEXT UNIQUE NOT NULL
) STRICT;
INSERT INTO node_types VALUES
    (1, "meta"),
    (2, "storage"),
    (3, "client"),
    (4, "management")
;

CREATE TABLE quota_id_types (
    quota_id_type INTEGER PRIMARY KEY,
    name TEXT UNIQUE NOT NULL
) STRICT;
INSERT INTO quota_id_types VALUES
    (1, "user"),
    (2, "group")
;

CREATE TABLE quota_types (
    quota_type INTEGER PRIMARY KEY,
    name TEXT UNIQUE NOT NULL
) STRICT;
INSERT INTO quota_types VALUES
    (1, "space"),
    (2, "inode")
;

CREATE TABLE consistency_types (
    consistency_type INTEGER PRIMARY KEY,
    name TEXT UNIQUE NOT NULL
) STRICT;
INSERT INTO consistency_types VALUES
    (1, "good"),
    (2, "needs_resync"),
    (3, "bad")
;

CREATE TABLE nic_types (
    nic_type INTEGER PRIMARY KEY,
    name TEXT UNIQUE NOT NULL
) STRICT;
INSERT INTO nic_types VALUES
    (1, "ethernet"),
    (2, "rdma")
;

CREATE TABLE config (
    key TEXT PRIMARY KEY,
    value ANY NOT NULL
) STRICT;



CREATE TABLE entities (
    uid INTEGER PRIMARY KEY AUTOINCREMENT
        CHECK(uid > 0),
    entity_type INTEGER NOT NULL
        REFERENCES entity_types (entity_type) ON DELETE RESTRICT,
    alias TEXT UNIQUE NOT NULL
        CHECK(LENGTH(alias) > 0),

    UNIQUE(entity_type, uid)
) STRICT;



CREATE TABLE nodes (
    node_uid INTEGER PRIMARY KEY,
    node_type INTEGER NOT NULL
        REFERENCES node_types (node_type) ON DELETE RESTRICT,
    port INTEGER NOT NULL
        CHECK(port BETWEEN 0 AND 0xFFFF),
    last_contact TEXT NOT NULL,

    entity_type INTEGER GENERATED ALWAYS AS (1)
        REFERENCES entity_types (entity_type) ON DELETE RESTRICT,

    machine_uuid TEXT,

    -- Required to allow being referenced on a foreign key. Also creates an index on both fields.
    -- node_type being first is intended as the index can then be used for selects filtered
    -- by node_type.
    UNIQUE(node_type, node_uid),
    FOREIGN KEY (node_uid, entity_type) REFERENCES entities (uid, entity_type) ON DELETE CASCADE
) STRICT;

CREATE TRIGGER auto_delete_entity_after_node AFTER DELETE ON nodes
FOR EACH ROW
BEGIN
    DELETE FROM entities WHERE uid = OLD.node_uid;
END;



CREATE TABLE meta_nodes (
    node_id INTEGER PRIMARY KEY
        CHECK(node_id BETWEEN 1 AND 0xFFFF),
    node_uid INTEGER UNIQUE NOT NULL,

    node_type INTEGER GENERATED ALWAYS AS (1)
        REFERENCES node_types (node_type) ON DELETE RESTRICT,

    FOREIGN KEY (node_uid, node_type) REFERENCES nodes (node_uid, node_type) ON DELETE CASCADE
) STRICT;

CREATE TRIGGER auto_delete_node_after_meta AFTER DELETE ON meta_nodes
FOR EACH ROW
BEGIN
    DELETE FROM nodes WHERE node_uid = OLD.node_uid;
END;



CREATE TABLE storage_nodes (
    node_id INTEGER PRIMARY KEY
        CHECK(node_id BETWEEN 1 AND 0xFFFFFFFF),
    node_uid INTEGER UNIQUE NOT NULL,

    node_type INTEGER GENERATED ALWAYS AS (2)
        REFERENCES node_types (node_type) ON DELETE RESTRICT,

    FOREIGN KEY (node_uid, node_type) REFERENCES nodes (node_uid, node_type) ON DELETE CASCADE
) STRICT;

CREATE TRIGGER auto_delete_node_after_storage AFTER DELETE ON storage_nodes
FOR EACH ROW
BEGIN
    DELETE FROM nodes WHERE node_uid = OLD.node_uid;
END;



CREATE TABLE client_nodes (
    node_id INTEGER PRIMARY KEY
        CHECK(node_id BETWEEN 1 AND 0xFFFFFFFF),
    node_uid INTEGER UNIQUE NOT NULL,

    node_type INTEGER GENERATED ALWAYS AS (3)
        REFERENCES node_types (node_type) ON DELETE RESTRICT,

    FOREIGN KEY (node_uid, node_type) REFERENCES nodes (node_uid, node_type) ON DELETE CASCADE
) STRICT;

CREATE TRIGGER auto_delete_node_after_client AFTER DELETE ON client_nodes
FOR EACH ROW
BEGIN
    DELETE FROM nodes WHERE node_uid = OLD.node_uid;
END;



CREATE TABLE management_nodes (
    node_id INTEGER PRIMARY KEY
        CHECK(node_id BETWEEN 1 AND 0xFFFFFFFF),
    node_uid INTEGER UNIQUE NOT NULL,

    node_type INTEGER GENERATED ALWAYS AS (4)
        REFERENCES node_types (node_type) ON DELETE RESTRICT,

    FOREIGN KEY (node_uid, node_type) REFERENCES nodes (node_uid, node_type) ON DELETE CASCADE
) STRICT;

CREATE TRIGGER auto_delete_node_after_management AFTER DELETE ON management_nodes
FOR EACH ROW
BEGIN
    DELETE FROM nodes WHERE node_uid = OLD.node_uid;
END;

-- Default / local management node
INSERT INTO entities VALUES (1, 1, "management");
INSERT INTO nodes (node_uid, node_type, port, last_contact) VALUES (1, 4, 0, "");
INSERT INTO management_nodes (node_id, node_uid) VALUES (1, 1);

CREATE TRIGGER keep_default_management_node BEFORE DELETE ON management_nodes
FOR EACH ROW WHEN OLD.node_id == 1
BEGIN
    SELECT RAISE (ABORT, "Deleting the management node is not allowed");
END;



CREATE TABLE node_nics (
    node_uid INTEGER NOT NULL
        REFERENCES nodes (node_uid) ON DELETE CASCADE,
    nic_type INTEGER NOT NULL
        REFERENCES nic_types (nic_type) ON DELETE RESTRICT,
    addr BLOB NOT NULL,
    name TEXT NOT NULL
        -- Nic names tend to contain null bytes which we don't want to be in the database.
        -- This feels dirty, but I don't know any better way to check for that
        CHECK(HEX(name) NOT LIKE "%00%")
) STRICT;

CREATE INDEX index_node_nics_1 ON node_nics(node_uid);



CREATE TABLE targets (
    target_uid INTEGER PRIMARY KEY,
    node_type INTEGER NOT NULL
        REFERENCES node_types (node_type) ON DELETE RESTRICT,

    total_space INTEGER
        CHECK(total_space >= 0),
    total_inodes INTEGER
        CHECK(total_inodes >= 0),
    free_space INTEGER
        CHECK(free_space >= 0),
    free_inodes INTEGER
        CHECK(free_inodes >= 0),
    consistency INTEGER NOT NULL DEFAULT 1
        REFERENCES consistency_types (consistency_type) ON DELETE RESTRICT,

    entity_type INTEGER GENERATED ALWAYS AS (2)
        REFERENCES entity_types (entity_type) ON DELETE RESTRICT,

    UNIQUE(node_type, target_uid),
    FOREIGN KEY (target_uid, entity_type) REFERENCES entities (uid, entity_type) ON DELETE CASCADE
) STRICT;

CREATE TRIGGER auto_delete_entity_after_target AFTER DELETE ON targets
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

    node_type INTEGER GENERATED ALWAYS AS (1)
        REFERENCES node_types (node_type) ON DELETE RESTRICT,

    FOREIGN KEY (target_uid, node_type) REFERENCES targets (target_uid, node_type) ON DELETE CASCADE
) STRICT;

CREATE TRIGGER auto_delete_target_after_meta AFTER DELETE ON meta_targets
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

    node_type INTEGER GENERATED ALWAYS AS (2)
        REFERENCES node_types (node_type) ON DELETE RESTRICT,

    FOREIGN KEY (target_uid, node_type) REFERENCES targets (target_uid, node_type) ON DELETE CASCADE
) STRICT;

CREATE TRIGGER auto_delete_target_after_storage AFTER DELETE ON storage_targets
FOR EACH ROW
BEGIN
    DELETE FROM targets WHERE target_uid = OLD.target_uid;
END;



CREATE TABLE storage_pools (
    pool_id INTEGER PRIMARY KEY
        CHECK(pool_id BETWEEN 1 AND 0xFFFF),
    pool_uid INTEGER UNIQUE NOT NULL,

    entity_type INTEGER GENERATED ALWAYS AS (3)
        REFERENCES entity_types (entity_type) ON DELETE RESTRICT,

    FOREIGN KEY (pool_uid, entity_type) REFERENCES entities (uid, entity_type) ON DELETE CASCADE
) STRICT;

CREATE TRIGGER auto_delete_entity_after_pool AFTER DELETE ON storage_pools
FOR EACH ROW
BEGIN
    DELETE FROM entities WHERE uid = OLD.pool_uid;
END;

-- Default storage pool
INSERT INTO entities VALUES (2, 3, "storage_pool_default");
INSERT INTO storage_pools (pool_id, pool_uid) VALUES (1, 2);

CREATE TRIGGER keep_default_pool BEFORE DELETE ON storage_pools
FOR EACH ROW WHEN OLD.pool_id == 1
BEGIN
    SELECT RAISE (ABORT, "Deleting the default pool is not allowed");
END;



CREATE TABLE buddy_groups (
    group_uid INTEGER PRIMARY KEY,
    node_type INTEGER NOT NULL
        REFERENCES node_types (node_type) ON DELETE RESTRICT,

    entity_type INTEGER GENERATED ALWAYS AS (4)
        REFERENCES entity_types (entity_type) ON DELETE RESTRICT,

    UNIQUE(node_type, group_uid),
    FOREIGN KEY (group_uid, entity_type) REFERENCES entities (uid, entity_type) ON DELETE CASCADE
) STRICT;

CREATE TRIGGER auto_delete_entity_after_buddy_group AFTER DELETE ON buddy_groups
FOR EACH ROW
BEGIN
    DELETE FROM entities WHERE uid = OLD.group_uid;
END;



CREATE TABLE meta_buddy_groups (
    group_id INTEGER PRIMARY KEY
        CHECK(group_id BETWEEN 1 AND 0xFFFF),
    group_uid INTEGER UNIQUE NOT NULL
        REFERENCES buddy_groups (group_uid) ON DELETE CASCADE,

    p_target_id INTEGER UNIQUE NOT NULL
        REFERENCES meta_targets (target_id) ON DELETE RESTRICT,
    s_target_id INTEGER UNIQUE NOT NULL
        REFERENCES meta_targets (target_id) ON DELETE RESTRICT,

    node_type INTEGER GENERATED ALWAYS AS (1)
        REFERENCES node_types (node_type) ON DELETE RESTRICT,

    FOREIGN KEY (group_uid, node_type) REFERENCES buddy_groups (group_uid, node_type)
) STRICT;

CREATE TRIGGER auto_delete_buddy_group_after_meta AFTER DELETE ON meta_buddy_groups
FOR EACH ROW
BEGIN
    DELETE FROM buddy_groups WHERE group_uid = OLD.group_uid;
END;



CREATE TABLE storage_buddy_groups (
    group_id INTEGER PRIMARY KEY
        CHECK(group_id BETWEEN 1 AND 0xFFFF),
    group_uid INTEGER UNIQUE NOT NULL
        REFERENCES buddy_groups (group_uid) ON DELETE CASCADE,

    p_target_id INTEGER UNIQUE NOT NULL
        REFERENCES storage_targets (target_id) ON DELETE RESTRICT,
    s_target_id INTEGER UNIQUE NOT NULL
        REFERENCES storage_targets (target_id) ON DELETE RESTRICT,

    pool_id INTEGER NOT NULL
        REFERENCES storage_pools (pool_id) ON DELETE RESTRICT,

    node_type INTEGER GENERATED ALWAYS AS (2)
        REFERENCES node_types (node_type) ON DELETE RESTRICT,

    FOREIGN KEY (group_uid, node_type) REFERENCES buddy_groups (group_uid, node_type)
) STRICT;

CREATE TRIGGER auto_delete_buddy_group_after_storage AFTER DELETE ON storage_buddy_groups
FOR EACH ROW
BEGIN
    DELETE FROM buddy_groups WHERE group_uid = OLD.group_uid;
END;



CREATE TABLE root_inode (
    _only_one_row INTEGER PRIMARY KEY DEFAULT 1
        CHECK(_only_one_row = 1),

    target_id INTEGER
        REFERENCES meta_targets (target_id) ON DELETE RESTRICT,
    group_id INTEGER
        REFERENCES meta_buddy_groups (group_id) ON DELETE RESTRICT,

    -- Ensure that one and only one of target_id or group_id is set
    CHECK (target_id IS NOT NULL OR group_id IS NOT NULL),
    CHECK (target_id IS NULL OR group_id IS NULL)
) STRICT;



-- Tables with a composite primary key usually benefit from a WITHOUT ROWID table if the
-- row size is small: https://www.sqlite.org/withoutrowid.html
CREATE TABLE quota_default_limits (
    id_type INTEGER NOT NULL
        REFERENCES quota_id_types (quota_id_type) ON DELETE RESTRICT,
    quota_type INTEGER NOT NULL
        REFERENCES quota_types (quota_type) ON DELETE RESTRICT,
    pool_id INTEGER NOT NULL
        REFERENCES storage_pools (pool_id) ON DELETE CASCADE,
    value INTEGER NOT NULL,

    PRIMARY KEY (id_type, quota_type, pool_id)
) STRICT, WITHOUT ROWID;



CREATE TABLE quota_limits (
    quota_id INTEGER NOT NULL,
    id_type INTEGER NOT NULL
        REFERENCES quota_id_types (quota_id_type) ON DELETE RESTRICT,
    quota_type INTEGER NOT NULL
        REFERENCES quota_types (quota_type) ON DELETE RESTRICT,
    pool_id INTEGER NOT NULL
        REFERENCES storage_pools (pool_id) ON DELETE CASCADE,
    value INTEGER NOT NULL,

    PRIMARY KEY (quota_id, id_type, quota_type, pool_id)
) STRICT, WITHOUT ROWID;



CREATE TABLE quota_usage (
    quota_id INTEGER NOT NULL,
    id_type INTEGER NOT NULL
        REFERENCES quota_id_types (quota_id_type) ON DELETE RESTRICT,
    quota_type INTEGER NOT NULL
        REFERENCES quota_types (quota_type) ON DELETE RESTRICT,
    target_id INTEGER NOT NULL
        REFERENCES storage_targets (target_id) ON DELETE CASCADE,
    value INTEGER NOT NULL,

    PRIMARY KEY (quota_id, id_type, quota_type, target_id)
) STRICT, WITHOUT ROWID;



-- Views

CREATE VIEW all_nodes_v AS
    SELECT
        COALESCE(mn.node_id, sn.node_id, cn.node_id, man.node_id) AS node_id,
        e.alias, n.*
    FROM nodes AS n
    INNER JOIN entities AS e ON e.uid = n.node_uid

    LEFT JOIN meta_nodes AS mn ON mn.node_uid = n.node_uid
    LEFT JOIN storage_nodes AS sn ON sn.node_uid = n.node_uid
    LEFT JOIN client_nodes AS cn ON cn.node_uid = n.node_uid
    LEFT JOIN management_nodes AS man ON man.node_uid = n.node_uid

    WHERE COALESCE(mn.node_id, sn.node_id, cn.node_id, man.node_id) IS NOT NULL
;

CREATE VIEW all_targets_v AS
    SELECT
        COALESCE(mt.target_id, st.target_id) AS target_id,
        e.alias, t.*, st.pool_id,
        COALESCE(mt.node_id, st.node_id) AS node_id,
        COALESCE(mn.node_uid, sn.node_uid) AS node_uid
    FROM targets AS t
    INNER JOIN entities AS e ON e.uid = t.target_uid

    LEFT JOIN meta_targets AS mt ON mt.target_uid = t.target_uid
    LEFT JOIN meta_nodes AS mn ON mn.node_id = mt.node_id

    LEFT JOIN storage_targets AS st ON st.target_uid = t.target_uid
    LEFT JOIN storage_nodes AS sn ON sn.node_id = st.node_id

    WHERE COALESCE(mt.target_id, st.target_id) IS NOT NULL
    -- Exclude targets without an assigned node
    AND COALESCE(mt.node_id, st.node_id) IS NOT NULL
;

CREATE VIEW all_pools_v AS
    SELECT
        e.alias, p.*
    FROM storage_pools AS p
    INNER JOIN entities AS e ON e.uid = p.pool_uid
;

CREATE VIEW all_buddy_groups_v AS
    SELECT
        COALESCE(mg.group_id, sg.group_id) AS group_id,
        e.alias, g.*, sg.pool_id, sp.pool_uid,
        COALESCE(mg.p_target_id, sg.p_target_id) AS p_target_id,
        COALESCE(mg.s_target_id , sg.s_target_id ) AS s_target_id,
        COALESCE(p_mt.target_uid, p_st.target_uid) AS p_target_uid,
        COALESCE(s_mt.target_uid, s_st.target_uid) AS s_target_uid
    FROM buddy_groups AS g
    INNER JOIN entities AS e ON e.uid = g.group_uid

    LEFT JOIN meta_buddy_groups AS mg ON mg.group_uid = g.group_uid
    LEFT JOIN meta_targets AS p_mt ON p_mt.target_id = mg.p_target_id
    LEFT JOIN meta_targets AS s_mt ON s_mt.target_id = mg.s_target_id

    LEFT JOIN storage_buddy_groups AS sg ON sg.group_uid = g.group_uid
    LEFT JOIN storage_targets AS p_st ON p_st.target_id = sg.p_target_id
    LEFT JOIN storage_targets AS s_st ON s_st.target_id = sg.s_target_id

    LEFT JOIN storage_pools AS sp ON sp.pool_id = sg.pool_id

    WHERE COALESCE(mg.group_id, sg.group_id) IS NOT NULL
;
