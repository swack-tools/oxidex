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
import directory_validation
import word_directory


PROCESS_CANON_RAW = "Image::ExifTool::CanonRaw::ProcessCanonRaw"
PROCESS_BINARY_DATA = "Image::ExifTool::ProcessBinaryData"
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


def _word_layout(meta, reader_contracts):
    """Return a generated layout only when the complete native body proves it.

    There is intentionally no processor-name, module, table, or tag-ID rule
    here.  A future captured body with this exact source shape takes the same
    path; absent or malformed provenance remains outside emitted tables.
    """
    process = (meta or {}).get("PROCESS_PROC")
    try:
        descriptor = word_directory.compile_word_directory(process, reader_contracts)
    except word_directory.WordDirectoryRefused:
        return None
    return "word", f"KeyedLayout::LengthPrefixedU16Pairs({descriptor.rust(codegen.rust_str)})"


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
        # ProcessCanonRaw passes its Format directly to ReadValue.  Unlike
        # ProcessBinaryData, it does not normalise `int16u[N]`/`string[N]`
        # first; ExifTool.pm:6290-6293 warns that those are unknown formats.
        # Refuse the whole row rather than importing the other reader's
        # convenient but wrong array interpretation.
        stats["keyed_format"] += 1
        return None, None
    if f == "string":
        # ProcessCanonRaw retains source Count (or derives it from value
        # size when Count is false); it never assigns `$count = $more`.
        # `Str(1)` gives its one-byte width to that future reader.
        return "Some(Fmt::Str(1))", f
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
        self.table_meta = {}
        self.validation_helpers = doc.get("subdirectory_validate_functions", {})
        self.reader_contracts = doc.get("native_reader_contracts", {})
        self.layouts = {}
        self.word_processor_refusals = set()
        self.word_target_ready = set()
        for module, mod in doc.get("modules", {}).items():
            for table, value in mod.get("tables", {}).items():
                meta = value.get("meta") or {}
                key = (module, table)
                self.processors[key] = processor_name(meta)
                self.table_meta[key] = meta
                if is_keyed_directory_table(meta):
                    self.layouts[key] = ("ciff", "KeyedLayout::Ciff10")
                elif (word := _word_layout(meta, self.reader_contracts)) is not None:
                    self.layouts[key] = word
                elif word_directory.is_candidate(meta.get("PROCESS_PROC")):
                    self.word_processor_refusals.add(key)

    def assess_word_target_gates(self, doc, verified_exprs, source_modules):
        """Mark only emitted word targets whose own Gate A has no blockers.

        A parent edge cannot be described as walkable just because its child
        processor body compiled: a row-level refusal in that child remains a
        source-visible blocker. Word rows refuse subdirectories, so this
        isolated prepass has no recursive target dependency or output effect.
        """
        for module in source_modules:
            for table, data in sorted(doc.get("modules", {}).get(module, {}).get("tables", {}).items()):
                layout = self.layouts.get((module, table))
                if layout is None or layout[0] != "word":
                    continue
                trial, omitted = _stats(), []
                if gen_table(module, table, data, trial, verified_exprs, self, omitted) is None:
                    continue
                if not any(key.startswith("keyed_") and value for key, value in trial.items()):
                    self.word_target_ready.add((module, table))

    def target_status(self, module, table):
        # Existing keyed parent edges may still target a generated flat binary
        # table.  A staged word layout adds a second source-authenticated
        # target shape; it must not withdraw the established binary path.
        layout = self.layouts.get((module, table))
        if layout is not None:
            if layout[0] != "word" or (module, table) in self.word_target_ready:
                return "ready"
            return "gate_a"
        return "ready" if self.processors.get((module, table)) == PROCESS_BINARY_DATA else "processor"


