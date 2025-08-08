use crate::db::*;
use anyhow::{Context, Result, anyhow, bail};
use rusqlite::Transaction;
use shared::bee_msg::buddy_group::CombinedTargetState;
use shared::bee_msg::node::Nic;
use shared::bee_msg::quota::QuotaEntry;
use shared::bee_msg::storage_pool::StoragePool;
use shared::bee_serde::{BeeSerdeConversion, Deserializable, Deserializer};
use shared::types::*;
use sqlite_check::sql;
use std::io::Write;
use std::path::Path;

/// Import v7 management data into the database. The database must be new, there must be no entries
/// except for the default ones.
///
/// The import only works with a "standard" setup from BeeGFS 7.2 or 7.4. The format.conf file
/// must be unmodified. Certain data is ignored as it is ephemeral anyway and will be filled
/// automatically on the running management. This includes quota usage data, client nodes and the
/// nodes nic lists. The old BeeGFS should be completely shut down before upgrading and all targets
/// must be in GOOD state.
pub fn import_v7(tx: &rusqlite::Transaction, base_path: &Path) -> Result<()> {
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
    storage_nodes(tx, &base_path.join("storage.nodes")).context("storage.nodes")?;
    storage_targets(tx, &base_path.join("targets"))
        .context("storage targets (target + targetNumIDs)")?;
    buddy_groups(
        tx,
        &base_path.join("storagebuddygroups"),
        NodeTypeServer::Storage,
    )
    .context("storage buddy groups (storagebuddygroups)")?;
    storage_pools(tx, &base_path.join("storagePools")).context("storagePools")?;

    // Meta
    let (root_id, root_mirrored) =
        meta_nodes(tx, &base_path.join("meta.nodes")).context("meta.nodes")?;
    buddy_groups(tx, &base_path.join("metabuddygroups"), NodeTypeServer::Meta)
        .context("meta buddy groups (metabuddygroups)")?;
    set_meta_root(tx, root_id, root_mirrored).context("meta root")?;

    // Quota
    if std::path::Path::try_exists(&base_path.join("quota"))? {
        quota(tx, &base_path.join("quota"))?;
    }

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

fn node_nics(tx: &Transaction, node_uid: Uid, nics: Vec<Nic>) -> Result<()> {
    let mut stmt = tx.prepare_cached(sql!(
        "INSERT INTO node_nics (node_uid, nic_type, addr, name) VALUES (?1, ?2, ?3, ?4)"
    ))?;

    for nic in nics {
        stmt.execute(params![
            node_uid,
            nic.nic_type.sql_variant(),
            nic.addr.to_string(),
            String::from_utf8_lossy(&nic.name)
        ])?;
    }

    Ok(())
}

/// Imports meta nodes / targets. Intentionally ignores nics as they are refreshed on first contact
/// anyway.
fn meta_nodes(tx: &Transaction, f: &Path) -> Result<(NodeId, bool)> {
    let ReadNodesResult {
        root_id,
        root_mirrored,
        nodes,
    } = read_nodes(f)?;

    for (num_id, port, nics) in nodes {
        let node = node::insert(tx, num_id, None, NodeType::Meta, port)?;
        node_nics(tx, node.uid, nics)?;

        // A meta target has to be explicitly created with the same ID as the node.
        let Ok(target_id) = TargetId::try_from(num_id) else {
            bail!(
                "{num_id} is not a valid numeric meta node/target id (must be between 1 and 65535)",
            );
        };
        target::insert(
            tx,
            target_id,
            None,
            NodeTypeServer::Meta,
            Some(target_id.into()),
        )?;
    }

    if root_id == 0 {
        bail!("numeric meta root id can not be 0");
    }

    // root ID is set later after buddy groups have been imported
    Ok((root_id, root_mirrored))
}

// Imports storage nodes
fn storage_nodes(tx: &Transaction, f: &Path) -> Result<()> {
    let ReadNodesResult { nodes, .. } = read_nodes(f)?;

    for (num_id, port, nics) in nodes {
        let node = node::insert(tx, num_id, None, NodeType::Storage, port)?;
        node_nics(tx, node.uid, nics)?;
    }

    Ok(())
}

struct ReadNodesResult {
    root_id: NodeId,
    root_mirrored: bool,
    nodes: Vec<(NodeId, Port, Vec<Nic>)>,
}

// Deserialize nodes from file
fn read_nodes(f: &Path) -> Result<ReadNodesResult> {
    let s = std::fs::read(f)?;

    let mut des = Deserializer::new(&s, 0);
    let version = des.u32()?;
    let root_id = des.u32()?;
    let root_mirrored = des.u8()?;

    // Define the node data deserialization manually because the `Nic` type used by `Node` had
    // changes for v8 (the ipv6 changes). The v7 on-disk-data is of course still in the old format.
    let nodes = des.seq(false, |des| {
        des.cstr(0)?;
        // The v7 on-disk-data does NOT contain the total size field, so putting `false` here is
        // correct. It only got introduced with the ipv6 changes.
        let nics = des.seq(false, |des| {
            // The v7 on-disk-data uses the old format without a protocol field, thus we deserialize
            // it manually here.
            let addr = des.u32()?.to_le_bytes().into();
            let mut name = des.bytes(15)?;
            des.u8()?;
            let nic_type = NicType::try_from_bee_serde(des.u8()?)?;
            des.skip(3)?;

            // Remove null bytes from name
            name.retain(|b| b != &0);

            Ok(Nic {
                addr,
                name,
                nic_type,
            })
        })?;

        let num_id = NodeId::deserialize(des)?;
        let port = Port::deserialize(des)?;
        Port::deserialize(des)?;
        des.u8()?;
        Ok((num_id, port, nics))
    })?;
    des.finish()?;

    if version != 0 {
        bail!("invalid version {version}");
    }

    Ok(ReadNodesResult {
        root_id,
        root_mirrored: root_mirrored > 0,
        nodes,
    })
}

// Imports buddy groups
fn buddy_groups(tx: &Transaction, f: &Path, nt: NodeTypeServer) -> Result<()> {
    let s = std::fs::read_to_string(f)?;

    for l in s.lines() {
        let (g, ts) = l
            .trim()
            .split_once('=')
            .ok_or_else(|| anyhow!("invalid line '{l}'"))?;

        let g = BuddyGroupId::from_str_radix(g.trim(), 16)?;
        let (p_id, s_id) = ts
            .trim()
            .split_once(',')
            .ok_or_else(|| anyhow!("invalid line '{l}'"))?;

        buddy_group::insert(
            tx,
            g,
            None,
            nt,
            BuddyGroupId::from_str_radix(p_id.trim(), 16)?,
            BuddyGroupId::from_str_radix(s_id.trim(), 16)?,
        )?;
    }

    Ok(())
}

/// Imports storage targets
fn storage_targets(tx: &Transaction, targets_path: &Path) -> Result<()> {
    let targets = std::fs::read_to_string(targets_path)?;

    for l in targets.lines() {
        let (target, node) = l
            .trim()
            .split_once('=')
            .ok_or_else(|| anyhow!("invalid line '{}'", l))?;

        let node_id = NodeId::from_str_radix(node.trim(), 16)?;
        let target_id = TargetId::from_str_radix(target.trim(), 16)?;

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
    let mut used_aliases = vec![];
    for _ in 0..count {
        let pool = StoragePool::deserialize(&mut des)?;

        let mut alias_input = String::from_utf8_lossy(&pool.alias).to_string();

        let alias = loop {
            match Alias::try_from(alias_input.trim()) {
                Ok(a) => {
                    if used_aliases.contains(&a) {
                        println!(
                            "Storage pool {}: Alias '{a}' is already used by another pool",
                            pool.id
                        );
                    } else {
                        break a;
                    }
                }
                Err(err) => {
                    println!("Storage pool {}: {err}", pool.id);
                }
            }

            print!("Please provide a new alias for this pool: ");
            std::io::stdout().flush().ok();
            alias_input.clear();
            std::io::stdin().read_line(&mut alias_input).ok();
        };

        if pool.id == DEFAULT_STORAGE_POOL {
            tx.execute(
                sql!("UPDATE entities SET alias = ?1 WHERE uid = 2"),
                [alias.as_ref()],
            )?;
        } else {
            storage_pool::insert(tx, pool.id, &alias)?;
        }

        target::update_storage_pools(tx, pool.id, &pool.targets)?;
        buddy_group::update_storage_pools(tx, pool.id, &pool.buddy_groups)?;

        used_aliases.push(alias);
    }
    des.finish()?;

    Ok(())
}

/// Sets the root inode info according to the given info. Should only be used against a new empty
/// root_inode table with no rows as part of the import process.
fn set_meta_root(tx: &Transaction, root_id: NodeId, root_mirrored: bool) -> Result<()> {
    if root_mirrored {
        tx.execute(
            sql!("INSERT INTO root_inode (target_id, group_id) VALUES (NULL, ?1)"),
            [root_id],
        )?;
    } else {
        tx.execute(
            sql!("INSERT INTO root_inode (target_id, group_id) VALUES (?1, NULL)"),
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
                format!("quota default limits ({pool_id}/quotaDefaultLimits.store)")
            })?;

        quota_limits(
            tx,
            &e.path().join("quotaUserLimits.store"),
            pool_id,
            QuotaIdType::User,
        )
        .with_context(|| format!("quota user limits ({pool_id}/quotaUserLimits.store)"))?;

        quota_limits(
            tx,
            &e.path().join("quotaGroupLimits.store"),
            pool_id,
            QuotaIdType::Group,
        )
        .with_context(|| format!("quota group limits ({pool_id}/quotaGroupLimits.store)"))?;

        // We intentionally ignore the quota usage data - it is fetched and updated from the
        // nodes on a regular basis anyway.
    }

    Ok(())
}

/// Imports the default quota limits
fn quota_default_limits(tx: &Transaction, f: &Path, pool_id: PoolId) -> Result<()> {
    // If the file is missing, skip it
    let s = match std::fs::read(f) {
        Ok(s) => s,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            println!("WARNING: Ignoring missing quota limits file {f:?} for pool {pool_id}");
            return Ok(());
        }
        Err(err) => return Err(err.into()),
    };

    let mut des = Deserializer::new(&s, 0);
    let user_inode_limit = des.u64()?;
    let user_space_limit = des.u64()?;
    let group_inode_limit = des.u64()?;
    let group_space_limit = des.u64()?;
    des.finish()?;

    let mut stmt = tx.prepare_cached(sql!(
        "INSERT INTO quota_default_limits
        (id_type, quota_type, pool_id, value)
        VALUES (?1, ?2, ?3, ?4)"
    ))?;

    if user_space_limit <= (i64::MAX as u64) {
        let affected = stmt.execute(params![
            QuotaIdType::User.sql_variant(),
            QuotaType::Space.sql_variant(),
            pool_id,
            user_space_limit
        ])?;
        check_affected_rows(affected, [1])?;
        println!(
            "NOTE: Treating very large (> 2^63 bytes) default user space limit on pool {pool_id} as unlimited"
        );
    }
    if user_inode_limit <= (i64::MAX as u64) {
        let affected = stmt.execute(params![
            QuotaIdType::User.sql_variant(),
            QuotaType::Inode.sql_variant(),
            pool_id,
            user_inode_limit
        ])?;
        check_affected_rows(affected, [1])?;
        println!(
            "NOTE: Treating very large (> 2^63 inodes) default user inode limit on pool {pool_id} as unlimited"
        );
    }
    if group_space_limit <= (i64::MAX as u64) {
        let affected = stmt.execute(params![
            QuotaIdType::Group.sql_variant(),
            QuotaType::Space.sql_variant(),
            pool_id,
            group_space_limit
        ])?;
        check_affected_rows(affected, [1])?;
        println!(
            "NOTE: Treating very large (> 2^63 bytes) default group space limit on pool {pool_id} as unlimited"
        );
    }
    if group_inode_limit <= (i64::MAX as u64) {
        let affected = stmt.execute(params![
            QuotaIdType::Group.sql_variant(),
            QuotaType::Inode.sql_variant(),
            pool_id,
            group_inode_limit
        ])?;
        check_affected_rows(affected, [1])?;
        println!(
            "NOTE: Treating very large (> 2^63 inodes) default group inode limit on pool {pool_id} as unlimited"
        );
    }

    Ok(())
}

