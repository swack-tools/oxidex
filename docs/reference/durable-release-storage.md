# Durable release storage

Release sources, toolchains, corpora, logs, receipts, and controller state live
below `/Users/allen/oxidex-ops`. Cargo targets and worktrees live below
`/Users/allen/git`. Release tooling resolves paths before use and refuses
system temporary directories, environment overrides outside the durable root,
and every path containing a symlink component (including symlinks whose target
would remain inside the durable root).

## Pinned oracle layout

- Perl 5.38.2: `/Users/allen/oxidex-ops/toolchains/perl-5.38.2/prefix`
- ExifTool 13.59 checkout: `/Users/allen/oxidex-ops/cache/exiftool/13.59/exiftool`
- ExifTool executable: `/Users/allen/oxidex-ops/cache/exiftool/13.59/exiftool/exiftool`
- Combined corpus: `/Users/allen/oxidex-ops/cache/exiftool/13.59/combined-samples`
- Downloads: `/Users/allen/oxidex-ops/cache/downloads`
- Manifest and journal:
  `/Users/allen/oxidex-ops/evidence/20260919-beta1-functional/durable-controller-oracle-bootstrap`

`tools/release/oracle-lock.json` pins the Perl and Archive::Zip archive hashes,
the ExifTool tag object, the locked corpus-tree hash, and every manufacturer sample archive. Provisioning
downloads to a `.partial` sibling, checks SHA-256, and renames only after the
hash matches. Builds and extracted trees use a staging sibling on the same
filesystem. A verified canonical tree is immutable: provisioning verifies and
reuses it, while a fresh verified staging tree is renamed only into an absent
destination. A conflicting canonical tree fails closed in place; no backup or
implicit repair path moves it. Each staging sibling has a durable sidecar
transaction intent written before materialization and upgraded with a locked
candidate-tree hash only after verification. On startup, a dead-owner candidate
that still passes the current verifier is published if the canonical path is
absent; if an authenticated canonical tree already exists, the candidate is
moved to named durable staging quarantine. An unowned, malformed, symlinked,
changed, or multiply authenticated staging state fails closed. Only a dead
owned candidate that the current lock positively rejects is removed
automatically, with an append-only recovery-journal entry.

Provision and verify with:

```bash
python3 tools/release/bootstrap_oracle.py provision \
  --root /Users/allen/oxidex-ops
python3 tools/release/bootstrap_oracle.py verify \
  --root /Users/allen/oxidex-ops --pin 13.59 \
  --manifest /Users/allen/oxidex-ops/evidence/20260919-beta1-functional/durable-controller-oracle-bootstrap/storage-manifest.json
```

Verification requires Perl `v5.38.2`, Archive::Zip `1.68`, ExifTool `13.59`,
the locked Git tag object, `OOXML.docx` reporting `DOCX`, and at least 4,000
combined-corpus files. The manifest records SHA-256 identities for source
archives, installed trees, executables, the Archive::Zip and ExifTool library
files, the locked corpus tree, and the sibling corpus manifest. `verify`
recomputes those hashes before it refreshes the manifest. `provision` refuses
authenticated tree or library damage in place; repair or quarantine is an
explicit operation. If the obsolete nested
`exiftool/combined-samples` copy is present, provisioning refuses it. An
operator may run `quarantine-legacy-corpus` only when the nested and sibling
trees have the same lock hash; it journals the disposition and never selects
the nested tree silently. A matching version string alone is not accepted.

The `compare-exiftool-full` and `compare-exiftool-full-update` recipes run
`bootstrap_oracle.py verify` before consuming this installation. The
`docs-coverage` and `duplicate-loss-scan` recipes only run `check-path` to
fence the override, then make their own selected tree/corpus checks; they do
not claim a complete locked-oracle verification. The two full-comparison
recipes reject an `EXIFTOOL_CACHE_DIR` override unless it resolves exactly to
`/Users/allen/oxidex-ops/cache/exiftool/13.59`; their wrapper, library, and
bounded sample path are then derived from that verified canonical cache, not
from an arbitrary same-version tree. None of these recipes replaces
the shared oracle or creates release evidence under a system temporary
directory. Each full-comparison invocation atomically publishes a private mode
`0700` wrapper, fsyncs it and its parent, and directly execs the durable Perl as
`perl5.38.2 -I<locked-exiftool-lib> <locked-exiftool-script> <args...>`.
The wrapper writes a unique durable JSON identity marker before that exec; its
private wrapper directory is removed and the receipt parent is fsynced on exit.
The marker, rather than a script shebang or resolver-only claim, is the evidence
of the Perl used by a successful measurement.
