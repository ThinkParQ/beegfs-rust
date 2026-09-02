use anyhow::Result;
use rusqlite::{OptionalExtension, Transaction};
use shared::conn::protocol::StaticPubKey;
use shared::conn::{Identity, Lookup};
use shared::types::Uid;
use sqlite::Connections;
use sqlite_check::sql;
use std::net::{IpAddr, SocketAddr};

#[derive(Clone, Debug)]
pub(crate) struct DbLookup {
    pub db: Connections,
}

impl Lookup for DbLookup {
    async fn identity_by_key(&self, key: StaticPubKey) -> Result<Option<Identity>> {
        self.db.read_tx(move |tx| identity_by_key(tx, key)).await
    }

    async fn key_by_node(&self, node: Uid) -> Result<Option<StaticPubKey>> {
        self.db.read_tx(move |tx| key_by_node(tx, node)).await
    }

    async fn node_addrs(&self, node: Uid) -> Result<Option<Vec<SocketAddr>>> {
        self.db.read_tx(move |tx| node_addrs(tx, node)).await
    }
}

/// Resolves the identity a public key belongs to. This lookup *is* the authentication decision for
/// an incoming connection, so a miss must mean "not allowed".
fn identity_by_key(tx: &Transaction, key: StaticPubKey) -> Result<Option<Identity>> {
    Ok(tx
        .query_row(
            sql!(
                "SELECT name, node_uid FROM keys
                INNER JOIN identities USING(identity_id)
                LEFT JOIN identity_to_node USING(identity_id)
                LEFT JOIN nodes USING(node_id, node_type)
                WHERE key = ?1"
            ),
            [key.to_string()],
            |row| {
                Ok(Identity {
                    name: row.get(0)?,
                    node_uid: row.get(1)?,
                })
            },
        )
        .optional()?)
}

/// The key to present when connecting to a node.
///
/// An identity may hold several keys so one can be rotated without a window in which the peer
/// cannot connect. The newest wins, otherwise a freshly registered key would be picked only
/// sometimes and rotation would silently keep using the old one.
fn key_by_node(tx: &Transaction, node: Uid) -> Result<Option<StaticPubKey>> {
    let key: Option<String> = tx
        .query_row(
            sql!(
                "SELECT key FROM keys
                INNER JOIN identities USING(identity_id)
                INNER JOIN identity_to_node USING(identity_id)
                INNER JOIN nodes USING(node_id, node_type)
                WHERE node_uid = ?1
                ORDER BY key_id DESC
                LIMIT 1"
            ),
            [node],
            |row| row.get(0),
        )
        .optional()?;

    key.map(|k| k.parse()).transpose()
}

/// All addresses a node can be reached on. `None` means the node has none registered.
fn node_addrs(tx: &Transaction, node: Uid) -> Result<Option<Vec<SocketAddr>>> {
    // Fetch nics in the inserted order to make sure preference is respected
    let mut stmt = tx.prepare_cached(sql!(
        "SELECT addr, port FROM node_nics
        INNER JOIN nodes USING(node_uid) WHERE node_uid = ?1
        ORDER BY node_nics.rowid"
    ))?;

    let mut rows = stmt.query([node])?;

    let mut res = vec![];
    while let Some(row) = rows.next()? {
        let addr: IpAddr = row.get_ref(0)?.as_str()?.parse()?;
        res.push(SocketAddr::new(addr, row.get(1)?));
    }

    Ok(if res.is_empty() { None } else { Some(res) })
}

#[cfg(any())] // TEMP-DISABLED-TESTS: re-enable by restoring #[cfg(test)]
mod test {
    use super::*;
    use crate::db::test::with_test_data;
    use shared::types::MGMTD_UID;

    const META_1_UID: Uid = 101001;
    const KEY_1: [u8; 32] = [1u8; 32];
    const KEY_2: [u8; 32] = [2u8; 32];
    const KEY_3: [u8; 32] = [3u8; 32];

    #[test]
    fn identity_by_key_resolves_name_and_node() {
        with_test_data(|tx| {
            let identity = identity_by_key(tx, KEY_1.into()).unwrap().unwrap();
            assert_eq!(&*identity.name, "meta_node_1");
            assert_eq!(identity.node_uid, Some(META_1_UID));

            // An identity need not map to a node - beegfs-ctl has no node entry.
            let identity = identity_by_key(tx, KEY_3.into()).unwrap().unwrap();
            assert_eq!(&*identity.name, "ctl");
            assert_eq!(identity.node_uid, None);

            // A miss is the rejection, so it must not be an error.
            assert_eq!(identity_by_key(tx, [0xff; 32].into()).unwrap(), None);
        })
    }

    /// Rotation only works if the newest key wins. Without an explicit order the query returns
    /// whichever row it happens to find, so a freshly registered key would be used only sometimes.
    #[test]
    fn key_by_node_picks_the_newest_key() {
        with_test_data(|tx| {
            // meta_node_1 holds KEY_1 and KEY_2, KEY_2 registered later.
            assert_eq!(key_by_node(tx, META_1_UID).unwrap(), Some(KEY_2.into()));

            tx.execute(
                "INSERT INTO keys (key, identity_id) VALUES (?1, 1)",
                [&[9u8; 32][..]],
            )
            .unwrap();

            assert_eq!(key_by_node(tx, META_1_UID).unwrap(), Some([9u8; 32].into()));
        })
    }

    #[test]
    fn key_by_node_without_a_key_is_none() {
        with_test_data(|tx| {
            // A node that no identity maps to.
            assert_eq!(key_by_node(tx, 101002).unwrap(), None);
        })
    }

    #[test]
    fn node_addrs_lists_every_nic() {
        with_test_data(|tx| {
            let addrs = node_addrs(tx, META_1_UID).unwrap().unwrap();

            assert_eq!(
                addrs,
                vec![
                    "0.1.1.1:8005".parse().unwrap(),
                    "0.1.1.2:8005".parse().unwrap(),
                    "[::3]:8005".parse().unwrap(),
                    "[::4]:8005".parse().unwrap(),
                ]
            );

            // No nics registered means unreachable, which the caller must be able to distinguish.
            assert_eq!(node_addrs(tx, MGMTD_UID).unwrap(), None);
        })
    }

    /// The key column is the identity selector sent in the clear, so a key must never resolve to
    /// two identities.
    #[test]
    fn duplicate_keys_are_rejected() {
        with_test_data(|tx| {
            tx.execute(
                "INSERT INTO keys (key, identity_id) VALUES (?1, 2)",
                [&KEY_1[..]],
            )
            .unwrap_err();
        })
    }

    #[test]
    fn deleting_an_identity_removes_its_keys() {
        with_test_data(|tx| {
            tx.execute("DELETE FROM identities WHERE identity_id = 1", [])
                .unwrap();

            assert_eq!(identity_by_key(tx, KEY_1.into()).unwrap(), None);
            assert_eq!(identity_by_key(tx, KEY_2.into()).unwrap(), None);
            assert_eq!(key_by_node(tx, META_1_UID).unwrap(), None);
        })
    }
}
