use crate::db::*;
use anyhow::{anyhow, bail, Context, Result};
use rusqlite::Transaction;
use shared::bee_msg::buddy_group::CombinedTargetState;
use shared::bee_msg::quota::{QuotaDefaultLimits, QuotaEntry};
use shared::bee_msg::storage_pool::StoragePool;
use shared::bee_serde::{Deserializable, Deserializer};
use shared::types::*;
use sqlite_check::sql;
use std::path::Path;

/// Import v7 management data into the database. The database must be new, there must be no entries
/// except for the default ones.
///
/// The import only works with a "standard" setup from BeeGFS 7.2 or 7.4. The format.conf file
/// must be unmodified. Certain data is ignored as it is ephemeral anyway and will be filled
/// automatically on the running management. This includes quota usage data, client nodes and the
/// nodes nic lists. The old BeeGFS should be completely shut down before upgrading and all targets
/// must be in GOOD state.
pub fn import_v7(conn: &mut rusqlite::Connection, base_path: &Path) -> Result<()> {
    let tx = conn.transaction()?;

    // Check DB is new
    let max_uid: Uid = tx.query_row(sql!("SELECT MAX(uid) FROM entities"), [], |row| row.get(0))?;
    if max_uid > 2 {
        bail!("Database is not new");
    }

    // Check sanity
    check_format_conf(&base_path.join("format.conf")).context("on-disk-format (format.conf)")?;
    check_target_states(&base_path.join("nodeStates")).context("meta targets (nodeStates)")?;
    check_target_states(&base_path.join("targetStates"))
        .context("storage targets (targetStates)")?;

    // Read from files, write to database. Order is important.

    // Storage
    storage_nodes(&tx, &base_path.join("storage.nodes")).context("storage.nodes")?;
    storage_targets(
        &tx,
        &base_path.join("targets"),
        &base_path.join("targetNumIDs"),
    )
    .context("storage targets (target + targetNumIDs)")?;
    buddy_groups(
        &tx,
        &base_path.join("storagebuddygroups"),
        NodeTypeServer::Storage,
    )
    .context("storage buddy groups (storagebuddygroups)")?;
    storage_pools(&tx, &base_path.join("storagePools")).context("storagePools")?;

    // Meta
    let (root_id, root_mirrored) =
        meta_nodes(&tx, &base_path.join("meta.nodes")).context("meta.nodes")?;
    buddy_groups(
        &tx,
        &base_path.join("metabuddygroups"),
        NodeTypeServer::Meta,
    )
    .context("meta buddy groups (metabuddygroups)")?;
    set_meta_root(&tx, root_id, root_mirrored).context("meta root")?;

    // Quota
    if std::path::Path::try_exists(&base_path.join("quota"))? {
        quota(&tx, &base_path.join("quota"))?;
    }

    tx.commit()?;
    Ok(())
}

/// Checks that the format.conf file is in the original state
fn check_format_conf(f: &Path) -> Result<()> {
    let s = std::fs::read_to_string(f)?;

    // No need for fancy parsing, we only allow upgrading from this exact file
    if s != "# This file was auto-generated. Do not modify it!
version=5
nodeStates=1
targetStates=1
"
    {
        bail!("Unexpected format.conf:\n{s}");
    }

    Ok(())
}

/// Ensures that all stored targets (nodes in case of meta) are in GOOD state
fn check_target_states(f: &Path) -> Result<()> {
    let s = std::fs::read(f)?;

    let mut des = Deserializer::new(&s, 0);
    let states = des.map(
        false,
        |des| TargetId::deserialize(des),
        |des| {
            let res = CombinedTargetState::deserialize(des)?;
            // Ignore the last change time, we don't need it
            des.i64()?;
            des.i64()?;

            Ok(res)
        },
    )?;
    des.finish()?;

    let not_good: Vec<_> = states
        .iter()
        .filter(|e| e.1.consistency != TargetConsistencyState::Good)
        .map(|e| e.0)
        .collect();
    if !not_good.is_empty() {
        bail!("targets {:?} are not in GOOD state", not_good);
    }

    Ok(())
}

