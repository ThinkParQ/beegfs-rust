//! Provides storage for open TCP streams as well as message buffers and provides functions to
//! acquire them and put them back
//!
//! Also provides a permit system to limit outgoing connections to a defined maximum.

use super::TCP_BUF_LEN;
use super::async_queue::AsyncQueue;
use crate::conn::stream::Stream;
use std::collections::{HashMap, VecDeque};
use std::fmt::{Debug, Display};
use std::hash::Hash;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex, RwLock};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

/// The Store structure
#[derive(Debug, Default)]
pub struct Store<T: Debug> {
    #[allow(clippy::type_complexity)]
    streams: Mutex<HashMap<T, (Arc<AsyncQueue<StoredStream<T>>>, Arc<Semaphore>)>>,
    bufs: Mutex<VecDeque<Vec<u8>>>,
    addrs: RwLock<HashMap<T, Arc<[SocketAddr]>>>,
    connection_limit: usize,
}

impl<T: Debug + Display + Clone + Default + PartialEq + Eq + Hash> Store<T> {
    /// Create a new store
    pub fn new(connection_limit: usize) -> Self {
        Self {
            connection_limit,
            ..Default::default()
        }
    }

    /// Create a new entry
    fn new_streams_entry(&self) -> (Arc<AsyncQueue<StoredStream<T>>>, Arc<Semaphore>) {
        (
            Arc::new(AsyncQueue::new()),
            Arc::new(Semaphore::new(self.connection_limit)),
        )
    }

    /// Try to pop a stored stream for the given peer. Returns immediately.
    ///
    /// This should be the first thing to try when acquiring a connection (reusing existing
    /// connections > opening a new one)
    pub fn try_pop_stream(&self, key: T) -> Option<StoredStream<T>> {
        let mut streams = self.streams.lock().unwrap();

        let (queue, _) = streams
            .entry(key)
            .or_insert_with(|| self.new_streams_entry());

        queue.try_pop()
    }

    /// Try to acquire a permit to open a new connection.
    ///
    /// If no connection is available, a requester can open a new one if there are permits left.
    pub fn try_acquire_permit(&self, key: T) -> Option<StoredStreamPermit<T>> {
        let mut streams = self.streams.lock().unwrap();

        let (_, sem) = streams
            .entry(key.clone())
            .or_insert_with(|| self.new_streams_entry());

        sem.clone()
            .try_acquire_owned()
            .ok()
            .map(|p| StoredStreamPermit { _permit: p, key })
    }

    /// Acquire a stored stream, waiting for one to become available.
    ///
    /// This is the last resort if the store is empty and no more permits are available.
    pub async fn pop_stream(&self, key: T) -> StoredStream<T> {
        let queue = {
            let mut streams = self.streams.lock().unwrap();

            let (queue, _) = streams
                .entry(key.clone())
                .or_insert_with(|| self.new_streams_entry());

            queue.clone()
        };

        queue.pop().await
    }

    /// Push a used stream back into the store.
    ///
    /// After being done, a stream should be put back into the store for reuse.
    pub fn push_stream(&self, stream: StoredStream<T>) {
        let mut streams = self.streams.lock().unwrap();

        let p = streams
            .entry(stream.permit.key.clone())
            .or_insert_with(|| self.new_streams_entry());

        p.0.push(stream);
    }

    /// Pop a message buffer from the store
    pub fn pop_buf(&self) -> Option<Vec<u8>> {
        self.bufs.lock().unwrap().pop_front()
    }

    /// Pop a message buffer from the store or create a new one suitable for stream / TCP messages
    pub fn pop_buf_or_create(&self) -> Vec<u8> {
        self.pop_buf().unwrap_or_else(|| vec![0; TCP_BUF_LEN])
    }

    /// Push back a message buffer to the store
    pub fn push_buf(&self, buf: Vec<u8>) {
        self.bufs.lock().unwrap().push_back(buf);
    }

    /// Get a list of known addresses for the given node UID
    pub fn get_node_addrs(&self, key: T) -> Option<Arc<[SocketAddr]>> {
        self.addrs.read().unwrap().get(&key).cloned()
    }

    /// Replace **all** addresses for the given node UID
    pub fn replace_node_addrs(&self, key: T, new_addrs: impl Into<Arc<[SocketAddr]>>) {
        let mut addrs = self.addrs.write().unwrap();
        let addr = addrs.entry(key).or_insert_with(|| Arc::new([]));
        *addr = new_addrs.into();
    }
}

/// A permit, representing the permission to open a new stream to a specific node
#[derive(Debug)]
pub struct StoredStreamPermit<T: Debug> {
    key: T,
    _permit: OwnedSemaphorePermit,
}

/// A wrapper around a stored Stream.
///
/// This is handed out by the store to the user, tracking the permit used for opening the contained
/// stream. If dropped, the permit is invalidated and a new one can be handed out.
///
/// Implements [AsRef] and [AsMut] to give access to the inner stream.
#[derive(Debug)]
pub struct StoredStream<T: Debug> {
    stream: Stream,
    permit: StoredStreamPermit<T>,
}

impl<T: Debug> StoredStream<T> {
    pub fn from_stream(stream: Stream, permit: StoredStreamPermit<T>) -> Self {
        Self { stream, permit }
    }
}

impl<T: Debug> AsRef<Stream> for StoredStream<T> {
    fn as_ref(&self) -> &Stream {
        &self.stream
    }
}

impl<T: Debug> AsMut<Stream> for StoredStream<T> {
    fn as_mut(&mut self) -> &mut Stream {
        &mut self.stream
    }
}
