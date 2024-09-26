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
    node_id INTEGER NOT NULL,
    entity_type INTEGER GENERATED ALWAYS AS (1)
        REFERENCES entity_types (entity_type) ON DELETE RESTRICT,

    port INTEGER NOT NULL
        CHECK(port BETWEEN 0 AND 0xFFFF),
    last_contact TEXT NOT NULL,
    machine_uuid TEXT,

    UNIQUE (node_type, node_id),
    FOREIGN KEY (node_uid, entity_type) REFERENCES entities (uid, entity_type) ON DELETE CASCADE
) STRICT;

CREATE TRIGGER auto_delete_entity_after_node AFTER DELETE ON nodes
FOR EACH ROW
BEGIN
    DELETE FROM entities WHERE uid = OLD.node_uid;
END;

-- Default / local management node
INSERT INTO entities VALUES (1, 1, "management");
INSERT INTO nodes (node_uid, node_id, node_type, port, last_contact) VALUES (1, 1, 4, 0, "");

CREATE TRIGGER keep_default_management_node BEFORE DELETE ON nodes
FOR EACH ROW WHEN OLD.node_uid == 1
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
    target_id INTEGER NOT NULL,
    entity_type INTEGER GENERATED ALWAYS AS (2)
        REFERENCES entity_types (entity_type) ON DELETE RESTRICT,

    node_id INTEGER,
    pool_id INTEGER,
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


    UNIQUE (node_type, target_id),
    FOREIGN KEY (target_uid, entity_type) REFERENCES entities (uid, entity_type) ON DELETE CASCADE,
    FOREIGN KEY (node_type, node_id) REFERENCES nodes (node_type, node_id) ON DELETE RESTRICT
    FOREIGN KEY (node_type, pool_id) REFERENCES pools (node_type, pool_id) ON DELETE RESTRICT
) STRICT;

CREATE TRIGGER auto_delete_entity_after_target AFTER DELETE ON targets
FOR EACH ROW
BEGIN
    DELETE FROM entities WHERE uid = OLD.target_uid;
END;


CREATE TABLE pools (
    pool_uid INTEGER PRIMARY KEY,
    node_type INTEGER NOT NULL
        REFERENCES node_types (node_type) ON DELETE RESTRICT,
    pool_id INTEGER
        CHECK(pool_id BETWEEN 1 AND 0xFFFF),
    entity_type INTEGER GENERATED ALWAYS AS (3)
        REFERENCES entity_types (entity_type) ON DELETE RESTRICT,

    UNIQUE (node_type, pool_id),
    FOREIGN KEY (pool_uid, entity_type) REFERENCES entities (uid, entity_type) ON DELETE CASCADE
) STRICT;

CREATE TRIGGER auto_delete_entity_after_pool AFTER DELETE ON pools
FOR EACH ROW
BEGIN
    DELETE FROM entities WHERE uid = OLD.pool_uid;
END;

-- Default storage pool
INSERT INTO entities VALUES (2, 3, "storage_pool_default");
INSERT INTO pools (pool_uid, node_type, pool_id) VALUES (2, 2, 1);

CREATE TRIGGER keep_default_storage_pool BEFORE DELETE ON pools
FOR EACH ROW WHEN OLD.pool_uid == 2
BEGIN
    SELECT RAISE (ABORT, "Deleting the default storage pool is not allowed");
END;



CREATE TABLE buddy_groups (
    group_uid INTEGER PRIMARY KEY,
    node_type INTEGER NOT NULL
        REFERENCES node_types (node_type) ON DELETE RESTRICT,
    group_id INTEGER,
    entity_type INTEGER GENERATED ALWAYS AS (4)
        REFERENCES entity_types (entity_type) ON DELETE RESTRICT,

    p_target_id INTEGER NOT NULL,
    s_target_id INTEGER NOT NULL,
    pool_id INTEGER,

    UNIQUE(node_type, group_id),
    FOREIGN KEY (group_uid, entity_type) REFERENCES entities (uid, entity_type) ON DELETE CASCADE,
    FOREIGN KEY (node_type, p_target_id) REFERENCES targets (node_type, target_id) ON DELETE RESTRICT,
    FOREIGN KEY (node_type, s_target_id) REFERENCES targets (node_type, target_id) ON DELETE RESTRICT,
    FOREIGN KEY (node_type, pool_id) REFERENCES pools (node_type, pool_id) ON DELETE RESTRICT
) STRICT;

CREATE TRIGGER auto_delete_entity_after_buddy_group AFTER DELETE ON buddy_groups
FOR EACH ROW
BEGIN
    DELETE FROM entities WHERE uid = OLD.group_uid;
END;



