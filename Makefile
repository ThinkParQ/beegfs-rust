SHELL = /bin/bash

# By default, we don't set a target and use the current default toolchain.
ifneq ($(TARGET),)
	export CARGO_BUILD_TARGET := $(TARGET)
	TARGET_FLAG := --target=$(TARGET)
endif

# Defines VERSION from the git history. Used by the binaries to build their own version string.
# To satisfy semver we fall back to `0.0.0` if there is no matching tag
VERSION := $(shell git describe --tags --match "v*.*.*" 2>/dev/null || echo "v0.0.0")
# Strip the first character, which is usually "v" to allow usage in semver contexts
VERSION_TRIMMED := $(shell V="$(VERSION)" && echo $${V:1})



### Simple cargo wrappers ###

all: build

# By setting TARGET (or CARGO_BUILD_TARGET directly) to a Rust target triple, the following commands
# can cross compile.

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

# Build the normal dev/debug profile
.PHONY: build
.ONESHELL: build
build:
	@set -xe
	VERSION="$(VERSION)" cargo build

.PHONY: clean
clean:
	cargo clean



### Release build and packaging ###

# Where to put the packages
PACKAGE_DIR := target/package

# Output dir of all artifacts, including the manually generated
TARGET_DIR := target/$(TARGET)/release

# Define the command to build the release binaries based on configuration
ifneq ($(TARGET),)
	ifneq ($(GLIBC_VERSION),)
		RELEASE_BUILD_CMD := cargo zigbuild --target=$(TARGET).$(GLIBC_VERSION)
	else
		RELEASE_BUILD_CMD := cargo build --target=$(TARGET)
	endif
else
	RELEASE_BUILD_CMD := cargo build
endif
# We want to include the full debug info so it can be split off
RELEASE_BUILD_CMD := VERSION="$(VERSION)" $(RELEASE_BUILD_CMD) \
	--release --locked --config='profile.release.debug = "full"'

# Build release binaries and package them.
# In addition to all environment variables accepted by cargo, this target reads the following:
# * TARGET: Define a build target for cross compiling by giving a Rust target triple. Requires
#   all the necessary tools (compilers, linkers, ...) to be installed
# * BIN_UTIL_PREFIX: The prefix to put before binutil commands when cross compiling (like strip) to
#   call the correct one.
# * GLIBC_VERSION: The glibc version to link against when applicable. This requires `cargo-zigbuild`
#   and the zig compiler to be installed and available.
.PHONY: package
.ONESHELL: package
package:
	@set -xe

	# Build thirdparty license summary
	mkdir -p $(TARGET_DIR)
	cargo about generate about.hbs --all-features -o $(TARGET_DIR)/thirdparty-licenses.html

	# Build after cleaning the binary generating crates to prevent accidental reuse of already
	# stripped binaries.
	cargo clean $(TARGET_FLAG) --release -p mgmtd
	$(RELEASE_BUILD_CMD)

	# Post process binaries
	$(BIN_UTIL_PREFIX)objcopy --only-keep-debug \
		$(TARGET_DIR)/beegfs-mgmtd $(TARGET_DIR)/beegfs-mgmtd.debug
	$(BIN_UTIL_PREFIX)strip -s $(TARGET_DIR)/beegfs-mgmtd

	# Build packages
	# These don't respect CARGO_BUILD_TARGET, so we need to add --target manually using $(TARGET_FLAG)
	cargo deb $(TARGET_FLAG) --no-build -p mgmtd -o $(PACKAGE_DIR)/ \
		--deb-version="$(VERSION_TRIMMED)"
	cargo deb $(TARGET_FLAG) --no-build -p mgmtd -o $(PACKAGE_DIR)/ --variant=debug \
		--deb-version="$(VERSION_TRIMMED)"
	# We add a license field since generate-rpm fails if it is not there (even if license-file is given)
	cargo generate-rpm $(TARGET_FLAG) -p mgmtd -o $(PACKAGE_DIR)/ \
		--set-metadata='version = "$(VERSION_TRIMMED)"' \
		--set-metadata='license = "BeeGFS EULA"'
	cargo generate-rpm $(TARGET_FLAG) -p mgmtd -o $(PACKAGE_DIR)/ --variant=debug \
		--set-metadata='version = "$(VERSION_TRIMMED)"' \
		--set-metadata='license = "BeeGFS EULA"'

.PHONY: clean-package
clean-package:
	rm -rf $(PACKAGE_DIR)


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
