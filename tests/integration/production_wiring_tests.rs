use base64::Engine as _;
use oxidex::core::operations::{read_metadata, write_metadata};
use oxidex::core::{MetadataMap, TagValue};
use oxidex::error::ExifToolError;
use std::fs;
use std::io::Write;
use tempfile::NamedTempFile;

fn temp_with_suffix(suffix: &str) -> NamedTempFile {
    tempfile::Builder::new()
        .suffix(suffix)
        .tempfile()
        .expect("create temp file")
}

fn copy_fixture_to_temp(path: &str, suffix: &str) -> NamedTempFile {
    let temp = tempfile::Builder::new()
        .suffix(suffix)
        .tempfile()
        .expect("create temp fixture copy");
    fs::copy(path, temp.path()).expect("copy fixture");
    temp
}

fn read_temp_file(bytes: &[u8], suffix: &str) -> MetadataMap {
    let mut file = temp_with_suffix(suffix);
    file.write_all(bytes)
        .unwrap_or_else(|err| panic!("write fixture bytes for {suffix}: {err}"));
    read_metadata(file.path())
        .unwrap_or_else(|err| panic!("read through production metadata path for {suffix}: {err:?}"))
}

fn evtx_fixture() -> Vec<u8> {
    let mut data = vec![0u8; 4096];
    data[0..8].copy_from_slice(b"ElfFile\0");
    data[16..24].copy_from_slice(&4u64.to_le_bytes());
    data[24..32].copy_from_slice(&501u64.to_le_bytes());
    data[32..36].copy_from_slice(&128u32.to_le_bytes());
    data[36..38].copy_from_slice(&1u16.to_le_bytes());
    data[38..40].copy_from_slice(&3u16.to_le_bytes());
    data[40..42].copy_from_slice(&4096u16.to_le_bytes());
    data[42..44].copy_from_slice(&5u16.to_le_bytes());
    data[120..124].copy_from_slice(&0xDEADBEEFu32.to_le_bytes());
    data
}

fn prefetch_fixture() -> Vec<u8> {
    let mut data = vec![0u8; 256];
    data[0..4].copy_from_slice(&30u32.to_le_bytes());
    data[4..8].copy_from_slice(b"SCCA");
    data[12..16].copy_from_slice(&45_000u32.to_le_bytes());
    for (i, ch) in "NOTEPAD.EXE".encode_utf16().take(30).enumerate() {
        data[16 + i * 2..18 + i * 2].copy_from_slice(&ch.to_le_bytes());
    }
    data[76..80].copy_from_slice(&0xDEADBEEFu32.to_le_bytes());
    data[128..136].copy_from_slice(&133500420450000000u64.to_le_bytes());
    data[144..148].copy_from_slice(&7u32.to_le_bytes());
    data
}

fn registry_fixture() -> Vec<u8> {
    let mut data = vec![0u8; 4096];
    data[0..4].copy_from_slice(b"regf");
    data[4..8].copy_from_slice(&100u32.to_le_bytes());
    data[8..12].copy_from_slice(&100u32.to_le_bytes());
    data[12..20].copy_from_slice(&133000000000000000u64.to_le_bytes());
    data[20..24].copy_from_slice(&1u32.to_le_bytes());
    data[24..28].copy_from_slice(&5u32.to_le_bytes());
    data[36..40].copy_from_slice(&0x1000u32.to_le_bytes());
    data[40..44].copy_from_slice(&1_048_576u32.to_le_bytes());
    for (i, ch) in "SYSTEM".encode_utf16().enumerate() {
        data[48 + i * 2..50 + i * 2].copy_from_slice(&ch.to_le_bytes());
    }
    data
}

fn pcap_fixture() -> Vec<u8> {
    let mut data = Vec::new();
    data.extend_from_slice(&[0xd4, 0xc3, 0xb2, 0xa1]);
    data.extend_from_slice(&2u16.to_le_bytes());
    data.extend_from_slice(&4u16.to_le_bytes());
    data.extend_from_slice(&0i32.to_le_bytes());
    data.extend_from_slice(&0u32.to_le_bytes());
    data.extend_from_slice(&65535u32.to_le_bytes());
    data.extend_from_slice(&1u32.to_le_bytes());
    data
}

fn pcapng_fixture() -> Vec<u8> {
    let mut data = Vec::new();

    data.extend_from_slice(&0x0a0d0d0au32.to_le_bytes());
    data.extend_from_slice(&28u32.to_le_bytes());
    data.extend_from_slice(&0x1a2b3c4du32.to_le_bytes());
    data.extend_from_slice(&1u16.to_le_bytes());
    data.extend_from_slice(&0u16.to_le_bytes());
    data.extend_from_slice(&u64::MAX.to_le_bytes());
    data.extend_from_slice(&28u32.to_le_bytes());

    data.extend_from_slice(&1u32.to_le_bytes());
    data.extend_from_slice(&24u32.to_le_bytes());
    data.extend_from_slice(&1u16.to_le_bytes());
    data.extend_from_slice(&0u16.to_le_bytes());
    data.extend_from_slice(&65535u32.to_le_bytes());
    data.extend_from_slice(&0u32.to_le_bytes());
    data.extend_from_slice(&24u32.to_le_bytes());

    data
}

fn pcapng_with_idb_option_fixture() -> Vec<u8> {
    let mut data = Vec::new();

    data.extend_from_slice(&0x0a0d0d0au32.to_le_bytes());
    data.extend_from_slice(&28u32.to_le_bytes());
    data.extend_from_slice(&0x1a2b3c4du32.to_le_bytes());
    data.extend_from_slice(&1u16.to_le_bytes());
    data.extend_from_slice(&0u16.to_le_bytes());
    data.extend_from_slice(&u64::MAX.to_le_bytes());
    data.extend_from_slice(&28u32.to_le_bytes());

    data.extend_from_slice(&1u32.to_le_bytes());
    data.extend_from_slice(&32u32.to_le_bytes());
    data.extend_from_slice(&1u16.to_le_bytes());
    data.extend_from_slice(&0u16.to_le_bytes());
    data.extend_from_slice(&65535u32.to_le_bytes());
    data.extend_from_slice(&2u16.to_le_bytes());
    data.extend_from_slice(&4u16.to_le_bytes());
    data.extend_from_slice(b"eth0");
    data.extend_from_slice(&0u16.to_le_bytes());
    data.extend_from_slice(&0u16.to_le_bytes());
    data.extend_from_slice(&32u32.to_le_bytes());

    data
}

fn big_endian_pcapng_fixture() -> Vec<u8> {
    let mut data = Vec::new();

    data.extend_from_slice(&[0x0a, 0x0d, 0x0d, 0x0a]);
    data.extend_from_slice(&28u32.to_be_bytes());
    data.extend_from_slice(&0x1a2b3c4du32.to_be_bytes());
    data.extend_from_slice(&1u16.to_be_bytes());
    data.extend_from_slice(&0u16.to_be_bytes());
    data.extend_from_slice(&u64::MAX.to_be_bytes());
    data.extend_from_slice(&28u32.to_be_bytes());

    data.extend_from_slice(&1u32.to_be_bytes());
    data.extend_from_slice(&24u32.to_be_bytes());
    data.extend_from_slice(&1u16.to_be_bytes());
    data.extend_from_slice(&0u16.to_be_bytes());
    data.extend_from_slice(&65535u32.to_be_bytes());
    data.extend_from_slice(&0u32.to_be_bytes());
    data.extend_from_slice(&24u32.to_be_bytes());

    data
}

fn optionless_pcapng_fixture() -> Vec<u8> {
    let mut data = Vec::new();

    data.extend_from_slice(&[0x0a, 0x0d, 0x0d, 0x0a]);
    data.extend_from_slice(&28u32.to_le_bytes());
    data.extend_from_slice(&0x1a2b3c4du32.to_le_bytes());
    data.extend_from_slice(&1u16.to_le_bytes());
    data.extend_from_slice(&0u16.to_le_bytes());
    data.extend_from_slice(&u64::MAX.to_le_bytes());
    data.extend_from_slice(&28u32.to_le_bytes());

    data.extend_from_slice(&1u32.to_le_bytes());
    data.extend_from_slice(&20u32.to_le_bytes());
    data.extend_from_slice(&1u16.to_le_bytes());
    data.extend_from_slice(&0u16.to_le_bytes());
    data.extend_from_slice(&65535u32.to_le_bytes());
    data.extend_from_slice(&20u32.to_le_bytes());

    data
}

