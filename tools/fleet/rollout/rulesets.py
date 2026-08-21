#!/usr/bin/env python3
"""The code repo's branch rulesets, declared as data, applied idempotently.

GitHub runs no hooks for us, so every policy `tools/fleet/hooks/{update,
pre-receive}` used to enforce on the old ssh hub has to be re-expressed as a
*repository ruleset* on `swack-tools/oxidex` (docs/AGENT-SERVER-SPEC.md §8,
docs/AGENT-SERVER-PLAN.md Stage 1). This file is the single declaration of
what those rulesets are; `apply` reconciles GitHub to it and `show` reports
the drift. Both shell to `gh api` -- no PAT is read here, no token is stored
here, and the credential is whatever `gh auth status` already says it is.

    python3 tools/fleet/rollout/rulesets.py show
    python3 tools/fleet/rollout/rulesets.py apply --dry-run
    python3 tools/fleet/rollout/rulesets.py apply

WHY FIVE RULESETS AND NOT TWO
-----------------------------
A bypass actor bypasses the *whole ruleset it is attached to*, not the one
rule you meant it to relax. So "the train may update the tip, but nobody may
force-push or delete it" cannot be one ruleset with one bypass -- that
bypass would hand the train `--force` and `--delete` as well. It is two:

  tip-guard    deletion + non_fast_forward on the tip, NO bypass actors
  tip-update   restrict-updates on the tip, bypass = the keel-train deploy key

and the same split, on `refs/heads/keel-proof/*`, as the live test target so
`tests/live/test_tip_ruleset.py` never exercises the real tip:

  proof-guard  deletion + non_fast_forward, NO bypass actors
  proof-update restrict-updates, bypass = the keel-train deploy key

plus one standalone guard for the train's rescue refs, which `train.py` L624
creates and nothing is ever supposed to remove:

  rescued-guard  deletion + non_fast_forward on `refs/heads/rescued/*`

`main` keeps its own pre-existing ruleset. This tool NEVER deletes a ruleset
and never touches one whose name it does not declare (`show` lists those as
`foreign`), so `main` is out of its reach by construction.

WHY `--skip-update-rulesets` DEFAULTS ON
----------------------------------------
The two `*-update` rulesets are restrict-updates: with them active, the ONLY
principal that can advance the matching ref is a bypass actor. Create one
before the `keel-train` deploy key exists and the tip is locked against
everyone, with no bypass to unlock it -- a self-inflicted outage that takes
a repo-admin ruleset edit to undo. So they are skipped by default, and
`--no-skip-update-rulesets` still REFUSES unless a deploy key titled
`keel-train` is actually present on the repo (`_resolve_bypass_actors`).
The guard rulesets have no such hazard and are applied unconditionally.

MEASURED: HOW GITHUB STORES A DeployKey BYPASS ACTOR
----------------------------------------------------
Instrument: `gh api -X POST repos/swack-tools/oxidex/rulesets` with
`bypass_actors: [{"actor_id": 999999999, "actor_type": "DeployKey",
"bypass_mode": "always"}]` on a throwaway disabled ruleset
(`keel-bypass-probe`, id 21158251, created and deleted 2026-08-21).
GitHub accepted the deliberately-nonexistent id 999999999 and read the
ruleset back as:

    "bypass_actors":[{"actor_id":null,"actor_type":"DeployKey",
                      "bypass_mode":"always"}]

That is: **a DeployKey bypass actor is not a particular key.** GitHub
normalizes the id away and the bypass grants EVERY write-capable deploy key
on the repository. There is no per-key granularity to request, and -- worse
for anyone auditing this later -- a wrong id does not 422, it silently
becomes "all deploy keys".

Consequences, which belong in the runbook and not only here:
  1. `keel-train` must be the ONLY write-capable deploy key on
     `swack-tools/oxidex`. `apply` refuses to create an update ruleset when
     it finds more than one (see `_resolve_bypass_actors`).
  2. Adding any second write deploy key to the repo later silently widens
     tip-push authority. There is no ruleset change to notice.
Re-measure item 1's shape once the real key exists: if a future GitHub API
does honour a specific `actor_id`, `_readback_warns` below will say so,
because it compares what we sent against what came back.
"""

