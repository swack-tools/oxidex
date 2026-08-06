//! RealAudio metafile (RAM) parser.
//!
//! RAM files may contain a Real streaming URL as their first text record.
//! This intentionally handles only that URL form; RealMedia and RealAudio
//! binary containers have different layouts and are out of scope here.

use crate::core::{FileReader, MetadataMap, TagValue};

const MAX_RAM_RECORD_LEN: usize = 256;

/// Extract the first streaming URL from a RAM metafile.
///
/// This follows ExifTool 13.59's `Real.pm` gate for RAM files: the first
/// record must begin with `pnm://`, `rtsp://`, or `http://`; HTTP URLs must
/// name a Real media resource.
pub fn parse_ram_metadata(reader: &dyn FileReader) -> std::result::Result<MetadataMap, String> {
    let data = reader
        .read(
            0,
            reader.size().min((MAX_RAM_RECORD_LEN + 1) as u64) as usize,
        )
        .map_err(|error| error.to_string())?;
    let url = ram_url(data).ok_or_else(|| "Invalid RAM streaming URL".to_string())?;

    let mut metadata = MetadataMap::new();
    metadata.insert("Real:URL", TagValue::new_string(url));
    Ok(metadata)
}

/// Return the bounded first record when it satisfies ExifTool's RAM URL gate.
pub(crate) fn ram_url(data: &[u8]) -> Option<&str> {
    let record_end = data
        .iter()
        .position(|&byte| matches!(byte, b'\r' | b'\n'))
        .unwrap_or(data.len());

    if record_end > MAX_RAM_RECORD_LEN {
        return None;
    }

    let url = std::str::from_utf8(&data[..record_end]).ok()?;
    is_ram_url(url).then_some(url)
}

fn is_ram_url(url: &str) -> bool {
    if url.starts_with("pnm://") || url.starts_with("rtsp://") {
        return true;
    }
    if !url.starts_with("http://") {
        return false;
    }

    let lowercase = url.to_ascii_lowercase();
    [".ra", ".rm", ".rv", ".rmvb", ".smil"]
        .iter()
        .any(|suffix| lowercase.ends_with(suffix))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_the_real_metafile_protocols() {
        assert!(is_ram_url("pnm://example.test/audio.rm"));
        assert!(is_ram_url("rtsp://example.test/video.rm"));
        assert!(is_ram_url("http://example.test/video.RMVB"));
        assert!(!is_ram_url("https://example.test/video.rm"));
        assert!(!is_ram_url("http://example.test/landing.html"));
    }
}
