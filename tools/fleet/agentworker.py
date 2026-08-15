"""agentworker -- a fleet-launched headless coding agent (FLEET.md M2's
`agents` column, first real implementation).

fleetd spawns one of these per agent slot with a target branch. The worker
builds a task prompt and hands it to whichever headless coding CLI the
host actually has -- `claude` or `codex` -- chosen at random among the
installed ones ("randomize depending on what is on the box"). The CLIs
are invoked exactly as their headless modes document:

    claude -p "<prompt>" --dangerously-skip-permissions
    codex exec --yolo "<prompt>"

The v1 task is queue convergence: take a STALE staging branch (one whose
gate would fail only because the tip moved) and re-converge it -- merge
onto the current tip, resolve per doctrine, fastcheck, push the branch
back with force-with-lease. That is the fleet's actual recurring need:
every branch that went red this week from drift rather than defect is
this task. Authoring NEW work from intents is the next iteration.

Guardrails, all encoded in the prompt and enforced by what the worker
verifies afterwards:
- never push to main or refactor/tag-machinery; exactly one branch
- generated files are never hand-merged (tip's version; regen is i7-only)
- the worker VERIFIES the branch moved before reporting success -- an
  agent's claim of success is not evidence (name the instrument)
- hard wall-clock timeout; the process group dies with it
"""

from __future__ import annotations

import argparse
import os
import random
import shutil
import subprocess
import sys
import tempfile
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from fleetlib import Hub

AGENT_TIMEOUT_S = int(os.environ.get("FLEET_AGENT_TIMEOUT_S", "3600"))
TIP_REF = "refs/heads/refactor/tag-machinery"

# The PATH gate.sh uses, mirrored here: fleetd under systemd/launchd
# inherits a minimal PATH that misses ~/.local/bin (claude) and the nvm
# bin (codex) -- found live on the i7, where both CLIs existed but
# neither was visible to a daemon-spawned which().
_HOME = str(Path.home())
FLEET_PATH = os.pathsep.join([
    f"{_HOME}/.nvm/versions/node/v24.13.1/bin",
    f"{_HOME}/.cargo/bin",
    f"{_HOME}/.local/bin",
    "/opt/homebrew/bin",
    "/usr/local/bin",
    os.environ.get("PATH", "/usr/bin:/bin"),
])


def _agent_env() -> dict:
    return {**os.environ, "PATH": FLEET_PATH}


def available_clis() -> list:
    """Headless CLIs present on this host, as (name, argv-builder) pairs.
    FLEET_AGENT_CLI_OVERRIDE=<path> substitutes a stub for tests."""
    override = os.environ.get("FLEET_AGENT_CLI_OVERRIDE")
    if override:
        return [("stub", lambda prompt: [override, prompt])]
    out = []
    claude_bin = shutil.which("claude", path=FLEET_PATH)
    codex_bin = shutil.which("codex", path=FLEET_PATH)
    if claude_bin:
        out.append(("claude", lambda prompt, b=claude_bin: [
            b, "-p", prompt, "--dangerously-skip-permissions"]))
    if codex_bin:
        out.append(("codex", lambda prompt, b=codex_bin: [b, "exec", "--yolo", prompt]))
    return out


def build_prompt(branch: str, hub_url: str, tip_sha: str, host: str) -> str:
    return f"""You are a fleet agent worker on host {host}. Re-converge the stale branch `{branch}` of the oxidex repo onto the current integration tip so its gate can pass. Work entirely inside the current directory, which is already a clone.

FACTS
- Hub remote (already configured as `origin`): {hub_url}
- Integration tip: refs/heads/refactor/tag-machinery at {tip_sha} -- already fetched.
- Your branch: `{branch}` -- already checked out.

TASK
1. Merge `refactor/tag-machinery` ({tip_sha}) INTO this branch (a merge, not a rebase).
2. Resolve conflicts on their merits, with these hard rules:
   - `src/exiftool_tables/binary_tables.rs` is GENERATED: never hand-edit it, never invent enum variants. Resolve it to the tip's version ONLY IF THE MERGE CONFLICTS ON IT. If the merge does not touch it, LEAVE IT ALONE -- a branch may legitimately carry freshly REGENERATED tables (its commits will say so), and overwriting those with the tip's copy silently destroys completed regen work (this exact corruption happened once: an agent following an unconditional take-tip rule reset a verified regen; the fix cost a force-push recovery). When you do take the tip's side of a conflict and the branch changed the GENERATOR (tools/exiftool-tables/*.py), note in the commit message that regen on the i7 is still required.
   - Census/count assertions and docs counts: take the tip's side (derived invariants replaced hardcoded counts deliberately).
   - If a conflict requires deciding which of two IMPLEMENTATIONS is semantically correct, STOP: commit nothing, run `git merge --abort`, and print exactly `BLOCKED: <one-line reason>` as your final output.
3. Run: `cargo fmt --all` then `cargo clippy --release --all-features --features jpeg-tag-matrix-binary -- -D warnings` (NOT --all-targets) then `cargo check --features jpeg-tag-matrix-binary`. All must pass; fix what they flag if the fix is mechanical, otherwise BLOCKED as above.
4. Commit with a message that names what was merged and every conflict resolution, then push: `git push origin HEAD:refs/heads/{branch} --force-with-lease`.
5. Final output line: `CONVERGED {branch} <new-sha>` on success.

HARD RULES
- Never push to `main` or to `refactor/tag-machinery`. Never create other branches. Never invoke bare `exiftool` (the pinned oracle is /tmp/oxidex-exiftool-cache/exiftool-pinned.sh if needed).
- Do not weaken any test or gate to get green; a genuine failure is reported as BLOCKED, which is a valid outcome.
"""


