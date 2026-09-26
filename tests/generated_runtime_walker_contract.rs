//! Task 17 contract: every generated-table walker hands its selected, read
//! value to ONE shared FoundTag conversion tail (`pipeline::execute`) and
//! gets back the same occurrence shape (`Emitted`).
//!
//! Three layers, each a different instrument:
//!
//! * **Inventory** -- `docs/reference/generated-runtime-walker-inventory.json`
//!   names every enabled walker, its ExifTool callback, acquisition kind,
//!   callers, carrier and detected-versus-parsed state, plus the per-walker
//!   policy row the shared stage takes. The tests below hold the document to
//!   the source it describes.
//! * **Structure** -- each engine file routes through `pipeline::execute` and
//!   no longer carries its own ValueConv/PrintConv/Binary-placeholder stage;
//!   the `attribution::Token` guard stays at each engine's emit site, because
//!   `tools/exiftool-tables/genshare/attribute.py` scans only the engine files.
//! * **Behaviour** -- hand-built tables driven through each walker's PUBLIC
//!   entry point (`process_binary_data`, `process_exif`,
//!   `process_keyed_directory`, `process_serial_directory`). These pin the
//!   pre-consolidation outputs exactly, including the deliberate per-walker
//!   differences (list form, member-failure policy, group-1 source), so the
//!   refactor is observed to change none of them. Numeric IFD, keyed raw-id,
//!   serial-index and binary-index identities stay distinct.
//!
//! Route controls at the bottom read real `t/images` carriers through
//! `read_metadata` and require metadata beyond the FileType identity tags.

use std::collections::HashMap;

use oxidex::core::TagValue;
use oxidex::core::operations::read_metadata;
use oxidex::exiftool_tables::session::Session;
use oxidex::exiftool_tables::{
    BinaryTable, Ctx, Dir, Emitted, Field, Fmt, GateA, IfdDir, IfdFlags, IfdTable, IfdTag,
    KeyedBlock, KeyedDirectoryTable, KeyedEmissionSink, KeyedLayout, KeyedNativeFacts, KeyedScope,
    KeyedTag, KeyedVariantGroup, MemberValue, Omitted, PrintConv, RawConvEffect, SerialCount,
    SerialDir, SerialEmissionSink, SerialEntry, SerialFormat, SerialPrintConv,
    SerialProcessorFacts, SerialTable, SerialTag, TagGroups, VariantGroup, process_binary_data,
    process_exif, process_keyed_directory, process_serial_directory,
};
use oxidex::io::ByteOrder;
use oxidex_tags::TagId;
use serde_json::Value;

#[path = "common/fixtures.rs"]
mod fixtures;

const REPO: &str = env!("CARGO_MANIFEST_DIR");
const INVENTORY: &str = "docs/reference/generated-runtime-walker-inventory.json";
const PIPELINE: &str = "src/exiftool_tables/pipeline.rs";

/// `(walker id, engine file, attribution token the emit site keeps)`.
const WALKERS: [(&str, &str, &str); 4] = [
    ("binary", "src/exiftool_tables/engine.rs", "Engine"),
    ("ifd", "src/exiftool_tables/ifd_engine.rs", "Engine"),
    ("keyed", "src/exiftool_tables/keyed_engine.rs", "Keyed"),
    ("serial", "src/exiftool_tables/serial_engine.rs", "Serial"),
];

fn source(relative: &str) -> String {
    std::fs::read_to_string(format!("{REPO}/{relative}"))
        .unwrap_or_else(|error| panic!("read {relative}: {error}"))
}

/// The production half of a Rust file: everything before its first
/// `#[cfg(test)]` test module, so a unit test that spells a stage literal
/// does not count as a duplicate conversion stage.
fn production(relative: &str) -> String {
    let text = source(relative);
    let lines: Vec<&str> = text.lines().collect();
    let mut end = lines.len();
    for (index, line) in lines.iter().enumerate() {
        if line.trim() == "#[cfg(test)]"
            && lines
                .get(index + 1)
                .is_some_and(|next| next.trim_start().starts_with("mod tests"))
        {
            end = index;
            break;
        }
    }
    lines[..end].join("\n")
}

fn inventory() -> Value {
    serde_json::from_str(&source(INVENTORY)).expect("walker inventory is JSON")
}

// ---------------------------------------------------------------------------
// Inventory
// ---------------------------------------------------------------------------

