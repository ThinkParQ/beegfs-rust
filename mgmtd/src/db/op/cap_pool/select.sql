WITH
-- Select chosen entities ((meta, storage), (target, buddy_group)) including their free
-- (space, inodes) information (see the separate statement files)
-- In case of buddy groups, choose the lower value from both of their member targets
entities AS (
    {select_entities}
),
-- Groups up the data from above by each cap pool (calculated by the given limits) AND storage pool
-- id (0 for meta) and calculates their spread from all their targets/groups
spreads AS (
    SELECT pool_id,
        CASE
            WHEN free_space >= :space_low_limit AND free_inodes >= :inodes_low_limit THEN "normal"
            WHEN free_space >= :space_em_limit AND free_inodes >= :inodes_em_limit THEN "low"
            ELSE "emergency"
        END AS cap_pool,
        MAX(free_space) - MIN(free_space) AS space_spread,
        MAX(free_inodes) - MIN(free_inodes) AS inodes_spread
    FROM entities
    GROUP BY pool_id, cap_pool
),
-- Takes the (pool, cap pool) rows with their spreads, determines the limit to apply (normal or
-- dynamic) and merges normal and low cap rows for the same pool_id into one row
limits AS (
    SELECT DISTINCT b.pool_id,
        CASE WHEN COALESCE(n.space_spread, 0) >= :space_normal_threshold THEN :space_low_dynamic_limit ELSE :space_low_limit END AS space_low_limit,
        CASE WHEN COALESCE(l.space_spread, 0) >= :space_low_threshold THEN :space_em_dynamic_limit ELSE :space_em_limit END AS space_em_limit,
        CASE WHEN COALESCE(n.inodes_spread, 0) >= :inodes_normal_threshold THEN :inodes_low_dynamic_limit ELSE :inodes_low_limit END AS inodes_low_limit,
        CASE WHEN COALESCE(l.inodes_spread, 0) >= :inodes_low_threshold THEN :inodes_em_dynamic_limit ELSE :inodes_em_limit END inodes_em_limit
    FROM spreads AS b
    LEFT JOIN spreads AS n ON n.pool_id IS b.pool_id AND n.cap_pool = "normal"
    LEFT JOIN spreads AS l ON l.pool_id IS b.pool_id AND l.cap_pool = "low"
)
--
SELECT e.entity_id, e.node_id, e.pool_id,
CASE
    WHEN e.free_space >= l.space_low_limit AND e.free_inodes >= l.inodes_low_limit THEN "normal"
    WHEN e.free_space >= l.space_em_limit AND e.free_inodes >= l.inodes_em_limit THEN "low"
    ELSE "emergency"
END AS cap_pool
FROM entities AS e
LEFT JOIN limits AS l USING(pool_id)