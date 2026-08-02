//! The single place that decides *which* ExifTool a parity run grades against.
//!
//! # Why this module exists
//!
//! OxiDex's tables and parsers are transcribed from a specific ExifTool source
//! tree. Every parity harness in this repo then scores OxiDex's output against
//! an `exiftool` it shells out to. When those two are not the same ExifTool,
//! the score measures the gap between two ExifTools as much as it measures
//! OxiDex -- and it does so silently.
//!
//! Two independent ways that went wrong here, both of which this module closes:
//!
//! **Wrong release.** The harnesses defaulted to a bare `"exiftool"` string,
//! which resolves off `PATH` to whatever the package manager last installed
//! (13.55), while the transcriptions came from the pinned tree (13.59). ExifTool
//! 13.59 selects `Canon::ColorData11` with
//! `($count == 3973 or $count == 3778) and $$valPt =~ /^[\0-\x40]/`, where 13.55
//! uses `$$valPt !~ /^\x41\0/`. A Canon R6 Mark III lands on a different
//! sub-table under each, so sixteen correctly-transcribed tags were reported as
//! regressions. The failure is symmetric -- the same skew manufactures phantom
//! *fixes* -- and neither is distinguishable from the real thing afterwards.
//!
//! **Wrong interpreter.** The pinned tree's `exiftool` script begins
//! `#!/usr/bin/env perl`, which on this machine finds Homebrew perl 5.42. That
//! perl has no `Archive::Zip`, so ExifTool cannot look inside a ZIP container:
//! `OOXML.docx` comes back as `FileType: ZIP`, and every container-ish format
//! degrades at once. Crucially **`-ver` still prints 13.59**. A version check
//! alone passes this oracle, which is exactly why [`Oracle`] also carries a
//! capability probe -- version equality is the assertion that missed it.
//!
//! So: one resolution point, and checks that are loud rather than silent. A
//! parity run that cannot say which ExifTool it graded against, under which
//! interpreter, has not earned the right to report a number.
//!
//! # Resolution order
//!
//! 1. `$EXIFTOOL` -- an explicit binary path, for callers who mean it.
//! 2. `$EXIFTOOL_CACHE_DIR/exiftool/exiftool` (cache dir defaults to
//!    [`DEFAULT_CACHE_DIR`]), run under an explicitly chosen perl. This is the
//!    default on purpose.
//! 3. `exiftool` off `PATH` -- last resort, and reported as unverified.
//!
//! Because the pinned form needs an interpreter and an `-I` flag, an oracle is
//! an **argv prefix**, not a path. Build every invocation with
//! [`Oracle::command`] and append your own arguments.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

/// Explicit override naming an `exiftool` binary to grade against.
pub const BINARY_ENV: &str = "EXIFTOOL";

/// Root of the cached ExifTool checkout (the tree holding `exiftool` and `lib/`).
pub const CACHE_DIR_ENV: &str = "EXIFTOOL_CACHE_DIR";

/// Explicit override naming the perl interpreter to run the pinned tree under.
pub const PERL_ENV: &str = "EXIFTOOL_PERL";

/// Set to `1` to proceed despite a version mismatch or a missing capability.
pub const ALLOW_SKEW_ENV: &str = "OXIDEX_ALLOW_EXIFTOOL_SKEW";

/// Where `just compare-exiftool-full` puts the pinned checkout.
pub const DEFAULT_CACHE_DIR: &str = "/tmp/oxidex-exiftool-cache";

/// The ExifTool release this checkout is transcribed from, as declared by the
/// repo itself.
///
/// Baked in at compile time from `.exiftool-version`, deliberately. The
/// alternative -- trusting whatever release happens to be sitting in the cache
/// directory -- means a re-download silently redefines what "correct" is, and
/// the tables would then be graded against a release nobody chose. The repo is
/// the authority on what it was written against; the cache is just a copy that
/// may or may not match.
pub const REPO_PIN: &str = include_str!("../.exiftool-version");

/// [`REPO_PIN`], trimmed.
pub fn repo_pin() -> &'static str {
    REPO_PIN.trim()
}

