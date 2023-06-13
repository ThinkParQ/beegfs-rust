-- Basic test data for testing operations on the database. Used by the db tests.
--
-- UID numbers look like this: EETTXX
-- where
--     EE: 10 for node, 20 for target, 30 for buddy group
--     TT: 10 for meta, 20 for storage, 30 for client
--     XX: Individual part, starting from 01

INSERT INTO entities (uid, entity_type, alias) VALUES
    (101001, "node", "meta_node_1"),
    (101002, "node", "meta_node_2"),
    (101003, "node", "meta_node_3"),
    (101004, "node", "meta_node_4"),
    (101099, "node", "meta_node_no_target"),
    (102001, "node", "storage_node_1"),
    (102002, "node", "storage_node_2"),
    (102003, "node", "storage_node_3"),
    (102004, "node", "storage_node_4"),
    (103001, "node", "client_node_1"),
    (103002, "node", "client_node_2"),
    (103003, "node", "client_node_3"),
    (103004, "node", "client_node_4")
;

INSERT INTO nodes (node_uid, node_type, port, last_contact) VALUES
    (101001, "meta", 8005, DATETIME("NOW")),
    (101002, "meta", 8005, DATETIME("NOW")),
    (101003, "meta", 8005, DATETIME("NOW")),
    (101004, "meta", 8005, DATETIME("NOW")),

    (101099, "meta", 8005, DATETIME("NOW")),

    (102001, "storage", 8003, DATETIME("NOW")),
    (102002, "storage", 8003, DATETIME("NOW")),
    (102003, "storage", 8003, DATETIME("NOW")),
    (102004, "storage", 8003, DATETIME("NOW")),

    (103001, "client", 8008, DATETIME("NOW")),
    (103002, "client", 8008, DATETIME("NOW")),
    (103003, "client", 8008, DATETIME("NOW")),
    (103004, "client", 8008, DATETIME("NOW"))
;

INSERT INTO meta_nodes (node_id, node_uid) VALUES
    (1, 101001),
    (2, 101002),
    (3, 101003),
    (4, 101004),

    (99, 101099)
;

INSERT INTO storage_nodes (node_id, node_uid) VALUES
    (1, 102001),
    (2, 102002),
    (3, 102003),
    (4, 102004)
;

INSERT INTO client_nodes (node_id, node_uid) VALUES
    (1, 103001),
    (2, 103002),
    (3, 103003),
    (4, 103004)
;

INSERT INTO node_nics (node_uid, nic_type, addr, name) VALUES
    (101001, "ethernet", X'00000001', "eth0"),
    (101001, "ethernet", X'00000002', "eth1"),
    (101001, "ethernet", X'00000003', "eth2"),
    (101001, "ethernet", X'00000004', "eth3"),
    (101002, "ethernet", X'00000005', "eth0"),
    (101003, "ethernet", X'00000006', "eth0"),
    (101004, "ethernet", X'00000007', "eth0"),
    (102001, "ethernet", X'00000008', "eth0"),
    (102001, "ethernet", X'00000009', "eth1"),
    (102001, "ethernet", X'0000000A', "eth2"),
    (102001, "ethernet", X'0000000B', "eth3"),
    (102002, "ethernet", X'0000000C', "eth0"),
    (102003, "ethernet", X'0000000D', "eth0"),
    (102004, "ethernet", X'0000000E', "eth0"),
    (103001, "ethernet", X'0000000F', "eth0"),
    (103001, "rdma", X'00000010', "rdma"),
    (103002, "ethernet", X'00000011', "eth0"),
    (103002, "rdma", X'00000012', "rdma"),
    (103003, "ethernet", X'00000013', "eth0"),
    (103003, "rdma", X'00000014', "rdma"),
    (103004, "ethernet", X'00000015', "eth0"),
    (103004, "rdma", X'00000016', "rdma")
;

INSERT INTO entities (uid, entity_type, alias) VALUES
    (401002, "storage_pool", "storage_pool_2"),
    (401003, "storage_pool", "storage_pool_3"),
    (401004, "storage_pool", "storage_pool_4")
;

INSERT INTO storage_pools (pool_id, pool_uid) VALUES
    (2, 401002),
    (3, 401003),
    (4, 401004)
;

