//! Text-based format detection
//!
//! Handles detection of text-based 3D and interchange formats including
//! DXF, OBJ, GLTF, STL, and EPS.

use crate::core::FileFormat;
use chrono::{DateTime, FixedOffset};

fn looks_like_ics(text: &str) -> bool {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text).trim_start();
    let mut lines = text.lines();
    let Some(first_line) = lines.next() else {
        return false;
    };

    if !first_line.trim_end_matches('\r').eq("BEGIN:VCALENDAR") {
        return false;
    }

    lines.any(|line| {
        let line = line.trim_end_matches('\r');
        line.get(..8)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("VERSION:"))
    })
}

fn looks_like_eml(text: &str) -> bool {
    let header_end = text
        .find("\r\n\r\n")
        .or_else(|| text.find("\n\n"))
        .unwrap_or(text.len());
    let headers = &text[..header_end];
    let mut has_from = false;
    let mut has_valid_date = false;
    let mut has_address_like_from = false;
    let mut has_address_like_recipient = false;
    let mut has_mail_specific_header = false;
    let mut has_subject = false;
    let mut saw_header = false;

    for line in headers.lines() {
        let line = line.trim_end_matches('\r');
        if line.is_empty() {
            break;
        }

        if line.starts_with(' ') || line.starts_with('\t') {
            if !saw_header {
                return false;
            }
            continue;
        }

        let Some((name, value)) = line.split_once(':') else {
            return false;
        };
        if name.is_empty()
            || !name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        {
            return false;
        }

        saw_header = true;
        has_from |= name.eq_ignore_ascii_case("from");
        has_address_like_from |=
            name.eq_ignore_ascii_case("from") && looks_like_email_address(value.trim());
        has_address_like_recipient |=
            matches!(name.to_ascii_lowercase().as_str(), "to" | "cc" | "bcc")
                && looks_like_email_address(value.trim());
        has_valid_date |=
            name.eq_ignore_ascii_case("date") && looks_like_rfc_email_date(value.trim());
        has_subject |= name.eq_ignore_ascii_case("subject");
        has_mail_specific_header |= matches!(
            name.to_ascii_lowercase().as_str(),
            "message-id"
                | "mime-version"
                | "received"
                | "content-type"
                | "content-transfer-encoding"
                | "content-disposition"
                | "return-path"
        );
    }

    has_from
        && ((has_valid_date
            && ((has_address_like_from && has_address_like_recipient) || has_mail_specific_header))
            || (has_address_like_from && has_address_like_recipient && has_subject))
}

fn eml_header_bytes(data: &[u8]) -> &[u8] {
    if let Some(index) = data.windows(4).position(|window| window == b"\r\n\r\n") {
        return &data[..index];
    }
    if let Some(index) = data.windows(2).position(|window| window == b"\n\n") {
        return &data[..index];
    }
    data
}

fn looks_like_rfc_email_date(value: &str) -> bool {
    DateTime::<FixedOffset>::parse_from_rfc2822(value).is_ok()
}

fn looks_like_email_address(value: &str) -> bool {
    value
        .split(|character: char| {
            character.is_ascii_whitespace()
                || matches!(character, '<' | '>' | '(' | ')' | ',' | ';' | '"')
        })
        .any(|candidate| {
            let Some((local, domain)) = candidate.rsplit_once('@') else {
                return false;
            };

            !local.is_empty()
                && !domain.is_empty()
                && !local.contains('@')
                && domain
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
        })
}

/// Wavefront OBJ geometry directives, matched only at the start of a line
///
/// OBJ has no magic number, and these directives are one or two letters of
/// ordinary alphabet, so the line anchor *is* the signature. See
/// [`looks_like_obj`].
const OBJ_VERTEX_DIRECTIVES: [&str; 3] = ["v", "vn", "vt"];

/// The JSON key every glTF asset is required to carry
const GLTF_ASSET_KEY: &str = "\"asset\"";

