//! Minimal CBOR (RFC 7049) reader for JUMBF / C2PA metadata.
//!
//! The C2PA specification stores its claim, assertion and signature stores as
//! CBOR inside JUMBF `cbor` boxes. ExifTool reads them with
//! `Image::ExifTool::CBOR::ReadCBORValue` and then hands the decoded structure
//! to `Image::ExifTool::JSON::ProcessTag` to be flattened into tags, so this
//! reader deliberately mirrors ExifTool's decoder rather than a general-purpose
//! CBOR library: the value model it produces is exactly what `ProcessTag`
//! expects to walk (ordered maps, arrays, byte strings kept distinct from text
//! strings, and the type-7 simple values rendered as ExifTool's words).
//!
//! Only the subset C2PA actually uses is implemented; anything unrecognised
//! stops the parse rather than guessing, so no value is ever invented.
//!
//! Reference: `Image::ExifTool::CBOR` (CBOR.pm), <https://c2pa.org/specifications/>

/// Maximum nesting depth accepted before the parse is abandoned.
///
/// C2PA structures are only a handful of levels deep; this is purely a guard
/// against a hostile or corrupt file driving unbounded recursion.
const MAX_DEPTH: usize = 32;

/// A decoded CBOR value, modelled on what ExifTool's `ReadCBORValue` returns.
#[derive(Debug, Clone, PartialEq)]
pub enum CborValue {
    /// Major types 0 and 1 (unsigned / negative integer).
    Int(i64),
    /// Type 7 float (half, single or double precision).
    Float(f64),
    /// Major type 2 - a byte string. ExifTool keeps these separate from text
    /// strings and reports them as `(Binary data N bytes, ...)`.
    Bytes(Vec<u8>),
    /// Major type 3 - a UTF-8 text string.
    Text(String),
    /// Major type 4 - an array.
    Array(Vec<CborValue>),
    /// Major type 5 - a map. Keys are stringified (ExifTool stores them as Perl
    /// hash keys) and the original encoding order is preserved, because
    /// ExifTool walks `_ordered_keys_` and the order decides which value wins
    /// when two paths flatten onto the same tag name.
    Map(Vec<(String, CborValue)>),
    /// Major type 7 simple values, spelled the way ExifTool spells them
    /// (`False`, `True`, `null`, `undef`, or `Unknown (N)`).
    Simple(String),
}

impl CborValue {
    /// Renders a map key the way Perl stringifies it when used as a hash key.
    fn as_map_key(&self) -> String {
        match self {
            CborValue::Int(i) => i.to_string(),
            CborValue::Text(s) => s.clone(),
            CborValue::Bytes(b) => String::from_utf8_lossy(b).into_owned(),
            CborValue::Float(f) => crate::core::formatters::perl_number(*f),
            CborValue::Simple(s) => s.clone(),
            // Structured keys are not used by C2PA; an empty key is dropped by
            // the tag-name rules downstream rather than inventing a name.
            CborValue::Array(_) | CborValue::Map(_) => String::new(),
        }
    }
}

/// Reads one CBOR value starting at `*pos`, advancing `*pos` past it.
///
/// Returns `None` on truncated or unsupported input, in which case `*pos` is
/// left unspecified and the caller must abandon the item.
pub fn read_value(data: &[u8], pos: &mut usize, depth: usize) -> Option<CborValue> {
    if depth > MAX_DEPTH {
        return None;
    }
    let initial = *data.get(*pos)?;
    *pos += 1;
    let mut additional = (initial & 0x1f) as u64;
    let major = initial >> 5;

    // Additional-information encoding: 0..23 is the value itself, 24..27 is a
    // 1/2/4/8-byte big-endian follow-on, 31 is the indefinite-length marker
    // (never emitted by C2PA) and 28..30 are reserved.
    let mut indefinite = false;
    match additional {
        0..=23 => {}
        24 | 25 | 26 | 27 => {
            let n = 1usize << (additional - 24);
            let bytes = data.get(*pos..*pos + n)?;
            *pos += n;
            let mut v: u64 = 0;
            for b in bytes {
                v = (v << 8) | u64::from(*b);
            }
            additional = v;
        }
        31 => {
            indefinite = true;
            additional = 0;
        }
        _ => return None,
    }

    match major {
        0 => Some(CborValue::Int(i64::try_from(additional).ok()?)),
        // ExifTool computes `-1 * $num` here (CBOR.pm, "negative integer"),
        // where RFC 7049 defines the value as -1 - n. Matching ExifTool is the
        // point of this crate, so its arithmetic is reproduced verbatim; no
        // sample in the corpus encodes a negative CBOR integer, so nothing
        // observable depends on the choice today.
        1 => Some(CborValue::Int(-i64::try_from(additional).ok()?)),
        2 | 3 => {
            if indefinite {
                return None;
            }
            let n = usize::try_from(additional).ok()?;
            let bytes = data.get(*pos..pos.checked_add(n)?)?;
            *pos += n;
            if major == 2 {
                Some(CborValue::Bytes(bytes.to_vec()))
            } else {
                Some(CborValue::Text(String::from_utf8_lossy(bytes).into_owned()))
            }
        }
        4 => {
            if indefinite {
                return None;
            }
            let n = usize::try_from(additional).ok()?;
            // A length field can claim far more items than the box could hold;
            // one byte is the smallest possible item, so this bounds the
            // allocation without trusting the file.
            if n > data.len() {
                return None;
            }
            let mut items = Vec::with_capacity(n);
            for _ in 0..n {
                items.push(read_value(data, pos, depth + 1)?);
            }
            Some(CborValue::Array(items))
        }
        5 => {
            if indefinite {
                return None;
            }
            let n = usize::try_from(additional).ok()?;
            if n > data.len() {
                return None;
            }
            let mut entries = Vec::with_capacity(n);
            for _ in 0..n {
                let key = read_value(data, pos, depth + 1)?;
                let value = read_value(data, pos, depth + 1)?;
                entries.push((key.as_map_key(), value));
            }
            Some(CborValue::Map(entries))
        }
        // Optional (semantic) tag: ExifTool reads the tagged value and returns
        // it in place of the tag, which is how a COSE_Sign1 (tag 18) message
        // surfaces as a plain 4-element array. Only the bignum conversions are
        // applied, matching CBOR.pm; the remaining conversions there are marked
        // untested by its author and none are reachable from C2PA data.
        6 => {
            let inner = read_value(data, pos, depth + 1)?;
            Some(match (additional, &inner) {
                (2, CborValue::Bytes(b)) => CborValue::Int(bignum(b, false)?),
                (3, CborValue::Bytes(b)) => CborValue::Int(bignum(b, true)?),
                _ => inner,
            })
        }
        7 => {
            if indefinite {
                // "break" terminates an indefinite-length item, which C2PA
                // never produces.
                return None;
            }
            match initial & 0x1f {
                20 => Some(CborValue::Simple("False".to_string())),
                21 => Some(CborValue::Simple("True".to_string())),
                22 => Some(CborValue::Simple("null".to_string())),
                23 => Some(CborValue::Simple("undef".to_string())),
                25 => Some(CborValue::Float(half_to_f64(additional as u16))),
                26 => Some(CborValue::Float(f64::from(f32::from_bits(
                    u32::try_from(additional).ok()?,
                )))),
                27 => Some(CborValue::Float(f64::from_bits(additional))),
                _ => Some(CborValue::Simple(format!("Unknown ({})", additional))),
            }
        }
        _ => None,
    }
}

