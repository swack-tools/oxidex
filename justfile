# ExifTool-RS Justfile
# Run `just` to see available commands
# Run `just <command>` to execute a command

# Default command when running `just` with no arguments
default:
    @just --list

# Run all tests (unit, integration, doc tests)
test:
    @echo "Running all tests..."
    cargo test --workspace

# Run tests with output
test-verbose:
    @echo "Running all tests with verbose output..."
    cargo test --workspace -- --nocapture --test-threads=1

# Run only unit tests
test-unit:
    @echo "Running unit tests..."
    cargo test --lib --workspace

# Run only integration tests
test-integration:
    @echo "Running integration tests..."
    cargo test --test integration

# Run only doc tests
test-doc:
    @echo "Running doc tests..."
    cargo test --doc --workspace

# Run tests for specific package
test-package package:
    @echo "Running tests for {{package}}..."
    cargo test -p {{package}}

# Run tests for all tag crates
test-tags:
    @echo "Running tests for all tag crates..."
    cargo test -p exiftool-tags -p exiftool-tags-core -p exiftool-tags-camera -p exiftool-tags-media -p exiftool-tags-image -p exiftool-tags-document -p exiftool-tags-specialty

# Build the project in debug mode
build:
    @echo "Building project (debug)..."
    cargo build --workspace

# Build the project in release mode
build-release:
    @echo "Building project (release)..."
    cargo build --workspace --release

# Build just the binary
build-bin:
    @echo "Building binary..."
    cargo build --bin exiftool-rs

# Build release binary
build-bin-release:
    @echo "Building release binary..."
    cargo build --bin exiftool-rs --release

# Check the project for errors without building
check:
    @echo "Checking project..."
    cargo check --workspace

# Check with all features
check-all:
    @echo "Checking project with all features..."
    cargo check --workspace --all-features

# Run clippy linter
lint:
    @echo "Running clippy..."
    cargo clippy --workspace --all-targets -- -D warnings

# Fix clippy warnings automatically
lint-fix:
    @echo "Running clippy with fixes..."
    cargo clippy --workspace --all-targets --fix --allow-dirty --allow-staged

# Format code with rustfmt
fmt:
    @echo "Formatting code..."
    cargo fmt --all

# Check if code is formatted
fmt-check:
    @echo "Checking code formatting..."
    cargo fmt --all -- --check

# Clean build artifacts
clean:
    @echo "Cleaning build artifacts..."
    cargo clean

# Clean and rebuild
rebuild: clean build

# Run the binary with arguments
run *args:
    @echo "Running exiftool-rs..."
    cargo run --bin exiftool-rs -- {{args}}

# Run the release binary with arguments
run-release *args:
    @echo "Running exiftool-rs (release)..."
    cargo run --bin exiftool-rs --release -- {{args}}

# Generate and open documentation
docs:
    @echo "Generating documentation..."
    cargo doc --workspace --no-deps --open

# Generate documentation without opening
docs-build:
    @echo "Generating documentation..."
    cargo doc --workspace --no-deps

# Run benchmarks
bench:
    @echo "Running benchmarks..."
    cargo bench --workspace

# Update dependencies
update:
    @echo "Updating dependencies..."
    cargo update

# Check for outdated dependencies
outdated:
    @echo "Checking for outdated dependencies..."
    cargo outdated

# Run cargo audit for security vulnerabilities
audit:
    @echo "Auditing dependencies..."
    cargo audit

# Install the binary locally
install:
    @echo "Installing exiftool-rs..."
    cargo install --path .

# Uninstall the binary
uninstall:
    @echo "Uninstalling exiftool-rs..."
    cargo uninstall exiftool-rs

# Build Debian package (requires cargo-deb)
deb:
    @echo "Building Debian package..."
    cargo deb

# Build RPM package (requires cargo-generate-rpm)
rpm:
    @echo "Building RPM package..."
    cargo build --release
    cargo generate-rpm

# Run CI checks (test, lint, format check)
ci: fmt-check lint test
    @echo "All CI checks passed!"

# Pre-commit hook: format, lint, test
pre-commit: fmt lint test
    @echo "Pre-commit checks passed!"

# Coverage report (requires cargo-tarpaulin)
coverage:
    @echo "Generating coverage report..."
    cargo tarpaulin --out Html --output-dir coverage --workspace

# Watch for changes and run tests (requires cargo-watch)
watch:
    @echo "Watching for changes..."
    cargo watch -x test

# Watch for changes and run specific command
watch-run cmd:
    @echo "Watching for changes to run: {{cmd}}..."
    cargo watch -x "{{cmd}}"

# Bloat analysis (requires cargo-bloat)
bloat:
    @echo "Analyzing binary bloat..."
    cargo bloat --release -n 20

# Show crate dependency tree
tree:
    @echo "Showing dependency tree..."
    cargo tree

# Show workspace information
workspace:
    @echo "Workspace members:"
    @cargo metadata --format-version 1 --no-deps | jq -r '.workspace_members[]'

# Git commands
# -------------

# Create a new git commit with formatted message
commit message:
    @echo "Creating commit: {{message}}"
    git add -A
    git commit -m "{{message}}"

# Commit and push to current branch
push message:
    @echo "Committing and pushing: {{message}}"
    git add -A
    git commit -m "{{message}}"
    git push origin $(git branch --show-current)

# Show git status
status:
    @git status

# Show recent commits
log:
    @git log --oneline -10

# Tag management
# -------------

# Create and push a new version tag
tag version:
    @echo "Creating tag: v{{version}}"
    git tag -a "v{{version}}" -m "Release v{{version}}"
    git push origin "v{{version}}"

# Delete a tag locally and remotely
untag version:
    @echo "Deleting tag: v{{version}}"
    git tag -d "v{{version}}"
    git push origin :refs/tags/v{{version}}

# Release workflow
# ----------------

# Prepare for release: run all checks
release-check: ci
    @echo "Release checks passed!"
    @echo "Ready for release."

# Full release workflow: check, build release, create tag
release version: release-check
    @echo "Creating release v{{version}}..."
    cargo build --release
    just tag {{version}}
    @echo "Release v{{version}} created and tagged!"
