#!/usr/bin/env bash
#
# lint.sh - Lint the exiftool-rs codebase
#
# This script ensures dependencies and linting tools are installed, then
# lints the project source code. Output is exclusively JSON to stdout.
#
# Exit codes:
#   0 - Linting passed (no errors/warnings)
#   Non-zero - Linting found issues or script error

set -e  # Exit on error
set -u  # Exit on undefined variable
set -o pipefail  # Exit on pipe failure

# Configuration
readonly SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
readonly PROJECT_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
readonly INSTALL_SCRIPT="${SCRIPT_DIR}/install.sh"

# Logging to stderr only (stdout reserved for JSON)
log_error() {
    echo "[ERROR] $*" >&2
}

log_info() {
    echo "[INFO] $*" >&2
}

# Ensure environment and dependencies are ready
ensure_dependencies() {
    if [[ ! -x "${INSTALL_SCRIPT}" ]]; then
        log_error "install.sh not found or not executable at ${INSTALL_SCRIPT}"
        exit 1
    fi

    log_info "Ensuring dependencies are installed..." >&2
    bash "${INSTALL_SCRIPT}" > /dev/null 2>&1 || {
        log_error "Dependency installation failed"
        exit 1
    }
}

# Ensure clippy is installed
ensure_clippy() {
    if ! cargo clippy --version &> /dev/null; then
        log_info "Installing clippy..." >&2
        rustup component add clippy > /dev/null 2>&1
    fi
}

# Convert clippy JSON output to required format
convert_clippy_to_json() {
    local clippy_output="$1"

    # Parse clippy JSON messages and convert to required format
    echo "${clippy_output}" | jq -c '
        select(.reason == "compiler-message") |
        .message |
        select(.level == "error" or .level == "warning") |
        {
            type: (if .level == "error" then "error" else "warning" end),
            path: (.spans[0].file_name // "unknown"),
            obj: (.spans[0].label // ""),
            message: .message,
            line: (.spans[0].line_start // 0 | tostring),
            column: (.spans[0].column_start // 0 | tostring)
        }
    ' 2>/dev/null || echo "[]"
}

# Run clippy and format output as JSON
run_clippy_lint() {
    log_info "Running clippy linter..." >&2

    cd "${PROJECT_ROOT}"

    # Capture clippy output in JSON format
    local clippy_output
    local exit_code=0

    clippy_output=$(cargo clippy --all-targets --message-format=json -- \
        -W clippy::all \
        -D warnings 2>&1) || exit_code=$?

    # Convert to required JSON format
    local json_output
    json_output=$(convert_clippy_to_json "${clippy_output}")

    # If no issues found, output empty array
    if [[ -z "${json_output}" || "${json_output}" == "[]" ]]; then
        echo "[]"
        return 0
    fi

    # Output issues as JSON array
    echo "${json_output}" | jq -s '.'

    # Return non-zero if there were errors
    return "${exit_code}"
}

# Alternative: Parse cargo clippy text output if JSON parsing fails
run_clippy_text_fallback() {
    log_info "Running clippy linter (text mode)..." >&2

    cd "${PROJECT_ROOT}"

    local clippy_output
    local exit_code=0

    clippy_output=$(cargo clippy --all-targets -- \
        -W clippy::all \
        -D warnings 2>&1) || exit_code=$?

    # Parse text output and convert to JSON
    local json_errors="[]"

    # Check if there are any errors or warnings
    if echo "${clippy_output}" | grep -qE "(error|warning):"; then
        # Simple parsing - create JSON for each error/warning line
        json_errors=$(echo "${clippy_output}" | grep -E "(error|warning):" | while IFS= read -r line; do
            local type_match=""
            local message_match=""

            if echo "$line" | grep -q "^error:"; then
                type_match="error"
                message_match=$(echo "$line" | sed 's/^error: //')
            elif echo "$line" | grep -q "^warning:"; then
                type_match="warning"
                message_match=$(echo "$line" | sed 's/^warning: //')
            fi

            if [[ -n "$type_match" ]]; then
                jq -n --arg type "$type_match" \
                      --arg path "unknown" \
                      --arg obj "" \
                      --arg message "$message_match" \
                      --arg line "0" \
                      --arg column "0" \
                      '{type: $type, path: $path, obj: $obj, message: $message, line: $line, column: $column}'
            fi
        done | jq -s '.')
    fi

    echo "${json_errors}"
    return "${exit_code}"
}

# Main linting logic
main() {
    # Silence all stderr from dependencies installation
    ensure_dependencies 2>/dev/null || ensure_dependencies
    ensure_clippy 2>/dev/null || ensure_clippy

    # Check if jq is available for JSON processing
    if ! command -v jq &> /dev/null; then
        log_error "jq is required for JSON output but not found. Please install jq."
        echo '[]'
        exit 1
    fi

    # Run clippy and output JSON
    local exit_code=0
    run_clippy_lint || exit_code=$?

    exit "${exit_code}"
}

# Run main function
main "$@"
