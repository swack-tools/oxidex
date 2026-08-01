//! Generic flattening of nested XMP structures.
//!
//! ExifTool does not report an XMP structure as one tag. It walks the RDF
//! property path and concatenates a tag ID from it (`XMP.pm`, `GetXMPTagID`):
//! every property whose namespace prefix is not one of `x`, `rdf`, `xmlns`,
//! `xml`, `svg` or `office` contributes `ucfirst` of its local name, and the
//! rest -- `rdf:RDF`, `rdf:Description`, `rdf:Bag`/`Seq`/`Alt`, `rdf:li`,
//! `rdf:value`, `rdf:type` -- contribute nothing at all. So
//!
//! ```text
//! crs:Look / crs:Parameters / crs:LookTable          -> LookParametersLookTable
//! xmpMM:History / rdf:Seq / rdf:li / stEvt:action     -> HistoryAction
//! Device:Container / Container:Directory /
//!     rdf:Seq / rdf:li / rdf:value / Item:Mime        -> ContainerDirectoryMime
//! ```
//!
//! Before this module oxidex had one hand-written pass per structure it knew
//! about (JobRef, LocationShown, AboutCvTerm, ...), each with its own depth
//! assumptions, and reported nothing at all for the rest -- a structure it had
//! no pass for either vanished or, worse, surfaced as a single container tag
//! holding its fields' text run together, which ExifTool never emits.
//!
//! # FlatName overrides
//!
//! The concatenated ID is also the tag's *name*, except where a structure
//! declares `FlatName` (`XMP.pm`, `AddFlattenedTags`: the flattened name is
//! the parent's `FlatName // Name` followed by the field name). Only a handful
//! of schemas use it, and each rewrites a fixed prefix of the ID:
//!
//! | schema                  | ID prefix             | reported as |
//! |-------------------------|-----------------------|-------------|
//! | MWG regions             | `RegionsRegionList`   | `Region`    |
//! | MWG regions             | `Regions`             | `Region`    |
//! | MWG collections         | `Collections`         | (dropped)   |
//! | Google depth-map Device | `Cameras`/`Profiles`/`Planes` | (dropped) |
//!
//! The overrides are keyed on the namespace URI of the *first* path segment so
//! that an unrelated schema's `Regions` or `Cameras` property is left alone.

use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};

use super::namespace_resolver::NamespaceResolver;
use crate::error::{ExifToolError, Result};

/// Namespace prefixes that never contribute to a flattened tag ID
/// (`XMP.pm`, `%ignoreNamespace`).
const IGNORED_PREFIXES: [&str; 6] = ["x", "rdf", "xmlns", "xml", "svg", "office"];

const MWG_REGIONS_NS: &str = "http://www.metadataworkinggroup.com/schemas/regions/";
const MWG_COLLECTIONS_NS: &str = "http://www.metadataworkinggroup.com/schemas/collections/";
const MWG_KEYWORDS_NS: &str = "http://www.metadataworkinggroup.com/schemas/keywords/";
const GOOGLE_DEVICE_NS: &str = "http://ns.google.com/photos/dd/1.0/device/";

/// `(root namespace URI, ID prefix, replacement)` -- the schemas whose
/// structures carry a `FlatName`. Matched longest-prefix-first, and only when
/// something follows the prefix (the structure tag itself keeps its own name).
const FLAT_NAME_PREFIXES: &[(&str, &str, &str)] = &[
    // MWG.pm: Regions => { Name => 'RegionInfo', FlatName => 'Region', ... }
    // with RegionList => { FlatName => 'Region' } inside it, so both
    // `RegionsAppliedToDimensionsW` and `RegionsRegionListName` collapse onto
    // the same `Region` prefix.
    (MWG_REGIONS_NS, "RegionsRegionList", "Region"),
    (MWG_REGIONS_NS, "Regions", "Region"),
    // MWG.pm: Collections => { FlatName => '' } -- its fields are already
    // named CollectionName/CollectionURI.
    (MWG_COLLECTIONS_NS, "Collections", ""),
    // Google.pm: Cameras/Profiles/Planes => { FlatName => '' }, so the
    // singular field below each one starts the name.
    (GOOGLE_DEVICE_NS, "Cameras", ""),
    (GOOGLE_DEVICE_NS, "Profiles", ""),
    (GOOGLE_DEVICE_NS, "Planes", ""),
];