/// Yields each line of `text` with its indentation removed
///
/// OBJ, DXF and STL are line-oriented and all three tolerate leading
/// whitespace, so "anchored" here means *first token on a line*, not
/// *character 0 of a line* -- the same shape as the `^\s*0\s+` opening
/// ExifTool's DXF magic uses.
fn indented_lines(text: &str) -> impl Iterator<Item = &str> {
    text.lines()
        .map(|line| line.trim_start_matches([' ', '\t']))
}

/// Whether `line` opens with `token` as a whole word
fn line_opens_with(line: &str, token: &str) -> bool {
    line.strip_prefix(token)
        .is_some_and(|rest| rest.is_empty() || rest.starts_with([' ', '\t']))
}

/// Whether the probe carries a Wavefront OBJ vertex directive on a real line
///
/// The directive must be the first token on some line and be followed by its
/// operands -- `^v `, `^vn `, `^vt `, as ExifTool's magic patterns anchor.
/// A bare `contains("v ")` is not the same test and is not remotely as
/// selective: `Radiance.hdr` opens
///
/// ```text
/// #?RADIANCE
/// oconv mat.rad sky.rad surfaces.rad
/// ```
///
/// and the `v ` ending `oconv ` satisfied it, so a Radiance HDR image was
/// dispatched to the OBJ parser. That reported `HasNormals`/`HasTextureCoords`
/// for an image file while ExifTool reports `Software`, `View`, `Format`,
/// `Exposure` and the image dimensions -- and it did so silently, because
/// `File:FileType` is decided by the magic table and kept saying HDR.
pub(crate) fn looks_like_obj(text: &str) -> bool {
    indented_lines(text).any(|line| {
        OBJ_VERTEX_DIRECTIVES.iter().any(|directive| {
            line.strip_prefix(directive)
                .is_some_and(|rest| rest.starts_with([' ', '\t']))
        })
    })
}

/// Whether the probe opens an AutoCAD DXF section table
///
/// ExifTool's magic is `^\s*0\s+\x00?\s*SECTION\s+2\s+HEADER`: the group code
/// and its `SECTION` value are consecutive records, never a keyword loose in
/// the header. Only the anchoring is tightened here -- requiring the
/// `2`/`HEADER` records too would reject the ENTITIES-first files this has
/// always accepted.
fn looks_like_dxf(text: &str) -> bool {
    text.starts_with("0\n") && indented_lines(text).any(|line| line.trim_end() == "SECTION")
}

/// Whether the probe is a JSON glTF asset
///
/// glTF is a JSON *object* carrying a required `asset` key, so the probe has
/// to open one: `contains("{") && contains("\"asset\"")` also accepts any
/// document that merely mentions both, in either order and at any depth.
fn looks_like_gltf(text: &str) -> bool {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text).trim_start();
    if !text.starts_with('{') {
        return false;
    }

    text.match_indices(GLTF_ASSET_KEY).any(|(index, _)| {
        text[index + GLTF_ASSET_KEY.len()..]
            .trim_start()
            .starts_with(':')
    })
}

/// Whether the probe is an ASCII STL solid
///
/// `solid` is already anchored to byte 0, so this rule never had the OBJ
/// misroute in it. It has the two smaller ones: `solidification` matched the
/// prefix, and any English prose opening "solid " was handed to the STL
/// parser. An ASCII STL declares facets, so require one -- or `endsolid` for
/// the empty solid that has none.
///
/// The corroborating directive is looked for across the whole probe rather
/// than the 100-byte window: a facet line is the *second* line of every real
/// STL, but only once the solid's name has ended.
fn looks_like_stl(text: &str) -> bool {
    let Some(rest) = text.strip_prefix("solid") else {
        return false;
    };
    if !(rest.is_empty() || rest.starts_with([' ', '\t', '\r', '\n'])) {
        return false;
    }

    indented_lines(text)
        .any(|line| line_opens_with(line, "facet") || line_opens_with(line, "endsolid"))
}

