# OxiDex Justfile
# Run `just` to see available commands
# Run `just <command>` to execute a command

# Default command when running `just` with no arguments
default:
    @just --list

# Run all tests (matches CI exactly)
# Note: Doctests are run separately without --release due to panic='abort' incompatibility
test:
    @echo "Running all tests (matching CI)..."
    cargo test --release --all-features --features tag-comparison-binary --lib --bins --tests
    @echo "Running doctests (requires panic=unwind)..."
    cargo test --all-features --features tag-comparison-binary --doc

# Run all tests with cargo-nextest (faster parallel execution)
test-nextest:
    @echo "Running all tests with nextest..."
    cargo nextest run --release --all-features

# Run tests in debug mode
test-debug:
    @echo "Running tests in debug mode..."
    cargo test --workspace

# Run tests with output capture disabled
test-nocapture:
    @echo "Running all tests with output..."
    cargo test --release --verbose --all-features -- --nocapture --test-threads=1

# Run only unit tests
test-unit:
    @echo "Running unit tests..."
    cargo test --lib --workspace

# Run only integration tests (excludes comparison tests)
test-integration:
    @echo "Running integration tests..."
    cargo test --test integration --release

# Run C FFI integration test
test-ffi-c:
    @echo "Running C FFI integration test..."
    cargo test --test ffi_c_integration -- --nocapture

# Run ExifTool comparison tests (requires ExifTool installed)
test-comparison:
    @echo "Running ExifTool comparison tests..."
    @echo "Note: Requires 'exiftool' command to be available"
    cargo test --release --features exiftool-comparison -- --nocapture

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
    cargo test -p oxidex-tags -p oxidex-tags-core -p oxidex-tags-camera -p oxidex-tags-media -p oxidex-tags-image -p oxidex-tags-document -p oxidex-tags-specialty -p oxidex-tags-shared

# Build the project in debug mode
build:
    @echo "Building project (debug)..."
    cargo build --workspace

# Build the project in release mode (matches CI configuration)
build-release: cbindgen-check
    @echo "Building project (release, matching CI)..."
    cargo build --release --all-features

# Build just the binary
build-bin:
    @echo "Building binary..."
    cargo build --bin oxidex

# Build release binary
build-bin-release:
    @echo "Building release binary..."
    cargo build --bin oxidex --release

# Check the project for errors without building
check:
    @echo "Checking project..."
    cargo check --workspace

# Check with all features
check-all:
    @echo "Checking project with all features..."
    cargo check --workspace --all-features

# Run clippy linter (dev profile)
lint:
    @echo "Running clippy (dev profile)..."
    cargo clippy --all-features -- -D warnings

# Run clippy linter (release profile - shares artifacts with build-release)
lint-release:
    @echo "Running clippy (release profile)..."
    cargo clippy --release --all-features -- -D warnings

# Fix clippy warnings automatically
lint-fix:
    @echo "Running clippy with fixes..."
    cargo clippy --workspace --all-targets --fix --allow-dirty --allow-staged

# Format code with rustfmt
fmt:
    @echo "Formatting code..."
    @cargo fmt --all 2>&1 | grep -v "^Warning:" || true

# Check if code is formatted
fmt-check:
    @echo "Checking code formatting..."
    @cargo fmt --all -- --check 2>&1 | grep -v "^Warning:" || test $? -eq 1

# Clean build artifacts
clean:
    @echo "Cleaning build artifacts..."
    cargo clean

# Run the binary with arguments
run *args:
    @echo "Running oxidex..."
    cargo run --bin oxidex -- {{args}}

# Run the release binary with arguments
run-release *args:
    @echo "Running oxidex (release)..."
    cargo run --bin oxidex --release -- {{args}}

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

# Profiling
# ---------

# Simple text-based profiling (recommended, accessible)
profile-simple:
    @echo "Running text-based performance profiling..."
    @./scripts/profile_simple.sh

# Profile with flamegraph (requires sudo on macOS, generates SVG)
profile-flamegraph benchmark:
    @echo "Generating flamegraph for {{benchmark}}..."
    @echo "Note: Requires sudo on macOS. Use profile-simple for accessible alternative."
    cargo flamegraph --bench parse_benchmarks --root -o flamegraph-{{benchmark}}.svg -- --bench {{benchmark}}
    @echo "Flamegraph saved to: flamegraph-{{benchmark}}.svg"
    @echo "Convert to text: python3 scripts/parse_flamegraph.py flamegraph-{{benchmark}}.svg"

# Convert flamegraph SVG to accessible text
flamegraph-to-text svg:
    @echo "Converting flamegraph to accessible text..."
    python3 scripts/parse_flamegraph.py {{svg}}

