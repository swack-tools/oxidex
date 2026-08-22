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
- generated files are never hand-merged (tip's version only when the tip
  itself moved the file since the merge-base; a branch's own regen is
  preserved -- regen is i7-only)
- the worker VERIFIES the branch moved before reporting success -- an
  agent's claim of success is not evidence (name the instrument)
- hard wall-clock timeout; the process group dies with it

## Preflight (ARCH-FIX-SPEC.md R5)

`run()` re-asks `dispatch.economic_refusal` before it clones anything or
launches a CLI. `fleetd` already asked the same question before spawning
this process, and asking twice is the point rather than an oversight:

  * the two asks are separated by a process spawn, a clone and whatever
    queueing delay the host had, and the tip moves during that window
    (that is the entire reason convergence work exists);
  * `agentworker.py` has a `main()` and gets run by hand -- when it does,
    fleetd's check never happened at all, and this is the only guard.

The predicate itself lives in `dispatch.py` so there is exactly one
implementation of "is this run structurally pointless" and two call sites,
rather than two implementations that agree until they don't. Exit code 8 is
reserved for a preflight refusal specifically so fleetd can tell "we bought
nothing" apart from "we bought a run that failed" and hand the attempt back
to the ledger.
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

import config
import dispatch
import fleetlib
from fleetlib import Hub, HubError

AGENT_TIMEOUT_S = int(os.environ.get("FLEET_AGENT_TIMEOUT_S", "3600"))
TIP_REF = "refs/heads/refactor/tag-machinery"

# Timeouts for THIS worker's own git commands (never the agent CLI's).
# `fleetlib.run_git`'s 30s default is sized for `ls-remote`/`push` of a
# single payload ref; a full clone of the code repo is minutes of transfer
# on a cold host and a timeout here would read as an unreachable remote.
CLONE_TIMEOUT_S = int(os.environ.get("FLEET_AGENT_CLONE_TIMEOUT_S", "1800"))
GIT_TIMEOUT_S = 120

# Same cache-dir convention as doctor.py (T0.1) and ledger.py (T1.4) --
# M2 (review finding): this used to be hardcoded four times below, in the
# prompt text handed to a headless agent, which is a worse place for a
# stale literal to hide than a script default (an agent that trusts the
# prompt verbatim runs a shell command against a path that only happens to
# be right on hosts using the fallback). `tests/test_no_hardcoded_hosts.py`
# scans this file for the literal to keep it that way; the default itself
# is spelled once, in `config.py` (R6).
CACHE_DIR = str(config.exiftool_cache_dir())

# Exit codes with meaning to fleetd's attempt ledger (see module docstring
# and `dispatch.record_outcome`). 0 is progress; everything else is not.
RC_PREFLIGHT_REFUSED = 8  # nothing was bought -- the attempt is handed back
RC_BLOCKED = 9  # a real, paid run that correctly declined to guess
# T5 (review of `staging/agent-server` @ 6bf59f2b). The push of the agent's
# work is the LAST step of a run that has already cost a full agent
# invocation, and until this code existed its failure was indistinguishable
# from the agent having done nothing: the worker re-read the branch sha,
# saw it unmoved, and returned 7 "no progress". On a private HTTPS spine
# with no credential that is the NORMAL outcome, forever, on every branch --
# a host burning agent time on a loop it can never complete, reporting the
# same benign-sounding reason a genuinely stuck branch reports. 10 says
# "the work exists and could not be delivered", which is a host condition
# with a named fix, not a verdict about the branch.
RC_PUSH_AUTH_FAILED = 10

# Substrings (lower-cased) that identify a push rejection as an
# AUTHENTICATION/AUTHORIZATION failure rather than a content race. Kept
# separate from `fleetlib._TRANSPORT_HINTS`, which deliberately lumps auth
# in with DNS and timeouts because for `Hub` every one of them means "ask
# again later"; here the whole point is to tell the one that will never
# resolve on its own apart from the ones that will.
_AUTH_FAILURE_HINTS = (
    "authentication failed",
    "could not read username",
    "could not read password",
    "terminal prompts disabled",
    "permission denied",
    # GitHub's own phrasing for a push a valid credential is not
    # authorized to make: "remote: Permission to <owner>/<repo>.git denied
    # to <actor>." -- which contains neither "permission denied" (the words
    # are split by the repo name) nor "authentication failed" (the
    # credential authenticated fine; it simply may not write here). This is
    # the exact message a deploy key with read-only scope, or a PAT missing
    # `contents: write`, produces.
    "denied to",
    "invalid username or password",
    "write access to repository not granted",
    "403 forbidden",
    "the requested url returned error: 403",
    "support for password authentication was removed",
)


