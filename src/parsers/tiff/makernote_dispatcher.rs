//! MakerNote dispatcher
//!
//! Dispatches MakerNote data to the appropriate manufacturer parser
//! based on camera make.

#![allow(dead_code)]

use crate::core::TagOccurrence;
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

/// Whether this Make/payload pair dispatches to [`pentax::PentaxParser`].
///
/// Mirrors the dispatcher's own route order: the source-condition routes
/// (HP4, Kodak2, Minolta2, Phase One, Ricoh2) win before any Make prefix is
/// consulted. For Pentax the occurrence-aware entry differs from the legacy
/// map entry only by also returning canonical occurrences (the CAF point and
/// flash guide-number fields whose ValueConv and PrintConv differ), so
/// callers that record occurrences can opt in on this predicate without
/// changing any other vendor's route.
pub fn dispatches_to_pentax(make: &str, model: Option<&str>, data: &[u8]) -> bool {
    !source_condition_claims(make, model, data)
        && parser_for_make_prefix(&make.trim().to_lowercase(), data)
            .is_some_and(|parser| parser.manufacturer_name() == "Pentax")
}

/// Whether the source-condition chain in
/// `dispatch_makernote_with_context_and_values_and_session_impl` selects a
/// parser for this note before the Make fallback runs. Keep the two in step,
/// including which conditions see the trimmed Make and which the raw one
/// (ExifTool strips only trailing blanks from Make, Exif.pm:585).
fn source_condition_claims(make: &str, model: Option<&str>, data: &[u8]) -> bool {
    let source_make = make.trim();
    if claimed_before_hp4(source_make, data) {
        return false;
    }
    hp::is_type4(data)
        || (!claimed_between_hp4_and_kodak2(source_make, data)
            && (kodak::is_type2(data)
                || data.starts_with(b"MINOL\0")
                || data.starts_with(b"CAMER\0")
                || phaseone::is_phaseone_makernote(data)
                || ricoh::is_type2_selector(make, model, data)))
}

/// Conditions before HP4 in pinned MakerNotes.pm:38-205. An earlier match
/// owns the note even if its payload also happens to have the HP4/Kodak2
/// signature; leave its existing Make route (or omission) untouched.
fn claimed_before_hp4(make: &str, data: &[u8]) -> bool {
    data.starts_with(b"Apple iOS\0")
        || data.starts_with(b"Nikon\0\x02")
        || make.starts_with("Canon")
        || make.starts_with("CASIO")
        || data.starts_with(b"QVC\0")
        || data.starts_with(b"DCI\0")
        || data.starts_with(b"[ae_dbg_info:")
        || (make == "DJI" && data.get(3..8) != Some(&b"@AMBA"[..]) && !data.starts_with(b"DJI"))
        || make.starts_with("FLIR Systems")
        || make.starts_with("Teledyne FLIR")
        || data.starts_with(b"FUJIFILM")
        || data.starts_with(b"GENERALE")
        || data.starts_with(b"GE\0\0")
        || data.starts_with(b"GENIC\0")
        || data.starts_with(b"GE\x0c\0\0\0\x16\0\0\0")
        || data.starts_with(b"HDRP\x02")
        || data.starts_with(b"HDRP\x03")
        || make == "Hasselblad"
        || data.starts_with(b"Hewlett-Packard")
        || data.starts_with(b"Vivitar")
        || (data.starts_with(b"610") && data.get(3).is_some_and(|byte| *byte <= 4))
}

/// Conditions after HP4 but before Kodak2 in MakerNotes.pm:216-274. HP4
/// itself must win before these are considered.
fn claimed_between_hp4_and_kodak2(make: &str, data: &[u8]) -> bool {
    data.starts_with(b"IIII\x06\0")
        || data.starts_with(b"ISLMAKERNOTE000\0")
        || data.starts_with(b"JVC ")
        || ((make.starts_with("JVC") || make.starts_with("Victor")) && data.starts_with(b"VER:"))
        || (make.starts_with("EASTMAN KODAK") && data.starts_with(b"KDK"))
}

