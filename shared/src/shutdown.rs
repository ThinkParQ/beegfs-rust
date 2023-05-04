use tokio::sync::watch;

#[derive(Clone, Debug)]
pub struct Shutdown {
    shutdown: bool,
    rx: watch::Receiver<()>,
}

#[derive(Debug)]
pub struct ShutdownControl {
    tx: watch::Sender<()>,
}

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
    pub async fn wait(&mut self) {
        if self.shutdown {
            return;
        }

        let _ = self.rx.changed().await;

        self.shutdown = true;
    }
}

impl ShutdownControl {
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
