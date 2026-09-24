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
//!
//! It also names the compiler that built the binary under test
//! ([`toolchain_report`]): `rust-toolchain.toml` is read only by rustup's
//! proxies, so a PATH with Homebrew's `rustc` first silently builds with
//! Homebrew's release instead of the pin (every local measurement on
//! 2026-09-23 was built by 1.98.1 while CI used the pinned 1.97.1). The
//! binary's own embedded `/rustc/<commit-hash>/` std paths say which
//! toolchain built it, independent of what PATH resolves today.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::LazyLock;
use std::time::{SystemTime, UNIX_EPOCH};

use regex::bytes::Regex as BytesRegex;

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

// --- which compiler -------------------------------------------------------
// Mirrors `scripts/instrument.py`'s toolchain section; keep the two in step.

/// std's panic locations embed `/rustc/<full commit hash>/library/...` in
/// every Rust executable, stripped or not. The hash is `rustc -vV`'s
/// `commit-hash:`, so it names the toolchain that built the binary.
static EMBEDDED_RUSTC: LazyLock<BytesRegex> =
    LazyLock::new(|| BytesRegex::new(r"/rustc/([0-9a-f]{40})/").expect("static regex"));

/// `[toolchain] channel` from the checkout's own `rust-toolchain.toml` (or a
/// legacy extensionless `rust-toolchain` holding just the channel).
pub fn pinned_rust_channel(repo: &Path) -> Option<String> {
    for name in ["rust-toolchain.toml", "rust-toolchain"] {
        let Ok(text) = std::fs::read_to_string(repo.join(name)) else {
            continue;
        };
        for line in text.lines() {
            let line = line.trim();
            let Some(rest) = line.strip_prefix("channel") else {
                continue;
            };
            let Some(value) = rest.trim_start().strip_prefix('=') else {
                continue;
            };
            let value = value.trim();
            if let Some(inner) = value.strip_prefix('"')
                && let Some(end) = inner.find('"')
            {
                return Some(inner[..end].trim().to_string());
            }
        }
        if name == "rust-toolchain" {
            return text
                .lines()
                .map(str::trim)
                .find(|l| !l.is_empty())
                .map(str::to_string);
        }
        return None;
    }
    None
}

fn is_numeric_release(s: &str, parts: usize) -> bool {
    let split: Vec<&str> = s.split('.').collect();
    split.len() == parts
        && split
            .iter()
            .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
}

/// Does `release` satisfy `channel`? `Some(bool)` for a numeric pin (exact
/// `1.97.1`, or any `1.97.x` for `1.97`); `None` for a symbolic channel.
pub fn channel_matches(channel: &str, release: Option<&str>) -> Option<bool> {
    let Some(release) = release else {
        return Some(false);
    };
    if is_numeric_release(channel, 3) {
        Some(release == channel)
    } else if is_numeric_release(channel, 2) {
        Some(release.starts_with(&format!("{channel}.")))
    } else {
        None
    }
}

/// One resolved rustc and what `-vV` says about it.
#[derive(Clone, Debug, Default)]
pub struct RustcIdentity {
    pub command: String,
    pub path: Option<String>,
    pub version: String,
    pub release: Option<String>,
    pub commit_hash: Option<String>,
}

impl RustcIdentity {
    fn describe(&self) -> String {
        format!(
            "{} at {}",
            if self.version.is_empty() {
                "rustc ?"
            } else {
                &self.version
            },
            self.path.as_deref().unwrap_or(&self.command)
        )
    }
}

/// Parse `rustc -vV` output into an identity for `command`.
pub fn parse_rustc_verbose(command: &str, text: &str) -> RustcIdentity {
    let mut ident = RustcIdentity {
        command: command.to_string(),
        ..Default::default()
    };
    let mut lines = text.trim().lines();
    ident.version = lines.next().unwrap_or("").trim().to_string();
    for line in lines {
        if let Some((key, value)) = line.split_once(':') {
            match key.trim() {
                "release" => ident.release = Some(value.trim().to_string()),
                "commit-hash" => ident.commit_hash = Some(value.trim().to_string()),
                _ => {}
            }
        }
    }
    ident
}

fn run_stdout(cmd: &mut Command) -> Option<String> {
    let out = cmd.output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).to_string())
}

