# Global pitfalls — lessons every fixer must read

Seeded from the first 29 human sweep-review verdicts (18 accepted / 11 rejected).
`[seed]` bullets are human-curated and must never be evicted by the distiller.
Runtime copy lives at `$OXIDEX_HOME/logs/knowledge/GLOBAL-PITFALLS.md`; this file
is the checked-in seed (`scripts/knowledge-seed/`). Cap: 3000 chars / 12 bullets.

- [seed] Never hardcode a group prefix like "EXIF:" on a tag name. Use the codebase's
  lookup_tag_name()/tag_db (or whatever the surrounding code already uses) so the
  prefix matches the IFD/table the tag was parsed from. (RW2 CFAPattern, X3F)
- [seed] Match tags by the exact tag ID in ExifTool's source, never by name
  similarity — CFAPattern is 0xA302, CFAPattern2 is 0x828E. (NEF)
- [seed] Copy PrintConv value strings byte-for-byte from the .pm shown in your
  prompt. Never paraphrase from vendor or platform documentation, even when it
  reads better. (MachO ObjectFileType: "Demand paged executable", not "...file")
- [seed] Verify a table index against the table's FIRST_ENTRY and its neighboring
  entries in the .pm. A synthetic test fixture built from your own assumed constant
  only proves self-consistency. (MRW BWFilter: index 42, not 0x26)
- [seed] Check the ExifTool entry's Format/Count before writing a decoder: a single
  int16u with a PrintConv table is a lookup, not a per-byte array decode. (RW2)
- [seed] Verify display format against `exiftool <file>` text output, not `-j`
  JSON — "1, 2" (comma-space), not "[1,2]". (XMP AboutCvTermCvId)
- [seed] `rg` the tag name (bare AND prefixed) before adding any emission. If any
  existing code path already emits it, edit THAT emitter in place — a parallel
  insert double-emits, and double emission escapes the gap-count recheck.
  (TTF FontFamily)
- [seed] For a value_difference, decode the EXISTING tag key in place; never add a
  second key under a different group. (X3F ComponentsConfiguration)
- [seed] Trace the call chain from the format dispatcher to your new code and state
  it in your plan. A unit test that calls your new function directly proves nothing
  about reachability. (JPEG APP12 MODE3 dead fallthrough; PSD IFD1 traversal)
- [seed] Before writing a new decoder, grep the codebase for the tag name — a
  correct, tested implementation may already exist elsewhere (e.g. src/core/) that
  just isn't wired into your code path. Reuse it. (JPEG MODE3 correct fix)
- [seed] Scope structured-document scans (XML/XMP) to the exact position ExifTool
  reads. Do not join values from sibling structures nested elsewhere (e.g.
  mwg-rs:Regions copies of ArtworkOrObject). (XMP ArtworkTitle)
