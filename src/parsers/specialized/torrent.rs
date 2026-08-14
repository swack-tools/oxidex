//! BitTorrent (.torrent) metadata parser.
//!
//! ExifTool 13.59 routes these files through
//! `Image::ExifTool::Torrent::ProcessTorrent` (Torrent.pm:276-292), which
//! bencode-decodes the whole file into a dictionary, requires that dictionary
//! to carry at least one of `announce`, `created by` or `info`
//! (Torrent.pm:286), and then walks it with `ExtractTags`
//! (Torrent.pm:193-270) against `Torrent::Main` (Torrent.pm:23-49).
//!
//! Only eight keys are pre-declared in `Torrent::Main`, eleven in
//! `Torrent::Info` (Torrent.pm:51-67), four in `Torrent::Files`
//! (Torrent.pm:77-83) and four in `Torrent::Profiles` (Torrent.pm:69-75).
//! Every *other* key found in the file is named on the fly
//! (Torrent.pm:203-210), so a faithful port has to reproduce the naming rule
//! as well as the tables -- see [`dynamic_name`].
//!
//! # List expansion
//!
//! `ExtractTags` expands list items into individually-indexed tags
//! (Torrent.pm:213-243): the index digits are substituted at the position of
//! each literal `1` in the declared tag name (`AnnounceList1` ->
//! `AnnounceList3`, `File1Length` -> `File2Length`), and any index left over
//! after those substitutions is appended. Nested lists are flattened one
//! level at a time (Torrent.pm:219) before an index is opened, which is why
//! the fixture's list-of-lists `announce-list` yields a single index run
//! `AnnounceList1..3` rather than a two-dimensional one.
//!
//! # Value classification
//!
//! Torrent.pm:166-181 decides at *parse* time whether a bencoded byte string
//! is text or binary: longer than 256 bytes is always binary; otherwise a
//! string containing anything outside `\t` and `\x20-\x7e` is decoded as
//! UTF-8 when it is valid UTF-8 and treated as binary when it is not. This
//! parser makes the same three-way decision, so `Pieces` (a concatenation of
//! raw SHA-1 digests) lands as [`TagValue::Binary`] without any tag-specific
//! special-casing.
//!
//! # What is deliberately not converted
//!
//! `Torrent::Main` declares exactly two conversions -- `creation date`'s
//! `ConvertUnixTime($val,1)` (Torrent.pm:40-41) and `Torrent::Files`'
//! `length` `ConvertFileSize` (Torrent.pm:79) -- and both are implemented
//! below against the cited Perl. Every other declared key is a bare
//! `{ }` or a `Name =>` rename, so there is nothing further to convert.
//!
//! # References
//!
//! - ExifTool source: `lib/Image/ExifTool/Torrent.pm`

use crate::core::value_formatter::format_file_size;
use crate::core::{FileReader, MetadataMap, TagValue};
use crate::io::timestamp::unix_time_to_local_exif_datetime;
use std::collections::{BTreeMap, VecDeque};

/// Torrent.pm:168, `if (length($value) > 256) { $val = \$value }` -- byte
/// strings longer than this are binary regardless of their content.
const MAX_TEXT_LEN: usize = 256;

/// Torrent.pm:153, `elsif ($more > 10000000)` -- byte strings whose declared
/// length runs this far past the buffer are skipped rather than read, and
/// reported as a placeholder string.
const SKIP_LEN: usize = 10_000_000;

/// A decoded bencode value.
///
/// The `Text`/`Binary` split is not a bencode distinction -- bencode has one
/// byte-string type. It is Torrent.pm:166-181's classification, applied at
/// parse time exactly as ExifTool applies it, so that the extraction walk
/// below sees the same two Perl shapes (plain scalar vs. SCALAR ref) that
/// `ExtractTags` sees.
#[derive(Debug, Clone)]
enum Bencode {
    Int(i64),
    /// A byte string that passed Torrent.pm:176-178's printable-ASCII test,
    /// or Torrent.pm:171-172's valid-UTF-8 test.
    Text(String),
    /// A byte string ExifTool would have returned as a SCALAR ref.
    Binary(Vec<u8>),
    List(Vec<Bencode>),
    /// Perl hash. `ExtractTags` iterates it with `sort keys`
    /// (Torrent.pm:198), i.e. bytewise ascending, which is exactly
    /// `BTreeMap<Vec<u8>, _>`'s iteration order.
    Dict(BTreeMap<Vec<u8>, Bencode>),
}

