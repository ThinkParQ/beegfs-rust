use super::async_queue::AsyncQueue;
use crate::conn::stream::Stream;
use crate::conn::MsgBuf;
use crate::EntityUID;
use anyhow::{anyhow, Result};
use std::collections::{HashMap, VecDeque};
use std::fmt::Debug;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use tokio::time::timeout;

const TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Debug, Default)]
pub struct Store {
    #[allow(clippy::type_complexity)]
    streams: Mutex<HashMap<EntityUID, (Arc<AsyncQueue<StoredStream>>, Arc<Semaphore>)>>,
    bufs: Mutex<VecDeque<MsgBuf>>,
    addrs: RwLock<HashMap<EntityUID, Arc<[SocketAddr]>>>,
    connection_limit: usize,
}

impl Store {
    pub fn new(connection_limit: usize) -> Self {
        Self {
            connection_limit,
            ..Default::default()
        }
    }

    fn new_streams_entry(&self) -> (Arc<AsyncQueue<StoredStream>>, Arc<Semaphore>) {
        (
            Arc::new(AsyncQueue::new()),
            Arc::new(Semaphore::new(self.connection_limit)),
        )
    }

    pub fn try_pop_stream(&self, node_uid: EntityUID) -> Option<StoredStream> {
        let mut streams = self.streams.lock().unwrap();

        let (queue, _) = streams
            .entry(node_uid)
            .or_insert_with(|| self.new_streams_entry());

        queue.try_pop()
    }

    pub fn try_acquire_permit(&self, node_uid: EntityUID) -> Option<StoredStreamPermit> {
        let mut streams = self.streams.lock().unwrap();

        let (_, sem) = streams
            .entry(node_uid)
            .or_insert_with(|| self.new_streams_entry());

        sem.clone()
            .try_acquire_owned()
            .ok()
            .map(|p| StoredStreamPermit {
                _permit: p,
                node_uid,
            })
    }

    pub async fn pop_stream(&self, node_uid: EntityUID) -> Result<StoredStream> {
        let queue = {
            let mut streams = self.streams.lock().unwrap();

            let (queue, _) = streams
                .entry(node_uid)
                .or_insert_with(|| self.new_streams_entry());

            queue.clone()
        };

        timeout(TIMEOUT, queue.pop())
            .await
            .map_err(|_| anyhow!("Popping a stream to {node_uid:?} timed out"))
    }

    pub fn push_stream(&self, stream: StoredStream) {
        let mut streams = self.streams.lock().unwrap();

        let p = streams
            .entry(stream.permit.node_uid)
            .or_insert_with(|| self.new_streams_entry());

        p.0.push(stream);
    }

    pub fn pop_buf(&self) -> Option<MsgBuf> {
        let mut bufs = self.bufs.lock().unwrap();

        bufs.pop_front()
    }

    pub fn push_buf(&self, buf: MsgBuf) {
        let mut bufs = self.bufs.lock().unwrap();
        bufs.push_back(buf);
    }

    pub fn get_node_addrs(&self, node_uid: EntityUID) -> Option<Arc<[SocketAddr]>> {
        let addrs = self.addrs.read().unwrap();
        addrs.get(&node_uid).cloned()
    }

    pub fn replace_node_addrs(&self, node_uid: EntityUID, new_addrs: impl Into<Arc<[SocketAddr]>>) {
        let mut addrs = self.addrs.write().unwrap();
        let addr = addrs.entry(node_uid).or_insert_with(|| Arc::new([]));
        *addr = new_addrs.into();
    }
}

#[derive(Debug)]
pub struct StoredStreamPermit {
    node_uid: EntityUID,
    _permit: OwnedSemaphorePermit,
}

#[derive(Debug)]
pub struct StoredStream {
    stream: Stream,
    permit: StoredStreamPermit,
}

impl StoredStream {
    pub fn from_stream(stream: Stream, permit: StoredStreamPermit) -> Self {
        Self { stream, permit }
    }
}

impl AsRef<Stream> for StoredStream {
    fn as_ref(&self) -> &Stream {
        &self.stream
    }
}

impl AsMut<Stream> for StoredStream {
    fn as_mut(&mut self) -> &mut Stream {
        &mut self.stream
    }
}
