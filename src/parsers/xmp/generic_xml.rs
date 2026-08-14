//! Plain-XML metadata, the way ExifTool reads it: as XMP with no schema.
//!
//! An XML file that is not XMP, RDF, SVG, PLIST, INX or RMD still reaches
//! `XMP::ProcessXMP` -- `SetFileType('XML')` happens *inside* that function
//! (XMP.pm:4425-4431) -- and is then walked by `ParseXMPElement` exactly like
//! an XMP packet. There is no GPX table, no KML table and no Garmin table
//! anywhere in ExifTool; `Geotag.pm` writes GPX but never reads one. Every tag
//! ExifTool reports for these files is *emergent*, minted on the spot by
//! `FoundXMP` from the property path alone.
//!
//! # The naming rule
//!
//! `GetXMPTagID` (XMP.pm:3018-3070) walks the property path and concatenates
//! one segment per property that is not ignored:
//!
//! ```perl
//!     foreach $prop (@$props) {
//!         my ($ns, $nm) = ($prop =~ /(.*?):(.*)/) ? ($1, $2) : ('', $prop);
//!         if ($ignoreNamespace{$ns} or $ignoreProp{$prop} or $ignoreEtProp{$prop}) {
//!             unless ($prop =~ /^rdf:(_\d+)$/) { ...; next; }
//!             $tag .= $1 if defined $tag;
//!         } else {
//!             ...
//!             if (defined $tag) { $tag .= ucfirst($nm) } else { $tag = $nm }
//!         }
//!         # save namespace of first property to contribute to tag name
//!         $namespace = $ns unless $namespace;
//!     }
//! ```
//!
//! and `FoundXMP` finishes with `my $name = ucfirst($tag);` (XMP.pm:3519). So
//! for `Geotag.gpx`'s `<gpx><trk><trkseg><trkpt lat="...">`:
//!
//! ```text
//!   gpx / trk / trkseg / trkpt / lat  ->  "gpx" + "Trk" + "Trkseg" + "Trkpt" + "Lat"
//!                                     ->  ucfirst -> GpxTrkTrksegTrkptLat
//! ```
//!
//! That is the whole of it. The name is a path, not a table lookup, which is
//! why `rg 'GpxTrk'` finds nothing in either codebase.
//!
//! # Groups
//!
//! `$namespace` is set by the line *after* the if/else above, which the ignore
//! branch's `next` skips -- so only a contributing property can set it, and
//! `unless $namespace` means the first **non-empty** prefix wins. `FoundXMP`
//! then translates it through `%stdXlatNS` (XMP.pm:3444) and sets family 1
//! from it (XMP.pm:3785-3787):
//!
//! ```perl
//!     } elsif ($ns and not $$tagInfo{StaticGroup1}) {
//!         $et->SetGroup($key, "$$tagTablePtr{GROUPS}{0}-$ns");
//!     }
//! ```
//!
//! An empty prefix leaves the table's own group 1, `XMP`. This is why
//! `Geotag.kml`'s `<gx:Track><when>` reports under `XMP-gx` even though `when`
//! itself is unprefixed, and why `xsi:schemaLocation` on a `<gpx>` root reports
//! under `XMP-xsi`.
//!
//! # Repeats are occurrences, not a list
//!
//! A track is a *repeated element*, not an `rdf:Bag`, so `FoundXMP` mints no
//! `List` flag (XMP.pm:3617-3627 only does that for `rdf:li` under
//! `rdf:Bag`/`Seq`/`Alt`) and each `<trkpt>` calls `FoundTag` again. ExifTool
//! keeps all nine under one name and `-a` prints all nine. These are therefore
//! recorded through [`MetadataMap::insert_occurrence`], one occurrence per
//! track point, and not joined into a single comma-separated value.
//!
//! # What this module does not model
//!
//! Two `GetXMPTagID`/`ParseXMPElement` behaviours are deliberately omitted
//! rather than approximated, because modelling them faithfully needs tables
//! this path does not have. Both are inert on every file that reaches here;
//! see the module tests for the pinned reasoning.
//!
//! 1. **The `%uri2ns` prefix rewrite** (XMP.pm:3903-3976). If a document binds
//!    a prefix to a URI ExifTool has a *standard* prefix for, the standard one
//!    is substituted before naming. That needs ExifTool's whole `%nsURI` table.
//!    A plain XML file binding an Adobe XMP URI would arrive at the RDF parser
//!    instead, not here.
//! 2. **The all-uppercase exemption** (XMP.pm:3039-3048). A property name with
//!    no lowercase letter is lowercased (`_x` -> `X`) *unless* the namespace's
//!    own tag table already defines it. The lowercasing is implemented; the
//!    table exemption is not, since by construction this path only sees
//!    namespaces that have no ExifTool table.