/// MWG hierarchical keywords: MWG.pm names each level of the unrolled
/// `Keywords/Hierarchy/Children...` ladder explicitly rather than by prefix.
const MWG_KEYWORD_IDS: &[(&str, &str)] = &[
    ("KeywordsHierarchyKeyword", "HierarchicalKeywords1"),
    ("KeywordsHierarchyApplied", "HierarchicalKeywords1Applied"),
    ("KeywordsHierarchyChildrenKeyword", "HierarchicalKeywords2"),
    (
        "KeywordsHierarchyChildrenApplied",
        "HierarchicalKeywords2Applied",
    ),
    (
        "KeywordsHierarchyChildrenChildrenKeyword",
        "HierarchicalKeywords3",
    ),
    (
        "KeywordsHierarchyChildrenChildrenApplied",
        "HierarchicalKeywords3Applied",
    ),
    (
        "KeywordsHierarchyChildrenChildrenChildrenKeyword",
        "HierarchicalKeywords4",
    ),
    (
        "KeywordsHierarchyChildrenChildrenChildrenApplied",
        "HierarchicalKeywords4Applied",
    ),
    (
        "KeywordsHierarchyChildrenChildrenChildrenChildrenKeyword",
        "HierarchicalKeywords5",
    ),
    (
        "KeywordsHierarchyChildrenChildrenChildrenChildrenApplied",
        "HierarchicalKeywords5Applied",
    ),
    (
        "KeywordsHierarchyChildrenChildrenChildrenChildrenChildrenKeyword",
        "HierarchicalKeywords6",
    ),
    (
        "KeywordsHierarchyChildrenChildrenChildrenChildrenChildrenApplied",
        "HierarchicalKeywords6Applied",
    ),
];

/// Structure fields ExifTool reports under a different local name than the one
/// in the file, keyed on the field's own namespace URI.
const FIELD_RENAMES: &[(&str, &str, &str)] = &[
    // XMP2.pl, %Image::ExifTool::XMP::apple_fi: Timestamp => { Name => 'TimeStamp' }
    (
        "http://ns.apple.com/faceinfo/1.0/",
        "Timestamp",
        "TimeStamp",
    ),
];

/// One element on the RDF property path.
struct Frame {
    /// This element's contribution to the flattened tag ID, or `None` when its
    /// namespace prefix is one ExifTool ignores.
    part: Option<String>,
    /// Namespace URI backing `part`, used to scope the FlatName overrides.
    uri: Option<String>,
    /// `xml:lang` carried by this element (an `rdf:li` of a lang-alt).
    lang: Option<String>,
    text: String,
    child_elements: usize,
    /// Whether this element carried shorthand attributes of its own, which
    /// already became fields one level down.
    has_fields: bool,
}