/// Bencode reader over an in-memory buffer.
///
/// ExifTool streams through a 64 kB sliding window (Torrent.pm:90-98) because
/// its input is a `RAF`; the window is a buffering strategy, not a semantic,
/// so this reads the file once and indexes it. The one place the window is
/// observable -- the >10 MB skip at Torrent.pm:153 -- is reproduced by
/// [`SKIP_LEN`].
struct BencodeReader<'a> {
    data: &'a [u8],
    pos: usize,
    error: Option<&'static str>,
}

impl<'a> BencodeReader<'a> {
    fn new(data: &'a [u8]) -> Self {
        BencodeReader {
            data,
            pos: 0,
            error: None,
        }
    }

    /// Torrent.pm:105-186, `ReadBencode`. Returns `None` for both "end of
    /// list/dictionary" (the `e` token, Torrent.pm:144) and for an error,
    /// the same conflation the Perl makes; callers distinguish by checking
    /// [`Self::error`].
    fn read(&mut self) -> Option<Bencode> {
        let tok = *self.data.get(self.pos)?;
        self.pos += 1;
        match tok {
            // Torrent.pm:120-122, `if ($tok eq 'i') { /\G(-?\d+)e/g }`.
            b'i' => {
                let start = self.pos;
                let mut end = start;
                if self.data.get(end) == Some(&b'-') {
                    end += 1;
                }
                let digits_start = end;
                while matches!(self.data.get(end), Some(c) if c.is_ascii_digit()) {
                    end += 1;
                }
                // The Perl match is anchored at \G and must reach an 'e'; a
                // failure returns undef *without* setting an error, leaving
                // the position where the token started.
                if end == digits_start || self.data.get(end) != Some(&b'e') {
                    return None;
                }
                let text = std::str::from_utf8(&self.data[start..end]).ok()?;
                let value = text.parse::<i64>().ok()?;
                self.pos = end + 1;
                Some(Bencode::Int(value))
            }
            // Torrent.pm:123-136, dictionary.
            b'd' => {
                let mut map = BTreeMap::new();
                loop {
                    let key = match self.read() {
                        Some(k) => k,
                        None => break,
                    };
                    // Torrent.pm:129-132: a SCALAR ref key is dereferenced
                    // and used; only ARRAY/HASH refs are rejected.
                    let key_bytes = match key {
                        Bencode::Text(s) => s.into_bytes(),
                        Bencode::Binary(b) => b,
                        Bencode::Int(_) => {
                            // A bencoded integer decodes to a plain Perl
                            // scalar, which `ref $k` reports as false, so
                            // Perl uses it as a key verbatim.
                            break;
                        }
                        Bencode::List(_) | Bencode::Dict(_) => {
                            self.error = Some("Bad dictionary key");
                            break;
                        }
                    };
                    let value = match self.read() {
                        Some(v) => v,
                        None => break,
                    };
                    map.insert(key_bytes, value);
                }
                Some(Bencode::Dict(map))
            }
            // Torrent.pm:137-143, list.
            b'l' => {
                let mut list = Vec::new();
                while let Some(v) = self.read() {
                    list.push(v);
                }
                Some(Bencode::List(list))
            }
            // Torrent.pm:144-145, end of dictionary or list: undef, no error.
            b'e' => None,
            // Torrent.pm:146-181, byte string.
            c if c.is_ascii_digit() => {
                let digits_start = self.pos - 1;
                let mut end = self.pos;
                while matches!(self.data.get(end), Some(d) if d.is_ascii_digit()) {
                    end += 1;
                }
                if self.data.get(end) != Some(&b':') {
                    self.error = Some("Bad format");
                    return None;
                }
                let len: usize = match std::str::from_utf8(&self.data[digits_start..end])
                    .ok()
                    .and_then(|s| s.parse().ok())
                {
                    Some(len) => len,
                    None => {
                        self.error = Some("Truncated byte string");
                        return None;
                    }
                };
                let value_start = end + 1;
                let Some(value_end) = value_start.checked_add(len) else {
                    self.error = Some("Truncated byte string");
                    return None;
                };
                if value_end > self.data.len() {
                    // Torrent.pm:153-155: an over-long value is skipped and
                    // reported as a placeholder; anything else that cannot be
                    // completed is a truncation error (Torrent.pm:179-181).
                    if len > SKIP_LEN {
                        self.pos = self.data.len();
                        return Some(Bencode::Binary(
                            format!("(Binary data {len} bytes)").into_bytes(),
                        ));
                    }
                    self.error = Some("Truncated byte string");
                    return None;
                }
                self.pos = value_end;
                Some(classify(&self.data[value_start..value_end]))
            }
            _ => {
                self.error = Some("Bad format");
                None
            }
        }
    }
}