from __future__ import annotations

import argparse
import json
import shutil
import subprocess
import sys
from typing import Any

DEFAULT_REPO = "swack-tools/oxidex"

TIP_REF = "refs/heads/refactor/tag-machinery"
RESCUED_GLOB = "refs/heads/rescued/*"
PROOF_GLOB = "refs/heads/keel-proof/*"

# ------------------------------------------------------------------ #
# TODO(keel-train): the deploy-key bypass actor.
#
# THIS IS THE PLACEHOLDER the PLAN Stage 1 task refers to. It is a
# sentinel, not a dict, on purpose: an unresolved bypass actor must be a
# hard stop, never an empty `bypass_actors: []` that would quietly create a
# restrict-updates ruleset nobody can bypass.
#
# To land the update rulesets, a human must first:
#   1. create the deploy key on swack-tools/oxidex titled exactly
#      `keel-train`, WITH write access (SPEC §8: `~/.keel/train_deploy_key`,
#      0600, server-eligible hosts only);
#   2. confirm it is the ONLY write-capable deploy key on the repo (see the
#      module docstring's measurement -- a DeployKey bypass covers all of
#      them);
#   3. run `apply --no-skip-update-rulesets`.
#
# `_resolve_bypass_actors` turns this sentinel into the real payload and
# enforces (1) and (2). Nothing else in this file should special-case it.
#
# Note there is NO id to fill in here: `actor_id` is normalized to null by
# GitHub for actor_type DeployKey. The thing that has to exist before this
# resolves is the KEY, not a number in this file.
# ------------------------------------------------------------------ #
BYPASS_KEEL_TRAIN = "<unresolved: keel-train deploy key>"

DEPLOY_KEY_TITLE = "keel-train"

# Blocks deletion of the ref and any non-fast-forward update of it. Ref
# CREATION is deliberately not blocked: the train creates a new
# `rescued/<slug>` on every land (`train.py` L624-627), and the live proof
# test creates `keel-proof/x` on its first run.
GUARD_RULES: list[dict[str, Any]] = [
    {"type": "deletion"},
    {"type": "non_fast_forward"},
]

# "Restrict updates": only a bypass actor may advance the ref at all.
# `update_allows_fetch_and_merge` false = no exemption for merges.
UPDATE_RULES: list[dict[str, Any]] = [
    {"type": "update", "parameters": {"update_allows_fetch_and_merge": False}},
]


def _ruleset(
    name: str,
    refs: list[str],
    rules: list[dict[str, Any]],
    bypass: list[Any],
    kind: str,
) -> dict[str, Any]:
    return {
        "name": name,
        "kind": kind,  # local metadata, stripped before the API call
        "target": "branch",
        "enforcement": "active",
        "conditions": {"ref_name": {"include": list(refs), "exclude": []}},
        "rules": rules,
        "bypass_actors": bypass,
    }


# The declaration. Order is apply order: guards before the updates that
# depend on them, so a half-applied run never leaves a ref updatable-only.
RULESETS: list[dict[str, Any]] = [
    _ruleset("tip-guard", [TIP_REF], GUARD_RULES, [], kind="guard"),
    _ruleset("rescued-guard", [RESCUED_GLOB], GUARD_RULES, [], kind="guard"),
    _ruleset("proof-guard", [PROOF_GLOB], GUARD_RULES, [], kind="guard"),
    _ruleset("tip-update", [TIP_REF], UPDATE_RULES, [BYPASS_KEEL_TRAIN], kind="update"),
    _ruleset("proof-update", [PROOF_GLOB], UPDATE_RULES, [BYPASS_KEEL_TRAIN], kind="update"),
]

GUARD_NAMES = tuple(r["name"] for r in RULESETS if r["kind"] == "guard")
UPDATE_NAMES = tuple(r["name"] for r in RULESETS if r["kind"] == "update")
DECLARED_NAMES = tuple(r["name"] for r in RULESETS)


class RulesetError(RuntimeError):
    pass


# --------------------------------------------------------------------- #
# gh transport
# --------------------------------------------------------------------- #


