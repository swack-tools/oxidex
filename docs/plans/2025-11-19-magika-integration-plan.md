# Magika AI-Powered File Type Detection Integration

**Created**: 2025-11-19
**Updated**: 2025-11-19
**Status**: Planning
**Author**: Claude Code
**Objective**: Add Google's Magika AI system as optional file detection enhancement

---

## Executive Summary

This plan proposes integrating Google's Magika deep-learning file type detection library as an **optional enhancement** to our current signature-based format detection system. Magika will be:

- **Opt-in via CLI flag**: `--use-magika` or `--detector=magika`
- **Optional cargo feature**: Enabled with `--features magika`
- **Zero breaking changes**: Existing behavior unchanged by default
- **Additive enhancement**: 200+ content types available on demand

**Key Benefits**:
- **200+ content types** vs. our current 38 signatures (when enabled)
- **~99% accuracy** on binary and textual formats
- **1000 files/sec** processing speed on modern hardware
- **Better textual file detection** than traditional libmagic
- **Native Rust implementation** with zero-cost abstractions

**Expected Impact**:
- Provide power users with advanced AI-based detection
- Support 5x more file formats for those who need it
- Maintain fast, lightweight default behavior
- Enable future ML-based metadata extraction
- Lower risk migration path (no forced changes)

---

## Background & Motivation

### Current State

**File**: `src/parsers/format_detector.rs` (1,059 lines)
- 38 manually-defined signatures
- Simple byte-pattern matching at fixed offsets
- Limited to common formats (JPEG, TIFF, PNG, PDF, etc.)
- No support for complex detection heuristics
- Requires manual updates for new formats

**Limitations**:
1. **Coverage Gap**: Only 38 formats vs. 200+ industry standard
2. **Text File Blindness**: Poor at distinguishing source code, configs, scripts
3. **Maintenance Burden**: Each new format requires manual signature research
4. **False Positives**: Simple patterns can misidentify similar formats
5. **No Context Awareness**: Cannot use content semantics for detection

### Why Magika?

**Google's Proven Solution**:
- Used at scale in Gmail, Drive, Safe Browsing
- Processes "hundreds of billions of samples weekly"
- Open-sourced November 2025 as Magika 1.0
- Rewritten from Python to Rust for performance

**Technical Advantages**:
1. **AI-Powered**: Deep learning model trained on 100M samples
2. **Fast**: ~5ms per file, 1000 files/sec on laptop
3. **Lightweight**: Model weighs only ~5MB
4. **Accurate**: 99% precision/recall on test sets
5. **Maintained**: Backed by Google Security Research team

---

## Technical Analysis

### Magika Architecture

```
Input File → Sample Extraction → Deep Learning Model → Content Type
              (start/mid/end)      (ONNX Runtime)       (200+ types)
```

**Key Components**:
1. **Sampling Strategy**: Extracts 3×512 bytes (start, middle, end)
2. **Model**: Optimized neural network (~5MB ONNX model)
3. **Inference Engine**: ONNX Runtime for fast CPU inference
4. **Async Processing**: Tokio for parallel file scanning

**Performance Characteristics**:
- **Cold Start**: ~100ms (model loading)
- **Warm Inference**: ~5ms per file
- **Throughput**: 1000 files/sec (MacBook M4)
- **Memory**: ~50MB (model + runtime)
- **CPU**: Single-core capable, scales to multi-core

### Magika Rust API

```rust
// Initialize session (reusable, thread-safe)
let magika = magika::Session::new()?;

// Identify from file path
let result = magika.identify_file_sync("image.jpg")?;
println!("Type: {}", result.info().label); // "jpeg"

// Identify from byte content
let bytes = std::fs::read("script.sh")?;
let result = magika.identify_content_sync(&bytes)?;
println!("Type: {}", result.info().label); // "shell"
```

**Structs**:
- `Session`: Main detection engine (thread-safe, reusable)
- `Builder`: Configuration (model path, thresholds)
- `InferredType`: Result with type info + confidence
- `TypeInfo`: Metadata (label, MIME type, extensions)

**Dependencies**:
- `ort` (ONNX Runtime): Model inference
- `tokio`: Async I/O and parallelism
- `serde`: Optional JSON serialization

---

## Current Implementation Analysis

### Existing Format Detection

**Architecture** (`src/parsers/format_detector.rs`):

```rust
pub enum FileFormat {
    JPEG,
    TIFF,
    PNG,
    // ... 38 total formats
}

pub fn detect_format(reader: &dyn FileReader) -> io::Result<FileFormat> {
    // Read magic bytes
    // Match against signature table
    // Return FileFormat variant
}
```