/// Extracts every structure field in `xml_bytes` as `("XMP:<FlatName>", value)`.
///
/// Only paths with two or more contributing segments are reported: a one-segment
/// path is a plain top-level property, which the main RDF pass already handles
/// (and handles better, since it knows the schema-specific value formatting).
///
/// Repeated IDs -- the same field in every `rdf:li` of a Bag or Seq -- are
/// collected into one List-valued tag joined with `", "`, the way ExifTool's own
/// text output joins a List.
pub fn extract_flattened_struct_fields(xml_bytes: &[u8]) -> Result<Vec<(String, Vec<String>)>> {
    let mut reader = Reader::from_reader(xml_bytes);
    reader.config_mut().trim_text(true);

    let mut resolver = NamespaceResolver::new();
    let mut buf = Vec::new();
    let mut stack: Vec<Frame> = Vec::new();
    // (flattened id, values) in first-seen order.
    let mut collected: Vec<(String, Vec<String>)> = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                let frame = push_frame(&e, &mut resolver, &mut stack)?;
                // The frame must be on the stack BEFORE its own attributes are
                // read: an RDF shorthand attribute is a field of the element
                // carrying it, so `<Camera:DepthMap DepthMap:Far="0.32"/>`
                // is CamerasCameraDepthMapFar, not CamerasCameraFar.
                stack.push(frame);
                emit_attributes(&e, &resolver, &stack, &mut collected)?;
            }

            Ok(Event::Empty(e)) => {
                let frame = push_frame(&e, &mut resolver, &mut stack)?;
                stack.push(frame);
                emit_attributes(&e, &resolver, &stack, &mut collected)?;
                close_frame(&mut stack, &mut collected);
            }

            Ok(Event::Text(e)) => {
                if let (Some(frame), Ok(decoded)) = (stack.last_mut(), e.xml10_content()) {
                    let unescaped = quick_xml::escape::unescape(&decoded)
                        .unwrap_or_else(|_| decoded.clone())
                        .into_owned();
                    frame.text.push_str(&unescaped);
                }
            }

            Ok(Event::End(_)) => close_frame(&mut stack, &mut collected),

            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(e) => {
                return Err(ExifToolError::parse_error(format!(
                    "Invalid XMP XML structure: {}",
                    e
                )));
            }
        }
        buf.clear();
    }

    Ok(collected
        .into_iter()
        .map(|(id, values)| (format!("XMP:{id}"), values))
        .collect())
}

/// Builds the [`Frame`] for `element` and counts it against its parent.
fn push_frame(
    element: &BytesStart,
    resolver: &mut NamespaceResolver,
    stack: &mut [Frame],
) -> Result<Frame> {
    register_namespaces(element, resolver)?;

    let name = element.name();
    let qname = std::str::from_utf8(name.as_ref()).map_err(|e| {
        ExifToolError::parse_error(format!("Invalid UTF-8 in XMP element name: {}", e))
    })?;
    let prefix = NamespaceResolver::extract_prefix(qname).unwrap_or("");
    let ignored = IGNORED_PREFIXES.contains(&prefix);

    if let Some(parent) = stack.last_mut() {
        parent.child_elements += 1;
    }

    let uri = resolver.resolve_prefix(prefix).map(str::to_string);
    Ok(Frame {
        part: (!ignored).then(|| {
            let local = NamespaceResolver::extract_local_name(qname);
            tag_id_segment(rename_field(uri.as_deref(), local))
        }),
        uri,
        lang: lang_attribute(element)?,
        // An element with no content of its own takes its value from
        // rdf:resource (XMP.pm's ParseXMPElement: "if element value is empty,
        // take value from RDF 'value' or 'resource' attribute"). That is how
        // mwg-rs:seeAlso carries "plus:Licensee" in XMP5.xmp.
        text: attribute_value(element, b"rdf:resource")?.unwrap_or_default(),
        child_elements: 0,
        has_fields: element.attributes().flatten().any(|attr| {
            std::str::from_utf8(attr.key.as_ref())
                .ok()
                .and_then(|key| key.split_once(':'))
                .is_some_and(|(prefix, _)| !IGNORED_PREFIXES.contains(&prefix))
        }),
    })
}

/// Pops the innermost frame, reporting its text as a leaf value when it had no
/// child elements of its own.
fn close_frame(stack: &mut Vec<Frame>, collected: &mut Vec<(String, Vec<String>)>) {
    let Some(frame) = stack.pop() else {
        return;
    };
    if frame.child_elements != 0 {
        return;
    }
    let value = frame.text.trim().to_string();
    if value.is_empty() {
        // An empty struct field -- `<mwg-rs:Extensions rdf:parseType="Resource"/>`
        // -- is still a tag ExifTool reports, with an empty value. Only a
        // NAMED element qualifies: an empty `<rdf:Seq/>` contributes nothing to
        // the ID and would otherwise blank out its parent. An element whose
        // shorthand attributes already became fields is not itself a leaf.
        if frame.part.is_none() || frame.has_fields {
            return;
        }
    }
    stack.push(frame);
    if let Some(tag) = flat_tag_name(stack, None) {
        record(collected, tag, value);
    }
    stack.pop();
}

