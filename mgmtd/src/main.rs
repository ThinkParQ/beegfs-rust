use anyhow::{Context, Result};
use mgmtd::config::LogTarget;
use mgmtd::db::{self};
use mgmtd::license::LicenseVerifier;
use mgmtd::{start, StaticInfo};
use shared::journald_logger;
use shared::types::AuthSecret;
use std::backtrace::Backtrace;
use std::panic;
use std::path::Path;
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
fn inner_main() -> Result<()> {
    let (user_config, info_log) = mgmtd::config::load_and_parse()?;

    // Initialize logging
    match user_config.log_target {
        LogTarget::Std => Ok(env_logger::Builder::from_env(
            env_logger::Env::default().default_filter_or(user_config.log_level.as_str()),
        )
        .format_target(false)
        .try_init()?),
        LogTarget::Journald => journald_logger::init(user_config.log_level),
    }
    .expect("Logger initialization failed");

    // late log info from load_and_parse
    for l in info_log {
        log::info!(target: "mgmtd::config", "{l}");
    }

    if user_config.init || user_config.import_from_v7.is_some() {
        setup_db(
            &user_config.db_file,
            user_config.init,
            user_config.import_from_v7.as_deref(),
        )?;
        return Ok(());
    }

    if user_config.upgrade {
        upgrade_db(&user_config.db_file)?;
        return Ok(());
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

    // SAFETY:
    // There is no way to verify that the user loaded dynamic library matches the requirements
    // of LicenseVerifier. After all, users can load anything they want. Therefore, this is just not
    // safe to do from the Rust compilers perspective and loading anything with non-matching fp
    // signatures or not behaving as expected will lead to undefined behavior.
    let lic = unsafe { LicenseVerifier::new(&user_config.license_lib_file) };

    // Ensure the program ends if a task panics
    panic::set_hook(Box::new(|info| {
        let backtrace = Backtrace::capture();
        eprintln!("PANIC occurred: {info}\n\nBACKTRACE:\n{backtrace}");
        std::process::exit(1);
    }));

    // Configure the tokio runtime
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_stack_size(16 * 1024 * 1024)
        .build()?;

    let network_addrs = shared::ethernet_interfaces(&user_config.interfaces)?;

    // Run the tokio executor
    rt.block_on(async move {
        if let Err(err) = lic
            .load_and_verify_cert(user_config.license_cert_file.as_path())
            .await
        {
            log::warn!(
                "Initializing licensing library failed.\
                Licensed features will be unavailable: {err}"
            );
        }

        // Start the actual daemon
        let run = start(
            StaticInfo {
                user_config,
                auth_secret,
                network_addrs,
            },
            lic,
        )
        .await?;

        // Mgmtds systemd unit is set to service type "notify". Here we send out the
        // notification that the service has completed startup and is ready for serving
        let _ = sd_notify::notify(true, &[sd_notify::NotifyState::Ready]);

        run.wait_for_shutdown(ctrl_c).await;

        Ok(())
    })
}

/// Create and initialize a new database and / or import data from old v7 management
fn setup_db(db_file: impl AsRef<Path>, init: bool, v7_path: Option<&Path>) -> Result<()> {
    let db_file = db_file.as_ref();

    // Create database file
    if init {
        sqlite::create_db_file(db_file)
            .with_context(|| format!("Creating database file {db_file:?} failed"))?;
        println!("Database file created at {db_file:?}");
    }

    // Connect
    let mut conn = sqlite::open(db_file)?;

    // Fill database
    if init {
        let tx = conn.transaction()?;

        let version = sqlite::migrate_schema(&tx, db::MIGRATIONS)
            .context("Creating database schema failed")?;
        db::initial_entries(&tx).context("Creating initial database entries failed")?;

        tx.commit()?;
        println!("Created and migrated new database to version {version}");
    }

    // Import data from v7 management
    if let Some(v7_path) = v7_path {
        db::import_v7(&mut conn, v7_path).context("v7 management data import failed")?;
        println!("v7 management data imported");
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