**Detection Logic**:
1. Read first 512 bytes via `FileReader`
2. Check each signature in `SIMPLE_SIGNATURES` table
3. Special handling for complex formats (QuickTime, RIFF)
4. Return `FileFormat::Unknown` if no match

**Integration Points**:
- Used by: `src/core/oxidex.rs` (main entry point)
- Depends on: `FileReader` trait (abstraction over I/O)
- Returns: `FileFormat` enum (parsed by format-specific parsers)

### Format Mapping Required

| Current FileFormat | Magika Label | Notes |
|-------------------|--------------|-------|
| `JPEG` | `"jpeg"` | Direct mapping |
| `TIFF` | `"tiff"` | Direct mapping |
| `PNG` | `"png"` | Direct mapping |
| `PDF` | `"pdf"` | Direct mapping |
| `FLAC` | `"flac"` | Direct mapping |
| `HEIC` | `"heic"` | New support |
| `AVIF` | `"avif"` | New support |
| `WebP` | `"webp"` | New support |
| `MP4` / `QuickTime` | `"mp4"` / `"mov"` | Magika distinguishes |
| `AVI` | `"avi"` | New support |
| `MKV` | `"mkv"` | New support |
| `Unknown` | Various | 200+ new types |

---

## Integration Strategy

### Chosen Approach: Optional Opt-In Feature (Recommended)

**Strategy**: Keep signature-based detection as default, add Magika as opt-in via CLI flag and cargo feature

**Benefits**:
- **Zero breaking changes** - existing users unaffected
- **No binary size increase** by default (Magika is optional feature)
- **Lower risk** - new feature doesn't disturb existing functionality
- **User choice** - power users opt into AI detection
- **Gradual adoption** - can become default in future if proven
- **Easy to maintain** - both paths independent

**Implementation**:
```rust
pub enum DetectionMode {
    Signature,  // Default: fast, 38 formats
    Magika,     // Opt-in: AI-powered, 200+ formats
}

pub fn detect_format(
    reader: &dyn FileReader,
    mode: DetectionMode,
) -> io::Result<FileFormat> {
    match mode {
        DetectionMode::Signature => detect_with_signatures(reader),
        #[cfg(feature = "magika")]
        DetectionMode::Magika => detect_with_magika(reader),
        #[cfg(not(feature = "magika"))]
        DetectionMode::Magika => Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "Magika not compiled in (build with --features magika)"
        )),
    }
}
```

**CLI Integration**:
```bash
# Default: fast signature-based detection
oxidex image.jpg

# Opt-in: AI-powered detection
oxidex --use-magika image.jpg
oxidex --detector=magika image.jpg

# Compare both methods
oxidex --detector=both image.jpg
```

**Cargo.toml**:
```toml
[features]
default = []
magika = ["dep:magika", "dep:ort", "dep:tokio"]

[dependencies]
magika = { version = "0.1", optional = true }
ort = { version = "2.0", optional = true }
tokio = { version = "1.0", features = ["rt"], optional = true }
```

**Why This Approach?**:
1. **No Risk to Existing Users**: Default behavior unchanged
2. **No Binary Bloat**: Users who don't need Magika don't pay 5MB cost
3. **Easy Testing**: Can A/B test both detection methods
4. **Marketing**: "Now with optional AI-powered detection!"
5. **Future Path**: Can make default if successful

### Alternative Approach: Automatic Fallback

**Strategy**: Try Magika if available, fall back to signatures automatically

**Implementation**:
```rust
pub fn detect_format(reader: &dyn FileReader) -> io::Result<FileFormat> {
    #[cfg(feature = "magika")]
    {
        if let Some(magika) = MAGIKA.as_ref() {
            if let Ok(format) = detect_with_magika(reader, magika) {
                return Ok(format);
            }
        }
    }

    // Always fall back to signature-based detection
    detect_with_signatures(reader)
}
```

**Pros**: Seamless experience, tries AI if available
**Cons**: Less predictable, harder to debug, no user control

**Decision**: Not recommended - prefer explicit opt-in for clarity

---

## Implementation Plan

### Phase 1: Proof of Concept

**Goal**: Validate Magika integration as optional parallel detection path

