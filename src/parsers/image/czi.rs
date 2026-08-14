//! ZISRAW (CZI) metadata parser -- Zeiss Integrated Software RAW.
//!
//! ExifTool routes `.czi` files through `Image::ExifTool::ZISRAW::ProcessCZI`
//! (ZISRAW.pm:165-201), which validates a `ZISRAWFILE\0{6}` signature, reads
//! the first 100 bytes as `ZISRAW::Main` (ZISRAW.pm:19-41), then follows a
//! 64-bit offset at byte 92 to a `ZISRAWMETADATA` section holding a block of
//! XML.
//!
//! # What comes from the transcription
//!
//! `ZISRAW::Main` is a real `ProcessBinaryData` layout, so all three of its
//! fields are read from the generated table. Two carry a `ValueConv` the
//! transcription declines to model (`unpack("H*",$val)` over a 16-byte GUID,
//! ZISRAW.pm:30-40) and one carries a `PrintConv` it drops
//! (`$val =~ tr/ /./` over an `int32u[2]`, ZISRAW.pm:23-27); all three are
//! hand-implemented below against the cited Perl.
//!
//! # What is deliberately absent
//!
//! **Every XML-derived tag, and the `XML` block itself.** ZISRAW.pm:186-199
//! reads the metadata section and hands it to `XMP::XML` with two options
//! set: `XmpIgnoreProps` (dropping the `ImageDocument`/`Metadata`/
//! `Information` path prefixes) and `ShortenXmpTags => \&ShortenTagNames`.
//!
//! `ShortenTagNames` (ZISRAW.pm:47-160) is ~130 `s///` substitutions applied
//! in a fixed order to the concatenated XML path. The order is load-bearing
//! -- several rules only match because an earlier rule already rewrote the
//! string, and a number use repeated-group patterns
//! (`s/(ApoTomeDepthInfo)+Element/ApoTomeDepth/`) or backreferences
//! (`s/Interval(.*Interval)/$1/`). Getting any single substitution wrong, or
//! applying two out of order, does not fail loudly: it emits a
//! plausible-looking tag *name* that is not the one ExifTool produces, while
//! the real tag goes missing. On the pinned `t/images/ZISRAW.czi` that path
//! accounts for 28 tags (`MicroscopeName`, `EyePieceTotalMag` and so on).
//!
//! Reproducing that chain is a self-contained piece of work with its own
//! verification needs, so it is left undone rather than approximated here.
//! This parser closes the three header tags it can decode exactly, which is
//! the same trade `mrc.rs` makes for the FEI12 extended header.
//!
//! # References
//!
//! - ExifTool source: `lib/Image/ExifTool/ZISRAW.pm`

use crate::core::{FileReader, MetadataMap, TagValue};
use crate::exiftool_tables::{
    Acknowledged, DecodedValue, PerlCitation, RawAccess, decode_binary_table, find_table,
};
use crate::io::ByteOrder;

/// ZISRAW.pm:173, `$raf->Read($buff, 100) == 100`.
const HEADER_LEN: usize = 100;

/// ZISRAW.pm:174, `$buff =~ /^ZISRAWFILE\0{6}/`.
const CZI_SIGNATURE: &[u8] = b"ZISRAWFILE\0\0\0\0\0\0";

const fn citation(tag: &'static str, lines: &'static str) -> PerlCitation {
    PerlCitation {
        module: "ZISRAW",
        table: "Main",
        tag,
        lines,
    }
}

const ZISRAW_VERSION: PerlCitation = citation("ZISRAWVersion", "ZISRAW.pm:23-27");
const PRIMARY_FILE_GUID: PerlCitation = citation("PrimaryFileGUID", "ZISRAW.pm:30-34");
const FILE_GUID: PerlCitation = citation("FileGUID", "ZISRAW.pm:35-40");

/// Extract ZISRAW (CZI) header metadata (`Image::ExifTool::ZISRAW::ProcessCZI`).
pub fn parse_czi_metadata(reader: &dyn FileReader) -> std::result::Result<MetadataMap, String> {
    if reader.size() < HEADER_LEN as u64 {
        return Err("CZI file is too short for the 100-byte header".to_string());
    }
    let header = reader.read(0, HEADER_LEN).map_err(|e| e.to_string())?;
    if !header.starts_with(CZI_SIGNATURE) {
        return Err("invalid ZISRAW signature".to_string());
    }

    let table = find_table("ZISRAW", "Main").ok_or("missing ZISRAW::Main table")?;
    // ZISRAW.pm:176, `SetByteOrder('II')`.
    let decode = decode_binary_table(table, &header, ByteOrder::Little);

    let mut metadata = MetadataMap::new();
    for decoded in decode.fields() {
        let name = decoded.field.name;
        let key = format!("File:{name}");
        match name {
            // ZISRAW.pm:23-27: `Format => 'int32u[2]'` with
            // `PrintConv => '$val =~ tr/ /./; $val'` -- ExifTool renders an
            // array as space-separated, so the PrintConv turns "1 0" into
            // "1.0". The generator drops this PrintConv, so the raw array
            // reaches here through the ordinary `.emit()` path.
            "ZISRAWVersion" => {
                // ExifTool renders the `int32u[2]` space-separated and the
                // PrintConv transliterates each space to a dot, so the two
                // elements end up joined by ".". Read the decoded array
                // directly rather than re-splitting a rendered string.
                if let Some(access) = RawAccess::new(decoded, Acknowledged::NONE, &ZISRAW_VERSION)
                    && let Some(rendered) = version_string(access.raw())
                {
                    metadata.insert(key, TagValue::new_string(rendered));
                }
            }
            // ZISRAW.pm:30-40: `Format => 'undef[16]'` with
            // `ValueConv => 'unpack("H*",$val)'` -- the 16 raw GUID bytes as
            // lowercase hex, in file order (`H*` is high-nibble-first).
            "PrimaryFileGUID" | "FileGUID" => {
                let cite = if name == "PrimaryFileGUID" {
                    &PRIMARY_FILE_GUID
                } else {
                    &FILE_GUID
                };
                if let Some(access) = RawAccess::new(decoded, Acknowledged::VALUE_CONV, cite)
                    && let DecodedValue::Undefined(bytes) = access.raw()
                {
                    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
                    metadata.insert(key, TagValue::new_string(hex));
                }
            }
            _ => {
                if let Some(value) = decoded.emit() {
                    metadata.insert(key, value);
                }
            }
        }
    }

    Ok(metadata)
}

/// ZISRAW.pm:23-27's `int32u[2]` under `PrintConv => '$val =~ tr/ /./; $val'`.
fn version_string(raw: &DecodedValue) -> Option<String> {
    let DecodedValue::Array(items) = raw else {
        return None;
    };
    let parts: Vec<String> = items
        .iter()
        .filter_map(DecodedValue::as_integer)
        .map(|v| v.to_string())
        .collect();
    if parts.len() < 2 {
        return None;
    }
    Some(parts.join("."))
}
