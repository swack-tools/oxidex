#!/usr/bin/env bash
# docs-local-deploy.sh -- build the docs site the way deploy-docs.yml does,
# into a temp dir, serve it on localhost and open it in Chrome. Use it to see
# what oxidex.net would look like before anything is pushed to main.
#
# It mirrors the `publish` job of .github/workflows/deploy-docs.yml:
#   1. comparison report   -> docs/reference/comparison/  (optional: --full-report)
#   2. benchmark results   -> target/criterion/            (optional: --bench)
#   3. benchmark metadata  -> docs/performance/index.md   (only with results)
#   4. npm ci && npm run docs:build
#   5. copy criterion reports into dist/benchmarks/
# and then serves dist/ with `vitepress preview` instead of uploading it.
#
# The site is built from a snapshot of a git ref (`git archive`), never from
# your working tree, so the checkout you run it from is not touched. Pass
# --worktree to build the current directory's uncommitted state instead.
#
# Why not open index.html from disk: the site uses cleanUrls and absolute
# asset paths (/assets/...), so a file:// URL breaks every link and style.
#
# USAGE
#   tools/docs-local-deploy.sh                       # refactor/tag-machinery tip, stub report
#   tools/docs-local-deploy.sh --ref origin/main     # what main would publish
#   tools/docs-local-deploy.sh --worktree            # this checkout, uncommitted edits included
#   tools/docs-local-deploy.sh --bench auto          # + latest CI benchmark-results artifact
#   tools/docs-local-deploy.sh --full-report         # + real ExifTool comparison (slow:
#                                                    #   release build + full corpus run)
#   tools/docs-local-deploy.sh --worktree --build-only --output /tmp/docs-snapshot
#   tools/docs-local-deploy.sh --port 4180 --no-open --keep
#
# Environment passes through to the VitePress build, so a config that reads
# e.g. DOCS_BASE / DOCS_CHANNEL can be previewed the same way.
#
# Ctrl-C stops the server; the temp dir is removed unless --keep.
set -euo pipefail

REF="origin/refactor/tag-machinery"
USE_WORKTREE=0
FULL_REPORT=0
BENCH=""
PORT=4173
OPEN=1
KEEP=0
BUILD_ONLY=0
OUTPUT=""

usage() { sed -n '2,33p' "$0" | sed 's/^# \{0,1\}//'; exit "${1:-0}"; }

while [ $# -gt 0 ]; do
  case "$1" in
    --ref) REF="$2"; shift 2 ;;
    --worktree) USE_WORKTREE=1; shift ;;
    --full-report) FULL_REPORT=1; shift ;;
    --bench) BENCH="$2"; shift 2 ;;
    --port) PORT="$2"; shift 2 ;;
    --no-open) OPEN=0; shift ;;
    --keep) KEEP=1; shift ;;
    --build-only) BUILD_ONLY=1; OPEN=0; shift ;;
    --output) OUTPUT="$2"; shift 2 ;;
    -h|--help) usage 0 ;;
    *) echo "unknown argument: $1" >&2; usage 64 ;;
  esac
done

if [ "$BUILD_ONLY" = 1 ] && [ -z "$OUTPUT" ]; then
  echo "--build-only requires --output DIR" >&2
  exit 64
fi
if [ -n "$OUTPUT" ] && [ "$BUILD_ONLY" != 1 ]; then
  echo "--output requires --build-only" >&2
  exit 64
fi

REPO=$(git rev-parse --show-toplevel)
REPO_SLUG=$(gh repo view --json nameWithOwner --jq .nameWithOwner 2>/dev/null || echo swack-tools/oxidex)
TMP=$(mktemp -d "${TMPDIR:-/tmp}/oxidex-docs-XXXXXX")
SRC="$TMP/src"
SERVER_PID=""

cleanup() {
  [ -n "$SERVER_PID" ] && kill "$SERVER_PID" 2>/dev/null || true
  if [ "$KEEP" = 1 ]; then echo "kept: $TMP"; else rm -rf "$TMP"; fi
}
trap cleanup EXIT INT TERM

step() { printf '\n==> %s\n' "$*"; }

