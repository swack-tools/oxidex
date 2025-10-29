#!/usr/bin/env bash
#
# test.sh - Run tests for exiftool-rs
#
# This script ensures dependencies are installed and then runs the project
# test suite.
#
# Exit codes:
#   0 - Tests passed
#   Non-zero - Tests failed or setup error

set -e  # Exit on error
set -u  # Exit on undefined variable
set -o pipefail  # Exit on pipe failure

# Configuration
readonly SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
readonly PROJECT_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
readonly INSTALL_SCRIPT="${SCRIPT_DIR}/install.sh"

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

log_error() {
    echo -e "${RED}[ERROR]${NC} $*" >&2
}

# Ensure environment and dependencies are ready
ensure_dependencies() {
    if [[ ! -x "${INSTALL_SCRIPT}" ]]; then
        log_error "install.sh not found or not executable at ${INSTALL_SCRIPT}"
        exit 1
    fi

    log_info "Ensuring dependencies are installed..."
    bash "${INSTALL_SCRIPT}" > /dev/null 2>&1 || {
        log_error "Dependency installation failed"
        exit 1
    }
}

# Run the test suite
run_tests() {
    log_info "Running test suite..."

    cd "${PROJECT_ROOT}"

    # Run cargo test with all features
    # --all-targets runs tests for lib, bins, examples, tests, and benches
    # --quiet reduces output verbosity
    cargo test --all-targets "$@"
}

# Main execution logic
main() {
    # Ensure dependencies are installed
    ensure_dependencies

    # Run tests with any arguments passed to this script
    run_tests "$@"

    log_info "All tests passed successfully"
}

# Run main function
main "$@"
