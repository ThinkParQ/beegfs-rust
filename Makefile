SHELL = /bin/bash


# By default, we don't set a target and use the current default toolchain.
ifneq ($(TARGET),)
	export CARGO_BUILD_TARGET := $(TARGET)
endif

# By default, we build the dev profile. To make a release build, set PROFILE=release.
ifeq ($(PROFILE),)
	PROFILE := dev
endif

# Defines VERSION from the git history. Used by the binaries to build their own version string.
# To satisfy semver we fall back to `0.0.0` if there is no matching tag
export VERSION := $(shell git describe --tags --match "v*.*.*" 2>/dev/null || echo "v0.0.0")
# Strip the first character, which is usually "v" to allow usage in semver contexts
VERSION_TRIMMED := $(shell V="$(VERSION)" && echo $${V:1})



### Simple cargo wrappers ###

all: build

# Run quick sanity checks
.PHONY: check
check:
	cargo +nightly fmt --check
	cargo clippy --all-features -- -D warnings

# Run cargo deny
.PHONY: deny
deny:
	cargo deny --all-features check

# Run tests
.PHONY: test
test:
	cargo test --all-features

# Builds the selected profile (see PROFILE) for the selected target (see TARGET)
.PHONY: build
build:
	cargo build --profile=$(PROFILE)

.PHONY: clean
clean:
	cargo clean



### Release build and packaging ###

# To not interfere with cargo, this makefile copies the output to its own directory before
# modifying it. This also helps when building packages with different profiles as the binaries
# are always found under $(MAKE_DIR) after copying and one can simply refer to that from Cargo.toml.
MAKE_DIR := target/make

# Name of the generated management binary. Must match the binaries name from mgmtd/Cargo.toml
MANAGEMENT_BIN := beegfs-mgmtd

# If we build release profile, we always want to include full debug info
export CARGO_PROFILE_RELEASE_DEBUG := full

# Build release binaries and package them.
# Note that this is intentionally all within one target as building a release requires slightly
# different parameters on the cargo commands than the ones passed above. Since one usually
# doesn't need any of this separately during development, we avoid making a mess with a ton of
# extra targets and checks. In the end, cargo already handles this stuff and this Makefile just
# servers as a wrapper.
.PHONY: package
.ONESHELL: package
package:
	@set -x

	# Checks and tests
	cargo +nightly fmt --check
	cargo clippy --release --all-features --locked -- -D warnings
	cargo deny --all-features --locked check

	# Tests are only run if we are building for the current host, cross compiled tests obviously
	# won't work
	if [[ "$(TARGET)" == "" ]]; then
		cargo test --release --all-features --locked
	fi

	mkdir -p $(MAKE_DIR)

	# Build thirdparty license summary
	cargo about generate about.hbs --all-features -o $(MAKE_DIR)/thirdparty-licenses.html

	# Build binaries
	cargo build --release

	# Post process binaries
	cp -af target/$(TARGET)/release/$(MANAGEMENT_BIN) $(MAKE_DIR)
	$(BIN_UTIL_PREFIX)objcopy --only-keep-debug $(MAKE_DIR)/$(MANAGEMENT_BIN) $(MAKE_DIR)/$(MANAGEMENT_BIN).debug
	$(BIN_UTIL_PREFIX)strip -s $(MAKE_DIR)/$(MANAGEMENT_BIN)

	# Build packages
	cargo deb --no-build -p mgmtd -o $(MAKE_DIR)/packages/ --deb-version="$(VERSION_TRIMMED)"
	cargo deb --no-build -p mgmtd -o $(MAKE_DIR)/packages/ --deb-version="$(VERSION_TRIMMED)" --variant=debuginfo
	# We add a license field since generate-rpm fails if it is not there (even if license-file is given)
	cargo generate-rpm -p mgmtd -o $(MAKE_DIR)/packages/ \
		--set-metadata="version = \"$(VERSION_TRIMMED)\"" \
		--set-metadata="license = \"BeeGFS EULA\""
	cargo generate-rpm -p mgmtd -o $(MAKE_DIR)/packages/ --variant=debuginfo \
		--set-metadata="version = \"$(VERSION_TRIMMED)\"" \
		--set-metadata="license = \"BeeGFS EULA\""



### Utilities ###

# Quickly installs the newest versions most of the tools and components used by this repo. Requires
# `rustup` to be installed. You can install that by either going to `https://rustup.rs` and use the
# script there or install the version from your distros package manager if available.
#
# IMPORTANT: Only installs the tools needed for a local, native build. Not sufficient for cross
# compiling or using special C compilers like `musl`.
.PHONY: install-tools
install-tools:
	rustup update
	rustup toolchain install nightly
	rustup component add --toolchain nightly rustfmt
	cargo install --locked cargo-deny cargo-about cargo-generate-rpm cargo-deb