def _edge(tag, raw_id, ctx, stats):
    sd = tag.get("SubDirectory")
    if sd is None:
        return "None"
    if not isinstance(sd, dict):
        stats["keyed_subdirectory"] += 1
        return 'Some(KeyedEdge::BoundedValue { module: "", table: "", start: KeyedStart::Zero, validation: None, unwalked: &["subdirectory"] })'

    # ProcessCanonRaw recurses before ordinary SubDirectory handling when the
    # entry type is 0x28/0x30 and the value is not inline. `{}` only supplies
    # the native directory name; it is not a pointer edge.
    if not sd:
        if _same_table_directory(tag, raw_id):
            return "Some(KeyedEdge::SameTableDirectory)"
        stats["keyed_subdirectory"] += 1
        return 'Some(KeyedEdge::BoundedValue { module: "", table: "", start: KeyedStart::Zero, validation: None, unwalked: &["tag_table"] })'

    try:
        module, table = subdirs.parse_tag_table(sd.get("TagTable"))
    except subdirs.SubdirCompileError:
        stats["keyed_subdirectory"] += 1
        return 'Some(KeyedEdge::BoundedValue { module: "", table: "", start: KeyedStart::Zero, validation: None, unwalked: &["tag_table"] })'

    reasons = []
    # CanonRaw.pm applies Start only when truthy. The first reader supports
    # exactly the native default zero; a declared zero remains that default.
    if codegen.perl_truthy(sd.get("Start")):
        reasons.append("start")
    validation = "None"
    if sd.get("Validate") is not None:
        try:
            compiled = directory_validation.compile_validation(sd["Validate"], ctx.validation_helpers, ctx.reader_contracts)
            validation = compiled.rust(codegen.rust_str)
            if compiled.reader_contract_sha256 is None:
                reasons.append("validate_reader_contract")
        except directory_validation.ValidationRefused:
            reasons.append("validate")
    if sd.get("ProcessProc") is not None:
        reasons.append("process_proc")
    target_status = ctx.target_status(module, table)
    if target_status != "ready":
        reasons.append("target_gate_a" if target_status == "gate_a" else "target_processor")
    if reasons:
        stats["keyed_edge_unwalked"] += 1
    body = ", ".join(f'"{reason}"' for reason in reasons)
    return (
        "Some(KeyedEdge::BoundedValue { "
        f'module: "{codegen.rust_str(module)}", table: "{codegen.rust_str(table)}", '
        f"start: KeyedStart::Zero, validation: {validation}, unwalked: &[{body}] }})"
    )


def _raw_conv(tag):
    member = codegen.raw_conv_effect(tag.get("RawConv"))
    if member is not None:
        return f'Some(RawConvEffect::SetMember {{ member: "{codegen.rust_str(member)}" }})'
    if codegen.raw_conv_value_local(tag.get("RawConv")):
        return "Some(RawConvEffect::ValueLocal)"
    return "None"


def _reporting_flags(tag, table_meta):
    """Reuse the shared flag representation, after native flag expansion.

    Unknown is report policy, not a missing schema row. Keeping it lets a
    first-match variant group preserve its known alternatives and fallback.
    """
    priority = tag.get("Priority")
    if priority is None:
        priority = table_meta.get("PRIORITY")
    if priority is not None:
        if isinstance(priority, (dict, list, bool)) or re.fullmatch(r"[+-]?\d+", str(priority).strip()) is None:
            raise ValueError(f"unrepresentable keyed Priority: {priority!r}")
        priority = int(str(priority).strip())
        if not -(1 << 63) <= priority < (1 << 63):
            raise ValueError("keyed Priority is outside the shared signed domain")
    avoid = table_meta.get("AVOID")
    if avoid is None:
        avoid = tag.get("Avoid")
    return codegen.ifd_flags_literal(
        codegen.perl_truthy(tag.get("Unknown")),
        codegen.perl_truthy(tag.get("Binary")),
        codegen.perl_truthy(tag.get("List")),
        codegen.perl_truthy(tag.get("Protected")),
        codegen.perl_truthy(avoid), priority,
    )


def _expanded_tag(tag):
    # SetupTagTable calls ExpandFlags only when Flags is Perl-truthy.
    if not codegen.perl_truthy(tag.get("Flags")):
        return tag
    stats = _stats()
    expanded = codegen.expand_flags(tag, stats)
    if stats["ifd_flags_unreadable"]:
        raise ValueError("unreadable keyed Flags; refusing an incomplete reporting policy")
    return expanded


