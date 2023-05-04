mod setup;

pub use macros::*;
use mgmtd::config::Config;
pub use setup::*;
pub use shared::msg;
use shared::shutdown;
use std::future::Future;
use std::path::Path;

const DB_PATH: &str = "/tmp/mgmtd.sqlite";

#[allow(unused)]
pub fn run_test_internal<F>(test_future: F)
where
    F: Future + Send + 'static,
    F::Output: Send,
{
    let _ = env_logger::builder()
        .is_test(true)
        .format_timestamp(None)
        .try_init();

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .thread_stack_size(16 * 1024 * 1024)
        .build()
        .unwrap();

    rt.block_on(async {
        let _mgmtd_task = tokio::spawn(async move {
            let _ = std::fs::remove_file(DB_PATH);

            mgmtd::initialize_database(Path::new(DB_PATH)).unwrap();

            let (shutdown, _shutdown_control) = shutdown::new();

            mgmtd::start(
                Config {
                    db_file: DB_PATH.into(),
                    ..Config::default()
                },
                None,
                None,
                shutdown,
            )
            .await
            .unwrap();
        });

        tokio::time::sleep(std::time::Duration::from_millis(200)).await;

        let test_task = tokio::spawn(test_future);

        tokio::select! {
            res = test_task => {
                // catch error returned from JoinHandle on panic and let the test actually fail
                res.expect("");
            }
            _ = tokio::signal::ctrl_c() => {
                // make sure process doesn't keep running in background
                std::process::abort();
            }
        }
    });
}

#[allow(unused)]
pub fn run_test_docker<F>(test_future: F, config: &[&str])
where
    F: Future + Send + 'static,
    F::Output: Send,
{
    let _ = env_logger::builder()
        .is_test(true)
        .format_timestamp(None)
        .try_init();

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .thread_stack_size(16 * 1024 * 1024)
        .build()
        .unwrap();

    let _mgmtd_docker = DockerMgmtd::setup(config);

    rt.block_on(async {
        let test_task = tokio::spawn(test_future);

        tokio::select! {
            res = test_task => {
                // catch error returned from JoinHandle on panic and let the test actually fail
                res.expect("");
            }
            _ = tokio::signal::ctrl_c() => {
                // make sure process doesn't keep running in background
                std::process::abort();
            }
        }
    });
}

pub struct DockerMgmtd {}

const CONTAINER_NAME: &str = "beemsg-mgmtd";
const IMAGE_NAME: &str = "beegfs-mgmtd";

impl DockerMgmtd {
    pub fn setup(config: &[&str]) -> Self {
        Self::kill();

        let _ch = std::process::Command::new("docker")
            .args(
                [
                    &[
                        "run",
                        "--detach",
                        "--rm",
                        "--net=host",
                        "--name",
                        CONTAINER_NAME,
                        IMAGE_NAME,
                    ],
                    config,
                ]
                .concat(),
            )
            .output()
            .unwrap();

        // Wait for mgmtd to start
        std::thread::sleep(std::time::Duration::from_millis(100));

        Self {}
    }

    fn kill() {
        std::process::Command::new("docker")
            .args(["kill", CONTAINER_NAME])
            .output()
            .unwrap();
    }
}

impl Drop for DockerMgmtd {
    fn drop(&mut self) {
        Self::kill();
    }
}