/// Perl interpreters to try for the pinned tree, best first.
///
/// Ordering is a preference, not the decision: [`choose_perl`] picks the first
/// candidate that actually loads [`REQUIRED_MODULES`], so a machine whose
/// system perl differs still lands on a working interpreter instead of a
/// nominally-correct one. Bare `perl` is last precisely because `#!/usr/bin/env
/// perl` finding a module-less Homebrew perl is the bug this module exists to
/// catch.
const PERL_CANDIDATES: &[&str] = &["/usr/bin/perl5.34", "/usr/bin/perl", "perl"];

/// Perl modules ExifTool needs for formats this project measures.
///
/// `Archive::Zip` gates every OOXML/ZIP-container format. Without it ExifTool
/// reports `FileType: ZIP` for a `.docx` and says so only in a `Warning` that
/// nothing was reading.
const REQUIRED_MODULES: &[&str] = &["Archive::Zip"];

/// How the oracle binary was chosen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// A caller-supplied path, e.g. a `--exiftool` flag.
    Explicit,
    /// `$EXIFTOOL` named it.
    Env,
    /// The pinned source tree under the cache directory.
    PinnedTree,
    /// A bare `exiftool`, resolved by `PATH` lookup.
    Path,
}

impl Source {
    fn describe(self) -> &'static str {
        match self {
            Source::Explicit => "--exiftool",
            Source::Env => "$EXIFTOOL",
            Source::PinnedTree => "pinned source tree",
            Source::Path => "PATH lookup",
        }
    }
}

/// A resolved ExifTool oracle: how to invoke it, what version it is, which
/// interpreter runs it, and what it is missing.
#[derive(Debug, Clone)]
pub struct Oracle {
    /// Argv prefix: program plus any leading arguments. Append your own after.
    pub argv: Vec<String>,
    /// Trimmed output of `-ver`.
    pub version: String,
    /// `$VERSION` of the pinned source tree, when one was found.
    pub pinned_version: Option<String>,
    /// How the binary was chosen.
    pub source: Source,
    /// The perl running it, when it could be determined.
    pub interpreter: Option<String>,
    /// Required modules the interpreter could not load.
    pub missing_modules: Vec<String>,
}

impl Oracle {
    /// A `Command` preloaded with this oracle's argv prefix.
    pub fn command(&self) -> Command {
        let mut cmd = Command::new(&self.argv[0]);
        cmd.args(&self.argv[1..]);
        cmd
    }

    /// The invocation as a single human-readable string, for messages.
    pub fn display(&self) -> String {
        self.argv.join(" ")
    }

    /// True only when the version was confirmed equal to the pin *and* no
    /// required module is missing.
    ///
    /// Both halves matter: the wrong-interpreter oracle reports the right
    /// version, and a right-version oracle that cannot open a ZIP still
    /// produces a wrong number.
    pub fn is_verified(&self) -> bool {
        self.version_matches() && self.missing_modules.is_empty()
    }

    fn version_matches(&self) -> bool {
        matches!(&self.pinned_version, Some(p) if *p == self.version)
    }

    /// One line naming the oracle, fit for a report header. Any report quoting
    /// a parity number should quote this next to it.
    pub fn provenance(&self) -> String {
        let mut s = format!("ExifTool {}", self.version);
        match &self.pinned_version {
            Some(pin) if *pin == self.version => s.push_str(" (pinned"),
            Some(pin) => s.push_str(&format!(" (SKEWED -- pinned tree is {pin}")),
            None => s.push_str(" (UNVERIFIED -- no pinned source tree"),
        }
        if let Some(perl) = &self.interpreter {
            s.push_str(&format!(", perl {perl}"));
        }
        if !self.missing_modules.is_empty() {
            s.push_str(&format!(
                ", DEGRADED -- missing {}",
                self.missing_modules.join(", ")
            ));
        }
        s.push_str(&format!(", via {})", self.source.describe()));
        s
    }

    /// Functional proof that container formats work: a `.docx` must come back
    /// as `DOCX`, not `ZIP`.
    ///
    /// The module probe in [`resolve`] is the cheap always-on check; this is
    /// the end-to-end one, for callers holding a real sample. It asserts the
    /// property that actually broke rather than a proxy for it.
    pub fn check_container_support(&self, docx: &Path) -> Result<(), String> {
        let out = self
            .command()
            .args(["-s", "-s", "-s", "-FileType"])
            .arg(docx)
            .output()
            .map_err(|e| format!("could not run {}: {e}", self.display()))?;
        let got = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if got == "DOCX" {
            Ok(())
        } else {
            Err(format!(
                "{} reports FileType {got:?} for {} -- expected \"DOCX\". The interpreter \
                 is probably missing Archive::Zip, which silently degrades every ZIP-container \
                 format while `-ver` still prints the right release.",
                self.display(),
                docx.display()
            ))
        }
    }
}

