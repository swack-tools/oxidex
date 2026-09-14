# Historical fresh-JPEG byte-order profiles

The fresh-JPEG compiler authenticates four final-loaded native callables:
`SetPreferredByteOrder`, `SetByteOrder`, `GetByteOrder`, and `DoProcessTIFF`.
A profile checks every B::Deparse token in the preferred-order method except
the two source fallback literals, checks complete token digests of the core
helpers and caller, and matches the exact new-header caller block. It does not
use the selected release label or a source-file hash as a behavior rule.

The persisted 11.78 and 12.64 pair has two IFD0 source predicates:

| Release | `DoProcessTIFF` operand | emitted caller mode |
| --- | --- | --- |
| 11.78 | calls `SetPreferredByteOrder()` with no default argument | `NoDefaultArgument` |
| 12.64 | computes `$defaultByteOrder` only for `DirName eq 'GPS'`; fixed IFD0 passes undef to `SetPreferredByteOrder($defaultByteOrder)` | `Ifd0DefaultArgumentUndef` |

Both helper captures observed the fixed fresh-IFD0 no-override result `MM` and
observed the three unadmitted override sources as `II`. The runtime continues
to refuse those overrides. The common result is not assumed: the caller mode is
carried from the matched source profile.

Changing both native fallback literals from `MM` to `II` remains within the
complete preferred-order grammar and changes the emitted selected order. An
inserted/reordered method token, changed helper, or changed caller predicate
refuses. Historical fresh-JPEG file writes still require version-specific
native/generated comparison; these facts authenticate only header byte-order
selection.
