# Numeric public writer draft

This compiler admits six additional Exif::Main rows from captured properties,
without numeric tag-name or ID exceptions. The original nine string rows remain.
The complete CheckValue body, IsInt/IsHex/IsFloat dependencies and intRange are
consumed. The numeric WriteValue dispatcher, Rationalize, AssembleRational,
Set16u, Set32u, DoPackStd and byte-order dependencies are independently consumed.
Unknown source statements, callbacks, row controls or bindings refuse admission.

The bounded runtime supports count-one int16u/rational64u inputs: unsigned
integers, source-ordered hex conversion, decimal/comma/exponent forms, native
integer rounding and range exception, explicit fractions (including zero
denominators and bounded integer wrapping), and the native continued-fraction
algorithm. Negative unsigned values fail the source checks; negative values
rounding to zero follow the integer source branch. Non-finite Float inputs and
integer strings beyond the represented machine domain remain explicit refusals.
Public Integer, Rational and finite Float values use this route only when their
source format compiled. Descriptor classes and preferred IFD0 reverse names
join the generated numeric format and core/Writer capture.

The full SetNewValue caller produces an identity-bearing operand with explicit
directory names and its undefined-input conversion bypass. Resolver, dispatch
and final selected-directory validation bind it to the current address capture.
The selected directory is distinct from preferred WriteGroup. Final recipes,
numeric helpers, address requests and migration compilation also join native
core identity, preventing mixed generated source artifacts from granting writes.

Undefined authored values bypass ConvInv, as native SetNewValue requires.
The generated final DeleteEntry operand retains absence intent through fresh
mandatory-default insertion. Deletion-only operations require a retained
nonmandatory entry (or an authored set); the native mandatory-only directory
cleanup branch still refuses explicitly and remains unfinished work. Broader
bare-name selection across multiple native candidates also remains unfinished.

The public `generated_scalar_write_matrix_v4` declares 1242 cases over TIFF LE,
TIFF BE and JPEG. Its independently labeled coverage families are:

- Original string alias cases: 432.
- Original numeric alias cases: 252.
- Added selected-directory string cases: 216.
- Added selected-directory numeric cases: 126.
- Added public decimal/Float numeric alias cases: 144.
- Added public decimal/Float selected-directory cases: 72.

Every selected-directory case retains the same tag in IFD0 and compares the
complete selected directory. Explicit directory operands are read from generated
Rust and their ledger, with the address capture checked independently. Numeric
public scalar types and per-case bytes are published in the cohort contract.
The original 684-case baseline remains identifiable. Fresh batch keeps its
nine string anchors for mandatory overrides/removals, and reports numeric rows
as non-anchor targets. Version-adapter changes are owned separately.

Actual evidence before the new public gate: portable source/runtime tests;
132 canonical native helper cases (86 exact accepted-byte matches, 46 mutual
rejections, no native-accepted refusals); copied CheckValue insertion changes
bytes and refuses after all source hashes are rebound; six native IFD1 fixture
seed/update cases across all three wire families and TIFF LE/BE preserve the
same-ID IFD0 values. These do not substitute for a fresh Cargo build, complete
library tests, the 1242 public-file cases, fresh batch cases, and Clippy.
