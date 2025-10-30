# Task Briefing Package

This package contains all necessary information and strategic guidance for the Coder Agent.

---

## 1. Current Task Details

This is the full specification of the task you must complete.

```json
{
  "task_id": "I5.T6",
  "iteration_id": "I5",
  "iteration_goal": "Implement C FFI bindings for cross-language integration, automate tag database generation from ExifTool specs, set up cross-compilation and release builds, create comprehensive documentation, and polish for v1.0 release.",
  "description": "Configure cross-compilation for Linux, macOS, Windows, and ARM targets using cross tool. Create Cross.toml configuration. Set up GitHub Actions release workflow in .github/workflows/release.yml to build binaries for: (1) x86_64-unknown-linux-musl (static Linux), (2) x86_64-apple-darwin (macOS Intel), (3) aarch64-apple-darwin (macOS ARM), (4) x86_64-pc-windows-gnu (Windows), (5) aarch64-unknown-linux-musl (Linux ARM). Apply optimizations: LTO, strip symbols, UPX compression (optional). Upload artifacts to GitHub Releases on git tag push.",
  "agent_type_hint": "SetupAgent",
  "inputs": "cross tool documentation, GitHub Actions best practices",
  "target_files": ["Cross.toml", ".github/workflows/release.yml", "Cargo.toml"],
  "input_files": ["Cargo.toml"],
  "deliverables": "Cross-compilation configuration, GitHub Actions release workflow, optimized release binaries",
  "acceptance_criteria": "Cross.toml exists with target configurations, release workflow builds for all 5 targets, binaries are statically linked (no external dependencies), release profile has: lto = true, codegen-units = 1, opt-level = z (size) or 3 (speed), binaries stripped of debug symbols, workflow uploads binaries to GitHub Releases on tag push (test with manual tag), binary sizes: Linux ~8MB, Windows ~9MB, macOS ~10MB (approximate)",
  "dependencies": [],
  "parallelizable": true,
  "done": false
}
```

---

## 2. Architectural & Planning Context

The following are the relevant sections from the architecture and plan documents, which I found by analyzing the task description.

### Context: deployment-view (from 05_Operational_Architecture.md)

```markdown
### 3.9. Deployment View

#### Target Environment

**Primary**: **Local developer machines and CI/CD pipelines**

**Supported Platforms**:

| **OS** | **Architecture** | **Distribution Method** |
|--------|-----------------|------------------------|
| Linux | x86_64, aarch64 | Static binary, `.deb`/`.rpm` packages, cargo install |
| macOS | x86_64 (Intel), aarch64 (Apple Silicon) | Homebrew formula, static binary |
| Windows | x86_64 | `.exe` installer, `scoop`/`chocolatey` packages, static binary |
| FreeBSD | x86_64 | Cargo install, ports tree |
| WebAssembly | wasm32-unknown-unknown | NPM package (`@exiftool-rs/wasm`) |

**Secondary**: **Embedded in larger applications** (via Rust crate or FFI bindings)
```

### Context: deployment-strategy (from 05_Operational_Architecture.md)

```markdown
#### Deployment Strategy

**Distribution Models**:

1. **Standalone Binary** (Primary):
   - Single static executable with no dependencies
   - Cross-compiled via `cross` tool for all platforms
   - Distributed via GitHub Releases with checksums
   - Size: ~8MB (stripped, LTO, compressed with UPX)

2. **Rust Crate** (Library):
   - Published to crates.io as `exiftool-rs`
   - Applications add `exiftool-rs = "1.0"` to `Cargo.toml`
   - Compiled into consuming application's binary

3. **C FFI Shared Library**:
   - `libexiftool_rs.so` (Linux), `.dylib` (macOS), `.dll` (Windows)
   - Header file generated via `cbindgen`
   - Used by Python (`ctypes`), Node.js (`ffi-napi`), Go (`cgo`)

4. **Container Image** (Optional):
   - Minimal Alpine Linux image with static binary
   - Size: ~15MB total
   - Example: `docker run exiftool-rs/exiftool:latest photo.jpg`

**Build Process**:

```yaml
# GitHub Actions workflow
name: Release Build

on:
  push:
    tags: ['v*']

jobs:
  build-matrix:
    strategy:
      matrix:
        target:
          - x86_64-unknown-linux-musl
          - x86_64-apple-darwin
          - aarch64-apple-darwin
          - x86_64-pc-windows-gnu

    steps:
      - uses: actions/checkout@v3
      - uses: dtolnay/rust-toolchain@stable
      - uses: cross-rs/cross@v1

      - run: cross build --release --target ${{ matrix.target }}
      - run: strip target/${{ matrix.target }}/release/exiftool-rs  # Reduce size
      - run: upx target/${{ matrix.target }}/release/exiftool-rs    # Compress

      - uses: actions/upload-artifact@v3
        with:
          name: exiftool-rs-${{ matrix.target }}
          path: target/${{ matrix.target }}/release/exiftool-rs
