//! An async task safe queue based on [VecDeque]

use std::collections::VecDeque;
use std::sync::Mutex;
use tokio::sync::Notify;

/// An async task safe queue based on [VecDeque]
#[derive(Debug)]
pub struct AsyncQueue<T> {
    queue: Mutex<VecDeque<T>>,
    notification: Notify,
}

impl<T> AsyncQueue<T> {
    /// Create a new `AsyncQueue`
    pub fn new() -> Self {
        Self {
            queue: Mutex::new(VecDeque::new()),
            notification: Notify::new(),
        }
    }

    /// Push an item to the queue
    pub fn push(&self, item: T) {
        self.queue.lock().unwrap().push_back(item);
        self.notification.notify_one();
    }

    /// Try to pop an item from the queue
    ///
    /// Returns immediately with `None` if the queue is empty.
    pub fn try_pop(&self) -> Option<T> {
        let (item, is_empty) = {
            let mut queue = self.queue.lock().unwrap();
            let item = queue.pop_front();
            (item, queue.is_empty())
        };

        // Ensure a permit is stored in the notification if the queue is not empty after popping
        if item.is_some() && !is_empty {
            self.notification.notify_one();
        }

        item
    }

    /// Pop an item from the queue asynchronously
    ///
    /// The future completes as soon as an item is available in the queue.
    ///
    /// Note: Even when being the only waiter, it is not guaranteed to receive the next available
    /// item. The actual queue is locked after receiving the notification, so, in rare cases,
    /// another non-async requester might call `try_pop()` in between and receives the item instead.
    /// In that case this request is queued up again.
    pub async fn pop(&self) -> T {
        loop {
            self.notification.notified().await;

            if let Some(item) = self.try_pop() {
                return item;
            }
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use std::sync::Arc;
    use std::time::Duration;
    use tokio::time::sleep;

    #[test]
    fn push_try_pop() {
        let store = AsyncQueue::<i32>::new();

        assert_eq!(None, store.try_pop());
        store.push(1);
        store.push(2);
        store.push(3);
        assert_eq!(Some(1), store.try_pop());
        assert_eq!(Some(2), store.try_pop());
        store.push(4);
        assert_eq!(Some(3), store.try_pop());
        assert_eq!(Some(4), store.try_pop());
        assert_eq!(None, store.try_pop());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn push_pop() {
        let store = Arc::new(AsyncQueue::<i32>::new());

        store.push(1);
        assert_eq!(1, store.pop().await);

        let store2 = store.clone();
        let t = tokio::spawn(async move {
            tokio::select! {
                item = store2.pop() => {
                    item
                }

                _ = sleep(Duration::from_millis(1000)) => {
                    panic!("Timeout hit");
                }
            }
        });

        store.push(2);

        assert_eq!(2, t.await.unwrap());
        assert_eq!(None, store.try_pop());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 16)]
    async fn concurrent() {
        let store = Arc::new(AsyncQueue::<i32>::new());

        for i in 0..16000 {
            store.push(i);
        }

        let mut tasks = vec![];
        for _ in 0..16 {
            let store = store.clone();
            tasks.push(tokio::spawn(async move {
                for _ in 0..1000 {
                    store.pop().await;
                }
            }));
        }

        for t in tasks {
            t.await.unwrap();
        }

        assert_eq!(0, store.queue.lock().unwrap().len());
    }
}
