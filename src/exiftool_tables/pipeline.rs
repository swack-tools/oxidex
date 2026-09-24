//! The one `FoundTag` conversion tail every generated-table walker shares
//! (Task 17).
//!
//! # Why this module exists
//!
//! The four generated walkers -- [`super::engine`] (`ProcessBinaryData`),
//! [`super::ifd_engine`] (`ProcessExif`), [`super::keyed_engine`]
//! (`ProcessCanonRaw`/`ProcessCanonCustom`) and [`super::serial_engine`]
//! (`ProcessSerialData`) -- acquire bytes in four unlike ways, and must keep
//! doing so: an IFD entry, a CIFF record, a serial slot and a binary offset
//! are different things and have different identities. But once a walker has
//! selected a row and read its value, ExifTool runs ONE sequence for all of
//! them -- `FoundTag` (ExifTool.pm:9448) applies the `RawConv`
//! (ExifTool.pm:9484-9505) and `GetValue` later applies `ValueConv` and
//! `PrintConv` (ExifTool.pm:3477-3664) -- and each walker used to carry its
//! own copy of that sequence. This is the single copy.
//!
//! # What it owns, stage by stage
//!
//! 1. **Condition.** A row whose `Condition` the walker has already evaluated
//!    true is no longer withheld for it (`condition_resolved`).
//! 2. **RawConv.** The one shape carried as data, `$$self{Member} = $val`,
//!    stores the value before reportability is considered; the IFD walker
//!    also mirrors it into the file-scoped [`Session`].
//! 3. **Omission.** Any remaining [`Omitted`] flag withholds the value.
//! 4. **Binary placeholder** (exiftool:3987), or **ValueConv** then
//!    **PrintConv**, with the unconverted form kept as the `-n` value.
//! 5. **Emission shape.** One [`Emitted`] for every walker, with the walker's
//!    stable identity ([`StableFieldIdentity`]) kept distinct.
//!
//! # What it deliberately does not own
//!
//! Acquisition (`ReadValue`, masks, cursors, IFD location, CIFF/word/serial
//! layouts), `GetTagInfo` selection, `SubDirectory` descent, Unknown
//! suppression, walker bookkeeping, and the attribution silence guard. The
//! guard stays at each engine's emit site because the Task 8 attribution
//! instrument (`tools/exiftool-tables/genshare/attribute.py`) finds guards
//! only in the engine files.
//!
//! # Per-walker policy is explicit, not harmonized
//!
//! The walkers differ in a handful of places ExifTool itself does not force
//! to agree -- how a state value the member model cannot hold stops the walk,
//! whether a modeled `RawConv` clears its omission, which Perl string an
//! unconverted scalar prints as, how long a `Binary` scalar is. Those are
//! [`Policy`] fields each engine states as a `const`, so this refactor keeps
//! every walker's pre-existing behaviour byte for byte.
//! `docs/reference/generated-runtime-walker-inventory.json` tabulates them.

use std::collections::HashMap;

use oxidex_tags::TagId;

use crate::core::TagValue;

use super::cond::MemberValue;
use super::engine::Emitted;
use super::ifd_schema::RawConvEffect;
use super::runtime::{self, DecodedValue};
use super::session::{MemberVal, Session};
use super::{ExprId, Omitted, PrintConv};

/// Where a row lives in its source table. The four kinds are never coerced
/// into one another: a keyed raw id or a serial index is not an IFD tag id.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StableFieldIdentity {
    /// A `ProcessExif` table's numeric tag id.
    IfdNumeric(u16),
    /// A keyed directory's native raw id (CIFF type bits included).
    KeyedRawId(u16),
    /// A `ProcessSerialData` slot index.
    SerialIndex(usize),
    /// A `ProcessBinaryData` key: index plus the optional `N.M` sub-index.
    BinaryIndex { index: i64, sub: Option<u32> },
}

impl StableFieldIdentity {
    /// The true source coordinate [`Emitted::source_id`] reports.
    #[must_use]
    pub fn source_id(&self) -> TagId {
        match *self {
            Self::IfdNumeric(id) | Self::KeyedRawId(id) => TagId::Numeric(id),
            Self::SerialIndex(index) => u16::try_from(index)
                .map_or_else(|_| TagId::Named(index.to_string()), TagId::Numeric),
            Self::BinaryIndex { index, sub } => match (u16::try_from(index), sub) {
                (Ok(index), None) => TagId::Numeric(index),
                (_, Some(sub)) => TagId::Named(format!("{index}.{sub}")),
                _ => TagId::Named(index.to_string()),
            },
        }
    }
}

