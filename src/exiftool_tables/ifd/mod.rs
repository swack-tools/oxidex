//! ExifTool IFD-style tag tables -- the `Exif::ProcessExif` tables --
//! generated from ExifTool 13.59's own Perl hashes.
//!
//! DO NOT EDIT. Regenerate with:
//!
//! ```sh
//! perl tools/exiftool-tables/dump_tables.pl <exiftool>/lib > tables.json
//! python3 tools/exiftool-tables/codegen.py tables.json \
//!     -o src/exiftool_tables/binary/mod.rs --ifd-out src/exiftool_tables/ifd/mod.rs
//! ```
//!
//! This file is the hub of a one-file-per-ExifTool-module layout: every
//! `IfdTable` static lives in `<module>.rs` beside it (`exif.rs`, `canon.rs`,
//! ...) and the `pub use <module>::*;` block below re-exports each one, so
//! `ifd_tables::IFD_EXIF_MAIN` resolves exactly as it did when this was one
//! file. `tools/exiftool-tables/table_modules.py` owns the layout;
//! `src/exiftool_tables/mod.rs` mounts this hub as `ifd_tables`.
//!
//! Selection is `codegen.py::is_ifd_table`: every table whose `PROCESS_PROC`
//! is absent (ExifTool.pm:9052 defaults it to `Exif::ProcessExif`) or names
//! `Exif::ProcessExif`. That rule over-includes tables ExifTool never walks
//! as an IFD -- `Composite`, XMP, and record tables such as IPTC, PICT and
//! Matroska whose keys are not integer tag ids or whose formats are not EXIF
//! scalars. Those are still emitted, with every entry refused and counted
//! (`ifd_tag_id_unrepresentable`, `ifd_format_unsupported`) and Gate A
//! blocked, so the census says which tables are real IFDs and why the rest
//! are not, instead of dropping them silently.
//!
//! Every conversion here is the same oracle-verified `ExprId`/enum the
//! `ProcessBinaryData` tables carry (`super::binary_tables`), compiled by the
//! same code path; a conversion the generator could not reproduce is refused
//! and counted, never approximated. `codegen.py`'s REPORT ("IFD tables"
//! section) is the accounting, and `src/exiftool_tables/ifd_schema.rs`
//! documents every field. Two facts a walk must not read into this data:
//! `IfdSubdirEdge::fix_format` is write-side only (WriteExif.pl:1760), and
//! `IfdTable::set_group1` is ExifTool's `SET_GROUP1` payload verbatim -- a
//! flag meaning "group 1 = the directory name the walk was given"
//! (Exif.pm:7183), never a group name itself.

#![allow(clippy::unreadable_literal, clippy::too_many_lines, unused_parens)]

/// The ExifTool release these tables were transcribed from. Must equal
/// `super::EXIFTOOL_VERSION` (`binary/mod.rs`'s stamp): the two artifacts
/// are one regeneration, and a skew between them is the mixed-release hazard
/// `tools/exiftool-tables/regen-all.sh` exists to prevent.
pub const IFD_EXIFTOOL_VERSION: &str = "13.59";

// Imported unconditionally, for binary/mod.rs's reason: which of these a
// given run constructs depends on the pinned tree, not on this file's logic,
// so a conditional `use` would be generator-output nondeterminism.
#[allow(unused_imports)]
use super::cond::{CmpOp, Cond, EffectSource};
#[allow(unused_imports)]
use super::ifd_schema::{
    IfdByteOrder, IfdFlags, IfdStart, IfdSubdirEdge, IfdSubdirProcessor, IfdTable, IfdTag,
    IfdVariantGroup, RawConvEffect,
};
#[allow(unused_imports)]
use super::subdir::BaseExpr;
#[allow(unused_imports)]
use super::validation::{SizeExpectation, U16SizeCheck};
#[allow(unused_imports)]
use super::{ExprId, Fmt, GateA, Omitted, OtherId, PrintConv, TagGroups};