**Tasks**:
1. Add `magika`, `ort`, and `tokio` as **optional** dependencies to `Cargo.toml`
2. Create `src/parsers/magika_detector.rs` (completely separate from existing code)
3. Implement Magika → `FileFormat` enum mapping for existing 38 formats
4. Test detection on 10 sample files (JPEG, PNG, TIFF, etc.)
5. Benchmark performance vs. signature-based detection
6. Verify zero impact on existing tests (no changes to existing code)

**Acceptance Criteria**:
- [ ] Magika correctly maps to existing 38 `FileFormat` variants
- [ ] Processing speed < 10ms per file (warm)
- [ ] Memory usage < 100MB (including model)
- [ ] **Zero changes to existing detection code**
- [ ] **All existing tests pass without modification**
- [ ] Feature compiles with `--features magika` and without

**Deliverables**:
- `src/parsers/magika_detector.rs` with isolated implementation
- Magika → FileFormat mapping function
- Performance benchmark comparison
- Go/no-go decision

**Implementation Notes**:
```rust
// New file: src/parsers/magika_detector.rs
#[cfg(feature = "magika")]
use magika::Session;

#[cfg(feature = "magika")]
pub fn detect_with_magika(data: &[u8]) -> io::Result<FileFormat> {
    let session = Session::new()
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

    let result = session.identify_content_sync(data)
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

    // Map Magika label to our existing FileFormat enum
    magika_label_to_format(result.info().label)
}

fn magika_label_to_format(label: &str) -> io::Result<FileFormat> {
    match label {
        "jpeg" => Ok(FileFormat::JPEG),
        "tiff" => Ok(FileFormat::TIFF),
        "png" => Ok(FileFormat::PNG),
        "pdf" => Ok(FileFormat::PDF),
        // ... map all 38 existing formats
        _ => Ok(FileFormat::Unknown), // Graceful fallback for unsupported types
    }
}
```

### Phase 2: CLI Integration

**Goal**: Add `--detector` CLI flag to choose between signature and Magika detection

**Tasks**:
1. Add `--detector` flag to CLI argument parser (default: `signature`)
2. Wire CLI flag to call either `detect_format()` or `detect_with_magika()`
3. Keep existing `detect_format()` completely unchanged
4. Add helpful error message if `--detector=magika` used without feature enabled
5. Update CLI help text to document the new flag

**Code Changes**:

**`Cargo.toml`**:
```toml
[features]
default = []
magika = ["dep:magika", "dep:ort", "dep:tokio"]

[dependencies]
# Existing dependencies unchanged...

# Optional Magika dependencies
magika = { version = "0.1", optional = true }
ort = { version = "2.0", optional = true }
tokio = { version = "1.0", features = ["rt"], optional = true }
```

**`src/main.rs` (or CLI entry point)**:
```rust
#[derive(clap::ValueEnum, Clone)]
enum DetectorMode {
    Signature,  // Default
    Magika,     // Opt-in
}

#[derive(clap::Parser)]
struct Args {
    #[arg(long, default_value = "signature")]
    detector: DetectorMode,

    // ... existing args
}

fn main() {
    let args = Args::parse();

    let format = match args.detector {
        DetectorMode::Signature => {
            // Use existing detection (unchanged)
            detect_format(&reader)?
        }
        #[cfg(feature = "magika")]
        DetectorMode::Magika => {
            // Use new Magika detection
            magika_detector::detect_with_magika(&data)?
        }
        #[cfg(not(feature = "magika"))]
        DetectorMode::Magika => {
            eprintln!("Error: Magika support not compiled in.");
            eprintln!("Rebuild with: cargo build --features magika");
            std::process::exit(1);
        }
    };
}
```

**Acceptance Criteria**:
- [ ] `oxidex image.jpg` works exactly as before (default signature detection)
- [ ] `oxidex --detector=signature image.jpg` uses signature detection
- [ ] `oxidex --detector=magika image.jpg` uses Magika (when compiled with feature)
- [ ] Clear error message when Magika requested but not compiled
- [ ] CLI help shows available detector options

**Deliverables**:
- CLI flag implementation
- Feature gate implementation
- Updated CLI help text
- Integration tests for both detection modes

### Phase 3: Testing & Validation

**Goal**: Verify Magika detection accuracy and performance on test files

**Test Strategy**:
1. **Regression**: Verify all existing tests still pass (no changes to existing code)
2. **Magika Accuracy**: Test Magika detection on existing 38 format types
3. **Edge Cases**: Test small files, corrupted files, edge cases
4. **Performance**: Benchmark Magika vs. signature detection
5. **Feature Flag**: Verify builds work with and without `--features magika`

