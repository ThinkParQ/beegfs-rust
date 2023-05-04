use super::*;

#[derive(Clone, Debug)]
#[allow(dead_code)]
pub(crate) struct StoragePool {
    pub pool_id: StoragePoolID,
    pub alias: EntityAlias,
}

pub(crate) fn all(tx: &mut Transaction) -> Result<Vec<StoragePool>> {
    let mut stmt = tx.prepare_cached(
        r#"
        SELECT pool_id, alias FROM storage_pools
        "#,
    )?;

    let res = stmt
        .query_map([], |row| {
            Ok(StoragePool {
                pool_id: row.get(0)?,
                alias: row.get(1)?,
            })
        })?
        .try_collect()?;

    Ok(res)
}

pub(crate) fn insert(
    tx: &mut Transaction,
    id: Option<StoragePoolID>,
    alias: &EntityAlias,
) -> Result<StoragePoolID> {
    let pool_id = if let Some(pool_id) = id {
        pool_id
    } else {
        misc::find_new_id(tx, "storage_pools", "pool_id", 1..=0xFFFF)?.into()
    };

    tx.execute(
        r#"
        INSERT INTO storage_pools (pool_id, alias) VALUES (?1, ?2);
        "#,
        params![pool_id, alias],
    )?;

    Ok(pool_id)
}

pub(crate) fn update_alias(
    tx: &mut Transaction,
    pool_id: StoragePoolID,
    new_alias: &EntityAlias,
) -> Result<()> {
    let affected = tx.execute(
        r#"
        UPDATE storage_pools SET alias = ?1 WHERE pool_id = ?2
        "#,
        params![new_alias, pool_id],
    )?;

    ensure_rows_modified!(affected, pool_id);

    Ok(())
}

pub(crate) fn delete(tx: &mut Transaction, pool_id: StoragePoolID) -> Result<()> {
    // move targets back to default pool
    tx.execute(
        r#"
        UPDATE storage_targets SET pool_id = 1 WHERE pool_id = ?1
        "#,
        params![pool_id],
    )?;

    let affected = tx.execute(
        r#"
        DELETE FROM storage_pools WHERE pool_id = ?1
        "#,
        params![pool_id],
    )?;

    ensure_rows_modified!(affected, pool_id);

    Ok(())
}
