.PHONY: all build check test clippy fmt doc clean install publish publish-dry-run

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
	cargo install --path crates/arch-lint-cli

# Run all CI checks
preflight: fmt-check clippy test

# Publish dry-run (all crates in dependency order)
publish-dry-run:
	cargo publish -p arch-lint-macros --dry-run
	cargo publish -p arch-lint-core --dry-run
	cargo publish -p arch-lint-rules --dry-run
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
	cargo publish -p arch-lint-cli

# Bump version (usage: make bump-version VERSION=0.2.0)
bump-version:
ifndef VERSION
	$(error VERSION is required. Usage: make bump-version VERSION=0.2.0)
endif
	@echo "Bumping version to $(VERSION)"
	sed -i '' 's/^version = ".*"/version = "$(VERSION)"/' Cargo.toml
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
