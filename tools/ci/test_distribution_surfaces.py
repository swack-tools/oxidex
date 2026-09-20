"""Keep beta distribution documentation and helpers honest.

The beta release workflow publishes GitHub release assets only.  These tests
are intentionally text-level and buildless so the CI tools discovery catches
regressions before a stale package instruction becomes user-facing again.
"""

from pathlib import Path
import tomllib
import unittest


REPO = Path(__file__).resolve().parents[2]
README = (REPO / "README.md").read_text(encoding="utf-8")
CARGO_TOML = (REPO / "Cargo.toml").read_text(encoding="utf-8")
PACKAGING_GUIDE = (REPO / "docs/reference/packaging/packaging-guide.md").read_text(
    encoding="utf-8"
)
RELEASE_PAGE = (REPO / "docs/RELEASE-2.0.0-beta.1.md").read_text(encoding="utf-8")
LIBRARY_GUIDE = (REPO / "docs/guide/library-api.md").read_text(encoding="utf-8")
GETTING_STARTED = (REPO / "docs/guide/getting-started.md").read_text(encoding="utf-8")
API_REFERENCE = (REPO / "docs/reference/api-reference.md").read_text(encoding="utf-8")
TROUBLESHOOTING = (REPO / "docs/guide/troubleshooting.md").read_text(
    encoding="utf-8"
)
MIGRATION_GUIDE = (REPO / "docs/guide/migrating-from-1x.md").read_text(
    encoding="utf-8"
)
RELEASE_TODO = (REPO / "TODO_RELEASE_BETA.md").read_text(encoding="utf-8")
HOMEBREW_README_PATH = REPO / "packaging/homebrew/README.md"
HOMEBREW_README = (
    HOMEBREW_README_PATH.read_text(encoding="utf-8")
    if HOMEBREW_README_PATH.exists()
    else ""
)
PACKAGE_TEST = (REPO / "scripts/test-packages.sh").read_text(encoding="utf-8")
PACKAGE_BUILD = (REPO / "scripts/build-all-packages.sh").read_text(encoding="utf-8")


