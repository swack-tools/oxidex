"""Generate the facts ExifTool's XMP read path uses to give a property its priority.

Input is capture_xmp_priorities.pl's JSON for the pinned tree (committed as
fixtures/xmp_priorities_13_59.json). Output is src/parsers/xmp/generated_priorities.rs:

- XMP_TABLES: for every %Image::ExifTool::XMP::Main key that names a tag table
  (the namespace prefix after %stdXlatNS -- FoundXMP's table lookup key), the
  table's NAMESPACE, PRIORITY and AVOID, and every raw tag ID after
  AddFlattenedTags (FoundXMP's case-sensitive GetTagInfo key) with the
  effective FoundTag priority of a direct hit, the tag's own Priority and
  Avoid (needed when FoundXMP copies a variable-namespace structure field's
  tagInfo into another table), and whether the entry is a structure with a
  fixed or variable namespace;
- XMP_NS: XMP.pm's %xmpNS (group prefix -> standard XMP prefix).

The ID construction and lookup order live in src/parsers/xmp/priority.rs.
"""
from __future__ import annotations

import argparse
import json
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
FIXTURE = ROOT / "tools/exiftool-tables/fixtures/xmp_priorities_13_59.json"
RUST = ROOT / "src/parsers/xmp/generated_priorities.rs"
SCHEMA_KEYS = {"exiftool_version", "namespaces", "xmp_ns"}
NAMESPACE_KEYS = {"table", "namespace", "table_priority", "table_avoid", "tags"}
TAG_KEYS = {"name", "priority", "own_priority", "avoid", "struct"}
STRUCT_KINDS = {"none": "XmpStruct::None", "fixed": "XmpStruct::Fixed", "variable": "XmpStruct::Variable"}
PRINTABLE = re.compile(r"[\x20-\x7e]+")

TESTS = """#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tables_are_sorted_for_binary_search() {
        assert!(XMP_TABLES.windows(2).all(|pair| pair[0].key < pair[1].key));
        for table in XMP_TABLES {
            assert!(table.tags.windows(2).all(|pair| pair[0].id < pair[1].id));
        }
        assert!(XMP_NS.windows(2).all(|pair| pair[0].0 < pair[1].0));
    }

    #[test]
    fn tables_are_keyed_by_translated_prefix_and_raw_tag_id() {
        let tiff = xmp_table("tiff").unwrap();
        assert_eq!(tiff.priority, Some(0));
        assert_eq!(tiff.tag("NativeDigest").map(|tag| tag.priority), Some(0));
        let pdf = xmp_table("pdf").unwrap();
        assert_eq!(pdf.tag("Keywords").map(|tag| tag.own_priority), Some(Some(-1)));
        // Raw IDs, case-sensitive: cc:legalcode is reported as LegalCode.
        let cc = xmp_table("cc").unwrap();
        assert!(cc.tag("legalcode").is_some());
        assert!(cc.tag("LegalCode").is_none());
        assert_eq!(xmp_table("iptcCore").unwrap().namespace, "Iptc4xmpCore");
        assert_eq!(
            xmp_table("iptcExt").unwrap().tag("ImageRegion").map(|tag| tag.structure),
            Some(XmpStruct::Variable)
        );
        assert_eq!(xmp_ns("iptcExt"), Some("Iptc4xmpExt"));
        assert!(xmp_table("Iptc4xmpCore").is_none());
    }
}
"""


def _check_string(value: object) -> None:
    if not (isinstance(value, str) and PRINTABLE.fullmatch(value) and "\\" not in value and '"' not in value):
        raise ValueError(f"priority strings must be printable ASCII without quotes or backslashes: {value!r}")


def _check_priority(value: object, nullable: bool) -> None:
    if value is None and nullable:
        return
    if not isinstance(value, int) or isinstance(value, bool) or not -128 <= value <= 127:
        raise ValueError(f"priority {value!r} is not an i8")


def _check_flag(value: object) -> None:
    if value is not None and not isinstance(value, bool):
        raise ValueError(f"flag {value!r} is not a boolean or null")


def validate(capture: dict) -> None:
    if set(capture) != SCHEMA_KEYS:
        raise ValueError("XMP priority capture has unexpected keys")
    if not capture["namespaces"] or not capture["xmp_ns"]:
        raise ValueError("XMP priority capture is empty")
    for prefix, target in capture["xmp_ns"].items():
        _check_string(prefix); _check_string(target)
    for prefix, fact in capture["namespaces"].items():
        if set(fact) != NAMESPACE_KEYS:
            raise ValueError(f"namespace {prefix} has unexpected keys")
        for value in (prefix, fact["table"], fact["namespace"]):
            _check_string(value)
        _check_priority(fact["table_priority"], nullable=True)
        _check_flag(fact["table_avoid"])
        for tag_id, tag in fact["tags"].items():
            if set(tag) != TAG_KEYS or tag["struct"] not in STRUCT_KINDS:
                raise ValueError(f"{prefix}:{tag_id} has unexpected keys")
            _check_string(tag_id); _check_string(tag["name"])
            _check_priority(tag["priority"], nullable=False)
            _check_priority(tag["own_priority"], nullable=True)
            _check_flag(tag["avoid"])


def _opt_i8(value) -> str:
    return "None" if value is None else f"Some({value})"


def _opt_bool(value) -> str:
    return "None" if value is None else f"Some({'true' if value else 'false'})"


