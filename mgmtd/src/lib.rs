//! The BeeGFS management service

mod app;
mod bee_msg;
mod cap_pool;
pub mod config;
pub mod db;
mod error;
mod grpc;
pub mod license;
mod quota;
mod timer;
mod types;

use crate::app::RuntimeApp;
use crate::config::Config;
use anyhow::{Context, Result};
use app::App;
use db::config::Config as dbConfig;
use db::node_nic::ReplaceNic;
use license::LicenseVerifier;
use protobuf::license::CertType;
use rusqlite::Transaction;
use shared::bee_msg::target::RefreshTargetStates;
use shared::conn::identity::{Identity, IdentityStore};
use shared::conn::noise::StaticKeypair;
use shared::conn::outgoing::Pool;
use shared::conn::{ConnConfig, incoming};
use shared::nic::Nic;
use shared::protocol::Protocol;
use shared::run_state::{self, RunStateControl};
use shared::types::{AuthSecret, MGMTD_UID, NicType, NodeId, NodeType, StaticPubKey, Uid};
use sqlite::TransactionExt;
use sqlite_check::sql;
use std::collections::HashSet;
use std::future::Future;
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::Arc;
use tokio::net::UdpSocket;
use tokio::sync::mpsc;
use tokio::time::Instant;
use types::SqliteEnumExt;

/// Contains information that is obtained at the start of the app and then never changes again.
#[derive(Debug)]
pub struct StaticInfo {
    pub user_config: Config,
    pub auth_secret: Option<AuthSecret>,
    pub network_addrs: Vec<Nic>,
    pub use_ipv6: bool,
    pub protocol: Protocol,
    pub beemsg_keypair: Option<Arc<StaticKeypair>>,
}

