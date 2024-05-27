# Build the management

1. Read through [Getting started with Rust]( https://github.com/ThinkParQ/developer-handbook/tree/main/getting_started/rust) and setup your environment as described.

2. Clone the managment repository:

   ```shell
   git clone https://github.com/ThinkParQ/beegfs-rs
   cd beegfs-rs
   ```

3. The management can be built and run through cargo in one step:

   ```shell
   cargo run -p mgmtd -- --help
   ```

   Note the `--` to separate management arguments from cargo arguments.

   Or build it and then run it:

   ```shell
   cargo build -p mgmtd
   ./target/debug/mgmtd --help
   ```

   Both ways are valid - down below, the first variant is used as it is quicker.

# Run the management

The management server binary runs by itself, no additional installation needed. Some setup is necessary though as described below.

## Provide a config file

The management can optionally read in a configuration file in TOML format. The default location is `/etc/beegfs/mgmtd.toml`, can be set by `--config-file`.

The config file is ignored if not present - most configuration options can also be given by command line flags. The exceptions is quota.

## Provide TLS certificates

Management requires a TLS certificate and its private key for encrypted gRPC communication:

* A X.509 certificate file at `/etc/beegfs/mgmtd.pem`. Can be set by `--tls-cert-file`.
* A private key file at `/etc/beegfs/mgmtd.key`. Can be set by `--tls-key-file`.

Alternatively, disable TLS using `--grpc-tls-enable=false`.

To quickly create a (throwaway) certificate:

```shell
openssl req -x509 -days 9999 -keyout mgmtd.key -out mgmtd.cert -nodes
```

## Provide BeeMsg authentication file

To use BeeMsg authentication, you have to provide the same BeeMsg authentication file used for the other nodes (formerly known as "connAuthFile"). The default location is `/etc/beegfs/mgmtd.auth`, can be set using `--auth-file`.

Alternatively, disable BeeMsg authentication using `--auth-enable=false`.

## Set up the database

The management requires explicit initialization and database creation, which can be achieved by running it with the `--init` flag. The default database location at `/var/lib/beegfs/mgmtd.sqlite` is only writable by root, for playing around you might want to run management as user and provide an alternative path:

```shell
cargo run -p mgmtd -- --init --db-file=/tmp/mgmtd.sqlite
```

## Run the server

Run the binary according to the preparations above. For example, for disabling BeeMsg authentication and using the generated TLS certificate for gRPC:

```shell
cargo run -p mgmtd -- --db-file=/tmp/db.sqlite --auth-enable=false --tls-cert-file=./mgmtd.cert --tls-key-file=./mgmtd.key
```

This logs to systemd. For playing around and debugging, it is advisable to log to stdout instead. To do that, provide the `--log-target=std` flag. For that purpose, the management uses [env_logger](https://docs.rs/env_logger/latest/env_logger/). It can be configured by setting the `RUST_LOG` environment variable. For example, to log everything down to debug level:

```shell
RUST_LOG=debug cargo run -p mgmtd -- --db-file=/tmp/db.sqlite --auth-enable=false --tls-cert-file=./mgmtd.cert --tls-key-file=./mgmtd.key --log-target=std
```

env_logger can be configured relatively fine grained using `RUST_LOG`, look up its documentation for more details.

Remember that you can also provide these arguments (except for the `RUST_LOG` variable, of course) using a config file by specifiying `--config-file`.
