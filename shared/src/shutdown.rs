//! Gracefully handle app shutdown.

use tokio::sync::watch;

/// Handle for receiving the shutdown signal.
#[derive(Clone, Debug)]
pub struct Shutdown {
    shutdown: bool,
    rx: watch::Receiver<()>,
}

/// Control handle for signaling app shutdown.
#[derive(Debug)]
pub struct ShutdownControl {
    tx: watch::Sender<()>,
}

/// Create a new connected signaler / receiver pair.
pub fn new() -> (Shutdown, ShutdownControl) {
    let (tx, rx) = watch::channel(());

    (
        Shutdown {
            shutdown: false,
            rx,
        },
        ShutdownControl { tx },
    )
}

impl Shutdown {
    /// Asynchronously wait for a shutdown signal to happen.
    ///
    /// This is meant to be used in tasks that wait for other futures to complete. A
    /// `tokio::select!` block can be used to await multiple futures in parallel. When this future
    /// completes, the task should clean up and end.
    ///
    /// If called again after shutdown has been signalled, the function immediately returns.
    pub async fn wait(&mut self) {
        if !self.shutdown {
            let _ = self.rx.changed().await;
            self.shutdown = true;
        }
    }
}

impl ShutdownControl {
    /// Signal shutdown to all receiving handles and await completion.
    ///
    /// After sending the signal, this function waits until all corresponding receiving handles have
    /// been dropped (= the task owning that handle has been shutdown).
    pub async fn shutdown(self) {
        let _ = self.tx.send(());

        self.tx.closed().await;
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use std::time::Duration;
    use tokio::time::sleep;

    #[tokio::test]
    async fn shutdown() {
        let (mut s, sc) = new();

        {
            let mut s = s.clone();
            tokio::spawn(async move {
                s.wait().await;
            });
        }

        tokio::spawn(async move {
            s.wait().await;
        });

        tokio::select! {
            _ = sleep(Duration::from_secs(1)) => { panic!("Timeout hit");}
            _ = sc.shutdown() => {}
        }
    }
}