fn mixed_endian_pcapng_fixture() -> Vec<u8> {
    let mut data = optionless_pcapng_fixture();

    data.extend_from_slice(&[0x0a, 0x0d, 0x0d, 0x0a]);
    data.extend_from_slice(&28u32.to_be_bytes());
    data.extend_from_slice(&0x1a2b3c4du32.to_be_bytes());
    data.extend_from_slice(&1u16.to_be_bytes());
    data.extend_from_slice(&0u16.to_be_bytes());
    data.extend_from_slice(&u64::MAX.to_be_bytes());
    data.extend_from_slice(&28u32.to_be_bytes());

    data.extend_from_slice(&1u32.to_be_bytes());
    data.extend_from_slice(&20u32.to_be_bytes());
    data.extend_from_slice(&105u16.to_be_bytes());
    data.extend_from_slice(&0u16.to_be_bytes());
    data.extend_from_slice(&65535u32.to_be_bytes());
    data.extend_from_slice(&20u32.to_be_bytes());

    data
}

const CFB_FREE_SECTOR: u32 = 0xffff_ffff;
const CFB_END_OF_CHAIN: u32 = 0xffff_fffe;
const CFB_FAT_SECTOR: u32 = 0xffff_fffd;

fn ole_fixture() -> Vec<u8> {
    let mut data = vec![0u8; 512 * 3];
    data[0..8].copy_from_slice(&[0xd0, 0xcf, 0x11, 0xe0, 0xa1, 0xb1, 0x1a, 0xe1]);
    data[24..26].copy_from_slice(&0x003eu16.to_le_bytes());
    data[26..28].copy_from_slice(&3u16.to_le_bytes());
    data[28..30].copy_from_slice(&0xfffeu16.to_le_bytes());
    data[30..32].copy_from_slice(&9u16.to_le_bytes());
    data[32..34].copy_from_slice(&6u16.to_le_bytes());
    data[44..48].copy_from_slice(&1u32.to_le_bytes());
    data[48..52].copy_from_slice(&1u32.to_le_bytes());
    data[56..60].copy_from_slice(&4096u32.to_le_bytes());
    data[68..72].copy_from_slice(&CFB_END_OF_CHAIN.to_le_bytes());
    data[76..80].copy_from_slice(&0u32.to_le_bytes());
    for offset in (80..512).step_by(4) {
        data[offset..offset + 4].copy_from_slice(&CFB_FREE_SECTOR.to_le_bytes());
    }

    for offset in (512..1024).step_by(4) {
        data[offset..offset + 4].copy_from_slice(&CFB_FREE_SECTOR.to_le_bytes());
    }
    data[512..516].copy_from_slice(&CFB_FAT_SECTOR.to_le_bytes());
    data[516..520].copy_from_slice(&CFB_END_OF_CHAIN.to_le_bytes());

    data
}

fn ole_v4_fixture() -> Vec<u8> {
    let mut data = vec![0u8; 4096 * 3];
    data[0..8].copy_from_slice(&[0xd0, 0xcf, 0x11, 0xe0, 0xa1, 0xb1, 0x1a, 0xe1]);
    data[24..26].copy_from_slice(&0x003eu16.to_le_bytes());
    data[26..28].copy_from_slice(&4u16.to_le_bytes());
    data[28..30].copy_from_slice(&0xfffeu16.to_le_bytes());
    data[30..32].copy_from_slice(&12u16.to_le_bytes());
    data[32..34].copy_from_slice(&6u16.to_le_bytes());
    data[40..44].copy_from_slice(&1u32.to_le_bytes());
    data[44..48].copy_from_slice(&1u32.to_le_bytes());
    data[48..52].copy_from_slice(&1u32.to_le_bytes());
    data[56..60].copy_from_slice(&4096u32.to_le_bytes());
    data[68..72].copy_from_slice(&CFB_END_OF_CHAIN.to_le_bytes());
    data[76..80].copy_from_slice(&0u32.to_le_bytes());
    for offset in (80..512).step_by(4) {
        data[offset..offset + 4].copy_from_slice(&CFB_FREE_SECTOR.to_le_bytes());
    }

    let fat_offset = 4096;
    for offset in (fat_offset..fat_offset + 4096).step_by(4) {
        data[offset..offset + 4].copy_from_slice(&CFB_FREE_SECTOR.to_le_bytes());
    }
    data[fat_offset..fat_offset + 4].copy_from_slice(&CFB_FAT_SECTOR.to_le_bytes());
    data[fat_offset + 4..fat_offset + 8].copy_from_slice(&CFB_END_OF_CHAIN.to_le_bytes());

    let entry_offset = 4096 * 2;
    let name: Vec<u16> = "Root Entry\0".encode_utf16().collect();
    for (index, code_unit) in name.iter().enumerate() {
        data[entry_offset + index * 2..entry_offset + index * 2 + 2]
            .copy_from_slice(&code_unit.to_le_bytes());
    }
    data[entry_offset + 64..entry_offset + 66]
        .copy_from_slice(&((name.len() * 2) as u16).to_le_bytes());
    data[entry_offset + 66] = 5;

    data
}

fn write_ole_directory_entry(
    data: &mut [u8],
    offset: usize,
    name: &str,
    entry_type: u8,
    start_sector: u32,
    size: u32,
) {
    let name: Vec<u16> = format!("{name}\0").encode_utf16().collect();
    for (index, code_unit) in name.iter().enumerate() {
        data[offset + index * 2..offset + index * 2 + 2].copy_from_slice(&code_unit.to_le_bytes());
    }
    data[offset + 64..offset + 66].copy_from_slice(&((name.len() * 2) as u16).to_le_bytes());
    data[offset + 66] = entry_type;
    data[offset + 116..offset + 120].copy_from_slice(&start_sector.to_le_bytes());
    data[offset + 120..offset + 124].copy_from_slice(&size.to_le_bytes());
}

fn ole_v4_with_vba_stream_fixture() -> Vec<u8> {
    let vba_code = b"Sub Auto_Open()\n  Shell \"cmd.exe /c calc\"\nEnd Sub";
    let mut vba_stream = vec![0u8; 4096];
    vba_stream[..vba_code.len()].copy_from_slice(vba_code);
    let mut data = vec![0u8; 4096 * 4];
    data[0..8].copy_from_slice(&[0xd0, 0xcf, 0x11, 0xe0, 0xa1, 0xb1, 0x1a, 0xe1]);
    data[24..26].copy_from_slice(&0x003eu16.to_le_bytes());
    data[26..28].copy_from_slice(&4u16.to_le_bytes());
    data[28..30].copy_from_slice(&0xfffeu16.to_le_bytes());
    data[30..32].copy_from_slice(&12u16.to_le_bytes());
    data[32..34].copy_from_slice(&6u16.to_le_bytes());
    data[40..44].copy_from_slice(&1u32.to_le_bytes());
    data[44..48].copy_from_slice(&1u32.to_le_bytes());
    data[48..52].copy_from_slice(&1u32.to_le_bytes());
    data[56..60].copy_from_slice(&4096u32.to_le_bytes());
    data[68..72].copy_from_slice(&CFB_END_OF_CHAIN.to_le_bytes());
    data[76..80].copy_from_slice(&0u32.to_le_bytes());
    for offset in (80..512).step_by(4) {
        data[offset..offset + 4].copy_from_slice(&CFB_FREE_SECTOR.to_le_bytes());
    }

    let fat_offset = 4096;
    for offset in (fat_offset..fat_offset + 4096).step_by(4) {
        data[offset..offset + 4].copy_from_slice(&CFB_FREE_SECTOR.to_le_bytes());
    }
    for (sector, next) in [
        (0usize, CFB_FAT_SECTOR),
        (1, CFB_END_OF_CHAIN),
        (2, CFB_END_OF_CHAIN),
    ] {
        let offset = fat_offset + sector * 4;
        data[offset..offset + 4].copy_from_slice(&next.to_le_bytes());
    }

    let directory_offset = 4096 * 2;
    write_ole_directory_entry(
        &mut data,
        directory_offset,
        "Root Entry",
        5,
        CFB_FREE_SECTOR,
        0,
    );
    write_ole_directory_entry(
        &mut data,
        directory_offset + 128,
        "VBA",
        1,
        CFB_FREE_SECTOR,
        0,
    );
    write_ole_directory_entry(
        &mut data,
        directory_offset + 256,
        "Module1",
        2,
        2,
        vba_stream.len() as u32,
    );

    let stream_offset = 4096 * 3;
    data[stream_offset..stream_offset + vba_stream.len()].copy_from_slice(&vba_stream);
    data
}

