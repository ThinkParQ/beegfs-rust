use anyhow::Context;
use mgmtd::config::LogTarget;
use mgmtd::start;
use shared::{journald_logger, shutdown, AuthenticationSecret};
use std::backtrace::Backtrace;
use std::panic;
use tokio::signal::ctrl_c;

fn main() -> Result<(), i32> {
    inner_main().map_err(|err| {
        eprintln!("{err:#}");
        1
    })?;

    Ok(())
}

fn inner_main() -> anyhow::Result<()> {
    let (static_config, dynamic_config_args, info_log) = mgmtd::config::load_and_parse()?;

    match static_config.log_target {
        LogTarget::Std => Ok(env_logger::Builder::from_env(
            env_logger::Env::default().default_filter_or(static_config.log_level.as_str()),
        )
        .try_init()?),
        LogTarget::Journald => journald_logger::init(static_config.log_level),
    }
    .expect("Logger initialization failed");

    // log info from load_and_parse
    for l in info_log {
        log::info!(target: "mgmtd::config", "{l}");
    }

    match static_config.init {
        true => {
            mgmtd::db::initialize(static_config.db_file.as_path())?;

            println!("Database initialized");
        }
        // run the daemon
        false => {
            let auth_secret = if static_config.auth_enable {
                let secret = std::fs::read(&static_config.auth_file).with_context(|| {
                    format!(
                        "Could not open authentication file {:?}",
                        static_config.auth_file
                    )
                })?;
                Some(AuthenticationSecret::from_bytes(secret))
            } else {
                None
            };

            let (shutdown, shutdown_control) = shutdown::new();

            let rt = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .thread_stack_size(16 * 1024 * 1024)
                .build()?;

            // Ensure the program ends if a task panics
            panic::set_hook(Box::new(|info| {
                let backtrace = Backtrace::capture();
                eprintln!("PANIC occured: {info}\n\nBACKTRACE:\n{backtrace}");
                std::process::exit(1);
            }));

            rt.block_on(async move {
                start(static_config, dynamic_config_args, auth_secret, shutdown).await?;

                // notify systemd manager that the process is ready
                let _ = sd_notify::notify(true, &[sd_notify::NotifyState::Ready]);

                log::info!("Waiting for SIGINT / Ctrl-C ...");

                let _ = ctrl_c().await;

                log::warn!("Received SIGINT. Waiting for all tasks to complete ...");

                tokio::select! {
                    _ = shutdown_control.shutdown() => {
                        log::warn!("Shutdown completed");
                    }
                    _ = ctrl_c() => {
                        log::warn!("Shutdown forced");
                    }
                }

                Ok::<(), anyhow::Error>(())
            })?;
        }
    };

    Ok(())
}
