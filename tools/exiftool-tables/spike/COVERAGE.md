# SPIKE: would an AST interpreter cover materially more ExifTool Perl than `exprs.py`?

**Status: measurement spike. Not for merge as is. No production path is touched -- everything here lives under `tools/exiftool-tables/spike/` and nothing in `src/` or the generator output changes.**

## Instrument

Every number below comes from one command:

```
python3 tools/exiftool-tables/spike/run_spike.py \
    /Users/allen/oxidex-ops/evidence/20260917-observed-refresh/capture/catalog-writer-source.json
```

- repo commit: `v1.2.1-1586-g53d27645` (`53d276459ddac1e1a5a380ba1846e5c32ff6f9b8`), tree clean
- dump: `/Users/allen/oxidex-ops/evidence/20260917-observed-refresh/capture/catalog-writer-source.json`
- dump sha256: `536386691b0df2e6a6ebabed21bb8eb2de5dd66947901a4ea3284c87eecaf461`
- pinned release declared by the dump: **13.59**
- ExifTool Perl source read (never executed): `/tmp/oxidex-exiftool-cache/exiftool/lib/Image`
- translators measured: the committed `tools/exiftool-tables/exprs.py` and `conds.py`, imported in-process. No oxidex binary and no `exiftool` process is involved, so no corpus claim is made anywhere in this file.
- **Parse is a ceiling, not coverage.** `perl_subset.py` produces an AST and refuses cleanly outside its grammar; it does not evaluate. Every row below states what an interpreter would have to satisfy, not what one has been proven to reproduce.

## 1. Census of the pinned dump

- **13290 uses** (a use = one table field referencing an expression) across **3448 distinct expressions**.
- of those uses, 1826 sit on `_variants` members (conditional tag alternatives).
- by form: 12424 string expressions, 866 code refs (B::Deparse bodies).

| slot / form | uses | distinct |
| --- | ---: | ---: |
| `PrintConv/str` | 3342 | 483 |
| `ValueConv/str` | 2862 | 700 |
| `Condition/str` | 2233 | 1140 |
| `ValueConvInv/str` | 1401 | 328 |
| `PrintConvInv/str` | 1376 | 158 |
| `RawConv/str` | 1210 | 508 |
| `PrintConv/code` | 584 | 93 |
| `ValueConv/code` | 109 | 28 |
| `PrintConvInv/code` | 73 | 21 |
| `ValueConvInv/code` | 56 | 14 |
| `RawConv/code` | 44 | 14 |

> `expr_coverage.py`'s own walk sees a strict subset of this: it descends lists but not the `_variants` array this dump uses for conditional tag alternatives, and it reads only `ValueConv`/`PrintConv`/`RawConv` with `kind == "expr"`. Frame A below reproduces that denominator exactly so the baseline is comparable; Frame B is the whole surface.

## 2. Coverage ladder -- Frame A (`expr_coverage.py`'s own denominator)

denominator: **6993 uses / 1529 distinct expressions**

| rung | uses | % uses | distinct | % distinct |
| --- | ---: | ---: | ---: | ---: |
| a. exprs.py/conds.py today (accepts) | 5271 | 75.4% | 576 | 37.7% |
| b. parseable by the spike grammar | 6986 | 99.9% | 1526 | 99.8% |
| c. evaluable PURE ($val + operators + core builtins) | 4865 | 69.6% | 930 | 60.8% |
| d. + top 5 session keys (no helpers) | 4997 | 71.5% | 969 | 63.4% |
| d. + top 10 session keys (no helpers) | 5053 | 72.3% | 986 | 64.5% |
| d. + top 20 session keys (no helpers) | 5098 | 72.9% | 999 | 65.3% |
| d. + ALL session keys (no helpers) | 5422 | 77.5% | 1233 | 80.6% |
| e. + all session keys + top 10 helpers | 6560 | 93.8% | 1311 | 85.7% |
| e. + all session keys + top 25 helpers | 6779 | 96.9% | 1382 | 90.4% |
| e. + all session keys + top 50 helpers | 6908 | 98.8% | 1459 | 95.4% |
| e. + all session keys + ALL helpers (= b) | 6986 | 99.9% | 1526 | 99.8% |

