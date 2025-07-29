SHELL = /bin/bash
# Reuse the same shell for all commands in all recipes
.ONESHELL:
# Always error out early on any non zero result and echo all commands
.SHELLFLAGS = -cex
# Do not echo recipes
.SILENT:


# By default, we don't set a target and use the current default toolchain.
ifneq ($(CARGO_TARGET),)
    export CARGO_BUILD_TARGET := $(CARGO_TARGET)
    # CAUTION: Do not use tabs to indent here, because it changes the scoping to the local recipe
    # and doesn't make the variable globally available
    TARGET_FLAG := --target=$(CARGO_TARGET)
endif

# Defines VERSION from the git history. Used by the binaries to build their own version string.
# To satisfy semver we fall back to `0.0.0` if there is no matching tag
VERSION := $(shell (git describe --tags --match "v*.*.*" 2>/dev/null || echo "v0.0.0") | sed 's/\-/~/g')
# Strip the first character, which is usually "v" to allow usage in semver contexts
VERSION_TRIMMED := $(shell V="$(VERSION)" && echo $${V:1})

ifneq ($(CARGO_LOCKED),)
    # CAUTION: Do not use tabs to indent here, because it changes the scoping to the local recipe
    # and doesn't make the variable globally available
    LOCKED_FLAG := --locked
endif



### Simple cargo wrappers ###

all: build

# By setting TARGET (or CARGO_BUILD_TARGET directly) to a Rust target triple, the following commands
# can cross compile.

# Run quick sanity checks
.PHONY: check
check:
	cargo +nightly fmt --check
	cargo clippy $(LOCKED_FLAG) --all-features -- -D warnings

# Run cargo deny
.PHONY: deny
deny:
	cargo deny $(LOCKED_FLAG) --all-features check

# Run tests
.PHONY: test
test:
	cargo test $(LOCKED_FLAG) --all-features

# Build the normal dev/debug profile
.PHONY: build
build:
	VERSION="$(VERSION)" cargo build $(LOCKED_FLAG)

.PHONY: clean
clean:
	cargo clean $(LOCKED_FLAG)



### Release build and packaging ###

# Where to put the packages
PACKAGE_DIR := target/package

# Output dir of all artifacts, including the manually generated
TARGET_DIR := target/$(CARGO_TARGET)/release

# Define the command to build the release binaries based on configuration
ifneq ($(CARGO_TARGET),)
	ifneq ($(GLIBC_VERSION),)
		RELEASE_BUILD_CMD := cargo zigbuild --target=$(CARGO_TARGET).$(GLIBC_VERSION)
	else
		RELEASE_BUILD_CMD := cargo build --target=$(CARGO_TARGET)
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
package:
	# Build thirdparty license summary
	mkdir -p $(TARGET_DIR)
	cargo about generate about.hbs --all-features -o $(TARGET_DIR)/thirdparty-licenses.html

	# Build after cleaning the binary generating crates to prevent accidental reuse of already
	# stripped binaries.
	cargo clean --locked $(TARGET_FLAG) --release -p mgmtd
	$(RELEASE_BUILD_CMD)

	# Post process binaries
	$(BIN_UTIL_PREFIX)objcopy --only-keep-debug \
		$(TARGET_DIR)/beegfs-mgmtd $(TARGET_DIR)/beegfs-mgmtd.debug
	$(BIN_UTIL_PREFIX)strip -s $(TARGET_DIR)/beegfs-mgmtd

	# Build packages
	# These don't respect CARGO_BUILD_TARGET, so we need to add --target manually using $(TARGET_FLAG)
	cargo deb --locked $(TARGET_FLAG) --no-build -p mgmtd -o $(PACKAGE_DIR)/ \
		--deb-version="20:$(VERSION_TRIMMED)" 
	cargo deb --locked $(TARGET_FLAG) --no-build -p mgmtd -o $(PACKAGE_DIR)/ --variant=debug \
		--deb-version="20:$(VERSION_TRIMMED)" 

	# We don't want the epoch in the file names
	find $(PACKAGE_DIR) -name "*_20:*.deb" -exec bash -c 'mv "$$1" $$(echo "$$1" | sed "s/_20:/_/g")' bash {} \;

	# We add a license field since generate-rpm fails if it is not there (even if license-file is given)
	cargo generate-rpm $(TARGET_FLAG) -p mgmtd -o $(PACKAGE_DIR)/ \
		--set-metadata='version="$(VERSION_TRIMMED)"' \
		--set-metadata='epoch=20' \
		--set-metadata='license="BeeGFS EULA"' \
		--set-metadata='provides={"beegfs-mgmtd" = "= $(VERSION_TRIMMED)"}'
	cargo generate-rpm $(TARGET_FLAG) -p mgmtd -o $(PACKAGE_DIR)/ --variant=debug \
		--set-metadata='version="$(VERSION_TRIMMED)"' \
		--set-metadata='epoch=20' \
		--set-metadata='license="BeeGFS EULA"' \
		--set-metadata='provides={"beegfs-mgmtd" = "= $(VERSION_TRIMMED)"}'
	
	# Replace tilde in package filename with hypens.
	# Github release action and api substitutes tilde (~) with dot (.) in file names when uploaded to Github packages. 
	find $(PACKAGE_DIR)/ -type f \( -name "*~*.deb" -o -name "*~*.rpm" \) -exec bash -c 'mv "$$1" "$${1//\~/-}"' _ {} \;

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
	cargo install --locked cargo-deny@0.18.3 cargo-about@0.7.1 cargo-generate-rpm@0.17.0 cargo-deb@3.3.0
