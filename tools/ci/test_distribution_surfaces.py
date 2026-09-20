"""Keep beta distribution documentation and helpers honest.

The beta release workflow publishes GitHub release assets only.  These tests
are intentionally text-level and buildless so the CI tools discovery catches
regressions before a stale package instruction becomes user-facing again.
"""

from pathlib import Path
import unittest


REPO = Path(__file__).resolve().parents[2]
README = (REPO / "README.md").read_text(encoding="utf-8")
PACKAGING_GUIDE = (REPO / "docs/reference/packaging/packaging-guide.md").read_text(
    encoding="utf-8"
)
RELEASE_PAGE = (REPO / "docs/RELEASE-2.0.0-beta.1.md").read_text(encoding="utf-8")
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
        self.assertIn("not published to crates.io", README)

    def test_package_guide_states_beta_policy_and_has_no_stale_release_literals(self):
        self.assertRegex(
            PACKAGING_GUIDE,
            r"not published\s+by the current beta release automation",
        )
        self.assertIn("local experiments", PACKAGING_GUIDE)
        self.assertNotIn("github.com/oxidex/oxidex", PACKAGING_GUIDE)
        self.assertNotIn("v0.1.0", PACKAGING_GUIDE)
        self.assertNotIn("0.1.0", PACKAGING_GUIDE)
        self.assertNotIn("UPDATE_THIS_SHA256_AFTER_RELEASE", PACKAGING_GUIDE)

    def test_homebrew_is_quarantined_without_a_fake_url_or_checksum(self):
        self.assertFalse((REPO / "packaging/homebrew/oxidex.rb").exists())
        self.assertIn("not enabled for the beta", HOMEBREW_README)
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
        self.assertNotIn("One leftover: the `README.md` crates.io badge", RELEASE_PAGE)
        self.assertNotIn("points at `crates.io/crates/oxidex`", RELEASE_PAGE)

    def test_release_page_names_the_disabled_homebrew_placeholder(self):
        self.assertIn("packaging/homebrew/oxidex.rb.disabled", RELEASE_PAGE)
        self.assertIn("Homebrew is disabled for this beta", RELEASE_PAGE)
        self.assertNotIn("`packaging/homebrew/oxidex.rb`", RELEASE_PAGE)


if __name__ == "__main__":
    unittest.main()
