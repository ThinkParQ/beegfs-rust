-- Basic test data for testing operations on the database. Used by the db tests.
--
-- UID numbers look like this: EETTXX
-- where
--     EE: 10 for node, 20 for target, 30 for buddy group
--     TT: 10 for meta, 20 for storage, 30 for client
--     XX: Individual part, starting from 01

INSERT INTO entities (uid, entity_type) VALUES
    (101001, "node"),
    (101002, "node"),
    (101003, "node"),
    (101004, "node"),
    (102001, "node"),
    (102002, "node"),
    (102003, "node"),
    (102004, "node"),
    (103001, "node"),
    (103002, "node"),
    (103003, "node"),
    (103004, "node")
;

INSERT INTO nodes (node_uid, alias, port, last_contact, node_type) VALUES
    (101001, "meta_node_1", 8005, DATETIME("NOW"), "meta"),
    (101002, "meta_node_2", 8005, DATETIME("NOW"), "meta"),
    (101003, "meta_node_3", 8005, DATETIME("NOW"), "meta"),
    (101004, "meta_node_4", 8005, DATETIME("NOW"), "meta"),

    (102001, "storage_node_1", 8003, DATETIME("NOW"), "storage"),
    (102002, "storage_node_2", 8003, DATETIME("NOW"), "storage"),
    (102003, "storage_node_3", 8003, DATETIME("NOW"), "storage"),
    (102004, "storage_node_4", 8003, DATETIME("NOW"), "storage"),

    (103001, "client_node_1", 8008, DATETIME("NOW"), "client"),
    (103002, "client_node_2", 8008, DATETIME("NOW"), "client"),
    (103003, "client_node_3", 8008, DATETIME("NOW"), "client"),
    (103004, "client_node_4", 8008, DATETIME("NOW"), "client")
;

INSERT INTO meta_nodes (node_id, node_uid) VALUES
    (1, 101001),
    (2, 101002),
    (3, 101003),
    (4, 101004)
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
    (101001, "ethernet", 0x00000011, "eth0"),
    (101001, "ethernet", 0x00000111, "eth1"),
    (101001, "ethernet", 0x00000211, "eth2"),
    (101001, "ethernet", 0x00000311, "eth3"),
    (101002, "ethernet", 0x00000012, "eth0"),
    (101003, "ethernet", 0x00000013, "eth0"),
    (101004, "ethernet", 0x00000014, "eth0"),
    (102001, "ethernet", 0x00000021, "eth0"),
    (102001, "ethernet", 0x00000121, "eth1"),
    (102001, "ethernet", 0x00000221, "eth2"),
    (102001, "ethernet", 0x00000321, "eth3"),
    (102002, "ethernet", 0x00000022, "eth0"),
    (102003, "ethernet", 0x00000023, "eth0"),
    (102004, "ethernet", 0x00000024, "eth0"),
    (103001, "ethernet", 0x00000031, "eth0"),
    (103001, "rdma", 0x00000031, "rdma"),
    (103002, "ethernet", 0x00000032, "eth0"),
    (103002, "rdma", 0x00000032, "rdma"),
    (103003, "ethernet", 0x00000033, "eth0"),
    (103003, "rdma", 0x00000033, "rdma"),
    (103004, "ethernet", 0x00000034, "eth0"),
    (103004, "rdma", 0x0000003F, "rdma")
;

INSERT INTO storage_pools (pool_id, alias) VALUES
    (2, "storage_pool_2"),
    (3, "storage_pool_3"),
    (4, "storage_pool_4")
;

INSERT INTO entities VALUES
    (201001, "target"),
    (201002, "target"),
    (201003, "target"),
    (201004, "target"),

    (202001, "target"),
    (202002, "target"),
    (202003, "target"),
    (202004, "target"),
    (202005, "target"),
    (202006, "target"),
    (202007, "target"),
    (202008, "target"),
    (202009, "target"),
    (202010, "target"),
    (202011, "target"),
    (202012, "target"),
    (202013, "target"),
    (202014, "target"),
    (202015, "target"),
    (202016, "target"),

    (202099, "target")
