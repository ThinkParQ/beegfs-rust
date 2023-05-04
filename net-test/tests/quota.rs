mod common;
use common::*;
use shared::msg::types::{QuotaData, QuotaInodeSupport, QuotaQueryType};

#[net_test("quotaEnableEnforcement=true")]
async fn default() {
    let n = DefaultNodes::setup().await;

    let r: GetDefaultQuotaResp = n
        .ctl
        .request(GetDefaultQuota {
            pool_id: StoragePoolID::DEFAULT,
        })
        .await;

    assert_eq!(0, r.limits.user_space_limit);
    assert_eq!(0, r.limits.user_inode_limit);
    assert_eq!(0, r.limits.group_space_limit);
    assert_eq!(0, r.limits.group_inode_limit);

    let r: SetDefaultQuotaResp = n
        .ctl
        .request(SetDefaultQuota {
            pool_id: StoragePoolID::DEFAULT,
            space: 1024,
            inodes: 100,
            quota_type: QuotaDataType::User,
        })
        .await;

    assert!(r.result);

    let r: SetDefaultQuotaResp = n
        .ctl
        .request(SetDefaultQuota {
            pool_id: StoragePoolID::DEFAULT,
            space: 2048,
            inodes: 200,
            quota_type: QuotaDataType::Group,
        })
        .await;

    assert!(r.result);

    let r: GetDefaultQuotaResp = n
        .ctl
        .request(GetDefaultQuota {
            pool_id: StoragePoolID::DEFAULT,
        })
        .await;

    assert_eq!(1024, r.limits.user_space_limit);
    assert_eq!(100, r.limits.user_inode_limit);
    assert_eq!(2048, r.limits.group_space_limit);
    assert_eq!(200, r.limits.group_inode_limit);
}

#[net_test("quotaEnableEnforcement=true")]
async fn set_get() {
    let n = DefaultNodes::setup().await;

    let q = vec![
        QuotaData {
            space: 4096,
            inodes: 300,
            id: 1234,
            quota_type: QuotaDataType::User,
            valid: true,
        },
        QuotaData {
            space: 8192,
            inodes: 400,
            id: 1235,
            quota_type: QuotaDataType::User,
            valid: true,
        },
    ];

    let r: SetQuotaResp = n
        .ctl
        .request(SetQuota {
            pool_id: StoragePoolID::DEFAULT,
            quota_entry: q.clone(),
        })
        .await;

    assert!(r.result);

    let r: GetQuotaInfoResp = n
        .ctl
        .request(GetQuotaInfo {
            query_type: QuotaQueryType::Single,
            data_type: QuotaDataType::User,
            id_range_start: 1234,
            id_range_end: 0,
            id_list: vec![],
            transfer_method: msg::types::GetQuotaInfoTransferMethod::AllTargetsOneRequest,
            target_id: TargetID::ZERO,
            pool_id: StoragePoolID::DEFAULT,
        })
        .await;

    assert_eq!(QuotaInodeSupport::Unknown, r.quota_inode_support);
    assert_eq!(vec![q[0].clone()], r.quota_entry);

    // let r: GetQuotaInfoResp = n
    //     .ctl
    //     .request_tcp(GetQuotaInfo {
    //         query_type: QuotaQueryType::Range,
    //         data_type: QuotaDataType::User,
    //         id_range_start: 1,
    //         id_range_end: 10000,
    //         id_list: vec![],
    //         target_selection: 0,
    //         target: TargetNumID::ZERO,
    //         pool_id: StoragePoolID::DEFAULT,
    //     })
    //     .await;

    // assert_eq!(q, r.quota_data);

    // let r: GetQuotaInfoResp = n
    //     .ctl
    //     .request_tcp(GetQuotaInfo {
    //         query_type: QuotaQueryType::List,
    //         data_type: QuotaDataType::User,
    //         id_range_start: 1,
    //         id_range_end: 10000,
    //         id_list: vec![1235],
    //         target_selection: 0,
    //         target: TargetNumID::ZERO,
    //         pool_id: StoragePoolID::DEFAULT,
    //     })
    //     .await;

    // assert_eq!(vec![q[1].clone()], r.quota_data);
}

#[net_test(
    "quotaEnableEnforcement=true",
    "quotaQueryUIDRange=1000,1010",
    "quotaQueryGIDRange=2000,2010",
    "quotaQueryType=range"
)]
async fn quota_exceeded() {
    let n = DefaultNodes::setup().await;

    let _t00 = Target::setup(&n.storage[0], "t00").await;

    let r: SetQuotaResp = n
        .ctl
        .request(SetQuota {
            pool_id: StoragePoolID::DEFAULT,
            quota_entry: vec![
                QuotaData {
                    space: 1000,
                    inodes: 1000,
                    id: 1001,
                    quota_type: QuotaDataType::User,
                    valid: true,
                },
                QuotaData {
                    space: 1000,
                    inodes: 1000,
                    id: 2001,
                    quota_type: QuotaDataType::Group,
                    valid: true,
                },
            ],
        })
        .await;

    assert!(r.result);

    n.storage[0]
        .msg_store
        .add_resp(
            GetQuotaInfo::ID,
            msg::GetQuotaInfoResp {
                quota_entry: vec![QuotaData {
                    space: 999,
                    inodes: 2000,
                    id: 1001,
                    quota_type: QuotaDataType::User,
                    valid: true,
                }],
                quota_inode_support: QuotaInodeSupport::Unknown,
            },
        )
        .await;

    n.storage[0]
        .msg_store
        .add_resp(
            GetQuotaInfo::ID,
            msg::GetQuotaInfoResp {
                quota_entry: vec![QuotaData {
                    space: 999,
                    inodes: 2000,
                    id: 2001,
                    quota_type: QuotaDataType::Group,
                    valid: true,
                }],
                quota_inode_support: QuotaInodeSupport::Unknown,
            },
        )
        .await;

    let (a, b) = tokio::join!(
        n.storage[0]
            .msg_store
            .wait_for_req_count::<GetQuotaInfo>(2, 6000),
        n.storage[0]
            .msg_store
            .wait_for_req(10000, |r: SetExceededQuota| {
                r.quota_type == QuotaDataType::User && r.exceeded_quota_ids == vec![1001]
            }),
        // TODO doesnt seem to work for group quota
        // n.storage[0].msg_store.wait_for_req(10000, |r: SetExceededQuota| {
        //     println!("AAAAA {r:?}");
        //     r.data_type == QuotaDataType::GROUP as i32 && r.exceeded_quota_ids == vec![2002]
        // })
    );

    a.unwrap();
    b.unwrap();
}