def gh_api(args: list[str], payload: dict[str, Any] | None = None) -> Any:
    """`gh api <args>`, returning parsed JSON (None for an empty body).

    Kept as one function so tests can stub exactly one seam and so every
    failure carries gh's own stderr rather than a bare CalledProcessError.
    """
    if shutil.which("gh") is None:
        raise RulesetError("gh not on PATH -- install the GitHub CLI and `gh auth login`")
    cmd = ["gh", "api", *args]
    text = None
    if payload is not None:
        cmd += ["--input", "-"]
        text = json.dumps(payload)
    proc = subprocess.run(cmd, input=text, capture_output=True, text=True)
    if proc.returncode != 0:
        raise RulesetError(
            f"{' '.join(cmd)} -> rc {proc.returncode}\n{proc.stderr.strip()}"
        )
    out = proc.stdout.strip()
    if not out:
        return None
    try:
        return json.loads(out)
    except json.JSONDecodeError as exc:  # pragma: no cover - gh always emits JSON
        raise RulesetError(f"{' '.join(cmd)} -> unparseable output: {exc}") from exc


def list_rulesets(repo: str) -> list[dict[str, Any]]:
    return gh_api([f"repos/{repo}/rulesets"]) or []


def get_ruleset(repo: str, ruleset_id: int) -> dict[str, Any]:
    return gh_api([f"repos/{repo}/rulesets/{ruleset_id}"])


def list_deploy_keys(repo: str) -> list[dict[str, Any]]:
    return gh_api([f"repos/{repo}/keys"]) or []


# --------------------------------------------------------------------- #
# bypass resolution
# --------------------------------------------------------------------- #


def _resolve_bypass_actors(bypass: list[Any], repo: str) -> list[dict[str, Any]]:
    """Turn declared bypass entries into API payloads, or refuse.

    The only sentinel is `BYPASS_KEEL_TRAIN`. Resolving it asserts both
    halves of the measurement in the module docstring: the key exists, and
    it is the only write-capable deploy key on the repo (because a DeployKey
    bypass covers every one of them).
    """
    out: list[dict[str, Any]] = []
    for entry in bypass:
        if entry != BYPASS_KEEL_TRAIN:
            out.append(dict(entry))
            continue
        keys = list_deploy_keys(repo)
        writable = [k for k in keys if not k.get("read_only", True)]
        named = [k for k in keys if k.get("title") == DEPLOY_KEY_TITLE]
        if not named:
            raise RulesetError(
                f"deploy key {DEPLOY_KEY_TITLE!r} does not exist on {repo} "
                f"(found {len(keys)}: {[k.get('title') for k in keys]}). "
                "Creating a restrict-updates ruleset now would lock the ref "
                "against everyone with no bypass. See the TODO(keel-train) "
                "block in this file."
            )
        if len(writable) > 1:
            raise RulesetError(
                f"{repo} has {len(writable)} write-capable deploy keys "
                f"({[k.get('title') for k in writable]}); a DeployKey bypass "
                "actor covers ALL of them (see this file's measurement), so "
                "the tip would be pushable by every one. Remove the extras "
                "first."
            )
        # actor_id is normalized to null by GitHub for DeployKey; sending
        # the real id anyway makes the readback comparison meaningful if
        # that ever changes.
        out.append(
            {
                "actor_id": named[0].get("id"),
                "actor_type": "DeployKey",
                "bypass_mode": "always",
            }
        )
    return out


def _readback_warns(sent: list[dict[str, Any]], got: list[dict[str, Any]]) -> list[str]:
    """Differences between the bypass actors we sent and the ones GitHub
    stored. Empty means GitHub honoured the request verbatim."""
    warns = []
    if len(sent) != len(got):
        warns.append(f"bypass_actors count {len(sent)} sent, {len(got)} stored")
        return warns
    for s, g in zip(sent, got):
        if s.get("actor_type") == "DeployKey" and g.get("actor_id") is None and s.get("actor_id") is not None:
            warns.append(
                f"DeployKey actor_id {s['actor_id']} was normalized to null: "
                "the bypass grants EVERY write deploy key on this repo"
            )
        elif s.get("actor_id") != g.get("actor_id"):
            warns.append(f"actor_id {s.get('actor_id')} sent, {g.get('actor_id')} stored")
    return warns


