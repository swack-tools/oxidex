# Task Briefing Package

This package contains all necessary information and strategic guidance for the Coder Agent.

---

## 1. Current Task Details

This is the full specification of the task you must complete.

```json
{
  "task_id": "I5.T8",
  "iteration_id": "I5",
  "iteration_goal": "Implement C FFI bindings for cross-language integration, automate tag database generation from ExifTool specs, set up cross-compilation and release builds, create comprehensive documentation, and polish for v1.0 release.",
  "description": "Create distribution packages: (1) Debian .deb package (use cargo-deb), (2) RPM package (use cargo-generate-rpm), (3) Homebrew formula (Ruby DSL defining package). Configure package metadata (name, version, description, dependencies, installation paths). Test packages: install, run, uninstall. Add instructions to README for each distribution method. Optionally set up package repository or publish to existing repos (crates.io for Rust crate, homebrew-core for Homebrew).",
  "agent_type_hint": "SetupAgent",
  "inputs": "Packaging tool documentation (cargo-deb, cargo-generate-rpm), Homebrew formula guide",
  "target_files": [
    "Cargo.toml",
    "packaging/homebrew/exiftool-rs.rb",
    "README.md"
  ],
  "input_files": [
    "Cargo.toml",
    "README.md"
  ],
  "deliverables": ".deb package, .rpm package, Homebrew formula, installation documentation",
  "acceptance_criteria": "cargo deb generates valid .deb package, cargo generate-rpm generates valid .rpm package, Homebrew formula installs from source or binary, packages install binary to /usr/bin or /usr/local/bin, packages include man page (optional) and README, manual test: install package, run exiftool-rs --version, uninstall, README documents installation for each package type",
  "dependencies": ["I5.T6"],
  "parallelizable": false,
  "done": false
}
```

---

## 2. Architectural & Planning Context

The following are the relevant sections from the architecture and plan documents, which I found by analyzing the task description.

### Context: deployment-view (from 05_Operational_Architecture.md)

```markdown
<!-- anchor: deployment-view -->
### 3.9. Deployment View

<!-- anchor: target-environment -->
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

<!-- anchor: deployment-strategy -->
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

### Context: task-i5-t8 (from 02_Iteration_I5.md)

```markdown
<!-- anchor: task-i5-t8 -->
*   **Task 5.8: Create Packaging for Distribution (deb, rpm, Homebrew)**
    *   **Task ID:** `I5.T8`
    *   **Description:** Create distribution packages: (1) Debian .deb package (use `cargo-deb`), (2) RPM package (use `cargo-generate-rpm`), (3) Homebrew formula (Ruby DSL defining package). Configure package metadata (name, version, description, dependencies, installation paths). Test packages: install, run, uninstall. Add instructions to README for each distribution method. Optionally set up package repository or publish to existing repos (crates.io for Rust crate, homebrew-core for Homebrew).
    *   **Agent Type Hint:** `SetupAgent`
    *   **Inputs:** Packaging tool documentation (cargo-deb, cargo-generate-rpm), Homebrew formula guide
    *   **Input Files:** [`Cargo.toml`, `README.md`]
    *   **Target Files:**
        *   `Cargo.toml` (add `[package.metadata.deb]` and `[package.metadata.generate-rpm]` sections)
        *   `packaging/homebrew/exiftool-rs.rb` (Homebrew formula)
        *   `README.md` (add package installation instructions)
    *   **Deliverables:**
        *   .deb package
        *   .rpm package
        *   Homebrew formula
        *   Installation documentation
    *   **Acceptance Criteria:**
        *   `cargo deb` generates valid .deb package
        *   `cargo generate-rpm` generates valid .rpm package
        *   Homebrew formula installs from source or binary
        *   Packages install binary to /usr/bin or /usr/local/bin
        *   Packages include man page (optional) and README
        *   Manual test: install package, run `exiftool-rs --version`, uninstall
        *   README documents installation for each package type
    *   **Dependencies:** `I5.T6` (needs release binaries)
    *   **Parallelizable:** Partially (can define package metadata early, test after binaries available)