/// The table a row came out of.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Provenance {
    pub module: &'static str,
    pub table: &'static str,
}

/// The row's groups, resolved by the walker's own precedence. `g1: None`
/// means the walker's family-1 rule withholds the row (an IFD `SET_GROUP1`
/// table walked without a directory name); it is honoured only after the
/// `RawConv` state write, where ExifTool would already have stored it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Groups {
    pub g0: &'static str,
    pub g1: Option<&'static str>,
    pub g2: &'static str,
}

/// Per-row reporting facts the walker resolves from its schema.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Reporting {
    pub name: &'static str,
    pub low_priority: bool,
    pub avoid: bool,
    pub is_list: bool,
}

/// The `PrintConv` stage: the shared typed conversion, or a renderer the
/// walker keeps local because its native input is walker-specific
/// (`ProcessSerialData`'s `DecodeBitsWords`).
#[derive(Clone, Copy)]
pub enum PrintStage<'a> {
    Shared(PrintConv),
    Adapter(&'a dyn Fn(&DecodedValue) -> Option<String>),
}

impl std::fmt::Debug for PrintStage<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Shared(conv) => f.debug_tuple("Shared").field(conv).finish(),
            Self::Adapter(_) => f.write_str("Adapter(..)"),
        }
    }
}

/// The conversions one row declares.
#[derive(Clone, Copy, Debug)]
pub struct Conversions<'a> {
    pub omitted: Omitted,
    /// The walker evaluated this row's `Condition` and it selected the row.
    pub condition_resolved: bool,
    pub raw_conv: Option<RawConvEffect>,
    pub value_conv: Option<ExprId>,
    pub print_conv: PrintStage<'a>,
    /// ExifTool's `Binary` flag. With no `ValueConv` the value is reported
    /// as the `(Binary data N bytes, ...)` placeholder.
    pub binary: bool,
}

/// What an acquisition adapter hands the shared stage.
#[derive(Clone, Debug)]
pub struct PipelineInput<'a> {
    pub identity: StableFieldIdentity,
    pub provenance: Provenance,
    pub groups: Groups,
    pub reporting: Reporting,
    pub conversions: Conversions<'a>,
    /// The value `FoundTag` receives (post-Mask, post-RoundFloat).
    pub raw: DecodedValue,
    /// The typed source value before Mask, RoundFloat or any conversion.
    /// Kept beside `raw` rather than folded into one member-shaped value:
    /// neither can be rebuilt from the other without loss.
    pub stored: TagValue,
    /// The single rational `ReadValue` returned, when the walker keeps it
    /// (`TAG_EXTRA{Rational}`); reported only while the value is that number
    /// unconverted.
    pub rational: Option<(i64, i64)>,
}

/// What a state value the member model cannot hold does to the walk.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OnUnrepresentableMember {
    /// Later rows could observe fabricated state: stop ([`Outcome::Tainted`]).
    Taint,
    /// Skip this row only ([`Outcome::Declined`]).
    Decline,
}

/// What an `Omitted::raw_conv` with no modeled effect does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OnUnmodeledRawConv {
    /// The unmodeled Perl may mutate shared state: stop ([`Outcome::Tainted`]).
    Taint,
    /// Withhold only this row, through the omission stage.
    Withhold,
}

/// One walker's explicit conversion policy. Each engine states its own as a
/// `const`; nothing here chooses a default for it.
#[derive(Clone, Copy, Debug)]
pub struct Policy {
    /// The scalar `$$self{Member} = $val` stores, or `None` when the member
    /// model cannot hold it faithfully.
    pub member_value: fn(&DecodedValue) -> Option<MemberValue>,
    pub on_unrepresentable_member: OnUnrepresentableMember,
    /// Whether a modeled `RawConv` clears the row's `Omitted::raw_conv`.
    pub set_member_clears_omission: bool,
    pub on_unmodeled_raw_conv: OnUnmodeledRawConv,
    /// `length($$val)` for the Binary placeholder.
    pub perl_length: fn(&DecodedValue) -> Option<usize>,
    /// The unconverted form of a non-`List` value.
    pub scalar_form: fn(&DecodedValue) -> TagValue,
}

/// What the shared stage did with one row.
#[derive(Clone, Debug)]
pub enum Outcome {
    /// Report this row. The walker's attribution guard decides whether it
    /// is published.
    Report(Emitted),
    /// An [`Omitted`] flag withholds the value.
    Omitted,
    /// A conversion stage produced no value: the member could not be
    /// represented under a `Decline` policy, the `Binary` scalar has no Perl
    /// length, `ValueConv` returned undef, or the family-1 rule withholds it.
    Declined,
    /// A state-affecting refusal: the walker must not interpret later rows.
    Tainted,
}

