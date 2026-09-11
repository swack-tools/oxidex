# Shared shell access to artifacts.py. Source after setting HERE and ROOT.
# Keep output declarations in Python; this file only adapts Bash 3 arrays/traps.
artifact_path() {
    python3 "$HERE/artifacts.py" --root "$ROOT" path "$1"
}

load_artifact_paths() {
    local paths
    paths="$(python3 "$HERE/artifacts.py" --root "$ROOT" paths "$@")"
    ARTIFACT_PATHS=()
    while IFS= read -r path; do ARTIFACT_PATHS+=("$path"); done <<< "$paths"
}

finish_regeneration() {
    local status=$? check_status=0
    trap - EXIT
    python3 "$HERE/artifacts.py" --root "$ROOT" check "$WRITE_SNAPSHOT" || check_status=$?
    if [[ -n "$REGEN_CLEANUP_FILE" ]]; then
        rm -f "$REGEN_CLEANUP_FILE" || check_status=1
    fi
    if [[ $check_status -eq 0 && $status -eq 0 ]]; then
        rm -f "$WRITE_SNAPSHOT"
    else
        echo ">> regeneration failed; entry snapshot retained at $WRITE_SNAPSHOT (no rollback)" >&2
    fi
    # Unexpected writes must fail a successful producer, but never erase the
    # original producer's nonzero status when it already failed.
    if [[ $status -eq 0 ]]; then status=$check_status; fi
    exit "$status"
}

begin_regeneration() {
    local tier="$1" cache="$2"
    local cache_args=(--cache "$cache")
    REGEN_CLEANUP_FILE=""
    if [[ -n "${CARGO_TARGET_DIR:-}" ]]; then cache_args+=(--cache "$CARGO_TARGET_DIR"); fi
    WRITE_SNAPSHOT="$(mktemp "${TMPDIR:-/tmp}/oxidex-regen.XXXXXX")"
    if ! python3 "$HERE/artifacts.py" --root "$ROOT" snapshot --tier "$tier" \
            "${cache_args[@]}" --output "$WRITE_SNAPSHOT"; then
        rm -f "$WRITE_SNAPSHOT"
        return 1
    fi
    trap finish_regeneration EXIT
    trap 'exit 130' INT
    trap 'exit 143' TERM
    trap 'exit 129' HUP
}

format_artifacts() {
    load_artifact_paths --tier "$1" --kind rust --absolute
    # cargo fmt can walk workspace modules beyond its positional arguments.
    # Scope rustfmt directly to the inventory and retain the repo config.
    # Match Cargo.toml's package edition (rustfmt.toml still says 2021).
    rustfmt --edition 2024 --config-path "$ROOT/rustfmt.toml" "${ARTIFACT_PATHS[@]}"
}
