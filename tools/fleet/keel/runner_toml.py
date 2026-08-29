#!/usr/bin/env python3
"""``~/.keel/runner.toml`` loader (PLAN Stage 3 task 7).

WHAT THIS OWNS. `keel-runner` (SPEC SS2 C7) is the one component every
host runs, unsupervised, from a `units/*` template -- there is no
argv a human types per invocation the way there is for `keel status` or
`verdict.py store`. Its four remotes (state hub, code repo, keel-server,
the two token files) and its two local capacity caps need a place to
live that survives a reboot and does not require editing a systemd unit
or a launchd plist to change. `~/.keel/runner.toml` is that place; this
module is the ONE parser for it (`tomllib`, Python 3.11+ stdlib -- no
third-party TOML library, matching every other file under
`tools/fleet/keel/`: stdlib only). A host with no file at all parses as
"every field unset", not an error -- exactly the shape a freshly imaged
host has today, with every value coming from its production default
instead.

PRECEDENCE (env beats file beats default), same order as every other
config surface in this tree (`keel/cli.py`'s `_resolve`:
"explicit arg or env var"; `config.default_git_token_file`'s "env var, or
the default path"). Concretely: `resolve()` reads the file, then
overwrites any field whose environment variable is SET (non-empty) --

    field               toml path             env var (wins)
    ------------------  --------------------  -----------------------
    hub_url             [hub]    url          FLEET_HUB_URL
    code_url            [code]   url          FLEET_CODE_URL
    server_url          [server] url          KEEL_SERVER_URL
    git_token_file      [token]  git_file     FLEET_GIT_TOKEN_FILE
    server_token_file   [token]  server_file  KEEL_TOKEN_FILE
    max_gates           [limits] max_gates    KEEL_MAX_GATES
    max_agents          [limits] max_agents   KEEL_MAX_AGENTS
    autonomous_when_serverless
                        [autonomy] when_serverless
                                              KEEL_AUTONOMOUS_WHEN_SERVERLESS
    rank                [server] rank         KEEL_RANK
    server_eligible     [server] eligible     KEEL_SERVER_ELIGIBLE

The three keel-3R fields all default to `None`, and the consumer's own
production default for `autonomous_when_serverless` is FALSE (SPEC SS12:
"config, default false; enabled on the i7 only"). `_from_toml` reads
named tables and silently ignores unknown ones, so `[autonomy]` and the
two new `[server]` keys are backward-compatible with every deployed file
-- a host whose `runner.toml` predates them parses exactly as before.

The BOOLEAN env vars parse `1/true/yes/on` and `0/false/no/off`,
case-insensitively, and RAISE on anything else. That is deliberate and
is the opposite of a truthiness test: `KEEL_AUTONOMOUS_WHEN_SERVERLESS=0`
under `bool(os.environ.get(...))` is TRUE, so the one spelling an
operator would reach for to turn the feature off would turn it on. A
loud parse error is the only safe answer to a value nobody can read.

`hub_url`/`code_url`/`git_token_file` reuse the EXACT env var names
`fleetlib`/`config`/`gate.sh`/`cli.py` already read (`FLEET_HUB_URL`,
`FLEET_CODE_URL`, `FLEET_GIT_TOKEN_FILE`) -- not a second name for the
same thing. `server_url`/`server_token_file` reuse `keel/cli.py`'s own
`KEEL_SERVER_URL`/`KEEL_TOKEN_FILE`. (One spec table row, SPEC SS9's
reuse map, names the state-repo env var `FLEET_STATE_URL`; nowhere else
in this tree -- not `fleetlib.py`, not `gate.sh`, not `cli.py`, not
`doctor.py` -- uses that spelling, and introducing a second name for the
hub URL is exactly the "spelled in more than one place" failure mode
`config.py`'s own docstring exists to prevent. `FLEET_HUB_URL` is used
here, matching the other 90%+ of this codebase, not that one table
cell.)

`max_gates`/`max_agents` are a LOCAL ceiling this runner enforces
regardless of what `refs/fleet/desired` asks for -- independent of, and
composed with (`min(...)`, by the caller), the hub-side `gates`/`agents`
counts `fleetd.reconcile_once` already reads off `my_desired` (`fleetd.py`
L2032, L2151). This loader does not talk to the hub at all; it is purely
local, so a host can cap its own exposure without needing a hub write
(and without an operator with hub access) whenever that is the safer
knob -- a laptop that should never take more than one gate no matter
what `desired` says while a human is debugging it, for example. `None`
(the toml key absent, the env var absent) means "no local cap", i.e.
defer entirely to `desired`; it is never conflated with `0`, which means
"cap this host at zero" and is a real, distinct value a runner honours.

WHAT THIS DOES NOT OWN. This module never reads `os.environ` beyond
`resolve()`'s explicit overrides (`load()` alone is pure), never opens
either token file's CONTENTS (paths only, exactly like
`doctor.check_git_token_file`), and never talks to git, the hub, or a
`keel-server` -- it is config parsing, nothing else. Turning a
`RunnerConfig` into the actual env a runner subprocess/daemon runs under
is the caller's job (`runner.py`, once it exists); `NotImplementedError`
in this module means "read the file, don't guess at the rest."
"""