With every session key available, adding helper ports in most-used-first order:

- 90% of uses: **5 helper ports** (of 112 distinct helper/data dependencies)
- 95% of uses: **14 helper ports** (of 112 distinct helper/data dependencies)
- 99% of uses: **56 helper ports** (of 112 distinct helper/data dependencies)

With every helper available, adding session keys in most-used-first order:

- 90% of uses: **2 session keys** (of 250 distinct keys)
- 95% of uses: **25 session keys** (of 250 distinct keys)
- 99% of uses: **186 session keys** (of 250 distinct keys)

Greedy dependency ladder (session keys and helpers ranked together, each step taking the dependency that unlocks the most uses):

- **90% of uses: 45 dependencies** (session keys + helper ports, combined).
- **95% of uses: 248 dependencies** (session keys + helper ports, combined).
- **99% of uses: 314 dependencies** (session keys + helper ports, combined).

## 3. Coverage ladder -- Frame B (every expression in the dump)

denominator: **13290 uses / 3448 distinct expressions**

> Rung `a` here is *what the committed translators accept when asked*, not what `codegen.py` routes: codegen sends only a binary table's `PrintConv` through `exprs.py` (its own docstring records that `ValueConv`/`RawConv` are recorded omitted), and no `*Inv` slot is routed through it at all. Frame B's rung `a` is therefore an UPPER bound on today's translator, which makes the interpreter's margin over it a lower bound.

| rung | uses | % uses | distinct | % distinct |
| --- | ---: | ---: | ---: | ---: |
| a. exprs.py/conds.py today (accepts) | 9068 | 68.2% | 1828 | 53.0% |
| b. parseable by the spike grammar | 13254 | 99.7% | 3437 | 99.7% |
| c. evaluable PURE ($val + operators + core builtins) | 8431 | 63.4% | 1791 | 51.9% |
| d. + top 5 session keys (no helpers) | 9435 | 71.0% | 2245 | 65.1% |
| d. + top 10 session keys (no helpers) | 9694 | 72.9% | 2334 | 67.7% |
| d. + top 20 session keys (no helpers) | 9870 | 74.3% | 2387 | 69.2% |
| d. + ALL session keys (no helpers) | 10872 | 81.8% | 2953 | 85.6% |
| e. + all session keys + top 10 helpers | 12300 | 92.6% | 3070 | 89.0% |
| e. + all session keys + top 25 helpers | 12713 | 95.7% | 3164 | 91.8% |
| e. + all session keys + top 50 helpers | 12931 | 97.3% | 3229 | 93.6% |
| e. + all session keys + ALL helpers (= b) | 13254 | 99.7% | 3437 | 99.7% |

With every session key available, adding helper ports in most-used-first order:

- 90% of uses: **6 helper ports** (of 199 distinct helper/data dependencies)
- 95% of uses: **22 helper ports** (of 199 distinct helper/data dependencies)
- 99% of uses: **113 helper ports** (of 199 distinct helper/data dependencies)

With every helper available, adding session keys in most-used-first order:

- 90% of uses: **13 session keys** (of 285 distinct keys)
- 95% of uses: **52 session keys** (of 285 distinct keys)
- 99% of uses: **192 session keys** (of 285 distinct keys)

Greedy dependency ladder (session keys and helpers ranked together, each step taking the dependency that unlocks the most uses):

- **90% of uses: 75 dependencies** (session keys + helper ports, combined).
- **95% of uses: 144 dependencies** (session keys + helper ports, combined).
- **99% of uses: 413 dependencies** (session keys + helper ports, combined).

## 4. Which session keys, and how the curve falls off

