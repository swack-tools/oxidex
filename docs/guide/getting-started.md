# Installation

::: warning Beta: v2.0.0-beta.1
These docs describe the pre-tag 2.0 development line, not a published beta.
The signed v2.0.0-beta.1 tag and binary assets are pending; until they are
published on the [releases page](https://github.com/swack-tools/oxidex/releases),
build this development line from source as shown below. The previous stable release is
[v1.2.1](https://github.com/swack-tools/oxidex/releases/tag/v1.2.1). It
behaves differently in several ways; see
[Migrating from 1.x to 2.0](/guide/migrating-from-1x).
:::

## Build from source

```bash
git clone https://github.com/swack-tools/oxidex.git
cd oxidex
git switch refactor/tag-machinery   # pre-tag 2.0 development line; signed beta.1 tag pending
cargo build --release
./target/release/oxidex --version

# optional: put it on your PATH
cargo install --path .
```

`rust-toolchain.toml` pins the Rust toolchain (1.97.1), and rustup installs
it on first build. The crate uses edition 2024. A release build compiles the
six `oxidex-tags-*` crates and the generated tables, so the first build takes
a while.

Optional Cargo features:

| Feature | Adds |
| --- | --- |
| `magika` | the Magika file-type detector (`--detector magika`) |
| `exiftool-comparison` | the ExifTool comparison tests (development only) |

::: danger Do not run `cargo install oxidex`
The `oxidex` name on crates.io belongs to an unrelated, reserved stub crate
(version 0.0.1). OxiDex is not published on crates.io. Install from source
or from a GitHub release.
:::

## Prebuilt binaries

Published GitHub releases carry prebuilt binaries. v1.2.1 has these; do not
assume a v2.0.0-beta.1 asset exists until its signed tag and release receipt
are recorded:

| Platform | Asset |
| --- | --- |
| Linux x86_64 (static, musl) | `oxidex-x86_64-unknown-linux-musl` |
| Linux ARM64 (static, musl) | `oxidex-aarch64-unknown-linux-musl` |
| macOS Apple Silicon | `oxidex-aarch64-apple-darwin`, or the `.dmg` |
| Windows x86_64 | `oxidex-x86_64-pc-windows-gnu.exe` |

Download the asset for your platform, make it executable (`chmod +x`) and
put it on your `PATH`. Release tags also publish a container image,
`swackhamer/oxidex`, on Docker Hub.

## First commands

```bash
oxidex photo.jpg                          # every tag
oxidex -Make -Model -DateTimeOriginal photo.jpg
oxidex -j -a -G1 photo.jpg                # JSON, comparable with `exiftool -j -a -G1`
oxidex -EXIF:Artist="Jane Doe" photo.jpg  # write (in place, atomic)
```

Then continue with the [command line guide](/guide/cli-usage) or the
[Rust library guide](/guide/library-api).

## Getting help

- [Troubleshooting](/guide/troubleshooting)
- [GitHub issues](https://github.com/swack-tools/oxidex/issues)
