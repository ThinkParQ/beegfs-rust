//! Defines an application run state including handles to access and update it.

use std::ops::{Deref, DerefMut};
use tokio::sync::watch;

/// Represents an overall application run state.
#[derive(Debug, PartialEq, Eq)]
enum RunState {
    Running,
    PreShutdown,
    Shutdown,
}

/// Weak handle giving access to the run state but does not block shutdown until being dropped.
#[derive(Clone, Debug)]
pub struct WeakRunStateHandle {
    rx: watch::Receiver<RunState>,
}

/// Handle for receiving the shutdown signal. Blocks shutdown on the control side while hold.
#[derive(Clone, Debug)]
pub struct RunStateHandle {
    weak: WeakRunStateHandle,
    /// Only used to wait for awaiting handle drop
    #[allow(unused)]
    count_rx: watch::Receiver<()>,
}

/// Control handle for signaling app shutdown.
#[derive(Debug)]
pub struct RunStateControl {
    tx: watch::Sender<RunState>,
    count_tx: watch::Sender<()>,
}

/// Create a new connected signaler / receiver pair.
pub fn new() -> (RunStateHandle, RunStateControl) {
    let (tx, rx) = watch::channel(RunState::Running);
    let (count_tx, count_rx) = watch::channel(());

    (
        RunStateHandle {
            weak: WeakRunStateHandle { rx },
            count_rx,
        },
        RunStateControl { tx, count_tx },
    )
}

impl WeakRunStateHandle {
    /// Asynchronously wait for shutdown.
    ///
    /// This is meant to be used in tasks that wait for other futures to complete. A
    /// `tokio::select!` block can be used to await multiple futures in parallel. When this future
    /// completes, the task should clean up and end.
    ///
    /// If called again after shutdown has been signalled, the function immediately returns. The
    /// future will not complete yet on a deferred shutdown, only when actual shutdown happens.
    pub async fn wait_for_shutdown(&mut self) {
        while *self.rx.borrow() != RunState::Shutdown {
            if self.rx.changed().await.is_err() {
                break;
            }
        }
    }

    /// Asynchronously wait for prepared shutdown or shutdown.
    ///
    /// This is meant to be used in tasks that wait for other futures to complete. A
    /// `tokio::select!` block can be used to await multiple futures in parallel. When this future
    /// completes, the task should clean up and end.
    pub async fn wait_for_pre_shutdown(&mut self) {
        while !self.pre_shutdown() {
            if self.rx.changed().await.is_err() {
                break;
            }
        }
    }

    /// Returns true if the control handle has changed state to (pre) shutdown.
    pub fn pre_shutdown(&self) -> bool {
        matches!(
            *self.rx.borrow(),
            RunState::PreShutdown | RunState::Shutdown
        )
    }
}

impl Deref for RunStateHandle {
    type Target = WeakRunStateHandle;

    fn deref(&self) -> &Self::Target {
        &self.weak
    }
}

impl DerefMut for RunStateHandle {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.weak
    }
}

impl RunStateHandle {
    pub fn clone_weak(&self) -> WeakRunStateHandle {
        self.weak.clone()
    }
}

impl RunStateControl {
    /// Signal shutdown to all receiving handles and await completion.
    ///
    /// After sending the signal, this function awaits all corresponding receiving handles being
    /// dropped (= usually meaning the tasks owning all the handles have stopped).
    pub async fn shutdown(self) {
        let _ = self.tx.send(RunState::Shutdown);
        // We wait for the count_tx to be closed to exclude weak handles
        self.count_tx.closed().await;
    }

    /// Signals incoming shutdown to all receiving handles.
    ///
    /// Calling this marks the system as shutting down, but does NOT cause `.wait()` futures to
    /// complete yet. Instead, `.is_shutting_down()` will return true to allow the system to prepare
    /// for being shut down.
    pub fn pre_shutdown(&self) {
        if *self.tx.borrow() == RunState::Running {
            let _ = self.tx.send(RunState::PreShutdown);
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use std::time::Duration;
    use tokio::time::sleep;

    #[tokio::test]
    async fn shutdown() {
        let (s, sc) = new();

        for _ in 0..2 {
            let mut s = s.clone();
            tokio::spawn(async move {
                s.wait_for_shutdown().await;
                eprintln!("hello");
            });
        }

        assert!(!s.pre_shutdown());
        sc.pre_shutdown();
        assert!(s.pre_shutdown());
        let _ws = s.clone_weak();
        drop(s);

        tokio::select! {
            _ = sleep(Duration::from_millis(100)) => { panic!("Timeout hit");}
            _ = sc.shutdown() => {}
        }
    }
}