def _is_auth_failure(stderr: str) -> bool:
    low = (stderr or "").lower()
    return any(hint in low for hint in _AUTH_FAILURE_HINTS)

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
    """The environment the headless CLI runs under.

    T5: built from `fleetlib.credential_env()`, not from `os.environ`
    directly. The agent's task ends in `git push origin HEAD:refs/heads/
    staging/...` -- a real write to the code repo, executed by a git the
    worker never spawns -- and that push inherited whatever ambient
    credentials the box had: on the laptops the operator's osxkeychain
    entry, on a headless runner nothing at all. `credential_env` puts the
    fleet's own file-backed helper (and the `credential.helper=` reset that
    stops the host helper answering first) into `GIT_CONFIG_*`, which every
    git process started under this environment inherits, so the agent's own
    push authenticates as the host PAT like every other fleet write.

    `credential_env` is a no-op copy of `os.environ` when no token file is
    resolvable, so an ssh-spine or fixture host runs exactly the
    environment this function returned before it existed.
    """
    return {**fleetlib.credential_env(), "PATH": FLEET_PATH}


def _git(repo: Path, args: list, timeout: int = GIT_TIMEOUT_S):
    """One of this worker's own git commands, through `fleetlib.run_git`.

    T5: these were bare `subprocess.run(["git", ...])` calls. Three of them
    talk to the code remote (the clone, the tip fetch, `_fetchable`'s
    object fetch), so against a private HTTPS spine they ran with no
    credential helper, with whatever `GIT_SSH_COMMAND` the ambient
    environment carried instead of the pinned `BatchMode=yes`/
    `ConnectTimeout=10`, and with `GIT_TERMINAL_PROMPT` unset -- one prompt
    away from an agent worker parked forever. Same fix `workqueue.Queue._git`
    got in R5, and applied to the local commands too for the same reason
    given there: "which of these talks to a remote" is precisely the
    judgement that was wrong the first time.
    """
    return fleetlib.run_git(["git", "-C", str(repo), *args], timeout=timeout)


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


def build_prompt(branch: str, code_url: str, tip_sha: str, host: str) -> str:
    return f"""You are a fleet agent worker on host {host}. Re-converge the stale branch `{branch}` of the oxidex repo onto the current integration tip so its gate can pass. Work entirely inside the current directory, which is already a clone.

FACTS
- Code remote (already configured as `origin`): {code_url}
- Integration tip: refs/heads/refactor/tag-machinery at {tip_sha} -- already fetched.
- Your branch: `{branch}` -- already checked out.

TASK
1. BEFORE merging, record two shas the generated-file rule below needs: `BASE=$(git merge-base HEAD {tip_sha})` and `BRANCH_BEFORE=$(git rev-parse HEAD)`. Then merge `refactor/tag-machinery` ({tip_sha}) INTO this branch (a merge, not a rebase).
2. Resolve conflicts on their merits, with these hard rules:
   - `src/exiftool_tables/binary_tables.rs` is GENERATED: never hand-edit it, never invent enum variants. Which side wins is decided by CONTENT -- did the tip itself move the file since the merge base? Check: `git diff --quiet $BASE {tip_sha} -- src/exiftool_tables/binary_tables.rs`.
     * Tip UNCHANGED since the base (diff empty): LEAVE THE FILE ALONE. The branch's copy is the only live edit -- a branch may legitimately carry freshly REGENERATED tables (its commits will say so), and overwriting those with the tip's copy silently destroys completed regen work (this exact corruption happened once: an agent following an unconditional take-tip rule reset a verified regen; the fix cost a force-push recovery). VERIFY after committing the merge: `git diff $BRANCH_BEFORE -- src/exiftool_tables/binary_tables.rs` must be empty. A non-empty `git diff {tip_sha} -- <that path>` is EXPECTED and CORRECT here -- never "fix" it to match the tip.
     * Tip CHANGED since the base (diff non-empty): take the tip's version verbatim -- even if git auto-merged the file without conflicting, because a textual auto-merge of two regens is a chimera no generator ever produced. `--ours`/`--theirs` semantics depend on merge direction, so VERIFY by content: `git diff {tip_sha} -- src/exiftool_tables/binary_tables.rs` must be empty. If the branch had also changed the file since $BASE, note in the commit message that regen on the i7 is required before this branch can pass verify-tables.
     * Either way: if the branch changed the GENERATOR (tools/exiftool-tables/*.py), note in the commit message that regen on the i7 is still required.
   - Census/count assertions and docs counts: take the tip's side (derived invariants replaced hardcoded counts deliberately).
   - If a conflict requires deciding which of two IMPLEMENTATIONS is semantically correct, STOP: commit nothing, run `git merge --abort`, and print exactly `BLOCKED: <one-line reason>` as your final output. The worker pushes nothing for a BLOCKED run; anything already committed locally stays local and unpushed.
3. Run: `cargo fmt --all` then `cargo clippy --release --all-features --features jpeg-tag-matrix-binary -- -D warnings` (NOT --all-targets) then `cargo check --features jpeg-tag-matrix-binary`. All must pass; fix what they flag if the fix is mechanical, otherwise BLOCKED as above.
4. Commit with a message that names what was merged and every conflict resolution, then push: `git push origin HEAD:refs/heads/{branch} --force-with-lease`.
5. Final output line: `CONVERGED {branch} <new-sha>` on success.

HARD RULES
- Never push to `main` or to `refactor/tag-machinery`. Never create other branches. Never invoke bare `exiftool` (the pinned oracle is {CACHE_DIR}/exiftool-pinned.sh if needed).
- Do not weaken any test or gate to get green; a genuine failure is reported as BLOCKED, which is a valid outcome.
"""