/// Torrent.pm:166-181's three-way classification of a bencoded byte string.
fn classify(value: &[u8]) -> Bencode {
    if value.len() > MAX_TEXT_LEN {
        return Bencode::Binary(value.to_vec());
    }
    // Torrent.pm:170, `$value =~ /[^\t\x20-\x7e]/`.
    let printable = value
        .iter()
        .all(|&b| b == b'\t' || (0x20..=0x7e).contains(&b));
    if printable {
        // Every byte is ASCII, so this cannot fail.
        return Bencode::Text(String::from_utf8_lossy(value).into_owned());
    }
    // Torrent.pm:171-174: valid UTF-8 is decoded, anything else is binary.
    match std::str::from_utf8(value) {
        Ok(text) => Bencode::Text(text.to_string()),
        Err(_) => Bencode::Binary(value.to_vec()),
    }
}

/// One entry of a `Torrent::*` tag table.
struct TorrentTag {
    /// The bencode key, i.e. the Perl hash key the table is indexed by.
    id: &'static str,
    /// `Name =>`, or `None` when ExifTool derives it from the ID.
    name: Option<&'static str>,
    /// `SubDirectory => { TagTable => ... }`.
    subdir: Option<&'static [TorrentTag]>,
    /// `JoinPath => 1` (Torrent.pm:81-82).
    join_path: bool,
    conv: Conv,
}

/// The conversions the four `Torrent::*` tables declare. Everything else in
/// those tables is a bare `{ }` or a plain `Name =>` rename.
#[derive(Clone, Copy, PartialEq)]
enum Conv {
    None,
    /// Torrent.pm:40-41, `ValueConv => 'ConvertUnixTime($val,1)'` +
    /// `PrintConv => '$self->ConvertDateTime($val)'`.
    UnixTime,
    /// Torrent.pm:79, `PrintConv => 'ConvertFileSize($val)'`.
    FileSize,
}

const fn tag(id: &'static str, name: Option<&'static str>) -> TorrentTag {
    TorrentTag {
        id,
        name,
        subdir: None,
        join_path: false,
        conv: Conv::None,
    }
}

/// `%Image::ExifTool::Torrent::Files` (Torrent.pm:77-83).
const FILES: &[TorrentTag] = &[
    TorrentTag {
        id: "length",
        name: Some("File1Length"),
        subdir: None,
        join_path: false,
        conv: Conv::FileSize,
    },
    tag("md5sum", Some("File1MD5Sum")),
    TorrentTag {
        id: "path",
        name: Some("File1Path"),
        subdir: None,
        join_path: true,
        conv: Conv::None,
    },
    TorrentTag {
        id: "path.utf-8",
        name: Some("File1PathUTF-8"),
        subdir: None,
        join_path: true,
        conv: Conv::None,
    },
];

/// `%Image::ExifTool::Torrent::Profiles` (Torrent.pm:69-75).
const PROFILES: &[TorrentTag] = &[
    tag("acodec", Some("Profile1AudioCodec")),
    tag("height", Some("Profile1Height")),
    tag("vcodec", Some("Profile1VideoCodec")),
    tag("width", Some("Profile1Width")),
];

