"""Source-driven schema compiler for native keyed binary directories.

This module emits facts only.  It deliberately has no reader, no table index
in the production module and no enablement hook.  Selection is the named
native `PROCESS_PROC`, never a module/table or tag-ID allowlist.
"""

from collections import Counter
import re

import codegen
import conds
import subdirs


PROCESS_CANON_RAW = "Image::ExifTool::CanonRaw::ProcessCanonRaw"
_RAW_ID = re.compile(r"^(?:0|[1-9][0-9]*|0x[0-9a-fA-F]+)$")


def _stats():
    stats = Counter()
    for key in (
        "unsupported_exprs", "pc_directives_dropped", "other_unregistered_bodies",
        "value_conv_refused_expressions", "dropped_code_refs", "hook_refusal_reasons",
    ):
        stats[key] = Counter()
    return stats


def processor_name(meta):
    pp = (meta or {}).get("PROCESS_PROC")
    if isinstance(pp, dict):
        return pp.get("__name")
    return pp if isinstance(pp, str) else None


def is_keyed_directory_table(meta):
    """A known keyed-process family, selected by native processor identity."""
    return processor_name(meta) == PROCESS_CANON_RAW


def parse_raw_id(raw):
    if not isinstance(raw, str) or _RAW_ID.fullmatch(raw) is None:
        return None
    try:
        value = int(raw, 0)
    except ValueError:
        return None
    return value if 0 <= value <= 0x3fff else None


def _type_default(raw_id):
    # ProcessCanonRaw: ($tag >> 8) & 0x38 selects this default.  This is a
    # layout rule, not a per-vendor tag dictionary.
    return {0x00: "int8u", 0x08: "string", 0x10: "int16u", 0x18: "int32u"}.get(
        (raw_id >> 8) & 0x38
    )


def _field_format(tag, default, stats):
    """Return `(Rust Option<Fmt>, source spelling)`.

    The keyed reader derives an absent Format from the raw entry type, so it
    remains `None` in generated data.  Explicit formats reuse codegen's closed
    spelling maps; no decoder is invented here.
    """
    f = tag.get("Format")
    if f is None:
        return "None", default
    if not isinstance(f, str):
        stats["keyed_format"] += 1
        return None, None
    m = codegen.SIZED_RE.match(f)
    if m:
        base, count = m.group(1), int(m.group(2))
        if base == "string":
            return f"Some(Fmt::Str({count}))", f
        if base == "undef":
            return f"Some(Fmt::Undef({count}))", f
        if base in codegen.SCALAR_FORMATS:
            return f"Some(Fmt::{codegen.SCALAR_FORMATS[base][0]})", base
        stats["keyed_format"] += 1
        return None, None
    if f == "string":
        return "Some(Fmt::RemainderString)", f
    if f in codegen.SCALAR_FORMATS:
        return f"Some(Fmt::{codegen.SCALAR_FORMATS[f][0]})", f
    if f in codegen.PER_FIELD_FORMATS:
        return f"Some(Fmt::{codegen.PER_FIELD_FORMATS[f]})", f
    stats["keyed_format"] += 1
    return None, None


def _same_table_directory(tag, raw_id):
    """Whether ProcessCanonRaw owns this entry as a nested CIFF directory.

    The 0x28/0x30 type values have no scalar default format.  They are still
    source rows a future keyed reader must see, because the native procedure
    recurses through them before normal value handling.  Keeping the edge
    here is schema data, not a reader implementation.
    """
    return (
        isinstance(tag.get("SubDirectory"), dict)
        and not tag["SubDirectory"]
        and (raw_id >> 8) & 0x38 in (0x28, 0x30)
    )


