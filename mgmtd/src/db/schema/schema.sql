-- TODO: Indexes

CREATE TABLE entities (
    uid INTEGER PRIMARY KEY AUTOINCREMENT
        CHECK(uid >= 0),
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
        -- Nic names tend to contain null bytes which we don't want to be in our database.
        -- This feels dirty, but I don't know any better way to check for that
        CHECK(HEX(name) NOT LIKE "%00%")
) STRICT;
CREATE INDEX index_node_nics_1 ON node_nics(node_uid);

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

CREATE TABLE quota_usage (
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


-- Views


CREATE VIEW all_nodes_v AS
    WITH all_nodes AS (
        SELECT n.*, e.alias, (STRFTIME('%s', 'now') - STRFTIME('%s', last_contact)) AS last_contact_s
        FROM nodes AS n
        INNER JOIN entities AS e ON e.uid = n.node_uid
    )
        SELECT n.*, mn.node_id
        FROM all_nodes AS n
        INNER JOIN meta_nodes AS mn USING(node_uid)
    UNION ALL
        SELECT n.*, sn.node_id
        FROM all_nodes AS n
        INNER JOIN storage_nodes AS sn USING(node_uid)
    UNION ALL
        SELECT n.*, cn.node_id
        FROM all_nodes AS n
        INNER JOIN client_nodes AS cn USING(node_uid)
;

CREATE VIEW meta_targets_v AS
    SELECT t.*, e.alias, mt.target_id, mn.node_id, NULL AS pool_id, n.node_uid,
        mgp.buddy_group_id AS primary_of,
        mgs.buddy_group_id AS secondary_of,
        (STRFTIME('%s', 'now') - STRFTIME('%s', n.last_contact)) AS last_contact_s
    FROM targets AS t
    INNER JOIN meta_targets AS mt USING(target_uid)
    INNER JOIN meta_nodes AS mn USING(node_id)
    INNER JOIN nodes AS n USING(node_uid)
    INNER JOIN entities AS e ON e.uid = t.target_uid
    LEFT JOIN meta_buddy_groups AS mgp ON mgp.primary_target_id = mt.target_id
    LEFT JOIN meta_buddy_groups AS mgs ON mgs.secondary_target_id = mt.target_id
;

CREATE VIEW storage_targets_v AS
    SELECT t.*, e.alias, st.target_id, sn.node_id, st.pool_id, n.node_uid,
        sgp.buddy_group_id AS primary_of,
        sgs.buddy_group_id AS secondary_of,
        (STRFTIME('%s', 'now') - STRFTIME('%s', n.last_contact)) AS last_contact_s
    FROM targets AS t
    INNER JOIN storage_targets AS st USING(target_uid)
    INNER JOIN storage_nodes AS sn USING(node_id)
    INNER JOIN nodes AS n USING(node_uid)
    INNER JOIN entities AS e ON e.uid = t.target_uid
    LEFT JOIN storage_buddy_groups AS sgp ON sgp.primary_target_id = st.target_id
    LEFT JOIN storage_buddy_groups AS sgs ON sgs.secondary_target_id = st.target_id
;

CREATE VIEW all_targets_v AS
    SELECT * FROM meta_targets_v
    UNION ALL
    SELECT * FROM storage_targets_v
;

CREATE VIEW all_buddy_groups_v AS
    WITH merged_groups AS (
        SELECT b.*, mb.buddy_group_id, mb.primary_target_id, mb.secondary_target_id, NULL AS pool_id
        FROM buddy_groups AS b
        INNER JOIN meta_buddy_groups AS mb USING(buddy_group_uid)
        UNION ALL
        SELECT b.*, sb.buddy_group_id, sb.primary_target_id, sb.secondary_target_id, sb.pool_id
        FROM buddy_groups AS b
        INNER JOIN storage_buddy_groups AS sb USING(buddy_group_uid)
    )
    SELECT g.*,
        pt.target_uid AS primary_target_uid, pt.node_id AS primary_node_id,
        pt.free_space AS primary_free_space, pt.free_inodes AS primary_free_inodes,
        pt.last_contact_s AS primary_last_contact_s, pt.consistency AS primary_consistency,
        st.target_uid AS secondary_target_uid, st.node_id AS secondary_node_id,
        st.free_space AS secondary_free_space, st.free_inodes AS secondary_free_inodes,
        st.last_contact_s AS secondary_last_contact_s, st.consistency AS secondary_consistency
    FROM merged_groups AS g
    INNER JOIN all_targets_v AS pt ON pt.node_type = g.node_type AND pt.target_id = g.primary_target_id
    INNER JOIN all_targets_v AS st ON st.node_type = g.node_type AND st.target_id = g.secondary_target_id
;

CREATE VIEW quota_limits_combined_v AS
    SELECT DISTINCT l.quota_id, l.id_type, l.pool_id,
        s.value AS "space_value", i.value AS "inodes_value"
        FROM quota_limits AS l
    LEFT JOIN quota_limits AS s ON s.quota_id = l.quota_id AND s.id_type = l.id_type
        AND s.pool_id = l.pool_id AND s.quota_type = "space"
    LEFT JOIN quota_limits AS i ON i.quota_id = l.quota_id AND i.id_type = l.id_type
        AND i.pool_id = l.pool_id AND i.quota_type = "inodes"
;

CREATE VIEW quota_default_limits_combined_v AS
    SELECT DISTINCT l.pool_id,
            us.value AS "user_space_value", ui.value AS "user_inodes_value",
            gs.value AS "group_space_value", gi.value AS "group_inodes_value"
    FROM quota_default_limits AS l
    LEFT JOIN quota_default_limits AS us ON us.pool_id = l.pool_id
        AND us.quota_type = "space" AND us.id_type = "user"
    LEFT JOIN quota_default_limits AS ui ON ui.pool_id = l.pool_id
        AND ui.quota_type = "inodes" AND ui.id_type = "user"
    LEFT JOIN quota_default_limits AS gs ON gs.pool_id = l.pool_id
        AND gs.quota_type = "space" AND gs.id_type = "group"
    LEFT JOIN quota_default_limits AS gi ON gi.pool_id = l.pool_id
        AND gi.quota_type = "inodes" AND gi.id_type = "group"
;

CREATE VIEW exceeded_quota_v AS
    SELECT e.quota_id, e.id_type, e.quota_type, st.pool_id, SUM(e.value) AS value_sum
    FROM quota_usage AS e
    INNER JOIN storage_targets AS st USING(target_id)
    LEFT JOIN quota_default_limits AS d USING(id_type, quota_type, pool_id)
    LEFT JOIN quota_limits AS l USING(quota_id, id_type, quota_type, pool_id)
    GROUP BY e.quota_id, e.id_type, e.quota_type, st.pool_id
    HAVING SUM(e.value) > COALESCE(l.value, d.value)
;
