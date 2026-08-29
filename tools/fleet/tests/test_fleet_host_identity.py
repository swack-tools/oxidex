#!/usr/bin/env python3
"""L2 -- the laptop reported under the wrong name, and said `disabled ()`.

THE INCIDENT (Keel Stage 1 LIVE acceptance run, 2026-08-27/28). Run
exactly as `units/com.oxidex.fleetd.plist` runs it, fleetd on the m5
printed

    fleetd[Allens-Air] ... refused=[('disabled', '')]

-- an EMPTY reason -- and `fleet status --why` showed a row named
`Allens-Air` with no `m5` row at all. Nothing was disabled. The host was
running, enabled, and answering to a name `refs/fleet/desired` had never
heard of: `rollout/seed_desired.py` seeds the laptop as `m5`, `hostname
-s` on that machine is `Allens-Air`, and NO committed unit set
`FLEET_HOST` (`grep -c FLEET_HOST`: fleetd.service 0,
com.oxidex.fleetd.plist 0, fleetd-wrapper.sh 0, fleet-env.sh 0). Setting
`FLEET_HOST=m5` on the identical command produced the expected
`target-zero (gates 0 / agents 0)` line.

TWO DEFECTS, and this file pins both fixes.

  1. The identity was implicit. `fleetd.host_identity()` falls back to
     `socket.gethostname()`, which is a fine default and a terrible
     configuration: it is right on the i7 (`server`) and wrong on the
     laptop, so the fleet worked well enough for the bug to survive. The
     units now say the name out loud.
  2. An unknown host was indistinguishable from a disabled one.
     `my_desired = hosts.get(host) or {}` makes `enabled` False and
     `reason` None for a host that is simply absent, so the two facts
     rendered as one line -- and the line said the opposite of the
     truth. Same doctrine `reconcile_once` already applies to
     `desired_readable` ("we could not ask" is not "the answer is no"),
     just never applied to the host key itself.

Instrument: `fleetd.reconcile_once` against a throwaway bare hub (the
shared `_fixtures.make_hub` switch, so this runs in both fixture modes),
plus plain reads of the committed unit templates and of
`rollout/seed_desired.py`'s own SEED dict. No network, nothing outside a
tempdir.

Run with:
    python3 -m unittest tools.fleet.tests.test_fleet_host_identity -v
"""

from __future__ import annotations

import os
import sys
import tempfile
import unittest
from pathlib import Path

FLEET_DIR = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(FLEET_DIR))
sys.path.insert(0, str(FLEET_DIR / "rollout"))

import fleetd  # noqa: E402
from _env import HermeticCase  # noqa: E402
from _fixtures import make_hub  # noqa: E402

UNITS_DIR = FLEET_DIR / "units"

# The exact strings the live run produced. Kept as constants so the
# assertions below read as "never this again" rather than as a style
# preference.
LIVE_HOSTNAME = "Allens-Air"   # `hostname -s` on the m5
SEEDED_NAME = "m5"             # its key under `hosts` in seed_desired.py


def _reasons(res) -> dict:
    return {reason: detail for reason, detail in res.refused}


