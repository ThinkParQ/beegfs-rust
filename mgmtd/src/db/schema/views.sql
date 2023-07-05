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