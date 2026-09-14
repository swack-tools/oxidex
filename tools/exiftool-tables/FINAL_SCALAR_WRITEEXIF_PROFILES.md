# Final scalar `WriteExif` profile admission

`final_scalar_stage.py` consumes a complete token sequence from the final-loaded
`Image::ExifTool::Exif::WriteExif` CV. A matching token profile is provenance
for the complete function; it is not, on its own, permission to emit a scalar
recipe. The compiler then extracts the terminal ordinary-scalar source path
from that same B::Deparse body after whitespace removal.

The emitted executor represents only these source operations, in their native
order:

| Source operation | Generated final-stage behavior |
| --- | --- |
| `not defined($newVal)` or the `xDelete` condition | `Undefined` resolves to deletion. |
| `WriteValue($newVal, $newFormName, $newCount)` and `defined($newValue)` | shared scalar helper serializes the scalar; invalid serialization refuses/no-overwrite. |
| `length($newValue)` and `goto NoOverwrite` | defined empty serialization is a warning/no-overwrite, distinct from deletion. |
| guarded `strEnc`/`string` `Encode` | selected `string` rows require this source guard. The final boundary is after Sanitize, so default disabled CharsetEXIF preserves its byte input. |
| `Encode($newValue, 'UTF8')` for wire format `utf8` | required only if an emitted row actually selects the native `utf8` wire format. |
| later `int(($newSize + $fsize - 1) / $fsize)` assignment | count is ceiling division after serialization and encoding. |
| out-of-line even/count-width padding plus `$valBuff .= $$newValuePt` | executor returns encoded semantic bytes; TIFF carrier owns placement/alignment. |

`Scalar::Utf8` is a Rust representation of a Perl UTF8-flagged scalar, not the
native TIFF format name `utf8`. The final executor refuses `Scalar::Utf8`: the
input boundary is after source-derived Sanitize, where the supported default
path has produced bytes. It therefore does not need, or claim, a native
`$newFormName eq 'utf8'` branch when all selected rows are wire format `string`.

The committed 11.78 and 12.64 profiles both contain the ordinary `string`
CharsetEXIF guard and all non-encoding terminal operands. Both may emit that
scope when the other captured helper, registry, row, and context checks pass.
If a future selected row uses wire format `utf8`, profile admission additionally
requires the native UTF-8 encoding branch; 11.78 lacks it and will then refuse
with a named operative-path reason. This is source-dependent omission, not a
claim of historical generated write parity.

A changed, reordered, or inserted executable token first fails whole-body
admission. If a profile body matched but lacked or reordered a represented
operand, the second check refuses it as well. Native file comparisons are still
required before any historical write result can be considered parity.