```
```

### Context: task-i5-t6 (from 02_Iteration_I5.md)

```markdown
*   **Task 5.6: Set Up Cross-Compilation with cross**
    *   **Task ID:** `I5.T6`
    *   **Description:** Configure cross-compilation for Linux, macOS, Windows, and ARM targets using `cross` tool. Create `Cross.toml` configuration. Set up GitHub Actions release workflow in `.github/workflows/release.yml` to build binaries for: (1) x86_64-unknown-linux-musl (static Linux), (2) x86_64-apple-darwin (macOS Intel), (3) aarch64-apple-darwin (macOS ARM), (4) x86_64-pc-windows-gnu (Windows), (5) aarch64-unknown-linux-musl (Linux ARM). Apply optimizations: LTO, strip symbols, UPX compression (optional). Upload artifacts to GitHub Releases on git tag push.
    *   **Agent Type Hint:** `SetupAgent`
    *   **Inputs:** `cross` tool documentation, GitHub Actions best practices
    *   **Input Files:** [`Cargo.toml`]
    *   **Target Files:**
        *   `Cross.toml`
        *   `.github/workflows/release.yml`
        *   `Cargo.toml` (add release profile optimizations: lto, codegen-units)
    *   **Deliverables:**
        *   Cross-compilation configuration
        *   GitHub Actions release workflow
        *   Optimized release binaries
    *   **Acceptance Criteria:**
        *   Cross.toml exists with target configurations
        *   Release workflow builds for all 5 targets
        *   Binaries are statically linked (no external dependencies)
        *   Release profile has: `lto = true`, `codegen-units = 1`, `opt-level = "z"` (size) or `"3"` (speed)
        *   Binaries stripped of debug symbols
        *   Workflow uploads binaries to GitHub Releases on tag push (test with manual tag)
        *   Binary sizes: Linux ~8MB, Windows ~9MB, macOS ~10MB (approximate)
    *   **Dependencies:** `I1` (project setup)
    *   **Parallelizable:** Yes (can be set up anytime)
```

---

## 3. Codebase Analysis & Strategic Guidance

The following analysis is based on my direct review of the current codebase. Use these notes and tips to guide your implementation.

### Relevant Existing Code

*   **File:** `Cargo.toml`
    *   **Summary:** This is the main Cargo manifest for the exiftool-rs project. It defines package metadata, library/binary configuration, dependencies, build dependencies, dev dependencies, and profiles.
    *   **Current Release Profile:** The `[profile.release]` section ALREADY has excellent optimization settings: `opt-level = 3`, `lto = true`, `codegen-units = 1`, `strip = true`. You DO NOT need to modify these settings - they already meet the acceptance criteria.
    *   **Crate Types:** The library is configured with `crate-type = ["lib", "staticlib", "cdylib"]`, which supports both static and dynamic linking for FFI use cases.
    *   **Recommendation:** You SHOULD keep the existing release profile as-is. It already satisfies the requirement for LTO, codegen-units, and stripping.

*   **File:** `.github/workflows/ci.yml`
    *   **Summary:** This is the existing CI workflow that runs tests, clippy, formatting checks, cbindgen verification, security audit, and code coverage on all platforms (ubuntu-latest, macos-latest, windows-latest).
    *   **Structure:** The workflow is well-organized with separate jobs for `test`, `audit`, and `coverage`. It uses modern GitHub Actions patterns including `actions/checkout@v4`, `dtolnay/rust-toolchain@stable`, and caching with `Swatinem/rust-cache@v2`.
    *   **Recommendation:** You SHOULD create a separate `release.yml` workflow file rather than modifying the CI workflow. This keeps concerns separated - CI for continuous validation, release for artifact distribution.

*   **File:** `README.md`
    *   **Summary:** Comprehensive project documentation covering vision, features, architecture, status, installation, usage, development, tag database generation, testing, benchmarking, fuzzing, and licensing.
    *   **Current Status:** Shows the project is in "Iteration 5" based on the completed features (FFI bindings, tag database automation) described in the README.
    *   **Recommendation:** After completing the cross-compilation setup, you SHOULD update the README to document the release process and available binary downloads.

### Implementation Tips & Notes

*   **Tip 1: Cross.toml Configuration:**
    *   The `cross` tool uses `Cross.toml` to configure build environment overrides for cross-compilation targets.
    *   You MUST specify each target platform in the configuration.
    *   For targets that require special setup (like macOS cross-compilation from Linux), the cross project provides pre-built Docker images with the necessary toolchains.
    *   The configuration file should be minimal - cross handles most complexity automatically.

