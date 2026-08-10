//! `ar` archive identity, following `EXE.pm`'s static-library path.
//!
//! ExifTool routes `.a` through EXE.pm like every other executable extension:
//! `%fileTypeLookup` maps `a => [['EXE'], ...]`, and the reported `FileType`
//! then comes from the content. A `!<arch>\x0a` file starts as a
//! `Static library` and is promoted to a `Mach-O static library` when one of
//! its members turns out to be a Mach-O object (EXE.pm:1479-1512):
//!
//! ```text
//!     } elsif ($buff =~ /^!<arch>\x0a/) {
//!         $et->SetFileType('Static library', undef, 'A');
//!         ...
//!         my $pos = 8;    # current file position
//!         my $max = 10;   # maximum number of archive files to check
//!         while ($max-- > 0) {
//!             $raf->Seek($pos, 0) and $raf->Read($buff, 60) == 60 or last;
//!             substr($buff, 58, 2) eq "`\n" or $et->Warn(...), last;
//!             ...
//!             $raf->Read($buff, 28) == 28 or last;  # read (possible) Mach header
//!             ExtractMachTags($et, \$buff) and last;  # try to extract tags
//!             $pos += 60 + $arSize;
//!             ++$pos if $pos & 0x01;
//!         }
//! ```
//!
//! and `ExtractMachTags`, called without its object-type flag, is what performs
//! the promotion (EXE.pm:1211-1213):
//!
//! ```text
//!     } else { # otherwise this was a static library
//!         $et->OverrideFileType('Mach-O static library', undef, 'A');
//!     }
//! ```
//!
//! The walk is why this lives in a parser rather than in `crate::filetype`:
//! the deciding member is not the first one. In `EXE.a` the first member is the
//! `__.SYMDEF` symbol table and the Mach header sits at offset 184; in a real
//! library that symbol table runs to many kilobytes, putting the answer well
//! past the 1 KiB header the identification layer is handed.
//!
//! Only the identity is modelled here. ExifTool also reads `EXE::AR` tags from
//! the first member header and Mach CPU tags from the member it stops on;
//! those remain unextracted.

use crate::core::{FileFormat, FileReader, FormatParser, MetadataMap, TagValue};
use crate::error::{ExifToolError, Result};

/// `!<arch>\x0a`, the global archive header.
pub const AR_MAGIC: &[u8] = b"!<arch>\n";

/// Bytes in one archive member header.
const MEMBER_HEADER_LEN: u64 = 60;

/// `$max = 10` -- how many members ExifTool inspects before giving up.
const MAX_MEMBERS: usize = 10;

/// The four thin Mach-O magic numbers of `%machType` (EXE.pm:1176-1181).
///
/// Deliberately not the `cafebabe` fat magics: `ExtractMachTags` tests against
/// `%machType` alone, so a fat member does not promote the archive.
const MACH_MAGICS: [[u8; 4]; 4] = [
    [0xFE, 0xED, 0xFA, 0xCE],
    [0xCE, 0xFA, 0xED, 0xFE],
    [0xFE, 0xED, 0xFA, 0xCF],
    [0xCF, 0xFA, 0xED, 0xFE],
];

/// Whether any member within the first [`MAX_MEMBERS`] is a Mach-O object.
fn has_mach_member(reader: &dyn FileReader) -> bool {
    let size = reader.size();
    let mut pos = AR_MAGIC.len() as u64;

    for _ in 0..MAX_MEMBERS {
        if pos + MEMBER_HEADER_LEN > size {
            return false;
        }
        let Ok(header) = reader.read(pos, MEMBER_HEADER_LEN as usize) else {
            return false;
        };
        // ExifTool stops on a member header without the trailing "`\n".
        if &header[58..60] != b"`\n" {
            return false;
        }

        // `$arSize` is the size field, taking its leading digits only. It
        // counts a BSD extended name as part of the member, so it is the right
        // stride whether or not one is present.
        let Some(ar_size) = leading_number(&header[48..58]) else {
            return false;
        };

        // BSD stores a name longer than 16 bytes in front of the data as
        // `#1/<len>`, which shifts where the Mach header would start.
        let name_len = extended_name_len(&header[0..16]).unwrap_or(0);
        let data = pos + MEMBER_HEADER_LEN + name_len;

        // ExifTool reads 28 bytes here and tests the first four.
        if let Ok(head) = reader.read(data, 4)
            && let Ok(magic) = <[u8; 4]>::try_from(head)
            && MACH_MAGICS.contains(&magic)
        {
            return true;
        }

        pos += MEMBER_HEADER_LEN + ar_size;
        pos += pos & 1; // members are padded to an even offset
    }
    false
}

/// `$arSize =~ s/^(\d+).*/$1/s` -- leading decimal digits, or `None`.
fn leading_number(field: &[u8]) -> Option<u64> {
    let digits: Vec<u8> = field
        .iter()
        .copied()
        .take_while(u8::is_ascii_digit)
        .collect();
    if digits.is_empty() {
        return None;
    }
    std::str::from_utf8(&digits).ok()?.parse().ok()
}

