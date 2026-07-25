# Getting Started

This guide will help you install OxiDex and run your first commands.

## Installation

OxiDex provides multiple installation methods. Choose the one that works best for your workflow.

### Option 1: Cargo (Recommended for Rust Users)

Install directly from crates.io:

```bash
cargo install oxidex
```

Verify installation:

```bash
oxidex --version
```

### Option 2: Homebrew (macOS)

For macOS users with [Homebrew](https://brew.sh):

```bash
# Install from Homebrew formula (source build)
brew install --build-from-source https://raw.githubusercontent.com/swack-tools/oxidex/main/packaging/homebrew/oxidex.rb

# Verify installation
oxidex --version
```

**Note:** The Homebrew formula builds from source, which may take 5-10 minutes.

### Option 3: Pre-Built Binaries

Download static binaries from the [GitHub Releases](https://github.com/swack-tools/oxidex/releases) page:

**Linux (x86_64):**
```bash
wget https://github.com/swack-tools/oxidex/releases/download/v1.1.0/oxidex-x86_64-linux-musl.tar.gz
tar xzf oxidex-x86_64-linux-musl.tar.gz
sudo mv oxidex /usr/local/bin/
oxidex --version
```

**Linux (ARM64):**
```bash
wget https://github.com/swack-tools/oxidex/releases/download/v1.1.0/oxidex-aarch64-linux-musl.tar.gz
tar xzf oxidex-aarch64-linux-musl.tar.gz
sudo mv oxidex /usr/local/bin/
oxidex --version
```

**macOS (Intel):**
```bash
wget https://github.com/swack-tools/oxidex/releases/download/v1.1.0/oxidex-x86_64-macos.tar.gz
tar xzf oxidex-x86_64-macos.tar.gz
sudo mv oxidex /usr/local/bin/
oxidex --version
```

**macOS (Apple Silicon):**
```bash
wget https://github.com/swack-tools/oxidex/releases/download/v1.1.0/oxidex-aarch64-macos.tar.gz
tar xzf oxidex-aarch64-macos.tar.gz
sudo mv oxidex /usr/local/bin/
oxidex --version
```

**Windows (x86_64):**
Download `oxidex-x86_64-windows.zip` from releases, extract, and add to PATH.

### Option 4: Build from Source

For development or custom builds:

```bash
# Clone the repository
git clone https://github.com/swack-tools/oxidex.git
cd oxidex

# Build release binary
cargo build --release

# Run
./target/release/oxidex --version

# Optional: Install to system
cargo install --path .
```

## First Steps

### Extract Metadata from a File

```bash
oxidex photo.jpg
```

Output:
```
 FileName: photo.jpg
FileSize: 2.3 MB
Make: Canon
Model: Canon EOS 5D Mark IV
DateTimeOriginal: 2024:11:15 14:23:05
ISO: 400
FNumber: 5.6
ExposureTime: 1/250
...
```

### Extract Specific Tags

```bash
oxidex -Make -Model -DateTimeOriginal photo.jpg
```

Output:
```
Make: Canon
Model: Canon EOS 5D Mark IV
DateTimeOriginal: 2024:11:15 14:23:05
```

### Write Metadata

```bash
oxidex -Artist="Jane Doe" -Copyright="Copyright 2024" photo.jpg
```

### Process Multiple Files

```bash
# Recursive directory scan
oxidex -r /path/to/photos/

# Specific file pattern
oxidex *.jpg
```

### Output Formats

**JSON:**
```bash
oxidex -json photo.jpg
```

**CSV (for batch analysis):**
```bash
oxidex -csv -r /path/to/photos/ > metadata.csv
```

## Verification

Test your installation with a sample command:

```bash
# Create a test file (if you don't have one)
echo "test" > test.txt

# Extract metadata
oxidex test.txt
```

Expected output should include file information like FileName, FileSize, etc.

## Next Steps

- [CLI Usage Guide](/guide/cli-usage) - Learn all command-line options
- [Library API Guide](/guide/library-api) - Use OxiDex in Rust projects
- [Troubleshooting](/guide/troubleshooting) - Common issues and solutions

## System Requirements

- **OS:** Linux (Ubuntu 18.04+), macOS (10.15+), Windows (10+)
- **Architecture:** x86_64 or ARM64
- **For source builds:** Rust 1.75+

## Getting Help

- [GitHub Issues](https://github.com/swack-tools/oxidex/issues) - Report bugs or request features
- [Troubleshooting Guide](/guide/troubleshooting) - Common problems
- [GitHub Discussions](https://github.com/swack-tools/oxidex/discussions) - Ask questions
