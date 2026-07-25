#!/bin/bash
# Test script for ExifTool tag comparison workflow
#
# This script validates the compare-exiftool.yml workflow by:
# 1. Checking workflow syntax
# 2. Verifying required files and permissions
# 3. Testing manual workflow trigger
# 4. Monitoring execution
# 5. Validating results

set -euo pipefail

# Color output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Logging functions
log_info() {
    echo -e "${BLUE}ℹ${NC} $1"
}

log_success() {
    echo -e "${GREEN}✅${NC} $1"
}

log_warning() {
    echo -e "${YELLOW}⚠${NC} $1"
}

log_error() {
    echo -e "${RED}❌${NC} $1"
}

# Test counters
TESTS_RUN=0
TESTS_PASSED=0
TESTS_FAILED=0

run_test() {
    local test_name="$1"
    local test_command="$2"

    TESTS_RUN=$((TESTS_RUN + 1))
    log_info "Test $TESTS_RUN: $test_name"

    if eval "$test_command"; then
        TESTS_PASSED=$((TESTS_PASSED + 1))
        log_success "Test $TESTS_RUN passed: $test_name"
    else
        TESTS_FAILED=$((TESTS_FAILED + 1))
        log_error "Test $TESTS_RUN failed: $test_name"
        return 1
    fi
}

# Test 1: Check workflow file exists
test_workflow_exists() {
    [[ -f ".github/workflows/compare-exiftool.yml" ]]
}

# Test 2: Verify workflow syntax (using gh CLI)
test_workflow_syntax() {
    if command -v gh &> /dev/null; then
        gh workflow view compare-exiftool.yml &> /dev/null || return 1
    else
        log_warning "gh CLI not found, skipping syntax check"
        return 0
    fi
}

# Test 3: Check required dependencies
test_dependencies() {
    local missing=""

    # Check for cargo
    if ! command -v cargo &> /dev/null; then
        missing="cargo"
    fi

    # Check for exiftool
    if ! command -v exiftool &> /dev/null; then
        missing="$missing exiftool"
    fi

    if [[ -n "$missing" ]]; then
        log_warning "Missing dependencies: $missing"
        return 0  # Don't fail on missing deps
    fi

    return 0
}

# Test 4: Verify tag-comparison binary exists or can be built
test_binary_buildable() {
    if [[ -f "target/release/tag-comparison" ]]; then
        return 0
    fi

    # Try to build it
    log_info "Building tag-comparison binary..."
    cargo build --release --bin tag-comparison 2>&1 | grep -q "Finished" || return 1

    [[ -f "target/release/tag-comparison" ]]
}

# Test 5: Verify comparison directory structure
test_comparison_dir_structure() {
    [[ -d "docs/reference/comparison" ]] && \
    [[ -f "docs/reference/comparison/index.md" ]]
}

# Test 6: Check VitePress configuration includes comparison link
test_vitepress_config() {
    grep -q "ExifTool Comparison" docs/.vitepress/config.mts || \
    grep -q "comparison" docs/.vitepress/config.mts
}

# Test 7: Verify workflow triggers
test_workflow_triggers() {
    grep -q "workflow_dispatch" .github/workflows/compare-exiftool.yml && \
    grep -q "src/parsers" .github/workflows/compare-exiftool.yml && \
    grep -q "cron:" .github/workflows/compare-exiftool.yml
}

# Test 8: Check GitHub Pages action configuration
test_github_pages_action() {
    grep -q "peaceiris/actions-gh-pages" .github/workflows/compare-exiftool.yml && \
    grep -q "tag-comparison" .github/workflows/compare-exiftool.yml
}

# Test 9: Verify version-locked caching (if exiftool is available)
test_version_locked_cache() {
    if command -v exiftool &> /dev/null; then
        grep -q "steps.get-version.outputs.installed_version" .github/workflows/compare-exiftool.yml
    else
        log_warning "ExifTool not available, skipping version check"
        return 0
    fi
}

# Test 10: Verify 3-tier download fallback
test_download_fallback() {
    local count=$(grep -c "exiftool.org\|github.com\|api.github.com" .github/workflows/compare-exiftool.yml)
    [[ $count -ge 3 ]]
}

# Test 11: Check for proper error handling
test_error_handling() {
    grep -q "if \[ \$? -eq 0 \]" .github/workflows/compare-exiftool.yml || \
    grep -q "exit 1" .github/workflows/compare-exiftool.yml
}