fn ole_minifat_vba_stream_fixture() -> Vec<u8> {
    let vba_code = b"Sub Auto_Open()\n  Shell \"cmd.exe /c calc\"\nEnd Sub";
    let mut data = vec![0u8; 512 * 6];

    data[0..8].copy_from_slice(&[0xd0, 0xcf, 0x11, 0xe0, 0xa1, 0xb1, 0x1a, 0xe1]);
    data[24..26].copy_from_slice(&0x003eu16.to_le_bytes());
    data[26..28].copy_from_slice(&3u16.to_le_bytes());
    data[28..30].copy_from_slice(&0xfffeu16.to_le_bytes());
    data[30..32].copy_from_slice(&9u16.to_le_bytes());
    data[32..34].copy_from_slice(&6u16.to_le_bytes());
    data[44..48].copy_from_slice(&1u32.to_le_bytes());
    data[48..52].copy_from_slice(&1u32.to_le_bytes());
    data[56..60].copy_from_slice(&4096u32.to_le_bytes());
    data[60..64].copy_from_slice(&3u32.to_le_bytes());
    data[64..68].copy_from_slice(&1u32.to_le_bytes());
    data[68..72].copy_from_slice(&CFB_END_OF_CHAIN.to_le_bytes());
    data[76..80].copy_from_slice(&0u32.to_le_bytes());
    for offset in (80..512).step_by(4) {
        data[offset..offset + 4].copy_from_slice(&CFB_FREE_SECTOR.to_le_bytes());
    }

    let fat_offset = 512;
    for offset in (fat_offset..fat_offset + 512).step_by(4) {
        data[offset..offset + 4].copy_from_slice(&CFB_FREE_SECTOR.to_le_bytes());
    }
    for (sector, next) in [
        (0usize, CFB_FAT_SECTOR),
        (1, 2),
        (2, CFB_END_OF_CHAIN),
        (3, CFB_END_OF_CHAIN),
        (4, CFB_END_OF_CHAIN),
    ] {
        let offset = fat_offset + sector * 4;
        data[offset..offset + 4].copy_from_slice(&next.to_le_bytes());
    }

    let first_directory_offset = 512 * 2;
    write_ole_directory_entry(&mut data, first_directory_offset, "Root Entry", 5, 4, 512);
    write_ole_directory_entry(
        &mut data,
        first_directory_offset + 128,
        "VBA",
        1,
        CFB_FREE_SECTOR,
        0,
    );

    let second_directory_offset = 512 * 3;
    write_ole_directory_entry(
        &mut data,
        second_directory_offset,
        "Module1",
        2,
        0,
        vba_code.len() as u32,
    );

    let mini_fat_offset = 512 * 4;
    for offset in (mini_fat_offset..mini_fat_offset + 512).step_by(4) {
        data[offset..offset + 4].copy_from_slice(&CFB_FREE_SECTOR.to_le_bytes());
    }
    data[mini_fat_offset..mini_fat_offset + 4].copy_from_slice(&CFB_END_OF_CHAIN.to_le_bytes());

    let mini_stream_offset = 512 * 5;
    data[mini_stream_offset..mini_stream_offset + vba_code.len()].copy_from_slice(vba_code);

    data
}

fn binary_plist_fixture() -> Vec<u8> {
    let mut data = Vec::new();
    data.extend_from_slice(b"bplist00");
    data.extend(vec![0u8; 100]);
    let mut trailer = vec![0u8; 32];
    trailer[6] = 2;
    trailer[7] = 1;
    trailer[8..16].copy_from_slice(&5u64.to_be_bytes());
    trailer[24..32].copy_from_slice(&108u64.to_be_bytes());
    data.extend(trailer);
    data
}

fn xml_plist_fixture() -> Vec<u8> {
    br#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleIdentifier</key>
    <string>com.example.production</string>
</dict>
</plist>
"#
    .to_vec()
}

fn der_len(len: usize) -> Vec<u8> {
    if len < 0x80 {
        vec![len as u8]
    } else if len <= 0xFF {
        vec![0x81, len as u8]
    } else if len <= 0xFFFF {
        vec![0x82, (len >> 8) as u8, len as u8]
    } else if len <= 0xFF_FFFF {
        vec![0x83, (len >> 16) as u8, (len >> 8) as u8, len as u8]
    } else {
        vec![
            0x84,
            (len >> 24) as u8,
            (len >> 16) as u8,
            (len >> 8) as u8,
            len as u8,
        ]
    }
}

fn der_tlv(tag: u8, value: &[u8]) -> Vec<u8> {
    let mut der = vec![tag];
    der.extend(der_len(value.len()));
    der.extend_from_slice(value);
    der
}

fn der_sequence(elements: &[Vec<u8>]) -> Vec<u8> {
    let value: Vec<u8> = elements
        .iter()
        .flat_map(|part| part.iter().copied())
        .collect();
    der_tlv(0x30, &value)
}

fn der_integer(value: &[u8]) -> Vec<u8> {
    der_tlv(0x02, value)
}

fn der_oid(value: &[u8]) -> Vec<u8> {
    der_tlv(0x06, value)
}

fn der_null() -> Vec<u8> {
    der_tlv(0x05, &[])
}

fn der_utctime(value: &[u8]) -> Vec<u8> {
    der_tlv(0x17, value)
}

fn der_bit_string(value: &[u8]) -> Vec<u8> {
    let mut with_unused_bits = vec![0x00];
    with_unused_bits.extend_from_slice(value);
    der_tlv(0x03, &with_unused_bits)
}

fn der_algorithm_identifier(oid: &[u8]) -> Vec<u8> {
    der_sequence(&[der_oid(oid), der_null()])
}

fn der_x509_fixture_with_algorithms(
    signature_algorithm_oid: &[u8],
    public_key_algorithm_oid: &[u8],
    signature_size: usize,
) -> Vec<u8> {
    let signature_algorithm = der_algorithm_identifier(signature_algorithm_oid);
    let issuer = der_sequence(&[]);
    let validity = der_sequence(&[der_utctime(b"240101000000Z"), der_utctime(b"300101000000Z")]);
    let subject = der_sequence(&[]);
    let subject_public_key_info = der_sequence(&[
        der_algorithm_identifier(public_key_algorithm_oid),
        der_bit_string(&[0x00]),
    ]);
    let tbs_certificate = der_sequence(&[
        der_integer(&[0x01]),
        signature_algorithm.clone(),
        issuer,
        validity,
        subject,
        subject_public_key_info,
    ]);

    der_sequence(&[
        tbs_certificate,
        signature_algorithm,
        der_bit_string(&vec![0x00; signature_size]),
    ])
}

fn der_x509_fixture_with_empty_spki_bit_string() -> Vec<u8> {
    let sha256_with_rsa = b"\x2A\x86\x48\x86\xF7\x0D\x01\x01\x0B";
    let rsa_encryption = b"\x2A\x86\x48\x86\xF7\x0D\x01\x01\x01";
    let signature_algorithm = der_algorithm_identifier(sha256_with_rsa);
    let issuer = der_sequence(&[]);
    let validity = der_sequence(&[der_utctime(b"240101000000Z"), der_utctime(b"300101000000Z")]);
    let subject = der_sequence(&[]);
    let subject_public_key_info =
        der_sequence(&[der_algorithm_identifier(rsa_encryption), der_tlv(0x03, &[])]);
    let tbs_certificate = der_sequence(&[
        der_integer(&[0x01]),
        signature_algorithm.clone(),
        issuer,
        validity,
        subject,
        subject_public_key_info,
    ]);

    der_sequence(&[
        tbs_certificate,
        signature_algorithm,
        der_bit_string(&[0x00]),
    ])
}