def build_authoring_prompt(slug: str, intent: dict, code_url: str, tip_sha: str, host: str) -> str:
    scope = intent.get("scope") or {}
    fmts = ", ".join(scope.get("formats") or []) or "(see title)"
    return f"""You are a fleet agent worker on host {host}. AUTHOR the work described by a registered intent for the oxidex repo (a Rust reimplementation of ExifTool). Work entirely inside the current directory, which is already a clone checked out at the integration tip.

INTENT
- slug: {slug}
- title: {intent.get("title", "")}
- formats in scope: {fmts}
- The title quotes the measured baseline (the MISSING count under conformance.py). Your success metric is that number DROPPING, measured the same way.

TASK
1. Reproduce the baseline first: `cargo build --release --bin oxidex`, find the sample (`ls {CACHE_DIR}/combined-samples/ | grep -i <format>`), then `python3 scripts/compare_file.py <sample>`. Quote the counts.
2. Read ExifTool's own implementation in the pinned tree: {CACHE_DIR}/exiftool/lib/Image/ExifTool/<Module>.pm -- the byte layout and conversions live there. Check `src/exiftool_tables` for an existing transcription BEFORE hand-writing any layout (AGENTS.md law: re-deriving a table ExifTool already declares is the expensive way).
3. Implement in the obvious parser location (follow the existing per-format file pattern under src/parsers/). NEVER approximate a conversion: if a semantic is unresolved, omit it -- absence is correct output; a plausible-but-wrong value under a real tag name is worse.
4. Iterate implement -> build -> compare_file until the MISSING count stops dropping for honest reasons. Do not chase WRONG values into guesswork.
5. `cargo fmt --all`, then `cargo clippy --release --all-features --features jpeg-tag-matrix-binary -- -D warnings` (NOT --all-targets), then `cargo test --lib` for your module.
6. Commit quoting the instrument ("MISSING {{before}} -> {{after}} under scripts/compare_file.py on <sample>") and push: `git push origin HEAD:refs/heads/staging/{slug}`.
7. Final line on success: `AUTHORED staging/{slug} <sha>`. If genuinely blocked: push nothing and make `BLOCKED: <one-line reason>` your final line. Whatever you have committed locally stays local and unpushed -- the worker never delivers a BLOCKED run -- so do not try to undo it (a bare `git reset --hard` only discards uncommitted changes; it does not remove your own commits, and nothing needs removing).

HARD RULES
- Never push to `main` or `refactor/tag-machinery`; exactly the one branch named above.
- Never invoke bare `exiftool` -- only {CACHE_DIR}/exiftool-pinned.sh.
- Never edit src/exiftool_tables/binary_tables.rs by hand (generated).
- Do not weaken any existing test.
"""