def _count(tag, stats):
    """Preserve the raw Count contract for a future ProcessCanonRaw reader.

    CanonRaw.pm starts from `tagInfo->{Count}`.  An inline normal value only
    gets the special count-one default when Count is *undefined*; zero stays
    false and is later derived from the entry's byte size.  A scalar Format
    never silently changes either source fact into `Some(1)`.
    """
    raw = tag.get("Count")
    if raw is None:
        return "None", 1
    try:
        count = int(str(raw), 0)
    except (TypeError, ValueError):
        stats["keyed_count"] += 1
        return None, None
    if count < 0:
        stats["keyed_count"] += 1
        return None, None
    # Count zero remains visible.  The future reader will use Perl's false
    # count path to derive it from byte size, rather than mistake it for one.
    return f"Some({count})", count or 1


class Context:
    def __init__(self, doc):
        self.processors = {}
        for module, mod in doc.get("modules", {}).items():
            for table, value in mod.get("tables", {}).items():
                self.processors[(module, table)] = processor_name(value.get("meta"))

    def target_supported(self, module, table):
        return self.processors.get((module, table)) in {
            "Image::ExifTool::ProcessBinaryData", PROCESS_CANON_RAW
        }


def _edge(tag, raw_id, ctx, stats):
    sd = tag.get("SubDirectory")
    if sd is None:
        return "None"
    if not isinstance(sd, dict):
        stats["keyed_subdirectory"] += 1
        return 'Some(KeyedEdge::BoundedValue { module: "", table: "", start: KeyedStart::Zero, unwalked: &["subdirectory"] })'

    # ProcessCanonRaw recurses before ordinary SubDirectory handling when the
    # entry type is 0x28/0x30 and the value is not inline. `{}` only supplies
    # the native directory name; it is not a pointer edge.
    if not sd:
        if _same_table_directory(tag, raw_id):
            return "Some(KeyedEdge::SameTableDirectory)"
        stats["keyed_subdirectory"] += 1
        return 'Some(KeyedEdge::BoundedValue { module: "", table: "", start: KeyedStart::Zero, unwalked: &["tag_table"] })'

    try:
        module, table = subdirs.parse_tag_table(sd.get("TagTable"))
    except subdirs.SubdirCompileError:
        stats["keyed_subdirectory"] += 1
        return 'Some(KeyedEdge::BoundedValue { module: "", table: "", start: KeyedStart::Zero, unwalked: &["tag_table"] })'

    reasons = []
    # CanonRaw.pm applies Start only when truthy. The first reader supports
    # exactly the native default zero; a declared zero remains that default.
    if codegen.perl_truthy(sd.get("Start")):
        reasons.append("start")
    if sd.get("Validate") is not None:
        reasons.append("validate")
    if sd.get("ProcessProc") is not None:
        reasons.append("process_proc")
    if not ctx.target_supported(module, table):
        reasons.append("target_processor")
    if reasons:
        stats["keyed_edge_unwalked"] += 1
    body = ", ".join(f'"{reason}"' for reason in reasons)
    return (
        "Some(KeyedEdge::BoundedValue { "
        f'module: "{codegen.rust_str(module)}", table: "{codegen.rust_str(table)}", '
        f"start: KeyedStart::Zero, unwalked: &[{body}] }})"
    )


def _raw_conv(tag):
    member = codegen.raw_conv_effect(tag.get("RawConv"))
    if member is not None:
        return f'Some(RawConvEffect::SetMember {{ member: "{codegen.rust_str(member)}" }})'
    if codegen.raw_conv_value_local(tag.get("RawConv")):
        return "Some(RawConvEffect::ValueLocal)"
    return "None"


def _native_facts(tag):
    """Verbatim source facts retained beside executable keyed schema fields.

    They authenticate omission rows and detect source-condition changes that
    cannot be reconstructed from a compiled `Cond`.  The reader must use the
    typed fields, not this audit copy.
    """
    option = lambda value: "None" if not isinstance(value, str) else f'Some("{codegen.rust_str(value)}")'
    groups = codegen.compile_groups_field(tag.get("Groups"), _stats())
    sd = tag.get("SubDirectory")
    if isinstance(sd, dict):
        subdir = (
            "Some(KeyedNativeSubdir { "
            f"tag_table: {option(sd.get('TagTable'))}, start: {option(sd.get('Start'))}, "
            f"validate: {str(sd.get('Validate') is not None).lower()}, "
            f"process_proc: {str(sd.get('ProcessProc') is not None).lower()} }})"
        )
    else:
        subdir = "None"
    return (
        "KeyedNativeFacts { "
        f"format: {option(tag.get('Format'))}, count: {option(str(tag['Count']) if tag.get('Count') is not None else None)}, "
        f"condition: {option(tag.get('Condition'))}, groups: {groups}, subdir: {subdir} }}"
    )