INSERT INTO entities (uid, entity_type, alias) VALUES
    (201001, "target", "meta_target_1"),
    (201002, "target", "meta_target_2"),
    (201003, "target", "meta_target_3"),
    (201004, "target", "meta_target_4"),

    (202001, "target", "storage_target_1"),
    (202002, "target", "storage_target_2"),
    (202003, "target", "storage_target_3"),
    (202004, "target", "storage_target_4"),
    (202005, "target", "storage_target_5"),
    (202006, "target", "storage_target_6"),
    (202007, "target", "storage_target_7"),
    (202008, "target", "storage_target_8"),
    (202009, "target", "storage_target_9"),
    (202010, "target", "storage_target_10"),
    (202011, "target", "storage_target_11"),
    (202012, "target", "storage_target_12"),
    (202013, "target", "storage_target_13"),
    (202014, "target", "storage_target_14"),
    (202015, "target", "storage_target_15"),
    (202016, "target", "storage_target_16"),

    (202099, "target", "storage_target_unmapped")
;

INSERT INTO targets (target_uid, node_type, total_space, total_inodes, free_space, free_inodes,
    consistency) VALUES
    (201001, "meta", 1000000, 1000000, 450000, 450000, "good"),
    (201002, "meta", 1000000, 1000000, 550000, 550000, "good"),
    (201003, "meta", 1000000, 1000000, 550000, 550000, "good"),
    (201004, "meta", 1000000, 1000000, 450000, 450000, "good"),

    (202001, "storage", 1000000, 1000000, 450000, 450000, "good"),
    (202002, "storage", 1000000, 1000000, 500000, 500000, "good"),
    (202003, "storage", 1000000, 1000000, 500000, 500000, "good"),
    (202004, "storage", 1000000, 1000000, 500000, 500000, "good"),
    (202005, "storage", 1000000, 1000000, 450000, 450000, "good"),
    (202006, "storage", 1000000, 1000000, 500000, 500000, "good"),
    (202007, "storage", 1000000, 1000000, 500000, 500000, "good"),
    (202008, "storage", 1000000, 1000000, 500000, 500000, "good"),
    (202009, "storage", 1000000, 1000000, 550000, 550000, "good"),
    (202010, "storage", 1000000, 1000000, 500000, 500000, "good"),
    (202011, "storage", 1000000, 1000000, 500000, 500000, "good"),
    (202012, "storage", 1000000, 1000000, 500000, 500000, "good"),
    (202013, "storage", 1000000, 1000000, 550000, 550000, "good"),
    (202014, "storage", 1000000, 1000000, 500000, 500000, "good"),
    (202015, "storage", 1000000, 1000000, 500000, 500000, "good"),
    (202016, "storage", 1000000, 1000000, 500000, 500000, "good"),

    (202099, "storage", NULL, NULL, NULL, NULL, "good")
;

INSERT INTO meta_targets (target_id, target_uid, node_id) VALUES
    (1, 201001, 1),
    (2, 201002, 2),
    (3, 201003, 3),
    (4, 201004, 4)
;

INSERT INTO storage_targets (target_id, target_uid, node_id, pool_id) VALUES
    (1, 202001, 1, 1),
    (2, 202002, 1, 2),
    (3, 202003, 1, 3),
    (4, 202004, 1, 4),
    (5, 202005, 2, 1),
    (6, 202006, 2, 2),
    (7, 202007, 2, 3),
    (8, 202008, 2, 4),
    (9, 202009, 3, 1),
    (10, 202010, 3, 2),
    (11, 202011, 3, 3),
    (12, 202012, 3, 4),
    (13, 202013, 4, 1),
    (14, 202014, 4, 2),
    (15, 202015, 4, 3),
    (16, 202016, 4, 4),

    (99, 202099, NULL, 1)
;

INSERT INTO entities (uid, entity_type, alias) VALUES
    (301001, "buddy_group", "meta_buddy_group_1"),
    (302001, "buddy_group", "storage_buddy_group_1"),
    (302002, "buddy_group", "storage_buddy_group_2")
;

INSERT INTO buddy_groups (buddy_group_uid, node_type) VALUES
    (301001, "meta"),
    (302001, "storage"),
    (302002, "storage")
;

INSERT INTO meta_buddy_groups (buddy_group_id, buddy_group_uid, primary_target_id, secondary_target_id)
VALUES
    (1, 301001, 1, 2)
;

INSERT INTO storage_buddy_groups (buddy_group_id, buddy_group_uid, primary_target_id, secondary_target_id, pool_id)
VALUES
    (1, 302001, 1, 5, 1),
    (2, 302002, 9, 13, 1)
;

INSERT INTO root_inode (_only_one_row, target_id, buddy_group_id) VALUES
    (1, 1, NULL)
;

INSERT INTO quota_default_limits (id_type, quota_type, pool_id, value) VALUES
    ("user", "space", 1, 1000),
    ("user", "inodes", 1, 1000),
    ("group", "space", 1, 1000),
    ("group", "inodes", 1, 1000)
;

INSERT INTO config (key, value) VALUES
    ("key_1", "value_1"),
    ("key_2", "value_2"),
    ("key_3", "value_3"),
    ("key_4", "value_4")
;