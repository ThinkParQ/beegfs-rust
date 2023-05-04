mod common;
use common::*;

#[net_test]
async fn register_nodes() {
    let nodes = DefaultNodes::setup().await;

    let meta_list: GetNodesResp = nodes
        .ctl
        .request(GetNodes {
            node_type: NodeType::Meta,
        })
        .await;

    let storage_list: GetNodesResp = nodes
        .ctl
        .request(GetNodes {
            node_type: NodeType::Storage,
        })
        .await;

    let client_list: GetNodesResp = nodes
        .ctl
        .request(GetNodes {
            node_type: NodeType::Client,
        })
        .await;

    assert_eq!(2, meta_list.nodes.len());
    assert_eq!(2, storage_list.nodes.len());
    assert_eq!(2, client_list.nodes.len());

    assert_ne!(nodes.meta[0].num_id, nodes.meta[1].num_id);
    assert_ne!(nodes.storage[0].num_id, nodes.storage[1].num_id);
    assert_ne!(nodes.client[0].num_id, nodes.client[1].num_id);

    assert_eq!(meta_list.nodes[0].num_id, nodes.meta[0].num_id);
    assert_eq!(storage_list.nodes[0].num_id, nodes.storage[0].num_id);
    assert_eq!(client_list.nodes[0].num_id, nodes.client[0].num_id);
}

#[net_test]
async fn remove_nodes() {
    let nodes = DefaultNodes::setup().await;

    let remove_res: RemoveNodeResp = nodes
        .ctl
        .request(RemoveNode {
            node_type: NodeType::Meta,
            num_id: NodeID::from(9999),
            ack_id: "".into(),
        })
        .await;

    assert_ne!(OpsErr::SUCCESS, remove_res.result);

    let remove_res: RemoveNodeResp = nodes
        .ctl
        .request(RemoveNode {
            node_type: NodeType::Meta,
            num_id: nodes.meta[0].num_id,
            ack_id: "".into(),
        })
        .await;

    assert_eq!(OpsErr::SUCCESS, remove_res.result);

    let remove_res: RemoveNodeResp = nodes
        .ctl
        .request(RemoveNode {
            node_type: NodeType::Storage,
            num_id: nodes.storage[0].num_id,
            ack_id: "".into(),
        })
        .await;

    assert_eq!(OpsErr::SUCCESS, remove_res.result);

    let remove_res: RemoveNodeResp = nodes
        .ctl
        .request(RemoveNode {
            node_type: NodeType::Client,
            num_id: nodes.client[0].num_id,
            ack_id: "".into(),
        })
        .await;

    assert_eq!(OpsErr::SUCCESS, remove_res.result);

    let meta_list: GetNodesResp = nodes
        .ctl
        .request(GetNodes {
            node_type: NodeType::Meta,
        })
        .await;

    let storage_list: GetNodesResp = nodes
        .ctl
        .request(GetNodes {
            node_type: NodeType::Storage,
        })
        .await;

    let client_list: GetNodesResp = nodes
        .ctl
        .request(GetNodes {
            node_type: NodeType::Client,
        })
        .await;

    assert_eq!(1, meta_list.nodes.len());
    assert_eq!(1, storage_list.nodes.len());
    assert_eq!(1, client_list.nodes.len());
}

#[net_test]
async fn setup_targets() {
    let n = DefaultNodes::setup().await;
    let t00 = Target::setup(&n.storage[0], "t00").await;
    let t01 = Target::setup(&n.storage[0], "t01").await;
    let t10 = Target::setup(&n.storage[1], "t10").await;
    let t11 = Target::setup(&n.storage[1], "t11").await;

    let r: GetTargetMappingsResp = n.ctl.request(GetTargetMappings {}).await;

    assert_eq!(4, r.mapping.len());
    assert!(r.mapping.iter().any(|e| e == (&t00.id, &t00.on_node)));
    assert!(r.mapping.iter().any(|e| e == (&t01.id, &t01.on_node)));
    assert!(r.mapping.iter().any(|e| e == (&t10.id, &t10.on_node)));
    assert!(r.mapping.iter().any(|e| e == (&t11.id, &t11.on_node)));

    let r: GetTargetStatesResp = n
        .ctl
        .request(GetTargetStates {
            node_type: NodeTypeServer::Storage,
        })
        .await;

    assert_eq!(4, r.targets.len());
    assert_eq!(4, r.consistency_states.len());
    assert_eq!(4, r.reachability_states.len());

    assert!(r
        .consistency_states
        .iter()
        .all(|e| e == &TargetConsistencyState::Good));
    assert!(r
        .reachability_states
        .iter()
        .all(|e| e == &TargetReachabilityState::ProbablyOffline));
}

#[net_test]
async fn unmap_target() {
    let n = DefaultNodes::setup().await;
    let t0 = Target::setup(&n.storage[0], "t0").await;
    let _t1 = Target::setup(&n.storage[0], "t1").await;

    let r: UnmapStorageTargetResp = n.ctl.request(UnmapStorageTarget { target_id: t0.id }).await;

    assert_eq!(OpsErr::SUCCESS, r.result);

    let r: GetTargetMappingsResp = n.ctl.request(GetTargetMappings {}).await;

    assert_eq!(1, r.mapping.len());
    assert!(!r.mapping.iter().any(|e| e.0 == &t0.id));
}

