use clap::Parser;
use ctl::config::{CmdLineArgs, StaticConfig};
use ctl::run;
use env_logger::Env;
use shared::AuthenticationSecret;
use std::net::{Ipv4Addr, SocketAddr};
use std::str::FromStr;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), i32> {
    env_logger::Builder::from_env(Env::default().default_filter_or("warn")).init();

    let args = CmdLineArgs::parse();

    let auth_secret = if let Some(ref path) = args.auth_file {
        let secret = std::fs::read(path).unwrap();
        Some(AuthenticationSecret::from_bytes(secret))
    } else {
        None
    };

    let management_addr = match std::env::var("MANAGEMENT_HOST") {
        Ok(s) => match SocketAddr::from_str(&s) {
            Ok(addr) => addr,
            Err(err) => {
                log::error!(
                    "MANAGEMENT_HOST variable can not be parsed into a host address: {err}"
                );
                return Err(1);
            }
        },
        Err(_) => {
            log::warn!("MANAGEMENT_HOST not specified, using 127.0.0.1:8008");
            SocketAddr::new(Ipv4Addr::LOCALHOST.into(), 8008)
        }
    };

    let static_config = StaticConfig {
        args,
        auth_secret,
        management_addr,
    };

    match run(static_config).await {
        Ok(_) => Ok(()),
        Err(e) => {
            log::error!("{:?}", e);
            Err(1)
        }
    }
}
