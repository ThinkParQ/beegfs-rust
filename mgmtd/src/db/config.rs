use super::*;
use ::config::ConfigMap;

pub(crate) fn set(tx: &mut Transaction, entries: ConfigMap) -> Result<()> {
    let mut stmt = tx.prepare_cached(
        r#"
        INSERT INTO config (key, value) VALUES (?1, ?2)
        ON CONFLICT (key) DO
        UPDATE SET value = ?2
        "#,
    )?;

    for e in entries {
        stmt.execute(params![e.0, e.1,])?;
    }

    Ok(())
}

pub(crate) fn get(tx: &mut Transaction) -> Result<ConfigMap> {
    let mut stmt = tx.prepare_cached(
        r#"
        SELECT key, value FROM config
        "#,
    )?;

    let map: ConfigMap = stmt
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
        .try_collect()?;

    Ok(map)
}

#[cfg(test)]
mod test {
    use super::*;
    use tests::with_test_data;

    #[test]
    fn set_get() {
        with_test_data(|tx| {
            super::set(
                tx,
                [("test_key_1".to_string(), "test_value_1".to_string())]
                    .into_iter()
                    .collect(),
            )
            .unwrap();

            let map = super::get(tx).unwrap();

            assert_eq!(5, map.len());
            assert_eq!("test_value_1", map["test_key_1"]);

            super::set(
                tx,
                [("test_key_1".to_string(), "test_value_2".to_string())]
                    .into_iter()
                    .collect(),
            )
            .unwrap();

            let map = super::get(tx).unwrap();

            assert_eq!("test_value_2", map["test_key_1"]);
        })
    }
}