/// Reports RDF shorthand attributes (`<Item:Container Item:Mime="image/jpeg"/>`)
/// as struct fields one level below the element carrying them.
fn emit_attributes(
    element: &BytesStart,
    resolver: &NamespaceResolver,
    stack: &[Frame],
    collected: &mut Vec<(String, Vec<String>)>,
) -> Result<()> {
    for attr in element.attributes().flatten() {
        let key = std::str::from_utf8(attr.key.as_ref()).map_err(|e| {
            ExifToolError::parse_error(format!("Invalid UTF-8 in XMP attribute name: {}", e))
        })?;
        let Some((prefix, local)) = key.split_once(':') else {
            continue; // unprefixed attributes inherit the element's namespace
        };
        if IGNORED_PREFIXES.contains(&prefix) {
            continue;
        }
        let Ok(value) = std::str::from_utf8(&attr.value) else {
            continue;
        };
        let value = value.trim();
        if value.is_empty() {
            continue;
        }
        let uri = resolver.resolve_prefix(prefix);
        let leaf = rename_field(uri, local);
        if let Some(tag) = flat_tag_name(stack, Some(&tag_id_segment(leaf))) {
            record(collected, tag, value.to_string());
        }
    }
    Ok(())
}

/// Builds the reported tag name for the current path, or `None` when the path
/// is not a structure field.
fn flat_tag_name(stack: &[Frame], extra_segment: Option<&str>) -> Option<String> {
    let mut id = String::new();
    let mut segments = 0usize;
    let mut root_uri: Option<&str> = None;

    for frame in stack {
        let Some(part) = &frame.part else { continue };
        if segments == 0 {
            root_uri = frame.uri.as_deref();
        }
        segments += 1;
        id.push_str(part);
    }
    if let Some(extra) = extra_segment {
        if segments == 0 {
            return None; // an attribute on rdf:Description is a plain property
        }
        segments += 1;
        id.push_str(extra);
    }
    if segments < 2 || id.is_empty() {
        return None;
    }

    // The innermost xml:lang wins: it sits on the rdf:li of a lang-alt.
    let lang = stack.iter().rev().find_map(|f| f.lang.as_deref());

    let name = apply_flat_name(root_uri.unwrap_or(""), &id);
    if name.is_empty() {
        return None;
    }
    match lang {
        Some(lang) if lang != "x-default" => Some(format!("{name}-{lang}")),
        _ => Some(name),
    }
}

/// Applies the schema `FlatName` rewrites to a concatenated tag ID.
fn apply_flat_name(root_uri: &str, id: &str) -> String {
    if root_uri == MWG_KEYWORDS_NS
        && let Some((_, name)) = MWG_KEYWORD_IDS.iter().find(|(raw, _)| *raw == id)
    {
        return (*name).to_string();
    }
    let mut best: Option<(&str, &str)> = None;
    for (uri, prefix, replacement) in FLAT_NAME_PREFIXES {
        if *uri != root_uri || id.len() <= prefix.len() || !id.starts_with(prefix) {
            continue;
        }
        if best.is_none_or(|(current, _)| prefix.len() > current.len()) {
            best = Some((prefix, replacement));
        }
    }
    match best {
        Some((prefix, replacement)) => format!("{replacement}{}", &id[prefix.len()..]),
        None => id.to_string(),
    }
}

/// One path segment's contribution: `ucfirst` of the local name, minus the
/// U+2182 escape marker XMP uses for otherwise-invalid name characters.
fn tag_id_segment(local: &str) -> String {
    let mut cleaned: String = local.chars().filter(|ch| *ch != '\u{2182}').collect();
    if let Some(first) = cleaned.chars().next() {
        let upper: String = first.to_uppercase().collect();
        cleaned.replace_range(..first.len_utf8(), &upper);
    }
    cleaned
}

fn rename_field<'a>(uri: Option<&str>, local: &'a str) -> &'a str {
    let Some(uri) = uri else { return local };
    FIELD_RENAMES
        .iter()
        .find(|(u, l, _)| *u == uri && *l == local)
        // The replacement is a 'static str, which outlives 'a.
        .map_or(local, |(_, _, renamed)| *renamed)
}