/// Detect text-based 3D and interchange formats
///
/// Several formats use text-based representations with distinctive patterns:
/// - DXF: AutoCAD exchange format
/// - OBJ: Wavefront 3D object
/// - GLTF: GL Transmission Format (JSON)
/// - STL: Stereolithography (ASCII variant)
/// - EPS: Encapsulated PostScript
///
/// # Arguments
///
/// * `data` - Magic bytes buffer (at least 100 bytes recommended)
///
/// # Returns
///
/// `Some(FileFormat)` if text format detected, `None` otherwise
pub fn detect_text_formats(data: &[u8]) -> Option<FileFormat> {
    // EPS detection first (can be shorter than 100 bytes)
    // ASCII EPS: %!PS-Adobe
    if data.starts_with(b"%!PS-Adobe") {
        return Some(FileFormat::EPS);
    }

    // Binary EPS (DOS EPS): 0xC5D0D3C6 magic
    if data.len() >= 4 && data[0] == 0xC5 && data[1] == 0xD0 && data[2] == 0xD3 && data[3] == 0xC6 {
        return Some(FileFormat::EPS);
    }

    // The bounded probe may cut a multibyte character; judge the valid prefix.
    if looks_like_ics(super::helpers::utf8_prefix(data)) {
        return Some(FileFormat::ICS);
    }

    let eml_headers = String::from_utf8_lossy(eml_header_bytes(data));
    if looks_like_eml(&eml_headers) {
        return Some(FileFormat::EML);
    }

    if data.len() < 100 {
        return None;
    }

    // Judge the valid UTF-8 prefix of the probe so a multibyte character
    // straddling the 100-byte cut does not disqualify these text formats.
    let text = super::helpers::utf8_prefix(&data[0..100]);
    if text.is_empty() {
        return None;
    }

    // DXF: group code 0 opening a SECTION record
    if looks_like_dxf(text) {
        return Some(FileFormat::DXF);
    }

    // OBJ: a vertex directive at the start of a line
    if looks_like_obj(text) {
        return Some(FileFormat::OBJ);
    }

    // GLTF: a JSON object carrying the required "asset" key
    if looks_like_gltf(text) {
        return Some(FileFormat::GLTF);
    }

    // STL ASCII: "solid" opening a solid that declares facets. The whole probe
    // is judged, not the 100-byte window, so a long solid name cannot hide the
    // corroborating directive.
    if text.starts_with("solid") && looks_like_stl(super::helpers::utf8_prefix(data)) {
        return Some(FileFormat::STL);
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every rule below the EML gate is skipped for a probe under 100 bytes,
    /// so a fixture has to reach the window to be a test of anything.
    fn probe(text: &str) -> Vec<u8> {
        let mut data = text.as_bytes().to_vec();
        data.resize(data.len().max(100), b'\n');
        data
    }

    /// The header of `Radiance.hdr` in the shared corpus. `oconv ` ends in
    /// `v `, which is why `contains("v ")` routed this image to the OBJ parser
    /// -- silently, since `File:FileType` comes from the magic table and went
    /// on reporting HDR.
    const RADIANCE_HEADER: &str = concat!(
        "#?RADIANCE\n",
        "oconv mat.rad sky.rad surfaces.rad\n",
        "oconv -f -i test4.oct ila01728\n",
        "rpict -t 30 -vf test4.vf -x 1536 -y 1536 -ps 3 -pt .04\n",
    );

    #[test]
    fn radiance_header_is_not_obj() {
        assert!(!looks_like_obj(RADIANCE_HEADER));
        assert_eq!(detect_text_formats(&probe(RADIANCE_HEADER)), None);
    }

    #[test]
    fn vertex_directive_at_a_line_start_is_obj() {
        let obj = concat!(
            "# Blender v2.79 (sub 0) OBJ File: ''\n",
            "mtllib cube.mtl\n",
            "o Cube\n",
            "v 1.000000 -1.000000 -1.000000\n",
            "vn 0.0000 1.0000 0.0000\n",
            "vt 0.7500 0.2500\n",
        );
        assert!(looks_like_obj(obj));
        assert_eq!(detect_text_formats(&probe(obj)), Some(FileFormat::OBJ));
    }

    /// OBJ tolerates indentation, so the anchor is the first token on a line
    /// rather than byte 0 of one.
    #[test]
    fn indented_vertex_directive_is_obj() {
        assert!(looks_like_obj("o Cube\n  v 1.0 2.0 3.0\n"));
        assert!(looks_like_obj("o Cube\n\tvt 0.5 0.5\n"));
    }

    /// The directive is a whole token: `vertex` is an STL keyword, and `v`
    /// with no operands is not a vertex.
    #[test]
    fn vertex_lookalikes_are_not_obj() {
        assert!(!looks_like_obj("vertex 0.0 0.0 0.0\n"));
        assert!(!looks_like_obj("vp 1.0 2.0\n"));
        assert!(!looks_like_obj("v\n"));
        assert!(!looks_like_obj("# exported by modeller v 4.2\n"));
    }

    /// The DXF group code and its SECTION value are consecutive records.
    #[test]
    fn dxf_section_must_be_its_own_record() {
        let dxf = "0\nSECTION\n  2\nHEADER\n  9\n$ACADVER\n  1\nAC1015\n";
        assert!(looks_like_dxf(dxf));
        assert_eq!(detect_text_formats(&probe(dxf)), Some(FileFormat::DXF));

        assert!(!looks_like_dxf("0\ndescribing a SECTION of the plan\n"));
        assert!(!looks_like_dxf("1\nSECTION\n"));
    }

    #[test]
    fn gltf_must_open_a_json_object() {
        let gltf = "{\n  \"asset\": { \"version\": \"2.0\", \"generator\": \"COLLADA2GLTF\" },\n  \"scene\": 0\n}\n";
        assert!(looks_like_gltf(gltf));
        assert_eq!(detect_text_formats(&probe(gltf)), Some(FileFormat::GLTF));

        // Mentions both tokens, opens neither an object nor an `asset` key.
        assert!(!looks_like_gltf(
            "the \"asset\" register lists { and } counts\n"
        ));
        assert!(!looks_like_gltf("[{ \"name\": \"asset\" }]\n"));
    }

    #[test]
    fn stl_needs_a_facet_directive() {
        let stl = concat!(
            "solid cube\n",
            "  facet normal 0.0 0.0 1.0\n",
            "    outer loop\n",
            "      vertex 0.0 0.0 0.0\n",
            "    endloop\n",
            "  endfacet\n",
            "endsolid cube\n",
        );
        assert!(looks_like_stl(stl));
        assert_eq!(detect_text_formats(&probe(stl)), Some(FileFormat::STL));

        // An empty solid declares no facets but still closes itself.
        assert!(looks_like_stl("solid empty\nendsolid empty\n"));
    }

    #[test]
    fn stl_lookalikes_are_not_stl() {
        assert!(!looks_like_stl("solidification of the melt was measured\n"));
        assert!(!looks_like_stl(
            "solid state drives were bought in bulk this year\n"
        ));
    }

    /// A facet line lands past the 100-byte window once the solid's name is
    /// long enough, so the corroboration reads the whole probe.
    #[test]
    fn stl_with_a_long_name_is_still_stl() {
        let name = "a".repeat(200);
        let stl =
            format!("solid {name}\n  facet normal 0.0 0.0 1.0\n  endfacet\nendsolid {name}\n");
        assert!(
            stl.find("facet").is_some_and(|index| index > 100),
            "fixture must put the facet directive past the 100-byte window"
        );
        assert_eq!(detect_text_formats(stl.as_bytes()), Some(FileFormat::STL));
    }
}
