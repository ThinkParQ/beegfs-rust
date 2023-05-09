SELECT st.target_id AS entity_id, st.node_id, st.pool_id, t.free_space, t.free_inodes
FROM storage_targets AS st
INNER JOIN targets AS t USING(target_uid)
WHERE st.node_id IS NOT NULL