# --------------------------------------------------------------------- #
# comparison
# --------------------------------------------------------------------- #


def _norm_rules(rules: list[dict[str, Any]]) -> list[dict[str, Any]]:
    """Rules in a form the declaration and the API readback agree on.

    GitHub drops `parameters` entries it considers default: a rule sent as
    `{"type":"update","parameters":{"update_allows_fetch_and_merge":false}}`
    reads back as `{"type":"update"}` (measured, same probe as the docstring).
    So falsy parameter values are dropped on both sides before comparing,
    and rules are sorted by type -- order is not semantic.
    """
    norm = []
    for rule in rules:
        params = {k: v for k, v in (rule.get("parameters") or {}).items() if v}
        item: dict[str, Any] = {"type": rule["type"]}
        if params:
            item["parameters"] = params
        norm.append(item)
    return sorted(norm, key=lambda r: r["type"])


def _norm_actors(actors: list[dict[str, Any]]) -> list[dict[str, Any]]:
    return sorted(
        (
            {
                "actor_type": a.get("actor_type"),
                "actor_id": a.get("actor_id"),
                "bypass_mode": a.get("bypass_mode"),
            }
            for a in actors
        ),
        key=lambda a: (str(a["actor_type"]), str(a["actor_id"])),
    )


def _norm_refs(conditions: dict[str, Any]) -> dict[str, list[str]]:
    ref = (conditions or {}).get("ref_name") or {}
    return {
        "include": sorted(ref.get("include") or []),
        "exclude": sorted(ref.get("exclude") or []),
    }


def comparable(spec: dict[str, Any]) -> dict[str, Any]:
    """The semantic identity of a ruleset: what `apply` compares."""
    return {
        "target": spec.get("target"),
        "enforcement": spec.get("enforcement"),
        "conditions": _norm_refs(spec.get("conditions") or {}),
        "rules": _norm_rules(spec.get("rules") or []),
        "bypass_actors": _norm_actors(spec.get("bypass_actors") or []),
    }


def api_payload(spec: dict[str, Any], bypass: list[dict[str, Any]]) -> dict[str, Any]:
    """The create/update body: the declaration minus our local `kind`, with
    bypass actors already resolved."""
    return {
        "name": spec["name"],
        "target": spec["target"],
        "enforcement": spec["enforcement"],
        "conditions": spec["conditions"],
        "rules": spec["rules"],
        "bypass_actors": bypass,
    }


# --------------------------------------------------------------------- #
# subcommands
# --------------------------------------------------------------------- #


def selected(skip_update: bool) -> list[dict[str, Any]]:
    return [r for r in RULESETS if not (skip_update and r["kind"] == "update")]


def cmd_show(args: argparse.Namespace) -> int:
    live = list_rulesets(args.repo)
    by_name = {r["name"]: r for r in live}
    rows = []
    for spec in RULESETS:
        name = spec["name"]
        if name not in by_name:
            state = "absent"
            detail = ""
        else:
            full = get_ruleset(args.repo, by_name[name]["id"])
            # A declared-but-unresolved bypass cannot be compared without a
            # network call for the key; report it as such rather than lying.
            if BYPASS_KEEL_TRAIN in spec["bypass_actors"]:
                state = "present"
                detail = f"id={full['id']} bypass_actors={_norm_actors(full.get('bypass_actors') or [])}"
            elif comparable(full) == comparable(spec):
                state = "in-sync"
                detail = f"id={full['id']}"
            else:
                state = "drift"
                detail = f"id={full['id']}"
        rows.append((name, spec["kind"], state, ",".join(_norm_refs(spec["conditions"])["include"]), detail))
    foreign = [r for r in live if r["name"] not in DECLARED_NAMES]

    if args.json:
        print(json.dumps({
            "repo": args.repo,
            "declared": [{"name": n, "kind": k, "state": s, "refs": f, "detail": d} for n, k, s, f, d in rows],
            "foreign": [{"name": r["name"], "id": r["id"], "enforcement": r["enforcement"]} for r in foreign],
        }, indent=2))
        return 0

    print(f"=== instrument: rulesets.py show === repo={args.repo} gh={_gh_version()}")
    width = max(len(n) for n, *_ in rows)
    for name, kind, state, refs, detail in rows:
        print(f"  {name:<{width}}  {kind:<6}  {state:<9}  {refs}  {detail}")
    for r in foreign:
        print(f"  {r['name']:<{width}}  FOREIGN  {r['enforcement']}  (not declared here; never touched)")
    return 0