/// `%Image::ExifTool::Torrent::Info` (Torrent.pm:51-67).
const INFO: &[TorrentTag] = &[
    tag("file-duration", Some("File1Duration")),
    tag("file-media", Some("File1Media")),
    TorrentTag {
        id: "files",
        name: None,
        subdir: Some(FILES),
        join_path: false,
        conv: Conv::None,
    },
    tag("length", None),
    tag("md5sum", Some("MD5Sum")),
    tag("name", None),
    tag("name.utf-8", Some("NameUTF-8")),
    tag("piece length", Some("PieceLength")),
    tag("pieces", Some("Pieces")),
    tag("private", None),
    TorrentTag {
        id: "profiles",
        name: None,
        subdir: Some(PROFILES),
        join_path: false,
        conv: Conv::None,
    },
];

/// `%Image::ExifTool::Torrent::Main` (Torrent.pm:23-49).
const MAIN: &[TorrentTag] = &[
    tag("announce", None),
    tag("announce-list", Some("AnnounceList1")),
    tag("comment", None),
    tag("created by", Some("Creator")),
    TorrentTag {
        id: "creation date",
        name: Some("CreateDate"),
        subdir: None,
        join_path: false,
        conv: Conv::UnixTime,
    },
    tag("encoding", None),
    TorrentTag {
        id: "info",
        name: None,
        subdir: Some(INFO),
        join_path: false,
        conv: Conv::None,
    },
    tag("url-list", Some("URLList1")),
];