from __future__ import annotations

import os
from dataclasses import asdict, dataclass, fields
from pathlib import Path
from typing import Any, Dict, Optional

DEFAULT_PATH = Path.home() / ".keel" / "runner.toml"

# field name -> the environment variable that overrides it. Order here is
# purely documentary (dict iteration order); `resolve()` applies every
# entry independently.
ENV_VARS: Dict[str, str] = {
    "hub_url": "FLEET_HUB_URL",
    "code_url": "FLEET_CODE_URL",
    "server_url": "KEEL_SERVER_URL",
    "git_token_file": "FLEET_GIT_TOKEN_FILE",
    "server_token_file": "KEEL_TOKEN_FILE",
    "max_gates": "KEEL_MAX_GATES",
    "max_agents": "KEEL_MAX_AGENTS",
    "autonomous_when_serverless": "KEEL_AUTONOMOUS_WHEN_SERVERLESS",
    "rank": "KEEL_RANK",
    "server_eligible": "KEEL_SERVER_ELIGIBLE",
}

_INT_FIELDS = frozenset({"max_gates", "max_agents", "rank"})
_BOOL_FIELDS = frozenset({"autonomous_when_serverless", "server_eligible"})

#: The exact accepted spellings. Anything else raises -- see the module
#: docstring on why a truthiness test is unsafe here.
_TRUE = frozenset({"1", "true", "yes", "on"})
_FALSE = frozenset({"0", "false", "no", "off"})


@dataclass(frozen=True)
class RunnerConfig:
    """Every field defaults to `None` -- "not configured here", not "off"
    or "zero". A consumer that needs a concrete value applies its OWN
    production default on top of a `None`, the same way `fleetlib.Hub`'s
    own `--hub`/`FLEET_HUB_URL` resolution works today; this loader's job
    ends at "what did the file/environment say", not "and here is what to
    do about nothing having said anything"."""

    hub_url: Optional[str] = None
    code_url: Optional[str] = None
    server_url: Optional[str] = None
    git_token_file: Optional[str] = None
    server_token_file: Optional[str] = None
    max_gates: Optional[int] = None
    max_agents: Optional[int] = None
    #: SPEC SS12. `None` means "not configured", and the consumer
    #: (`runner.main`) applies FALSE as the production default -- never
    #: conflated, because "the operator has not decided" and "the operator
    #: turned it off" are different facts even where they act the same.
    autonomous_when_serverless: Optional[bool] = None
    #: Server-election rank and eligibility (`keel.election`). Parsed here
    #: so one file describes the host; this loader neither elects nor
    #: validates against the fleet.
    rank: Optional[int] = None
    server_eligible: Optional[bool] = None


class RunnerTomlError(ValueError):
    """A `runner.toml` (or an env override) this loader cannot make sense
    of -- malformed TOML, a `[limits]` value that is not an integer, a
    table where a table is expected. Never raised for a MISSING file or
    a missing key; only for a PRESENT one that is wrong."""