```

### Context: task-i5-t6 (from 02_Iteration_I5.md)

```markdown
<!-- anchor: task-i5-t6 -->
*   **Task 5.6: Set Up Cross-Compilation with cross**
    *   **Task ID:** `I5.T6`
    *   **Description:** Configure cross-compilation for Linux, macOS, Windows, and ARM targets using `cross` tool. Create `Cross.toml` configuration. Set up GitHub Actions release workflow in `.github/workflows/release.yml` to build binaries for: (1) x86_64-unknown-linux-musl (static Linux), (2) x86_64-apple-darwin (macOS Intel), (3) aarch64-apple-darwin (macOS ARM), (4) x86_64-pc-windows-gnu (Windows), (5) aarch64-unknown-linux-musl (Linux ARM). Apply optimizations: LTO, strip symbols, UPX compression (optional). Upload artifacts to GitHub Releases on git tag push.
    *   **Agent Type Hint:** `SetupAgent`
    *   **Inputs:** `cross` tool documentation, GitHub Actions best practices
    *   **Input Files:** [`Cargo.toml`]
    *   **Target Files:**
        *   `Cross.toml`
        *   `.github/workflows/release.yml`
        *   `Cargo.toml` (add `[profile.release]` optimizations)
    *   **Deliverables:**
        *   Cross-compilation configuration
        *   GitHub Actions release workflow
        *   Optimized release binaries
    *   **Acceptance Criteria:**
        *   Cross.toml exists with target configurations
        *   Release workflow builds for all 5 targets
        *   Binaries are statically linked (no external dependencies)
        *   Release profile has: lto = true, codegen-units = 1, opt-level = z (size) or 3 (speed)
        *   Binaries stripped of debug symbols
        *   Workflow uploads binaries to GitHub Releases on tag push (test with manual tag)
        *   Binary sizes: Linux ~8MB, Windows ~9MB, macOS ~10MB (approximate)
    *   **Dependencies:** []
    *   **Parallelizable:** Yes
