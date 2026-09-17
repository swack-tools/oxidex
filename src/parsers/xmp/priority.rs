//! ExifTool's FoundTag priority for an XMP property, computed from the
//! property path the way `XMP.pm` FoundXMP finds its tagInfo.
//!
//! FoundXMP (XMP.pm ~3437-3640):
//!
//! 1. `GetXMPTagID` builds the tag ID from the path: every property outside
//!    the ignored namespaces (`x`, `rdf`, `xmlns`, `xml`, `svg`, `office`)
//!    contributes its local name -- the first as-is, each later one
//!    `ucfirst`ed -- and an all-uppercase name is lower-cased and camel-cased
//!    unless its namespace table has that exact entry.
//! 2. The table is `%XMP::Main{ns}` for the first contributing property's
//!    prefix after `%stdXlatNS`; no table means `XMP::other`.
//! 3. `GetTagInfo(table, id)`, case-sensitive. The capture pre-flattens every
//!    table (`AddFlattenedTags`), which is what FoundXMP's retry does for the
//!    structure it meets.
//! 4. On a miss inside a structure: the innermost containing structure
//!    decides. A fixed-namespace structure leaves the field unknown. For a
//!    variable-namespace structure (`NAMESPACE => undef`) FoundXMP copies the
//!    field's tagInfo from the field's own namespace table and adds it to the
//!    structure's table (`AddTagToTable`), so the copy keeps its own
//!    `Priority` and `Avoid` but takes the structure table's `PRIORITY`
//!    (and `AVOID` when it had no `Avoid`).
//! 5. Anything still unknown is minted `{ Name => ..., Priority => 0 }`.
//!
//! FoundTag (ExifTool.pm ~9468) then uses `Priority`, else the table's
//! `PRIORITY`, else 0 for `Avoid`, else 1 -- precomputed for direct hits in
//! [`super::generated_priorities`].
//!
//! Not modelled: `PRIORITY_DIR`/`LOW_PRIORITY_DIR` (ExifTool.pm ~9553-9561),
//! conditional tag entries (none differ in the pinned XMP tables), and the
//! SVG/XML special tables.

use super::generated_priorities::{XmpStruct, xmp_ns, xmp_table};

/// Namespaces `GetXMPTagID` skips (XMP.pm `%ignoreNamespace`).
const IGNORED_NAMESPACES: [&str; 6] = ["x", "rdf", "xmlns", "xml", "svg", "office"];

/// One property of a path: its namespace prefix after ExifTool's translation
/// and `%stdXlatNS` (the `XMP-<prefix>` group suffix; empty for none) and its
/// raw local name exactly as written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PathProperty<'a> {
    pub namespace: &'a str,
    pub local: &'a str,
}

impl<'a> PathProperty<'a> {
    pub fn new(namespace: &'a str, local: &'a str) -> Self {
        Self { namespace, local }
    }
}

/// `GetXMPTagID`'s contribution of one property name.
fn id_part(namespace: &str, local: &str) -> String {
    // "remove nodeID if it exists"
    let name = local.split(' ').next().unwrap_or(local);
    if name.bytes().any(|b| b.is_ascii_lowercase()) {
        return name.to_string();
    }
    // "all uppercase is ugly, so convert it" -- unless the table has it.
    if xmp_table(namespace).is_some_and(|table| table.tag(name).is_some()) {
        return name.to_string();
    }
    let lower = name.to_ascii_lowercase();
    let mut out = String::with_capacity(lower.len());
    let mut chars = lower.chars().peekable();
    while let Some(c) = chars.next() {
        // `s/_([a-z])/\u$1/g`
        if c == '_'
            && let Some(&next) = chars.peek()
            && next.is_ascii_lowercase()
        {
            out.push(next.to_ascii_uppercase());
            chars.next();
            continue;
        }
        out.push(c);
    }
    out
}

