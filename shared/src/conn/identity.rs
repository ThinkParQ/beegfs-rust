//! The list of identities allowed to open an authenticated BeeMsg connection.
//!
//! Filled from the management database. How the keys get there is out of scope - they are
//! pre-shared, as the KK pattern requires.

use crate::types::{StaticPubKey, Uid};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

/// A principal allowed to connect. Usually a node, but not necessarily - beegfs-ctl has no node
/// entry, for example.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Identity {
    pub name: Arc<str>,
    pub node_uid: Option<Uid>,
}

/// Maps public keys to identities for the incoming side and node uids to public keys for the
/// outgoing side.
#[derive(Debug, Default)]
pub struct IdentityStore {
    by_key: RwLock<HashMap<StaticPubKey, Identity>>,
    by_node: RwLock<HashMap<Uid, StaticPubKey>>,
}

impl IdentityStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Replaces the whole list.
    ///
    /// Wholesale rather than incremental so a revoked key cannot linger. `keys` must be ordered
    /// oldest first per node: an identity may hold several keys so one can be rotated without a
    /// window in which the peer cannot connect, and the last one wins for the outgoing direction.
    pub fn replace_all(&self, keys: impl IntoIterator<Item = (StaticPubKey, Identity)>) {
        let mut by_key = HashMap::new();
        let mut by_node = HashMap::new();

        for (key, identity) in keys {
            if let Some(node_uid) = identity.node_uid {
                by_node.insert(node_uid, key);
            }

            if let Some(clash) = by_key.insert(key, identity) {
                // The key is the identity selector, so a duplicate would make the authenticated
                // principal ambiguous. The database rejects this, so it means someone bypassed it.
                log::error!(
                    "Public key {key} is registered for more than one identity, ignoring {}",
                    clash.name
                );
            }
        }

        *self.by_key.write().expect("lock is not poisoned") = by_key;
        *self.by_node.write().expect("lock is not poisoned") = by_node;
    }

    /// Looks up an incoming peer. A miss *is* the rejection.
    pub fn identity_by_key(&self, key: &StaticPubKey) -> Option<Identity> {
        self.by_key
            .read()
            .expect("lock is not poisoned")
            .get(key)
            .cloned()
    }

    /// The key to expect from a node we connect to.
    ///
    /// KK commits the initiator to one remote static before the handshake starts, so there is no
    /// retry across a nodes keys. Rotation therefore has an order: register the new key with
    /// management first, then swap the key file on the node.
    pub fn key_by_node(&self, node_uid: Uid) -> Option<StaticPubKey> {
        self.by_node
            .read()
            .expect("lock is not poisoned")
            .get(&node_uid)
            .copied()
    }

    pub fn is_empty(&self) -> bool {
        self.by_key.read().expect("lock is not poisoned").is_empty()
    }
}

#[cfg(test)]
mod test {
    use super::*;

    fn key(byte: u8) -> StaticPubKey {
        [byte; StaticPubKey::LEN].into()
    }

    fn identity(name: &str, node_uid: Option<Uid>) -> Identity {
        Identity {
            name: name.into(),
            node_uid,
        }
    }

    #[test]
    fn lookup_both_directions() {
        let store = IdentityStore::new();
        assert!(store.is_empty());

        store.replace_all([
            (key(1), identity("meta1", Some(10))),
            (key(2), identity("ctl", None)),
        ]);

        assert_eq!(
            store.identity_by_key(&key(1)).unwrap().name.as_ref(),
            "meta1"
        );
        assert_eq!(store.key_by_node(10), Some(key(1)));

        // An identity without a node can still connect to us, but we never connect to it.
        assert_eq!(store.identity_by_key(&key(2)).unwrap().name.as_ref(), "ctl");
        assert!(!store.is_empty());

        assert!(store.identity_by_key(&key(3)).is_none());
        assert!(store.key_by_node(11).is_none());
    }

    /// The newest key of a node is the one we present, but every registered key still gets us in.
    #[test]
    fn rotation_keeps_the_old_key_valid() {
        let store = IdentityStore::new();
        store.replace_all([
            (key(1), identity("meta1", Some(10))),
            (key(2), identity("meta1", Some(10))),
        ]);

        assert_eq!(store.key_by_node(10), Some(key(2)));
        assert!(store.identity_by_key(&key(1)).is_some());
        assert!(store.identity_by_key(&key(2)).is_some());
    }

    /// A revoked key must not survive a reload.
    #[test]
    fn replace_all_drops_removed_keys() {
        let store = IdentityStore::new();
        store.replace_all([(key(1), identity("meta1", Some(10)))]);
        store.replace_all([(key(2), identity("meta2", Some(11)))]);

        assert!(store.identity_by_key(&key(1)).is_none());
        assert!(store.key_by_node(10).is_none());
        assert!(store.identity_by_key(&key(2)).is_some());
    }
}
