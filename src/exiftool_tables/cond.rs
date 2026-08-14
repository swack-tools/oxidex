//! `Cond`: a closed grammar for ExifTool's `Condition` strings, and the
//! interpreter that evaluates it.
//!
//! # Why this exists
//!
//! `dump_tables.pl` (see `_variants` in that script, and its comment there)
//! represents ExifTool's model-dependent binary layouts -- Canon
//! `CameraInfo`'s 33 alternatives, Sony `ExtraInfo3`'s NEX-vs-everything-else
//! `CameraOrientation` -- as a Perl arrayref of tag-info hashes, each guarded
//! by a `Condition` string. Before Step 23, `tools/exiftool-tables/codegen.py`
//! refused every `_variants` entry outright (its own comment: "needs
//! Condition evaluation, which is a Perl expression. Out of scope for the
//! mechanical pass by design"). Step 23 lifts that refusal for the closed
//! subset of `Condition` shapes Step 15's census found cover 80.4% of all
//! condition *uses* in the pinned tree (`OVERHAUL_STEP15_DECISION.md` s2),
//! plus a bitmask shape the maintainer approved as a seventh (the largest
//! cluster in the 19.6% residue).
//!
//! Same doctrine as `tools/exiftool-tables/exprs.py`'s expression compiler:
//! `tools/exiftool-tables/conds.py` parses a `Condition` string and either
//! produces one of the variants below, by construction proving it reproduces
//! the Perl exactly, or refuses -- there is no partial/approximate mode.
//! Every compiled `Cond` is checked against the pinned ExifTool release's own
//! Perl by `tools/exiftool-tables/verify_cond.py`, mirroring
//! `verify_exprs.py`'s differential oracle for value conversions.
//!
//! # First-match-wins (`Image::ExifTool::GetTagInfo`)
//!
//! ExifTool.pm's `GetTagInfo` (the routine every `_variants` array is
//! resolved through, both for ordinary IFD tag lookup and, via
//! `ProcessBinaryData`'s `$tagInfo = $self->GetTagInfo($tagTablePtr,
//! $index)`, for binary-table fields) walks the alternatives in array order:
//!
//! ```perl
//! foreach $tagInfo (@infoArray) {
//!     my $condition = $$tagInfo{Condition};
//!     if ($condition) {
//!         ...
//!         unless (eval $condition) {
//!             ...
//!             next;
//!         }
//!     }
//!     ...
//!     return $tagInfo;
//! }
//! ```
//!
//! The first entry whose `Condition` evaluates true (or has none) wins, and
//! evaluation stops there -- entries after the winner are never evaluated at
//! all. [`Cond::first_match`] reproduces exactly this: entries are tried in
//! order, the first `true` short-circuits the walk.
//!
//! # `eval $condition`'s side effects run even on a losing entry
//!
//! `unless (eval $condition)` evaluates the *whole* Perl expression for every
//! entry it visits, including ones that end up `next`-ed past. When a
//! `Condition` is `($$self{Member} = EXPR) and <rest>` -- ExifTool's
//! idiom for stashing a value as a side effect of testing a condition, e.g.
//! `Condition => '($$self{CameraInfoCount} = $count) and $$self{Model} =~
//! /\b1DS?$/'` (Canon.pm:1312, "save size of this record ... for later
//! tests"), or the degenerate always-true `Condition => '$$self{NewLensData}
//! = 1'` (Pentax.pm:4343, "not really a condition, just used to set flag") --
//! the assignment fires whether or not that entry ultimately wins. A losing
//! entry earlier in the array can still set a data member a later entry, or a
//! sibling field entirely, depends on. [`Cond::SetMember`] reproduces this:
//! [`Cond::eval`] always performs the assignment before testing `then`, same
//! as Perl always performs the assignment before `and` short-circuits on it.
//! This is the same contract `src/parsers/tiff/makernotes/sony.rs`'s
//! `TAG_PANORAMA` handling already implements by hand for Sony.pm:902's
//! `Condition => '$$self{Panorama} = ($$valPt =~ /^(\0\0)?\x01\x01/)'`
//! ("ExifTool's Condition is an assignment: the flag is set ... whether or
//! not the sub-directory that follows is processed").