*   **Tip 2: GitHub Actions Release Workflow:**
    *   You MUST trigger the workflow only on git tag pushes matching `v*` pattern (e.g., `v1.0.0`).
    *   Use a build matrix to parallelize compilation across all 5 target platforms for efficiency.
    *   The workflow SHOULD use `cross-rs/cross` action or install cross manually via `cargo install cross`.
    *   Binary names will differ by platform: no extension (Linux/macOS), `.exe` (Windows).
    *   You SHOULD use conditional logic to handle platform-specific file extensions when uploading artifacts.

*   **Tip 3: UPX Compression:**
    *   UPX (Ultimate Packer for eXecutables) can reduce binary sizes by 50-70%, but it's OPTIONAL per the acceptance criteria.
    *   UPX may trigger false positives in antivirus software on Windows.
    *   If you implement UPX compression, make it conditional or provide both compressed and uncompressed binaries.
    *   UPX is not available on all platforms by default in CI environments - you may need to install it as a separate step.

*   **Tip 4: GitHub Releases Upload:**
    *   You SHOULD use the `softprops/action-gh-release@v1` action to create GitHub Releases and upload artifacts.
    *   Provide checksums (SHA256) for all binaries to allow users to verify downloads.
    *   Include release notes in the workflow, potentially auto-generated from commit messages or CHANGELOG.md.

*   **Tip 5: Testing the Workflow:**
    *   The acceptance criteria requires testing with a manual tag push.
    *   You can test locally by creating a test tag: `git tag v0.1.0-test && git push origin v0.1.0-test`
    *   IMPORTANT: Test on a branch first or be prepared to delete test releases and tags.
    *   You SHOULD add a condition to distinguish pre-release tags (alpha, beta, rc) from stable releases.

*   **Note: Build Times:**
    *   Cross-compilation for 5 targets in GitHub Actions will take 15-30 minutes per build.
    *   Each target builds independently in the matrix, so total wall-clock time is limited by the slowest build (usually macOS or Windows).
    *   You SHOULD add caching for Cargo registry and build artifacts to speed up subsequent builds.

*   **Note: Static Linking:**
    *   The `x86_64-unknown-linux-musl` and `aarch64-unknown-linux-musl` targets produce fully static binaries with no libc dependencies.
    *   The Windows `x86_64-pc-windows-gnu` target produces binaries that depend only on system DLLs (kernel32, msvcrt).
    *   The macOS targets (`x86_64-apple-darwin`, `aarch64-apple-darwin`) link against system libraries but are generally portable across macOS versions.
    *   You DO NOT need special configuration for static linking on musl targets - it's automatic.

*   **Warning: macOS Cross-Compilation Constraints:**
    *   Cross-compiling for macOS from Linux requires the osxcross toolchain, which `cross` provides via Docker.
    *   Apple's licensing restricts where macOS binaries can be built, but CI/CD usage is generally acceptable.
    *   If macOS cross-compilation fails in CI, you may need to use `macos-latest` runners for those specific targets instead of `cross`.
    *   Consider splitting the workflow: Linux/ARM builds use `cross` on ubuntu-latest, macOS builds use native runners.

*   **Warning: Binary Size Targets:**
    *   The acceptance criteria specifies approximate sizes: Linux ~8MB, Windows ~9MB, macOS ~10MB.
    *   These are APPROXIMATE targets. Actual sizes will vary based on dependencies and Rust version.
    *   With the current dependencies (clap, nom, serde, etc.) and existing optimizations, expect 5-10MB stripped binaries.
    *   The sizes will meet the acceptance criteria - don't over-optimize for exact sizes.

### Project Structure Notes

*   The project follows a hexagonal architecture with clear separation between `src/core/`, `src/parsers/`, `src/writers/`, `src/ffi/`, and `src/cli/`.
*   The `cbindgen.toml` file already exists, indicating FFI bindings are in place (confirmed by tasks I5.T1-I5.T5 being marked as done).
*   The `build.rs` file exists and handles tag database generation from ExifTool source.
*   No existing `Cross.toml` file was found, so you're creating it from scratch.
*   No existing `release.yml` workflow file was found in `.github/workflows/`, so you're creating it from scratch.

### Success Criteria Summary

To complete this task successfully, you MUST deliver:

1. **Cross.toml** file with configuration for all 5 target platforms
2. **.github/workflows/release.yml** workflow that:
   - Triggers on `v*` tags
   - Builds for all 5 targets using `cross`
   - Strips symbols (handled by Cargo.toml profile)
   - Optionally compresses with UPX
   - Uploads binaries to GitHub Releases with checksums
3. **No changes needed to Cargo.toml** - release profile already optimal
4. **Verification:** Test with a manual tag push to confirm artifacts are uploaded correctly

Good luck! This is a DevOps/infrastructure task focused on release automation and binary distribution.
