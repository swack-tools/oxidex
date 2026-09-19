# origin/main vs refactor/tag-machinery: what releasing the tip without main's 45 commits loses

Measured 2026-09-18 for the v2.0.0-beta.1 release decision. `origin/main` (4a38afde) carries 45 commits
(#691-#736, 2026-08-11..13) that were never brought into `refactor/tag-machinery`; `git cherry` marks all 45
`+`. Merge-base 91e0ba02. This document measures what the tip loses by not having them, classifies each
commit, and recommends how to bring the still-needed ones forward. `main` is not touched.

## Instrument

* **Oracle**: pinned ExifTool 13.59 (`.exiftool-version`), perl 5.38.2, pinned source tree
  `/tmp/oxidex-exiftool-cache/exiftool`. Both probes asserted before any run: `-ver` -> `13.59`,
  `t/images/OOXML.docx` -> `FileType: DOCX`. Oracle output was identical across the separate runs
  (4238/4238 files equal modulo conformance.py's `IGNORE` set).
* **Corpus**: `combined-samples` (4,238 files: ExifTool `t/images` (194) + the 13 manufacturer sample sets
  from the justfile's `compare-exiftool-full-update` MANUFACTURERS list, the Aug-13 cache that main's
  commits measured against). 518,919 oracle tag occurrences; floors asserted (>=4000 files,
  >=400,000 tags) so a degraded oracle cannot produce a number.
* **Matcher**: `tools/exiftool-tables/conformance.py` from the tip -- its `run_exiftool` (`-G0:1:4 -s -j -a`),
  `run_oxidex` (`oxidex -j`), `tags_by_name` and four-tier `_match_bucket` imported unchanged. A thin
  wrapper (`mdiv_conformance.py`, in the evidence dir) runs the oracle once per file, grades every binary
  against the same oracle JSON, and records *which oracle key* (`EXIF:IFD0:Make`, one per occurrence
  under `-G0:1:4`) each binary matched -- conformance.py's `--json-out` does not carry matched keys.
* **Binaries** (release, each from its own detached worktree and `CARGO_TARGET_DIR`):
  `main` = 4a38afde, `base` = merge-base 91e0ba02, `tip` = 91836753 (the tip at measurement time),
  `pilot` = the forward-port branch below. The first pass used tip 3892a022; #838 landed in between, so
  the tip was rebuilt and re-graded -- numbers below are 91836753 unless marked. The tip has since moved to
  b9fcb759 (#841, beta.1 prep); `git diff 91836753 b9fcb759 -- src` is empty (version bumps only), so the
  91836753 numbers are the beta.1 numbers.
* **Evidence**: `~/oxidex-ops/evidence/20260918-main-divergence/` (rows.jsonl per binary, oracle.jsonl,
  regressions.json, net-regressions.json, partition-final.json, cherry-pick-probe.tsv, merge-probe-*.txt).

| binary | matched | value_diff | unpaired oracle (missing+rename) |
|---|---:|---:|---:|
| base 91e0ba02 | 442,604 | 2,300 | 35,865 |
| main 4a38afde | 446,243 | 2,316 | 32,210 |
| tip 3892a022 (first pass) | 468,172 | 420 | 12,177 |
| **tip 91836753** | **468,294** | 322 | 12,153 |
| pilot (tip + DJI port) | 468,390 | 322 | 12,057 |

Main gained +3,639 matched occurrences over the merge-base; the tip gained +25,690. The tip is
far ahead overall -- but it is ahead on *different* tags, and a release ships every tag main fixed and the
tip does not have.

## 1. Regressions a release would ship (matched on main, not on the tip)

* **Occurrence level**: 1,289 `(file, oracle key)` occurrences matched on main and not on the tip, in
  351 files; tip state {'MISSING': 1275, 'VALUE': 14}.
* **Net, per `(file, Group1:TagName)`** (what a user sees; cancels `-j` duplicate-winner swaps, where
  ExifTool prints a tag twice -- e.g. `[MakerNotes:Olympus]` and `[MakerNotes:Olympus:Copy1]`
  `WhiteBalanceBracket` -- `-j` can carry one, and base/tip pick different winners):
  **1,243** in 312 files, 272 distinct `Group1:TagName`.
  * **1,226 are main fixes the tip lacks** (unmatched at the merge-base, matched on main).
  * 17 are tags the tip lost relative to the merge-base itself -- not caused by main's absence, listed
    separately below.

### By family (net, Group1)

| Group1 | lost | files | top tags |
|---|---:|---:|---|
| Google | 313 | 22 | InitParamsText 12, ShotParamsText 12, StaticMetadataText 12, SummaryText 12, TimeLogText 12, PayloadMetadataText 11 ... |
| Canon | 233 | 86 | FaceWidth 33, AFConfigTool 23, HDR-PQ 23, AutoAFPointSelEOSiTRAF 21, AFStatusViewfinder 15, InitialAFPointInServo 14 ... |
| CIFF | 132 | 4 | ApertureValue 4, BaseISO 4, CanonFirmwareVersion 4, ColorBW 4, ColorBitDepth 4, ComponentBitDepth 4 ... |
| Sony | 119 | 48 | PixelShiftInfo 25, HiddenDataLength 11, HiddenDataOffset 11, FocalLengthTeleZoom 10, Barcode 9, TextInfo1 9 ... |
| DJI | 116 | 13 | Pitch 11, Roll 11, SpeedX 11, SpeedY 11, SpeedZ 11, Yaw 11 ... |
| CameraIFD | 67 | 4 | ApertureValue 4, FacesDetected 4, FlashFired 4, FocalLengthIn35mmFormat 4, FocusStepCount 4, FocusStepNear 4 ... |
| Composite | 50 | 32 | LensType 17, BlueBalance 5, RedBalance 5, WB_RGGBLevels 5, Aperture 3, ShutterSpeed 3 ... |
| Olympus | 42 | 35 | StackedImage 28, CameraParameters 5, Quality 5, ZoomedPreviewImage 2, CameraType 2 |
| Nikon | 35 | 14 | FirmwareVersion56 5, PixelShiftActive 5, Converter 3, Focus 3, WBBracketingSteps 3, BurstShotNumber 2 ... |
| Pentax | 27 | 16 | CAFPointsSelected 8, CAFPointsInFocus 7, ExternalFlashGuideNumber 6, DestinationCityCode 2, HometownCityCode 2, ISO 2 |
| Leica | 20 | 20 | OriginalDirectory 19, JPEGSize 1 |
| Panasonic | 16 | 10 | MakerNoteType 10, Gain 6 |
| XMP-drone-dji | 11 | 1 | AbsoluteAltitude 1, CamReverse 1, FlightPitchDegree 1, FlightRollDegree 1, FlightYawDegree 1, GimbalPitchDegree 1 ... |
| MediaJukebox | 7 | 1 | Album 1, Date 1, Name 1, People 1, Places 1, Tool_Name 1 ... |
| XMP-Device | 7 | 2 | Cameras 2, Profiles 2, ContainerDirectoryDataURI 1, ContainerDirectoryLength 1, ContainerDirectoryMime 1 |
| Casio | 4 | 4 | BestShotMode 2, ArtMode 1, CasioQuality 1 |
| XMP-crs | 4 | 1 | LookParametersToneCurvePV2012 1, LookParametersToneCurvePV2012Blue 1, LookParametersToneCurvePV2012Green 1, LookParametersToneCurvePV2012Red 1 |
| Kodak | 4 | 3 | KodakMaker 2, DateTimeStamp 1, TimeCreated 1 |
| XMP-exif | 3 | 2 | ComponentsConfiguration 1, GPSLatitude 1, GPSLongitude 1 |
| FLIR | 3 | 1 | Emissivity 1, ImageTemperatureMax 1, ImageTemperatureMin 1 |
| Samsung | 2 | 1 | EmbeddedAudioFile 1, EmbeddedAudioFileName 1 |
| JVC | 2 | 1 | CPUVersions 1, Quality 1 |
| HP | 2 | 1 | CameraDateTime 1, ISO 1 |
| Ricoh | 2 | 1 | RicohMake 1, RicohModel 1 |
| XMP-GContainer | 2 | 1 | DirectoryItemMime 1, DirectoryItemSemantic 1 |
| XMP-rdf | 1 | 1 | About 1 |
| Vivo | 1 | 1 | JSONInfo 1 |
| XMP-GMask | 1 | 1 | Data 1 |

### By manufacturer sample set (net)

Canon 344 | Google 312 | DJI 128 | Sony 122 | Leica 87 | t/images 78 | Pentax 46 | Nikon 35 | Panasonic 33 | Olympus 30 | Samsung 7 | FujiFilm 4

### Tags the tip lost against the merge-base (not a main issue)

`InteropIFD:ResolutionUnit` 2, `Track1:SourceImageHeight` 1, `Track1:SourceImageWidth` 1, `Composite:ImageSize` 1, `Composite:Megapixels` 1, `Copy1:Comment` 1, `Composite:BlueBalance` 1, `Composite:RedBalance` 1, `JFIF:JFIFVersion` 1, `UserData:Album` 1, `UserData:Artist` 1, `UserData:Comment` 1, `ItemList:Composer` 1, `ItemList:Genre` 1, `Copy1:FormattedName` 1, `Copy1:Note` 1

Mostly QuickTime/VCard/JFIF/Composite single-file rows plus InteropIFD:ResolutionUnit. The 46 Olympus
`Copy1` occurrences that appear at occurrence level are duplicate-winner swaps (tip matches the other
occurrence) and net to zero. Worth a separate look, but they are the tip's own changes, not main's.

### Reverse, for context (matched on the tip, not on main)

23,340 occurrences in 3,881 files. Top: IFD1 16,295, Pentax 1,229, Olympus 1,026, Composite 1,017, Canon 969, Panasonic 523, PreviewIFD 260, FujiFilm 252, ExifIFD 233, File 227, XML 125, DICOM 92.

## 2. Per-commit classification

Each of main's gains (occurrence unmatched on base, matched on main) is attributed to the earliest main
commit whose diff introduces that tag name (quoted string in src/tests), falling back to `git log -S`;
`PayloadFrameN` (built by `format!`) is attributed by `-G`. Name attribution is a heuristic: a name reused
by two commits goes to the earlier one. `tip has`/`tip lacks` split each commit's gains by the tip.
`pick` = independent `git cherry-pick --no-commit` onto the tip (pessimistic: later main commits depend on
earlier ones -- see the merge probe below).

**Counts: still-needed 22, conflicting 5, superseded 11, obsolete 7** (of 45).

* **still-needed** -- main matches, tip does not, and the hand code path main edited is still the one the
  tip uses: port it (textual conflicts only).
* **conflicting** -- still needed, but the tip replaced the code path (generated `Olympus::Main`/
  `CameraSettings` via `enabled_ifd.rs`, or `xmp/namespace_mapping.rs` deleted by #790): re-express, do not
  cherry-pick.
* **superseded** -- the tip matches everything the commit gained, by other means.
* **obsolete** -- fleet tooling for the stopped fleet, test-only guards for main-only tests, or
  `tag-comparison` harness normalizations that change no oxidex output.

| commit | subject | +lines | pick | gains | tip has | tip lacks | class | family / evidence |
|---|---|---:|---|---:|---:|---:|---|---|
| cec6d16a | fix(fleet): stop the dispatcher from re-publishing an already-open sweep PR (#691) | 195 | clean | 0 | 0 | 0 | **obsolete** | fleet. scripts/overlord_sweep.py dispatcher idempotency; the fleet is stopped |
| 7c2ad6e1 | fix(fleet): scope the sweep duplicate-check to the paths this round touched (#693) | 81 | conflict | 0 | 0 | 0 | **obsolete** | fleet. scripts/overlord_sweep.py sweep duplicate-check scoping; fleet stopped |
| c874e651 | fix(fleet): normalize formatting before the sweep idempotency checks (#695) | 104 | conflict | 0 | 0 | 0 | **obsolete** | fleet. scripts/overlord_sweep.py formatting normalization; fleet stopped |
| 06bb194e | [needs review] sweep sweep/tags-2026-08-11-9: 3 tag fix(es) across 2 squad(s) (#692) | 13 | conflict | 1 | 1 | 0 | **superseded** | misc. APE/DjVu/BPG one-tag sweep fixes; the tip matches the gained occurrence |
| 9b215f03 | Improve ExifTool parity across formats (#696) | 1936 | conflict | 391 | 171 | 220 | **still-needed** | CIFF-in-JPEG, Leica/PanasonicRaw CameraIFD, Sony, Pentax, DJI. new app_segments/ciff.rs (+422) and leica.rs (+300); the tip has neither decoder. Large: split by family when porting. Lacks: CIFF:MeasuredEV 4, CIFF:ShutterReleaseMethod 4, CIFF:FileNumber 4, CIFF:BaseISO 4, CIFF:ShutterReleaseTiming 4 ... |
| ed2982e1 | Expand JPEG maker note parity (#697) | 422 | conflict | 231 | 92 | 139 | **still-needed** | Google HDRP v2, Sony MoreInfo. google_hdrp.rs is unchanged on the tip since the base; the HDRP v2 text/image payloads are absent. Lacks: Google:SummaryText 12, Google:TimeLogText 12, Google:InitParamsText 12, Google:ShotParamsText 12, Google:StaticMetadataText 12 ... |
| 7dac13f9 | test: guard Panasonic corpus fixture (#698) | 8 | conflict | 0 | 0 | 0 | **obsolete** | test. test guard for a main-only Panasonic fixture test; carried with whichever Panasonic port needs it |
| c8887915 | Expand JPEG maker-note parity (#699) | 520 | conflict | 754 | 684 | 70 | **still-needed** | Canon AFConfig, Sony TextInfo/Barcode, Nikon. the hand sub-table code these edit (canon/binary_tables.rs, nikon/sub_tables.rs) is unchanged on the tip; the Olympus/Panasonic hunks conflict. Lacks: Canon:AFConfigTool 23, Sony:Barcode 9, Sony:TextInfo1 9, Sony:TextInfo2 9, Nikon:FirmwareVersion56 5 ... |
| c07cd328 | Close JPEG follow-up metadata gaps (#700) | 147 | conflict | 145 | 144 | 1 | **conflicting** | DJI XMP, Leica TimeInfo, Olympus focus. 144/145 gained occurrences already match on the tip; the residual is embedded DJI XMP, which edited xmp/namespace_mapping.rs -- deleted on the tip by #790 (family-1 XMP groups). Lacks: XMP-drone-dji:RtkFlag 1 |
| 5cd52a0e | Expand JPEG Leica Nikon and Pentax parity (#701) | 268 | conflict | 97 | 58 | 39 | **still-needed** | Leica, Pentax CAF, Nikon AFInfo2. Leica OriginalDirectory 19 files; leica.rs and pentax.rs are hand paths on the tip. Lacks: Leica:OriginalDirectory 19, Pentax:CAFPointsSelected 8, Pentax:CAFPointsInFocus 7, Nikon:FocusPositionVertical 2, Nikon:FocusPositionHorizontal 2 ... |
| 75d06225 | fix: expand JPEG legacy maker-note parity (#702) | 274 | conflict | 53 | 29 | 24 | **still-needed** | Panasonic MakerNoteType/Gain, Pentax flash, Olympus. Panasonic/Pentax hunks are hand paths; the Olympus hunk (ZoomedPreviewImage) must be re-expressed on the generated Olympus::Main. Lacks: Panasonic:MakerNoteType 10, Panasonic:Gain 6, Pentax:ExternalFlashGuideNumber 6, Olympus:ZoomedPreviewImage 2 |
| 12a2b7d7 | fix: expand JPEG DJI Canon Sony parity (#703) | 199 | conflict | 62 | 0 | 62 | **still-needed** | Sony PixelShiftInfo/HiddenData, DJI debug, Canon ModifiedInfo. none of its 62 gained occurrences match on the tip; sony.rs and dji_dbg.rs are hand paths. Lacks: Sony:PixelShiftInfo 25, Sony:HiddenDataLength 11, Sony:HiddenDataOffset 11, Canon:ModifiedDigitalGain 2, DJI:HyperlapsDebugInfo 2 ... |
| 502b69ce | fix: extend JPEG legacy and composite parity (#704) | 207 | clean | 19 | 6 | 13 | **conflicting** | Olympus/Casio/JVC Quality, Nikon Converter/Focus. Olympus:Quality now comes from the generated Olympus::Main (enabled_ifd.rs); Nikon hunks are portable. Lacks: Olympus:Quality 5, Nikon:Converter 3, Nikon:Focus 3, Casio:CasioQuality 1, JVC:Quality 1 |
| badda311 | fix: close JPEG Canon Olympus Sony parity gaps (#705) | 329 | conflict | 232 | 148 | 84 | **conflicting** | Olympus StackedImage, Canon HDR-PQ/RawJpgQuality, Sony. Olympus:StackedImage (28 files) is Olympus::CameraSettings, generated and enabled on the tip -- re-express there; Canon/Sony hunks are portable. Lacks: Olympus:StackedImage 28, Canon:HDR-PQ 23, Canon:RawJpgQuality 10, Sony:FocalLengthTeleZoom 10, Sony:FlashExposureCompSet2 3 ... |
| be400747 | fix: extend JPEG FujiFilm and DJI parity (#706) | 73 | conflict | 5 | 3 | 2 | **still-needed** | DJI FlightSpeed, FujiFilm. 2 DJI occurrences; FujiFilm hunk superseded by the generated FujiFilm::Main. Lacks: DJI:FlightSpeed 2 |
| 8e2af335 | fix: expand JPEG composite and maker-note parity (#707) | 413 | conflict | 477 | 434 | 43 | **still-needed** | Canon AF (AFStatusViewfinder, InitialAFPointInServo, USMLensElectronicMF), Composite. 434/477 already match on the tip; residual is Canon hand sub-tables. Lacks: Canon:AFStatusViewfinder 15, Canon:InitialAFPointInServo 14, Canon:USMLensElectronicMF 11, Nikon:DistortionControl 1, XMP-exif:GPSLatitude 1 ... |
| 95d16186 | fix: extend JPEG maker note parity (#708) | 513 | conflict | 426 | 32 | 394 | **still-needed** | Google HDRP payloads, DJI::Main floats, Canon FaceWidth, FLIR, Pentax. largest single loss; google_hdrp.rs, dji.rs, flir.rs are unchanged hand paths on the tip. DJI::Main floats forward-ported in the pilot PR. Lacks: Canon:FaceWidth 33, DJI:SpeedY 11, DJI:SpeedX 11, DJI:SpeedZ 11, DJI:Roll 11 ... |
| 8b91de9f | fix: extend Canon JPEG tag parity (#709) | 282 | conflict | 398 | 358 | 40 | **still-needed** | Canon AutoAFPointSelEOSiTRAF, processing tables. 358/398 already match; residual in canon/binary_tables.rs (hand, unchanged on tip). Lacks: Canon:AutoAFPointSelEOSiTRAF 21, Canon:AFPointsInFocus1D 6, Canon:ToneCurveMatching 2, Canon:SharpnessFreqTable 2, Canon:WhiteBalanceMatching 2 ... |
| 47037a04 | fix: extend Google and Nikon JPEG parity (#710) | 205 | conflict | 4 | 0 | 4 | **still-needed** | Google XMP-Device Cameras/Profiles, Composite. google_hdrp.rs/struct_flatten.rs; small. Lacks: XMP-Device:Profiles 2, XMP-Device:Cameras 2 |
| 9d3ae030 | fix: reconcile CIFF JPEG tag parity (#711) | 83 | conflict | 113 | 112 | 1 | **superseded** | CIFF JPEG reconcile. 112/113 match on the tip; the 1 residual (CIFF:ComponentVersion) rides with 9b215f03's CIFF decoder. Lacks: CIFF:ComponentVersion 1 |
| f6d86743 | fix: close remaining JPEG maker note gaps (#712) | 94 | conflict | 3 | 0 | 3 | **still-needed** | Casio ArtMode/BestShotMode, Pentax, Google Rectiface. 3 occurrences. Lacks: Casio:BestShotMode 2, Casio:ArtMode 1 |
| 74d2b7ec | fix: extend JPEG maker note compatibility (#713) | 183 | conflict | 3 | 0 | 3 | **still-needed** | Kodak KodakMaker, JVC CPUVersions. 3 occurrences. Lacks: Kodak:KodakMaker 2, JVC:CPUVersions 1 |
| 86bb9284 | fix: refine JPEG maker note parity (#714) | 32 | conflict | 1 | 1 | 0 | **superseded** | FujiFilm/Pentax refinement. the 1 gained occurrence matches on the tip |
| 404b0d80 | fix: compute Panasonic JPEG lens type (#715) | 21 | conflict | 18 | 1 | 17 | **still-needed** | Composite:LensType (Panasonic). 21-line composite; 17 files; textual conflict in composite/compute.rs only. Lacks: Composite:LensType 17 |
| 8011bfe4 | fix: complete Kodak JPEG maker note fields (#716) | 44 | clean | 2 | 0 | 2 | **still-needed** | Kodak DateTimeStamp/TimeCreated. cherry-picks CLEAN onto the tip; 2 occurrences. Lacks: Kodak:DateTimeStamp 1, Kodak:TimeCreated 1 |
| b762a2da | JPEG parity: Vivo trailer and CIFF free bytes (#717) | 71 | conflict | 3 | 0 | 3 | **still-needed** | Vivo trailer, CIFF FreeBytes. new parsers/vivo.rs; CIFF part depends on 9b215f03. Lacks: CIFF:FreeBytes 2, Vivo:JSONInfo 1 |
| bb326810 | JPEG parity: Canon original decision data (#718) | 171 | conflict | 2 | 0 | 2 | **still-needed** | Composite/Canon OriginalDecisionData. operations.rs hand path; Canon::Main generated engine keeps OriginalDecisionDataOffset in its hand residual. Lacks: Composite:OriginalDecisionData 2 |
| bb85f60a | JPEG parity: dispatch HP Type4 maker notes (#719) | 58 | conflict | 2 | 1 | 1 | **still-needed** | HP Type4 maker notes. 1 occurrence (HP:CameraDateTime). Lacks: HP:CameraDateTime 1 |
| 32f3881c | fix(jpeg): match Panasonic FaceRec UTF-8 rendering (#720) | 26 | conflict | 1 | 1 | 0 | **superseded** | Panasonic FaceRec UTF-8. the gained occurrence matches on the tip |
| a1a9eb1c | fix(jpeg): enable Nikon D810 bracketing tag (#721) | 106 | conflict | 3 | 0 | 3 | **still-needed** | Nikon WBBracketingSteps (D810). 3 occurrences. Lacks: Nikon:WBBracketingSteps 3 |
| 8a264d7e | JPEG parity: retain primary Google container item (#722) | 45 | conflict | 3 | 0 | 3 | **still-needed** | Google container primary item. 3 XMP-Device occurrences; jpeg_helpers.rs textual conflict. Lacks: XMP-Device:ContainerDirectoryLength 1, XMP-Device:ContainerDirectoryDataURI 1, XMP-Device:ContainerDirectoryMime 1 |
| fb159f2d | JPEG parity: parse Media Jukebox and Samsung trailers (#723) | 210 | conflict | 8 | 0 | 8 | **still-needed** | MediaJukebox / Samsung trailers. new parsers/samsung_trailer.rs; 8 occurrences. Lacks: MediaJukebox:Places 1, MediaJukebox:Name 1, MediaJukebox:Date 1, MediaJukebox:Album 1, Samsung:EmbeddedAudioFile 1 ... |
| a2696abc | JPEG parity: align Vivo and Ricoh Type2 tags (#724) | 95 | conflict | 2 | 0 | 2 | **still-needed** | Ricoh Type2 make/model. 2 occurrences; also carries a tag-comparison engine hunk (drop it). Lacks: Ricoh:RicohModel 1, Ricoh:RicohMake 1 |
| 785ee876 | JPEG parity: route CAMER maker notes through Olympus (#725) | 28 | conflict | 5 | 0 | 5 | **conflicting** | CAMER maker notes -> Olympus. Olympus:CameraParameters 5 files; dispatches into the Olympus path the tip now serves from the generated Olympus::Main. Lacks: Olympus:CameraParameters 5 |
| 634a84e5 | JPEG comparison: preserve array transport values (#726) | 31 | clean | 1 | 1 | 0 | **obsolete** | harness. tag-comparison transport normalization only; oxidex output unchanged |
| a5fdaf48 | JPEG comparison: close remaining XMP gaps (#727) | 122 | conflict | 1 | 0 | 1 | **conflicting** | XMP-crs tone curves, Google XMP. edited xmp/namespace_mapping.rs (deleted on the tip, #790) and the tag-comparison engine; 1 occurrence. Lacks: XMP-crs:LookParametersToneCurvePV2012 1 |
| 6fad16f9 | CRW: decode shared CIFF records (#728) | 109 | conflict | 8 | 8 | 0 | **superseded** | CRW shared CIFF records. CanonRaw.crw: tip matches 159/170 oracle tags vs main 141 |
| 700b7c83 | CRW: decode Canon settings records (#729) | 46 | conflict | 1 | 1 | 0 | **superseded** | CRW Canon settings. see 6fad16f9 |
| 3d2d56dc | CRW: decode Canon shot info records (#730) | 119 | conflict | 15 | 15 | 0 | **superseded** | CRW shot info. see 6fad16f9 |
| cc76d9e6 | CRW: decode Canon color balance records (#731) | 28 | conflict | 0 | 0 | 0 | **superseded** | CRW color balance. see 6fad16f9; no occurrence lost |
| d167c2ae | CRW: decode Canon AF info scalars | 67 | conflict | 6 | 6 | 0 | **superseded** | CRW AF info. see 6fad16f9 |
| c9a6fec4 | comparison: normalize QuickTime source dimensions (#733) | 57 | clean | 0 | 0 | 0 | **obsolete** | harness. tag-comparison QuickTime source-dimension normalization only |
| 91b378c2 | CR3: render QuickTime timestamps locally (#734) | 108 | conflict | 0 | 0 | 0 | **superseded** | CR3 QuickTime timestamps. CanonRaw.cr3: tip 329 matched vs main 328; no timestamp occurrence lost |
| 028b56e6 | comparison: prefer EXIF XMP native digest | 6 | clean | 0 | 0 | 0 | **obsolete** | harness. tag-comparison NativeDigest preference only |
| 4a38afde | XMP: retain focal-plane rational forms (#736) | 55 | conflict | 5 | 5 | 0 | **superseded** | XMP focal-plane rationals. all 5 gained occurrences match on the tip |
| (composite_cascade) | Composite tags derived from main-fixed maker-note inputs | | | 49 | 32 | 17 | | Composite:RedBalance 5, Composite:BlueBalance 5, Composite:FOV 2, Composite:FocalLength35efl 2, Composite:LightValue 2, Composite:HyperfocalDistance 1 |
| (unattributed) | names no main commit introduces literally (mostly XMP-drone-dji: c07cd328) | | | 93 | 73 | 20 | | Olympus:CameraType 2, XMP-exif:ComponentsConfiguration 1, XMP-rdf:About 1, XMP-drone-dji:CamReverse 1, XMP-drone-dji:FlightYawDegree 1, XMP-drone-dji:AbsoluteAltitude 1 |

Sum of `tip lacks` = 1,226 = the net main-fix regression count above.

### How much of main is simply missing vs. in conflict

`git merge --no-commit 4a38afde` onto the tip (probe only, aborted): **30 conflicted files, ~91 conflict hunks**, plus a modify/delete on `src/parsers/xmp/namespace_mapping.rs`.
Independent per-commit picks: CLEAN 6, CONFLICT 39.
11 files main created do not exist on the tip (ciff.rs, vivo.rs, samsung_trailer.rs, 7 test files) and
`google_hdrp.rs`, `dji.rs`, `dji_dbg.rs`, `flir.rs`, `hp.rs`, `jvc.rs`, `kodak.rs`, `ricoh.rs`,
`canon/binary_tables.rs`, `nikon/sub_tables.rs`, `nikon/af_info2.rs` are *unchanged* on the tip since the
merge-base -- those hunks apply as written. A wholesale merge is not recommended: it drags in the
obsolete harness and fleet changes and forces ~91 hand resolutions across files the tip has reworked 5-20
times (tiff_helpers.rs, jpeg_helpers.rs, canon.rs, raw/metadata.rs, operations.rs).

## 3. Pilot forward-port (DJI::Main floats, from 95d16186)

Branch `staging/fwd-dji-main-floats` (separate PR). main's `dji.rs` hunk applied verbatim with
`git apply --3way`: **clean, no conflict**. Tests were re-expressed for the tip: a synthetic unit test
of the float arm plus its format guard, and a corpus test under the tip's `#[ignore = "needs <corpus>"]`
convention (main's version silently returned when the fixture was absent). fmt, clippy
(`--release --all-features -D warnings`), `cargo test --workspace` (0 failed) and the ignored corpus
test (`--include-ignored`) pass.

Same instrument, tip 91836753 -> pilot: **96 occurrences MISSING -> matched** in 11 files (DJI:SpeedY 11, DJI:SpeedX 11, DJI:SpeedZ 11, DJI:Roll 11, DJI:Pitch 11, DJI:Yaw 11, DJI:CameraPitch 10, DJI:CameraRoll 10, DJI:CameraYaw 10), **0 matched -> not matched**, 0 new VALUE rows.
`tools/ci/read_regression_gate.py` on an authenticated `corpus_read_receipt.py` receipt of the pilot over
`t/images`: **PASS** -- published 2378, lost 0, newly credited 0, public failures 0 (snapshot 7a9c7576, receipt head e49e9744; t/images has no Phantom-era DJI file, so the gain shows only on combined-samples).

## 4. Recommendation

Do not merge `main` wholesale, and do not ship beta.1 as if the tip were a superset of main: it is not --
1,226 tag occurrences over 304 corpus files regress relative to the
last main users could build. Forward-port the still-needed fixes as a handful of family PRs, each proven with
the same instrument as the pilot (this matcher over combined-samples: the targeted tags MISSING -> matched,
0 matched -> lost, 0 new VALUE; plus `read_regression_gate.py` PASS on a t/images receipt). Order by payoff
per hour:

| PR | scope | why cheap / what's hard | recovers (net occ.) | effort |
|---|---|---|---:|---|
| 1 | P1 Google HDRP (ed2982e1, 95d16186 HDRP part, 47037a04, f6d86743 Rectiface, 8a264d7e) | google_hdrp.rs untouched on tip; hunks apply; new test file | ~320 (Google 313 + XMP-Device + XMP-GContainer) | 0.5 day |
| 2 | P2 DJI (95d16186 floats = pilot, 12a2b7d7 + be400747 dji_dbg, c07cd328 drone-dji XMP) | dji.rs/dji_dbg.rs untouched; the XMP part must be re-expressed on #790's family-1 XMP groups | ~128 | 0.5 day (pilot done: 96) |
| 3 | P3 Canon hand sub-tables (c8887915, 8e2af335, 8b91de9f, 95d16186 FaceWidth, badda311 HDR-PQ/RawJpgQuality, 12a2b7d7 Modified*, bb326810) | canon/binary_tables.rs unchanged on tip; canon.rs has 17 tip commits (Canon::Main generated top level only) | ~235 | 1 day |
| 4 | P4 CIFF-in-JPEG + Leica/PanasonicRaw CameraIFD (9b215f03, b762a2da, 9d3ae030 residual, 5cd52a0e Leica) | new ciff.rs (422 lines) and leica.rs (+300); largest single port | ~220 | 1-1.5 days |
| 5 | P5 Sony (12a2b7d7 PixelShift/HiddenData, c8887915 TextInfo/Barcode, badda311, ed2982e1 MoreInfo) | sony.rs / sony/amount.rs: 2-3 tip commits, textual conflicts | ~119 | 0.5 day |
| 6 | P6 Olympus, re-expressed on the generated path (badda311 StackedImage, 502b69ce Quality, 75d06225 ZoomedPreviewImage, 785ee876 CAMER dispatch) | Olympus::Main/CameraSettings are generated + enabled on the tip: find why the generated rows omit these (Omitted:: reason) and close it there, with the enabled_ifd.rs A/B | ~42 | 1 day |
| 7 | P7 long tail: Nikon (c8887915, 5cd52a0e AFInfo2, a1a9eb1c, 502b69ce), Pentax (5cd52a0e, 75d06225), Panasonic (75d06225, 404b0d80 Composite:LensType), Kodak/JVC/Casio/HP/Ricoh/Samsung/MediaJukebox/Vivo (8011bfe4, 74d2b7ec, f6d86743, bb85f60a, a2696abc, fb159f2d) | small, independent hand-path hunks; 8011bfe4 picks clean | ~120 | 1 day |

Total ~5-6 engineer-days; P1+P2+P3 recover ~680 of the 1,226 (56%) in ~2 days. Nothing to do for the 11 superseded
and 7 obsolete commits (fleet, test guard, harness). If beta.1 cannot wait, ship it with this document's
family table as a known-regressions note and land P1-P3 for beta.2.

Porting notes: port the *decoder* hunks only -- drop every `src/bin/tag-comparison` hunk (harness-only;
the release instrument is conformance.py), and re-express main's early-return corpus tests under
`#[ignore = "needs ..."]` and sweep them with `--include-ignored` (they otherwise escape the gate).
Composite cascades (RedBalance/BlueBalance/FOV/LightValue...) should reappear on their own once the
maker-note inputs they derive from land; re-measure rather than port composite code for them.