**Test Files**:
- Use existing test corpus (JPEG, PNG, TIFF, PDF, etc.)
- Add a few additional samples for formats Magika supports well
- Keep test corpus small and focused (10-20 files, not 200+)

**Metrics**:
- **Accuracy**: Does Magika correctly map to existing FileFormat variants?
- **Speed**: Magika detection time vs. signature detection time
- **Memory**: Peak memory with model loaded
- **Compilation**: Both feature configurations build successfully

**Tasks**:
1. Create `tests/magika_detection.rs` integration test (feature-gated)
2. Test all 38 existing formats map correctly
3. Benchmark performance difference
4. Document performance characteristics
5. Test error handling (model missing, corrupted files)

**Acceptance Criteria**:
- [ ] 100% accuracy mapping existing 38 formats
- [ ] <10ms per file detection time (warm)
- [ ] <100MB memory usage
- [ ] **All existing tests pass unchanged**
- [ ] Compiles with and without `--features magika`
- [ ] Clear error messages for misconfiguration

**Deliverables**:
- Feature-gated integration tests
- Performance benchmark results
- Documentation of supported formats
- Error handling verification

### Phase 4: Documentation

**Goal**: Document the new optional Magika feature

**Tasks**:
1. Update README with "Optional AI Detection" section
2. Add build instructions (`cargo build --features magika`)
3. Document CLI flag usage (`--detector=magika`)
4. Add performance comparison notes
5. Create troubleshooting guide
6. Update CHANGELOG

**Documentation Additions**:

**README.md**:
```markdown
## Optional AI-Powered Detection

Oxidex supports optional AI-powered file type detection using Google's Magika library.

### Building with Magika

```bash
cargo build --features magika --release
```

### Usage

```bash
# Default: Fast signature-based detection
oxidex image.jpg

# AI-powered detection (requires --features magika)
oxidex --detector=magika image.jpg
```

### Performance

- **Signature detection**: <1μs per file (default)
- **Magika detection**: ~5ms per file (opt-in)

Magika provides higher accuracy for complex formats but has a performance trade-off.
```

**Acceptance Criteria**:
- [ ] README documents feature flag and usage
- [ ] Build instructions clear and tested
- [ ] Performance characteristics documented
- [ ] Troubleshooting section added
- [ ] CHANGELOG updated with new feature

**Deliverables**:
- Updated README.md
- Updated CHANGELOG.md
- Optional: Blog post or announcement

---

## Performance Considerations

### Benchmarks: Signature vs. Magika

**Hypothesis**:
- Signatures: <1μs per file (simple byte comparison)
- Magika: ~5ms per file (neural network inference)

**Trade-off**:
- 5000× slower per file
- BUT: 5× more formats, 99% accuracy, no maintenance

**Mitigation**:
1. **Caching**: Cache results for identical files (hash-based)
2. **Batch Processing**: Process multiple files in parallel
3. **Fast Path**: Quick check for common formats first
4. **Lazy Loading**: Defer model loading until needed

### Memory Impact

**Current**: ~10KB (signature table)
**With Magika**: ~50MB (model + runtime)

**Mitigation**:
1. **Lazy Initialization**: Only load model when first needed
2. **Shared Model**: Single model instance across threads
3. **Feature Flag**: Allow disabling Magika for memory-constrained envs

### Model Distribution

**Challenge**: 5MB ONNX model file

**Options**:

**A) Bundle in Binary** (Recommended)
- Include model in `assets/` directory
- Use `include_bytes!()` macro
- Increases binary size by 5MB

**B) Download on First Use**
- Download from GitHub releases
- Cache in `~/.oxidex/models/`
- Requires network access

**C) System Package**
- Package model separately (DEB, RPM, Homebrew)
- Oxidex finds via `MODEL_PATH` env var
- More complex distribution

**Recommendation**: Bundle for simplicity, document download option

---

## Testing Strategy

### Unit Tests

**New Tests Required**:
1. `test_magika_initialization()` - Model loads successfully
2. `test_magika_jpeg_detection()` - Correctly identifies JPEG
3. `test_magika_python_detection()` - Correctly identifies Python source
4. `test_magika_unknown_format()` - Handles unknown formats
5. `test_magika_corrupted_file()` - Graceful error handling
6. `test_file_reader_samples()` - Sampling logic correct
7. `test_format_mapping()` - Magika → FileFormat conversion

### Integration Tests