def preflight(hub: Hub, branch: str, tip_sha: str, intent_slug: str = None) -> "tuple | None":
    """`(code, detail)` for why this run would be structurally wasted, or
    None to proceed.

    Thin wrapper over `dispatch.economic_refusal` -- it exists so the
    dispatch key this worker was launched under (`intent:<slug>` for an
    authoring run, the branch name otherwise) is reconstructed in exactly
    one place, and so a future worker-local check has an obvious home that
    is not the middle of `run()`.
    """
    key = f"{dispatch.INTENT_PREFIX}{intent_slug}" if intent_slug else branch
    return dispatch.economic_refusal(hub, key, tip_sha, tip_ref=TIP_REF)


def run(branch: str, hub_url: str, host: str, intent_slug: str = None,
        code_url: str = None) -> int:
    """`hub_url` is the STATE repo -- it answers `refs/fleet/intents/*`
    and nothing else here. `code_url` is the repo this worker CLONES and
    whose `refs/heads/*` it probes; it defaults to `hub_url`, which is the
    single-repo topology and also the safe answer when a caller (fleetd's
    `start_agent`) has not been taught to pass `--code` yet.

    The clone source was `hub_url` outright. On a split spine that clones
    the PRIVATE state repo -- a bare tree of orphan `payload.json` commits
    -- hands the agent a checkout with no source in it, and asks it to
    push a staging branch to the repo where coordination state lives.
    """
    clis = available_clis()
    if not clis:
        print("agentworker: neither `claude` nor `codex` installed on this host; exiting")
        return 4
    random.shuffle(clis)  # randomize among what the box has; order = fallback order

    hub = Hub(hub_url, workdir=Path.home() / ".fleetd" / "agentcache",
              code_url=code_url)
    code_url = hub.code_url
    tip_sha = hub.code_sha(TIP_REF)
    if intent_slug:
        branch = f"staging/{intent_slug}"
        intent = hub.read(f"refs/fleet/intents/{intent_slug}")
        if tip_sha is None or intent is None:
            print(f"agentworker: missing tip on {code_url} or intent {intent_slug} "
                  f"on {hub_url}; exiting")
            return 5
        ref = f"refs/heads/{branch}"
        before_sha = hub.code_sha(ref)  # None expected: authoring CREATES it
        if before_sha is not None:
            print(f"agentworker: {ref} already exists; intent {intent_slug} looks in-progress; exiting")
            return 0
    else:
        intent = None
        ref = f"refs/heads/{branch}"
        before_sha = hub.code_sha(ref)
        if tip_sha is None or before_sha is None:
            print(f"agentworker: missing tip or {ref} on the code repo {code_url}; exiting")
            return 5

    # PREFLIGHT -- before the clone, before any CLI, i.e. before a cent is
    # spent. fleetd asked this too; the tip may have moved since, and a
    # hand-run worker was never asked at all.
    refusal = preflight(hub, branch, tip_sha, intent_slug=intent_slug)
    if refusal is not None:
        code, detail = refusal
        print(f"agentworker[{host}] {branch}: PREFLIGHT REFUSED [{code}] {detail}")
        return RC_PREFLIGHT_REFUSED

    work = Path(tempfile.mkdtemp(prefix=f"agent-{branch.replace('/', '-')}-"))
    try:
        clone = fleetlib.run_git(["git", "clone", "-q", code_url, str(work / "r")],
                                 timeout=CLONE_TIMEOUT_S)
        if clone.returncode != 0:
            print(f"agentworker[{host}] {branch}: clone of {code_url} failed: "
                  f"{clone.stderr.strip()[-400:]}", file=sys.stderr)
            return RC_PUSH_AUTH_FAILED if _is_auth_failure(clone.stderr) else 5
        repo = work / "r"
        # T5: the agent pushes to `origin`, and `origin` is the READ url.
        # `pushurl` is the one-line way to make the agent's own push land on
        # `code_push_url` without teaching the prompt a second remote name --
        # on a single-URL fleet the two are equal and this is a no-op.
        _git(repo, ["config", "remote.origin.pushurl", hub.code_push_url])
        # detached-safe: create the local branch at the work base
        base = tip_sha if intent_slug else before_sha
        _git(repo, ["checkout", "-q", "-B", "agent-work", base])
        _git(repo, ["fetch", "-q", "origin", f"+{TIP_REF}:refs/tipref"])

        if intent_slug:
            prompt = build_authoring_prompt(intent_slug, intent, code_url, tip_sha, host)
        else:
            prompt = build_prompt(branch, code_url, tip_sha, host)
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

        # F1 (Stage 1d review): the agent's VERDICT is read FIRST, before
        # anything is pushed. The T5 delivery below used to run ahead of
        # this check, so an agent that committed partial work and then
        # correctly declared BLOCKED had that work delivered anyway and
        # reported as CONVERGED/AUTHORED -- the one outcome the prompt's
        # "BLOCKED is a valid outcome" rule exists to make impossible.
        # BLOCKED means: push NOTHING, whatever is committed locally, and
        # say what was left behind.
        blocked = any("BLOCKED" in l for l in tail)
        if blocked:
            head = _git(repo, ["rev-parse", "HEAD"]).stdout.strip()
            unpushed = head if head and head != base else None
            print(f"agentworker[{host}] {branch}: agent reported BLOCKED under {name} "
                  f"exit {proc.returncode}; pushing nothing"
                  + (f" -- {unpushed} is committed locally in {repo} and stays "
                     f"unpushed (the sandbox is discarded at exit; the sha names "
                     f"what was not delivered)" if unpushed else
                     " (no local commits)"))
            # The ref is still the instrument: an agent that pushed for
            # itself despite BLOCKED has moved the branch, and a reader of
            # this log must not be told otherwise.
            after_sha = hub.code_sha(ref)
            if after_sha != before_sha:
                print(f"agentworker[{host}] {branch}: NOTE the remote moved anyway "
                      f"({(before_sha or 'absent')[:8]} -> {(after_sha or 'absent')[:8]}): "
                      f"the agent pushed for itself despite declaring BLOCKED",
                      file=sys.stderr)
            # BLOCKED gets its OWN code rather than sharing 0 with success.
            # Both are legitimate terminal outcomes, but only one made
            # progress, and fleetd's attempt ledger resets the
            # consecutive-failure count on exit 0. Sharing the code meant a
            # branch an agent correctly refused to guess at could be
            # re-bought forever, its count reset by every refusal -- the cap
            # would never bind on the one case it exists for.
            return RC_BLOCKED

        # T5: DELIVER the work, through `fleetlib.Hub.push_code_ref`, before
        # asking whether the branch moved.
        #
        # The prompt still tells the agent to push, and an agent that
        # already did makes this an "Everything up-to-date" no-op. But the
        # agent's push and this one fail differently, and that is the whole
        # point: the agent's failure is three lines of CLI transcript the
        # worker greps for "BLOCKED" and otherwise discards, after which
        # the unmoved branch is reported as `no progress` -- exit 7, the
        # same code a branch nobody could make progress on returns. This
        # push's failure is a `_Result` with a stderr this worker reads.
        push = _deliver(hub, repo, branch, base, before_sha)
        if push is not None:
            rc, detail = push
            if rc == RC_PUSH_AUTH_FAILED:
                print(f"agentworker[{host}] {branch}: PUSH AUTHENTICATION FAILED to "
                      f"{hub.code_push_url} -- the agent's work is committed locally and "
                      f"could NOT be delivered. This is a host credential condition, not "
                      f"a verdict on the branch: check FLEET_GIT_TOKEN_FILE / "
                      f"{fleetlib.default_git_token_file()}. git said: {detail}",
                      file=sys.stderr)
                return RC_PUSH_AUTH_FAILED
            if rc != 0:
                print(f"agentworker[{host}] {branch}: push to {hub.code_push_url} "
                      f"failed (exit {rc}): {detail}", file=sys.stderr)

        # The agent's word is not the instrument -- the code ref is.
        after_sha = hub.code_sha(ref)
        if intent_slug and after_sha:
            print(f"agentworker[{host}] {branch}: AUTHORED at {after_sha[:8]} under {name}")
            return 0
        if after_sha and after_sha != before_sha:
            base_ok = _git(
                repo, ["merge-base", "--is-ancestor", tip_sha, after_sha]
            ).returncode == 0 if _fetchable(repo, after_sha) else False
            print(f"agentworker[{host}] {branch}: CONVERGED "
                  f"{before_sha[:8]} -> {after_sha[:8]} (tip-ancestry verified: {base_ok}) "
                  f"under {name} exit {proc.returncode}")
            return 0
        print(f"agentworker[{host}] {branch}: branch unchanged (no progress); "
              f"{name} exit {proc.returncode}")
        return 7
    finally:
        shutil.rmtree(work, ignore_errors=True)