use std::collections::HashMap;

/// One `$$self{...}` (or `$self->{...}`) data member's value, as read or
/// written by a [`Cond`]. ExifTool's data members are dynamically typed Perl
/// scalars; the closed grammar only ever compares a given member against one
/// concrete type (a model-name member is always matched with a string
/// operator, a schema/version member always with a numeric one), so callers
/// populate whichever variant the member actually holds.
#[derive(Clone, Debug, PartialEq)]
pub enum MemberValue {
    Str(String),
    Num(i64),
}

/// Evaluation context for [`Cond::eval`]: the subset of ExifTool's per-file
/// `$self` state, `$$valPt`, `$format` and `$count` the closed grammar can
/// read -- and, for [`Cond::SetMember`], write back into `members` exactly
/// as `eval $condition`'s assignment would mutate `$self`.
pub struct Ctx<'a> {
    pub members: &'a mut HashMap<&'static str, MemberValue>,
    /// `$$valPt`: the raw bytes of the value about to be read, when the
    /// caller has them available (`ProcessBinaryData` only passes this to a
    /// `GetTagInfo` call when the un-conditioned index lookup fails and a
    /// short read is taken to re-check with `$$valPt` in scope).
    pub val_pt: Option<&'a [u8]>,
    pub format: Option<&'a str>,
    pub count: Option<i64>,
}

impl<'a> Ctx<'a> {
    #[must_use]
    pub fn new(members: &'a mut HashMap<&'static str, MemberValue>) -> Self {
        Self {
            members,
            val_pt: None,
            format: None,
            count: None,
        }
    }

    #[must_use]
    pub fn with_val_pt(mut self, val_pt: &'a [u8]) -> Self {
        self.val_pt = Some(val_pt);
        self
    }

    #[must_use]
    pub fn with_count(mut self, count: i64) -> Self {
        self.count = Some(count);
        self
    }
}

/// A numeric comparison operator, for [`Cond::MemberCmp`] and
/// [`Cond::CountCmp`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CmpOp {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

impl CmpOp {
    #[must_use]
    const fn apply(self, lhs: i64, rhs: i64) -> bool {
        match self {
            CmpOp::Eq => lhs == rhs,
            CmpOp::Ne => lhs != rhs,
            CmpOp::Lt => lhs < rhs,
            CmpOp::Le => lhs <= rhs,
            CmpOp::Gt => lhs > rhs,
            CmpOp::Ge => lhs >= rhs,
        }
    }
}

/// Where a [`Cond::SetMember`] assignment's value comes from. `$count` is the
/// only interpolated variable the census's assignment idioms ever assign
/// (Canon.pm:1312's `$$self{CameraInfoCount} = $count`); a bare literal
/// (Pentax.pm:4343's `$$self{NewLensData} = 1`) is [`EffectSource::Const`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EffectSource {
    Count,
    Const(i64),
}