use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};

use crate::core::tag_occurrence::Instance;
use crate::core::{FileReader, MetadataMap, TagValue};
use crate::error::{ExifToolError, Result};

/// Namespace prefixes that never contribute to a tag name
/// (XMP.pm:261, `%ignoreNamespace`).
const IGNORE_NAMESPACE: [&str; 6] = ["x", "rdf", "xmlns", "xml", "svg", "office"];

/// ExifTool's own control properties (XMP.pm:264-265, `%ignoreEtProp`). Unlike
/// [`super::struct_flatten::is_ignored_et_attr`], which resolves the `et`
/// prefix through its URI, `%ignoreEtProp` is keyed on the *literal* property
/// string -- and `GetXMPTagID` tests it that way, against the raw `$prop`. This
/// path reproduces the Perl's literal test.
const IGNORE_ET_PROP: [&str; 8] = [
    "et:desc",
    "et:prt",
    "et:val",
    "et:id",
    "et:tagid",
    "et:toolkit",
    "et:table",
    "et:index",
];

/// `%stdXlatNS` (XMP.pm:82-91): "shorten ugly namespace prefixes". Applied to
/// the group-1 namespace by `FoundXMP` at XMP.pm:3444.
const STD_XLAT_NS: [(&str, &str); 7] = [
    ("Iptc4xmpCore", "iptcCore"),
    ("Iptc4xmpExt", "iptcExt"),
    ("photomechanic", "photomech"),
    ("MicrosoftPhoto", "microsoft"),
    ("prismusagerights", "pur"),
    ("GettyImagesGIFT", "getty"),
    ("hdr_metadata", "hdr"),
];

/// One property on the path: the literal prefix and local name as they appear
/// in the document. `GetXMPTagID` splits `$prop` on the first `:` and works on
/// those two strings, so this stores exactly what it would see.
#[derive(Debug, Clone)]
struct Prop {
    prefix: String,
    local: String,
}

impl Prop {
    fn new(qname: &str) -> Self {
        match qname.split_once(':') {
            Some((prefix, local)) => Self {
                prefix: prefix.to_string(),
                local: local.to_string(),
            },
            None => Self {
                prefix: String::new(),
                local: qname.to_string(),
            },
        }
    }

    /// `$ignoreNamespace{$ns} or $ignoreEtProp{$prop}` (XMP.pm:3026).
    /// `%ignoreProp` is only ever populated from a SubDirectory's `IgnoreProp`
    /// (XMP.pm:4601-4603), which no plain-XML file has, so it is empty here.
    fn ignored(&self) -> bool {
        IGNORE_NAMESPACE.contains(&self.prefix.as_str())
            || IGNORE_ET_PROP.contains(&format!("{}:{}", self.prefix, self.local).as_str())
    }
}

/// A tag as ExifTool would report it for a plain XML file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct XmlProperty {
    /// Family-1 group: `XMP`, or `XMP-<prefix>`.
    pub group1: String,
    /// The reported tag name, `ucfirst` of the concatenated path.
    pub name: String,
    pub value: String,
}

