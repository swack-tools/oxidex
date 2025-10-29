#!/usr/bin/env bash
#
# install.sh - Environment setup and dependency installation for exiftool-rs
#
# This script ensures the Rust environment is properly configured and all
# dependencies are installed. It is idempotent and can be safely re-run.
#
# Exit codes:
#   0 - Success
#   1 - Missing required tools (cargo/rustc)
#   2 - Dependency installation failed

set -e  # Exit on error
set -u  # Exit on undefined variable
set -o pipefail  # Exit on pipe failure

# Configuration
readonly SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
readonly PROJECT_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
readonly CARGO_TOML="${PROJECT_ROOT}/Cargo.toml"

# Color output for better UX (only if terminal supports it)
if [[ -t 2 ]]; then
    readonly RED='\033[0;31m'
    readonly GREEN='\033[0;32m'
    readonly YELLOW='\033[1;33m'
    readonly NC='\033[0m' # No Color
else
    readonly RED=''
    readonly GREEN=''
    readonly YELLOW=''
    readonly NC=''
fi

# Logging functions
log_info() {
    echo -e "${GREEN}[INFO]${NC} $*" >&2
}

log_warn() {
    echo -e "${YELLOW}[WARN]${NC} $*" >&2
}

log_error() {
    echo -e "${RED}[ERROR]${NC} $*" >&2
}

# Check if cargo is installed
check_rust_toolchain() {
    if ! command -v cargo &> /dev/null; then
        log_error "cargo not found. Please install Rust from https://rustup.rs/"
        exit 1
    fi

    if ! command -v rustc &> /dev/null; then
        log_error "rustc not found. Please install Rust from https://rustup.rs/"
        exit 1
    fi

    local rust_version
    rust_version="$(rustc --version | awk '{print $2}')"
    log_info "Using Rust version: ${rust_version}"
}

# Install or update Rust dependencies
install_dependencies() {
    log_info "Installing/updating project dependencies..."

    cd "${PROJECT_ROOT}"

    # Build dependencies (this will install them if not present)
    if cargo fetch --locked 2>&1 | grep -q "lock file"; then
        log_warn "Cargo.lock updated, running cargo fetch again"
        cargo fetch
    else
        cargo fetch
    fi

    log_info "Dependencies installed successfully"
}

# Install development tools if not present
install_dev_tools() {
    log_info "Checking development tools..."

    # Check for clippy
    if ! cargo clippy --version &> /dev/null; then
        log_warn "clippy not found, installing..."
        rustup component add clippy
    fi

    # Check for rustfmt
    if ! cargo fmt --version &> /dev/null; then
        log_warn "rustfmt not found, installing..."
        rustup component add rustfmt
    fi

    log_info "Development tools ready"
}

# Main installation logic
main() {
    log_info "Starting environment setup for exiftool-rs..."

    # Change to project root
    cd "${PROJECT_ROOT}"

    # Verify Cargo.toml exists
    if [[ ! -f "${CARGO_TOML}" ]]; then
        log_error "Cargo.toml not found at ${CARGO_TOML}"
        exit 2
    fi

    # Check Rust toolchain
    check_rust_toolchain

    # Install dependencies
    install_dependencies

    # Install development tools
    install_dev_tools

    log_info "Environment setup completed successfully"
    exit 0
}

# Run main function
main "$@"
