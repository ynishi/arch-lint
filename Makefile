.PHONY: all build check test clippy fmt doc clean install publish publish-dry-run schema schema-check

# Default target
all: check test

# Build all crates
build:
	cargo build --workspace

# Build release
build-release:
	cargo build --workspace --release

# Type check
check:
	cargo check --workspace

# Run tests
test:
	cargo test --workspace

# Run clippy
clippy:
	cargo clippy --workspace -- -D warnings

# Format code
fmt:
	cargo fmt --all

# Check formatting
fmt-check:
	cargo fmt --all -- --check

# Generate documentation
doc:
	cargo doc --workspace --no-deps

# Open documentation in browser
doc-open:
	cargo doc --workspace --no-deps --open

# Clean build artifacts
clean:
	cargo clean

# Install CLI locally
install:
	cargo install --locked --path crates/arch-lint-cli

# Regenerate the committed JSON Schema (issue #16). Runs through `cargo run`
# rather than invoking `./target/debug/arch-lint` directly, since a
# `CARGO_TARGET_DIR` override would leave that path empty; `--bin arch-lint`
# is required because `cargo-arch-lint` shares the same `main.rs`, so `cargo
# run` cannot otherwise pick a default binary.
schema:
	cargo run -q -p arch-lint-cli --locked --bin arch-lint -- schema --out schema/arch-lint.schema.json

# Verify the committed JSON Schema has not drifted from the code: regenerate
# into a temp file and diff it against schema/arch-lint.schema.json byte for
# byte. Exits 1 (via diff) on drift. The temp file is removed on both
# outcomes, success and drift alike.
schema-check:
	cargo run -q -p arch-lint-cli --locked --bin arch-lint -- schema --out target/arch-lint.schema.json.check
	diff schema/arch-lint.schema.json target/arch-lint.schema.json.check; status=$$?; rm -f target/arch-lint.schema.json.check; exit $$status

# Run all CI checks
preflight: fmt-check clippy test

# Publish dry-run (all crates in dependency order)
publish-dry-run:
	cargo publish -p arch-lint-macros --dry-run
	cargo publish -p arch-lint-core --dry-run
	cargo publish -p arch-lint-rules --dry-run
	cargo publish -p arch-lint-ts --dry-run
	cargo publish -p arch-lint --dry-run
	cargo publish -p arch-lint-cli --dry-run

# Publish to crates.io (all crates in dependency order)
# Note: Run `cargo login` first if not already logged in
publish:
	cargo publish -p arch-lint-macros
	sleep 30  # Wait for crates.io to index
	cargo publish -p arch-lint-core
	sleep 30
	cargo publish -p arch-lint-rules
	sleep 30
	cargo publish -p arch-lint-ts
	sleep 30
	cargo publish -p arch-lint
	sleep 30
	cargo publish -p arch-lint-cli

# Bump version (usage: make bump-version VERSION=0.2.0)
# Updates [workspace.package].version and the internal crate versions in
# [workspace.dependencies]. Portable across GNU and BSD sed.
bump-version:
ifndef VERSION
	$(error VERSION is required. Usage: make bump-version VERSION=0.2.0)
endif
	@echo "Bumping version to $(VERSION)"
	sed -i.bak \
		-e 's/^version = ".*"/version = "$(VERSION)"/' \
		-e 's/^\(arch-lint[a-z-]* = { path = "[^"]*", version = "\)[^"]*"/\1$(VERSION)"/' \
		Cargo.toml
	rm -f Cargo.toml.bak
	cargo check --workspace
	@echo "Version bumped to $(VERSION). Don't forget to commit!"

# Show help
help:
	@echo "Available targets:"
	@echo "  all            - Run check and test (default)"
	@echo "  build          - Build all crates"
	@echo "  build-release  - Build all crates in release mode"
	@echo "  check          - Type check all crates"
	@echo "  test           - Run all tests"
	@echo "  clippy         - Run clippy lints"
	@echo "  fmt            - Format code"
	@echo "  fmt-check      - Check code formatting"
	@echo "  doc            - Generate documentation"
	@echo "  doc-open       - Generate and open documentation"
	@echo "  clean          - Clean build artifacts"
	@echo "  install        - Install CLI locally"
	@echo "  preflight      - Run all Preflight checks"
	@echo "  publish-dry-run- Test publishing without uploading"
	@echo "  publish        - Publish all crates to crates.io"
	@echo "  bump-version   - Bump version (VERSION=x.y.z required)"
	@echo "  help           - Show this help"
