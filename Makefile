.DEFAULT_GOAL := help

CARGO ?= cargo
RUSTC ?= rustc
CARGO_TARGET_DIR ?= target
DIST_DIR ?= dist
INSTALL_DIR ?= $(HOME)/.local/bin
TARGET ?= $(shell $(RUSTC) -vV | sed -n 's/^host: //p')
VERSION := $(shell awk '/^\[workspace.package\]/{p=1;next} /^\[/{p=0} p && /^version = /{gsub(/"/, "", $$3);print $$3;exit}' Cargo.toml)
ARCHIVE := wtflow-$(VERSION)-$(TARGET).tar.gz

.PHONY: help release dist install

help:
	@printf '%s\n' \
	  'make help                 Show release commands and supported platforms' \
	  'make release              Build for the host and update ./wtflow' \
	  'make install              Build and install wtflow into ~/.local/bin' \
	  'make dist                 Build and package a release with a SHA-256 checksum' \
	  'make release TARGET=...   Build for a specific Rust target' \
	  'make dist TARGET=...      Package for a specific Rust target' \
	  '' \
	  'Release targets:' \
	  '  x86_64-unknown-linux-musl' \
	  '  aarch64-unknown-linux-musl' \
	  '  x86_64-apple-darwin' \
	  '  aarch64-apple-darwin' \
	  '' \
	  'Install the Rust target and its native compiler/linker before cross-building.' \
	  'Overrides: CARGO, RUSTC, CARGO_TARGET_DIR, DIST_DIR, INSTALL_DIR, TARGET'

release:
	@test -n "$(TARGET)" || { echo 'Cannot determine Rust target; set TARGET explicitly.' >&2; exit 1; }
	$(CARGO) build --release --locked -p wtflow-cli --target "$(TARGET)" --target-dir "$(CARGO_TARGET_DIR)"
	@if [ -e wtflow ] && [ ! -L wtflow ]; then echo 'Cannot create ./wtflow: an existing file or directory occupies that path.' >&2; exit 1; fi
	ln -sfn "$(CARGO_TARGET_DIR)/$(TARGET)/release/wtflow" wtflow

install: release
	mkdir -p "$(INSTALL_DIR)"
	install -m 755 "$(CARGO_TARGET_DIR)/$(TARGET)/release/wtflow" "$(INSTALL_DIR)/wtflow"
	@printf 'Installed wtflow to %s\n' "$(INSTALL_DIR)/wtflow"
	@case ":$$PATH:" in \
	  *":$(INSTALL_DIR):"*) ;; \
	  *) printf '%s\n' 'To use wtflow from any directory, run:' \
	       'export PATH="$(INSTALL_DIR):$$PATH"' \
	       'Add that line to ~/.zshrc or ~/.bashrc to keep it for new terminals.' ;; \
	esac

dist: release
	@test -n "$(VERSION)" || { echo 'Cannot read workspace version from Cargo.toml.' >&2; exit 1; }
	mkdir -p "$(DIST_DIR)"
	COPYFILE_DISABLE=1 tar -czf "$(DIST_DIR)/$(ARCHIVE)" -C "$(CARGO_TARGET_DIR)/$(TARGET)/release" wtflow
	cd "$(DIST_DIR)" && shasum -a 256 "$(ARCHIVE)" > "$(ARCHIVE).sha256"
	@printf 'Release archive: %s\n' "$(DIST_DIR)/$(ARCHIVE)"