/// Resolves RDF blank nodes: properties whose value is an `rdf:nodeID`
/// reference rather than an inline structure.
///
/// RDF/XML lets one resource be described in several places and stitched back
/// together by a shared `rdf:nodeID`. ExifTool implements this with
/// `SaveBlankInfo`/`ProcessBlankInfo` (`XMP.pm`): every field written against a
/// node ID is collected, and each property that *references* that node reports
/// the whole union. XMP3.xmp writes `ph:supervisor`, `ph:programmer` and
/// `ph:tester` as three references to one node `abc`, and ExifTool reports the
/// same five fields under all three names.
///
/// Fields defined under a node ID are deliberately NOT reported on their own:
/// `ParseXMPElement` diverts them into the blank-node table instead of calling
/// `FoundXMP`.
pub fn extract_blank_node_fields(xml_bytes: &[u8]) -> Result<Vec<(String, String)>> {
    let mut reader = Reader::from_reader(xml_bytes);
    reader.config_mut().trim_text(true);

    let mut resolver = NamespaceResolver::new();
    let mut buf = Vec::new();
    let mut stack: Vec<Frame> = Vec::new();
    // nodeID -> (field name, value), in first-seen order.
    let mut nodes: Vec<(String, Vec<(String, String)>)> = Vec::new();
    // (tag-name prefix, nodeID) for each property referencing a node.
    let mut references: Vec<(String, String)> = Vec::new();
    // Depth at which the innermost node definition started, and its ID.
    let mut node_scope: Vec<(usize, String)> = Vec::new();

    loop {
        let event = reader.read_event_into(&mut buf);
        match event {
            Ok(Event::Start(ref e)) | Ok(Event::Empty(ref e)) => {
                let is_empty = matches!(event, Ok(Event::Empty(_)));
                let frame = push_frame(e, &mut resolver, &mut stack)?;
                stack.push(frame);

                let node_id = attribute_value(e, b"rdf:nodeID")?;
                if let Some(node_id) = node_id {
                    // A named property carrying the reference itself
                    // (`<ph:tester rdf:nodeID="abc"/>`), or an rdf:Description
                    // that both defines the node and, when nested inside a
                    // property, is that property's value.
                    if let Some(prefix) = reference_prefix(&stack) {
                        references.push((prefix, node_id.clone()));
                    }
                    if nodes.iter().all(|(id, _)| *id != node_id) {
                        nodes.push((node_id.clone(), Vec::new()));
                    }
                    // Attributes on the node element are fields of the node.
                    for (name, value) in shorthand_fields(e, &resolver)? {
                        push_node_field(&mut nodes, &node_id, name, value);
                    }
                    if !is_empty {
                        node_scope.push((stack.len(), node_id));
                    }
                } else if let Some((scope_depth, id)) = node_scope.last().cloned()
                    && stack.len() == scope_depth + 1
                    && let Some(part) = stack[stack.len() - 1].part.clone()
                {
                    // A direct child element of a node definition is one of its
                    // fields; an rdf:resource attribute stands in for the value.
                    if let Some(resource) = attribute_value(e, b"rdf:resource")? {
                        push_node_field(&mut nodes, &id, part, resource);
                    }
                }

                if is_empty {
                    stack.pop();
                }
            }

            Ok(Event::Text(ref e)) => {
                if let (Some(frame), Ok(decoded)) = (stack.last_mut(), e.xml10_content()) {
                    let unescaped = quick_xml::escape::unescape(&decoded)
                        .unwrap_or_else(|_| decoded.clone())
                        .into_owned();
                    frame.text.push_str(&unescaped);
                }
            }

            Ok(Event::End(_)) => {
                if let Some(frame) = stack.pop() {
                    if let Some((scope_depth, id)) = node_scope.last().cloned()
                        && stack.len() == scope_depth
                        && let Some(part) = &frame.part
                    {
                        let value = frame.text.trim();
                        if !value.is_empty() {
                            push_node_field(&mut nodes, &id, part.clone(), value.to_string());
                        }
                    }
                    if node_scope
                        .last()
                        .is_some_and(|(d, _)| *d == stack.len() + 1)
                    {
                        node_scope.pop();
                    }
                }
            }

            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(e) => {
                return Err(ExifToolError::parse_error(format!(
                    "Invalid XMP XML structure: {}",
                    e
                )));
            }
        }
        buf.clear();
    }

    let mut out = Vec::new();
    for (prefix, node_id) in &references {
        let Some((_, fields)) = nodes.iter().find(|(id, _)| id == node_id) else {
            continue;
        };
        for (field, value) in fields {
            let tag = format!("XMP:{prefix}{field}");
            if !out.iter().any(|(t, _): &(String, String)| *t == tag) {
                out.push((tag, value.clone()));
            }
        }
    }
    Ok(out)
}