#[test]
fn inventory_names_every_enabled_walker_with_its_contract() {
    let inventory = inventory();
    assert_eq!(
        inventory["schema"],
        "oxidex-generated-runtime-walker-inventory/v1"
    );
    assert_eq!(inventory["shared_entry_point"]["path"], PIPELINE);
    assert_eq!(
        inventory["shared_entry_point"]["symbol"],
        "pipeline::execute"
    );
    let walkers = inventory["walkers"].as_array().expect("walkers array");
    let ids: Vec<&str> = walkers
        .iter()
        .map(|walker| walker["id"].as_str().expect("walker id"))
        .collect();
    assert_eq!(ids, ["binary", "ifd", "keyed", "serial"]);
    for (walker, (id, file, token)) in walkers.iter().zip(WALKERS) {
        assert_eq!(walker["id"], id);
        assert_eq!(walker["engine_file"], file, "{id}");
        assert_eq!(walker["attribution_token"], token, "{id}");
        for key in [
            "exiftool_callback",
            "acquisition",
            "identity",
            "current_conversion_path",
            "target_conversion_path",
            "real_carrier_fixture",
            "detected_versus_parsed",
        ] {
            assert!(
                walker[key].as_str().is_some_and(|text| !text.is_empty()),
                "{id}: {key} must be stated"
            );
        }
        assert!(
            walker["target_conversion_path"]
                .as_str()
                .is_some_and(|path| path.starts_with("pipeline::execute")),
            "{id}: target is the shared stage"
        );
        let text = production(file);
        for entry in walker["entry_points"].as_array().expect("entry points") {
            let entry = entry.as_str().expect("entry point name");
            assert!(
                text.contains(&format!("fn {entry}(")),
                "{id}: entry point {entry} exists in {file}"
            );
        }
        for key in [
            "member_value",
            "unrepresentable_member",
            "set_member_clears_raw_conv_omission",
            "unmodeled_raw_conv",
            "binary_placeholder",
            "unconverted_scalar_form",
            "print_conv",
            "group1",
            "priority",
            "rational",
        ] {
            assert!(
                !walker["adapter_policy"][key].is_null(),
                "{id}: policy row {key} must be stated"
            );
        }
    }
}

/// The keyed walker has no production caller, and the Task 8 attribution
/// instrument refuses to measure if one appears without controller review.
/// The inventory must say so rather than claim a parsed route.
#[test]
fn inventory_records_keyed_as_detected_not_parsed() {
    let inventory = inventory();
    let keyed = &inventory["walkers"][2];
    assert_eq!(keyed["id"], "keyed");
    assert_eq!(keyed["production_callers"], Value::Array(Vec::new()));
    assert!(
        keyed["detected_versus_parsed"]
            .as_str()
            .is_some_and(|state| state.starts_with("detected-not-parsed"))
    );
    for relative in [
        "src/main.rs",
        "src/core/operations.rs",
        "src/core/file_metadata.rs",
        "src/exiftool_tables/mod.rs",
        "src/exiftool_tables/engine.rs",
        "src/exiftool_tables/ifd_engine.rs",
        "src/exiftool_tables/serial_engine.rs",
        PIPELINE,
    ] {
        if std::path::Path::new(&format!("{REPO}/{relative}")).is_file() {
            assert!(
                !production(relative).contains("process_keyed_directory("),
                "{relative} must not add a keyed production caller"
            );
        }
    }
}

