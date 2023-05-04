#!/bin/bash
set -x

cargo install cargo-deny

# Check for licenses, security advisories and banned packages
cargo deny check

# Check formatting
cargo fmt --check

# Check clippy lint, taking warnings as errors
cargo clippy -- -D warnings