/// The flattened tag-name prefix a blank-node reference hangs off: the path
/// built from every contributing segment, which for `<ph:supervisor>` is just
/// `Supervisor`. `None` when the reference is not inside any named property
/// (a top-level node definition defines fields but names no tag).
fn reference_prefix(stack: &[Frame]) -> Option<String> {
    let mut prefix = String::new();
    for frame in stack {
        if let Some(part) = &frame.part {
            prefix.push_str(part);
        }
    }
    (!prefix.is_empty()).then_some(prefix)
}

fn push_node_field(
    nodes: &mut Vec<(String, Vec<(String, String)>)>,
    node_id: &str,
    name: String,
    value: String,
) {
    let entry = match nodes.iter_mut().find(|(id, _)| id == node_id) {
        Some(entry) => entry,
        None => {
            nodes.push((node_id.to_string(), Vec::new()));
            nodes
                .last_mut()
                .expect("just pushed a node entry, so last_mut cannot be None")
        }
    };
    if !entry.1.iter().any(|(n, _)| *n == name) {
        entry.1.push((name, value));
    }
}

/// The non-ignored prefixed attributes of `element` as `(FieldName, value)`.
fn shorthand_fields(
    element: &BytesStart,
    resolver: &NamespaceResolver,
) -> Result<Vec<(String, String)>> {
    let mut out = Vec::new();
    for attr in element.attributes().flatten() {
        let Ok(key) = std::str::from_utf8(attr.key.as_ref()) else {
            continue;
        };
        let Some((prefix, local)) = key.split_once(':') else {
            continue;
        };
        if IGNORED_PREFIXES.contains(&prefix) {
            continue;
        }
        let Ok(value) = std::str::from_utf8(&attr.value) else {
            continue;
        };
        let value = value.trim();
        if value.is_empty() {
            continue;
        }
        let uri = resolver.resolve_prefix(prefix);
        out.push((tag_id_segment(rename_field(uri, local)), value.to_string()));
    }
    Ok(out)
}

fn attribute_value(element: &BytesStart, name: &[u8]) -> Result<Option<String>> {
    for attr in element.attributes().flatten() {
        if attr.key.as_ref() == name {
            let value = std::str::from_utf8(&attr.value).map_err(|e| {
                ExifToolError::parse_error(format!("Invalid UTF-8 in XMP attribute: {}", e))
            })?;
            return Ok(Some(value.trim().to_string()));
        }
    }
    Ok(None)
}

fn record(collected: &mut Vec<(String, Vec<String>)>, tag: String, value: String) {
    match collected.iter_mut().find(|(id, _)| *id == tag) {
        Some((_, values)) => values.push(value),
        None => collected.push((tag, vec![value])),
    }
}

fn lang_attribute(element: &BytesStart) -> Result<Option<String>> {
    for attr in element.attributes().flatten() {
        if attr.key.as_ref() == b"xml:lang" {
            let value = std::str::from_utf8(&attr.value).map_err(|e| {
                ExifToolError::parse_error(format!("Invalid UTF-8 in xml:lang: {}", e))
            })?;
            return Ok(Some(value.trim().to_string()));
        }
    }
    Ok(None)
}

