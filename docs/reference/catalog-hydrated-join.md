# Catalog-to-hydrated source join

This report records exact source and generated-declaration identities. Generated declarations remain unobserved until immutable fixture evidence joins them.

- ExifTool: `13.59`
- Catalog SHA-256: `78baef58da0b5dce7c8ed5a9fc16beca8dd7049b45ddac78899d12f53c084e53`
- Hydrated SHA-256: `933f0c6b585b2b6039d68c9ac1f553fb596ba33f4b889f589d2b19bba444f068`

| Measurement | Count |
| --- | ---: |
| Ordinary catalog entries | 33487 |
| Hydrated source coordinates | 35886 |
| Preserved joined records | 33487 |
| `source_row_joined` | 33487 |

## Source-derived implementation

| Classification | Count |
| --- | ---: |
| `blocked_generated_reader_refusal` | 156 |
| `generated_reader_declaration_option_gated` | 1123 |
| `generated_reader_declaration_unobserved` | 778 |
| `ifd_schema_declaration_eligible_unobserved` | 2888 |
| `ifd_schema_declaration_omitted_unobserved` | 418 |
| `ifd_schema_declaration_refused_unobserved` | 9562 |
| `source_row_not_yet_consumed` | 18562 |

## Native writability

ExifTool's TagNames **Writable** column decides which entries are write-parity work. Only `writable` entries count toward the write denominator; `native_not_writable`, `native_writable_protected_indirect` (Protected, written only indirectly) and `native_not_listed` are not missing writers.

| Native Writable class | Count |
| --- | ---: |
| `not_listed` | 1 |
| `not_writable` | 19262 |
| `writable` | 14169 |
| `writable_protected` | 55 |

| Write parity (writable entries only) | Count |
| --- | ---: |
| Entries ExifTool writes directly | 14169 |
| Distinct case-insensitive writable names | 7068 |
| With a generated writer declaration | 19 |
| Observed write matching pinned ExifTool read-back | 0 |

## Writer implementation

| Classification | Count |
| --- | ---: |
| `generated_writer_declaration_unobserved` | 19 |
| `native_not_listed` | 1 |
| `native_not_writable` | 19262 |
| `native_writable_protected_indirect` | 55 |
| `writer_not_declared` | 14150 |

## Observed reads

| Classification | Count |
| --- | ---: |
| `not_observed_yet` | 33487 |

A join requires exact `(table full name, raw key, variant index)` and exact public-name spelling. Observations additionally require authenticated native comparisons in the exact Group1 context. Entries without imported evidence remain unobserved. Published historical receipts are linked at [authenticated catalog observations](catalog-hydrated-observed.md); they remain historical if this source ledger changes.

## Source-table progress

Declarations below are authenticated schema facts, not runtime reachability or observed coverage. Reader declarations exclude `generated_reader_declaration_option_gated` rows, which ExifTool reaches only through an option OxiDex does not expose. IFD declarations replay their exact source and, when bound, the expression-oracle ledger. Eligible, omitted and refused schema rows remain separate; schema eligibility does not establish a runtime route. Unaccounted rows may have runtime consumers that this join has not indexed. Refusal reasons can overlap; their totals are not an additional row denominator.