**Test Files** (200+ samples):
```
tests/fixtures/formats/
├── images/
│   ├── photo.jpg
│   ├── diagram.png
│   ├── logo.webp
│   └── ... (20 more)
├── videos/
│   ├── clip.mp4
│   ├── movie.mkv
│   └── ... (15 more)
├── documents/
│   ├── report.pdf
│   ├── sheet.xlsx
│   └── ... (10 more)
└── code/
    ├── script.py
    ├── module.rs
    └── ... (50 more)
```

**Test Scenarios**:
1. Detect all 200+ formats correctly
2. Handle edge cases (empty, huge, corrupted)
3. Performance under load (10k files)
4. Memory usage monitoring
5. Thread safety (parallel detection)

### Regression Tests

**Ensure No Breakage**:
1. All existing oxidex tests pass
2. CLI still works (`oxidex image.jpg`)
3. Library API unchanged (or documented)
4. Performance within acceptable range

### Benchmark Tests

**Measure**:
1. Cold start time (first detection)
2. Warm detection time (subsequent)
3. Throughput (files/sec)
4. Memory usage (peak)
5. vs. Signature-based (comparison)

**Tools**:
- Criterion for micro-benchmarks
- Hyperfine for CLI benchmarks
- Valgrind/Heaptrack for memory profiling

---

## Risks & Mitigation

### Risk 1: Model Size Bloat

**Risk**: 5MB model increases binary size significantly

**Impact**: Larger downloads, slower installation

**Mitigation**:
1. Compress model with Zstd (reduces to ~3MB)
2. Offer separate `oxidex-lite` build without Magika
3. Document model as optional download

**Likelihood**: High
**Severity**: Low

### Risk 2: Performance Regression

**Risk**: 5ms detection vs. 1μs is 5000× slower

**Impact**: CLI feels slower on single files

**Mitigation**:
1. Implement result caching for repeated files
2. Fast-path common formats with signature check
3. Parallelize batch operations with Tokio
4. Document performance characteristics

**Likelihood**: High
**Severity**: Medium

### Risk 3: Dependency Bloat

**Risk**: Adding ONNX Runtime + Tokio increases deps

**Impact**: Longer compile times, larger binary

**Mitigation**:
1. Use feature flags to make optional
2. Ensure `ort` uses system libraries when available
3. Profile compile times and optimize

**Likelihood**: Medium
**Severity**: Low

### Risk 4: Magika Accuracy Issues

**Risk**: AI model misidentifies files (false positives)

**Impact**: Wrong parsers invoked, metadata extraction fails

**Mitigation**:
1. Extensive testing on corpus (10k+ files)
2. Confidence threshold filtering (reject low-confidence)
3. Fallback to signature-based for critical formats
4. User feedback mechanism for misidentifications

**Likelihood**: Low
**Severity**: High

### Risk 5: Model Maintenance

**Risk**: Magika model becomes outdated or deprecated

**Impact**: Detection accuracy degrades over time

**Mitigation**:
1. Monitor Magika releases for model updates
2. Implement model versioning system
3. Auto-update model on new releases
4. Contribute improvements back to Magika project

**Likelihood**: Medium
**Severity**: Medium

### Risk 6: Platform Compatibility

**Risk**: ONNX Runtime may not work on all platforms

**Impact**: Oxidex breaks on unsupported systems

**Mitigation**:
1. Test on Linux, macOS, Windows, BSD
2. Fallback to signature-based on unsupported platforms
3. Document platform requirements clearly
4. Provide pre-built binaries with bundled runtime

**Likelihood**: Low
**Severity**: High

---

## Timeline & Milestones

### 4-Phase Implementation Schedule

| Phase | Focus | Milestone | Deliverables |
|-------|-------|-----------|--------------|
| 1 | POC | Magika Validated | Isolated magika_detector.rs, benchmarks |
| 2 | CLI | Integration Complete | CLI flag, feature gates working |
| 3 | Testing | Validation Complete | Tests pass, benchmarks documented |
| 4 | Docs | Ready for Use | README updated, CHANGELOG updated |

**Estimated Duration**: 2-3 weeks (depending on testing depth)

### Key Decision Points

**Phase 1 (Go/No-Go)**: Does Magika meet performance & accuracy requirements?
**Phase 3 (Quality Gate)**: Are benchmarks acceptable for production use?
**Phase 4 (Launch)**: Documentation complete and ready for users?

---

## Success Criteria

### Must-Have (Release Blockers)

