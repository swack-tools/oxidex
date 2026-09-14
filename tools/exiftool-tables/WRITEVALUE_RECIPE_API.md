# Source-derived scalar WriteValue execution

`writevalue_recipes.compile_scalar_write` consumes the final captured
`native_write_helpers.write_value` CODE fact. It authenticates the exact
entry bindings, the lexical `%writeValueProc` lookup, and the `if ($proc) …
elsif (string/undef)` control-flow. Admission requires the live captured
lexical hash to be resolved and to have no entry for either source-derived
scalar format. Thus an installed dispatch entry cannot silently take the
numeric branch before scalar handling.

`evaluate_scalar_write` preserves undefined, byte and UTF8-flagged scalar
states, source-derived terminator/truncation/padding behaviour, and the final
native count. It deliberately refuses all formats outside the captured scalar
branch and all non-integer counts. It does not model optional `$dataPt`/
`$offset` mutation, numeric packing, tag lookup, CharsetEXIF encoding, or a
public writer route.

This is an inactive reference mechanism. The captured `%writeValueProc` is
read as loaded state, so platform-time dispatch removal is observed. A missing
or unresolved pad never means an empty dispatch map.

## Validation and current integration boundary

Nine tests pass with explicit pinned native inputs and no skips. The native
scalar differential covers 272 cases, including wide characters outside the
selected substring. Independent review exercised actual native lexical
shadowing; a local that aliases the input value must refuse. The regression
suite also covers source-derived format and count-bound changes.

A separate actual-source rehearsal changes only WriteValue's count guard
from `> 0` to `> 2` in a copied native Writer.pl. Fresh capture and compilation
match all 240 original native cases; 43 native outcomes change, with zero
handwritten tag-rule or generated-output edits. Evidence relative to the
continuation root is
`shared-pilot/write-upgrade-integration-20260913/writevalue-source-replay-20260914/`.
The native test observes return bytes, defined/UTF8 state and length. Its count
observation is the unchanged caller argument, not the helper-local final count;
the latter remains a reference result checked by source/unit tests.

## Generated Rust execution

`scalar_helper_codegen.py` compiles the captured CheckValue and WriteValue
operands into `src/writers/generated_scalar_rules.rs`. Normal `regen.sh` owns
that artifact and `scalar_helper_ledger.json`; the inventory now has 34 outputs.
The shared `generated_scalar.rs` executor consumes those operands. Supported
source changes replace the operands; unsupported source emits `None` and a
named ledger gap instead of retaining an old rule. The native suite also
compares both committed artifacts with generation from CI's fresh pinned dump,
so omitting regeneration cannot leave stale rules behind a green helper test.
The public writer remains inactive for these definitions.

The actual compiled Rust executor matches 512 authenticated native helper cases
(240 validation and 272 serialization) for the canonical source and again for
the copied WriteValue count-guard change. A third actual-source replay changes
CheckValue's comparisons, including a branch that executes a negative Perl
repetition; all 512 cases match that native source too. The native return
records supply the expected values, defined/UTF8 state and validation errors.
Rust's helper-local count is separately compared with the Python source
reference; this is not a native observation of that local variable.

The first Rust replay exposed an ASCII substring's UTF8 storage downgrade.
The corrected executor preserves native behavior when non-ASCII characters
occur outside the selected prefix as well. These are helper-level checks,
not evidence of a public generated write route.

Composition with CheckExif/CheckValue, CharsetEXIF encoding, public tag routing
and complete file write/read-back remain the next steps. Numeric packing and
the optional native data-target mutation are also unfinished. This checkpoint
does not establish full release-upgrade or read/write conformance.
