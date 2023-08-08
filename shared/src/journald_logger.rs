//! Journald logger implementation for the `log` interface

use bytes::BufMut;
use log::{Level, LevelFilter, Log, Metadata, Record};
use std::os::unix::net::UnixDatagram;

#[derive(Debug)]
pub struct JournaldLogger {
    sock: UnixDatagram,
    level_filter: LevelFilter,
}

/// Initializes `log` logger with [JournaldLogger]
pub fn init(level_filter: LevelFilter) -> anyhow::Result<()> {
    let sock = UnixDatagram::unbound()?;
    sock.connect("/run/systemd/journal/socket")?;

    log::set_boxed_logger(Box::new(JournaldLogger { sock, level_filter }))?;
    log::set_max_level(level_filter);
    Ok(())
}

/// [Log] Interface implementation
impl Log for JournaldLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= self.level_filter
    }

    fn log(&self, record: &Record) {
        if self.enabled(record.metadata()) {
            let msg = format!("{}", record.args()).into_bytes();

            let mut buf = format!(
                "PRIORITY={}\nSYSLOG_IDENTIFIER=beegfs-mgmtd\nMESSAGE\n",
                level_to_priority(record.level())
            )
            .into_bytes();

            buf.reserve(msg.len() + 8 + 1);
            buf.put_u64_le(msg.len() as u64);
            buf.extend(msg);
            buf.extend(b"\n");

            // If sending the data to the socket fails, report this to stderr
            if let Err(err) = self.sock.send(&buf) {
                eprintln!("Sending log to systemd failed: {err}");
            }
        }
    }

    fn flush(&self) {}
}

/// Convert [log::Level] into corresponding journald log level
fn level_to_priority(level: Level) -> u8 {
    match level {
        Level::Error => 3,
        Level::Warn => 4,
        Level::Info => 5,
        Level::Debug => 6,
        Level::Trace => 7,
    }
}