# --- 0. snapshot the source -------------------------------------------------
mkdir -p "$SRC"
if [ "$USE_WORKTREE" = 1 ]; then
  step "snapshot: working tree $REPO (uncommitted edits included)"
  SHA=$(git -C "$REPO" rev-parse HEAD)
  SOURCE_KIND=worktree
  SOURCE_IDENTITY="${SHA}-worktree"
  CANDIDATE_SHA=""
  # Tracked + untracked-but-not-ignored files, as they are on disk.
  (
    cd "$REPO"
    git ls-files -co --exclude-standard -z |
      while IFS= read -r -d '' path; do
        if [ -e "$path" ] || [ -L "$path" ]; then printf '%s\0' "$path"; fi
      done |
      tar --null -T - -cf -
  ) | tar -xf - -C "$SRC"
else
  if [[ "$REF" == origin/* ]]; then
    git -C "$REPO" fetch -q origin "${REF#origin/}" || echo "warning: fetch failed; using the local copy of $REF" >&2
  fi
  SHA=$(git -C "$REPO" rev-parse --verify "$REF^{commit}")
  SOURCE_KIND=commit
  SOURCE_IDENTITY="$SHA"
  CANDIDATE_SHA="$SHA"
  step "snapshot: $REF @ ${SHA:0:12}"
  git -C "$REPO" archive "$SHA" | tar -xf - -C "$SRC"
fi

if [ "$USE_WORKTREE" = 1 ]; then
  SNAPSHOT_TREE_HASH=$(
    cd "$SRC"
    find . -type f -print0 | LC_ALL=C sort -z | xargs -0 shasum -a 256 | shasum -a 256 | awk '{print $1}'
  )
else
  SNAPSHOT_TREE_HASH=$(git -C "$REPO" rev-parse "$SHA^{tree}")
fi

# --- 1. comparison report ---------------------------------------------------
if [ "$FULL_REPORT" = 1 ]; then
  step "comparison report: just compare-exiftool-full-update (release build + corpus; slow)"
  command -v just >/dev/null || { echo "just is required for --full-report" >&2; exit 1; }
  # Same cache path as the workflow; the recipe pins ExifTool from .exiftool-version.
  (cd "$SRC" && EXIFTOOL_CACHE_DIR="${EXIFTOOL_CACHE_DIR:-/tmp/oxidex-exiftool-cache}" \
     CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$TMP/target}" just compare-exiftool-full-update)
else
  echo "comparison report: stub (pass --full-report for the real one)"
fi

# --- 2/3. benchmark results + metadata --------------------------------------
BENCH_SHA=""
if [ -n "$BENCH" ]; then
  RUN_ID="$BENCH"
  if [ "$BENCH" = auto ]; then
    step "benchmarks: locating the newest ci.yml run with a benchmark-results artifact"
    RUN_ID=""
    for cand in $(gh run list -R "$REPO_SLUG" --workflow=ci.yml --status=success --limit=30 \
                    --json databaseId --jq '.[].databaseId'); do
      if gh api "repos/$REPO_SLUG/actions/runs/$cand/artifacts" \
           --jq '.artifacts[] | select(.name=="benchmark-results" and .expired==false) | .name' | grep -q .; then
        RUN_ID="$cand"; break
      fi
    done
  fi
  if [ -n "$RUN_ID" ]; then
    BENCH_SHA=$(gh run view -R "$REPO_SLUG" "$RUN_ID" --json headSha --jq .headSha)
    step "benchmarks: downloading benchmark-results from run $RUN_ID (${BENCH_SHA:0:7})"
    mkdir -p "$SRC/target/criterion"
    gh run download -R "$REPO_SLUG" "$RUN_ID" -n benchmark-results -D "$SRC/target/criterion"
    DATE=$(date -u '+%Y-%m-%d %H:%M:%S UTC')
    PERF="$SRC/docs/performance/index.md"
    if [ -f "$PERF" ]; then
      # BSD and GNU sed both accept -i.bak.
      sed -i.bak "s/- \*\*Date:\*\* .*/- **Date:** $DATE/" "$PERF"
      sed -i.bak "s/- \*\*Commit:\*\* .*/- **Commit:** [\`${BENCH_SHA:0:7}\`](https:\/\/github.com\/$REPO_SLUG\/commit\/${BENCH_SHA:0:7})/" "$PERF"
      rm -f "$PERF.bak"
    fi
  else
    echo "benchmarks: no unexpired benchmark-results artifact found; performance page keeps its numbers"
  fi
fi

# --- 4. build ---------------------------------------------------------------
step "docs: npm ci && npm run docs:build"
(cd "$SRC/docs" && npm ci --no-audit --no-fund --loglevel=error && npm run docs:build)
DIST="$SRC/docs/.vitepress/dist"

# --- 5. benchmark reports into the site ------------------------------------
if [ -d "$SRC/target/criterion" ] && [ -n "$(ls -A "$SRC/target/criterion" 2>/dev/null)" ]; then
  mkdir -p "$DIST/benchmarks" && cp -R "$SRC/target/criterion/." "$DIST/benchmarks/"
fi

# --- durable build-only snapshot -------------------------------------------
if [ "$BUILD_ONLY" = 1 ]; then
  OUTPUT=$(mkdir -p "$OUTPUT" && cd "$OUTPUT" && pwd)
  if [ -n "$(find "$OUTPUT" -mindepth 1 -maxdepth 1 -print -quit)" ]; then
    echo "output directory must be empty: $OUTPUT" >&2
    exit 1
  fi
  mkdir -p "$OUTPUT/dist"
  cp -R "$DIST/." "$OUTPUT/dist/"
  DIST_SHA256=$(
    cd "$OUTPUT/dist"
    find . -type f -print0 | LC_ALL=C sort -z | xargs -0 shasum -a 256 | shasum -a 256 | awk '{print $1}'
  )
  node - "$OUTPUT/snapshot-manifest.json" "$CANDIDATE_SHA" "$SHA" "$SOURCE_KIND" "$SOURCE_IDENTITY" \
    "$SNAPSHOT_TREE_HASH" "$DIST_SHA256" "$OUTPUT/dist" "${DOCS_BASE:-/}" <<'NODE'
const fs = require('node:fs');
const path = require('node:path');
const [manifestPath, candidateSha, sourceHeadSha, sourceKind, sourceIdentity, treeHash, distSha256, dist, basePath] = process.argv.slice(2);
function walk(directory, relative = '') {
  return fs.readdirSync(path.join(directory, relative), { withFileTypes: true })
    .sort((left, right) => left.name.localeCompare(right.name))
    .flatMap(entry => {
      const child = path.join(relative, entry.name);
      return entry.isDirectory() ? walk(directory, child) : [child];
    });
}
function routeFor(filename) {
  let route = `/${filename.split(path.sep).join('/')}`.replace(/\/index\.html$/, '/').replace(/\.html$/, '');
  if (route.length > 1) route = route.replace(/\/$/, '');
  return route;
}
const routes = walk(dist).filter(file => file.endsWith('.html')).map(routeFor).sort();
const manifest = {
  schema_version: 1,
  status: 'built',
  generated_at: new Date().toISOString(),
  candidate_sha: candidateSha || null,
  source_head_sha: sourceHeadSha,
  source_kind: sourceKind,
  source_identity: sourceIdentity,
  base_path: basePath,
  tree_hash: treeHash,
  dist_sha256: distSha256,
  dist: path.resolve(dist),
  routes,
};
fs.writeFileSync(manifestPath, `${JSON.stringify(manifest, null, 2)}\n`);
NODE
  step "build-only snapshot: $OUTPUT"
  echo "manifest: $OUTPUT/snapshot-manifest.json"
  echo "dist: $OUTPUT/dist"
  exit 0
fi

# --- serve + open -----------------------------------------------------------
if lsof -nP -iTCP:"$PORT" -sTCP:LISTEN >/dev/null 2>&1; then
  echo "port $PORT is in use; pass --port" >&2; exit 1
fi
step "serving $DIST on http://localhost:$PORT  (built from ${SHA:0:12}; Ctrl-C to stop)"
(cd "$SRC/docs" && exec npx --no-install vitepress preview --port "$PORT" --strictPort) &
SERVER_PID=$!
for _ in $(seq 1 50); do
  curl -fsS -o /dev/null "http://localhost:$PORT/" 2>/dev/null && break
  sleep 0.2
done
curl -fsS -o /dev/null "http://localhost:$PORT/" || { echo "server did not come up" >&2; exit 1; }

if [ "$OPEN" = 1 ]; then
  if [ -d "/Applications/Google Chrome.app" ]; then open -a "Google Chrome" "http://localhost:$PORT/"
  else open "http://localhost:$PORT/"; fi
fi
wait "$SERVER_PID"
