#!/usr/bin/env -S uv run
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Keep the fleet's model pools pointed at models that are actually UP,
rotate the mix periodically, and score which models perform best.

Why this exists: clawbay's upstream degrades per-model. On 2026-07-25 a
sweep of 11 models found only 3 serving -- every Kimi, every GLM, MiniMax
and Qwen all returned 503 "model service unavailable" together. A pool
member that is 503 does not fail fast: model_fix_loop retries it up to
max_retries with backoff, so a dead pool member burns real worker time.
Earlier the same day, ~50% of fleet calls were going to a gpt-5.5 that
was 503, purely because nothing noticed and swapped it out.

Three cadences (all configurable):
  --health-seconds   (default 240)  probe pool members; swap out any that
                                    are down for a healthy candidate
  --rotate-seconds   (default 900)  re-mix the pool from healthy models
  --report-seconds   (default 3600) write a scoreboard to the log

SAFETY -- this edits the LIVE config that 20+ workers read every round:
  * A pool is NEVER left empty. If no candidate is healthy the pool is
    left exactly as-is and the failure is logged loudly; a stale pool is
    strictly better than a config the loader rejects.
  * The rewrite is line-targeted (only `name = "..."` inside a
    [[<section>.models]] block changes), so every tuning comment in
    config.toml survives -- a tomllib round-trip would delete all of them.
  * The new text must parse as TOML AND still contain a non-empty pool
    for every section before it is installed; otherwise it is discarded.
  * Write is atomic (tempfile + os.replace) because workers copy this
    file at arbitrary times and must never observe a half-written config.

Scoring uses the existing manifest.log (one line per model call), so no
new instrumentation is needed: per model, OK rate, retry count, latency
p50, and how many landed `fix(...)` commits list it in their via-line.

Usage:
    nohup uv run scripts/model_rotator.py >> ~/.oxidex/logs/model-rotator.log 2>&1 &
    uv run scripts/model_rotator.py --once --dry-run    # inspect, change nothing
