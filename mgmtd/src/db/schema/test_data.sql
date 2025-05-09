-- Basic test data for testing operations on the database. Used by the db tests.
--
-- UID numbers look like this: EETTXX
-- where
--     EE: 10 for node, 20 for target, 30 for buddy group
--     TT: 10 for meta, 20 for storage, 30 for client
--     XX: Individual part, starting from 01

INSERT INTO entities (uid, entity_type, alias) VALUES
    (101001, 1, "meta_node_1"),
    (101002, 1, "meta_node_2"),
    (101003, 1, "meta_node_3"),
    (101004, 1, "meta_node_4"),
    (101099, 1, "meta_node_no_target"),
    (102001, 1, "storage_node_1"),
    (102002, 1, "storage_node_2"),
    (102003, 1, "storage_node_3"),
    (102004, 1, "storage_node_4"),
    (103001, 1, "client_node_1"),
    (103002, 1, "client_node_2"),
    (103003, 1, "client_node_3"),
    (103004, 1, "client_node_4")
;

INSERT INTO nodes (node_uid, node_id, node_type, port, last_contact) VALUES
    (101001, 1, 1, 8005, DATETIME("NOW")),
    (101002, 2, 1, 8005, DATETIME("NOW")),
    (101003, 3, 1, 8005, DATETIME("NOW")),
    (101004, 4, 1, 8005, DATETIME("NOW")),

    (101099, 99, 1, 8005, DATETIME("NOW")),

    (102001, 1, 2, 8003, DATETIME("NOW")),
    (102002, 2, 2, 8003, DATETIME("NOW")),
    (102003, 3, 2, 8003, DATETIME("NOW")),
    (102004, 4, 2, 8003, DATETIME("NOW")),

    (103001, 1, 3, 8008, DATETIME("NOW")),
    (103002, 2, 3, 8008, DATETIME("NOW")),
    (103003, 3, 3, 8008, DATETIME("NOW")),
    (103004, 4, 3, 8008, DATETIME("NOW"))
;

INSERT INTO node_nics (node_uid, nic_type, addr, name) VALUES
    (101001, 1, '0.1.1.1', "eth0"),
    (101001, 1, '0.1.1.2', "eth1"),
    (101001, 1, '::3', "eth2"),
    (101001, 1, '::4', "eth3"),
    (101002, 1, '0.1.2.1', "eth0"),
    (101003, 1, '0.1.3.1', "eth0"),
    (101004, 1, '0.1.4.1', "eth0"),
    (102001, 1, '0.2.1.1', "eth0"),
    (102001, 1, '0.2.1.2', "eth1"),
    (102001, 1, 'fe80:a123::0001', "eth2"),
    (102001, 1, 'fe80:a123::0002', "eth3"),
    (102002, 1, '0.2.2.1', "eth0"),
    (102003, 1, '0.2.3.1', "eth0"),
    (102004, 1, '0.2.4.1', "eth0"),
    (103001, 1, '0.3.1.1', "eth0"),
    (103001, 2, '0.3.1.2', "rdma"),
    (103002, 1, '0.3.2.1', "eth0"),
    (103002, 2, '0.3.2.2', "rdma"),
    (103003, 1, '0.3.3.1', "eth0"),
    (103003, 2, '0.3.3.2', "rdma"),
    (103004, 1, '0.3.4.1', "eth0"),
    (103004, 2, '0.3.4.2', "rdma")
;

INSERT INTO entities (uid, entity_type, alias) VALUES
    (401002, 3, "storage_pool_2"),
    (401003, 3, "storage_pool_3"),
    (401004, 3, "storage_pool_4")
;

INSERT INTO pools (node_type, pool_id, pool_uid) VALUES
    (2, 2, 401002),
    (2, 3, 401003),
    (2, 4, 401004)
;

