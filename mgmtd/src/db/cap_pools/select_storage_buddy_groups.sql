SELECT g.buddy_group_id AS entity_id, 0 AS node_id, g.pool_id, MIN(t.free_space) AS free_space, MIN(t.free_inodes) AS free_inodes
FROM storage_targets AS st
INNER JOIN targets AS t USING(target_uid)
INNER JOIN storage_buddy_groups AS g ON g.primary_target_id = st.target_id OR g.secondary_target_id = st.target_id
WHERE st.node_id IS NOT NULL
GROUP BY g.buddy_group_id