/// Imports meta nodes / targets. Intentionally ignores nics as they are refreshed on first contact
/// anyway.
fn meta_nodes(tx: &Transaction, f: &Path) -> Result<(NodeId, bool)> {
    let (root_id, root_mirrored, nodes) = read_nodes(f)?;

    for n in nodes {
        node::insert(tx, n.num_id, None, NodeType::Meta, n.port)?;

        // A meta target has to be explicitly created with the same ID as the node.
        let Ok(target_id) = TargetId::try_from(n.num_id) else {
            bail!(
                "{} is not a valid numeric meta node/target id (must be between 1 and 65535)",
                n.num_id
            );
        };
        target::insert_meta(tx, target_id, None)?;
    }

    if root_id == 0 {
        bail!("numeric meta root id can not be 0");
    }

    // root ID is set later after buddy groups have been imported
    Ok((root_id, root_mirrored))
}

// Imports storage nodes
fn storage_nodes(tx: &Transaction, f: &Path) -> Result<()> {
    let (_, _, nodes) = read_nodes(f)?;

    for n in nodes {
        node::insert(tx, n.num_id, None, NodeType::Storage, n.port)?;
    }

    Ok(())
}

// Deserialize nodes from file
fn read_nodes(f: &Path) -> Result<(NodeId, bool, Vec<shared::bee_msg::node::Node>)> {
    let s = std::fs::read(f)?;

    let mut des = Deserializer::new(&s, 0);
    let version = des.u32()?;
    let root_id = des.u32()?;
    let root_mirrored = des.u8()?;
    let nodes = des.seq(false, |des| shared::bee_msg::node::Node::deserialize(des))?;
    des.finish()?;

    if version != 0 {
        bail!("invalid version {version}");
    }

    Ok((root_id, root_mirrored > 0, nodes))
}

// Imports buddy groups
fn buddy_groups(tx: &Transaction, f: &Path, nt: NodeTypeServer) -> Result<()> {
    let s = std::fs::read_to_string(f)?;

    for l in s.lines() {
        let (g, ts) = l
            .trim()
            .split_once('=')
            .ok_or_else(|| anyhow!("invalid line '{l}'"))?;

        let g: BuddyGroupId = g.parse()?;
        let (p_id, s_id) = ts
            .trim()
            .split_once(',')
            .ok_or_else(|| anyhow!("invalid line '{l}'"))?;

        buddy_group::insert(
            tx,
            g,
            &format!("buddy_group_{}_{}", nt.sql_table_str(), g).try_into()?,
            nt,
            p_id.parse()?,
            s_id.parse()?,
        )?;
    }

    Ok(())
}

/// Imports storage targets
fn storage_targets(
    tx: &Transaction,
    targets_path: &Path,
    target_num_ids_path: &Path,
) -> Result<()> {
    let targets = std::fs::read_to_string(targets_path)?;
    let target_num_ids = std::fs::read_to_string(target_num_ids_path)?;

    if targets.lines().count() != target_num_ids.lines().count() {
        bail!("line count mismatch between {targets_path:?} and {target_num_ids_path:?}");
    }

    for l in targets.lines().zip(target_num_ids.lines()) {
        let (target, node) =
            l.0.trim()
                .split_once('=')
                .ok_or_else(|| anyhow!("invalid line '{}'", l.0))?;

        let node_id: NodeId = node.parse()?;
        let target_id: TargetId = target.parse()?;

        target::insert_storage(tx, target_id, None)?;
        target::update_storage_node_mappings(tx, &[target_id], node_id)?;
    }

    Ok(())
}

/// Imports storage pools
fn storage_pools(tx: &Transaction, f: &Path) -> Result<()> {
    let s = std::fs::read(f)?;

    let mut des = Deserializer::new(&s, 0);
    // Serialized as size_t, which should usually be 64 bit.
    let count = des.i64()?;
    for _ in 0..count {
        let pool = StoragePool::deserialize(&mut des)?;

        if pool.id == DEFAULT_STORAGE_POOL {
            continue;
        }

        let alias: Alias = std::str::from_utf8(&pool.alias)?.try_into()?;

        storage_pool::insert(tx, pool.id, &alias)?;
        target::update_storage_pools(tx, pool.id, &pool.targets)?;
        buddy_group::update_storage_pools(tx, pool.id, &pool.buddy_groups)?;
    }
    des.finish()?;

    Ok(())
}

/// Sets the root inode info according to the given info
fn set_meta_root(tx: &Transaction, root_id: NodeId, root_mirrored: bool) -> Result<()> {
    // overwrite root inode with the correct setting
    if root_mirrored {
        tx.execute(
            sql!("UPDATE root_inode SET target_id = NULL, group_id = ?1"),
            [root_id],
        )?;
    } else {
        tx.execute(
            sql!("UPDATE root_inode SET group_id = NULL, target_id = ?1"),
            [root_id],
        )?;
    }

    Ok(())
}

