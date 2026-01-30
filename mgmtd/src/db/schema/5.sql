DROP VIEW targets_ext;

CREATE VIEW targets_ext AS
    SELECT e.alias, t.*, n.node_uid
    FROM targets AS t
    INNER JOIN entities AS e ON e.uid = t.target_uid
    LEFT JOIN nodes AS n USING(node_type, node_id)
;
