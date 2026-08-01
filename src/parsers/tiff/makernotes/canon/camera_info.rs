//! Canon `CameraInfo` (MakerNote tag 0x000D) record walker.
//!
//! The tables themselves live in [`super::camera_info_tables`], which is
//! transcribed by script from ExifTool's own Perl hashes. This module is the
//! interpreter for them: it reproduces `Image::ExifTool::ProcessBinaryData`
//! closely enough that a table entry means here exactly what it means there.
//!
//! What that requires, in the order the walk hits it:
//!
//! 1. **Model dispatch.** ExifTool's tag 0x0D is a list of alternatives, each
//!    with a `Condition` on `$$self{Model}`, falling through to four
//!    format/count-selected tables at the end. There is no self-describing
//!    header in the record, so a body we cannot name gets the fall-through
//!    tables and nothing else -- never another body's field names.
//! 2. **The `varSize` running offset.** Fields are visited in ascending index
//!    order and read at `index * increment + varSize`. A field's `Hook` adjusts
//!    `varSize` *after* its own offset is computed, shifting every later field.
//! 3. **Firmware look-ahead.** The hooks are all conditioned on `CanonFirm`,
//!    which a hidden field at index 0 sets by probing a few byte offsets for an
//!    `N.N.N` version string. When none matches, ExifTool adds 0x10000, which
//!    pushes the next field past the end of the record and ends the walk.
//! 4. **`PRIORITY => 0`.** Every CameraInfo table is priority zero, so these
//!    values never displace a tag another Canon table already produced.

use std::collections::HashMap;

use super::camera_info_tables::{
    ALL_TABLES, Cmp, Cond, DISPATCH, F, Fmt, Pc, Rc, SubTable, TBL_POWERSHOT, TBL_POWERSHOT2,
    TBL_UNKNOWN, TBL_UNKNOWN16, TBL_UNKNOWN32, Table, Vc, sub_table,
};
use crate::parsers::tiff::ifd_parser::ByteOrder;

/// TIFF field types, as they appear in the MakerNote IFD entry for tag 0x0D.
/// ExifTool turns these back into its own format names before testing
/// `$format eq "int32u"` / `$format =~ /^int16/` in the dispatch conditions.
const TYPE_BYTE: u16 = 1;
const TYPE_SHORT: u16 = 3;
const TYPE_LONG: u16 = 4;
const TYPE_UNDEFINED: u16 = 7;
const TYPE_SSHORT: u16 = 8;
const TYPE_SLONG: u16 = 9;

/// Everything the table conditions can ask about the file, gathered once.
pub(crate) struct Ctx<'a> {
    /// EXIF `Model` if the dispatcher supplied one, else Canon's own
    /// `CanonImageType` (MakerNote tag 0x0006), which carries the same body
    /// name on every body in the ExifTool sample corpus but one.
    pub model: &'a str,
    /// `$$self{CameraInfoCount}` -- the element count declared by the IFD entry.
    pub count: u32,
    /// `$$self{LensType}` -- `%Canon::CameraSettings` key 22, a DATAMEMBER.
    pub lens_type: Option<i64>,
    /// `$$self{CanonFirm}`, set by the firmware look-ahead field.
    pub canon_firm: u8,
}

/// A value read out of the record, before conversion.
enum Val {
    Int(i64),
    Str(String),
    Bytes(Vec<u8>),
}

/// A value after `ValueConv`.
pub(crate) enum Conv {
    Int(i64),
    Float(f64),
    Str(String),
    /// Raw bytes, kept undecoded until the conversion that consumes them. The
    /// only `undef[N]` field that reaches a conversion is `LensSerialNumber`,
    /// whose `unpack("H*",$val)` hexes the stored bytes -- decoding them as
    /// text first would replace every byte above 0x7f with U+FFFD and hex that.
    Bytes(Vec<u8>),
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

/// Walks the `CameraInfo` record for one file and returns the tags it yields.
///
/// The result is deliberately returned rather than written straight into the
/// caller's map: every CameraInfo table is `PRIORITY => 0` in ExifTool, so
/// these values must not displace a tag some other Canon table produced. See
/// [`merge_priority0`].
pub(crate) fn parse_camera_info(
    record: &[u8],
    field_type: u16,
    value_count: u32,
    byte_order: ByteOrder,
    model: &str,
    lens_type: Option<i64>,
) -> HashMap<String, String> {
    let mut out = HashMap::new();
    let table = choose_table(model, field_type, value_count);
    let mut ctx = Ctx {
        model,
        count: value_count,
        lens_type,
        canon_firm: 0,
    };
    walk(table, record, byte_order, &mut ctx, &mut out);
    out
}

/// Folds CameraInfo output into the parser's tag map with ExifTool's
/// `PRIORITY => 0` semantics: a name another Canon table already produced keeps
/// that value, because a priority-0 tag never wins a name collision.
pub(crate) fn merge_priority0(tags: &mut HashMap<String, String>, from: HashMap<String, String>) {
    for (k, v) in from {
        tags.entry(k).or_insert(v);
    }
}

/// The `0xd => [...]` alternative list from `Canon.pm`, in ExifTool's order:
/// model conditions first, then the format/count fall-throughs.
fn choose_table(model: &str, field_type: u16, count: u32) -> &'static Table {
    for (pattern, table) in DISPATCH {
        if model_matches(model, pattern) {
            return table;
        }
    }
    // `$format eq "int32u" and ($count == 138 or $count == 148)`
    let is_int32u = field_type == TYPE_LONG;
    if is_int32u && (count == 138 || count == 148) {
        return TBL_POWERSHOT;
    }
    if is_int32u && matches!(count, 156 | 162 | 167 | 171 | 264) {
        return TBL_POWERSHOT2;
    }
    // `$format =~ /^int32/` then `$format =~ /^int16/`.
    if field_type == TYPE_LONG || field_type == TYPE_SLONG {
        return TBL_UNKNOWN32;
    }
    if field_type == TYPE_SHORT || field_type == TYPE_SSHORT {
        return TBL_UNKNOWN16;
    }
    // `CanonCameraInfoUnknown` carries no Condition at all, so it is the
    // unconditional last alternative -- every record that reaches it lands here,
    // whatever its format.
    TBL_UNKNOWN
}

