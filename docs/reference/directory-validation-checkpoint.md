# Directory validation implementation checkpoint

Base: `5fb979d8` on `refactor/tag-machinery`. This work records native helper
source, compiles supported call arguments and supplies a common Rust size
comparison. It does not activate Canon directory routing or remove manual code.

`dump_tables.pl` records the loaded helper body, source file and hash, resolving
aliases only after all requested modules load. Nested function dependencies
are captured as provenance, with explicit cycle and depth refusals; global
unpacking state still requires a separate contract. The compiler recognizes a closed
u16 membership-check body and takes offsets and expected sizes from the source
call. It does not select behavior by camera, tag or helper name. Unsupported
syntax stays explicitly unwalked. The independent oracle reads live source
provenance, and its verifier parses call operands independently of the compiler.

The primitive preserves inherited byte order, declared-size comparisons,
alternative sizes, exact division and native short-read numeric coercion. These
comparisons are not structural buffer bounds checks.

## Verified locally

- Focused Python suite: 39 checks, including extraction after deferred module
  loading, aliases, real generated-artifact parsing, source/operand mutations,
  and rejection when an artifact drops the required reader-contract blocker.
- Rust primitive suite: three checks, with the log confirming compilation from
  the owned checkout. Exact repository CI lint command passes.
- ExifTool 13.59 with explicit Perl 5.34.1: seven compiled CanonRaw validation
  calls; independent keyed inventory accounts for 61 native rows, with 57
  represented and four explicit omissions, zero native-fact discrepancies.
  This is source-fact evidence, not a complete runtime validation verdict.

The local commands are:

```sh
python3 -m unittest discover -s tools/exiftool-tables -p 'test_directory_validation.py'
python3 -m unittest discover -s tools/exiftool-tables -p 'test_dump_validate_functions.py'
python3 -m unittest discover -s tools/exiftool-tables -p 'test_keyed_directory.py'
cargo test --lib --all-features exiftool_tables::validation::tests
cargo clippy --all-features -- -D warnings
```

An additional lint attempt with `--lib --tests -- -D warnings` failed on 23
warnings in unchanged integration tests. No passing verdict is claimed for
that command. The initial source replay also had two report-format errors;
the dump, artifact and oracle outputs were preserved and the final source-fact
assertions passed. Neither failed attempt is counted as validation.

## Required before activation

Independent review changed native `Get16u` from a 16-bit to a 32-bit unpack.
The outer validation helper's source hash stayed unchanged, while its result
changed. This proves that outer-helper provenance alone is insufficient.
Every compiled edge therefore retains `validate_reader_contract` in its
unwalked reasons, and the independent verifier refuses its removal. The seven
call sites are implemented operands; zero of these edges are cleared for use.

Capture and verify the numeric reader and its unpacking/state dependencies,
including a copied-source mutation that changes only the reader. Then integrate
validation into the shared reader and prove a rejected child leaves subsequent
parent behavior intact. Regenerate official artifacts and ledgers through
`tools/exiftool-tables/regen.sh` with the pinned release and canonical Perl
5.38.2 environment, holding the i7 heavy-job lock for every phase. The new dump
facts deliberately change its hash; an old ledger cannot authenticate them.

Other parent blockers and both CRW/JPEG carrier comparisons still remain.
Complete those before enabling Canon routing and retiring manual Make/Model
code. This checkpoint establishes no whole-project automation percentage.