/// Perl `ucfirst` on an ASCII-or-not string.
fn ucfirst(part: &str) -> String {
    let mut chars = part.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

/// The concatenated tag ID of `parts` (`$tag .= ucfirst($nm)`).
fn concat_id(parts: &[String]) -> String {
    let mut id = String::new();
    for (index, part) in parts.iter().enumerate() {
        if index == 0 {
            id.push_str(part);
        } else {
            id.push_str(&ucfirst(part));
        }
    }
    id
}

/// The FoundTag priority ExifTool gives the XMP property at `path` (outermost
/// property first, ignored-namespace elements such as `rdf:Bag` may be
/// included or omitted).
pub fn property_priority(path: &[PathProperty<'_>]) -> i8 {
    let contributing: Vec<&PathProperty<'_>> = path
        .iter()
        .filter(|property| !IGNORED_NAMESPACES.contains(&property.namespace))
        .collect();
    if contributing.is_empty() {
        return 0;
    }
    // `$namespace = $ns unless $namespace`: the first non-empty namespace.
    let Some(namespace) = contributing
        .iter()
        .map(|property| property.namespace)
        .find(|namespace| !namespace.is_empty())
    else {
        return 0;
    };
    let Some(table) = xmp_table(namespace) else {
        return 0; // XMP::other
    };
    let parts: Vec<String> = contributing
        .iter()
        .map(|property| id_part(property.namespace, property.local))
        .collect();
    let last = parts.len() - 1;
    if let Some(tag) = table.tag(&concat_id(&parts)) {
        return tag.priority;
    }
    // The innermost containing structure decides an unknown field.
    for index in (0..last).rev() {
        let Some(container) = table.tag(&concat_id(&parts[..=index])) else {
            continue;
        };
        match container.structure {
            XmpStruct::None => continue,
            XmpStruct::Fixed => return 0,
            XmpStruct::Variable => {}
        }
        let field_namespace = contributing[index + 1].namespace;
        let standard = xmp_ns(field_namespace).unwrap_or(field_namespace);
        if standard == table.namespace {
            return 0;
        }
        let Some(field_table) = xmp_table(field_namespace) else {
            return 0;
        };
        let Some(field) = field_table.tag(&concat_id(&parts[index + 1..])) else {
            return 0;
        };
        let avoid = field.avoid.or(table.avoid).unwrap_or(false);
        return field
            .own_priority
            .or(table.priority)
            .unwrap_or(if avoid { 0 } else { 1 });
    }
    0
}

/// [`property_priority`] for a single top-level property.
pub fn simple_property_priority(namespace: &str, local: &str) -> i8 {
    property_priority(&[PathProperty::new(namespace, local)])
}

/// The priority of a tag ExifTool extracts straight from a table entry
/// (`HandleTag` on XMP.pm `%recognizedAttrs`: `x:xmptk`, `rdf:about`).
pub fn table_entry_priority(namespace: &str, id: &str) -> i8 {
    xmp_table(namespace)
        .and_then(|table| table.tag(id))
        .map_or(0, |tag| tag.priority)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(path: &[(&str, &str)]) -> i8 {
        let path: Vec<PathProperty> = path.iter().map(|(n, l)| PathProperty::new(n, l)).collect();
        property_priority(&path)
    }

    #[test]
    fn raw_ids_are_case_sensitive() {
        // review packets k1/k2: cc:LegalCode and dc:Title are not the tables'
        // `legalcode`/`title` entries, so they are unknown (0).
        assert_eq!(p(&[("cc", "legalcode")]), 1);
        assert_eq!(p(&[("cc", "LegalCode")]), 0);
        assert_eq!(p(&[("dc", "title")]), 1);
        assert_eq!(p(&[("dc", "Title")]), 0);
        assert_eq!(p(&[("aaa", "Title")]), 0);
    }

    #[test]
    fn table_and_tag_priorities() {
        assert_eq!(p(&[("tiff", "NativeDigest")]), 0);
        assert_eq!(p(&[("xmp", "CreateDate")]), 0);
        assert_eq!(p(&[("pdf", "Keywords")]), -1);
        assert_eq!(p(&[("photomech", "CountryCode")]), 0);
        assert_eq!(p(&[("iptcCore", "CountryCode")]), 1);
        assert_eq!(table_entry_priority("x", "xmptk"), 1);
        assert_eq!(table_entry_priority("rdf", "about"), 1);
    }

    #[test]
    fn flattened_structure_ids() {
        // exif:Flash / exif:Fired -> FlashFired (exif PRIORITY 0).
        assert_eq!(
            p(&[("exif", "Flash"), ("rdf", "Description"), ("exif", "Fired")]),
            0
        );
        // xmpMM:DerivedFrom / stRef:documentID -> DerivedFromDocumentID.
        assert_eq!(p(&[("xmpMM", "DerivedFrom"), ("stRef", "documentID")]), 1);
        // An unknown field of a fixed-namespace structure is unknown.
        assert_eq!(p(&[("xmpMM", "DerivedFrom"), ("stRef", "bogus")]), 0);
    }

    #[test]
    fn variable_namespace_structure_fields_copy_their_own_table_entry() {
        // review packet v1: Iptc4xmpExt:ImageRegion / rdf:li / xmp:Rating.
        assert_eq!(
            p(&[
                ("iptcExt", "ImageRegion"),
                ("rdf", "Bag"),
                ("rdf", "li"),
                ("xmp", "Rating")
            ]),
            1
        );
        // xmp:CreateDate carries Priority => 0 into the copy.
        assert_eq!(p(&[("iptcExt", "ImageRegion"), ("xmp", "CreateDate")]), 0);
        // A field with no entry in its own table stays unknown.
        assert_eq!(p(&[("iptcExt", "ImageRegion"), ("xmp", "Bogus")]), 0);
        assert_eq!(p(&[("iptcExt", "ImageRegion"), ("zzz", "Rating")]), 0);
    }

    #[test]
    fn uppercase_names_are_camel_cased_unless_the_table_has_them() {
        assert_eq!(id_part("dc", "TITLE"), "title");
        assert_eq!(id_part("zzz", "MY_NAME"), "myName");
        assert_eq!(id_part("dc", "title rdf:nodeID"), "title");
    }
}