`self:X` is `$$self{X}` / `$self->{X}` / `$$et{X}`; `ctx:$x` is one of the lexicals ExifTool has in scope at the eval site (`$tag`, `$format`, `$count`, ...), which an interpreter must be handed; `self:<object>` is a bare `$self` passed to a helper (the helper's own $self reads are counted against the helper, not here); `self:VALUE{*} (other tags)` is a read of another tag's value, which is one mechanism rather than one key per tag.

| rank | session key | uses (Frame B) |
| ---: | --- | ---: |
| 1 | `self:Model` | 734 |
| 2 | `self:<object>` | 270 |
| 3 | `self:FacesDetected` | 166 |
| 4 | `self:Make` | 117 |
| 5 | `self:BitM` | 96 |
| 6 | `ctx:$count` | 71 |
| 7 | `ctx:$format` | 66 |
| 8 | `self:FacesA` | 62 |
| 9 | `self:ColorDataVersion` | 47 |
| 10 | `self:OPTIONS` | 43 |
| 11 | `self:ShutterMode` | 42 |
| 12 | `ctx:$tag` | 41 |
| 13 | `self:FirmwareVersion` | 33 |
| 14 | `self:LensMount` | 33 |
| 15 | `self:MakerNoteSigmaVer` | 31 |
| 16 | `self:FlashControlMode` | 30 |
| 17 | `self:FocusPointSchema` | 30 |
| 18 | `ctx:$tagInfo` | 28 |
| 19 | `self:LayoutFlags` | 28 |
| 20 | `self:DIR_NAME` | 27 |
| 21 | `self:TIFF_TYPE` | 27 |
| 22 | `self:NumChannelDescriptions` | 25 |
| 23 | `self:FlashControlBuiltin` | 23 |
| 24 | `self:IntervalShooting` | 23 |
| 25 | `self:AFDetectionMethod` | 22 |

## 5. Which helper subs, and whether oxidex already ports them

- distinct helper subs called across the whole dump: **157**
- of those, resolvable to a sub in the pinned 13.59 source: **155**
- pure functions of their arguments (no `$self`/`$et` read, no engine call): **103**
- read the ExifTool object themselves: **52**
- drive the reader engine (ProcessDirectory / HandleTag / FoundTag / ExtractInfo / raf I/O): **16**
- **already ported to Rust** in `src/exiftool_tables/exprs.rs`: **10 complete + 4 partial (one branch only -- e.g. ConvertDateTime as the identity with no -d/DateFormat, Decode only for UCS2)** of 157; that file has 33 `pub fn`s in total.
- module-level data tables referenced (`%canonLensTypes`-shaped, port-once static data, not subs): **41**

| rank | helper | uses | pure? | reads $self | engine | Rust port today |
| ---: | --- | ---: | --- | --- | --- | --- |
| 1 | `ET->ConvertDateTime` | 403 | no ($self) | yes | no | partial |
| 2 | `Exif::ConvertFraction` | 191 | yes | no | no | - |
| 3 | `GPS::ToDMS` | 179 | no ($self) | yes | no | partial |
| 4 | `Exif::PrintExposureTime` | 168 | yes | no | no | YES |
| 5 | `ConvertUnixTime` | 157 | yes | no | no | YES |
| 6 | `ET->InverseDateTime` | 90 | no ($self) | yes | no | - |
| 7 | `ConvertDuration` | 89 | yes | no | no | YES |
| 8 | `IsFloat` | 75 | yes | no | no | - |
| 9 | `ET->Decode` | 64 | no ($self) | yes | no | partial |
| 10 | `ConvertBitrate` | 41 | yes | no | no | YES |
| 11 | `Exif::PrintFraction` | 40 | yes | no | no | YES |
| 12 | `GPS::ToDegrees` | 40 | yes | no | no | - |
| 13 | `Canon::CanonEv` | 37 | yes | no | no | YES |
| 14 | `Canon::CanonEvInv` | 37 | yes | no | no | - |
| 15 | `GetUnixTime` | 34 | yes | no | no | - |
| 16 | `Exif::PrintFNumber` | 32 | yes | no | no | YES |
| 17 | `ET->ValidateImage` | 31 | no ($self) | yes | no | - |
| 18 | `ET->Warn` | 27 | no (engine) | yes | yes | - |
| 19 | `IsInt` | 27 | yes | no | no | - |
| 20 | `XMP::ConvertXMPDate` | 27 | yes | no | no | - |
| 21 | `ET->Encode` | 26 | no ($self) | yes | no | - |
| 22 | `Samsung::Crypt` | 25 | no ($self) | yes | no | - |
| 23 | `Nikon::PrintPC` | 23 | yes | no | no | YES |
| 24 | `Nikon::PrintPCInv` | 22 | yes | no | no | - |
| 25 | `ReverseLookup` | 21 | yes | no | no | - |
| 26 | `XMP::DecodeBase64` | 21 | yes | no | no | - |
| 27 | `ASF::GetGUID` | 20 | yes | no | no | YES |
| 28 | `Pentax::PrintFilter` | 20 | yes | no | no | - |
| 29 | `PrintHex` | 17 | yes | no | no | - |
| 30 | `Sony::Decipher` | 17 | yes | no | no | - |
| 31 | `Nikon::PrintAFPoints` | 16 | yes | no | no | - |
| 32 | `Nikon::PrintAFPointsInv` | 16 | yes | no | no | - |
| 33 | `ET->OverrideFileType` | 12 | no ($self) | yes | no | - |
| 34 | `ToFloat` | 12 | yes | no | no | - |
| 35 | `DecodeBits` | 11 | yes | no | no | - |
| 36 | `Exif::ExifDate` | 11 | yes | no | no | - |
| 37 | `GetByteOrder` | 11 | yes | no | no | - |
| 38 | `IPTC::InverseDateOrTime` | 11 | no ($self) | yes | no | - |
| 39 | `TimeZoneString` | 11 | yes | no | no | - |
| 40 | `Get32u` | 10 | yes | no | no | - |

## 6. Grammar growth: which production unlocked how much

Productions are added greedily (the one unlocking the most new uses first). Literal/`$val`/operator core alone parses 436 uses.

| step | production | new uses | new distinct | cumulative uses | cumulative % |
| ---: | --- | ---: | ---: | ---: | ---: |
| 1 | `arith` | 2016 | 220 | 2452 | 18.4% |
| 2 | `call` | 1179 | 156 | 3631 | 27.3% |
| 3 | `list_expr` | 1161 | 209 | 4792 | 36.1% |
| 4 | `string_interp` | 1019 | 132 | 5811 | 43.7% |
| 5 | `method` | 526 | 18 | 6337 | 47.7% |
| 6 | `ternary` | 292 | 50 | 6629 | 49.9% |
| 7 | `compare` | 422 | 100 | 7051 | 53.1% |
| 8 | `named_unary` | 387 | 65 | 7438 | 56.0% |
| 9 | `regex_match` | 91 | 36 | 7529 | 56.7% |
| 10 | `deref` | 373 | 283 | 7902 | 59.5% |
| 11 | `hash_elem` | 1278 | 515 | 9180 | 69.1% |
| 12 | `logic` | 355 | 206 | 9535 | 71.7% |
| 13 | `strcmp` | 384 | 165 | 9919 | 74.6% |
| 14 | `assign` | 327 | 176 | 10246 | 77.1% |
| 15 | `bitops` | 166 | 77 | 10412 | 78.3% |
| 16 | `special_vars` | 114 | 34 | 10526 | 79.2% |
| 17 | `concat` | 115 | 55 | 10641 | 80.1% |
| 18 | `stmt_sequence` | 98 | 61 | 10739 | 80.8% |
| 19 | `subst` | 568 | 168 | 11307 | 85.1% |
| 20 | `tr` | 202 | 41 | 11509 | 86.6% |
| 21 | `ref` | 89 | 25 | 11598 | 87.3% |
| 22 | `subscript` | 67 | 50 | 11665 | 87.8% |
| 23 | `my` | 69 | 49 | 11734 | 88.3% |
| 24 | `listop_no_parens` | 138 | 53 | 11872 | 89.3% |
| 25 | `return` | 93 | 64 | 11965 | 90.0% |
| 26 | `stmt_modifier` | 260 | 140 | 12225 | 92.0% |
| 27 | `require_module` | 58 | 25 | 12283 | 92.4% |
| 28 | `if_block` | 50 | 38 | 12333 | 92.8% |
| 29 | `map_grep_sort` | 22 | 16 | 12355 | 93.0% |
| 30 | `anon_ref` | 13 | 12 | 12368 | 93.1% |
| 31 | `range` | 6 | 1 | 12374 | 93.1% |
| 32 | `do_eval` | 6 | 1 | 12380 | 93.2% |
| 33 | `subst_e` | 5 | 5 | 12385 | 93.2% |
| 34 | `loop` | 4 | 4 | 12389 | 93.2% |
| 35 | `incdec` | 4 | 4 | 12393 | 93.3% |
| 36 | `qw` | 4 | 3 | 12397 | 93.3% |
| 37 | `loop_ctl` | 3 | 3 | 12400 | 93.3% |
| 38 | `c_style_for` | 5 | 5 | 12405 | 93.3% |
| 39 | `array_funcs` | 2 | 2 | 12407 | 93.4% |
| 40 | `bareword_call` | 1 | 1 | 12408 | 93.4% |
| 41 | `coderef` | 0 | 0 | 12408 | 93.4% |
| 42 | `deparse_wrapper` | 770 | 146 | 13178 | 99.2% |
| 43 | `amp_call` | 67 | 15 | 13245 | 99.7% |
| 44 | `regex_interp` | 5 | 2 | 13250 | 99.7% |
| 45 | `postfix_deref` | 2 | 1 | 13252 | 99.7% |
| 46 | `quote_ops` | 1 | 1 | 13253 | 99.7% |
| 47 | `anon_sub` | 0 | 0 | 13253 | 99.7% |
| 48 | `bare_block` | 0 | 0 | 13253 | 99.7% |
| 49 | `local` | 1 | 1 | 13254 | 99.7% |

## 7. Residue: what the interpreter still cannot do

- outside the grammar entirely (refused by the parser): **36 uses / 11 distinct**
- parsed, but calling a helper that drives the reader engine or that does not resolve to a sub in the pinned source: **32 uses / 20 distinct**
- parsed, but calling through a code ref / dynamic method (no static callee): **1 uses / 1 distinct**

Refusal reasons by uses:

| refusal | uses |
| --- | ---: |
| code ref body does not start with a block | 17 |
| unterminated statement before op:')' | 5 |
| expected ')', found eof:None | 4 |
| expected '}', found op:';' | 4 |
| unterminated quote-like construct | 2 |
| unterminated statement before ident:'m' | 2 |
| expected ')', found op:':' | 1 |
| unterminated statement before str:'\n            my $tzmin = $2 * 60 + $3;\n            $tzmin = -$tzmin if $1 eq ' | 1 |

Top 20 unparseable expressions, verbatim:

| uses | form | slot | expression | refusal |
| ---: | --- | --- | --- | --- |
| 14 | code | PrintConvInv | `($) ;` | code ref body does not start with a block |
| 4 | str | ValueConvInv[0] | `int(log($val)*2400) + 0.5)` | unterminated statement before op:')' |
| 4 | str | RawConv | `$self->Decode($val, ($$self{URIFlags} & 0x80) ? "UTF16" : "Latin"` | expected ')', found eof:None |
| 4 | code | PrintConvInv | `($;$$) { package Image::ExifTool::Sony; use strict; (my($val, $self, $features) = @_); (($val =~ /Unknown \((.*)\)/i) and (return $1)); (my($sf, $lf, $sa, $la) = &Image::ExifTool::Exif::GetLensInfo($val)); my($str); if (` | expected '}', found op:';' |
| 3 | code | PrintConvInv | `($$) ;` | code ref body does not start with a block |
| 2 | str | PrintConv | `$val m` | unterminated statement before ident:'m' |
| 1 | str | Condition | `$$self{HasIJPEG}"` | unterminated quote-like construct |
| 1 | str | ValueConvInv | `$val>0 ? log(12*($val+80)/log(2) : 0` | expected ')', found op:':' |
| 1 | str | ValueConv | `$_=join(".", unpack("C*", $val))); s/(:.*?:.*?:.*?):/$1 /; $_` | unterminated statement before op:')' |
| 1 | str | PrintConvInv | `return undef unless $val =~ /^([-+])(\d{1,2}):?(\d{2})$/' my $tzmin = $2 * 60 + $3; $tzmin = -$tzmin if $1 eq '-'; return $tzmin;` | unterminated statement before str:'\n            my $tzmin = $2 * 60 + $3;\n            $tzmin = -$tzmin if $1 eq ' |
| 1 | str | PrintConvInv | `$val=~s/to /; $val` | unterminated quote-like construct |

Engine-coupled examples (parsed, but not a value conversion at all):

| uses | expression | blocking helper(s) |
| ---: | --- | --- |
| 9 | `($$) { package Image::ExifTool::Apple; use strict; (my($val, $et) = @_); (my($dirInfo) = {'DataPt', (\$val), 'NoVerboseDir', 1}); (my($oldOrder) = $et->GetByteO` | PLIST::ProcessBinaryPLIST |
| 2 | `my $et = Image::ExifTool->new; my @tags = qw{ImageWidth ImageHeight FileType}; my $info = $et->ImageInfo(\$val, @tags); my ($w, $h, $type) = @$info{@tags}; $w a` | ET->ImageInfo, new |
| 2 | `Image::ExifTool::Nikon::PrintPCInv2($val,4)` | Nikon::PrintPCInv2 |
| 2 | `Image::ExifTool::Jpeg2000::ProcessJXLCodestream($self,\$val); undef` | Jpeg2000::ProcessJXLCodestream |
| 2 | `$val =~ s/\0+$//; # remove trailing nulls if (length $val and $$self{OPTIONS}{ExtractEmbedded}) { my $tagTbl = GetTagTable('Image::ExifTool::QuickTime::Stream')` | QuickTime::ProcessGPSLog |
| 1 | `return 'none' unless $val; my $e = Image::ExifTool->new; my $info = $e->ImageInfo(\$val,'ImageWidth','ImageHeight'); return undef unless $$info{ImageWidth} and ` | <dynamic>->ImageInfo, new |
| 1 | `require Image::ExifTool::MPF; @grps = $self->GetGroup($$val{0}); # set groups from input tag Image::ExifTool::MPF::ExtractMPImages($self);` | MPF::ExtractMPImages |
| 1 | `my @dim = unpack("x4N*", $val); return undef if @dim < 2; unless ($$self{DOC_NUM}) { $self->FoundTag(ImageWidth => $dim[0]); $self->FoundTag(ImageHeight => $dim` | ET->FoundTag |
| 1 | `my $tiff; ($tiff, @grps) = Image::ExifTool::Exif::RebuildTIFF($self, @val); return $tiff;` | Exif::RebuildTIFF |
| 1 | `my $pt = $self->ValidateImage(\$val, $tag); if ($pt) { $$self{BASE} += 0x20c; $$self{DOC_NUM} = ++$$self{DOC_COUNT}; $self->ExtractInfo($pt, { ReEntry => 1 }); ` | ET->ExtractInfo |
| 1 | `my $hdOff = $val[0]; my $reqTag = $$self{REQ_TAG_LOOKUP}{hiddendata}; my $hDump = $self->Options('HtmlDump'); return undef unless $reqTag or $self->Options('Val` | Sony::ReadHiddenData |
| 1 | `my $dat = Image::ExifTool::TNEF::DecompressRTF($self,$val); \$dat` | TNEF::DecompressRTF |
| 1 | `if ($val[3] and $val[4] and $val[0] ne $val[3]) { my %val = ( 0 => 'PreviewImageStart (1)', 1 => 'PreviewImageLength (1)', 2 => 'PreviewImageValid', ); $self->F` | ET->FoundTag |
| 1 | `if ($val[2] and $val[3]) { my $i = 1; for (;;) { my %val = ( 0 => $$val{2}, 1 => $$val{3} ); $self->FoundTag($tagInfo, \%val); ++$i; $$val{2} = "$$val{0} ($i)";` | ET->FoundTag |
| 1 | `if ($val[2] and $val[3]) { my $i = 1; for (;;) { my %val = ( 0 => $$val{2}, 1 => $$val{3} ); $self->FoundTag($tagInfo, \%val); ++$i; $$val{2} = "$$val{0} ($i)";` | ET->ExtractBinary, ET->FoundTag |

## 8. Instrument cross-check: the spike parser against perl itself

Every distinct expression was also handed to `/tmp/oxidex-perl538-build-20260913-r2/prefix/bin/perl5.38.2` as `eval "sub { ... }"` -- which COMPILES the body and never runs it (`spike/perl_syntax_check.pl`). A refusal perl also rejects is ExifTool's own broken source; a refusal perl accepts is a hole in this grammar. From inside the parser the two look identical, which is the whole reason for this table.

| | perl compiles | perl rejects |
| --- | ---: | ---: |
| spike parses | 13177 uses | 77 uses (77 of them only because the eval happens outside the module's lexical scope, see below) |
| spike refuses | 0 uses | 36 uses |

No grammar holes: every expression the spike refused, perl also refuses to compile. The refusal list in section 7 is ExifTool's own invalid Perl, not this parser's blind spot.

The cross-check has one known bias of its own: a code ref that closes over a module-level `my` variable (`\%afPoints51`, `\@lensFeatures`) or calls an imported bareword sub compiles inside its module and fails inside a bare `eval` -- **77 uses / 31 distinct** land in that cell and are NOT parser defects. The spike records exactly those variables as module-data dependencies (section 5's 'data tables' count), which is why they are broken out here rather than left to inflate the disagreement count.

Regex constructs Rust's `regex` crate cannot compile (an interpreter would need `fancy-regex`/PCRE for these):

| construct | uses |
| --- | ---: |
| lookahead | 11 |
| backreference | 2 |

## 9. Verdict

**Does the interpreter cover materially more than `exprs.py`? Yes -- but the coverage comes from the helper library and the session model, not from the parser.**

1. **The grammar is not the hard part.** A 1301-line recursive-descent parser reaches 99.7% of uses (99.7% of distinct expressions) on the whole surface, and 99.9% on `expr_coverage.py`'s frame. Perl itself refuses everything this parser refuses (section 8), so the grammar is effectively closed at 13.59.
2. **An interpreter with no ExifTool knowledge is WORSE than what ships today.** PURE-only evaluation -- `$val`, operators, core builtins -- is 4865 uses (69.6%) in Frame A against `exprs.py`'s 5271 (75.4%). The translator already inlines a dozen helpers and the identity cases; a bare AST walker does not.
3. **The crossover is the helper library.** All session keys plus the 25 most-used helpers puts Frame A at 6779 uses (96.9%, +1508 uses over today) and Frame B at 12713 (95.7%, +3645). In distinct-expression terms the gain is larger: 53.0% -> 91.8%, because the residue `exprs.py` leaves is a long tail of one-off expressions, not a few high-traffic ones.
4. **Cost, in ports:**

   - Frame A, helpers needed (every session key available, most-used first) -- 90%: 5, 95%: 14, 99%: 56 of 112 distinct helper/data dependencies.
   - Frame A, session keys needed (every helper available, most-used first) -- 90%: 2, 95%: 25, 99%: 186 of 250 distinct keys.
   - Frame B, helpers needed (every session key available, most-used first) -- 90%: 6, 95%: 22, 99%: 113 of 199 distinct helper/data dependencies.
   - Frame B, session keys needed (every helper available, most-used first) -- 90%: 13, 95%: 52, 99%: 192 of 285 distinct keys.

   oxidex has 10 complete and 4 partial Rust helper ports today, so the 95% rung is roughly a dozen more ports -- and 103 of the 157 helpers are pure functions of their arguments, which is the cheap kind.
5. **The residue is bounded, not a long tail.** 36 uses / 11 distinct expressions are outside the grammar, and every one of them is invalid Perl in ExifTool's own source (section 8's cross-check; `'$val m'`, a missing `)` in LNK.pm, a stray `"` in JPEG.pm). A further 32 uses / 20 distinct parse but call into the reader engine (`FoundTag`, `ProcessBinaryPLIST`, `ImageInfo`) -- those are not value conversions at all and no interpreter closes them; they need the engine.
6. **What this measurement does NOT say.** Parse success is a ceiling. An interpreter still has to reproduce Perl's semantics exactly (numeric/string duality, `sprintf` `%g`, `int()` truncation, regex dialect -- section 8's table shows a handful of patterns Rust's `regex` crate cannot even compile), and every ported helper still needs the differential check `verify_exprs.py` runs today. The interpreter moves the verification work from once-per-expression to once-per-production and once-per-helper; it does not remove it.

