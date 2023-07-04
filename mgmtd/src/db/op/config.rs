use super::*;
pub use crate::config::*;

pub(crate) fn set_one<T: Field>(tx: &mut Transaction, value: &T::Value) -> DbResult<()> {
    let json = serde_json::to_string(&value)?;

    tx.execute_cached(
        r#"
        INSERT INTO config (key, value) VALUES (?1, ?2)
        ON CONFLICT (key) DO
        UPDATE SET value = ?2
        "#,
        [T::KEY, &json],
    )?;

    Ok(())
}

pub(crate) fn get_one<T: Field>(tx: &mut Transaction) -> DbResult<T::Value> {
    let json: Option<String> = tx
        .query_row_cached("SELECT value FROM config WHERE key = ?", [T::KEY], |row| {
            row.get(0)
        })
        .optional()?;

    let value = match json {
        Some(json) => serde_json::from_str(&json).unwrap(),
        None => T::default(),
    };

    Ok(value)
}

pub(crate) fn get_all(tx: &mut Transaction) -> DbResult<DynamicConfig> {
    let mut stmt = tx.prepare_cached("SELECT key, value FROM config")?;
    let mut rows = stmt.query([])?;

    let mut config = DynamicConfig::default();

    while let Some(row) = rows.next()? {
        // TODO proper error
        let key = row.get_ref(0)?.as_str().map_err(DbError::other)?;
        let ser_value = row.get_ref(1)?.as_str().map_err(DbError::other)?;

        config.set_by_json_str(key, ser_value)?;
    }

    Ok(config)
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn set_get() {
        with_test_data(|tx| {
            super::set_one::<quota_enable>(tx, &true).unwrap();

            let value = super::get_one::<quota_enable>(tx).unwrap();
            assert!(value);

            let config = super::get_all(tx).unwrap();
            assert!(config.quota_enable);

            super::set_one::<quota_enable>(tx, &false).unwrap();

            let value = super::get_one::<quota_enable>(tx).unwrap();
            assert!(!value);

            let config = super::get_all(tx).unwrap();
            assert!(!config.quota_enable);
        })
    }
}