fn register_namespaces(element: &BytesStart, resolver: &mut NamespaceResolver) -> Result<()> {
    for attr in element.attributes().flatten() {
        let Ok(key) = std::str::from_utf8(attr.key.as_ref()) else {
            continue;
        };
        let prefix = if let Some(prefix) = key.strip_prefix("xmlns:") {
            prefix
        } else if key == "xmlns" {
            ""
        } else {
            continue;
        };
        let uri = std::str::from_utf8(&attr.value).map_err(|e| {
            ExifToolError::parse_error(format!("Invalid UTF-8 in namespace URI: {}", e))
        })?;
        resolver.register_namespace(prefix, uri);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn value_of(tags: &[(String, Vec<String>)], tag: &str) -> Option<String> {
        tags.iter()
            .find(|(t, _)| t == tag)
            .map(|(_, v)| v.join(", "))
    }

    const MWG_REGION_XMP: &[u8] = br#"<x:xmpmeta xmlns:x="adobe:ns:meta/">
 <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
  <rdf:Description rdf:about=""
      xmlns:mwg-rs="http://www.metadataworkinggroup.com/schemas/regions/"
      xmlns:stDim="http://ns.adobe.com/xap/1.0/sType/Dimensions#"
      xmlns:apple-fi="http://ns.apple.com/faceinfo/1.0/"
      xmlns:stArea="http://ns.adobe.com/xmp/sType/Area#">
   <mwg-rs:Regions rdf:parseType="Resource">
    <mwg-rs:AppliedToDimensions rdf:parseType="Resource">
     <stDim:w>3264</stDim:w>
     <stDim:h>2448</stDim:h>
     <stDim:unit>pixel</stDim:unit>
    </mwg-rs:AppliedToDimensions>
    <mwg-rs:RegionList>
     <rdf:Bag>
      <rdf:li rdf:parseType="Resource">
       <mwg-rs:Extensions rdf:parseType="Resource">
        <apple-fi:Timestamp>-1179414036</apple-fi:Timestamp>
       </mwg-rs:Extensions>
       <mwg-rs:Area rdf:parseType="Resource">
        <stArea:x>0.578</stArea:x>
       </mwg-rs:Area>
       <mwg-rs:Type>Face</mwg-rs:Type>
      </rdf:li>
     </rdf:Bag>
    </mwg-rs:RegionList>
   </mwg-rs:Regions>
  </rdf:Description>
 </rdf:RDF>
</x:xmpmeta>"#;

    #[test]
    fn mwg_regions_use_their_flat_name_prefix() {
        let tags = extract_flattened_struct_fields(MWG_REGION_XMP).unwrap();
        assert_eq!(
            value_of(&tags, "XMP:RegionAppliedToDimensionsW").as_deref(),
            Some("3264")
        );
        assert_eq!(value_of(&tags, "XMP:RegionAreaX").as_deref(), Some("0.578"));
        assert_eq!(value_of(&tags, "XMP:RegionType").as_deref(), Some("Face"));
        // apple-fi:Timestamp is reported as TimeStamp (XMP2.pl).
        assert_eq!(
            value_of(&tags, "XMP:RegionExtensionsTimeStamp").as_deref(),
            Some("-1179414036")
        );
        // The un-rewritten concatenations must not leak out.
        assert!(value_of(&tags, "XMP:RegionsAppliedToDimensionsW").is_none());
        assert!(value_of(&tags, "XMP:RegionsRegionListType").is_none());
    }

    #[test]
    fn a_bag_of_structs_becomes_one_list_valued_tag_per_field() {
        const XMP: &[u8] = br#"<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
 <rdf:Description xmlns:xmpMM="http://ns.adobe.com/xap/1.0/mm/"
     xmlns:stEvt="http://ns.adobe.com/xap/1.0/sType/ResourceEvent#">
  <xmpMM:History>
   <rdf:Seq>
    <rdf:li rdf:parseType="Resource"><stEvt:action>saved</stEvt:action></rdf:li>
    <rdf:li rdf:parseType="Resource"><stEvt:action>derived</stEvt:action></rdf:li>
   </rdf:Seq>
  </xmpMM:History>
 </rdf:Description>
</rdf:RDF>"#;
        let tags = extract_flattened_struct_fields(XMP).unwrap();
        assert_eq!(
            value_of(&tags, "XMP:HistoryAction").as_deref(),
            Some("saved, derived")
        );
    }

    #[test]
    fn google_device_cameras_drop_their_container_name() {
        const XMP: &[u8] = br#"<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
 <rdf:Description xmlns:Device="http://ns.google.com/photos/dd/1.0/device/"
     xmlns:Camera="http://ns.google.com/photos/dd/1.0/camera/"
     xmlns:DepthMap="http://ns.google.com/photos/dd/1.0/depthmap/">
  <Device:Cameras>
   <rdf:Seq>
    <rdf:li rdf:parseType="Resource">
     <rdf:value rdf:parseType="Resource">
      <Camera:Trait>Physical</Camera:Trait>
      <Camera:DepthMap rdf:parseType="Resource">
       <DepthMap:Far>6.145783</DepthMap:Far>
      </Camera:DepthMap>
     </rdf:value>
     <rdf:type>http://ns.google.com/photos/dd/1.0/device/:Camera</rdf:type>
    </rdf:li>
   </rdf:Seq>
  </Device:Cameras>
 </rdf:Description>
</rdf:RDF>"#;
        let tags = extract_flattened_struct_fields(XMP).unwrap();
        assert_eq!(value_of(&tags, "XMP:Trait").as_deref(), Some("Physical"));
        assert_eq!(
            value_of(&tags, "XMP:DepthMapFar").as_deref(),
            Some("6.145783")
        );
        assert!(value_of(&tags, "XMP:CamerasTrait").is_none());
    }

    #[test]
    fn rdf_shorthand_attributes_are_struct_fields_too() {
        const XMP: &[u8] = br#"<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
 <rdf:Description xmlns:GContainer="http://ns.google.com/photos/1.0/container/"
     xmlns:GItem="http://ns.google.com/photos/1.0/container/item/">
  <GContainer:Directory>
   <rdf:Seq>
    <rdf:li rdf:parseType="Resource">
     <GContainer:Item GItem:Mime="image/jpeg" GItem:Semantic="Primary"/>
    </rdf:li>
   </rdf:Seq>
  </GContainer:Directory>
 </rdf:Description>
</rdf:RDF>"#;
        let tags = extract_flattened_struct_fields(XMP).unwrap();
        assert_eq!(
            value_of(&tags, "XMP:DirectoryItemMime").as_deref(),
            Some("image/jpeg")
        );
        assert_eq!(
            value_of(&tags, "XMP:DirectoryItemSemantic").as_deref(),
            Some("Primary")
        );
    }

    #[test]
    fn a_plain_top_level_property_is_left_to_the_main_pass() {
        const XMP: &[u8] = br#"<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
 <rdf:Description xmlns:dc="http://purl.org/dc/elements/1.1/">
  <dc:title><rdf:Alt><rdf:li xml:lang="x-default">Hello</rdf:li></rdf:Alt></dc:title>
 </rdf:Description>
</rdf:RDF>"#;
        let tags = extract_flattened_struct_fields(XMP).unwrap();
        assert!(tags.is_empty(), "unexpected tags: {tags:?}");
    }

    #[test]
    fn a_lang_alt_inside_a_struct_keeps_its_language_suffix() {
        const XMP: &[u8] = br#"<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
 <rdf:Description xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/">
  <crs:Look rdf:parseType="Resource">
   <crs:Name><rdf:Alt>
     <rdf:li xml:lang="x-default">Adobe Color</rdf:li>
     <rdf:li xml:lang="de-DE">Adobe Farbe</rdf:li>
   </rdf:Alt></crs:Name>
  </crs:Look>
 </rdf:Description>
</rdf:RDF>"#;
        let tags = extract_flattened_struct_fields(XMP).unwrap();
        assert_eq!(
            value_of(&tags, "XMP:LookName").as_deref(),
            Some("Adobe Color")
        );
        assert_eq!(
            value_of(&tags, "XMP:LookName-de-DE").as_deref(),
            Some("Adobe Farbe")
        );
    }
}