// ---------------------------------------------------------------------------
// The walk
// ---------------------------------------------------------------------------

fn walk(
    table: &'static Table,
    data: &[u8],
    byte_order: ByteOrder,
    ctx: &mut Ctx<'_>,
    out: &mut HashMap<String, String>,
) {
    let increment = table.default_fmt.size() as i64;
    let size = data.len() as i64;
    let mut var_size: i64 = 0;

    let fields = table.fields;
    let mut i = 0usize;
    while i < fields.len() {
        // ExifTool visits one table key at a time; a key whose value is an
        // array is a list of alternatives and the first passing Condition wins.
        let idx = fields[i].idx;
        let mut j = i;
        while j < fields.len() && fields[j].idx == idx {
            j += 1;
        }
        let group = &fields[i..j];
        i = j;

        // A negative key counts elements back from the end of the record.
        let mut entry = idx * increment + var_size;
        if entry < 0 {
            entry += size;
            if entry < 0 {
                continue;
            }
        }
        let more = size - entry;
        if more <= 0 {
            // `last if $more <= 0` -- the rest of the table is out of range.
            break;
        }

        let Some(field) = group
            .iter()
            .find(|f| condition_holds(f.cond, ctx, data, entry as usize))
        else {
            continue;
        };

        // `next if $$tagInfo{Unknown}` -- Unknown tags need -u, which we never set.
        if field.unknown {
            continue;
        }

        let fmt = field.fmt.unwrap_or(table.default_fmt);
        let value = read_value(data, entry as usize, fmt, more as usize, byte_order);

        // The Hook runs after this field's own offset is fixed, so it only
        // moves the fields that come after it.
        apply_hook(field, ctx, &mut var_size);

        let Some(mut value) = value else {
            continue;
        };
        if let (Some(mask), Val::Int(n)) = (field.mask, &value) {
            // ExifTool derives BitShift from the mask's lowest set bit.
            let shift = mask.trailing_zeros();
            value = Val::Int((n & mask) >> shift);
        }

        if let Some(which) = field.sub {
            walk(
                sub_table(which),
                &data[entry as usize..],
                byte_order,
                ctx,
                out,
            );
            continue;
        }

        let Some(converted) = raw_conv(field.rc, value, ctx) else {
            continue;
        };
        if field.hidden {
            continue;
        }
        let converted = value_conv(field.vc, converted);
        let printed = print_conv(field.pc, converted);
        out.insert(format!("Canon:{}", field.name), printed);
    }
}

fn apply_hook(field: &F, ctx: &Ctx<'_>, var_size: &mut i64) {
    for rule in field.hook {
        let holds = match rule.cmp {
            Cmp::Lt => ctx.canon_firm < rule.firm,
            Cmp::Gt => ctx.canon_firm > rule.firm,
            Cmp::Eq => ctx.canon_firm == rule.firm,
            Cmp::Ge => ctx.canon_firm >= rule.firm,
            Cmp::Le => ctx.canon_firm <= rule.firm,
        };
        if holds {
            // `($$self{CanonFirm} ? A : B)` -- B is the arm taken when no
            // firmware string matched, and is normally 0x10000, which ends the
            // walk on the next field rather than reading it at a wrong offset.
            *var_size += if ctx.canon_firm == 0 {
                rule.zero_delta
            } else {
                rule.delta
            };
        }
    }
}

// ---------------------------------------------------------------------------
// Reading
// ---------------------------------------------------------------------------

fn read_value(data: &[u8], at: usize, fmt: Fmt, more: usize, byte_order: ByteOrder) -> Option<Val> {
    let need = fmt.size() as usize;
    if need == 0 || more < need {
        // `$count < 1 and return undef` -- not enough data left for one element.
        return None;
    }
    let bytes = data.get(at..at + need)?;
    let le = matches!(byte_order, ByteOrder::LittleEndian);
    Some(match fmt {
        Fmt::Int8u => Val::Int(bytes[0] as i64),
        Fmt::Int8s => Val::Int(bytes[0] as i8 as i64),
        Fmt::Int16u => Val::Int(u16_at(bytes, le) as i64),
        Fmt::Int16s => Val::Int(u16_at(bytes, le) as i16 as i64),
        // `int16uRev` is read with the byte order reversed relative to the rest
        // of the record -- Canon really does mix endianness inside one table.
        Fmt::Int16uRev => Val::Int(u16_at(bytes, !le) as i64),
        Fmt::Int32u => Val::Int(u32_at(bytes, le) as i64),
        Fmt::Int32s => Val::Int(u32_at(bytes, le) as i32 as i64),
        Fmt::Str(_) => {
            let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
            Val::Str(String::from_utf8_lossy(&bytes[..end]).into_owned())
        }
        Fmt::Undef(_) => Val::Bytes(bytes.to_vec()),
    })
}