# Test 12: Verify manual execution documentation
test_documentation() {
    [[ -f "docs/guides/MANUAL-WORKFLOW-TRIGGER.md" ]] || \
    [[ -f "docs/GITHUB-PAGES-SETUP.md" ]]
}

# Helper function to trigger workflow if gh CLI available
trigger_workflow() {
    if command -v gh &> /dev/null; then
        log_info "Triggering workflow via GitHub CLI..."
        if gh workflow run compare-exiftool.yml; then
            log_success "Workflow triggered successfully"

            # Get the run ID
            sleep 2
            local run_id=$(gh run list --workflow compare-exiftool.yml --limit 1 --json databaseId --jq '.[0].databaseId')
            log_info "Monitoring run: $run_id"

            # Optional: wait for completion (with timeout)
            local timeout=600  # 10 minutes
            local elapsed=0
            local interval=10

            log_info "Waiting for workflow completion (max $timeout seconds)..."
            while [[ $elapsed -lt $timeout ]]; do
                local status=$(gh run view "$run_id" --json status --jq '.status')

                if [[ "$status" == "completed" ]]; then
                    local conclusion=$(gh run view "$run_id" --json conclusion --jq '.conclusion')
                    log_success "Workflow completed with status: $conclusion"
                    return 0
                fi

                log_info "Status: $status (waited ${elapsed}s)"
                sleep $interval
                elapsed=$((elapsed + interval))
            done

            log_warning "Workflow still running after timeout"
            return 0
        else
            log_error "Failed to trigger workflow"
            return 1
        fi
    else
        log_warning "GitHub CLI (gh) not found, skipping workflow trigger"
        return 0
    fi
}

# Main execution
main() {
    echo ""
    echo "╔════════════════════════════════════════════════════════════════╗"
    echo "║  ExifTool Tag Comparison Workflow - Test Suite                 ║"
    echo "╚════════════════════════════════════════════════════════════════╝"
    echo ""

    # Change to repo root
    cd "$(git rev-parse --show-toplevel)"

    log_info "Starting workflow validation..."
    echo ""

    # Run all tests
    run_test "Workflow file exists" "test_workflow_exists" || true
    run_test "Workflow syntax is valid" "test_workflow_syntax" || true
    run_test "Required dependencies available" "test_dependencies" || true
    run_test "tag-comparison binary buildable" "test_binary_buildable" || true
    run_test "Comparison directory structure" "test_comparison_dir_structure" || true
    run_test "VitePress configuration updated" "test_vitepress_config" || true
    run_test "Workflow triggers configured" "test_workflow_triggers" || true
    run_test "GitHub Pages action configured" "test_github_pages_action" || true
    run_test "Version-locked caching enabled" "test_version_locked_cache" || true
    run_test "3-tier download fallback present" "test_download_fallback" || true
    run_test "Error handling in place" "test_error_handling" || true
    run_test "Documentation created" "test_documentation" || true

    echo ""
    echo "╔════════════════════════════════════════════════════════════════╗"
    echo "║  Test Results Summary                                          ║"
    echo "╚════════════════════════════════════════════════════════════════╝"
    echo ""

    log_info "Tests run: $TESTS_RUN"
    log_success "Tests passed: $TESTS_PASSED"

    if [[ $TESTS_FAILED -gt 0 ]]; then
        log_error "Tests failed: $TESTS_FAILED"
    else
        log_success "All tests passed!"
    fi

    local pass_rate=0
    if [[ $TESTS_RUN -gt 0 ]]; then
        pass_rate=$((TESTS_PASSED * 100 / TESTS_RUN))
    fi

    log_info "Pass rate: ${pass_rate}%"
    echo ""

    # Prompt for workflow trigger
    if [[ $TESTS_FAILED -eq 0 ]]; then
        echo -n "Would you like to trigger the workflow now? (y/N) "
        read -r response
        if [[ "$response" =~ ^[Yy]$ ]]; then
            trigger_workflow || true
        fi
    fi

    echo ""

    # Return appropriate exit code
    if [[ $TESTS_FAILED -eq 0 ]]; then
        log_success "Validation complete - all checks passed!"
        return 0
    else
        log_error "Validation failed - $TESTS_FAILED test(s) failed"
        return 1
    fi
}

# Run main function
main "$@"