/// Root of the cached ExifTool checkout.
pub fn cache_dir() -> PathBuf {
    std::env::var_os(CACHE_DIR_ENV)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(DEFAULT_CACHE_DIR))
}

/// The `exiftool` script inside a cached checkout.
pub fn pinned_binary(cache_dir: &Path) -> PathBuf {
    cache_dir.join("exiftool").join("exiftool")
}

/// The release a cached checkout actually contains.
///
/// `lib/Image/ExifTool.pm`'s `$VERSION` is what that tree really is, so it is
/// read first; the `.exiftool-version` marker the justfile writes is a fallback
/// for a tree whose `lib/` is absent or unreadable. Note this answers "what is
/// in the cache", not "what should we be grading against" -- that is
/// [`repo_pin`], and the two disagreeing is itself a reportable fault.
pub fn tree_version(cache_dir: &Path) -> Option<String> {
    let pm = cache_dir
        .join("exiftool")
        .join("lib")
        .join("Image")
        .join("ExifTool.pm");
    if let Ok(src) = std::fs::read_to_string(&pm)
        && let Some(v) = parse_pm_version(&src)
    {
        return Some(v);
    }

    std::fs::read_to_string(cache_dir.join(".exiftool-version"))
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Pull `13.59` out of a line like `$VERSION = '13.59';`.
fn parse_pm_version(src: &str) -> Option<String> {
    src.lines()
        .find(|line| line.trim_start().starts_with("$VERSION"))
        .and_then(|line| {
            let mut parts = line.splitn(3, ['\'', '"']);
            parts.next()?;
            parts.next()
        })
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_string)
}

/// Can `perl` load every module in `modules`?
fn missing_modules(perl: &str, modules: &[&str]) -> Vec<String> {
    modules
        .iter()
        .filter(|m| {
            !Command::new(perl)
                .arg(format!("-M{m}"))
                .args(["-e", "1"])
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false)
        })
        .map(|m| (*m).to_string())
        .collect()
}

/// Pick the perl to run the pinned tree under: the first candidate that loads
/// every required module, else the first that runs at all.
fn choose_perl() -> Option<String> {
    if let Ok(p) = std::env::var(PERL_ENV)
        && !p.trim().is_empty()
    {
        return Some(p.trim().to_string());
    }

    let mut fallback = None;
    for cand in PERL_CANDIDATES {
        let runs = Command::new(cand)
            .args(["-e", "1"])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        if !runs {
            continue;
        }
        if missing_modules(cand, REQUIRED_MODULES).is_empty() {
            return Some((*cand).to_string());
        }
        fallback.get_or_insert((*cand).to_string());
    }
    fallback
}

/// Read the interpreter out of a script's `#!` line, resolving `env perl`.
fn shebang_interpreter(binary: impl AsRef<Path>) -> Option<String> {
    let first = std::fs::read_to_string(binary.as_ref())
        .ok()?
        .lines()
        .next()?
        .trim()
        .to_string();
    let rest = first.strip_prefix("#!")?.trim();
    let mut parts = rest.split_whitespace();
    let prog = parts.next()?;
    // `#!/usr/bin/env perl` -- the interpreter is the argument, found on PATH.
    if Path::new(prog).file_name().and_then(|f| f.to_str()) == Some("env") {
        return parts.next().map(str::to_string);
    }
    Some(prog.to_string())
}

/// Ask an argv prefix for its version.
fn probe_version(argv: &[String]) -> Result<String, String> {
    let out = Command::new(&argv[0])
        .args(&argv[1..])
        .arg("-ver")
        .output()
        .map_err(|e| format!("could not run `{} -ver`: {e}", argv.join(" ")))?;
    if !out.status.success() {
        return Err(format!("`{} -ver` exited {}", argv.join(" "), out.status));
    }
    let v = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if v.is_empty() {
        return Err(format!("`{} -ver` printed nothing", argv.join(" ")));
    }
    Ok(v)
}