;

INSERT INTO targets (target_uid, alias, total_space, total_inodes, free_space, free_inodes,
    consistency, node_type) VALUES
    (201001, "meta_target_1", 1000000, 1000000, 200000, 200000, "good", "meta"),
    (201002, "meta_target_2", 1000000, 1000000, 400000, 400000, "good", "meta"),
    (201003, "meta_target_3", 1000000, 1000000, 600000, 600000, "good", "meta"),
    (201004, "meta_target_4", 1000000, 1000000, 800000, 800000, "good", "meta"),

    (202001, "storage_target_1", 1000000, 1000000, 500000, 500000, "good", "storage"),
    (202002, "storage_target_2", 1000000, 1000000, 500000, 500000, "good", "storage"),
    (202003, "storage_target_3", 1000000, 1000000, 500000, 500000, "good", "storage"),
    (202004, "storage_target_4", 1000000, 1000000, 500000, 500000, "good", "storage"),
    (202005, "storage_target_5", 1000000, 1000000, 500000, 500000, "good", "storage"),
    (202006, "storage_target_6", 1000000, 1000000, 500000, 500000, "good", "storage"),
    (202007, "storage_target_7", 1000000, 1000000, 500000, 500000, "good", "storage"),
    (202008, "storage_target_8", 1000000, 1000000, 500000, 500000, "good", "storage"),
    (202009, "storage_target_9", 1000000, 1000000, 500000, 500000, "good", "storage"),
    (202010, "storage_target_10", 1000000, 1000000, 500000, 500000, "good", "storage"),
    (202011, "storage_target_11", 1000000, 1000000, 500000, 500000, "good", "storage"),
    (202012, "storage_target_12", 1000000, 1000000, 500000, 500000, "good", "storage"),
    (202013, "storage_target_13", 1000000, 1000000, 500000, 500000, "good", "storage"),
    (202014, "storage_target_14", 1000000, 1000000, 500000, 500000, "good", "storage"),
    (202015, "storage_target_15", 1000000, 1000000, 500000, 500000, "good", "storage"),
    (202016, "storage_target_16", 1000000, 1000000, 500000, 500000, "good", "storage"),

    (202099, "unassigned_storage_target", NULL, NULL, NULL, NULL, "good", "storage")
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

INSERT INTO entities (uid, entity_type) VALUES
    (301001, "buddy_group"),
    (302001, "buddy_group")
;

INSERT INTO buddy_groups (buddy_group_uid, node_type) VALUES
    (301001, "meta"),
    (302001, "storage")
;

INSERT INTO meta_buddy_groups (buddy_group_id, buddy_group_uid, primary_target_id, secondary_target_id)
VALUES
    (1, 301001, 1, 2)
;

INSERT INTO storage_buddy_groups (buddy_group_id, buddy_group_uid, primary_target_id, secondary_target_id, pool_id)
VALUES
    (1, 302001, 1, 5, 1)
;

INSERT INTO root_inode (_only_one_row, target_id, buddy_group_id) VALUES
    (1, 1, NULL)
;

INSERT INTO quota_default_limits (id_type, quota_type, pool_id, value) VALUES
    ("user", "space", 1, 1000),
    ("user", "inodes", 1, 1000)
;

INSERT INTO quota_limits (quota_id, id_type, quota_type, pool_id, value) VALUES
    (1000, "user", "space", 1, 500),
    (1001, "user", "space", 1, 1500),
    (1001, "user", "inodes", 1, 1500)
;

INSERT INTO quota_entries (quota_id, id_type, quota_type, target_id, value) VALUES
    (1000, "user", "space", 1, 750),
    (1001, "user", "space", 1, 1250),
    (1000, "user", "inodes", 1, 750),
    (1001, "user", "inodes", 1, 1250)
;

INSERT INTO config (key, value) VALUES
    ("key_1", "value_1"),
    ("key_2", "value_2"),
    ("key_3", "value_3"),
    ("key_4", "value_4")
;