def _native_facts(tag, table_meta=None):
    """Expanded source facts retained beside executable keyed schema fields.

    They authenticate omission rows and detect source-condition changes that
    cannot be reconstructed from a compiled `Cond`.  The reader must use the
    typed fields, not this audit copy. Reporting policy includes native table
    precedence; callers expand Flags exactly once before entering here.
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
        f"condition: {option(tag.get('Condition'))}, groups: {groups}, subdir: {subdir}, "
        f"flags: {_reporting_flags(tag, table_meta or {})} }}"
    )


def _tag_literal(tag, raw_id, stats, verified_exprs, ctx, table_meta, layout):
    name = tag.get("Name")
    if not isinstance(name, str) or not name:
        stats["keyed_name"] += 1
        return None, "raw_id"
    condition = tag.get("Condition")
    condition_src = "None"
    if condition is not None:
        condition_src = conds.compile_cond(condition)
        if condition_src is None or conds.needs_initial_get_tag_info_context(condition):
            stats["keyed_condition"] += 1
            return None, "condition"
        condition_src = f"Some({condition_src})"
    kind, _layout_src = layout
    default = _type_default(raw_id) if kind == "ciff" else "int8u"
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
    # A word-directory processor supplies the exact HandleTag value shape.
    # A source row may repeat it, but cannot silently override its already
    # authenticated format/count/size operands.
    if kind == "word" and ((tag.get("Format") is not None and format_name != "int8u")
                           or (tag.get("Count") is not None and count_src != "Some(1)")):
        stats["keyed_word_value_shape"] += 1
        return None, "format" if format_name != "int8u" else "count"
    if kind == "word" and tag.get("SubDirectory") is not None:
        stats["keyed_word_subdirectory"] += 1
        return None, "subdirectory"
    # ProcessCanonRaw calls ReadValue then FoundTag directly; it never
    # applies ProcessBinaryData's Mask/BitShift transformation.
    domain = codegen.value_domain(format_name, count_for_domain)
    value_conv, modeled_value = codegen.value_conv_for(tag, stats, domain, verified_exprs)
    print_conv, refused_print = codegen.conv_for(
        tag, stats, codegen.print_conv_input_domain(tag, domain, modeled_value), verified_exprs
    )
    return (
        "KeyedTag { "
        f'raw_id: {raw_id:#06x}, name: "{codegen.rust_str(name)}", '
        f"format: {fmt_src}, count: {count_src}, flags: {_reporting_flags(tag, table_meta)}, condition: {condition_src}, "
        f"raw_conv: {_raw_conv(tag)}, "
        f"omitted: {codegen.omitted_for(tag, stats, False, modeled_value, refused_print)}, "
        f"value_conv: {value_conv}, print_conv: {print_conv}, "
        f"groups: {codegen.compile_groups_field(tag.get('Groups'), stats)}, "
        f"edge: {_edge(tag, raw_id, ctx, stats) if kind == 'ciff' else 'None'}, native: {_native_facts(tag, table_meta)} }}",
        None,
    )


def _record_omission(omissions, module, table, raw, variant, tag, reason):
    omissions.append((module, table, raw, variant, tag, reason))


def gen_table(module, table, data, run_stats, verified_exprs, ctx, omissions):
    """Compile one keyed table. Variants are all-or-nothing per raw id."""
    layout = ctx.layouts.get((module, table))
    if layout is None:
        if (module, table) in ctx.word_processor_refusals:
            run_stats["keyed_word_processor"] += 1
        return None
    stats = _stats()
    table_meta = data.get("meta") or {}
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
            source_alts = [_expanded_tag(alt) if isinstance(alt, dict) else alt
                           for alt in tag.get("_variants", ())]
            for alt in source_alts:
                if not isinstance(alt, dict) or "_variants" in alt:
                    reason = "condition"
                    break
                src, reason = _tag_literal(alt, raw_id, trial, verified_exprs, ctx, table_meta, layout)
                if src is None:
                    break
                cond = conds.compile_cond(alt.get("Condition"))
                alternatives.append(f"({cond}, {src})")
            if reason is not None:
                stats["keyed_variant"] += 1
                for n, alt in enumerate(source_alts):
                    _record_omission(omissions, module, table, f"{raw}#{n}", True, alt, reason)
                continue
            codegen._merge_stats(stats, trial)
            variants.append(f"KeyedVariantGroup {{ raw_id: {raw_id:#06x}, alternatives: &[{', '.join(alternatives)}] }}")
            continue
        if not isinstance(tag, dict):
            stats["keyed_row_shape"] += 1
            continue
        tag = _expanded_tag(tag)
        src, reason = _tag_literal(tag, raw_id, stats, verified_exprs, ctx, table_meta, layout)
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
        f'group2: "{codegen.rust_str(group(2, "Other"))}", layout: {layout[1]}, '
        f"gate_a: GateA {{ blocked_by: {gate} }}, tags: &[{', '.join(tags)}], variants: &[{', '.join(variants)}] }};\n"
    )


def generate(doc, verified_exprs=None, modules=None):
    """Return opt-in keyed source for the selected native module scope.

    `codegen.py` always asks for this source before it freezes the shared
    ExprId enum, even if ``--keyed-out`` does not write the optional artifact.
    Matching the binary/IFD module scope keeps that enum deterministic across
    output flags and prevents a keyed-only expression from dangling.
    """
    source_modules = sorted(doc.get("modules", {})) if modules is None else modules
    ctx, stats, omissions, chunks = Context(doc), _stats(), [], []
    ctx.assess_word_target_gates(doc, verified_exprs, source_modules)
    for module in source_modules:
        mod = doc.get("modules", {}).get(module)
        if mod is None:
            continue
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
            f'native: {_native_facts(tag, ctx.table_meta[(module, table)]) if isinstance(tag, dict) else "KeyedNativeFacts { format: None, count: None, condition: None, groups: TagGroups::NONE, subdir: None, flags: IfdFlags::NONE }"}, '
            f'reasons: &["{reason}"] }},'
        )
    index = ["    &" + re.search(r"KEYED_[A-Z0-9_]+", chunk).group(0) + "," for chunk in chunks]
    prelude = "//! Generated keyed-directory facts; no reader is activated by this file.\nuse super::*;\n"
    return prelude + "".join(chunks) + (
        "pub static ALL_KEYED_TABLES: &[&KeyedDirectoryTable] = &[\n" + "\n".join(index) + "\n];\n"
        "pub static OMITTED_KEYED_NATIVE_ROWS: &[OmittedKeyedNativeRow] = &[\n" + "\n".join(rows) + "\n];\n"
    ), stats
