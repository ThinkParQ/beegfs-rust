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

/// Management main function.
///
/// The binary related setup is made here, before execution is passed to the actual app.
fn inner_main() -> anyhow::Result<()> {
    let (static_config, dynamic_config_args, info_log) = mgmtd::config::load_and_parse()?;

    // Initialize logging
    match static_config.log_target {
        LogTarget::Std => Ok(env_logger::Builder::from_env(
            env_logger::Env::default().default_filter_or(static_config.log_level.as_str()),
        )
        .try_init()?),
        LogTarget::Journald => journald_logger::init(static_config.log_level),
    }
    .expect("Logger initialization failed");

    // late log info from load_and_parse
    for l in info_log {
        log::info!(target: "mgmtd::config", "{l}");
    }

    // If the user set --init, init the database and then exit
    if static_config.init {
        mgmtd::db::initialize(static_config.db_file.as_path())?;
        println!("Database initialized");
        return Ok(());
    }

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

    // Ensure the program ends if a task panics
    panic::set_hook(Box::new(|info| {
        let backtrace = Backtrace::capture();
        eprintln!("PANIC occured: {info}\n\nBACKTRACE:\n{backtrace}");
        std::process::exit(1);
    }));

    // Configure the tokio runtime
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_stack_size(16 * 1024 * 1024)
        .build()?;

    // Run the tokio executor
    rt.block_on(async move {
        // Start the actual daemon
        start(static_config, dynamic_config_args, auth_secret, shutdown).await?;

        // Mgmtds systemd unit is set to service type "notify". Here we send out the
        // notification that the service has completed startup and is ready for serving
        let _ = sd_notify::notify(true, &[sd_notify::NotifyState::Ready]);

        // Wait for a SIGINT. When received, notify the holders of shutdown handles
        log::info!("Waiting for SIGINT / Ctrl-C ...");

        let _ = ctrl_c().await;

        log::warn!("Received SIGINT. Waiting for all tasks to complete ...");

        tokio::select! {
            // Wait for all tasks to complete
            _ = shutdown_control.shutdown() => {
                log::warn!("Shutdown completed");
            }
            // When receiving another SIGINT, end the program immediately
            _ = ctrl_c() => {
                log::warn!("Shutdown forced");
            }
        }

        Ok(())
    })
}