fn lookup(table: &'static [TorrentTag], id: &str) -> Option<&'static TorrentTag> {
    table.iter().find(|entry| entry.id == id)
}

/// ExifTool's fallback tag name for an ID with no table entry
/// (Torrent.pm:204-207):
///
/// ```text
/// my $name = ucfirst $tag;
/// $name =~ s/[^-_a-zA-Z0-9]+(.?)/\U$1/g;
/// $name = "Tag$name" if length($name) < 2 or $name !~ /^[A-Z]/;
/// ```
///
/// The substitution deletes each run of illegal characters and upper-cases
/// whichever character followed it, which is what turns `created by` into
/// `CreatedBy`. It also applies to a table entry with no explicit `Name`,
/// because `GetTagInfo` derives that name the same way.
fn dynamic_name(tag: &str) -> String {
    let mut name = String::with_capacity(tag.len());
    let mut chars = tag.chars();
    // `ucfirst`.
    if let Some(first) = chars.next() {
        name.extend(first.to_uppercase());
    }
    name.push_str(chars.as_str());

    let mut out = String::with_capacity(name.len());
    let mut rest = name.chars().peekable();
    while let Some(c) = rest.next() {
        if is_legal_name_char(c) {
            out.push(c);
            continue;
        }
        // Consume the whole illegal run, then upper-case the one character
        // that follows it (`(.?)` -- possibly nothing, at end of string).
        while matches!(rest.peek(), Some(&next) if !is_legal_name_char(next)) {
            rest.next();
        }
        if let Some(next) = rest.next() {
            out.extend(next.to_uppercase());
        }
    }

    if out.chars().count() < 2 || !out.starts_with(|c: char| c.is_ascii_uppercase()) {
        out.insert_str(0, "Tag");
    }
    out
}

/// `AddTagToTable`'s name derivation for a table entry that declares no
/// `Name =>` (ExifTool.pm:9254-9266):
///
/// ```text
/// $name = $tagID unless defined $name;
/// $name =~ tr/-_a-zA-Z0-9//dc;    # remove illegal characters
/// $name = ucfirst $name;          # capitalize first letter
/// $name = "Tag$name" if length($name) < 2 or $name !~ /^[A-Z]/i;
/// ```
///
/// Note that this *deletes* illegal characters, where [`dynamic_name`]'s rule
/// deletes them and upper-cases whatever followed. The two only agree on
/// single-word IDs, which happens to cover every unnamed entry in the four
/// `Torrent::*` tables -- the distinction is kept because it is real.
fn static_name(id: &str) -> String {
    let mut name: String = id.chars().filter(|&c| is_legal_name_char(c)).collect();
    if let Some(first) = name.chars().next() {
        let upper: String = first.to_uppercase().collect();
        name.replace_range(..first.len_utf8(), &upper);
    }
    if name.chars().count() < 2 || !name.starts_with(|c: char| c.is_ascii_alphabetic()) {
        name.insert_str(0, "Tag");
    }
    name
}

/// Torrent.pm:206's character class, `[^-_a-zA-Z0-9]`.
fn is_legal_name_char(c: char) -> bool {
    c == '-' || c == '_' || c.is_ascii_alphanumeric()
}

/// Torrent.pm:228-239: embed `index` into `name`, substituting at the
/// position of each literal `1` first and appending whatever is left over.
fn apply_indices(name: &str, index: &[u32]) -> String {
    let hash_count = name.matches('1').count();
    let mut out = String::with_capacity(name.len() + index.len());
    let mut consumed = 0usize;
    for c in name.chars() {
        if c == '1' && consumed < hash_count {
            // `my $idx = $index[$j] || ''` -- a missing or zero index
            // substitutes the empty string.
            match index.get(consumed) {
                Some(&i) if i != 0 => out.push_str(&i.to_string()),
                _ => {}
            }
            consumed += 1;
        } else {
            out.push(c);
        }
    }
    for &i in index.iter().skip(hash_count) {
        if out.ends_with(|c: char| c.is_ascii_digit()) {
            out.push('_');
        }
        out.push_str(&i.to_string());
    }
    out
}

/// Torrent.pm:193-270, `ExtractTags`.
///
/// A direct transliteration: `queue` is the Perl `@more` flattening queue,
/// `i` is the per-tag list counter `$i`, and `index` is `@index` (passed by
/// value in Perl, hence the local clone).
fn extract_tags(
    dict: &BTreeMap<Vec<u8>, Bencode>,
    table: &'static [TorrentTag],
    base_id: Option<&str>,
    base_name: Option<&str>,
    inherited_index: &[u32],
    metadata: &mut MetadataMap,
) -> usize {
    let mut count = 0;
    // Torrent.pm:198, `foreach $tag (sort keys %$hashPtr)`.
    for (key, value) in dict {
        let tag_key = String::from_utf8_lossy(key).into_owned();
        // Torrent.pm:202, `my $id = defined $baseID ? "$baseID/$tag" : $tag`.
        let id = match base_id {
            Some(base) => format!("{base}/{tag_key}"),
            None => tag_key.clone(),
        };
        let entry = lookup(table, &id);
        let declared_name = match entry {
            // A declared entry: either its explicit `Name =>`, or the name
            // `AddTagToTable` derives from the ID (ExifTool.pm:9254-9266).
            Some(entry) => entry.name.map_or_else(|| static_name(&id), str::to_string),
            // Torrent.pm:203-210's dynamic naming, which is the only path
            // that prefixes the enclosing hash's name.
            None => {
                let mut name = dynamic_name(&tag_key);
                if let Some(base) = base_name {
                    name.insert_str(0, base);
                }
                name
            }
        };
        let join_path = entry.is_some_and(|e| e.join_path);
        let subdir = entry.and_then(|e| e.subdir);
        let conv = entry.map_or(Conv::None, |e| e.conv);

        let mut queue: VecDeque<&Bencode> = VecDeque::new();
        let mut index = inherited_index.to_vec();
        let mut i: Option<u32> = None;
        let mut next = Some(value);

        // Torrent.pm:201, `for (; defined $val; $val = shift @more)`.
        while let Some(mut val) = next {
            // `join '/'` replaces the value outright (Torrent.pm:214-215);
            // every other list is flattened into `@more`.
            let mut joined: Option<String> = None;
            if let Bencode::List(items) = val {
                if join_path {
                    joined = Some(
                        items
                            .iter()
                            .map(|item| match item {
                                Bencode::Text(text) => text.clone(),
                                Bencode::Int(number) => number.to_string(),
                                // Torrent.pm:215, `ref $_ ? '(Binary data)'`.
                                _ => "(Binary data)".to_string(),
                            })
                            .collect::<Vec<_>>()
                            .join("/"),
                    );
                } else {
                    // Torrent.pm:217, `next unless @$val`.
                    if items.is_empty() {
                        next = queue.pop_front();
                        continue;
                    }
                    queue.extend(items.iter());
                    // Torrent.pm:219, `next if ref $more[0] eq 'ARRAY'`.
                    if matches!(queue.front(), Some(Bencode::List(_))) {
                        next = queue.pop_front();
                        continue;
                    }
                    val = queue.pop_front().expect("queue non-empty after extend");
                    // Torrent.pm:221, `$i or $i = 0, push(@index, $i)`.
                    if i.is_none() {
                        i = Some(0);
                        index.push(0);
                    }
                }
            }

            // Torrent.pm:224, `$index[-1] = ++$i if defined $i`.
            if let Some(counter) = i.as_mut() {
                *counter += 1;
                if let Some(last) = index.last_mut() {
                    *last = *counter;
                }
            }

            // Torrent.pm:225-243: append the indices to the ID and embed them
            // in the name.
            let mut id = id.clone();
            let mut name = declared_name.clone();
            if !index.is_empty() {
                id.push_str(
                    &index
                        .iter()
                        .map(u32::to_string)
                        .collect::<Vec<_>>()
                        .join("_"),
                );
                name = apply_indices(&name, &index);
            }

            match (joined, val) {
                (Some(path), _) => {
                    insert_tag(metadata, &name, ScalarValue::Text(path), conv);
                    count += 1;
                }
                // Torrent.pm:244-260, `if (ref $val eq 'HASH')`.
                (None, Bencode::Dict(hash)) => {
                    let (sub_table, root_id, root_name) = match subdir {
                        Some(sub) => (sub, None, None),
                        // Torrent.pm:255-258: a plain hash stays in the
                        // current table but namespaces what it contains.
                        None => (table, Some(id), Some(name)),
                    };
                    count += extract_tags(
                        hash,
                        sub_table,
                        root_id.as_deref(),
                        root_name.as_deref(),
                        &index,
                        metadata,
                    );
                }
                // Torrent.pm:261-265, `$et->HandleTag(...)`.
                (None, Bencode::Text(text)) => {
                    insert_tag(metadata, &name, ScalarValue::Text(text.clone()), conv);
                    count += 1;
                }
                (None, Bencode::Int(number)) => {
                    insert_tag(metadata, &name, ScalarValue::Int(*number), conv);
                    count += 1;
                }
                (None, Bencode::Binary(bytes)) => {
                    insert_tag(metadata, &name, ScalarValue::Binary(bytes.clone()), conv);
                    count += 1;
                }
                // Unreachable: a list is either joined or flattened above.
                (None, Bencode::List(_)) => {}
            }

            next = queue.pop_front();
        }
    }
    count
}

/// A bencode value that `HandleTag` receives, after list flattening.
enum ScalarValue {
    Text(String),
    Int(i64),
    Binary(Vec<u8>),
}

fn insert_tag(metadata: &mut MetadataMap, name: &str, value: ScalarValue, conv: Conv) {
    let key = format!("Torrent:{name}");
    let tag_value = match (conv, value) {
        (Conv::UnixTime, ScalarValue::Int(seconds)) => {
            TagValue::new_string(unix_time_to_local_exif_datetime(seconds))
        }
        (Conv::FileSize, ScalarValue::Int(bytes)) if bytes >= 0 => {
            TagValue::new_string(format_file_size(bytes as u64))
        }
        (_, ScalarValue::Text(s)) => TagValue::new_string(s),
        (_, ScalarValue::Int(n)) => TagValue::Integer(n),
        (_, ScalarValue::Binary(b)) => TagValue::Binary(b),
    };
    metadata.insert(key, tag_value);
}

/// Extract BitTorrent metadata by bencode-decoding the file and walking it
/// the way `Torrent::ExtractTags` does.
pub fn parse_torrent_metadata(reader: &dyn FileReader) -> std::result::Result<MetadataMap, String> {
    let size = usize::try_from(reader.size()).map_err(|_| "torrent file is too large")?;
    let data = reader.read(0, size).map_err(|error| error.to_string())?;

    let mut bencode = BencodeReader::new(&data);
    let root = bencode.read();

    // Torrent.pm:286: the file is only accepted as a torrent when the root is
    // a dictionary carrying at least one of these three keys.
    let Some(Bencode::Dict(dict)) = root else {
        return Err("not a bencoded dictionary".to_string());
    };
    if !dict.contains_key(b"announce".as_slice())
        && !dict.contains_key(b"created by".as_slice())
        && !dict.contains_key(b"info".as_slice())
    {
        return Err("bencoded dictionary is not a torrent".to_string());
    }

    let mut metadata = MetadataMap::new();
    extract_tags(&dict, MAIN, None, None, &[], &mut metadata);
    Ok(metadata)
}