/// Starts the management service.
///
/// Opens the necessary connections and starts all the tasks that provide the functionality of this
/// program. This is supposed to be called by the binary in main.rs, but could also be run in a
/// testing context without involving a standalone binary.
///
/// # Return behavior
/// Returns after all setup work is done and all tasks are started. The caller is responsible for
/// keeping the shutdown control handle and send a shutdown request when the program shall
/// be terminated.
pub async fn start(info: StaticInfo, license: LicenseVerifier) -> Result<RunControl> {
    // Initialization

    let (run_state, run_state_control) = run_state::new();

    // Static configuration which doesn't change at runtime
    let info = Box::leak(Box::new(info));

    let beemsg_serve_addr = SocketAddr::new(
        if info.use_ipv6 {
            Ipv6Addr::UNSPECIFIED.into()
        } else {
            Ipv4Addr::UNSPECIFIED.into()
        },
        info.user_config.beemsg_port,
    );

    // UDP socket for in- and outgoing messages
    let udp_socket = Arc::new(UdpSocket::bind(beemsg_serve_addr).await?);

    let identities = Arc::new(IdentityStore::new());

    // Shared by the incoming and outgoing side so both cannot disagree on the protocol. The legacy
    // secret only applies to the legacy protocol, the key exchange replaces it everywhere else.
    let conn_cfg = Arc::new(ConnConfig {
        protocol: info.protocol,
        legacy_auth_required: info.protocol.is_legacy() && info.auth_secret.is_some(),
        auth_secret: info
            .protocol
            .is_legacy()
            .then_some(info.auth_secret)
            .flatten(),
        keypair: info.beemsg_keypair.clone(),
        identities: identities.clone(),
    });
    conn_cfg.check()?;

    // Node address store and connection pool
    let conn_pool = Pool::new(
        udp_socket.clone(),
        info.user_config.connection_limit,
        conn_cfg.clone(),
        info.use_ipv6,
    );

    let db = sqlite::Connections::new(info.user_config.db_file.as_path());

    let need_migration = db
        .read_tx(|tx| Ok(sqlite::check_schema(tx, db::MIGRATIONS)))
        .await??;

    if need_migration {
        migrate_db_schema(&db).await?;
    }

    log::info!(
        "Opened database at {:?}",
        info.user_config.db_file.as_path()
    );

    db.write_tx(|tx| {
        // Update management node entry in db
        db::node::update(tx, MGMTD_UID, info.user_config.beemsg_port, None)?;

        // Update management nics entry in db
        db::node_nic::replace(
            tx,
            MGMTD_UID,
            info.network_addrs.iter().map(|e| ReplaceNic {
                nic_type: NicType::Tcp,
                addr: &e.address,
                name: e.name.as_str().into(),
            }),
        )
    })
    .await?;

    let prev_trial_serial: Option<String> = db
        .read_tx(|tx| db::config::get(tx, db::config::Config::TrialSerial))
        .await?;

    // Load and verify license certificate
    match license
        .load_and_verify_license_cert(
            &info.user_config.license_cert_file,
            prev_trial_serial.as_deref(),
        )
        .await
    {
        Ok(serial) => {
            if license
                .get_license_cert_data()?
                .data
                .is_some_and(|d| d.r#type() == CertType::Trial)
                && prev_trial_serial.is_none()
            {
                db.write_tx(|tx| db::config::set(tx, dbConfig::TrialSerial, serial))
                    .await?;
            }
        }
        Err(err) => log::warn!(
            "Loading and verifying license certificate failed. \
                Licensed features will be unavailable: {err}"
        ),
    };

    // Fill node addrs store from db
    db.read_tx(db::node_nic::get_all_addrs)
        .await?
        .into_iter()
        .for_each(|a| conn_pool.replace_node_addrs(a.0, a.1));

    let keys = db.read_tx(beemsg_identities).await?;

    if info.protocol.needs_handshake() && keys.is_empty() {
        log::warn!(
            "The BeeMsg identity list is empty - no peer can connect until keys are registered"
        );
    }

    identities.replace_all(keys);

    // This is used to signal a client that pulled its state back to the RunControl
    let (shutdown_client_tx, shutdown_client_rx) = mpsc::channel(16);

    // Combines all handles for sharing between tasks
    let app = RuntimeApp::new(
        conn_pool,
        db,
        license,
        info,
        run_state.clone_weak(),
        shutdown_client_tx,
    );

    // Listen for incoming TCP connections
    // Fall back to ipv4 socket if ipv6 is not available
    incoming::listen_tcp(
        beemsg_serve_addr,
        app.clone(),
        conn_cfg.clone(),
        run_state.clone(),
    )
    .await?;

    // Recv UDP datagrams
    incoming::recv_udp(udp_socket, app.clone(), run_state.clone())?;

    // Run the timers
    timer::start_tasks(app.clone(), run_state.clone());

    // Start gRPC service
    grpc::serve(app.clone(), run_state)?;

    Ok(RunControl {
        app: app.clone(),
        run_state_control,
        shutdown_client_rx,
    })
}

/// Reads the BeeMsg identity list.
///
/// Ordered by `key_id` so the newest key of a node wins for the outgoing direction in
/// [`IdentityStore::replace_all`]. An identity without a node can connect to us but is never
/// connected to, hence the outer join.
///
/// The `keys` table is not restricted to X25519 keys - it is meant to hold other credential
/// material later as well - so anything that is not one is skipped instead of failing the whole
/// load and locking every peer out.
fn beemsg_identities(tx: &Transaction) -> Result<Vec<(StaticPubKey, Identity)>> {
    let rows: Vec<(Vec<u8>, String, Option<Uid>)> = tx.query_map_collect(
        sql!(
            "SELECT k.key, i.name, n.node_uid
            FROM keys AS k
            INNER JOIN identities AS i USING(identity_id)
            LEFT JOIN identity_to_node AS idn USING(identity_id)
            LEFT JOIN nodes AS n USING(node_type, node_id)
            ORDER BY k.key_id ASC"
        ),
        [],
        |row| {
            Ok((
                row.get_ref(0)?.as_blob()?.to_vec(),
                row.get(1)?,
                row.get(2)?,
            ))
        },
    )?;

    Ok(rows
        .into_iter()
        .filter_map(
            |(key, name, node_uid)| match StaticPubKey::try_from(key.as_slice()) {
                Ok(key) => Some((
                    key,
                    Identity {
                        name: name.into(),
                        node_uid,
                    },
                )),
                Err(err) => {
                    log::debug!("Ignoring a credential of identity {name} for BeeMsg: {err:#}");
                    None
                }
            },
        )
        .collect())
}

/// Db schema migration
async fn migrate_db_schema(db: &sqlite::Connections) -> Result<()> {
    log::warn!("The database needs to be migrated. Applying migrations...");

    db.conn(|conn| {
        let backup_file = sqlite::backup_db(conn)?;
        log::warn!("Old database backed up to {backup_file:?}");
        Ok(())
    })
    .await?;

    let version = db
        .write_tx(|tx| {
            sqlite::migrate_schema(tx, db::MIGRATIONS)
                .with_context(|| "Migrating database schema failed")
        })
        .await?;

    log::warn!("Database automatically migrated to version {version}");
    Ok(())
}

/// Controls the running application.
#[derive(Debug)]
pub struct RunControl {
    app: RuntimeApp,
    run_state_control: RunStateControl,
    shutdown_client_rx: mpsc::Receiver<ClientPulledStateNotification>,
}

/// Represents a client having pulled meta or storage states. Clients pull them separately, and to
/// skip the wait on shutdown we need to be sure that both have been received.
pub type ClientPulledStateNotification = (NodeType, NodeId);

impl RunControl {
    /// Waits for the provided future to complete before initiating shutdown. Completes after
    /// shutdown is done.
    pub async fn wait_for_shutdown<F, R>(mut self, shutdown_signal: F)
    where
        F: Fn() -> R,
        R: Future,
    {
        log::warn!("Waiting for shutdown signal ...");
        shutdown_signal().await;
        log::warn!("Received shutdown signal");

        // Set pre shutdown state to freeze the relevant system state - message handlers that do
        // modify e.g. target states should now deny change.
        self.run_state_control.pre_shutdown();

        let client_list: HashSet<ClientPulledStateNotification> = self
            .app
            .db
            .read_tx(move |tx| {
                let buddy_groups: i64 =
                    tx.query_row(sql!("SELECT COUNT(*) FROM buddy_groups"), [], |row| {
                        row.get(0)
                    })?;

                if buddy_groups == 0 {
                    return Ok(HashSet::new());
                }

                // Build the client list as a cartesian product `client_id x node_type` as each
                // client updates its state separately for meta and storage and we
                // have to wait until both have been pulled.
                let clients = tx.query_map_collect(
                    sql!(
                        "SELECT n.node_id, t.node_type FROM client_nodes AS n
                        CROSS JOIN node_types AS t
                        WHERE t.name IN ('meta', 'storage')"
                    ),
                    [],
                    |row| Ok((NodeType::from_row(row, 1)?, row.get(0)?)),
                )?;

                Ok(clients)
            })
            .await
            .unwrap_or_default();

        // We only need to wait in pre shutdown if there are clients mounted AND buddy groups exist
        // in the system. Otherwise, nothing bad can happen.
        if !client_list.is_empty() {
            log::warn!(
                "Buddy groups are in use and clients are registered - \
                waiting for all clients to pull state (timeout after {:?}) ...",
                self.app.info.user_config.node_offline_timeout
            );

            // Let the nodes pull the new states as soon as possible
            self.app
                .send_notifications(
                    &[NodeType::Client, NodeType::Meta, NodeType::Storage],
                    &RefreshTargetStates {
                        ack_id: b"".to_vec(),
                    },
                )
                .await;

            tokio::select! {
                // Wait for all clients having downloaded the state
                _ = self.wait_for_clients(client_list) => {}
                // or wait for another shutdown signal
                _ = shutdown_signal() => {}
            }
        }

        log::warn!("Waiting for all tasks to complete ... ");

        tokio::select! {
            // Wait for all tasks dropping the RunState handles
            _ = self.run_state_control.shutdown() => {
                log::warn!("Shutdown completed");
            }
            // or wait for another shutdown signal
            _ = shutdown_signal() => {
                log::warn!("Shutdown forced");
            }
        }
    }

    /// Waits until every client in `client_list` has been received to the `self.shutdown_client_`
    async fn wait_for_clients(&mut self, mut client_list: HashSet<ClientPulledStateNotification>) {
        let deadline = Instant::now() + self.app.info.user_config.node_offline_timeout;

        let receive_client_ids = async {
            while let Some(client_id) = self.shutdown_client_rx.recv().await {
                client_list.remove(&client_id);

                if client_list.is_empty() {
                    break;
                }
            }
        };

        tokio::select! {
            // Wait for all clients having downloaded the state
            _ = receive_client_ids => {}
            // or wait for the deadline
            _ = tokio::time::sleep_until(deadline) => {}
        }

        // If the receive loop exited due to error (e.g. all senders being dropped), we just wait
        if !client_list.is_empty() {
            tokio::time::sleep_until(deadline).await;
        }
    }
}

/// Constructs a version str from the `VERSION` environment variable at compile time
pub const fn version_str() -> &'static str {
    match option_env!("VERSION") {
        Some(version) => version,
        None => "undefined",
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use db::test::with_test_data;

    #[test]
    fn read_beemsg_identities() {
        with_test_data(|tx| {
            let keys = beemsg_identities(tx).unwrap();
            assert_eq!(keys.len(), 3);

            // Ordered by key_id, so the newest key of a node comes last and wins.
            assert_eq!(keys[0].0, StaticPubKey::from([1; 32]));
            assert_eq!(keys[1].0, StaticPubKey::from([2; 32]));
            assert_eq!(keys[1].1.name.as_ref(), "meta_node_1");
            assert_eq!(keys[0].1.node_uid, keys[1].1.node_uid);
            assert!(keys[0].1.node_uid.is_some());

            // An identity without a node still gets in, it just cannot be connected to.
            assert_eq!(keys[2].1.name.as_ref(), "ctl");
            assert_eq!(keys[2].1.node_uid, None);
        });
    }

    /// Uniqueness keeps the authenticated principal unambiguous, and a credential of an identity
    /// that does not exist has nothing to authenticate as.
    #[test]
    fn keys_table_rejects_bad_rows() {
        with_test_data(|tx| {
            tx.execute(
                "INSERT INTO keys (key, identity_id) VALUES (?1, 2)",
                [vec![1u8; 32]],
            )
            .unwrap_err();

            tx.execute(
                "INSERT INTO keys (key, identity_id) VALUES (?1, 999)",
                [vec![9u8; 32]],
            )
            .unwrap_err();
        });
    }

    /// The column is meant to carry other credential material later, so a row that is not an
    /// X25519 key must be skipped rather than lock every peer out.
    #[test]
    fn non_key_credentials_are_skipped() {
        with_test_data(|tx| {
            tx.execute(
                "INSERT INTO keys (key, identity_id) VALUES (?1, 2)",
                [b"a password hash, not a key".to_vec()],
            )
            .unwrap();

            let keys = beemsg_identities(tx).unwrap();
            assert_eq!(keys.len(), 3);
            assert!(keys.iter().all(|(k, _)| k.as_bytes().len() == 32));
        });
    }

    /// Deleting an identity must take its keys with it, otherwise a revoked peer keeps working.
    #[test]
    fn deleting_an_identity_removes_its_keys() {
        with_test_data(|tx| {
            tx.execute("DELETE FROM identities WHERE identity_id = 1", [])
                .unwrap();

            let keys = beemsg_identities(tx).unwrap();
            assert_eq!(keys.len(), 1);
            assert_eq!(keys[0].1.name.as_ref(), "ctl");
        });
    }
}