/// No decrypted buffer reaches a generated walker: the encrypted carriers
/// feed hand readers. Pin that absence instead of inventing a fixture.
#[test]
fn no_encrypted_carrier_reaches_a_generated_walker() {
    let inventory = inventory();
    assert_eq!(inventory["encrypted_walker"]["status"], "none");
    for relative in [
        "src/parsers/tiff/makernotes/nikon/encrypted.rs",
        "src/parsers/tiff/makernotes/sony/enciphered.rs",
    ] {
        let text = source(relative);
        for entry in [
            "process_binary_data",
            "process_exif",
            "process_keyed_directory",
            "process_serial_directory",
            "pipeline::execute",
        ] {
            assert!(
                !text.contains(entry),
                "{relative} now reaches {entry}: record the encrypted walker"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Structure
// ---------------------------------------------------------------------------

fn assert_routes_through_pipeline(id: &str) {
    let (_, file, token) = WALKERS
        .iter()
        .find(|(walker, _, _)| *walker == id)
        .copied()
        .expect("known walker");
    assert!(
        std::path::Path::new(&format!("{REPO}/{PIPELINE}")).is_file(),
        "the shared entry point {PIPELINE} is absent"
    );
    let text = production(file);
    assert!(
        text.contains("pipeline::execute("),
        "{file} must hand its FoundTag tail to pipeline::execute"
    );
    for duplicate in ["apply_value_conv(", "runtime::render("] {
        assert!(
            !text.contains(duplicate),
            "{file} still carries its own conversion stage ({duplicate})"
        );
    }
    assert!(
        !text.contains("(Binary data {len}") && !text.contains("(Binary data {length}"),
        "{file} still builds its own Binary placeholder"
    );
    assert!(
        text.contains(&format!("attribution::Token::{token}")),
        "{file} must keep its {token} attribution guard at the emit site"
    );
}

#[test]
fn binary_walker_routes_through_the_shared_pipeline() {
    assert_routes_through_pipeline("binary");
}

#[test]
fn ifd_walker_routes_through_the_shared_pipeline() {
    assert_routes_through_pipeline("ifd");
}

#[test]
fn keyed_walker_routes_through_the_shared_pipeline() {
    assert_routes_through_pipeline("keyed");
}

#[test]
fn serial_walker_routes_through_the_shared_pipeline() {
    assert_routes_through_pipeline("serial");
}

/// The shared stage owns the conversions and never the attribution guard:
/// the Task 8 census finds guards only in the engine files.
#[test]
fn pipeline_owns_the_stages_but_not_the_attribution_guard() {
    let text = production(PIPELINE);
    for stage in [
        "pub fn execute(",
        "apply_value_conv(",
        "runtime::render(",
        "(Binary data {",
    ] {
        assert!(text.contains(stage), "{PIPELINE} must own {stage}");
    }
    assert!(
        !text.contains("attribution::"),
        "{PIPELINE} must not carry an attribution guard"
    );
}

// ---------------------------------------------------------------------------
// Behaviour: hand tables through each public entry point
// ---------------------------------------------------------------------------

const ANSWER_MAP: &[(i64, &str)] = &[(1, "One"), (2, "Two")];
const ANSWER_PRINT: PrintConv = PrintConv::IntEnum(ANSWER_MAP);
const NO_GATE: GateA = GateA { blocked_by: &[] };
const SET_STATE: Option<RawConvEffect> = Some(RawConvEffect::SetMember { member: "State" });
const RAW_CONV_OMITTED: Omitted = Omitted {
    raw_conv: true,
    ..Omitted::NONE
};

/// What one fixture observes of a row. Everything but the table identity,
/// which differs per walker by construction.
#[derive(Debug, PartialEq)]
struct Shape {
    group0: &'static str,
    group1: &'static str,
    group2: &'static str,
    name: &'static str,
    source_id: TagId,
    stored: TagValue,
    value: TagValue,
    value_conv: Option<TagValue>,
    low_priority: bool,
    avoid: bool,
    rational: Option<(i64, i64)>,
    is_list: bool,
}

fn shape(row: &Emitted) -> Shape {
    Shape {
        group0: row.group0,
        group1: row.group1,
        group2: row.group2,
        name: row.name,
        source_id: row.source_id.clone(),
        stored: row.stored.clone(),
        value: row.value.clone(),
        value_conv: row.value_conv.clone(),
        low_priority: row.low_priority,
        avoid: row.avoid,
        rational: row.rational,
        is_list: row.is_list,
    }
}

fn answer_shape(source_id: TagId) -> Shape {
    Shape {
        group0: "Contract",
        group1: "Contract1",
        group2: "Other",
        name: "Answer",
        source_id,
        stored: TagValue::Integer(2),
        value: TagValue::String("Two".into()),
        value_conv: Some(TagValue::Integer(2)),
        low_priority: false,
        avoid: false,
        rational: None,
        is_list: false,
    }
}

// -- binary ------------------------------------------------------------------

const fn binary_field(index: i64, name: &'static str) -> Field {
    Field {
        index,
        sub: None,
        name,
        format: None,
        count: 1,
        mask: None,
        condition: None,
        raw_conv: None,
        omitted: Omitted::NONE,
        value_conv: None,
        print_conv: PrintConv::None,
        subdir: None,
        hook: &[],
        groups: TagGroups::NONE,
    }
}

const NO_BINARY_VARIANTS: &[VariantGroup] = &[];

const fn binary_table(table: &'static str, fields: &'static [Field]) -> BinaryTable {
    BinaryTable {
        module: "Contract",
        table,
        group0: "Contract",
        group1: "Contract1",
        group2: "Other",
        first_entry: 0,
        default_format: Fmt::Int16u,
        offsets_sound_until: None,
        priority: None,
        gate_a: NO_GATE,
        fields,
        variants: NO_BINARY_VARIANTS,
    }
}

fn walk_binary(
    table: &'static BinaryTable,
    data: &[u8],
) -> (Vec<Emitted>, HashMap<&'static str, MemberValue>) {
    let mut members = HashMap::new();
    let mut out = Vec::new();
    {
        let mut ctx = Ctx::new(&mut members);
        process_binary_data(table, Dir::whole(data, ByteOrder::Big), &mut ctx, &mut out);
    }
    (out, members)
}

// -- IFD ---------------------------------------------------------------------

const fn ifd_tag(id: u16, name: &'static str) -> IfdTag {
    IfdTag {
        id,
        name,
        format: None,
        count: None,
        writable: None,
        groups: TagGroups::NONE,
        flags: IfdFlags::NONE,
        condition: None,
        omitted: Omitted::NONE,
        raw_conv: None,
        value_conv: None,
        print_conv: PrintConv::None,
        subdir: None,
    }
}

const fn ifd_table(table: &'static str, tags: &'static [IfdTag]) -> IfdTable {
    IfdTable {
        module: "Contract",
        table,
        group0: "Contract",
        group1: "Contract1",
        group2: "Other",
        set_group1: None,
        priority: None,
        gate_a: NO_GATE,
        tags,
        variants: &[],
    }
}

/// `(tag, TIFF type, count, inline value bytes)` entries of one big-endian
/// IFD at offset 8 (after an 8-byte pad standing in for the TIFF header).
fn ifd_bytes(entries: &[(u16, u16, u32, &[u8])]) -> Vec<u8> {
    let mut data = vec![0u8; 8];
    data.extend_from_slice(&u16::try_from(entries.len()).unwrap().to_be_bytes());
    for (tag, kind, count, value) in entries {
        data.extend_from_slice(&tag.to_be_bytes());
        data.extend_from_slice(&kind.to_be_bytes());
        data.extend_from_slice(&count.to_be_bytes());
        let mut inline = value.to_vec();
        assert!(inline.len() <= 4, "fixture values stay inline");
        inline.resize(4, 0);
        data.extend_from_slice(&inline);
    }
    data.extend_from_slice(&0u32.to_be_bytes());
    data
}

fn walk_ifd(
    table: &'static IfdTable,
    data: &[u8],
    group1: Option<&'static str>,
) -> (Vec<Emitted>, HashMap<&'static str, MemberValue>, Session) {
    let mut members = HashMap::new();
    let mut session = Session::new();
    let mut out = Vec::new();
    {
        let mut ctx = Ctx::new(&mut members);
        process_exif(
            table,
            IfdDir {
                data,
                data_domain: 0,
                ifd_start: 8,
                base: Some(0),
                byte_order: ByteOrder::Big,
                group1,
            },
            &mut session,
            &mut ctx,
            &mut out,
        );
    }
    (out, members, session)
}

// -- keyed -------------------------------------------------------------------

const KEYED_FACTS: KeyedNativeFacts = KeyedNativeFacts {
    format: None,
    count: None,
    condition: None,
    groups: TagGroups::NONE,
    subdir: None,
    flags: IfdFlags::NONE,
};
const NO_KEYED_VARIANTS: &[KeyedVariantGroup] = &[];

const fn keyed_tag(raw_id: u16, name: &'static str) -> KeyedTag {
    KeyedTag {
        raw_id,
        name,
        format: Some(Fmt::Int16u),
        count: None,
        flags: IfdFlags::NONE,
        condition: None,
        raw_conv: None,
        omitted: Omitted::NONE,
        value_conv: None,
        print_conv: PrintConv::None,
        groups: TagGroups::NONE,
        edge: None,
        native: KEYED_FACTS,
    }
}

const fn keyed_table(table: &'static str, tags: &'static [KeyedTag]) -> KeyedDirectoryTable {
    KeyedDirectoryTable {
        module: "Contract",
        table,
        group0: "Contract",
        group1: "Contract1",
        group2: "Other",
        layout: KeyedLayout::Ciff10,
        gate_a: NO_GATE,
        tags,
        variants: NO_KEYED_VARIANTS,
    }
}

/// A big-endian CIFF block whose entries are all inline (`0x4000`): the
/// type bits `0x1000` select int16u, and the raw id is the low 14 bits.
fn ciff_inline(entries: &[(u16, &[u8])]) -> Vec<u8> {
    let mut data = Vec::new();
    data.extend_from_slice(&u16::try_from(entries.len()).unwrap().to_be_bytes());
    for (raw_id, value) in entries {
        data.extend_from_slice(&(0x4000 | raw_id).to_be_bytes());
        let mut inline = value.to_vec();
        inline.resize(8, 0);
        data.extend_from_slice(&inline);
    }
    data.extend_from_slice(&0u32.to_be_bytes());
    // The directory starts at offset 0 of the block.
    data.extend_from_slice(&0u32.to_be_bytes());
    data
}

#[derive(Default)]
struct KeyedSink {
    rows: Vec<Emitted>,
}

impl KeyedEmissionSink for KeyedSink {
    fn emit(&mut self, row: Emitted) {
        self.rows.push(row);
    }

    fn keyed_enabled(&self, _table: &'static KeyedDirectoryTable) -> bool {
        true
    }
}

fn walk_keyed(
    table: &'static KeyedDirectoryTable,
    data: &[u8],
    scope: KeyedScope,
    max_scalar_bytes: usize,
) -> (Vec<Emitted>, usize, usize) {
    let mut members = HashMap::new();
    let mut ctx = Ctx::new(&mut members);
    let mut sink = KeyedSink::default();
    let mut block = KeyedBlock::new(data, ByteOrder::Big, scope);
    block.max_scalar_bytes = max_scalar_bytes;
    let result = process_keyed_directory(table, block, &mut ctx, &mut sink);
    (sink.rows, result.omitted, result.large_scalar)
}

const NATIVE_SCOPE: KeyedScope = KeyedScope {
    group1_override: None,
};

// -- serial ------------------------------------------------------------------

const SERIAL_FACTS: SerialProcessorFacts = SerialProcessorFacts {
    name: "Contract::ProcessSerialData",
    source_file: "contract",
    source_sha256: "contract",
    source_body_sha256: "contract",
};

const fn serial_tag(name: &'static str, count: usize) -> SerialTag {
    SerialTag {
        name,
        format: SerialFormat {
            format: Fmt::Int16u,
            count: SerialCount::Fixed { value: count },
        },
        condition: None,
        flags: IfdFlags::NONE,
        raw_conv: None,
        omitted: Omitted::NONE,
        value_conv: None,
        print_conv: SerialPrintConv::None,
        groups: TagGroups::NONE,
    }
}

const fn serial_table(table: &'static str, entries: &'static [SerialEntry]) -> SerialTable {
    SerialTable {
        module: "Contract",
        table,
        group0: "Contract",
        group1: "Contract1",
        group2: "Other",
        default_format: Fmt::Int16u,
        processor: SERIAL_FACTS,
        gate_a: NO_GATE,
        entries,
    }
}

struct SerialSink {
    rows: Vec<Emitted>,
    unknown: bool,
}

impl SerialEmissionSink for SerialSink {
    fn emit(&mut self, row: Emitted) {
        self.rows.push(row);
    }

    fn serial_enabled(&self, _table: &'static SerialTable) -> bool {
        true
    }

    fn unknown_enabled(&self) -> bool {
        self.unknown
    }
}

fn walk_serial(
    table: &'static SerialTable,
    data: &[u8],
    unknown: bool,
) -> (Vec<Emitted>, usize, bool) {
    let mut members = HashMap::new();
    let mut ctx = Ctx::new(&mut members);
    let mut sink = SerialSink {
        rows: Vec::new(),
        unknown,
    };
    let result = process_serial_directory(
        table,
        SerialDir {
            data,
            dir_start: 0,
            dir_len: data.len(),
            base: 0,
            data_pos: 0,
            byte_order: ByteOrder::Big,
        },
        &mut ctx,
        &mut sink,
    );
    (sink.rows, result.omitted, result.tainted)
}

// -- the shared answer ---------------------------------------------------------

static BINARY_ANSWER: BinaryTable = binary_table(
    "Answer",
    &[Field {
        print_conv: ANSWER_PRINT,
        ..binary_field(0, "Answer")
    }],
);
static IFD_ANSWER: IfdTable = ifd_table(
    "Answer",
    &[IfdTag {
        print_conv: ANSWER_PRINT,
        ..ifd_tag(0x0001, "Answer")
    }],
);
static KEYED_ANSWER: KeyedDirectoryTable = keyed_table(
    "Answer",
    &[KeyedTag {
        print_conv: ANSWER_PRINT,
        ..keyed_tag(0x1001, "Answer")
    }],
);
static SERIAL_ANSWER: SerialTable = serial_table(
    "Answer",
    &[SerialEntry {
        serial_index: 0,
        alternatives: &[SerialTag {
            print_conv: SerialPrintConv::Shared(ANSWER_PRINT),
            ..serial_tag("Answer", 1)
        }],
    }],
);

/// One field, one PrintConv, four acquisitions: every walker reports the
/// same occurrence shape and keeps its own stable identity kind.
#[test]
fn every_walker_reports_the_same_occurrence_shape() {
    let (binary, _) = walk_binary(&BINARY_ANSWER, &[0, 2]);
    let (ifd, _, _) = walk_ifd(&IFD_ANSWER, &ifd_bytes(&[(0x0001, 3, 1, &[0, 2])]), None);
    let (keyed, _, _) = walk_keyed(
        &KEYED_ANSWER,
        &ciff_inline(&[(0x1001, &[0, 2])]),
        NATIVE_SCOPE,
        512,
    );
    let (serial, _, _) = walk_serial(&SERIAL_ANSWER, &[0, 2], false);
    let rows = [
        ("binary", binary, TagId::Numeric(0)),
        ("ifd", ifd, TagId::Numeric(0x0001)),
        ("keyed", keyed, TagId::Numeric(0x1001)),
        ("serial", serial, TagId::Numeric(0)),
    ];
    for (walker, rows, identity) in rows {
        assert_eq!(rows.len(), 1, "{walker}: one row");
        assert_eq!(rows[0].module, "Contract", "{walker}");
        assert_eq!(rows[0].table, "Answer", "{walker}");
        assert_eq!(shape(&rows[0]), answer_shape(identity), "{walker}");
    }
}

/// A fixed-count list: the three walkers whose schema carries `List` keep
/// the typed array; a ProcessBinaryData field has no `List` flag and keeps
/// ExifTool's one space-joined string (ExifTool.pm:6312).
static BINARY_PAIR: BinaryTable = binary_table(
    "Pair",
    &[Field {
        count: 2,
        ..binary_field(0, "Pair")
    }],
);
static IFD_PAIR: IfdTable = ifd_table(
    "Pair",
    &[IfdTag {
        flags: IfdFlags {
            list: true,
            ..IfdFlags::NONE
        },
        ..ifd_tag(0x0002, "Pair")
    }],
);
static KEYED_PAIR: KeyedDirectoryTable = keyed_table(
    "Pair",
    &[KeyedTag {
        count: Some(2),
        flags: IfdFlags {
            list: true,
            ..IfdFlags::NONE
        },
        ..keyed_tag(0x1002, "Pair")
    }],
);
static SERIAL_PAIR: SerialTable = serial_table(
    "Pair",
    &[SerialEntry {
        serial_index: 0,
        alternatives: &[SerialTag {
            flags: IfdFlags {
                list: true,
                ..IfdFlags::NONE
            },
            ..serial_tag("Pair", 2)
        }],
    }],
);

#[test]
fn list_form_is_a_per_walker_policy() {
    let typed = TagValue::Array(vec![TagValue::Integer(1), TagValue::Integer(2)]);
    let (binary, _) = walk_binary(&BINARY_PAIR, &[0, 1, 0, 2]);
    let (ifd, _, _) = walk_ifd(
        &IFD_PAIR,
        &ifd_bytes(&[(0x0002, 3, 2, &[0, 1, 0, 2])]),
        None,
    );
    let (keyed, _, _) = walk_keyed(
        &KEYED_PAIR,
        &ciff_inline(&[(0x1002, &[0, 1, 0, 2])]),
        NATIVE_SCOPE,
        512,
    );
    let (serial, _, _) = walk_serial(&SERIAL_PAIR, &[0, 1, 0, 2], false);
    assert_eq!(binary.len(), 1);
    assert_eq!(binary[0].value, TagValue::String("1 2".into()));
    assert!(!binary[0].is_list);
    for (walker, rows) in [("ifd", ifd), ("keyed", keyed), ("serial", serial)] {
        assert_eq!(rows.len(), 1, "{walker}");
        assert_eq!(rows[0].value, typed, "{walker}");
        assert_eq!(rows[0].value_conv, None, "{walker}");
        assert!(rows[0].is_list, "{walker}");
    }
}

/// `Binary` with no ValueConv reports ExifTool's placeholder with the Perl
/// scalar's length (exiftool:3987) in every walker whose schema has the flag.
static IFD_BLOB: IfdTable = ifd_table(
    "Blob",
    &[IfdTag {
        flags: IfdFlags {
            binary: true,
            ..IfdFlags::NONE
        },
        print_conv: ANSWER_PRINT,
        ..ifd_tag(0x0003, "Blob")
    }],
);
static KEYED_BLOB: KeyedDirectoryTable = keyed_table(
    "Blob",
    &[KeyedTag {
        flags: IfdFlags {
            binary: true,
            ..IfdFlags::NONE
        },
        print_conv: ANSWER_PRINT,
        ..keyed_tag(0x1003, "Blob")
    }],
);
static SERIAL_BLOB: SerialTable = serial_table(
    "Blob",
    &[SerialEntry {
        serial_index: 0,
        alternatives: &[SerialTag {
            flags: IfdFlags {
                binary: true,
                ..IfdFlags::NONE
            },
            print_conv: SerialPrintConv::Shared(ANSWER_PRINT),
            ..serial_tag("Blob", 1)
        }],
    }],
);

#[test]
fn binary_placeholder_replaces_both_conversions() {
    let placeholder = TagValue::String("(Binary data 1 bytes, use -b option to extract)".into());
    let (ifd, _, _) = walk_ifd(&IFD_BLOB, &ifd_bytes(&[(0x0003, 3, 1, &[0, 2])]), None);
    let (keyed, _, _) = walk_keyed(
        &KEYED_BLOB,
        &ciff_inline(&[(0x1003, &[0, 2])]),
        NATIVE_SCOPE,
        512,
    );
    let (serial, _, _) = walk_serial(&SERIAL_BLOB, &[0, 2], false);
    for (walker, rows) in [("ifd", ifd), ("keyed", keyed), ("serial", serial)] {
        assert_eq!(rows.len(), 1, "{walker}");
        assert_eq!(rows[0].value, placeholder, "{walker}");
        assert_eq!(rows[0].value_conv, None, "{walker}");
        assert_eq!(rows[0].stored, TagValue::Integer(2), "{walker}");
    }
}

/// Family-1 group: each walker's own source for it.
static BINARY_GROUPED: BinaryTable = binary_table(
    "Grouped",
    &[Field {
        groups: TagGroups {
            g0: None,
            g1: Some("FieldGroup"),
            g2: None,
        },
        ..binary_field(0, "Grouped")
    }],
);
static IFD_SET_GROUP1: IfdTable = IfdTable {
    set_group1: Some("1"),
    ..ifd_table("SetGroup1", &[ifd_tag(0x0004, "Grouped")])
};
static SERIAL_GROUPED: SerialTable = serial_table(
    "Grouped",
    &[SerialEntry {
        serial_index: 0,
        alternatives: &[SerialTag {
            groups: TagGroups {
                g0: None,
                g1: Some("TagGroup"),
                g2: None,
            },
            ..serial_tag("Grouped", 1)
        }],
    }],
);

#[test]
fn group1_source_is_a_per_walker_policy() {
    let (binary, _) = walk_binary(&BINARY_GROUPED, &[0, 2]);
    assert_eq!(binary[0].group1, "FieldGroup");

    let data = ifd_bytes(&[(0x0004, 3, 1, &[0, 2])]);
    let (withheld, _, _) = walk_ifd(&IFD_SET_GROUP1, &data, None);
    assert!(
        withheld.is_empty(),
        "a SET_GROUP1 table walked without a directory name is withheld"
    );
    let (named, _, _) = walk_ifd(&IFD_SET_GROUP1, &data, Some("DirName"));
    assert_eq!(named.len(), 1);
    assert_eq!(named[0].group1, "DirName");

    let (carrier, _, _) = walk_keyed(
        &KEYED_ANSWER,
        &ciff_inline(&[(0x1001, &[0, 2])]),
        KeyedScope {
            group1_override: Some("Carrier"),
        },
        512,
    );
    assert_eq!(carrier[0].group1, "Carrier");

    let (serial, _, _) = walk_serial(&SERIAL_GROUPED, &[0, 2], false);
    assert_eq!(serial[0].group1, "TagGroup");
}

/// `Unknown` rows are suppressed; ProcessSerialData alone reports one when
/// its caller requested Unknown output (`-u`).
const UNKNOWN: IfdFlags = IfdFlags {
    unknown: true,
    ..IfdFlags::NONE
};
static IFD_UNKNOWN: IfdTable = ifd_table(
    "Unknown",
    &[IfdTag {
        flags: UNKNOWN,
        ..ifd_tag(0x0005, "Unknown")
    }],
);
static KEYED_UNKNOWN: KeyedDirectoryTable = keyed_table(
    "Unknown",
    &[KeyedTag {
        flags: UNKNOWN,
        ..keyed_tag(0x1005, "Unknown")
    }],
);
static SERIAL_UNKNOWN: SerialTable = serial_table(
    "Unknown",
    &[SerialEntry {
        serial_index: 0,
        alternatives: &[SerialTag {
            flags: UNKNOWN,
            ..serial_tag("Unknown", 1)
        }],
    }],
);

#[test]
fn unknown_rows_follow_the_request() {
    let (ifd, _, _) = walk_ifd(&IFD_UNKNOWN, &ifd_bytes(&[(0x0005, 3, 1, &[0, 2])]), None);
    assert!(ifd.is_empty());
    let (keyed, _, _) = walk_keyed(
        &KEYED_UNKNOWN,
        &ciff_inline(&[(0x1005, &[0, 2])]),
        NATIVE_SCOPE,
        512,
    );
    assert!(keyed.is_empty());
    let (quiet, _, _) = walk_serial(&SERIAL_UNKNOWN, &[0, 2], false);
    assert!(quiet.is_empty());
    let (requested, _, _) = walk_serial(&SERIAL_UNKNOWN, &[0, 2], true);
    assert_eq!(requested.len(), 1);
    assert_eq!(requested[0].value, TagValue::Integer(2));
}

/// Request filtering: a keyed scalar larger than the caller's request limit
/// is withheld before conversion and counted, never truncated.
#[test]
fn keyed_request_limit_filters_before_conversion() {
    let (rows, omitted, large) = walk_keyed(
        &KEYED_ANSWER,
        &ciff_inline(&[(0x1001, &[0, 2])]),
        NATIVE_SCOPE,
        4,
    );
    assert!(rows.is_empty());
    assert_eq!((omitted, large), (0, 1));
}

/// `RawConv => $$self{State} = $val` with a value the member model cannot
/// represent: binary, keyed and serial stop (taint); the IFD walker skips
/// that entry only and keeps walking. The policy is deliberate per walker.
static BINARY_STATE: BinaryTable = binary_table(
    "State",
    &[
        Field {
            count: 2,
            raw_conv: SET_STATE,
            omitted: RAW_CONV_OMITTED,
            ..binary_field(0, "State")
        },
        binary_field(2, "After"),
    ],
);
static KEYED_STATE: KeyedDirectoryTable = keyed_table(
    "State",
    &[
        KeyedTag {
            count: Some(2),
            raw_conv: SET_STATE,
            omitted: RAW_CONV_OMITTED,
            ..keyed_tag(0x1006, "State")
        },
        keyed_tag(0x1007, "After"),
    ],
);
static SERIAL_STATE: SerialTable = serial_table(
    "State",
    &[
        SerialEntry {
            serial_index: 0,
            alternatives: &[SerialTag {
                raw_conv: SET_STATE,
                omitted: RAW_CONV_OMITTED,
                ..serial_tag("State", 2)
            }],
        },
        SerialEntry {
            serial_index: 1,
            alternatives: &[serial_tag("After", 1)],
        },
    ],
);
static IFD_STATE: IfdTable = ifd_table(
    "State",
    &[
        IfdTag {
            raw_conv: SET_STATE,
            ..ifd_tag(0x0006, "State")
        },
        ifd_tag(0x0007, "After"),
    ],
);

#[test]
fn unrepresentable_member_policy_is_explicit_per_walker() {
    let (binary, members) = walk_binary(&BINARY_STATE, &[0, 1, 0, 2, 0, 3]);
    assert!(binary.is_empty(), "binary taints: no row after the refusal");
    assert!(!members.contains_key("State"));

    let (keyed, omitted, _) = walk_keyed(
        &KEYED_STATE,
        &ciff_inline(&[(0x1006, &[0, 1, 0, 2]), (0x1007, &[0, 3])]),
        NATIVE_SCOPE,
        512,
    );
    assert!(keyed.is_empty(), "keyed taints the directory");
    assert_eq!(omitted, 1);

    let (serial, omitted, tainted) = walk_serial(&SERIAL_STATE, &[0, 1, 0, 2, 0, 3], false);
    assert!(serial.is_empty(), "serial taints the directory");
    assert_eq!((omitted, tainted), (1, true));

    // A three-byte undef run that is not UTF-8 has no Perl string, so the
    // IFD walker skips this entry. (A one-byte undef reads as an integer.)
    let (ifd, members, _) = walk_ifd(
        &IFD_STATE,
        &ifd_bytes(&[(0x0006, 7, 3, &[0xff, 0xfe, 0xfd]), (0x0007, 3, 1, &[0, 3])]),
        None,
    );
    assert!(!members.contains_key("State"), "{members:?} {ifd:?}");
    assert_eq!(ifd.len(), 1, "IFD declines only the refused entry");
    assert_eq!(ifd[0].name, "After");
    assert_eq!(ifd[0].value, TagValue::Integer(3));
}

/// A representable member is written before the tag is reported; the IFD
/// walker writes the file Session too, exactly once.
#[test]
fn representable_member_is_written_before_reporting() {
    let (keyed, _, _) = walk_keyed(
        keyed_state_scalar(),
        &ciff_inline(&[(0x1006, &[0, 5])]),
        NATIVE_SCOPE,
        512,
    );
    assert_eq!(keyed.len(), 1);
    assert_eq!(keyed[0].value, TagValue::Integer(5));

    let (ifd, members, session) =
        walk_ifd(&IFD_STATE, &ifd_bytes(&[(0x0006, 3, 1, &[0, 5])]), None);
    assert_eq!(ifd.len(), 1);
    assert_eq!(members.get("State"), Some(&MemberValue::Num(5)));
    assert!(
        format!("{:?}", session.member("State")).contains('5'),
        "the IFD walker mirrors the member into the file Session"
    );
}

fn keyed_state_scalar() -> &'static KeyedDirectoryTable {
    static TABLE: KeyedDirectoryTable = keyed_table(
        "StateScalar",
        &[KeyedTag {
            raw_conv: SET_STATE,
            omitted: RAW_CONV_OMITTED,
            ..keyed_tag(0x1006, "State")
        }],
    );
    &TABLE
}

// ---------------------------------------------------------------------------
// Route controls: real carriers report more than FileType identity
// ---------------------------------------------------------------------------

const IDENTITY_GROUPS: [&str; 4] = ["File", "System", "ExifTool", "Composite"];

/// `(t/images file, a `read_metadata` key group that must carry real
/// metadata, an exact tag the named walker produces)`. `read_metadata` keys
/// are not always the `-G1` family-1 group (`ICC_Profile:`, not
/// `ICC-header:`).
/// `(key, exact value)` a named walker produces on a route carrier.
type WalkerTag = Option<(&'static str, &'static str)>;

const ROUTES: [(&str, &str, WalkerTag); 11] = [
    ("ExifTool.jpg", "IFD0", Some(("IFD0:Make", "FUJIFILM"))),
    ("QuickTime.mov", "QuickTime", None),
    ("RIFF.wav", "RIFF", None),
    (
        "Real.ra",
        "Real-RA4",
        Some(("Real-RA4:Title", "The Sewing Girls")),
    ),
    ("PDF.pdf", "PDF", None),
    ("FlashPix.ppt", "FlashPix", None),
    ("OOXML.docx", "ZIP", None),
    ("ZIP.zip", "ZIP", None),
    (
        "Olympus.jpg",
        "Olympus",
        Some(("Olympus:CameraType", "C2000Z")),
    ),
    (
        "ICC_Profile.icc",
        "ICC_Profile",
        Some(("ICC_Profile:CMMFlags", "Not Embedded, Independent")),
    ),
    ("CanonRaw.crw", "CanonRaw", None),
];

#[test]
fn route_controls_report_metadata_beyond_file_identity() {
    for (file, group, walker_tag) in ROUTES {
        let Some(path) = fixtures::pinned_t_images_fixture_path(file) else {
            continue;
        };
        let metadata = read_metadata(&path).unwrap_or_else(|error| panic!("{file}: {error}"));
        let real: Vec<&String> = metadata
            .keys()
            .filter(|key| {
                key.split_once(':')
                    .is_some_and(|(prefix, _)| !IDENTITY_GROUPS.contains(&prefix))
            })
            .collect();
        assert!(
            real.iter().any(|key| key.starts_with(&format!("{group}:"))),
            "{file}: no {group} metadata beyond FileType identity: {real:?}"
        );
        if let Some((key, want)) = walker_tag {
            assert_eq!(
                metadata.get(key).and_then(|value| value.as_string()),
                Some(want),
                "{file}: {key}"
            );
        }
    }
}
