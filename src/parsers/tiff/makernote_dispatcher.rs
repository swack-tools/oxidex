//! MakerNote dispatcher
//!
//! Dispatches MakerNote data to the appropriate manufacturer parser
//! based on camera make.

#![allow(dead_code)]

use crate::parsers::tiff::ifd_parser::ByteOrder;
use crate::parsers::tiff::makernotes::makernote_context::MakerNoteContext;
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
    dispatch_makernote_with_model(make, None, data, byte_order, tags)
}

/// Dispatches MakerNote data to the appropriate manufacturer parser, passing
/// along the camera model.
///
/// Some MakerNote structures cannot be decoded from their own bytes alone --
/// Nikon's `AFInfo` picks its byte order from the model string, for example --
/// so callers that already know the model should prefer this entry point.
/// [`dispatch_makernote`] is the same call with no model.
///
/// # Arguments
/// * `make` - Camera manufacturer name (e.g., "Canon", "Nikon", "Sony")
/// * `model` - Camera model name (EXIF `Model`), if known
/// * `data` - Raw MakerNote data bytes
/// * `byte_order` - Byte order for parsing
/// * `tags` - HashMap to insert extracted tags into
///
/// # Returns
/// Ok(()) on success, Err(message) on parse failure
pub fn dispatch_makernote_with_model(
    make: &str,
    model: Option<&str>,
    data: &[u8],
    byte_order: ByteOrder,
    tags: &mut HashMap<String, String>,
) -> Result<(), String> {
    dispatch_makernote_with_model_and_values(
        make,
        model,
        data,
        byte_order,
        tags,
        &mut HashMap::new(),
    )
}

pub fn dispatch_makernote_with_model_and_values(
    make: &str,
    model: Option<&str>,
    data: &[u8],
    byte_order: ByteOrder,
    tags: &mut HashMap<String, String>,
    value_forms: &mut HashMap<String, String>,
) -> Result<(), String> {
    dispatch_makernote_with_context_and_values(
        make,
        model,
        &MakerNoteContext::detached(data),
        byte_order,
        tags,
        value_forms,
    )
}

/// Pentax's own MakerNote signature, also used by the Pentax-built Samsung
/// GX bodies (ExifTool dispatches Pentax MakerNotes on this signature, not on
/// the Make string).
const PENTAX_AOC_SIGNATURE: &[u8] = b"AOC\0";

/// Match the vendors whose Make string varies too much for a literal list.
///
/// Returns `None` for everything else so the caller falls through to the
/// exact-match table.
fn parser_for_make_prefix(
    make: &str,
    data: &[u8],
) -> Option<Box<dyn crate::parsers::tiff::makernotes::shared::MakerNoteParser>> {
    use crate::parsers::tiff::makernotes::shared::MakerNoteParser;

    if make.starts_with("olympus") || make.starts_with("om digital solutions") {
        return Some(Box::new(olympus::OlympusParser) as Box<dyn MakerNoteParser>);
    }
    if make.starts_with("pentax") || make.starts_with("asahi optical") {
        return Some(Box::new(pentax::PentaxParser::default()) as Box<dyn MakerNoteParser>);
    }
    // `make` reaches here already lowercased, so this is ExifTool's
    // `$$self{Make} =~ /^RICOH/` (Pentax.pm:3032) -- which the modern
    // "RICOH IMAGING COMPANY, LTD." Pentax bodies satisfy too.
    if make.starts_with("ricoh imaging") {
        return Some(
            Box::new(pentax::PentaxParser { ricoh_make: true }) as Box<dyn MakerNoteParser>
        );
    }
    // GE cameras are branded "General Imaging Co." in EXIF -- the literal
    // table below only listed "ge" and "general electric", so the one GE file
    // in the sample corpus never reached the GE parser. ExifTool keys off the
    // maker note signature instead (MakerNotes.pm:137,
    // `Condition => '$$valPt =~ /^GE(\0\0|NIC\0)/'`).
    if make.starts_with("general imaging") {
        return Some(Box::new(ge::GeParser) as Box<dyn MakerNoteParser>);
    }
    // MakerNotes.pm's MakerNoteFLIR is selected by `Make =~ /^(FLIR
    // Systems|Teledyne FLIR)/`.  The corpus FLIR_i7 identifies itself as
    // "FLIR Systems AB", so an exact "flir systems" arm silently skipped its
    // TIFF MakerNote and left the three rational measurements unreachable.
    if make.starts_with("flir systems") || make.starts_with("teledyne flir") {
        return Some(Box::new(flir::FlirParser) as Box<dyn MakerNoteParser>);
    }
    if make.starts_with("samsung") {
        // The Samsung GX-1L/GX-1S/GX10/GX20 are rebadged Pentax bodies and
        // write a Pentax "AOC\0" MakerNote; ExifTool files their tags under
        // family-1 "Pentax". Every other Samsung goes to the Samsung parser.
        if data.len() >= 4 && &data[0..4] == PENTAX_AOC_SIGNATURE {
            return Some(Box::new(pentax::PentaxParser::default()) as Box<dyn MakerNoteParser>);
        }
        return Some(Box::new(samsung::SamsungParser) as Box<dyn MakerNoteParser>);
    }
    None
}

