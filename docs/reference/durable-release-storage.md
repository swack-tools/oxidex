# Durable release storage

Release sources, toolchains, corpora, logs, receipts, and controller state live
below `~/oxidex-ops`, or the absolute durable directory selected by
`OXIDEX_OPS_DIR`. `scripts/ops_paths.py` owns this default and validates overrides.
Existing fleet worktrees keep their `~/git` default (`OXIDEX_WORKTREE_ROOT`);
targets keep `~/git/oxidex-beta1-targets` (`OXIDEX_TARGET_ROOT`). Both may be
set to durable children of the operational root for a new installation.
Changing these roots does not relocate an existing controller ledger: select
the roots that contain its recorded worktrees and targets when resuming it.
Release tooling resolves paths before use and refuses
system temporary directories, environment overrides outside the durable root,
and every path containing a symlink component (including symlinks whose target
would remain inside the durable root).

## Pinned oracle layout

- Perl 5.38.2: `~/oxidex-ops/toolchains/perl-5.38.2/prefix`
- ExifTool 13.59 checkout: `~/oxidex-ops/cache/exiftool/13.59/exiftool`
- ExifTool executable: `~/oxidex-ops/cache/exiftool/13.59/exiftool/exiftool`
- Combined corpus: `~/oxidex-ops/cache/exiftool/13.59/combined-samples`
- Downloads: `~/oxidex-ops/cache/downloads`
- Manifest and journal:
  `~/oxidex-ops/evidence/20260919-beta1-functional/durable-controller-oracle-bootstrap`

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
export OXIDEX_OPS_DIR="$(python3 scripts/ops_paths.py)"
python3 tools/release/bootstrap_oracle.py provision \
  --root "$OXIDEX_OPS_DIR"
python3 tools/release/bootstrap_oracle.py verify \
  --root "$OXIDEX_OPS_DIR" --pin 13.59 \
  --manifest "$OXIDEX_OPS_DIR/evidence/20260919-beta1-functional/durable-controller-oracle-bootstrap/storage-manifest.json"
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
`~/oxidex-ops/cache/exiftool/13.59`; their wrapper, library, and
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

Hosted documentation uses `just compare-exiftool-ci-update`, the pinned CI
source action, the runner's capability-checked Perl, and hash-locked sample
archives. Its ephemeral cache and generated Pages report are separate from
maintainer release qualification. The indicative benchmark workflow uses the
same explicit CI source/interpreter channel and does not relax the durable
resolver when `GITHUB_ACTIONS` happens to be set.