def _require_tomllib():
    try:
        import tomllib
    except ImportError as exc:  # pragma: no cover -- exercised only on <3.11
        raise RunnerTomlError(
            "runner.toml parsing needs Python 3.11+ (the stdlib `tomllib` "
            f"module): {exc}. This host's `python3` is too old for "
            "keel-runner's config file; doctor.py should gain a check for "
            "this before any such host is registered."
        ) from exc
    return tomllib


def _table(data: Dict[str, Any], name: str, path: Path) -> Dict[str, Any]:
    value = data.get(name, {})
    if value is None:
        return {}
    if not isinstance(value, dict):
        raise RunnerTomlError(f"{path}: [{name}] must be a table, got {type(value).__name__}")
    return value


def _optional_str(value: Any, *, where: str) -> Optional[str]:
    if value is None:
        return None
    if not isinstance(value, str):
        raise RunnerTomlError(f"{where} must be a string, got {type(value).__name__}: {value!r}")
    value = value.strip()
    return value or None


def _optional_path_str(value: Any, *, where: str) -> Optional[str]:
    """Like `_optional_str`, plus `~` expansion -- paths in a config file
    a human hand-edits are exactly where `~/...` is the natural thing to
    write, and every other path-shaped config in this tree (`--token-file`
    on the CLI, `FLEET_GIT_TOKEN_FILE`) expands it via `Path.expanduser()`
    at the point of use; expanding here means every consumer of a
    `RunnerConfig` gets an already-usable string instead of reimplementing
    the expansion."""
    s = _optional_str(value, where=where)
    if s is None:
        return None
    return str(Path(s).expanduser())


def _optional_int(value: Any, *, where: str) -> Optional[int]:
    if value is None:
        return None
    if isinstance(value, bool) or not isinstance(value, int):
        raise RunnerTomlError(f"{where} must be an integer, got {type(value).__name__}: {value!r}")
    if value < 0:
        raise RunnerTomlError(f"{where} must not be negative, got {value}")
    return value


def _optional_bool(value: Any, *, where: str) -> Optional[bool]:
    """A TOML `true`/`false`, and nothing else.

    `isinstance(value, bool)` is checked FIRST and exclusively: TOML has a
    real boolean type, so an operator who wrote `when_serverless = 1`
    meant something this loader should refuse rather than silently read as
    true. (Note the mirror of `_optional_int`'s `isinstance(value, bool)`
    guard, which exists because `bool` is a subclass of `int` in Python
    and `max_gates = true` would otherwise parse as 1.)
    """
    if value is None:
        return None
    if not isinstance(value, bool):
        raise RunnerTomlError(
            f"{where} must be a boolean (true/false), got {type(value).__name__}: {value!r}")
    return value


def _env_bool(var: str, raw: str) -> bool:
    text = raw.strip().lower()
    if text in _TRUE:
        return True
    if text in _FALSE:
        return False
    raise RunnerTomlError(
        f"{var}={raw!r} is not a boolean; use one of "
        f"{sorted(_TRUE)} or {sorted(_FALSE)}")


def _from_toml(data: Dict[str, Any], path: Path) -> RunnerConfig:
    hub = _table(data, "hub", path)
    code = _table(data, "code", path)
    server = _table(data, "server", path)
    token = _table(data, "token", path)
    limits = _table(data, "limits", path)
    autonomy = _table(data, "autonomy", path)
    return RunnerConfig(
        hub_url=_optional_str(hub.get("url"), where=f"{path}: [hub].url"),
        code_url=_optional_str(code.get("url"), where=f"{path}: [code].url"),
        server_url=_optional_str(server.get("url"), where=f"{path}: [server].url"),
        git_token_file=_optional_path_str(token.get("git_file"), where=f"{path}: [token].git_file"),
        server_token_file=_optional_path_str(token.get("server_file"), where=f"{path}: [token].server_file"),
        max_gates=_optional_int(limits.get("max_gates"), where=f"{path}: [limits].max_gates"),
        max_agents=_optional_int(limits.get("max_agents"), where=f"{path}: [limits].max_agents"),
        autonomous_when_serverless=_optional_bool(
            autonomy.get("when_serverless"), where=f"{path}: [autonomy].when_serverless"),
        rank=_optional_int(server.get("rank"), where=f"{path}: [server].rank"),
        server_eligible=_optional_bool(
            server.get("eligible"), where=f"{path}: [server].eligible"),
    )


