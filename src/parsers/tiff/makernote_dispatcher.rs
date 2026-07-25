//! MakerNote dispatcher
//!
//! Dispatches MakerNote data to the appropriate manufacturer parser
//! based on camera make.

#![allow(dead_code)]

use crate::parsers::tiff::ifd_parser::ByteOrder;
use crate::parsers::tiff::makernotes::*;
use std::collections::HashMap;

/// Dispatches MakerNote data to appropriate manufacturer parser
///
/// # Arguments
/// * `make` - Camera manufacturer name (e.g., "Canon", "Nikon", "Sony")
/// * `data` - Raw MakerNote data bytes
/// * `byte_order` - Byte order for parsing
/// * `tags` - HashMap to insert extracted tags into
///
/// # Returns
/// Ok(()) on success, Err(message) on parse failure
pub fn dispatch_makernote(
    make: &str,
    data: &[u8],
    byte_order: ByteOrder,
    tags: &mut HashMap<String, String>,
) -> Result<(), String> {
    use crate::parsers::tiff::makernotes::shared::MakerNoteParser;

    // Normalize make string (trim whitespace, case-insensitive matching)
    let make_normalized = make.trim().to_lowercase();

    // Dispatch to appropriate parser based on manufacturer
    let parser: Option<Box<dyn MakerNoteParser>> = match make_normalized.as_str() {
        "canon" => Some(Box::new(canon::CanonParser)),
        "nikon" | "nikon corporation" => Some(Box::new(nikon::NikonParser)),
        "sony" => Some(Box::new(sony::SonyParser)),
        "olympus" | "olympus corporation" | "olympus imaging corp." => {
            Some(Box::new(olympus::OlympusParser))
        }
        "panasonic" => Some(Box::new(panasonic::PanasonicParser)),
        "pentax" | "pentax corporation" | "ricoh imaging company, ltd." => {
            Some(Box::new(pentax::PentaxParser))
        }
        "fujifilm" | "fuji photo film co., ltd." => Some(Box::new(fujifilm::FujifilmParser)),
        "leica" | "leica camera ag" => Some(Box::new(leica::LeicaMakerNoteParser)),
        "sigma" | "sigma corporation" => Some(Box::new(sigma::SigmaMakerNoteParser)),
        "phase one" | "phase one a/s" => Some(Box::new(phaseone::PhaseOneMakerNoteParser)),
        "minolta" | "konica minolta" | "minolta co., ltd." => {
            Some(Box::new(minolta::MinoltaParser))
        }

        // Smartphones
        "apple" => Some(Box::new(apple::AppleParser)),
        "google" => Some(Box::new(google::GoogleParser)),
        "samsung" | "samsung electronics" => Some(Box::new(samsung::SamsungParser)),
        "microsoft" | "microsoft corporation" => Some(Box::new(microsoft::MicrosoftParser)),
        "qualcomm" => Some(Box::new(qualcomm::QualcommParser)),

        // Specialty devices
        "dji" => Some(Box::new(dji::DjiParser)),
        "flir" | "flir systems" => Some(Box::new(flir::FlirParser)),
        "gopro" => Some(Box::new(gopro::GoProParser)),
        "infiray" => Some(Box::new(infiray::InfiRayParser)),
        "lytro" | "lytro, inc." => Some(Box::new(lytro::LytroParser)),
        "nintendo" => Some(Box::new(nintendo::NintendoParser)),
        "parrot" => Some(Box::new(parrot::ParrotParser)),
        "reconyx" => Some(Box::new(reconyx::ReconxyParser)),
        "red" | "red.com" | "red digital cinema" => Some(Box::new(red::RedParser)),

        // Legacy cameras
        "casio" | "casio computer co.,ltd." => Some(Box::new(casio::CasioParser)),
        "ge" | "general electric" => Some(Box::new(ge::GeParser)),
        "hp" | "hewlett-packard" => Some(Box::new(hp::HpParser)),
        "jvc" | "victor company of japan, limited" => Some(Box::new(jvc::JvcParser)),
        "kodak" | "eastman kodak company" => Some(Box::new(kodak::KodakParser)),
        "leaf" => Some(Box::new(leaf::LeafParser)),
        "motorola" => Some(Box::new(motorola::MotorolaParser)),
        "ricoh" | "ricoh company, ltd." => Some(Box::new(ricoh::RicohParser)),
        "sanyo" | "sanyo electric co.,ltd." => Some(Box::new(sanyo::SanyoParser)),

        // Software applications
        "capture one" => Some(Box::new(captureone::CaptureOneParser)),
        "fotostation" | "fotoware" => Some(Box::new(fotostation::FotoStationParser)),
        "gimp" => Some(Box::new(gimp::GimpParser)),
        "adobe indesign" | "indesign" => Some(Box::new(indesign::InDesignParser)),
        "nikon capture" | "capture nx" => Some(Box::new(nikoncapture::NikonCaptureParser)),
        "photo mechanic" => Some(Box::new(photomechanic::PhotoMechanicParser)),
        "photoshop" | "adobe photoshop" => Some(Box::new(photoshop::PhotoshopParser)),
        "scalado" => Some(Box::new(scalado::ScaladoParser)),

        _ => None, // Unknown manufacturer
    };

    // If we have a parser, validate and parse
    if let Some(parser) = parser {
        // Validate header if parser provides validation
        if parser.validate_header(data) {
            // Parse MakerNote data
            parser.parse(data, byte_order, tags)?;
        }
    }

    // Silently succeed - not all makes have MakerNotes or valid headers
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dispatch_canon_makernote() {
        let data = b"Canon data here";
        let mut tags = HashMap::new();

        let result = dispatch_makernote("Canon", data, ByteOrder::LittleEndian, &mut tags);

        // Should succeed even with invalid header (dispatcher validates and skips)
        assert!(
            result.is_ok(),
            "Should handle invalid Canon data gracefully"
        );
        assert!(
            tags.is_empty(),
            "Should not extract tags from invalid Canon data"
        );
    }

    #[test]
    fn test_dispatch_unknown_manufacturer() {
        let data = b"unknown data";
        let mut tags = HashMap::new();

        let result = dispatch_makernote("UnknownMake", data, ByteOrder::LittleEndian, &mut tags);

        // Should succeed but not extract any tags
        assert!(result.is_ok());
        assert!(tags.is_empty(), "Should not extract tags for unknown make");
    }
}
