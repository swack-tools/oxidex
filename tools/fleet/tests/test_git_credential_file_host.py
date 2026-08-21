#!/usr/bin/env python3
"""Tests for `tools/fleet/keel/git-credential-file`'s `host=` handling (M1).

`test_fleetlib.TestCredentialHelper` already proves the file/protocol
contract end-to-end via `git credential fill`; every one of its fixtures
happens to use `host=github.com`, which is also this helper's default, so
that suite could not have caught a helper that ignores `host=` entirely
and answers for ANY remote asking over https. That is exactly the M1
review finding: a scoped runner PAT handed out regardless of which host
asked would leak the state-repo (or code-repo) token to whatever other
https remote a git subprocess happens to touch first.

This file is standalone (no `Hub`, no repo, no network) because the
behaviour under test is entirely in the shell script's own parsing of the
`key=value` request on stdin -- exercised the same way
`TestCredentialHelper._helper` does, by invoking the script directly.

Run with:
    python3 -m unittest discover -s tools/fleet/tests -v
"""

from __future__ import annotations

import os
import subprocess
import tempfile
import unittest
from pathlib import Path

HELPER = Path(__file__).resolve().parents[1] / "keel" / "git-credential-file"
TOKEN = "ghp_TESTTOKEN_not_a_real_credential_0123456789"


class TestGitCredentialFileHost(unittest.TestCase):
    def setUp(self):
        self._tmp = tempfile.mkdtemp(prefix="cred-host-")
        self.addCleanup(self._rmtree)
        self.token_file = str(Path(self._tmp) / "token")
        Path(self.token_file).write_text(TOKEN + "\n")
        os.chmod(self.token_file, 0o600)

    def _rmtree(self):
        import shutil

        shutil.rmtree(self._tmp, ignore_errors=True)

    def _run(self, request: str, extra_env: dict | None = None):
        env = {**os.environ, "FLEET_GIT_TOKEN_FILE": self.token_file, **(extra_env or {})}
        return subprocess.run(
            [str(HELPER), "get"],
            input=request.encode(),
            capture_output=True,
            env=env,
        )

    # -- default host is github.com --------------------------------

    def test_default_host_github_com_is_answered(self):
        r = self._run("protocol=https\nhost=github.com\n\n")
        self.assertEqual(r.returncode, 0, msg=r.stderr)
        self.assertIn(f"password={TOKEN}".encode(), r.stdout)

    def test_a_different_host_is_refused_by_default(self):
        """The core M1 property: without any override, a request for some
        other https remote must NOT get this token."""
        r = self._run("protocol=https\nhost=gitlab.example.com\n\n")
        self.assertEqual(r.returncode, 0, msg=r.stderr)
        self.assertEqual(r.stdout, b"", "token leaked to an unconfigured host")

    def test_refusal_is_silent_like_every_other_refusal_here(self):
        """A host mismatch is not a misconfiguration to shout about (the
        same remote-config might legitimately touch several hosts in one
        session) -- it is answered exactly like a protocol mismatch:
        exit 0, nothing on stdout, so git moves on to its next candidate
        credential rather than dying on a shell error.
        """
        r = self._run("protocol=https\nhost=example.org\n\n")
        self.assertEqual(r.returncode, 0)
        self.assertEqual(r.stdout, b"")

    # -- FLEET_GIT_TOKEN_HOST overrides the default ------------------

    def test_configured_host_overrides_the_default(self):
        r = self._run(
            "protocol=https\nhost=code.internal\n\n",
            {"FLEET_GIT_TOKEN_HOST": "code.internal"},
        )
        self.assertEqual(r.returncode, 0, msg=r.stderr)
        self.assertIn(f"password={TOKEN}".encode(), r.stdout)

    def test_configuring_a_host_stops_answering_for_github_com(self):
        """Overriding the target host is exclusive, not additive -- a
        runner's PAT is scoped to ONE remote at a time, matching SPEC 8's
        per-repo credential split (state vs code)."""
        r = self._run(
            "protocol=https\nhost=github.com\n\n",
            {"FLEET_GIT_TOKEN_HOST": "code.internal"},
        )
        self.assertEqual(r.returncode, 0)
        self.assertEqual(r.stdout, b"")

    # -- host= carries a port for nonstandard-port URLs --------------

    def test_port_suffixed_host_still_matches(self):
        """Per gitcredentials(7), and confirmed live against real `git
        credential fill` (see the script's comment): `host=` is
        `host[:port]` for a nonstandard port. A URL's userinfo
        (`https://user@host/...`) never lands in `host=` at all -- git
        sends it separately as `username=` -- so unlike a raw URL, `host=`
        needs only the port stripped before comparing."""
        r = self._run("protocol=https\nhost=github.com:443\n\n")
        self.assertEqual(r.returncode, 0, msg=r.stderr)
        self.assertIn(f"password={TOKEN}".encode(), r.stdout)

    def test_a_different_hosts_port_variant_is_still_refused(self):
        r = self._run("protocol=https\nhost=evil.example.com:443\n\n")
        self.assertEqual(r.returncode, 0)
        self.assertEqual(r.stdout, b"")

    def test_missing_host_line_is_refused(self):
        """No `host=` at all (a malformed or unusual request) must not be
        treated as a wildcard match."""
        r = self._run("protocol=https\n\n")
        self.assertEqual(r.returncode, 0)
        self.assertEqual(r.stdout, b"")


if __name__ == "__main__":
    unittest.main()