def build_authoring_prompt(slug: str, intent: dict, hub_url: str, tip_sha: str, host: str) -> str:
    scope = intent.get("scope") or {}
    fmts = ", ".join(scope.get("formats") or []) or "(see title)"
    return f"""You are a fleet agent worker on host {host}. AUTHOR the work described by a registered intent for the oxidex repo (a Rust reimplementation of ExifTool). Work entirely inside the current directory, which is already a clone checked out at the integration tip.

INTENT
- slug: {slug}
- title: {intent.get("title", "")}
- formats in scope: {fmts}
- The title quotes the measured baseline (the MISSING count under conformance.py). Your success metric is that number DROPPING, measured the same way.

TASK
1. Reproduce the baseline first: `cargo build --release --bin oxidex`, find the sample (`ls /tmp/oxidex-exiftool-cache/combined-samples/ | grep -i <format>`), then `python3 scripts/compare_file.py <sample>`. Quote the counts.
2. Read ExifTool's own implementation in the pinned tree: /tmp/oxidex-exiftool-cache/exiftool/lib/Image/ExifTool/<Module>.pm -- the byte layout and conversions live there. Check `src/exiftool_tables` for an existing transcription BEFORE hand-writing any layout (AGENTS.md law: re-deriving a table ExifTool already declares is the expensive way).
3. Implement in the obvious parser location (follow the existing per-format file pattern under src/parsers/). NEVER approximate a conversion: if a semantic is unresolved, omit it -- absence is correct output; a plausible-but-wrong value under a real tag name is worse.
4. Iterate implement -> build -> compare_file until the MISSING count stops dropping for honest reasons. Do not chase WRONG values into guesswork.
5. `cargo fmt --all`, then `cargo clippy --release --all-features --features jpeg-tag-matrix-binary -- -D warnings` (NOT --all-targets), then `cargo test --lib` for your module.
6. Commit quoting the instrument ("MISSING {{before}} -> {{after}} under scripts/compare_file.py on <sample>") and push: `git push origin HEAD:refs/heads/staging/{slug}`.
7. Final line on success: `AUTHORED staging/{slug} <sha>`. If genuinely blocked, `git reset --hard`, push nothing, final line `BLOCKED: <one-line reason>`.

HARD RULES
- Never push to `main` or `refactor/tag-machinery`; exactly the one branch named above.
- Never invoke bare `exiftool` -- only /tmp/oxidex-exiftool-cache/exiftool-pinned.sh.
- Never edit src/exiftool_tables/binary_tables.rs by hand (generated).
- Do not weaken any existing test.
"""


