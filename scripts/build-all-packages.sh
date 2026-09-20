#!/usr/bin/env bash
# Quick local package-building script for OxiDex
# This script creates optional local Debian/RPM artifacts. The beta release
# workflow does not publish those artifacts or a Homebrew formula.
#
# Usage: ./scripts/build-all-packages.sh [VERSION]
# Without VERSION, the value is derived from the root Cargo package.

set -euo pipefail

# Colors for output
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
NC='\033[0m' # No Color

log_info() {
    echo -e "${GREEN}[INFO]${NC} $1"
}

log_warn() {
    echo -e "${YELLOW}[WARN]${NC} $1"
}

log_error() {
    echo -e "${RED}[ERROR]${NC} $1"
}

# Get the product version from Cargo metadata if not provided. An explicit
# argument remains useful for a local package rehearsal, but is never a
# release publication claim.
VERSION="${1:-}"
if [[ -z "$VERSION" ]]; then
    VERSION=$(cargo metadata --no-deps --format-version 1 | python3 -c 'import json, sys; print(next(p["version"] for p in json.load(sys.stdin)["packages"] if p["name"] == "oxidex"))')
    log_info "Using version from Cargo.toml: $VERSION"
fi

log_info "Building all packages for OxiDex v$VERSION"
log_info "================================================"
echo ""

# Check if packaging tools are installed
check_tools() {
    local missing_tools=()

    if ! command -v cargo &> /dev/null; then
        log_error "Cargo is not installed. Install Rust from https://rustup.rs"
        exit 1
    fi

    if ! cargo deb --version &> /dev/null; then
        log_warn "cargo-deb not installed"
        missing_tools+=("cargo-deb")
    fi

    if ! cargo generate-rpm --version &> /dev/null; then
        log_warn "cargo-generate-rpm not installed"
        missing_tools+=("cargo-generate-rpm")
    fi

    if [[ ${#missing_tools[@]} -gt 0 ]]; then
        echo ""
        log_warn "Some packaging tools are missing. Install them with:"
        for tool in "${missing_tools[@]}"; do
            echo "  cargo install $tool"
        done
        echo ""
        read -p "Do you want to continue anyway? (y/N) " -n 1 -r
        echo
        if [[ ! $REPLY =~ ^[Yy]$ ]]; then
            exit 1
        fi
    fi
}

# Build release binary
build_release_binary() {
    log_info "Step 1: Building release binary..."
    if cargo build --release; then
        log_info "✓ Release binary built successfully"
        log_info "  Binary location: target/release/oxidex"
        local binary_size=$(du -h target/release/oxidex | cut -f1)
        log_info "  Binary size: $binary_size"
    else
        log_error "Failed to build release binary"
        exit 1
    fi
    echo ""
}

# Build Debian package
build_deb_package() {
    log_info "Step 2: Building Debian package..."

    if ! command -v cargo-deb &> /dev/null; then
        log_warn "cargo-deb not installed. Skipping .deb package."
        return 0
    fi

    if cargo deb; then
        local deb_file=$(find target/debian -name "*.deb" -type f | head -1)
        if [[ -f "$deb_file" ]]; then
            log_info "✓ Debian package created successfully"
            log_info "  Package location: $deb_file"
            log_info "  Package size: $(du -h "$deb_file" | cut -f1)"

            # Show package info
            if command -v dpkg-deb &> /dev/null; then
                log_info "  Package info:"
                dpkg-deb --info "$deb_file" | grep -E "Package:|Version:|Architecture:|Description:" | sed 's/^/    /'
            fi
        fi
    else
        log_error "Failed to build Debian package"
        return 1
    fi
    echo ""
}

# Build RPM package
build_rpm_package() {
    log_info "Step 3: Building RPM package..."

    if ! command -v cargo-generate-rpm &> /dev/null; then
        log_warn "cargo-generate-rpm not installed. Skipping .rpm package."
        return 0
    fi

    if cargo generate-rpm; then
        local rpm_file=$(find target/generate-rpm -name "*.rpm" -type f | head -1)
        if [[ -f "$rpm_file" ]]; then
            log_info "✓ RPM package created successfully"
            log_info "  Package location: $rpm_file"
            log_info "  Package size: $(du -h "$rpm_file" | cut -f1)"

            # Show package info
            if command -v rpm &> /dev/null; then
                log_info "  Package info:"
                rpm -qip "$rpm_file" 2>/dev/null | grep -E "Name|Version|Architecture|Summary" | sed 's/^/    /'
            fi
        fi
    else
        log_error "Failed to build RPM package"
        return 1
    fi
    echo ""
}

# Homebrew is intentionally not a beta distribution channel.
report_homebrew_policy() {
    log_info "Step 4: Homebrew distribution is not enabled for this beta; no formula was checked."
    echo ""
}

# Generate checksums
generate_checksums() {
    log_info "Step 5: Generating checksums..."

    local checksum_file="target/CHECKSUMS.txt"
    > "$checksum_file"  # Clear file

    # Find all packages
    local packages=(
        $(find target/debian -name "*.deb" -type f 2>/dev/null || true)
        $(find target/generate-rpm -name "*.rpm" -type f 2>/dev/null || true)
    )

    if [[ ${#packages[@]} -eq 0 ]]; then
        log_warn "No packages found to generate checksums"
        return 0
    fi

    for package in "${packages[@]}"; do
        local filename=$(basename "$package")
        local sha256=$(shasum -a 256 "$package" | cut -d' ' -f1)
        echo "$sha256  $filename" >> "$checksum_file"
        log_info "  $filename"
        log_info "    SHA256: $sha256"
    done

    log_info "✓ Checksums written to: $checksum_file"
    echo ""
}

# Summary
print_summary() {
    log_info "Build Summary"
    log_info "============="
    echo ""

    log_info "Artifacts created:"

    # Release binary
    if [[ -f "target/release/oxidex" ]]; then
        echo "  ✓ Release binary: target/release/oxidex ($(du -h target/release/oxidex | cut -f1))"
    fi

    # Debian package
    local deb_file=$(find target/debian -name "*.deb" -type f 2>/dev/null | head -1 || true)
    if [[ -f "$deb_file" ]]; then
        echo "  ✓ Debian package: $deb_file ($(du -h "$deb_file" | cut -f1))"
    else
        echo "  ✗ Debian package: not built"
    fi

    # RPM package
    local rpm_file=$(find target/generate-rpm -name "*.rpm" -type f 2>/dev/null | head -1 || true)
    if [[ -f "$rpm_file" ]]; then
        echo "  ✓ RPM package: $rpm_file ($(du -h "$rpm_file" | cut -f1))"
    else
        echo "  ✗ RPM package: not built"
    fi

    echo "  - Homebrew formula: not published for this beta"

    # Checksums
    if [[ -f "target/CHECKSUMS.txt" ]]; then
        echo "  ✓ Checksums: target/CHECKSUMS.txt"
    fi

    echo ""
    log_info "Next steps for local validation:"
    echo "  1. Test locally generated packages: ./scripts/test-packages.sh all"
    echo "  2. Use the signed binaries and checksums from the GitHub release workflow"
    echo "     for beta distribution; Debian/RPM/Homebrew publication is not enabled."
    echo ""
}

# Main execution
main() {
    check_tools
    echo ""

    build_release_binary
    build_deb_package
    build_rpm_package
    report_homebrew_policy
    generate_checksums

    print_summary
}

main "$@"
