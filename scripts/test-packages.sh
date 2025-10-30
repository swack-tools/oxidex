#!/usr/bin/env bash
# Package Testing Script for exiftool-rs
# Tests .deb, .rpm, and Homebrew packages
#
# Usage: ./scripts/test-packages.sh [deb|rpm|brew|all]

set -euo pipefail

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Configuration
BINARY_NAME="exiftool-rs"
EXPECTED_VERSION="0.1.0"

# Helper functions
log_info() {
    echo -e "${GREEN}[INFO]${NC} $1"
}

log_warn() {
    echo -e "${YELLOW}[WARN]${NC} $1"
}

log_error() {
    echo -e "${RED}[ERROR]${NC} $1"
}

check_command() {
    if command -v "$1" &> /dev/null; then
        return 0
    else
        return 1
    fi
}

test_binary() {
    log_info "Testing binary functionality..."

    if ! check_command "$BINARY_NAME"; then
        log_error "Binary '$BINARY_NAME' not found in PATH"
        return 1
    fi

    # Test version output
    local version_output
    version_output=$("$BINARY_NAME" --version 2>&1 || true)

    if [[ "$version_output" == *"$EXPECTED_VERSION"* ]]; then
        log_info "Version check passed: $version_output"
    else
        log_error "Version check failed. Expected '$EXPECTED_VERSION', got: $version_output"
        return 1
    fi

    # Test help output
    if "$BINARY_NAME" --help &> /dev/null; then
        log_info "Help command passed"
    else
        log_error "Help command failed"
        return 1
    fi

    return 0
}

# Debian Package Testing
test_deb_package() {
    log_info "=== Testing Debian Package ==="

    # Check if package file exists
    local deb_file="target/debian/${BINARY_NAME}_${EXPECTED_VERSION}_amd64.deb"

    if [[ ! -f "$deb_file" ]]; then
        log_error "Debian package not found at $deb_file"
        log_info "Run 'cargo deb' to build the package first"
        return 1
    fi

    log_info "Found package: $deb_file"

    # Inspect package contents
    log_info "Package contents:"
    dpkg-deb --contents "$deb_file" | grep -E "(bin|doc)" || true

    # Check if we can inspect without root
    if dpkg-deb --info "$deb_file" &> /dev/null; then
        log_info "Package info:"
        dpkg-deb --info "$deb_file" | head -20
    fi

    # Check if running with sudo
    if [[ $EUID -ne 0 ]]; then
        log_warn "Not running as root. Skipping installation test."
        log_info "To test installation, run:"
        log_info "  sudo dpkg -i $deb_file"
        log_info "  $BINARY_NAME --version"
        log_info "  sudo dpkg -r $BINARY_NAME"
        return 0
    fi

    # Install package
    log_info "Installing package..."
    dpkg -i "$deb_file"

    # Test binary
    if test_binary; then
        log_info "Debian package test: PASSED"
    else
        log_error "Debian package test: FAILED"
        dpkg -r "$BINARY_NAME" || true
        return 1
    fi

    # Uninstall package
    log_info "Uninstalling package..."
    dpkg -r "$BINARY_NAME"

    # Verify removal
    if check_command "$BINARY_NAME"; then
        log_error "Binary still found after uninstall"
        return 1
    fi

    log_info "Debian package test completed successfully"
    return 0
}