fn der_x509_fixture_with_unused_bits_only_spki_bit_string() -> Vec<u8> {
    let sha256_with_rsa = b"\x2A\x86\x48\x86\xF7\x0D\x01\x01\x0B";
    let rsa_encryption = b"\x2A\x86\x48\x86\xF7\x0D\x01\x01\x01";
    let signature_algorithm = der_algorithm_identifier(sha256_with_rsa);
    let issuer = der_sequence(&[]);
    let validity = der_sequence(&[der_utctime(b"240101000000Z"), der_utctime(b"300101000000Z")]);
    let subject = der_sequence(&[]);
    let subject_public_key_info = der_sequence(&[
        der_algorithm_identifier(rsa_encryption),
        der_tlv(0x03, &[0x00]),
    ]);
    let tbs_certificate = der_sequence(&[
        der_integer(&[0x01]),
        signature_algorithm.clone(),
        issuer,
        validity,
        subject,
        subject_public_key_info,
    ]);

    der_sequence(&[
        tbs_certificate,
        signature_algorithm,
        der_bit_string(&[0x00]),
    ])
}

fn pem_certificate_fixture(der: &[u8]) -> Vec<u8> {
    let encoded = base64::engine::general_purpose::STANDARD.encode(der);
    format!("-----BEGIN CERTIFICATE-----\n{encoded}\n-----END CERTIFICATE-----\n").into_bytes()
}

fn der_x509_fixture() -> Vec<u8> {
    let sha256_with_rsa = b"\x2A\x86\x48\x86\xF7\x0D\x01\x01\x0B";
    let rsa_encryption = b"\x2A\x86\x48\x86\xF7\x0D\x01\x01\x01";
    der_x509_fixture_with_algorithms(sha256_with_rsa, rsa_encryption, 1)
}

fn der_ed25519_x509_fixture() -> Vec<u8> {
    let ed25519 = b"\x2B\x65\x70";
    der_x509_fixture_with_algorithms(ed25519, ed25519, 64)
}

fn der_dsa_x509_fixture() -> Vec<u8> {
    let sha256_with_rsa = b"\x2A\x86\x48\x86\xF7\x0D\x01\x01\x0B";
    let dsa = b"\x2A\x86\x48\xCE\x38\x04\x01";
    der_x509_fixture_with_algorithms(sha256_with_rsa, dsa, 48)
}

fn large_der_x509_fixture() -> Vec<u8> {
    let sha256_with_rsa = b"\x2A\x86\x48\x86\xF7\x0D\x01\x01\x0B";
    let rsa_encryption = b"\x2A\x86\x48\x86\xF7\x0D\x01\x01\x01";
    der_x509_fixture_with_algorithms(sha256_with_rsa, rsa_encryption, 5000)
}

fn very_large_der_x509_fixture() -> Vec<u8> {
    let sha256_with_rsa = b"\x2A\x86\x48\x86\xF7\x0D\x01\x01\x0B";
    let rsa_encryption = b"\x2A\x86\x48\x86\xF7\x0D\x01\x01\x01";
    der_x509_fixture_with_algorithms(sha256_with_rsa, rsa_encryption, 70_000)
}

fn der_tbs_certificate_only_fixture() -> Vec<u8> {
    let sha256_with_rsa = b"\x2A\x86\x48\x86\xF7\x0D\x01\x01\x0B";
    let rsa_encryption = b"\x2A\x86\x48\x86\xF7\x0D\x01\x01\x01";
    let signature_algorithm = der_algorithm_identifier(sha256_with_rsa);
    let issuer = der_sequence(&[]);
    let validity = der_sequence(&[der_utctime(b"240101000000Z"), der_utctime(b"300101000000Z")]);
    let subject = der_sequence(&[]);
    let subject_public_key_info = der_sequence(&[
        der_algorithm_identifier(rsa_encryption),
        der_bit_string(&[0x00]),
    ]);
    let tbs_certificate = der_sequence(&[
        der_integer(&[0x01]),
        signature_algorithm,
        issuer,
        validity,
        subject,
        subject_public_key_info,
    ]);

    der_sequence(&[tbs_certificate])
}

fn generic_der_sequence_fixture() -> Vec<u8> {
    let mut der = vec![0x30, 0x10];
    der.extend_from_slice(&[0x04, 0x04, b't', b'e', b's', b't']);
    der.extend_from_slice(&[0x02, 0x01, 0x01]);
    der.extend_from_slice(&[0x02, 0x01, 0x02]);
    der.extend_from_slice(&[0x01, 0x01, 0xFF]);
    der
}

fn long_body_ics_fixture() -> Vec<u8> {
    let mut ics = Vec::new();
    ics.extend_from_slice(b"BEGIN:VCALENDAR\r\n");
    ics.extend_from_slice(b"VERSION:2.0\r\n");
    ics.extend_from_slice(b"BEGIN:VEVENT\r\n");
    ics.extend_from_slice(b"SUMMARY:Long planning session\r\n");
    ics.extend_from_slice(b"DESCRIPTION:");
    ics.extend(std::iter::repeat_n(b'a', 1200));
    ics.extend_from_slice(b"\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n");

    let end_position = ics
        .windows(b"END:VCALENDAR".len())
        .position(|window| window == b"END:VCALENDAR")
        .expect("fixture includes END:VCALENDAR");
    assert!(end_position > 1024);
    ics
}

fn long_header_eml_fixture() -> Vec<u8> {
    let mut eml = Vec::new();
    for _ in 0..10 {
        eml.extend_from_slice(b"Received: by relay.example; Wed, 10 Jun 2026 12:00:00 +0000\r\n");
    }
    assert!(eml.len() > 600);
    eml.extend_from_slice(b"From: a@example.com\r\n");
    eml.extend_from_slice(b"To: b@example.com\r\n");
    eml.extend_from_slice(b"Date: Wed, 10 Jun 2026 12:00:00 +0000\r\n\r\nBody");
    assert!(eml.len() < 1024);
    eml
}

fn very_long_header_eml_fixture() -> Vec<u8> {
    let mut eml = Vec::new();
    for index in 0..24 {
        eml.extend_from_slice(
            format!("Received: from relay{index}.example by mx.example; Wed, 10 Jun 2026 12:00:00 +0000\r\n")
                .as_bytes(),
        );
    }
    assert!(eml.len() > 1024);
    eml.extend_from_slice(b"From: a@example.com\r\n");
    eml.extend_from_slice(b"To: b@example.com\r\n");
    eml.extend_from_slice(b"Date: Wed, 10 Jun 2026 12:00:00 +0000\r\n\r\nBody");
    assert!(eml.len() < 64 * 1024);
    eml
}

fn eml_with_non_utf8_body_fixture() -> Vec<u8> {
    let mut eml = b"From: a@example.com\r\n\
To: b@example.com\r\n\
Subject: Non UTF-8 Body\r\n\
Date: Wed, 10 Jun 2026 12:00:00 +0000\r\n\
Content-Type: text/plain; charset=windows-1252\r\n\
Content-Transfer-Encoding: 8bit\r\n\
\r\n"
        .to_vec();
    eml.extend_from_slice(&[0xff, 0xfe, 0x80, b'B', b'o', b'd', b'y']);
    eml
}

fn write_zip_fixture(entries: &[(&str, &[u8])], suffix: &str) -> tempfile::NamedTempFile {
    let file = temp_with_suffix(suffix);
    {
        let output = fs::File::create(file.path()).expect("open temp zip");
        let mut zip = zip::ZipWriter::new(output);
        let options = zip::write::SimpleFileOptions::default();
        for (name, data) in entries {
            zip.start_file(*name, options).expect("start zip entry");
            zip.write_all(data).expect("write zip entry");
        }
        zip.finish().expect("finish zip");
    }
    file
}

fn pages_zip_with_acsp_decoy_fixture() -> tempfile::NamedTempFile {
    let file = write_zip_fixture(
        &[
            ("XXXXXXacsp-decoy", b""),
            ("Index/Document.iwa", b""),
            (
                "Index/Metadata.plist",
                br#"<plist><dict><key>Author</key><string>Ada</string></dict></plist>"#,
            ),
        ],
        ".pages",
    );
    let bytes = fs::read(file.path()).expect("read Pages ZIP decoy fixture");
    assert_eq!(&bytes[36..40], b"acsp");
    file
}

#[test]
fn read_metadata_routes_evtx() {
    assert_eq!(
        read_temp_file(&evtx_fixture(), ".evtx").get("FileType"),
        Some(&TagValue::String("Windows Event Log".to_string()))
    );
}

