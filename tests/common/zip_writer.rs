//! Dependency-free ZIP writer for test fixtures.
//!
//! These fixtures were built with the `zip` crate until that dependency was
//! removed: `cargo audit` reads dev-dependencies too, so keeping it for tests
//! would have kept RUSTSEC-2026-0367/0368/0369 and the four transitive `zstd`
//! advisories in the audit output.
//!
//! It lives here rather than in `src/test_support.rs` because that module is
//! `#[cfg(test)]` on the library and so is invisible to integration tests,
//! which are separate crates.
//!
//! Writes only what the fixtures need: Stored and Deflate entries with chosen
//! DOS timestamps, directory entries, and an archive comment.

#![allow(dead_code)]

/// Dependency-free ZIP *writer*, for building test fixtures.
///
/// The fixtures in `zip.rs` were built with the `zip` crate. That crate had to
/// leave `Cargo.lock` entirely -- `cargo audit` reads dev-dependencies too, so
/// keeping it for tests would have kept RUSTSEC-2026-0367/0368/0369 and the
/// four transitive `zstd` advisories in the audit output. This replaces it.
///
/// It writes only what the tests need: Stored and Deflate entries with chosen
/// DOS timestamps, directory entries, and an archive comment. It is not a
/// general ZIP writer and is not compiled into the library.

/// Signatures, duplicated from the reader rather than imported: a writer
/// that shared the reader's constants could not catch the reader getting
/// them wrong, which is the one bug a round-trip test exists to find.
const LOCAL_HEADER_SIG: u32 = 0x0403_4b50;
const CENTRAL_HEADER_SIG: u32 = 0x0201_4b50;
const EOCD_SIG: u32 = 0x0605_4b50;

/// How an entry should be stored.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Method {
    Stored,
    Deflated,
}

impl Method {
    fn code(self) -> u16 {
        match self {
            Self::Stored => 0,
            Self::Deflated => 8,
        }
    }
}

/// One entry to write.
pub struct Entry<'a> {
    pub name: &'a str,
    pub body: &'a [u8],
    pub method: Method,
    /// `(year, month, day, hour, minute, second)`. Seconds are rounded
    /// down to even, as DOS stores them in two-second units.
    pub modified: Option<(u16, u8, u8, u8, u8, u8)>,
}

impl<'a> Entry<'a> {
    pub fn stored(name: &'a str, body: &'a [u8]) -> Self {
        Self {
            name,
            body,
            method: Method::Stored,
            modified: None,
        }
    }
    pub fn deflated(name: &'a str, body: &'a [u8]) -> Self {
        Self {
            name,
            body,
            method: Method::Deflated,
            modified: None,
        }
    }
    pub fn at(mut self, y: u16, mo: u8, d: u8, h: u8, mi: u8, s: u8) -> Self {
        self.modified = Some((y, mo, d, h, mi, s));
        self
    }
}

fn dos_datetime(modified: Option<(u16, u8, u8, u8, u8, u8)>) -> (u16, u16) {
    match modified {
        None => (0, 0),
        Some((y, mo, d, h, mi, s)) => {
            let date = (y.saturating_sub(1980) << 9) | ((mo as u16) << 5) | d as u16;
            let time = ((h as u16) << 11) | ((mi as u16) << 5) | (s as u16 / 2);
            (date, time)
        }
    }
}