# RPM Package Testing
test_rpm_package() {
    log_info "=== Testing RPM Package ==="

    # Check if package file exists
    local rpm_file="target/generate-rpm/${BINARY_NAME}-${EXPECTED_VERSION}-1.x86_64.rpm"

    if [[ ! -f "$rpm_file" ]]; then
        log_error "RPM package not found at $rpm_file"
        log_info "Run 'cargo build --release && cargo generate-rpm' to build the package first"
        return 1
    fi

    log_info "Found package: $rpm_file"

    # Inspect package contents
    log_info "Package contents:"
    rpm -qlp "$rpm_file" 2>/dev/null || true

    # Check package info
    log_info "Package info:"
    rpm -qip "$rpm_file" 2>/dev/null | head -20 || true

    # Check if running with sudo
    if [[ $EUID -ne 0 ]]; then
        log_warn "Not running as root. Skipping installation test."
        log_info "To test installation, run:"
        log_info "  sudo rpm -i $rpm_file"
        log_info "  $BINARY_NAME --version"
        log_info "  sudo rpm -e $BINARY_NAME"
        return 0
    fi

    # Install package
    log_info "Installing package..."
    rpm -i "$rpm_file"

    # Test binary
    if test_binary; then
        log_info "RPM package test: PASSED"
    else
        log_error "RPM package test: FAILED"
        rpm -e "$BINARY_NAME" || true
        return 1
    fi

    # Uninstall package
    log_info "Uninstalling package..."
    rpm -e "$BINARY_NAME"

    # Verify removal
    if check_command "$BINARY_NAME"; then
        log_error "Binary still found after uninstall"
        return 1
    fi

    log_info "RPM package test completed successfully"
    return 0
}

# Homebrew Package Testing
test_homebrew_package() {
    log_info "=== Testing Homebrew Formula ==="

    if ! check_command "brew"; then
        log_error "Homebrew not found. Install from https://brew.sh"
        return 1
    fi

    local formula_path="packaging/homebrew/${BINARY_NAME}.rb"

    if [[ ! -f "$formula_path" ]]; then
        log_error "Homebrew formula not found at $formula_path"
        return 1
    fi

    log_info "Found formula: $formula_path"

    # Audit formula syntax
    log_info "Auditing formula..."
    if brew audit --formula "$formula_path" 2>&1 | grep -v "bottle"; then
        log_info "Formula audit passed (ignoring bottle warnings)"
    else
        log_warn "Formula audit has warnings (may be acceptable)"
    fi

    # Check if package is already installed
    if brew list "$BINARY_NAME" &>/dev/null; then
        log_warn "Package already installed via Homebrew. Uninstalling first..."
        brew uninstall "$BINARY_NAME"
    fi

    # Install from formula
    log_info "Installing from formula (this may take several minutes to compile)..."
    log_warn "Note: The SHA256 in the formula must be updated after creating a GitHub release"

    if brew install --build-from-source "$formula_path" 2>&1 | tee /tmp/brew-install.log; then
        log_info "Installation completed"
    else
        log_error "Installation failed. Check /tmp/brew-install.log for details"
        return 1
    fi

    # Test binary
    if test_binary; then
        log_info "Homebrew package test: PASSED"
    else
        log_error "Homebrew package test: FAILED"
        brew uninstall "$BINARY_NAME" || true
        return 1
    fi

    # Uninstall package
    log_info "Uninstalling package..."
    brew uninstall "$BINARY_NAME"

    # Verify removal
    if check_command "$BINARY_NAME"; then
        log_error "Binary still found after uninstall"
        return 1
    fi

    log_info "Homebrew package test completed successfully"
    return 0
}

# Main execution
main() {
    local test_type="${1:-all}"

    log_info "ExifTool-RS Package Testing Script"
    log_info "===================================="

    case "$test_type" in
        deb)
            test_deb_package
            ;;
        rpm)
            test_rpm_package
            ;;
        brew)
            test_homebrew_package
            ;;
        all)
            log_info "Testing all package types..."
            local failed=0

            if test_deb_package; then
                log_info "✓ Debian package test passed"
            else
                log_error "✗ Debian package test failed"
                failed=$((failed + 1))
            fi

            echo ""

            if test_rpm_package; then
                log_info "✓ RPM package test passed"
            else
                log_error "✗ RPM package test failed"
                failed=$((failed + 1))
            fi

            echo ""

            if test_homebrew_package; then
                log_info "✓ Homebrew package test passed"
            else
                log_error "✗ Homebrew package test failed"
                failed=$((failed + 1))
            fi

            echo ""
            log_info "===================================="
            if [[ $failed -eq 0 ]]; then
                log_info "All package tests passed!"
                return 0
            else
                log_error "$failed package test(s) failed"
                return 1
            fi
            ;;
        *)
            log_error "Unknown test type: $test_type"
            echo "Usage: $0 [deb|rpm|brew|all]"
            return 1
            ;;
    esac
}

main "$@"
