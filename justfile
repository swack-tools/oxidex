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

# Reject tests that read the pinned sample corpus without gating on its
# presence. The corpus is a local developer cache, absent on CI and in fresh
# clones, so an unguarded read passes here and panics there -- and because
# nextest is fail-fast, one panic aborts the whole suite.
check-corpus-guards:
    @python3 tools/ci/check-corpus-guards.py

# Run clippy linter (dev profile)
lint:
    @echo "Running clippy (dev profile)..."
    cargo clippy --all-features -- -D warnings
    @just check-corpus-guards
    @just check-tag-stats

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
    #!/usr/bin/env bash
    set -uo pipefail
    echo "Checking code formatting..."
    output=$(cargo fmt --all -- --check 2>&1)
    status=$?
    echo "$output" | grep -v "^Warning:" || true
    exit $status

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

# Build Debian package for Linux x86_64 using zigbuild (requires cargo-zigbuild, zig, cargo-deb)
deb-x86:
    @echo "Building Debian package for x86_64-unknown-linux-musl..."
    cargo zigbuild --release --target x86_64-unknown-linux-musl
    cargo deb --target x86_64-unknown-linux-musl --no-build
    @echo "Package created at target/x86_64-unknown-linux-musl/debian/"

# Build Debian package for Linux aarch64 using zigbuild (requires cargo-zigbuild, zig, cargo-deb)
deb-arm64:
    @echo "Building Debian package for aarch64-unknown-linux-musl..."
    cargo zigbuild --release --target aarch64-unknown-linux-musl
    cargo deb --target aarch64-unknown-linux-musl --no-build
    @echo "Package created at target/aarch64-unknown-linux-musl/debian/"