def run(branch: str, hub_url: str, host: str, intent_slug: str = None) -> int:
    clis = available_clis()
    if not clis:
        print("agentworker: neither `claude` nor `codex` installed on this host; exiting")
        return 4
    random.shuffle(clis)  # randomize among what the box has; order = fallback order

    hub = Hub(hub_url, workdir=Path.home() / ".fleetd" / "agentcache")
    tip_sha = hub.sha(TIP_REF)
    if intent_slug:
        branch = f"staging/{intent_slug}"
        intent = hub.read(f"refs/fleet/intents/{intent_slug}")
        if tip_sha is None or intent is None:
            print(f"agentworker: missing tip or intent {intent_slug} on hub; exiting")
            return 5
        ref = f"refs/heads/{branch}"
        before_sha = hub.sha(ref)  # None expected: authoring CREATES it
        if before_sha is not None:
            print(f"agentworker: {ref} already exists; intent {intent_slug} looks in-progress; exiting")
            return 0
    else:
        intent = None
        ref = f"refs/heads/{branch}"
        before_sha = hub.sha(ref)
        if tip_sha is None or before_sha is None:
            print(f"agentworker: missing tip or {ref} on hub; exiting")
            return 5

    work = Path(tempfile.mkdtemp(prefix=f"agent-{branch.replace('/', '-')}-"))
    try:
        subprocess.run(["git", "clone", "-q", hub_url, str(work / "r")], check=True)
        repo = work / "r"
        subprocess.run(["git", "-C", str(repo), "checkout", "-q", "-B", branch.split("/", 1)[-1]
                        if False else branch, before_sha], check=False)
        # detached-safe: create the local branch at the branch head
        base = tip_sha if intent_slug else before_sha
        subprocess.run(["git", "-C", str(repo), "checkout", "-q", "-B", "agent-work", base],
                       check=True)
        subprocess.run(["git", "-C", str(repo), "fetch", "-q", "origin",
                        f"+{TIP_REF}:refs/tipref"], check=True)

        if intent_slug:
            prompt = build_authoring_prompt(intent_slug, intent, hub_url, tip_sha, host)
        else:
            prompt = build_prompt(branch, hub_url, tip_sha, host)
        proc, name, tail = None, None, []
        for name, argv_of in clis:
            print(f"agentworker[{host}] {branch}: launching {name} (timeout {AGENT_TIMEOUT_S}s)")
            t0 = time.time()
            try:
                proc = subprocess.run(
                    argv_of(prompt), cwd=repo, timeout=AGENT_TIMEOUT_S,
                    capture_output=True, text=True, errors="replace", env=_agent_env(),
                )
            except subprocess.TimeoutExpired:
                print(f"agentworker[{host}] {branch}: {name} hit the {AGENT_TIMEOUT_S}s wall; killed")
                return 6
            dur = int(time.time() - t0)
            out = (proc.stdout or "") + (proc.stderr or "")
            tail = out.strip().splitlines()[-3:]
            # Auth failures are a HOST condition, not a task verdict: fall
            # back to the other CLI once (found live: m5's claude CLI said
            # "Not logged in" while its codex was fine).
            auth_broken = any(sig in out for sig in ("Not logged in", "please run /login",
                                                     "Please run /login", "not authenticated"))
            if auth_broken and len(clis) > 1:
                print(f"agentworker[{host}] {branch}: {name} not authenticated; falling back")
                continue
            print(f"agentworker[{host}] {branch}: {name} exited {proc.returncode} in {dur}s; "
                  f"tail: {' | '.join(tail) if tail else '(no output)'}")
            break

        # The agent's word is not the instrument -- the hub ref is.
        after_sha = hub.sha(ref)
        if intent_slug and after_sha:
            print(f"agentworker[{host}] {branch}: AUTHORED at {after_sha[:8]} under {name}")
            return 0
        if after_sha and after_sha != before_sha:
            base_ok = subprocess.run(
                ["git", "-C", str(repo), "merge-base", "--is-ancestor", tip_sha, after_sha],
            ).returncode == 0 if _fetchable(repo, after_sha) else False
            print(f"agentworker[{host}] {branch}: CONVERGED "
                  f"{before_sha[:8]} -> {after_sha[:8]} (tip-ancestry verified: {base_ok}) "
                  f"under {name} exit {proc.returncode}")
            return 0
        blocked = any("BLOCKED" in l for l in tail)
        print(f"agentworker[{host}] {branch}: branch unchanged "
              f"({'agent reported BLOCKED' if blocked else 'no progress'}); {name} exit {proc.returncode}")
        return 0 if blocked else 7
    finally:
        shutil.rmtree(work, ignore_errors=True)


def _fetchable(repo: Path, sha: str) -> bool:
    r = subprocess.run(["git", "-C", str(repo), "fetch", "-q", "origin", sha], capture_output=True)
    return r.returncode == 0


def main(argv=None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--branch", help="staging/<slug> to converge")
    ap.add_argument("--intent", help="intent slug to AUTHOR (mutually exclusive with --branch)")
    ap.add_argument("--hub", default=os.environ.get("FLEET_HUB_URL"))
    ap.add_argument("--host", default=os.environ.get("FLEET_HOST", "unknown"))
    args = ap.parse_args(argv)
    if not args.hub:
        print("agentworker: no hub URL", file=sys.stderr)
        return 2
    if bool(args.branch) == bool(args.intent):
        print("agentworker: exactly one of --branch / --intent", file=sys.stderr)
        return 2
    return run(args.branch, args.hub, args.host, intent_slug=args.intent)


if __name__ == "__main__":
    raise SystemExit(main())
