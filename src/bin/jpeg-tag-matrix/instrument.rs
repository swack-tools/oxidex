//! The shared "which oxidex, from what commit, against what ExifTool"
//! header this binary's subcommands print before their first number.
//!
//! Mirrors `scripts/instrument.py` (see that module's doc comment for the
//! full rationale) because this binary cannot import Python. The concrete
//! incident this module exists to make mechanically impossible: `matrix::run`
//! used to resolve its oxidex binary as `$repo/target/release/oxidex` by
//! naive convention (`std::env::current_dir()?.join("target/release/oxidex")`)
//! while `CARGO_TARGET_DIR` pointed elsewhere. That path never existed, every
//! `oxidex -j` subprocess call failed to spawn, and every read attempt
//! reported as unreadable -- `readable 2702 -> 0`, on nine consecutive gate
//! runs, before anyone thought to check which binary actually ran.
//! `resolve_binary` below fails loudly, before a single tag is compared,
//! instead of letting a missing binary masquerade as a total regression.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

pub const DIRTY_OVERRIDE_ENV: &str = "OXIDEX_ALLOW_DIRTY_TREE";

/// The identity of the source tree a measurement is attributable to.
pub struct GitState {
    pub repo_root: PathBuf,
    pub commit: Option<String>,
    pub describe: Option<String>,
    pub dirty: bool,
    pub dirty_files: Vec<String>,
    /// Unix timestamp of HEAD's commit.
    pub head_time: Option<u64>,
}

impl GitState {
    pub fn short(&self) -> String {
        let commit = self.commit.as_deref().unwrap_or("unknown");
        let short_commit = &commit[..commit.len().min(12)];
        let describe = self
            .describe
            .clone()
            .unwrap_or_else(|| short_commit.to_string());
        let state = if self.dirty {
            format!(
                "DIRTY ({} file{})",
                self.dirty_files.len(),
                if self.dirty_files.len() != 1 { "s" } else { "" }
            )
        } else {
            "clean".to_string()
        };
        format!("{describe} ({short_commit}, {state})")
    }
}

/// Run a git subcommand, returning stdout with only the trailing newline
/// removed -- not `.trim()`. `git status --porcelain` prefixes every line
/// with a fixed-width status column starting with a literal space for an
/// unstaged modification (`" M path"`); a full trim eats that leading space
/// off the FIRST line only (later lines are untouched, since trimming only
/// affects the ends of the whole string), and `git_state`'s fixed-offset
/// slice then truncates that one file's name by a character. Caught by
/// reading this module's own output rather than trusting the slice logic.
fn git(repo: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout)
        .trim_end_matches('\n')
        .to_string();
    if s.is_empty() { None } else { Some(s) }
}

/// The current identity of `repo`.
pub fn git_state(repo: &Path) -> GitState {
    let commit = git(repo, &["rev-parse", "HEAD"]);
    let describe = git(
        repo,
        &["describe", "--always", "--tags", "--long", "--dirty"],
    );
    let status = git(repo, &["status", "--porcelain"]).unwrap_or_default();
    let dirty_files: Vec<String> = status
        .lines()
        .filter(|l| !l.is_empty())
        .map(|l| l.get(3..).unwrap_or("").to_string())
        .collect();
    let head_time = git(repo, &["log", "-1", "--format=%ct"]).and_then(|s| s.parse().ok());
    GitState {
        repo_root: repo.to_path_buf(),
        commit,
        describe,
        dirty: !dirty_files.is_empty(),
        dirty_files,
        head_time,
    }
}

/// Refuse (process::exit) to measure against a dirty tree unless overridden
/// via `$OXIDEX_ALLOW_DIRTY_TREE=1`. Returns whether the caller overrode it,
/// so the header can record that this run's numbers are not attributable to
/// a clean commit.
pub fn refuse_if_dirty(git: &GitState, tool: &str) -> bool {
    if !git.dirty {
        return false;
    }
    if std::env::var(DIRTY_OVERRIDE_ENV)
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
    {
        return true;
    }
    let shown: Vec<&str> = git.dirty_files.iter().take(8).map(|s| s.as_str()).collect();
    let more = if git.dirty_files.len() > 8 {
        format!(", +{} more", git.dirty_files.len() - 8)
    } else {
        String::new()
    };
    eprintln!(
        "\u{274c} {tool}: refusing to measure against a dirty working tree \
         ({} modified file(s) in {}): {}{}\n   A number measured against an \
         uncommitted, unreproducible tree state cannot be attributed to any \
         commit -- see AGENTS.md 'Name the instrument'.\n   Commit or stash \
         first, or set {DIRTY_OVERRIDE_ENV}=1 to measure anyway (the header \
         will record the override).",
        git.dirty_files.len(),
        git.repo_root.display(),
        shown.join(", "),
        more
    );
    std::process::exit(2);
}

/// A resolved, EXISTING executable -- never a path that merely should exist.
/// See [`resolve_binary`].
pub struct BinaryIdentity {
    pub kind: String,
    pub path: PathBuf,
    pub mtime: Option<SystemTime>,
}