#[test]
fn read_metadata_routes_prefetch() {
    assert_eq!(
        read_temp_file(&prefetch_fixture(), ".pf").get("Prefetch:FileType"),
        Some(&TagValue::String("Windows Prefetch".to_string()))
    );
}

#[test]
fn read_metadata_routes_registry() {
    assert_eq!(
        read_temp_file(&registry_fixture(), ".dat").get("Registry:SequenceValid"),
        Some(&TagValue::String("Yes".to_string()))
    );
}

#[test]
fn read_metadata_routes_pcap() {
    assert!(read_temp_file(&pcap_fixture(), ".pcap").contains_key("PCAP:Version"));
}

#[test]
fn read_metadata_routes_pcapng() {
    let metadata = read_temp_file(&pcapng_fixture(), ".pcapng");

    assert!(metadata.contains_key("PCAPNG:SectionCount"));
    assert!(metadata.contains_key("PCAPNG:LinkType"));
}

#[test]
fn read_metadata_routes_pcapng_idb_options() {
    let metadata = read_temp_file(&pcapng_with_idb_option_fixture(), ".pcapng");

    assert_eq!(
        metadata.get("PCAPNG:InterfaceName"),
        Some(&TagValue::String("eth0".to_string()))
    );
}

#[test]
fn read_metadata_routes_big_endian_pcapng() {
    let metadata = read_temp_file(&big_endian_pcapng_fixture(), ".pcapng");

    assert_eq!(
        metadata.get("PCAPNG:ByteOrder"),
        Some(&TagValue::String("Big-endian".to_string()))
    );
    assert_eq!(
        metadata.get("PCAPNG:LinkType"),
        Some(&TagValue::String("1".to_string()))
    );
    assert_eq!(
        metadata.get("PCAPNG:SectionCount"),
        Some(&TagValue::String("1".to_string()))
    );
}

#[test]
fn read_metadata_routes_optionless_pcapng_idb() {
    let metadata = read_temp_file(&optionless_pcapng_fixture(), ".pcapng");

    assert_eq!(
        metadata.get("PCAPNG:LinkType"),
        Some(&TagValue::String("1".to_string()))
    );
    assert_eq!(
        metadata.get("PCAPNG:SnapLen"),
        Some(&TagValue::String("65535 bytes".to_string()))
    );
    assert_eq!(
        metadata.get("PCAPNG:InterfaceCount"),
        Some(&TagValue::String("1".to_string()))
    );
}

#[test]
fn read_metadata_routes_mixed_endian_pcapng_sections() {
    let metadata = read_temp_file(&mixed_endian_pcapng_fixture(), ".pcapng");

    assert_eq!(
        metadata.get("PCAPNG:SectionCount"),
        Some(&TagValue::String("2".to_string()))
    );
    assert_eq!(
        metadata.get("PCAPNG:InterfaceCount"),
        Some(&TagValue::String("2".to_string()))
    );
}

#[test]
fn read_metadata_routes_ole() {
    let metadata = read_temp_file(&ole_fixture(), ".doc");

    assert_eq!(
        metadata.get("OLE:SectorSize"),
        Some(&TagValue::Integer(512))
    );
    assert!(metadata.contains_key("OLE:DirectoryEntryCount"));
}

#[test]
fn read_metadata_rejects_invalid_ole_sector_shift_without_panicking() {
    let mut fixture = ole_fixture();
    fixture[30..32].copy_from_slice(&u16::MAX.to_le_bytes());

    let mut file = temp_with_suffix(".doc");
    file.write_all(&fixture)
        .expect("write malformed OLE fixture");

    let outcome =
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| read_metadata(file.path())));

    assert!(outcome.is_ok(), "malformed OLE sector shift panicked");
    assert!(
        outcome.expect("checked above").is_err(),
        "malformed OLE sector shift should return an error"
    );
}

#[test]
fn read_metadata_rejects_invalid_ole_sector_shift() {
    let mut fixture = ole_fixture();
    fixture[30..32].copy_from_slice(&7u16.to_le_bytes());

    let mut file = temp_with_suffix(".doc");
    file.write_all(&fixture)
        .expect("write invalid OLE sector shift fixture");

    assert!(
        read_metadata(file.path()).is_err(),
        "invalid OLE sector shift should return an error"
    );
}

#[test]
fn read_metadata_rejects_excessive_ole_fat_sector_count_without_panicking() {
    let mut fixture = ole_fixture();
    fixture[44..48].copy_from_slice(&10_000u32.to_le_bytes());

    let mut file = temp_with_suffix(".doc");
    file.write_all(&fixture)
        .expect("write excessive OLE FAT sector count fixture");

    let outcome =
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| read_metadata(file.path())));

    assert!(outcome.is_ok(), "excessive FAT sector count panicked");
    let err = outcome
        .expect("checked above")
        .expect_err("excessive FAT sector count should return an error");
    assert!(
        matches!(
            err,
            ExifToolError::ParseError { ref message, .. }
                if message.contains("FAT sector count")
        ),
        "expected bounded FAT sector count error, got {err:?}"
    );
}

#[test]
fn read_metadata_routes_ole_v4_sector_layout() {
    let metadata = read_temp_file(&ole_v4_fixture(), ".doc");

    assert_eq!(
        metadata.get("OLE:SectorSize"),
        Some(&TagValue::Integer(4096))
    );
    assert_eq!(
        metadata.get("OLE:DirectoryEntryCount"),
        Some(&TagValue::Integer(1))
    );
}

#[test]
fn read_metadata_routes_ole_v4_vba_stream_from_sector_base() {
    let metadata = read_temp_file(&ole_v4_with_vba_stream_fixture(), ".doc");

    assert_eq!(
        metadata.get("OLE:HasVBAMacros"),
        Some(&TagValue::String("Yes".to_string()))
    );
    assert_eq!(
        metadata.get("OLE:HasAutoExec"),
        Some(&TagValue::String("Yes".to_string()))
    );
    assert_eq!(
        metadata.get("OLE:HasShellExecution"),
        Some(&TagValue::String("Yes".to_string()))
    );
}

#[test]
fn read_metadata_routes_ole_vba_stream_from_minifat_chain() {
    let metadata = read_temp_file(&ole_minifat_vba_stream_fixture(), ".doc");

    assert_eq!(
        metadata.get("OLE:HasVBAMacros"),
        Some(&TagValue::String("Yes".to_string()))
    );
    assert_eq!(
        metadata.get("OLE:VBAModuleNames"),
        Some(&TagValue::Array(vec![TagValue::String(
            "Module1".to_string()
        )]))
    );
    assert_eq!(
        metadata.get("OLE:HasAutoExec"),
        Some(&TagValue::String("Yes".to_string()))
    );
    assert_eq!(
        metadata.get("OLE:HasShellExecution"),
        Some(&TagValue::String("Yes".to_string()))
    );
}

#[test]
fn read_metadata_routes_binary_plist() {
    assert_eq!(
        read_temp_file(&binary_plist_fixture(), ".plist").get("Plist:Format"),
        Some(&TagValue::String("Binary".to_string()))
    );
}

#[test]
fn read_metadata_routes_xml_plist() {
    let metadata = read_temp_file(&xml_plist_fixture(), ".plist");

    assert_eq!(
        metadata.get("FileType"),
        Some(&TagValue::String("Plist".to_string()))
    );
    assert_eq!(
        metadata.get("Plist:Format"),
        Some(&TagValue::String("XML".to_string()))
    );
    assert_eq!(
        metadata.get("Plist:CFBundleIdentifier"),
        Some(&TagValue::String("com.example.production".to_string()))
    );
}

#[test]
fn read_metadata_routes_ics_before_txt() {
    let ics = b"BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\nSUMMARY:Planning\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";

    assert!(read_temp_file(ics, ".ics").contains_key("ICS:EventCount"));
}

#[test]
fn read_metadata_routes_long_root_ics_before_txt() {
    let metadata = read_temp_file(&long_body_ics_fixture(), ".ics");

    assert_eq!(
        metadata.get("FileType"),
        Some(&TagValue::String("ICS".to_string()))
    );
    assert_eq!(metadata.get("ICS:EventCount"), Some(&TagValue::Integer(1)));
}

