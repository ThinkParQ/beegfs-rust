use crate::default_response::default_response;
use anyhow::{bail, Result};
use shared::conn::{MsgBuffer, MsgDispatcher, RequestHandle};
use shared::msg::Msg;
use shared::types::{AckID, MsgID};
use shared::*;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Mutex, Notify};

#[derive(Clone, Debug, Default)]
pub struct MsgStore {
    notify: Arc<Notify>,
    received_requests: Arc<Mutex<Vec<msg::Generic>>>,
    prepared_responses: Arc<Mutex<Vec<(MsgID, msg::Generic)>>>,
}

impl MsgStore {
    pub fn new() -> Self {
        Self {
            notify: Arc::new(Notify::new()),
            received_requests: Arc::new(Mutex::new(vec![])),
            prepared_responses: Arc::new(Mutex::new(vec![])),
        }
    }

    pub async fn push_msg(&self, msg: msg::Generic) {
        log::debug!("Pushed msg of type {}", msg.msg_id());

        self.received_requests.lock().await.push(msg);
        self.notify.notify_waiters();
    }

    pub async fn wait_for_req_count<M: Msg>(
        &self,
        target_count: usize,
        timeout_ms: u64,
    ) -> Result<()> {
        log::debug!("Waiting for {target_count} msgs of type {}", M::ID);

        let start_count = self
            .received_requests
            .lock()
            .await
            .iter()
            .filter(|e| e.msg_id() == M::ID)
            .count();

        self.wait(timeout_ms, |msgs| {
            let count = msgs.iter().filter(|e| e.msg_id() == M::ID).count();
            count >= start_count + target_count
        })
        .await
    }

    pub async fn wait_for_req<M: Msg, C: Fn(M) -> bool>(
        &self,
        timeout_ms: u64,
        check: C,
    ) -> Result<()> {
        log::debug!("Waiting for Msg of type {} with given condition", M::ID);

        self.wait(timeout_ms, |msgs| {
            msgs.iter()
                .filter(|e| e.msg_id() == M::ID)
                .any(|e| check(e.clone().into_beemsg()))
        })
        .await
    }

    pub async fn wait_for_ack(&self, ack_id: AckID) -> Result<()> {
        log::debug!("Waiting for Ack with ID {ack_id:?})");

        self.wait(1000, |msgs| {
            msgs.iter().filter(|e| e.msg_id() == msg::Ack::ID).any(|m| {
                let msg: msg::Ack = m.clone().into_beemsg();
                msg.ack_id == ack_id
            })
        })
        .await
    }

    async fn wait<C: Fn(&Vec<msg::Generic>) -> bool>(
        &self,
        timeout_ms: u64,
        check: C,
    ) -> Result<()> {
        let timeout = tokio::time::sleep(Duration::from_millis(timeout_ms));
        tokio::pin!(timeout);

        log::debug!("Waiting for condition with {timeout_ms} ms timeout)");

        loop {
            if check(self.received_requests.lock().await.as_ref()) {
                log::debug!("Condition met");
                break;
            }

            tokio::select! {
                _ = self.notify.notified() => {}
                _ = &mut timeout => {
                    // TODO proper error handling/location output
                    log::debug!("Reached timeout of {timeout_ms} ms");
                    bail!(format!("Reached timeout of {timeout_ms} ms"));
                }
            }
        }

        Ok(())
    }

    pub async fn add_resp<M: Msg>(&self, req_id: MsgID, msg: M) {
        self.prepared_responses
            .lock()
            .await
            .push((req_id, msg::Generic::from_beemsg(msg)));
    }

    pub async fn get_resp(&self, req_id: MsgID) -> Option<msg::Generic> {
        let mut o = self.prepared_responses.lock().await;

        let result = o.iter().enumerate().find(|e| e.1 .0 == req_id);
        let index = if let Some(result) = result {
            result.0
        } else {
            return None;
        };

        Some(o.remove(index).1)
    }
}

impl MsgDispatcher for MsgStore {
    async fn dispatch_msg(&mut self, req: impl RequestHandle) -> Result<()> {
        self.push_msg(msg.clone()).await;

        if let Some(resp) = self.get_resp(msg.msg_id()).await {
            req.respond(&resp).await
        } else if let Some(resp) = default_response(msg) {
            req.respond(&resp).await
        } else {
            Ok(())
        }
    }
}