/// Resolve `requested` to an absolute path, failing LOUDLY (exit, not a
/// silently-failing subprocess spawn later) if it is not there.
pub fn resolve_binary(requested: &str, kind: &str) -> BinaryIdentity {
    let p = Path::new(requested);
    if !p.is_file() {
        eprintln!(
            "\u{274c} {kind} binary not found at {}\n   (resolved from {requested:?}). Build it \
             first (`cargo build --release --bin {kind}`), or pass the correct path via \
             ${}.\n   Refusing to proceed: every comparison against a missing binary fails \
             closed and looks exactly like a real regression -- this is the \
             `readable 2702 -> 0` incident AGENTS.md now names explicitly.",
            p.display(),
            kind.to_uppercase(),
        );
        std::process::exit(2);
    }
    let mtime = std::fs::metadata(p).ok().and_then(|m| m.modified().ok());
    BinaryIdentity {
        kind: kind.to_string(),
        path: p.canonicalize().unwrap_or_else(|_| p.to_path_buf()),
        mtime,
    }
}

/// A warning if `binary` looks older than the source it should reflect --
/// the shape of a stale prebuilt binary being graded as though it were
/// current. Not proof (mtimes can lie), but cheap, and it is exactly the
/// check that would have caught an old, dirty-tree binary being scored as
/// today's build.
pub fn staleness_note(binary: &BinaryIdentity, git: &GitState) -> Option<String> {
    let bt = binary.mtime?.duration_since(UNIX_EPOCH).ok()?.as_secs();
    let mut newest = git.head_time;
    for f in &git.dirty_files {
        let Ok(meta) = std::fs::metadata(git.repo_root.join(f)) else {
            continue;
        };
        let Ok(mt) = meta.modified() else {
            continue;
        };
        let Ok(d) = mt.duration_since(UNIX_EPOCH) else {
            continue;
        };
        newest = Some(newest.map_or(d.as_secs(), |n| n.max(d.as_secs())));
    }
    let newest = newest?;
    if bt >= newest {
        return None;
    }
    Some(format!(
        "{} binary at {} (mtime unix:{bt}) predates the newest relevant source change \
         (unix:{newest}). It may not reflect {} -- rebuild before trusting this run.",
        binary.kind,
        binary.path.display(),
        if git.dirty {
            "the dirty working tree"
        } else {
            "HEAD"
        },
    ))
}

/// ExifTool's own identity: not just `-ver`, but a functional capability
/// probe -- AGENTS.md's "a matching -ver is not a working oracle". A perl
/// missing Archive::Zip still prints the right release and still reports
/// `FileType: ZIP` for a `.docx`, so the probe asserts the container decode
/// actually works rather than trusting the version string alone.
pub struct ExiftoolIdentity {
    pub exe: String,
    pub version: Option<String>,
    pub capability: String,
}

pub fn exiftool_identity(exe: &str, cache_dir: &Path) -> ExiftoolIdentity {
    let version = Command::new(exe)
        .arg("-ver")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());

    let docx = cache_dir.join("combined-samples").join("OOXML.docx");
    let capability = if docx.is_file() {
        match Command::new(exe)
            .args(["-s", "-s", "-s", "-FileType"])
            .arg(&docx)
            .output()
        {
            Ok(o) if o.status.success() => {
                let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
                if s == "DOCX" {
                    "OK (DOCX container probe passed)".to_string()
                } else {
                    format!(
                        "DEGRADED -- probe reported FileType {s:?}, expected \"DOCX\" \
                         (likely a perl missing Archive::Zip)"
                    )
                }
            }
            _ => "UNKNOWN -- probe invocation failed".to_string(),
        }
    } else {
        format!("not probed -- no sample at {}", docx.display())
    };

    ExiftoolIdentity {
        exe: exe.to_string(),
        version,
        capability,
    }
}

/// Print (and return, so a caller can persist it for a downstream
/// subcommand -- see `matrix::run` writing `work/instrument.txt` for
/// `report::run` to echo) the standard instrument-identity header.
#[allow(clippy::too_many_arguments)]
pub fn print_header(
    tool: &str,
    git: &GitState,
    binary: Option<&BinaryIdentity>,
    dirty_overridden: bool,
    exiftool: Option<&ExiftoolIdentity>,
    extra: &[String],
) -> String {
    let mut lines = Vec::new();
    lines.push(format!("=== instrument: {tool} ==="));
    if let Some(b) = binary {
        lines.push(format!("oxidex:  {}", b.path.display()));
        if let Some(note) = staleness_note(b, git) {
            lines.push(format!("         \u{26a0}\u{fe0f}  {note}"));
        }
    }
    let mut tree_line = format!("repo:    {}", git.short());
    if dirty_overridden {
        tree_line.push_str("  [OXIDEX_ALLOW_DIRTY_TREE=1: measuring anyway]");
    }
    lines.push(tree_line);
    if git.dirty {
        let shown: Vec<&str> = git.dirty_files.iter().take(8).map(|s| s.as_str()).collect();
        let more = if git.dirty_files.len() > 8 {
            format!(", +{} more", git.dirty_files.len() - 8)
        } else {
            String::new()
        };
        lines.push(format!("         dirty: {}{}", shown.join(", "), more));
    }
    if let Some(e) = exiftool {
        lines.push(format!(
            "exiftool: {} ({})",
            e.version.as_deref().unwrap_or("UNKNOWN"),
            e.exe
        ));
        lines.push(format!("         capability: {}", e.capability));
    }
    for l in extra {
        lines.push(l.clone());
    }
    lines.push(String::new());
    let text = lines.join("\n");
    println!("{text}");
    text
}