/// Dispatches a MakerNote whose position inside its enclosing TIFF block is
/// known.
///
/// This is the entry point to prefer. MakerNote value offsets are measured from
/// the enclosing TIFF header rather than from the MakerNote payload, and they
/// routinely address bytes past the payload's declared end, so a decoder handed
/// only the payload cannot resolve them -- see
/// [`MakerNoteContext`](crate::parsers::tiff::makernotes::makernote_context::MakerNoteContext).
/// [`dispatch_makernote`] and [`dispatch_makernote_with_model`] are this call
/// with a detached context, which reaches exactly as far as the declared block
/// and so behaves as they always did.
///
/// # Arguments
/// * `make` - Camera manufacturer name (e.g., "Canon", "Nikon", "Sony")
/// * `model` - Camera model name (EXIF `Model`), if known
/// * `ctx` - Where the MakerNote sits, and how far its decoder may read
/// * `byte_order` - Byte order for parsing
/// * `tags` - HashMap to insert extracted tags into
///
/// # Returns
/// Ok(()) on success, Err(message) on parse failure
pub fn dispatch_makernote_with_context(
    make: &str,
    model: Option<&str>,
    ctx: &MakerNoteContext<'_>,
    byte_order: ByteOrder,
    tags: &mut HashMap<String, String>,
) -> Result<(), String> {
    dispatch_makernote_with_context_and_values(
        make,
        model,
        ctx,
        byte_order,
        tags,
        &mut HashMap::new(),
    )
}

pub fn dispatch_makernote_with_context_and_values(
    make: &str,
    model: Option<&str>,
    ctx: &MakerNoteContext<'_>,
    byte_order: ByteOrder,
    tags: &mut HashMap<String, String>,
    value_forms: &mut HashMap<String, String>,
) -> Result<(), String> {
    dispatch_makernote_with_context_and_values_and_file_type(
        make,
        model,
        ctx,
        byte_order,
        None,
        tags,
        value_forms,
    )
}

