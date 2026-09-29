"""Reviewed QuickTime source fixtures for selected-artifact replay tests."""

import json
from contextlib import ExitStack, contextmanager
from pathlib import Path
import shutil
import tempfile
from unittest.mock import patch


HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
REVIEWED_VERSIONS = frozenset({"11.78", "12.64", "13.59"})


def source_document(version):
    if version not in REVIEWED_VERSIONS:
        raise ValueError(f"unreviewed QuickTime source pin: {version}")
    # Historical captures are test inputs; only the current pin belongs to the
    # generated fixtures directory checked by CI's staleness diff.
    directory = "fixtures" if version == "13.59" else "testdata"
    document = json.loads((HERE / directory / f"quicktime_source_{version.replace('.', '_')}.json").read_text())
    if document.get("exiftool_version") != version:
        raise ValueError(f"QuickTime source fixture differs from repository pin: {version}")
    return document


def selected_document():
    pin = (ROOT / ".exiftool-version").read_text().strip()
    return source_document(pin)


@contextmanager
def fixed_source_context(version, *compilers):
    """Compile a fixed semantic fixture under its own matching pin."""
    if version not in REVIEWED_VERSIONS:
        raise ValueError(f"unreviewed QuickTime source pin: {version}")
    with tempfile.TemporaryDirectory() as directory, ExitStack() as stack:
        root = Path(directory)
        (root / ".exiftool-version").write_text(version + "\n")
        for compiler in compilers:
            stack.enter_context(patch.object(compiler, "ROOT", root))
        yield


@contextmanager
def fixed_capture_context(version, selector):
    """Replay a bounded capture and its fixed golden ledger under its own pin."""
    if version not in REVIEWED_VERSIONS:
        raise ValueError(f"unreviewed QuickTime source pin: {version}")
    with tempfile.TemporaryDirectory() as directory:
        root = Path(directory)
        (root / ".exiftool-version").write_text(version + "\n")
        script = root / "tools/exiftool-tables/quicktime_atom_tables.py"
        script.parent.mkdir(parents=True)
        shutil.copyfile(selector.__file__, script)
        shutil.copyfile(selector.ROOT / "tools/exiftool-tables/dump_tables.pl",
                        script.parent / "dump_tables.pl")
        with patch.object(selector, "ROOT", root), patch.object(selector, "__file__", str(script)):
            yield
