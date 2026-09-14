# Source artifact and enablement baseline

Status: active in the renewed full-parity goal. PR #776 repairs the earlier
checkpoint inventory. This is a source/artifact accounting milestone; it does
not claim generated declarations are readable tags or add runtime behavior.

The join conserves every table in its recorded dump, reads generated definitions
and enablement policy from explicit immutable commits, and retains missing
artifacts, Gate A blockers, and unknown policy separately. Its strict family
check now recognizes the source-shaped word-directory candidate alongside CIFF;
an unclassified source table is never silently accepted as a keyed family just
because a generated artifact exists.

The enablement parser rejects unsupported syntax, missing commas, duplicates,
unterminated literals/comments, and misleading declarations inside comments or
strings. It handles nested comments without counting commented policy tuples.
Candidate executor blobs have commit-pinned hashes. Their existence does not prove
registry calls, carrier dispatch, or observed extraction. Allowlisted tables
without verified dispatch are explicitly unverified; a missing policy is unknown.

## Reproduction

Run under the host shared heavy-job lock, using a new output directory outside
the worktree. Supply the exact captured dump SHA and immutable source/artifact
commits rather than a moving branch tip:

```sh
python3 tools/exiftool-tables/join_source_artifacts.py \
  --dump "$DUMP" --repo . --expected-dump-sha "$DUMP_SHA" \
  --selector-ref "$SELECTOR_COMMIT" --expected-selector-commit "$SELECTOR_COMMIT" \
  --artifact-ref "$ARTIFACT_COMMIT" --expected-artifact-commit "$ARTIFACT_COMMIT" \
  --out "$SOURCE_ARTIFACT_OUTPUT"
```

Focused validation: 18 join tests and 6 source-inventory tests. Runtime Rust and
Cargo source match the base, already validated with Clippy. Full-capture counts
must be regenerated after this fix before being quoted as current.

## Limits and subsequent work

This join covers binary, IFD and keyed artifacts. Serial and other producer
registries remain outside that scope, and no manual implementation census exists
here. The legacy full dump also misses hydrated catalog identities: the separate
catalog reconciliation must drive its expansion. Neither omission can disappear
from the full-goal denominator. The remaining work is complete hydrated layout
capture, all producer registries, verified consumer/caller evidence, and independent
read/write observations. No global read/write support percentage is claimed.