/// Converts a CBOR bignum byte string to an `i64`, or `None` if it does not fit.
fn bignum(bytes: &[u8], negative: bool) -> Option<i64> {
    let mut acc: i64 = 0;
    for b in bytes {
        acc = acc.checked_mul(256)?.checked_add(i64::from(*b))?;
    }
    Some(if negative { -acc } else { acc })
}

/// Decodes an IEEE 754 half-precision float.
fn half_to_f64(bits: u16) -> f64 {
    let sign = if bits & 0x8000 != 0 { -1.0 } else { 1.0 };
    let exponent = ((bits >> 10) & 0x1f) as i32;
    let mantissa = f64::from(bits & 0x03ff);
    let magnitude = match exponent {
        0 => mantissa * 2f64.powi(-24),
        31 => {
            return if mantissa == 0.0 {
                sign * f64::INFINITY
            } else {
                f64::NAN
            };
        }
        _ => (mantissa + 1024.0) * 2f64.powi(exponent - 25),
    };
    sign * magnitude
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(data: &[u8]) -> Option<CborValue> {
        let mut pos = 0;
        read_value(data, &mut pos, 0)
    }

    #[test]
    fn reads_small_unsigned_integer() {
        assert_eq!(read(&[0x06]), Some(CborValue::Int(6)));
    }

    #[test]
    fn reads_multibyte_unsigned_integer() {
        // 0x1a = uint32 follow-on; 0x00377c1f = 3636255 (the byteOffset of the
        // second part in GooglePixel10ProXL.jpg's c2pa.hash.multi-asset box).
        assert_eq!(
            read(&[0x1a, 0x00, 0x37, 0x7c, 0x1f]),
            Some(CborValue::Int(3_636_255))
        );
    }

    #[test]
    fn reads_text_and_byte_strings() {
        assert_eq!(
            read(b"\x66sha256"),
            Some(CborValue::Text("sha256".to_string()))
        );
        assert_eq!(
            read(&[0x42, 0xde, 0xad]),
            Some(CborValue::Bytes(vec![0xde, 0xad]))
        );
    }

    #[test]
    fn reads_map_preserving_encoded_order() {
        // {"start": 6, "length": 7831} - the first exclusion of the Pixel's
        // c2pa.hash.data assertion.
        let data = b"\xa2\x65start\x06\x66length\x19\x1e\x97";
        assert_eq!(
            read(data),
            Some(CborValue::Map(vec![
                ("start".to_string(), CborValue::Int(6)),
                ("length".to_string(), CborValue::Int(7831)),
            ]))
        );
    }

    #[test]
    fn unwraps_semantic_tag() {
        // 0xd2 = tag(18) (COSE_Sign1), wrapping an array of one element.
        assert_eq!(
            read(&[0xd2, 0x81, 0x01]),
            Some(CborValue::Array(vec![CborValue::Int(1)]))
        );
    }

    #[test]
    fn reads_simple_values() {
        assert_eq!(read(&[0xf4]), Some(CborValue::Simple("False".to_string())));
        assert_eq!(read(&[0xf5]), Some(CborValue::Simple("True".to_string())));
        assert_eq!(read(&[0xf6]), Some(CborValue::Simple("null".to_string())));
    }

    #[test]
    fn rejects_truncated_input() {
        assert_eq!(read(&[0x64, b'a', b'b']), None);
        assert_eq!(read(&[]), None);
    }

    #[test]
    fn rejects_absurd_array_length() {
        // Claims 2^32 items in a 5-byte buffer.
        assert_eq!(read(&[0x9a, 0xff, 0xff, 0xff, 0xff]), None);
    }
}
