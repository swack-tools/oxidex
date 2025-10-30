#!/usr/bin/env bash
# Quick Package Building Script for ExifTool-RS
# This script builds all package types in the correct order
#
# Usage: ./scripts/build-all-packages.sh [VERSION]
# Example: ./scripts/build-all-packages.sh 0.1.0

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

# Get version from Cargo.toml if not provided
VERSION="${1:-}"
if [[ -z "$VERSION" ]]; then
    VERSION=$(grep '^version = ' Cargo.toml | head -1 | cut -d'"' -f2)
    log_info "Using version from Cargo.toml: $VERSION"
fi

log_info "Building all packages for ExifTool-RS v$VERSION"
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
        log_info "  Binary location: target/release/exiftool-rs"
        local binary_size=$(du -h target/release/exiftool-rs | cut -f1)
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

# Verify Homebrew formula
verify_homebrew_formula() {
    log_info "Step 4: Verifying Homebrew formula..."

    local formula_path="packaging/homebrew/exiftool-rs.rb"

    if [[ ! -f "$formula_path" ]]; then
        log_error "Homebrew formula not found at $formula_path"
        return 1
    fi

    log_info "✓ Homebrew formula exists: $formula_path"

    # Check if formula needs SHA256 update
    if grep -q "UPDATE_THIS_SHA256_AFTER_RELEASE" "$formula_path"; then
        log_warn "  Homebrew formula SHA256 needs to be updated after creating a GitHub release"
        log_info "  Run: curl -sL https://github.com/exiftool-rs/exiftool-rs/archive/refs/tags/v$VERSION.tar.gz | shasum -a 256"
    fi

    # Audit formula if brew is available
    if command -v brew &> /dev/null; then
        if brew audit --formula "$formula_path" 2>&1 | grep -v "bottle" > /dev/null; then
            log_info "  Formula audit: PASSED"
        else
            log_warn "  Formula audit has warnings (may be acceptable)"
        fi
    else
        log_warn "  Homebrew not installed, skipping formula audit"
    fi

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
    if [[ -f "target/release/exiftool-rs" ]]; then
        echo "  ✓ Release binary: target/release/exiftool-rs ($(du -h target/release/exiftool-rs | cut -f1))"
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

    # Homebrew formula
    if [[ -f "packaging/homebrew/exiftool-rs.rb" ]]; then
        echo "  ✓ Homebrew formula: packaging/homebrew/exiftool-rs.rb"
    fi

    # Checksums
    if [[ -f "target/CHECKSUMS.txt" ]]; then
        echo "  ✓ Checksums: target/CHECKSUMS.txt"
    fi

    echo ""
    log_info "Next steps:"
    echo "  1. Test packages: ./scripts/test-packages.sh all"
    echo "  2. Create git tag: git tag -a v$VERSION -m 'Release v$VERSION'"
    echo "  3. Push tag: git push origin v$VERSION"
    echo "  4. Upload packages to GitHub Release"
    echo "  5. Update Homebrew formula SHA256"
    echo ""
}

# Main execution
main() {
    check_tools
    echo ""

    build_release_binary
    build_deb_package
    build_rpm_package
    verify_homebrew_formula
    generate_checksums

    print_summary
}

main "$@"