/// Run the shared `FoundTag` tail for one selected, read row.
///
/// `members` is the walker's `$$self{...}` condition state; `session` is the
/// file-scoped [`Session`] a walker also mirrors member writes into (only
/// the IFD walker has one).
pub fn execute(
    input: PipelineInput<'_>,
    policy: &Policy,
    members: &mut HashMap<&'static str, MemberValue>,
    session: Option<&mut Session>,
) -> Outcome {
    let PipelineInput {
        identity,
        provenance,
        groups,
        reporting,
        conversions,
        raw,
        stored,
        rational,
    } = input;

    // Condition: GetTagInfo already selected this row on its Condition.
    let mut omitted = conversions.omitted;
    if conversions.condition_resolved {
        omitted.condition = false;
    }

    // RawConv (ExifTool.pm:9484-9505): FoundTag stores the member before it
    // considers whether to report the tag; the assignment returns `$val`.
    match conversions.raw_conv {
        Some(RawConvEffect::SetMember { member }) => {
            let Some(value) = (policy.member_value)(&raw) else {
                return match policy.on_unrepresentable_member {
                    OnUnrepresentableMember::Taint => Outcome::Tainted,
                    OnUnrepresentableMember::Decline => Outcome::Declined,
                };
            };
            match session {
                Some(session) => {
                    let session_value = match &value {
                        MemberValue::Str(s) => MemberVal::Str(s.clone()),
                        MemberValue::Bytes(bytes) => MemberVal::from_bytes(bytes.clone()),
                        MemberValue::Num(n) => MemberVal::Int(*n),
                    };
                    members.insert(member, value);
                    // A non-UTF-8 Make/Model leaves the typed slot unsupplied.
                    let _ = session.set_member(member, session_value);
                }
                None => {
                    members.insert(member, value);
                }
            }
            if policy.set_member_clears_omission {
                omitted.raw_conv = false;
            }
        }
        // A value-local RawConv changes only FoundTag's `$val`; the row stays
        // withheld by its omission until a renderer is modeled.
        Some(RawConvEffect::ValueLocal) | None => {}
    }
    if omitted.raw_conv
        && conversions.raw_conv.is_none()
        && policy.on_unmodeled_raw_conv == OnUnmodeledRawConv::Taint
    {
        return Outcome::Tainted;
    }

    // Omission: any semantics the transcription does not reproduce withhold
    // the value rather than report a raw one under the real tag name.
    if omitted.any() {
        return Outcome::Omitted;
    }

    // ExifTool.pm:3535-3539: `Binary` with no `ValueConv` gets `\$val`, which
    // prints as the placeholder (exiftool:3987) and skips PrintConv.
    let placeholder = conversions.binary && conversions.value_conv.is_none();
    let (value, value_conv) = if placeholder {
        let Some(len) = (policy.perl_length)(&raw) else {
            return Outcome::Declined;
        };
        (
            TagValue::String(format!(
                "(Binary data {len} bytes, use -b option to extract)"
            )),
            None,
        )
    } else {
        let Some(converted) = runtime::apply_value_conv(conversions.value_conv, &raw) else {
            // A verified ValueConv may faithfully return Perl undef. That is
            // tag suppression, not permission to emit the raw value.
            return Outcome::Declined;
        };
        let unconverted = || {
            if reporting.is_list {
                runtime::to_tag_value(&converted)
            } else {
                (policy.scalar_form)(&converted)
            }
        };
        let rendered = match conversions.print_conv {
            PrintStage::Shared(conv) => runtime::render(conv, &converted),
            PrintStage::Adapter(render) => render(&converted),
        };
        match rendered {
            Some(rendered) => (TagValue::String(rendered), Some(unconverted())),
            None => (unconverted(), None),
        }
    };

    let Some(group1) = groups.g1 else {
        return Outcome::Declined;
    };
    let untouched = !placeholder && conversions.value_conv.is_none() && value_conv.is_none();
    Outcome::Report(Emitted {
        module: provenance.module,
        table: provenance.table,
        group0: groups.g0,
        group1,
        group2: groups.g2,
        name: reporting.name,
        source_id: identity.source_id(),
        stored,
        value,
        value_conv,
        low_priority: reporting.low_priority,
        avoid: reporting.avoid,
        rational: rational.filter(|_| untouched),
        is_list: reporting.is_list,
    })
}

