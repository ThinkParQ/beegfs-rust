use super::*;

#[derive(Clone, Debug)]
pub struct StoragePool {
    pub pool_id: StoragePoolID,
    pub alias: EntityAlias,
}

pub fn all(tx: &mut Transaction) -> Result<Vec<StoragePool>> {
    let mut stmt = tx.prepare_cached(
        r#"
        SELECT p.pool_id, alias FROM storage_pools AS p
        INNER JOIN entities AS e ON e.uid = p.pool_uid
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

pub fn insert(
    tx: &mut Transaction,
    id: Option<StoragePoolID>,
    alias: &EntityAlias,
) -> Result<StoragePoolID> {
    let pool_id = if let Some(pool_id) = id {
        pool_id
    } else {
        misc::find_new_id(tx, "storage_pools", "pool_id", 1..=0xFFFF)?.into()
    };

    let mut stmt = tx.prepare_cached(
        r#"
        INSERT INTO entities (entity_type, alias) VALUES ("storage_pool", ?1)
        "#,
    )?;

    stmt.execute(params![alias])?;

    let new_uid: NodeUID = tx.last_insert_rowid().into();

    tx.execute(
        r#"
        INSERT INTO storage_pools (pool_id, pool_uid) VALUES (?1, ?2);
        "#,
        params![pool_id, new_uid],
    )?;

    Ok(pool_id)
}

pub fn update_alias(
    tx: &mut Transaction,
    pool_id: StoragePoolID,
    new_alias: &EntityAlias,
) -> Result<()> {
    let affected = tx.execute(
        r#"
        UPDATE entities SET alias = ?1
        WHERE uid = (
            SELECT pool_uid FROM storage_pools WHERE pool_id = ?2
        )
        "#,
        params![new_alias, pool_id],
    )?;

    ensure_rows_modified!(affected, pool_id);

    Ok(())
}

pub fn delete(tx: &mut Transaction, pool_id: StoragePoolID) -> Result<()> {
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

#[cfg(test)]
mod test {
    use crate::db::test::with_test_data;

    #[test]
    fn set_get() {
        with_test_data(|tx| {
            assert_eq!(4, super::all(tx).unwrap().len());
            super::insert(tx, None, &"new_pool".into()).unwrap();
            assert_eq!(5, super::all(tx).unwrap().len());
            super::insert(tx, None, &"new_pool".into()).unwrap_err();
            super::insert(tx, Some(1.into()), &"new_pool2".into()).unwrap_err();

            super::update_alias(tx, 2.into(), &"new_pool".into()).unwrap_err();
            super::update_alias(tx, 2.into(), &"new_pool3".into()).unwrap();

            super::delete(tx, 2.into()).unwrap();
            super::delete(tx, 2.into()).unwrap_err();
            assert_eq!(4, super::all(tx).unwrap().len());
        })
    }
}