/// Resolve the oracle, refusing to return one that is skewed or degraded.
pub fn resolve() -> Result<Oracle, String> {
    resolve_with_override(None)
}

/// [`resolve`], with an `explicit` path (typically a `--exiftool` flag) taking
/// precedence over `$EXIFTOOL` and the pinned tree.
///
/// An explicit path still gets both checks. Naming a binary says which one to
/// run; it does not establish that running it produces a number anyone should
/// quote.
pub fn resolve_with_override(explicit: Option<&str>) -> Result<Oracle, String> {
    let cache = cache_dir();

    // What we require comes from the repo, not from whatever is in the cache.
    // A cache that disagrees is a fault in its own right: re-downloading it
    // would otherwise silently redefine "correct" without touching a tracked
    // file, and every later run would grade against a release nobody chose.
    let expected = repo_pin().to_string();
    if let Some(actual) = tree_version(&cache)
        && actual != expected
        && !skew_allowed()
    {
        return Err(format!(
            "Cached ExifTool tree is {actual}, but this checkout declares {expected} \
             (.exiftool-version).\n\
             The cache under {} was fetched for a different release than the tables were \
             transcribed from, so grading against it would measure ExifTool-vs-ExifTool.\n\
             Re-fetch the cache at {expected}, or update .exiftool-version and regenerate \
             the transcriptions -- do not simply grade against the newer tree.",
            cache.display(),
        ));
    }
    let pinned = Some(expected);

    let named = explicit
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .map(|p| (p.to_string(), Source::Explicit))
        .or_else(|| match std::env::var(BINARY_ENV) {
            Ok(p) if !p.trim().is_empty() => Some((p.trim().to_string(), Source::Env)),
            _ => None,
        });

    // A named binary is invoked as-is; its own shebang picks the interpreter,
    // so that is what we probe. The pinned tree instead gets an interpreter we
    // choose, because its `#!/usr/bin/env perl` is precisely what cannot be
    // trusted to find a capable one.
    let (argv, source, interpreter) = match named {
        Some((path, source)) => {
            let interp = shebang_interpreter(&path);
            (vec![path], source, interp)
        }
        None => {
            let tree = pinned_binary(&cache);
            if tree.is_file() {
                let perl = choose_perl()
                    .ok_or_else(|| "no usable perl found to run the pinned ExifTool".to_string())?;
                let lib = cache.join("exiftool").join("lib");
                (
                    vec![
                        perl.clone(),
                        format!("-I{}", lib.display()),
                        tree.to_string_lossy().into_owned(),
                    ],
                    Source::PinnedTree,
                    Some(perl),
                )
            } else {
                let interp = which("exiftool").and_then(shebang_interpreter);
                (vec!["exiftool".to_string()], Source::Path, interp)
            }
        }
    };

    let version = probe_version(&argv)?;
    let missing = interpreter
        .as_deref()
        .map(|p| missing_modules(p, REQUIRED_MODULES))
        .unwrap_or_default();

    let oracle = Oracle {
        argv,
        version,
        pinned_version: pinned,
        source,
        interpreter,
        missing_modules: missing,
    };

    if !skew_allowed() {
        if let Some(pin) = &oracle.pinned_version
            && *pin != oracle.version
        {
            return Err(skew_message(&oracle, pin, &cache));
        }
        if !oracle.missing_modules.is_empty() {
            return Err(degraded_message(&oracle));
        }
    }

    Ok(oracle)
}

/// Minimal `which`, to find the PATH `exiftool` so its shebang can be read.
fn which(prog: &str) -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|dir| dir.join(prog))
            .find(|p| p.is_file())
    })
}

fn skew_allowed() -> bool {
    matches!(
        std::env::var(ALLOW_SKEW_ENV).ok().as_deref(),
        Some("1") | Some("true")
    )
}

fn skew_message(oracle: &Oracle, pin: &str, cache: &Path) -> String {
    format!(
        "ExifTool version skew: grading against {oracle_ver} ({invocation}, via {how}), but the \
         transcriptions come from {pin} ({cache}).\n\
         Different releases select different sub-tables for the same bytes, so every number from \
         this run would be part OxiDex and part ExifTool-vs-ExifTool -- including tags that look \
         like regressions but are correct, and tags that look fixed but are not.\n\
         Fix by unsetting ${binary_env} so the pinned tree is used.\n\
         If the skew is deliberate, set {allow}=1 and say which version you graded against.",
        oracle_ver = oracle.version,
        invocation = oracle.display(),
        how = oracle.source.describe(),
        cache = cache.display(),
        binary_env = BINARY_ENV,
        allow = ALLOW_SKEW_ENV,
    )
}