def _fetchable(repo: Path, sha: str) -> bool:
    return _git(repo, ["fetch", "-q", "origin", sha]).returncode == 0


def _deliver(hub: Hub, repo: Path, branch: str, base: str, before_sha: "str | None"):
    """Push the agent's local work to `hub.code_push_url` through
    `fleetlib.Hub.push_code_ref`, or None when there is nothing to push.
    Returns `(returncode, detail)`.

    ROUTED THROUGH `Hub` (ARCH-FIX R9; `tests/test_no_raw_hub_push.py` is
    the fence -- this was a raw `git -C <repo> push` argv and tripped it).
    `Hub` pushes from its OWN object cache (`hub.workdir`), never from
    `repo`, so the agent's HEAD is fetched into that cache first, exactly
    as `train._fetch_into_hub_cache` does for `rescued/*`. That fetch is
    LOCAL (`repo` is a directory path, SPEC 4.4's "neither" row); the push
    is the one remote write here, and it carries `credential_env` and the
    pinned transport because every `Hub` write does.

    `force_with_lease=<before_sha>` on the convergence path, never a bare
    `--force`: the branch's remote head is the one the worker read before
    the agent started, and anything else there is somebody's commit the
    run must not discard (the same rule `train._retire_staging_ref`
    follows for the same reason). The authoring path creates the ref, so
    there is no lease to take.

    An agent that already pushed leaves the remote equal to local HEAD, so
    git answers "Everything up-to-date" and never consults the lease -- the
    fallback is free when it is not needed.
    """
    head = _git(repo, ["rev-parse", "HEAD"]).stdout.strip()
    if not head or head == base:
        return None  # the agent committed nothing; there is no work to deliver
    ref = f"refs/heads/{branch}"
    try:
        fetch = fleetlib.run_git(
            ["git", "--git-dir", str(hub.workdir), "fetch", "--quiet", str(repo), head],
            timeout=GIT_TIMEOUT_S)
        if fetch.returncode != 0:
            return (fetch.returncode or 1,
                    f"fetch of {head[:8]} into the hub cache failed: "
                    f"{fetch.stderr.strip()[-400:]}")
        r = hub.push_code_ref(f"{head}:{ref}", force_with_lease=before_sha,
                              timeout=GIT_TIMEOUT_S)
    except HubError as exc:
        return (1, f"{type(exc).__name__}: {exc}")
    if r.returncode == 0:
        return (0, head)
    if _is_auth_failure(r.stderr):
        return (RC_PUSH_AUTH_FAILED, r.stderr.strip()[-400:])
    return (r.returncode, r.stderr.strip()[-400:])


