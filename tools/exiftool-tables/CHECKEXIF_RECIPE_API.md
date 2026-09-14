# Inactive CheckExif source recipes

`checkexif_recipes.py` is an optional source-only compiler. It turns the
captured, effective `CHECK_PROC` facts in `native_write_tables` into a compact
recipe only when the entire executable `CheckExif` body matches its closed
grammar. The output is separate from the generated Rust artifacts and has no
writer route or runtime effect.

Each recipe preserves the source order of the three `Format` fallbacks, the
falsey-or-literal missing-format branch, the `Groups->{0}` MakerNotes return,
the count operand, and the requested and final effective `CheckValue` binding.
`source_tables` identifies the source tables sharing that exact recipe. A
changed supported selector order or literal produces a different recipe hash;
an additional executable statement, a changed call shape, or an unrepresentable
binding is an explicit omission. Corrupt provenance is an error, never an
omission.

The compiler consumes `native_write_helpers.check_value` only when it includes
both the requested glob name and the final loaded callable fact. The requested
name permits a legitimate anonymous final coderef while still detecting a
rebound requested glob. The actual body, source file, source SHA-256, and
captured dependencies remain provenance; they are not a claim that `CheckValue`
or a writer has been modeled.

The rendered JSON has `runtime_status:
"inactive_source_checkexif_recipe_no_writer_route"`. Recipe execution remains
open, as do `WriteExif`, `WriteValue`, scalar format branches, carrier editing,
charset behavior, and all write admission and public API work. This module does
not select tags by name or ID, and it does not modify the existing read or
write-candidate projections.

Run it only against a recorded dump when inspecting source facts:

```sh
python3 tools/exiftool-tables/checkexif_recipes.py tables.json -o checkexif-recipes.json
```