fn degraded_message(oracle: &Oracle) -> String {
    format!(
        "Degraded ExifTool oracle: {invocation} runs under perl {perl}, which cannot load {missing}.\n\
         ExifTool needs Archive::Zip to look inside ZIP containers; without it a .docx reports \
         `FileType: ZIP` and every OOXML-ish format degrades at once. `-ver` still prints \
         {version}, so a version check alone does not catch this.\n\
         Fix by pointing ${perl_env} at a perl that has the module (macOS system perl \
         /usr/bin/perl5.34 does; Homebrew perl does not).\n\
         If you really want the degraded oracle, set {allow}=1 -- and do not quote coverage \
         numbers from that run.",
        invocation = oracle.display(),
        perl = oracle.interpreter.as_deref().unwrap_or("<unknown>"),
        missing = oracle.missing_modules.join(", "),
        version = oracle.version,
        perl_env = PERL_ENV,
        allow = ALLOW_SKEW_ENV,
    )
}

static SHARED: OnceLock<Result<Oracle, String>> = OnceLock::new();

/// The process-wide oracle, resolved once.
///
/// Every call site should go through this rather than naming a binary itself.
/// One resolution per process also means one `-ver` probe and one capability
/// check, so a suite with dozens of ExifTool invocations still states its
/// oracle exactly once.
pub fn shared() -> Result<&'static Oracle, &'static str> {
    match SHARED.get_or_init(resolve) {
        Ok(oracle) => Ok(oracle),
        Err(msg) => Err(msg.as_str()),
    }
}

/// A `Command` for the shared oracle, or `None` when it could not be resolved.
///
/// Availability probes should use this: a skewed or degraded ExifTool is not an
/// oracle, and a parity test that runs against one is worse than a skipped test
/// because it reports a result.
pub fn shared_command() -> Option<Command> {
    shared().ok().map(Oracle::command)
}

/// True when a usable oracle is available to grade against.
///
/// Prints the reason on failure, once: a suite that silently skips every parity
/// test looks identical to one that passes them.
pub fn available() -> bool {
    match shared() {
        Ok(oracle) => {
            if !oracle.is_verified() {
                warn_once(&format!("⚠️  {}", oracle.provenance()));
            }
            true
        }
        Err(msg) => {
            warn_once(&format!("⚠️  ExifTool oracle unavailable: {msg}"));
            false
        }
    }
}

fn warn_once(msg: &str) {
    static WARNED: OnceLock<()> = OnceLock::new();
    if WARNED.set(()).is_ok() {
        eprintln!("{msg}");
    }
}

/// [`resolve`], but prints the failure and exits rather than propagating.
/// Intended for `main()` in the measurement binaries.
pub fn resolve_or_exit() -> Oracle {
    resolve_or_exit_with(None)
}

