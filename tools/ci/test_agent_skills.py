"""Tests for the canonical Claude-to-Codex skill mirror."""

from __future__ import annotations

import json
import os
import pathlib
import re
import shlex
import shutil
import subprocess
import tempfile
import unittest

from tools.ci import sync_agent_skills as sync


REPO = pathlib.Path(__file__).resolve().parents[2]
RELEASE_SKILLS = (
    "exiftool-parity",
    "oxidex-release-documentation",
    "oxidex-release-finalization",
)


def canonical(skill: str, relative: str) -> str:
    """Read a file from the canonical shared-skill tree."""

    return (REPO / ".claude/skills" / skill / relative).read_text(encoding="utf-8")


def bash_snippets(skill: str, relative: str) -> list[str]:
    """Return fenced Bash snippets from one canonical skill file."""

    return re.findall(r"```bash\n(.*?)```", canonical(skill, relative), re.DOTALL)


def _shell_tokens(snippet: str):
    """Yield raw shell words/operators, excluding comments and heredoc data.

    Keep quoting until after operator classification: a printed ';' is a word,
    and a newline inside a quoted word is not a command boundary.
    """

    token_pattern = re.compile(
        r"(?P<continuation>\\\n)|(?P<space>[^\S\n]+)|(?P<comment>\#[^\n]*)|"
        r"(?P<word>(?:\\[\s\S]|'[^']*'|\"(?:\\[\s\S]|[^\"\\])*\"|"
        r"[^\s\\'\";&|()<>])+)|(?P<operator><<-|[;&|()<>]+)|(?P<newline>\n)"
    )
    word_parts = re.compile(r"'[^']*'|\"(?:\\[\s\S]|[^\"\\])*\"|\\[\s\S]|[^\\'\"]+")
    delimiters: list[tuple[str, bool]] = []
    heredoc_operator = None
    position = 0
    while position < len(snippet):
        match = token_pattern.match(snippet, position)
        if match is None:
            raise ValueError("Unclosed shell quote or escape")
        position = match.end()
        kind, token = match.lastgroup, match.group()
        if kind in {"continuation", "space", "comment"}:
            continue
        if kind == "word":
            # Single quotes preserve every byte. Elsewhere consume escapes
            # in pairs, so an escaped backslash cannot continue a newline.
            token = "".join(
                part if part.startswith("'") else re.sub(
                    r"\\[\s\S]",
                    lambda escape: "" if escape.group() == "\\\n" else escape.group(),
                    part,
                )
                for part in word_parts.findall(token)
            )
        if heredoc_operator is not None and kind == "word":
            delimiters.append((shlex.split(token)[0], heredoc_operator == "<<-"))
            heredoc_operator = None
        if kind == "operator" and token in {"<<", "<<-"}:
            heredoc_operator = token
        yield token
        if kind == "newline":
            # Consume raw lines, not shell tokens: heredoc data may contain
            # unmatched quotes, comments, and apparent command separators.
            for delimiter, strip_tabs in delimiters:
                while position < len(snippet):
                    end = snippet.find("\n", position)
                    end = len(snippet) if end == -1 else end + 1
                    candidate = snippet[position:end].rstrip("\r\n")
                    position = end
                    if strip_tabs:
                        candidate = candidate.lstrip("\t")
                    if candidate == delimiter:
                        break
            delimiters.clear()


def active_shell_commands(snippets: list[str]) -> list[tuple[str, ...]]:
    """Return normalized argv for syntactic shell command invocations."""

    commands: list[tuple[str, ...]] = []
    separators = {"then", "do", "else", "elif", "fi", "done"}
    prefixes = {"if", "while", "until", "!"}
    assignment = re.compile(r"^[A-Za-z_]\w*=.*$", re.DOTALL)

    def append_command(words: list[str]) -> None:
        while words and (words[0] in prefixes or assignment.match(words[0])):
            words.pop(0)
        if words and words[0] in {"command", "env"}:
            words.pop(0)
            while words and (words[0].startswith("-") or assignment.match(words[0])):
                words.pop(0)
        if words:
            commands.append(tuple(shlex.split(" ".join(words))))

    for snippet in snippets:
        current: list[str] = []
        for token in _shell_tokens(snippet):
            if token == "\n" or (token and set(token) <= set(";&|()")):
                append_command(current)
                current = []
            elif not current and token in separators:
                continue
            else:
                current.append(token)
        append_command(current)
    return commands


def has_shell_command(
    commands: list[tuple[str, ...]],
    prefix: str,
    *required_fragments: str,
) -> bool:
    """Return whether an invocation has the prefix and contiguous fragments."""

    expected_prefix = tuple(shlex.split(prefix))
    fragments = [tuple(shlex.split(fragment)) for fragment in required_fragments]
    for command in commands:
        if command[: len(expected_prefix)] != expected_prefix:
            continue
        if all(
            any(
                command[index : index + len(fragment)] == fragment
                for index in range(len(command) - len(fragment) + 1)
            )
            for fragment in fragments
        ):
            return True
    return False


