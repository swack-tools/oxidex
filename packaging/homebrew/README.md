# Homebrew distribution

Homebrew is not enabled for the beta `2.0.0-beta.1` release.
The release workflow publishes signed GitHub release assets only; it does not
publish a formula or bottles to Homebrew.

There is deliberately no active formula in this directory. The disabled
placeholder is retained only as a reminder that a future formula must be
created from a real published release asset. Do not add a URL, tag, or
checksum until that release policy is approved and the asset has been
published. A source build from a checkout remains available to developers:

```bash
cargo build --release
```

When Homebrew support is intentionally enabled, add a real formula and its
tests in the same change as the workflow and documentation that publish it.