"""
import argparse
import json
import os
import re
import statistics
import sys
import tempfile
import time
import tomllib
import urllib.error
import urllib.request
from pathlib import Path

OXIDEX_HOME = Path(os.environ.get("OXIDEX_HOME", Path.home() / ".oxidex"))
DEFAULT_CONFIG = OXIDEX_HOME / "worktrees/fleet-ops/config.toml"
DEFAULT_MANIFEST = OXIDEX_HOME / "logs/model-fix-requests/manifest.log"
DEFAULT_SCOREBOARD = OXIDEX_HOME / "logs/model-scoreboard.jsonl"
DEFAULT_REPO = OXIDEX_HOME / "worktrees/fleet-ops"

# Candidates are PER PROVIDER: model ids are not portable between them
# (clawbay serves "deepseek-v4-pro", wafer serves "DeepSeek-V4-Pro", and
# wafer has no gpt-* at all). Probing one provider's ids against the
# other's endpoint finds nothing healthy, which would either wedge the
# daemon into "never rotate" or -- worse -- let it overwrite a working
# pool with names that provider cannot serve.
#
# Keyed by a substring of the endpoint so the right list is chosen from
# whatever base_url the live config currently points at. Best-known
# model first; ordering is only a tie-break before the scoreboard has
# samples.
CANDIDATES_BY_PROVIDER = {
    "theclawbay.com": [
        "deepseek-v4-pro",
        "kimi-k2.7-code",
        "kimi-k2.6",
        "glm-5.2",
        "gpt-5.6",
        "minimax-m2.5",
        "deepseek-v4-flash",
        "glm-5.1",
        "qwen3.5-397b-a17b",
        "gpt-5.5",
    ],
    "wafer.ai": [
        "Kimi-K2.6",
        "GLM-5.2",
        "DeepSeek-V4-Pro",
    ],
}
DEFAULT_CANDIDATES = CANDIDATES_BY_PROVIDER["theclawbay.com"]


def candidates_for(base_url, config=None):
    """Model ids valid for this endpoint.

    A `[rotator] candidates = [...]` list in config.toml always wins, so
    a new provider can be adopted without editing this file.
    """
    override = ((config or {}).get("rotator") or {}).get("candidates")
    if override:
        return list(override)
    for host, models in CANDIDATES_BY_PROVIDER.items():
        if host in (base_url or ""):
            return list(models)
    return list(DEFAULT_CANDIDATES)

# Sections whose [[<name>.models]] pools this daemon manages, and how many
# entries each should carry. worker has two phases (explore/patch) and the
# rewriter preserves each block's own `phase = ` line, so only names move.
MANAGED = {"worker": 2, "reviewer": 1, "table_job": 1}

_MODELS_BLOCK_RE = r"^\[\[{section}\.models\]\]\s*$"
_NAME_RE = re.compile(r'^(\s*name\s*=\s*)"([^"]*)"(.*)$')
_VIA_RE = re.compile(r"\(via ([^)]+)\)")


# ---------------------------------------------------------------------------
# Health probing
# ---------------------------------------------------------------------------

def probe_model(base_url, api_key, model, timeout=90):
    """(is_up, detail). A 503 "model service unavailable" is DOWN but
    transient -- the model stays a candidate for later rounds. A 400
    "unsupported model" is a permanent catalog answer.

    Any transport failure counts as down: from the fleet's point of view
    a model it cannot reach is indistinguishable from one that is off.
    """
    payload = json.dumps({
        "model": model,
        "messages": [{"role": "user", "content": "Reply with exactly: OK"}],
        "max_tokens": 2048,
    }).encode()
    req = urllib.request.Request(
        base_url.rstrip("/") + "/chat/completions",
        data=payload,
        headers={"Content-Type": "application/json",
                 "Authorization": f"Bearer {api_key}",
                 # urllib's default "Python-urllib/3.x" UA is rejected with
                 # a blanket 403 by this provider's edge -- every model,
                 # including ones answering 200 to curl a second earlier.
                 # Caught by dry-running before this daemon ever wrote a
                 # config; without a UA it would have concluded "nothing is
                 # healthy" forever and never rotated anything.
                 "User-Agent": "oxidex-model-rotator/1.0",
                 "Accept": "application/json"},
    )
    try:
        with urllib.request.urlopen(req, timeout=timeout) as resp:  # nosec B310
            body = json.loads(resp.read().decode("utf-8", "replace"))
    except urllib.error.HTTPError as e:
        try:
            detail = json.loads(e.read().decode("utf-8", "replace")).get("error")
        except Exception:  # nosec B110 -- error body shape is provider-defined
            detail = None
        return False, f"http {e.code}: {str(detail)[:80]}"
    except Exception as e:
        return False, f"{type(e).__name__}: {str(e)[:80]}"
    choices = body.get("choices") or [{}]
    content = (choices[0].get("message") or {}).get("content")
    if not content:
        return False, "empty reply"
    return True, "ok"


def probe_all(base_url, api_key, models, timeout=90, log=print):
    health = {}
    for m in models:
        up, detail = probe_model(base_url, api_key, m, timeout=timeout)
        health[m] = {"up": up, "detail": detail}
        log(f"    {m:<24} {'UP  ' if up else 'DOWN'} {detail}")
    return health


# ---------------------------------------------------------------------------
# Scoring from the existing manifest log
# ---------------------------------------------------------------------------

def _infer_provider(model_name):
    """Provider for manifest lines written before provider= existed.

    Historical lines carry only a model id, but the two providers used
    distinct casing/catalogues -- wafer's ids are CamelCase
    (DeepSeek-V4-Pro, Kimi-K2.6, GLM-5.2) and clawbay's are lowercase
    (deepseek-v4-pro, gpt-5.5). Best-effort, so old rows still land in a
    provider bucket instead of being silently merged with new ones.
    """
    if model_name in CANDIDATES_BY_PROVIDER["wafer.ai"]:
        return "wafer"
    if model_name.lower() == model_name:
        return "theclawbay"
    return "unknown"


def classify_failure(line):
    """'429' | '500' | 'retry' for a RETRY/ERROR manifest line.

    These are genuinely different problems and must not share a bucket:
      429  -> quota/rate. The account is the limit, not the model. This
              is what silently killed the fleet for five hours when a
              weekly spend cap hit -- indistinguishable from "the model
              got worse" if lumped into one retry count.
      500  -> provider-side fault (5xx). Transient, usually recovers.
      retry-> the HTTP call SUCCEEDED but the reply was unusable (empty
              reply, truncated reasoning). That is a model-quality
              signal, and the only one of the three that says anything
              about whether this model can actually do the work.
    """
    if "429" in line:
        return "429"
    if re.search(r"HTTPError (5\d\d)", line) or re.search(r"\b5\d\d: '", line):
        return "500"
    return "retry"


def score_models(manifest_path, since_iso=None, repo=None, git_run=None):
    """{provider-model: {ok, retry, http_429, http_500, success_rate,
    p50_latency, fixes}}.

    Keys are provider-qualified ("theclawbay-deepseek-v4-pro") because
    the same model id behaves differently per provider -- see
    provider_slug in model_fix_loop.py.

    `fixes` counts landed fix(...) commits whose subject via-list names
    the model -- the only per-model success signal the fleet records.
    """
    stats = {}
    path = Path(manifest_path)
    if path.exists():
        for line in path.read_text(errors="replace").splitlines():
            m = re.search(r"model=([A-Za-z0-9._-]+)", line)
            if not m:
                continue
            if since_iso and line[:19] < since_iso:
                continue
            model = m.group(1)
            pm = re.search(r"provider=([A-Za-z0-9._-]+)", line)
            provider = pm.group(1) if pm else _infer_provider(model)
            name = f"{provider}-{model}"
            s = stats.setdefault(name, {"ok": 0, "retry": 0, "http_429": 0,
                                        "http_500": 0, "lat": [], "fixes": 0})
            if "RETRY" in line or "ERROR=" in line:
                kind = classify_failure(line)
                s["http_429" if kind == "429" else "http_500" if kind == "500" else "retry"] += 1
            elif line.rstrip().endswith("OK"):
                s["ok"] += 1
                e = re.search(r"elapsed=([\d.]+)s", line)
                if e:
                    s["lat"].append(float(e.group(1)))

    if repo and git_run:
        try:
            out = git_run(["log", "--since=1 day ago", "--pretty=format:%s"], repo)
        except Exception:  # nosec B110 -- scoring is best-effort
            out = ""
        for subj in (out or "").splitlines():
            if not subj.startswith("fix("):
                continue
            via = _VIA_RE.search(subj)
            if not via:
                continue
            for name in {n.strip() for n in via.group(1).split("/")}:
                # via-lists carry bare model ids; credit the fix to every
                # provider bucket serving that model rather than inventing
                # a provider the commit never recorded.
                for key in list(stats) or []:
                    if key.split("-", 1)[-1] == name:
                        stats[key]["fixes"] += 1

    out = {}
    for name, s in stats.items():
        # success_rate is share of ALL attempts that came back usable --
        # quota and provider faults count against it, because from the
        # fleet's point of view they cost exactly the same wall-clock.
        total = s["ok"] + s["retry"] + s["http_429"] + s["http_500"]
        out[name] = {
            "ok": s["ok"],
            "retry": s["retry"],
            "http_429": s["http_429"],
            "http_500": s["http_500"],
            "success_rate": (s["ok"] / total) if total else None,
            "p50_latency": statistics.median(s["lat"]) if s["lat"] else None,
            "fixes": s["fixes"],
        }
    return out


def rank_candidates(healthy, scores, candidate_order=None):
    """Healthy models, best first.

    Sorts by success rate (the failure this daemon exists to avoid),
    then by landed fixes, then by lower latency. Models with no samples
    yet sort mid-pack on an assumed 0.9 rather than last, so a newly
    recovered model actually gets tried instead of being starved by
    incumbents that already have a record.
    """
    # Scores are keyed by whatever casing the manifest recorded
    # ("DeepSeek-V4-Pro"), while candidates use the API's canonical
    # lowercase ids ("deepseek-v4-pro"). Without folding case, every
    # model looks unscored and months of history is silently ignored.
    candidate_order = candidate_order or DEFAULT_CANDIDATES
    folded = {k.lower(): v for k, v in (scores or {}).items()}

    def key(m):
        s = folded.get(m.lower()) or {}
        sr = s.get("success_rate")
        sr = 0.9 if sr is None else sr
        lat = s.get("p50_latency")
        lat = 120.0 if lat is None else lat
        return (-round(sr, 2), -s.get("fixes", 0), lat, candidate_order.index(m) if m in candidate_order else 99)
    return sorted(healthy, key=key)


# ---------------------------------------------------------------------------
# Config rewriting (comment-preserving, line-targeted)
# ---------------------------------------------------------------------------

def read_pool(config_text, section):
    """[(line_index, current_name)] for each [[<section>.models]] block."""
    lines = config_text.splitlines()
    header = re.compile(_MODELS_BLOCK_RE.format(section=re.escape(section)))
    out = []
    i = 0
    while i < len(lines):
        if header.match(lines[i]):
            for j in range(i + 1, min(i + 8, len(lines))):
                if lines[j].startswith("[[") or lines[j].startswith("["):
                    break
                nm = _NAME_RE.match(lines[j])
                if nm:
                    out.append((j, nm.group(2)))
                    break
        i += 1
    return out


def rewrite_pool(config_text, section, new_names):
    """Replace only the `name = "..."` values in a section's model blocks.

    Every other line -- phase, base_url, api_key, and all the tuning
    commentary -- is untouched, which a tomllib round-trip could not do.
    """
    slots = read_pool(config_text, section)
    if not slots or not new_names:
        return config_text
    lines = config_text.splitlines(keepends=True)
    for idx, (line_no, _) in enumerate(slots):
        name = new_names[idx % len(new_names)]
        m = _NAME_RE.match(lines[line_no].rstrip("\n"))
        if not m:
            continue
        eol = "\n" if lines[line_no].endswith("\n") else ""
        lines[line_no] = f'{m.group(1)}"{name}"{m.group(3)}{eol}'
    return "".join(lines)


def config_is_safe(text):
    """Parse, and require every managed section to still have a pool."""
    try:
        data = tomllib.loads(text)
    except Exception as e:
        return False, f"TOML parse failed: {e}"
    for section in MANAGED:
        models = (data.get(section) or {}).get("models") or []
        if not models:
            return False, f"[{section}] would have an empty model pool"
        for m in models:
            if not m.get("name"):
                return False, f"[{section}] has a model with no name"
    return True, "ok"


def install_config(path, text, log=print):
    ok, why = config_is_safe(text)
    if not ok:
        log(f"  REFUSING to install config: {why}")
        return False
    path = Path(path)
    fd, tmp = tempfile.mkstemp(dir=str(path.parent), prefix=".config-rot-", suffix=".toml")
    try:
        with os.fdopen(fd, "w") as fh:
            fh.write(text)
        os.replace(tmp, path)  # atomic: workers never see a partial file
    except Exception:
        Path(tmp).unlink(missing_ok=True)
        raise
    return True


# ---------------------------------------------------------------------------
# Daemon
# ---------------------------------------------------------------------------

def apply_pools(config_path, ranked, log=print, dry_run=False):
    """Point every managed pool at the best healthy models. Returns the
    {section: [names]} actually applied (or that would be)."""
    text = Path(config_path).read_text()
    applied = {}
    for section, want in MANAGED.items():
        chosen = ranked[:want] if len(ranked) >= want else ranked[:] or None
        if not chosen:
            log(f"  [{section}] no healthy candidate -- leaving pool unchanged")
            continue
        before = [n for _, n in read_pool(text, section)]
        text = rewrite_pool(text, section, chosen)
        after = [n for _, n in read_pool(text, section)]
        applied[section] = after
        if before != after:
            log(f"  [{section}] {before} -> {after}")
    if dry_run:
        log("  (dry-run: config not written)")
        return applied
    if applied and install_config(config_path, text, log=log):
        log("  config installed (propagates to workers next round, no restart)")
    return applied


def ts():
    return time.strftime("%Y-%m-%dT%H:%M:%S")


def main(argv=None):
    p = argparse.ArgumentParser(description=__doc__,
                                formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("--config", default=str(DEFAULT_CONFIG))
    p.add_argument("--manifest", default=str(DEFAULT_MANIFEST))
    p.add_argument("--scoreboard", default=str(DEFAULT_SCOREBOARD))
    p.add_argument("--repo", default=str(DEFAULT_REPO))
    p.add_argument("--health-seconds", type=float, default=240)
    p.add_argument("--rotate-seconds", type=float, default=900)
    p.add_argument("--report-seconds", type=float, default=3600)
    p.add_argument("--probe-timeout", type=float, default=90)
    p.add_argument("--once", action="store_true", help="one pass then exit")
    p.add_argument("--dry-run", action="store_true", help="never write config")
    args = p.parse_args(argv)

    cfg = tomllib.loads(Path(args.config).read_text())
    base_url = cfg["worker"]["base_url"]
    api_key = cfg["worker"]["api_key"]
    candidates = candidates_for(base_url, cfg)

    from subprocess import run as _run  # local: only needed for fix attribution

    def git_run(a, cwd):
        return _run(["git", *a], cwd=cwd, capture_output=True, text=True, check=True).stdout  # nosec B603

    log = lambda m: print(f"[{ts()}] {m}", flush=True)  # noqa: E731
    log(f"provider={base_url} candidates={candidates}")
    log(f"model-rotator up (health={args.health_seconds}s rotate={args.rotate_seconds}s "
        f"report={args.report_seconds}s dry_run={args.dry_run})")

    last_rotate = 0.0
    last_report = 0.0
    while True:
        now = time.time()
        cur_text = Path(args.config).read_text()
        pool = {s: [n for _, n in read_pool(cur_text, s)] for s in MANAGED}
        in_use = sorted({n for names in pool.values() for n in names})

        # Always probe what is IN USE; on a rotate tick probe every candidate.
        rotating = (now - last_rotate) >= args.rotate_seconds
        to_probe = candidates if rotating else in_use
        log(f"health check ({'rotate' if rotating else 'pool-only'}): {len(to_probe)} model(s)")
        health = probe_all(base_url, api_key, to_probe, timeout=args.probe_timeout, log=log)

        scores = score_models(args.manifest, repo=args.repo, git_run=git_run)
        healthy = [m for m, h in health.items() if h["up"]]
        down_in_use = [m for m in in_use if m in health and not health[m]["up"]]

        if rotating:
            ranked = rank_candidates(healthy, scores, candidates)
            if ranked:
                log(f"rotating; healthy={ranked}")
                apply_pools(args.config, ranked, log=log, dry_run=args.dry_run)
            else:
                log("rotating: NO healthy candidate -- pools left unchanged")
            last_rotate = now
        elif down_in_use:
            log(f"pool member(s) DOWN: {down_in_use} -- swapping out")
            extra = probe_all(base_url, api_key,
                              [m for m in candidates if m not in health],
                              timeout=args.probe_timeout, log=log)
            health.update(extra)
            healthy = [m for m, h in health.items() if h["up"]]
            ranked = rank_candidates(healthy, scores, candidates)
            if ranked:
                apply_pools(args.config, ranked, log=log, dry_run=args.dry_run)
            else:
                log("  NO healthy candidate anywhere -- pools left unchanged")
        else:
            log(f"pool healthy: {in_use}")

        Path(args.scoreboard).parent.mkdir(parents=True, exist_ok=True)
        with open(args.scoreboard, "a") as fh:
            fh.write(json.dumps({"ts": ts(), "health": {m: h["up"] for m, h in health.items()},
                                 "pool": pool, "scores": scores}, separators=(",", ":")) + "\n")

        if (now - last_report) >= args.report_seconds:
            log("=" * 62)
            log("SCOREBOARD (per model, from manifest.log + landed fix commits)")
            log(f"  {'provider-model':<34}{'ok':>6}{'retry':>7}{'429':>6}{'500':>6}{'succ%':>8}{'p50s':>7}{'fixes':>7}")
            for name, s in sorted(scores.items(),
                                  key=lambda kv: -(kv[1]["success_rate"] or 0)):
                sr = "n/a" if s["success_rate"] is None else f"{100*s['success_rate']:.1f}"
                lat = "n/a" if s["p50_latency"] is None else f"{s['p50_latency']:.0f}"
                log(f"  {name:<34}{s['ok']:>6}{s['retry']:>7}{s['http_429']:>6}"
                    f"{s['http_500']:>6}{sr:>8}{lat:>7}{s['fixes']:>7}")
            log("=" * 62)
            last_report = now

        if args.once:
            return 0
        time.sleep(args.health_seconds)


if __name__ == "__main__":
    sys.exit(main())