# Build Debian packages for all Linux architectures
deb-all: deb-x86 deb-arm64
    @echo "All Debian packages built!"
    @ls -la target/*/debian/*.deb 2>/dev/null || true

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

    # Step 2: C header freshness (cheap, and the check that broke main twice —
    # #211 and #256 both added a public constant without regenerating
    # api/oxidex.h while every one of the 3,830+ tests still passed).
    echo ""
    echo "📄 Checking C header is up-to-date..."
    just cbindgen-check

    # Step 3: Clippy (builds release artifacts that nextest will reuse)
    echo ""
    echo "🔍 Running clippy (release profile)..."
    cargo clippy --release --all-features -- -D warnings

    # Step 4: Build all targets including test binaries
    echo ""
    echo "🔨 Building all targets (release)..."
    cargo build --release --all-features --all-targets

    # Step 5: Run nextest and doc tests in PARALLEL
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

    # Step 6: Run C FFI integration test
    echo ""
    echo "Running C FFI integration test..."
    cargo test --test ffi_c_integration -- --nocapture

    END_TIME=$(date +%s)
    ELAPSED=$((END_TIME - START_TIME))

    echo ""
    echo "✅ All CI checks passed in ${ELAPSED}s!"
    echo "   ✓ Format check"
    echo "   ✓ C header up-to-date"
    echo "   ✓ Clippy (release profile)"
    echo "   ✓ Build (release with all features)"
    echo "   ✓ Tests (nextest + doc tests)"
    echo "   ✓ C FFI integration test"

# Run CI without nextest (fallback if nextest not installed)
ci-standard: fmt-check cbindgen-check lint-release build-release test test-ffi-c
    @echo "All CI checks passed!"
    @echo "✓ Format check"
    @echo "✓ C header up-to-date"
    @echo "✓ Clippy (release profile)"
    @echo "✓ Build (release with all features)"
    @echo "✓ Tests (cargo test)"
    @echo "✓ C FFI integration test"

# Pre-commit hook: format check, header check, lint, test
# cbindgen-check runs early because it is the cheapest of the four and is the
# check that broke main twice (#211, #256) by being absent from this list.
pre-commit: fmt-check cbindgen-check lint test
    @echo "Pre-commit checks passed!"

# Install git hooks (absolute core.hooksPath so linked worktrees resolve the
# shims too — a relative path silently disables all hooks in worktrees that
# lack a .githooks checkout)
install-hooks:
    @echo "Installing git hooks..."
    git config core.hooksPath "$(cd "$(git rev-parse --git-common-dir)/.." && pwd)/.githooks"
    @echo "Git hooks installed! Pre-commit will run fmt-check, cbindgen-check, lint, and test."
    @echo "Skip with OXIDEX_SKIP_HOOKS=1; linked worktrees are exempt from"
    @echo "everything except the (cheap) C header check."

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

# Autonomous fix fleet
# --------------------

# Bring the whole fix pipeline up, supervised (mergers + dispatcher + judgment queue)
# Runs in the FOREGROUND and supervises until ^C; see `just fleet-status` / `just fleet-down`.
# Extra args are forwarded, e.g. `just fleet-up "--workers 16"`.
fleet-up *ARGS:
    @./scripts/fleet_up.sh {{ARGS}}

# What the fleet is actually doing right now (pidfile-exact, not a pgrep guess)
fleet-status:
    @./scripts/fleet_up.sh --status

# Stop every tier this launcher started
fleet-down:
    @./scripts/fleet_up.sh --down

# Preflight + resolved plan without starting or touching anything
fleet-check:
    @./scripts/fleet_up.sh --dry-run

# Model-call failure rate from the manifests (the only logs carrying BOTH
# outcomes). Exits 2 rather than guessing when the rate cannot be measured.
# Default window is the last 30 minutes; pass e.g. "--last 2h" or an ISO cutoff.
fleet-failrate *ARGS='--last 30m':
    @python3 ./scripts/fleet_failrate.py {{ARGS}}

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

# cbindgen version pinned by CI — .github/workflows/ci.yml installs
# cbindgen@0.29.2. Newer cbindgen emits extra #define macros, so regenerating
# with a mismatched local binary writes a header CI then rejects. Keep the two
# in sync; changing this value means changing ci.yml too.
cbindgen_version := "0.29.2"

# Refuse to generate or verify with a cbindgen that disagrees with CI. Without
# this, `just cbindgen` on a newer toolchain "fixes" the header locally and
# breaks it in CI, which looks identical to the bug it was meant to fix.
_cbindgen-version-guard:
    #!/usr/bin/env bash
    set -euo pipefail
    if ! command -v cbindgen >/dev/null 2>&1; then
        echo "cbindgen is not installed. CI pins {{cbindgen_version}}:" >&2
        echo "  cargo install cbindgen --version {{cbindgen_version}} --locked" >&2
        exit 1
    fi
    have=$(cbindgen --version 2>/dev/null | awk '{print $2}')
    if [ "$have" != "{{cbindgen_version}}" ]; then
        echo "cbindgen $have is installed but CI pins {{cbindgen_version}}." >&2
        echo "Different versions emit different headers, so this check cannot" >&2
        echo "tell you what CI will see. Install the pinned version:" >&2
        echo "  cargo install cbindgen --version {{cbindgen_version}} --locked --force" >&2
        exit 1
    fi

# Regenerate C header file (requires cbindgen)
cbindgen: _cbindgen-version-guard
    @echo "Regenerating C header..."
    cbindgen --config cbindgen.toml --crate oxidex --output api/oxidex.h
    @echo "C header updated at api/oxidex.h"

# Verify C header is up-to-date
cbindgen-check: _cbindgen-version-guard
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

# Regenerate tag coverage analysis (measured vs pinned ExifTool; OXIDEX_DEEP_CORPUS=1 for the deep corpus)
docs-coverage:
    #!/usr/bin/env bash
    set -euo pipefail
    # OXIDEX_DEEP_CORPUS=1 additionally scores $EXIFTOOL_CACHE_DIR/combined-samples
    # (~4,200 manufacturer sample files, populated by `just compare-exiftool-full`).
    # That corpus is a local developer cache absent on CI, so the COMMITTED report
    # is never generated from it -- see docs/contributing/measuring-coverage.md.
    CACHE_DIR="${EXIFTOOL_CACHE_DIR:-/tmp/oxidex-exiftool-cache}"
    ET_DIR="$CACHE_DIR/exiftool"
    V=$(tr -d '[:space:]' < .exiftool-version)

    # ExifTool's own t/images is the format-breadth corpus (~126 formats).
    # Pinned to the same release the transcriptions came from, so it cannot
    # drift from the oracle grading against it.
    #
    # Deliberately never `rm -rf "$ET_DIR"`. That path is the SHARED oracle
    # tree: compare-exiftool-full populates it from a tarball (with a GCS
    # fallback), and the exiftool-coverage-loop and find_tag_gaps.py read it
    # afterwards from separate invocations. A tarball extract may or may not
    # carry t/images, so "missing corpus" is not evidence the tree is broken
    # and must not be treated as a licence to delete it. Fall back to a
    # dedicated clone instead -- version-suffixed, so it is idempotent and a
    # pin bump cannot serve last release's samples.
    if [ -d "$ET_DIR/t/images" ]; then
        CORPUS_TREE="$ET_DIR"
    else
        CORPUS_TREE="$CACHE_DIR/exiftool-corpus-$V"
        if [ ! -d "$CORPUS_TREE/t/images" ]; then
            echo "Cloning pinned ExifTool $V for its t/images corpus..."
            mkdir -p "$CACHE_DIR"
            rm -rf "$CORPUS_TREE"
            git clone -q --depth 1 --branch "$V" \
                https://github.com/exiftool/exiftool "$CORPUS_TREE"
        fi
    fi
    # Assert against whichever tree the samples actually came from: sample
    # files change between releases, so a corpus from the wrong version is the
    # same skew problem as an oracle from the wrong version.
    GOT=$("$CORPUS_TREE/exiftool" -ver)
    [ "$GOT" = "$V" ] || { echo "corpus tree is ExifTool $GOT, expected $V" >&2; exit 1; }

    CORPORA=("$CORPUS_TREE/t/images" tests/fixtures)
    MIN_FILES=200
    MIN_TAGS=10000
    if [ "${OXIDEX_DEEP_CORPUS:-0}" = "1" ]; then
        if [ ! -d "$CACHE_DIR/combined-samples" ]; then
            echo "OXIDEX_DEEP_CORPUS=1 but $CACHE_DIR/combined-samples is absent." >&2
            echo "Populate it with: just compare-exiftool-full" >&2
            exit 1
        fi
        CORPORA+=("$CACHE_DIR/combined-samples")
        # The deep corpus is ~20x the file count; floors scale with it, and
        # this run is for local inspection rather than for the committed doc.
        MIN_FILES=2000
        MIN_TAGS=100000
    fi

    echo "Building oxidex..."
    cargo build --bin oxidex

    echo "Measuring extraction coverage against pinned ExifTool $V..."
    # --exclude-ext, not --ext: a deny-list of things that are never metadata
    # keeps scoring every real format including ones added later, whereas an
    # allow-list silently omits each new format until someone extends it.
    # --min-*: a degraded oracle does not crash, it reports a confident wrong
    # number over a fraction of the corpus. Fail instead.
    uv run tools/exiftool-tables/conformance.py "${CORPORA[@]}" \
        --recursive \
        --exclude-ext sh,md,py,json \
        --oxidex ./target/debug/oxidex \
        --min-files "$MIN_FILES" \
        --min-tags "$MIN_TAGS" \
        --json-out /tmp/oxidex-conformance.json

    echo "Regenerating tag coverage analysis..."
    uv run scripts/generate_tag_coverage.py \
        --conformance /tmp/oxidex-conformance.json \
        --corpus-desc "ExifTool $V \`t/images\` + \`tests/fixtures\`" \
        --output docs/reference/tag-coverage-analysis.md
    echo "Tag coverage report updated"

# Sync the tag statistics quoted in README/docs prose to the measured values
sync-tag-stats:
    uv run scripts/sync_tag_stats.py

# Fail if any quoted tag statistic is stale (changes nothing)
check-tag-stats:
    @uv run scripts/sync_tag_stats.py --check

# Tag definitions only, to stdout (no build, no ExifTool, cannot overwrite the report)
docs-coverage-definitions:
    uv run scripts/generate_tag_coverage.py --skip-conformance

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

    # Pinned, not "latest". This used to ask exiftool.org for the newest
    # release on every run, so the ExifTool the corpus was graded against
    # changed whenever upstream published -- while the transcriptions in this
    # repo stayed put. Different releases select different sub-tables for the
    # same bytes, so that drift silently manufactures both regressions and
    # fixes. .exiftool-version is the one source of truth, shared with the Rust
    # and Python oracles and with CI.
    VERSION=$(tr -d '[:space:]' < .exiftool-version)
    if [[ ! "$VERSION" =~ ^[0-9]+\.[0-9]+$ ]]; then
        echo "   ❌ .exiftool-version does not hold a numeric release: '$VERSION'"
        exit 1
    fi
    echo "📌 Pinned ExifTool version: $VERSION"

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

    # Pinned, not "latest". This used to ask exiftool.org for the newest
    # release on every run, so the ExifTool the corpus was graded against
    # changed whenever upstream published -- while the transcriptions in this
    # repo stayed put. Different releases select different sub-tables for the
    # same bytes, so that drift silently manufactures both regressions and
    # fixes. .exiftool-version is the one source of truth, shared with the Rust
    # and Python oracles and with CI.
    VERSION=$(tr -d '[:space:]' < .exiftool-version)
    if [[ ! "$VERSION" =~ ^[0-9]+\.[0-9]+$ ]]; then
        echo "   ❌ .exiftool-version does not hold a numeric release: '$VERSION'"
        exit 1
    fi
    echo "📌 Pinned ExifTool version: $VERSION"

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

# Diff ONE file against the pinned oracle (fast, no re-download; exits non-zero on any difference)
compare-file path *args:
    uv run scripts/compare_file.py {{path}} {{args}}

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

    # Pinned, not "latest" -- see .exiftool-version. Grading against whatever
    # upstream published today, while the transcriptions stay put, silently
    # manufactures both regressions and fixes.
    VERSION=$(tr -d '[:space:]' < .exiftool-version)
    if [[ ! "$VERSION" =~ ^[0-9]+\.[0-9]+$ ]]; then
        echo "   ❌ .exiftool-version does not hold a numeric release: '$VERSION'"
        exit 1
    fi
    echo "📌 Pinned ExifTool version: $VERSION"
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

    # Pinned, not "latest". This used to ask exiftool.org for the newest
    # release on every run, so the ExifTool the corpus was graded against
    # changed whenever upstream published -- while the transcriptions in this
    # repo stayed put. Different releases select different sub-tables for the
    # same bytes, so that drift silently manufactures both regressions and
    # fixes. .exiftool-version is the one source of truth, shared with the Rust
    # and Python oracles and with CI.
    VERSION=$(tr -d '[:space:]' < .exiftool-version)
    if [[ ! "$VERSION" =~ ^[0-9]+\.[0-9]+$ ]]; then
        echo "   ❌ .exiftool-version does not hold a numeric release: '$VERSION'"
        exit 1
    fi
    echo "📌 Pinned ExifTool version: $VERSION"

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
    # Persistent, not ephemeral: both the exiftool-coverage-loop Workflow
    # script and find_tag_gaps.py re-run tag-comparison directly against
    # this same path from separate agent/script invocations after this
    # recipe has already exited, so it must survive past this shell's
    # lifetime (unlike the old `/tmp/exiftool-combined-$$` +
    # `trap cleanup EXIT`, which deleted it on exit).
    COMBINED_DIR="$CACHE_DIR/combined-samples"
    GCS_BUCKET="https://storage.googleapis.com/oxidex-samples/exiftool"

    mkdir -p "$CACHE_DIR"

    # Pinned, not "latest". This used to ask exiftool.org for the newest
    # release on every run, so the ExifTool the corpus was graded against
    # changed whenever upstream published -- while the transcriptions in this
    # repo stayed put. Different releases select different sub-tables for the
    # same bytes, so that drift silently manufactures both regressions and
    # fixes. .exiftool-version is the one source of truth, shared with the Rust
    # and Python oracles and with CI.
    VERSION=$(tr -d '[:space:]' < .exiftool-version)
    if [[ ! "$VERSION" =~ ^[0-9]+\.[0-9]+$ ]]; then
        echo "   ❌ .exiftool-version does not hold a numeric release: '$VERSION'"
        exit 1
    fi
    echo "📌 Pinned ExifTool version: $VERSION"

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
        # rm -rf FIRST. `mv src dest` when dest already exists as a directory
        # moves src INSIDE it, producing exiftool/exiftool-<ver>/ instead of
        # exiftool/. The cache probe above looks for "$EXIFTOOL_DIR/exiftool",
        # one level too high for that layout, so it misses forever -- every
        # run re-downloads and the mv then fails outright with "Directory not
        # empty". Measured 2026-07-30: this crash-looped the dispatcher to
        # restart 4/5 and the fleet made zero model calls. The update branch
        # above already does this; the fresh-download branch did not.
        rm -rf "$EXIFTOOL_DIR"
        curl -sL "https://github.com/exiftool/exiftool/archive/refs/tags/$VERSION.tar.gz" | \
            tar -xzf - -C "$CACHE_DIR" && \
            rm -rf "$EXIFTOOL_DIR" && \
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

    # Pinned, not "latest" -- see .exiftool-version. Grading against whatever
    # upstream published today, while the transcriptions stay put, silently
    # manufactures both regressions and fixes.
    VERSION=$(tr -d '[:space:]' < .exiftool-version)
    if [[ ! "$VERSION" =~ ^[0-9]+\.[0-9]+$ ]]; then
        echo "   ❌ .exiftool-version does not hold a numeric release: '$VERSION'"
        exit 1
    fi
    echo "📌 Pinned ExifTool version: $VERSION"

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
            rm -rf "$EXIFTOOL_DIR" && \
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

# Regenerate ExifTool binary tag tables from ExifTool's Perl sources.
# Extracts, generates Rust, and verifies the output against ExifTool itself.
# The release defaults to .exiftool-version; regen.sh refuses any other.
regen-tables version="":
    tools/exiftool-tables/regen.sh {{version}}

# Verify the committed generated tables still match ExifTool exactly.
# Defaults to .exiftool-version -- the pin is the only source of truth for the
# release this repo grades against. Reading it out of the generated file
# instead (which this recipe used to do) made the check circular: the artifact
# named the release it was verified against, so a stale table set chose its own
# oracle and passed forever. verify.py now refuses a stamp that isn't the pin.
verify-tables version="":
    #!/usr/bin/env bash
    set -euo pipefail

    GENERATED="src/exiftool_tables/binary_tables.rs"
    VERSION="{{version}}"
    if [[ -z "$VERSION" ]]; then
        VERSION=$(tr -d '[:space:]' < .exiftool-version)
        [[ -n "$VERSION" ]] || {
            echo "❌ .exiftool-version is empty; it must name one ExifTool release" >&2
            exit 1
        }
    fi

    CACHE="${OXIDEX_ET_CACHE:-target/exiftool-src}"
    LIB="$CACHE/exiftool-$VERSION/lib"
    if [[ ! -d "$LIB" ]]; then
        echo "📦 Fetching ExifTool $VERSION (not cached)"
        mkdir -p "$CACHE"
        curl -sSL -o "$CACHE/et-$VERSION.tar.gz" \
            "https://github.com/exiftool/exiftool/archive/refs/tags/$VERSION.tar.gz"
        tar xzf "$CACHE/et-$VERSION.tar.gz" -C "$CACHE"
    fi

    python3 tools/exiftool-tables/verify.py "$GENERATED" "$LIB" \
        --oracle tools/exiftool-tables/oracle.pl
