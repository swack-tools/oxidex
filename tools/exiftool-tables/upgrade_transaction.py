#!/usr/bin/env python3
"""Private old/new builds behind bump-exiftool.sh; checked, recoverable promotion.

Only the manifest outputs and pin may be promoted. Caller source and index are
never used as scratch space. The external journal is durable before each write;
its pointer and advisory lock live in the checkout's Git administration directory.
Recovery refuses unknown concurrent content, including HEAD/index changes. This
is final-state accounting, not a sandbox or a trace of transient producer writes.
"""
from __future__ import annotations

import argparse
import contextlib
import fcntl
import hashlib
import json
import os
from pathlib import Path
import re
import shlex
import shutil
import signal
import stat
import subprocess
import sys
import tempfile
import time

import artifacts

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent.parent
PIN = ".exiftool-version"
EXCLUDED_EXTENSIONS = ("sh", "md", "py", "json")
ACTIVE = {"prepared", "promoting", "recovering", "recovery-blocked"}


class Refused(RuntimeError):
    pass


def digest(path):
    h = hashlib.sha256()
    with Path(path).open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            h.update(block)
    return h.hexdigest()


def durable(path, data, mode=None):
    """Atomic same-directory write, including durable directory metadata."""
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    fd, name = tempfile.mkstemp(prefix=".upgrade-", dir=path.parent)
    try:
        with os.fdopen(fd, "wb") as stream:
            stream.write(data)
            if mode is not None:
                os.fchmod(stream.fileno(), mode)
            stream.flush()
            os.fsync(stream.fileno())
        os.replace(name, path)
        directory = os.open(path.parent, os.O_RDONLY)
        try:
            os.fsync(directory)
        finally:
            os.close(directory)
    finally:
        if os.path.lexists(name):
            os.unlink(name)


def save(path, value):
    durable(path, (json.dumps(value, indent=2, sort_keys=True) + "\n").encode())


def git(root, *args):
    return subprocess.check_output(["git", "-C", str(root), *args],
                                   env={**os.environ, "GIT_OPTIONAL_LOCKS": "0"})


def entry(root):
    result = artifacts.state(root, [])
    index = Path(os.fsdecode(git(root, "rev-parse", "--git-path", "index")).strip())
    if not index.is_absolute():
        index = root / index
    result["index_bytes"] = digest(index) if index.exists() else None
    result["branch"] = git(root, "symbolic-ref", "-q", "HEAD").decode().strip()
    return result


def assert_entry(root, expected):
    if entry(root) != expected:
        raise Refused("caller source, HEAD, branch or index changed; preserving concurrent work")


def ordinary_file(root, rel):
    path = root / rel
    if path.resolve() != path or not path.is_file():
        raise Refused(f"required regular nonsymlink file missing: {path}")
    return path


def file_record(root, rel):
    path = ordinary_file(root, rel)
    return ["file", stat.S_IMODE(path.stat().st_mode), digest(path)]