- [ ] Magika correctly maps all 38 existing formats to `FileFormat` enum
- [ ] **All existing tests pass without modification**
- [ ] **Zero changes to existing signature-based detection code**
- [ ] Compiles successfully with and without `--features magika`
- [ ] CLI flag works correctly (`--detector=magika`)
- [ ] No memory leaks or crashes when using Magika
- [ ] Documentation complete (README, CHANGELOG)
- [ ] Clear error message when Magika requested but not compiled

### Should-Have (Quality Goals)

- [ ] <10ms per file detection time (Magika, warm inference)
- [ ] <100MB memory usage (with model loaded)
- [ ] Performance benchmarks comparing signature vs. Magika
- [ ] Integration tests for both detection modes
- [ ] Troubleshooting guide for common issues

### Nice-to-Have (Future Work)

- [ ] Support additional formats beyond current 38 (expand FileFormat enum)
- [ ] Confidence scores in CLI output
- [ ] Model auto-update system
- [ ] Hybrid detection mode (signature + Magika verification)
- [ ] Batch processing optimizations

---

## Future Enhancements

### Phase 2 Ideas

1. **Content Analysis**: Use Magika features for metadata extraction
2. **Custom Models**: Train models for domain-specific formats
3. **Hybrid Detection**: Combine signature + AI for best accuracy
4. **Streaming Detection**: Identify files from partial data
5. **Confidence Thresholds**: Configurable confidence levels

### Integration Opportunities

- **Virus Scanning**: Feed detected types to security scanners
- **OCR Pipeline**: Trigger OCR on detected document images
- **Video Transcoding**: Auto-select codec based on video format
- **Metadata Enrichment**: Enhance tags based on content type

---

## References

### Documentation

- [Magika GitHub](https://github.com/google/magika)
- [Magika 1.0 Announcement](https://opensource.googleblog.com/2025/11/announcing-magika-10-now-faster-smarter.html)
- [magika crate docs](https://docs.rs/magika)
- [ONNX Runtime](https://onnxruntime.ai/)

### Research

- Magika: AI-Powered Fast and Efficient File Type Identification (Google Research, 2024)
- Deep Learning for Malware Detection (various papers)

### Related Work

- libmagic: Traditional signature-based detection
- Apache Tika: Java-based content detection
- file command: Unix file type identifier

---

## Appendix

### A. Magika Format Coverage

**Categories** (20+):
- Images: 20+ formats
- Videos: 15+ formats
- Audio: 12+ formats
- Documents: 10+ formats
- Archives: 8+ formats
- Code/Text: 50+ languages
- Executables: 10+ binary formats
- Databases: 5+ formats
- Fonts: 5+ formats
- CAD: 3+ formats

See: https://github.com/google/magika/blob/main/docs/supported_types.md

### B. Performance Benchmark Plan

**Hardware**: MacBook Pro M4 (2025)

**Tests**:
1. Single file detection (cold start)
2. Single file detection (warm)
3. Batch 100 files
4. Batch 1000 files
5. Batch 10,000 files

**Metrics**:
- Latency (p50, p95, p99)
- Throughput (files/sec)
- Memory (peak, average)
- CPU (% utilization)

**Tools**: Criterion, Hyperfine, Heaptrack

### C. Model File Handling

**Bundling Strategy**:
```rust
// Embed model at compile time
const MODEL_BYTES: &[u8] = include_bytes!(
    "../assets/magika_model.onnx"
);

static MAGIKA: Lazy<Session> = Lazy::new(|| {
    Builder::new()
        .model_bytes(MODEL_BYTES)
        .build()
        .expect("Failed to load Magika model")
});
```

**Download Strategy**:
```rust
fn ensure_model_downloaded() -> io::Result<PathBuf> {
    let model_path = dirs::cache_dir()
        .ok_or(...)?
        .join("oxidex/magika_model.onnx");

    if !model_path.exists() {
        download_model(&model_path)?;
    }

    Ok(model_path)
}
```

---

## Conclusion

Adding Magika as an **optional feature** offers significant benefits for power users:
- **AI-powered accuracy** (~99% for complex formats)
- **Industry-proven** (used in Gmail, Drive, Safe Browsing)
- **Rust-native** (fast, safe integration)
- **Zero breaking changes** (opt-in via CLI flag and cargo feature)
- **Low risk** (existing users completely unaffected)

The 4-phase implementation plan provides a structured path with clear milestones and minimal scope. By keeping Magika opt-in:
- Default users get fast, lightweight signature detection
- Power users can enable AI detection when needed
- No forced migration or breaking changes
- Future flexibility to expand FileFormat enum if desired

**Recommendation**: Proceed with Phase 1 POC to validate Magika integration and performance characteristics before committing to full implementation.