def _gh_version() -> str:
    try:
        proc = subprocess.run(["gh", "--version"], capture_output=True, text=True)
        return proc.stdout.splitlines()[0] if proc.stdout else "?"
    except OSError:
        return "?"


def cmd_apply(args: argparse.Namespace) -> int:
    specs = selected(args.skip_update_rulesets)
    if args.skip_update_rulesets:
        print(
            "rulesets: --skip-update-rulesets is ON (default): "
            f"{', '.join(UPDATE_NAMES)} NOT applied. See TODO(keel-train).",
            file=sys.stderr,
        )
    live = {r["name"]: r for r in list_rulesets(args.repo)}
    print(f"=== instrument: rulesets.py apply === repo={args.repo} gh={_gh_version()} "
          f"dry_run={args.dry_run} skip_update={args.skip_update_rulesets}")
    rc = 0
    for spec in specs:
        name = spec["name"]
        try:
            bypass = _resolve_bypass_actors(spec["bypass_actors"], args.repo)
        except RulesetError as exc:
            print(f"  {name}: REFUSED -- {exc}", file=sys.stderr)
            rc = 1
            continue
        body = api_payload(spec, bypass)
        want = comparable(body)
        if name in live:
            full = get_ruleset(args.repo, live[name]["id"])
            if comparable(full) == want:
                print(f"  {name}: unchanged (id={full['id']})")
                continue
            if args.dry_run:
                print(f"  {name}: WOULD UPDATE (id={full['id']})")
                continue
            got = gh_api(["-X", "PUT", f"repos/{args.repo}/rulesets/{full['id']}"], body)
            print(f"  {name}: updated (id={got['id']})")
        else:
            if args.dry_run:
                print(f"  {name}: WOULD CREATE")
                continue
            got = gh_api(["-X", "POST", f"repos/{args.repo}/rulesets"], body)
            print(f"  {name}: created (id={got['id']})")
        for warn in _readback_warns(bypass, got.get("bypass_actors") or []):
            print(f"  {name}: WARNING -- {warn}", file=sys.stderr)
        if comparable(got) != want:
            print(f"  {name}: WARNING -- readback differs from declaration", file=sys.stderr)
            rc = 1
    return rc


def build_parser() -> argparse.ArgumentParser:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--repo", default=DEFAULT_REPO, help=f"owner/name (default {DEFAULT_REPO})")
    sub = ap.add_subparsers(dest="cmd", required=True)

    p_show = sub.add_parser("show", help="declared vs live, plus foreign rulesets")
    p_show.add_argument("--json", action="store_true")
    p_show.set_defaults(func=cmd_show)

    p_apply = sub.add_parser("apply", help="create-or-update by name; never deletes")
    p_apply.add_argument("--dry-run", action="store_true")
    # Default ON. `--skip-update-rulesets` is accepted anyway so a script
    # that spells the safe intent out loud keeps working when the default
    # flips after the deploy key exists.
    p_apply.add_argument(
        "--skip-update-rulesets", dest="skip_update_rulesets",
        action="store_true", default=True,
        help="do not apply tip-update/proof-update (DEFAULT: on)",
    )
    p_apply.add_argument(
        "--no-skip-update-rulesets", dest="skip_update_rulesets",
        action="store_false",
        help="also apply the restrict-updates rulesets; still refuses unless "
             f"the {DEPLOY_KEY_TITLE!r} deploy key exists",
    )
    p_apply.set_defaults(func=cmd_apply)
    return ap


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    try:
        return args.func(args)
    except RulesetError as exc:
        print(f"rulesets: {exc}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