/// Match the vendors whose Make string varies too much for a literal list.
///
/// Returns `None` for everything else so the caller falls through to the
/// exact-match table.
fn parser_for_make_prefix(
    make: &str,
    data: &[u8],
) -> Option<Box<dyn crate::parsers::tiff::makernotes::shared::MakerNoteParser>> {
    use crate::parsers::tiff::makernotes::shared::MakerNoteParser;

    if make.starts_with("olympus")
        || make.starts_with("om digital solutions")
        || make.starts_with("om system")
    {
        return Some(Box::new(olympus::OlympusParser) as Box<dyn MakerNoteParser>);
    }
    if make.starts_with("pentax") || make.starts_with("asahi optical") {
        return Some(Box::new(pentax::PentaxParser::default()) as Box<dyn MakerNoteParser>);
    }
    // MakerNotePanasonic2 is gated by `$$self{Make} =~ /^Panasonic/`
    // (MakerNotes.pm:743-750), not by one exact vendor spelling. Keep this
    // before the literal table so Panasonic Corporation reaches the same
    // parser while the bare-Leica Type2 exclusion below remains distinct.
    if make.starts_with("panasonic") {
        return Some(Box::new(panasonic::PanasonicParser) as Box<dyn MakerNoteParser>);
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
    let mut session = crate::exiftool_tables::session::Session::new();
    let mut members = HashMap::new();
    let mut cond_ctx = crate::exiftool_tables::Ctx::new(&mut members);
    dispatch_makernote_with_context_and_values_and_session(
        make,
        model,
        ctx,
        byte_order,
        &mut session,
        &mut cond_ctx,
        tags,
        value_forms,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn dispatch_makernote_with_context_and_values_and_session(
    make: &str,
    model: Option<&str>,
    ctx: &MakerNoteContext<'_>,
    byte_order: ByteOrder,
    session: &mut crate::exiftool_tables::session::Session,
    cond_ctx: &mut crate::exiftool_tables::Ctx<'_>,
    tags: &mut HashMap<String, String>,
    value_forms: &mut HashMap<String, String>,
) -> Result<(), String> {
    dispatch_makernote_with_context_and_values_and_session_impl(
        make,
        model,
        ctx,
        byte_order,
        session,
        cond_ctx,
        tags,
        value_forms,
        None,
    )
}

/// Session-aware dispatcher that additionally accepts ordered canonical
/// occurrences from parsers which retain source identity and value forms.
#[allow(clippy::too_many_arguments)]
pub fn dispatch_makernote_with_context_and_values_and_session_and_occurrences(
    make: &str,
    model: Option<&str>,
    ctx: &MakerNoteContext<'_>,
    byte_order: ByteOrder,
    session: &mut crate::exiftool_tables::session::Session,
    cond_ctx: &mut crate::exiftool_tables::Ctx<'_>,
    tags: &mut HashMap<String, String>,
    value_forms: &mut HashMap<String, String>,
    occurrences: &mut Vec<(String, TagOccurrence)>,
) -> Result<(), String> {
    dispatch_makernote_with_context_and_values_and_session_impl(
        make,
        model,
        ctx,
        byte_order,
        session,
        cond_ctx,
        tags,
        value_forms,
        Some(occurrences),
    )
}

#[allow(clippy::too_many_arguments)]
fn dispatch_makernote_with_context_and_values_and_session_impl(
    make: &str,
    model: Option<&str>,
    ctx: &MakerNoteContext<'_>,
    byte_order: ByteOrder,
    session: &mut crate::exiftool_tables::session::Session,
    cond_ctx: &mut crate::exiftool_tables::Ctx<'_>,
    tags: &mut HashMap<String, String>,
    value_forms: &mut HashMap<String, String>,
    mut occurrences: Option<&mut Vec<(String, TagOccurrence)>>,
) -> Result<(), String> {
    use crate::parsers::tiff::makernotes::shared::MakerNoteParser;

    let data = ctx.payload();

    // Normalize make string (trim whitespace, case-insensitive matching)
    let make_normalized = make.trim().to_lowercase();

    // Preserve the first matching source condition before any Make fallback:
    // HP4 (:206), Kodak2 (:275), Minolta2 (:508), PhaseOne (:841), Ricoh2
    // (:924) in pinned MakerNotes.pm. Kodak2 permits arbitrary leading bytes,
    // so its payload can also match either later signature. The selected
    // decoder must own a real table before this route consumes the note.
    let source_make = make.trim();
    let source_parser: Option<Box<dyn MakerNoteParser>> = if claimed_before_hp4(source_make, data) {
        None
    } else if hp::is_type4(data) {
        Some(Box::new(hp::HpParser))
    } else if claimed_between_hp4_and_kodak2(source_make, data) {
        None
    } else if kodak::is_type2(data) {
        Some(Box::new(kodak::KodakParser))
    } else if data.starts_with(b"MINOL\0") || data.starts_with(b"CAMER\0") {
        // Minolta2's condition sets OlympusCAMER, selecting Olympus::Main
        // 0x2050's binary CameraParameters alternative. Seed both stores.
        cond_ctx
            .members
            .insert("OlympusCAMER", crate::exiftool_tables::MemberValue::Num(1));
        session
            .set_member(
                "OlympusCAMER",
                crate::exiftool_tables::session::MemberVal::Int(1),
            )
            .expect("OlympusCAMER is an untyped ExifTool member");
        Some(Box::new(olympus::OlympusParser))
    } else if phaseone::is_phaseone_makernote(data) {
        // PhaseOne is signature-only and can appear under Leaf Make.
        Some(Box::new(phaseone::PhaseOneMakerNoteParser))
    } else if ricoh::is_type2_selector(make, model, data) {
        Some(Box::new(ricoh::RicohType2Parser))
    } else {
        None
    };
    if let Some(parser) = source_parser {
        if let Some(rows) = occurrences.as_deref_mut() {
            parser.parse_with_context_and_values_and_session_and_occurrences(
                ctx,
                byte_order,
                model,
                session,
                cond_ctx,
                tags,
                value_forms,
                rows,
            )?;
        } else {
            parser.parse_with_context_and_values_and_session(
                ctx,
                byte_order,
                model,
                session,
                cond_ctx,
                tags,
                value_forms,
            )?;
        }
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
            // `..._and_values`, not `parse_with_context`: the prefix-dispatched
            // makes reached the value-less entry point, so any ValueConv form
            // they produced was dropped on the floor before
            // `core::tiff_helpers` could call `set_value_form` with it --
            // which is why Olympus `FocusDistance` could not feed
            // `Composite:DOF`. The trait's default implementation ignores
            // `value_forms` and calls `parse_with_context`, so Pentax, Ricoh,
            // GE and Samsung are unaffected.
            if let Some(rows) = occurrences.as_deref_mut() {
                parser.parse_with_context_and_values_and_session_and_occurrences(
                    ctx,
                    byte_order,
                    model,
                    session,
                    cond_ctx,
                    tags,
                    value_forms,
                    rows,
                )?;
            } else {
                parser.parse_with_context_and_values_and_session(
                    ctx,
                    byte_order,
                    model,
                    session,
                    cond_ctx,
                    tags,
                    value_forms,
                )?;
            }
        }
        return Ok(());
    }

    // Dispatch to appropriate parser based on manufacturer
    let parser: Option<Box<dyn MakerNoteParser>> = match make_normalized.as_str() {
        "canon" => Some(Box::new(canon::CanonParser)),
        "nikon" | "nikon corporation" => Some(Box::new(nikon::NikonParser)),
        "sony" => Some(Box::new(sony::SonyParser)),
        "fujifilm" | "fuji photo film co., ltd." => Some(Box::new(fujifilm::FujifilmParser)),
        // The unnumbered `MakerNoteLeica` (bare `Make eq "LEICA"`, header
        // "LEICA\0\0\0", MakerNotes.pm:599-604) shares Panasonic's own
        // `Main` tag table and "Panasonic:" group -- it is not one of the
        // `Leica2`..`Leica10` layouts, which key on the "Leica Camera AG"
        // prefix instead (MakerNotes.pm:611 onward).
        // `MakerNoteLeica` and `MakerNotePanasonic2` both select the shared
        // Panasonic parser only after different source conditions. In
        // particular, Type2 additionally requires `Make =~ /^Panasonic/`
        // (MakerNotes.pm:743-750), so a bare Leica MKE payload is not a
        // Panasonic Type2 record.
        "leica" if panasonic::is_panasonic_type2_makernote(data) => None,
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
            if let Some(rows) = occurrences.as_deref_mut() {
                parser.parse_with_context_and_values_and_session_and_occurrences(
                    ctx,
                    byte_order,
                    model,
                    session,
                    cond_ctx,
                    tags,
                    value_forms,
                    rows,
                )?;
            } else {
                parser.parse_with_context_and_values_and_session(
                    ctx,
                    byte_order,
                    model,
                    session,
                    cond_ctx,
                    tags,
                    value_forms,
                )?;
            }
        }
    }

    // Silently succeed - not all makes have MakerNotes or valid headers
    Ok(())
}

/// Staleness/consistency test (tag-machinery overhaul Step 16, R5 stage 1):
/// diffs ExifTool's real `@MakerNotes::Main` dispatch table against this
/// file's hand-written `match`.
///
/// `@MakerNotes::Main` (MakerNotes.pm:35-...) is the 94-row list ExifTool
/// itself dispatches MakerNote parsing on, tried in order until a
/// `Condition` matches. Until Step 16, `dump_tables.pl` only ever walked the
/// stash's HASH globs, so this array -- the single ARRAY-shaped tag table in
/// the entire pinned ExifTool tree -- was invisible to every instrument in
/// this repo. The #636 regen found 401 stale hand-embedded facts and nothing
/// could report them; this is one category of that class made checkable.
///
/// This module does NOT try to reproduce every `Condition` regex (many are
/// multi-line Perl on `$$valPt`/`$$self{Make}`/`$$self{Model}` together, not
/// mechanically translatable without Step 15's expression compiler). What it
/// checks instead, at VENDOR granularity: every vendor prefix that
/// `SubDirectory.TagTable` names in the current dump is accounted for by name
/// in exactly one of three hand-maintained buckets below (mapped by this
/// dispatcher's `match`, mapped elsewhere in the codebase, or explicitly
/// out of scope) -- and, in the other direction, every vendor the buckets
/// name still has a real route in the current dump. A vendor ExifTool adds
/// that fits none of the buckets, or a bucket entry ExifTool has since
/// removed, is exactly the drift this step exists to make visible instead of
/// silent.
///
/// Fixture: `tools/exiftool-tables/fixtures/makernote_routes.json`, produced
/// by `gen_staleness_facts.py` from `dump_tables.pl`'s output (pinned
/// ExifTool 13.59). Regenerate on a bump; see that script's header.
#[cfg(test)]
mod staleness_tests {
    use std::collections::BTreeSet;

    /// Compiled in at build time -- hermetic, no `/tmp` read, no live
    /// ExifTool needed to run `cargo test`.
    const ROUTES_FIXTURE: &str =
        include_str!("../../../tools/exiftool-tables/fixtures/makernote_routes.json");

    /// Vendors this dispatcher reaches through its `match` on the normalized
    /// Make string, or through [`parser_for_make_prefix`]'s prefix matching.
    /// Keep this in sync with the match arms above BY HAND: that hand-sync
    /// obligation is exactly what the two tests below check, not assume.
    const MAPPED_BY_MAKE: &[&str] = &[
        "Apple",
        "Canon",
        "Casio",
        "DJI",
        "FLIR",
        "FujiFilm",
        "GE",
        "HP",
        "JVC",
        "Kodak",
        "Minolta",
        "Motorola",
        "Nikon",
        "Nintendo",
        "Olympus",
        "Panasonic",
        "Pentax",
        "Reconyx",
        "Ricoh",
        "Samsung",
        "Sanyo",
        "Sony",
    ];

    /// Vendors dispatched before the Make-keyed `match` even runs, by
    /// MakerNote signature alone -- see the `phaseone::is_phaseone_makernote`
    /// check at the top of `dispatch_makernote_with_context_and_values`.
    const MAPPED_BY_SIGNATURE: &[&str] = &["PhaseOne"];

    /// Vendors `@MakerNotes::Main` names as a `SubDirectory` target that this
    /// dispatcher does NOT reach -- a different code path in this crate reads
    /// them instead. (vendor, reason).
    const HANDLED_ELSEWHERE: &[(&str, &str)] = &[(
        "Sigma",
        "Sigma MakerNote value offsets are relative to the enclosing TIFF \
         header, unreadable from the payload alone -- \
         core::tiff_helpers::parse_exif_subifd routes Sigma to \
         makernotes::sigma directly, bypassing this dispatcher entirely.",
    )];

    /// Vendors `@MakerNotes::Main` names that this dispatcher deliberately
    /// does not implement, with the reason (mirrors the prose comments
    /// above, e.g. `"google" is absent on purpose`). (vendor, reason).
    const INTENTIONALLY_UNMAPPED: &[(&str, &str)] = &[
        (
            "Google",
            "Google::HDRPlusMakerNote is string-id-keyed and reads an \
             encrypted/gzipped protobuf blob, not a numeric TIFF IFD -- it \
             cannot be reached through this Make-keyed dispatch at all.",
        ),
        (
            "Unknown",
            "ExifTool's own generic fallback table for makes with no \
             vendor-specific structure (Hasselblad, ISL, Kyocera signatures \
             in the current dump) -- it carries no vendor-specific tags to \
             lose by not implementing it.",
        ),
    ];

    #[derive(serde::Deserialize)]
    struct RoutesFixture {
        row_count: usize,
        rows: Vec<Row>,
    }

    #[derive(serde::Deserialize)]
    struct Row {
        name: String,
        target: Option<String>,
        #[serde(rename = "condition")]
        #[allow(dead_code)]
        _condition: String,
    }

    fn fixture() -> RoutesFixture {
        serde_json::from_str(ROUTES_FIXTURE).expect("makernote_routes.json fixture is valid JSON")
    }

    fn all_known_vendors() -> BTreeSet<&'static str> {
        MAPPED_BY_MAKE
            .iter()
            .copied()
            .chain(MAPPED_BY_SIGNATURE.iter().copied())
            .chain(HANDLED_ELSEWHERE.iter().map(|(v, _)| *v))
            .chain(INTENTIONALLY_UNMAPPED.iter().map(|(v, _)| *v))
            .collect()
    }

    #[test]
    fn fixture_row_count_matches_exiftool_main_table() {
        let f = fixture();
        assert_eq!(
            f.rows.len(),
            f.row_count,
            "fixture's own row_count disagrees with its rows array length -- corrupt fixture"
        );
        // Not a magic number: @MakerNotes::Main had exactly 94 rows when this
        // fixture was generated (pinned ExifTool 13.59). A large drop would
        // mean dump_tables.pl's ARRAY capture regressed, not that ExifTool
        // shrank its own dispatch table.
        assert!(
            f.row_count >= 90,
            "MakerNotes::Main row_count {} looks too small for ExifTool 13.59 \
             (expected 94) -- did dump_tables.pl's ARRAY capture regress?",
            f.row_count
        );
    }

    /// Forward direction: every vendor ExifTool's `MakerNotes::Main` routes
    /// to is accounted for by name. A vendor present in the fixture but
    /// absent from every bucket is a route nothing in this file has been
    /// told about.
    #[test]
    fn every_dumped_vendor_is_accounted_for() {
        let f = fixture();
        let known = all_known_vendors();

        let mut missing = Vec::new();
        for row in &f.rows {
            let Some(target) = &row.target else {
                continue;
            };
            let vendor = target.split("::").next().unwrap_or(target);
            if !known.contains(vendor) {
                missing.push(format!("{} (row {}, target {})", vendor, row.name, target));
            }
        }
        assert!(
            missing.is_empty(),
            "MakerNotes::Main routes to a vendor this staleness mirror does \
             not know about -- after confirming whether the dispatcher above \
             actually handles it, add the vendor to MAPPED_BY_MAKE, \
             HANDLED_ELSEWHERE or INTENTIONALLY_UNMAPPED in this test module:\n  {}",
            missing.join("\n  ")
        );
    }

    /// Reverse direction: every vendor a bucket claims is handled must still
    /// have a real route in the current dump. A name here ExifTool no longer
    /// routes to is a claim about a route that does not exist any more.
    #[test]
    fn every_known_vendor_still_appears_in_exiftool() {
        let f = fixture();
        let seen: BTreeSet<&str> = f
            .rows
            .iter()
            .filter_map(|r| r.target.as_deref())
            .map(|t| t.split("::").next().unwrap_or(t))
            .collect();

        let stale: Vec<&str> = all_known_vendors()
            .into_iter()
            .filter(|vendor| !seen.contains(vendor))
            .collect();
        assert!(
            stale.is_empty(),
            "this test's mirror claims ExifTool routes MakerNotes to {:?}, but \
             the current dump has no such route for at least one -- either \
             ExifTool removed it, or the vendor name in the mirror is wrong",
            stale
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn panasonic_quality_note() -> Vec<u8> {
        let mut data = b"Panasonic\0\0\0".to_vec();
        data.extend_from_slice(&1u16.to_le_bytes());
        data.extend_from_slice(&0x0001u16.to_le_bytes());
        data.extend_from_slice(&3u16.to_le_bytes());
        data.extend_from_slice(&1u32.to_le_bytes());
        data.extend_from_slice(&2u16.to_le_bytes());
        data.extend_from_slice(&[0, 0]);
        data.extend_from_slice(&0u32.to_le_bytes());
        data
    }

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

    /// `MakerNotePanasonic2` requires both `Make =~ /^Panasonic/` and an
    /// `MKE` payload (MakerNotes.pm:743-750). A bare Leica Make normally
    /// shares Panasonic::Main, but must not gain Panasonic::Type2 merely
    /// because arbitrary MakerNote bytes begin with `MKE`.
    #[test]
    fn leica_mke_payload_does_not_dispatch_panasonic_type2() {
        let mut tags = HashMap::new();

        dispatch_makernote("LEICA", b"MKEM\0\0\x88\0", ByteOrder::BigEndian, &mut tags)
            .expect("unmatched Leica MakerNote is ignored");

        assert!(
            tags.is_empty(),
            "MakerNotes.pm requires a Panasonic Make before Type2 can emit tags"
        );
    }

    /// The raw-file occurrence opt-in follows the dispatcher's own Pentax
    /// routes (Make prefixes, Samsung GX "AOC\0") and nothing else.
    #[test]
    fn dispatches_to_pentax_matches_the_pentax_routes_only() {
        assert!(dispatches_to_pentax("PENTAX", None, b"AOC\0MM"));
        assert!(dispatches_to_pentax(
            "  Asahi Optical Co.,Ltd ",
            None,
            b"AOC\0II"
        ));
        assert!(dispatches_to_pentax(
            "RICOH IMAGING COMPANY, LTD.",
            None,
            b"PENTAX \0"
        ));
        assert!(dispatches_to_pentax("SAMSUNG TECHWIN", None, b"AOC\0MM"));
        assert!(!dispatches_to_pentax("SAMSUNG TECHWIN", None, b"\x01\0"));
        assert!(!dispatches_to_pentax("Panasonic", None, b"Panasonic\0\0\0"));
        assert!(!dispatches_to_pentax("PENTAX", None, b"MINOL\0"));
    }

    /// Source-condition routes (MakerNotes.pm HP4 :206, Kodak2 :275, Ricoh2
    /// :924) claim these notes before the Pentax Make fallback, so the raw
    /// path must not treat them as Pentax.
    #[test]
    fn dispatches_to_pentax_defers_to_earlier_source_conditions() {
        let mut kodak2 = b"\x01\0\0\0\0\0\x04\0ABCD".to_vec();
        kodak2.resize(64, 0);
        assert!(!dispatches_to_pentax("PENTAX Corporation", None, &kodak2));
        let hp4 = b"IIII\x04\0rest";
        assert!(!dispatches_to_pentax("PENTAX", None, hp4));
        let ricoh2 = b"II*\0\x08\0\0\0\x02\0\0\0";
        assert!(!dispatches_to_pentax(
            "RICOH IMAGING COMPANY, LTD.",
            Some("PENTAX XG-1"),
            ricoh2
        ));
        assert!(!dispatches_to_pentax(
            "RICOH IMAGING COMPANY, LTD.",
            Some("RICOH WG-M1"),
            b"PENTAX \0"
        ));
        // Ricoh2's Make test sees the raw Make, exactly as the dispatcher's
        // own chain does, so a leading-blank Make is not claimed by Ricoh2.
        let leading_blank = "  RICOH IMAGING COMPANY, LTD.";
        assert_eq!(
            source_condition_claims(leading_blank, Some("PENTAX XG-1"), ricoh2),
            ricoh::is_type2_selector(leading_blank, Some("PENTAX XG-1"), ricoh2)
        );
        assert!(!source_condition_claims(
            leading_blank,
            Some("PENTAX XG-1"),
            ricoh2
        ));
    }

    /// `MakerNotePanasonic2` accepts every Make beginning with Panasonic, not
    /// only the exact vendor spelling (MakerNotes.pm:743-750).
    #[test]
    fn panasonic_prefixed_make_dispatches_type2() {
        let mut tags = HashMap::new();

        dispatch_makernote(
            "Panasonic Corporation",
            b"MKEM\0\0\x88\0",
            ByteOrder::BigEndian,
            &mut tags,
        )
        .expect("Panasonic-prefixed Type2 MakerNote dispatches");

        assert_eq!(
            tags.get("Panasonic:MakerNoteType").map(String::as_str),
            Some("MKEM")
        );
        assert_eq!(tags.get("Panasonic:Gain").map(String::as_str), Some("136"));
    }

    #[test]
    fn real_untouched_vendor_uses_the_structured_default_without_behavior_change() {
        let data = panasonic_quality_note();
        let mut legacy = HashMap::new();
        dispatch_makernote("Panasonic", &data, ByteOrder::LittleEndian, &mut legacy)
            .expect("real Panasonic dispatcher path");
        assert_eq!(
            legacy.get("Panasonic:ImageQuality").map(String::as_str),
            Some("High")
        );

        let mut session = crate::exiftool_tables::session::Session::new();
        let mut members = HashMap::new();
        let mut cond_ctx = crate::exiftool_tables::Ctx::new(&mut members);
        let mut structured = HashMap::new();
        let mut values = HashMap::new();
        let mut occurrences = Vec::new();
        dispatch_makernote_with_context_and_values_and_session_and_occurrences(
            "Panasonic",
            None,
            &MakerNoteContext::detached(&data),
            ByteOrder::LittleEndian,
            &mut session,
            &mut cond_ctx,
            &mut structured,
            &mut values,
            &mut occurrences,
        )
        .expect("structured dispatcher reaches the real default implementation");

        assert_eq!(structured, legacy);
        assert!(values.is_empty());
        assert!(occurrences.is_empty());
    }
}