/// [`resolve_or_exit`], honouring an explicit `--exiftool`-style override.
pub fn resolve_or_exit_with(explicit: Option<&str>) -> Oracle {
    match resolve_with_override(explicit) {
        Ok(oracle) => {
            if !oracle.is_verified() {
                eprintln!("⚠️  {}", oracle.provenance());
                eprintln!("⚠️  This run's numbers are not attributable to a known-good ExifTool.");
            }
            oracle
        }
        Err(e) => {
            eprintln!("❌ {e}");
            std::process::exit(2);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn oracle(version: &str, pin: Option<&str>, missing: &[&str]) -> Oracle {
        Oracle {
            argv: vec!["exiftool".into()],
            version: version.into(),
            pinned_version: pin.map(str::to_string),
            source: Source::Path,
            interpreter: Some("/usr/bin/perl5.34".into()),
            missing_modules: missing.iter().map(|m| (*m).to_string()).collect(),
        }
    }

    #[test]
    fn parses_version_from_pm_source() {
        assert_eq!(
            parse_pm_version("use strict;\n$VERSION = '13.59';\n").as_deref(),
            Some("13.59")
        );
    }

    #[test]
    fn parses_double_quoted_version() {
        assert_eq!(
            parse_pm_version("$VERSION = \"13.30\";").as_deref(),
            Some("13.30")
        );
    }

    #[test]
    fn ignores_unrelated_lines() {
        assert_eq!(parse_pm_version("my $x = 'nope';\n"), None);
    }

    #[test]
    fn unverified_when_no_pin_available() {
        let o = oracle("13.55", None, &[]);
        assert!(!o.is_verified());
        assert!(o.provenance().contains("UNVERIFIED"));
    }

    #[test]
    fn unverified_when_versions_disagree() {
        let o = oracle("13.55", Some("13.59"), &[]);
        assert!(!o.is_verified());
        assert!(o.provenance().contains("SKEWED"));
    }

    /// The regression that a `-ver` check cannot see: right release, wrong perl.
    #[test]
    fn right_version_still_unverified_when_module_missing() {
        let o = oracle("13.59", Some("13.59"), &["Archive::Zip"]);
        assert!(o.version_matches(), "version alone looks fine");
        assert!(!o.is_verified(), "but the oracle is degraded");
        assert!(o.provenance().contains("DEGRADED"));
        assert!(o.provenance().contains("Archive::Zip"));
    }

    #[test]
    fn verified_only_when_version_and_capability_agree() {
        let o = oracle("13.59", Some("13.59"), &[]);
        assert!(o.is_verified());
        assert!(o.provenance().contains("pinned"));
    }

    #[test]
    fn skew_message_names_both_versions() {
        let o = oracle("13.55", Some("13.59"), &[]);
        let m = skew_message(&o, "13.59", Path::new("/tmp/oxidex-exiftool-cache"));
        assert!(m.contains("13.55"), "{m}");
        assert!(m.contains("13.59"), "{m}");
        assert!(m.contains(ALLOW_SKEW_ENV), "{m}");
    }

    #[test]
    fn degraded_message_names_module_and_interpreter() {
        let o = oracle("13.59", Some("13.59"), &["Archive::Zip"]);
        let m = degraded_message(&o);
        assert!(m.contains("Archive::Zip"), "{m}");
        assert!(m.contains("/usr/bin/perl5.34"), "{m}");
        assert!(m.contains(PERL_ENV), "{m}");
    }

    #[test]
    fn command_replays_the_whole_argv_prefix() {
        let o = Oracle {
            argv: vec![
                "/usr/bin/perl5.34".into(),
                "-I/x/lib".into(),
                "/x/exiftool".into(),
            ],
            version: "13.59".into(),
            pinned_version: Some("13.59".into()),
            source: Source::PinnedTree,
            interpreter: Some("/usr/bin/perl5.34".into()),
            missing_modules: vec![],
        };
        let cmd = o.command();
        assert_eq!(cmd.get_program(), "/usr/bin/perl5.34");
        let args: Vec<_> = cmd.get_args().collect();
        assert_eq!(args, ["-I/x/lib", "/x/exiftool"]);
        assert_eq!(o.display(), "/usr/bin/perl5.34 -I/x/lib /x/exiftool");
    }

    #[test]
    fn shebang_resolves_env_indirection() {
        let dir = std::env::temp_dir().join(format!("oracle-shebang-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let direct = dir.join("direct");
        std::fs::write(&direct, "#!/usr/bin/perl5.34\n1;\n").unwrap();
        assert_eq!(
            shebang_interpreter(&direct).as_deref(),
            Some("/usr/bin/perl5.34")
        );
        let via_env = dir.join("via_env");
        std::fs::write(&via_env, "#!/usr/bin/env perl\n1;\n").unwrap();
        assert_eq!(shebang_interpreter(&via_env).as_deref(), Some("perl"));
        std::fs::remove_dir_all(&dir).ok();
    }

    /// End-to-end: whatever this machine resolves must be able to see inside a
    /// ZIP container. Skips when neither the pinned tree nor the corpus is
    /// present, but never passes vacuously when they are.
    #[test]
    fn resolved_oracle_identifies_docx() {
        let Ok(oracle) = resolve() else {
            return;
        };
        let docx = cache_dir().join("combined-samples").join("OOXML.docx");
        if !docx.is_file() {
            return;
        }
        oracle
            .check_container_support(&docx)
            .expect("resolved oracle must identify a .docx as DOCX, not ZIP");
    }
}
