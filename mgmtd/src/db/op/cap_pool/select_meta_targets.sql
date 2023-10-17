SELECT mt.target_id AS entity_id, mt.node_id, 0 AS pool_id, t.free_space, t.free_inodes
FROM meta_targets AS mt
INNER JOIN targets AS t USING(target_uid)