/// CRC-32 (IEEE), computed directly so no crc crate is needed.
pub fn crc32(data: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &b in data {
        crc ^= b as u32;
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

fn deflate(data: &[u8]) -> Vec<u8> {
    use std::io::Write;
    let mut e = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
    e.write_all(data).expect("deflate write");
    e.finish().expect("deflate finish")
}

/// Builds an archive. `comment` becomes the EOCD comment; names ending in
/// `/` are written as directory entries with an empty payload.
pub fn build(entries: &[Entry<'_>], comment: &str) -> Vec<u8> {
    struct Central {
        name: Vec<u8>,
        method: u16,
        crc: u32,
        comp_len: u32,
        uncomp_len: u32,
        date: u16,
        time: u16,
        offset: u32,
        is_dir: bool,
    }

    let mut out: Vec<u8> = Vec::new();
    let mut centrals: Vec<Central> = Vec::new();

    for e in entries {
        let is_dir = e.name.ends_with('/');
        let body: &[u8] = if is_dir { &[] } else { e.body };
        let payload = match e.method {
            Method::Stored => body.to_vec(),
            Method::Deflated => deflate(body),
        };
        let (date, time) = dos_datetime(e.modified);
        let crc = crc32(body);
        let name = e.name.as_bytes().to_vec();
        let offset = out.len() as u32;

        out.extend_from_slice(&LOCAL_HEADER_SIG.to_le_bytes());
        out.extend_from_slice(&20u16.to_le_bytes()); // version needed
        // Bit 11 declares the name is UTF-8, which it is here.
        out.extend_from_slice(&0x0800u16.to_le_bytes());
        out.extend_from_slice(&e.method.code().to_le_bytes());
        out.extend_from_slice(&time.to_le_bytes());
        out.extend_from_slice(&date.to_le_bytes());
        out.extend_from_slice(&crc.to_le_bytes());
        out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        out.extend_from_slice(&(body.len() as u32).to_le_bytes());
        out.extend_from_slice(&(name.len() as u16).to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes()); // extra len
        out.extend_from_slice(&name);
        out.extend_from_slice(&payload);

        centrals.push(Central {
            name,
            method: e.method.code(),
            crc,
            comp_len: payload.len() as u32,
            uncomp_len: body.len() as u32,
            date,
            time,
            offset,
            is_dir,
        });
    }

    let cd_offset = out.len() as u32;
    for c in &centrals {
        out.extend_from_slice(&CENTRAL_HEADER_SIG.to_le_bytes());
        // Host 3 (UNIX) so unix_mode() is meaningful, matching what the
        // zip crate produced on the platforms these tests ran on.
        out.extend_from_slice(&0x031eu16.to_le_bytes());
        out.extend_from_slice(&20u16.to_le_bytes());
        out.extend_from_slice(&0x0800u16.to_le_bytes());
        out.extend_from_slice(&c.method.to_le_bytes());
        out.extend_from_slice(&c.time.to_le_bytes());
        out.extend_from_slice(&c.date.to_le_bytes());
        out.extend_from_slice(&c.crc.to_le_bytes());
        out.extend_from_slice(&c.comp_len.to_le_bytes());
        out.extend_from_slice(&c.uncomp_len.to_le_bytes());
        out.extend_from_slice(&(c.name.len() as u16).to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes()); // extra
        out.extend_from_slice(&0u16.to_le_bytes()); // comment
        out.extend_from_slice(&0u16.to_le_bytes()); // disk
        out.extend_from_slice(&0u16.to_le_bytes()); // internal attrs
        let mode: u32 = if c.is_dir { 0o040755 } else { 0o100644 };
        out.extend_from_slice(&(mode << 16).to_le_bytes());
        out.extend_from_slice(&c.offset.to_le_bytes());
        out.extend_from_slice(&c.name);
    }
    let cd_len = out.len() as u32 - cd_offset;

    let comment_bytes = comment.as_bytes();
    out.extend_from_slice(&EOCD_SIG.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes()); // this disk
    out.extend_from_slice(&0u16.to_le_bytes()); // cd start disk
    out.extend_from_slice(&(centrals.len() as u16).to_le_bytes());
    out.extend_from_slice(&(centrals.len() as u16).to_le_bytes());
    out.extend_from_slice(&cd_len.to_le_bytes());
    out.extend_from_slice(&cd_offset.to_le_bytes());
    out.extend_from_slice(&(comment_bytes.len() as u16).to_le_bytes());
    out.extend_from_slice(comment_bytes);
    out
}
