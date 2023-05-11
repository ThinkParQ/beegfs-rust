-- dummy data for testing operations on the database

INSERT INTO entities VALUES
    (11, "node"),
    (12, "node"),
    (13, "node"),
    (14, "node"),
    (21, "node"),
    (22, "node"),
    (23, "node"),
    (24, "node"),
    (31, "node"),
    (32, "node"),
    (33, "node"),
    (34, "node")
;

INSERT INTO nodes VALUES
    (11, "meta_node_1", 8005, DATETIME("NOW"), "meta"),
    (12, "meta_node_2", 8005, DATETIME("NOW"), "meta"),
    (13, "meta_node_3", 8005, DATETIME("NOW"), "meta"),
    (14, "meta_node_4", 8005, DATETIME("NOW"), "meta"),

    (21, "storage_node_1", 8003, DATETIME("NOW"), "storage"),
    (22, "storage_node_2", 8003, DATETIME("NOW"), "storage"),
    (23, "storage_node_3", 8003, DATETIME("NOW"), "storage"),
    (24, "storage_node_4", 8003, DATETIME("NOW"), "storage"),

    (31, "client_node_1", 8008, DATETIME("NOW"), "client"),
    (32, "client_node_2", 8008, DATETIME("NOW"), "client"),
    (33, "client_node_3", 8008, DATETIME("NOW"), "client"),
    (34, "client_node_4", 8008, DATETIME("NOW"), "client")
;

INSERT INTO meta_nodes (node_id, node_uid) VALUES
    (1, 11),
    (2, 12),
    (3, 13),
    (4, 14)
;

INSERT INTO storage_nodes (node_id, node_uid) VALUES
    (1, 21),
    (2, 22),
    (3, 23),
    (4, 24)
;

INSERT INTO client_nodes (node_id, node_uid) VALUES
    (1, 31),
    (2, 32),
    (3, 33),
    (4, 34)
;

INSERT INTO node_nics (node_uid, nic_type, addr, name) VALUES
    (11, "ethernet", 0x00000011, "eth0"),
    (12, "ethernet", 0x00000012, "eth0"),
    (13, "ethernet", 0x00000013, "eth0"),
    (14, "ethernet", 0x00000014, "eth0"),
    (21, "ethernet", 0x00000021, "eth0"),
    (22, "ethernet", 0x00000022, "eth0"),
    (23, "ethernet", 0x00000023, "eth0"),
    (24, "ethernet", 0x00000024, "eth0"),
    (31, "ethernet", 0x00000031, "eth0"),
    (31, "ethernet", 0x00000031, "rdma"),
    (32, "ethernet", 0x00000032, "eth0"),
    (32, "ethernet", 0x00000032, "rdma"),
    (33, "ethernet", 0x00000033, "eth0"),
    (33, "ethernet", 0x00000033, "rdma"),
    (34, "ethernet", 0x00000034, "eth0"),
    (34, "ethernet", 0x0000003F, "rdma")
;

INSERT INTO storage_pools (pool_id, alias) VALUES
    (2, "storage_pool_2")
;

INSERT INTO entities VALUES
    (111, "target"),
    (112, "target"),
    (113, "target"),
    (114, "target"),

    (211, "target"),
    (212, "target"),
    (221, "target"),
    (222, "target"),
    (231, "target"),
    (241, "target"),

    (999, "target")
;

INSERT INTO targets VALUES
    (111, "meta_target_1", 100000, 100, 10000, 10, "good", "meta"),
    (112, "meta_target_2", 200000, 200, 20000, 20, "good", "meta"),
    (113, "meta_target_3", 300000, 300, 30000, 30, "good", "meta"),
    (114, "meta_target_4", 400000, 400, 40000, 40, "good", "meta"),

    (211, "storage_target_1_1", 1100000, 1100, 110000, 110, "good", "storage"),
    (212, "storage_target_1_2", 1200000, 1200, 120000, 120, "good", "storage"),
    (221, "storage_target_2_1", 2100000, 2100, 210000, 210, "good", "storage"),
    (222, "storage_target_2_2", 2200000, 2200, 220000, 220, "good", "storage"),
    (231, "storage_target_3_1", 3100000, 3100, 310000, 310, "good", "storage"),
    (241, "storage_target_4_1", 4100000, 4100, 410000, 410, "good", "storage"),

    (999, "unassigned_storage_target", 0, 0, 0, 0, "good", "storage")
;

INSERT INTO meta_targets (target_id, target_uid, node_id) VALUES
    (1, 111, 1),
    (2, 112, 2),
    (3, 113, 3),
    (4, 114, 4)
;

INSERT INTO storage_targets (target_id, target_uid, node_id, pool_id) VALUES
    (11, 211, 1, 1),
    (12, 212, 1, 2),
    (21, 221, 2, 1),
    (22, 222, 2, 2),
    (31, 231, 3, 1),
    (41, 241, 4, 2),

    (99, 999, NULL, 1)
;

INSERT INTO entities VALUES
    (1001, "buddy_group"),
    (2001, "buddy_group")
;

INSERT INTO buddy_groups VALUES
    (1001, "meta"),
    (2001, "storage")
;

INSERT INTO meta_buddy_groups (buddy_group_id, buddy_group_uid, primary_target_id, secondary_target_id)
VALUES
    (1, 1001, 3, 4)
;

INSERT INTO storage_buddy_groups (buddy_group_id, buddy_group_uid, primary_target_id, secondary_target_id, pool_id)
VALUES
    (2, 2001, 31, 41, 1)
;

INSERT INTO root_inode VALUES
    (1, 1, NULL)
;

INSERT INTO quota_default_limits VALUES
    ("user", "space", 1, 1000),
    ("group", "space", 2, 1000)
;

INSERT INTO quota_limits VALUES
    (1000, "user", "space", 1, 10000),
    (1000, "group", "space", 2, 10000)
;

INSERT INTO quota_entries VALUES
    (1000, "user", "space", 11, 5000),
    (1000, "user", "space", 12, 99000),
    (1001, "user", "space", 11, 6000),
    (1000, "group", "space", 12, 5000),
    (1001, "group", "space", 12, 12000)
;

INSERT INTO config VALUES
    ("key_1", "value_1"),
    ("key_2", "value_2"),
    ("key_3", "value_3"),
    ("key_4", "value_4")
;