fn which(command: &str) -> Option<PathBuf> {
    let candidate = Path::new(command);
    if candidate.components().count() > 1 {
        return candidate.is_file().then(|| candidate.to_path_buf());
    }
    std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|dir| dir.join(command))
        .find(|p| p.is_file())
}

/// What `rustc` means in `repo` now -- cargo's own resolution (`$RUSTC`,
/// else `rustc` on PATH), run in the checkout so a rustup proxy honours its
/// toolchain file.
pub fn rustc_identity(repo: &Path) -> Option<RustcIdentity> {
    let command = std::env::var("RUSTC").unwrap_or_else(|_| "rustc".to_string());
    let text = run_stdout(Command::new(&command).arg("-vV").current_dir(repo))?;
    let mut ident = parse_rustc_verbose(&command, &text);
    ident.path = which(&command).map(|p| p.display().to_string());
    Some(ident)
}

fn rustup_executable() -> Option<PathBuf> {
    which("rustup").or_else(|| {
        let home = std::env::var_os("CARGO_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cargo")))?;
        let candidate = home.join("bin").join("rustup");
        candidate.is_file().then_some(candidate)
    })
}

/// The pinned toolchain's own rustc, asked of rustup -- never of PATH.
/// First `rustup which --toolchain <channel> rustc` (run that executable
/// directly), then `rustup run <channel> rustc -vV`. `None` when rustup or
/// the toolchain is absent, or when what rustup returns does not report the
/// channel's release. `RUSTUP_AUTO_INSTALL=0`: never download to identify.
pub fn pinned_rustc_identity(channel: &str, repo: &Path) -> Option<RustcIdentity> {
    let rustup = rustup_executable()?;
    let rustup_cmd = |args: &[&str]| {
        let mut cmd = Command::new(&rustup);
        cmd.args(args)
            .current_dir(repo)
            .env("RUSTUP_AUTO_INSTALL", "0")
            .env_remove("RUSTUP_TOOLCHAIN");
        run_stdout(&mut cmd)
    };
    let path = rustup_cmd(&["which", "--toolchain", channel, "rustc"])
        .map(|s| s.trim().to_string())
        .filter(|p| Path::new(p).is_file());
    let direct = path.as_ref().and_then(|p| {
        run_stdout(
            Command::new(p)
                .arg("-vV")
                .current_dir(repo)
                .env("RUSTUP_AUTO_INSTALL", "0"),
        )
        .map(|text| parse_rustc_verbose(p, &text))
    });
    let mut ident = match direct {
        Some(ident) => ident,
        None => parse_rustc_verbose(
            &format!("rustup run {channel} rustc"),
            &rustup_cmd(&["run", channel, "rustc", "-vV"])?,
        ),
    };
    if channel_matches(channel, ident.release.as_deref()) == Some(false)
        || ident.commit_hash.is_none()
    {
        return None;
    }
    ident.path = path;
    Some(ident)
}

/// `oxidex` -> `oxidex binary`; a kind that already says "binary" is kept.
pub fn binary_label(kind: &str) -> String {
    if kind == "binary" || kind.ends_with(" binary") {
        kind.to_string()
    } else {
        format!("{kind} binary")
    }
}

/// Every distinct `/rustc/<commit-hash>/` embedded in `bytes`, sorted.
pub fn embedded_rustc_commits_in(bytes: &[u8]) -> Vec<String> {
    let mut found: Vec<String> = EMBEDDED_RUSTC
        .captures_iter(bytes)
        .filter_map(|c| c.get(1))
        .map(|m| String::from_utf8_lossy(m.as_bytes()).to_string())
        .collect();
    found.sort();
    found.dedup();
    found
}

/// What the header says about compilers.
pub struct ToolchainReport {
    pub lines: Vec<String>,
    /// Known to differ from the pin. The header prints `lines`; the verdict
    /// flags are for callers and tests, as in `scripts/instrument.py`.
    #[cfg_attr(not(test), allow(dead_code))]
    pub mismatch: bool,
    /// Could not be confirmed either way.
    #[cfg_attr(not(test), allow(dead_code))]
    pub unverified: bool,
}

const WARN: &str = "         \u{26a0}\u{fe0f}  ";

