#!/usr/bin/env python3
"""``python3 tools/fleet/doctor.py <host>`` -- is this machine fit to gate?

T0.1 (see ``docs/FLEET.md``, mechanism M7) exists because the M4 (``oldair``)
produced FAIL on every branch, including the known-good tip, for a reason that
had nothing to do with the branches: its gate-resolved ``rustc`` was 1.90.0
while the rest of the fleet ran 1.97.1. A verdict whose outcome depends on
*which host ran it* is not a verdict. ``rust-toolchain.toml`` (repo root) is
the fix that keeps this from recurring; this script is the fix that catches it
being *out of effect* on some host before that host ever produces a verdict.

This is a standalone health check, not a claim or a gate run. It asserts:

  1. toolchain id     -- sha256 of a normalized ``rustc -vV``, matched against
                          the fleet's canonical id (see CANONICAL_TOOLCHAIN_ID).
  2. linker version    -- recorded only, never asserted. Two machines can share
                          an identical toolchain id and still disagree here
                          (``oldair`` ld-27037 vs the working ``m5``'s
                          ld-27036.1) -- that is exactly the variable T0.1 left
                          open, not one it closed.
  3. oracle fitness    -- ``-ver`` reports the pinned release *and* the
                          ``OOXML.docx`` -> ``DOCX`` capability probe passes.
                          ``-ver`` alone is not a working oracle: the pinned
                          tree's ``#!/usr/bin/env perl`` can resolve a Homebrew
                          perl with no ``Archive::Zip``, which reports
                          ``FileType: ZIP`` for a .docx while still printing
                          the right ``-ver`` -- every container format degrades
                          at once, silently. See AGENTS.md "A matching -ver is
                          not a working oracle."
  4. corpus count      -- the combined-samples corpus is the expected size.
                          A short corpus is not a "smaller" run, it is a
                          *different* run that happens to look the same shape.
  5. free disk         -- below the fleet's floor (``limits.min_free_gb`` in
                          ``refs/fleet/desired``, currently 14) a host should
                          not be handed more work.
  6. clock offset       -- PLAN Stage 3 task 7: leases (``claim.py``) are
                          absolute timestamps (``expires_at``), so a host
                          whose clock is off refuses instead of silently
                          expiring a healthy lease early or over-extending a
                          lost one. Measured against a real NTP server (a
                          minimal stdlib SNTP client, RFC 4330 -- no
                          dependency on the OS's own NTP daemon actually
                          running); REFUSES (FAILs) at more than
                          ``MAX_CLOCK_SKEW_S`` (5 s) off, per the same
                          never-silently-degrade rule as every other check
                          here.

Every number here names its instrument, per AGENTS.md and
``scripts/instrument.py`` -- this script prints that header before its first
check, and every check line states what was measured and how.

``--json`` emits one JSON object on stdout instead of the human report --
the registration payload PLAN Stage 3 task 7 names (``platform_id``,
``rustc_id``, ``cores``, ``free_disk_gb``, ``free_mem_gb``, ``has_oracle``,
``gate_version``, ``keel_version``, ``ntp_offset_s``), plus the full
``checks`` list so a caller that only wants the payload still sees why a
check failed. Nothing else is printed to stdout in ``--json`` mode -- no
instrument header, no per-check text lines -- so a runner can parse stdout
directly without stripping human-readable preamble first.

Exit code is the number of failed checks (0 == healthy), in both modes.
Never silently degrades a check into a warning: a check this script cannot
perform (e.g. the oracle script is simply absent, as it was found to be on
``ubuntuwork`` (removed 2026-08-22) during
T0.1) is a FAIL, not a skip, because a host silently missing its own
capability probe is the exact failure mode this script exists to catch.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import platform
import shutil
import socket
import stat
import struct
import subprocess  # nosec B404 -- list-argv only, no shell=True anywhere below
import sys
import time
from pathlib import Path
from typing import Callable, List, Optional

REPO_ROOT = Path(__file__).resolve().parents[2]
FLEET_DIR = Path(__file__).resolve().parent
sys.path.insert(0, str(REPO_ROOT))
sys.path.insert(0, str(FLEET_DIR))

from scripts import instrument  # noqa: E402  (path must be set up first)
import claim  # noqa: E402  (sibling module; the ONE rustc_id/platform_id implementation --
               # fleetd's own registration/heartbeat payload calls the same two functions)
import config  # noqa: E402  (sibling module; R6 -- the one EXIFTOOL_CACHE_DIR default)
import fleetlib  # noqa: E402  (sibling module; R2 -- the one token-file resolver)
from keel.election import KEEL_VERSION  # noqa: E402  (the server/runner protocol version)

# ---------------------------------------------------------------------------
# Canonical constants
# ---------------------------------------------------------------------------

# sha256 of `rustc -vV` with the `host:` line removed, i.e. everything that
# identifies *which compiler* (release, commit-hash, commit-date, LLVM
# version) but not *which target it runs on*. The `host:` line is stripped
# deliberately: this fleet is heterogeneous by design (aarch64-apple-darwin on
# the Macs, x86_64-unknown-linux-gnu on the Linux boxes), and a verdict about
# "is this the pinned compiler" must not vary by architecture the way a verdict
# about "is this the pinned target ABI" legitimately would.
#
# Derived 2026-08-14 (measured, not assumed) from `bash -lc 'export
# PATH="$HOME/.cargo/bin:$HOME/.local/bin:$PATH"; rustc -vV'` -- i.e. under the
# exact PATH gate-nocache.sh exports, not a bare login-shell rustc -- on
# `server` (x86_64-unknown-linux-gnu) and cross-checked identical on
# `ubuntuwork` (x86_64-unknown-linux-gnu; removed from the fleet
# 2026-08-22), `oldair` (aarch64-apple-darwin,
# after `rustup update stable` picked up rust-toolchain.toml's 1.97.1 pin) and
# `localhost`/m5 (aarch64-apple-darwin, same). All four produced:
#     rustc 1.97.1 (8bab26f4f 2026-07-14), commit-date 2026-07-14,
#     LLVM version 22.1.6
# Regenerate with:
#     bash -lc 'export PATH="$HOME/.cargo/bin:$HOME/.local/bin:$PATH"; \
#       rustc -vV | grep -v "^host:" | shasum -a 256'
CANONICAL_TOOLCHAIN_ID = (
    "b5d143364ae0334870dfbce0e72e0ea6ecb1bc07d68d023ab6c88b6d20f58577"
)
CANONICAL_CHANNEL = "1.97.1"  # must track rust-toolchain.toml's `channel`

# R6: the literal used to be spelled out here directly; it now lives in
# exactly one place (config.DEFAULT_EXIFTOOL_CACHE_DIR / units/fleet-env.sh)
# and every consumer, this file included, imports/sources that instead.
CACHE_DIR = config.exiftool_cache_dir()
ORACLE_SCRIPT = CACHE_DIR / "exiftool-pinned.sh"
DOCX_SAMPLE = CACHE_DIR / "exiftool" / "t" / "images" / "OOXML.docx"
CORPUS_DIR = CACHE_DIR / "combined-samples"
EXPECTED_ORACLE_VERSION = "13.59"  # .exiftool-version at the repo root is the authority
EXPECTED_CORPUS_COUNT = 4238
MIN_FREE_GB = 14  # matches `limits.min_free_gb` in FLEET_SPEC's example refs/fleet/desired

# --------------------------------------------------------------------- #
# Clock check (PLAN Stage 3 task 7)
# --------------------------------------------------------------------- #
#
# Leases are absolute timestamps (`claim.py`'s `expires_at`): a clock that
# is fast makes a healthy lease look expired to the OWNER before the store
# agrees, and a clock that is slow lets a renewal happen late enough that
# the store's own TTL has already reaped it -- both are "the lease's
# writer and the lease's judge disagree about what time it is", which is
# exactly the class of bug T0.1 (this file's own opening story) fixed for
# `rustc`. `FLEET_NTP_SERVER`/`FLEET_NTP_PORT` are overridable the same
# way every other fleet-wide default here is (env var, single source);
# `pool.ntp.org` is a public NTP pool, not a fleet host, so it is not
# subject to tests/test_no_hardcoded_hosts.py's fence (that fence forbids
# the OLD topology's own hosts, not a third-party time source).
NTP_SERVER = os.environ.get("FLEET_NTP_SERVER", "pool.ntp.org")
NTP_PORT = int(os.environ.get("FLEET_NTP_PORT", "123") or "123")
NTP_TIMEOUT_S = 2.0
MAX_CLOCK_SKEW_S = 5.0
_NTP_EPOCH_DELTA = 2208988800  # seconds between 1900-01-01 and 1970-01-01 (RFC 4330 sec. 3)


class Check:
    def __init__(self, name: str):
        self.name = name
        self.ok: bool | None = None
        self.detail = ""
        self.informational = False
        # The raw numeric measurement behind a check, when it has one
        # (free disk in GB, NTP offset in seconds) -- set by the check
        # function itself. `None` for checks with no single number
        # (toolchain id, oracle, corpus, token file). Exists so a
        # consumer of the check (the `--json` registration payload) reads
        # the exact value the check reasoned about instead of re-deriving
        # it, or worse, parsing it back out of `detail`'s prose.
        self.value: "float | None" = None

    def passed(self, detail: str) -> None:
        self.ok = True
        self.detail = detail

    def failed(self, detail: str) -> None:
        self.ok = False
        self.detail = detail

    def info(self, detail: str) -> None:
        self.ok = None
        self.informational = True
        self.detail = detail

    def line(self) -> str:
        if self.informational:
            tag = "INFO"
        elif self.ok:
            tag = "PASS"
        else:
            tag = "FAIL"
        return f"[{tag}] {self.name}: {self.detail}"


def gate_path_env() -> dict:
    """The exact PATH gate-nocache.sh's first line exports, replicated here.

    A bare `rustc --version` on these hosts is misleading in both directions:
    on the Macs (`oldair`, `localhost`/m5) a normal login shell resolves a
    Homebrew rustc off `/opt/homebrew/bin` *ahead of* `~/.cargo/bin`, so
    `bash -lc 'rustc --version'` alone reports 1.97.1 even when the
    rustup-managed toolchain the gate actually uses (once PATH is overridden
    the way the gate overrides it) is a stale 1.90.0. Measuring with anything
    other than this exact PATH order measures the wrong rustc.
    """
    home = os.environ.get("HOME", str(Path.home()))
    env = dict(os.environ)
    env["PATH"] = f"{home}/.cargo/bin:{home}/.local/bin:{env.get('PATH', '')}"
    return env


def run(argv: list[str], **kwargs) -> subprocess.CompletedProcess:
    return subprocess.run(  # nosec B603 -- list-argv, no shell
        argv, capture_output=True, text=True, errors="replace", **kwargs
    )


def check_toolchain() -> Check:
    c = Check("toolchain id")
    env = gate_path_env()
    try:
        out = run(["rustc", "-vV"], env=env)
    except OSError as exc:
        c.failed(f"could not run rustc under the gate PATH: {exc}")
        return c
    if out.returncode != 0:
        c.failed(f"`rustc -vV` under the gate PATH exited {out.returncode}: {out.stderr.strip()}")
        return c
    raw = out.stdout
    # Trailing newline included deliberately: this must byte-for-byte match
    # `rustc -vV | grep -v "^host:"` (the shell form CANONICAL_TOOLCHAIN_ID was
    # derived with), and `grep` preserves the trailing newline on the last
    # line. Dropping it here silently changes the hash and desyncs from the
    # documented regeneration command -- caught by doctor.py failing against
    # itself on a host that was, in fact, canonical.
    normalized = "\n".join(
        line for line in raw.splitlines() if not line.startswith("host:")
    ) + "\n"
    digest = hashlib.sha256(normalized.encode("utf-8")).hexdigest()
    release_line = next((l for l in raw.splitlines() if l.startswith("release:")), "release: ?")
    if digest == CANONICAL_TOOLCHAIN_ID:
        c.passed(
            f"{release_line.strip()}, sha256(rustc -vV, host-line stripped)={digest[:12]}... "
            f"matches canonical (channel {CANONICAL_CHANNEL})"
        )
    else:
        c.failed(
            f"{release_line.strip()}, sha256(rustc -vV, host-line stripped)={digest} "
            f"!= canonical {CANONICAL_TOOLCHAIN_ID}\n"
            f"  measured via: PATH=\"$HOME/.cargo/bin:$HOME/.local/bin:$PATH\" rustc -vV\n"
            f"  full output:\n" + "\n".join(f"    {l}" for l in raw.splitlines())
        )
    return c


def check_linker() -> Check:
    c = Check("linker version")
    argvs = [["ld", "-v"], ["ld", "--version"]]
    for argv in argvs:
        try:
            out = run(argv)
        except OSError:
            continue
        text = (out.stdout + out.stderr).strip()
        if text:
            first_line = text.splitlines()[0]
            c.info(f"{first_line}  (recorded only, not asserted -- see docstring)")
            return c
    c.info("could not determine linker version (ld not found)")
    return c


def check_oracle() -> Check:
    c = Check("oracle (-ver + OOXML.docx capability)")
    if not ORACLE_SCRIPT.is_file():
        c.failed(f"pinned oracle script missing: {ORACLE_SCRIPT} does not exist")
        return c
    if not os.access(ORACLE_SCRIPT, os.X_OK):
        c.failed(f"pinned oracle script not executable: {ORACLE_SCRIPT}")
        return c
    try:
        ver_out = run([str(ORACLE_SCRIPT), "-ver"])
    except OSError as exc:
        c.failed(f"could not run {ORACLE_SCRIPT}: {exc}")
        return c
    ver = ver_out.stdout.strip()
    if ver != EXPECTED_ORACLE_VERSION:
        c.failed(
            f"`{ORACLE_SCRIPT} -ver` -> {ver!r}, expected {EXPECTED_ORACLE_VERSION!r} "
            f"(stderr: {ver_out.stderr.strip()!r})"
        )
        return c
    if not DOCX_SAMPLE.is_file():
        c.failed(
            f"-ver matched ({ver}) but capability sample missing: {DOCX_SAMPLE}. "
            "A matching -ver alone is not a working oracle -- see AGENTS.md."
        )
        return c
    try:
        docx_out = run([str(ORACLE_SCRIPT), "-s3", "-FileType", str(DOCX_SAMPLE)])
    except OSError as exc:
        c.failed(f"could not run capability probe: {exc}")
        return c
    filetype = docx_out.stdout.strip()
    if filetype != "DOCX":
        c.failed(
            f"-ver reports {ver} but `-s3 -FileType {DOCX_SAMPLE.name}` -> {filetype!r}, "
            "expected 'DOCX'. This is the degraded-interpreter failure mode: the "
            "pinned tree's #!/usr/bin/env perl found a perl with no Archive::Zip, "
            "so every ZIP-container format silently degrades while -ver still lies."
        )
        return c
    c.passed(f"-ver={ver}, OOXML.docx -> DOCX  (via {ORACLE_SCRIPT})")
    return c


def check_corpus() -> Check:
    c = Check("corpus count")
    if not CORPUS_DIR.is_dir():
        c.failed(f"corpus directory missing: {CORPUS_DIR}")
        return c
    count = sum(1 for p in CORPUS_DIR.rglob("*") if p.is_file())
    if count == EXPECTED_CORPUS_COUNT:
        c.passed(f"{count} files under {CORPUS_DIR} (expected {EXPECTED_CORPUS_COUNT})")
    else:
        c.failed(f"{count} files under {CORPUS_DIR}, expected {EXPECTED_CORPUS_COUNT}")
    return c


def check_git_token_file() -> Check:
    """B5 (review finding): `FLEET_HUB_URL` pointed at a private GitHub
    repo over HTTPS needs a credential, or fleetd's very first git op --
    the singleton claim, before any heartbeat -- raises an uncaught
    `HubUnreachableError` (`keel/git-credential-file` answers `get` with
    nothing when `FLEET_GIT_TOKEN_FILE` is unset, which is correct
    per-request behaviour, but leaves the daemon with no way to
    authenticate at all). This is the ONE health check answering "would
    fleetd even get past its first git command", not "did it".

    R2 (review finding): `FLEET_GIT_TOKEN_FILE` being unset does not mean
    no token is available -- `install_secrets.sh` (and every `units/*`
    template) already default the SAME file to
    `config.default_git_token_file()` (`~/.keel/secrets/git-token`) -- the
    path `fleetlib.git_token_file()` resolves for every git command -- so a
    hand-run step that forgot to `export FLEET_GIT_TOKEN_FILE` was failing
    this check even when a perfectly good, correctly-permissioned token
    file already sat at the default path. This check now falls back to
    that default path when the env var is unset, through the very
    resolver `fleetlib.credential_env` uses -- it never invents a NEW
    acceptance path, it just stops requiring the redundant `export`
    `install_secrets.sh` itself doesn't require.

    Deliberately does not read the token (this script's own PATH/`-vV`
    checks above never touch a secret either): existence + mode are
    checkable without opening the file, and `stat` on a 0600 file the
    caller doesn't own to read still succeeds -- reading it is the one
    thing that risks putting the token in this script's own stdout on a
    future `print(...)`-while-debugging mistake, so it is never opened
    here at all.
    """
    c = Check("git token file")
    hub_url = os.environ.get("FLEET_HUB_URL", "")
    if not hub_url.startswith("https://"):
        c.info(
            f"FLEET_HUB_URL={hub_url!r} is not an https:// remote -- no token file required"
        )
        return c

    hint = "run tools/fleet/rollout/install_secrets.sh to create and validate one"
    # The SAME resolver every fleet git spawner uses (`fleetlib.credential_env`
    # -> `fleetlib.git_token_file`): the variable when set, else the default
    # path when it is a readable file. Re-implementing the rule here would
    # let doctor and the daemon disagree about whether a token exists.
    explicit = os.environ.get("FLEET_GIT_TOKEN_FILE", "")
    token_file = fleetlib.git_token_file() or ""
    used_default = bool(token_file) and not explicit
    if not token_file:
        default_path = config.default_git_token_file()
        c.failed(
            f"FLEET_HUB_URL is https but FLEET_GIT_TOKEN_FILE is unset and the "
            f"default {default_path} does not exist (or is not a readable file) -- {hint}"
        )
        return c

    label = f"FLEET_GIT_TOKEN_FILE (default, env var unset)={token_file}" if used_default \
        else f"FLEET_GIT_TOKEN_FILE={token_file}"
    path = Path(token_file)
    try:
        mode = stat.S_IMODE(path.stat().st_mode)
    except OSError as exc:
        c.failed(f"{label} does not exist or is unreadable ({exc}) -- {hint}")
        return c
    if mode != 0o600:
        c.failed(f"{label} has mode {oct(mode)}, expected 0600 -- {hint}")
        return c
    c.passed(f"{label} exists, mode 0600 (contents not read)")
    return c


def check_disk() -> Check:
    c = Check("free disk")
    home = Path(os.environ.get("HOME", str(Path.home())))
    try:
        usage = shutil.disk_usage(home)
    except OSError as exc:
        c.failed(f"could not stat {home}: {exc}")
        return c
    free_gb = usage.free / (1024**3)
    c.value = free_gb
    if free_gb >= MIN_FREE_GB:
        c.passed(f"{free_gb:.1f} GB free on {home} (floor {MIN_FREE_GB} GB)")
    else:
        c.failed(f"{free_gb:.1f} GB free on {home}, below floor {MIN_FREE_GB} GB")
    return c


def free_mem_gb() -> float:
    """Best-effort available memory in GB (decimal, ``/1e9``); ``-1.0`` if
    unknowable (e.g. macOS with no ``vm_stat``, or an unsupported OS). A
    probe that cannot answer says so rather than guessing.

    Mirrors ``fleetd.free_mem_gb``'s algorithm and units byte-for-byte
    (Linux: ``/proc/meminfo``'s ``MemAvailable``; macOS: ``vm_stat``'s
    free+inactive pages) so a number this script reports and a number a
    runner's heartbeat reports for the same host are the same number.
    Duplicated here rather than imported: `fleetd.py` is mid-split into
    `keel/runner.py` in a parallel PLAN Stage 3 task, and importing a
    2,000-line daemon module (with its own argv/signal setup) into a
    health-check script is the wrong direction of dependency regardless.
    Keep the two in sync by hand until the split lands and one of them
    can import the other's home.
    """
    try:
        if sys.platform == "linux":
            with open("/proc/meminfo") as f:
                for line in f:
                    if line.startswith("MemAvailable:"):
                        return int(line.split()[1]) / 1e6
        elif sys.platform == "darwin":
            out = run(["vm_stat"], timeout=10).stdout
            page_size = 16384
            free_pages = 0
            for line in out.splitlines():
                if "page size of" in line:
                    page_size = int(
                        "".join(ch for ch in line.split("page size of")[1] if ch.isdigit())
                    )
                for key in ("Pages free:", "Pages inactive:"):
                    if line.startswith(key):
                        free_pages += int(line.split(":")[1].strip().rstrip("."))
            return free_pages * page_size / 1e9
    except (OSError, ValueError, subprocess.TimeoutExpired):
        pass
    return -1.0


def _gate_version() -> "str | None":
    """The pinned gate version this host would run
    (`tools/fleet/gate_version.txt`), or None if it cannot be read --
    same file `fleetd._gate_version` reads, resolved relative to this
    script rather than a passed-in repo root since `doctor.py` always
    knows exactly where it lives."""
    try:
        return (FLEET_DIR / "gate_version.txt").read_text().strip()
    except OSError:
        return None


def _ntp_timestamp_to_unix(raw: bytes) -> float:
    """An 8-byte NTP timestamp (32-bit seconds since 1900 + 32-bit
    fraction, RFC 4330 sec. 4) as a Unix `time.time()`-style float."""
    seconds, frac = struct.unpack("!II", raw)
    return (seconds - _NTP_EPOCH_DELTA) + frac / 2**32


def query_ntp_offset(
    server: "str | None" = None,
    port: "int | None" = None,
    timeout: float = NTP_TIMEOUT_S,
) -> float:
    """Local-clock-minus-true-time offset in seconds, via a minimal SNTP
    (RFC 4330) client -- stdlib `socket`/`struct` only, no dependency on
    the OS's own NTP daemon actually running (a freshly imaged or
    container host is exactly the case this check exists to catch, and
    such a host's NTP daemon is the least reliable thing about it).

    Uses the classic four-timestamp offset estimate: client-send (T1),
    server-receive (T2), server-transmit (T3), client-receive (T4);
    offset = ((T2 - T1) + (T3 - T4)) / 2. One UDP round trip, `timeout`
    seconds.

    Raises `OSError` (DNS failure, connection refused, timeout, a short
    or malformed reply -- `socket.gaierror` and `socket.timeout` are both
    `OSError` subclasses) rather than ever guessing an offset: an
    unmeasurable clock must FAIL the check, per this module's rule that a
    check it cannot perform is a FAIL, never a silent skip.
    """
    server = server or NTP_SERVER
    port = NTP_PORT if port is None else port
    # LI=0 (no warning), VN=4, Mode=3 (client); every other field
    # (including the optional Transmit Timestamp) left zero -- servers
    # answer an all-zero request exactly the same as one with a properly
    # encoded originate timestamp, and skipping it keeps this function to
    # one obvious literal instead of an encode/decode round trip.
    packet = b"\x23" + b"\x00" * 47
    with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as sock:
        sock.settimeout(timeout)
        t1 = time.time()
        sock.sendto(packet, (server, port))
        data, _ = sock.recvfrom(48)
        t4 = time.time()
    if len(data) < 48:
        raise OSError(f"short NTP reply from {server}:{port}: {len(data)} bytes, expected >= 48")
    t2 = _ntp_timestamp_to_unix(data[32:40])  # Receive Timestamp
    t3 = _ntp_timestamp_to_unix(data[40:48])  # Transmit Timestamp
    return ((t2 - t1) + (t3 - t4)) / 2.0


def check_ntp_offset(query_fn: "Callable[[], float] | None" = None) -> Check:
    """PLAN Stage 3 task 7: leases are absolute timestamps
    (`claim.py`'s `expires_at`), so a host whose clock disagrees with
    real time by more than `MAX_CLOCK_SKEW_S` silently expires a healthy
    lease early or over-extends a lost one -- refuse before that host is
    ever handed a claim.

    `query_fn` defaults to `query_ntp_offset` (a real network query);
    tests inject a stub so the suite never depends on network access or
    on wall-clock reality. `c.value` carries the raw offset (or `None` on
    an unmeasurable clock) for `registration_payload`'s `ntp_offset_s`.
    """
    c = Check("clock (NTP offset)")
    fn = query_fn if query_fn is not None else query_ntp_offset
    try:
        offset = fn()
    except OSError as exc:
        c.failed(
            f"could not measure clock offset against {NTP_SERVER}:{NTP_PORT}: {exc} -- "
            "an unmeasurable clock is refused, not assumed sane (leases are absolute "
            "timestamps)"
        )
        return c
    c.value = offset
    if abs(offset) > MAX_CLOCK_SKEW_S:
        c.failed(
            f"clock offset {offset:+.3f}s from {NTP_SERVER} exceeds the {MAX_CLOCK_SKEW_S}s "
            "floor -- leases are absolute timestamps; a skewed clock silently expires or "
            "over-extends them"
        )
    else:
        c.passed(f"clock offset {offset:+.3f}s from {NTP_SERVER} (within {MAX_CLOCK_SKEW_S}s)")
    return c


def registration_payload(host: str, checks: "List[Check]") -> dict:
    """The `--json` payload: PLAN Stage 3 task 7's exact field list
    (`platform_id`, `rustc_id`, `cores`, free disk/mem, `has_oracle`,
    `gate_version`, `keel_version`) plus `ntp_offset_s` and the full
    `checks` list, so a runner's `register` call has both the summary
    numbers and the reasoning behind each one in a single stdout parse.

    `platform_id`/`rustc_id` come from `claim.compute_platform_id`/
    `compute_rustc_id` -- the SAME two functions `fleetd.py`'s own
    registration/heartbeat payload calls (`fleetd.py` L2187-2188) -- not
    `verdict.compute_ids`, which is the identical algorithm under a
    different name for the verdict-cache side of the fleet; using
    claim.py's copy here keeps doctor.py's number and a runner's number
    for the same host computed by literally the same code.
    """
    by_name = {c.name: c for c in checks}
    oracle_check = by_name.get("oracle (-ver + OOXML.docx capability)")
    disk_check = by_name.get("free disk")
    ntp_check = by_name.get("clock (NTP offset)")

    mem_gb = free_mem_gb()

    payload = {
        "host": host,
        "platform_id": claim.compute_platform_id(),
        "rustc_id": claim.compute_rustc_id(),
        "cores": os.cpu_count(),
        "free_disk_gb": (
            round(disk_check.value, 1) if disk_check is not None and disk_check.value is not None else None
        ),
        "free_mem_gb": round(mem_gb, 1) if mem_gb != -1.0 else None,
        "has_oracle": bool(oracle_check is not None and oracle_check.ok is True),
        "gate_version": _gate_version(),
        "keel_version": KEEL_VERSION,
        "ntp_offset_s": (
            round(ntp_check.value, 3) if ntp_check is not None and ntp_check.value is not None else None
        ),
        "checks": [
            {"name": c.name, "ok": c.ok, "detail": c.detail} for c in checks
        ],
    }
    payload["failed"] = sum(1 for c in checks if c.ok is False)
    payload["ok"] = payload["failed"] == 0
    return payload


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Assert this host is fit to gate: toolchain id, oracle fitness, "
        "corpus size, free disk, clock offset. Exit code is the number of failed checks."
    )
    parser.add_argument(
        "host",
        help="Label for this host in the report (e.g. server, oldair, "
        "localhost). Not verified against the OS hostname -- ssh aliases and actual "
        "hostnames routinely disagree; recorded for the record, not asserted.",
    )
    parser.add_argument(
        "--json",
        action="store_true",
        help="emit the registration payload (platform_id, rustc_id, cores, free disk/mem, "
        "has_oracle, gate_version, keel_version, ntp_offset_s, checks) as one JSON object on "
        "stdout, instead of the human report. Nothing else goes to stdout in this mode. Exit "
        "code is still the number of failed checks.",
    )
    args = parser.parse_args()

    # Computed once, before branching on --json: both output modes must
    # see the identical set of checks (same subprocess calls, same
    # network query), never two independently-run passes that could
    # disagree with each other about the same host at the same moment.
    checks = [
        check_toolchain(),
        check_linker(),
        check_oracle(),
        check_corpus(),
        check_disk(),
        check_ntp_offset(),
        check_git_token_file(),
    ]

    if args.json:
        payload = registration_payload(args.host, checks)
        print(json.dumps(payload, sort_keys=True))
        return payload["failed"]

    git = instrument.git_state(REPO_ROOT)
    corpus_count = None
    if CORPUS_DIR.is_dir():
        corpus_count = sum(1 for p in CORPUS_DIR.rglob("*") if p.is_file())
    instrument.print_header(
        tool="fleet-doctor",
        git=git,
        corpus_paths=[CORPUS_DIR] if CORPUS_DIR.is_dir() else None,
        file_count=corpus_count,
        extra=[
            f"host:    {args.host} (platform.node()={platform.node()!r}, "
            f"{platform.system()} {platform.machine()})",
        ],
    )

    for c in checks:
        print(c.line())

    failed = [c for c in checks if c.ok is False]
    print()
    if failed:
        print(f"DOCTOR: FAIL ({len(failed)}/{len(checks)} checks failed) on {args.host}")
    else:
        print(f"DOCTOR: PASS (all checks) on {args.host}")
    return len(failed)


if __name__ == "__main__":
    sys.exit(main())
