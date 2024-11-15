use super::*;
use crate::config::Config;
use shared::NetworkAddr;
use shared::bee_msg::MsgId;
use sqlite::Connections;
use std::net::Ipv4Addr;
use std::sync::Mutex;

#[derive(Debug, Clone)]
pub struct TestApp {
    pub db: Connections,
    pub info: Arc<StaticInfo>,
    #[allow(clippy::type_complexity)]
    pub notifications: Arc<Mutex<Vec<(MsgId, Vec<NodeType>)>>>,
}

impl TestApp {
    pub async fn new() -> Self {
        let db = crate::db::test::setup_with_test_data().await;
        Self {
            db,
            info: Arc::new(StaticInfo {
                user_config: Config::default(),
                auth_secret: None,
                network_addrs: vec![NetworkAddr {
                    addr: Ipv4Addr::LOCALHOST.into(),
                    name: "localhost".to_string(),
                }],
            }),
            notifications: Arc::new(Mutex::new(vec![])),
        }
    }

    pub fn has_sent_notification<M: Msg>(&self, receivers: &[NodeType]) -> bool {
        self.notifications
            .lock()
            .unwrap()
            .contains(&(M::ID, receivers.to_vec()))
    }
}

impl AppInfo for TestApp {
    fn static_info(&self) -> &StaticInfo {
        &self.info
    }
}

impl AppDb for TestApp {
    async fn read_tx<T: Send + 'static + FnOnce(&Transaction) -> Result<R>, R: Send + 'static>(
        &self,
        op: T,
    ) -> Result<R> {
        Connections::read_tx(&self.db, op).await
    }

    async fn write_tx<T: Send + 'static + FnOnce(&Transaction) -> Result<R>, R: Send + 'static>(
        &self,
        op: T,
    ) -> Result<R> {
        Connections::write_tx(&self.db, op).await
    }
    async fn write_tx_no_sync<
        T: Send + 'static + FnOnce(&Transaction) -> Result<R>,
        R: Send + 'static,
    >(
        &self,
        op: T,
    ) -> Result<R> {
        Connections::write_tx_no_sync(&self.db, op).await
    }
    async fn conn<
        T: Send + 'static + FnOnce(&mut rusqlite::Connection) -> Result<R>,
        R: Send + 'static,
    >(
        &self,
        op: T,
    ) -> Result<R> {
        Connections::conn(&self.db, op).await
    }
}

impl AppConn for TestApp {
    async fn request<M: Msg + Serializable, R: Msg + Deserializable>(
        &self,
        _node_uid: Uid,
        _msg: &M,
    ) -> Result<R> {
        // TODO
        Ok(R::default())
    }

    async fn send_notifications<M: Msg + Serializable>(
        &self,
        node_types: &'static [NodeType],
        _msg: &M,
    ) {
        self.notifications
            .lock()
            .unwrap()
            .push((M::ID, node_types.to_owned()));
        // nop
    }

    fn replace_node_addrs(&self, _node_uid: Uid, _new_addrs: impl Into<Arc<[SocketAddr]>>) {
        // nop
    }
}

impl AppRunState for TestApp {
    fn pre_shutdown(&self) -> bool {
        false
    }

    fn notify_client_pulled_state(&self, _node_type: NodeType, _node_id: NodeId) {
        // nop
    }
}

impl AppLicense for TestApp {
    async fn load_and_verify_cert(&self, _cert_path: &std::path::Path) -> Result<String> {
        Ok("dummy cert".to_string())
    }

    fn get_cert_data(&self) -> Result<protobuf::license::GetCertDataResult> {
        Ok(protobuf::license::GetCertDataResult::default())
    }

    fn get_num_machines(&self) -> Result<u32> {
        Ok(128)
    }
}

/// Queries a single value from the db and asserts on it
macro_rules! assert_eq_db {
    ($handle:expr, $sql:literal, [$($params:expr),* $(,)?], $expect:expr $(,$arg:tt)* $(,)?) => {{
        // Little trick to "detect" the type of $expect
        #[allow(unused_assignments)]
        let mut res = $expect.to_owned();

        res = $handle
            .read_tx(|tx| {
                tx.query_row(
                    ::sqlite_check::sql!($sql),
                    ::rusqlite::params![$($params),*],
                    |row| row.get(0),
                )
                .map_err(Into::into)
            })
            .await
            .unwrap();

        assert_eq!(res, $expect $(,$arg)*);
    }};
}

pub(crate) use assert_eq_db;