// One file per ExifTool module, declared in rustfmt's order and re-exported
// so every table static keeps the path it had when this was one file
// (tools/exiftool-tables/table_modules.py owns the layout).
mod aiff;
mod ape;
mod apple;
mod audible;
mod bmp;
mod bpg;
mod canon;
mod casio;
mod darwincore;
mod dicom;
mod dji;
mod djvu;
mod dv;
mod exe;
mod exif;
mod fits;
mod flac;
mod flash;
mod flashpix;
mod flif;
mod flir;
mod font;
mod fujifilm;
mod garmin;
mod ge;
mod geotiff;
mod gif;
mod gimp;
mod google;
mod gps;
mod h264;
mod hp;
mod html;
mod id3;
mod iptc;
mod iso;
mod itc;
mod jpeg;
mod jvc;
mod kodak;
mod leaf;
mod lnk;
mod lytro;
mod m2ts;
mod macos;
mod matroska;
mod microsoft;
mod miff;
mod minolta;
mod misb;
mod mng;
mod motorola;
mod mpeg;
mod mpf;
mod mwg;
mod mxf;
mod nikon;
mod nikoncustom;
mod nintendo;
mod ogg;
mod olympus;
mod openexr;
mod opus;
mod other;
mod panasonic;
mod panasonicraw;
mod parrot;
mod pcap;
mod pdf;
mod pentax;
mod photomechanic;
mod photoshop;
mod pict;
mod plus;
mod png;
mod postscript;
mod psp;
mod quicktime;
mod radiance;
mod rawzor;
mod real;
mod red;
mod ricoh;
mod riff;
mod rtf;
mod samsung;
mod sanyo;
mod shortcuts;
mod sigma;
mod sigmaraw;
mod sony;
mod sonyidc;
mod stim;
mod taginfoxml;
mod text;
mod theora;
mod tnef;
mod torrent;
mod trailer;
mod unknown;
mod vcard;
mod vorbis;
mod wpg;
mod wtv;
mod xisf;
mod xmp;
mod zip;

pub use aiff::*;
pub use ape::*;
pub use apple::*;
pub use audible::*;
pub use bmp::*;
pub use bpg::*;
pub use canon::*;
pub use casio::*;
pub use darwincore::*;
pub use dicom::*;
pub use dji::*;
pub use djvu::*;
pub use dv::*;
pub use exe::*;
pub use exif::*;
pub use fits::*;
pub use flac::*;
pub use flash::*;
pub use flashpix::*;
pub use flif::*;
pub use flir::*;
pub use font::*;
pub use fujifilm::*;
pub use garmin::*;
pub use ge::*;
pub use geotiff::*;
pub use gif::*;
pub use gimp::*;
pub use google::*;
pub use gps::*;
pub use h264::*;
pub use hp::*;
pub use html::*;
pub use id3::*;
pub use iptc::*;
pub use iso::*;
pub use itc::*;
pub use jpeg::*;
pub use jvc::*;
pub use kodak::*;
pub use leaf::*;
pub use lnk::*;
pub use lytro::*;
pub use m2ts::*;
pub use macos::*;
pub use matroska::*;
pub use microsoft::*;
pub use miff::*;
pub use minolta::*;
pub use misb::*;
pub use mng::*;
pub use motorola::*;
pub use mpeg::*;
pub use mpf::*;
pub use mwg::*;
pub use mxf::*;
pub use nikon::*;
pub use nikoncustom::*;
pub use nintendo::*;
pub use ogg::*;
pub use olympus::*;
pub use openexr::*;
pub use opus::*;
pub use other::*;
pub use panasonic::*;
pub use panasonicraw::*;
pub use parrot::*;
pub use pcap::*;
pub use pdf::*;
pub use pentax::*;
pub use photomechanic::*;
pub use photoshop::*;
pub use pict::*;
pub use plus::*;
pub use png::*;
pub use postscript::*;
pub use psp::*;
pub use quicktime::*;
pub use radiance::*;
pub use rawzor::*;
pub use real::*;
pub use red::*;
pub use ricoh::*;
pub use riff::*;
pub use rtf::*;
pub use samsung::*;
pub use sanyo::*;
pub use shortcuts::*;
pub use sigma::*;
pub use sigmaraw::*;
pub use sony::*;
pub use sonyidc::*;
pub use stim::*;
pub use taginfoxml::*;
pub use text::*;
pub use theora::*;
pub use tnef::*;
pub use torrent::*;
pub use trailer::*;
pub use unknown::*;
pub use vcard::*;
pub use vorbis::*;
pub use wpg::*;
pub use wtv::*;
pub use xisf::*;
pub use xmp::*;
pub use zip::*;

