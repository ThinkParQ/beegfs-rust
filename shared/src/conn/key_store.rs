//! The list of long-term public keys this node accepts.
//!
//! Management owns the authoritative list (the `keys` table) and every node downloads it. This
//! store is the in-memory view used by the connection layer, filled by the owning daemon the same
//! way node addresses are (see [`super::store::Store::replace_node_addrs`]) - the `shared` crate
//! never reaches into a database itself.

use crate::types::{StaticPubKey, Uid};
use std::collections::HashMap;
use std::sync::RwLock;

/// Maps between nodes and their long-term public keys, in both directions.
#[derive(Debug, Default)]
pub struct KeyStore {
    /// Incoming direction: is this key allowed, and which node does it belong to?
    ///
    /// A miss here is a rejected connection - this lookup *is* the authentication decision, which
    /// is what makes identifying peers by their raw public key rather than by a numeric id
    /// worthwhile.
    by_key: RwLock<HashMap<StaticPubKey, Uid>>,
    /// Outgoing direction: which key should we expect from the node we are dialing?
    by_uid: RwLock<HashMap<Uid, StaticPubKey>>,
}

impl KeyStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Replaces the **entire** key list.
    ///
    /// Wholesale replacement rather than incremental updates, so a revoked key cannot linger: the
    /// new list is exactly what management currently has.
    pub fn replace_all(&self, keys: impl IntoIterator<Item = (Uid, StaticPubKey)>) {
        let mut by_key = HashMap::new();
        let mut by_uid = HashMap::new();

        for (uid, key) in keys {
            // A key mapped to two different nodes would make the authenticated identity ambiguous.
            // The `UNIQUE` constraint on `keys.key` in the management schema prevents this at the
            // source; complain loudly if it ever gets through anyway.
            if let Some(previous) = by_key.insert(key, uid)
                && previous != uid
            {
                log::error!(
                    "Public key {key} is assigned to more than one node (uid {previous} and \
                     {uid}) - refusing to use it for authentication"
                );
                by_key.remove(&key);
            }

            by_uid.insert(uid, key);
        }

        *self.by_key.write().unwrap() = by_key;
        *self.by_uid.write().unwrap() = by_uid;
    }

    /// Sets the key for a single node, leaving the rest of the list untouched.
    pub fn insert(&self, uid: Uid, key: StaticPubKey) {
        self.by_key.write().unwrap().insert(key, uid);
        self.by_uid.write().unwrap().insert(uid, key);
    }

    /// Looks up the node a public key belongs to.
    ///
    /// [`None`] means the key is not in the list and the peer must be rejected.
    pub fn node_by_key(&self, key: &StaticPubKey) -> Option<Uid> {
        self.by_key.read().unwrap().get(key).copied()
    }

    /// The public key expected from the given node, for outgoing connections.
    pub fn key_by_node(&self, uid: Uid) -> Option<StaticPubKey> {
        self.by_uid.read().unwrap().get(&uid).copied()
    }

    pub fn is_empty(&self) -> bool {
        self.by_key.read().unwrap().is_empty()
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn lookup_both_directions() {
        let store = KeyStore::new();
        let key = StaticPubKey::from([7u8; 32]);

        store.replace_all([(42, key)]);

        assert_eq!(store.node_by_key(&key), Some(42));
        assert_eq!(store.key_by_node(42), Some(key));
        assert_eq!(store.node_by_key(&StaticPubKey::from([8u8; 32])), None);
        assert_eq!(store.key_by_node(43), None);
    }

    /// A replacement must drop keys that are no longer in the list, otherwise a revoked key would
    /// keep working until restart.
    #[test]
    fn replace_all_drops_removed_keys() {
        let store = KeyStore::new();
        let old = StaticPubKey::from([1u8; 32]);
        let new = StaticPubKey::from([2u8; 32]);

        store.replace_all([(1, old)]);
        store.replace_all([(1, new)]);

        assert_eq!(store.node_by_key(&old), None, "revoked key still accepted");
        assert_eq!(store.node_by_key(&new), Some(1));
    }

    /// One key claimed by two nodes is ambiguous, so it must not authenticate either of them.
    #[test]
    fn duplicate_key_is_rejected() {
        let store = KeyStore::new();
        let key = StaticPubKey::from([3u8; 32]);

        store.replace_all([(1, key), (2, key)]);

        assert_eq!(store.node_by_key(&key), None);
    }
}
