#!/usr/bin/env rust-script
//! Generate minimal MP4 corpus files for fuzzing
//!
//! Run with: rustc create_corpus.rs && ./create_corpus

use std::fs;
use std::path::Path;

/// Create a minimal QuickTime file structure with user data
fn create_test_quicktime_file() -> Vec<u8> {
    let mut data = Vec::new();

    // ftyp atom (file type)
    data.extend_from_slice(&[
        0x00, 0x00, 0x00, 0x20, // size = 32
        b'f', b't', b'y', b'p', // type = ftyp
        b'q', b't', b' ', b' ', // major brand = "qt  "
        0x00, 0x00, 0x00, 0x00, // minor version
        b'q', b't', b' ', b' ', // compatible brand 1
        b'm', b'p', b'4', b'2', // compatible brand 2
        0x00, 0x00, 0x00, 0x00, // padding
        0x00, 0x00, 0x00, 0x00, // padding
    ]);

    // Create a ©nam (title) user data atom
    let title_text = b"Test Title";
    let title_data_size = 4 + title_text.len(); // 2 bytes size + 2 bytes lang + text
    let title_atom_size = 8 + title_data_size; // header + data

    let mut title_atom = Vec::new();
    title_atom.extend_from_slice(&(title_atom_size as u32).to_be_bytes());
    title_atom.extend_from_slice(b"\xa9nam"); // ©nam
    title_atom.extend_from_slice(&(title_text.len() as u16).to_be_bytes());
    title_atom.extend_from_slice(&[0x00, 0x00]); // language
    title_atom.extend_from_slice(title_text);

    // Create udta atom containing the title atom
    let udta_size = 8 + title_atom.len();
    let mut udta_atom = Vec::new();
    udta_atom.extend_from_slice(&(udta_size as u32).to_be_bytes());
    udta_atom.extend_from_slice(b"udta");
    udta_atom.extend_from_slice(&title_atom);

    // Create moov atom containing udta
    let moov_size = 8 + udta_atom.len();
    data.extend_from_slice(&(moov_size as u32).to_be_bytes());
    data.extend_from_slice(b"moov");
    data.extend_from_slice(&udta_atom);

    data
}

/// Create a minimal MP4 file with iTunes metadata
fn create_test_itunes_file() -> Vec<u8> {
    let mut data = Vec::new();

    // ftyp atom
    data.extend_from_slice(&[
        0x00, 0x00, 0x00, 0x20, // size = 32
        b'f', b't', b'y', b'p', // type = ftyp
        b'M', b'4', b'A', b' ', // major brand
        0x00, 0x00, 0x00, 0x00, // minor version
        b'M', b'4', b'A', b' ', // compatible brand 1
        b'm', b'p', b'4', b'2', // compatible brand 2
        0x00, 0x00, 0x00, 0x00, // padding
        0x00, 0x00, 0x00, 0x00, // padding
    ]);

    // Create a data atom with UTF-8 text "Artist Name"
    let artist_text = b"Artist Name";
    let mut data_atom = Vec::new();
    data_atom.extend_from_slice(&((8 + 8 + artist_text.len()) as u32).to_be_bytes());
    data_atom.extend_from_slice(b"data");
    data_atom.extend_from_slice(&[0x00, 0x00, 0x00, 0x01]); // type = UTF-8
    data_atom.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // reserved
    data_atom.extend_from_slice(artist_text);

    // Create ©ART atom containing data atom
    let artist_size = 8 + data_atom.len();
    let mut artist_atom = Vec::new();
    artist_atom.extend_from_slice(&(artist_size as u32).to_be_bytes());
    artist_atom.extend_from_slice(b"\xa9ART"); // ©ART
    artist_atom.extend_from_slice(&data_atom);

    // Create ilst atom containing artist atom
    let ilst_size = 8 + artist_atom.len();
    let mut ilst_atom = Vec::new();
    ilst_atom.extend_from_slice(&(ilst_size as u32).to_be_bytes());
    ilst_atom.extend_from_slice(b"ilst");
    ilst_atom.extend_from_slice(&artist_atom);

    // Create meta atom with hdlr and ilst
    let hdlr_atom = [
        0x00, 0x00, 0x00, 0x21, // size = 33
        b'h', b'd', b'l', b'r', // type = hdlr
        0x00, 0x00, 0x00, 0x00, // version/flags
        0x00, 0x00, 0x00, 0x00, // pre-defined
        b'm', b'd', b'i', b'r', // handler type
        b'a', b'p', b'p', b'l', // reserved
        0x00, 0x00, 0x00, 0x00, // reserved
        0x00, 0x00, 0x00, 0x00, // reserved
        0x00, // name (empty)
    ];

    let meta_size = 8 + 4 + hdlr_atom.len() + ilst_atom.len();
    let mut meta_atom = Vec::new();
    meta_atom.extend_from_slice(&(meta_size as u32).to_be_bytes());
    meta_atom.extend_from_slice(b"meta");
    meta_atom.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // version/flags
    meta_atom.extend_from_slice(&hdlr_atom);
    meta_atom.extend_from_slice(&ilst_atom);

    // Create udta atom containing meta
    let udta_size = 8 + meta_atom.len();
    let mut udta_atom = Vec::new();
    udta_atom.extend_from_slice(&(udta_size as u32).to_be_bytes());
    udta_atom.extend_from_slice(b"udta");
    udta_atom.extend_from_slice(&meta_atom);

    // Create moov atom
    let moov_size = 8 + udta_atom.len();
    data.extend_from_slice(&(moov_size as u32).to_be_bytes());
    data.extend_from_slice(b"moov");
    data.extend_from_slice(&udta_atom);

    data
}

fn main() -> std::io::Result<()> {
    let corpus_dir = Path::new("fuzz/corpus/fuzz_mp4");

    // Create minimal QuickTime file
    let quicktime_data = create_test_quicktime_file();
    fs::write(corpus_dir.join("minimal_quicktime.mp4"), quicktime_data)?;
    println!("Created minimal_quicktime.mp4");

    // Create minimal iTunes file
    let itunes_data = create_test_itunes_file();
    fs::write(corpus_dir.join("minimal_itunes.mp4"), itunes_data)?;
    println!("Created minimal_itunes.mp4");

    println!("\nMP4 corpus files created successfully!");
    Ok(())
}