# Profile a specific benchmark with samply
profile benchmark:
    @echo "Profiling {{benchmark}} benchmark..."
    samply record cargo bench --bench parse_benchmarks {{benchmark}}

# Profile integration benchmarks
profile-integration benchmark:
    @echo "Profiling integration benchmark: {{benchmark}}..."
    samply record cargo bench --bench integration_benchmarks {{benchmark}}

# Profile the CLI binary with arguments
profile-bin *args:
    @echo "Profiling binary with args: {{args}}..."
    cargo build --release
    samply record ./target/release/oxidex {{args}}

# Profile all parse benchmarks (warning: takes a while)
profile-all:
    @echo "Profiling all parse benchmarks..."
    samply record cargo bench --bench parse_benchmarks

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

# Check for unused dependencies (requires cargo-udeps and nightly)
udeps:
    @echo "Checking for unused dependencies..."
    cargo +nightly udeps --all-targets --all-features

# Install the binary locally
install:
    @echo "Installing oxidex..."
    cargo install --path .

# Uninstall the binary
uninstall:
    @echo "Uninstalling oxidex..."
    cargo uninstall oxidex

# Build Debian package (requires cargo-deb)
deb:
    @echo "Building Debian package..."
    cargo deb

# Build RPM package (requires cargo-generate-rpm)
rpm:
    @echo "Building RPM package..."
    cargo build --release
    cargo generate-rpm

# Run CI checks (optimized with nextest + merged doctests)
# Edition 2024 merges doctests into single binary (~36s vs ~3min)
ci:
    #!/usr/bin/env bash
    set -euo pipefail

    echo "🚀 Running CI checks..."
    START_TIME=$(date +%s)

    # Step 1: Format check (fast, run first to fail early)
    echo ""
    echo "📝 Checking code formatting..."
    if ! cargo fmt --all -- --check 2>&1 | grep -v "^Warning:"; then
        # grep returns 1 if no lines matched (which is success for fmt --check)
        # But if cargo fmt actually failed, we need to check
        cargo fmt --all -- --check 2>/dev/null || { echo "❌ Format check failed"; exit 1; }
    fi

    # Step 2: Clippy (builds release artifacts that nextest will reuse)
    echo ""
    echo "🔍 Running clippy (release profile)..."
    cargo clippy --release --all-features -- -D warnings

    # Step 3: Build all targets including test binaries
    echo ""
    echo "🔨 Building all targets (release)..."
    cargo build --release --all-features --all-targets

    # Step 4: Run nextest and doc tests in PARALLEL
    # Edition 2024 merges doctests into single binary (~36s vs ~3min in 2021)
    echo ""
    echo "🧪 Running tests (nextest + doc tests in parallel)..."

    # Create temp file for doc test output
    DOC_OUTPUT=$(mktemp)
    trap "rm -f $DOC_OUTPUT" EXIT

    # Start doc tests in background (fast with edition 2024 merged doctests)
    cargo test --doc --release --all-features > "$DOC_OUTPUT" 2>&1 &
    DOC_PID=$!

    # Run nextest in foreground
    cargo nextest run --release --all-features --no-fail-fast

    # Wait for doc tests
    echo ""
    echo "📚 Waiting for doc tests..."
    if wait $DOC_PID; then
        grep -E "^test result:|merged doctests" "$DOC_OUTPUT" || true
    else
        echo "❌ Doc tests failed:"
        cat "$DOC_OUTPUT"
        exit 1
    fi

    # Step 5: Run C FFI integration test
    echo ""
    echo "Running C FFI integration test..."
    cargo test --test ffi_c_integration -- --nocapture

    END_TIME=$(date +%s)
    ELAPSED=$((END_TIME - START_TIME))

    echo ""
    echo "✅ All CI checks passed in ${ELAPSED}s!"
    echo "   ✓ Format check"
    echo "   ✓ Clippy (release profile)"
    echo "   ✓ Build (release with all features)"
    echo "   ✓ Tests (nextest + doc tests)"
    echo "   ✓ C FFI integration test"

# Run CI without nextest (fallback if nextest not installed)
ci-standard: fmt-check lint-release build-release test test-ffi-c
    @echo "All CI checks passed!"
    @echo "✓ Format check"
    @echo "✓ Clippy (release profile)"
    @echo "✓ Build (release with all features)"
    @echo "✓ Tests (cargo test)"
    @echo "✓ C FFI integration test"

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

# macOS packaging
# ----------------

# Create macOS DMG installer
create-dmg version:
    @echo "Creating DMG for {{version}}..."
    mkdir -p dist/dmg-contents
    cp target/release/oxidex dist/dmg-contents/
    create-dmg \
      --volname "OxiDex {{version}}" \
      --no-internet-enable \
      --skip-jenkins \
      "dist/oxidex-{{version}}.dmg" \
      "dist/dmg-contents/"
    rm -rf dist/dmg-contents
    @echo "DMG created at dist/oxidex-{{version}}.dmg"

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