/// Every generated IFD-style table, sorted by `(module, table)` for
/// `find_ifd_table`'s binary search.
pub static ALL_IFD_TABLES: &[&IfdTable] = &[
    &IFD_AIFF_COMPOSITE,
    &IFD_AIFF_MAIN,
    &IFD_APE_COMPOSITE,
    &IFD_APE_MAIN,
    &IFD_APPLE_COMPOSITE,
    &IFD_APPLE_MAIN,
    &IFD_AUDIBLE_MAIN,
    &IFD_AUDIBLE_TAGS,
    &IFD_AUDIBLE_TSEG,
    &IFD_BMP_EXTRA,
    &IFD_BPG_EXTENSIONS,
    &IFD_CANON_CCTP,
    &IFD_CANON_CDI1,
    &IFD_CANON_CNOP,
    &IFD_CANON_CNTH,
    &IFD_CANON_COMPOSITE,
    &IFD_CANON_MAIN,
    &IFD_CANON_SKIP,
    &IFD_CANON_UNKNOWNIFD,
    &IFD_CANON_UUID,
    &IFD_CANON_UUID2,
    &IFD_CASIO_MAIN,
    &IFD_CASIO_TYPE2,
    &IFD_DICOM_MAIN,
    &IFD_DJI_MAIN,
    &IFD_DJI_XMP,
    &IFD_DV_MAIN,
    &IFD_DARWINCORE_MAIN,
    &IFD_DJVU_MAIN,
    &IFD_EXE_MACHO,
    &IFD_EXE_MISC,
    &IFD_EXE_PESTRING,
    &IFD_EXIF_COMPOSITE,
    &IFD_EXIF_MAIN,
    &IFD_EXIF_UNKNOWN,
    &IFD_EXIF_FLASH,
    &IFD_EXIF_PRINTPARAMETER,
    &IFD_EXIF_SUBFILETYPE,
    &IFD_FITS_MAIN,
    &IFD_FLAC_COMPOSITE,
    &IFD_FLAC_MAIN,
    &IFD_FLIF_MAIN,
    &IFD_FLIR_AFF,
    &IFD_FLIR_COMPOSITE,
    &IFD_FLIR_MAIN,
    &IFD_FLIR_USERDATA,
    &IFD_FLASH_FLV,
    &IFD_FLASH_MAIN,
    &IFD_FLASHPIX_COMPOSITE,
    &IFD_FLASHPIX_DOCTABLE,
    &IFD_FONT_AFM,
    &IFD_FONT_MAIN,
    &IFD_FONT_NAME,
    &IFD_FONT_PSINFO,
    &IFD_FONT_TTLANG,
    &IFD_FUJIFILM_IFD,
    &IFD_FUJIFILM_MAIN,
    &IFD_GE_MAIN,
    &IFD_GIF_EXTENSIONS,
    &IFD_GIF_MAIN,
    &IFD_GIMP_MAIN,
    &IFD_GPS_COMPOSITE,
    &IFD_GPS_MAIN,
    &IFD_GARMIN_AADACCELFEATURES,
    &IFD_GARMIN_ACCELEROMETERDATA,
    &IFD_GARMIN_ACTIVITY,
    &IFD_GARMIN_ACTIVITYMETRICS,
    &IFD_GARMIN_ALARMSETTINGS,
    &IFD_GARMIN_ALERT,
    &IFD_GARMIN_ANTCHANNELID,
    &IFD_GARMIN_ANTRX,
    &IFD_GARMIN_ANTTX,
    &IFD_GARMIN_AVIATIONATTITUDE,
    &IFD_GARMIN_BAROMETERDATA,
    &IFD_GARMIN_BEATINTERVALS,
    &IFD_GARMIN_BESTEFFORT,
    &IFD_GARMIN_BIKEPROFILE,
    &IFD_GARMIN_BLOODPRESSURE,
    &IFD_GARMIN_CPESTATUS,
    &IFD_GARMIN_CADENCEZONE,
    &IFD_GARMIN_CAMERAEVENT,
    &IFD_GARMIN_CAPABILITIES,
    &IFD_GARMIN_CHRONOSHOTDATA,
    &IFD_GARMIN_CHRONOSHOTSESSION,
    &IFD_GARMIN_CLIMBPRO,
    &IFD_GARMIN_CLUBS,
    &IFD_GARMIN_COMMON,
    &IFD_GARMIN_CONNECTIQFIELD,
    &IFD_GARMIN_CONNECTIVITY,
    &IFD_GARMIN_COURSE,
    &IFD_GARMIN_COURSEPOINT,
    &IFD_GARMIN_DATASCREEN,
    &IFD_GARMIN_DEV,
    &IFD_GARMIN_DEVELOPERDATAID,
    &IFD_GARMIN_DEVICEAUXBATTERYINFO,
    &IFD_GARMIN_DEVICEINFO,
    &IFD_GARMIN_DEVICESETTINGS,
    &IFD_GARMIN_DEVICESTATUS,
    &IFD_GARMIN_DEVICEUSED,
    &IFD_GARMIN_DIVEALARM,
    &IFD_GARMIN_DIVEAPNEAALARM,
    &IFD_GARMIN_DIVEGAS,
    &IFD_GARMIN_DIVESETTINGS,
    &IFD_GARMIN_DIVESUMMARY,
    &IFD_GARMIN_ECGRAWSAMPLE,
    &IFD_GARMIN_ECGSMOOTHSAMPLE,
    &IFD_GARMIN_ECGSUMMARY,
    &IFD_GARMIN_EPOSTATUS,
    &IFD_GARMIN_ENDURANCESCORE,
    &IFD_GARMIN_EVENT,
    &IFD_GARMIN_EXDDATACONCEPTCONFIGURATION,
    &IFD_GARMIN_EXDDATAFIELDCONFIGURATION,
    &IFD_GARMIN_EXDSCREENCONFIGURATION,
    &IFD_GARMIN_EXERCISETITLE,
    &IFD_GARMIN_FIT,
    &IFD_GARMIN_FIELDCAPABILITIES,
    &IFD_GARMIN_FIELDDESCRIPTION,
    &IFD_GARMIN_FILECAPABILITIES,
    &IFD_GARMIN_FILECREATOR,
    &IFD_GARMIN_FILEID,
    &IFD_GARMIN_FUNCTIONALMETRICS,
    &IFD_GARMIN_GPS,
    &IFD_GARMIN_GPSEVENT,
    &IFD_GARMIN_GOAL,
    &IFD_GARMIN_GOLFCOURSE,
    &IFD_GARMIN_GOLFSTATS,
    &IFD_GARMIN_GYROSCOPEDATA,
    &IFD_GARMIN_HR,
    &IFD_GARMIN_HRMPROFILE,
    &IFD_GARMIN_HRV,
    &IFD_GARMIN_HRVSTATUSSUMMARY,
    &IFD_GARMIN_HRVVALUE,
    &IFD_GARMIN_HRZONE,
    &IFD_GARMIN_HSAACCELEROMETERDATA,
    &IFD_GARMIN_HSABODYBATTERYDATA,
    &IFD_GARMIN_HSACONFIGURATIONDATA,
    &IFD_GARMIN_HSAEVENT,
    &IFD_GARMIN_HSAGYROSCOPEDATA,
    &IFD_GARMIN_HSAHEARTRATEDATA,
    &IFD_GARMIN_HSARESPIRATIONDATA,
    &IFD_GARMIN_HSASTEPDATA,
    &IFD_GARMIN_HSASTRESSDATA,
    &IFD_GARMIN_HSAWRISTTEMPERATUREDATA,
    &IFD_GARMIN_HSA_SPO2DATA,
    &IFD_GARMIN_HILLSCORE,
    &IFD_GARMIN_HOLE,
    &IFD_GARMIN_JUMP,
    &IFD_GARMIN_LAP,
    &IFD_GARMIN_LENGTH,
    &IFD_GARMIN_LOCATION,
    &IFD_GARMIN_MAGNETOMETERDATA,
    &IFD_GARMIN_MAPLAYER,
    &IFD_GARMIN_MAXMETDATA,
    &IFD_GARMIN_MEMOGLOB,
    &IFD_GARMIN_MESGCAPABILITIES,
    &IFD_GARMIN_METZONE,
    &IFD_GARMIN_METRONOME,
    &IFD_GARMIN_MONITORING,
    &IFD_GARMIN_MONITORINGHRDATA,
    &IFD_GARMIN_MONITORINGINFO,
    &IFD_GARMIN_MTBCX,
    &IFD_GARMIN_MULTISPORTACTIVITY,
    &IFD_GARMIN_MULTISPORTSETTINGS,
    &IFD_GARMIN_MUSICINFO,
    &IFD_GARMIN_NMEASENTENCE,
    &IFD_GARMIN_NAPEVENT,
    &IFD_GARMIN_OBDIIDATA,
    &IFD_GARMIN_OHRSETTINGS,
    &IFD_GARMIN_ONEDSENSORCALIBRATION,
    &IFD_GARMIN_OPENWATEREVENT,
    &IFD_GARMIN_PERSONALRECORD,
    &IFD_GARMIN_POWERMODE,
    &IFD_GARMIN_POWERZONE,
    &IFD_GARMIN_RACE,
    &IFD_GARMIN_RACEEVENT,
    &IFD_GARMIN_RANGEALERT,
    &IFD_GARMIN_RAWBBI,
    &IFD_GARMIN_RECORD,
    &IFD_GARMIN_RESPIRATIONRATE,
    &IFD_GARMIN_ROUTING,
    &IFD_GARMIN_SDMPROFILE,
    &IFD_GARMIN_SPO2DATA,
    &IFD_GARMIN_SCHEDULE,
    &IFD_GARMIN_SCORE,
    &IFD_GARMIN_SEGMENTFILE,
    &IFD_GARMIN_SEGMENTID,
    &IFD_GARMIN_SEGMENTLAP,
    &IFD_GARMIN_SEGMENTLEADERBOARDENTRY,
    &IFD_GARMIN_SEGMENTPOINT,
    &IFD_GARMIN_SENSORSETTINGS,
    &IFD_GARMIN_SESSION,
    &IFD_GARMIN_SET,
    &IFD_GARMIN_SHOT,
    &IFD_GARMIN_SKINTEMPOVERNIGHT,
    &IFD_GARMIN_SLAVEDEVICE,
    &IFD_GARMIN_SLEEPASSESSMENT,
    &IFD_GARMIN_SLEEPDATAINFO,
    &IFD_GARMIN_SLEEPDISRUPTIONOVERNIGHTSEVERITY,
    &IFD_GARMIN_SLEEPDISRUPTIONSEVERITYPERIOD,
    &IFD_GARMIN_SLEEPLEVEL,
    &IFD_GARMIN_SLEEPRESTLESSMOMENTS,
    &IFD_GARMIN_SLEEPSCHEDULE,
    &IFD_GARMIN_SOFTWARE,
    &IFD_GARMIN_SPEEDZONE,
    &IFD_GARMIN_SPLIT,
    &IFD_GARMIN_SPLITSUMMARY,
    &IFD_GARMIN_SPLITTIME,
    &IFD_GARMIN_SPORT,
    &IFD_GARMIN_STRESSLEVEL,
    &IFD_GARMIN_TANKSUMMARY,
    &IFD_GARMIN_TANKUPDATE,
    &IFD_GARMIN_THREEDSENSORCALIBRATION,
    &IFD_GARMIN_TIMEINZONE,
    &IFD_GARMIN_TIMESTAMPCORRELATION,
    &IFD_GARMIN_TOTALS,
    &IFD_GARMIN_TRAININGFILE,
    &IFD_GARMIN_TRAININGLOAD,
    &IFD_GARMIN_TRAININGREADINESS,
    &IFD_GARMIN_TRAININGSETTINGS,
    &IFD_GARMIN_USERMETRICS,
    &IFD_GARMIN_USERPROFILE,
    &IFD_GARMIN_VIDEO,
    &IFD_GARMIN_VIDEOCLIP,
    &IFD_GARMIN_VIDEODESCRIPTION,
    &IFD_GARMIN_VIDEOFRAME,
    &IFD_GARMIN_VIDEOTITLE,
    &IFD_GARMIN_WATCHFACESETTINGS,
    &IFD_GARMIN_WAYPOINTHANDLING,
    &IFD_GARMIN_WEATHERALERT,
    &IFD_GARMIN_WEATHERCONDITIONS,
    &IFD_GARMIN_WEIGHTSCALE,
    &IFD_GARMIN_WORKOUT,
    &IFD_GARMIN_WORKOUTSCHEDULE,
    &IFD_GARMIN_WORKOUTSESSION,
    &IFD_GARMIN_WORKOUTSTEP,
    &IFD_GARMIN_ZONESTARGET,
    &IFD_GEOTIFF_MAIN,
    &IFD_GOOGLE_DEVICE,
    &IFD_GOOGLE_GAUDIO,
    &IFD_GOOGLE_GCAMERA,
    &IFD_GOOGLE_GCONTAINER,
    &IFD_GOOGLE_GCREATIONS,
    &IFD_GOOGLE_GDEPTH,
    &IFD_GOOGLE_GFOCUS,
    &IFD_GOOGLE_GIMAGE,
    &IFD_GOOGLE_GPANO,
    &IFD_GOOGLE_GSPHERICAL,
    &IFD_H264_MAIN,
    &IFD_HP_MAIN,
    &IFD_HTML_MAIN,
    &IFD_HTML_OFFICE,
    &IFD_HTML_DC,
    &IFD_HTML_EQUIV,
    &IFD_HTML_NCC,
    &IFD_HTML_PROD,
    &IFD_HTML_VW96,
    &IFD_ID3_COMPOSITE,
    &IFD_ID3_LYRICS3,
    &IFD_IPTC_APPLICATIONRECORD,
    &IFD_IPTC_COMPOSITE,
    &IFD_IPTC_ENVELOPERECORD,
    &IFD_IPTC_FOTOSTATION,
    &IFD_IPTC_NEWSPHOTO,
    &IFD_IPTC_OBJECTDATA,
    &IFD_IPTC_POSTOBJECTDATA,
    &IFD_IPTC_PREOBJECTDATA,
    &IFD_ISO_COMPOSITE,
    &IFD_ISO_MAIN,
    &IFD_ITC_MAIN,
    &IFD_JPEG_EPPIM,
    &IFD_JPEG_GRAPHCONV,
    &IFD_JPEG_MAIN,
    &IFD_JPEG_MEDIAJUKEBOX,
    &IFD_JPEG_SOF,
    &IFD_JVC_MAIN,
    &IFD_KODAK_BORDERS,
    &IFD_KODAK_CAMERAINFO,
    &IFD_KODAK_COMPOSITE,
    &IFD_KODAK_DCEM,
    &IFD_KODAK_DCMD,
    &IFD_KODAK_DCME,
    &IFD_KODAK_FREE,
    &IFD_KODAK_IFD,
    &IFD_KODAK_KDC_IFD,
    &IFD_KODAK_META,
    &IFD_KODAK_SPECIALEFFECTS,
    &IFD_KODAK_SUBIFD0,
    &IFD_KODAK_TYPE10,
    &IFD_KODAK_TYPE11,
    &IFD_KODAK_TYPE8,
    &IFD_KODAK_FREA,
    &IFD_LNK_INI,
    &IFD_LEAF_SUBIFD,
    &IFD_LYTRO_MAIN,
    &IFD_M2TS_AC3,
    &IFD_M2TS_MAIN,
    &IFD_MIFF_MAIN,
    &IFD_MISB_MAIN,
    &IFD_MNG_MAIN,
    &IFD_MPEG_AUDIO,
    &IFD_MPEG_COMPOSITE,
    &IFD_MPEG_VIDEO,
    &IFD_MPEG_XING,
    &IFD_MPF_COMPOSITE,
    &IFD_MPF_MAIN,
    &IFD_MWG_COLLECTIONS,
    &IFD_MWG_COMPOSITE,
    &IFD_MWG_KEYWORDS,
    &IFD_MWG_REGIONS,
    &IFD_MXF_MAIN,
    &IFD_MACOS_MDITEM,
    &IFD_MACOS_MAIN,
    &IFD_MACOS_XATTR,
    &IFD_MATROSKA_MAIN,
    &IFD_MATROSKA_PROJECTION,
    &IFD_MATROSKA_STDTAG,
    &IFD_MICROSOFT_MP,
    &IFD_MICROSOFT_MP1,
    &IFD_MICROSOFT_XMP,
    &IFD_MINOLTA_MAIN,
    &IFD_MINOLTA_AFSTATUSINFO,
    &IFD_MINOLTA_METABONESID,
    &IFD_MINOLTA_MINOLTALENSTYPES,
    &IFD_MOTOROLA_MAIN,
    &IFD_NIKON_AVI,
    &IFD_NIKON_COMPOSITE,
    &IFD_NIKON_NCDB,
    &IFD_NIKON_NCDT,
    &IFD_NIKON_NEFINFO,
    &IFD_NIKON_PREVIEWIFD,
    &IFD_NIKON_SCAN,
    &IFD_NIKON_TYPE2,
    &IFD_NIKON_BUTTONSZ8,
    &IFD_NIKON_BUTTONSZ9,
    &IFD_NIKON_NIKONLENSIDS,
    &IFD_NIKONCUSTOM_BUTTONSZ8,
    &IFD_NIKONCUSTOM_BUTTONSZ9,
    &IFD_NINTENDO_MAIN,
    &IFD_OGG_MAIN,
    &IFD_OLYMPUS_CAMERASETTINGS,
    &IFD_OLYMPUS_COMPOSITE,
    &IFD_OLYMPUS_EQUIPMENT,
    &IFD_OLYMPUS_FETAGS,
    &IFD_OLYMPUS_FOCUSINFO,
    &IFD_OLYMPUS_IMAGEPROCESSING,
    &IFD_OLYMPUS_MOV3,
    &IFD_OLYMPUS_MAIN,
    &IFD_OLYMPUS_OLYM2,
    &IFD_OLYMPUS_RAWDEVSUBIFD,
    &IFD_OLYMPUS_RAWDEVELOPMENT,
    &IFD_OLYMPUS_RAWDEVELOPMENT2,
    &IFD_OLYMPUS_RAWINFO,
    &IFD_OLYMPUS_UNKNOWNINFO,
    &IFD_OPENEXR_MAIN,
    &IFD_OPUS_MAIN,
    &IFD_OTHER_PFM,
    &IFD_PCAP_MAIN,
    &IFD_PDF_AIPRIVATE,
    &IFD_PDF_ADOBEPHOTOSHOP,
    &IFD_PDF_COLORSPACE,
    &IFD_PDF_DEFAULTRGB,
    &IFD_PDF_EF,
    &IFD_PDF_ENCRYPT,
    &IFD_PDF_F,
    &IFD_PDF_ILLUSTRATOR,
    &IFD_PDF_IM,
    &IFD_PDF_INFO,
    &IFD_PDF_KIDS,
    &IFD_PDF_MC,
    &IFD_PDF_MAIN,
    &IFD_PDF_MARKINFO,
    &IFD_PDF_METADATA,
    &IFD_PDF_PAGES,
    &IFD_PDF_PERMS,
    &IFD_PDF_PIECEINFO,
    &IFD_PDF_PRIVATE,
    &IFD_PDF_PROPERTIES,
    &IFD_PDF_REFERENCE,
    &IFD_PDF_RESOURCES,
    &IFD_PDF_ROOT,
    &IFD_PDF_SIGNATURE,
    &IFD_PDF_TRANSFORMPARAMS,
    &IFD_PDF_UNKNOWN,
    &IFD_PDF_XOBJECT,
    &IFD_PICT_MAIN,
    &IFD_PLUS_XMP,
    &IFD_PNG_MAIN,
    &IFD_PSP_MAIN,
    &IFD_PANASONIC_COMPOSITE,
    &IFD_PANASONIC_LEICA2,
    &IFD_PANASONIC_LEICA3,
    &IFD_PANASONIC_LEICA4,
    &IFD_PANASONIC_LEICA5,
    &IFD_PANASONIC_LEICA6,
    &IFD_PANASONIC_LEICA9,
    &IFD_PANASONIC_MAIN,
    &IFD_PANASONIC_SUBDIR,
    &IFD_PANASONIC_LEICALENSTYPES,
    &IFD_PANASONICRAW_CAMERAIFD,
    &IFD_PANASONICRAW_COMPOSITE,
    &IFD_PANASONICRAW_MAIN,
    &IFD_PARROT_COMPOSITE,
    &IFD_PENTAX_AVI,
    &IFD_PENTAX_MAIN,
    &IFD_PENTAX_S1,
    &IFD_PENTAX_TYPE2,
    &IFD_PENTAX_PENTAXLENSTYPES,
    &IFD_PHOTOMECHANIC_SOFTEDIT,
    &IFD_PHOTOMECHANIC_XMP,
    &IFD_PHOTOSHOP_UNKNOWN,
    &IFD_POSTSCRIPT_COMPOSITE,
    &IFD_QUICKTIME_COMPOSITE,
    &IFD_QUICKTIME_EEBOX,
    &IFD_RIFF_COMPOSITE,
    &IFD_RTF_MAIN,
    &IFD_RTF_USERPROPS,
    &IFD_RADIANCE_MAIN,
    &IFD_RAWZOR_MAIN,
    &IFD_REAL_AUDIO,
    &IFD_REAL_MEDIA,
    &IFD_REAL_METAFILE,
    &IFD_RED_MAIN,
    &IFD_RICOH_AVI,
    &IFD_RICOH_COMPOSITE,
    &IFD_RICOH_MAIN,
    &IFD_RICOH_RDTL,
    &IFD_RICOH_SUBDIR,
    &IFD_RICOH_THETASUBDIR,
    &IFD_RICOH_TYPE2,
    &IFD_SAMSUNG_APP5,
    &IFD_SAMSUNG_COMPOSITE,
    &IFD_SAMSUNG_TYPE2,
    &IFD_SAMSUNG_SMTA,
    &IFD_SAMSUNG_SVSS,
    &IFD_SANYO_MAIN,
    &IFD_SHORTCUTS_MAIN,
    &IFD_SIGMA_MAIN,
    &IFD_SIGMARAW_HEADEREXT,
    &IFD_SONY_COMPOSITE,
    &IFD_SONY_ERICSSON,
    &IFD_SONY_MAIN,
    &IFD_SONY_SR2DATAIFD,
    &IFD_SONY_SR2SUBIFD,
    &IFD_SONY_SONYLENSTYPES,
    &IFD_SONYIDC_COMPOSITE,
    &IFD_SONYIDC_MAIN,
    &IFD_STIM_MAIN,
    &IFD_TNEF_MAIN,
    &IFD_TAGINFOXML_ALLTABLES,
    &IFD_TEXT_MAIN,
    &IFD_THEORA_MAIN,
    &IFD_TORRENT_FILES,
    &IFD_TORRENT_INFO,
    &IFD_TORRENT_MAIN,
    &IFD_TORRENT_PROFILES,
    &IFD_TRAILER_GOOGLE,
    &IFD_TRAILER_ONEPLUS,
    &IFD_TRAILER_VIVO,
    &IFD_UNKNOWN_MAIN,
    &IFD_VCARD_MAIN,
    &IFD_VCARD_VCALENDAR,
    &IFD_VCARD_VNOTE,
    &IFD_VORBIS_COMPOSITE,
    &IFD_VORBIS_MAIN,
    &IFD_WPG_MAIN,
    &IFD_WTV_MAIN,
    &IFD_XISF_MAIN,
    &IFD_XMP_ALBUM,
    &IFD_XMP_COMPOSITE,
    &IFD_XMP_EXIFTOOL,
    &IFD_XMP_LIGHTROOM,
    &IFD_XMP_AUX,
    &IFD_XMP_CRS,
    &IFD_XMP_DC,
    &IFD_XMP_EXIF,
    &IFD_XMP_EXIFEX,
    &IFD_XMP_IPTCCORE,
    &IFD_XMP_OTHER,
    &IFD_XMP_PDF,
    &IFD_XMP_PDFX,
    &IFD_XMP_PHOTOSHOP,
    &IFD_XMP_RDF,
    &IFD_XMP_SAREA,
    &IFD_XMP_SCOLORANT,
    &IFD_XMP_SDIMENSIONS,
    &IFD_XMP_SPECIALSTRUCT,
    &IFD_XMP_TIFF,
    &IFD_XMP_X,
    &IFD_XMP_XMP,
    &IFD_XMP_XMPBJ,
    &IFD_XMP_XMPMM,
    &IFD_XMP_XMPNOTE,
    &IFD_XMP_XMPRIGHTS,
    &IFD_XMP_XMPTPG,
    &IFD_XMP_XMPTABLEDEFAULTS,
    &IFD_ZIP_RAR5,
];
