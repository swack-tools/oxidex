//! XMP Dynamic Media (xmpDM) namespace handler
//!
//! Extracts audio and video metadata from the xmpDM namespace.
//!
//! **Namespace URI:** http://ns.adobe.com/xmp/1.0/DynamicMedia/
//!
//! **Key properties:**
//! - videoAlphaMode: Alpha channel mode for video
//! - videoFrameRate: Frame rate (fps)
//! - audioSampleRate: Audio sampling rate (Hz)
//! - audioChannelType: Audio channel configuration
//! - duration: Media duration
//! - videoCompressor: Video codec

use crate::error::Result;
use crate::parsers::xmp::namespaces::extract_text_content;
use quick_xml::events::Event;
use quick_xml::Reader;

/// Extract xmpDM namespace values from XMP data
///
/// # Parameters
///
/// - `xml_bytes`: Raw XMP XML data
///
/// # Returns
///
/// Vector of (tag_name, value) pairs with XMP-xmpDM: prefix
pub fn extract_xmp_dm_values(xml_bytes: &[u8]) -> Result<Vec<(String, String)>> {
    let mut reader = Reader::from_reader(xml_bytes);
    reader.config_mut().trim_text(true);

    let mut results = Vec::new();
    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                let tag_name = std::str::from_utf8(e.name().as_ref()).unwrap_or("");

                // Handle xmpDM properties
                if tag_name.contains("videoAlphaMode") {
                    if let Ok(value) = extract_text_content(&mut reader, &mut buf) {
                        if !value.is_empty() {
                            results.push(("XMP-xmpDM:VideoAlphaMode".to_string(), value));
                        }
                    }
                } else if tag_name.contains("videoFrameRate") {
                    if let Ok(value) = extract_text_content(&mut reader, &mut buf) {
                        if !value.is_empty() {
                            results.push(("XMP-xmpDM:VideoFrameRate".to_string(), value));
                        }
                    }
                } else if tag_name.contains("audioSampleRate") {
                    if let Ok(value) = extract_text_content(&mut reader, &mut buf) {
                        if !value.is_empty() {
                            results.push(("XMP-xmpDM:AudioSampleRate".to_string(), value));
                        }
                    }
                } else if tag_name.contains("audioChannelType") {
                    if let Ok(value) = extract_text_content(&mut reader, &mut buf) {
                        if !value.is_empty() {
                            results.push(("XMP-xmpDM:AudioChannelType".to_string(), value));
                        }
                    }
                } else if tag_name.contains("duration") && !tag_name.contains("scale") {
                    if let Ok(value) = extract_text_content(&mut reader, &mut buf) {
                        if !value.is_empty() {
                            results.push(("XMP-xmpDM:Duration".to_string(), value));
                        }
                    }
                } else if tag_name.contains("videoCompressor") {
                    if let Ok(value) = extract_text_content(&mut reader, &mut buf) {
                        if !value.is_empty() {
                            results.push(("XMP-xmpDM:VideoCompressor".to_string(), value));
                        }
                    }
                } else if tag_name.contains("audioCompressor") {
                    if let Ok(value) = extract_text_content(&mut reader, &mut buf) {
                        if !value.is_empty() {
                            results.push(("XMP-xmpDM:AudioCompressor".to_string(), value));
                        }
                    }
                } else if tag_name.contains("videoFrameSize") {
                    // Handle struct with w and h fields
                    if let Ok(value) = extract_text_content(&mut reader, &mut buf) {
                        if !value.is_empty() {
                            results.push(("XMP-xmpDM:VideoFrameSize".to_string(), value));
                        }
                    }
                } else if tag_name.contains("videoPixelAspectRatio") {
                    if let Ok(value) = extract_text_content(&mut reader, &mut buf) {
                        if !value.is_empty() {
                            results.push(("XMP-xmpDM:VideoPixelAspectRatio".to_string(), value));
                        }
                    }
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
        buf.clear();
    }

    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_video_metadata() {
        let xml = br#"
            <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"
                     xmlns:xmpDM="http://ns.adobe.com/xmp/1.0/DynamicMedia/">
              <rdf:Description>
                <xmpDM:videoAlphaMode>none</xmpDM:videoAlphaMode>
                <xmpDM:videoFrameRate>29.97</xmpDM:videoFrameRate>
                <xmpDM:videoCompressor>H.264</xmpDM:videoCompressor>
              </rdf:Description>
            </rdf:RDF>
        "#;

        let result = extract_xmp_dm_values(xml).unwrap();
        assert!(result
            .iter()
            .any(|(k, v)| k == "XMP-xmpDM:VideoAlphaMode" && v == "none"));
        assert!(result
            .iter()
            .any(|(k, v)| k == "XMP-xmpDM:VideoFrameRate" && v == "29.97"));
        assert!(result
            .iter()
            .any(|(k, v)| k == "XMP-xmpDM:VideoCompressor" && v == "H.264"));
    }

    #[test]
    fn test_extract_audio_metadata() {
        let xml = br#"
            <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"
                     xmlns:xmpDM="http://ns.adobe.com/xmp/1.0/DynamicMedia/">
              <rdf:Description>
                <xmpDM:audioSampleRate>48000</xmpDM:audioSampleRate>
                <xmpDM:audioChannelType>Stereo</xmpDM:audioChannelType>
                <xmpDM:audioCompressor>AAC</xmpDM:audioCompressor>
              </rdf:Description>
            </rdf:RDF>
        "#;

        let result = extract_xmp_dm_values(xml).unwrap();
        assert!(result
            .iter()
            .any(|(k, v)| k == "XMP-xmpDM:AudioSampleRate" && v == "48000"));
        assert!(result
            .iter()
            .any(|(k, v)| k == "XMP-xmpDM:AudioChannelType" && v == "Stereo"));
        assert!(result
            .iter()
            .any(|(k, v)| k == "XMP-xmpDM:AudioCompressor" && v == "AAC"));
    }

    #[test]
    fn test_extract_duration() {
        let xml = br#"
            <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"
                     xmlns:xmpDM="http://ns.adobe.com/xmp/1.0/DynamicMedia/">
              <rdf:Description>
                <xmpDM:duration>120.5</xmpDM:duration>
              </rdf:Description>
            </rdf:RDF>
        "#;

        let result = extract_xmp_dm_values(xml).unwrap();
        assert!(result
            .iter()
            .any(|(k, v)| k == "XMP-xmpDM:Duration" && v == "120.5"));
    }
}
