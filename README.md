# Build the management

1. Read through [Getting started with Rust]( https://github.com/ThinkParQ/developer-handbook/tree/main/getting_started/rust) and setup your environment as described.

2. Clone the management repository:

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

## Setting the binaries version output

One can define the output of the `--version` command at compile time by passing the `VERSION`
environment to `cargo`. This can be set to a BeeGFS release version during a release build. Gets
set to "unknown" if not given.

# Run the management

The management server binary runs by itself, no additional installation needed. Some setup is necessary though as described below.

## Provide a config file

The management can optionally read in a configuration file in TOML format. The default location is `/etc/beegfs/beegfs-mgmtd.toml`, can be set by `--config-file`.

The config file is ignored if not present - most configuration options can also be given by command line flags. The exceptions is quota.

## Provide TLS certificates

Management requires a TLS certificate and its private key for encrypted gRPC communication:

* A X.509 certificate file at `/etc/beegfs/cert.pem`. The default path be overridden by `--tls-cert-file`.
* A private key file at `/etc/beegfs/key.pem`. The default path can be overridden by `--tls-key-file`.

The following can be used to setup a self signed certificate:

(1) Create a file `san.cnf` updating the `alt_names` to include all DNS names and/or IPs where the management is accessible:

```
[ req ]
default_bits       = 2048
distinguished_name = req_distinguished_name
req_extensions     = req_ext
x509_extensions    = v3_ca # The extensions to add to the self signed cert

[ req_distinguished_name ]
commonName                  = Common Name (eg, fully qualified host name)
commonName_default          = localhost

[ req_ext ]
subjectAltName = @alt_names

[ v3_ca ]
subjectAltName = @alt_names

[ alt_names ]
DNS.1   = localhost
IP.1    = 127.0.0.1
```
(2) Run the following command to generate the certificate and key:
```shell
openssl req -x509 -nodes -days 365 -newkey rsa:2048 -keyout key.pem -out cert.pem -config san.cnf
```

(3) On the mgmtd install the key and cert at /etc/beegfs. On nodes with the CTL install the certificate to /etc/beegfs.

Alternatively, disable TLS using `--tls-disable`.

## Provide BeeMsg authentication file

To use BeeMsg authentication, you have to provide the same BeeMsg authentication file used for the other nodes (formerly known as "connAuthFile"). The default location is `/etc/beegfs/conn.auth`, but can be overridden using `--auth-file`.

Alternatively, disable BeeMsg authentication using `--auth-disable`.

## Set up the database

The management requires explicit initialization and database creation, which can be achieved by running it with the `--init` flag. The default database location at `/var/lib/beegfs/mgmtd.sqlite` is only writable by root, for playing around you might want to run management as user and provide an alternative path:

```shell
cargo run -p mgmtd -- --init --db-file=/tmp/mgmtd.sqlite
```

## Run the server

Run the binary according to the preparations above. For example, for disabling BeeMsg authentication and using the generated TLS certificate for gRPC:

```shell
cargo run -p mgmtd -- --db-file=/tmp/mgmtd.sqlite --auth-disable --tls-cert-file=./mgmtd-cert.pem --tls-key-file=./mgmtd-key.pem
```

This logs to systemd. For playing around and debugging, it is advisable to log to stdout instead. To do that, provide the `--log-target=std` flag. For that purpose, the management uses [env_logger](https://docs.rs/env_logger/latest/env_logger/). It can be configured by setting the `RUST_LOG` environment variable. For example, to log everything down to debug level:

```shell
RUST_LOG=debug cargo run -p mgmtd -- --db-file=/tmp/mgmtd.sqlite --auth-disable --tls-cert-file=./mgmtd-cert.pem --tls-key-file=./mgmtd-key.pem --log-target=std
```

env_logger can be configured relatively fine grained using `RUST_LOG`, look up its documentation for more details.

Remember that you can also provide these arguments (except for the `RUST_LOG` variable, of course) using a config file by specifying `--config-file`.