/// Imports the specific (per user/group) quota limits
fn quota_limits(
    tx: &Transaction,
    f: &Path,
    pool_id: PoolId,
    quota_id_type: QuotaIdType,
) -> Result<()> {
    // If the file is missing, skip it
    let s = match std::fs::read(f) {
        Ok(s) => s,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            println!("WARNING: Ignoring missing quota limits file {f:?} for pool {pool_id}");
            return Ok(());
        }
        Err(err) => return Err(err.into()),
    };

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
        if l.space > 0 && l.space <= (i64::MAX as u64) {
            insert_stmt.execute(params![
                l.id,
                l.id_type.sql_variant(),
                QuotaType::Space.sql_variant(),
                pool_id,
                l.space
            ])?;
        } else {
            println!(
                "NOTE: Treating very large (> 2^63 bytes) {quota_id_type} space limit on pool {pool_id} as unlimited"
            );
        }

        if l.inodes > 0 && l.inodes <= (i64::MAX as u64) {
            insert_stmt.execute(params![
                l.id,
                l.id_type.sql_variant(),
                QuotaType::Inode.sql_variant(),
                pool_id,
                l.inodes
            ])?;
        } else {
            println!(
                "NOTE: Treating very large (> 2^63 bytes) {quota_id_type} inode limit on pool {pool_id} as unlimited"
            );
        }
    }

    Ok(())
}