def main(argv=None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--branch", help="staging/<slug> to converge")
    ap.add_argument("--intent", help="intent slug to AUTHOR (mutually exclusive with --branch)")
    ap.add_argument("--hub", default=os.environ.get("FLEET_HUB_URL"),
                    help="STATE repo (refs/fleet/*)")
    # DEFAULTS TO --hub, deliberately. `fleetd.start_agent` spawns this
    # process with `--hub` only until that one argv line lands (it is
    # owned by another task in this wave), and a worker that hard-failed
    # on a missing --code would take the agent path down on every host in
    # the interim. Defaulting keeps the pre-split topology working and
    # makes the split opt-in from the spawner's side.
    ap.add_argument("--code", default=os.environ.get("FLEET_CODE_URL"),
                    help="CODE repo to clone and probe refs/heads/* on "
                         "(default: --hub)")
    ap.add_argument("--host", default=os.environ.get("FLEET_HOST", "unknown"))
    # Inert provenance stamp fleetd appends at spawn (`fleet-scope=<hex>`).
    # Its only consumer is `ps` via fleetd's orphan sweep; accepted here so
    # argparse doesn't reject the spawn, and otherwise unused.
    ap.add_argument("fleet_scope", nargs="?", default=None,
                    metavar="fleet-scope=TOKEN")
    args = ap.parse_args(argv)
    if not args.hub:
        print("agentworker: no hub URL", file=sys.stderr)
        return 2
    if bool(args.branch) == bool(args.intent):
        print("agentworker: exactly one of --branch / --intent", file=sys.stderr)
        return 2
    return run(args.branch, args.hub, args.host, intent_slug=args.intent,
               code_url=args.code or args.hub)


if __name__ == "__main__":
    raise SystemExit(main())