pub fn dispatch_makernote_with_context_and_values_and_file_type(
    make: &str,
    model: Option<&str>,
    ctx: &MakerNoteContext<'_>,
    byte_order: ByteOrder,
    file_type: Option<&'static str>,
    tags: &mut HashMap<String, String>,
    value_forms: &mut HashMap<String, String>,
) -> Result<(), String> {
    use crate::parsers::tiff::makernotes::shared::MakerNoteParser;

    let data = ctx.payload();

    // Normalize make string (trim whitespace, case-insensitive matching)
    let make_normalized = make.trim().to_lowercase();

    // Phase One's MakerNote is dispatched by ExifTool purely on its own
    // signature (MakerNotes.pm's `MakerNotePhaseOne` Condition has no Make
    // check at all), because the format is OEMed under multiple brand names.
    // Leaf -- acquired by Phase One -- writes the identical directory shape
    // under `Make: Leaf`; matching only "phase one"/"phase one a/s" left
    // every Leaf-branded .IIQ unreachable (`Make=="Leaf"` matched nothing in
    // the table below, so it silently produced zero PhaseOne: tags for real
    // Leaf/Phase One IIQ files). Check the signature before the Make-keyed
    // table so it wins regardless of brand.
    if phaseone::is_phaseone_makernote(data) {
        let parser = phaseone::PhaseOneMakerNoteParser;
        parser.parse_with_context(ctx, byte_order, model, tags)?;
        return Ok(());
    }

    // MakerNotes.pm:275-284 selects Kodak Type-2 by its payload, not Make:
    // Kodak, HP, Pentax and Minolta all sold cameras carrying this record.
    // This must precede the Pentax Make-prefix route so EI-200 keeps its
    // ExifTool `Kodak:` family.
    if kodak::is_kodak_type2_makernote(data) {
        let parser = kodak::KodakParser;
        parser.parse_with_context(ctx, byte_order, model, tags)?;
        return Ok(());
    }

    // MakerNotes.pm:206-213 dispatches HP Type4 by its `IIII\x04|\x05\0`
    // payload before Make matching.  Pentax-branded Optio E65 files carry
    // this HP record, so routing by Make would otherwise lose its exact
    // binary-table fields.
    if hp::HpParser::is_type4_makernote(data) {
        hp::HpParser::parse_type4(data, tags);
        return Ok(());
    }

    // MakerNotes.pm selects Ricoh Type2 from its TIFF signature before Make
    // matching.  Pentax-branded Ricoh XG bodies otherwise take the broad
    // `ricoh imaging` Pentax path and lose these two Type2 fields.
    if ricoh::RicohParser::is_type2_makernote(data) {
        ricoh::RicohParser::parse_type2(data, tags);
        return Ok(());
    }

    // MakerNotes.pm routes the OEM `CAMER\0` signature to Olympus::Main
    // before Make dispatch; several Pentax compact cameras use this layout.
    if data.starts_with(b"CAMER\0") {
        let parser = olympus::OlympusParser;
        parser.parse_with_context(ctx, byte_order, model, tags)?;
        return Ok(());
    }

    if matches!(make_normalized.as_str(), "nikon" | "nikon corporation") {
        nikon::NikonParser.parse_with_context_and_file_type(
            ctx,
            byte_order,
            model,
            file_type,
            tags,
            value_forms,
        )?;
        return Ok(());
    }

    // Vendors that spell their own name several ways across model generations
    // are matched by prefix rather than by an exhaustive literal list. Olympus
    // alone writes six different strings across the sample corpus -- "OLYMPUS
    // IMAGING CORP.", "OLYMPUS OPTICAL CO.,LTD", "OLYMPUS CORPORATION",
    // "OLYMPUS CORP.", "OLYMPUS_IMAGING_CORP." and "OM Digital Solutions" --
    // and only the first was recognised, so 102 of 315 Olympus JPEGs never
    // reached a parser at all.
    if let Some(parser) = parser_for_make_prefix(&make_normalized, data) {
        if parser.validate_header(data) {
            parser.parse_with_context(ctx, byte_order, model, tags)?;
        }
        return Ok(());
    }

    // Dispatch to appropriate parser based on manufacturer
    let parser: Option<Box<dyn MakerNoteParser>> = match make_normalized.as_str() {
        "canon" => Some(Box::new(canon::CanonParser)),
        "nikon" | "nikon corporation" => Some(Box::new(nikon::NikonParser)),
        "sony" => Some(Box::new(sony::SonyParser)),
        "panasonic" => Some(Box::new(panasonic::PanasonicParser)),
        "fujifilm" | "fuji photo film co., ltd." => Some(Box::new(fujifilm::FujifilmParser)),
        // The unnumbered `MakerNoteLeica` (bare `Make eq "LEICA"`, header
        // "LEICA\0\0\0", MakerNotes.pm:599-604) shares Panasonic's own
        // `Main` tag table and "Panasonic:" group -- it is not one of the
        // `Leica2`..`Leica10` layouts, which key on the "Leica Camera AG"
        // prefix instead (MakerNotes.pm:611 onward).
        "leica" => Some(Box::new(panasonic::PanasonicParser)),
        // `MakerNoteLeica10` (MakerNotes.pm:724-731) is keyed on the signature
        // alone -- `Condition => '$$valPt =~ /^LEICA CAMERA AG\0/'` -- and
        // routes to `Panasonic::Main`, not to any `Leica2`..`Leica9` table, so
        // it has to be separated from its Make-mates before they are. The
        // D-Lux 7/D-Lux 8/V-Lux 5 are Panasonic-built and ExifTool prints
        // their tags under "MakerNotes:Panasonic".
        "leica camera ag" if panasonic::is_leica10_makernote(data) => {
            Some(Box::new(panasonic::PanasonicParser))
        }
        "leica camera ag" => Some(Box::new(leica::LeicaMakerNoteParser)),
        // Sigma is absent on purpose. Its MakerNote entries store value offsets
        // relative to the enclosing TIFF header, so nothing handed only the
        // payload can read their values; `core::tiff_helpers::parse_exif_subifd`
        // routes Sigma to `makernotes::sigma` instead, which takes the TIFF.
        //
        // Phase One is also absent here on purpose: it's dispatched by
        // signature, above, before Make is ever consulted.
        "minolta" | "konica minolta" | "minolta co., ltd." => {
            Some(Box::new(minolta::MinoltaParser))
        }

        // Smartphones
        "apple" => Some(Box::new(apple::AppleParser)),
        // "google" is absent on purpose: there is no fabricated `google`
        // parser to dispatch to. ExifTool's real Google MakerNote table
        // (Google::HDRPlusMakerNote) is string-id-keyed and reads an
        // encrypted/gzipped protobuf blob, not a numeric TIFF IFD, so it
        // can't be reached through this Make-keyed dispatch at all.
        // "microsoft" | "microsoft corporation" is absent on purpose: there is
        // no fabricated `microsoft` parser to dispatch to. MakerNotes.pm has
        // no MakerNoteMicrosoft TIFF-IFD dispatch entry at all -- Microsoft's
        // only MakerNotes-group table (Microsoft::Stitch) is binary data read
        // from EXIF tag 0x4748, not a MakerNote IFD.
        // "qualcomm" is absent on purpose: there is no fabricated `qualcomm`
        // parser to dispatch to. ExifTool has no TIFF-IFD MakerNote table for
        // Qualcomm -- its two Qualcomm.pm tables are read from JPEG APP7/APP4
        // segments, not a Make="Qualcomm" MakerNote IFD.

        // Specialty devices
        "dji" => Some(Box::new(dji::DjiParser)),
        "flir" | "flir systems" => Some(Box::new(flir::FlirParser)),
        "gopro" => Some(Box::new(gopro::GoProParser)),
        "infiray" => Some(Box::new(infiray::InfiRayParser)),
        "nintendo" => Some(Box::new(nintendo::NintendoParser)),
        "parrot" => Some(Box::new(parrot::ParrotParser)),
        "reconyx" => Some(Box::new(reconyx::ReconxyParser)),
        "red" | "red.com" | "red digital cinema" => Some(Box::new(red::RedParser)),

        // Legacy cameras
        //
        // `Casio2.jpg`'s real `Make` is `"CASIO COMPUTER CO.,LTD "` (trailing
        // space, no period) -- `make_normalized` above trims it to
        // `"casio computer co.,ltd"`, which the former `"casio computer
        // co.,ltd."` arm (trailing period, no trailing space trimmed to
        // nothing) never matched. `MakerNotes.pm:75` only conditions on
        // `$$self{Make}=~/^CASIO/`, so every Casio Make string reaches this
        // parser in ExifTool; the exact-match arm here silently dropped every
        // Type2 ("QVC\0"/"DCI\0"-signed) MakerNote's tags.
        "casio" | "casio computer co.,ltd." | "casio computer co.,ltd" => {
            Some(Box::new(casio::CasioParser))
        }
        "ge" | "general electric" => Some(Box::new(ge::GeParser)),
        "hp" | "hewlett-packard" => Some(Box::new(hp::HpParser)),
        "jvc" | "victor company of japan, limited" => Some(Box::new(jvc::JvcParser)),
        "kodak" | "eastman kodak company" => Some(Box::new(kodak::KodakParser)),
        // Leaf is absent on purpose. ExifTool has no MakerNote parser for
        // Make=="Leaf" at all -- %Image::ExifTool::Leaf::Main is reached
        // exclusively via literal EXIF tag 0x8606 as a SubDirectory (a
        // .MOS-specific, string-keyed PKTS chunk structure unrelated to the
        // standard MakerNote tag 0x927C this dispatcher handles). A prior
        // numeric-IFD Leaf tag map here was invented and had no basis in
        // Leaf.pm; it misparsed real vendor MakerNote data on files that
        // merely carry the legacy "Leaf" Make string (e.g. Phase One IIQ
        // files from backs acquired from Leaf), producing spurious "Invalid
        // entry count" warnings.
        "motorola" => Some(Box::new(motorola::MotorolaParser)),
        "ricoh" | "ricoh company, ltd." => Some(Box::new(ricoh::RicohParser)),
        "sanyo" | "sanyo electric co.,ltd." => Some(Box::new(sanyo::SanyoParser)),

        // Software applications
        "capture one" => Some(Box::new(captureone::CaptureOneParser)),
        "fotostation" | "fotoware" => Some(Box::new(fotostation::FotoStationParser)),
        "gimp" => Some(Box::new(gimp::GimpParser)),
        "adobe indesign" | "indesign" => Some(Box::new(indesign::InDesignParser)),
        "nikon capture" | "capture nx" => Some(Box::new(nikoncapture::NikonCaptureParser)),
        "photoshop" | "adobe photoshop" => Some(Box::new(photoshop::PhotoshopParser)),
        "scalado" => Some(Box::new(scalado::ScaladoParser)),

        _ => None, // Unknown manufacturer
    };

    // If we have a parser, validate and parse
    if let Some(parser) = parser {
        // Validate header if parser provides validation. The signature lives at
        // the start of the declared block either way, so this reads `payload`
        // whether or not the decoder goes on to use the wider window.
        if parser.validate_header(data) {
            // Parse MakerNote data
            parser.parse_with_context_and_values(ctx, byte_order, model, tags, value_forms)?;
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