INSERT INTO entities (uid, entity_type, alias) VALUES
    (201001, 2, "meta_target_1"),
    (201002, 2, "meta_target_2"),
    (201003, 2, "meta_target_3"),
    (201004, 2, "meta_target_4"),

    (202001, 2, "storage_target_1"),
    (202002, 2, "storage_target_2"),
    (202003, 2, "storage_target_3"),
    (202004, 2, "storage_target_4"),
    (202005, 2, "storage_target_5"),
    (202006, 2, "storage_target_6"),
    (202007, 2, "storage_target_7"),
    (202008, 2, "storage_target_8"),
    (202009, 2, "storage_target_9"),
    (202010, 2, "storage_target_10"),
    (202011, 2, "storage_target_11"),
    (202012, 2, "storage_target_12"),
    (202013, 2, "storage_target_13"),
    (202014, 2, "storage_target_14"),
    (202015, 2, "storage_target_15"),
    (202016, 2, "storage_target_16"),

    (202099, 2, "storage_target_unmapped")
;

INSERT INTO targets (target_uid, node_type, target_id, node_id, pool_id, total_space, total_inodes,
free_space, free_inodes, consistency, last_update) VALUES
    (201001, 1, 1, 1, NULL, 1000000, 1000000, 450000, 450000, 1, DATETIME("NOW")),
    (201002, 1, 2, 2, NULL, 1000000, 1000000, 550000, 550000, 1, DATETIME("NOW")),
    (201003, 1, 3, 3, NULL, 1000000, 1000000, 550000, 550000, 1, DATETIME("NOW")),
    (201004, 1, 4, 4, NULL, 1000000, 1000000, 450000, 450000, 1, DATETIME("NOW")),

    (202001, 2, 1, 1, 1, 1000000, 1000000, 450000, 450000, 1, DATETIME("NOW")),
    (202002, 2, 2, 1, 2, 1000000, 1000000, 500000, 500000, 1, DATETIME("NOW")),
    (202003, 2, 3, 1, 3, 1000000, 1000000, 500000, 500000, 1, DATETIME("NOW")),
    (202004, 2, 4, 1, 4, 1000000, 1000000, 500000, 500000, 1, DATETIME("NOW")),
    (202005, 2, 5, 2, 1, 1000000, 1000000, 450000, 450000, 1, DATETIME("NOW")),
    (202006, 2, 6, 2, 2, 1000000, 1000000, 500000, 500000, 1, DATETIME("NOW")),
    (202007, 2, 7, 2, 3, 1000000, 1000000, 500000, 500000, 1, DATETIME("NOW")),
    (202008, 2, 8, 2, 4, 1000000, 1000000, 500000, 500000, 1, DATETIME("NOW")),
    (202009, 2, 9, 3, 1, 1000000, 1000000, 550000, 550000, 1, DATETIME("NOW")),
    (202010, 2, 10, 3, 2, 1000000, 1000000, 500000, 500000, 1, DATETIME("NOW")),
    (202011, 2, 11, 3, 3, 1000000, 1000000, 500000, 500000, 1, DATETIME("NOW")),
    (202012, 2, 12, 3, 4, 1000000, 1000000, 500000, 500000, 1, DATETIME("NOW")),
    (202013, 2, 13, 4, 1, 1000000, 1000000, 550000, 550000, 1, DATETIME("NOW")),
    (202014, 2, 14, 4, 2, 1000000, 1000000, 500000, 500000, 1, DATETIME("NOW")),
    (202015, 2, 15, 4, 3, 1000000, 1000000, 500000, 500000, 1, DATETIME("NOW")),
    (202016, 2, 16, 4, 4, 1000000, 1000000, 500000, 500000, 1, DATETIME("NOW")),

    (202099, 2, 99, NULL, 1, NULL, NULL, NULL, NULL, 1, DATETIME("NOW"))
;

INSERT INTO entities (uid, entity_type, alias) VALUES
    (301001, 4, "meta_buddy_group_1"),
    (302001, 4, "storage_buddy_group_1"),
    (302002, 4, "storage_buddy_group_2")
;

INSERT INTO buddy_groups (group_uid, node_type, group_id, p_target_id, s_target_id, pool_id) VALUES
    (301001, 1, 1, 1, 2, NULL),
    (302001, 2, 1, 1, 5, 1),
    (302002, 2, 2, 9, 13, 1)
;


INSERT INTO root_inode (_only_one_row, target_id, group_id) VALUES
    (1, 1, NULL)
;

INSERT INTO quota_default_limits (id_type, quota_type, pool_id, value) VALUES
    (1, 1, 1, 1000),
    (1, 2, 1, 1000),
    (2, 1, 1, 1000),
    (2, 2, 1, 1000)
;