# C FFI header generation
# -----------------------

# Regenerate C header file (requires cbindgen)
cbindgen:
    @echo "Regenerating C header..."
    cbindgen --config cbindgen.toml --crate oxidex --output api/oxidex.h
    @echo "C header updated at api/oxidex.h"

# Verify C header is up-to-date
cbindgen-check:
    @echo "Checking C header is up-to-date..."
    cbindgen --config cbindgen.toml --crate oxidex --output api/oxidex.h.tmp
    diff -q api/oxidex.h api/oxidex.h.tmp || (rm api/oxidex.h.tmp && echo "C header out of date! Run 'just cbindgen'" && exit 1)
    rm api/oxidex.h.tmp
    @echo "C header is up-to-date"

# Documentation
# -------------

# Regenerate tag domain documentation
docs-generate-tags:
    @echo "Regenerating tag domain documentation..."
    cargo run -p oxidex-tags --example render_domain -- core docs/tag-domains/core.md
    cargo run -p oxidex-tags --example render_domain -- camera docs/tag-domains/camera.md
    cargo run -p oxidex-tags --example render_domain -- media docs/tag-domains/media.md
    cargo run -p oxidex-tags --example render_domain -- image docs/tag-domains/image.md
    cargo run -p oxidex-tags --example render_domain -- document docs/tag-domains/document.md
    cargo run -p oxidex-tags --example render_domain -- specialty docs/tag-domains/specialty.md

# Regenerate tag coverage analysis report
docs-coverage:
    @echo "Regenerating tag coverage analysis..."
    uv run scripts/generate_tag_coverage.py --output docs/reference/tag-coverage-analysis.md
    @echo "Tag coverage report updated"

# ExifTool Comparison
# -------------------

