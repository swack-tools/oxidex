# Keyed reporting-policy validation

This compiler change preserves native reporting flags in the keyed schema.
A native variant group with known SerialNumber alternatives and a final
Unknown alternative can now be represented without discarding its known
alternatives. The optional artifact remains inactive: there is no production
keyed caller, Canon table enablement, or manual Canon decoder removal.

The implementation reuses shared flag expansion and `IfdFlags`. Expansion
happens once before names, conditions, formats and omission facts are read.
A table's defined `AVOID` overrides a row's `Avoid`, including zero. A row's
defined `Priority` overrides table `PRIORITY`, including zero. A selected
Unknown alternative suppressed by native policy ends selection; it must not
fall through to another alternative. Falsy scalar `Flags` values do nothing.

## Recorded source and results

- Source: pinned ExifTool 13.59; recorded Perl 5.38 dump SHA-256
  `193cf4e91326f53c7bdfd674cb10da0937c8bcdb4c0285ad4f507fd9f96a8fb8`.
- Baseline schema: PR #748 at `ebbe1ece`. The supplied expression ledger
  matches the dump hash. No fresh dump from another Perl was substituted.
- Instrument: `codegen.py`, with the recorded dump, matching expression
  ledger and explicit binary, IFD and keyed outputs.
- Instrument: `verify.py`, parsing all three generated artifacts and comparing
  them with the explicit pinned Perl `oracle.pl`. Keyed native inventory:
  **61 named rows, 57 represented, four declared omissions, zero fact
  mismatches**. Each native named row has exactly one reporting-flags fact.
  Binary and IFD soundness checks also pass. Wider strict binary completeness
  is not claimed by this run.
- The parent remains blocked by eight unwalked edges, one unsupported format
  and three unsupported raw IDs. Represented rows with unwalked edges are
  not active runtime coverage. The seven source validators and one unsupported
  child processor still need shared support.
- The generated binary artifact adds only the newly reachable serial-format
  expression identifier and its generated evaluation/render arms. The
  generated IFD artifact and C header are unchanged.
- The exact optional keyed artifact compiles against metadata produced from
  this checkout. Workspace formatting and the C-header check pass.
- Actual-source all-features library/test Clippy passed in 17.916 seconds.
  Existing unrelated integration-test warnings remain.
- The full Python table-tool suite passed 587 tests in 183.714 seconds before
  the final review follow-up. The follow-up adds falsy-Flags handling and
  contract comments; all 25 keyed tests pass, independent native verification
  passes, and all three generated artifacts remain byte-identical to the
  full-suite version. This is not a claim that the full suite was rerun after
  the follow-up.

Independent pinned Perl probes confirm expansion before selection, native
Unknown suppression, defined-zero precedence and table-level policy. Mutation
checks cover expanded names and conditions, unknown fallback preservation,
malformed policy refusal and stale executable/source reporting facts.

The first verifier invocation used its cwd-relative default oracle path from
an incompatible directory and failed to start the oracle. The corrected
explicit-oracle invocation passed; that original failure remains in the local
evidence record. No corpus output or current generated-share measurement is
claimed for this source-only checkpoint.

## Remaining work

Integrate the schema into the published shared reader, preserve native
selection and state behavior, close the parent's blockers, and validate both
standalone CRW and embedded JPEG carriers. Only then activate the route and
retire the two manual Make/Model decoders and their numeric parent dispatches.
The [main plan](../AUTOGENERATION-PLAN.md) keeps that retirement separate from
compiler and source-inventory progress.
