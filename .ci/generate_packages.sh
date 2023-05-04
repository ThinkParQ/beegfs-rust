#!/bin/bash
set -x

# We use the musl libc implementation (https://musl.libc.org/) which is statically linked into
# the binary
apt-get update && apt-get -y install musl-tools
rustup target add x86_64-unknown-linux-musl

# cargo tools tp generate packages
cargo install cargo-generate-rpm
cargo install cargo-deb

# build the release binary
cargo build --release --target=x86_64-unknown-linux-musl
strip -s target/x86_64-unknown-linux-musl/release/mgmtd

# generate packages
mkdir -p packages
rm -rf packages/*
cargo deb -p mgmtd --target=x86_64-unknown-linux-musl -o packages
cargo generate-rpm -p mgmtd --target=x86_64-unknown-linux-musl --auto-req=disabled -o packages