def _tag_literal(tag, raw_id, stats, verified_exprs, ctx):
    name = tag.get("Name")
    if not isinstance(name, str) or not name:
        stats["keyed_name"] += 1
        return None, "raw_id"
    if codegen.perl_truthy(tag.get("Unknown")):
        stats["keyed_unknown"] += 1
        return None, "unknown"
    condition = tag.get("Condition")
    condition_src = "None"
    if condition is not None:
        condition_src = conds.compile_cond(condition)
        if condition_src is None or conds.needs_initial_get_tag_info_context(condition):
            stats["keyed_condition"] += 1
            return None, "condition"
        condition_src = f"Some({condition_src})"
    default = _type_default(raw_id)
    if default is None and _same_table_directory(tag, raw_id):
        # A directory entry has no scalar format.  `format: None` tells the
        # future reader to apply the raw keyed type rule before trying a
        # scalar decoder; `undef` only supplies a closed input domain while
        # compiling any source conversions for the declaration.
        default = "undef"
    if default is None:
        stats["keyed_raw_id"] += 1
        return None, "raw_id"
    fmt_src, format_name = _field_format(tag, default, stats)
    if fmt_src is None:
        return None, "format"
    count_src, count_for_domain = _count(tag, stats)
    if count_src is None:
        return None, "count"
    mask = codegen.mask_for(tag, stats)
    if mask is None:
        return None, "mask"
    domain = codegen.value_domain(format_name, count_for_domain)
    value_conv, modeled_value = codegen.value_conv_for(tag, stats, domain, verified_exprs)
    print_conv, refused_print = codegen.conv_for(
        tag, stats, codegen.print_conv_input_domain(tag, domain, modeled_value), verified_exprs
    )
    return (
        "KeyedTag { "
        f'raw_id: {raw_id:#06x}, name: "{codegen.rust_str(name)}", '
        f"format: {fmt_src}, count: {count_src}, condition: {condition_src}, "
        f"raw_conv: {_raw_conv(tag)}, "
        f"omitted: {codegen.omitted_for(tag, stats, False, modeled_value, refused_print)}, "
        f"value_conv: {value_conv}, print_conv: {print_conv}, "
        f"groups: {codegen.compile_groups_field(tag.get('Groups'), stats)}, "
        f"edge: {_edge(tag, raw_id, ctx, stats)}, native: {_native_facts(tag)} }}",
        None,
    )


def _record_omission(omissions, module, table, raw, variant, tag, reason):
    omissions.append((module, table, raw, variant, tag, reason))


