# dawn build helper.
#
# Keep this file boring: every target maps to one cargo command. Installation is
# deliberately not a target. Copy the release binary where you want it, and let
# your package manager own the dependency and greetd setup.

CARGO ?= cargo
BIN := target/release/dawn
# Preview the neutral config by default. Point CONFIG at any example.
CONFIG ?= examples/config.toml
PREVIEW ?= dawn-preview.png

.PHONY: all build release test lint audit fmt fmt-check check-config preview clean help

all: build

# The release binary is what the greeter runs. `--locked` keeps CI and local
# builds on the versions recorded in Cargo.lock.
build:
	$(CARGO) build --release --locked

release: build

test:
	$(CARGO) test --locked

lint:
	$(CARGO) clippy --all-targets --locked -- -D warnings

# Same policy CI enforces: advisories, bans, licenses, and sources against
# deny.toml. Install the tool once with `cargo install cargo-deny --locked`.
audit:
	$(CARGO) deny check

fmt:
	$(CARGO) fmt

fmt-check:
	$(CARGO) fmt --check

check-config: build
	$(BIN) --config examples/config.toml --check-config

preview: build
	$(BIN) --config $(CONFIG) --screenshot $(PREVIEW) --size 1920x1080

clean:
	$(CARGO) clean

help:
	@printf '%s\n' \
		'dawn make targets:' \
		'  build            release build with Cargo.lock' \
		'  test             cargo test' \
		'  lint             cargo clippy with warnings denied' \
		'  audit            cargo deny check (advisories, licenses, sources)' \
		'  fmt              format the tree' \
		'  fmt-check        check formatting in CI' \
		'  check-config     validate the default example config' \
		'  preview          render CONFIG to PREVIEW' \
		'  clean            cargo clean' \
		'' \
		'Installation is intentionally not a make target. See docs/installation.md.'