| Source table | Source variants | Catalog entries | Reader declarations | Natively writable entries | Writer declarations | Observed read entries | Observed write entries | Refusal reasons |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| Image::ExifTool::AAC::Main | 4 | 4 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::AFCP::Main | 4 | 3 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::AIFF::Comment | 3 | 3 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::AIFF::Common | 6 | 6 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::AIFF::FormatVers | 1 | 1 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::AIFF::Main | 9 | 5 | 0 | 0 | 0 | 0 | 0 | raw_key_unrepresentable: 5 |
| Image::ExifTool::APE::Main | 9 | 9 | 0 | 0 | 0 | 0 | 0 | raw_key_unrepresentable: 9 |
| Image::ExifTool::APE::NewHeader | 7 | 7 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::APE::OldHeader | 6 | 6 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::APP12::Ducky | 3 | 3 | 0 | 3 | 0 | 0 | 0 | — |
| Image::ExifTool::APP12::PictureInfo | 27 | 27 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::ASF::CodecList | 6 | 6 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::ASF::ContentBranding | 4 | 4 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::ASF::ContentDescr | 5 | 5 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::ASF::ExtendedDescr | 156 | 155 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::ASF::FileProperties | 11 | 11 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::ASF::Header | 16 | 9 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::ASF::HeaderExtension | 13 | 11 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::ASF::Main | 7 | 5 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::ASF::Picture | 4 | 4 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::ASF::StreamProperties | 9 | 9 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Apple::Main | 42 | 41 | 31 | 32 | 0 | 0 | 0 | print_conv: 1; value_conv: 9 |
| Image::ExifTool::Apple::RunTime | 4 | 4 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Audible::Main | 6 | 6 | 0 | 0 | 0 | 0 | 0 | raw_key_unrepresentable: 4 |
| Image::ExifTool::Audible::cvrx | 2 | 2 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Audible::meta | 11 | 11 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Audible::tags | 3 | 0 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Audible::tseg | 2 | 1 | 0 | 0 | 0 | 0 | 0 | raw_key_unrepresentable: 1 |
| Image::ExifTool::BMP::Extra | 4 | 3 | 0 | 0 | 0 | 0 | 0 | raw_key_unrepresentable: 3 |
| Image::ExifTool::BMP::Main | 25 | 25 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::BMP::OS2 | 5 | 5 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::BPG::Extensions | 5 | 2 | 2 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::BPG::Main | 8 | 8 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::CBOR::Main | 9 | 9 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Canon::AFConfig | 28 | 28 | 0 | 28 | 0 | 0 | 0 | — |
| Image::ExifTool::Canon::AFInfo | 14 | 14 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Canon::AFInfo2 | 16 | 16 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Canon::AFMicroAdj | 2 | 2 | 0 | 2 | 0 | 0 | 0 | — |
| Image::ExifTool::Canon::Ambience | 1 | 1 | 0 | 1 | 0 | 0 | 0 | — |
| Image::ExifTool::Canon::AspectInfo | 5 | 5 | 0 | 5 | 0 | 0 | 0 | — |
| Image::ExifTool::Canon::CCTP | 0 | 0 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Canon::CDI1 | 1 | 0 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Canon::CMP1 | 2 | 2 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Canon::CNOP | 0 | 0 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Canon::CNTH | 1 | 1 | 0 | 1 | 0 | 0 | 0 | raw_key_unrepresentable: 1 |
| Image::ExifTool::Canon::CTMD | 6 | 1 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Canon::CameraInfo1000D | 21 | 20 | 0 | 20 | 0 | 0 | 0 | — |
| Image::ExifTool::Canon::CameraInfo1D | 15 | 15 | 0 | 15 | 0 | 0 | 0 | — |
| Image::ExifTool::Canon::CameraInfo1DX | 19 | 17 | 0 | 16 | 0 | 0 | 0 | — |
| Image::ExifTool::Canon::CameraInfo1DmkII | 16 | 16 | 0 | 16 | 0 | 0 | 0 | — |
| Image::ExifTool::Canon::CameraInfo1DmkIII | 22 | 21 | 0 | 21 | 0 | 0 | 0 | — |
| Image::ExifTool::Canon::CameraInfo1DmkIIN | 13 | 13 | 0 | 13 | 0 | 0 | 0 | — |
| Image::ExifTool::Canon::CameraInfo1DmkIV | 22 | 20 | 0 | 19 | 0 | 0 | 0 | — |
| Image::ExifTool::Canon::CameraInfo40D | 20 | 19 | 0 | 19 | 0 | 0 | 0 | — |
| Image::ExifTool::Canon::CameraInfo450D | 19 | 18 | 0 | 18 | 0 | 0 | 0 | — |
| Image::ExifTool::Canon::CameraInfo500D | 22 | 21 | 0 | 20 | 0 | 0 | 0 | — |
| Image::ExifTool::Canon::CameraInfo50D | 23 | 21 | 0 | 20 | 0 | 0 | 0 | — |
| Image::ExifTool::Canon::CameraInfo550D | 20 | 19 | 0 | 18 | 0 | 0 | 0 | — |
| Image::ExifTool::Canon::CameraInfo5D | 59 | 59 | 0 | 59 | 0 | 0 | 0 | — |
| Image::ExifTool::Canon::CameraInfo5DmkII | 26 | 24 | 0 | 23 | 0 | 0 | 0 | — |
| Image::ExifTool::Canon::CameraInfo5DmkIII | 22 | 20 | 0 | 19 | 0 | 0 | 0 | — |
| Image::ExifTool::Canon::CameraInfo600D | 20 | 19 | 0 | 18 | 0 | 0 | 0 | — |
| Image::ExifTool::Canon::CameraInfo60D | 18 | 16 | 0 | 15 | 0 | 0 | 0 | — |
| Image::ExifTool::Canon::CameraInfo650D | 21 | 20 | 0 | 18 | 0 | 0 | 0 | — |
| Image::ExifTool::Canon::CameraInfo6D | 18 | 17 | 0 | 16 | 0 | 0 | 0 | — |
| Image::ExifTool::Canon::CameraInfo70D | 16 | 15 | 0 | 14 | 0 | 0 | 0 | — |
| Image::ExifTool::Canon::CameraInfo750D | 16 | 16 | 0 | 14 | 0 | 0 | 0 | — |
| Image::ExifTool::Canon::CameraInfo7D | 24 | 22 | 0 | 21 | 0 | 0 | 0 | — |
| Image::ExifTool::Canon::CameraInfo80D | 15 | 15 | 0 | 14 | 0 | 0 | 0 | — |
| Image::ExifTool::Canon::CameraInfoG5XII | 4 | 4 | 0 | 4 | 0 | 0 | 0 | — |
| Image::ExifTool::Canon::CameraInfoPowerShot | 6 | 6 | 0 | 6 | 0 | 0 | 0 | — |
| Image::ExifTool::Canon::CameraInfoPowerShot2 | 9 | 9 | 0 | 9 | 0 | 0 | 0 | — |
| Image::ExifTool::Canon::CameraInfoR6 | 2 | 2 | 0 | 2 | 0 | 0 | 0 | — |
| Image::ExifTool::Canon::CameraInfoR6m2 | 1 | 1 | 0 | 1 | 0 | 0 | 0 | — |
| Image::ExifTool::Canon::CameraInfoR6m3 | 1 | 1 | 0 | 1 | 0 | 0 | 0 | — |
| Image::ExifTool::Canon::CameraInfoUnknown | 2 | 2 | 0 | 1 | 0 | 0 | 0 | — |
| Image::ExifTool::Canon::CameraInfoUnknown16 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Canon::CameraInfoUnknown32 | 6 | 6 | 0 | 6 | 0 | 0 | 0 | — |
| Image::ExifTool::Canon::CameraSettings | 40 | 40 | 0 | 40 | 0 | 0 | 0 | — |
| Image::ExifTool::Canon::ColorBalance | 11 | 11 | 0 | 11 | 0 | 0 | 0 | — |
| Image::ExifTool::Canon::ColorCalib | 15 | 15 | 0 | 15 | 0 | 0 | 0 | — |
| Image::ExifTool::Canon::ColorCalib2 | 15 | 15 | 0 | 15 | 0 | 0 | 0 | — |
| Image::ExifTool::Canon::ColorCoefs | 46 | 46 | 0 | 46 | 0 | 0 | 0 | — |
| Image::ExifTool::Canon::ColorCoefs2 | 46 | 46 | 0 | 46 | 0 | 0 | 0 | — |
| Image::ExifTool::Canon::ColorData1 | 21 | 20 | 0 | 20 | 0 | 0 | 0 | — |
| Image::ExifTool::Canon::ColorData10 | 86 | 85 | 0 | 85 | 0 | 0 | 0 | — |
| Image::ExifTool::Canon::ColorData11 | 82 | 81 | 0 | 81 | 0 | 0 | 0 | — |
| Image::ExifTool::Canon::ColorData12 | 86 | 85 | 0 | 85 | 0 | 0 | 0 | — |
| Image::ExifTool::Canon::ColorData2 | 58 | 57 | 0 | 57 | 0 | 0 | 0 | — |
| Image::ExifTool::Canon::ColorData3 | 35 | 34 | 0 | 34 | 0 | 0 | 0 | — |
| Image::ExifTool::Canon::ColorData4 | 19 | 17 | 0 | 17 | 0 | 0 | 0 | — |
| Image::ExifTool::Canon::ColorData5 | 10 | 6 | 0 | 6 | 0 | 0 | 0 | — |
| Image::ExifTool::Canon::ColorData6 | 58 | 57 | 0 | 57 | 0 | 0 | 0 | — |
| Image::ExifTool::Canon::ColorData7 | 75 | 74 | 0 | 74 | 0 | 0 | 0 | — |
| Image::ExifTool::Canon::ColorData8 | 91 | 90 | 0 | 90 | 0 | 0 | 0 | — |
| Image::ExifTool::Canon::ColorData9 | 84 | 83 | 0 | 83 | 0 | 0 | 0 | — |
| Image::ExifTool::Canon::ColorDataUnknown | 1 | 1 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Canon::ColorInfo | 3 | 3 | 0 | 3 | 0 | 0 | 0 | — |
| Image::ExifTool::Canon::ContrastInfo | 1 | 1 | 0 | 1 | 0 | 0 | 0 | — |
| Image::ExifTool::Canon::CropInfo | 4 | 4 | 0 | 4 | 0 | 0 | 0 | — |
| Image::ExifTool::Canon::ExifInfo | 2 | 0 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Canon::ExposureInfo | 3 | 3 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Canon::FaceDetect1 | 11 | 11 | 0 | 11 | 0 | 0 | 0 | — |
| Image::ExifTool::Canon::FaceDetect2 | 2 | 2 | 0 | 2 | 0 | 0 | 0 | — |
| Image::ExifTool::Canon::FaceDetect3 | 1 | 1 | 0 | 1 | 0 | 0 | 0 | — |
| Image::ExifTool::Canon::FileInfo | 23 | 23 | 0 | 23 | 0 | 0 | 0 | — |
| Image::ExifTool::Canon::FilterInfo | 10 | 10 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Canon::Flags | 1 | 1 | 0 | 1 | 0 | 0 | 0 | — |
| Image::ExifTool::Canon::FocalInfo | 1 | 1 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Canon::FocalLength | 6 | 6 | 0 | 6 | 0 | 0 | 0 | — |
| Image::ExifTool::Canon::FocusBracketingInfo | 7 | 7 | 0 | 7 | 0 | 0 | 0 | — |
| Image::ExifTool::Canon::HDRInfo | 2 | 2 | 0 | 2 | 0 | 0 | 0 | — |
| Image::ExifTool::Canon::IAD1 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Canon::LensInfo | 1 | 1 | 0 | 1 | 0 | 0 | 0 | — |
| Image::ExifTool::Canon::LevelInfo | 5 | 5 | 0 | 5 | 0 | 0 | 0 | — |
| Image::ExifTool::Canon::LightingOpt | 7 | 7 | 0 | 7 | 0 | 0 | 0 | — |
| Image::ExifTool::Canon::LogInfo | 7 | 7 | 0 | 7 | 0 | 0 | 0 | — |
| Image::ExifTool::Canon::Main | 146 | 39 | 20 | 25 | 0 | 0 | 0 | ifd_isoffset_unsupported: 2; print_conv: 7; raw_conv: 2; value_conv: 8 |
| Image::ExifTool::Canon::MeasuredColor | 1 | 1 | 0 | 1 | 0 | 0 | 0 | — |
| Image::ExifTool::Canon::ModifiedInfo | 11 | 11 | 0 | 11 | 0 | 0 | 0 | — |
| Image::ExifTool::Canon::MovieInfo | 9 | 9 | 0 | 9 | 0 | 0 | 0 | — |
| Image::ExifTool::Canon::MultiExp | 3 | 3 | 0 | 3 | 0 | 0 | 0 | — |
| Image::ExifTool::Canon::MyColors | 1 | 1 | 0 | 1 | 0 | 0 | 0 | — |
| Image::ExifTool::Canon::PSInfo | 57 | 57 | 0 | 57 | 0 | 0 | 0 | — |
| Image::ExifTool::Canon::PSInfo2 | 63 | 63 | 0 | 63 | 0 | 0 | 0 | — |
| Image::ExifTool::Canon::Panorama | 2 | 2 | 0 | 2 | 0 | 0 | 0 | — |
| Image::ExifTool::Canon::PreviewImageInfo | 5 | 5 | 0 | 3 | 0 | 0 | 0 | — |
| Image::ExifTool::Canon::Processing | 15 | 15 | 0 | 15 | 0 | 0 | 0 | — |
| Image::ExifTool::Canon::RawBurstInfo | 2 | 2 | 0 | 2 | 0 | 0 | 0 | — |
| Image::ExifTool::Canon::SensorInfo | 10 | 10 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Canon::SerialInfo | 2 | 2 | 0 | 2 | 0 | 0 | 0 | — |
| Image::ExifTool::Canon::ShotInfo | 29 | 29 | 0 | 29 | 0 | 0 | 0 | — |
| Image::ExifTool::Canon::Skip | 1 | 1 | 0 | 0 | 0 | 0 | 0 | raw_key_unrepresentable: 1 |
| Image::ExifTool::Canon::TimeInfo | 3 | 3 | 0 | 3 | 0 | 0 | 0 | — |
| Image::ExifTool::Canon::UnknownD30 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Canon::VignettingCorr | 9 | 9 | 0 | 8 | 0 | 0 | 0 | — |
| Image::ExifTool::Canon::VignettingCorr2 | 4 | 4 | 0 | 4 | 0 | 0 | 0 | — |
| Image::ExifTool::Canon::VignettingCorrUnknown | 1 | 1 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Canon::WBInfo | 10 | 10 | 0 | 10 | 0 | 0 | 0 | — |
| Image::ExifTool::Canon::uuid | 9 | 2 | 0 | 0 | 0 | 0 | 0 | raw_key_unrepresentable: 2 |
| Image::ExifTool::Canon::uuid2 | 1 | 0 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::CanonCustom::FuncsUnknown | 0 | 0 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::CanonCustom::Functions10D | 17 | 17 | 0 | 17 | 0 | 0 | 0 | — |
| Image::ExifTool::CanonCustom::Functions1D | 22 | 22 | 0 | 22 | 0 | 0 | 0 | — |
| Image::ExifTool::CanonCustom::Functions2 | 145 | 145 | 0 | 145 | 0 | 0 | 0 | — |
| Image::ExifTool::CanonCustom::Functions20D | 18 | 18 | 0 | 18 | 0 | 0 | 0 | — |
| Image::ExifTool::CanonCustom::Functions30D | 19 | 19 | 0 | 19 | 0 | 0 | 0 | — |
| Image::ExifTool::CanonCustom::Functions350D | 9 | 9 | 0 | 9 | 0 | 0 | 0 | — |
| Image::ExifTool::CanonCustom::Functions400D | 11 | 11 | 0 | 11 | 0 | 0 | 0 | — |
| Image::ExifTool::CanonCustom::Functions5D | 21 | 21 | 0 | 21 | 0 | 0 | 0 | — |
| Image::ExifTool::CanonCustom::FunctionsD30 | 15 | 15 | 0 | 15 | 0 | 0 | 0 | — |
| Image::ExifTool::CanonCustom::PersonalFuncValues | 24 | 24 | 0 | 24 | 0 | 0 | 0 | — |
| Image::ExifTool::CanonCustom::PersonalFuncs | 29 | 29 | 0 | 29 | 0 | 0 | 0 | — |
| Image::ExifTool::CanonRaw::DecoderTable | 3 | 3 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::CanonRaw::ExposureInfo | 3 | 3 | 0 | 3 | 0 | 0 | 0 | — |
| Image::ExifTool::CanonRaw::FlashInfo | 2 | 2 | 0 | 2 | 0 | 0 | 0 | — |
| Image::ExifTool::CanonRaw::ImageFormat | 2 | 2 | 0 | 2 | 0 | 0 | 0 | — |
| Image::ExifTool::CanonRaw::ImageInfo | 7 | 7 | 0 | 1 | 0 | 0 | 0 | — |
| Image::ExifTool::CanonRaw::Main | 61 | 34 | 0 | 31 | 0 | 0 | 0 | — |
| Image::ExifTool::CanonRaw::MakeModel | 2 | 2 | 0 | 2 | 0 | 0 | 0 | — |
| Image::ExifTool::CanonRaw::RawJpgInfo | 4 | 4 | 0 | 4 | 0 | 0 | 0 | — |
| Image::ExifTool::CanonRaw::TimeStamp | 3 | 3 | 0 | 3 | 0 | 0 | 0 | — |
| Image::ExifTool::CanonRaw::WhiteSample | 6 | 6 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::CanonVRD::CropInfo | 11 | 11 | 0 | 11 | 0 | 0 | 0 | — |
| Image::ExifTool::CanonVRD::DLOInfo | 3 | 3 | 0 | 2 | 0 | 0 | 0 | — |
| Image::ExifTool::CanonVRD::DR4 | 75 | 69 | 0 | 69 | 0 | 0 | 0 | — |
| Image::ExifTool::CanonVRD::DR4Header | 1 | 1 | 0 | 1 | 0 | 0 | 0 | — |
| Image::ExifTool::CanonVRD::DustInfo | 1 | 1 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::CanonVRD::Edit | 3 | 0 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::CanonVRD::Edit4 | 1 | 0 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::CanonVRD::GammaInfo | 13 | 13 | 0 | 13 | 0 | 0 | 0 | — |
| Image::ExifTool::CanonVRD::IHL | 6 | 5 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::CanonVRD::Main | 4 | 0 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::CanonVRD::StampInfo | 1 | 1 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::CanonVRD::StampTool | 1 | 1 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::CanonVRD::ToneCurve | 10 | 10 | 0 | 10 | 0 | 0 | 0 | — |
| Image::ExifTool::CanonVRD::Ver1 | 43 | 43 | 0 | 42 | 0 | 0 | 0 | — |
| Image::ExifTool::CanonVRD::Ver2 | 153 | 152 | 0 | 150 | 0 | 0 | 0 | — |
| Image::ExifTool::Casio::AVI | 1 | 1 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Casio::FaceInfo1 | 12 | 12 | 0 | 12 | 0 | 0 | 0 | — |
| Image::ExifTool::Casio::FaceInfo2 | 13 | 13 | 0 | 13 | 0 | 0 | 0 | — |
| Image::ExifTool::Casio::Main | 19 | 18 | 17 | 18 | 0 | 0 | 0 | print_conv: 1 |
| Image::ExifTool::Casio::QVCI | 6 | 6 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Casio::Type2 | 78 | 75 | 65 | 73 | 0 | 0 | 0 | ifd_isoffset_unsupported: 3; print_conv: 5; tag_variant_cond_unsupported: 1; value_conv: 1 |
| Image::ExifTool::Composite | 111 | 111 | 0 | 14 | 0 | 0 | 0 | — |
| Image::ExifTool::DICOM::Main | 5669 | 5669 | 0 | 0 | 0 | 0 | 0 | raw_key_unrepresentable: 5669 |
| Image::ExifTool::DJI::DroneInfo | 3 | 3 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::DJI::FrameInfo | 3 | 3 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::DJI::GPSInfo | 3 | 3 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::DJI::GimbalInfo | 3 | 3 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::DJI::Glamour | 15 | 15 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::DJI::Info | 15 | 15 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::DJI::Main | 10 | 10 | 10 | 10 | 0 | 0 | 0 | — |
| Image::ExifTool::DJI::Protobuf | 198 | 152 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::DJI::ThermalParams | 16 | 16 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::DJI::ThermalParams2 | 6 | 6 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::DJI::ThermalParams3 | 4 | 4 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::DJI::XMP | 28 | 28 | 0 | 28 | 0 | 0 | 0 | raw_key_unrepresentable: 28 |
| Image::ExifTool::DNG::AdobeData | 101 | 4 | 0 | 4 | 0 | 0 | 0 | — |
| Image::ExifTool::DNG::ImageSeq | 6 | 6 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::DNG::OriginalRaw | 8 | 8 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::DNG::ProfileDynamicRange | 3 | 3 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::DPX::Main | 40 | 40 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::DSF::Main | 8 | 8 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::DV::Main | 13 | 13 | 0 | 0 | 0 | 0 | 0 | raw_key_unrepresentable: 13 |
| Image::ExifTool::DarwinCore::Main | 263 | 263 | 0 | 263 | 0 | 0 | 0 | raw_key_unrepresentable: 37 |
| Image::ExifTool::DjVu::Ant | 2 | 0 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::DjVu::Form | 1 | 1 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::DjVu::Info | 6 | 6 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::DjVu::Main | 5 | 1 | 0 | 0 | 0 | 0 | 0 | raw_key_unrepresentable: 1 |
| Image::ExifTool::DjVu::Meta | 34 | 34 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::EXE::AR | 1 | 1 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::EXE::CHM | 2 | 2 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::EXE::DebugNB10 | 4 | 4 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::EXE::DebugRSDS | 3 | 3 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::EXE::ELF | 4 | 4 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::EXE::MachO | 7 | 7 | 5 | 0 | 0 | 0 | 0 | print_conv: 2 |
| Image::ExifTool::EXE::Main | 13 | 13 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::EXE::Misc | 1 | 1 | 1 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::EXE::PEF | 3 | 3 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::EXE::PEString | 17 | 17 | 0 | 0 | 0 | 0 | 0 | raw_key_unrepresentable: 17 |
| Image::ExifTool::EXE::PEVersion | 7 | 7 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Exif::Main | 727 | 602 | 469 | 314 | 19 | 0 | 0 | ifd_isoffset_unsupported: 14; print_conv: 25; raw_conv: 18; value_conv: 43; variant_makernotes_dispatch: 4; variant_unreported_skipped: 35 |
| Image::ExifTool::Extra | 91 | 91 | 0 | 29 | 0 | 0 | 0 | — |
| Image::ExifTool::FITS::Main | 14 | 14 | 0 | 0 | 0 | 0 | 0 | raw_key_unrepresentable: 14 |
| Image::ExifTool::FLAC::Main | 8 | 4 | 3 | 0 | 0 | 0 | 0 | tag_variant_cond_unsupported: 1 |
| Image::ExifTool::FLAC::Picture | 9 | 9 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::FLAC::StreamInfo | 9 | 9 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::FLIF::Main | 9 | 6 | 6 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::FLIR::AFF | 3 | 0 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::FLIR::AFF1 | 3 | 2 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::FLIR::AFF5 | 3 | 2 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::FLIR::CameraInfo | 45 | 44 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::FLIR::CoarseData | 5 | 4 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::FLIR::EmbeddedImage | 5 | 4 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::FLIR::FFF | 15 | 1 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::FLIR::FPF | 36 | 36 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::FLIR::GPSInfo | 15 | 15 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::FLIR::GPS_UUID | 3 | 3 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::FLIR::GainDeadData | 5 | 4 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::FLIR::Header | 1 | 1 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::FLIR::Main | 6 | 6 | 6 | 6 | 0 | 0 | 0 | — |
| Image::ExifTool::FLIR::MeasInfo | 3 | 3 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::FLIR::MeterLink | 16 | 16 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::FLIR::MoreInfo | 3 | 3 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::FLIR::PaintData | 5 | 4 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::FLIR::PaletteInfo | 12 | 12 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::FLIR::ParamInfo | 5 | 5 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::FLIR::Params | 8 | 8 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::FLIR::Parts | 15 | 15 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::FLIR::PiP | 7 | 7 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::FLIR::RawData | 5 | 4 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::FLIR::SerialNums | 6 | 6 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::FLIR::TextInfo | 8 | 8 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::FLIR::UnknownUUID | 0 | 0 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::FLIR::UserData | 10 | 4 | 0 | 0 | 0 | 0 | 0 | raw_key_unrepresentable: 4 |
| Image::ExifTool::Flash::Audio | 4 | 4 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Flash::CuePoint | 4 | 3 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Flash::FLV | 3 | 0 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Flash::Main | 9 | 8 | 1 | 0 | 0 | 0 | 0 | raw_key_unrepresentable: 7 |
| Image::ExifTool::Flash::Meta | 41 | 39 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Flash::Parameter | 0 | 0 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Flash::Video | 1 | 1 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::FlashPix::AudioInfo | 0 | 0 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::FlashPix::CompObj | 2 | 2 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::FlashPix::DOP | 10 | 10 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::FlashPix::DataObject | 13 | 13 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::FlashPix::DocTable | 7 | 3 | 0 | 0 | 0 | 0 | 0 | raw_key_unrepresentable: 3 |
| Image::ExifTool::FlashPix::DocumentInfo | 25 | 25 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::FlashPix::Extensions | 15 | 15 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::FlashPix::GlobalInfo | 7 | 7 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::FlashPix::Image | 15 | 15 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::FlashPix::ImageInfo | 69 | 69 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::FlashPix::Main | 29 | 11 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::FlashPix::Operation | 1 | 1 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::FlashPix::PreviewInfo | 2 | 2 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::FlashPix::SubimageHdr | 6 | 6 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::FlashPix::SummaryInfo | 23 | 23 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::FlashPix::Transform | 18 | 18 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::FlashPix::WordDocument | 5 | 5 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Font::AFM | 18 | 18 | 0 | 0 | 0 | 0 | 0 | raw_key_unrepresentable: 18 |
| Image::ExifTool::Font::Main | 10 | 4 | 0 | 0 | 0 | 0 | 0 | raw_key_unrepresentable: 4 |
| Image::ExifTool::Font::Name | 22 | 22 | 22 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Font::PFM | 24 | 24 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Font::PSInfo | 13 | 13 | 0 | 0 | 0 | 0 | 0 | raw_key_unrepresentable: 13 |
| Image::ExifTool::Font::XML | 26 | 26 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::FotoStation::Main | 4 | 2 | 0 | 2 | 0 | 0 | 0 | — |
| Image::ExifTool::FotoStation::SoftEdit | 10 | 10 | 0 | 10 | 0 | 0 | 0 | — |
| Image::ExifTool::FujiFilm::AFCSettings | 4 | 4 | 0 | 4 | 0 | 0 | 0 | — |
| Image::ExifTool::FujiFilm::DriveSettings | 2 | 2 | 0 | 2 | 0 | 0 | 0 | — |
| Image::ExifTool::FujiFilm::FFMV | 1 | 1 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::FujiFilm::FaceRecInfo | 24 | 24 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::FujiFilm::FocusSettings | 5 | 5 | 0 | 5 | 0 | 0 | 0 | — |
| Image::ExifTool::FujiFilm::IFD | 13 | 12 | 10 | 0 | 0 | 0 | 0 | ifd_isoffset_unsupported: 2 |
| Image::ExifTool::FujiFilm::MOV | 5 | 5 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::FujiFilm::MRAW | 6 | 4 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::FujiFilm::Main | 103 | 98 | 91 | 98 | 0 | 0 | 0 | print_conv: 6; raw_conv: 1 |
| Image::ExifTool::FujiFilm::PrioritySettings | 2 | 2 | 0 | 2 | 0 | 0 | 0 | — |
| Image::ExifTool::FujiFilm::RAF | 25 | 24 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::FujiFilm::RAFData | 6 | 6 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::FujiFilm::RAFHeader | 2 | 2 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::GE::Main | 3 | 3 | 3 | 3 | 0 | 0 | 0 | — |
| Image::ExifTool::GIF::Animation | 1 | 1 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::GIF::Extensions | 6 | 1 | 0 | 0 | 0 | 0 | 0 | raw_key_unrepresentable: 1 |
| Image::ExifTool::GIF::MIDIControl | 6 | 6 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::GIF::Main | 8 | 6 | 0 | 1 | 0 | 0 | 0 | raw_key_unrepresentable: 6 |
| Image::ExifTool::GIF::Screen | 7 | 7 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::GIMP::Header | 4 | 4 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::GIMP::Main | 6 | 3 | 3 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::GIMP::Parasite | 8 | 2 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::GIMP::Resolution | 2 | 2 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::GM::marl | 76 | 76 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::GM::mrld | 15 | 15 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::GM::mrlh | 1 | 1 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::GM::mrlv | 9 | 9 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::GPS::Main | 32 | 32 | 17 | 32 | 0 | 0 | 0 | print_conv: 12; raw_conv: 3; value_conv: 6 |
| Image::ExifTool::Garmin::AADAccelFeatures | 5 | 5 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::AccelerometerData | 11 | 11 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::Activity | 7 | 7 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::ActivityMetrics | 19 | 19 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::AlarmSettings | 8 | 8 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::Alert | 5 | 5 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::AntChannelID | 5 | 5 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::AntRx | 5 | 5 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::AntTx | 5 | 5 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::AviationAttitude | 11 | 11 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::BarometerData | 3 | 3 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::BeatIntervals | 2 | 2 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::BestEffort | 5 | 5 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::BikeProfile | 31 | 31 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::BloodPressure | 10 | 10 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::CPEStatus | 3 | 3 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::CadenceZone | 2 | 2 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::CameraEvent | 4 | 4 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::Capabilities | 4 | 4 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::ChronoShotData | 2 | 2 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::ChronoShotSession | 7 | 7 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::ClimbPro | 6 | 6 | 6 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::Clubs | 2 | 2 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::Common | 3 | 3 | 3 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::ConnectIQField | 4 | 4 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::Connectivity | 13 | 13 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::Course | 4 | 4 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::CoursePoint | 7 | 7 | 7 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::DataScreen | 5 | 5 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::DeveloperDataID | 5 | 5 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::DeviceAuxBatteryInfo | 4 | 4 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::DeviceInfo | 19 | 19 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::DeviceSettings | 27 | 27 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::DeviceStatus | 3 | 3 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::DeviceUsed | 6 | 6 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::DiveAlarm | 12 | 12 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::DiveApneaAlarm | 12 | 12 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::DiveGas | 4 | 4 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::DiveSettings | 33 | 33 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::DiveSummary | 22 | 22 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::ECGRawSample | 1 | 1 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::ECGSmoothSample | 1 | 1 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::ECGSummary | 7 | 7 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::EPOStatus | 3 | 3 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::EnduranceScore | 8 | 8 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::Event | 18 | 18 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::ExdDataConceptConfiguration | 11 | 11 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::ExdDataFieldConfiguration | 6 | 6 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::ExdScreenConfiguration | 4 | 4 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::ExerciseTitle | 3 | 3 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::FIT | 173 | 2 | 2 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::FieldCapabilities | 4 | 4 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::FieldDescription | 14 | 14 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::FileCapabilities | 5 | 5 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::FileCreator | 2 | 2 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::FileID | 7 | 7 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::FunctionalMetrics | 4 | 4 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::GPS | 8 | 8 | 7 | 0 | 0 | 0 | 0 | value_conv_uncompiled: 1 |
| Image::ExifTool::Garmin::GPSEvent | 13 | 13 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::Goal | 12 | 12 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::GolfCourse | 11 | 11 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::GolfStats | 7 | 7 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::GyroscopeData | 8 | 8 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::HR | 5 | 5 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::HRMProfile | 4 | 4 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::HRV | 1 | 1 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::HRVStatusSummary | 7 | 7 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::HRVValue | 1 | 1 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::HRZone | 2 | 2 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::HSAAccelerometerData | 6 | 6 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::HSABodyBatteryData | 4 | 4 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::HSAConfigurationData | 2 | 2 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::HSAEvent | 1 | 1 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::HSAGyroscopeData | 6 | 6 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::HSAHeartRateData | 3 | 3 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::HSARespirationData | 2 | 2 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::HSAStepData | 2 | 2 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::HSAStressData | 2 | 2 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::HSAWristTemperatureData | 2 | 2 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::HSA_SPO2Data | 3 | 3 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::HillScore | 4 | 4 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::Hole | 6 | 6 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::Jump | 9 | 9 | 9 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::Lap | 136 | 136 | 136 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::Length | 20 | 20 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::Location | 7 | 7 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::MagnetometerData | 8 | 8 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::MapLayer | 13 | 13 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::MaxMetData | 8 | 8 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::MemoGlob | 5 | 5 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::MesgCapabilities | 4 | 4 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::MetZone | 3 | 3 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::Metronome | 4 | 4 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::Monitoring | 28 | 28 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::MonitoringHRData | 2 | 2 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::MonitoringInfo | 5 | 5 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::MtbCx | 2 | 2 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::MultisportActivity | 4 | 4 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::MultisportSettings | 10 | 10 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::MusicInfo | 5 | 5 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::NMEASentence | 2 | 2 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::NapEvent | 8 | 8 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::OBDIIData | 8 | 8 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::OHRSettings | 1 | 1 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::OneDSensorCalibration | 5 | 5 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::OpenWaterEvent | 2 | 2 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::PersonalRecord | 4 | 4 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::PowerMode | 3 | 3 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::PowerZone | 2 | 2 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::Race | 4 | 4 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::RaceEvent | 11 | 11 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::RangeAlert | 5 | 5 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::RawBBI | 5 | 5 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::Record | 94 | 94 | 94 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::RespirationRate | 1 | 1 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::Routing | 7 | 7 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::SDMProfile | 7 | 7 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::SPO2Data | 3 | 3 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::Schedule | 7 | 7 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::Score | 4 | 4 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::SegmentFile | 8 | 8 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::SegmentID | 9 | 9 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::SegmentLap | 93 | 93 | 93 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::SegmentLeaderboardEntry | 6 | 6 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::SegmentPoint | 6 | 6 | 6 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::SensorSettings | 12 | 12 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::Session | 179 | 179 | 179 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::Set | 10 | 10 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::Shot | 6 | 6 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::SkinTempOvernight | 4 | 4 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::SlaveDevice | 2 | 2 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::SleepAssessment | 14 | 14 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::SleepDataInfo | 3 | 3 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::SleepDisruptionOvernightSeverity | 1 | 1 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::SleepDisruptionSeverityPeriod | 1 | 1 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::SleepLevel | 1 | 1 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::SleepRestlessMoments | 2 | 2 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::SleepSchedule | 2 | 2 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::Software | 2 | 2 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::SpeedZone | 2 | 2 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::Split | 42 | 42 | 42 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::SplitSummary | 25 | 25 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::SplitTime | 11 | 11 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::Sport | 10 | 10 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::StressLevel | 3 | 3 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::TankSummary | 4 | 4 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::TankUpdate | 2 | 2 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::ThreeDSensorCalibration | 6 | 6 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::TimeInZone | 16 | 16 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::TimeStampCorrelation | 6 | 6 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::Totals | 9 | 9 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::TrainingFile | 5 | 5 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::TrainingLoad | 2 | 2 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::TrainingReadiness | 3 | 3 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::TrainingSettings | 47 | 47 | 0 | 0 | 0 | 0 | 0 | field_key_outside_u8_protocol: 4 |
| Image::ExifTool::Garmin::UserMetrics | 16 | 16 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::UserProfile | 32 | 32 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::Video | 3 | 3 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::VideoClip | 7 | 7 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::VideoDescription | 2 | 2 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::VideoFrame | 2 | 2 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::VideoTitle | 2 | 2 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::WatchfaceSettings | 2 | 2 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::WaypointHandling | 1 | 1 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::WeatherAlert | 5 | 5 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::WeatherConditions | 15 | 15 | 15 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::WeightScale | 13 | 13 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::Workout | 13 | 13 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::WorkoutSchedule | 6 | 6 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::WorkoutSession | 6 | 6 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::WorkoutStep | 20 | 20 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Garmin::ZonesTarget | 5 | 5 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::GeoTiff::Main | 64 | 64 | 64 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::GoPro::GLPI | 9 | 8 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::GoPro::GPMF | 122 | 115 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::GoPro::GPRI | 10 | 6 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::GoPro::GPS5 | 5 | 5 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::GoPro::GPS9 | 9 | 8 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::GoPro::KBAT | 15 | 9 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::GoPro::fdsc | 4 | 4 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Google::Device | 107 | 107 | 0 | 107 | 0 | 0 | 0 | raw_key_unrepresentable: 8 |
| Image::ExifTool::Google::GAudio | 2 | 2 | 0 | 2 | 0 | 0 | 0 | raw_key_unrepresentable: 2 |
| Image::ExifTool::Google::GCamera | 18 | 14 | 0 | 14 | 0 | 0 | 0 | raw_key_unrepresentable: 14 |
| Image::ExifTool::Google::GContainer | 8 | 8 | 0 | 8 | 0 | 0 | 0 | raw_key_unrepresentable: 1 |
| Image::ExifTool::Google::GCreations | 2 | 2 | 0 | 2 | 0 | 0 | 0 | raw_key_unrepresentable: 2 |
| Image::ExifTool::Google::GDepth | 14 | 14 | 0 | 14 | 0 | 0 | 0 | raw_key_unrepresentable: 14 |
| Image::ExifTool::Google::GFocus | 4 | 4 | 0 | 4 | 0 | 0 | 0 | raw_key_unrepresentable: 4 |
| Image::ExifTool::Google::GImage | 2 | 2 | 0 | 2 | 0 | 0 | 0 | raw_key_unrepresentable: 2 |
| Image::ExifTool::Google::GPano | 27 | 27 | 0 | 27 | 0 | 0 | 0 | raw_key_unrepresentable: 27 |
| Image::ExifTool::Google::GSpherical | 16 | 16 | 0 | 16 | 0 | 0 | 0 | raw_key_unrepresentable: 16 |
| Image::ExifTool::Google::HDRPMakerNote | 15 | 15 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Google::HDRPlusMakerNote | 20 | 20 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Google::ShotLogData | 2 | 2 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::H264::Camera1 | 5 | 5 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::H264::Camera2 | 1 | 1 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::H264::FrameInfo | 2 | 2 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::H264::MDPM | 39 | 33 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::H264::Main | 3 | 2 | 0 | 0 | 0 | 0 | 0 | raw_key_unrepresentable: 2 |
| Image::ExifTool::H264::MakeModel | 1 | 1 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::H264::RecInfo | 1 | 1 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::H264::Shutter | 1 | 1 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::HP::Main | 1 | 0 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::HP::TDHD | 4 | 2 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::HP::Type2 | 3 | 3 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::HP::Type4 | 5 | 5 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::HP::Type6 | 5 | 5 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::HTML::Main | 31 | 25 | 0 | 0 | 0 | 0 | 0 | raw_key_unrepresentable: 25 |
| Image::ExifTool::HTML::Office | 21 | 21 | 0 | 0 | 0 | 0 | 0 | raw_key_unrepresentable: 21 |
| Image::ExifTool::HTML::dc | 15 | 15 | 0 | 0 | 0 | 0 | 0 | raw_key_unrepresentable: 15 |
| Image::ExifTool::HTML::equiv | 22 | 22 | 0 | 0 | 0 | 0 | 0 | raw_key_unrepresentable: 22 |
| Image::ExifTool::HTML::ncc | 26 | 26 | 0 | 0 | 0 | 0 | 0 | raw_key_unrepresentable: 26 |
| Image::ExifTool::HTML::prod | 2 | 2 | 0 | 0 | 0 | 0 | 0 | raw_key_unrepresentable: 2 |
| Image::ExifTool::HTML::vw96 | 1 | 1 | 0 | 0 | 0 | 0 | 0 | raw_key_unrepresentable: 1 |
| Image::ExifTool::ICC_Profile::Chromaticity | 6 | 6 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::ICC_Profile::ColorRep | 4 | 4 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::ICC_Profile::ColorantTable | 7 | 7 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::ICC_Profile::Header | 16 | 16 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::ICC_Profile::Main | 158 | 151 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::ICC_Profile::Measurement | 5 | 5 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::ICC_Profile::Metadata | 4 | 4 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::ICC_Profile::ViewingConditions | 3 | 3 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::ICO::IconDir | 8 | 8 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::ICO::Main | 3 | 2 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::ID3::GEOB | 5 | 4 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::ID3::Lyrics3 | 9 | 9 | 0 | 0 | 0 | 0 | 0 | raw_key_unrepresentable: 9 |
| Image::ExifTool::ID3::Main | 5 | 0 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::ID3::Private | 9 | 8 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::ID3::SynLyrics | 3 | 3 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::ID3::v1 | 7 | 7 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::ID3::v1_Enh | 7 | 7 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::ID3::v2_2 | 65 | 64 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::ID3::v2_3 | 81 | 77 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::ID3::v2_4 | 88 | 84 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::IPTC::ApplicationRecord | 70 | 70 | 10 | 70 | 0 | 0 | 0 | ifd_format_unsupported: 56; print_conv: 2; value_conv: 4 |
| Image::ExifTool::IPTC::EnvelopeRecord | 14 | 14 | 5 | 14 | 0 | 0 | 0 | ifd_format_unsupported: 8; value_conv: 1 |
| Image::ExifTool::IPTC::FotoStation | 0 | 0 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::IPTC::Main | 7 | 0 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::IPTC::NewsPhoto | 26 | 26 | 25 | 21 | 0 | 0 | 0 | print_conv: 1 |
| Image::ExifTool::IPTC::ObjectData | 1 | 1 | 1 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::IPTC::PostObjectData | 1 | 1 | 1 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::IPTC::PreObjectData | 4 | 4 | 4 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::ISO::BootRecord | 2 | 2 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::ISO::Main | 2 | 0 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::ISO::PrimaryVolume | 20 | 20 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::ITC::Header | 1 | 1 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::ITC::Item | 6 | 6 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::ITC::Main | 3 | 1 | 0 | 0 | 0 | 0 | 0 | raw_key_unrepresentable: 1 |
| Image::ExifTool::InfiRay::Factory | 20 | 20 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::InfiRay::Isothermal | 4 | 4 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::InfiRay::MixMode | 4 | 4 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::InfiRay::OpMode | 6 | 6 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::InfiRay::Picture | 10 | 10 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::InfiRay::Sensor | 14 | 14 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::InfiRay::Version | 22 | 22 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::JFIF::Extension | 3 | 3 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::JFIF::Main | 7 | 7 | 0 | 3 | 0 | 0 | 0 | — |
| Image::ExifTool::JPEG::AVI1 | 1 | 1 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::JPEG::Adobe | 4 | 4 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::JPEG::AdobeCM | 1 | 1 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::JPEG::EPPIM | 1 | 0 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::JPEG::GraphConv | 1 | 1 | 0 | 0 | 0 | 0 | 0 | raw_key_unrepresentable: 1 |
| Image::ExifTool::JPEG::HDR | 8 | 8 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::JPEG::HDRGainInfo | 2 | 2 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::JPEG::JPS | 7 | 6 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::JPEG::Main | 82 | 18 | 0 | 2 | 0 | 0 | 0 | raw_key_unrepresentable: 18 |
| Image::ExifTool::JPEG::MediaJukebox | 9 | 9 | 0 | 0 | 0 | 0 | 0 | raw_key_unrepresentable: 9 |
| Image::ExifTool::JPEG::NITF | 12 | 12 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::JPEG::Ocad | 1 | 1 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::JPEG::SOF | 6 | 6 | 0 | 0 | 0 | 0 | 0 | raw_key_unrepresentable: 6 |
| Image::ExifTool::JPEG::SPIFF | 11 | 11 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::JSON::Main | 9 | 8 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::JVC::Main | 2 | 2 | 1 | 0 | 0 | 0 | 0 | value_conv: 1 |
| Image::ExifTool::JVC::Text | 2 | 2 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Jpeg2000::CaptureResolution | 4 | 4 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Jpeg2000::ColorSpec | 6 | 5 | 0 | 5 | 0 | 0 | 0 | — |
| Image::ExifTool::Jpeg2000::DisplayResolution | 4 | 4 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Jpeg2000::FileType | 3 | 3 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Jpeg2000::ImageHeader | 5 | 5 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Jpeg2000::JUMD | 5 | 5 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Jpeg2000::Main | 71 | 40 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Kandao::FrameISP | 2 | 2 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Kandao::GPS | 4 | 4 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Kandao::GPSX | 5 | 5 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Kandao::IMU | 3 | 3 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Kandao::Main | 64 | 59 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Kodak::Borders | 6 | 6 | 5 | 0 | 0 | 0 | 0 | print_conv: 1 |
| Image::ExifTool::Kodak::CameraInfo | 12 | 12 | 12 | 12 | 0 | 0 | 0 | — |
| Image::ExifTool::Kodak::DcEM | 0 | 0 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Kodak::DcMD | 4 | 2 | 0 | 0 | 0 | 0 | 0 | raw_key_unrepresentable: 2 |
| Image::ExifTool::Kodak::DcME | 0 | 0 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Kodak::Free | 12 | 11 | 0 | 0 | 0 | 0 | 0 | raw_key_unrepresentable: 11 |
| Image::ExifTool::Kodak::IFD | 787 | 784 | 782 | 281 | 0 | 0 | 0 | print_conv: 1; value_conv: 1 |
| Image::ExifTool::Kodak::KDC_IFD | 7 | 7 | 7 | 2 | 0 | 0 | 0 | — |
| Image::ExifTool::Kodak::MOV | 7 | 7 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Kodak::Main | 34 | 34 | 0 | 34 | 0 | 0 | 0 | — |
| Image::ExifTool::Kodak::Meta | 34 | 32 | 30 | 2 | 0 | 0 | 0 | raw_conv: 2 |
| Image::ExifTool::Kodak::Processing | 1 | 1 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Kodak::Scrn | 4 | 4 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Kodak::SpecialEffects | 3 | 3 | 2 | 0 | 0 | 0 | 0 | print_conv: 1 |
| Image::ExifTool::Kodak::SubIFD0 | 17 | 17 | 17 | 17 | 0 | 0 | 0 | — |
| Image::ExifTool::Kodak::SubIFD1 | 2 | 2 | 0 | 2 | 0 | 0 | 0 | — |
| Image::ExifTool::Kodak::SubIFD2 | 8 | 8 | 0 | 8 | 0 | 0 | 0 | — |
| Image::ExifTool::Kodak::SubIFD3 | 23 | 1 | 0 | 1 | 0 | 0 | 0 | — |
| Image::ExifTool::Kodak::SubIFD4 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Kodak::SubIFD5 | 1 | 1 | 0 | 1 | 0 | 0 | 0 | — |
| Image::ExifTool::Kodak::SubIFD6 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Kodak::TextualInfo | 33 | 33 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Kodak::Type10 | 5 | 5 | 4 | 5 | 0 | 0 | 0 | print_conv: 1 |
| Image::ExifTool::Kodak::Type11 | 13 | 13 | 13 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Kodak::Type2 | 4 | 4 | 0 | 4 | 0 | 0 | 0 | — |
| Image::ExifTool::Kodak::Type3 | 8 | 8 | 0 | 8 | 0 | 0 | 0 | — |
| Image::ExifTool::Kodak::Type4 | 1 | 1 | 0 | 1 | 0 | 0 | 0 | — |
| Image::ExifTool::Kodak::Type5 | 9 | 9 | 0 | 9 | 0 | 0 | 0 | — |
| Image::ExifTool::Kodak::Type6 | 7 | 7 | 0 | 7 | 0 | 0 | 0 | — |
| Image::ExifTool::Kodak::Type7 | 1 | 1 | 0 | 1 | 0 | 0 | 0 | — |
| Image::ExifTool::Kodak::Type8 | 16 | 0 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Kodak::Type9 | 8 | 8 | 0 | 6 | 0 | 0 | 0 | — |
| Image::ExifTool::Kodak::Unknown | 0 | 0 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Kodak::frea | 4 | 4 | 0 | 0 | 0 | 0 | 0 | raw_key_unrepresentable: 4 |
| Image::ExifTool::Kodak::pose | 2 | 2 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::KyoceraRaw::Main | 11 | 11 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::LIF::Main | 1 | 1 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::LNK::Beef0003 | 1 | 1 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::LNK::Beef0004 | 5 | 4 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::LNK::Beef0014 | 19 | 19 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::LNK::Beef0025 | 2 | 2 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::LNK::Beef0026a | 3 | 3 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::LNK::ConsoleData | 17 | 17 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::LNK::ConsoleFEData | 1 | 1 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::LNK::ControlPanelCPL | 3 | 3 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::LNK::ControlPanelInfo | 1 | 1 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::LNK::EnvVarData | 2 | 2 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::LNK::GameFolderInfo | 1 | 1 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::LNK::INI | 13 | 13 | 0 | 0 | 0 | 0 | 0 | raw_key_unrepresentable: 13 |
| Image::ExifTool::LNK::Item00Info | 6 | 3 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::LNK::ItemID | 26 | 10 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::LNK::LinkInfo | 11 | 11 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::LNK::MTPType2 | 15 | 11 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::LNK::Main | 28 | 14 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::LNK::PropertyStore | 1 | 1 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::LNK::RootFolder | 2 | 2 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::LNK::TargetInfo | 3 | 3 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::LNK::TrackerData | 1 | 1 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::LNK::URI | 11 | 6 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::LNK::UnknownData | 0 | 0 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::LNK::UsersFilesFolder | 3 | 2 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Leaf::CameraProfile | 6 | 4 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Leaf::CameraSetup | 9 | 9 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Leaf::CaptureProfile | 23 | 23 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Leaf::CaptureSetup | 10 | 6 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Leaf::ColorSetup | 9 | 9 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Leaf::ImageProfile | 7 | 6 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Leaf::LookHeader | 4 | 4 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Leaf::Main | 11 | 8 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Leaf::Neutrals | 8 | 8 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Leaf::SaveSetup | 21 | 21 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Leaf::Selection | 9 | 9 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Leaf::Sharpness | 7 | 7 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Leaf::ShootSetup | 9 | 4 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Leaf::SubIFD | 0 | 0 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Leaf::ToneCurve | 7 | 7 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Lytro::Main | 24 | 24 | 0 | 0 | 0 | 0 | 0 | raw_key_unrepresentable: 24 |
| Image::ExifTool::M2TS::AC3 | 4 | 4 | 0 | 0 | 0 | 0 | 0 | raw_key_unrepresentable: 4 |
| Image::ExifTool::M2TS::Main | 6 | 3 | 0 | 0 | 0 | 0 | 0 | raw_key_unrepresentable: 3 |
| Image::ExifTool::MIE::Audio | 8 | 8 | 0 | 8 | 0 | 0 | 0 | — |
| Image::ExifTool::MIE::Camera | 27 | 24 | 0 | 24 | 0 | 0 | 0 | — |
| Image::ExifTool::MIE::Canon | 1 | 0 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::MIE::Doc | 14 | 14 | 0 | 14 | 0 | 0 | 0 | — |
| Image::ExifTool::MIE::Extender | 4 | 4 | 0 | 4 | 0 | 0 | 0 | — |
| Image::ExifTool::MIE::Flash | 8 | 8 | 0 | 8 | 0 | 0 | 0 | — |
| Image::ExifTool::MIE::GPS | 12 | 12 | 0 | 12 | 0 | 0 | 0 | — |
| Image::ExifTool::MIE::Geo | 7 | 5 | 0 | 5 | 0 | 0 | 0 | — |
| Image::ExifTool::MIE::Image | 10 | 10 | 0 | 10 | 0 | 0 | 0 | — |
| Image::ExifTool::MIE::Lens | 13 | 12 | 0 | 12 | 0 | 0 | 0 | — |
| Image::ExifTool::MIE::Main | 10 | 9 | 0 | 9 | 0 | 0 | 0 | — |
| Image::ExifTool::MIE::MakerNotes | 12 | 0 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::MIE::Meta | 14 | 0 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::MIE::Orient | 5 | 5 | 0 | 5 | 0 | 0 | 0 | — |
| Image::ExifTool::MIE::Preview | 4 | 4 | 0 | 4 | 0 | 0 | 0 | — |
| Image::ExifTool::MIE::Thumbnail | 4 | 4 | 0 | 4 | 0 | 0 | 0 | — |
| Image::ExifTool::MIE::UTM | 4 | 4 | 0 | 4 | 0 | 0 | 0 | — |
| Image::ExifTool::MIE::Unknown | 0 | 0 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::MIE::Video | 5 | 5 | 0 | 5 | 0 | 0 | 0 | — |
| Image::ExifTool::MIFF::Main | 35 | 29 | 0 | 0 | 0 | 0 | 0 | raw_key_unrepresentable: 29 |
| Image::ExifTool::MISB::ChurchillNav | 16 | 0 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::MISB::Main | 4 | 0 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::MISB::Security | 21 | 21 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::MISB::UASDatalink | 105 | 96 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::MISB::Unknown | 0 | 0 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::MNG::Background | 4 | 4 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::MNG::BasisObject | 12 | 12 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::MNG::ClipObjects | 4 | 4 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::MNG::CloneObject | 7 | 7 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::MNG::DefineObject | 5 | 5 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::MNG::DeltaPNGHeader | 5 | 5 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::MNG::ExportImage | 2 | 2 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::MNG::FramePriority | 2 | 2 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::MNG::JNGHeader | 10 | 10 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::MNG::Loop | 5 | 5 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::MNG::MNGHeader | 7 | 7 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::MNG::MagnifyObject | 10 | 10 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::MNG::Main | 27 | 9 | 0 | 0 | 0 | 0 | 0 | raw_key_unrepresentable: 9 |
| Image::ExifTool::MNG::MoveObjects | 4 | 4 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::MNG::PasteImage | 10 | 10 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::MNG::PromoteParent | 3 | 3 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::MNG::ShowObjects | 3 | 3 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::MNG::TerminationAction | 4 | 4 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::MOI::Main | 7 | 7 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::MPC::Main | 11 | 11 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::MPEG::Audio | 17 | 17 | 0 | 0 | 0 | 0 | 0 | raw_key_unrepresentable: 17 |
| Image::ExifTool::MPEG::Lame | 4 | 4 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::MPEG::Video | 5 | 5 | 0 | 0 | 0 | 0 | 0 | raw_key_unrepresentable: 5 |
| Image::ExifTool::MPEG::Xing | 7 | 6 | 6 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::MPF::MPImage | 7 | 7 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::MPF::Main | 19 | 18 | 17 | 0 | 0 | 0 | 0 | print_conv: 1; value_conv: 1 |
| Image::ExifTool::MRC::FEI12 | 98 | 98 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::MRC::Main | 36 | 36 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::MWG::Collections | 3 | 3 | 0 | 3 | 0 | 0 | 0 | raw_key_unrepresentable: 1 |
| Image::ExifTool::MWG::Keywords | 19 | 19 | 0 | 19 | 0 | 0 | 0 | raw_key_unrepresentable: 19 |
| Image::ExifTool::MWG::Regions | 21 | 21 | 0 | 21 | 0 | 0 | 0 | raw_key_unrepresentable: 2 |
| Image::ExifTool::MXF::Header | 3 | 3 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::MXF::Main | 1652 | 1581 | 0 | 0 | 0 | 0 | 0 | raw_key_unrepresentable: 1581 |
| Image::ExifTool::MacOS::MDItem | 131 | 131 | 0 | 4 | 0 | 0 | 0 | raw_key_unrepresentable: 131 |
| Image::ExifTool::MacOS::Main | 2 | 0 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::MacOS::XAttr | 11 | 11 | 0 | 2 | 0 | 0 | 0 | raw_key_unrepresentable: 11 |
| Image::ExifTool::Matroska::Main | 206 | 162 | 34 | 0 | 0 | 0 | 0 | ifd_format_unsupported: 92; print_conv: 2; raw_conv: 1; raw_key_unrepresentable: 19; value_conv: 15 |
| Image::ExifTool::Matroska::Projection | 7 | 5 | 4 | 0 | 0 | 0 | 0 | ifd_format_unsupported: 1 |
| Image::ExifTool::Matroska::StdTag | 109 | 107 | 0 | 0 | 0 | 0 | 0 | raw_key_unrepresentable: 104 |
| Image::ExifTool::Microsoft::MP | 8 | 8 | 0 | 8 | 0 | 0 | 0 | raw_key_unrepresentable: 8 |
| Image::ExifTool::Microsoft::MP1 | 15 | 15 | 0 | 15 | 0 | 0 | 0 | raw_key_unrepresentable: 15 |
| Image::ExifTool::Microsoft::Stitch | 7 | 7 | 0 | 7 | 0 | 0 | 0 | — |
| Image::ExifTool::Microsoft::XMP | 12 | 12 | 0 | 12 | 0 | 0 | 0 | raw_key_unrepresentable: 12 |
| Image::ExifTool::Microsoft::Xtra | 452 | 452 | 0 | 29 | 0 | 0 | 0 | — |
| Image::ExifTool::Minolta::CameraInfoA100 | 17 | 17 | 0 | 17 | 0 | 0 | 0 | — |
| Image::ExifTool::Minolta::CameraSettings | 50 | 50 | 0 | 50 | 0 | 0 | 0 | — |
| Image::ExifTool::Minolta::CameraSettings5D | 26 | 26 | 0 | 26 | 0 | 0 | 0 | — |
| Image::ExifTool::Minolta::CameraSettings7D | 26 | 26 | 0 | 26 | 0 | 0 | 0 | — |
| Image::ExifTool::Minolta::CameraSettingsA100 | 78 | 78 | 0 | 78 | 0 | 0 | 0 | — |
| Image::ExifTool::Minolta::ISInfoA100 | 1 | 1 | 0 | 1 | 0 | 0 | 0 | — |
| Image::ExifTool::Minolta::MMA | 2 | 2 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Minolta::MOV1 | 6 | 6 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Minolta::MOV2 | 6 | 6 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Minolta::Main | 33 | 24 | 19 | 20 | 0 | 0 | 0 | ifd_isoffset_unsupported: 3; print_conv: 1; value_conv: 1 |
| Image::ExifTool::Minolta::WBInfoA100 | 64 | 64 | 0 | 63 | 0 | 0 | 0 | — |
| Image::ExifTool::MinoltaRaw::Main | 4 | 0 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::MinoltaRaw::PRD | 9 | 9 | 0 | 9 | 0 | 0 | 0 | — |
| Image::ExifTool::MinoltaRaw::RIF | 29 | 29 | 0 | 28 | 0 | 0 | 0 | — |
| Image::ExifTool::MinoltaRaw::WBG | 3 | 3 | 0 | 3 | 0 | 0 | 0 | — |
| Image::ExifTool::Motorola::Main | 6 | 6 | 6 | 6 | 0 | 0 | 0 | — |
| Image::ExifTool::Nikon::AFInfo | 3 | 3 | 0 | 3 | 0 | 0 | 0 | — |
| Image::ExifTool::Nikon::AFInfo2V0100 | 21 | 21 | 0 | 20 | 0 | 0 | 0 | — |
| Image::ExifTool::Nikon::AFInfo2V0101 | 25 | 25 | 0 | 24 | 0 | 0 | 0 | — |
| Image::ExifTool::Nikon::AFInfo2V0200 | 9 | 9 | 0 | 8 | 0 | 0 | 0 | — |
| Image::ExifTool::Nikon::AFInfo2V0300 | 24 | 24 | 0 | 17 | 0 | 0 | 0 | — |
| Image::ExifTool::Nikon::AFInfo2V0400 | 20 | 20 | 0 | 13 | 0 | 0 | 0 | — |
| Image::ExifTool::Nikon::AFTune | 4 | 4 | 0 | 4 | 0 | 0 | 0 | — |
| Image::ExifTool::Nikon::AVI | 4 | 2 | 0 | 0 | 0 | 0 | 0 | raw_key_unrepresentable: 2 |
| Image::ExifTool::Nikon::AVITags | 30 | 23 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Nikon::AVIVers | 2 | 2 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Nikon::AutoCaptureInfo | 11 | 11 | 0 | 11 | 0 | 0 | 0 | — |
| Image::ExifTool::Nikon::BarometerInfo | 2 | 2 | 0 | 1 | 0 | 0 | 0 | — |
| Image::ExifTool::Nikon::BracketingInfoD500 | 4 | 4 | 0 | 4 | 0 | 0 | 0 | — |
| Image::ExifTool::Nikon::BracketingInfoD810 | 3 | 3 | 0 | 3 | 0 | 0 | 0 | — |
| Image::ExifTool::Nikon::CaptureOffsets | 3 | 3 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Nikon::CaptureOutput | 3 | 3 | 0 | 3 | 0 | 0 | 0 | — |
| Image::ExifTool::Nikon::ColorBalance1 | 1 | 1 | 0 | 1 | 0 | 0 | 0 | — |
| Image::ExifTool::Nikon::ColorBalance2 | 1 | 1 | 0 | 1 | 0 | 0 | 0 | — |
| Image::ExifTool::Nikon::ColorBalance3 | 1 | 1 | 0 | 1 | 0 | 0 | 0 | — |
| Image::ExifTool::Nikon::ColorBalance4 | 1 | 1 | 0 | 1 | 0 | 0 | 0 | — |
| Image::ExifTool::Nikon::ColorBalanceA | 8 | 8 | 0 | 8 | 0 | 0 | 0 | — |
| Image::ExifTool::Nikon::ColorBalanceB | 9 | 9 | 0 | 9 | 0 | 0 | 0 | — |
| Image::ExifTool::Nikon::ColorBalanceC | 13 | 13 | 0 | 13 | 0 | 0 | 0 | — |
| Image::ExifTool::Nikon::ColorBalanceUnknown | 1 | 1 | 0 | 1 | 0 | 0 | 0 | — |
| Image::ExifTool::Nikon::ColorBalanceUnknown2 | 1 | 1 | 0 | 1 | 0 | 0 | 0 | — |
| Image::ExifTool::Nikon::CustomSettingsD500 | 2 | 0 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Nikon::DistortInfo | 2 | 2 | 0 | 1 | 0 | 0 | 0 | — |
| Image::ExifTool::Nikon::DistortionInfo | 5 | 5 | 0 | 5 | 0 | 0 | 0 | — |
| Image::ExifTool::Nikon::FaceDetect | 14 | 14 | 0 | 14 | 0 | 0 | 0 | — |
| Image::ExifTool::Nikon::FileInfo | 4 | 4 | 0 | 3 | 0 | 0 | 0 | — |
| Image::ExifTool::Nikon::FlashInfo0100 | 18 | 18 | 0 | 17 | 0 | 0 | 0 | — |
| Image::ExifTool::Nikon::FlashInfo0102 | 21 | 21 | 0 | 20 | 0 | 0 | 0 | — |
| Image::ExifTool::Nikon::FlashInfo0103 | 25 | 25 | 0 | 24 | 0 | 0 | 0 | — |
| Image::ExifTool::Nikon::FlashInfo0106 | 22 | 22 | 0 | 21 | 0 | 0 | 0 | — |
| Image::ExifTool::Nikon::FlashInfo0107 | 20 | 20 | 0 | 19 | 0 | 0 | 0 | — |
| Image::ExifTool::Nikon::FlashInfo0300 | 23 | 23 | 0 | 22 | 0 | 0 | 0 | — |
| Image::ExifTool::Nikon::FlashInfoUnknown | 1 | 1 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Nikon::GEM | 1 | 1 | 0 | 1 | 0 | 0 | 0 | — |
| Image::ExifTool::Nikon::HDRInfo | 5 | 5 | 0 | 4 | 0 | 0 | 0 | — |
| Image::ExifTool::Nikon::HDRInfo2 | 3 | 3 | 0 | 2 | 0 | 0 | 0 | — |
| Image::ExifTool::Nikon::ISOAutoInfoD810 | 2 | 2 | 0 | 2 | 0 | 0 | 0 | — |
| Image::ExifTool::Nikon::ISOInfo | 4 | 4 | 0 | 4 | 0 | 0 | 0 | — |
| Image::ExifTool::Nikon::IntervalInfoD6 | 18 | 18 | 0 | 17 | 0 | 0 | 0 | — |
| Image::ExifTool::Nikon::IntervalInfoZ7II | 3 | 3 | 0 | 3 | 0 | 0 | 0 | — |
| Image::ExifTool::Nikon::JPGInfoD500 | 1 | 1 | 0 | 1 | 0 | 0 | 0 | — |
| Image::ExifTool::Nikon::LensData00 | 8 | 8 | 0 | 7 | 0 | 0 | 0 | — |
| Image::ExifTool::Nikon::LensData01 | 14 | 14 | 0 | 13 | 0 | 0 | 0 | — |
| Image::ExifTool::Nikon::LensData0204 | 14 | 14 | 0 | 13 | 0 | 0 | 0 | — |
| Image::ExifTool::Nikon::LensData0400 | 2 | 2 | 0 | 1 | 0 | 0 | 0 | — |
| Image::ExifTool::Nikon::LensData0402 | 2 | 2 | 0 | 1 | 0 | 0 | 0 | — |
| Image::ExifTool::Nikon::LensData0403 | 2 | 2 | 0 | 1 | 0 | 0 | 0 | — |
| Image::ExifTool::Nikon::LensData0800 | 26 | 26 | 0 | 25 | 0 | 0 | 0 | — |
| Image::ExifTool::Nikon::LensDataUnknown | 1 | 1 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Nikon::LocationInfo | 5 | 5 | 0 | 5 | 0 | 0 | 0 | — |
| Image::ExifTool::Nikon::MOV | 9 | 9 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Nikon::Main | 182 | 78 | 0 | 77 | 0 | 0 | 0 | — |
| Image::ExifTool::Nikon::MakerNotes0x51 | 2 | 2 | 0 | 1 | 0 | 0 | 0 | — |
| Image::ExifTool::Nikon::MakerNotes0x56 | 8 | 7 | 0 | 6 | 0 | 0 | 0 | — |
| Image::ExifTool::Nikon::MenuInfoZ7II | 1 | 0 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Nikon::MenuInfoZ8 | 2 | 0 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Nikon::MenuInfoZ9 | 3 | 0 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Nikon::MenuSettingsD850 | 1 | 1 | 0 | 1 | 0 | 0 | 0 | — |
| Image::ExifTool::Nikon::MenuSettingsZ6III | 63 | 62 | 0 | 61 | 0 | 0 | 0 | — |
| Image::ExifTool::Nikon::MenuSettingsZ7II | 39 | 39 | 0 | 37 | 0 | 0 | 0 | — |
| Image::ExifTool::Nikon::MenuSettingsZ8 | 64 | 64 | 0 | 63 | 0 | 0 | 0 | — |
| Image::ExifTool::Nikon::MenuSettingsZ8v1 | 52 | 50 | 0 | 50 | 0 | 0 | 0 | — |
| Image::ExifTool::Nikon::MenuSettingsZ8v2 | 65 | 63 | 0 | 63 | 0 | 0 | 0 | — |
| Image::ExifTool::Nikon::MenuSettingsZ9 | 70 | 69 | 0 | 66 | 0 | 0 | 0 | — |
| Image::ExifTool::Nikon::MenuSettingsZ9v3 | 74 | 73 | 0 | 71 | 0 | 0 | 0 | — |
| Image::ExifTool::Nikon::MenuSettingsZ9v4 | 135 | 134 | 0 | 132 | 0 | 0 | 0 | — |
| Image::ExifTool::Nikon::MoreSettingsD850 | 2 | 2 | 0 | 2 | 0 | 0 | 0 | — |
| Image::ExifTool::Nikon::MultiExposure | 4 | 4 | 0 | 3 | 0 | 0 | 0 | — |
| Image::ExifTool::Nikon::MultiExposure2 | 4 | 4 | 0 | 3 | 0 | 0 | 0 | — |
| Image::ExifTool::Nikon::NCDB | 0 | 0 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Nikon::NCDT | 7 | 5 | 0 | 0 | 0 | 0 | 0 | raw_key_unrepresentable: 5 |
| Image::ExifTool::Nikon::NCTG | 86 | 54 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Nikon::NEFInfo | 2 | 0 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Nikon::NineEdits | 3 | 3 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Nikon::Offset13InfoZ9 | 4 | 4 | 0 | 4 | 0 | 0 | 0 | — |
| Image::ExifTool::Nikon::OrientationInfo | 3 | 3 | 0 | 3 | 0 | 0 | 0 | — |
| Image::ExifTool::Nikon::OtherInfoD500 | 1 | 1 | 0 | 1 | 0 | 0 | 0 | — |
| Image::ExifTool::Nikon::PictureControl | 13 | 13 | 0 | 12 | 0 | 0 | 0 | — |
| Image::ExifTool::Nikon::PictureControl2 | 14 | 14 | 0 | 13 | 0 | 0 | 0 | — |
| Image::ExifTool::Nikon::PictureControl3 | 15 | 15 | 0 | 14 | 0 | 0 | 0 | — |
| Image::ExifTool::Nikon::PictureControlUnknown | 1 | 1 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Nikon::PortraitInfoZ7II | 1 | 1 | 0 | 1 | 0 | 0 | 0 | — |
| Image::ExifTool::Nikon::PreviewIFD | 8 | 8 | 6 | 0 | 0 | 0 | 0 | ifd_isoffset_unsupported: 2 |
| Image::ExifTool::Nikon::ROC | 1 | 1 | 0 | 1 | 0 | 0 | 0 | — |
| Image::ExifTool::Nikon::RetouchInfo | 2 | 2 | 0 | 1 | 0 | 0 | 0 | — |
| Image::ExifTool::Nikon::RotationInfoD500 | 4 | 4 | 0 | 4 | 0 | 0 | 0 | — |
| Image::ExifTool::Nikon::Scan | 12 | 10 | 10 | 10 | 0 | 0 | 0 | — |
| Image::ExifTool::Nikon::SeqInfoD6 | 3 | 3 | 0 | 3 | 0 | 0 | 0 | — |
| Image::ExifTool::Nikon::SeqInfoZ9 | 3 | 3 | 0 | 3 | 0 | 0 | 0 | — |
| Image::ExifTool::Nikon::SettingsInfoD810 | 1 | 1 | 0 | 1 | 0 | 0 | 0 | — |
| Image::ExifTool::Nikon::ShootingMenuD500 | 6 | 6 | 0 | 6 | 0 | 0 | 0 | — |
| Image::ExifTool::Nikon::ShotInfo | 11 | 11 | 0 | 9 | 0 | 0 | 0 | — |
| Image::ExifTool::Nikon::ShotInfoD300S | 5 | 4 | 0 | 2 | 0 | 0 | 0 | — |
| Image::ExifTool::Nikon::ShotInfoD300a | 5 | 4 | 0 | 3 | 0 | 0 | 0 | — |
| Image::ExifTool::Nikon::ShotInfoD300b | 7 | 6 | 0 | 4 | 0 | 0 | 0 | — |
| Image::ExifTool::Nikon::ShotInfoD3S | 7 | 6 | 0 | 4 | 0 | 0 | 0 | — |
| Image::ExifTool::Nikon::ShotInfoD3X | 5 | 4 | 0 | 2 | 0 | 0 | 0 | — |
| Image::ExifTool::Nikon::ShotInfoD3a | 6 | 5 | 0 | 4 | 0 | 0 | 0 | — |
| Image::ExifTool::Nikon::ShotInfoD3b | 10 | 9 | 0 | 7 | 0 | 0 | 0 | — |
| Image::ExifTool::Nikon::ShotInfoD4 | 3 | 2 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Nikon::ShotInfoD40 | 4 | 3 | 0 | 2 | 0 | 0 | 0 | — |
| Image::ExifTool::Nikon::ShotInfoD4S | 13 | 10 | 0 | 8 | 0 | 0 | 0 | — |
| Image::ExifTool::Nikon::ShotInfoD500 | 9 | 2 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Nikon::ShotInfoD5000 | 5 | 4 | 0 | 2 | 0 | 0 | 0 | — |
| Image::ExifTool::Nikon::ShotInfoD5100 | 4 | 3 | 0 | 1 | 0 | 0 | 0 | — |
| Image::ExifTool::Nikon::ShotInfoD5200 | 4 | 3 | 0 | 1 | 0 | 0 | 0 | — |
| Image::ExifTool::Nikon::ShotInfoD6 | 6 | 3 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Nikon::ShotInfoD610 | 3 | 2 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Nikon::ShotInfoD700 | 5 | 4 | 0 | 2 | 0 | 0 | 0 | — |
| Image::ExifTool::Nikon::ShotInfoD7000 | 4 | 3 | 0 | 1 | 0 | 0 | 0 | — |
| Image::ExifTool::Nikon::ShotInfoD7500 | 3 | 2 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Nikon::ShotInfoD780 | 3 | 2 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Nikon::ShotInfoD80 | 8 | 7 | 0 | 6 | 0 | 0 | 0 | — |
| Image::ExifTool::Nikon::ShotInfoD800 | 11 | 10 | 0 | 8 | 0 | 0 | 0 | — |
| Image::ExifTool::Nikon::ShotInfoD810 | 7 | 2 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Nikon::ShotInfoD850 | 6 | 2 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Nikon::ShotInfoD90 | 5 | 4 | 0 | 2 | 0 | 0 | 0 | — |
| Image::ExifTool::Nikon::ShotInfoZ6III | 7 | 5 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Nikon::ShotInfoZ7II | 10 | 5 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Nikon::ShotInfoZ8 | 9 | 5 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Nikon::ShotInfoZ9 | 10 | 5 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Nikon::Type2 | 8 | 8 | 8 | 8 | 0 | 0 | 0 | — |
| Image::ExifTool::Nikon::UnknownInfo | 1 | 1 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Nikon::UnknownInfo2 | 1 | 1 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Nikon::VRInfo | 5 | 5 | 0 | 4 | 0 | 0 | 0 | — |
| Image::ExifTool::Nikon::VignetteInfo | 4 | 4 | 0 | 4 | 0 | 0 | 0 | — |
| Image::ExifTool::Nikon::WorldTime | 3 | 3 | 0 | 3 | 0 | 0 | 0 | — |
| Image::ExifTool::Nikon::ast | 14 | 12 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Nikon::nine | 6 | 5 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Nikon::sdc | 4 | 4 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::NikonCapture::Brightness | 2 | 2 | 0 | 2 | 0 | 0 | 0 | — |
| Image::ExifTool::NikonCapture::ColorBoost | 2 | 2 | 0 | 2 | 0 | 0 | 0 | — |
| Image::ExifTool::NikonCapture::CropData | 13 | 13 | 0 | 13 | 0 | 0 | 0 | — |
| Image::ExifTool::NikonCapture::DLightingHQ | 3 | 3 | 0 | 3 | 0 | 0 | 0 | — |
| Image::ExifTool::NikonCapture::DLightingHS | 2 | 2 | 0 | 2 | 0 | 0 | 0 | — |
| Image::ExifTool::NikonCapture::Exposure | 4 | 4 | 0 | 4 | 0 | 0 | 0 | — |
| Image::ExifTool::NikonCapture::HighlightData | 3 | 3 | 0 | 3 | 0 | 0 | 0 | — |
| Image::ExifTool::NikonCapture::Main | 39 | 25 | 0 | 25 | 0 | 0 | 0 | — |
| Image::ExifTool::NikonCapture::NoiseReduction | 9 | 9 | 0 | 9 | 0 | 0 | 0 | — |
| Image::ExifTool::NikonCapture::PhotoEffects | 4 | 4 | 0 | 4 | 0 | 0 | 0 | — |
| Image::ExifTool::NikonCapture::PictureCtrl | 8 | 8 | 0 | 8 | 0 | 0 | 0 | — |
| Image::ExifTool::NikonCapture::RedEyeData | 1 | 1 | 0 | 1 | 0 | 0 | 0 | — |
| Image::ExifTool::NikonCapture::UnsharpData | 17 | 17 | 0 | 17 | 0 | 0 | 0 | — |
| Image::ExifTool::NikonCapture::WBAdjData | 6 | 6 | 0 | 6 | 0 | 0 | 0 | — |
| Image::ExifTool::NikonCustom::SettingsD3 | 90 | 90 | 0 | 90 | 0 | 0 | 0 | — |
| Image::ExifTool::NikonCustom::SettingsD4 | 89 | 89 | 0 | 89 | 0 | 0 | 0 | — |
| Image::ExifTool::NikonCustom::SettingsD40 | 22 | 22 | 0 | 22 | 0 | 0 | 0 | — |
| Image::ExifTool::NikonCustom::SettingsD5 | 89 | 89 | 0 | 89 | 0 | 0 | 0 | — |
| Image::ExifTool::NikonCustom::SettingsD500 | 89 | 89 | 0 | 89 | 0 | 0 | 0 | — |
| Image::ExifTool::NikonCustom::SettingsD5000 | 26 | 26 | 0 | 26 | 0 | 0 | 0 | — |
| Image::ExifTool::NikonCustom::SettingsD5100 | 23 | 23 | 0 | 23 | 0 | 0 | 0 | — |
| Image::ExifTool::NikonCustom::SettingsD5200 | 26 | 26 | 0 | 26 | 0 | 0 | 0 | — |
| Image::ExifTool::NikonCustom::SettingsD610 | 25 | 25 | 0 | 25 | 0 | 0 | 0 | — |
| Image::ExifTool::NikonCustom::SettingsD700 | 70 | 70 | 0 | 70 | 0 | 0 | 0 | — |
| Image::ExifTool::NikonCustom::SettingsD7000 | 64 | 64 | 0 | 64 | 0 | 0 | 0 | — |
| Image::ExifTool::NikonCustom::SettingsD80 | 48 | 48 | 0 | 48 | 0 | 0 | 0 | — |
| Image::ExifTool::NikonCustom::SettingsD800 | 21 | 21 | 0 | 21 | 0 | 0 | 0 | — |
| Image::ExifTool::NikonCustom::SettingsD810 | 86 | 86 | 0 | 86 | 0 | 0 | 0 | — |
| Image::ExifTool::NikonCustom::SettingsD850 | 89 | 89 | 0 | 89 | 0 | 0 | 0 | — |
| Image::ExifTool::NikonCustom::SettingsD90 | 54 | 54 | 0 | 54 | 0 | 0 | 0 | — |
| Image::ExifTool::NikonCustom::SettingsZ6III | 97 | 97 | 0 | 94 | 0 | 0 | 0 | — |
| Image::ExifTool::NikonCustom::SettingsZ8 | 127 | 127 | 0 | 124 | 0 | 0 | 0 | — |
| Image::ExifTool::NikonCustom::SettingsZ9 | 142 | 142 | 0 | 139 | 0 | 0 | 0 | — |
| Image::ExifTool::NikonCustom::SettingsZ9v4 | 143 | 143 | 0 | 140 | 0 | 0 | 0 | — |
| Image::ExifTool::NikonSettings::Main | 234 | 234 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Nintendo::CameraInfo | 5 | 5 | 0 | 5 | 0 | 0 | 0 | — |
| Image::ExifTool::Nintendo::Main | 1 | 0 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::OOXML::Main | 61 | 59 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Ogg::Main | 5 | 0 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Olympus::AFInfo | 1 | 1 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Olympus::AFTargetInfo | 3 | 3 | 0 | 3 | 0 | 0 | 0 | — |
| Image::ExifTool::Olympus::AVI | 6 | 5 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Olympus::CameraSettings | 72 | 70 | 43 | 68 | 0 | 0 | 0 | ifd_isoffset_unsupported: 2; print_conv: 22; raw_conv: 1; value_conv: 3 |
| Image::ExifTool::Olympus::DSS | 5 | 5 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Olympus::Equipment | 25 | 25 | 18 | 25 | 0 | 0 | 0 | print_conv: 4; raw_conv: 1; value_conv: 2 |
| Image::ExifTool::Olympus::FETags | 1 | 1 | 1 | 1 | 0 | 0 | 0 | — |
| Image::ExifTool::Olympus::FocusInfo | 28 | 27 | 17 | 26 | 0 | 0 | 0 | print_conv: 7; raw_conv: 3; value_conv: 2 |
| Image::ExifTool::Olympus::ImageProcessing | 64 | 64 | 62 | 63 | 0 | 0 | 0 | print_conv: 1; raw_conv: 1 |
| Image::ExifTool::Olympus::MOV1 | 6 | 6 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Olympus::MOV2 | 7 | 7 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Olympus::MOV3 | 1 | 0 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Olympus::MP4 | 6 | 4 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Olympus::Main | 138 | 98 | 87 | 89 | 0 | 0 | 0 | condition: 1; ifd_isoffset_unsupported: 7; print_conv: 3 |
| Image::ExifTool::Olympus::MovableInfo | 4 | 3 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Olympus::OLYM | 7 | 7 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Olympus::OLYM2 | 3 | 0 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Olympus::RawDevSubIFD | 0 | 0 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Olympus::RawDevelopment | 14 | 14 | 13 | 14 | 0 | 0 | 0 | raw_conv: 1 |
| Image::ExifTool::Olympus::RawDevelopment2 | 25 | 24 | 22 | 24 | 0 | 0 | 0 | print_conv: 1; raw_conv: 1 |
| Image::ExifTool::Olympus::RawInfo | 36 | 36 | 35 | 35 | 0 | 0 | 0 | raw_conv: 1 |
| Image::ExifTool::Olympus::SubjectDetectInfo | 4 | 4 | 0 | 4 | 0 | 0 | 0 | — |
| Image::ExifTool::Olympus::TextInfo | 2 | 2 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Olympus::Thumbnail | 4 | 4 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Olympus::UnknownInfo | 0 | 0 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Olympus::WAV | 22 | 22 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Olympus::prms | 5 | 5 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Olympus::scrn | 2 | 2 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Olympus::scrn2 | 1 | 0 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Olympus::thmb | 2 | 2 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Olympus::thmb2 | 4 | 4 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::OpenEXR::Main | 43 | 41 | 0 | 0 | 0 | 0 | 0 | raw_key_unrepresentable: 39 |
| Image::ExifTool::Opus::Header | 4 | 4 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Opus::Main | 2 | 0 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Other::PFM | 4 | 4 | 0 | 0 | 0 | 0 | 0 | raw_key_unrepresentable: 4 |
| Image::ExifTool::PCAP::Main | 29 | 29 | 3 | 0 | 0 | 0 | 0 | raw_key_unrepresentable: 24; value_conv: 2 |
| Image::ExifTool::PCX::Main | 15 | 15 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::PDF::AF | 1 | 0 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::PDF::AIMetaData | 1 | 0 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::PDF::AIPrivate | 6 | 3 | 0 | 0 | 0 | 0 | 0 | raw_key_unrepresentable: 3 |
| Image::ExifTool::PDF::AcroForm | 1 | 1 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::PDF::AdobePhotoshop | 1 | 0 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::PDF::ColorSpace | 4 | 0 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::PDF::DefaultRGB | 1 | 0 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::PDF::EF | 1 | 0 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::PDF::Encrypt | 2 | 2 | 0 | 0 | 0 | 0 | 0 | raw_key_unrepresentable: 2 |
| Image::ExifTool::PDF::F | 1 | 0 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::PDF::ICCBased | 1 | 0 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::PDF::Illustrator | 1 | 0 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::PDF::Im | 5 | 5 | 0 | 0 | 0 | 0 | 0 | raw_key_unrepresentable: 5 |
| Image::ExifTool::PDF::ImageResources | 1 | 0 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::PDF::Info | 11 | 11 | 0 | 11 | 0 | 0 | 0 | raw_key_unrepresentable: 11 |
| Image::ExifTool::PDF::Kids | 4 | 0 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::PDF::MC | 1 | 0 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::PDF::Main | 4 | 1 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::PDF::MarkInfo | 1 | 1 | 0 | 0 | 0 | 0 | 0 | raw_key_unrepresentable: 1 |
| Image::ExifTool::PDF::Metadata | 1 | 0 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::PDF::Pages | 3 | 2 | 0 | 0 | 0 | 0 | 0 | raw_key_unrepresentable: 2 |
| Image::ExifTool::PDF::Perms | 3 | 0 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::PDF::PieceInfo | 2 | 0 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::PDF::Private | 1 | 0 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::PDF::Properties | 1 | 0 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::PDF::Reference | 1 | 0 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::PDF::Resources | 3 | 0 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::PDF::Root | 10 | 4 | 0 | 0 | 0 | 0 | 0 | raw_key_unrepresentable: 4 |
| Image::ExifTool::PDF::Signature | 8 | 7 | 0 | 0 | 0 | 0 | 0 | raw_key_unrepresentable: 7 |
| Image::ExifTool::PDF::TransformParams | 10 | 10 | 0 | 0 | 0 | 0 | 0 | raw_key_unrepresentable: 10 |
| Image::ExifTool::PDF::XObject | 1 | 0 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::PGF::Main | 9 | 9 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::PICT::Main | 146 | 144 | 27 | 0 | 0 | 0 | 0 | ifd_format_unsupported: 117 |
| Image::ExifTool::PLIST::Main | 31 | 30 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::PLUS::XMP | 86 | 86 | 0 | 86 | 0 | 0 | 0 | raw_key_unrepresentable: 62 |
| Image::ExifTool::PNG::AnimationControl | 2 | 2 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::PNG::CICodePoints | 4 | 4 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::PNG::ImageHeader | 7 | 7 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::PNG::Main | 38 | 19 | 0 | 4 | 0 | 0 | 0 | raw_key_unrepresentable: 19 |
| Image::ExifTool::PNG::PhysicalPixel | 3 | 3 | 0 | 3 | 0 | 0 | 0 | — |
| Image::ExifTool::PNG::PrimaryChromaticities | 8 | 8 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::PNG::StereoImage | 1 | 1 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::PNG::SubjectScale | 3 | 3 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::PNG::TextualData | 31 | 22 | 0 | 22 | 0 | 0 | 0 | — |
| Image::ExifTool::PNG::VirtualPage | 3 | 3 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::PSP::Creator | 8 | 8 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::PSP::Ext | 1 | 0 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::PSP::Image | 8 | 8 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::PSP::Main | 5 | 1 | 0 | 0 | 0 | 0 | 0 | raw_key_unrepresentable: 1 |
| Image::ExifTool::Palm::EXTH | 44 | 44 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Palm::MOBI | 8 | 8 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Palm::Main | 6 | 6 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Panasonic::DSA | 10 | 10 | 0 | 10 | 0 | 0 | 0 | — |
| Image::ExifTool::Panasonic::Data1 | 1 | 1 | 0 | 1 | 0 | 0 | 0 | — |
| Image::ExifTool::Panasonic::Data2 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Panasonic::FaceDetInfo | 6 | 6 | 0 | 6 | 0 | 0 | 0 | — |
| Image::ExifTool::Panasonic::FaceRecInfo | 10 | 10 | 0 | 10 | 0 | 0 | 0 | — |
| Image::ExifTool::Panasonic::FocusInfo | 2 | 2 | 0 | 2 | 0 | 0 | 0 | — |
| Image::ExifTool::Panasonic::Leica2 | 19 | 19 | 17 | 19 | 0 | 0 | 0 | print_conv: 2 |
| Image::ExifTool::Panasonic::Leica3 | 2 | 1 | 1 | 1 | 0 | 0 | 0 | — |
| Image::ExifTool::Panasonic::Leica4 | 4 | 0 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Panasonic::Leica5 | 11 | 8 | 7 | 8 | 0 | 0 | 0 | print_conv: 1 |
| Image::ExifTool::Panasonic::Leica6 | 8 | 8 | 5 | 7 | 0 | 0 | 0 | ifd_isoffset_unsupported: 1; print_conv: 1; value_conv: 1 |
| Image::ExifTool::Panasonic::Leica9 | 10 | 10 | 9 | 10 | 0 | 0 | 0 | print_conv: 1 |
| Image::ExifTool::Panasonic::Main | 140 | 136 | 113 | 135 | 0 | 0 | 0 | condition: 1; print_conv: 10; raw_conv: 4; tag_variant_cond_unsupported: 4; value_conv: 7 |
| Image::ExifTool::Panasonic::PANA | 23 | 17 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Panasonic::SerialInfo | 1 | 1 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Panasonic::ShotInfo | 1 | 1 | 0 | 1 | 0 | 0 | 0 | — |
| Image::ExifTool::Panasonic::Subdir | 21 | 19 | 18 | 19 | 0 | 0 | 0 | print_conv: 1 |
| Image::ExifTool::Panasonic::TimeInfo | 2 | 2 | 0 | 2 | 0 | 0 | 0 | — |
| Image::ExifTool::Panasonic::Type2 | 2 | 2 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::PanasonicRaw::CameraIFD | 23 | 23 | 21 | 0 | 0 | 0 | 0 | raw_conv: 2; value_conv: 1 |
| Image::ExifTool::PanasonicRaw::DistortionInfo | 8 | 8 | 0 | 8 | 0 | 0 | 0 | — |
| Image::ExifTool::PanasonicRaw::Main | 54 | 45 | 39 | 34 | 0 | 0 | 0 | ifd_isoffset_unsupported: 4; raw_conv: 2 |
| Image::ExifTool::PanasonicRaw::WBInfo | 15 | 15 | 0 | 15 | 0 | 0 | 0 | — |
| Image::ExifTool::PanasonicRaw::WBInfo2 | 15 | 15 | 0 | 15 | 0 | 0 | 0 | — |
| Image::ExifTool::Parrot::ARCoreAccel | 2 | 2 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Parrot::ARCoreAccel0 | 1 | 1 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Parrot::ARCoreCustom | 0 | 0 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Parrot::ARCoreGyro | 2 | 2 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Parrot::ARCoreGyro0 | 1 | 1 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Parrot::ARCoreVideo | 0 | 0 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Parrot::Automation | 8 | 8 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Parrot::FollowMe | 5 | 5 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Parrot::TimeStamp | 1 | 1 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Parrot::V1 | 23 | 23 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Parrot::V2 | 22 | 22 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Parrot::V3 | 25 | 25 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Parrot::mett | 12 | 0 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Pentax::AEInfo | 17 | 17 | 0 | 16 | 0 | 0 | 0 | — |
| Image::ExifTool::Pentax::AEInfo2 | 12 | 12 | 0 | 12 | 0 | 0 | 0 | — |
| Image::ExifTool::Pentax::AEInfo3 | 7 | 7 | 0 | 7 | 0 | 0 | 0 | — |
| Image::ExifTool::Pentax::AEInfoUnknown | 0 | 0 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Pentax::AFInfo | 17 | 17 | 0 | 16 | 0 | 0 | 0 | — |
| Image::ExifTool::Pentax::AFInfoK3III | 8 | 8 | 0 | 5 | 0 | 0 | 0 | — |
| Image::ExifTool::Pentax::AFPointInfo | 4 | 4 | 0 | 1 | 0 | 0 | 0 | — |
| Image::ExifTool::Pentax::AVI | 2 | 0 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Pentax::AWBInfo | 2 | 2 | 0 | 2 | 0 | 0 | 0 | — |
| Image::ExifTool::Pentax::BatteryInfo | 22 | 22 | 0 | 22 | 0 | 0 | 0 | — |
| Image::ExifTool::Pentax::CAFPointInfo | 4 | 4 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Pentax::CameraInfo | 4 | 4 | 0 | 4 | 0 | 0 | 0 | — |
| Image::ExifTool::Pentax::CameraSettings | 31 | 31 | 0 | 31 | 0 | 0 | 0 | — |
| Image::ExifTool::Pentax::CameraSettingsUnknown | 0 | 0 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Pentax::ColorInfo | 2 | 2 | 0 | 2 | 0 | 0 | 0 | — |
| Image::ExifTool::Pentax::EVStepInfo | 3 | 3 | 0 | 3 | 0 | 0 | 0 | — |
| Image::ExifTool::Pentax::FaceInfo | 2 | 2 | 0 | 2 | 0 | 0 | 0 | — |
| Image::ExifTool::Pentax::FaceInfoK3III | 65 | 65 | 0 | 65 | 0 | 0 | 0 | — |
| Image::ExifTool::Pentax::FacePos | 32 | 32 | 0 | 32 | 0 | 0 | 0 | — |
| Image::ExifTool::Pentax::FaceSize | 32 | 32 | 0 | 32 | 0 | 0 | 0 | — |
| Image::ExifTool::Pentax::FilterInfo | 22 | 22 | 0 | 22 | 0 | 0 | 0 | — |
| Image::ExifTool::Pentax::FlashInfo | 11 | 11 | 0 | 11 | 0 | 0 | 0 | — |
| Image::ExifTool::Pentax::FlashInfoUnknown | 0 | 0 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Pentax::Junk | 1 | 1 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Pentax::Junk2 | 9 | 9 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Pentax::KelvinWB | 17 | 17 | 0 | 17 | 0 | 0 | 0 | — |
| Image::ExifTool::Pentax::LensCorr | 4 | 4 | 0 | 4 | 0 | 0 | 0 | — |
| Image::ExifTool::Pentax::LensData | 23 | 22 | 0 | 22 | 0 | 0 | 0 | — |
| Image::ExifTool::Pentax::LensInfo | 2 | 1 | 0 | 1 | 0 | 0 | 0 | — |
| Image::ExifTool::Pentax::LensInfo2 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | — |
| Image::ExifTool::Pentax::LensInfo3 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | — |
| Image::ExifTool::Pentax::LensInfo4 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | — |
| Image::ExifTool::Pentax::LensInfo5 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | — |
| Image::ExifTool::Pentax::LensInfoQ | 2 | 2 | 0 | 2 | 0 | 0 | 0 | — |
| Image::ExifTool::Pentax::LensRec | 2 | 2 | 0 | 2 | 0 | 0 | 0 | — |
| Image::ExifTool::Pentax::LevelInfo | 7 | 7 | 0 | 7 | 0 | 0 | 0 | — |
| Image::ExifTool::Pentax::LevelInfoK3III | 3 | 3 | 0 | 3 | 0 | 0 | 0 | — |
| Image::ExifTool::Pentax::MOV | 7 | 7 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Pentax::Main | 179 | 136 | 87 | 130 | 0 | 0 | 0 | ifd_isoffset_unsupported: 2; print_conv: 42; raw_conv: 3; value_conv: 20 |
| Image::ExifTool::Pentax::PENT | 24 | 24 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Pentax::PXTH | 2 | 2 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Pentax::PixelShiftInfo | 1 | 1 | 0 | 1 | 0 | 0 | 0 | — |
| Image::ExifTool::Pentax::S1 | 1 | 1 | 1 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Pentax::SRInfo | 4 | 4 | 0 | 4 | 0 | 0 | 0 | — |
| Image::ExifTool::Pentax::SRInfo2 | 2 | 2 | 0 | 2 | 0 | 0 | 0 | — |
| Image::ExifTool::Pentax::ShotInfo | 1 | 1 | 0 | 1 | 0 | 0 | 0 | — |
| Image::ExifTool::Pentax::TempInfo | 6 | 6 | 0 | 5 | 0 | 0 | 0 | — |
| Image::ExifTool::Pentax::TimeInfo | 5 | 5 | 0 | 5 | 0 | 0 | 0 | — |
| Image::ExifTool::Pentax::Type2 | 14 | 13 | 13 | 13 | 0 | 0 | 0 | — |
| Image::ExifTool::Pentax::Type4 | 1 | 1 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Pentax::UnknownInfo | 0 | 0 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Pentax::WBLevels | 11 | 11 | 0 | 11 | 0 | 0 | 0 | — |
| Image::ExifTool::PhaseOne::Main | 45 | 40 | 0 | 36 | 0 | 0 | 0 | — |
| Image::ExifTool::PhaseOne::SensorCalibration | 18 | 8 | 0 | 1 | 0 | 0 | 0 | — |
| Image::ExifTool::PhotoCD::Main | 27 | 26 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::PhotoMechanic::Main | 1 | 0 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::PhotoMechanic::SoftEdit | 19 | 19 | 11 | 19 | 0 | 0 | 0 | print_conv: 8; value_conv: 8 |
| Image::ExifTool::PhotoMechanic::XMP | 8 | 8 | 0 | 8 | 0 | 0 | 0 | raw_key_unrepresentable: 8 |
| Image::ExifTool::Photoshop::ChannelOptions | 4 | 4 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Photoshop::DocumentData | 2 | 0 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Photoshop::Header | 5 | 5 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Photoshop::ImageData | 1 | 1 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Photoshop::JPEG_Quality | 3 | 3 | 0 | 1 | 0 | 0 | 0 | — |
| Image::ExifTool::Photoshop::Layers | 11 | 11 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Photoshop::Main | 86 | 75 | 0 | 7 | 0 | 0 | 0 | — |
| Image::ExifTool::Photoshop::PixelInfo | 1 | 1 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Photoshop::PrintScaleInfo | 3 | 3 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Photoshop::Resolution | 4 | 4 | 0 | 4 | 0 | 0 | 0 | — |
| Image::ExifTool::Photoshop::SliceInfo | 2 | 2 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Photoshop::VersionInfo | 3 | 3 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::PostScript::Main | 28 | 24 | 0 | 11 | 0 | 0 | 0 | — |
| Image::ExifTool::PrintIM::Main | 1 | 1 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Qualcomm::DualCamera | 43 | 43 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Qualcomm::Main | 1188 | 1188 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::QuickTime::AV1Config | 9 | 9 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::QuickTime::Accel360Fly | 3 | 3 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::QuickTime::AudioHeader | 1 | 1 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::QuickTime::AudioKeys | 6 | 6 | 0 | 6 | 0 | 0 | 0 | — |
| Image::ExifTool::QuickTime::AudioProf | 9 | 9 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::QuickTime::AudioSampleDesc | 12 | 5 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::QuickTime::Bitrate | 3 | 3 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::QuickTime::CMovie | 1 | 1 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::QuickTime::ChannelLayout | 28 | 28 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::QuickTime::CleanAperture | 4 | 4 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::QuickTime::ColorRep | 5 | 5 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::QuickTime::ContentLightLevel | 2 | 2 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::QuickTime::DataInfo | 1 | 0 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::QuickTime::DataRef | 3 | 3 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::QuickTime::DecodeConfig | 2 | 2 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::QuickTime::EncodingParams | 24 | 24 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::QuickTime::FaceInfo | 1 | 0 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::QuickTime::FaceRec | 1 | 0 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::QuickTime::FileProf | 2 | 2 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::QuickTime::FileType | 3 | 3 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::QuickTime::Flip | 5 | 5 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::QuickTime::Fusion360Fly | 3 | 3 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::QuickTime::GPS360Fly | 8 | 8 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::QuickTime::GenMediaHeader | 3 | 1 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::QuickTime::GenMediaInfo | 5 | 5 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::QuickTime::Gyro360Fly | 3 | 3 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::QuickTime::HEVCConfig | 16 | 16 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::QuickTime::HTCBinary | 0 | 0 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::QuickTime::HTCInfo | 1 | 1 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::QuickTime::Handler | 4 | 4 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::QuickTime::HintHeader | 4 | 4 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::QuickTime::HintInfo | 2 | 2 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::QuickTime::HintSampleDesc | 6 | 6 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::QuickTime::HintTrackInfo | 17 | 17 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::QuickTime::INSV_MakerNotes | 4 | 4 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::QuickTime::ImageFile | 3 | 1 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::QuickTime::ItemInfo | 1 | 1 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::QuickTime::ItemList | 105 | 104 | 92 | 104 | 0 | 0 | 0 | uncaptured_source_property:Shift: 1; unsupported_format:undef: 3; unsupported_print_conversion: 4; unsupported_source_property:Binary: 3; unsupported_source_property:Unknown: 3; unsupported_source_property:ValueConv: 6 |
| Image::ExifTool::QuickTime::ItemProp | 2 | 1 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::QuickTime::ItemPropCont | 13 | 8 | 0 | 3 | 0 | 0 | 0 | — |
| Image::ExifTool::QuickTime::ItemRef | 4 | 4 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::QuickTime::Keys | 81 | 77 | 70 | 63 | 0 | 0 | 0 | uncaptured_source_property:Shift: 3; unsupported_print_conversion: 5; unsupported_source_property:ValueConv: 6 |
| Image::ExifTool::QuickTime::Mag360Fly | 3 | 3 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::QuickTime::Main | 59 | 25 | 0 | 2 | 0 | 0 | 0 | — |
| Image::ExifTool::QuickTime::Media | 4 | 1 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::QuickTime::MediaHeader | 6 | 6 | 0 | 2 | 0 | 0 | 0 | — |
| Image::ExifTool::QuickTime::MediaInfo | 8 | 1 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::QuickTime::Meta | 22 | 10 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::QuickTime::MetaData | 7 | 7 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::QuickTime::MetaRelation | 0 | 0 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::QuickTime::MetaSampleDesc | 4 | 2 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::QuickTime::Movie | 14 | 4 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::QuickTime::MovieFragHdr | 1 | 1 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::QuickTime::MovieFragment | 3 | 0 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::QuickTime::MovieHeader | 15 | 15 | 0 | 2 | 0 | 0 | 0 | — |
| Image::ExifTool::QuickTime::Nextbase | 65 | 65 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::QuickTime::OtherMeta | 2 | 0 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::QuickTime::OtherSampleDesc | 7 | 4 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::QuickTime::Pittasoft | 7 | 5 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::QuickTime::Preview | 4 | 4 | 0 | 1 | 0 | 0 | 0 | — |
| Image::ExifTool::QuickTime::PreviewInfo | 1 | 1 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::QuickTime::Profile | 4 | 0 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::QuickTime::ProtectionInfo | 4 | 2 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::QuickTime::RVMI_gReV | 4 | 4 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::QuickTime::RVMI_sReV | 1 | 1 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::QuickTime::Rights | 8 | 8 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::QuickTime::Rot360Fly | 3 | 3 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::QuickTime::RoveGPS | 4 | 4 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::QuickTime::SampleTable | 23 | 18 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::QuickTime::SchemeInfo | 6 | 5 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::QuickTime::SchemeType | 3 | 3 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::QuickTime::SkipInfo | 2 | 2 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::QuickTime::SpatialAudio | 7 | 7 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::QuickTime::Stream | 73 | 46 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::QuickTime::TCMediaInfo | 6 | 6 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::QuickTime::Tags360Fly | 6 | 0 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::QuickTime::TimeCode | 1 | 0 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::QuickTime::TomTom | 5 | 4 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::QuickTime::Track | 10 | 1 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::QuickTime::TrackAperture | 3 | 3 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::QuickTime::TrackFragment | 1 | 0 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::QuickTime::TrackHeader | 11 | 10 | 0 | 3 | 0 | 0 | 0 | — |
| Image::ExifTool::QuickTime::TrackRef | 11 | 11 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::QuickTime::UserData | 213 | 149 | 17 | 137 | 0 | 0 | 0 | conditional_variant_selection: 9; uncaptured_source_property:IText: 13; uncaptured_source_property:NoDecode: 1; uncaptured_source_property:RawConvInv: 2; uncaptured_source_property:SetBase: 1; uncaptured_source_property:Shift: 3; unsupported_format:rational64s: 9; unsupported_format:undef: 1; unsupported_print_conversion: 7; unsupported_source_property:Binary: 7; unsupported_source_property:Condition: 5; unsupported_source_property:RawConv: 11; unsupported_source_property:Unknown: 7; unsupported_source_property:ValueConv: 16; unsupported_userdata_implicit_format: 121 |
| Image::ExifTool::QuickTime::UserMedia | 1 | 0 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::QuickTime::Video | 2 | 2 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::QuickTime::VideoHeader | 2 | 2 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::QuickTime::VideoKeys | 5 | 5 | 0 | 5 | 0 | 0 | 0 | — |
| Image::ExifTool::QuickTime::VideoProf | 11 | 11 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::QuickTime::VisualSampleDesc | 20 | 14 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::QuickTime::Wave | 2 | 2 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::QuickTime::camm0 | 1 | 1 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::QuickTime::camm1 | 2 | 2 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::QuickTime::camm2 | 1 | 1 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::QuickTime::camm3 | 1 | 1 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::QuickTime::camm4 | 1 | 1 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::QuickTime::camm5 | 3 | 3 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::QuickTime::camm6 | 11 | 11 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::QuickTime::camm7 | 1 | 1 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::QuickTime::cbmp | 2 | 2 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::QuickTime::equi | 4 | 4 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::QuickTime::grpl | 0 | 0 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::QuickTime::iTunesInfo | 45 | 40 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::QuickTime::prhd | 3 | 3 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::QuickTime::proj | 3 | 0 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::QuickTime::sdpd | 1 | 1 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::QuickTime::setu | 2 | 1 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::QuickTime::sv3d | 2 | 1 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::QuickTime::tx3g | 18 | 18 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::RIFF::ALPH | 3 | 3 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::RIFF::ANIM | 2 | 2 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::RIFF::ANMF | 1 | 1 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::RIFF::AVIHeader | 6 | 6 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::RIFF::Acidizer | 5 | 5 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::RIFF::AudioFormat | 5 | 5 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::RIFF::BroadcastExt | 8 | 8 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::RIFF::CSET | 4 | 4 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::RIFF::DS64 | 3 | 3 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::RIFF::Exif | 7 | 7 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::RIFF::ExtAVIHdr | 1 | 1 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::RIFF::Hdrl | 5 | 2 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::RIFF::Info | 87 | 87 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::RIFF::Instrument | 7 | 7 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::RIFF::Main | 60 | 14 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::RIFF::OpenDML | 1 | 0 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::RIFF::Sampler | 10 | 10 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::RIFF::Stream | 6 | 1 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::RIFF::StreamData | 4 | 2 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::RIFF::StreamHeader | 12 | 12 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::RIFF::Tdat | 0 | 0 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::RIFF::UserText | 6 | 6 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::RIFF::VP8 | 5 | 5 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::RIFF::VP8L | 3 | 3 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::RIFF::VP8X | 3 | 3 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::RSRC::Main | 9 | 6 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::RTF::Main | 24 | 24 | 0 | 0 | 0 | 0 | 0 | raw_key_unrepresentable: 24 |
| Image::ExifTool::Radiance::Main | 11 | 11 | 0 | 0 | 0 | 0 | 0 | raw_key_unrepresentable: 8 |
| Image::ExifTool::Rawzor::Main | 5 | 5 | 0 | 0 | 0 | 0 | 0 | raw_key_unrepresentable: 5 |
| Image::ExifTool::Real::Audio | 3 | 0 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Real::AudioV3 | 12 | 12 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Real::AudioV4 | 31 | 31 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Real::AudioV5 | 18 | 18 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Real::ContentDescr | 8 | 8 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Real::FileInfo | 14 | 14 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Real::Media | 4 | 0 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Real::MediaProps | 22 | 21 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Real::Metadata | 4 | 4 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Real::Metafile | 2 | 2 | 0 | 0 | 0 | 0 | 0 | raw_key_unrepresentable: 2 |
| Image::ExifTool::Real::Properties | 11 | 11 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Reconyx::HyperFire | 19 | 19 | 0 | 17 | 0 | 0 | 0 | — |
| Image::ExifTool::Reconyx::HyperFire2 | 26 | 26 | 0 | 26 | 0 | 0 | 0 | — |
| Image::ExifTool::Reconyx::HyperFire4K | 24 | 24 | 0 | 24 | 0 | 0 | 0 | — |
| Image::ExifTool::Reconyx::MicroFire | 30 | 30 | 0 | 30 | 0 | 0 | 0 | — |
| Image::ExifTool::Reconyx::UltraFire | 16 | 16 | 0 | 16 | 0 | 0 | 0 | — |
| Image::ExifTool::Red::Main | 42 | 40 | 29 | 0 | 0 | 0 | 0 | print_conv: 3; value_conv: 10 |
| Image::ExifTool::Red::RED1 | 5 | 5 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Red::RED2 | 4 | 4 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Ricoh::AVI | 4 | 3 | 0 | 0 | 0 | 0 | 0 | raw_key_unrepresentable: 3 |
| Image::ExifTool::Ricoh::FaceInfo | 10 | 10 | 0 | 10 | 0 | 0 | 0 | — |
| Image::ExifTool::Ricoh::FirmwareInfo | 2 | 2 | 0 | 2 | 0 | 0 | 0 | — |
| Image::ExifTool::Ricoh::ImageInfo | 11 | 11 | 0 | 9 | 0 | 0 | 0 | — |
| Image::ExifTool::Ricoh::Main | 49 | 43 | 39 | 43 | 0 | 0 | 0 | print_conv: 2; tag_variant_cond_unsupported: 2 |
| Image::ExifTool::Ricoh::RDTA | 2 | 2 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Ricoh::RDTB | 2 | 2 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Ricoh::RDTC | 2 | 2 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Ricoh::RDTG | 2 | 2 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Ricoh::RDTL | 4 | 4 | 2 | 0 | 0 | 0 | 0 | print_conv: 2; value_conv: 1 |
| Image::ExifTool::Ricoh::RMETA | 7 | 7 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Ricoh::SerialInfo | 4 | 4 | 0 | 4 | 0 | 0 | 0 | — |
| Image::ExifTool::Ricoh::Subdir | 6 | 3 | 3 | 3 | 0 | 0 | 0 | — |
| Image::ExifTool::Ricoh::Text | 5 | 5 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Ricoh::ThetaSubdir | 3 | 3 | 3 | 3 | 0 | 0 | 0 | — |
| Image::ExifTool::Ricoh::Type2 | 2 | 2 | 1 | 0 | 0 | 0 | 0 | value_conv: 1 |
| Image::ExifTool::Samsung::APP5 | 1 | 1 | 0 | 0 | 0 | 0 | 0 | raw_key_unrepresentable: 1 |
| Image::ExifTool::Samsung::ClipInfo | 14 | 14 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Samsung::DualShotExtra | 3 | 2 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Samsung::EffectInfo | 3 | 3 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Samsung::IFD | 0 | 0 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Samsung::INFO | 2 | 2 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Samsung::MP4 | 8 | 7 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Samsung::Main | 4 | 3 | 0 | 1 | 0 | 0 | 0 | — |
| Image::ExifTool::Samsung::OrientationInfo | 3 | 3 | 0 | 3 | 0 | 0 | 0 | — |
| Image::ExifTool::Samsung::PEgInfo | 2 | 2 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Samsung::PictureWizard | 5 | 5 | 0 | 5 | 0 | 0 | 0 | — |
| Image::ExifTool::Samsung::PortraitEffect | 17 | 17 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Samsung::ReEditData | 22 | 18 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Samsung::SingleShotMeta | 28 | 28 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Samsung::Thumbnail | 4 | 4 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Samsung::ToneInfo | 17 | 17 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Samsung::Trailer | 28 | 24 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Samsung::Type2 | 58 | 51 | 20 | 51 | 0 | 0 | 0 | condition: 1; print_conv: 2; raw_conv: 23; tag_variant_cond_unsupported: 2; value_conv: 3 |
| Image::ExifTool::Samsung::sec | 6 | 6 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Samsung::smta | 2 | 1 | 0 | 0 | 0 | 0 | 0 | raw_key_unrepresentable: 1 |
| Image::ExifTool::Samsung::svss | 0 | 0 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Sanyo::FaceInfo | 2 | 2 | 0 | 2 | 0 | 0 | 0 | — |
| Image::ExifTool::Sanyo::MOV | 7 | 7 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Sanyo::MP4 | 9 | 7 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Sanyo::Main | 28 | 26 | 25 | 25 | 0 | 0 | 0 | raw_conv: 1 |
| Image::ExifTool::Sanyo::Thumbnail | 4 | 4 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Scalado::Main | 4 | 4 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Sigma::Main | 85 | 83 | 52 | 79 | 0 | 0 | 0 | print_conv: 12; tag_variant_cond_unsupported: 2; tag_variant_field_unsupported: 8; value_conv: 11 |
| Image::ExifTool::Sigma::WBSettings | 10 | 10 | 0 | 10 | 0 | 0 | 0 | — |
| Image::ExifTool::Sigma::WBSettings2 | 10 | 10 | 0 | 10 | 0 | 0 | 0 | — |
| Image::ExifTool::SigmaRaw::Header | 8 | 8 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::SigmaRaw::Header4 | 4 | 4 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::SigmaRaw::HeaderExt | 11 | 11 | 1 | 0 | 0 | 0 | 0 | print_conv: 10 |
| Image::ExifTool::SigmaRaw::Main | 7 | 3 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::SigmaRaw::Properties | 43 | 43 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Sony::AFInfo | 25 | 22 | 0 | 22 | 0 | 0 | 0 | — |
| Image::ExifTool::Sony::AFStatus15 | 18 | 18 | 0 | 18 | 0 | 0 | 0 | — |
| Image::ExifTool::Sony::AFStatus19 | 30 | 30 | 0 | 30 | 0 | 0 | 0 | — |
| Image::ExifTool::Sony::AFStatus79 | 95 | 95 | 0 | 95 | 0 | 0 | 0 | — |
| Image::ExifTool::Sony::CameraInfo | 31 | 31 | 0 | 31 | 0 | 0 | 0 | — |
| Image::ExifTool::Sony::CameraInfo2 | 16 | 16 | 0 | 16 | 0 | 0 | 0 | — |
| Image::ExifTool::Sony::CameraInfo3 | 24 | 23 | 0 | 23 | 0 | 0 | 0 | — |
| Image::ExifTool::Sony::CameraInfoUnknown | 0 | 0 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Sony::CameraSettings | 55 | 55 | 0 | 55 | 0 | 0 | 0 | — |
| Image::ExifTool::Sony::CameraSettings2 | 45 | 45 | 0 | 45 | 0 | 0 | 0 | — |
| Image::ExifTool::Sony::CameraSettings3 | 68 | 68 | 0 | 68 | 0 | 0 | 0 | — |
| Image::ExifTool::Sony::CameraSettingsUnknown | 0 | 0 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Sony::Ericsson | 3 | 3 | 1 | 1 | 0 | 0 | 0 | ifd_isoffset_unsupported: 2 |
| Image::ExifTool::Sony::ExtraInfo | 6 | 6 | 0 | 4 | 0 | 0 | 0 | — |
| Image::ExifTool::Sony::ExtraInfo2 | 2 | 2 | 0 | 2 | 0 | 0 | 0 | — |
| Image::ExifTool::Sony::ExtraInfo3 | 12 | 12 | 0 | 12 | 0 | 0 | 0 | — |
| Image::ExifTool::Sony::FaceInfo | 9 | 9 | 0 | 9 | 0 | 0 | 0 | — |
| Image::ExifTool::Sony::FaceInfo1 | 8 | 8 | 0 | 8 | 0 | 0 | 0 | — |
| Image::ExifTool::Sony::FaceInfo2 | 8 | 8 | 0 | 8 | 0 | 0 | 0 | — |
| Image::ExifTool::Sony::FaceInfoA | 15 | 13 | 0 | 12 | 0 | 0 | 0 | — |
| Image::ExifTool::Sony::FocusInfo | 19 | 19 | 0 | 18 | 0 | 0 | 0 | — |
| Image::ExifTool::Sony::HiddenInfo | 2 | 2 | 0 | 2 | 0 | 0 | 0 | — |
| Image::ExifTool::Sony::ISOInfo | 3 | 3 | 0 | 3 | 0 | 0 | 0 | — |
| Image::ExifTool::Sony::Main | 160 | 92 | 67 | 87 | 0 | 0 | 0 | condition: 1; ifd_isoffset_unsupported: 1; print_conv: 13; raw_conv: 12; value_conv: 4 |
| Image::ExifTool::Sony::MeterInfo | 16 | 16 | 0 | 16 | 0 | 0 | 0 | — |
| Image::ExifTool::Sony::MeterInfo9 | 16 | 16 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Sony::MoreInfo | 6 | 1 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Sony::MoreInfo0201 | 3 | 3 | 0 | 3 | 0 | 0 | 0 | — |
| Image::ExifTool::Sony::MoreInfo0401 | 1 | 1 | 0 | 1 | 0 | 0 | 0 | — |
| Image::ExifTool::Sony::MoreSettings | 60 | 60 | 0 | 60 | 0 | 0 | 0 | — |
| Image::ExifTool::Sony::PIC | 11 | 10 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Sony::PMP | 14 | 14 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Sony::Panorama | 11 | 11 | 0 | 11 | 0 | 0 | 0 | — |
| Image::ExifTool::Sony::SR2DataIFD | 1 | 1 | 1 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Sony::SR2Private | 6 | 3 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Sony::SR2SubIFD | 36 | 35 | 33 | 27 | 0 | 0 | 0 | print_conv: 2 |
| Image::ExifTool::Sony::SRF | 2 | 2 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Sony::SRF2 | 26 | 26 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Sony::ShotInfo | 9 | 7 | 0 | 5 | 0 | 0 | 0 | — |
| Image::ExifTool::Sony::Tag2010a | 17 | 16 | 0 | 16 | 0 | 0 | 0 | — |
| Image::ExifTool::Sony::Tag2010b | 24 | 23 | 0 | 23 | 0 | 0 | 0 | — |
| Image::ExifTool::Sony::Tag2010c | 24 | 23 | 0 | 23 | 0 | 0 | 0 | — |
| Image::ExifTool::Sony::Tag2010d | 21 | 20 | 0 | 20 | 0 | 0 | 0 | — |
| Image::ExifTool::Sony::Tag2010e | 38 | 37 | 0 | 37 | 0 | 0 | 0 | — |
| Image::ExifTool::Sony::Tag2010f | 24 | 23 | 0 | 23 | 0 | 0 | 0 | — |
| Image::ExifTool::Sony::Tag2010g | 31 | 30 | 0 | 30 | 0 | 0 | 0 | — |
| Image::ExifTool::Sony::Tag2010h | 32 | 30 | 0 | 30 | 0 | 0 | 0 | — |
| Image::ExifTool::Sony::Tag2010i | 31 | 30 | 0 | 30 | 0 | 0 | 0 | — |
| Image::ExifTool::Sony::Tag202a | 17 | 17 | 0 | 17 | 0 | 0 | 0 | — |
| Image::ExifTool::Sony::Tag900b | 2 | 2 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Sony::Tag9050a | 24 | 24 | 0 | 24 | 0 | 0 | 0 | — |
| Image::ExifTool::Sony::Tag9050b | 34 | 34 | 0 | 33 | 0 | 0 | 0 | — |
| Image::ExifTool::Sony::Tag9050c | 12 | 12 | 0 | 12 | 0 | 0 | 0 | — |
| Image::ExifTool::Sony::Tag9050d | 5 | 5 | 0 | 5 | 0 | 0 | 0 | — |
| Image::ExifTool::Sony::Tag9400a | 10 | 10 | 0 | 10 | 0 | 0 | 0 | — |
| Image::ExifTool::Sony::Tag9400b | 10 | 10 | 0 | 10 | 0 | 0 | 0 | — |
| Image::ExifTool::Sony::Tag9400c | 13 | 13 | 0 | 13 | 0 | 0 | 0 | — |
| Image::ExifTool::Sony::Tag9401 | 20 | 0 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Sony::Tag9402 | 5 | 4 | 0 | 4 | 0 | 0 | 0 | — |
| Image::ExifTool::Sony::Tag9403 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | — |
| Image::ExifTool::Sony::Tag9404a | 3 | 3 | 0 | 3 | 0 | 0 | 0 | — |
| Image::ExifTool::Sony::Tag9404b | 4 | 4 | 0 | 4 | 0 | 0 | 0 | — |
| Image::ExifTool::Sony::Tag9404c | 2 | 2 | 0 | 2 | 0 | 0 | 0 | — |
| Image::ExifTool::Sony::Tag9405a | 9 | 9 | 0 | 9 | 0 | 0 | 0 | — |
| Image::ExifTool::Sony::Tag9405b | 36 | 36 | 0 | 36 | 0 | 0 | 0 | — |
| Image::ExifTool::Sony::Tag9406 | 4 | 4 | 0 | 4 | 0 | 0 | 0 | — |
| Image::ExifTool::Sony::Tag9406b | 5 | 2 | 0 | 2 | 0 | 0 | 0 | — |
| Image::ExifTool::Sony::Tag940a | 1 | 1 | 0 | 1 | 0 | 0 | 0 | — |
| Image::ExifTool::Sony::Tag940c | 5 | 5 | 0 | 5 | 0 | 0 | 0 | — |
| Image::ExifTool::Sony::Tag940e | 3 | 3 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Sony::Tag9416 | 36 | 35 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Sony::rtmd | 43 | 21 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::SonyIDC::Main | 54 | 53 | 51 | 51 | 0 | 0 | 0 | ifd_isoffset_unsupported: 2 |
| Image::ExifTool::Stim::CropX | 5 | 5 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Stim::CropY | 5 | 5 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Stim::Main | 20 | 18 | 14 | 0 | 0 | 0 | 0 | print_conv: 4 |
| Image::ExifTool::TNEF::AttachInfo | 13 | 13 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::TNEF::Main | 30 | 28 | 1 | 0 | 0 | 0 | 0 | raw_key_unrepresentable: 27 |
| Image::ExifTool::TNEF::MsgProps | 40 | 40 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Text::Main | 9 | 9 | 0 | 0 | 0 | 0 | 0 | raw_key_unrepresentable: 9 |
| Image::ExifTool::Theora::Identification | 11 | 11 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Theora::Main | 2 | 0 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Torrent::Files | 4 | 4 | 0 | 0 | 0 | 0 | 0 | raw_key_unrepresentable: 4 |
| Image::ExifTool::Torrent::Info | 11 | 9 | 0 | 0 | 0 | 0 | 0 | raw_key_unrepresentable: 9 |
| Image::ExifTool::Torrent::Main | 8 | 7 | 0 | 0 | 0 | 0 | 0 | raw_key_unrepresentable: 7 |
| Image::ExifTool::Torrent::Profiles | 4 | 4 | 0 | 0 | 0 | 0 | 0 | raw_key_unrepresentable: 4 |
| Image::ExifTool::Trailer::Google | 6 | 6 | 0 | 0 | 0 | 0 | 0 | raw_key_unrepresentable: 6 |
| Image::ExifTool::Trailer::OnePlus | 3 | 3 | 0 | 0 | 0 | 0 | 0 | raw_key_unrepresentable: 3 |
| Image::ExifTool::Trailer::Vivo | 3 | 3 | 0 | 0 | 0 | 0 | 0 | raw_key_unrepresentable: 3 |
| Image::ExifTool::Unknown::Main | 1 | 0 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::VCard::Main | 31 | 31 | 0 | 0 | 0 | 0 | 0 | raw_key_unrepresentable: 31 |
| Image::ExifTool::VCard::VCalendar | 65 | 65 | 0 | 0 | 0 | 0 | 0 | raw_key_unrepresentable: 65 |
| Image::ExifTool::VCard::VNote | 4 | 4 | 0 | 0 | 0 | 0 | 0 | raw_key_unrepresentable: 4 |
| Image::ExifTool::Vorbis::Comments | 32 | 31 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Vorbis::Identification | 6 | 6 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::Vorbis::Main | 2 | 0 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::WPG::Main | 5 | 5 | 0 | 0 | 0 | 0 | 0 | raw_key_unrepresentable: 5 |
| Image::ExifTool::WTV::Main | 1 | 0 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::WTV::Metadata | 73 | 73 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::WavPack::Main | 5 | 5 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::XISF::Main | 37 | 37 | 0 | 0 | 0 | 0 | 0 | raw_key_unrepresentable: 37 |
| Image::ExifTool::XMP::ACDSeeRegions | 19 | 19 | 0 | 19 | 0 | 0 | 0 | — |
| Image::ExifTool::XMP::Album | 1 | 1 | 0 | 1 | 0 | 0 | 0 | raw_key_unrepresentable: 1 |
| Image::ExifTool::XMP::DICOM | 14 | 14 | 0 | 14 | 0 | 0 | 0 | — |
| Image::ExifTool::XMP::ExifTool | 3 | 3 | 0 | 3 | 0 | 0 | 0 | raw_key_unrepresentable: 3 |
| Image::ExifTool::XMP::ExpressionMedia | 4 | 4 | 0 | 4 | 0 | 0 | 0 | — |
| Image::ExifTool::XMP::GettyImages | 20 | 20 | 0 | 20 | 0 | 0 | 0 | — |
| Image::ExifTool::XMP::HDRGainMap | 1 | 1 | 0 | 1 | 0 | 0 | 0 | — |
| Image::ExifTool::XMP::LImage | 3 | 3 | 0 | 3 | 0 | 0 | 0 | — |
| Image::ExifTool::XMP::Lightroom | 3 | 3 | 0 | 3 | 0 | 0 | 0 | raw_key_unrepresentable: 3 |
| Image::ExifTool::XMP::Main | 79 | 0 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::XMP::MediaPro | 6 | 6 | 0 | 6 | 0 | 0 | 0 | — |
| Image::ExifTool::XMP::PixelLive | 6 | 6 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::XMP::SEAL | 13 | 13 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::XMP::SVG | 5 | 5 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::XMP::XML | 2 | 1 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::XMP::aas | 27 | 27 | 0 | 27 | 0 | 0 | 0 | — |
| Image::ExifTool::XMP::acdsee | 19 | 19 | 0 | 19 | 0 | 0 | 0 | — |
| Image::ExifTool::XMP::apdi | 3 | 3 | 0 | 3 | 0 | 0 | 0 | — |
| Image::ExifTool::XMP::apple_fi | 5 | 5 | 0 | 5 | 0 | 0 | 0 | — |
| Image::ExifTool::XMP::aux | 26 | 26 | 0 | 26 | 0 | 0 | 0 | raw_key_unrepresentable: 26 |
| Image::ExifTool::XMP::cc | 11 | 11 | 0 | 11 | 0 | 0 | 0 | — |
| Image::ExifTool::XMP::cell | 6 | 6 | 0 | 6 | 0 | 0 | 0 | — |
| Image::ExifTool::XMP::crd | 1093 | 1093 | 0 | 1093 | 0 | 0 | 0 | — |
| Image::ExifTool::XMP::creatorAtom | 14 | 14 | 0 | 14 | 0 | 0 | 0 | — |
| Image::ExifTool::XMP::crs | 1093 | 1093 | 0 | 1093 | 0 | 0 | 0 | raw_key_unrepresentable: 271 |
| Image::ExifTool::XMP::dc | 15 | 15 | 0 | 15 | 0 | 0 | 0 | raw_key_unrepresentable: 15 |
| Image::ExifTool::XMP::dex | 8 | 8 | 0 | 8 | 0 | 0 | 0 | — |
| Image::ExifTool::XMP::digiKam | 9 | 9 | 0 | 9 | 0 | 0 | 0 | — |
| Image::ExifTool::XMP::exif | 100 | 100 | 0 | 100 | 0 | 0 | 0 | raw_key_unrepresentable: 81 |
| Image::ExifTool::XMP::exifEX | 42 | 42 | 0 | 42 | 0 | 0 | 0 | raw_key_unrepresentable: 32 |
| Image::ExifTool::XMP::extensis | 8 | 8 | 0 | 8 | 0 | 0 | 0 | — |
| Image::ExifTool::XMP::fpv | 1 | 1 | 0 | 1 | 0 | 0 | 0 | — |
| Image::ExifTool::XMP::hdr | 6 | 6 | 0 | 6 | 0 | 0 | 0 | — |
| Image::ExifTool::XMP::hdrgm | 9 | 9 | 0 | 9 | 0 | 0 | 0 | — |
| Image::ExifTool::XMP::ics | 30 | 30 | 0 | 30 | 0 | 0 | 0 | — |
| Image::ExifTool::XMP::iptcCore | 16 | 16 | 0 | 16 | 0 | 0 | 0 | raw_key_unrepresentable: 16 |
| Image::ExifTool::XMP::iptcExt | 251 | 251 | 0 | 251 | 0 | 0 | 0 | — |
| Image::ExifTool::XMP::panorama | 4 | 4 | 0 | 4 | 0 | 0 | 0 | — |
| Image::ExifTool::XMP::pdf | 12 | 12 | 0 | 12 | 0 | 0 | 0 | raw_key_unrepresentable: 12 |
| Image::ExifTool::XMP::pdfx | 1 | 1 | 0 | 1 | 0 | 0 | 0 | raw_key_unrepresentable: 1 |
| Image::ExifTool::XMP::photoshop | 53 | 53 | 0 | 53 | 0 | 0 | 0 | raw_key_unrepresentable: 26 |
| Image::ExifTool::XMP::pmi | 34 | 34 | 0 | 34 | 0 | 0 | 0 | — |
| Image::ExifTool::XMP::prism | 112 | 112 | 0 | 112 | 0 | 0 | 0 | — |
| Image::ExifTool::XMP::prl | 3 | 3 | 0 | 3 | 0 | 0 | 0 | — |
| Image::ExifTool::XMP::prm | 19 | 19 | 0 | 19 | 0 | 0 | 0 | — |
| Image::ExifTool::XMP::pur | 14 | 14 | 0 | 14 | 0 | 0 | 0 | — |
| Image::ExifTool::XMP::rdf | 1 | 1 | 0 | 1 | 0 | 0 | 0 | raw_key_unrepresentable: 1 |
| Image::ExifTool::XMP::seal | 1 | 0 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::XMP::swf | 4 | 4 | 0 | 4 | 0 | 0 | 0 | — |
| Image::ExifTool::XMP::tiff | 26 | 26 | 0 | 26 | 0 | 0 | 0 | raw_key_unrepresentable: 26 |
| Image::ExifTool::XMP::x | 1 | 1 | 0 | 1 | 0 | 0 | 0 | raw_key_unrepresentable: 1 |
| Image::ExifTool::XMP::xmp | 27 | 27 | 0 | 27 | 0 | 0 | 0 | raw_key_unrepresentable: 19 |
| Image::ExifTool::XMP::xmpBJ | 4 | 4 | 0 | 4 | 0 | 0 | 0 | raw_key_unrepresentable: 1 |
| Image::ExifTool::XMP::xmpDM | 161 | 161 | 0 | 161 | 0 | 0 | 0 | — |
| Image::ExifTool::XMP::xmpMM | 160 | 160 | 0 | 160 | 0 | 0 | 0 | raw_key_unrepresentable: 22 |
| Image::ExifTool::XMP::xmpNote | 1 | 1 | 0 | 0 | 0 | 0 | 0 | raw_key_unrepresentable: 1 |
| Image::ExifTool::XMP::xmpPLUS | 2 | 2 | 0 | 2 | 0 | 0 | 0 | — |
| Image::ExifTool::XMP::xmpRights | 5 | 5 | 0 | 5 | 0 | 0 | 0 | raw_key_unrepresentable: 5 |
| Image::ExifTool::XMP::xmpTPg | 52 | 52 | 0 | 52 | 0 | 0 | 0 | raw_key_unrepresentable: 13 |
| Image::ExifTool::ZIP::GZIP | 7 | 7 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::ZIP::Main | 10 | 9 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::ZIP::RAR | 7 | 6 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::ZIP::RAR5 | 6 | 6 | 0 | 0 | 0 | 0 | 0 | raw_key_unrepresentable: 6 |
| Image::ExifTool::ZISRAW::Main | 3 | 3 | 0 | 0 | 0 | 0 | 0 | — |
| Image::ExifTool::iWork::Main | 6 | 6 | 0 | 0 | 0 | 0 | 0 | — |