def gen_table(module, table, data, run_stats, verified_exprs, ctx, omissions):
    """Compile one keyed table. Variants are all-or-nothing per raw id."""
    if not is_keyed_directory_table((data.get("meta") or {})):
        return None
    stats = _stats()
    tags, variants = [], []
    for raw, tag in sorted(data.get("tags", {}).items()):
        raw_id = parse_raw_id(raw)
        if raw_id is None:
            stats["keyed_raw_id"] += 1
            continue
        if isinstance(tag, dict) and "_variants" in tag:
            trial = _stats()
            alternatives = []
            reason = None
            for alt in tag.get("_variants", ()):
                if not isinstance(alt, dict) or "_variants" in alt:
                    reason = "condition"
                    break
                src, reason = _tag_literal(alt, raw_id, trial, verified_exprs, ctx)
                if src is None:
                    break
                cond = conds.compile_cond(alt.get("Condition"))
                alternatives.append(f"({cond}, {src})")
            if reason is not None:
                stats["keyed_variant"] += 1
                for n, alt in enumerate(tag.get("_variants", ())):
                    _record_omission(omissions, module, table, f"{raw}#{n}", True, alt, reason)
                continue
            codegen._merge_stats(stats, trial)
            variants.append(f"KeyedVariantGroup {{ raw_id: {raw_id:#06x}, alternatives: &[{', '.join(alternatives)}] }}")
            continue
        if not isinstance(tag, dict):
            stats["keyed_row_shape"] += 1
            continue
        src, reason = _tag_literal(tag, raw_id, stats, verified_exprs, ctx)
        if src is None:
            _record_omission(omissions, module, table, raw, False, tag, reason)
        else:
            tags.append(src)

    blocked = sorted((key, value) for key, value in stats.items() if key.startswith("keyed_") and value)
    gate = "&[" + ", ".join(f'(\"{key}\", {value})' for key, value in blocked) + "]"
    meta = data.get("meta") or {}
    groups = meta.get("GROUPS") or {}
    group = lambda n, fallback: groups.get(str(n), fallback) if isinstance(groups.get(str(n), fallback), str) else fallback
    codegen._merge_stats(run_stats, stats)
    return (
        f"pub static KEYED_{re.sub(r'[^A-Za-z0-9]', '_', module + '_' + table).upper()}: KeyedDirectoryTable = "
        "KeyedDirectoryTable { "
        f'module: "{codegen.rust_str(module)}", table: "{codegen.rust_str(table)}", '
        f'group0: "{codegen.rust_str(group(0, module))}", group1: "{codegen.rust_str(group(1, ""))}", '
        f'group2: "{codegen.rust_str(group(2, "Other"))}", layout: KeyedLayout::Ciff10, '
        f"gate_a: GateA {{ blocked_by: {gate} }}, tags: &[{', '.join(tags)}], variants: &[{', '.join(variants)}] }};\n"
    )


def generate(doc, verified_exprs=None):
    """Return an opt-in keyed artifact and source-derived omission sidecar."""
    ctx, stats, omissions, chunks = Context(doc), _stats(), [], []
    for module, mod in sorted(doc.get("modules", {}).items()):
        for table, data in sorted(mod.get("tables", {}).items()):
            src = gen_table(module, table, data, stats, verified_exprs, ctx, omissions)
            if src is not None:
                chunks.append(src)
    rows = []
    for module, table, raw, variant, tag, reason in sorted(omissions, key=lambda x: x[:4]):
        name = tag.get("Name") if isinstance(tag, dict) and isinstance(tag.get("Name"), str) else None
        name_src = "None" if name is None else f'Some("{codegen.rust_str(name)}")'
        rows.append(
            f'    OmittedKeyedNativeRow {{ module: "{codegen.rust_str(module)}", table: "{codegen.rust_str(table)}", '
            f'raw_id: "{codegen.rust_str(raw)}", variant: {str(variant).lower()}, name: {name_src}, '
            f'native: {_native_facts(tag) if isinstance(tag, dict) else "KeyedNativeFacts { format: None, count: None, condition: None, groups: TagGroups::NONE, subdir: None }"}, '
            f'reasons: &["{reason}"] }},'
        )
    index = ["    &" + re.search(r"KEYED_[A-Z0-9_]+", chunk).group(0) + "," for chunk in chunks]
    prelude = "//! Generated keyed-directory facts; no reader is activated by this file.\nuse super::*;\n"
    return prelude + "".join(chunks) + (
        "pub static ALL_KEYED_TABLES: &[&KeyedDirectoryTable] = &[\n" + "\n".join(index) + "\n];\n"
        "pub static OMITTED_KEYED_NATIVE_ROWS: &[OmittedKeyedNativeRow] = &[\n" + "\n".join(rows) + "\n];\n"
    ), stats