class DistributionSurfaceTests(unittest.TestCase):
    def test_readme_does_not_advertise_unpublished_rust_registries(self):
        self.assertNotIn("crates.io/crates/oxidex", README)
        self.assertNotIn("docs.rs/oxidex", README)
        self.assertRegex(README, r"not\s+published to crates\.io")

    def test_pre_tag_install_surfaces_only_offer_development_source_builds(self):
        """Catch a future edit that directs users to inputs not yet released."""
        for name, surface in {
            "README": README,
            "getting started": GETTING_STARTED,
            "packaging guide": PACKAGING_GUIDE,
        }.items():
            with self.subTest(surface=name):
                self.assertIn("signed tag and binary assets are pending", surface)
                self.assertRegex(
                    surface,
                    r"(?i)build (?:from source|this development line from source|from a development checkout)",
                )
                self.assertNotIn("use the signed Git tag", surface)
                self.assertNotIn("supported beta installation path is the signed asset", surface)

    def test_pre_tag_library_surfaces_use_a_development_ref_not_release_inputs(self):
        """Catch an instruction that treats the pending tag or assets as real."""
        for name, surface in {
            "library guide": LIBRARY_GUIDE,
            "API reference": API_REFERENCE,
        }.items():
            with self.subTest(surface=name):
                self.assertIn("development branch or commit", surface)
                self.assertRegex(
                    surface, r"signed tag and exact\s+`main` SHA remain pending"
                )
                self.assertNotIn('tag = "v2.0.0-beta.1"', surface)

    def test_beta_docs_make_no_package_name_availability_claims(self):
        """Catch a return of stale registry probes to beta-facing guidance."""
        for name, surface in {
            "release page": RELEASE_PAGE,
            "getting started": GETTING_STARTED,
            "troubleshooting": TROUBLESHOOTING,
            "migration guide": MIGRATION_GUIDE,
        }.items():
            with self.subTest(surface=name):
                self.assertNotIn("belongs to an unrelated", surface)
                self.assertNotIn("reserved stub crate", surface)
                self.assertNotIn("verified free", surface)
                self.assertNotIn("crates.io/api/v1", surface)
                self.assertNotIn("cargo publish", surface)

    def test_every_tag_crate_is_explicitly_not_publishable_for_the_beta(self):
        """Catch Cargo's default-publish behavior on a newly added tag crate."""
        manifests = sorted(REPO.glob("oxidex-tags*/Cargo.toml"))
        self.assertEqual(len(manifests), 8)
        versions = {}
        for manifest in manifests:
            with manifest.open("rb") as fh:
                package = tomllib.load(fh)["package"]
            versions[package["name"]] = package["version"]
            with self.subTest(package=package["name"]):
                self.assertIs(package.get("publish"), False)
        self.assertEqual(versions["oxidex-tags-shared"], "0.1.0")

    def test_beta_docs_keep_format_and_detection_scopes_explicit(self):
        self.assertIn("16,684 generated metadata tag", README)
        self.assertIn("131 formats for detection and 129 to a", README)
        self.assertNotIn("140+ formats", README)
        package_description = next(
            line for line in CARGO_TOML.splitlines() if line.startswith("description =")
        )
        self.assertNotIn("300+", package_description)
        self.assertNotIn("~99% accuracy", README)
        self.assertIn("does\nnot make an accuracy or performance claim", README)
        self.assertNotIn("Static binaries for Linux, macOS, and Windows", README)

    def test_beta_docs_do_not_reintroduce_unreceipted_magika_or_file_parity(self):
        changelog = (REPO / "CHANGELOG.md").read_text(encoding="utf-8")
        cli_usage = (REPO / "docs/guide/cli-usage.md").read_text(encoding="utf-8")
        self.assertNotIn("~99% detection accuracy", changelog)
        self.assertNotIn("Benchmarks included for signature vs Magika", changelog)
        self.assertNotIn("all 156 tags", cli_usage)
        self.assertIn("does\nnot make a per-file or corpus parity claim", cli_usage)

    def test_package_guide_states_beta_policy_and_has_no_stale_release_literals(self):
        self.assertRegex(
            PACKAGING_GUIDE,
            r"are not published by the current beta\s+release\s+automation",
        )
        self.assertIn("local experiments", PACKAGING_GUIDE)
        self.assertNotIn("github.com/oxidex/oxidex", PACKAGING_GUIDE)
        self.assertNotIn("v0.1.0", PACKAGING_GUIDE)
        self.assertNotIn("0.1.0", PACKAGING_GUIDE)
        self.assertNotIn("UPDATE_THIS_SHA256_AFTER_RELEASE", PACKAGING_GUIDE)

    def test_homebrew_is_quarantined_without_a_fake_url_or_checksum(self):
        self.assertFalse((REPO / "packaging/homebrew/oxidex.rb").exists())
        self.assertIn("not enabled for the beta", HOMEBREW_README)
        self.assertIn("signed tag and binary assets are pending", HOMEBREW_README)
        self.assertNotIn(
            "The release workflow publishes signed GitHub release assets only",
            HOMEBREW_README,
        )
        self.assertNotIn("github.com/oxidex/oxidex", HOMEBREW_README)
        self.assertNotIn("UPDATE_THIS_SHA256_AFTER_RELEASE", HOMEBREW_README)

    def test_package_helpers_derive_the_product_version(self):
        self.assertIn("cargo metadata", PACKAGE_TEST)
        self.assertNotIn('EXPECTED_VERSION="0.1.0"', PACKAGE_TEST)
        self.assertIn("cargo metadata", PACKAGE_BUILD)
        self.assertNotIn("Example: ./scripts/build-all-packages.sh 0.1.0", PACKAGE_BUILD)
        self.assertNotIn("/tmp/", PACKAGE_TEST)
        self.assertNotIn("github.com/oxidex/oxidex", PACKAGE_BUILD)

    def test_release_page_matches_removed_readme_registry_surfaces(self):
        self.assertIn("Option (c) is selected for this beta", RELEASE_PAGE)
        self.assertRegex(
            RELEASE_PAGE,
            r"no\s+crates\.io publication for the root crate or any tag crate",
        )
        self.assertIn("signed tag and final `main` SHA remain pending", RELEASE_PAGE)
        self.assertNotIn("If the tag crates are published", RELEASE_PAGE)
        self.assertNotIn("under (a) or (b)", RELEASE_PAGE)
        self.assertNotIn("One leftover: the `README.md` crates.io badge", RELEASE_PAGE)
        self.assertNotIn("points at `crates.io/crates/oxidex`", RELEASE_PAGE)

    def test_release_page_names_the_disabled_homebrew_placeholder(self):
        self.assertIn("packaging/homebrew/oxidex.rb.disabled", RELEASE_PAGE)
        self.assertIn("Homebrew is disabled for this beta", RELEASE_PAGE)
        self.assertNotIn("`packaging/homebrew/oxidex.rb`", RELEASE_PAGE)

    def test_library_guide_records_beta_no_crates_io_policy_without_live_name_claims(self):
        self.assertRegex(
            LIBRARY_GUIDE,
            r"neither the root crate nor the tag crates will be\s+published to crates\.io",
        )
        self.assertRegex(
            LIBRARY_GUIDE, r"signed tag and exact\s+`main` SHA remain pending"
        )
        self.assertIn("signed-tag dependency instructions remain pending", LIBRARY_GUIDE)
        self.assertNotIn("final crates.io decision", LIBRARY_GUIDE)
        self.assertNotIn("belongs to an unrelated crate", LIBRARY_GUIDE)

    def test_release_todo_resolves_beta_package_policy_but_keeps_release_inputs_pending(self):
        section = RELEASE_TODO[
            RELEASE_TODO.index("## 4. Version and package audit"):
            RELEASE_TODO.index("## 5. Documentation and factual-release audit")
        ]
        self.assertRegex(
            section, r"- \[x\] Keep the root crate `publish = false` for this beta;"
        )
        self.assertRegex(
            section,
            r"- \[x\] Record the crates\.io decision: no crates\.io publication",
        )
        self.assertRegex(section, r"root\s+crate\s+or any tag crate")
        self.assertRegex(section, r"- \[x\] Do not publish tag crates for this beta;")
        self.assertIn("signed tag and exact", section)
        self.assertIn("frozen `main` SHA remain pending", section)
        self.assertNotIn("Decide whether tag crates will be published manually", section)
        self.assertNotIn("> Record the crates.io and package-publication decision here.", section)


if __name__ == "__main__":
    unittest.main()