fn u16_at(b: &[u8], le: bool) -> u16 {
    if le {
        u16::from_le_bytes([b[0], b[1]])
    } else {
        u16::from_be_bytes([b[0], b[1]])
    }
}

fn u32_at(b: &[u8], le: bool) -> u32 {
    if le {
        u32::from_le_bytes([b[0], b[1], b[2], b[3]])
    } else {
        u32::from_be_bytes([b[0], b[1], b[2], b[3]])
    }
}

// ---------------------------------------------------------------------------
// Conditions
// ---------------------------------------------------------------------------

fn is_word(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_'
}

/// `\b` before byte offset `at`.
fn boundary_before(s: &[u8], at: usize) -> bool {
    at == 0 || !is_word(s[at - 1])
}

/// `\b` at byte offset `at`.
fn boundary_after(s: &[u8], at: usize) -> bool {
    at >= s.len() || !is_word(s[at])
}

/// `/\bLIT$/`
fn ends_with_word(model: &str, lit: &str) -> bool {
    let (m, l) = (model.as_bytes(), lit.as_bytes());
    m.len() >= l.len() && &m[m.len() - l.len()..] == l && boundary_before(m, m.len() - l.len())
}

/// `/LIT\b/` -- the leading `\b` in `/\b(A|B)\b/` is implied by the literals
/// all starting with a word character, so only the trailing one is tested.
fn contains_word(model: &str, lit: &str) -> bool {
    let (m, l) = (model.as_bytes(), lit.as_bytes());
    if l.is_empty() || m.len() < l.len() {
        return false;
    }
    (0..=m.len() - l.len())
        .any(|i| &m[i..i + l.len()] == l && boundary_before(m, i) && boundary_after(m, i + l.len()))
}

/// One element of a tag 0x0D model condition.
///
/// The `DISPATCH` patterns are ExifTool's Perl regexes copied verbatim, so they
/// are matched rather than reshaped into string comparisons. Between them they
/// use six constructs and no more; anything else is refused by `compile` and
/// caught by `every_dispatch_pattern_compiles`, so an unsupported construct is
/// a test failure rather than a body that silently dispatches nowhere.
#[derive(Debug, PartialEq)]
enum Node {
    /// A literal character.
    Lit(char),
    /// `X?` -- an optional literal character, as in `\b1Ds? Mark III$`.
    Opt(char),
    /// `[abc]` -- one character from a set, as in `\bEOS R[56]$`.
    Class(Vec<char>),
    /// `(A|B|C)` -- alternation over plain literals.
    Alt(Vec<String>),
    /// `\b`
    Boundary,
    /// `$`
    End,
}

/// Parses one of ExifTool's model-condition regexes. Returns `None` for any
/// construct outside the six [`Node`] variants rather than approximating it.
fn compile(pattern: &str) -> Option<Vec<Node>> {
    let chars: Vec<char> = pattern.chars().collect();
    let mut nodes = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '\\' => {
                if chars.get(i + 1) != Some(&'b') {
                    return None;
                }
                nodes.push(Node::Boundary);
                i += 2;
            }
            '$' => {
                if i + 1 != chars.len() {
                    return None;
                }
                nodes.push(Node::End);
                i += 1;
            }
            '[' => {
                let close = chars[i..].iter().position(|&c| c == ']')? + i;
                let set: Vec<char> = chars[i + 1..close].to_vec();
                if set.is_empty() || set.contains(&'^') || set.contains(&'-') {
                    return None;
                }
                nodes.push(Node::Class(set));
                i = close + 1;
            }
            '(' => {
                let close = chars[i..].iter().position(|&c| c == ')')? + i;
                let body: String = chars[i + 1..close].iter().collect();
                if body.contains(['(', '[', '?', '\\', '.', '*', '+']) {
                    return None;
                }
                nodes.push(Node::Alt(body.split('|').map(str::to_string).collect()));
                i = close + 1;
            }
            // Regex metacharacters this matcher does not implement.
            '.' | '*' | '+' | '^' | '|' | ')' | ']' | '{' | '}' => return None,
            c => {
                if chars.get(i + 1) == Some(&'?') {
                    nodes.push(Node::Opt(c));
                    i += 2;
                } else {
                    nodes.push(Node::Lit(c));
                    i += 1;
                }
            }
        }
    }
    Some(nodes)
}