// Add, modify, remove storage pools
#[net_test]
async fn storage_pools() {
    let n = DefaultNodes::setup().await;
    let t00 = Target::setup(&n.storage[0], "t00").await;
    let t01 = Target::setup(&n.storage[0], "t01").await;
    let t10 = Target::setup(&n.storage[1], "t10").await;
    let t11 = Target::setup(&n.storage[1], "t11").await;

    let p2 = StoragePool::setup(&[t10.id], &n.ctl, "Pool2").await;
    let p3 = StoragePool::setup(&[t01.id, t11.id], &n.ctl, "Pool3").await;

    let r: GetStoragePoolsResp = n.ctl.request(GetStoragePools {}).await;

    let default_pool = r
        .pools
        .iter()
        .find(|e| e.id == StoragePoolID::DEFAULT)
        .unwrap();
    assert_eq!(0, default_pool.buddy_groups.len());
    assert_eq!(1, default_pool.targets.len());
    assert!(default_pool.targets.contains(&t00.id));

    let pool2 = r.pools.iter().find(|e| e.id == p2.id).unwrap();
    assert_eq!(0, default_pool.buddy_groups.len());
    assert_eq!(1, pool2.targets.len());
    assert!(pool2.targets.contains(&t10.id));

    let pool3 = r.pools.iter().find(|e| e.id == p3.id).unwrap();
    assert_eq!(0, default_pool.buddy_groups.len());
    assert_eq!(2, pool3.targets.len());
    assert!(pool3.targets.contains(&t01.id));
    assert!(pool3.targets.contains(&t11.id));

    let r: ModifyStoragePoolResp = n
        .ctl
        .request(ModifyStoragePool {
            id: p2.id,
            alias: Some("NewDesc2".into()),
            add_target_ids: vec![t00.id],
            remove_target_ids: vec![t10.id],
            add_buddy_group_ids: vec![],
            remove_buddy_group_ids: vec![],
        })
        .await;

    assert_eq!(OpsErr::SUCCESS, r.result);

    let r: GetStoragePoolsResp = n.ctl.request(GetStoragePools {}).await;

    let default_pool = r
        .pools
        .iter()
        .find(|e| e.id == StoragePoolID::DEFAULT)
        .unwrap();
    assert_eq!(1, default_pool.targets.len());
    assert_eq!(vec![t10.id], *default_pool.targets);

    let pool2 = r.pools.iter().find(|e| e.id == p2.id).unwrap();
    assert_eq!(1, pool2.targets.len());
    assert!(pool2.targets.contains(&t00.id));
    assert_eq!(b"NewDesc2", pool2.alias.as_ref());

    let pool3 = r.pools.iter().find(|e| e.id == p3.id).unwrap();
    assert_eq!(2, pool3.targets.len());

    let r: RemoveStoragePoolResp = n.ctl.request(RemoveStoragePool { id: p3.id }).await;

    assert_eq!(OpsErr::SUCCESS, r.result);

    let r: GetStoragePoolsResp = n.ctl.request(GetStoragePools {}).await;

    let default_pool = r
        .pools
        .iter()
        .find(|e| e.id == StoragePoolID::DEFAULT)
        .unwrap();
    assert_eq!(3, default_pool.targets.len());
    assert!(default_pool.targets.contains(&t01.id));
    assert!(default_pool.targets.contains(&t11.id));
}