/// `GetXMPTagID` (XMP.pm:3018-3070) plus `FoundXMP`'s closing
/// `ucfirst` (XMP.pm:3519) and `%stdXlatNS` group translation (XMP.pm:3444).
///
/// Returns `(name, group1-namespace)`, or `None` when no property contributed
/// -- `FoundXMP`'s own `return 0 unless $tag` (XMP.pm:3441), "ignore things
/// that aren't valid tags".
fn xmp_tag_id(props: &[Prop]) -> Option<(String, String)> {
    let mut tag: Option<String> = None;
    let mut namespace = String::new();

    for prop in props {
        if prop.ignored() {
            // The one exception: `rdf:_1`, `rdf:_2`, ... "not technically
            // allowed in XMP, but used in RDF/XML" (XMP.pm:3028-3030). It
            // appends the bare `_N` and, unlike every other ignored property,
            // falls through to the namespace line below.
            let Some(index) = prop
                .local
                .strip_prefix('_')
                .filter(|rest| !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit()))
                .filter(|_| prop.prefix == "rdf")
            else {
                continue;
            };
            if let Some(tag) = tag.as_mut() {
                tag.push('_');
                tag.push_str(index);
            }
        } else {
            let name = normalize_all_uppercase(&prop.local);
            match tag.as_mut() {
                // `$tag .= ucfirst($nm)`
                Some(tag) => tag.push_str(&ucfirst(&name)),
                // `$tag = $nm` -- the first segment is NOT ucfirst'd here;
                // `FoundXMP` does that once, at the end, to the whole ID.
                None => tag = Some(name),
            }
        }
        // `$namespace = $ns unless $namespace` -- reached only by a property
        // that contributed, since the ignore branch above `next`s past it.
        if namespace.is_empty() {
            namespace.clone_from(&prop.prefix);
        }
    }

    let tag = tag?;
    if tag.is_empty() {
        return None;
    }
    let namespace = STD_XLAT_NS
        .iter()
        .find(|(from, _)| *from == namespace)
        .map_or(namespace, |(_, to)| (*to).to_string());
    Some((ucfirst(&tag), namespace))
}