def frontmatter_description(skill: str) -> str:
    """Return the description from a skill's YAML frontmatter."""

    match = re.search(
        r"(?m)^description:\s*(.+)$", canonical(skill, "SKILL.md")
    )
    if match is None:
        raise AssertionError(f"{skill} has no frontmatter description")
    return match.group(1).strip().strip('"')


def make_fixture(root: pathlib.Path, *, canonical: str, mirror: str) -> pathlib.Path:
    """Create the smallest repository fixture needed by the mirror tests."""

    (root / ".claude/skills/alpha").mkdir(parents=True)
    (root / ".agents/skills/alpha").mkdir(parents=True)
    (root / ".gitignore").write_text(
        ".claude/*\n"
        "!.claude/skills/\n"
        ".claude/skills/*\n"
        "!.claude/skills/alpha/\n",
        encoding="utf-8",
    )
    (root / ".claude/skills/alpha/SKILL.md").write_text(canonical, encoding="utf-8")
    (root / ".agents/skills/alpha/SKILL.md").write_text(mirror, encoding="utf-8")
    return root


class SkillMirrorTests(unittest.TestCase):
    def test_active_shell_text_preserves_single_quoted_backslash_newline(self):
        command = "python3 tools/ci/validate_release_receipt.py --kind parity"
        snippet = "'python\\\n3' tools/ci/validate_release_receipt.py --kind parity\n"
        commands = active_shell_commands([snippet])
        self.assertFalse(has_shell_command(commands, command))
        self.assertEqual(commands[0][0], "python\\\n3")

    def test_active_shell_text_comment_backslash_does_not_hide_next_command(self):
        command = "python3 tools/ci/validate_release_receipt.py --kind parity"
        snippet = "# note \\\n" + command + "\n"
        self.assertTrue(has_shell_command(active_shell_commands([snippet]), command))

    def test_active_shell_text_escaped_backslash_does_not_continue_newline(self):
        command = "python3 tools/ci/validate_release_receipt.py --kind parity"
        snippet = "echo \\\\\n" + command + "\n"
        self.assertTrue(has_shell_command(active_shell_commands([snippet]), command))

    def test_active_shell_text_keeps_printed_separators_inert(self):
        command = "python3 tools/ci/validate_release_receipt.py --kind parity"
        for printer in ("echo", "printf '%s\\n'"):
            for separator in (";", "&&", "||", "|", "(", ")", "then", "do", "else"):
                for quoted in (f"'{separator}'", f'"{separator}"'):
                    snippet = f"{printer} {quoted} {command}\n"
                    with self.subTest(snippet=snippet):
                        self.assertFalse(
                            has_shell_command(active_shell_commands([snippet]), command)
                        )

    def test_active_shell_text_keeps_escaped_and_quoted_newlines_inert(self):
        command = "python3 tools/ci/validate_release_receipt.py --kind parity"
        for snippet in (
            f"echo \\; {command}\n",
            f"echo then {command}\n",
            f"echo '\n{command}\n'\n",
            f'printf "%s\\n" "\n{command}\n"\n',
        ):
            with self.subTest(snippet=snippet):
                self.assertFalse(
                    has_shell_command(active_shell_commands([snippet]), command)
                )

    def test_active_shell_text_finds_commands_after_comments(self):
        command = "python3 tools/ci/validate_release_receipt.py --kind parity"
        for preceding in ("# note", "echo note # trailing note", "# 'unclosed quote"):
            with self.subTest(preceding=preceding):
                self.assertTrue(has_shell_command(
                    active_shell_commands([f"{preceding}\n{command}\n"]), command
                ))

    def test_active_shell_text_resumes_after_heredocs(self):
        command = "python3 tools/ci/validate_release_receipt.py --kind parity"
        for opening, closing in (
            ("<<END", "END"), ("<<'END'", "END"), ('<<"END"', "END"),
            ("<<-END", "\tEND"),
        ):
            snippet = f"cat {opening}\n{command}\n{closing}\n{command}\n"
            with self.subTest(opening=opening):
                commands = active_shell_commands([snippet])
                self.assertEqual(commands.count(tuple(command.split())), 1)

    def test_active_shell_text_resumes_after_multiple_heredocs(self):
        command = "python3 tools/ci/validate_release_receipt.py --kind parity"
        snippet = f"cat <<FIRST <<'SECOND'\n{command}\nFIRST\n{command}\nSECOND\n{command}\n"
        self.assertEqual(active_shell_commands([snippet]).count(tuple(command.split())), 1)

    def test_active_shell_text_ignores_inert_heredoc_openers(self):
        command = "python3 tools/ci/validate_release_receipt.py --kind parity"
        for preceding in ("echo '<<END'", 'echo "<<END"', "# cat <<END"):
            with self.subTest(preceding=preceding):
                self.assertTrue(has_shell_command(
                    active_shell_commands([f"{preceding}\n{command}\n"]), command
                ))

    def test_active_shell_text_preserves_real_command_forms(self):
        command = "python3 tools/ci/validate_release_receipt.py --kind parity"
        for snippet in (
            command, f"echo note; {command}", f"true && {command}",
            f"false || {command}", f"printf data | {command}", f"({command})",
            f"if true; then {command}; fi", f"while false; do {command}; done",
            f"if false; then :; elif {command}; then :; else {command}; fi",
            f"CHECK=1 {command}", f"command {command}", f"env CHECK=1 {command}",
            "python3 tools/ci/validate_release_receipt.py \\\n --kind parity",
            '"python3" tools/ci/validate_release_receipt.py --kind "parity"',
        ):
            with self.subTest(snippet=snippet):
                self.assertTrue(has_shell_command(active_shell_commands([snippet]), command))

    def test_active_shell_text_ignores_inert_validator_mentions(self):
        command = "python3 tools/ci/validate_release_receipt.py --kind parity"
        inert = (
            f"printf '%s\\n' '{command}'\n",
            f"echo {command}\n",
            f": <<'NOT_RUN'\n{command}\nNOT_RUN\n",
        )
        for snippet in inert:
            with self.subTest(snippet=snippet):
                self.assertFalse(
                    has_shell_command(active_shell_commands([snippet]), command)
                )
        self.assertTrue(
            has_shell_command(active_shell_commands([f"{command}\n"]), command)
        )

    def test_all_release_skills_are_allowlisted(self):
        self.assertTrue(
            set(RELEASE_SKILLS).issubset(sync.shared_skill_names(REPO)),
            sync.shared_skill_names(REPO),
        )

    def test_release_skills_have_codex_ui_metadata_and_implicit_routing(self):
        for skill in RELEASE_SKILLS:
            path = REPO / ".claude/skills" / skill / "agents/openai.yaml"
            with self.subTest(skill=skill):
                self.assertTrue(path.is_file(), f"missing {path.relative_to(REPO)}")
                text = path.read_text(encoding="utf-8")
                display = re.search(r'(?m)^\s{2}display_name:\s*"([^"]+)"$', text)
                short = re.search(r'(?m)^\s{2}short_description:\s*"([^"]+)"$', text)
                prompt = re.search(r'(?m)^\s{2}default_prompt:\s*"([^"]+)"$', text)
                self.assertIsNotNone(display)
                self.assertIsNotNone(short)
                self.assertIsNotNone(prompt)
                assert short is not None and prompt is not None
                self.assertGreaterEqual(len(short.group(1)), 25)
                self.assertLessEqual(len(short.group(1)), 64)
                self.assertIn(f"${skill}", prompt.group(1))
                self.assertRegex(
                    text,
                    r"(?ms)^policy:\s*$.*^\s{2}allow_implicit_invocation:\s*true\s*$",
                )

    def test_release_skill_entrypoints_stay_under_500_words(self):
        for skill in RELEASE_SKILLS:
            words = canonical(skill, "SKILL.md").split()
            with self.subTest(skill=skill, words=len(words)):
                self.assertLessEqual(len(words), 500)

    def test_release_skill_descriptions_include_positive_and_negative_triggers(self):
        expected = {
            "exiftool-parity": ("pinned ExifTool", "ordinary parser"),
            "oxidex-release-documentation": ("GitHub Pages", "typo"),
            "oxidex-release-finalization": ("tagging", "ordinary feature"),
        }
        for skill, phrases in expected.items():
            description = frontmatter_description(skill)
            with self.subTest(skill=skill):
                self.assertTrue(description.startswith("Use when"), description)
                for phrase in phrases:
                    self.assertIn(phrase, description)

    def test_generic_release_skills_have_no_beta_or_dated_lock_constants(self):
        forbidden = {
            "fixed beta version": re.compile(r"v?2\.0\.0-beta\.1"),
            "dated lock controller": re.compile(r"20260917-group1-batch2"),
        }
        for skill in RELEASE_SKILLS:
            root = REPO / ".claude/skills" / skill
            for path in root.rglob("*"):
                if not path.is_file() or path.suffix not in {".md", ".json", ".yaml"}:
                    continue
                text = path.read_text(encoding="utf-8")
                for label, pattern in forbidden.items():
                    with self.subTest(skill=skill, path=path.relative_to(root), label=label):
                        self.assertNotRegex(text, pattern)

    def test_release_routing_names_all_three_skills(self):
        for relative in (
            "AGENTS.md",
            "docs/contributing/release-checklist.md",
        ):
            text = (REPO / relative).read_text(encoding="utf-8")
            for skill in (
                "exiftool-parity",
                "oxidex-release-documentation",
                "oxidex-release-finalization",
            ):
                with self.subTest(path=relative, skill=skill):
                    self.assertIn(skill, text)

    def test_claude_routes_only_authorized_release_promotion_to_main(self):
        text = " ".join((REPO / "AGENTS.md").read_text(encoding="utf-8").split())
        for phrase in (
            "Ordinary development",
            "reviewed PR whose base is `main`",
            "`main` commit",
            "separate explicit maintainer authorization",
        ):
            self.assertIn(phrase, text)

    def test_claude_is_an_import_only_not_a_duplicate_policy_store(self):
        self.assertEqual(
            (REPO / "CLAUDE.md").read_text(encoding="utf-8").strip(),
            "@AGENTS.md",
        )

    def test_release_checklist_requires_receipts_signed_tag_and_artifact_proof(self):
        text = (REPO / "docs/contributing/release-checklist.md").read_text(
            encoding="utf-8"
        )
        for phrase in (
            "parity receipt",
            "documentation receipt",
            "exact `main` commit",
            "signed tag",
            "GitHub release",
            "code signature",
            "Gatekeeper",
            "stapled notarization",
        ):
            self.assertIn(phrase, text)
        self.assertNotIn("git tag -a", text)

    def test_release_docs_route_to_local_audit_and_workflow_pages_proof(self):
        checklist = " ".join(
            (REPO / "docs/contributing/release-checklist.md")
            .read_text(encoding="utf-8")
            .split()
        )
        docs_site = " ".join(
            (REPO / "docs/contributing/docs-site.md")
            .read_text(encoding="utf-8")
            .split()
        )
        for phrase in (
            "production-equivalent local",
            "browser automation",
            "human visual review",
            "live Pages deployment is optional",
        ):
            with self.subTest(path="release-checklist.md", phrase=phrase):
                self.assertIn(phrase, checklist)
        for phrase in (
            "production-equivalent local",
            "every reconciled inventory route",
            "browser automation",
            "human visual review",
            "`build_type: workflow`",
            "not deployment proof",
        ):
            with self.subTest(path="docs-site.md", phrase=phrase):
                self.assertIn(phrase, docs_site)

    def test_docs_site_requires_all_route_browser_navigation(self):
        text = " ".join(
            (REPO / "docs/contributing/docs-site.md")
            .read_text(encoding="utf-8")
            .split()
        )
        self.assertIn(
            "Use browser automation to navigate every reconciled inventory route",
            text,
        )
        self.assertIn(
            "capture console, page, request, network, and HTTP failures",
            text,
        )
        self.assertIn("representative screenshot matrix", text)
        self.assertIn(
            "this matrix complements the exhaustive browser navigation",
            text,
        )

    def test_parity_markdown_has_no_bare_oracle_command(self):
        skill = REPO / ".claude/skills/exiftool-parity"
        # Command tokens, including inline examples, must use the pinned argv.
        bare = re.compile(r"(?<![\w/.-])exiftool\s+(?:-[A-Za-z]|FILE\b)")
        for path in skill.rglob("*.md"):
            with self.subTest(path=path.relative_to(skill)):
                self.assertNotRegex(path.read_text(encoding="utf-8"), bare)

    def test_parity_release_instrument_contract(self):
        entrypoint = canonical("exiftool-parity", "SKILL.md")
        text = canonical("exiftool-parity", "references/harnesses.md")
        self.assertIn("(references/harnesses.md)", entrypoint)
        self.assertNotIn("/tmp/oxidex-perl538-build-", text)
        for phrase in (
            "/Users/allen/oxidex-ops/toolchains/perl-5.38.2/prefix/bin/perl5.38.2",
            "/Users/allen/oxidex-ops/cache/exiftool/13.59/combined-samples",
            "/Users/allen/oxidex-ops/cache/exiftool/$PARITY_PIN",
            ".exiftool-version", "DOCX", "--recursive", "--min-files",
            "--min-tags", "--json-out", "blocked", "strict.pm",
        ):
            self.assertIn(phrase, text)

    def test_release_skills_call_receipt_and_oracle_validators(self):
        parity = active_shell_commands(
            bash_snippets("exiftool-parity", "references/harnesses.md")
        )
        finalization = active_shell_commands(
            bash_snippets("oxidex-release-finalization", "references/gates.md")
        )
        self.assertTrue(has_shell_command(parity, "python3 tools/ci/release_oracle.py"))
        self.assertTrue(
            has_shell_command(
                parity,
                "python3 tools/ci/validate_release_receipt.py --kind parity",
                "--candidate-sha $PARITY_SHA",
            )
        )
        for kind in ("parity", "documentation"):
            self.assertTrue(
                has_shell_command(
                    finalization,
                    f"python3 tools/ci/validate_release_receipt.py --kind {kind}",
                )
            )
        published = active_shell_commands(
            bash_snippets(
                "oxidex-release-finalization",
                "references/github-release-and-macos.md",
            )
        )
        self.assertTrue(
            has_shell_command(
                published,
                "python3 tools/ci/validate_release_receipt.py --kind finalization",
                "--candidate-sha $CANDIDATE_SHA",
            )
        )

    def test_release_promotion_requires_zero_unresolved_review_threads(self):
        text = canonical("oxidex-release-finalization", "references/gates.md")
        for required in (
            "reviewThreads(first: 100)",
            "isResolved",
            "isOutdated",
            "unresolved-actionable-review-threads",
            "review-threads.json",
            "python3 tools/ci/release_pr_gate.py",
            '--expected-head "$CANDIDATE_SHA"',
            "reviewed-promotion.json",
        ):
            self.assertIn(required, text)

    def test_macos_claim_matches_binary_signing_and_dmg_notarization(self):
        skill = canonical(
            "oxidex-release-finalization", "references/github-release-and-macos.md"
        )
        workflow = (REPO / ".github/workflows/release.yml").read_text(encoding="utf-8")
        combined = skill + workflow
        self.assertNotIn("Signed, notarized, stapled macOS disk image", combined)
        self.assertNotIn("Signed and notarized macOS DMG installer", combined)
        for required in (
            "Notarized and stapled macOS DMG containing the signed executable",
            "EXPECTED_DEVELOPER_ID",
            "EXPECTED_TEAM_IDENTIFIER",
            "TeamIdentifier",
        ):
            self.assertIn(required, combined)

    def test_parity_receipt_separates_measurement_families(self):
        path = REPO / ".claude/skills/exiftool-parity/templates/release-parity-receipt.json"
        self.assertTrue(path.is_file(), "release parity receipt is absent")
        receipt = json.loads(path.read_text(encoding="utf-8"))
        self.assertEqual(receipt["schema_version"], 1)
        self.assertEqual(receipt["status"], "unverified")
        for field in ("oxidex_sha", "exiftool_version", "oracle", "corpora", "regressions", "refusals"):
            self.assertIn(field, receipt)
        for family in ("conformance", "authenticated_reads", "generated_catalog", "write_matrix"):
            self.assertIsInstance(receipt[family], dict)
            self.assertEqual(receipt[family]["status"], "unverified")
        self.assertNotIn("overall_parity_percent", receipt)

    def test_release_receipts_have_schemas_and_validate_as_templates(self):
        from tools.ci import validate_release_receipt

        entries = (
            (
                "parity",
                REPO / ".claude/skills/exiftool-parity/templates/release-parity-receipt.json",
                REPO / ".claude/skills/exiftool-parity/templates/release-parity-receipt.schema.json",
            ),
            (
                "documentation",
                REPO / ".claude/skills/oxidex-release-documentation/templates/documentation-release-receipt.json",
                REPO / ".claude/skills/oxidex-release-documentation/templates/documentation-release-receipt.schema.json",
            ),
            (
                "finalization",
                REPO / ".claude/skills/oxidex-release-finalization/templates/release-finalization-receipt.json",
                REPO / ".claude/skills/oxidex-release-finalization/templates/release-finalization-receipt.schema.json",
            ),
        )
        for kind, template_path, schema_path in entries:
            with self.subTest(kind=kind):
                template = json.loads(template_path.read_text(encoding="utf-8"))
                schema = json.loads(schema_path.read_text(encoding="utf-8"))
                self.assertEqual(schema["$schema"], "https://json-schema.org/draft/2020-12/schema")
                self.assertEqual(schema["title"], f"OxiDex {kind} release receipt")
                self.assertIs(schema["additionalProperties"], False)
                self.assertTrue(schema["allOf"], "verified receipts need conditional constraints")
                self.assertIn("$defs", schema)
                self.assertEqual(
                    validate_release_receipt.validate_receipt(kind, template, template=True),
                    [],
                )

    def test_finalization_template_has_explicit_release_evidence_slots(self):
        receipt = json.loads(canonical(
            "oxidex-release-finalization", "templates/release-finalization-receipt.json"
        ))
        for field in (
            "candidate_tree", "main_tree", "receipts", "packaging", "promotion",
            "authorization", "tag_verification",
        ):
            self.assertIn(field, receipt)
        self.assertIn("review_threads_evidence", receipt["promotion"])
        self.assertIn("expected_developer_id", receipt["macos_verification"])
        self.assertIn("expected_team_identifier", receipt["macos_verification"])

    def test_parity_matrix_report_isolates_writes_and_checks_source(self):
        text = canonical("exiftool-parity", "references/harnesses.md")
        snippets = bash_snippets("exiftool-parity", "references/harnesses.md")
        report = next((s for s in snippets if "report --check-baseline" in s), "")
        self.assertTrue(report, "matrix report needs an executable isolation recipe")
        for required in (
            'TAGMATRIX_REPO="$PARITY_REPORT_ROOT"',
            'git show "${PARITY_SHA}:docs/reference/jpeg-tag-baseline.json"',
            'test -s "$PARITY_REPORT_BASELINE"',
            'shasum -a 256 "$PARITY_REPORT_BASELINE"',
            'test "$(git rev-parse \'HEAD^{commit}\')" = "$PARITY_SHA"',
            'test -z "$(git status --porcelain)"',
        ):
            self.assertIn(required, report)
        self.assertIn("Never revert", text)

    def test_parity_generated_table_recipe_is_explicitly_blocked(self):
        text = canonical("exiftool-parity", "references/harnesses.md")
        self.assertNotIn("Run `just verify-tables`", text)
        for required in ("Generated-table verification is blocked", "verify_subdirs.py", "/usr/bin/perl", "shebang"):
            self.assertIn(required, text)

    def test_parity_rename_votes_are_provisional_heuristics(self):
        text = canonical("exiftool-parity", "references/release-metrics.md")
        self.assertNotIn("table-supported name relationship", text)
        for required in ("infer_renames", "unique", "normalized name", "distinctive", "pinned-table investigation", "provisional"):
            self.assertIn(required, text)

    def test_shared_skill_allowlist_is_discovered(self):
        self.assertIn("exiftool-parity", sync.shared_skill_names(REPO))

    def test_agents_mirror_matches_canonical(self):
        self.assertEqual(sync.compare_skill_mirror(REPO), [])

    def test_release_finalization_contract(self):
        text = canonical("oxidex-release-finalization", "SKILL.md")
        for phrase in (
            "reviewed PR",
            "exact `main` commit",
            "explicit maintainer",
            "Gatekeeper",
            "stapled",
            "do not move",
        ):
            self.assertIn(phrase, text)

    def test_candidate_gates_use_recorded_release_specific_cargo_target(self):
        text = canonical("oxidex-release-finalization", "references/gates.md")
        self.assertNotIn("codex-release-engineering-skills-target", text)
        for required in (
            'CANDIDATE_CARGO_TARGET_DIR="$EVIDENCE_DIR/cargo-target-candidate"',
            'test ! -e "$CANDIDATE_CARGO_TARGET_DIR"',
            '"$EVIDENCE_DIR/candidate-cargo-target.txt"',
            'CARGO_TARGET_DIR="$CANDIDATE_CARGO_TARGET_DIR"',
        ):
            self.assertIn(required, text)

    def test_version_inventory_finds_old_archive_and_prose_versions(self):
        snippets = bash_snippets("oxidex-release-finalization", "references/gates.md")
        inventory = next((s for s in snippets if "VERSION_LITERAL_RE=" in s), "")
        self.assertTrue(inventory, "version inventory must search independently of VERSION")
        with tempfile.TemporaryDirectory() as tmp:
            root = pathlib.Path(tmp)
            (root / "evidence").mkdir()
            (root / "formula.rb").write_text(
                'url "https://example.test/archive/refs/tags/v0.1.0.tar.gz"\n',
                encoding="utf-8",
            )
            (root / "README.md").write_text("Install OxiDex 1.7.9 today.\n", encoding="utf-8")
            (root / "Cargo.toml").write_text('[dependencies]\nexample = "3.4.5"\n', encoding="utf-8")
            subprocess.run(["git", "init", "-q", str(root)], check=True)
            subprocess.run(["git", "add", "formula.rb", "README.md", "Cargo.toml"], cwd=root, check=True)
            result = subprocess.run(
                ["bash", "-c", inventory], cwd=root, text=True, capture_output=True,
                env={**os.environ, "VERSION": "2.0.0-beta.1", "EVIDENCE_DIR": str(root / "evidence")},
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            evidence = (root / "evidence/version-literals.txt").read_text(encoding="utf-8")
            for literal in ("v0.1.0.tar.gz", "1.7.9", "3.4.5"):
                self.assertIn(literal, evidence)

    def test_downloaded_macos_payloads_require_run_provenance_and_identity(self):
        text = canonical("oxidex-release-finalization", "references/github-release-and-macos.md")
        for required in (
            'actions/runs/$RELEASE_RUN_ID/artifacts',
            '.workflow_run.id == $run', '.workflow_run.head_sha == $sha',
            'gh run download "$RELEASE_RUN_ID"',
            'cmp "$MAC_BIN" "$RUN_MAC_BIN"', 'cmp "$DMG" "$RUN_DMG"',
            'mktemp -d "$EVIDENCE_DIR/dmg-mount.XXXXXX"',
            'hdiutil attach -readonly', '-mountpoint "$DMG_MOUNT"',
            'trap cleanup_macos_mount EXIT', 'hdiutil detach "$DMG_MOUNT"',
            'rmdir "$DMG_MOUNT"', 'macos-cleanup.log',
            'codesign --verify --strict --verbose=4 "$MAC_DMG_PAYLOAD"',
            'spctl --assess --type execute --verbose=4 "$MAC_DMG_PAYLOAD"',
            'test "$MAC_BIN_SHA256" = "$DMG_PAYLOAD_SHA256"',
            '"$MAC_BIN" --version', '"$MAC_DMG_PAYLOAD" --version',
            '"oxidex $VERSION"',
        ):
            self.assertIn(required, text)

    def test_release_comparison_tests_scope_panic_override_to_test_invocation(self):
        text = canonical("exiftool-parity", "references/harnesses.md")
        self.assertIn(
            "CARGO_PROFILE_RELEASE_PANIC=unwind cargo test --release --features exiftool-comparison -- --nocapture",
            text,
        )
        self.assertNotIn("export CARGO_PROFILE_RELEASE_PANIC", text)
        self.assertNotRegex(text, r"CARGO_PROFILE_RELEASE_PANIC=unwind[^\n]*cargo build")

    def test_release_documentation_contract(self):
        skill = REPO / ".claude/skills/oxidex-release-documentation"
        self.assertTrue((skill / "SKILL.md").is_file(), "documentation skill is absent")
        text = "\n".join(
            canonical("oxidex-release-documentation", relative)
            for relative in (
                "SKILL.md",
                "references/factuality-ledger.md",
                "references/github-pages-audit.md",
                "references/benchmark-policy.md",
            )
        )
        for phrase in (
            "every rendered route", "current", "historical", "mobile", "dark theme",
            "build_type", "workflow", "gh-pages", "exact candidate commit",
            "tools/docs-local-deploy.sh", "live deployment",
        ):
            with self.subTest(phrase=phrase):
                self.assertIn(phrase, text)

    def test_release_documentation_receipt_contract(self):
        path = REPO / ".claude/skills/oxidex-release-documentation/templates/documentation-release-receipt.json"
        self.assertTrue(path.is_file(), "documentation receipt is absent")
        receipt = json.loads(path.read_text(encoding="utf-8"))
        self.assertEqual(receipt["schema_version"], 1)
        for field in (
            "version", "candidate_sha", "parity_receipt", "claims", "pages", "benchmarks",
            "local_build", "visual_review", "pages_pipeline", "live_deployment", "status", "unresolved",
        ):
            self.assertIn(field, receipt)
        self.assertIsInstance(receipt["pages"], list)
        self.assertIsInstance(receipt["claims"], list)
        self.assertEqual(receipt["status"], "unverified")
        self.assertEqual(receipt["live_deployment"]["status"], "not_run")
        for field in ("snapshot_manifest_sha256", "crawl_sha256", "server_log"):
            self.assertIn(field, receipt["local_build"])
        for field in ("automation_manifest_sha256", "visual_matrix", "visual_matrix_sha256", "screenshots_path"):
            self.assertIn(field, receipt["visual_review"])

    def test_release_documentation_uses_tracked_browser_audit(self):
        package = json.loads((REPO / "docs/package.json").read_text(encoding="utf-8"))
        self.assertEqual(package["devDependencies"]["playwright"], "1.63.0")
        self.assertEqual(
            package["scripts"]["docs:audit-release"],
            "node ../tools/docs/release-audit.mjs",
        )
        audit = canonical("oxidex-release-documentation", "references/github-pages-audit.md")
        for required in (
            "--build-only", "--output", "snapshot-manifest.json",
            "node tools/docs/release-audit.mjs", "automation-manifest.json", "crawl.json",
            "visual-matrix.json", "server.log",
            "tools/docs/release-audit-representatives.json",
        ):
            self.assertIn(required, audit)
        representatives = json.loads(
            (REPO / "tools/docs/release-audit-representatives.json").read_text(encoding="utf-8")
        )["routes"]
        for route in (
            "/", "/changelog", "/guide", "/guide/getting-started",
            "/guide/migrating-from-1x", "/reference", "/guide/exiftool-parity",
            "/status", "/performance", "/reference/formats",
        ):
            self.assertIn(route, representatives)

    def test_docs_benchmark_links_have_factual_no_artifact_fallbacks(self):
        for relative in (
            "report/index.html",
            "single_extraction/report/index.html",
            "batch_100_jpegs/report/index.html",
            "format_comparison/report/index.html",
            "format_detection/report/index.html",
            "full_read_metadata/report/index.html",
        ):
            text = (REPO / "docs/public/benchmarks" / relative).read_text(encoding="utf-8")
            with self.subTest(relative=relative):
                self.assertIn("unavailable for this build", text)
                self.assertIn("did not receive a Criterion benchmark artifact", text)
                self.assertIn("No benchmark result is being claimed here", text)
        workflow = (REPO / ".github/workflows/deploy-docs.yml").read_text(encoding="utf-8")
        self.assertLess(
            workflow.index("Build VitePress site"),
            workflow.index("Copy benchmark reports to site"),
            "real Criterion output must overwrite the public fallback pages",
        )

    def test_release_documentation_verifies_local_candidate_without_deployment(self):
        receipt = json.loads(canonical(
            "oxidex-release-documentation", "templates/documentation-release-receipt.json"
        ))
        self.assertIs(receipt["live_deployment"].get("required_for_documentation_verification"), False)
        self.assertNotIn("promotion_readiness", receipt)
        self.assertNotIn("phase", receipt)
        self.assertIsNone(receipt["candidate_tree"])
        self.assertEqual(receipt["visual_review"]["human_review"]["status"], "unverified")
        self.assertIn("automation_manifest", receipt["visual_review"])
        skill = canonical("oxidex-release-documentation", "SKILL.md")
        for phrase in ("status: verified", "optional", "before tag authorization", "human screenshot review"):
            self.assertIn(phrase, skill)
        audit = canonical("oxidex-release-documentation", "references/github-pages-audit.md")
        for phrase in ("Playwright", "requestfailed", "pageerror", "1440", "390", "actionlint"):
            self.assertIn(phrase, audit)
        self.assertIn("every representative route/viewport/theme cell", audit)

    def test_release_finalization_pipeline_snippets_enable_pipefail(self):
        for relative in ("references/gates.md", "references/github-release-and-macos.md"):
            for index, snippet in enumerate(bash_snippets("oxidex-release-finalization", relative)):
                if "|" not in snippet:
                    continue
                first_command = next(line.strip() for line in snippet.splitlines() if line.strip())
                with self.subTest(relative=relative, snippet=index):
                    self.assertEqual(first_command, "set -euo pipefail")

    def test_release_finalization_reruns_are_bound_to_main_sha(self):
        text = canonical("oxidex-release-finalization", "references/gates.md")
        for phrase in (
            "POST_MERGE_WORKTREE",
            'git worktree add --detach "$POST_MERGE_WORKTREE" "$MAIN_SHA"',
            'MAIN_HEAD=$(git -C "$POST_MERGE_WORKTREE" rev-parse \'HEAD^{commit}\')',
            'test "$MAIN_HEAD" = "$MAIN_SHA"',
            "MAIN_CARGO_TARGET_DIR",
            "MAIN_EVIDENCE_DIR",
        ):
            self.assertIn(phrase, text)

    def test_release_finalization_selects_tag_bound_workflow_runs(self):
        text = canonical(
            "oxidex-release-finalization", "references/github-release-and-macos.md"
        )
        for phrase in (
            "headBranch",
            ".headBranch == $tag",
            ".headSha == $sha",
            ".workflowName == $workflow",
            '.event == "push"',
            "length == 1",
            "selected-release-run.json",
            "selected-docker-run.json",
            "RELEASE_RUN_ID=$(jq -er",
            "DOCKER_RUN_ID=$(jq -er",
        ):
            self.assertIn(phrase, text)

    def test_release_finalization_persists_successful_final_workflow_runs(self):
        text = canonical(
            "oxidex-release-finalization", "references/github-release-and-macos.md"
        )
        for run_id, workflow, evidence, next_marker in (
            (
                "RELEASE_RUN_ID",
                "Release",
                "final-release-run.json",
                'gh run watch "$DOCKER_RUN_ID"',
            ),
            ("DOCKER_RUN_ID", "Docker", "final-docker-run.json", "```"),
        ):
            watch = f'gh run watch "${run_id}" --exit-status'
            view = f'gh run view "${run_id}" --json'
            with self.subTest(workflow=workflow):
                self.assertIn(view, text)
                self.assertIn(f'> "$EVIDENCE_DIR/{evidence}"', text)
                self.assertLess(text.index(watch), text.index(view))
                view_index = text.index(view)
                final_record = text[view_index : text.index(next_marker, view_index)]
                for phrase in (
                    ".status == \"completed\"",
                    ".conclusion == \"success\"",
                    ".headBranch == $tag",
                    ".headSha == $sha",
                    ".workflowName == $workflow",
                    ".event == \"push\"",
                    f'--arg workflow "{workflow}"',
                    f'"$EVIDENCE_DIR/{evidence}"',
                ):
                    self.assertIn(phrase, final_record)

    def test_release_finalization_captures_created_pr(self):
        text = canonical("oxidex-release-finalization", "references/gates.md")
        for phrase in (
            "PR_URL=$(gh pr create",
            'printf \'%s\\n\' "$PR_URL"',
            'PR=$(gh pr view "$PR_URL"',
            'gh pr view "$PR"',
            'gh pr checks "$PR"',
        ):
            self.assertIn(phrase, text)

    def test_release_finalization_uses_durable_evidence_paths(self):
        gates = canonical("oxidex-release-finalization", "references/gates.md")
        github = canonical(
            "oxidex-release-finalization", "references/github-release-and-macos.md"
        )
        entrypoint = canonical("oxidex-release-finalization", "SKILL.md")
        self.assertIn("EVIDENCE_ROOT=/absolute/durable/evidence/root", gates)
        self.assertIn("outside tracked repository content", entrypoint)
        self.assertNotIn("/tmp/oxidex-release", gates + github)
        self.assertNotRegex(gates + github, r"(?m)^\s*rm\s+-[^\n]*r")

    def test_release_finalization_verifies_ssh_signed_tag(self):
        text = canonical("oxidex-release-finalization", "references/gates.md")
        for phrase in (
            "gpg.ssh.allowedSignersFile",
            "user.signingkey",
            'tag -v "$TAG"',
            '"${VERIFY[@]}"',
        ):
            self.assertIn(phrase, text)

    def test_check_mode_reports_drift(self):
        with tempfile.TemporaryDirectory() as tmp:
            repo = make_fixture(pathlib.Path(tmp), canonical="alpha", mirror="beta")
            self.assertNotEqual(sync.main(["--repo", str(repo), "--check"]), 0)

    def test_missing_canonical_skill_is_drift(self):
        with tempfile.TemporaryDirectory() as tmp:
            repo = make_fixture(pathlib.Path(tmp), canonical="alpha", mirror="alpha")
            shutil.rmtree(repo / ".claude/skills/alpha")
            differences = sync.compare_skill_mirror(repo)
            self.assertTrue(any("canonical skill" in difference for difference in differences))
            self.assertNotEqual(sync.main(["--repo", str(repo), "--check"]), 0)

    def test_write_replaces_only_allowlisted_skill(self):
        with tempfile.TemporaryDirectory() as tmp:
            repo = make_fixture(pathlib.Path(tmp), canonical="alpha", mirror="stale")
            mirror_skill = repo / ".agents/skills/alpha"
            (mirror_skill / "stale.txt").write_text("stale", encoding="utf-8")
            unlisted = repo / ".agents/skills/unlisted"
            unlisted.mkdir()
            sentinel = unlisted / "SENTINEL"
            sentinel.write_text("keep", encoding="utf-8")

            sync.write_skill_mirror(repo)

            self.assertTrue((repo / ".agents").is_dir())
            self.assertTrue((repo / ".agents/skills").is_dir())
            self.assertTrue(sentinel.is_file())
            self.assertEqual(sync.compare_skill_mirror(repo), [])
            self.assertFalse((mirror_skill / "stale.txt").exists())


if __name__ == "__main__":
    unittest.main()
