//! Functions for long-term public key management.
//!
//! These keys authenticate BeeMsg connections, see [`shared::crypto::handshake`]. Management owns
//! the authoritative list; nodes download it and keep it in a
//! [`shared::conn::key_store::KeyStore`].
use super::*;
use shared::types::StaticPubKey;

/// Retrieves all public keys together with the node they authenticate.
///
/// A key is attached to an *identity*, and an identity may cover several nodes, so the same key can
/// legitimately appear against more than one node uid here.
///
/// # Return value
/// A Vec containing (node Uid, StaticPubKey) entries.
pub(crate) fn get_all(tx: &Transaction) -> Result<Vec<(Uid, StaticPubKey)>> {
    tx.prepare_cached(sql!(
        "SELECT n.node_uid, k.key
        FROM keys AS k
        INNER JOIN identity_to_node AS idn USING(identity_id)
        INNER JOIN nodes AS n USING(node_type, node_id)
        ORDER BY n.node_uid ASC"
    ))?
    .query_and_then([], |row| {
        let key = row.get_ref(1)?.as_blob()?;

        Ok((row.get(0)?, StaticPubKey::try_from(key)?))
    })?
    .collect()
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::db::test::with_test_data;

    /// The test data set carries no keys yet - the list is maintained by hand for now - so this
    /// asserts the query itself is valid rather than a specific row count.
    #[test]
    fn get_all_is_empty_without_keys() {
        with_test_data(|tx| {
            assert_eq!(super::get_all(tx).unwrap(), vec![]);
        })
    }

    /// A key inserted against an identity that maps to a node must come back for that node.
    #[test]
    fn get_all_returns_inserted_key() {
        with_test_data(|tx| {
            let key = [7u8; 32];

            tx.execute("INSERT INTO identities (name) VALUES ('test-identity')", [])
                .unwrap();
            let identity_id = tx.last_insert_rowid();

            // 102001 is a storage node in the test data set.
            tx.execute(
                "INSERT INTO identity_to_node (identity_id, node_type, node_id)
                SELECT ?1, node_type, node_id FROM nodes WHERE node_uid = 102001",
                rusqlite::params![identity_id],
            )
            .unwrap();
            tx.execute(
                "INSERT INTO keys (key, identity_id) VALUES (?1, ?2)",
                rusqlite::params![key.as_slice(), identity_id],
            )
            .unwrap();

            let keys = super::get_all(tx).unwrap();
            assert_eq!(keys, vec![(102001, StaticPubKey::from(key))]);
        })
    }

    /// The schema must reject a key that is not exactly 32 bytes, so a mistyped hand-entered key
    /// fails at insert rather than at handshake time.
    #[test]
    fn wrong_length_key_is_rejected_by_schema() {
        with_test_data(|tx| {
            tx.execute("INSERT INTO identities (name) VALUES ('test-identity')", [])
                .unwrap();
            let identity_id = tx.last_insert_rowid();

            tx.execute(
                "INSERT INTO keys (key, identity_id) VALUES (?1, ?2)",
                rusqlite::params![[0u8; 31].as_slice(), identity_id],
            )
            .expect_err("a 31 byte key must be rejected");
        })
    }

    /// Two identities must not be able to claim the same key.
    #[test]
    fn duplicate_key_is_rejected_by_schema() {
        with_test_data(|tx| {
            let key = [9u8; 32];

            for name in ["identity-a", "identity-b"] {
                tx.execute(
                    "INSERT INTO identities (name) VALUES (?1)",
                    rusqlite::params![name],
                )
                .unwrap();
                let identity_id = tx.last_insert_rowid();

                let res = tx.execute(
                    "INSERT INTO keys (key, identity_id) VALUES (?1, ?2)",
                    rusqlite::params![key.as_slice(), identity_id],
                );

                if name == "identity-b" {
                    res.expect_err("the same key must not be reusable by another identity");
                } else {
                    res.unwrap();
                }
            }
        })
    }
}