```

---

## 3. Codebase Analysis & Strategic Guidance

The following analysis is based on my direct review of the current codebase. Use these notes and tips to guide your implementation.

### Relevant Existing Code

*   **File:** `Cargo.toml`
    *   **Summary:** This is the project's package manifest. It already contains comprehensive metadata including name, version (0.1.0), description, license (GPL-3.0), repository URL, keywords, and categories. The package is configured to build both a library and binary with multiple crate types (lib, staticlib, cdylib). Release profile is already optimized with LTO, strip, and codegen-units=1.
    *   **Recommendation:** You MUST add two new metadata sections to this file: `[package.metadata.deb]` for Debian packaging configuration and `[package.metadata.generate-rpm]` for RPM packaging configuration. DO NOT modify the existing `[package]` section or `[profile.release]` settings as they are already correctly configured per I5.T6.
    *   **Key Values to Use:** name="exiftool-rs", version="0.1.0", authors=["ExifTool-RS Contributors"], description="A modern, high-performance Rust reimplementation of ExifTool for reading, writing, and editing metadata in 300+ file formats", license="GPL-3.0"

*   **File:** `Cross.toml`
    *   **Summary:** This file exists and is fully configured with cross-compilation targets for all five platforms: x86_64-unknown-linux-musl, aarch64-unknown-linux-musl, x86_64-apple-darwin, aarch64-apple-darwin, x86_64-pc-windows-gnu. It uses the official cross-rs Docker images.
    *   **Recommendation:** This file is complete from I5.T6. You SHOULD NOT modify it unless you discover packaging-specific build requirements. The cross tool will use this configuration to build the binaries that your packages will install.

*   **File:** `.github/workflows/release.yml`
    *   **Summary:** This is a comprehensive GitHub Actions workflow that handles release automation. It creates GitHub releases, builds binaries for all 5 targets using cross/cargo, strips symbols, creates archives (.tar.gz for Linux/macOS, .zip for Windows), generates SHA256 checksums, and uploads artifacts to GitHub Releases. The workflow triggers on git tags matching 'v*' pattern.
    *   **Recommendation:** This workflow is complete from I5.T6. The binaries it produces are what your Debian and RPM packages will ultimately install. You MAY want to reference the archive naming convention (e.g., exiftool-rs-x86_64-linux-musl.tar.gz) when writing your Homebrew formula, as it can download directly from GitHub Releases.
    *   **Note:** The workflow already handles binary stripping and optimization, so your package configurations don't need to re-strip binaries.

*   **File:** `README.md`
    *   **Summary:** The README currently has basic project information, a "Current Status" section showing work in progress, and placeholders for usage documentation. It has an "Installation" section with only "From Source" instructions using cargo build.
    *   **Recommendation:** You MUST add a new section documenting all three package installation methods. Insert this BEFORE the "Usage" section. The structure should include: (1) "From Debian Package" with apt/dpkg commands, (2) "From RPM Package" with dnf/yum/rpm commands, (3) "From Homebrew" with brew install command, (4) keep existing "From Source" section. Use clear markdown formatting with code blocks for commands.

### Implementation Tips & Notes

*   **Tip:** The `packaging/` directory does not exist yet. You MUST create it with this structure: `packaging/homebrew/` for the Homebrew formula. The .deb and .rpm packages are generated artifacts (not source files), so they don't need dedicated directories - cargo-deb and cargo-generate-rpm will output them to the target/ directory.

*   **Tip:** For cargo-deb configuration, you MUST specify at a minimum: `assets` (which files to include in the package - at minimum the binary), `maintainer-scripts` (optional pre/post install scripts), `extended-description`, and `section` (typically "utils" for command-line tools). The binary should be installed to `/usr/bin/exiftool-rs`. Consider including the README and LICENSE files as documentation.

*   **Tip:** For cargo-generate-rpm configuration, you MUST specify: `assets` (similar to deb), `license` (GPL-3.0), and optionally `post_install_script` and `pre_uninstall_script`. The binary should install to `/usr/bin/exiftool-rs`.

*   **Tip:** For the Homebrew formula, you have TWO options:
    1. **Source-based installation:** The formula builds from source using `cargo install`. This is simpler but slower for users.
    2. **Binary bottles:** The formula downloads pre-built binaries from GitHub Releases. This is faster but requires maintaining bottles for each macOS version/architecture.

    For this task, I RECOMMEND starting with a source-based formula (option 1) as it's simpler and the release.yml workflow already provides binaries. You can add bottles later as an enhancement.

*   **Tip:** The Homebrew formula should follow this structure:
    ```ruby
    class ExiftoolRs < Formula
      desc "Modern, high-performance Rust reimplementation of ExifTool"
      homepage "https://github.com/exiftool-rs/exiftool-rs"
      url "https://github.com/exiftool-rs/exiftool-rs/archive/refs/tags/v0.1.0.tar.gz"
      sha256 "..." # Generate this
      license "GPL-3.0"

      depends_on "rust" => :build

      def install
        system "cargo", "install", *std_cargo_args
      end

      test do
        system "#{bin}/exiftool-rs", "--version"
      end
    end
    ```

*   **Warning:** Neither cargo-deb nor cargo-generate-rpm are installed in the project's development environment yet. Your implementation MUST include installation instructions in comments or README about installing these tools: `cargo install cargo-deb` and `cargo install cargo-generate-rpm`.

*   **Warning:** When testing packages, BE CAREFUL not to conflict with existing exiftool installations. The binary is named `exiftool-rs` (with hyphen) specifically to avoid conflicts with Perl ExifTool's `exiftool` binary. Make sure all package configurations preserve this naming.

*   **Note:** The project version is currently 0.1.0 (pre-release). While the task mentions v1.0 release preparation, for THIS specific task (I5.T8), you should use the current version 0.1.0 in all package configurations. Version bumping will happen in I5.T11.

*   **Note:** The project uses GPL-3.0 license. This MUST be reflected in all package metadata. Some packaging tools require specific license identifiers - use "GPL-3.0" or "GPL-3.0-only" depending on the tool's requirements.

*   **Recommendation:** For package testing, create a simple shell script in `scripts/test-packages.sh` that automates the install/run/uninstall cycle for each package type. This will help verify the acceptance criteria and provide a repeatable test process.

### Package-Specific Technical Requirements

**Debian Package (.deb):**
- Tool: cargo-deb (https://crates.io/crates/cargo-deb)
- Install: `cargo install cargo-deb`
- Build: `cargo deb`
- Output: `target/debian/exiftool-rs_0.1.0_amd64.deb` (or similar)
- Must specify architecture (likely amd64 or arm64 depending on build target)
- Should include copyright file (/usr/share/doc/exiftool-rs/copyright)

**RPM Package (.rpm):**
- Tool: cargo-generate-rpm (https://crates.io/crates/cargo-generate-rpm)
- Install: `cargo install cargo-generate-rpm`
- Build: `cargo build --release && cargo generate-rpm`
- Output: `target/generate-rpm/exiftool-rs-0.1.0-1.x86_64.rpm` (or similar)
- Requires release binary to already be built
- Should specify release number (typically "1" for first build of a version)

**Homebrew Formula:**
- Language: Ruby DSL
- Location: `packaging/homebrew/exiftool-rs.rb`
- Reference: https://docs.brew.sh/Formula-Cookbook
- Must include: desc, homepage, url, sha256, license, install method, test block
- Test block should minimally verify `--version` works

### Expected Workflow

1. Install packaging tools (cargo-deb, cargo-generate-rpm)
2. Add `[package.metadata.deb]` section to Cargo.toml
3. Add `[package.metadata.generate-rpm]` section to Cargo.toml
4. Create `packaging/homebrew/exiftool-rs.rb` with Homebrew formula
5. Build packages: `cargo deb` and `cargo build --release && cargo generate-rpm`
6. Test each package type (install, run, uninstall)
7. Update README.md with installation instructions for all three package types
8. Document the packaging process (in comments or a PACKAGING.md file)
