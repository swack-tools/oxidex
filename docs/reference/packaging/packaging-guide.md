# Packaging Guide for OxiDex

This document provides comprehensive instructions for creating and distributing packages for OxiDex across multiple platforms.

## Table of Contents

1. [Prerequisites](#prerequisites)
2. [Package Types](#package-types)
3. [Building Packages](#building-packages)
4. [Testing Packages](#testing-packages)
5. [Publishing Releases](#publishing-releases)
6. [Troubleshooting](#troubleshooting)

## Prerequisites

### Build Tools

Install the required Rust packaging tools:

```bash
# For Debian packages
cargo install cargo-deb

# For RPM packages
cargo install cargo-generate-rpm
```

### System Dependencies

**For Debian packaging (Ubuntu/Debian):**
```bash
sudo apt-get install dpkg-dev
```

**For RPM packaging (Fedora/RHEL):**
```bash
sudo dnf install rpm-build
# or on RHEL/CentOS
sudo yum install rpm-build
```

**For Homebrew (macOS):**
```bash
# Install Homebrew if not already installed
/bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)"
```

## Package Types

OxiDex supports three primary package distribution formats:

| Package Type | Platform | Tool | Output |
|--------------|----------|------|--------|
| **Debian (.deb)** | Ubuntu, Debian, Linux Mint | `cargo-deb` | `target/debian/oxidex_VERSION_amd64.deb` |
| **RPM (.rpm)** | Fedora, RHEL, CentOS, openSUSE | `cargo-generate-rpm` | `target/generate-rpm/oxidex-VERSION-1.x86_64.rpm` |
| **Homebrew (.rb)** | macOS | Homebrew formula | Source or binary installation |

## Building Packages

### 1. Debian Package (.deb)

The Debian package configuration is defined in `Cargo.toml` under `[package.metadata.deb]`.

#### Configuration

Key settings:
- **Binary location**: `/usr/bin/oxidex`
- **Documentation**: `/usr/share/doc/oxidex/`
- **Section**: `utils`
- **Priority**: `optional`

#### Build Process

```bash
# Step 1: Build the release binary (optional - cargo-deb does this)
cargo build --release

# Step 2: Generate the Debian package
cargo deb

# Output location
ls -lh target/debian/oxidex_*.deb
```

#### Architecture-Specific Builds

For cross-compilation:

```bash
# ARM64/aarch64
cargo deb --target aarch64-unknown-linux-gnu

# x86_64 (default)
cargo deb --target x86_64-unknown-linux-gnu
```

#### Package Contents

The `.deb` package includes:
- Statically linked binary: `/usr/bin/oxidex`
- Documentation: `/usr/share/doc/oxidex/README.md`
- License: `/usr/share/doc/oxidex/LICENSE`

#### Manual Installation

```bash
# Install
sudo dpkg -i target/debian/oxidex_0.1.0_amd64.deb

# Verify
oxidex --version

# Uninstall
sudo dpkg -r oxidex
```

### 2. RPM Package (.rpm)

The RPM package configuration is defined in `Cargo.toml` under `[package.metadata.generate-rpm]`.

#### Configuration

Key settings:
- **Binary location**: `/usr/bin/oxidex`
- **Documentation**: `/usr/share/doc/oxidex/`
- **License**: GPL-3.0 (inherited from `[package]` section)
- **Release number**: 1 (first build of this version)

#### Build Process

```bash
# Step 1: Build the release binary (REQUIRED - must be done first)
cargo build --release

# Step 2: Generate the RPM package
cargo generate-rpm

# Output location
ls -lh target/generate-rpm/oxidex-*.rpm
```

**Important**: Unlike `cargo-deb`, `cargo-generate-rpm` does NOT build the binary automatically. You MUST run `cargo build --release` first.

#### Package Contents

The `.rpm` package includes:
- Statically linked binary: `/usr/bin/oxidex`
- Documentation: `/usr/share/doc/oxidex/README.md`
- License: `/usr/share/doc/oxidex/LICENSE`

#### Manual Installation

```bash
# Install (Fedora/RHEL 8+)
sudo dnf install target/generate-rpm/oxidex-0.1.0-1.x86_64.rpm

# Install (older RHEL/CentOS)
sudo yum install target/generate-rpm/oxidex-0.1.0-1.x86_64.rpm

# Install (using rpm directly)
sudo rpm -i target/generate-rpm/oxidex-0.1.0-1.x86_64.rpm

# Verify
oxidex --version

# Uninstall
sudo rpm -e oxidex
```

### 3. Homebrew Formula (macOS)

The Homebrew formula is located at `packaging/homebrew/oxidex.rb`.

#### Configuration

The formula is a Ruby DSL file containing:
- **URL**: Points to GitHub release tarball
- **SHA256**: Checksum of the tarball (MUST be updated after each release)
- **Dependencies**: `rust` (build-time only)
- **Install method**: Builds from source using `cargo install`

#### Updating the SHA256

After creating a GitHub release:

```bash
# Calculate SHA256 of the release tarball
curl -sL https://github.com/oxidex/oxidex/archive/refs/tags/v0.1.0.tar.gz | shasum -a 256

# Update the sha256 field in packaging/homebrew/oxidex.rb
```

#### Testing the Formula

```bash
# Test local installation
brew install --build-from-source ./packaging/homebrew/oxidex.rb

# Verify
oxidex --version

# Test formula syntax
brew audit --strict ./packaging/homebrew/oxidex.rb

# Uninstall
brew uninstall oxidex
```

#### Publishing to Homebrew

To make the formula available via official Homebrew:

1. Fork [homebrew/homebrew-core](https://github.com/Homebrew/homebrew-core)
2. Add `Formula/oxidex.rb` to your fork
3. Submit a Pull Request following [Homebrew's guidelines](https://docs.brew.sh/How-To-Open-a-Homebrew-Pull-Request)

Requirements for homebrew-core:
- Project must be stable and actively maintained
- Must have at least 75 stars or 30 forks on GitHub
- Must be version 1.0.0 or later (semantic versioning)
- Formula must pass `brew audit --strict`

#### Binary Bottles (Future Enhancement)

For faster installation, Homebrew supports pre-compiled "bottles":

```ruby
bottle do
  sha256 cellar: :any_skip_relocation, arm64_sonoma: "abc123..."
  sha256 cellar: :any_skip_relocation, arm64_ventura: "def456..."
  sha256 cellar: :any_skip_relocation, sonoma: "ghi789..."
  sha256 cellar: :any_skip_relocation, ventura: "jkl012..."
end
```

Bottles are generated automatically by Homebrew's CI after the formula is accepted into homebrew-core.

## Testing Packages

### Automated Testing

Use the provided test script to validate all package types:

```bash
# Test all packages
./scripts/test-packages.sh all

# Test specific package type
./scripts/test-packages.sh deb
./scripts/test-packages.sh rpm
./scripts/test-packages.sh brew
```

The script performs:
1. Package existence verification
2. Package inspection (contents, metadata)
3. Installation test (requires sudo)
4. Binary execution test (`--version`)
5. Uninstallation test
6. Cleanup verification

### Manual Testing Checklist

For each package type, verify:

- [ ] Package file exists and is non-zero size
- [ ] Package installs without errors
- [ ] Binary is executable and in PATH
- [ ] `oxidex --version` outputs correct version
- [ ] `oxidex --help` displays help text
- [ ] Package uninstalls cleanly
- [ ] No files left behind after uninstall

### Platform-Specific Testing

**Debian/Ubuntu:**
```bash
# Inspect package
dpkg-deb --info target/debian/oxidex_0.1.0_amd64.deb
dpkg-deb --contents target/debian/oxidex_0.1.0_amd64.deb

# Test installation
sudo dpkg -i target/debian/oxidex_0.1.0_amd64.deb
which oxidex
oxidex --version
```

**Fedora/RHEL:**
```bash
# Inspect package
rpm -qip target/generate-rpm/oxidex-0.1.0-1.x86_64.rpm
rpm -qlp target/generate-rpm/oxidex-0.1.0-1.x86_64.rpm

# Test installation
sudo dnf install target/generate-rpm/oxidex-0.1.0-1.x86_64.rpm
which oxidex
oxidex --version
```

**macOS:**
```bash
# Audit formula
brew audit --strict ./packaging/homebrew/oxidex.rb

# Test installation (builds from source)
brew install --build-from-source --verbose ./packaging/homebrew/oxidex.rb
which oxidex
oxidex --version
```

## Publishing Releases

### 1. Pre-Release Checklist

Before creating packages for distribution:

- [ ] Update version in `Cargo.toml` (if needed)
- [ ] Run `cargo test` - all tests pass
- [ ] Run `cargo clippy` - no warnings
- [ ] Run `cargo build --release` - successful build
- [ ] Update CHANGELOG.md with release notes
- [ ] Commit all changes

### 2. Create Git Tag

```bash
# Create annotated tag
git tag -a v0.1.0 -m "Release version 0.1.0"

# Push tag to GitHub
git push origin v0.1.0
```

### 3. GitHub Actions Workflow

The `.github/workflows/release.yml` workflow automatically:
- Builds binaries for all platforms (Linux, macOS, Windows, ARM)
- Creates GitHub Release
- Uploads binary archives with checksums

Wait for the workflow to complete before generating packages.

### 4. Generate Packages

After the release workflow completes:

```bash
# Build Debian package
cargo deb

# Build RPM package
cargo build --release
cargo generate-rpm

# Update Homebrew formula SHA256
curl -sL https://github.com/oxidex/oxidex/archive/refs/tags/v0.1.0.tar.gz | shasum -a 256
# Update packaging/homebrew/oxidex.rb with the hash
```

### 5. Upload Packages to GitHub Release

```bash
# Using GitHub CLI (gh)
gh release upload v0.1.0 \
  target/debian/oxidex_0.1.0_amd64.deb \
  target/generate-rpm/oxidex-0.1.0-1.x86_64.rpm

# Or upload manually via GitHub web interface
```

### 6. Verify Release Assets

Check that the GitHub Release includes:
- Source code archives (`.tar.gz`, `.zip`)
- Platform-specific binaries (from release workflow)
- Debian package (`.deb`)
- RPM package (`.rpm`)
- SHA256 checksums

### 7. Announce Release

- Update README.md installation instructions (if needed)
- Post release notes on GitHub Discussions
- Announce on relevant forums/communities

## Troubleshooting

### Common Issues

#### cargo-deb: Binary not found

**Error**: `Error: failed to execute process cargo build --release`

**Solution**: Ensure you have a valid `[[bin]]` section in `Cargo.toml`:
```toml
[[bin]]
name = "oxidex"
path = "src/main.rs"
```

#### cargo-generate-rpm: No such file or directory

**Error**: `Error: No such file or directory (os error 2)`

**Solution**: Build the release binary first:
```bash
cargo build --release
cargo generate-rpm
```

#### Homebrew: SHA256 mismatch

**Error**: `Error: SHA256 mismatch`

**Solution**: Recalculate and update the SHA256 hash:
```bash
curl -sL https://github.com/oxidex/oxidex/archive/refs/tags/v0.1.0.tar.gz | shasum -a 256
# Update sha256 in packaging/homebrew/oxidex.rb
```

#### Package won't install: Permission denied

**Solution**: Use `sudo` for system-wide installation:
```bash
# Debian/Ubuntu
sudo dpkg -i package.deb

# Fedora/RHEL
sudo dnf install package.rpm
```

#### Binary not in PATH after installation

**Solution**: Packages install to `/usr/bin/`, which should be in PATH. Verify:
```bash
echo $PATH
which oxidex

# If needed, manually add to PATH
export PATH="/usr/bin:$PATH"
```

### Debugging Tips

**View package contents:**
```bash
# Debian
dpkg-deb --contents package.deb

# RPM
rpm -qlp package.rpm
```

**Check package dependencies:**
```bash
# Debian
dpkg-deb --info package.deb | grep Depends

# RPM
rpm -qRp package.rpm
```

**Verify binary is statically linked (no external dependencies):**
```bash
ldd target/release/oxidex
# Should show "not a dynamic executable" or minimal system libs
```

## Future Enhancements

Potential improvements for the packaging system:

1. **Automated Package Building in CI**
   - Add package generation steps to `.github/workflows/release.yml`
   - Automatically upload packages to GitHub Releases

2. **Homebrew Tap Repository**
   - Create `homebrew-oxidex` tap for easier installation
   - Maintain binary bottles for faster installation

3. **Additional Package Formats**
   - AppImage (universal Linux package)
   - Snap package (Ubuntu Software Center)
   - Flatpak (Flathub distribution)
   - Chocolatey (Windows package manager)
   - Scoop (Windows package manager)

4. **Package Signing**
   - GPG sign Debian packages
   - Sign RPM packages with GPG
   - Code sign macOS binaries

5. **Distribution Repositories**
   - Host custom APT repository for Debian/Ubuntu
   - Host custom YUM/DNF repository for RHEL/Fedora

## References

- **cargo-deb**: https://github.com/kornelski/cargo-deb
- **cargo-generate-rpm**: https://github.com/cat-in-136/cargo-generate-rpm
- **Homebrew Formula Cookbook**: https://docs.brew.sh/Formula-Cookbook
- **Debian Policy Manual**: https://www.debian.org/doc/debian-policy/
- **RPM Packaging Guide**: https://rpm-packaging-guide.github.io/
- **OxiDex Project**: https://github.com/oxidex/oxidex

---

**Maintained by**: OxiDex Contributors
**Last Updated**: 2025-10-30
**Version**: 0.1.0