/// `$name =~ m{^#1/(\d+) *$}` -- the BSD extended-name length, if present.
fn extended_name_len(name: &[u8]) -> Option<u64> {
    let name = std::str::from_utf8(name).ok()?;
    let rest = name.strip_prefix("#1/")?;
    let (digits, padding) = rest.split_at(rest.find(|c: char| !c.is_ascii_digit())?);
    if !padding.bytes().all(|b| b == b' ') {
        return None;
    }
    digits.parse().ok()
}

/// Parser for `ar` archives.
pub struct ARParser;

impl ARParser {
    /// Whether the reader starts with the archive magic.
    pub fn verify_signature(reader: &dyn FileReader) -> Result<bool> {
        Ok(reader
            .read(0, AR_MAGIC.len())
            .map(|head| head == AR_MAGIC)
            .unwrap_or(false))
    }
}

impl FormatParser for ARParser {
    fn parse(&self, reader: &dyn FileReader) -> Result<MetadataMap> {
        if !Self::verify_signature(reader)? {
            return Err(ExifToolError::parse_error("not an ar archive"));
        }

        let mut metadata = MetadataMap::new();
        let file_type = if has_mach_member(reader) {
            "Mach-O static library"
        } else {
            "Static library"
        };
        // Written into the `File` group for the same reason as the ELF and
        // Mach-O parsers: this is ExifTool's own answer for the format, and it
        // has to outrank `%fileTypeLookup`, which calls the file `A`.
        metadata.insert(
            "File:FileType".to_string(),
            TagValue::String(file_type.to_string()),
        );
        metadata.insert(
            "File:FileTypeExtension".to_string(),
            TagValue::String("a".to_string()),
        );
        Ok(metadata)
    }

    fn supports_format(&self, format: FileFormat) -> bool {
        matches!(format, FileFormat::AR)
    }
}

/// Dispatch entry point.
pub fn parse_ar_metadata(reader: &dyn FileReader) -> std::result::Result<MetadataMap, String> {
    ARParser.parse(reader).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestReader;

    /// One archive member: a 60-byte header, an optional BSD extended name,
    /// then `data`. `size` counts the name as ExifTool's `$arSize` does.
    fn member(name: &str, ext_name: &[u8], data: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(format!("{name:<16}").as_bytes());
        out.extend_from_slice(b"0           "); // date
        out.extend_from_slice(b"0     "); // uid
        out.extend_from_slice(b"0     "); // gid
        out.extend_from_slice(b"100644  "); // mode
        let size = ext_name.len() + data.len();
        out.extend_from_slice(format!("{size:<10}").as_bytes());
        out.extend_from_slice(b"`\n");
        assert_eq!(out.len(), 60);
        out.extend_from_slice(ext_name);
        out.extend_from_slice(data);
        if out.len() % 2 == 1 {
            out.push(b'\n');
        }
        out
    }

    fn archive(members: &[Vec<u8>]) -> Vec<u8> {
        let mut out = AR_MAGIC.to_vec();
        for m in members {
            out.extend_from_slice(m);
        }
        out
    }

    fn file_type_of(data: Vec<u8>) -> String {
        let metadata = ARParser.parse(&TestReader::new(data)).unwrap();
        metadata.get_string("File:FileType").unwrap().to_string()
    }

    #[test]
    fn an_archive_with_no_mach_member_is_a_plain_static_library() {
        let data = archive(&[member("hello.o", b"", &[0x08, 0, 0, 0, 1, 2, 3, 4])]);
        assert_eq!(file_type_of(data), "Static library");
    }

    #[test]
    fn a_mach_member_promotes_the_archive() {
        for magic in MACH_MAGICS {
            let data = archive(&[member("hello.o", b"", &magic)]);
            assert_eq!(file_type_of(data), "Mach-O static library", "{magic:02x?}");
        }
    }

    #[test]
    fn the_deciding_member_is_not_the_first_one() {
        // EXE.a's shape: a `__.SYMDEF` symbol table under a BSD extended name,
        // then the Mach-O object. Reading only the first member -- or only the
        // first kilobyte -- misses the answer.
        let data = archive(&[
            member("#1/20", b"__.SYMDEF\0\0\0\0\0\0\0\0\0\0\0", &[0x08; 512]),
            member(
                "#1/12",
                b"hello.o\0\0\0\0\0",
                &[0xCF, 0xFA, 0xED, 0xFE, 7, 0, 0, 1],
            ),
        ]);
        assert_eq!(file_type_of(data), "Mach-O static library");
    }

    #[test]
    fn a_member_header_without_its_terminator_stops_the_walk() {
        let mut data = archive(&[member("hello.o", b"", &[0xCF, 0xFA, 0xED, 0xFE])]);
        data[8 + 58] = b'X';
        assert_eq!(file_type_of(data), "Static library");
    }

    #[test]
    fn a_fat_member_does_not_promote() {
        // `ExtractMachTags` tests `%machType`, which has only the four thin
        // magics -- `cafebabe` is not among them.
        let data = archive(&[member("fat.o", b"", &[0xCA, 0xFE, 0xBA, 0xBE])]);
        assert_eq!(file_type_of(data), "Static library");
    }

    #[test]
    fn a_non_archive_is_rejected() {
        assert!(
            ARParser
                .parse(&TestReader::new(b"not an ar".to_vec()))
                .is_err()
        );
    }
}