// Define and remove buddy groups, change states, check the various messages for consistency
#[net_test]
async fn storage_buddy_groups() {
    let n = DefaultNodes::setup().await;
    let t00 = Target::setup(&n.storage[0], "t00").await;
    let t01 = Target::setup(&n.storage[0], "t01").await;
    let t10 = Target::setup(&n.storage[1], "t10").await;
    let t11 = Target::setup(&n.storage[1], "t11").await;

    let r: SetMirrorBuddyGroupResp = n
        .ctl
        .request(SetMirrorBuddyGroup {
            node_type: NodeTypeServer::Storage,
            primary_target: t00.id,
            secondary_target: t10.id,
            buddy_group_id: BuddyGroupID::ZERO,
            allow_update: true,
            ack_id: "".into(),
        })
        .await;

    assert_eq!(OpsErr::SUCCESS, r.result);
    assert_ne!(BuddyGroupID::ZERO, r.buddy_group_id);

    let g1 = r.buddy_group_id;

    let r: SetMirrorBuddyGroupResp = n
        .ctl
        .request(SetMirrorBuddyGroup {
            node_type: NodeTypeServer::Storage,
            primary_target: t01.id,
            secondary_target: t11.id,
            buddy_group_id: BuddyGroupID::ZERO,
            allow_update: true,
            ack_id: "".into(),
        })
        .await;

    assert_eq!(OpsErr::SUCCESS, r.result);
    assert_ne!(BuddyGroupID::ZERO, r.buddy_group_id);

    let g2 = r.buddy_group_id;

    let r: GetMirrorBuddyGroupsResp = n
        .ctl
        .request(GetMirrorBuddyGroups {
            node_type: NodeTypeServer::Storage,
        })
        .await;

    assert_eq!(2, r.buddy_groups.len());
    assert!(r.buddy_groups.contains(&g1));
    assert!(r.buddy_groups.contains(&g2));

    for e in r
        .buddy_groups
        .iter()
        .zip(r.primary_targets.iter().zip(r.secondary_targets.iter()))
    {
        let t = match e.0 {
            x if x == &g1 => (&t00.id, &t10.id),
            x if x == &g2 => (&t01.id, &t11.id),
            _ => panic!(),
        };
        assert_eq!(e.1, t);
    }

    let r: GetStatesAndBuddyGroupsResp = n
        .ctl
        .request(GetStatesAndBuddyGroups {
            node_type: NodeTypeServer::Storage,
        })
        .await;

    assert_eq!(r.groups[&g1].primary_target_id, t00.id);
    assert_eq!(r.groups[&g1].secondary_target_id, t10.id);
    assert_eq!(r.groups[&g2].primary_target_id, t01.id);
    assert_eq!(r.groups[&g2].secondary_target_id, t11.id);

    assert_eq!(r.states[&t00.id].consistency, TargetConsistencyState::Good);
    assert_eq!(
        r.states[&t00.id].reachability,
        TargetReachabilityState::ProbablyOffline
    );

    let r: ChangeTargetConsistencyStatesResp = n
        .ctl
        .request(ChangeTargetConsistencyStates {
            node_type: NodeTypeServer::Storage,
            target_ids: vec![t00.id],
            old_states: vec![TargetConsistencyState::NeedsResync],
            new_states: vec![TargetConsistencyState::Bad],
            ack_id: "".into(),
        })
        .await;

    assert_eq!(OpsErr::AGAIN, r.result);

    let r: ChangeTargetConsistencyStatesResp = n
        .ctl
        .request(ChangeTargetConsistencyStates {
            node_type: NodeTypeServer::Storage,
            target_ids: vec![t00.id],
            old_states: vec![TargetConsistencyState::Good],
            new_states: vec![TargetConsistencyState::Bad],
            ack_id: "".into(),
        })
        .await;

    assert_eq!(OpsErr::SUCCESS, r.result);

    let r: GetStatesAndBuddyGroupsResp = n
        .ctl
        .request(GetStatesAndBuddyGroups {
            node_type: NodeTypeServer::Storage,
        })
        .await;

    assert_eq!(r.states[&t00.id].consistency, TargetConsistencyState::Bad);
    assert_eq!(r.states[&t01.id].consistency, TargetConsistencyState::Good);

    let r: SetTargetConsistencyStatesResp = n
        .ctl
        .request(SetTargetConsistencyStates {
            node_type: NodeTypeServer::Storage,
            targets: vec![t01.id, t11.id],
            states: vec![
                TargetConsistencyState::NeedsResync,
                TargetConsistencyState::NeedsResync,
            ],
            ack_id: "".into(),
            set_online: true,
        })
        .await;

    assert_eq!(OpsErr::SUCCESS, r.result);

    let r: GetStatesAndBuddyGroupsResp = n
        .ctl
        .request(GetStatesAndBuddyGroups {
            node_type: NodeTypeServer::Storage,
        })
        .await;

    assert_eq!(
        r.states[&t01.id].consistency,
        TargetConsistencyState::NeedsResync
    );
    assert_eq!(
        r.states[&t01.id].reachability,
        TargetReachabilityState::Online
    );
    assert_eq!(
        r.states[&t11.id].consistency,
        TargetConsistencyState::NeedsResync
    );
    assert_eq!(
        r.states[&t11.id].reachability,
        TargetReachabilityState::Online
    );

    let r: RemoveBuddyGroupResp = n
        .ctl
        .request(RemoveBuddyGroup {
            node_type: NodeTypeServer::Storage,
            group_id: g2,
            check_only: false,
            force: false,
        })
        .await;

    assert_eq!(OpsErr::SUCCESS, r.result);

    let r: GetMirrorBuddyGroupsResp = n
        .ctl
        .request(GetMirrorBuddyGroups {
            node_type: NodeTypeServer::Storage,
        })
        .await;

    assert_eq!(1, r.buddy_groups.len());

    let r: AddStoragePoolResp = n
        .ctl
        .request(AddStoragePool {
            id: 2.into(),
            alias: "GroupPool".into(),
            move_target_ids: vec![],
            move_buddy_group_ids: vec![g1],
        })
        .await;

    assert_eq!(OpsErr::SUCCESS, r.result);

    let r: GetStoragePoolsResp = n.ctl.request(GetStoragePools {}).await;

    let pool2 = r.pools.iter().find(|e| e.id == 2.into()).unwrap();
    assert_eq!(1, pool2.buddy_groups.len());
    assert_eq!(2, pool2.targets.len());
}