/// XMP.pm:3039-3048: "all uppercase is ugly, so convert it". A local name with
/// no lowercase letter is lowercased and `_x` becomes `X`.
///
/// The Perl skips this when the namespace's tag table already defines the name;
/// see this module's header for why that exemption cannot apply here.
fn normalize_all_uppercase(local: &str) -> String {
    if local.chars().any(|c| c.is_ascii_lowercase()) {
        return local.to_string();
    }
    let lowered = local.to_lowercase();
    let mut out = String::with_capacity(lowered.len());
    let mut chars = lowered.chars();
    while let Some(c) = chars.next() {
        if c == '_' {
            match chars.next() {
                Some(next) if next.is_ascii_lowercase() => {
                    out.extend(next.to_uppercase());
                }
                Some(next) => {
                    out.push('_');
                    out.push(next);
                }
                None => out.push('_'),
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn ucfirst(value: &str) -> String {
    let mut chars = value.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

/// `ConvertRational` (XMP.pm:3400-3416) followed by `ConvertXMPDate` with
/// `$unsure` set (XMP.pm:3383-3395), which is what `XMPAutoConv` applies to a
/// tag `FoundXMP` just minted (`$$tagInfo{IsDefault}`, XMP.pm:3677-3688).
///
/// `$unsure` matters: it suppresses `ConvertXMPDate`'s second branch, so a bare
/// `2003-05-24` is left alone and only a full date-*time* is reshaped.
fn auto_convert(value: &str) -> String {
    if let Some(converted) = convert_rational(value) {
        return converted;
    }
    convert_xmp_date_unsure(value)
}

fn convert_rational(value: &str) -> Option<String> {
    let (numerator, denominator) = value.split_once('/')?;
    let numerator: i64 = numerator.parse().ok()?;
    let denominator: i64 = denominator.parse().ok()?;
    Some(if denominator != 0 {
        super::rdf_parser::format_perl_g15(numerator as f64 / denominator as f64)
    } else if numerator != 0 {
        "inf".to_string()
    } else {
        "undef".to_string()
    })
}

/// `^(\d{4})-(\d{2})-(\d{2})[T ](\d{2}:\d{2})(:\d{2})?\s*(\S*)$` -> `$1:$2:$3 $4$5$6`
fn convert_xmp_date_unsure(value: &str) -> String {
    let bytes = value.as_bytes();
    let digits = |range: std::ops::Range<usize>| {
        bytes.len() >= range.end && bytes[range].iter().all(u8::is_ascii_digit)
    };
    // YYYY-MM-DD[T ]HH:MM is 16 characters before the optional seconds.
    if bytes.len() < 16
        || !digits(0..4)
        || bytes[4] != b'-'
        || !digits(5..7)
        || bytes[7] != b'-'
        || !digits(8..10)
        || !matches!(bytes[10], b'T' | b' ')
        || !digits(11..13)
        || bytes[13] != b':'
        || !digits(14..16)
    {
        return value.to_string();
    }
    let mut rest = &value[16..];
    let mut seconds = "";
    if rest.len() >= 3 && rest.as_bytes()[0] == b':' && digits(17..19) {
        seconds = &rest[..3];
        rest = &rest[3..];
    }
    // `\s*(\S*)$`: the remainder must be optional whitespace then a run with no
    // whitespace in it. Anything else fails the anchored match, and the Perl
    // leaves the value untouched.
    let trailing = rest.trim_start();
    if trailing.chars().any(char::is_whitespace) {
        return value.to_string();
    }
    format!(
        "{}:{}:{} {}{seconds}{trailing}",
        &value[0..4],
        &value[5..7],
        &value[8..10],
        &value[11..16],
    )
}

/// One element being walked, mirroring `ParseXMPElement`'s per-element state.
struct Frame {
    /// Text accumulated directly inside this element.
    text: String,
    /// Whether a child element was found inside it. `ParseXMPElement` reports a
    /// value only when the recursive call found none (XMP.pm:4171-4174), which
    /// is what keeps a container element from emitting its children's
    /// whitespace as a tag.
    had_child: bool,
    /// Whether any of this element's attributes already became a tag --
    /// ExifTool's `$shorthand`. The value is then reported only if it is
    /// non-empty (XMP.pm:4180, `if (length $val or not $shorthand)`), so
    /// `<Foo bar="1"/>` reports `Foobar` alone and not also an empty `Foo`.
    shorthand: bool,
}

/// Every tag ExifTool would report for the plain XML document in `bytes`, in
/// extraction order.
pub(crate) fn extract_xml_properties(bytes: &[u8]) -> Result<Vec<XmlProperty>> {
    let mut reader = Reader::from_reader(bytes);
    // No `trim_text`: `ParseXMPElement` takes the raw substring between the
    // tags. `xsi:schemaLocation` in Geotag.gpx spans two lines and ExifTool
    // reports the newline and its indentation verbatim.
    let mut buf = Vec::new();
    let mut props: Vec<Prop> = Vec::new();
    let mut frames: Vec<Frame> = Vec::new();
    let mut found: Vec<XmlProperty> = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                open_element(&e, &mut props, &mut frames, &mut found)?;
            }
            Ok(Event::Empty(e)) => {
                open_element(&e, &mut props, &mut frames, &mut found)?;
                close_element(&mut props, &mut frames, &mut found);
            }
            Ok(Event::End(_)) => close_element(&mut props, &mut frames, &mut found),
            Ok(Event::Text(e)) => {
                if let (Some(frame), Ok(text)) = (frames.last_mut(), e.xml10_content()) {
                    frame.text.push_str(
                        &quick_xml::escape::unescape(&text).unwrap_or_else(|_| text.clone()),
                    );
                }
            }
            Ok(Event::CData(e)) => {
                if let (Some(frame), Ok(text)) = (frames.last_mut(), std::str::from_utf8(&e)) {
                    // A CDATA section's contents are taken literally --
                    // `FoundXMP` unescapes only the text around it
                    // (XMP.pm:3655-3667).
                    frame.text.push_str(text);
                }
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(e) => {
                return Err(ExifToolError::parse_error(format!(
                    "Invalid XML structure: {e}"
                )));
            }
        }
        buf.clear();
    }

    Ok(found)
}

fn open_element(
    element: &BytesStart,
    props: &mut Vec<Prop>,
    frames: &mut Vec<Frame>,
    found: &mut Vec<XmlProperty>,
) -> Result<()> {
    if let Some(parent) = frames.last_mut() {
        parent.had_child = true;
    }
    let qname = std::str::from_utf8(element.name().as_ref())
        .map_err(|e| ExifToolError::parse_error(format!("Invalid UTF-8 in XML element name: {e}")))?
        .to_string();
    props.push(Prop::new(&qname));
    frames.push(Frame {
        text: String::new(),
        had_child: false,
        shorthand: false,
    });
    let shorthand = emit_attributes(element, &qname, props, found)?;
    if let Some(frame) = frames.last_mut() {
        frame.shorthand = shorthand;
    }
    Ok(())
}

/// `ParseXMPElement`'s shorthand-attribute loop (XMP.pm:4074-4148): each
/// attribute becomes a property one level below the element carrying it.
///
/// Returns ExifTool's `$shorthand` -- whether any attribute actually became a
/// tag, which decides whether the element's own empty value is reported.
fn emit_attributes(
    element: &BytesStart,
    parent_qname: &str,
    props: &mut Vec<Prop>,
    found: &mut Vec<XmlProperty>,
) -> Result<bool> {
    let mut shorthand = false;
    for attr in element.attributes().flatten() {
        let key = std::str::from_utf8(attr.key.as_ref()).map_err(|e| {
            ExifToolError::parse_error(format!("Invalid UTF-8 in XML attribute name: {e}"))
        })?;
        // XMP.pm:4077-4088: an unprefixed attribute inherits its element's
        // prefix, if the element has one. Otherwise it stays unprefixed -- and
        // `xmlns` on an unprefixed root is exactly that case, which is why
        // ExifTool reports `GpxXmlns` but drops `xmlns:xsi`.
        let prop = match key.split_once(':') {
            Some(_) => Prop::new(key),
            None => match parent_qname.split_once(':') {
                Some((prefix, _)) => Prop {
                    prefix: prefix.to_string(),
                    local: key.to_string(),
                },
                None => Prop::new(key),
            },
        };
        if prop.ignored() {
            continue;
        }
        let Ok(value) = std::str::from_utf8(&attr.value) else {
            continue;
        };
        let value = quick_xml::escape::unescape(value)
            .map(|v| v.into_owned())
            .unwrap_or_else(|_| value.to_string());
        props.push(prop);
        record(props, &value, found);
        props.pop();
        shorthand = true;
    }
    Ok(shorthand)
}

fn close_element(props: &mut Vec<Prop>, frames: &mut Vec<Frame>, found: &mut Vec<XmlProperty>) {
    let Some(frame) = frames.pop() else { return };
    // Only a leaf reports a value. An empty element still does -- `<description/>`
    // in Geotag.kml is reported by ExifTool with an empty value -- unless the
    // element's own attributes already became tags (XMP.pm:4180).
    if !frame.had_child && (!frame.text.is_empty() || !frame.shorthand) {
        record(props, &frame.text, found);
    }
    props.pop();
}

fn record(props: &[Prop], value: &str, found: &mut Vec<XmlProperty>) {
    let Some((name, namespace)) = xmp_tag_id(props) else {
        return;
    };
    found.push(XmlProperty {
        group1: if namespace.is_empty() {
            "XMP".to_string()
        } else {
            format!("XMP-{namespace}")
        },
        name,
        value: auto_convert(value),
    });
}

/// Parses a plain XML file (`FileType: XML`) the way `XMP::ProcessXMP` does.
///
/// No `File:FileType`/`FileTypeExtension`/`MIMEType` is set here. `SetFileType`
/// names the type inside `ProcessXMP` (XMP.pm:4425-4431), but on this side
/// `filetype::identify_text` already answers `XML`/`xml`/`application/xml` for
/// exactly the documents that reach this parser, and `operations`' Step 5a runs
/// `add_identity_tags` for every file whether its parser succeeded or not.
/// Setting them here too would report each of the three twice.
pub fn parse_xml_file(reader: &dyn FileReader) -> Result<MetadataMap> {
    let mut metadata = MetadataMap::new();
    let size = reader.size() as usize;
    let data = reader.read(0, size)?;

    // A malformed tail is not a reason to discard the tags already read: this
    // is a best-effort reader over documents with no schema at all.
    let properties = extract_xml_properties(data).unwrap_or_default();
    for property in properties {
        // One occurrence per element, not a joined list: a repeated element is
        // not an `rdf:Bag`, so ExifTool mints no `List` flag and `-a` prints
        // every `<trkpt>`. See this module's header.
        //
        // Priority 0, not the default 1, because that is what `FoundXMP` mints
        // an unknown tag with (XMP.pm:3595):
        //
        //     $tagInfo = { Name => $name, IsDefault => 1, Priority => 0 };
        //
        // and priority is what decides which of nine `<trkpt>` values wins the
        // bare key. `ExifTool.pm:9541-9551` promotes an *existing* 0-priority
        // incumbent to 1, so a later 0-priority arrival never displaces it and
        // the FIRST track point wins -- which is what the oracle's `-j` prints
        // (`KmlDocumentPlacemarkTrackWhen` is the first `<when>`, not the
        // last). At the default priority 1 the ties resolve to the newest and
        // every repeated key reported the last occurrence instead.
        // `TagSink::record` already implements both halves of that rule; this
        // only has to declare the priority honestly.
        metadata.insert_occurrence(
            format!("{}:{}", property.group1, property.name),
            TagValue::new_string(property.value),
            0,
            &property.group1,
            Instance::default(),
        );
    }

    Ok(metadata)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn extract(xml: &str) -> Vec<(String, String, String)> {
        extract_xml_properties(xml.as_bytes())
            .expect("extract")
            .into_iter()
            .map(|p| (p.group1, p.name, p.value))
            .collect()
    }

    /// The GPX shape from ExifTool's own `t/images/Geotag.gpx`, inlined.
    ///
    /// Pins three things at once: the path concatenation
    /// (`gpx/trk/trkseg/trkpt/lat` -> `GpxTrkTrksegTrkptLat`), that a repeated
    /// `<trkpt>` yields one occurrence per point rather than a joined list, and
    /// that `xsi:schemaLocation` lands in `XMP-xsi` while the unprefixed
    /// `xmlns` attribute lands in `XMP` as `GpxXmlns`.
    #[test]
    fn gpx_track_points_flatten_to_one_occurrence_each() {
        let found = extract(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<gpx
  xmlns="http://www.topografix.com/GPX/1/0"
  version="1.0" creator="Wissenbach Map3D 2.21"
  xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance"
  xsi:schemaLocation="http://www.topografix.com/GPX/1/0 gpx.xsd">
<trk><name>TESTTRK</name>
<cmt>Test Track</cmt>
<trkseg>
<trkpt lat="43.641000" lon="-116.062231"><ele>1472.396484</ele><time>2003-05-24T17:10:05Z</time></trkpt>
<trkpt lat="43.641086" lon="-116.062059"><ele>1474.319092</ele><time>2003-05-24T17:09:55Z</time></trkpt>
</trkseg>
</trk>
</gpx>"#,
        );

        assert_eq!(
            found,
            vec![
                // Root attributes, in document order. `xmlns:xsi` is dropped
                // (`%ignoreNamespace{xmlns}`) but the bare `xmlns` is not.
                (
                    "XMP".into(),
                    "GpxXmlns".into(),
                    "http://www.topografix.com/GPX/1/0".into()
                ),
                ("XMP".into(), "GpxVersion".into(), "1.0".into()),
                (
                    "XMP".into(),
                    "GpxCreator".into(),
                    "Wissenbach Map3D 2.21".into()
                ),
                (
                    "XMP-xsi".into(),
                    "GpxSchemaLocation".into(),
                    "http://www.topografix.com/GPX/1/0 gpx.xsd".into()
                ),
                ("XMP".into(), "GpxTrkName".into(), "TESTTRK".into()),
                ("XMP".into(), "GpxTrkCmt".into(), "Test Track".into()),
                (
                    "XMP".into(),
                    "GpxTrkTrksegTrkptLat".into(),
                    "43.641000".into()
                ),
                (
                    "XMP".into(),
                    "GpxTrkTrksegTrkptLon".into(),
                    "-116.062231".into()
                ),
                (
                    "XMP".into(),
                    "GpxTrkTrksegTrkptEle".into(),
                    "1472.396484".into()
                ),
                (
                    "XMP".into(),
                    "GpxTrkTrksegTrkptTime".into(),
                    // ConvertXMPDate reshapes the ISO form.
                    "2003:05:24 17:10:05Z".into()
                ),
                (
                    "XMP".into(),
                    "GpxTrkTrksegTrkptLat".into(),
                    "43.641086".into()
                ),
                (
                    "XMP".into(),
                    "GpxTrkTrksegTrkptLon".into(),
                    "-116.062059".into()
                ),
                (
                    "XMP".into(),
                    "GpxTrkTrksegTrkptEle".into(),
                    "1474.319092".into()
                ),
                (
                    "XMP".into(),
                    "GpxTrkTrksegTrkptTime".into(),
                    "2003:05:24 17:09:55Z".into()
                ),
            ]
        );
    }

    /// The KML shape: an unprefixed `<when>` nested inside a prefixed
    /// `<gx:Track>` takes the *first non-empty* prefix on the path for its
    /// group, so it reports under `XMP-gx` and not `XMP` -- and its name still
    /// omits the prefix, giving `KmlDocumentPlacemarkTrackWhen`. An empty
    /// `<description/>` is a reported tag with an empty value.
    #[test]
    fn kml_track_takes_group_from_first_prefixed_ancestor() {
        let found = extract(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<kml xmlns="http://www.opengis.net/kml/2.2" xmlns:gx="http://www.google.com/kml/ext/2.2">
<Document>
<open>1</open>
<description/>
<Placemark>
<gx:Track>
<altitudeMode>clampToGround</altitudeMode>
<when>2013-11-13T01:03:34.245-08:00</when>
<gx:coord>-106.026069 34.166232 0</gx:coord>
<when>2013-11-13T01:04:35.259-08:00</when>
<gx:coord>-106.027012 34.165476 0</gx:coord>
</gx:Track>
</Placemark>
</Document>
</kml>"#,
        );

        assert_eq!(
            found,
            vec![
                (
                    "XMP".into(),
                    "KmlXmlns".into(),
                    "http://www.opengis.net/kml/2.2".into()
                ),
                ("XMP".into(), "KmlDocumentOpen".into(), "1".into()),
                // Empty element, empty value -- still reported.
                ("XMP".into(), "KmlDocumentDescription".into(), String::new()),
                (
                    "XMP-gx".into(),
                    "KmlDocumentPlacemarkTrackAltitudeMode".into(),
                    "clampToGround".into()
                ),
                (
                    "XMP-gx".into(),
                    "KmlDocumentPlacemarkTrackWhen".into(),
                    // Seconds plus fractional part and offset all survive as
                    // ConvertXMPDate's `(\S*)$` tail.
                    "2013:11:13 01:03:34.245-08:00".into()
                ),
                (
                    "XMP-gx".into(),
                    "KmlDocumentPlacemarkTrackCoord".into(),
                    "-106.026069 34.166232 0".into()
                ),
                (
                    "XMP-gx".into(),
                    "KmlDocumentPlacemarkTrackWhen".into(),
                    "2013:11:13 01:04:35.259-08:00".into()
                ),
                (
                    "XMP-gx".into(),
                    "KmlDocumentPlacemarkTrackCoord".into(),
                    "-106.027012 34.165476 0".into()
                ),
            ]
        );
    }

    /// The Garmin ForerunnerLogbook shape: a six-deep unprefixed path, and a
    /// container element (`<Position>`) that must not emit its children's
    /// whitespace as a tag of its own.
    #[test]
    fn garmin_history_flattens_six_levels_without_emitting_containers() {
        let found = extract(
            r#"<?xml version="1.0" ?>
<History xmlns="http://www.garmin.com/xmlschemas/ForerunnerLogbook" version="1">
<Run>
<Track>
<Trackpoint>
<Position>
<Latitude>43.64986</Latitude>
<Longitude>-79.58321</Longitude>
</Position>
<Time>2004-08-28T13:45:00Z</Time>
</Trackpoint>
<Trackpoint>
<Position>
<Latitude>43.64987</Latitude>
<Longitude>-79.58320</Longitude>
</Position>
<Time>2004-08-28T14:15:00Z</Time>
</Trackpoint>
</Track>
</Run>
</History>"#,
        );

        let names: Vec<&str> = found.iter().map(|(_, name, _)| name.as_str()).collect();
        assert_eq!(
            names,
            vec![
                "HistoryXmlns",
                "HistoryVersion",
                "HistoryRunTrackTrackpointPositionLatitude",
                "HistoryRunTrackTrackpointPositionLongitude",
                "HistoryRunTrackTrackpointTime",
                "HistoryRunTrackTrackpointPositionLatitude",
                "HistoryRunTrackTrackpointPositionLongitude",
                "HistoryRunTrackTrackpointTime",
            ],
            "a container element must not report a tag of its own"
        );
        assert!(found.iter().all(|(group, _, _)| group == "XMP"));
        assert_eq!(found[4].2, "2004:08:28 13:45:00Z");
    }

    /// `%ignoreNamespace` drops the whole property, so an `rdf`/`x`-prefixed
    /// wrapper contributes nothing to the name -- while `rdf:_1`, the one
    /// documented exception (XMP.pm:3028-3030), appends its bare index.
    #[test]
    fn ignored_prefixes_contribute_nothing_except_rdf_numbered_items() {
        let found = extract(
            r#"<root xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
<rdf:Description><child>v</child></rdf:Description>
<bag><rdf:_1>first</rdf:_1></bag>
</root>"#,
        );
        let names: Vec<&str> = found.iter().map(|(_, name, _)| name.as_str()).collect();
        assert_eq!(names, vec!["RootChild", "RootBag_1"]);
    }

    /// XMP.pm:4180, `if (length $val or not $shorthand)`: an element whose
    /// attributes already became tags does not also report its own empty value,
    /// but one with no attributes does (`<description/>` in Geotag.kml).
    #[test]
    fn empty_element_reports_a_value_only_when_it_had_no_shorthand_attributes() {
        let found = extract("<root><a/><b bar=\"1\"/><c bar=\"1\">t</c></root>");
        assert_eq!(
            found,
            vec![
                ("XMP".into(), "RootA".into(), String::new()),
                ("XMP".into(), "RootBBar".into(), "1".into()),
                ("XMP".into(), "RootCBar".into(), "1".into()),
                ("XMP".into(), "RootC".into(), "t".into()),
            ],
            "`b` must not also report an empty RootB"
        );
    }

    /// `ConvertXMPDate` runs with `$unsure` set for an auto-converted tag, which
    /// suppresses the branch that would rewrite a bare date's dashes. A
    /// date-only value must survive untouched.
    #[test]
    fn autoconv_reshapes_datetimes_but_leaves_bare_dates_alone() {
        assert_eq!(auto_convert("2003-05-24T17:10:05Z"), "2003:05:24 17:10:05Z");
        assert_eq!(auto_convert("2003-05-24 17:10"), "2003:05:24 17:10");
        // No time part: `$unsure` blocks the tr/-/:/ branch.
        assert_eq!(auto_convert("2003-05-24"), "2003-05-24");
        // Whitespace inside the tail fails the anchored `\s*(\S*)$`.
        assert_eq!(
            auto_convert("2003-05-24T17:10:05 not a zone"),
            "2003-05-24T17:10:05 not a zone"
        );
        // ConvertRational takes precedence over the date test.
        assert_eq!(auto_convert("2272000/224"), "10142.8571428571");
        assert_eq!(auto_convert("1/0"), "inf");
        assert_eq!(auto_convert("0/0"), "undef");
        // Not a rational: a decimal is left exactly as written, trailing
        // zeros and all, because ExifTool never parses it as a number.
        assert_eq!(auto_convert("43.641000"), "43.641000");
    }

    /// "all uppercase is ugly" (XMP.pm:3039-3048).
    #[test]
    fn all_uppercase_local_names_are_lowercased_with_underscore_capitalisation() {
        assert_eq!(normalize_all_uppercase("TESTTRK"), "testtrk");
        assert_eq!(normalize_all_uppercase("FOO_BAR"), "fooBar");
        // Any lowercase letter at all disables the rule.
        assert_eq!(normalize_all_uppercase("Trkpt"), "Trkpt");
        assert_eq!(normalize_all_uppercase("altitudeMode"), "altitudeMode");
    }
}