/// ExifTool.pm:9469-9473 -- a tag's own `Priority` wins, else the table's
/// `PRIORITY`, else `Avoid` supplies 0. A walker whose compiler already
/// folded the table value into the tag passes `table: None`.
#[must_use]
pub const fn effective_priority(own: Option<i64>, table: Option<i64>, avoid: bool) -> Option<i64> {
    match (own, table) {
        (Some(priority), _) | (None, Some(priority)) => Some(priority),
        (None, None) => {
            if avoid {
                Some(0)
            } else {
                None
            }
        }
    }
}

/// `length($$val)` for a `Binary` scalar the keyed and serial walkers read:
/// the byte run for `undef`/unflagged strings, the text for a decoded string,
/// and Perl's scalar text otherwise.
#[must_use]
pub fn scalar_perl_length(value: &DecodedValue) -> Option<usize> {
    match value {
        DecodedValue::Undefined(bytes) | DecodedValue::StringBytes(bytes) => Some(bytes.len()),
        DecodedValue::String(text) => Some(text.len()),
        other => other.perl_string().map(|text| text.len()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const POLICY: Policy = Policy {
        member_value: super::super::engine::member_value,
        on_unrepresentable_member: OnUnrepresentableMember::Taint,
        set_member_clears_omission: true,
        on_unmodeled_raw_conv: OnUnmodeledRawConv::Taint,
        perl_length: scalar_perl_length,
        scalar_form: runtime::to_exiftool_value,
    };
    const MAP: &[(i64, &str)] = &[(2, "Two")];

    fn input(raw: DecodedValue) -> PipelineInput<'static> {
        PipelineInput {
            identity: StableFieldIdentity::SerialIndex(3),
            provenance: Provenance {
                module: "M",
                table: "T",
            },
            groups: Groups {
                g0: "G0",
                g1: Some("G1"),
                g2: "G2",
            },
            reporting: Reporting {
                name: "Name",
                low_priority: false,
                avoid: false,
                is_list: false,
            },
            conversions: Conversions {
                omitted: Omitted::NONE,
                condition_resolved: false,
                raw_conv: None,
                value_conv: None,
                print_conv: PrintStage::Shared(PrintConv::IntEnum(MAP)),
                binary: false,
            },
            raw,
            stored: TagValue::Integer(2),
            rational: Some((4, 2)),
        }
    }

    fn run(
        input: PipelineInput<'_>,
        policy: &Policy,
    ) -> (Outcome, HashMap<&'static str, MemberValue>) {
        let mut members = HashMap::new();
        let outcome = execute(input, policy, &mut members, None);
        (outcome, members)
    }

    #[test]
    fn printed_value_keeps_the_unconverted_form_and_drops_the_fraction() {
        let (Outcome::Report(row), _) = run(input(DecodedValue::Integer(2)), &POLICY) else {
            panic!("reported");
        };
        assert_eq!(row.value, TagValue::String("Two".into()));
        assert_eq!(row.value_conv, Some(TagValue::Integer(2)));
        assert_eq!(row.source_id, TagId::Numeric(3));
        assert_eq!(row.rational, None);
    }

    #[test]
    fn untouched_value_keeps_the_fraction() {
        let mut plain = input(DecodedValue::Integer(2));
        plain.conversions.print_conv = PrintStage::Shared(PrintConv::None);
        let (Outcome::Report(row), _) = run(plain, &POLICY) else {
            panic!("reported");
        };
        assert_eq!(row.value, TagValue::Integer(2));
        assert_eq!(row.value_conv, None);
        assert_eq!(row.rational, Some((4, 2)));
    }

    #[test]
    fn condition_resolution_clears_only_the_condition_omission() {
        let mut gated = input(DecodedValue::Integer(2));
        gated.conversions.omitted.condition = true;
        assert!(matches!(run(gated.clone(), &POLICY).0, Outcome::Omitted));
        gated.conversions.condition_resolved = true;
        assert!(matches!(run(gated, &POLICY).0, Outcome::Report(_)));
    }

    #[test]
    fn member_write_precedes_reportability_and_follows_policy() {
        let mut state = input(DecodedValue::Integer(7));
        state.conversions.raw_conv = Some(RawConvEffect::SetMember { member: "S" });
        state.conversions.omitted.raw_conv = true;
        let (outcome, members) = run(state.clone(), &POLICY);
        assert!(matches!(outcome, Outcome::Report(_)));
        assert_eq!(members.get("S"), Some(&MemberValue::Num(7)));

        let keep = Policy {
            set_member_clears_omission: false,
            ..POLICY
        };
        let (outcome, members) = run(state, &keep);
        assert!(matches!(outcome, Outcome::Omitted));
        assert_eq!(members.get("S"), Some(&MemberValue::Num(7)));
    }

    #[test]
    fn unrepresentable_member_taints_or_declines() {
        let mut state = input(DecodedValue::Float(1.5));
        state.conversions.raw_conv = Some(RawConvEffect::SetMember { member: "S" });
        let (outcome, members) = run(state.clone(), &POLICY);
        assert!(matches!(outcome, Outcome::Tainted));
        assert!(members.is_empty());
        let decline = Policy {
            on_unrepresentable_member: OnUnrepresentableMember::Decline,
            ..POLICY
        };
        assert!(matches!(run(state, &decline).0, Outcome::Declined));
    }

    #[test]
    fn unmodeled_raw_conv_taints_or_withholds() {
        let mut unmodeled = input(DecodedValue::Integer(2));
        unmodeled.conversions.omitted.raw_conv = true;
        assert!(matches!(
            run(unmodeled.clone(), &POLICY).0,
            Outcome::Tainted
        ));
        let withhold = Policy {
            on_unmodeled_raw_conv: OnUnmodeledRawConv::Withhold,
            ..POLICY
        };
        assert!(matches!(run(unmodeled, &withhold).0, Outcome::Omitted));
    }

    #[test]
    fn session_mirrors_a_member_write() {
        let mut state = input(DecodedValue::Integer(7));
        state.conversions.raw_conv = Some(RawConvEffect::SetMember { member: "S" });
        let mut members = HashMap::new();
        let mut session = Session::new();
        let outcome = execute(state, &POLICY, &mut members, Some(&mut session));
        assert!(matches!(outcome, Outcome::Report(_)));
        assert_eq!(members.get("S"), Some(&MemberValue::Num(7)));
        assert!(matches!(session.member("S"), MemberVal::Int(7)));
    }

    #[test]
    fn binary_placeholder_skips_both_conversions() {
        let mut blob = input(DecodedValue::Undefined(vec![1, 2, 3]));
        blob.conversions.binary = true;
        let (Outcome::Report(row), _) = run(blob, &POLICY) else {
            panic!("reported");
        };
        assert_eq!(
            row.value,
            TagValue::String("(Binary data 3 bytes, use -b option to extract)".into())
        );
        assert_eq!(row.value_conv, None);
        assert_eq!(row.rational, None);
    }

    #[test]
    fn adapter_renderer_and_list_form() {
        let render = |_: &DecodedValue| Some("rendered".to_string());
        let mut list = input(DecodedValue::Array(vec![
            DecodedValue::Integer(1),
            DecodedValue::Integer(2),
        ]));
        list.conversions.print_conv = PrintStage::Adapter(&render);
        list.reporting.is_list = true;
        let (Outcome::Report(row), _) = run(list, &POLICY) else {
            panic!("reported");
        };
        assert_eq!(row.value, TagValue::String("rendered".into()));
        assert_eq!(
            row.value_conv,
            Some(TagValue::Array(vec![
                TagValue::Integer(1),
                TagValue::Integer(2)
            ]))
        );
    }

    #[test]
    fn withheld_group1_declines_after_the_member_write() {
        let mut state = input(DecodedValue::Integer(7));
        state.conversions.raw_conv = Some(RawConvEffect::SetMember { member: "S" });
        state.groups.g1 = None;
        let (outcome, members) = run(state, &POLICY);
        assert!(matches!(outcome, Outcome::Declined));
        assert_eq!(members.get("S"), Some(&MemberValue::Num(7)));
    }

    #[test]
    fn identities_stay_distinct() {
        assert_eq!(
            StableFieldIdentity::IfdNumeric(5).source_id(),
            TagId::Numeric(5)
        );
        assert_eq!(
            StableFieldIdentity::KeyedRawId(5).source_id(),
            TagId::Numeric(5)
        );
        assert_eq!(
            StableFieldIdentity::SerialIndex(70_000).source_id(),
            TagId::Named("70000".into())
        );
        assert_eq!(
            StableFieldIdentity::BinaryIndex {
                index: 12,
                sub: Some(1)
            }
            .source_id(),
            TagId::Named("12.1".into())
        );
        assert_eq!(
            StableFieldIdentity::BinaryIndex {
                index: -1,
                sub: None
            }
            .source_id(),
            TagId::Named("-1".into())
        );
        assert_ne!(
            StableFieldIdentity::IfdNumeric(5),
            StableFieldIdentity::KeyedRawId(5)
        );
    }

    #[test]
    fn effective_priority_follows_exiftool_precedence() {
        assert_eq!(effective_priority(Some(1), Some(0), true), Some(1));
        assert_eq!(effective_priority(None, Some(0), false), Some(0));
        assert_eq!(effective_priority(None, None, true), Some(0));
        assert_eq!(effective_priority(None, None, false), None);
    }
}
