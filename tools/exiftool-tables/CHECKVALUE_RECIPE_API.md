# Source-derived scalar CheckValue execution

`checkvalue_recipes.compile_scalar_check` consumes the actual captured
`native_write_helpers.check_value` CODE fact. It recognizes the complete
function entry and the complete string/undef early-return branch. Its recipe
retains source format literals, comparison operators, count bound, error text
and padding. Unknown executable statements or local aliasing refuse. Numeric
formats cannot enter this evaluator; their native tail remains unsupported.
The parser binds requested/native callable provenance but does not use a
stored body hash as semantic admission. A body hash alone is not a compiler.

`evaluate_scalar_check` executes the compiled operations over `NativeScalar`.
The scalar distinguishes undefined, byte strings and UTF8-flagged character
strings. Counts are absent or signed integers; other scalar count types are
explicitly unsupported. The result preserves value/flag changes and native
error strings. It includes zero/negative counts, defined empty, NULs, Unicode
codepoint lengths and source-derived padding. It does not perform tag lookup,
CharsetEXIF encoding, WriteValue packing, file edits or deletion.

This is one executable helper step toward generated writing, not a production
writer or a complete CheckValue translation. CheckExif integration, shared
WriteValue compilation, exact public input and generated identity/placement
still precede any writer admission. No tag-specific code is removed here.

## Validation

The eight source/unit tests include operator/error/format changes that alter
execution, explicit numeric/count refusals, and entry/branch statement or local
alias mutations that cannot be admitted. The fixture in
`testdata/checkvalue_scalar_body.txt` uses the canonical scalar prefix and an
unreachable rejecting numeric tail; it is a parser fixture, not an oracle.

The separate native differential test requires explicit
`OXIDEX_PINNED_EXIFTOOL`, `OXIDEX_TABLES_JSON` and `EXIFTOOL_PERL`. It checks
the actual loaded helper source/body hashes and compares 240 scalar/format/count
cases against actual Perl, including flags and raw output bytes. Without
those inputs the native test reports a skip and cannot count as native proof.
All nine tests were run with native inputs, with no skips.

Independent copied-source replay changed native `>=` to `>` and its error
text, captured the changed helper fresh, and compared all 240 cases again:
23 native results changed and every new result matched. An added entry
statement refused after a fresh dump. This is a bounded source-change proof,
not proof of an arbitrary ExifTool release upgrade. Evidence is under
`shared-pilot/write-upgrade-integration-20260913/checkvalue-scalar-native-01/`
relative to the continuation evidence root.