/// The closed `Condition` grammar Step 23 compiles, plus [`Cond::Always`] for
/// an array entry with no `Condition` at all (ExifTool: unconditional match).
///
/// Seven closed *comparison* shapes (`OVERHAUL_STEP15_DECISION.md` s2, plus
/// the maintainer-approved bitmask addition), a conjunction of atoms (the
/// census's "conjunction of the above") represented as binary [`Cond::And`]
/// nodes right-nested by `conds.py` for a 3+-clause `A and B and C` chain
/// (`And(A, And(B, C))` -- the same left-to-right short-circuit order Perl's
/// left-associative `and` produces), and [`Cond::SetMember`] for the
/// assignment-as-condition idiom described in this module's doc comment.
#[derive(Clone, Copy, Debug)]
pub enum Cond {
    /// `$$self{Member}` bare-truthy, or `not $$self{Member}` when `negate`.
    /// Perl truthiness: a numeric member is truthy iff nonzero, a string
    /// member is truthy iff neither empty nor the single character `"0"`.
    MemberTruthy { member: &'static str, negate: bool },
    /// `$$self{Member} <op> N`, `N` a signed integer literal (source may have
    /// been decimal or `0x`-hex; both parse to the same `i64` at compile
    /// time in `conds.py`, so no base is carried here).
    MemberCmp {
        member: &'static str,
        op: CmpOp,
        value: i64,
    },
    /// `$$self{Member} eq "str"` / `ne "str"` (`negate` for `ne`).
    MemberStrEq {
        member: &'static str,
        value: &'static str,
        negate: bool,
    },
    /// `$$self{Member} =~ /pattern/[i]` / `!~` (`negate` for `!~`).
    /// `pattern` is Rust `regex` syntax already translated from the Perl
    /// source by `conds.py` (e.g. `\0` -> `\x00`); it is restricted to a
    /// vetted subset -- literals, `^`/`$`/`\b` anchors, `|` alternation,
    /// grouping, character classes, and the `?` quantifier -- verified
    /// against the pinned ExifTool's own regex engine by `verify_cond.py`,
    /// not merely assumed compatible because the syntax looks similar.
    MemberRegex {
        member: &'static str,
        pattern: &'static str,
        ignore_case: bool,
        negate: bool,
    },
    /// `$$valPt =~ /pattern/` / `!~`, matched against raw bytes (FLIR::Parts'
    /// `\0`-terminated tag-name sniffing is the entire population of this
    /// shape in the binary-table `_variants` census).
    ValPtRegex { pattern: &'static str, negate: bool },
    /// Seventh shape (maintainer addition, `OVERHAUL_STEP15_DECISION.md`):
    /// `$$self{Member} & 0xNN` (bare, `op: Ne, value: 0` -- Perl numeric-
    /// context truthiness of the `&` result) or `($$self{Member} & 0xNN)
    /// <op> N` (Sony.pm's `($$self{FlashFired} & 0x01) != 1` idiom).
    MemberBitAnd {
        member: &'static str,
        mask: i64,
        op: CmpOp,
        value: i64,
    },
    /// `$format eq "..."`.
    FormatEq { value: &'static str },
    /// `$count <op> N`.
    CountCmp { op: CmpOp, value: i64 },
    /// `<left> and <right>`, both evaluated (never short-circuited away
    /// entirely -- `left` always runs; `right` runs only if `left` is true,
    /// matching Perl's `and`). A 3+-clause chain nests: see the enum's own
    /// doc comment.
    And(&'static Cond, &'static Cond),
    /// `($$self{Member} = <source>) [and <then>]` -- see this module's doc
    /// comment. The assignment always executes; the assigned value's Perl
    /// truthiness gates `then` as `and` would, and IS the result when `then`
    /// is absent (Pentax.pm:4343's bare-assignment idiom).
    SetMember {
        member: &'static str,
        source: EffectSource,
        then: Option<&'static Cond>,
    },
    /// No `Condition` in the source: ExifTool tries no `eval` at all and the
    /// entry always matches.
    Always,
}

impl Cond {
    /// Evaluate this condition against `ctx`, applying any [`Cond::SetMember`]
    /// side effects along the way -- exactly the side effects `eval
    /// $condition` would have produced in real ExifTool, whether or not the
    /// overall result is `true`. See this module's doc comment for why that
    /// matters to entries that end up losing [`first_match`].
    #[must_use]
    pub fn eval(&self, ctx: &mut Ctx) -> bool {
        match self {
            Cond::Always => true,
            Cond::MemberTruthy { member, negate } => {
                let truthy = perl_truthy(ctx.members.get(*member));
                truthy ^ negate
            }
            Cond::MemberCmp { member, op, value } => match ctx.members.get(*member) {
                Some(MemberValue::Num(n)) => op.apply(*n, *value),
                _ => false,
            },
            Cond::MemberStrEq {
                member,
                value,
                negate,
            } => {
                let eq =
                    matches!(ctx.members.get(*member), Some(MemberValue::Str(s)) if s == value);
                eq ^ negate
            }
            Cond::MemberRegex {
                member,
                pattern,
                ignore_case,
                negate,
            } => {
                let matched = match ctx.members.get(*member) {
                    Some(MemberValue::Str(s)) => regex_match_str(pattern, *ignore_case, s),
                    _ => false,
                };
                matched ^ negate
            }
            Cond::ValPtRegex { pattern, negate } => {
                let matched = match ctx.val_pt {
                    Some(bytes) => regex_match_bytes(pattern, bytes),
                    None => false,
                };
                matched ^ negate
            }
            Cond::MemberBitAnd {
                member,
                mask,
                op,
                value,
            } => match ctx.members.get(*member) {
                Some(MemberValue::Num(n)) => op.apply(n & mask, *value),
                _ => false,
            },
            Cond::FormatEq { value } => ctx.format == Some(*value),
            Cond::CountCmp { op, value } => ctx.count.is_some_and(|c| op.apply(c, *value)),
            Cond::And(left, right) => {
                // Both sides always run through `eval` when `left` is true,
                // matching Perl's short-circuit `and` -- `right` is not
                // evaluated at all when `left` is false, same as Perl skips
                // the right operand entirely rather than evaluating it for
                // its (unused) side effects.
                left.eval(ctx) && right.eval(ctx)
            }
            Cond::SetMember {
                member,
                source,
                then,
            } => {
                let value = match source {
                    EffectSource::Count => ctx.count.unwrap_or(0),
                    EffectSource::Const(c) => *c,
                };
                // Always runs, even when the caller is walking past a losing
                // entry -- this IS the effect `eval`'s assignment produces
                // whether or not the surrounding `and` ends up true.
                ctx.members.insert(member, MemberValue::Num(value));
                if value == 0 {
                    return false;
                }
                match then {
                    Some(t) => t.eval(ctx),
                    None => true,
                }
            }
        }
    }
}

/// Perl truthiness of an optional member value: absent is false (an
/// undefined `$$self{Member}` is falsy in Perl too), a numeric member is
/// truthy iff nonzero, a string member is truthy iff neither empty nor the
/// single character `"0"` (Perl's `"0"` string is the one non-empty string
/// that is still false in boolean context).
fn perl_truthy(v: Option<&MemberValue>) -> bool {
    match v {
        None => false,
        Some(MemberValue::Num(n)) => *n != 0,
        Some(MemberValue::Str(s)) => !s.is_empty() && s != "0",
    }
}

fn regex_match_str(pattern: &str, ignore_case: bool, subject: &str) -> bool {
    match regex::RegexBuilder::new(pattern)
        .case_insensitive(ignore_case)
        .build()
    {
        Ok(re) => re.is_match(subject),
        // `conds.py` validated `pattern` against the vetted subset before
        // emitting it; a build failure here would be a generator bug, not a
        // data problem. Fail closed (no match) rather than panic a metadata
        // reader over a malformed literal.
        Err(_) => false,
    }
}

fn regex_match_bytes(pattern: &str, subject: &[u8]) -> bool {
    match regex::bytes::RegexBuilder::new(pattern).build() {
        Ok(re) => re.is_match(subject),
        Err(_) => false,
    }
}

/// One offset's alternatives, tried in [`Cond::eval`] order -- the Rust
/// analogue of a `_variants` arrayref. `index`/`sub` mirror [`super::Field`]'s
/// own (a fractional `sub` is possible in principle; none of the pinned
/// tree's binary-table `_variants` entries land on one).
#[derive(Clone, Copy, Debug)]
pub struct VariantGroup {
    pub index: i64,
    pub sub: Option<u32>,
    pub alternatives: &'static [(Cond, super::Field)],
}

/// ExifTool's `GetTagInfo` walk: the first alternative whose [`Cond`]
/// evaluates true, or `None` if every one refuses (in real ExifTool this
/// means the tag is simply not produced for this file -- there is no
/// fallback rendering, matching "refuse and count", not "guess"). Side
/// effects from every alternative visited before (and including) the winner
/// are applied to `ctx` along the way -- entries after the winner are never
/// evaluated at all, matching `GetTagInfo`'s `return $tagInfo` inside the
/// loop.
#[must_use]
pub fn first_match<'a>(
    alternatives: &'a [(Cond, super::Field)],
    ctx: &mut Ctx,
) -> Option<&'a super::Field> {
    for (cond, field) in alternatives {
        if cond.eval(ctx) {
            return Some(field);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx_with(pairs: &[(&'static str, MemberValue)]) -> HashMap<&'static str, MemberValue> {
        pairs.iter().cloned().collect()
    }

    #[test]
    fn member_regex_matches_and_negates() {
        let mut members = ctx_with(&[("Model", MemberValue::Str("DSLR-A230".to_string()))]);
        let cond = Cond::MemberRegex {
            member: "Model",
            pattern: r"^DSLR-A(230|290|330|380|390)\b",
            ignore_case: false,
            negate: false,
        };
        assert!(cond.eval(&mut Ctx::new(&mut members)));

        let neg = Cond::MemberRegex {
            member: "Model",
            pattern: r"^DSLR-A(230|290|330|380|390)\b",
            ignore_case: false,
            negate: true,
        };
        assert!(!neg.eval(&mut Ctx::new(&mut members)));
    }

    #[test]
    fn member_cmp_numeric() {
        let mut members = ctx_with(&[("ColorDataVersion", MemberValue::Num(9))]);
        let cond = Cond::MemberCmp {
            member: "ColorDataVersion",
            op: CmpOp::Eq,
            value: 9,
        };
        assert!(cond.eval(&mut Ctx::new(&mut members)));
        let cond2 = Cond::MemberCmp {
            member: "ColorDataVersion",
            op: CmpOp::Lt,
            value: 330,
        };
        assert!(cond2.eval(&mut Ctx::new(&mut members)));
    }

    #[test]
    fn member_bit_and_seventh_shape() {
        let mut members = ctx_with(&[("BitM", MemberValue::Num(0x80))]);
        let cond = Cond::MemberBitAnd {
            member: "BitM",
            mask: 0x80,
            op: CmpOp::Ne,
            value: 0,
        };
        assert!(cond.eval(&mut Ctx::new(&mut members)));
        let miss = Cond::MemberBitAnd {
            member: "BitM",
            mask: 0x40,
            op: CmpOp::Ne,
            value: 0,
        };
        assert!(!miss.eval(&mut Ctx::new(&mut members)));
    }

    #[test]
    fn member_bit_and_with_explicit_comparison() {
        // Sony.pm's `($$self{FlashFired} & 0x01) != 1` idiom.
        let mut members = ctx_with(&[("FlashFired", MemberValue::Num(0x00))]);
        let cond = Cond::MemberBitAnd {
            member: "FlashFired",
            mask: 0x01,
            op: CmpOp::Ne,
            value: 1,
        };
        assert!(cond.eval(&mut Ctx::new(&mut members)));
    }

    #[test]
    fn valpt_regex_matches_raw_bytes_including_nul() {
        let mut members = HashMap::new();
        let mut ctx = Ctx::new(&mut members).with_val_pt(b"detector\0extra");
        let cond = Cond::ValPtRegex {
            pattern: r"^detector\x00",
            negate: false,
        };
        assert!(cond.eval(&mut ctx));
    }

    #[test]
    fn first_match_wins_and_stops_evaluating() {
        // A synthetic two-alternative array modelled on Sony.pm's
        // ExtraInfo3 0x0016 (MemoryCardConfiguration for DSLR, NEX-only
        // CameraOrientation with Mask 0xc0 -- see sony/amount.rs).
        static DSLR_COND: Cond = Cond::MemberRegex {
            member: "Model",
            pattern: r"^DSLR-",
            ignore_case: false,
            negate: false,
        };
        static NEX_COND: Cond = Cond::MemberRegex {
            member: "Model",
            pattern: r"^(NEX-(3|5|5C|C3|VG10|VG10E))\b",
            ignore_case: false,
            negate: false,
        };
        let dslr_field = super::super::Field {
            index: 0x16,
            sub: None,
            name: "MemoryCardConfiguration",
            format: None,
            count: 1,
            mask: None,
            omitted: super::super::Omitted::NONE,
            print_conv: super::super::PrintConv::None,
        };
        let nex_field = super::super::Field {
            name: "CameraOrientation",
            ..dslr_field
        };
        let alts: &[(Cond, super::super::Field)] =
            &[(DSLR_COND, dslr_field), (NEX_COND, nex_field)];

        let mut members = ctx_with(&[("Model", MemberValue::Str("NEX-VG10E".to_string()))]);
        let winner = first_match(alts, &mut Ctx::new(&mut members)).expect("NEX alt wins");
        assert_eq!(winner.name, "CameraOrientation");
    }

    #[test]
    fn set_member_effect_fires_even_on_a_losing_entry() {
        // Models Canon.pm:1312: 'Condition => ($$self{CameraInfoCount} =
        // $count) and $$self{Model} =~ /\b1DS?$/' -- for a model that does
        // NOT match the regex, the whole condition is false (so this entry
        // loses first_match), but $$self{CameraInfoCount} must still have
        // been set from $count by the time evaluation moves on, exactly as
        // `unless (eval $condition)` in ExifTool.pm's GetTagInfo would have
        // left it after `next`.
        static REGEX_PART: Cond = Cond::MemberRegex {
            member: "Model",
            pattern: r"\b1DS?$",
            ignore_case: false,
            negate: false,
        };
        static SET_THEN_MATCH: Cond = Cond::SetMember {
            member: "CameraInfoCount",
            source: EffectSource::Count,
            then: Some(&REGEX_PART),
        };
        let mut members = ctx_with(&[("Model", MemberValue::Str("EOS 5D".to_string()))]);
        let mut ctx = Ctx::new(&mut members).with_count(1548);
        assert!(!SET_THEN_MATCH.eval(&mut ctx), "EOS 5D is not a 1D/1DS");
        assert_eq!(
            ctx.members.get("CameraInfoCount"),
            Some(&MemberValue::Num(1548)),
            "the assignment must have run despite this entry losing"
        );
    }

    #[test]
    fn set_member_bare_assignment_is_always_true() {
        // Models Pentax.pm:4343: 'Condition => $$self{NewLensData} = 1' --
        // "not really a condition, just used to set flag".
        let mut members = HashMap::new();
        let cond = Cond::SetMember {
            member: "NewLensData",
            source: EffectSource::Const(1),
            then: None,
        };
        assert!(cond.eval(&mut Ctx::new(&mut members)));
        assert_eq!(members.get("NewLensData"), Some(&MemberValue::Num(1)));
    }

    #[test]
    fn member_truthy_bare_and_negated() {
        let mut members = ctx_with(&[("FujiWidth", MemberValue::Num(0))]);
        let bare = Cond::MemberTruthy {
            member: "FujiWidth",
            negate: false,
        };
        assert!(!bare.eval(&mut Ctx::new(&mut members)));
        let negated = Cond::MemberTruthy {
            member: "FujiWidth",
            negate: true,
        };
        assert!(negated.eval(&mut Ctx::new(&mut members)));
    }

    #[test]
    fn member_str_eq_and_ne() {
        let mut members = ctx_with(&[("RIFFStreamType", MemberValue::Str("auds".to_string()))]);
        let eq = Cond::MemberStrEq {
            member: "RIFFStreamType",
            value: "auds",
            negate: false,
        };
        assert!(eq.eval(&mut Ctx::new(&mut members)));
        let ne = Cond::MemberStrEq {
            member: "RIFFStreamType",
            value: "vids",
            negate: true,
        };
        assert!(ne.eval(&mut Ctx::new(&mut members)));
    }
}
