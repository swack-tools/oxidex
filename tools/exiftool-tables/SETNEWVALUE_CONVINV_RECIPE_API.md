# Source-selected SetNewValue ConvInv result handling

`setnewvalue_convinv_recipes.compile_setnewvalue_convinv` consumes the final
captured `native_write_helpers.set_new_value` CODE fact. It requires the
requested `Image::ExifTool::SetNewValue` binding, source provenance,
an authenticated direct `Image::ExifTool::ConvInv` binding, and an exact token
match for the complete native callable. It emits only the
ConvInv error-result operand: undefined error continues, defined false error
skips this tag through native `WriteAlso`, and a truthy error refuses.

The compiler does not recognize an isolated substring. Any added or reordered
statement anywhere in SetNewValue, including around the ConvInv call, is a
named omission. A changed false/defined control therefore cannot retain an old
operand silently.

This remains an inactive composition fact. Public SetNewValue behavior is not
implemented: option parsing, tag/group routing, list recursion, deletion,
DelCheck, NEW_VALUE construction, WriteAlso execution, value storage,
serialization, and file writes are all unsupported.

`RUNTIME_STATUS` is `inactive_source_operand_no_public_setnewvalue_admission`.
Emitting a source operand is therefore distinct from admitting any public
SetNewValue caller or writer route.

The native sidecar retains the caller's full recursive dependency graph. This
small operand validates only its own source plus the direct ConvInv binding,
because it never executes the rest of SetNewValue; the separately generated
ConvInv recipe must authenticate conversion behavior before composition.