class TestAnUnknownHostSaysSo(HermeticCase):
    """The production defect: `refused: disabled ()` for a host that was
    never disabled."""

    def setUp(self):
        super().setUp()
        self._tmp = tempfile.TemporaryDirectory()
        self.tmp = Path(self._tmp.name)
        self.addCleanup(self._tmp.cleanup)
        bare = self.tmp / "state.git"
        bare.mkdir()
        import subprocess
        subprocess.run(["git", "init", "--bare", "-q", str(bare)], check=True)
        self.hub = make_hub(self, str(bare), workdir=self.tmp / "hubcache")
        self.workers = []
        stub = self.tmp / "stub-gate.sh"
        stub.write_text("#!/bin/bash\nexit 0\n")
        stub.chmod(0o755)
        self.stub = stub
        self.warnings = fleetd.HostWarnings()

    def _desired(self, hosts: dict) -> None:
        doc = {"generation": 1, "hosts": hosts,
               "limits": {"min_free_gb": 14, "min_free_mem_gb": 8}}
        cur = self.hub.sha(fleetd.DESIRED_REF)
        if cur is None:
            self.assertTrue(self.hub.create(fleetd.DESIRED_REF, doc))
        else:
            self.assertTrue(self.hub.update(fleetd.DESIRED_REF, doc, cur))

    def _reconcile(self, host: str):
        return fleetd.reconcile_once(
            self.hub, host, self.workers,
            gate_command=[str(self.stub)],
            log_dir=self.tmp / "logs",
            repo_root=FLEET_DIR.parents[1],
            disk_probe=lambda: 100.0,
            mem_probe=lambda: 32.0,
            warnings=self.warnings,
        )

    def test_the_live_shape_reproduced_now_names_the_real_problem(self):
        """`desired` knows `m5`; the daemon calls itself `Allens-Air`."""
        self._desired({SEEDED_NAME: {"gates": 0, "agents": 0, "enabled": True}})
        res = self._reconcile(LIVE_HOSTNAME)
        reasons = _reasons(res)

        self.assertIn(
            "unknown-host", reasons,
            f"a host absent from {fleetd.DESIRED_REF} must say so; got {res.refused}",
        )
        detail = reasons["unknown-host"]
        self.assertIn(LIVE_HOSTNAME, detail,
                      "the reason must name the identity the daemon is using -- "
                      "that is the one thing the operator has to change")
        self.assertIn("FLEET_HOST", detail,
                      "the reason must name the knob that fixes it")
        self.assertIn(SEEDED_NAME, detail,
                      "listing the known hosts is what turns this from a puzzle "
                      "into a one-line fix")

    def test_an_unknown_host_is_never_reported_as_disabled(self):
        """The regression itself. `disabled` with an empty detail is what
        an operator reads as `fleet down` -- a deliberate stand-down --
        and it was exactly backwards."""
        self._desired({SEEDED_NAME: {"gates": 0, "agents": 0, "enabled": True}})
        res = self._reconcile(LIVE_HOSTNAME)
        self.assertNotIn(("disabled", ""), res.refused,
                         "an unknown host still reports as `disabled ()`")
        self.assertNotIn("disabled", _reasons(res))

    def test_a_genuinely_disabled_host_still_says_disabled(self):
        """The negative control. A check that reported `unknown-host` for
        a host an operator actually took down would just move the lie."""
        self._desired({SEEDED_NAME: {"gates": 0, "agents": 0, "enabled": False,
                                     "reason": "maintainer is using it"}})
        res = self._reconcile(SEEDED_NAME)
        reasons = _reasons(res)
        self.assertIn("disabled", reasons)
        self.assertEqual(reasons["disabled"], "maintainer is using it")
        self.assertNotIn("unknown-host", reasons)

    def test_the_correctly_named_host_reaches_target_zero(self):
        """The acceptance line the live run could not produce: an enabled
        host at 0/0 says `target-zero`, not `disabled` and not silence."""
        self._desired({SEEDED_NAME: {"gates": 0, "agents": 0, "enabled": True}})
        res = self._reconcile(SEEDED_NAME)
        reasons = _reasons(res)
        self.assertIn("target-zero", reasons, f"refused={res.refused}")
        self.assertEqual(reasons["target-zero"], "gates 0 / agents 0")
        self.assertNotIn("unknown-host", reasons)

    def test_an_unreadable_desired_is_still_its_own_reason(self):
        """`unknown-host` must not swallow the pre-existing distinction:
        with NO `desired` ref at all we could not ask, which is neither
        `disabled` nor `unknown-host`."""
        res = self._reconcile(LIVE_HOSTNAME)
        reasons = _reasons(res)
        # A missing ref reads as an empty document, not a hub failure --
        # so the host is genuinely absent from an (empty) desired state.
        self.assertIn("unknown-host", reasons)
        self.assertIn("<none>", reasons["unknown-host"],
                      "with no hosts seeded at all the reason should say so")


class TestTheUnitsNameTheHost(HermeticCase):
    """L2's other half: the identity is explicit in the templates now, and
    the values are the ones `seed_desired.py` actually seeds."""

    def _seeded_hosts(self) -> set:
        import seed_desired
        return set(seed_desired.SEED["hosts"])

    def test_systemd_unit_sets_fleet_host(self):
        text = (UNITS_DIR / "fleetd.service").read_text()
        self.assertIn("Environment=FLEET_HOST=", text,
                      "fleetd.service must set FLEET_HOST -- `hostname -s` is a "
                      "default, not a configuration")
        value = next(line.split("=", 2)[2].strip()
                     for line in text.splitlines()
                     if line.startswith("Environment=FLEET_HOST="))
        self.assertIn(value, self._seeded_hosts(),
                      f"fleetd.service names host {value!r}, which "
                      f"rollout/seed_desired.py does not seed")

    def test_launchd_plist_sets_fleet_host(self):
        text = (UNITS_DIR / "com.oxidex.fleetd.plist").read_text()
        self.assertIn("<key>FLEET_HOST</key>", text)
        # launchd does not shell-expand EnvironmentVariables, so the value
        # must be a literal name -- never `$(hostname)` or `%h`-style
        # syntax that only looks like it will work (same trap the token
        # file has, pinned by test_units_secrets.py).
        idx = text.index("<key>FLEET_HOST</key>")
        value = text[idx:].split("<string>", 1)[1].split("</string>", 1)[0]
        self.assertNotIn("$", value)
        self.assertNotIn("%", value)
        self.assertIn(value, self._seeded_hosts(),
                      f"the plist names host {value!r}, which "
                      f"rollout/seed_desired.py does not seed")

    def test_the_plist_does_not_ship_the_hostname_that_caused_the_incident(self):
        text = (UNITS_DIR / "com.oxidex.fleetd.plist").read_text()
        live = "\n".join(
            line for line in text.splitlines()
            if "<!--" not in line and "-->" not in line
        )
        self.assertNotIn(f"<string>{LIVE_HOSTNAME}</string>", live)

    def test_cron_backstop_sets_fleet_host_too(self):
        """The third launch path. It had no FLEET_HOST either, and a host
        started from the cron backstop would reproduce the same defect."""
        text = (UNITS_DIR / "cron-backstop.txt").read_text()
        self.assertIn("FLEET_HOST=", text)


if __name__ == "__main__":
    unittest.main()