/// Matches `nodes[n..]` against `haystack[at..]`, backtracking through `Opt`
/// and `Alt`. Byte indices are used so `\b` can look at the character on
/// either side.
fn match_here(nodes: &[Node], n: usize, haystack: &[u8], at: usize) -> bool {
    let Some(node) = nodes.get(n) else {
        return true;
    };
    match node {
        Node::Boundary => {
            let before = at > 0 && is_word(haystack[at - 1]);
            let after = at < haystack.len() && is_word(haystack[at]);
            before != after && match_here(nodes, n + 1, haystack, at)
        }
        Node::End => at == haystack.len(),
        Node::Lit(c) => {
            let mut buf = [0u8; 4];
            let bytes = c.encode_utf8(&mut buf).as_bytes();
            haystack[at..].starts_with(bytes)
                && match_here(nodes, n + 1, haystack, at + bytes.len())
        }
        Node::Opt(c) => {
            let mut buf = [0u8; 4];
            let bytes = c.encode_utf8(&mut buf).as_bytes();
            (haystack[at..].starts_with(bytes)
                && match_here(nodes, n + 1, haystack, at + bytes.len()))
                || match_here(nodes, n + 1, haystack, at)
        }
        Node::Class(set) => set.iter().any(|c| {
            let mut buf = [0u8; 4];
            let bytes = c.encode_utf8(&mut buf).as_bytes();
            haystack[at..].starts_with(bytes)
                && match_here(nodes, n + 1, haystack, at + bytes.len())
        }),
        Node::Alt(alts) => alts.iter().any(|a| {
            haystack[at..].starts_with(a.as_bytes())
                && match_here(nodes, n + 1, haystack, at + a.len())
        }),
    }
}

/// An unanchored search, which is what a Perl `=~` without `^` does.
fn model_matches(model: &str, pattern: &str) -> bool {
    let Some(nodes) = compile(pattern) else {
        // Refusing to match beats matching by accident: a pattern this matcher
        // cannot parse sends the body to the format-keyed fall-through tables
        // rather than to a table whose offsets do not describe it.
        return false;
    };
    let bytes = model.as_bytes();
    (0..=bytes.len()).any(|start| match_here(&nodes, 0, bytes, start))
}

fn condition_holds(cond: Cond, ctx: &Ctx<'_>, data: &[u8], entry: usize) -> bool {
    match cond {
        Cond::Always => true,
        Cond::ModelEndsWord(lit) => ends_with_word(ctx.model, lit),
        Cond::ModelHasWord(alts) => alts.iter().any(|a| contains_word(ctx.model, a)),
        Cond::ModelStartsWith(lit) => ctx.model.starts_with(lit),
        Cond::LensTypeIs(n) => ctx.lens_type.is_some_and(|l| l != 0 && l == n),
        Cond::CountEq(n) => ctx.count == n,
        Cond::CountEither(a, b) => ctx.count == a || ctx.count == b,
        Cond::CountGreater(n) => ctx.count > n,
        // `$$valPt =~ /^\d\.\d\.\d\0/`
        Cond::ValueLooksLikeVersion => data.get(entry..entry + 6).is_some_and(|v| {
            v[0].is_ascii_digit()
                && v[1] == b'.'
                && v[2].is_ascii_digit()
                && v[3] == b'.'
                && v[4].is_ascii_digit()
                && v[5] == 0
        }),
        // The MakerNote parser is handed the model but not the container file
        // type, so `$$self{FileType} eq "JPEG"` cannot be evaluated. Emitting
        // the field anyway would put a JPEG offset's bytes under a real tag
        // name on a CR3, so it is not emitted at all.
        Cond::FileTypeUnavailable => false,
    }
}

// ---------------------------------------------------------------------------
// Conversions
// ---------------------------------------------------------------------------

fn raw_conv(rc: Rc, val: Val, ctx: &mut Ctx<'_>) -> Option<Conv> {
    match rc {
        Rc::None => Some(match val {
            Val::Int(n) => Conv::Int(n),
            Val::Str(s) => Conv::Str(s),
            Val::Bytes(b) => Conv::Bytes(b),
        }),
        // `$val ? $val : undef`
        Rc::SkipZero => match val {
            Val::Int(0) => None,
            Val::Int(n) => Some(Conv::Int(n)),
            Val::Str(s) if s.is_empty() || s == "0" => None,
            Val::Str(s) => Some(Conv::Str(s)),
            Val::Bytes(b) => Some(Conv::Bytes(b)),
        },
        // `$val =~ /^\d+\.\d+\.\d+\s*$/ ? $val : undef`
        Rc::RequireVersionString => match val {
            Val::Str(s) if looks_like_version(&s) => Some(Conv::Str(s)),
            _ => None,
        },
        // The hidden `FirmwareVersionLookAhead`: probe each offset in turn for
        // an `N.N.N` string and record which one matched. Never a real tag.
        Rc::FirmwareProbe(probes) => {
            let bytes = match &val {
                Val::Bytes(b) => b.as_slice(),
                Val::Str(s) => s.as_bytes(),
                Val::Int(_) => &[],
            };
            ctx.canon_firm = 0;
            for &(offset, firm) in probes {
                let at = offset as usize;
                if let Some(window) = bytes.get(at..at + 6)
                    && starts_like_version(window)
                {
                    ctx.canon_firm = firm;
                    break;
                }
            }
            None
        }
    }
}