def render(capture: dict) -> str:
    validate(capture)
    lines = [
        "// @generated by tools/exiftool-tables/xmp_priority_specs.py -- do not edit.",
        f"// Source: ExifTool {capture['exiftool_version']} XMP::Main tag tables (after AddFlattenedTags), %xmpNS.",
        "",
        "/// Whether a tag entry is an XMP structure, and whether that structure",
        "/// declares a fixed namespace or `NAMESPACE => undef` (variable).",
        "#[derive(Debug, Clone, Copy, PartialEq, Eq)]",
        "pub enum XmpStruct {",
        "    None,",
        "    Fixed,",
        "    Variable,",
        "}",
        "",
        "/// One raw tag ID of an XMP namespace table.",
        "#[derive(Debug)]",
        "pub struct XmpTagFact {",
        "    /// The tag ID exactly as the table keys it (case-sensitive).",
        "    pub id: &'static str,",
        "    /// Effective FoundTag priority of a direct hit: `Priority`, else the",
        "    /// table `PRIORITY`, else 0 for `Avoid`, else 1.",
        "    pub priority: i8,",
        "    /// The tagInfo's own `Priority`.",
        "    pub own_priority: Option<i8>,",
        "    /// The tagInfo's `Avoid` after table setup (table `AVOID` included).",
        "    pub avoid: Option<bool>,",
        "    pub structure: XmpStruct,",
        "}",
        "",
        "/// One `%Image::ExifTool::XMP::Main` tag table.",
        "#[derive(Debug)]",
        "pub struct XmpTableFact {",
        "    /// The Main key: the namespace prefix after `%stdXlatNS`.",
        "    pub key: &'static str,",
        "    /// The table's `NAMESPACE`.",
        "    pub namespace: &'static str,",
        "    /// The table's `PRIORITY`.",
        "    pub priority: Option<i8>,",
        "    /// The table's `AVOID`.",
        "    pub avoid: Option<bool>,",
        "    /// Tag facts sorted by `id`.",
        "    pub tags: &'static [XmpTagFact],",
        "}",
        "",
        "impl XmpTableFact {",
        "    /// The entry for raw tag ID `id`, as `GetTagInfo` would find it.",
        "    pub fn tag(&self, id: &str) -> Option<&'static XmpTagFact> {",
        "        self.tags",
        "            .binary_search_by(|tag| tag.id.cmp(id))",
        "            .ok()",
        "            .map(|index| &self.tags[index])",
        "    }",
        "}",
        "",
        "/// Main tables sorted by key.",
        "#[rustfmt::skip]",
        "pub static XMP_TABLES: &[XmpTableFact] = &[",
    ]
    for key in sorted(capture["namespaces"]):
        fact = capture["namespaces"][key]
        lines.append(f'    XmpTableFact {{ key: "{key}", namespace: "{fact["namespace"]}", '
                     f'priority: {_opt_i8(fact["table_priority"])}, avoid: {_opt_bool(fact["table_avoid"])}, tags: &[')
        for tag_id in sorted(fact["tags"]):
            tag = fact["tags"][tag_id]
            lines.append(f'        XmpTagFact {{ id: "{tag_id}", priority: {tag["priority"]}, '
                         f'own_priority: {_opt_i8(tag["own_priority"])}, avoid: {_opt_bool(tag["avoid"])}, '
                         f'structure: {STRUCT_KINDS[tag["struct"]]} }},')
        lines.append("    ] },")
    lines += ["];", "",
              "/// XMP.pm's `%xmpNS`: group prefix -> standard XMP prefix, sorted.",
              "#[rustfmt::skip]",
              "pub static XMP_NS: &[(&str, &str)] = &["]
    lines += [f'    ("{key}", "{value}"),' for key, value in sorted(capture["xmp_ns"].items())]
    lines += ["];", "",
              "/// The Main table FoundXMP selects for a translated namespace prefix.",
              "pub fn xmp_table(key: &str) -> Option<&'static XmpTableFact> {",
              "    XMP_TABLES",
              "        .binary_search_by(|table| table.key.cmp(key))",
              "        .ok()",
              "        .map(|index| &XMP_TABLES[index])",
              "}", "",
              "/// `%xmpNS` lookup.",
              "pub fn xmp_ns(prefix: &str) -> Option<&'static str> {",
              "    XMP_NS",
              "        .binary_search_by(|(known, _)| (*known).cmp(prefix))",
              "        .ok()",
              "        .map(|index| XMP_NS[index].1)",
              "}", ""]
    lines += TESTS.splitlines()
    return "\n".join(lines) + "\n"


def formatted(rust: str) -> str:
    """rustfmt the rendered source so the committed file passes `cargo fmt --check`."""
    import subprocess
    run = subprocess.run(["rustfmt", "--edition", "2024", "--config-path", str(ROOT / "rustfmt.toml"), "--emit", "stdout"],
                         input=rust, text=True, capture_output=True, check=True)
    return run.stdout


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--capture", type=Path, default=FIXTURE)
    parser.add_argument("--fixture-out", type=Path)
    parser.add_argument("--rust-out", type=Path, default=RUST)
    args = parser.parse_args()
    capture = json.loads(args.capture.read_text())
    rust = render(capture)
    if args.fixture_out:
        args.fixture_out.write_text(json.dumps(capture, sort_keys=True, indent=1, ensure_ascii=False) + "\n")
    args.rust_out.write_text(formatted(rust))
    tags = sum(len(fact["tags"]) for fact in capture["namespaces"].values())
    print(f"{len(capture['namespaces'])} XMP namespace tables, {tags} tag IDs, {len(capture['xmp_ns'])} %xmpNS entries")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