# Run tag comparison against ExifTool's full test suite
# Downloads ExifTool to /tmp, runs comparison, then cleans up
compare-exiftool:
    #!/usr/bin/env bash
    set -euo pipefail

    EXIFTOOL_DIR="/tmp/exiftool-test-$$"

    cleanup() {
        echo "🧹 Cleaning up..."
        rm -rf "$EXIFTOOL_DIR"
        rm -f /tmp/exiftool-*.tar.gz
    }
    trap cleanup EXIT

    echo "📥 Fetching latest ExifTool version..."
    # Try exiftool.org first with User-Agent, fall back to GitHub tags API
    VERSION=$(curl -sA "OxiDex/1.0" https://exiftool.org/ver.txt 2>/dev/null | grep -E '^[0-9]+\.[0-9]+$' || \
              curl -s https://api.github.com/repos/exiftool/exiftool/tags 2>/dev/null | grep -m1 '"name"' | sed 's/.*"name": *"\([^"]*\)".*/\1/')
    if [[ ! "$VERSION" =~ ^[0-9]+\.[0-9]+$ ]]; then
        echo "   ❌ Failed to fetch ExifTool version from exiftool.org and GitHub API"
        exit 1
    fi
    echo "   Version: $VERSION"

    echo "📦 Downloading ExifTool $VERSION..."
    curl -L "https://github.com/exiftool/exiftool/archive/refs/tags/$VERSION.tar.gz" \
        -o "/tmp/exiftool-$VERSION.tar.gz" --progress-bar

    echo "📂 Extracting to $EXIFTOOL_DIR..."
    mkdir -p "$EXIFTOOL_DIR"
    tar -xzf "/tmp/exiftool-$VERSION.tar.gz" -C "$EXIFTOOL_DIR" --strip-components=1

    TEST_FILES=$(find "$EXIFTOOL_DIR/t/images" -type f 2>/dev/null | wc -l | tr -d ' ')
    echo "   Found $TEST_FILES test files"

    echo "🔨 Building tag-comparison tool..."
    cargo build --release --bin tag-comparison --features tag-comparison-binary

    OXIDEX_VERSION=$(grep '^version' Cargo.toml | head -1 | sed 's/.*"\(.*\)".*/\1/')

    echo "🔍 Running comparison..."
    echo "   ExifTool: v$VERSION"
    echo "   OxiDex:   v$OXIDEX_VERSION"
    echo ""

    ./target/release/tag-comparison \
        --exiftool "$EXIFTOOL_DIR/exiftool" \
        --samples "$EXIFTOOL_DIR/t/images" \
        --exiftool-version "$VERSION" \
        --oxidex-version "$OXIDEX_VERSION"

    echo ""
    echo "✅ Comparison complete!"

# Run comparison and update docs (like CI does)
compare-exiftool-update:
    #!/usr/bin/env bash
    set -euo pipefail

    EXIFTOOL_DIR="/tmp/exiftool-test-$$"

    cleanup() {
        echo "🧹 Cleaning up..."
        rm -rf "$EXIFTOOL_DIR"
        rm -f /tmp/exiftool-*.tar.gz
    }
    trap cleanup EXIT

    echo "📥 Fetching latest ExifTool version..."
    # Try exiftool.org first with User-Agent, fall back to GitHub tags API
    VERSION=$(curl -sA "OxiDex/1.0" https://exiftool.org/ver.txt 2>/dev/null | grep -E '^[0-9]+\.[0-9]+$' || \
              curl -s https://api.github.com/repos/exiftool/exiftool/tags 2>/dev/null | grep -m1 '"name"' | sed 's/.*"name": *"\([^"]*\)".*/\1/')
    if [[ ! "$VERSION" =~ ^[0-9]+\.[0-9]+$ ]]; then
        echo "   ❌ Failed to fetch ExifTool version from exiftool.org and GitHub API"
        exit 1
    fi
    echo "   Version: $VERSION"

    echo "📦 Downloading ExifTool $VERSION..."
    curl -L "https://github.com/exiftool/exiftool/archive/refs/tags/$VERSION.tar.gz" \
        -o "/tmp/exiftool-$VERSION.tar.gz" --progress-bar

    echo "📂 Extracting to $EXIFTOOL_DIR..."
    mkdir -p "$EXIFTOOL_DIR"
    tar -xzf "/tmp/exiftool-$VERSION.tar.gz" -C "$EXIFTOOL_DIR" --strip-components=1

    TEST_FILES=$(find "$EXIFTOOL_DIR/t/images" -type f 2>/dev/null | wc -l | tr -d ' ')
    echo "   Found $TEST_FILES test files"

    echo "🔨 Building tag-comparison tool..."
    cargo build --release --bin tag-comparison --features tag-comparison-binary

    OXIDEX_VERSION=$(grep '^version' Cargo.toml | head -1 | sed 's/.*"\(.*\)".*/\1/')

    echo "🔍 Running comparison and updating docs..."
    echo "   ExifTool: v$VERSION"
    echo "   OxiDex:   v$OXIDEX_VERSION"
    echo ""

    ./target/release/tag-comparison \
        --exiftool "$EXIFTOOL_DIR/exiftool" \
        --samples "$EXIFTOOL_DIR/t/images" \
        --baseline docs/reference/comparison/baseline.json \
        --output docs/reference/comparison/comparison.json \
        --markdown-dir docs/reference/comparison \
        --exiftool-version "$VERSION" \
        --oxidex-version "$OXIDEX_VERSION"

    echo ""
    echo "✅ Comparison complete! Docs updated in docs/reference/comparison/"

# Run comparison for a specific format only
compare-exiftool-format format:
    #!/usr/bin/env bash
    set -euo pipefail

    EXIFTOOL_DIR="/tmp/exiftool-test-$$"

    cleanup() {
        rm -rf "$EXIFTOOL_DIR"
        rm -f /tmp/exiftool-*.tar.gz
    }
    trap cleanup EXIT

    # Try exiftool.org first with User-Agent, fall back to GitHub tags API
    VERSION=$(curl -sA "OxiDex/1.0" https://exiftool.org/ver.txt 2>/dev/null | grep -E '^[0-9]+\.[0-9]+$' || \
              curl -s https://api.github.com/repos/exiftool/exiftool/tags 2>/dev/null | grep -m1 '"name"' | sed 's/.*"name": *"\([^"]*\)".*/\1/')
    if [[ ! "$VERSION" =~ ^[0-9]+\.[0-9]+$ ]]; then
        echo "   ❌ Failed to fetch ExifTool version"
        exit 1
    fi
    echo "📦 Downloading ExifTool $VERSION..."
    curl -sL "https://github.com/exiftool/exiftool/archive/refs/tags/$VERSION.tar.gz" \
        -o "/tmp/exiftool-$VERSION.tar.gz"

    mkdir -p "$EXIFTOOL_DIR"
    tar -xzf "/tmp/exiftool-$VERSION.tar.gz" -C "$EXIFTOOL_DIR" --strip-components=1

    cargo build --release --bin tag-comparison 2>/dev/null

    OXIDEX_VERSION=$(grep '^version' Cargo.toml | head -1 | sed 's/.*"\(.*\)".*/\1/')

    echo "🔍 Running {{format}} comparison (ExifTool v$VERSION, OxiDex v$OXIDEX_VERSION)..."
    echo ""

    ./target/release/tag-comparison \
        --exiftool "$EXIFTOOL_DIR/exiftool" \
        --samples "$EXIFTOOL_DIR/t/images" \
        --format "{{format}}" \
        --exiftool-version "$VERSION" \
        --oxidex-version "$OXIDEX_VERSION"

# Run comparison against ExifTool sample database (camera manufacturer samples)
# Downloads from exiftool.org/sample_images.html - 7,106 camera models from 109 manufacturers
# Falls back to GCS cache at gs://oxidex-samples/exiftool/ if exiftool.org is unavailable
compare-exiftool-samples:
    #!/usr/bin/env bash
    set -euo pipefail

    EXIFTOOL_DIR="/tmp/exiftool-test-$$"
    SAMPLES_DIR="/tmp/exiftool-samples-$$"
    GCS_BUCKET="https://storage.googleapis.com/oxidex-samples/exiftool"

    cleanup() {
        echo "🧹 Cleaning up..."
        rm -rf "$EXIFTOOL_DIR" "$SAMPLES_DIR"
        rm -f /tmp/exiftool-*.tar.gz /tmp/sample-*.tar.gz
    }
    trap cleanup EXIT

    echo "📥 Fetching latest ExifTool version..."
    # Try exiftool.org first with User-Agent, fall back to GitHub tags API
    VERSION=$(curl -sA "OxiDex/1.0" https://exiftool.org/ver.txt 2>/dev/null | grep -E '^[0-9]+\.[0-9]+$' || \
              curl -s https://api.github.com/repos/exiftool/exiftool/tags 2>/dev/null | grep -m1 '"name"' | sed 's/.*"name": *"\([^"]*\)".*/\1/')
    if [[ ! "$VERSION" =~ ^[0-9]+\.[0-9]+$ ]]; then
        echo "   ❌ Failed to fetch ExifTool version from exiftool.org and GitHub API"
        exit 1
    fi
    echo "   Version: $VERSION"

    echo "📦 Downloading ExifTool $VERSION..."
    curl -L "https://github.com/exiftool/exiftool/archive/refs/tags/$VERSION.tar.gz" \
        -o "/tmp/exiftool-$VERSION.tar.gz" --progress-bar

    echo "📂 Extracting ExifTool..."
    mkdir -p "$EXIFTOOL_DIR"
    tar -xzf "/tmp/exiftool-$VERSION.tar.gz" -C "$EXIFTOOL_DIR" --strip-components=1

    echo "📥 Downloading ExifTool sample database..."
    mkdir -p "$SAMPLES_DIR"

    # Download key manufacturer samples (most common cameras)
    # Try exiftool.org first, fall back to GCS cache
    MANUFACTURERS="Canon Nikon Sony FujiFilm Panasonic Apple Google Samsung Olympus Pentax Leica DJI GoPro"
    for mfr in $MANUFACTURERS; do
        echo "   Downloading $mfr samples..."
        # Try exiftool.org first
        if curl -sLA "OxiDex/1.0" --fail "https://exiftool.org/$mfr.tar.gz" -o "/tmp/sample-$mfr.tar.gz" 2>/dev/null; then
            tar -xzf "/tmp/sample-$mfr.tar.gz" -C "$SAMPLES_DIR" 2>/dev/null || true
            rm -f "/tmp/sample-$mfr.tar.gz"
        # Fall back to GCS cache
        elif curl -sL --fail "$GCS_BUCKET/$mfr.tar.gz" -o "/tmp/sample-$mfr.tar.gz" 2>/dev/null; then
            echo "      (using GCS cache)"
            tar -xzf "/tmp/sample-$mfr.tar.gz" -C "$SAMPLES_DIR" 2>/dev/null || true
            rm -f "/tmp/sample-$mfr.tar.gz"
        else
            echo "      ⚠️  $mfr samples unavailable"
        fi
    done

    SAMPLE_COUNT=$(find "$SAMPLES_DIR" -type f \( -name "*.jpg" -o -name "*.JPG" -o -name "*.jpeg" -o -name "*.JPEG" -o -name "*.tif" -o -name "*.TIF" -o -name "*.cr2" -o -name "*.CR2" -o -name "*.nef" -o -name "*.NEF" -o -name "*.arw" -o -name "*.ARW" -o -name "*.raf" -o -name "*.RAF" -o -name "*.dng" -o -name "*.DNG" -o -name "*.heic" -o -name "*.HEIC" \) 2>/dev/null | wc -l | tr -d ' ')
    echo "   Downloaded $SAMPLE_COUNT sample images"

    echo "🔨 Building tag-comparison tool..."
    cargo build --release --bin tag-comparison --features tag-comparison-binary

    OXIDEX_VERSION=$(grep '^version' Cargo.toml | head -1 | sed 's/.*"\(.*\)".*/\1/')

    echo "🔍 Running comparison against sample database..."
    echo "   ExifTool: v$VERSION"
    echo "   OxiDex:   v$OXIDEX_VERSION"
    echo ""

    ./target/release/tag-comparison \
        --exiftool "$EXIFTOOL_DIR/exiftool" \
        --samples "$SAMPLES_DIR" \
        --exiftool-version "$VERSION" \
        --oxidex-version "$OXIDEX_VERSION"

    echo ""
    echo "✅ Sample database comparison complete!"

# Run comparison against both test suite AND sample database (comprehensive)
# Falls back to GCS cache at gs://oxidex-samples/exiftool/ if exiftool.org is unavailable
# OPTIMIZED: Uses parallel downloads and caching
compare-exiftool-full:
    #!/usr/bin/env bash
    set -euo pipefail

    # Use fixed cache directory for reuse across runs
    CACHE_DIR="${EXIFTOOL_CACHE_DIR:-/tmp/oxidex-exiftool-cache}"
    EXIFTOOL_DIR="$CACHE_DIR/exiftool"
    # Persistent, not ephemeral: find_tag_gaps.py re-runs tag-comparison
    # directly against this same path from separate script invocations
    # after this recipe has already exited, so it must survive past this
    # shell's lifetime.
    COMBINED_DIR="$CACHE_DIR/combined-samples"
    GCS_BUCKET="https://storage.googleapis.com/oxidex-samples/exiftool"

    mkdir -p "$CACHE_DIR"

    echo "📥 Fetching latest ExifTool version..."
    # Try exiftool.org first with User-Agent, fall back to GitHub tags API
    VERSION=$(curl -sA "OxiDex/1.0" https://exiftool.org/ver.txt 2>/dev/null | grep -E '^[0-9]+\.[0-9]+$' || \
              curl -s https://api.github.com/repos/exiftool/exiftool/tags 2>/dev/null | grep -m1 '"name"' | sed 's/.*"name": *"\([^"]*\)".*/\1/')
    if [[ ! "$VERSION" =~ ^[0-9]+\.[0-9]+$ ]]; then
        echo "   ❌ Failed to fetch ExifTool version from exiftool.org and GitHub API"
        exit 1
    fi
    echo "   Version: $VERSION"

    # Check if ExifTool is already cached
    if [[ -f "$EXIFTOOL_DIR/exiftool" && -f "$CACHE_DIR/.exiftool-version" ]]; then
        CACHED_VERSION=$(cat "$CACHE_DIR/.exiftool-version")
        if [[ "$CACHED_VERSION" == "$VERSION" ]]; then
            echo "   ✓ Using cached ExifTool $VERSION"
        else
            echo "📦 Updating ExifTool from $CACHED_VERSION to $VERSION..."
            rm -rf "$EXIFTOOL_DIR"
            curl -sL "https://github.com/exiftool/exiftool/archive/refs/tags/$VERSION.tar.gz" | \
                tar -xzf - -C "$CACHE_DIR" && \
                mv "$CACHE_DIR/exiftool-$VERSION" "$EXIFTOOL_DIR"
            echo "$VERSION" > "$CACHE_DIR/.exiftool-version"
        fi
    else
        echo "📦 Downloading ExifTool $VERSION..."
        curl -sL "https://github.com/exiftool/exiftool/archive/refs/tags/$VERSION.tar.gz" | \
            tar -xzf - -C "$CACHE_DIR" && \
            mv "$CACHE_DIR/exiftool-$VERSION" "$EXIFTOOL_DIR"
        echo "$VERSION" > "$CACHE_DIR/.exiftool-version"
    fi

    # Create combined samples directory
    mkdir -p "$COMBINED_DIR"

    # Copy ExifTool test images
    echo "📋 Copying ExifTool test images..."
    cp -r "$EXIFTOOL_DIR/t/images"/* "$COMBINED_DIR/" 2>/dev/null || true

    # Download sample database IN PARALLEL - try exiftool.org first, fall back to GCS cache
    echo "📥 Downloading ExifTool sample database (parallel)..."
    MANUFACTURERS="Canon Nikon Sony FujiFilm Panasonic Apple Google Samsung Olympus Pentax Leica DJI GoPro"

    download_manufacturer() {
        local mfr="$1"
        local cache_dir="$2"
        local combined_dir="$3"
        local gcs_bucket="$4"
        local cache_file="$cache_dir/samples-$mfr.tar.gz"

        # Check cache first
        if [[ -f "$cache_file" ]]; then
            tar -xzf "$cache_file" -C "$combined_dir" 2>/dev/null || true
            echo "   ✓ $mfr (cached)"
            return 0
        fi

        # Try exiftool.org first
        if curl -sLA "OxiDex/1.0" --fail --connect-timeout 10 "https://exiftool.org/$mfr.tar.gz" -o "$cache_file" 2>/dev/null; then
            tar -xzf "$cache_file" -C "$combined_dir" 2>/dev/null || true
            echo "   ✓ $mfr"
            return 0
        fi

        # Fall back to GCS cache
        if curl -sL --fail --connect-timeout 10 "$gcs_bucket/$mfr.tar.gz" -o "$cache_file" 2>/dev/null; then
            tar -xzf "$cache_file" -C "$combined_dir" 2>/dev/null || true
            echo "   ✓ $mfr (GCS)"
            return 0
        fi

        echo "   ⚠️  $mfr unavailable"
        return 0
    }
    export -f download_manufacturer

    # Run downloads in parallel (up to 6 concurrent)
    echo "$MANUFACTURERS" | tr ' ' '\n' | \
        xargs -P 6 -I {} bash -c 'download_manufacturer "$@"' _ {} "$CACHE_DIR" "$COMBINED_DIR" "$GCS_BUCKET"

    TOTAL_FILES=$(find "$COMBINED_DIR" -type f 2>/dev/null | wc -l | tr -d ' ')
    echo "   Total files for comparison: $TOTAL_FILES"

    echo "🔨 Building tag-comparison tool..."
    cargo build --release --bin tag-comparison --features tag-comparison-binary 2>&1 | grep -v "^   Compiling" || true

    OXIDEX_VERSION=$(grep '^version' Cargo.toml | head -1 | sed 's/.*"\(.*\)".*/\1/')

    echo "🔍 Running comprehensive comparison..."
    echo "   ExifTool: v$VERSION"
    echo "   OxiDex:   v$OXIDEX_VERSION"
    echo ""

    ./target/release/tag-comparison \
        --exiftool "$EXIFTOOL_DIR/exiftool" \
        --samples "$COMBINED_DIR" \
        --exiftool-version "$VERSION" \
        --oxidex-version "$OXIDEX_VERSION"

    echo ""
    echo "✅ Comprehensive comparison complete!"

# Run full comparison and update docs (for CI)
# Falls back to GCS cache at gs://oxidex-samples/exiftool/ if exiftool.org is unavailable
# OPTIMIZED: Uses parallel downloads and caching
compare-exiftool-full-update:
    #!/usr/bin/env bash
    set -euo pipefail

    # Use fixed cache directory for reuse across runs
    CACHE_DIR="${EXIFTOOL_CACHE_DIR:-/tmp/oxidex-exiftool-cache}"
    EXIFTOOL_DIR="$CACHE_DIR/exiftool"
    COMBINED_DIR="/tmp/exiftool-combined-$$"
    GCS_BUCKET="https://storage.googleapis.com/oxidex-samples/exiftool"

    cleanup() {
        echo "🧹 Cleaning up temp files..."
        rm -rf "$COMBINED_DIR"
    }
    trap cleanup EXIT

    mkdir -p "$CACHE_DIR"

    echo "📥 Fetching latest ExifTool version..."
    # Try multiple sources for version with explicit error handling
    VERSION=""

    # Try exiftool.org first
    VERSION=$(curl -sA "OxiDex/1.0" --connect-timeout 10 --max-time 30 https://exiftool.org/ver.txt 2>/dev/null | grep -E '^[0-9]+\.[0-9]+$' || true)

    # Fall back to GitHub tags API (ExifTool doesn't use GitHub releases)
    if [[ -z "$VERSION" || ! "$VERSION" =~ ^[0-9]+\.[0-9]+$ ]]; then
        echo "   Trying GitHub tags API..."
        if [[ -n "${GITHUB_TOKEN:-}" ]]; then
            VERSION=$(curl -sL --connect-timeout 10 --max-time 30 \
                -H "Accept: application/vnd.github+json" \
                -H "Authorization: Bearer $GITHUB_TOKEN" \
                https://api.github.com/repos/exiftool/exiftool/tags 2>/dev/null | \
                grep -m1 '"name"' | sed 's/.*"name"[^"]*"\([^"]*\)".*/\1/' || true)
        else
            VERSION=$(curl -sL --connect-timeout 10 --max-time 30 \
                -H "Accept: application/vnd.github+json" \
                https://api.github.com/repos/exiftool/exiftool/tags 2>/dev/null | \
                grep -m1 '"name"' | sed 's/.*"name"[^"]*"\([^"]*\)".*/\1/' || true)
        fi
    fi

    if [[ -z "$VERSION" || ! "$VERSION" =~ ^[0-9]+\.[0-9]+$ ]]; then
        echo "   ❌ Failed to fetch ExifTool version from all sources"
        echo "   Tried: exiftool.org, GitHub tags API"
        exit 1
    fi
    echo "   Version: $VERSION"

    # Check if ExifTool is already cached
    if [[ -f "$EXIFTOOL_DIR/exiftool" && -f "$CACHE_DIR/.exiftool-version" ]]; then
        CACHED_VERSION=$(cat "$CACHE_DIR/.exiftool-version")
        if [[ "$CACHED_VERSION" == "$VERSION" ]]; then
            echo "   ✓ Using cached ExifTool $VERSION"
        else
            echo "📦 Updating ExifTool from $CACHED_VERSION to $VERSION..."
            rm -rf "$EXIFTOOL_DIR"
            curl -sL "https://github.com/exiftool/exiftool/archive/refs/tags/$VERSION.tar.gz" | \
                tar -xzf - -C "$CACHE_DIR" && \
                mv "$CACHE_DIR/exiftool-$VERSION" "$EXIFTOOL_DIR"
            echo "$VERSION" > "$CACHE_DIR/.exiftool-version"
        fi
    else
        echo "📦 Downloading ExifTool $VERSION..."
        curl -sL "https://github.com/exiftool/exiftool/archive/refs/tags/$VERSION.tar.gz" | \
            tar -xzf - -C "$CACHE_DIR" && \
            mv "$CACHE_DIR/exiftool-$VERSION" "$EXIFTOOL_DIR"
        echo "$VERSION" > "$CACHE_DIR/.exiftool-version"
    fi

    # Create combined samples directory
    mkdir -p "$COMBINED_DIR"

    # Copy ExifTool test images
    echo "📋 Copying ExifTool test images..."
    cp -r "$EXIFTOOL_DIR/t/images"/* "$COMBINED_DIR/" 2>/dev/null || true

    # Download sample database IN PARALLEL - try exiftool.org first, fall back to GCS cache
    echo "📥 Downloading ExifTool sample database (parallel)..."
    MANUFACTURERS="Canon Nikon Sony FujiFilm Panasonic Apple Google Samsung Olympus Pentax Leica DJI GoPro"

    download_manufacturer() {
        local mfr="$1"
        local cache_dir="$2"
        local combined_dir="$3"
        local gcs_bucket="$4"
        local cache_file="$cache_dir/samples-$mfr.tar.gz"

        # Check cache first
        if [[ -f "$cache_file" ]]; then
            tar -xzf "$cache_file" -C "$combined_dir" 2>/dev/null || true
            echo "   ✓ $mfr (cached)"
            return 0
        fi

        # Try exiftool.org first
        if curl -sLA "OxiDex/1.0" --fail --connect-timeout 10 "https://exiftool.org/$mfr.tar.gz" -o "$cache_file" 2>/dev/null; then
            tar -xzf "$cache_file" -C "$combined_dir" 2>/dev/null || true
            echo "   ✓ $mfr"
            return 0
        fi

        # Fall back to GCS cache
        if curl -sL --fail --connect-timeout 10 "$gcs_bucket/$mfr.tar.gz" -o "$cache_file" 2>/dev/null; then
            tar -xzf "$cache_file" -C "$combined_dir" 2>/dev/null || true
            echo "   ✓ $mfr (GCS)"
            return 0
        fi

        echo "   ⚠️  $mfr unavailable"
        return 0
    }
    export -f download_manufacturer

    # Run downloads in parallel (up to 6 concurrent)
    echo "$MANUFACTURERS" | tr ' ' '\n' | \
        xargs -P 6 -I {} bash -c 'download_manufacturer "$@"' _ {} "$CACHE_DIR" "$COMBINED_DIR" "$GCS_BUCKET"

    TOTAL_FILES=$(find "$COMBINED_DIR" -type f 2>/dev/null | wc -l | tr -d ' ')
    echo "   Total files for comparison: $TOTAL_FILES"

    echo "🔨 Building tag-comparison tool..."
    cargo build --release --bin tag-comparison --features tag-comparison-binary 2>&1 | grep -v "^   Compiling" || true

    OXIDEX_VERSION=$(grep '^version' Cargo.toml | head -1 | sed 's/.*"\(.*\)".*/\1/')

    echo "🔍 Running comprehensive comparison and updating docs..."
    echo "   ExifTool: v$VERSION"
    echo "   OxiDex:   v$OXIDEX_VERSION"
    echo ""

    # Ensure output directory exists
    mkdir -p docs/reference/comparison

    ./target/release/tag-comparison \
        --exiftool "$EXIFTOOL_DIR/exiftool" \
        --samples "$COMBINED_DIR" \
        --baseline docs/reference/comparison/baseline.json \
        --output docs/reference/comparison/comparison.json \
        --markdown-dir docs/reference/comparison \
        --exiftool-version "$VERSION" \
        --oxidex-version "$OXIDEX_VERSION"

    echo ""
    echo "✅ Comprehensive comparison complete! Docs updated in docs/reference/comparison/"