CREATE TABLE root_inode (
    target_id INTEGER,
    group_id INTEGER,

    _only_one_row INTEGER PRIMARY KEY DEFAULT 1
        CHECK(_only_one_row = 1),
    node_type INTEGER GENERATED ALWAYS AS (1)
        REFERENCES node_types (node_type) ON DELETE RESTRICT,

    -- Ensure that one and only one of target_id or group_id is set
    CHECK (target_id IS NOT NULL OR group_id IS NOT NULL),
    CHECK (target_id IS NULL OR group_id IS NULL),
    
    FOREIGN KEY (node_type, target_id) REFERENCES targets (node_type, target_id) ON DELETE RESTRICT,
    FOREIGN KEY (node_type, group_id) REFERENCES buddy_groups (node_type, group_id) ON DELETE RESTRICT
) STRICT;



-- Tables with a composite primary key usually benefit from a WITHOUT ROWID table if the
-- row size is small: https://www.sqlite.org/withoutrowid.html
CREATE TABLE quota_default_limits (
    id_type INTEGER NOT NULL
        REFERENCES quota_id_types (quota_id_type) ON DELETE RESTRICT,
    quota_type INTEGER NOT NULL
        REFERENCES quota_types (quota_type) ON DELETE RESTRICT,
    pool_id INTEGER NOT NULL,
    value INTEGER NOT NULL,

    node_type INTEGER GENERATED ALWAYS AS (2)
        REFERENCES node_types (node_type) ON DELETE RESTRICT,

    PRIMARY KEY (id_type, quota_type, pool_id),
    FOREIGN KEY (node_type, pool_id) REFERENCES pools (node_type, pool_id) ON DELETE CASCADE
) STRICT, WITHOUT ROWID;



CREATE TABLE quota_limits (
    quota_id INTEGER NOT NULL,
    id_type INTEGER NOT NULL
        REFERENCES quota_id_types (quota_id_type) ON DELETE RESTRICT,
    quota_type INTEGER NOT NULL
        REFERENCES quota_types (quota_type) ON DELETE RESTRICT,
    pool_id INTEGER NOT NULL,
    value INTEGER NOT NULL,

    node_type INTEGER GENERATED ALWAYS AS (2)
        REFERENCES node_types (node_type) ON DELETE RESTRICT,

    PRIMARY KEY (quota_id, id_type, quota_type, pool_id),
    FOREIGN KEY (node_type, pool_id) REFERENCES pools (node_type, pool_id) ON DELETE CASCADE
) STRICT, WITHOUT ROWID;



CREATE TABLE quota_usage (
    quota_id INTEGER NOT NULL,
    id_type INTEGER NOT NULL
        REFERENCES quota_id_types (quota_id_type) ON DELETE RESTRICT,
    quota_type INTEGER NOT NULL
        REFERENCES quota_types (quota_type) ON DELETE RESTRICT,
    target_id INTEGER NOT NULL,
    value INTEGER NOT NULL,

    node_type INTEGER GENERATED ALWAYS AS (2)
        REFERENCES node_types (node_type) ON DELETE RESTRICT,

    PRIMARY KEY (quota_id, id_type, quota_type, target_id),
    FOREIGN KEY (node_type, target_id) REFERENCES targets (node_type, target_id) ON DELETE CASCADE
) STRICT, WITHOUT ROWID;



-- Views

CREATE VIEW nodes_ext AS
    SELECT e.alias, n.*
    FROM nodes AS n
    INNER JOIN entities AS e ON e.uid = n.node_uid
;

CREATE VIEW meta_nodes AS
    SELECT * FROM nodes WHERE node_type = 1
;

CREATE VIEW storage_nodes AS
    SELECT * FROM nodes WHERE node_type = 2
;

CREATE VIEW client_nodes AS
    SELECT * FROM nodes WHERE node_type = 3
;

CREATE VIEW targets_ext AS
    SELECT e.alias, t.*, n.node_uid
    FROM targets AS t
    INNER JOIN entities AS e ON e.uid = t.target_uid
    -- Exclude targets without an assigned node
    INNER JOIN nodes AS n USING(node_type, node_id)
;

CREATE VIEW meta_targets AS
    SELECT * FROM targets WHERE node_type = 1
;

CREATE VIEW storage_targets AS
    SELECT * FROM targets WHERE node_type = 2
;

CREATE VIEW pools_ext AS
    SELECT e.alias, p.*
    FROM pools AS p
    INNER JOIN entities AS e ON e.uid = p.pool_uid
;

CREATE VIEW storage_pools AS
    SELECT * FROM pools WHERE node_type = 2
;

CREATE VIEW buddy_groups_ext AS
    SELECT
        e.alias, g.*, p.pool_uid, p_t.target_uid AS p_target_uid, s_t.target_uid AS s_target_uid
    FROM buddy_groups AS g
    INNER JOIN entities AS e ON e.uid = g.group_uid
    INNER JOIN targets AS p_t ON p_t.target_id = g.p_target_id AND p_t.node_type = g.node_type
    INNER JOIN targets AS s_t ON s_t.target_id = g.s_target_id AND s_t.node_type = g.node_type
    LEFT JOIN pools AS p USING (node_type, pool_id)
;

CREATE VIEW meta_buddy_groups AS
    SELECT * FROM buddy_groups WHERE node_type = 1
;

CREATE VIEW storage_buddy_groups AS
    SELECT * FROM buddy_groups WHERE node_type = 2
;