def load(path: "str | Path | None" = None) -> RunnerConfig:
    """The file's own values, with NO environment overrides applied --
    a pure parse. `path` defaults to `DEFAULT_PATH`
    (`~/.keel/runner.toml`); a file that does not exist parses as
    `RunnerConfig()` (every field `None`), exactly like every OTHER
    config default in this tree treats an absent file (`config.py`'s
    `default_git_token_file`, `train.load_domains`) -- never an error, a
    host simply has nothing configured here yet.

    Raises `RunnerTomlError` for a file that EXISTS but is malformed
    (bad TOML syntax, a table where a scalar belongs, a `[limits]` value
    that is not a non-negative integer) -- a present-but-wrong file is
    a configuration mistake worth surfacing loudly, not silently
    treating as absent.
    """
    p = Path(path) if path is not None else DEFAULT_PATH
    if not p.is_file():
        return RunnerConfig()
    tomllib = _require_tomllib()
    try:
        with p.open("rb") as f:
            data = tomllib.load(f)
    except tomllib.TOMLDecodeError as exc:
        raise RunnerTomlError(f"{p}: invalid TOML: {exc}") from exc
    if not isinstance(data, dict):  # pragma: no cover -- tomllib always returns a dict at top level
        raise RunnerTomlError(f"{p}: top level must be a table")
    return _from_toml(data, p)


def resolve(path: "str | Path | None" = None, env: Optional[Dict[str, str]] = None) -> RunnerConfig:
    """`load(path)`, then every `ENV_VARS` entry whose variable is SET
    (present and non-empty in `env`) overwrites the file's value for that
    field -- "env overrides", the exact contract this loader exists to
    provide (PLAN Stage 3 task 7). An unset or empty-string env var
    changes nothing, so `KEEL_MAX_GATES=` in a unit's environment block
    (a template that always sets the variable, sometimes to nothing)
    behaves as "not overridden", never as "override to empty".

    `env` defaults to `os.environ`; every test in this suite passes an
    explicit dict instead, so this loader is exercised without mutating
    (or depending on) the real process environment.
    """
    src = os.environ if env is None else env
    base = load(path)
    values = asdict(base)
    for field_name, var in ENV_VARS.items():
        raw = src.get(var)
        if raw is None or raw == "":
            continue
        if field_name in _BOOL_FIELDS:
            values[field_name] = _env_bool(var, raw)
        elif field_name in _INT_FIELDS:
            try:
                parsed = int(raw)
            except ValueError as exc:
                raise RunnerTomlError(f"{var}={raw!r} is not an integer") from exc
            if parsed < 0:
                raise RunnerTomlError(f"{var}={raw!r} must not be negative")
            values[field_name] = parsed
        elif field_name in ("git_token_file", "server_token_file"):
            values[field_name] = str(Path(raw).expanduser())
        else:
            values[field_name] = raw
    return RunnerConfig(**values)


def field_names() -> "tuple[str, ...]":
    """Every `RunnerConfig` field name, for a caller that wants to
    iterate without importing `dataclasses` itself (e.g. a `--json` CLI
    dump)."""
    return tuple(f.name for f in fields(RunnerConfig))


def _main(argv=None) -> int:
    """`python3 -m keel.runner_toml [path]` -- print the resolved config
    (file + real environment) as JSON, for a human debugging a host's
    setup. Not a supported machine interface; `runner.py` calls
    `resolve()` directly rather than shelling out to this."""
    import json
    import sys

    argv = sys.argv[1:] if argv is None else argv
    path = argv[0] if argv else None
    try:
        cfg = resolve(path)
    except RunnerTomlError as exc:
        print(f"runner_toml: {exc}", file=sys.stderr)
        return 1
    print(json.dumps(asdict(cfg), indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(_main())