/// Imports quota settings
fn quota(tx: &Transaction, quota_path: &Path) -> Result<()> {
    // Quota settings are stored per storage pool in a subdirectory named like the pool ID
    for e in std::fs::read_dir(quota_path)? {
        let e = e?;

        if !e.file_type()?.is_dir() {
            continue;
        }

        let pool_id: PoolId = e
            .file_name()
            .into_string()
            .map_err(|s| anyhow!("{s:?} is not a valid storage pool directory"))?
            .parse()
            .map_err(|_| anyhow!("{:?} is not a valid storage pool directory", e.file_name()))?;

        quota_default_limits(tx, &e.path().join("quotaDefaultLimits.store"), pool_id)
            .with_context(|| {
                format!(
                    "quota default limits ({}/quotaDefaultLimits.store)",
                    pool_id
                )
            })?;

        quota_limits(
            tx,
            &e.path().join("quotaUserLimits.store"),
            pool_id,
            QuotaIdType::User,
        )
        .with_context(|| format!("quota user limits ({}/quotaUserLimits.store)", pool_id))?;

        quota_limits(
            tx,
            &e.path().join("quotaGroupLimits.store"),
            pool_id,
            QuotaIdType::Group,
        )
        .with_context(|| format!("quota group limits ({}/quotaGroupLimits.store)", pool_id))?;

        // We intentionally ignore the quota usage data - it is fetched and updated from the
        // nodes on a regular basis anyway.
    }

    Ok(())
}

/// Imports the default quota limits
fn quota_default_limits(tx: &Transaction, f: &Path, pool_id: PoolId) -> Result<()> {
    let s = std::fs::read(f)?;

    let mut des = Deserializer::new(&s, 0);
    let limits = QuotaDefaultLimits::deserialize(&mut des)?;
    des.finish()?;

    let mut stmt = tx.prepare_cached(sql!(
        "INSERT INTO quota_default_limits
        (id_type, quota_type, pool_id, value)
        VALUES (?1, ?2, ?3, ?4)"
    ))?;

    let affected = stmt.execute(params![
        QuotaIdType::User.sql_variant(),
        QuotaType::Space.sql_variant(),
        pool_id,
        limits.user_space_limit
    ])?;
    check_affected_rows(affected, [1])?;
    let affected = stmt.execute(params![
        QuotaIdType::User.sql_variant(),
        QuotaType::Inodes.sql_variant(),
        pool_id,
        limits.user_inode_limit
    ])?;
    check_affected_rows(affected, [1])?;
    let affected = stmt.execute(params![
        QuotaIdType::Group.sql_variant(),
        QuotaType::Space.sql_variant(),
        pool_id,
        limits.group_space_limit
    ])?;
    check_affected_rows(affected, [1])?;
    let affected = stmt.execute(params![
        QuotaIdType::Group.sql_variant(),
        QuotaType::Inodes.sql_variant(),
        pool_id,
        limits.group_space_limit,
    ])?;
    check_affected_rows(affected, [1])?;

    Ok(())
}

/// Imports the specific (per user/group) quota limits
fn quota_limits(
    tx: &Transaction,
    f: &Path,
    pool_id: PoolId,
    quota_id_type: QuotaIdType,
) -> Result<()> {
    let s = std::fs::read(f)?;

    let mut des = Deserializer::new(&s, 0);
    let limits = des.seq(false, |des| QuotaEntry::deserialize(des))?;
    des.finish()?;

    // We filter out where the quota ID is 0 because old management seems to store the default
    // settings for a pool together with the specific limits. But this is redundant, the default
    // settings are also stored (and imported) explicitly in/from a different file.
    let mut insert_stmt = tx.prepare_cached(sql!(
        "INSERT INTO quota_limits (quota_id, id_type, quota_type, pool_id, value)
        VALUES(?1, ?2, ?3 ,?4 ,?5)"
    ))?;

    for l in limits.iter().filter(|e| e.id_type == quota_id_type) {
        if l.space > 0 {
            insert_stmt.execute(params![
                l.id,
                l.id_type.sql_variant(),
                QuotaType::Space.sql_variant(),
                pool_id,
                l.space
            ])?;
        }

        if l.inodes > 0 {
            insert_stmt.execute(params![
                l.id,
                l.id_type.sql_variant(),
                QuotaType::Inodes.sql_variant(),
                pool_id,
                l.inodes
            ])?;
        }
    }

    Ok(())
}