/// Pure verdict over gathered facts. `binary_commits` is `None` when no
/// binary is measured; then only the current PATH resolution is judged.
pub fn assess_toolchain(
    channel: Option<&str>,
    binary_commits: Option<&[String]>,
    binary_label: &str,
    pinned: Option<&RustcIdentity>,
    current: Option<&RustcIdentity>,
) -> ToolchainReport {
    let Some(channel) = channel else {
        return ToolchainReport {
            lines: vec![
                "rustc:   no rust-toolchain.toml in this checkout -- compiler not checked".into(),
            ],
            mismatch: false,
            unverified: true,
        };
    };
    let mut lines = Vec::new();
    let (mut mismatch, mut unverified) = (false, false);
    // Only the pinned toolchain itself (resolved through rustup) names the
    // pin's commit. A PATH compiler reporting the same release is NOT
    // adopted: it would let an unresolved pin read as a confirmed one.
    let pinned = pinned.filter(|p| channel_matches(channel, p.release.as_deref()) != Some(false));
    let pinned_hash: Option<String> = pinned.and_then(|p| p.commit_hash.clone());
    let known: Vec<&RustcIdentity> = [pinned, current].into_iter().flatten().collect();
    let name = |commit: &str| -> String {
        known
            .iter()
            .find(|i| i.commit_hash.as_deref() == Some(commit))
            .map(|i| {
                format!(
                    "{} ({})",
                    i.version,
                    i.path.as_deref().unwrap_or(&i.command)
                )
            })
            .unwrap_or_else(|| {
                format!(
                    "an unidentified rustc (commit {})",
                    &commit[..commit.len().min(12)]
                )
            })
    };
    let pin_text = format!(
        "pin {channel} (rust-toolchain.toml{})",
        pinned_hash
            .as_deref()
            .map(|h| format!(", commit {}", &h[..h.len().min(12)]))
            .unwrap_or_default()
    );
    if let Some(commits) = binary_commits {
        let names: Vec<String> = commits.iter().map(|c| name(c)).collect();
        if commits.is_empty() {
            lines.push(format!(
                "rustc:   compiler that built the {binary_label} is UNKNOWN; {pin_text}"
            ));
            lines.push(format!(
                "{WARN}no /rustc/<commit> fingerprint in the {binary_label}: cannot confirm it \
                 was built with the pinned toolchain."
            ));
            unverified = true;
        } else if pinned_hash.is_none() {
            lines.push(format!(
                "rustc:   {binary_label} built by {}; {pin_text}",
                names.join(", ")
            ));
            lines.push(format!(
                "{WARN}pinned toolchain {channel} is not resolvable through rustup here \
                 (`rustup which --toolchain {channel} rustc` / `rustup run {channel} rustc -vV` \
                 failed): cannot confirm the binary's compiler. PATH is never taken as the pin."
            ));
            unverified = true;
        } else if commits.len() == 1 && Some(commits[0].as_str()) == pinned_hash.as_deref() {
            lines.push(format!(
                "rustc:   {binary_label} built by the pinned toolchain -- {}",
                names[0]
            ));
        } else {
            mismatch = true;
            lines.push(format!(
                "rustc:   {binary_label} built by {}",
                names.join(", ")
            ));
            lines.push(format!(
                "{WARN}TOOLCHAIN MISMATCH: the {binary_label} was NOT compiled by the {pin_text}. \
                 Its numbers are not comparable with CI's. Rebuild with the pin: put ~/.cargo/bin \
                 ahead of /opt/homebrew/bin on PATH (see AGENTS.md 'Rust toolchain pin')."
            ));
        }
    }
    let lead = if lines.is_empty() {
        "rustc:   "
    } else {
        "         "
    };
    match current {
        None => {
            lines.push(format!("{lead}rustc on PATH now: none resolvable"));
            unverified = true;
        }
        Some(c) => {
            let verdict = channel_matches(channel, c.release.as_deref());
            let suffix = match verdict {
                Some(true) => String::new(),
                Some(false) => format!("  [!= pin {channel}]"),
                None => "  [symbolic pin: unchecked]".to_string(),
            };
            lines.push(format!("{lead}rustc on PATH now: {}{suffix}", c.describe()));
            if verdict == Some(false) && binary_commits.is_none() {
                mismatch = true;
                lines.push(format!(
                    "{WARN}TOOLCHAIN MISMATCH: a build here would use rustc {}, not the {pin_text}.",
                    c.release.as_deref().unwrap_or("?")
                ));
            }
        }
    }
    ToolchainReport {
        lines,
        mismatch,
        unverified,
    }
}

