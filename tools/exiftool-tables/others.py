#!/usr/bin/env python3
r"""Step 25's `OTHER` registry: exact deparse-text -> Rust translation.

Same doctrine as `exprs.py`'s `TRANSLATIONS` (this file's sibling for
`ValueConv`/`PrintConv` Perl expression strings), applied to a PrintConv
hash's `OTHER => sub {...}` closure instead: `dump_tables.pl` recovers the
closure's body as deparsed Perl source (`B::Deparse`), and a closure is
translated only when its FULL deparsed text matches an entry here BYTE FOR
BYTE. Anything else is unregistered, and an unregistered `OTHER` means the
whole PrintConv is refused -- the tag keeps no conversion at all, never a
guess -- exactly the same "under-claim, never approximate" rule `exprs.py`'s
module doc states for expressions.

Why exact-match and not a pattern: two closures that *look* similar can
differ in exactly the byte that matters (a `>` that should be `>=`, an
off-by-one shift amount), and `OTHER` closures are Perl control flow, not
data -- there is no structural AST-walk-and-allowlist available the way
`conds.py`/`subdirs.py` have for their closed grammars. Keying on the
complete deparsed body is what makes a translation provably bound to the
exact code it was written against: an upstream ExifTool edit changes the
deparse text, and the (now-stale) translation simply stops matching instead
of silently running against different logic.

Why so few entries: `codegen.py`'s REPORT counts 353 `OTHER`-carrying
`PrintConv` hashes across the pinned 13.59 binary-table population, but
those 353 occurrences collapse onto only 25 DISTINCT deparsed bodies -- the
same closure literal (or, for `Exif::PrintParameter`, the same *named* sub)
is reused across many sibling tag definitions. Five of those 25 bodies
account for 328 of the 353 occurrences (92.9%) and are registered below;
each was checked against ExifTool's own `.pm` source (not just the
deparse) before being trusted -- see the citation on every entry. The
remaining 20 bodies were left unregistered because they are either
non-scalar (return an array ref, e.g. Pentax's `PrintFilter`), reach into
external lookup hashes this generator has no static view of (Minolta's
`%metabonesID`, Nikon's `GetAFPointGrid`), or operate on a
space-joined-multi-value string rather than a single scalar (Nikon's
`ExternalFlashFirmware`, Panasonic's `LensType`) -- translating any of them
correctly would mean re-implementing a data table or a helper function this
generator cannot verify independently, which is exactly the "plausible but
wrong" risk `AGENTS.md` rules out. `codegen.py`'s REPORT lists the
unregistered residue by (truncated) body so the next entry to add is
visible, not guessed at.
"""

# --- the five registered closures --------------------------------------

# Minolta.pm:648-658's `%afStatusInfo` OTHER sub (used by every AFStatus*
# field in Minolta::CameraInfoA100 -- 204 occurrences, all sharing this one
# literal). Forward path only (the `$inv and ...` branch is PrintConvInv,
# never reached from PrintConv):
#     return $val < 0 ? "Front Focus ($val)" : "Back Focus (+$val)";
# Always returns a defined string, so ExifTool's Unknown($val) fallback is
# never reached for this closure specifically.
_MINOLTA_AF_STATUS_FOCUS = (
    "{\n    package Image::ExifTool::Minolta;\n    use strict;\n"
    "    (my($val, $inv) = @_);\n"
    "    ($inv and (($val =~ /([-+]?\\d+)/), (return $1)));\n"
    '    (return (($val < 0) ? ("Front Focus ($val)") : ("Back Focus (+$val)")));\n'
    "}"
)

# Canon.pm's several literal `OTHER => sub { shift }` closures (AFConfig's
# AFTrackingSensitivity/AFAccelDecelTracking/AFPointSwitching, CameraSettings'
# Clarity, PSInfo's ContrastStandard/SharpnessFaithful -- 100 occurrences, all
# sharing this one literal): the unmatched value passes straight through
# unchanged. Also always defined -- the Unknown($val) fallback is never
# reached here either.
_CANON_SHIFT_IDENTITY = (
    "{\n    package Image::ExifTool::Canon;\n    use strict;\n    (shift());\n}"
)

# Sony.pm:2603's `OTHER => sub { shift }, # pass all other numbers straight
# through` (SequenceNumber, FaceInfo's FacesDetected -- 3 occurrences).
# Textually distinct from Canon's identical-behaviour closure only in its
# `package` line, so it needs its own exact-match key; both map to the same
# `OtherId::Identity` Rust variant.
_SONY_SHIFT_IDENTITY = (
    "{\n    package Image::ExifTool::Sony;\n    use strict;\n    (shift());\n}"
)

# FujiFilm.pm:1085-1089's `AFAreaPointSize` OTHER sub (1 occurrence):
#     OTHER => sub { return $_[0] },
# The same identity behaviour as the Canon/Sony closures above, spelled with
# `$_[0]` instead of `shift`, which deparses to different text.
_FUJIFILM_RETURN_ARG0_IDENTITY = (
    "{\n    package Image::ExifTool::FujiFilm;\n    use strict;\n"
    "    (return $_[0]);\n}"
)