#[test]
fn read_metadata_routes_eml_before_txt() {
    let eml = b"From: a@example.com\r\nTo: b@example.com\r\nSubject: Wiring\r\nDate: Wed, 10 Jun 2026 12:00:00 +0000\r\n\r\nBody";

    assert!(read_temp_file(eml, ".eml").contains_key("EML:Subject"));
}

#[test]
fn read_metadata_routes_eml_without_subject_before_txt() {
    let eml = b"From: a@example.com\r\nTo: b@example.com\r\nDate: Wed, 10 Jun 2026 12:00:00 +0000\r\n\r\nBody";
    let metadata = read_temp_file(eml, ".eml");

    assert_eq!(
        metadata.get("EML:From"),
        Some(&TagValue::String("a@example.com".to_string()))
    );
    assert!(metadata.contains_key("EML:Date"));
}

#[test]
fn read_metadata_routes_parser_supported_eml_without_date() {
    let eml = b"From: sender@example.com\r\nTo: recipient@example.com\r\nSubject: Test\r\n\r\nBody";
    let metadata = read_temp_file(eml, ".eml");

    assert_eq!(
        metadata.get("FileType"),
        Some(&TagValue::String("EML".to_string()))
    );
    assert_eq!(
        metadata.get("EML:Subject"),
        Some(&TagValue::String("Test".to_string()))
    );
}

#[test]
fn read_metadata_routes_embedded_vcalendar_example_as_txt() {
    let text = b"Here is an example calendar invite:\n\nBEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\nSUMMARY:Example\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
    let metadata = read_temp_file(text, ".txt");

    assert_eq!(
        metadata.get("FileType"),
        Some(&TagValue::String("TXT".to_string()))
    );
    assert!(!metadata.contains_key("ICS:EventCount"));
}

#[test]
fn read_metadata_routes_letter_with_from_to_labels_as_txt() {
    let text = b"From: Alice\nTo: Bob\n\nThis is a draft note, not an RFC mail message.";
    let metadata = read_temp_file(text, ".txt");

    assert_eq!(
        metadata.get("FileType"),
        Some(&TagValue::String("TXT".to_string()))
    );
    assert!(!metadata.contains_key("EML:From"));
}

#[test]
fn read_metadata_routes_letter_with_invalid_date_as_txt() {
    let text =
        b"From: Alice\nTo: Bob\nDate: tomorrow\n\nThis is a draft note, not an RFC mail message.";
    let metadata = read_temp_file(text, ".txt");

    assert_eq!(
        metadata.get("FileType"),
        Some(&TagValue::String("TXT".to_string()))
    );
    assert!(!metadata.contains_key("EML:From"));
}

#[test]
fn read_metadata_routes_letter_with_iso_date_as_txt() {
    let text = b"From: Alice\nTo: Bob\nDate: 2026-06-10 12:00:00 +0000\n\nThis is a draft note, not an RFC mail message.";
    let metadata = read_temp_file(text, ".txt");

    assert_eq!(
        metadata.get("FileType"),
        Some(&TagValue::String("TXT".to_string()))
    );
    assert!(!metadata.contains_key("EML:From"));
}

#[test]
fn read_metadata_routes_letter_with_address_like_labels_and_iso_date_as_txt() {
    let text = b"From: alice@example.com\nTo: bob@example.com\nDate: 2026-06-10 12:00:00 +0000\n\nThis is a draft note, not an RFC mail message.";
    let metadata = read_temp_file(text, ".txt");

    assert_eq!(
        metadata.get("FileType"),
        Some(&TagValue::String("TXT".to_string()))
    );
    assert!(!metadata.contains_key("EML:From"));
}

#[test]
fn read_metadata_routes_letter_with_rfc_date_and_non_address_labels_as_txt() {
    let text = b"From: Alice\nTo: Bob\nDate: Wed, 10 Jun 2026 12:00:00 +0000\n\nThis is a draft note, not an RFC mail message.";
    let metadata = read_temp_file(text, ".txt");

    assert_eq!(
        metadata.get("FileType"),
        Some(&TagValue::String("TXT".to_string()))
    );
    assert!(!metadata.contains_key("EML:From"));
}

#[test]
fn read_metadata_routes_eml_headers_after_byte_600() {
    let metadata = read_temp_file(&long_header_eml_fixture(), ".eml");

    assert_eq!(
        metadata.get("EML:From"),
        Some(&TagValue::String("a@example.com".to_string()))
    );
    assert!(metadata.contains_key("EML:Date"));
}

#[test]
fn read_metadata_routes_eml_headers_after_byte_1024() {
    let metadata = read_temp_file(&very_long_header_eml_fixture(), ".eml");

    assert_eq!(
        metadata.get("EML:From"),
        Some(&TagValue::String("a@example.com".to_string()))
    );
    assert_eq!(
        metadata.get("EML:To"),
        Some(&TagValue::String("b@example.com".to_string()))
    );
    assert!(metadata.contains_key("EML:Date"));
}

#[test]
fn read_metadata_routes_eml_with_non_utf8_body() {
    let metadata = read_temp_file(&eml_with_non_utf8_body_fixture(), ".eml");

    assert_eq!(
        metadata.get("FileType"),
        Some(&TagValue::String("EML".to_string()))
    );
    assert_eq!(
        metadata.get("EML:Subject"),
        Some(&TagValue::String("Non UTF-8 Body".to_string()))
    );
}

#[test]
fn read_metadata_routes_eml_with_svg_body_as_eml() {
    let eml = b"From: a@example.com\r\nTo: b@example.com\r\nDate: Wed, 10 Jun 2026 12:00:00 +0000\r\n\r\n<svg xmlns=\"http://www.w3.org/2000/svg\"><rect/></svg>";
    let metadata = read_temp_file(eml, ".eml");

    assert_eq!(
        metadata.get("FileType"),
        Some(&TagValue::String("EML".to_string()))
    );
    assert_eq!(
        metadata.get("EML:From"),
        Some(&TagValue::String("a@example.com".to_string()))
    );
}

#[test]
fn read_metadata_routes_lone_subject_text_as_txt() {
    let text = b"Subject: this is a plain text heading\n\nNothing else here.";
    let metadata = read_temp_file(text, ".txt");

    assert_eq!(
        metadata.get("FileType"),
        Some(&TagValue::String("TXT".to_string()))
    );
    assert!(!metadata.contains_key("EML:Subject"));
}

#[test]
fn read_metadata_routes_indented_email_header_text_as_txt() {
    let text = b" From: a@example.com\n To: b@example.com\n\nbody";
    let metadata = read_temp_file(text, ".txt");

    assert_eq!(
        metadata.get("FileType"),
        Some(&TagValue::String("TXT".to_string()))
    );
    assert!(!metadata.contains_key("EML:From"));
}

#[test]
fn read_metadata_routes_vcalendar_without_version_as_txt() {
    let text =
        b"BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nSUMMARY:Draft\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
    let metadata = read_temp_file(text, ".txt");

    assert_eq!(
        metadata.get("FileType"),
        Some(&TagValue::String("TXT".to_string()))
    );
    assert!(!metadata.contains_key("ICS:EventCount"));
}

#[test]
fn read_metadata_routes_email_like_svg_as_svg() {
    let svg = br#"<svg xmlns="http://www.w3.org/2000/svg">
From: a@example.com
To: b@example.com
<rect width="10" height="10"/>
</svg>"#;
    let metadata = read_temp_file(svg, ".svg");

    assert_eq!(
        metadata.get("FileType"),
        Some(&TagValue::String("SVG".to_string()))
    );
    assert!(!metadata.contains_key("EML:From"));
}

#[test]
fn read_metadata_routes_vcalendar_like_svg_as_svg() {
    let svg = br#"<svg xmlns="http://www.w3.org/2000/svg">
<text>BEGIN:VCALENDAR
VERSION:2.0
END:VCALENDAR</text>
</svg>"#;
    let metadata = read_temp_file(svg, ".svg");

    assert_eq!(
        metadata.get("FileType"),
        Some(&TagValue::String("SVG".to_string()))
    );
    assert!(!metadata.contains_key("ICS:EventCount"));
}

#[test]
fn read_metadata_routes_der_x509_detection() {
    let metadata = read_temp_file(&der_x509_fixture(), ".der");
    assert!(metadata.contains_key("X509:SHA256Fingerprint"));
}

#[test]
fn read_metadata_routes_ed25519_der_x509_detection() {
    let metadata = read_temp_file(&der_ed25519_x509_fixture(), ".der");
    assert!(metadata.contains_key("X509:SHA256Fingerprint"));
}