## Families

| Family | Status counts |
| --- | --- |
| * | source_row_joined: 1 |
| AAC | source_row_joined: 4 |
| AADAccelFeatures | source_row_joined: 5 |
| AC3 | source_row_joined: 4 |
| AFCP | source_row_joined: 3 |
| AIFF | source_row_joined: 15 |
| APE | source_row_joined: 9 |
| APP2 | source_row_joined: 1 |
| AROT | source_row_joined: 2 |
| ASF | source_row_joined: 219 |
| AVI1 | source_row_joined: 1 |
| AccelData | source_row_joined: 11 |
| Activity | source_row_joined: 7 |
| ActivityMetrics | source_row_joined: 19 |
| Adobe | source_row_joined: 5 |
| AdobeCM | source_row_joined: 1 |
| AdobeDNG | source_row_joined: 4 |
| AlarmSettings | source_row_joined: 8 |
| Alert | source_row_joined: 5 |
| AntChannelID | source_row_joined: 5 |
| AntRx | source_row_joined: 5 |
| AntTx | source_row_joined: 5 |
| Apple | source_row_joined: 45 |
| Audible | source_row_joined: 20 |
| AudioKeys | source_row_joined: 6 |
| AuxBattery | source_row_joined: 4 |
| AviationAttitude | source_row_joined: 11 |
| Barometer | source_row_joined: 3 |
| BeatIntervals | source_row_joined: 2 |
| BestEffort | source_row_joined: 5 |
| BikeProfile | source_row_joined: 31 |
| BloodPress | source_row_joined: 10 |
| CBOR | source_row_joined: 9 |
| CPEStatus | source_row_joined: 3 |
| CadenceZone | source_row_joined: 2 |
| CameraEvent | source_row_joined: 4 |
| CameraIFD | source_row_joined: 23 |
| Canon | source_row_joined: 1801 |
| CanonCustom | source_row_joined: 330 |
| CanonDR4 | source_row_joined: 106 |
| CanonRaw | source_row_joined: 66 |
| CanonVRD | source_row_joined: 206 |
| Capabilities | source_row_joined: 4 |
| Casio | source_row_joined: 125 |
| Chapter# | source_row_joined: 1 |
| ChronoShotData | source_row_joined: 2 |
| ChronoShotSession | source_row_joined: 7 |
| ClimbPro | source_row_joined: 6 |
| Clubs | source_row_joined: 2 |
| Composite | source_row_joined: 105 |
| ConnectIQField | source_row_joined: 4 |
| Connectivity | source_row_joined: 13 |
| Course | source_row_joined: 4 |
| CoursePoint | source_row_joined: 7 |
| DICOM | source_row_joined: 5669 |
| DJI | source_row_joined: 233 |
| DNG | source_row_joined: 17 |
| DV | source_row_joined: 13 |
| DataScreen | source_row_joined: 5 |
| DevDataID | source_row_joined: 5 |
| DeviceInfo | source_row_joined: 19 |
| DeviceSettings | source_row_joined: 27 |
| DeviceStatus | source_row_joined: 3 |
| DeviceUsed | source_row_joined: 6 |
| DiveAlarm | source_row_joined: 12 |
| DiveApneaAlarm | source_row_joined: 12 |
| DiveGas | source_row_joined: 4 |
| DiveSettings | source_row_joined: 33 |
| DiveSummary | source_row_joined: 22 |
| DjVu | source_row_joined: 8 |
| DjVu-Meta | source_row_joined: 34 |
| Ducky | source_row_joined: 3 |
| ECGRawSample | source_row_joined: 1 |
| ECGSmoothSample | source_row_joined: 1 |
| ECGSummary | source_row_joined: 7 |
| EPOStatus | source_row_joined: 3 |
| EXE | source_row_joined: 62 |
| EXIF | source_row_joined: 1 |
| EnduranceScore | source_row_joined: 8 |
| Event | source_row_joined: 18 |
| ExdDataConceptConfig | source_row_joined: 11 |
| ExdDataFieldConfig | source_row_joined: 6 |
| ExdScreenConfig | source_row_joined: 4 |
| ExerciseTitle | source_row_joined: 3 |
| ExifTool | source_row_joined: 22 |
| FITS | source_row_joined: 14 |
| FLAC | source_row_joined: 22 |
| FLIR | source_row_joined: 218 |
| FieldCaps | source_row_joined: 4 |
| FieldDescr | source_row_joined: 14 |
| File | source_row_joined: 449 |
| FileCaps | source_row_joined: 5 |
| FileCreator | source_row_joined: 2 |
| FileID | source_row_joined: 7 |
| Flash | source_row_joined: 55 |
| FlashPix | source_row_joined: 212 |
| Font | source_row_joined: 81 |
| FotoStation | source_row_joined: 12 |
| FujiFilm | source_row_joined: 147 |
| FujiIFD | source_row_joined: 12 |
| FunctionalMetrics | source_row_joined: 4 |
| GE | source_row_joined: 3 |
| GIF | source_row_joined: 21 |
| GIMP | source_row_joined: 11 |
| GM | source_row_joined: 101 |
| GPS | source_row_joined: 90 |
| GPSEvent | source_row_joined: 13 |
| GSpherical | source_row_joined: 1 |
| GeoTiff | source_row_joined: 64 |
| GoPro | source_row_joined: 155 |
| Goal | source_row_joined: 12 |
| GolfCourse | source_row_joined: 11 |
| GolfStats | source_row_joined: 7 |
| Google | source_row_joined: 43 |
| GraphConv | source_row_joined: 1 |
| GyroData | source_row_joined: 8 |
| H264 | source_row_joined: 27 |
| HP | source_row_joined: 15 |
| HR | source_row_joined: 5 |
| HRMProfile | source_row_joined: 4 |
| HRV | source_row_joined: 1 |
| HRVStatusSummary | source_row_joined: 7 |
| HRVValue | source_row_joined: 1 |
| HRZone | source_row_joined: 2 |
| HSAAccelData | source_row_joined: 6 |
| HSABodyBattery | source_row_joined: 4 |
| HSAConfigData | source_row_joined: 2 |
| HSAEvent | source_row_joined: 1 |
| HSAGyroData | source_row_joined: 6 |
| HSARespirData | source_row_joined: 2 |
| HSAStepData | source_row_joined: 2 |
| HSAStressData | source_row_joined: 2 |
| HSAWristTemp | source_row_joined: 2 |
| HSA_HRData | source_row_joined: 3 |
| HSA_SPO2Data | source_row_joined: 3 |
| HTML | source_row_joined: 25 |
| HTML-dc | source_row_joined: 15 |
| HTML-ncc | source_row_joined: 26 |
| HTML-office | source_row_joined: 21 |
| HTML-prod | source_row_joined: 2 |
| HTML-vw96 | source_row_joined: 1 |
| HTTP-equiv | source_row_joined: 22 |
| HillScore | source_row_joined: 4 |
| Hole | source_row_joined: 6 |
| ICC-chrm | source_row_joined: 6 |
| ICC-cicp | source_row_joined: 4 |
| ICC-clrt | source_row_joined: 7 |
| ICC-header | source_row_joined: 16 |
| ICC-meas | source_row_joined: 5 |
| ICC-meta | source_row_joined: 4 |
| ICC-view | source_row_joined: 3 |
| ICC_Profile | source_row_joined: 152 |
| ID3 | source_row_joined: 11 |
| ID3v1 | source_row_joined: 7 |
| ID3v1_Enh | source_row_joined: 7 |
| ID3v2_2 | source_row_joined: 64 |
| ID3v2_3 | source_row_joined: 81 |
| ID3v2_4 | source_row_joined: 84 |
| IFD0 | source_row_joined: 642 |
| IFD1 | source_row_joined: 2 |
| IPTC | source_row_joined: 117 |
| ISO | source_row_joined: 22 |
| ITC | source_row_joined: 8 |
| InfiRay | source_row_joined: 81 |
| ItemList | source_row_joined: 104 |
| JFIF | source_row_joined: 7 |
| JFXX | source_row_joined: 3 |
| JPEG | source_row_joined: 12 |
| JPEG-HDR | source_row_joined: 8 |
| JPS | source_row_joined: 6 |
| JSON | source_row_joined: 8 |
| JUMBF | source_row_joined: 6 |
| JVC | source_row_joined: 4 |
| Jpeg2000 | source_row_joined: 61 |
| Jump | source_row_joined: 9 |
| KDC_IFD | source_row_joined: 7 |
| KVAR | source_row_joined: 68 |
| Keys | source_row_joined: 77 |
| Kodak | source_row_joined: 195 |
| KodakBordersIFD | source_row_joined: 6 |
| KodakEffectsIFD | source_row_joined: 3 |
| KodakIFD | source_row_joined: 784 |
| KyoceraRaw | source_row_joined: 11 |
| LNK | source_row_joined: 131 |
| Lap | source_row_joined: 136 |
| Leaf | source_row_joined: 125 |
| Leica | source_row_joined: 70 |
| Length | source_row_joined: 20 |
| Location | source_row_joined: 7 |
| Lyrics3 | source_row_joined: 9 |
| Lytro | source_row_joined: 24 |
| M-RAW | source_row_joined: 4 |
| M2TS | source_row_joined: 3 |
| MAC | source_row_joined: 13 |
| MIE-Audio | source_row_joined: 8 |
| MIE-Camera | source_row_joined: 24 |
| MIE-Doc | source_row_joined: 14 |
| MIE-Extender | source_row_joined: 4 |
| MIE-Flash | source_row_joined: 8 |
| MIE-GPS | source_row_joined: 12 |
| MIE-Geo | source_row_joined: 5 |
| MIE-Image | source_row_joined: 10 |
| MIE-Lens | source_row_joined: 12 |
| MIE-Main | source_row_joined: 9 |
| MIE-Orient | source_row_joined: 5 |
| MIE-Preview | source_row_joined: 4 |
| MIE-Thumbnail | source_row_joined: 4 |
| MIE-UTM | source_row_joined: 4 |
| MIE-Video | source_row_joined: 5 |
| MIFF | source_row_joined: 29 |
| MISB | source_row_joined: 117 |
| MNG | source_row_joined: 106 |
| MOBI | source_row_joined: 52 |
| MOI | source_row_joined: 7 |
| MPC | source_row_joined: 11 |
| MPEG | source_row_joined: 32 |
| MPF0 | source_row_joined: 18 |
| MPImage | source_row_joined: 7 |
| MS-DOC | source_row_joined: 13 |
| MXF | source_row_joined: 1584 |
| MacOS | source_row_joined: 142 |
| MagData | source_row_joined: 8 |
| MakerNotes | source_row_joined: 13 |
| MapLayer | source_row_joined: 13 |
| Matroska | source_row_joined: 273 |
| MaxMetData | source_row_joined: 8 |
| MediaJukebox | source_row_joined: 9 |
| MemoGlob | source_row_joined: 5 |
| MesgCaps | source_row_joined: 4 |
| MetZone | source_row_joined: 3 |
| Meta | source_row_joined: 10 |
| MetaIFD | source_row_joined: 32 |
| Metronome | source_row_joined: 4 |
| Microsoft | source_row_joined: 459 |
| Minolta | source_row_joined: 300 |
| MinoltaRaw | source_row_joined: 41 |
| MonitorHRData | source_row_joined: 2 |
| MonitorInfo | source_row_joined: 5 |
| Monitoring | source_row_joined: 28 |
| Motorola | source_row_joined: 6 |
| MtbCx | source_row_joined: 2 |
| MultisportActivity | source_row_joined: 4 |
| MultisportSettings | source_row_joined: 10 |
| MusicInfo | source_row_joined: 5 |
| NITF | source_row_joined: 12 |
| NMEA | source_row_joined: 2 |
| NapEvent | source_row_joined: 8 |
| Nextbase | source_row_joined: 65 |
| Nikon | source_row_joined: 1396 |
| NikonCapture | source_row_joined: 99 |
| NikonCustom | source_row_joined: 1421 |
| NikonScan | source_row_joined: 12 |
| NikonSettings | source_row_joined: 234 |
| NineEdits | source_row_joined: 3 |
| Nintendo | source_row_joined: 5 |
| OBDIIData | source_row_joined: 8 |
| OHRSettings | source_row_joined: 1 |
| Ocad | source_row_joined: 1 |
| Olympus | source_row_joined: 445 |
| OneDSensorCal | source_row_joined: 5 |
| OnePlus | source_row_joined: 3 |
| OpenEXR | source_row_joined: 41 |
| OpenWaterEvent | source_row_joined: 2 |
| Opus | source_row_joined: 4 |
| PDF | source_row_joined: 47 |
| PICT | source_row_joined: 144 |
| PNG | source_row_joined: 65 |
| PNG-cICP | source_row_joined: 4 |
| PNG-pHYs | source_row_joined: 3 |
| PSP | source_row_joined: 17 |
| Palm | source_row_joined: 6 |
| Panasonic | source_row_joined: 173 |
| PanasonicRaw | source_row_joined: 38 |
| Parrot | source_row_joined: 91 |
| Pentax | source_row_joined: 570 |
| PersonalRecord | source_row_joined: 4 |
| PhaseOne | source_row_joined: 48 |
| PhotoCD | source_row_joined: 26 |
| PhotoMechanic | source_row_joined: 19 |
| Photoshop | source_row_joined: 112 |
| PictureInfo | source_row_joined: 27 |
| PostScript | source_row_joined: 24 |
| PowerMode | source_row_joined: 3 |
| PowerZone | source_row_joined: 2 |
| PreviewIFD | source_row_joined: 8 |
| PrintIM | source_row_joined: 1 |
| Qualcomm | source_row_joined: 1231 |
| QuickTime | source_row_joined: 455 |
| RAF | source_row_joined: 25 |
| RAF2 | source_row_joined: 1 |
| RIFF | source_row_joined: 197 |
| RMETA | source_row_joined: 7 |
| RSRC | source_row_joined: 6 |
| RTF | source_row_joined: 24 |
| Race | source_row_joined: 4 |
| RaceEvent | source_row_joined: 11 |
| Radiance | source_row_joined: 11 |
| RangeAlert | source_row_joined: 5 |
| RawBBI | source_row_joined: 5 |
| Rawzor | source_row_joined: 5 |
| Real | source_row_joined: 2 |
| Real-CONT | source_row_joined: 8 |
| Real-MDPR | source_row_joined: 35 |
| Real-PROP | source_row_joined: 11 |
| Real-RA3 | source_row_joined: 12 |
| Real-RA4 | source_row_joined: 31 |
| Real-RA5 | source_row_joined: 18 |
| Real-RJMD | source_row_joined: 4 |
| Reconyx | source_row_joined: 115 |
| Record | source_row_joined: 94 |
| Red | source_row_joined: 49 |
| RespirationRate | source_row_joined: 1 |
| Ricoh | source_row_joined: 98 |
| Routing | source_row_joined: 7 |
| SDMProfile | source_row_joined: 7 |
| SEAL | source_row_joined: 13 |
| SPIFF | source_row_joined: 11 |
| SPO2Data | source_row_joined: 3 |
| SR2 | source_row_joined: 3 |
| SR2DataIFD | source_row_joined: 1 |
| SR2SubIFD | source_row_joined: 35 |
| SRF# | source_row_joined: 28 |
| SVG | source_row_joined: 5 |
| Samsung | source_row_joined: 208 |
| Sanyo | source_row_joined: 46 |
| Scalado | source_row_joined: 4 |
| Schedule | source_row_joined: 7 |
| Score | source_row_joined: 4 |
| SegFile | source_row_joined: 8 |
| SegLap | source_row_joined: 93 |
| SegLeaderboard | source_row_joined: 6 |
| SegPoint | source_row_joined: 6 |
| SegmentID | source_row_joined: 9 |
| SensorSettings | source_row_joined: 12 |
| Session | source_row_joined: 179 |
| Set | source_row_joined: 10 |
| Shot | source_row_joined: 6 |
| Sigma | source_row_joined: 103 |
| SigmaRaw | source_row_joined: 69 |
| SkinTempOvernight | source_row_joined: 4 |
| SlaveDevice | source_row_joined: 2 |
| SleepAssessment | source_row_joined: 14 |
| SleepDataInfo | source_row_joined: 3 |
| SleepDisruptOvernight | source_row_joined: 1 |
| SleepDisruptPeriod | source_row_joined: 1 |
| SleepLevel | source_row_joined: 1 |
| SleepRestlessMoments | source_row_joined: 2 |
| SleepSchedule | source_row_joined: 2 |
| Software | source_row_joined: 2 |
| Sony | source_row_joined: 1208 |
| SonyIDC | source_row_joined: 53 |
| SpeedZone | source_row_joined: 2 |
| Split | source_row_joined: 42 |
| SplitSummary | source_row_joined: 25 |
| SplitTime | source_row_joined: 11 |
| Sport | source_row_joined: 10 |
| Stim | source_row_joined: 28 |
| StressLevel | source_row_joined: 3 |
| SubIFD | source_row_joined: 4 |
| System | source_row_joined: 21 |
| TSCorrelation | source_row_joined: 6 |
| TankSummary | source_row_joined: 4 |
| TankUpdate | source_row_joined: 2 |
| Theora | source_row_joined: 11 |
| ThreeDSensorCal | source_row_joined: 6 |
| TimeInZone | source_row_joined: 16 |
| Torrent | source_row_joined: 24 |
| Totals | source_row_joined: 9 |
| Track# | source_row_joined: 33 |
| TrainingFile | source_row_joined: 5 |
| TrainingLoad | source_row_joined: 2 |
| TrainingReadiness | source_row_joined: 3 |
| TrainingSettings | source_row_joined: 47 |
| UserData | source_row_joined: 149 |
| UserMetrics | source_row_joined: 16 |
| UserProfile | source_row_joined: 32 |
| VCalendar | source_row_joined: 65 |
| VCard | source_row_joined: 31 |
| VNote | source_row_joined: 4 |
| Video | source_row_joined: 3 |
| VideoClip | source_row_joined: 7 |
| VideoDescr | source_row_joined: 2 |
| VideoFrame | source_row_joined: 2 |
| VideoKeys | source_row_joined: 5 |
| VideoTitle | source_row_joined: 2 |
| Vivo | source_row_joined: 3 |
| Vorbis | source_row_joined: 37 |
| WTV | source_row_joined: 73 |
| WatchfaceSettings | source_row_joined: 2 |
| WaypointHandling | source_row_joined: 1 |
| WeatherAlert | source_row_joined: 5 |
| WeatherConditions | source_row_joined: 15 |
| WeightScale | source_row_joined: 13 |
| Workout | source_row_joined: 13 |
| WorkoutSchedule | source_row_joined: 6 |
| WorkoutSess | source_row_joined: 6 |
| WorkoutStep | source_row_joined: 20 |
| XML | source_row_joined: 161 |
| XMP | source_row_joined: 1 |
| XMP-DICOM | source_row_joined: 14 |
| XMP-Device | source_row_joined: 107 |
| XMP-GAudio | source_row_joined: 2 |
| XMP-GCamera | source_row_joined: 14 |
| XMP-GContainer | source_row_joined: 8 |
| XMP-GCreations | source_row_joined: 2 |
| XMP-GDepth | source_row_joined: 14 |
| XMP-GFocus | source_row_joined: 4 |
| XMP-GImage | source_row_joined: 2 |
| XMP-GPano | source_row_joined: 27 |
| XMP-GSpherical | source_row_joined: 16 |
| XMP-HDRGainMap | source_row_joined: 1 |
| XMP-LImage | source_row_joined: 3 |
| XMP-MP | source_row_joined: 8 |
| XMP-MP1 | source_row_joined: 15 |
| XMP-PixelLive | source_row_joined: 6 |
| XMP-aas | source_row_joined: 27 |
| XMP-acdsee | source_row_joined: 19 |
| XMP-acdsee-rs | source_row_joined: 19 |
| XMP-album | source_row_joined: 1 |
| XMP-apdi | source_row_joined: 3 |
| XMP-apple-fi | source_row_joined: 5 |
| XMP-ast | source_row_joined: 12 |
| XMP-aux | source_row_joined: 26 |
| XMP-cc | source_row_joined: 11 |
| XMP-cell | source_row_joined: 6 |
| XMP-crd | source_row_joined: 1093 |
| XMP-creatorAtom | source_row_joined: 14 |
| XMP-crs | source_row_joined: 1093 |
| XMP-dc | source_row_joined: 15 |
| XMP-dex | source_row_joined: 8 |
| XMP-digiKam | source_row_joined: 9 |
| XMP-drone-dji | source_row_joined: 28 |
| XMP-dwc | source_row_joined: 263 |
| XMP-et | source_row_joined: 3 |
| XMP-exif | source_row_joined: 100 |
| XMP-exifEX | source_row_joined: 42 |
| XMP-expressionmedia | source_row_joined: 4 |
| XMP-extensis | source_row_joined: 8 |
| XMP-fpv | source_row_joined: 1 |
| XMP-getty | source_row_joined: 20 |
| XMP-hdr | source_row_joined: 6 |
| XMP-hdrgm | source_row_joined: 9 |
| XMP-ics | source_row_joined: 30 |
| XMP-iptcCore | source_row_joined: 16 |
| XMP-iptcExt | source_row_joined: 251 |
| XMP-lr | source_row_joined: 3 |
| XMP-mediapro | source_row_joined: 6 |
| XMP-microsoft | source_row_joined: 12 |
| XMP-mwg-coll | source_row_joined: 3 |
| XMP-mwg-kw | source_row_joined: 19 |
| XMP-mwg-rs | source_row_joined: 21 |
| XMP-nine | source_row_joined: 5 |
| XMP-panorama | source_row_joined: 4 |
| XMP-pdf | source_row_joined: 12 |
| XMP-pdfx | source_row_joined: 1 |
| XMP-photomech | source_row_joined: 8 |
| XMP-photoshop | source_row_joined: 53 |
| XMP-plus | source_row_joined: 86 |
| XMP-pmi | source_row_joined: 34 |
| XMP-prism | source_row_joined: 112 |
| XMP-prl | source_row_joined: 3 |
| XMP-prm | source_row_joined: 19 |
| XMP-pur | source_row_joined: 14 |
| XMP-rdf | source_row_joined: 1 |
| XMP-sdc | source_row_joined: 4 |
| XMP-swf | source_row_joined: 4 |
| XMP-tiff | source_row_joined: 26 |
| XMP-x | source_row_joined: 1 |
| XMP-xmp | source_row_joined: 27 |
| XMP-xmpBJ | source_row_joined: 4 |
| XMP-xmpDM | source_row_joined: 161 |
| XMP-xmpDSA | source_row_joined: 10 |
| XMP-xmpMM | source_row_joined: 160 |
| XMP-xmpNote | source_row_joined: 1 |
| XMP-xmpPLUS | source_row_joined: 2 |
| XMP-xmpRights | source_row_joined: 5 |
| XMP-xmpTPg | source_row_joined: 52 |
| ZIP | source_row_joined: 28 |
| ZonesTarget | source_row_joined: 5 |
| iTunes | source_row_joined: 40 |
