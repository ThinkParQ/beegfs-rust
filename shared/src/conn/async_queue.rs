use std::collections::VecDeque;
use std::sync::Mutex;
use tokio::sync::Notify;

#[derive(Debug)]
pub struct AsyncQueue<T> {
    store: Mutex<VecDeque<T>>,
    notification: Notify,
}

impl<T> AsyncQueue<T> {
    pub fn new() -> Self {
        Self {
            store: Mutex::new(VecDeque::new()),
            notification: Notify::new(),
        }
    }

    pub fn push(&self, item: T) {
        {
            let mut store = self.store.lock().unwrap();
            store.push_back(item);
        }

        self.notification.notify_one();
    }

    pub fn try_pop(&self) -> Option<T> {
        let (item, is_empty) = {
            let mut store = self.store.lock().unwrap();
            let item = store.pop_front();
            (item, store.is_empty())
        };

        if item.is_some() && !is_empty {
            self.notification.notify_one();
        }

        item
    }

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

        assert_eq!(0, store.store.lock().unwrap().len());
    }
}