/// Which compiler built `binary` (by its embedded fingerprint), and is it
/// `repo`'s pinned toolchain? Also records what PATH resolves `rustc` to
/// now -- what the NEXT build would use, a different question.
pub fn toolchain_report(repo: &Path, binary: Option<&BinaryIdentity>) -> ToolchainReport {
    let channel = pinned_rust_channel(repo);
    let current = rustc_identity(repo);
    let pinned = channel
        .as_deref()
        .and_then(|c| pinned_rustc_identity(c, repo));
    let commits = binary.map(|b| {
        std::fs::read(&b.path)
            .map(|bytes| embedded_rustc_commits_in(&bytes))
            .unwrap_or_default()
    });
    let label = binary
        .map(|b| binary_label(&b.kind))
        .unwrap_or_else(|| "binary".to_string());
    assess_toolchain(
        channel.as_deref(),
        commits.as_deref(),
        &label,
        pinned.as_ref(),
        current.as_ref(),
    )
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
        // The compiler that built THIS binary (embedded fingerprint), not
        // merely the one PATH resolves now -- see `toolchain_report`.
        lines.extend(toolchain_report(&git.repo_root, Some(b)).lines);
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

#[cfg(test)]
mod toolchain_tests {
    use super::*;

    const PIN: &str = "8bab26f4f68e0e26f0bb7960be334d5b520ea452";
    const BREW: &str = "48a229ceaefd4985c50990b14116b6d856af0985";

    fn ident(release: &str, commit: &str, path: &str) -> RustcIdentity {
        RustcIdentity {
            command: "rustc".into(),
            path: Some(path.into()),
            version: format!("rustc {release} ({})", &commit[..9]),
            release: Some(release.into()),
            commit_hash: Some(commit.into()),
        }
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "oxidex-instrument-{name}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn reads_the_checkouts_own_channel() {
        let dir = scratch("channel");
        assert_eq!(pinned_rust_channel(&dir), None);
        std::fs::write(
            dir.join("rust-toolchain.toml"),
            "[toolchain]\nchannel = \"1.97.1\" # pin\ncomponents = [\"clippy\"]\n",
        )
        .unwrap();
        assert_eq!(pinned_rust_channel(&dir).as_deref(), Some("1.97.1"));
        std::fs::remove_file(dir.join("rust-toolchain.toml")).unwrap();
        std::fs::write(dir.join("rust-toolchain"), "1.80.0\n").unwrap();
        assert_eq!(pinned_rust_channel(&dir).as_deref(), Some("1.80.0"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn channel_matching_is_exact_for_numeric_pins() {
        assert_eq!(channel_matches("1.97.1", Some("1.97.1")), Some(true));
        assert_eq!(channel_matches("1.97.1", Some("1.98.1")), Some(false));
        assert_eq!(channel_matches("1.97", Some("1.97.3")), Some(true));
        assert_eq!(channel_matches("1.97", Some("1.970.0")), Some(false));
        assert_eq!(channel_matches("1.97.1", None), Some(false));
        assert_eq!(channel_matches("stable", Some("1.97.1")), None);
    }

    #[test]
    fn parses_rustc_verbose_output() {
        let i = parse_rustc_verbose(
            "rustc",
            "rustc 1.98.1 (48a229cea 2026-09-01) (Homebrew)\nbinary: rustc\n\
             commit-hash: 48a229ceaefd4985c50990b14116b6d856af0985\nrelease: 1.98.1\n",
        );
        assert_eq!(i.version, "rustc 1.98.1 (48a229cea 2026-09-01) (Homebrew)");
        assert_eq!(i.release.as_deref(), Some("1.98.1"));
        assert_eq!(i.commit_hash.as_deref(), Some(BREW));
    }

    #[test]
    fn fingerprint_names_every_embedded_toolchain() {
        let bytes = format!(
            "\0/rustc/{BREW}/library/core/src/panicking.rs\0/rustc/{BREW}/library/std/x\0\
             /rustc/{PIN}/library/alloc/y\0/rustc/nothex/"
        );
        assert_eq!(
            embedded_rustc_commits_in(bytes.as_bytes()),
            vec![BREW.to_string(), PIN.to_string()]
        );
        assert!(embedded_rustc_commits_in(b"no fingerprint").is_empty());
    }

    #[test]
    fn binary_built_by_the_pin_passes_even_when_path_is_skewed() {
        let pinned = ident("1.97.1", PIN, "/rustup/1.97.1/bin/rustc");
        let current = ident("1.98.1", BREW, "/opt/homebrew/bin/rustc");
        let r = assess_toolchain(
            Some("1.97.1"),
            Some(&[PIN.to_string()]),
            "oxidex binary",
            Some(&pinned),
            Some(&current),
        );
        assert!(!r.mismatch && !r.unverified, "{:?}", r.lines);
        assert!(r.lines[0].contains("built by the pinned toolchain"));
        assert!(r.lines[1].contains("[!= pin 1.97.1]"));
    }

    #[test]
    fn binary_built_by_another_rustc_is_a_loud_mismatch() {
        let pinned = ident("1.97.1", PIN, "/rustup/1.97.1/bin/rustc");
        let current = ident("1.98.1", BREW, "/opt/homebrew/bin/rustc");
        let r = assess_toolchain(
            Some("1.97.1"),
            Some(&[BREW.to_string()]),
            "oxidex binary",
            Some(&pinned),
            Some(&current),
        );
        assert!(r.mismatch);
        assert!(
            r.lines[0].contains("/opt/homebrew/bin/rustc"),
            "{:?}",
            r.lines
        );
        assert!(r.lines[1].contains("TOOLCHAIN MISMATCH"));
        // Two toolchains in one binary is never "the pin".
        let mixed = assess_toolchain(
            Some("1.97.1"),
            Some(&[BREW.to_string(), PIN.to_string()]),
            "oxidex binary",
            Some(&pinned),
            None,
        );
        assert!(mixed.mismatch);
    }

    #[test]
    fn unfingerprinted_or_unresolvable_pin_is_unverified_not_passed() {
        let current = ident("1.98.1", BREW, "/opt/homebrew/bin/rustc");
        let none = assess_toolchain(Some("1.97.1"), Some(&[]), "oxidex binary", None, None);
        assert!(none.unverified && !none.mismatch);
        assert!(none.lines[0].contains("UNKNOWN"));
        let unresolved = assess_toolchain(
            Some("1.97.1"),
            Some(&[BREW.to_string()]),
            "oxidex binary",
            None,
            Some(&current),
        );
        assert!(unresolved.unverified && !unresolved.mismatch);
        // A PATH rustc that merely reports the pinned release is NOT the pin:
        // without the pinned toolchain itself resolved, nothing is confirmed.
        let via_path = assess_toolchain(
            Some("1.97.1"),
            Some(&[PIN.to_string()]),
            "oxidex binary",
            None,
            Some(&ident("1.97.1", PIN, "/usr/local/bin/rustc")),
        );
        assert!(
            via_path.unverified && !via_path.mismatch,
            "{:?}",
            via_path.lines
        );
        assert!(
            !via_path
                .lines
                .join("\n")
                .contains("built by the pinned toolchain")
        );
        // A "pinned" identity whose release is not the channel is not the pin.
        let wrong = assess_toolchain(
            Some("1.97.1"),
            Some(&[BREW.to_string()]),
            "oxidex binary",
            Some(&ident("1.98.1", BREW, "/rustup/toolchains/x/bin/rustc")),
            None,
        );
        assert!(wrong.unverified && !wrong.mismatch, "{:?}", wrong.lines);
    }

    #[test]
    fn binary_labels_never_repeat_binary() {
        assert_eq!(binary_label("oxidex"), "oxidex binary");
        assert_eq!(binary_label("binary"), "binary");
        assert_eq!(binary_label("test binary"), "test binary");
    }

    #[test]
    fn without_a_binary_only_the_path_resolution_is_judged() {
        let skewed = assess_toolchain(
            Some("1.97.1"),
            None,
            "binary",
            None,
            Some(&ident("1.98.1", BREW, "/opt/homebrew/bin/rustc")),
        );
        assert!(skewed.mismatch);
        assert!(skewed.lines[0].starts_with("rustc:   rustc on PATH now"));
        let fine = assess_toolchain(
            Some("1.97.1"),
            None,
            "binary",
            None,
            Some(&ident("1.97.1", PIN, "/home/u/.cargo/bin/rustc")),
        );
        assert!(!fine.mismatch && !fine.unverified);
        let unpinned = assess_toolchain(None, None, "binary", None, None);
        assert!(unpinned.unverified && !unpinned.mismatch);
    }
}
