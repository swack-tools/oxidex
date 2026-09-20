"""Security invariants for the documentation preview deployment workflow."""

from pathlib import Path
import unittest


WORKFLOW = (
    Path(__file__).resolve().parents[2]
    / ".github"
    / "workflows"
    / "deploy-docs-preview.yml"
)


def step_block(workflow: str, name: str) -> str:
    marker = f"      - name: {name}\n"
    start = workflow.index(marker)
    end = workflow.find("\n      - name: ", start + len(marker))
    return workflow[start:] if end == -1 else workflow[start:end]


class DocsPreviewWorkflowTests(unittest.TestCase):
    def test_untrusted_artifact_is_committed_before_deploy_key_is_available(self):
        workflow = WORKFLOW.read_text()
        prepare = step_block(workflow, "Prepare trusted publish repository")
        push = step_block(workflow, "Push to gh-pages of the preview repository")

        self.assertIn("find site -name .git", prepare)
        self.assertIn('PUBLISH="$RUNNER_TEMP/docs-preview-publish"', prepare)
        self.assertIn('git -C "$PUBLISH" init', prepare)
        self.assertIn('git -C "$PUBLISH" add -A', prepare)
        self.assertIn('git -C "$PUBLISH" commit', prepare)
        self.assertNotIn("DOCS_PREVIEW_DEPLOY_KEY", prepare)

        self.assertIn("DOCS_PREVIEW_DEPLOY_KEY", push)
        self.assertIn('git -C "$PUBLISH" push', push)
        self.assertNotIn("git init", push)
        self.assertNotIn("git add", push)
        self.assertNotIn("git commit", push)


if __name__ == "__main__":
    unittest.main()