/// `/^\d+\.\d+\.\d+\s*$/`
fn looks_like_version(s: &str) -> bool {
    let t = s.trim_end_matches([' ', '\t', '\n', '\r', '\x0c']);
    let mut parts = t.split('.');
    let ok = (0..3).all(|_| {
        parts
            .next()
            .is_some_and(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
    });
    ok && parts.next().is_none()
}

/// `/^\d+\.\d+\.\d+/` against a fixed six-byte window.
fn starts_like_version(w: &[u8]) -> bool {
    let mut i = 0;
    for part in 0..3 {
        if part > 0 {
            if w.get(i) != Some(&b'.') {
                return false;
            }
            i += 1;
        }
        let start = i;
        while w.get(i).is_some_and(u8::is_ascii_digit) {
            i += 1;
        }
        if i == start {
            return false;
        }
    }
    true
}

fn value_conv(vc: Vc, val: Conv) -> Conv {
    let n = match &val {
        Conv::Int(n) => *n as f64,
        Conv::Float(f) => *f,
        // `unpack("H*",$val)` is the only conversion these tables apply to a
        // non-numeric value; everything else passes strings and bytes through.
        Conv::Str(_) | Conv::Bytes(_) => {
            return match (vc, val) {
                (Vc::HexBytes, Conv::Bytes(b)) => {
                    Conv::Str(b.iter().map(|byte| format!("{:02x}", byte)).collect())
                }
                (Vc::HexBytes, Conv::Str(s)) => {
                    Conv::Str(s.bytes().map(|byte| format!("{:02x}", byte)).collect())
                }
                (_, other) => other,
            };
        }
    };
    match vc {
        Vc::None => val,
        Vc::Div100 => Conv::Float(n / 100.0),
        Vc::Plus1 => Conv::Int(n as i64 + 1),
        Vc::Minus1 => Conv::Int(n as i64 - 1),
        Vc::Minus128 => Conv::Int(n as i64 - 128),
        Vc::Ev8Minus6 => Conv::Float(n / 8.0 - 6.0),
        // `exp(4*log(2)*(1-CanonEv($val-24)))`
        Vc::CanonExposureTime => {
            Conv::Float((4.0 * std::f64::consts::LN_2 * (1.0 - canon_ev(n - 24.0))).exp())
        }
        // `exp(($val-8)/16*log(2))`
        Vc::CanonFNumber => Conv::Float(((n - 8.0) / 16.0 * std::f64::consts::LN_2).exp()),
        // `100*exp(($val/8-9)*log(2))`
        Vc::CanonIso => Conv::Float(100.0 * ((n / 8.0 - 9.0) * std::f64::consts::LN_2).exp()),
        // `exp((75-$val) * log(2) * 3 / 40)`
        Vc::MacroMagnification => {
            Conv::Float(((75.0 - n) * std::f64::consts::LN_2 * 3.0 / 40.0).exp())
        }
        Vc::UnixTime => Conv::Str(convert_unix_time(n as i64)),
        Vc::HexBytes => val,
        // `100*exp((($val-411)/96)*log(2))`
        Vc::PowerShotIso => {
            Conv::Float(100.0 * (((n - 411.0) / 96.0) * std::f64::consts::LN_2).exp())
        }
        // `exp($val/192*log(2))`
        Vc::PowerShotFNumber => Conv::Float((n / 192.0 * std::f64::consts::LN_2).exp()),
        // `exp(-$val/96*log(2))`
        Vc::PowerShotExposureTime => Conv::Float((-n / 96.0 * std::f64::consts::LN_2).exp()),
    }
}

/// `Image::ExifTool::Canon::CanonEv` (Canon.pm): Canon's 1/3-stop encoding,
/// where the fractional part is 0x0c for a third and 0x14 for two thirds.
fn canon_ev(val: f64) -> f64 {
    let mut val = val as i64;
    let sign = if val < 0 {
        val = -val;
        -1.0
    } else {
        1.0
    };
    let mut frac = (val & 0x1f) as f64;
    let whole = (val - (val & 0x1f)) as f64;
    if frac == 0x0c as f64 {
        frac = 32.0 / 3.0;
    } else if frac == 0x14 as f64 {
        frac = 64.0 / 3.0;
    }
    sign * (whole + frac) / 32.0
}

pub(crate) fn print_conv(pc: Pc, val: Conv) -> String {
    match pc {
        Pc::None => render(&val),
        Pc::Map(map, print_hex) => match &val {
            Conv::Int(n) => lookup(map, *n)
                .map(str::to_string)
                .unwrap_or_else(|| unknown(*n, print_hex)),
            _ => render(&val),
        },
        // `%psConv`'s `OTHER => sub { shift }` -- unmatched comes back unchanged.
        Pc::MapOrRaw(map) => match &val {
            Conv::Int(n) => lookup(map, *n)
                .map(str::to_string)
                .unwrap_or_else(|| render(&val)),
            _ => render(&val),
        },
        // `%filterConv`'s `OTHER => sub { "On ($val)" }`.
        Pc::MapOrOn(map) => match &val {
            Conv::Int(n) => lookup(map, *n)
                .map(str::to_string)
                .unwrap_or_else(|| format!("On ({})", n)),
            _ => format!("On ({})", render(&val)),
        },
        // `%printParameter`'s OTHER is `Exif::PrintParameter`, which prints a
        // positive adjustment with its sign and folds a value above 0xfff0 back
        // to the negative it really is.
        Pc::MapOrSigned(map) => match &val {
            Conv::Int(n) => lookup(map, *n)
                .map(str::to_string)
                .unwrap_or_else(|| print_parameter(*n)),
            _ => render(&val),
        },
        Pc::BitMask(map, bits) => match &val {
            Conv::Int(n) => lookup(map, *n).map(str::to_string).unwrap_or_else(|| {
                let set: Vec<&str> = bits
                    .iter()
                    .filter(|(bit, _)| n & (1 << bit) != 0)
                    .map(|(_, name)| *name)
                    .collect();
                if set.is_empty() {
                    format!("(none)")
                } else {
                    set.join(", ")
                }
            }),
            _ => render(&val),
        },
        Pc::Mm => format!("{} mm", render(&val)),
        // `$val > 655.345 ? "inf" : "$val m"`
        Pc::FocusDistance => {
            if as_f64(&val) > 655.345 {
                "inf".to_string()
            } else {
                format!("{} m", render(&val))
            }
        }
        Pc::Celsius => format!("{} C", render(&val)),
        Pc::ExposureTime => print_exposure_time(as_f64(&val)),
        Pc::Sprintf2G => sprintf_g(as_f64(&val), 2),
        Pc::Sprintf0F => format!("{:.0}", as_f64(&val)),
        Pc::Sprintf1Fx => format!("{:.1}x", as_f64(&val)),
        // `$self->ConvertDateTime($val)` with no DateFormat option is identity.
        Pc::DateTime => render(&val),
    }
}

fn lookup(map: &[(i64, &'static str)], key: i64) -> Option<&'static str> {
    map.binary_search_by_key(&key, |(k, _)| *k)
        .ok()
        .map(|i| map[i].1)
}

/// ExifTool's unmatched-PrintConv rendering. `PrintHex` switches it to hex, and
/// Perl's `%x` on a negative integer prints the 64-bit two's complement.
fn unknown(n: i64, print_hex: bool) -> String {
    if print_hex {
        format!("Unknown (0x{:x})", n as u64)
    } else {
        format!("Unknown ({})", n)
    }
}

fn as_f64(v: &Conv) -> f64 {
    match v {
        Conv::Int(n) => *n as f64,
        Conv::Float(f) => *f,
        Conv::Str(s) => s.parse().unwrap_or(0.0),
        Conv::Bytes(_) => 0.0,
    }
}

fn render(v: &Conv) -> String {
    match v {
        Conv::Int(n) => n.to_string(),
        Conv::Float(f) => format_perl_number(*f),
        Conv::Str(s) => s.clone(),
        Conv::Bytes(b) => String::from_utf8_lossy(b).into_owned(),
    }
}

/// Perl interpolates a float with up to 15 significant digits and no trailing
/// zeros; six decimals is enough for every value these tables produce.
fn format_perl_number(value: f64) -> String {
    let rendered = format!("{:.6}", value);
    let trimmed = rendered.trim_end_matches('0').trim_end_matches('.');
    if trimmed.is_empty() || trimmed == "-" {
        "0".to_string()
    } else {
        trimmed.to_string()
    }
}

/// `Image::ExifTool::Exif::PrintParameter`.
fn print_parameter(value: i64) -> String {
    if value > 0 {
        if value > 0xfff0 {
            return (value - 0x10000).to_string();
        }
        return format!("+{}", value);
    }
    value.to_string()
}

/// `Image::ExifTool::Exif::PrintExposureTime`.
fn print_exposure_time(seconds: f64) -> String {
    if seconds > 0.0 && seconds < 0.25001 {
        return format!("1/{}", (0.5 + 1.0 / seconds) as i64);
    }
    let rendered = format!("{:.1}", seconds);
    rendered
        .strip_suffix(".0")
        .map(str::to_string)
        .unwrap_or(rendered)
}

/// C's `%.<prec>g`, which is what Perl's `sprintf` gives these tags.
fn sprintf_g(value: f64, precision: usize) -> String {
    let p = precision.max(1);
    if value == 0.0 {
        return "0".to_string();
    }
    let sci = format!("{:.*e}", p - 1, value);
    let (mantissa, exponent) = match sci.split_once('e') {
        Some(parts) => parts,
        None => return sci,
    };
    let exp: i32 = exponent.parse().unwrap_or(0);
    if exp < -4 || exp >= p as i32 {
        let m = trim_fraction(mantissa);
        format!("{}e{}{:02}", m, if exp < 0 { '-' } else { '+' }, exp.abs())
    } else {
        let decimals = (p as i32 - 1 - exp).max(0) as usize;
        trim_fraction(&format!("{:.*}", decimals, value))
    }
}

fn trim_fraction(s: &str) -> String {
    if s.contains('.') {
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    } else {
        s.to_string()
    }
}

/// `Image::ExifTool::ConvertUnixTime($val)` -- UTC, `YYYY:MM:DD HH:MM:SS`.
fn convert_unix_time(seconds: i64) -> String {
    let days = seconds.div_euclid(86_400);
    let secs_of_day = seconds.rem_euclid(86_400);
    let (hour, minute, second) = (
        secs_of_day / 3600,
        (secs_of_day / 60) % 60,
        secs_of_day % 60,
    );

    // Howard Hinnant's civil_from_days.
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };

    format!(
        "{:04}:{:02}:{:02} {:02}:{:02}:{:02}",
        y, m, d, hour, minute, second
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The transcribed tables must stay sorted by index within each table:
    /// `ProcessBinaryData` walks keys in ascending numeric order, and `varSize`
    /// accumulates along the way, so an out-of-order key would read every later
    /// field at the wrong offset.
    #[test]
    fn tables_are_index_ordered() {
        for table in ALL_TABLES {
            let mut last: Option<i64> = None;
            for field in table.fields {
                // Negative keys sort after the positive ones, as in ExifTool's
                // `$a < 0 ? $a + 1e9 : $a` comparison.
                let key = if field.idx < 0 {
                    field.idx + 1_000_000_000
                } else {
                    field.idx
                };
                if let Some(prev) = last {
                    assert!(
                        key >= prev,
                        "{} field {} is out of index order",
                        table.name,
                        field.name
                    );
                }
                last = Some(key);
            }
        }
    }

    /// Every dispatch pattern must be one the mini matcher can parse. A pattern
    /// it cannot parse never matches, which would send that body silently to
    /// the format-keyed fall-through tables -- exactly the failure that dropped
    /// the whole 1D family before `Opt` was implemented (`\b1Ds? Mark III$`).
    #[test]
    fn every_dispatch_pattern_compiles() {
        for (pattern, table) in DISPATCH {
            assert!(
                compile(pattern).is_some(),
                "{} pattern {:?} is not supported by the matcher",
                table.name,
                pattern
            );
        }
    }

    /// Every model condition in the dispatch list has to be matched by the
    /// mini regex engine. A pattern it silently fails to recognise would send a
    /// body to the wrong table.
    #[test]
    fn every_dispatch_pattern_matches_a_real_model() {
        let samples: &[(&str, &str)] = &[
            ("Canon EOS-1D", "CameraInfo1D"),
            ("Canon EOS-1DS", "CameraInfo1D"),
            ("Canon EOS-1D Mark II", "CameraInfo1DmkII"),
            ("Canon EOS-1Ds Mark II", "CameraInfo1DmkII"),
            ("Canon EOS-1D Mark II N", "CameraInfo1DmkIIN"),
            ("Canon EOS-1D Mark III", "CameraInfo1DmkIII"),
            ("Canon EOS-1Ds Mark III", "CameraInfo1DmkIII"),
            ("Canon EOS-1D Mark IV", "CameraInfo1DmkIV"),
            ("Canon EOS-1D X", "CameraInfo1DX"),
            ("Canon EOS 5D", "CameraInfo5D"),
            ("Canon EOS 5D Mark II", "CameraInfo5DmkII"),
            ("Canon EOS 5D Mark III", "CameraInfo5DmkIII"),
            ("Canon EOS 6D", "CameraInfo6D"),
            ("Canon EOS 7D", "CameraInfo7D"),
            ("Canon EOS 40D", "CameraInfo40D"),
            ("Canon EOS 50D", "CameraInfo50D"),
            ("Canon EOS 60D", "CameraInfo60D"),
            ("Canon EOS 70D", "CameraInfo70D"),
            ("Canon EOS 80D", "CameraInfo80D"),
            ("Canon EOS 450D", "CameraInfo450D"),
            ("Canon EOS DIGITAL REBEL XSi", "CameraInfo450D"),
            ("Canon EOS Kiss X2", "CameraInfo450D"),
            ("Canon EOS 500D", "CameraInfo500D"),
            ("Canon EOS REBEL T1i", "CameraInfo500D"),
            ("Canon EOS 550D", "CameraInfo550D"),
            ("Canon EOS Kiss X4", "CameraInfo550D"),
            ("Canon EOS 600D", "CameraInfo600D"),
            ("Canon EOS REBEL T3i", "CameraInfo600D"),
            ("Canon EOS 650D", "CameraInfo650D"),
            ("Canon EOS 700D", "CameraInfo650D"),
            ("Canon EOS REBEL T5i", "CameraInfo650D"),
            ("Canon EOS 750D", "CameraInfo750D"),
            ("Canon EOS 760D", "CameraInfo750D"),
            ("Canon EOS 8000D", "CameraInfo750D"),
            ("Canon EOS 1000D", "CameraInfo1000D"),
            ("Canon EOS Kiss F", "CameraInfo1000D"),
            ("Canon EOS 1100D", "CameraInfo600D"),
            ("Canon EOS Kiss X50", "CameraInfo600D"),
            ("Canon EOS 1200D", "CameraInfo60D"),
            ("Canon EOS Kiss X70", "CameraInfo60D"),
            ("Canon EOS R5", "CameraInfoR6"),
            ("Canon EOS R6", "CameraInfoR6"),
            ("Canon EOS R6m2", "CameraInfoR6m2"),
            ("Canon EOS R8", "CameraInfoR6m2"),
            ("Canon EOS R50", "CameraInfoR6m2"),
            ("Canon EOS R6 Mark III", "CameraInfoR6m3"),
            ("Canon PowerShot G5 X Mark II", "CameraInfoG5XII"),
        ];
        for (model, expected) in samples {
            let table = choose_table(model, TYPE_UNDEFINED, 0);
            assert_eq!(&table.name, expected, "wrong table for {}", model);
        }
    }

    /// Bodies that share a name prefix with a table but are not that body must
    /// not borrow its offsets. `EOS 5D$` must not swallow the 5D Mark II, and
    /// `\b1D$` must not swallow the 1D X or the 1200D.
    #[test]
    fn model_conditions_do_not_over_match() {
        assert!(!ends_with_word("Canon EOS 5D Mark II", "EOS 5D"));
        assert!(!ends_with_word("Canon EOS-1D X", "1D"));
        assert!(!ends_with_word("Canon EOS 1200D", "1D"));
        assert!(!ends_with_word("Canon EOS-1DS", "1D"));
        assert!(ends_with_word("Canon EOS-1D", "1D"));
        assert!(ends_with_word("Canon EOS-1DS", "1DS"));
        // `\bKiss X5\b` must not match "Kiss X50" -- the 1100D would then be
        // read with the 600D's offsets.
        assert!(!contains_word("Canon EOS Kiss X50", "Kiss X5"));
        assert!(contains_word("Canon EOS Kiss X50", "Kiss X50"));
        // A body with no CameraInfo table of its own falls through to the
        // format-keyed tables rather than to a neighbouring model's.
        assert_eq!(
            choose_table("Canon PowerShot S100", TYPE_UNDEFINED, 500).name,
            "CameraInfoUnknown"
        );
        assert_eq!(
            choose_table("Canon PowerShot A560", TYPE_LONG, 148).name,
            "CameraInfoPowerShot"
        );
        assert_eq!(
            choose_table("Canon PowerShot G10", TYPE_LONG, 162).name,
            "CameraInfoPowerShot2"
        );
        assert_eq!(
            choose_table("Canon PowerShot A410", TYPE_LONG, 93).name,
            "CameraInfoUnknown32"
        );
        assert_eq!(
            choose_table("Canon IXUS 160", TYPE_SHORT, 80).name,
            "CameraInfoUnknown16"
        );
    }

    /// The firmware look-ahead is what makes every later offset in nine tables
    /// correct, so probe recognition is worth pinning: a `1.0.4` six-byte
    /// window is a version, `\0\0\0\0\0\0` is not.
    #[test]
    fn firmware_probe_recognises_version_strings() {
        assert!(starts_like_version(b"1.0.4\0"));
        assert!(starts_like_version(b"4.2.1\0"));
        assert!(starts_like_version(b"10.1.1"));
        assert!(!starts_like_version(b"\0\0\0\0\0\0"));
        assert!(!starts_like_version(b"1.0\0\0\0"));
        assert!(!starts_like_version(b"Canon "));
    }

    /// An unmatched firmware string makes ExifTool add 0x10000 to `varSize`,
    /// which is not an offset correction -- it is a deliberate way to end the
    /// walk. Reproducing it is what keeps a wrong-firmware body silent instead
    /// of emitting garbage under real tag names.
    #[test]
    fn unmatched_firmware_stops_the_walk() {
        let table = choose_table("Canon EOS-1D Mark IV", TYPE_UNDEFINED, 0);
        // A record long enough to reach the hooked field but with no version
        // string anywhere, so `CanonFirm` stays 0.
        let record = vec![0u8; 0x400];
        let mut ctx = Ctx {
            model: "Canon EOS-1D Mark IV",
            count: 0x400,
            lens_type: None,
            canon_firm: 0,
        };
        let mut out = HashMap::new();
        walk(table, &record, ByteOrder::LittleEndian, &mut ctx, &mut out);
        assert_eq!(ctx.canon_firm, 0);
        // 0x56 FocusDistanceLower carries the hook; nothing past it may appear.
        assert!(!out.contains_key("Canon:WhiteBalance"), "{:?}", out);
        assert!(!out.contains_key("Canon:LensType"), "{:?}", out);
        assert!(!out.contains_key("Canon:FirmwareVersion"), "{:?}", out);
    }

    #[test]
    fn value_conversions_match_exiftool() {
        // %ciCameraTemperature: 155 -> "27 C"
        assert_eq!(
            print_conv(Pc::Celsius, value_conv(Vc::Minus128, Conv::Int(155))),
            "27 C"
        );
        // %focusDistanceByteSwap: 385 -> "3.85 m", 65535 -> "inf"
        assert_eq!(
            print_conv(Pc::FocusDistance, value_conv(Vc::Div100, Conv::Int(385))),
            "3.85 m"
        );
        assert_eq!(
            print_conv(Pc::FocusDistance, value_conv(Vc::Div100, Conv::Int(65535))),
            "inf"
        );
        // %ciFNumber: raw 72 -> f/16
        assert_eq!(
            print_conv(Pc::Sprintf2G, value_conv(Vc::CanonFNumber, Conv::Int(72))),
            "16"
        );
        // %ciISO: raw 72 -> ISO 100
        assert_eq!(
            print_conv(Pc::Sprintf0F, value_conv(Vc::CanonIso, Conv::Int(72))),
            "100"
        );
        // %ciExposureTime, through CanonEv's thirds-of-a-stop encoding.
        for (raw, expected) in [(96, "1/32"), (112, "1/128"), (160, "1/8192")] {
            assert_eq!(
                print_conv(
                    Pc::ExposureTime,
                    value_conv(Vc::CanonExposureTime, Conv::Int(raw))
                ),
                expected,
                "ExposureTime raw {}",
                raw
            );
        }
        // PowerShot conversions, which use a different encoding again.
        assert_eq!(
            print_conv(Pc::Sprintf0F, value_conv(Vc::PowerShotIso, Conv::Int(411))),
            "100"
        );
        assert_eq!(
            print_conv(
                Pc::Sprintf2G,
                value_conv(Vc::PowerShotFNumber, Conv::Int(192))
            ),
            "2"
        );
        // ConvertUnixTime, UTC
        assert_eq!(convert_unix_time(0), "1970:01:01 00:00:00");
        assert_eq!(convert_unix_time(1_234_567_890), "2009:02:13 23:31:30");
    }

    #[test]
    fn sprintf_g_matches_c() {
        assert_eq!(sprintf_g(2.8284, 2), "2.8");
        assert_eq!(sprintf_g(5.6568, 2), "5.7");
        assert_eq!(sprintf_g(11.3137, 2), "11");
        assert_eq!(sprintf_g(1.4142, 2), "1.4");
        assert_eq!(sprintf_g(0.0, 2), "0");
    }
}