def install(root, rel, source, record, staging=None):
    """Promote/restore one complete file, including mixed-file handwritten text."""
    path = ordinary_file(root, rel)
    data = source.read_bytes()
    if hashlib.sha256(data).hexdigest() != record[2]:
        raise Refused(f"recovery/promotion payload was changed: {source}")
    # Mode is set before atomic replacement, never a second visible file state.
    if staging is None:
        fd, name = tempfile.mkstemp(prefix=".upgrade-", dir=path.parent)
    else:
        name = str(root / staging)
        fd = os.open(name, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    try:
        with os.fdopen(fd, "wb") as stream:
            stream.write(data)
            os.fchmod(stream.fileno(), record[1])
            stream.flush()
            os.fsync(stream.fileno())
        os.replace(name, path)
        directory = os.open(path.parent, os.O_RDONLY)
        try:
            os.fsync(directory)
        finally:
            os.close(directory)
    finally:
        if os.path.lexists(name):
            os.unlink(name)


def validate_conformance(doc, min_files, min_tags):
    if not isinstance(doc, dict) or not isinstance(doc.get("per_file"), dict) or not isinstance(doc.get("per_format"), dict):
        raise Refused("missing conformance structures")
    rows, totals = doc["per_file"], doc["per_format"]
    if len(rows) < min_files or not totals:
        raise Refused("missing/empty conformance evidence")
    for path, row in rows.items():
        if (not isinstance(path, str) or not isinstance(row, dict) or not isinstance(row.get("format"), str)
                or not isinstance(row.get("missing"), dict) or not isinstance(row.get("extra"), dict)
                or not isinstance(row.get("value_diff"), list)
                or any(not isinstance(v, list) or len(v) != 4 for v in row["value_diff"])):
            raise Refused("malformed conformance per-file record")
    fields = ("files", "matched", "missing", "value_diff", "renames", "extra")
    for row in totals.values():
        if not isinstance(row, dict) or any(type(row.get(k)) is not int or row[k] < 0 for k in fields):
            raise Refused("malformed conformance per-format counters")
    if sum(row["files"] for row in totals.values()) != len(rows):
        raise Refused("conformance file counts disagree")
    # conformance.py enforces --min-tags on raw oracle keys, before its
    # comparison filtering. These counters describe the smaller comparable
    # population; do not silently apply the raw floor a second time to it.
    if sum(row[k] for row in totals.values() for k in ("matched", "missing", "value_diff", "renames")) <= 0:
        raise Refused("conformance has no comparable tags")


def recovery_state(root, journal):
    """Allow only original or prepared bytes on owned paths; all else exact."""
    current = entry(root)
    expected = journal["entry"]
    if {k: v for k, v in current.items() if k != "files"} != {
            k: v for k, v in expected.items() if k != "files"}:
        raise Refused("recovery blocked: caller HEAD, branch or index changed")
    for rel in current["files"].keys() | expected["files"].keys():
        found = current["files"].get(rel)
        original = expected["files"].get(rel)
        owned = {v: k for k, v in journal.get("staging", {}).items()}
        if rel in owned:
            source_rel = owned[rel]
            if found not in (None, expected["files"][source_rel], journal["after"][source_rel]):
                raise Refused(f"recovery blocked: partial or changed staging file {rel}; preserve it externally for review")
            continue
        if found != original and found != journal["after"].get(rel, original):
            raise Refused(f"recovery blocked: unrecognized concurrent content at {rel}")
    # Check all backups before writing any caller file.
    for rel in journal["paths"]:
        ordinary_file(root, rel)
        for side, record in (("before", expected["files"][rel]),
                             ("after", journal["after"][rel])):
            if file_record(Path(journal["run"]) / side / "payload", rel) != record:
                raise Refused(f"recovery blocked: {side} payload changed at {rel}")


def recover(root, journal_path):
    journal_path = journal_path.resolve()
    journal = json.loads(journal_path.read_text())
    expected_paths = [a.path for a in artifacts.select()] + [PIN]
    if (journal.get("schema") != 1 or journal.get("manifest") != artifacts.manifest_digest()
            or journal.get("paths") != expected_paths or set(journal.get("after", {})) != set(expected_paths)
            or journal.get("run") != str(journal_path.parent)
            or journal_path.parent.is_relative_to(root)
            or journal.get("staging") != staging_paths(journal_path.parent, expected_paths)):
        raise Refused("recovery blocked: journal schema, manifest, paths or payload root changed")
    for side in ("before", "after"):
        payload = journal_path.parent / side / "payload"
        if payload.resolve() != payload:
            raise Refused("recovery blocked: payload root uses a symlink")
    if journal.get("root") != str(root) or journal.get("phase") not in ACTIVE:
        raise Refused("no unfinished promotion for this checkout")
    try:
        recovery_state(root, journal)
        journal["phase"] = "recovering"
        save(journal_path, journal)
        for stage in journal["staging"].values():
            recovery_state(root, journal)
            path = root / stage
            if path.exists():
                path.unlink()
        for rel in journal["paths"]:
            # Recheck before every write, including other paths and the index.
            recovery_state(root, journal)
            install(root, rel, Path(journal["run"]) / "before/payload" / rel,
                    journal["entry"]["files"][rel], journal["staging"][rel])
        assert_entry(root, journal["entry"])
        journal["phase"] = "recovered"
        save(journal_path, journal)
    except BaseException as exc:
        journal.update(phase="recovery-blocked", error=str(exc))
        save(journal_path, journal)
        raise


def staging_paths(run, paths):
    # These exact sibling names are journaled before creation. Never clean up
    # a filename glob. Unknown/partial contents stop recovery for review.
    return {rel: str(Path(rel).parent / f".upgrade-{run.name}-{i}") for i, rel in enumerate(paths)}


@contextlib.contextmanager
def checkout_lock(root):
    gitdir = Path(os.fsdecode(git(root, "rev-parse", "--absolute-git-dir")).strip())
    with (gitdir / "oxidex-upgrade.lock").open("a") as lock:
        try:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError as exc:
            raise Refused("another upgrade owns this checkout") from exc
        yield gitdir / "oxidex-upgrade.json"


class Transaction:
    def __init__(self, args, root, pointer):
        self.args, self.root, self.pointer = args, root, pointer
        self.paths = [a.path for a in artifacts.select()] + [PIN]
        if git(root, "status", "--porcelain", "--untracked-files=all").strip():
            raise Refused("caller must be clean (including ordinary untracked files); nothing was changed")
        self.start = entry(root)
        if self.start["branch"] in {"refs/heads/main", "refs/heads/refactor/tag-machinery"}:
            raise Refused("use an owned feature branch, not a protected checkout")
        for rel in self.paths:
            ordinary_file(root, rel)
            git(root, "cat-file", "-e", f"HEAD:{rel}")
        self.pin = (root / PIN).read_text().strip()
        self.old = args.from_version or self.pin
        self.new = args.version
        if not all(re.fullmatch(r"[0-9]+\.[0-9]+", v or "") for v in (self.pin, self.old, self.new)):
            raise Refused("versions must be numeric ExifTool releases")
        if not args.dry_run and (self.old != self.pin or self.new == self.pin):
            raise Refused("live upgrade requires --from matching the current pin and a different target")
        if args.skip_conformance and not args.dry_run:
            raise Refused("--skip-conformance is incomplete evidence and cannot promote; use --dry-run")
        parent = Path(args.report_dir).resolve() if args.report_dir else root.parent / ".oxidex-upgrade-reports"
        if parent == root or parent.is_relative_to(root):
            raise Refused("--report-dir must be outside the caller checkout")
        parent.mkdir(parents=True, exist_ok=True)
        self.run = Path(tempfile.mkdtemp(prefix="bump-", dir=parent))
        self.report = self.run / "transaction.json"
        self.doc = {"schema": 1, "root": str(root), "run": str(self.run), "phase": "starting",
                    "entry": self.start, "paths": self.paths, "manifest": artifacts.manifest_digest(),
                    "old": self.old, "new": self.new, "dry_run": args.dry_run,
                    "baseline": "committed" if self.old == self.pin else "retrospective-all-tiers",
                    "commands": [], "variants": {}, "sources": {}}
        save(self.report, self.doc)
        save(pointer, {"journal": str(self.report)})
        print(f"upgrade evidence: {self.run}", flush=True)
        # Strip source, skew, target and Perl injection settings. Each variant
        # supplies exact identities; unrelated PATH tools remain available.
        self.env = {k: v for k, v in os.environ.items()
                    if not k.startswith(("OXIDEX_", "EXIFTOOL", "PERL", "GIT_"))
                    and k not in {"CARGO_TARGET_DIR", "CARGO_BUILD_TARGET", "CARGO_BUILD_TARGET_DIR",
                                  "RUSTC", "RUSTC_WRAPPER", "RUSTC_WORKSPACE_WRAPPER", "PYTHONOPTIMIZE"}}
        self.env.update(GIT_OPTIONAL_LOCKS="0", PYTHONDONTWRITEBYTECODE="1", CARGO_BUILD_JOBS="2")
        self.corpora = [str(Path(p).resolve()) for p in (args.corpus or [root / "tests/fixtures"])]

    def stage(self, name):
        self.doc["phase"] = name
        save(self.report, self.doc)
        print(f">> {name}", flush=True)

    def command(self, name, argv, cwd=None, env=None, stdout_file=None):
        self.stage(name)
        out = self.run / f"{name}.log"
        item = {"stage": name, "argv": list(map(str, argv)), "cwd": str(cwd or self.root),
                "started": time.time(), "log": str(out)}
        self.doc["commands"].append(item)
        save(self.report, self.doc)
        with contextlib.ExitStack() as streams:
            stream = streams.enter_context(out.open("wb"))
            output = streams.enter_context(Path(stdout_file).open("wb")) if stdout_file else stream
            proc = subprocess.Popen(list(map(str, argv)), cwd=cwd or self.root,
                                    env=env or self.env, stdout=output, stderr=stream,
                                    start_new_session=True)
            try:
                proc.wait()
            finally:
                # Also reap a shell's descendants on interruption. Commands in
                # this transaction have no legitimate detached background work.
                try:
                    os.killpg(proc.pid, signal.SIGTERM)
                except ProcessLookupError:
                    pass
                try:
                    proc.wait(timeout=2)
                except subprocess.TimeoutExpired:
                    pass
                try:
                    os.killpg(proc.pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
                proc.wait()
        item.update(returncode=proc.returncode, finished=time.time())
        save(self.report, self.doc)
        if proc.returncode:
            raise Refused(f"{name} failed ({proc.returncode}); see {out}")
        return out

    def sources(self):
        sys.path.insert(0, str(self.root / "scripts"))
        import exiftool_oracle as eo
        # choose_perl's ambient override is not allowed to bypass --perl.
        with contextlib.ExitStack() as stack:
            old_env = dict(os.environ)
            os.environ.clear()
            os.environ.update(self.env)
            stack.callback(lambda: (os.environ.clear(), os.environ.update(old_env)))
            chosen = self.args.perl or eo.choose_perl()
        executable = shutil.which(chosen or "", path=self.env.get("PATH"))
        if not executable:
            raise Refused("no explicit usable Perl interpreter")
        self.perl = Path(executable).resolve()
        self.env["EXIFTOOL_PERL"] = str(self.perl)
        shim = self.run / "bin"
        shim.mkdir()
        (shim / "perl").symlink_to(self.perl)
        self.env["PATH"] = str(shim) + os.pathsep + self.env.get("PATH", "")
        self.doc["perl"] = {"path": str(self.perl), "sha256": digest(self.perl)}
        self.cargo = shutil.which("cargo", path=self.env.get("PATH"))
        rustc = shutil.which("rustc", path=self.env.get("PATH"))
        if not self.cargo or not rustc:
            raise Refused("Cargo and rustc must resolve explicitly")
        self.doc["toolchain"] = {}
        for name, executable, option in (("cargo", self.cargo, "--version"), ("rustc", rustc, "-vV")):
            log = self.command(f"identity-{name}", [executable, option])
            self.doc["toolchain"][name] = {"path": executable, "resolved": str(Path(executable).resolve()),
                "sha256": digest(executable), "version": log.read_text().strip()}
        self.doc["toolchain"]["flags"] = {k: self.env[k] for k in ("RUSTFLAGS", "CARGO_ENCODED_RUSTFLAGS", "RUSTUP_TOOLCHAIN") if k in self.env}
        cargo_home = Path(self.env.get("CARGO_HOME", str(Path.home() / ".cargo"))).resolve()
        self.doc["toolchain"]["global_config"] = {str(p): digest(p) for p in (cargo_home / "config", cargo_home / "config.toml") if p.is_file()}
        for label, version, supplied in (("old", self.old, self.args.old_exiftool_dir),
                                         ("new", self.new, self.args.new_exiftool_dir)):
            tree = self.run / "sources" / label
            tree.parent.mkdir(exist_ok=True)
            if supplied:
                origin = Path(supplied).resolve()
                if origin == self.root or origin.is_relative_to(self.root):
                    raise Refused("selected ExifTool tree must be outside the caller checkout")
                shutil.copytree(origin, tree, ignore=shutil.ignore_patterns(".git"), symlinks=False)
            else:
                archive = self.run / f"exiftool-{label}.tar.gz"
                self.command(f"fetch-{label}", ["curl", "--fail", "--location", "--silent", "--show-error",
                             "--output", archive, f"https://github.com/exiftool/exiftool/archive/refs/tags/{version}.tar.gz"])
                tree.mkdir()
                self.command(f"extract-{label}", ["tar", "xzf", archive, "--strip-components=1", "-C", tree])
                origin = None
            # Use the existing oracle's version/module and functional checks.
            probe = "\n".join([
                "import sys", "sys.path.insert(0, sys.argv[1])", "import exiftool_oracle as eo",
                "o = eo.resolve_tree(sys.argv[2])", "print(o.provenance())",
                "if o.version != sys.argv[3] or o.missing_modules:",
                "    raise RuntimeError('wrong version or degraded ExifTool oracle')",
                "o.check_container_support(sys.argv[4])",
            ])
            docx = next((p for p in [tree / "t/images/OOXML.docx", *[Path(c) / "OOXML.docx" for c in self.corpora]]
                         if p.is_file()), None)
            if docx is None:
                raise Refused(f"{label}: genuine OOXML.docx required for capability probe")
            self.command(f"probe-{label}", [sys.executable, "-c", probe, self.root / "scripts", tree, version, docx])
            self.doc["sources"][label] = {"tree": str(tree), "origin": str(origin) if origin else None,
                                             "version": version, "files": self.tree_files(tree)}
        save(self.report, self.doc)

    @staticmethod
    def tree_files(tree):
        return {str(p.relative_to(tree)): [stat.S_IMODE(p.stat().st_mode), digest(p)]
                for p in sorted(tree.rglob("*")) if p.is_file()}

    def identities(self):
        if digest(self.perl) != self.doc["perl"]["sha256"]:
            raise Refused("Perl interpreter changed during transaction")
        for name in ("cargo", "rustc"):
            tool = self.doc["toolchain"][name]
            if digest(tool["path"]) != tool["sha256"]:
                raise Refused(f"{name} tool changed during transaction")
        for path, sha in self.doc["toolchain"]["global_config"].items():
            if not Path(path).is_file() or digest(path) != sha:
                raise Refused("global Cargo config changed during transaction")
        for source in self.doc["sources"].values():
            if self.tree_files(Path(source["tree"])) != source["files"]:
                raise Refused("selected ExifTool source changed during transaction")

    def variant(self, label, version, regenerate):
        base = self.run / label
        tree, cache, target = base / "repo", base / "cache", base / "target"
        base.mkdir()
        source = Path(self.doc["sources"]["old" if label == "before" else "new"]["tree"])
        self.command(f"clone-{label}", ["git", "clone", "--no-local", "--no-checkout", self.root, tree])
        self.command(f"checkout-{label}", ["git", "checkout", "--detach", self.start["identity"]["head"]], tree)
        cache.mkdir()
        env = {**self.env, "OXIDEX_ET_CACHE": str(cache), "EXIFTOOL_CACHE_DIR": str(cache),
               "OXIDEX_EXIFTOOL_LIB": str(source / "lib"), "CARGO_TARGET_DIR": str(base / "oracle-target"),
               "OXIDEX_ALLOW_DIRTY_TREE": "1"}
        if regenerate:
            (tree / PIN).write_text(version + "\n")
        saved = artifacts.snapshot(tree, "all", [])
        dump = cache / f"tables-{version}.json"
        # A unique, freshly written dump, even for the unregenerated baseline.
        self.command(f"dump-{label}", [self.perl, tree / "tools/exiftool-tables/dump_tables.pl", source / "lib"], tree, env, stdout_file=dump)
        json.loads(dump.read_text())
        first_dump = digest(dump)
        if regenerate:
            try:
                self.command(f"generate-{label}", ["bash", tree / "tools/exiftool-tables/regen-all.sh"], tree, env)
            finally:
                # Failed producers are checked too, and their scratch is retained.
                artifacts.check(tree, saved)
            if digest(dump) != first_dump:
                raise Refused(f"{label}: generation replaced the bound fresh dump with different data")
            self.command(f"verify-{label}", [sys.executable, tree / "tools/exiftool-tables/verify.py",
                         tree / next(a.path for a in artifacts.select() if a.key == "binary"),
                         source / "lib", "--oracle", tree / "tools/exiftool-tables/oracle.pl"], tree, env)
        artifacts.check(tree, saved)
        source_state = artifacts.state(tree, [])
        self.doc["variants"][label] = {"tree": str(tree), "target": str(target), "version": version,
            "regenerated": regenerate, "source": str(source), "dump": str(dump), "dump_sha256": digest(dump),
            "state": source_state}
        save(self.report, self.doc)
        snapshot = self.run / f"tables-{version}.{label}.snapshot.json"
        shutil.copyfile(dump, snapshot)
        if self.args.skip_conformance:
            return tree, dump, None, env
        if target.exists():
            raise Refused("private target directory already exists before the build")
        env["CARGO_TARGET_DIR"] = str(target)
        log = self.command(f"build-{label}", [self.cargo, "build", "--locked", "--bin", "oxidex", "--jobs", "2",
                          "--target-dir", target, "--message-format=json-render-diagnostics"], tree, env)
        executables = []
        for line in log.read_text(errors="replace").splitlines():
            try:
                item = json.loads(line)
            except ValueError:
                continue
            if item.get("reason") == "compiler-artifact" and item.get("target", {}).get("name") == "oxidex" and item.get("executable"):
                if item.get("fresh"):
                    raise Refused("Cargo reported a cached executable in a new target directory")
                executables.append(Path(item["executable"]))
        if len(executables) != 1:
            raise Refused("Cargo did not report exactly one fresh oxidex executable")
        binary = executables[0].resolve()
        if not binary.is_relative_to(target) or not binary.is_file() or not os.access(binary, os.X_OK):
            raise Refused(f"Cargo executable is outside the explicit target or unusable: {binary}")
        # Build scripts cannot silently mutate the measured source.
        if artifacts.state(tree, []) != source_state:
            raise Refused(f"{label}: build or verification changed the source")
        self.doc["variants"][label].update(binary=str(binary), binary_sha256=digest(binary))
        save(self.report, self.doc)
        return tree, dump, binary, env

    def corpus_state(self):
        # Same resolved caller corpus for both binaries; detect mutations, new
        # files and removals between runs. conformance owns filtering/dedup.
        files = {}
        for root in map(Path, self.corpora):
            if not root.is_dir():
                raise Refused(f"corpus is not a directory: {root}")
            for path in root.rglob("*"):
                if path.is_file():
                    files[str(path)] = {"target": str(path.resolve()), "sha256": digest(path),
                                        "link": os.readlink(path) if path.is_symlink() else None}
        if not files:
            raise Refused("empty corpus")
        return files

    def grade(self, before, after):
        _, old_dump, _, _ = before
        new_tree, new_dump, _, new_env = after
        tools = new_tree / "tools/exiftool-tables"
        stem = f"{self.old}-to-{self.new}"
        triage = self.run / f"triage-{stem}.json"
        self.command("triage", [sys.executable, tools / "triage_bump.py", old_dump, new_dump,
                     "--markdown-out", self.run / f"triage-{stem}.md", "--json-out", triage], new_tree, new_env)
        triage_doc = json.loads(triage.read_text())
        counts = triage_doc.get("counts")
        if (not isinstance(counts, dict) or any(k not in {"AUTO", "EXPR", "COND", "HAND"}
                or type(v) is not int or v < 0 for k, v in counts.items())
                or triage_doc.get("total") != sum(counts.values())
                or not isinstance(triage_doc.get("deltas"), list)
                or len(triage_doc["deltas"]) != triage_doc["total"]
                or triage_doc.get("old_version") != self.old or triage_doc.get("new_version") != self.new):
            raise Refused("triage result missing valid versioned classified counts")
        if self.args.skip_conformance:
            return False
        corpus = self.corpus_state()
        # Match this command's fixed conformance filters and realpath dedup.
        # Alias paths remain in corpus identity above, even when not selected.
        selected = {}
        seen = set()
        for path, identity in sorted(corpus.items()):
            if Path(path).suffix.lstrip(".").lower() in EXCLUDED_EXTENSIONS or identity["target"] in seen:
                continue
            selected[path] = identity
            seen.add(identity["target"])
        min_files = self.args.min_files if self.args.min_files is not None else max(1, len(selected) * 90 // 100)
        reports, documents = [], []
        for label, variant in (("before", before), ("after", after)):
            _, _, binary, _ = variant
            self.identities()
            if self.corpus_state() != corpus:
                raise Refused("corpus changed between measurements")
            if digest(binary) != self.doc["variants"][label]["binary_sha256"]:
                raise Refused("measured binary changed after build")
            output = self.run / f"conformance-{label}-{self.doc['variants'][label]['version']}.json"
            self.command(f"conformance-{label}", [sys.executable, tools / "conformance.py", *self.corpora,
                         "--recursive", "--exclude-ext", ",".join(EXCLUDED_EXTENSIONS), "--exiftool-dir",
                         self.doc["sources"]["new"]["tree"], "--oxidex", binary,
                         "--min-files", min_files, "--min-tags", self.args.min_tags, "--json-out", output], new_tree, new_env)
            doc = json.loads(output.read_text())
            validate_conformance(doc, min_files, self.args.min_tags)
            if not set(doc["per_file"]).issubset(selected):
                raise Refused("conformance reported files outside the selected corpus aliases")
            reports.append(output)
            documents.append(doc)
        if set(documents[0]["per_file"]) != set(documents[1]["per_file"]):
            raise Refused("before/after conformance scored different files")
        if self.corpus_state() != corpus:
            raise Refused("corpus changed during measurements")
        self.doc["corpus"] = {"roots": self.corpora, "files": corpus, "selected": selected,
                              "min_files": min_files, "min_tags": self.args.min_tags}
        gate = self.run / f"gates-{stem}.md"
        self.command("gate", [sys.executable, tools / "bump_conformance_gate.py", *reports, triage,
                     "--report-out", gate], new_tree, new_env)
        if not gate.is_file() or not gate.read_text().strip():
            raise Refused("gate returned success without a report")
        # Every consumer must leave the generated source and binaries intact.
        for label in ("before", "after"):
            variant = self.doc["variants"][label]
            if artifacts.state(Path(variant["tree"]), []) != variant["state"] or digest(variant["binary"]) != variant["binary_sha256"]:
                raise Refused("measurement mutated source/binary identity")
        self.identities()
        return True

    def promote(self, tree):
        assert_entry(self.root, self.start)
        self.identities()
        # Durable payloads on both sides; prepared identity before first write.
        after = {}
        for rel in self.paths:
            for side, source in (("before", self.root), ("after", tree)):
                record = file_record(source, rel)
                dest = self.run / side / "payload" / rel
                durable(dest, ordinary_file(source, rel).read_bytes(), mode=record[1])
                if side == "after":
                    after[rel] = record
        self.doc.update(after=after, staging=staging_paths(self.run, self.paths), phase="prepared")
        save(self.report, self.doc)
        assert_entry(self.root, self.start)
        self.doc["phase"] = "promoting"
        save(self.report, self.doc)
        for rel in self.paths:
            recovery_state(self.root, self.doc)
            install(self.root, rel, self.run / "after/payload" / rel, after[rel], self.doc["staging"][rel])
        expected = {**self.start, "files": {**self.start["files"], **after}}
        assert_entry(self.root, expected)
        # Keep the in-memory phase recoverable until the durable terminal
        # state is written; failure/interruption in this write must roll back.
        finished = {**self.doc, "phase": "promoted"}
        save(self.report, finished)
        self.doc = finished
        print(">> promoted", flush=True)

    def execute(self):
        try:
            self.sources()
            before = self.variant("before", self.old, self.old != self.pin)
            after = self.variant("after", self.new, True)
            complete = self.grade(before, after)
            assert_entry(self.root, self.start)
            if not complete:
                self.stage("incomplete-conformance-skipped")
                return 2
            if self.args.dry_run:
                self.stage("dry-run-passed")
            else:
                self.promote(after[0])
            return 0
        except BaseException as exc:
            phase = self.doc["phase"]
            self.doc.update(error=str(exc), failed_stage=phase)
            must_recover = phase in ACTIVE or (phase == "promoted" and "after" in self.doc)
            if must_recover:
                self.doc["phase"] = "promoting"
            save(self.report, self.doc)
            if must_recover:
                recover(self.root, self.report)
                self.doc = json.loads(self.report.read_text())
            else:
                self.doc["phase"] = "failed"
                save(self.report, self.doc)
            raise


def _main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("version", nargs="?")
    ap.add_argument("--dry-run", action="store_true")
    ap.add_argument("--from", dest="from_version")
    ap.add_argument("--report-dir", help="external parent for a unique durable run directory")
    ap.add_argument("--corpus", action="append")
    ap.add_argument("--min-files", type=int)
    ap.add_argument("--min-tags", type=int, default=1000)
    ap.add_argument("--skip-conformance", action="store_true")
    ap.add_argument("--old-exiftool-dir")
    ap.add_argument("--new-exiftool-dir")
    ap.add_argument("--perl", help="exact Perl executable (otherwise choose by capability)")
    ap.add_argument("--recover", action="store_true", help="checked recovery of this checkout's interrupted promotion")
    args = ap.parse_args(argv)
    if not args.recover and not args.version:
        ap.error("target version required")
    if (args.min_files is not None and args.min_files < 1) or args.min_tags < 1:
        ap.error("conformance floors must be positive")
    root = artifacts.repository(ROOT)
    def interrupted(signum, _frame):
        raise Refused(f"interrupted by signal {signum}")
    for sig in (signal.SIGINT, signal.SIGTERM, signal.SIGHUP):
        signal.signal(sig, interrupted)
    try:
        with checkout_lock(root) as pointer:
            journal_path = Path(json.loads(pointer.read_text())["journal"]) if pointer.exists() else None
            unfinished = journal_path and json.loads(journal_path.read_text()).get("phase") in ACTIVE
            if args.recover:
                if not unfinished:
                    raise Refused("no unfinished promotion")
                recover(root, journal_path)
                print(f"recovered: {journal_path}")
                return 0
            if unfinished:
                raise Refused(f"unfinished promotion: run bump-exiftool.sh --recover; journal {journal_path}")
            return Transaction(args, root, pointer).execute()
    except (Refused, OSError, ValueError, subprocess.SubprocessError) as exc:
        print(f"upgrade refused: {exc}", file=sys.stderr)
        return 1


def main(argv=None):
    # Git redirection/config environment can make a clean alternate index or
    # another repository masquerade as the caller. Sanitize before even root
    # discovery and locking, including --recover; restore in-process callers.
    original = {k: v for k, v in os.environ.items() if k.startswith("GIT_")}
    try:
        for key in original:
            del os.environ[key]
        os.environ["GIT_OPTIONAL_LOCKS"] = "0"
        return _main(argv)
    finally:
        for key in list(os.environ):
            if key.startswith("GIT_"):
                del os.environ[key]
        os.environ.update(original)


if __name__ == "__main__":
    raise SystemExit(main())