# Exif.pm:5624-5639's NAMED sub `PrintParameter` (referenced via
# `OTHER => \&Image::ExifTool::Exif::PrintParameter` from Canon's
# CameraInfo1DmkII/1DmkIIN Saturation/ColorTone/Contrast -- 20 occurrences,
# all sharing this one deparse since it is one named sub, not an anonymous
# literal repeated at each call site):
#     return $val if $inv;
#     if ($val > 0) {
#         if ($val > 0xfff0) {       # a negative value in disguise?
#             $val = $val - 0x10000;
#         } else {
#             $val = "+$val";
#         }
#     }
#     return $val;
# Forward path (the `return $val if $inv` guard is PrintConvInv-only) always
# returns a defined value -- Unknown($val) is never reached for this closure
# either. Independently re-verified against `src/parsers/tiff/makernotes/
# canon/camera_info.rs`'s own hand-ported `print_parameter()` (written for
# `Pc::MapOrSigned` against the same Exif.pm source), which agrees exactly.
_EXIF_PRINT_PARAMETER = (
    "($$$) {\n    package Image::ExifTool::Exif;\n    use strict;\n"
    "    (my($val, $inv, $conv) = @_);\n"
    "    ($inv and (return $val));\n"
    "    if (($val > 0)) {\n"
    "        if (($val > 65520)) {\n"
    "            ($val = ($val - 65536));\n"
    "        } else {\n"
    '            ($val = "+$val");\n'
    "        }\n"
    "    }\n"
    "    (return $val);\n"
    "}"
)

# deparse text (exact, verbatim) -> Rust `OtherId` variant name.
OTHER_REGISTRY = {
    _MINOLTA_AF_STATUS_FOCUS: "MinoltaAfStatusFocus",
    _CANON_SHIFT_IDENTITY: "Identity",
    _SONY_SHIFT_IDENTITY: "Identity",
    _FUJIFILM_RETURN_ARG0_IDENTITY: "Identity",
    _EXIF_PRINT_PARAMETER: "ExifPrintParameter",
}


def translate_other(deparse_text):
    """`deparse_text` (dump_tables.pl's `__deparse` for a PrintConv hash's
    `OTHER` closure) -> the Rust `OtherId::<variant>` source text, or `None`
    if this exact closure is not registered. Byte-for-byte match only; see
    module docstring."""
    variant = OTHER_REGISTRY.get(deparse_text)
    if variant is None:
        return None
    return f"OtherId::{variant}"


# Rust source for the `OtherId` enum and its `apply`, appended to the
# generated file's PRELUDE. Hand-curated (not generated from a template the
# way `ExprId` is from `exprs.py`'s TRANSLATIONS) because these three
# variants are small enough, and different enough from one another in
# control flow, that a shared `{v}`-substitution template would not gain
# anything exprs.py's simple arithmetic templates get from theirs.
RUST_SUPPORT = '''
/// Step 25's OTHER registry (`tools/exiftool-tables/others.py`): a hand-
/// verified Rust port of an ExifTool PrintConv hash's `OTHER => sub {...}`
/// closure, keyed by the closure's exact deparsed text. Only closures that
/// are pure functions of the raw scalar value are eligible -- see the
/// module doc in others.py for what was left out and why.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OtherId {
    /// `sub { shift }` / `sub { return $_[0] }` (Canon.pm, Sony.pm:2603,
    /// FujiFilm.pm:1088): the unmatched value passes through unchanged.
    Identity,
    /// `Image::ExifTool::Exif::PrintParameter` (Exif.pm:5628-5639): a value
    /// over `0xfff0` is a negative one in disguise (`$val - 0x10000`);
    /// otherwise a positive value is prefixed `+`; zero or negative passes
    /// through unchanged. Independently re-verified against
    /// `src/parsers/tiff/makernotes/canon/camera_info.rs`'s own
    /// `print_parameter()`, hand-ported from the same source for
    /// `Pc::MapOrSigned`.
    ExifPrintParameter,
    /// Minolta.pm:648-658's `%afStatusInfo` OTHER sub: `"Front Focus
    /// ($val)"` for a negative value, `"Back Focus (+$val)"` otherwise.
    MinoltaAfStatusFocus,
}

impl OtherId {
    /// Apply this closure to the raw value. Every currently-registered
    /// variant always returns `Some` (none of the five source closures this
    /// enum ports have a code path that returns Perl `undef`), so
    /// `PrintConv::PartialEnumInt`'s `"Unknown ($val)"` fallback is never
    /// reached through any of them today -- but the `Option` return stays,
    /// because a future registration is not guaranteed the same property,
    /// and `PartialEnumInt::apply`'s fallback chain must stay correct for
    /// one that is not.
    #[must_use]
    pub fn apply(self, val: i64) -> Option<String> {
        match self {
            OtherId::Identity => Some(val.to_string()),
            OtherId::ExifPrintParameter => Some(if val > 0 {
                if val > 0xfff0 {
                    (val - 0x10000).to_string()
                } else {
                    format!("+{val}")
                }
            } else {
                val.to_string()
            }),
            OtherId::MinoltaAfStatusFocus => Some(if val < 0 {
                format!("Front Focus ({val})")
            } else {
                format!("Back Focus (+{val})")
            }),
        }
    }
}
'''
