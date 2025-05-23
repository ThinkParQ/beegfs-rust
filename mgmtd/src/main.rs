use anyhow::{Context, Result, anyhow, bail};
use log::LevelFilter;
use mgmtd::config::LogTarget;
use mgmtd::db::{self};
use mgmtd::license::LicenseVerifier;
use mgmtd::{StaticInfo, start};
use shared::journald_logger;
use shared::types::AuthSecret;
use std::backtrace::{Backtrace, BacktraceStatus};
use std::fmt::Write;
use std::path::Path;
use std::{fs, panic};
use tokio::signal::ctrl_c;
use uuid::Uuid;

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
fn inner_main() -> Result<()> {
    panic::set_hook(Box::new(panic_handler));

    let (user_config, info_log) = mgmtd::config::load_and_parse()?;

    // Daemonization
    // It has to happen as early as possible to make sure all the logs go into the redirected
    // stderr file. This also means there is no success or failure indication, except for the
    // daemonization itself.
    if user_config.daemonize {
        std::fs::create_dir_all(
            user_config
                .daemonize_pid_file
                .parent()
                .ok_or_else(|| anyhow!("File does not have a parent folder"))?,
        )?;
        let daemonize = daemonize::Daemonize::new().pid_file(&user_config.daemonize_pid_file);
        daemonize.start().context("Daemonization failed")?;
    }

    // Initialize logging
    match user_config.log_target {
        LogTarget::Stderr => Ok(env_logger::Builder::from_env(
            env_logger::Env::default()
                .default_filter_or(LevelFilter::from(user_config.log_level.clone()).as_str()),
        )
        .format_target(false)
        .try_init()?),
        LogTarget::Journald => journald_logger::init(user_config.log_level.clone().into()),
    }
    .expect("Logger initialization failed");

    // late log info from load_and_parse
    for l in info_log {
        log::info!(target: "mgmtd::config", "{l}");
    }

    if user_config.init || user_config.import_from_v7.is_some() {
        init_db(
            &user_config.db_file,
            user_config.import_from_v7.as_deref(),
            user_config.fs_uuid,
        )?;
        return Ok(());
    }

    if user_config.upgrade {
        upgrade_db(&user_config.db_file)?;
        return Ok(());
    }

    if let Err(err) = fs::metadata(&user_config.db_file) {
        anyhow::bail!(
            "No accessible database file found at {:?}: {err}
If you want to initialize a new system or upgrade an existing one, refer to --help or \
doc.beegfs.io.",
            user_config.db_file,
        );
    }

    let auth_secret = if !user_config.auth_disable {
        let secret = std::fs::read(&user_config.auth_file).with_context(|| {
            format!(
                "Could not open authentication file {:?}",
                user_config.auth_file
            )
        })?;
        Some(AuthSecret::hash_from_bytes(secret))
    } else {
        None
    };

    let network_addrs = shared::ethernet_interfaces(&user_config.interfaces)?;

    // Configure the tokio runtime
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_stack_size(16 * 1024 * 1024)
        .max_blocking_threads(user_config.max_blocking_threads)
        .build()?;

    // Run the tokio executor
    rt.block_on(async move {
        // Load the licensing library
        let license = if !user_config.license_disable {
            // SAFETY:
            // There is no way to verify that the user loaded dynamic library matches the
            // requirements of LicenseVerifier. After all, users can load anything they
            // want. Therefore, this is just not safe to do from the Rust compilers
            // perspective and loading anything with non-matching fp signatures or not
            // behaving as expected will lead to undefined behavior.
            let license = unsafe { LicenseVerifier::with_lib(&user_config.license_lib_file) };

            if let Err(err) = license
                .load_and_verify_cert(&user_config.license_cert_file)
                .await
            {
                log::warn!(
                    "Initializing licensing library failed. \
                    Licensed features will be unavailable: {err}"
                );
            }

            license
        } else {
            LicenseVerifier::with_no_lib()
        };

        // Start the actual daemon
        let run = start(
            StaticInfo {
                user_config,
                auth_secret,
                network_addrs,
            },
            license,
        )
        .await?;

        // Mgmtds systemd unit is set to service type "notify". Here we send out the
        // notification that the service has completed startup and is ready for serving
        let _ = sd_notify::notify(true, &[sd_notify::NotifyState::Ready]);

        run.wait_for_shutdown(ctrl_c).await;

        Ok(())
    })
}

/// Create and initialize a new database.
///
/// Optionally import v7 data from the given path. Optionally the FsUUID can be specified otherwise
/// it will be autogenerated. The database file is only written to disk if initialization succeeds.
fn init_db(db_file: &Path, v7_path: Option<&Path>, fs_uuid: Option<Uuid>) -> Result<()> {
    if db_file.try_exists()? {
        bail!("Database file {db_file:?} already exists");
    }

    let mut conn = sqlite::open_in_memory()?;

    // Create db in memory
    let version = (|| -> Result<_> {
        let tx = conn.transaction()?;

        let version =
            sqlite::migrate_schema(&tx, db::MIGRATIONS).context("Creating schema failed")?;
        db::initial_entries(&tx, fs_uuid).context("Creating initial entries failed")?;

        if let Some(v7_path) = v7_path {
            db::import_v7(&tx, v7_path).context("v7 management data import failed")?;
        }

        tx.commit()?;
        Ok(version)
    })()
    .context("Initializing database failed")?;

    // Import into memory succeeded, now write it to disk
    (|| -> Result<_> {
        std::fs::create_dir_all(
            db_file
                .parent()
                .ok_or_else(|| anyhow!("File does not have a parent folder"))?,
        )?;
        conn.backup(rusqlite::DatabaseName::Main, db_file, None)?;

        Ok(())
    })()
    .with_context(|| format!("Creating database file {db_file:?} failed"))?;

    print!("Created new database version {version} at {db_file:?}.");
    if let Some(v7_path) = v7_path {
        println!(
            " Successfully imported v7 management data from {v7_path:?}.

IMPORTANT: The import only contains managements data store, no configuration from \
beegfs-mgmtd.conf. Before starting the management, you must MANUALLY transfer your old settings \
(if they still apply) to the new config file (/etc/beegfs/beegfs-mgmtd.toml by default)."
        );
    } else {
        println!();
    }

    Ok(())
}

fn upgrade_db(db_file: &Path) -> Result<()> {
    let mut conn = sqlite::open(db_file)?;

    let backup_file = sqlite::backup_db(&mut conn)?;
    println!("Old database backed up to {backup_file:?}");

    let tx = conn.transaction()?;
    let version = sqlite::migrate_schema(&tx, db::MIGRATIONS)
        .with_context(|| "Upgrading database schema failed")?;
    tx.commit()?;

    println!("Upgraded database to version {version}");
    Ok(())
}

fn panic_handler(info: &std::panic::PanicHookInfo) {
    let backtrace = Backtrace::capture();

    let mut s = format!("PANIC: {info}");
    if backtrace.status() == BacktraceStatus::Captured {
        let _ = write!(s, "\n\nBACKTRACE:\n{backtrace}");
    }

    if log::log_enabled!(log::Level::Error) {
        log::error!("{s}");
    } else {
        eprintln!("{s}");
    }
}