#[test]
fn read_metadata_routes_dsa_der_x509_detection() {
    let metadata = read_temp_file(&der_dsa_x509_fixture(), ".der");
    assert!(metadata.contains_key("X509:SHA256Fingerprint"));
}

#[test]
fn read_metadata_rejects_empty_spki_bit_string_without_panicking() {
    let fixture = der_x509_fixture_with_empty_spki_bit_string();
    let mut file = temp_with_suffix(".der");
    file.write_all(&fixture)
        .expect("write empty SPKI BIT STRING fixture");

    let outcome =
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| read_metadata(file.path())));

    assert!(outcome.is_ok(), "empty SPKI BIT STRING panicked");
    assert!(
        outcome.expect("checked above").is_err(),
        "empty SPKI BIT STRING should be rejected"
    );
}

#[test]
fn read_metadata_rejects_unused_bits_only_spki_bit_string_without_panicking() {
    let fixture = der_x509_fixture_with_unused_bits_only_spki_bit_string();
    let mut file = temp_with_suffix(".der");
    file.write_all(&fixture)
        .expect("write unused-bits-only SPKI BIT STRING fixture");

    let outcome =
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| read_metadata(file.path())));

    assert!(outcome.is_ok(), "unused-bits-only SPKI BIT STRING panicked");
    assert!(
        outcome.expect("checked above").is_err(),
        "unused-bits-only SPKI BIT STRING should be rejected"
    );
}

#[test]
fn read_metadata_handles_empty_spki_bit_string_in_pem_without_panicking() {
    let fixture = pem_certificate_fixture(&der_x509_fixture_with_empty_spki_bit_string());
    let mut file = temp_with_suffix(".pem");
    file.write_all(&fixture)
        .expect("write PEM empty SPKI BIT STRING fixture");

    let outcome =
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| read_metadata(file.path())));

    assert!(
        outcome.is_ok(),
        "PEM empty SPKI BIT STRING reached a parser panic"
    );
}

#[test]
fn read_metadata_routes_large_der_x509_detection() {
    let fixture = large_der_x509_fixture();
    assert!(fixture.len() > 4096);

    let metadata = read_temp_file(&fixture, ".der");
    assert!(metadata.contains_key("X509:SHA256Fingerprint"));
}

#[test]
fn read_metadata_routes_der_x509_larger_than_64k() {
    let fixture = very_large_der_x509_fixture();
    assert!(fixture.len() > 64 * 1024);

    let metadata = read_temp_file(&fixture, ".der");
    assert!(metadata.contains_key("X509:SHA256Fingerprint"));
}

#[test]
fn read_metadata_does_not_route_der_x509_with_trailing_bytes() {
    let mut fixture = der_x509_fixture();
    fixture.extend_from_slice(b"trailing non-DER bytes");
    let mut file = temp_with_suffix(".der");
    file.write_all(&fixture)
        .expect("write DER certificate with trailing bytes fixture");

    let err =
        read_metadata(file.path()).expect_err("DER with trailing bytes should be unsupported");
    assert!(
        matches!(
            err,
            ExifToolError::UnsupportedFormat { ref message } if message.contains("Unknown")
        ),
        "expected unsupported Unknown format, got {err:?}"
    );
}

#[test]
fn read_metadata_does_not_route_generic_der_sequence_to_x509() {
    let mut file = temp_with_suffix(".der");
    file.write_all(&generic_der_sequence_fixture())
        .expect("write generic DER fixture");

    let err = read_metadata(file.path()).expect_err("generic DER should be unsupported");
    assert!(
        matches!(
            err,
            ExifToolError::UnsupportedFormat { ref message } if message.contains("Unknown")
        ),
        "expected unsupported Unknown format, got {err:?}"
    );
}

#[test]
fn read_metadata_does_not_route_tbs_only_der_sequence_to_x509() {
    let mut file = temp_with_suffix(".der");
    file.write_all(&der_tbs_certificate_only_fixture())
        .expect("write TBS-only DER fixture");

    let err = read_metadata(file.path()).expect_err("TBS-only DER should be unsupported");
    assert!(
        matches!(
            err,
            ExifToolError::UnsupportedFormat { ref message } if message.contains("Unknown")
        ),
        "expected unsupported Unknown format, got {err:?}"
    );
}

#[test]
fn read_metadata_routes_pages_to_iwork_parser() {
    let metadata_plist = br#"<plist><dict><key>Author</key><string>Ada</string></dict></plist>"#;
    let pages = write_zip_fixture(
        &[
            ("Index/Document.iwa", b""),
            ("Index/Metadata.plist", metadata_plist),
        ],
        ".pages",
    );

    assert_eq!(
        read_metadata(pages.path())
            .unwrap()
            .get("iWork:Application"),
        Some(&TagValue::String("Pages".to_string()))
    );
}

#[test]
fn read_metadata_routes_pages_zip_with_acsp_decoy_to_iwork_parser() {
    let pages = pages_zip_with_acsp_decoy_fixture();

    assert_eq!(
        read_metadata(pages.path())
            .unwrap()
            .get("iWork:Application"),
        Some(&TagValue::String("Pages".to_string()))
    );
}

#[test]
fn read_metadata_routes_odf_mimetype_zip_as_generic_zip() {
    let odf = write_zip_fixture(
        &[
            ("mimetype", b"application/vnd.oasis.opendocument.text"),
            ("content.xml", b"<office:document-content/>"),
        ],
        ".zip",
    );
    let metadata = read_metadata(odf.path()).expect("ODF mimetype ZIP should route as ZIP");

    assert_eq!(metadata.get("ZIP:FileCount"), Some(&TagValue::Integer(2)));
    assert!(!metadata.contains_key("EPUB:FormatVersion"));
}

#[test]
fn read_metadata_routes_epub_mimetype_extended_value_as_generic_zip() {
    let invalid_epub = write_zip_fixture(
        &[
            ("mimetype", b"application/epub+zip-extra"),
            ("META-INF/container.xml", b""),
        ],
        ".zip",
    );
    let metadata =
        read_metadata(invalid_epub.path()).expect("extended EPUB mimetype should route as ZIP");

    assert_eq!(metadata.get("ZIP:FileCount"), Some(&TagValue::Integer(2)));
    assert!(!metadata.contains_key("EPUB:Title"));
}

#[test]
fn read_metadata_routes_epub_mimetype_with_late_suffix_as_generic_zip() {
    let mut mimetype = b"application/epub+zip".to_vec();
    mimetype.extend(std::iter::repeat_n(b' ', 256 - mimetype.len()));
    mimetype.push(b'x');

    let invalid_epub = write_zip_fixture(
        &[("mimetype", &mimetype), ("META-INF/container.xml", b"")],
        ".zip",
    );
    let metadata =
        read_metadata(invalid_epub.path()).expect("late-suffix EPUB mimetype should route as ZIP");

    assert_eq!(metadata.get("ZIP:FileCount"), Some(&TagValue::Integer(2)));
    assert!(!metadata.contains_key("EPUB:Title"));
}

#[test]
fn read_metadata_routes_valid_epub_mimetype_to_epub_parser() {
    let epub = write_zip_fixture(
        &[
            ("mimetype", b"application/epub+zip"),
            (
                "META-INF/container.xml",
                br#"<?xml version="1.0"?><container><rootfiles><rootfile full-path="OEBPS/content.opf"/></rootfiles></container>"#,
            ),
            (
                "OEBPS/content.opf",
                br#"<?xml version="1.0"?><package><metadata><dc:title>Exact EPUB</dc:title></metadata></package>"#,
            ),
        ],
        ".epub",
    );
    let metadata = read_metadata(epub.path()).expect("valid EPUB should route to EPUB parser");

    assert_eq!(
        metadata.get("EPUB:Title"),
        Some(&TagValue::String("Exact EPUB".to_string()))
    );
}

#[test]
fn read_metadata_routes_numbers_to_iwork_parser() {
    let numbers = write_zip_fixture(
        &[("Index/Document.iwa", b""), ("Index/Tables/table.iwa", b"")],
        ".numbers",
    );

    assert_eq!(
        read_metadata(numbers.path())
            .unwrap()
            .get("iWork:Application"),
        Some(&TagValue::String("Numbers".to_string()))
    );
}

