SELECT g.buddy_group_id AS entity_id, 0 AS node_id, 0 AS pool_id, MIN(t.free_space) AS free_space, MIN(t.free_inodes) AS free_inodes
FROM meta_targets AS mt
INNER JOIN targets AS t USING(target_uid)
INNER JOIN meta_buddy_groups AS g ON g.primary_target_id = mt.target_id OR g.secondary_target_id = mt.target_id
GROUP BY g.buddy_group_id