#[test]
fn read_metadata_routes_keynote_to_iwork_parser() {
    let keynote = write_zip_fixture(&[("Index/Presentation.iwa", b"")], ".key");

    assert_eq!(
        read_metadata(keynote.path())
            .unwrap()
            .get("iWork:Application"),
        Some(&TagValue::String("Keynote".to_string()))
    );
}

#[test]
fn write_metadata_routes_png_and_pdf_writers() {
    let png = copy_fixture_to_temp("tests/fixtures/png/sample.png", ".png");
    let pdf = copy_fixture_to_temp("tests/fixtures/pdf/sample.pdf", ".pdf");

    let mut png_metadata = read_metadata(png.path()).expect("read png");
    // Keep the test focused on write routing; fixture metadata includes tags
    // that can fail validation before dispatch is reached.
    png_metadata.clear();
    png_metadata.insert("PNG:tEXt:Author", TagValue::new_string("OxiDex QA"));
    write_metadata(png.path(), &png_metadata).expect("write png through high-level API");
    let png_after = read_metadata(png.path()).expect("re-read png after write");
    assert_eq!(
        png_after.get("PNG:tEXt:Author"),
        Some(&TagValue::String("OxiDex QA".to_string())),
        "written PNG tag must survive a round-trip"
    );
    assert!(
        png_after.contains_key("PNG:ImageWidth"),
        "PNG image content must survive a metadata write"
    );

    let mut pdf_metadata = read_metadata(pdf.path()).expect("read pdf");
    pdf_metadata.clear();
    pdf_metadata.insert("PDF:Title", TagValue::new_string("OxiDex QA"));
    write_metadata(pdf.path(), &pdf_metadata).expect("write pdf through high-level API");
    let pdf_after = read_metadata(pdf.path()).expect("re-read pdf after write");
    assert_eq!(
        pdf_after.get("PDF:Title"),
        Some(&TagValue::String("OxiDex QA".to_string())),
        "written PDF tag must survive a round-trip"
    );
}

#[test]
fn write_metadata_rejects_tiff_until_writer_preserves_image_data() {
    // The TIFF writer rebuilds files from metadata alone and drops image data,
    // so the high-level API must refuse to route TIFF writes to it.
    let tiff = copy_fixture_to_temp("tests/fixtures/tiff/sample.tif", ".tif");
    let size_before = std::fs::metadata(tiff.path()).expect("stat tiff").len();

    let mut tiff_metadata = MetadataMap::new();
    tiff_metadata.insert("EXIF:Make", TagValue::new_string("OxiDex QA"));

    let result = write_metadata(tiff.path(), &tiff_metadata);
    assert!(
        result.is_err(),
        "TIFF writes must be rejected while the writer discards image data"
    );

    let size_after = std::fs::metadata(tiff.path()).expect("stat tiff").len();
    assert_eq!(
        size_before, size_after,
        "rejected TIFF write must leave the file untouched"
    );
}

#[test]
fn jpeg_write_preserves_scan_data_and_eoi() {
    // The segment parser cannot represent entropy-coded scan data; the writer
    // must copy everything after the SOS header verbatim or the image body is
    // silently amputated.
    let jpeg = copy_fixture_to_temp("tests/fixtures/jpeg/simple/synthetic_010.jpg", ".jpg");
    let before = fs::read(jpeg.path()).expect("read jpeg before write");
    assert_eq!(
        &before[before.len() - 2..],
        b"\xff\xd9",
        "fixture must end with EOI"
    );

    let mut metadata = MetadataMap::new();
    metadata.insert("IFD0:Make", TagValue::new_string("OxiDex QA"));
    write_metadata(jpeg.path(), &metadata).expect("write jpeg through high-level API");

    let after = fs::read(jpeg.path()).expect("read jpeg after write");
    assert_eq!(
        &after[after.len() - 2..],
        b"\xff\xd9",
        "EOI marker must survive a metadata write"
    );
    assert!(
        after.len() > before.len() / 2,
        "scan data must survive a metadata write: {} -> {} bytes",
        before.len(),
        after.len()
    );

    let reread = read_metadata(jpeg.path()).expect("re-read jpeg after write");
    assert_eq!(
        reread.get("IFD0:Make"),
        Some(&TagValue::String("OxiDex QA".to_string())),
        "written tag must survive a round-trip"
    );
}

#[test]
fn png_write_preserves_ztxt_on_unrelated_edit() {
    // The reader surfaces compressed text as PNG:zTXt:*, and the writer strips
    // all text chunks before rebuilding. Without re-serializing zTXt, an
    // unrelated tEXt edit silently drops every compressed text chunk.
    let png = copy_fixture_to_temp("tests/fixtures/png/sample.png", ".png");

    // Seed a zTXt chunk (exercises the new serializer end to end).
    let mut seed = MetadataMap::new();
    seed.insert("PNG:zTXt:Comment", TagValue::new_string("compressed note"));
    write_metadata(png.path(), &seed).expect("seed zTXt");
    assert_eq!(
        read_metadata(png.path())
            .expect("read seeded png")
            .get("PNG:zTXt:Comment"),
        Some(&TagValue::String("compressed note".to_string())),
        "zTXt must round-trip through the writer"
    );

    // Now make an unrelated tEXt edit and confirm the zTXt survives.
    let mut roundtrip = read_metadata(png.path()).expect("read png for round trip");
    roundtrip.insert("PNG:tEXt:Author", TagValue::new_string("OxiDex QA"));
    write_metadata(png.path(), &roundtrip).expect("write png round trip");

    let after = read_metadata(png.path()).expect("re-read png");
    assert_eq!(
        after.get("PNG:zTXt:Comment"),
        Some(&TagValue::String("compressed note".to_string())),
        "unrelated edit must not drop the zTXt chunk"
    );
    assert_eq!(
        after.get("PNG:tEXt:Author"),
        Some(&TagValue::String("OxiDex QA".to_string())),
    );
}

#[test]
fn write_metadata_rejects_incrementally_updated_pdf() {
    // Build a minimal incremental PDF: a base revision plus an update whose
    // trailer chains the base via /Prev. Rebuilding from the final xref alone
    // would drop the Catalog/Pages/Page objects, so the writer must reject it.
    let base = b"%PDF-1.4\n\
1 0 obj<</Type/Catalog/Pages 2 0 R>>endobj\n\
2 0 obj<</Type/Pages/Kids[3 0 R]/Count 1>>endobj\n\
3 0 obj<</Type/Page/Parent 2 0 R>>endobj\n\
4 0 obj<</Producer(orig)>>endobj\n";
    let mut pdf = base.to_vec();
    let base_xref_off = pdf.len();
    pdf.extend_from_slice(
        b"xref\n0 5\n\
0000000000 65535 f \n\
0000000009 00000 n \n\
0000000052 00000 n \n\
0000000101 00000 n \n\
0000000143 00000 n \n\
trailer<</Size 5/Root 1 0 R/Info 4 0 R>>\nstartxref\n",
    );
    pdf.extend_from_slice(format!("{base_xref_off}\n%%EOF\n").as_bytes());

    // Incremental update: rewrite object 4, chain the base xref via /Prev.
    let obj4_off = pdf.len();
    pdf.extend_from_slice(b"4 0 obj<</Producer(updated)>>endobj\n");
    let upd_xref_off = pdf.len();
    pdf.extend_from_slice(
        format!(
            "xref\n0 1\n0000000000 65535 f \n4 1\n{obj4_off:010} 00000 n \n\
trailer<</Size 5/Root 1 0 R/Info 4 0 R/Prev {base_xref_off}>>\nstartxref\n{upd_xref_off}\n%%EOF\n"
        )
        .as_bytes(),
    );

    let mut temp = temp_with_suffix(".pdf");
    temp.write_all(&pdf).expect("write incremental pdf");
    temp.flush().expect("flush pdf");
    let size_before = fs::metadata(temp.path()).expect("stat pdf").len();

    let mut metadata = MetadataMap::new();
    metadata.insert("PDF:Author", TagValue::new_string("Attacker"));
    let result = write_metadata(temp.path(), &metadata);
    assert!(
        result.is_err(),
        "incremental PDF writes must be rejected, not silently gut the document"
    );
    assert_eq!(
        size_before,
        fs::metadata(temp.path()).expect("stat pdf").len(),
        "rejected PDF write must leave the file untouched"
    );
}
