//! Recursive directory processing for batch operations
//!
//! This module handles batch processing of multiple files and directories.
//! It provides parallel processing capabilities using rayon for efficient
//! metadata operations on large file collections.

use crate::cli::args::CliArgs;
use crate::cli::non_utf8::{PathLine, json_path, os_bytes};
use crate::cli::output_formatter::{
    CsvFormatter, HumanReadableFormatter, JsonFormatter, JsonNode, OutputFormatter, ShortFormatter,
};
use crate::cli::tag_resolution::{ResolvedFileOutput, resolve_file_output};
use crate::cli::write_transaction::{WriteOutcome, partition_defined, write_file};
use crate::core::MetadataMap;
use crate::core::operations::read_metadata_report_with_detector_and_options;
use crate::core::read_report::{ParseStatus, ReadReport};
use crate::error::{ExifToolError, Result};
use indicatif::{ProgressBar, ProgressStyle};
use rayon::prelude::*;
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use walkdir::WalkDir;

/// Statistics collected during batch processing
#[derive(Debug, Clone)]
pub struct BatchStats {
    /// Number of files successfully read
    pub files_read: usize,
    /// Number of files successfully updated (for write operations)
    pub files_updated: usize,
    /// Number of files every write request succeeded on without changing a
    /// byte (ExifTool's `image files unchanged`); never counted as updated.
    pub files_unchanged: usize,
    /// Whether these are write statistics, which ExifTool summarizes
    /// differently from a read (see [`BatchStats::print`]).
    pub write_mode: bool,
    /// Number of files that encountered errors
    pub errors: usize,
    /// Number of files a directory walk found but never attempted to read,
    /// because their extension is not one `crate::filetype`'s table (see
    /// [`is_supported_file`]) recognizes at all.
    ///
    /// This is the count that used to not exist: a directory walk gated on
    /// a hand-maintained extension allow-list dropped every file with an
    /// extension missing from that list with no error, no warning and no
    /// number anywhere in the output -- a run over a directory containing
    /// only such files reported success having read zero of them. Every
    /// file this field counts was positively identified as unreadable (an
    /// extension absent from ExifTool's own `%fileTypeLookup`-derived
    /// table), not merely omitted from a list someone forgot to extend.
    pub unidentified: usize,
    /// Number of directories this run walked: one for each directory named
    /// on the command line, plus (when `-r` is set) one more for every
    /// subdirectory descended into -- exactly `$countDir` in ExifTool's
    /// `ScanDir` (`exiftool:4421`, 13.59), which increments once per call
    /// regardless of whether that directory held any files at all. A plain
    /// file argument never contributes to this count. Printed first in the
    /// summary, before every other line (`exiftool:2067`), whenever it is
    /// non-zero -- including a `0 image files read` line for a directory
    /// scan that matched nothing, which is ExifTool's own fallback
    /// (`exiftool:2076`: `$countDir and not $totWr`) rather than the usual
    /// `image files updated`/`read` count.
    pub directories_scanned: usize,
}

impl BatchStats {
    /// Creates a new BatchStats with zero counts
    fn new() -> Self {
        Self {
            files_read: 0,
            files_updated: 0,
            files_unchanged: 0,
            write_mode: false,
            errors: 0,
            unidentified: 0,
            directories_scanned: 0,
        }
    }

    /// Empty statistics for a write run (see [`BatchStats::print`]).
    pub fn for_write() -> Self {
        Self {
            write_mode: true,
            ..Self::new()
        }
    }

    /// Prints the statistics in ExifTool-compatible format
    pub fn print(&self) {
        // `directories scanned` always leads the summary when this run
        // walked at least one directory (`exiftool`:2067, 13.59), before the
        // read/write counts below.
        if self.directories_scanned > 0 {
            println!("{:5} directories scanned", self.directories_scanned);
        }
        // A write run prints what exiftool:2071-2074 (13.59) prints for one:
        // `updated` whenever a write was attempted (even `0`), `unchanged`
        // and `weren't updated due to errors` when non-zero, and no read
        // count -- `exiftool -XPTitle= a.jpg b.jpg` answers
        // `    0 image files updated` / `    2 image files unchanged`.
        if self.write_mode {
            println!("{:5} image files updated", self.files_updated);
            if self.files_unchanged > 0 {
                println!("{:5} image files unchanged", self.files_unchanged);
            }
            if self.errors > 0 {
                println!("{:5} files weren't updated due to errors", self.errors);
            }
            if self.unidentified > 0 {
                println!(
                    "{:5} files skipped (extension not recognized)",
                    self.unidentified
                );
            }
            return;
        }
        // ExifTool's summary is `printf("%5d image files read\n", ...)`
        // (`exiftool`:2071-2077, 13.59): the count is right-aligned in five
        // columns, so ten or more files print `   12 ...`, not `    12 ...`.
        // A directory scan that matched no files still gets this line at
        // `0` (`exiftool`:2076, `$countDir and not $totWr`) -- e.g. an empty
        // directory, or one holding only unrecognized extensions.
        if self.files_read > 0 || self.directories_scanned > 0 {
            println!("{:5} image files read", self.files_read);
        }
        if self.files_updated > 0 {
            println!("{:5} image files updated", self.files_updated);
        }
        if self.errors > 0 {
            println!("{:5} files could not be read", self.errors);
        }
        if self.unidentified > 0 {
            println!(
                "{:5} files skipped (extension not recognized)",
                self.unidentified
            );
        }
    }
}

/// Main entry point for batch processing operations.
///
/// This function handles both recursive directory traversal and batch file processing.
/// It automatically detects whether to perform read or write operations based on
/// the CLI arguments.
///
/// # Arguments
///
/// * `path` - Root path to start processing (file or directory)
/// * `args` - CLI arguments containing flags and tag modifications
///
/// # Returns
///
/// * `Ok(BatchStats)` - Processing completed with statistics
/// * `Err(ExifToolError)` - Fatal error occurred (e.g., invalid path)
///
/// # Processing Modes
///
/// - **Read mode**: No tag modifications specified - reads and outputs metadata
/// - **Write mode**: Tag modifications present - applies changes to all files
///
/// # Parallelization
///
/// Uses rayon's parallel iterators to process files concurrently across CPU cores.
/// Thread-safe atomic counters track statistics during parallel execution.
///
/// # Error Handling
///
/// Individual file errors are logged to stderr but do not stop batch processing.
/// All errors are counted and reported in the final statistics.
pub fn batch_process(path: &Path, args: &CliArgs) -> Result<BatchStats> {
    batch_process_requests(path, args, &args.plain_tag_modifications())
}

/// [`batch_process`] with the write requests given rather than reparsed from
/// `args`: the CLI passes its `WritePlan`'s sets, the one classification of
/// the command line every write path consumes (undefined names already
/// warned about and dropped, names in their canonical spelling), so a
/// directory write can never apply a different request list than the
/// single-file write of the same command (#957, PRRT_kwDOQNbr5M6mR8dA).
/// Empty `modifications` is a read.
pub fn batch_process_requests(
    path: &Path,
    args: &CliArgs,
    modifications: &[(String, OsString)],
) -> Result<BatchStats> {
    // Validate that the path exists
    if !path.exists() {
        return Err(ExifToolError::from(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("Path does not exist: {}", path.display()),
        )));
    }

    // Collect all files to process
    let (files, unidentified, directories_scanned) = collect_files(path, args.recursive)?;

    if files.is_empty() {
        PathLine::new("Warning: No supported image files found in ")
            .path(path)
            .eprint();
        let mut stats = BatchStats::new();
        stats.unidentified = unidentified;
        stats.directories_scanned = directories_scanned;
        if modifications.is_empty() && (args.json || args.csv) {
            print_structured_output_for_no_files(args, &stats)?;
        }
        return Ok(stats);
    }

    // Determine operation mode
    let is_write_mode = !modifications.is_empty();

    // Validate readonly flag for write operations
    if is_write_mode && args.readonly {
        return Err(ExifToolError::from(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "Cannot modify files in read-only mode. Remove --readonly flag or remove tag modifications.",
        )));
    }

    // Process files based on mode
    let mut stats = if is_write_mode {
        batch_write(files, modifications, args)?
    } else {
        batch_read(files, args)?
    };
    stats.unidentified = unidentified;
    stats.directories_scanned = directories_scanned;
    Ok(stats)
}

/// Expands a command line that mixes explicit file arguments with directory
/// arguments into one file list, the way ExifTool's `ProcessFiles` does
/// (`exiftool`:4258-4260, 13.59): an explicit file is processed as given, with
/// no extension filtering (matching `main.rs::handle_multi_file_processing`'s
/// existing explicit-file semantics), while a directory argument is walked
/// with [`collect_files`] and contributes to the `directories scanned` count.
/// Input order of the top-level paths does not affect the resulting counts
/// (each is independent), but the returned file list preserves it.
///
/// A path that does not exist is neither a directory nor rejected up front
/// (PRRT_kwDOQNbr5M6mTOLD): it falls through to the plain-file branch below
/// exactly as `Path::is_dir` already answers `false` for it, so it becomes
/// one more entry [`batch_read`]/[`batch_write`] will fail on and count as a
/// per-file error -- the same "ordinary multi-file processing" outcome a
/// missing path among several plain files has always had. An early `Err`
/// here would abort the whole command before any real directory in the mix
/// was ever read, which pinned 13.59 does not do: `exiftool realdir
/// missing.jpg` still reads `realdir` and reports the miss as one `files
/// could not be read`.
///
/// # Returns
///
/// `(files, unidentified, directories_scanned)`, ready to feed to
/// [`batch_read`]/[`batch_write`] and to attach to the resulting
/// [`BatchStats`].
pub fn collect_paths(paths: &[PathBuf], recursive: bool) -> Result<(Vec<PathBuf>, usize, usize)> {
    let mut files = Vec::new();
    let mut unidentified = 0usize;
    let mut directories_scanned = 0usize;

    for path in paths {
        if path.is_dir() {
            let (dir_files, dir_unidentified, dir_count) = collect_files(path, recursive)?;
            files.extend(dir_files);
            unidentified += dir_unidentified;
            directories_scanned += dir_count;
        } else {
            // Named explicitly on the command line: processed as given, not
            // filtered by extension (see `handle_multi_file_processing`'s
            // doc comment in `src/main.rs`) -- including one that turns out
            // not to exist at all, left for the per-file read/write attempt
            // to fail and count instead of aborting collection here.
            files.push(path.clone());
        }
    }

    Ok((files, unidentified, directories_scanned))
}

/// Collects all identifiable files from the given path, and counts (without
/// reading) every file it skips.
///
/// # Arguments
///
/// * `path` - Starting path (file or directory)
/// * `recursive` - Whether to recursively traverse subdirectories
///
/// # Returns
///
/// `(files, unidentified, directories_scanned)`: the files to attempt, a
/// count of files this walk declined to queue because [`is_supported_file`]
/// could not recognize their extension, and the number of directories this
/// walk visited (see [`BatchStats::directories_scanned`]). `unidentified` is
/// never dropped -- see [`BatchStats::unidentified`] -- it travels back up
/// through [`batch_process`] into the stats the caller prints, and neither is
/// `directories_scanned`.
///
/// A lone file argument scans zero directories. A directory argument that
/// can be opened scans at least the directory itself -- even when it is
/// empty or holds only unrecognized extensions -- exactly as ExifTool's
/// `ScanDir` increments `$countDir` once per call that reaches the end of
/// the function (`exiftool:4421`, 13.59), including an empty one. One that
/// cannot be opened (permission denied, or a symlink to nowhere) scans zero
/// directories instead, matching `ScanDir`'s own `opendir`-or-`Warn`-and-
/// `return` guard (`exiftool:4340-4342`), which returns before that
/// increment. Without `-r`, only the root is ever eligible: subdirectories
/// are neither descended into nor counted (`exiftool:4358`, `next unless
/// $recurse`). With `-r`, every subdirectory the walk opens adds one more,
/// however deep and whether or not it is empty.
fn collect_files(path: &Path, recursive: bool) -> Result<(Vec<PathBuf>, usize, usize)> {
    let mut files = Vec::new();
    let mut unidentified = 0usize;
    // Every directory this walk actually opens counts once -- see the loop
    // below for what "actually opens" means and why a directory is never
    // pre-counted just because `path.is_dir()`/`WalkDir` classified it as
    // one.
    let mut directories_scanned = 0usize;

    if path.is_file() {
        // Single file - check if supported
        if is_supported_file(path) {
            files.push(path.to_path_buf());
        } else {
            PathLine::new("Warning: File type not supported: ")
                .path(path)
                .eprint();
            unidentified += 1;
        }
    } else if path.is_dir() {
        // Directory - walk and collect files
        let walker = if recursive {
            // `WalkDir`'s `follow_root_links` (on by default, independent of
            // `follow_links(false)` below) already makes it descend into
            // `path` even when `path` is itself a symlink to a directory --
            // pinned 13.59 does too, since `IsDirectory`/`-d` follows a
            // symlink, and `path.is_dir()` above already relied on the same
            // following to route us into this branch. But the root
            // `DirEntry` the walk yields still reports itself with
            // `symlink_metadata`'s type (a symlink, never a directory), so
            // it needs its own carve-out in the loop below to be counted at
            // all (PRRT_kwDOQNbr5M6mTzLO). Nested symlinks are still never
            // followed -- `follow_links` stays `false` for everything past
            // the root, avoiding symlink loops -- so only the explicitly
            // named root is affected.
            WalkDir::new(path).follow_links(false) // Avoid symlink loops
        } else {
            WalkDir::new(path).max_depth(1).follow_links(false)
        };

        // `filter_entry` prunes a directory entry it rejects along with
        // everything under it, before the walk ever descends into it -- see
        // `keep_recursive_entry`'s doc comment for what it keeps and why.
        //
        // `.peekable()` lets a directory entry check the *next* item before
        // deciding whether it counts: pinned 13.59's `ScanDir` only reaches
        // `++$countDir` (`exiftool:4421`) after `opendir` succeeds
        // (`exiftool:4340-4342`), and returns immediately -- printing
        // `Error opening directory ...` and nothing else, no summary line at
        // all if it was the only directory -- when it does not. `WalkDir`
        // reports an unreadable directory the same way: the directory's own
        // `Ok` entry (from a plain `lstat`, which does not require opening
        // it) is followed immediately by an `Err` naming that exact path,
        // with nothing else in between (depth-first: a just-pushed
        // directory's own contents, even an open failure recorded for
        // later, are always visited before any sibling). So a directory is
        // counted only when the very next item is not that same-path `Err`;
        // an empty-but-readable directory's next item is unrelated (a
        // sibling, an ancestor's sibling, or the walk ending) and still
        // counts. Both `keep_recursive_entry_tests` and the oracle-gated
        // `directories_scanned_tests` pin this against a `chmod 000`
        // directory -- oxidex used to print `1 directories scanned` /
        // `0 image files read` for one, where pinned 13.59 prints neither.
        let mut walker = walker
            .into_iter()
            .filter_entry(keep_recursive_entry)
            .peekable();
        while let Some(entry) = walker.next() {
            match entry {
                Ok(entry) => {
                    // An ordinary directory (`is_dir()`), or the walk's own
                    // root when it is itself a symlink (`path.is_dir()`
                    // above already confirmed it resolves to one; its
                    // `DirEntry` reports `symlink_metadata`'s type instead,
                    // per `follow_links(false)`, and only ever at depth 0
                    // since nested symlinks are not followed at all).
                    let is_root_symlink = entry.depth() == 0 && entry.file_type().is_symlink();
                    if entry.file_type().is_dir() || is_root_symlink {
                        // Non-recursive: only the root (depth 0) is ever
                        // "scanned" -- a `max_depth(1)` walk still yields a
                        // child directory as an entry (so it can be
                        // skipped, not read into), but pinned 13.59 never
                        // counts it (`exiftool:4358`, `next unless
                        // $recurse`). Recursive: every directory the walk
                        // opens counts, root included.
                        let in_scope = recursive || entry.depth() == 0;
                        let open_failed = matches!(
                            walker.peek(),
                            Some(Err(e)) if e.path() == Some(entry.path())
                        );
                        if in_scope && !open_failed {
                            directories_scanned += 1;
                        }
                        continue;
                    }
                    if !entry.file_type().is_file() {
                        continue;
                    }
                    if is_supported_file(entry.path()) {
                        files.push(entry.path().to_path_buf());
                    } else {
                        unidentified += 1;
                    }
                }
                Err(e) => {
                    eprintln!("Warning: Error accessing path: {}", e);
                }
            }
        }
    }

    Ok((files, unidentified, directories_scanned))
}

#[cfg(test)]
mod collect_files_open_failure_tests {
    use super::*;

    /// A directory `collect_files` cannot open (permission denied) must not
    /// be counted -- pinned 13.59's `ScanDir` returns before `++$countDir`
    /// on an `opendir` failure (`exiftool:4340-4342, 4421`) and prints
    /// `Error opening directory ...` with no `directories scanned` /
    /// `image files read` summary at all. oxidex used to count it anyway
    /// (`directories_scanned` pre-set to `1` before the walk ever tried to
    /// open it), printing `1 directories scanned` / `0 image files read`
    /// where pinned 13.59 prints neither line (both codex reviews on PR
    /// #965's follow-up round, independently).
    #[cfg(unix)]
    #[test]
    fn a_directory_that_cannot_be_opened_is_not_counted() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let locked = dir.path().join("locked");
        std::fs::create_dir(&locked).unwrap();
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();

        // Root ignores directory permissions entirely, so the property this
        // test checks would be unobservable -- confirm the lockout actually
        // holds before asserting on it, matching this suite's existing
        // root-detection pattern (`tests/library_write_codex_threads.rs`).
        let locked_out = std::fs::read_dir(&locked).is_err();
        if !locked_out {
            std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();
            eprintln!("skipping: this directory is readable despite 0o000 (running as root?)");
            return;
        }

        for recursive in [false, true] {
            let result = collect_files(&locked, recursive);
            let (files, _unidentified, directories_scanned) = result.unwrap();
            assert!(files.is_empty(), "recursive={recursive}");
            assert_eq!(
                directories_scanned, 0,
                "an unopenable directory must not be counted (recursive={recursive})"
            );
        }
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    /// The counterpart to the test above: a directory that opens
    /// successfully but is simply empty must still be counted (pinned
    /// 13.59's `ScanDir` increments `$countDir` unconditionally once
    /// `opendir` succeeds, empty or not) -- this pins that the open-failure
    /// carve-out above did not also swallow the ordinary, successful case.
    #[test]
    fn an_empty_but_openable_directory_is_still_counted() {
        let dir = tempfile::tempdir().unwrap();
        let empty = dir.path().join("empty");
        std::fs::create_dir(&empty).unwrap();

        for recursive in [false, true] {
            let (files, _unidentified, directories_scanned) =
                collect_files(&empty, recursive).unwrap();
            assert!(files.is_empty(), "recursive={recursive}");
            assert_eq!(
                directories_scanned, 1,
                "an empty but openable directory must still be counted (recursive={recursive})"
            );
        }
    }
}

/// Whether a recursive walk should keep (not prune) `entry`.
///
/// `WalkDir::filter_entry` prunes a directory entry it rejects along with
/// everything under it, before the walk ever descends into it -- so a
/// dot-prefixed subdirectory (PRRT_kwDOQNbr5M6mTOLF) is neither counted nor
/// read from, matching ExifTool's own default `-r` (`exiftool:4358-4359`,
/// `next if $file =~ /^\./ and $recurse == 1`): only `-r.` (`$recurse == 2`,
/// which oxidex does not have a separate flag for) would include it. The
/// root itself (depth 0) is exempt, so a hidden directory named explicitly
/// on the command line is still scanned.
///
/// The check reads the name's raw bytes ([`os_bytes`]) rather than going
/// through `to_str()`: a non-UTF-8 name such as `.private\xff` still starts
/// with an ASCII `.` byte, but `to_str()` returns `None` for it, and
/// `None.is_some_and(..)` is `false` -- which used to let a hidden directory
/// with an invalid-UTF-8 name pass through unpruned (PRRT_kwDOQNbr5M6mTzLL).
/// ExifTool's own check (`$file =~ /^\./`) is a byte-string match against
/// whatever `readdir` returned, with no UTF-8 validity requirement, so this
/// matches it for a name Perl's regex would also see as leading with `.`.
fn keep_recursive_entry(entry: &walkdir::DirEntry) -> bool {
    entry.depth() == 0
        || !entry.file_type().is_dir()
        || !os_bytes(entry.file_name()).starts_with(b".")
}

#[cfg(test)]
mod keep_recursive_entry_tests {
    use super::*;

    /// Builds a real temp directory tree so a `WalkDir` traversal (which
    /// needs actual filesystem entries) yields real `DirEntry` values --
    /// exercising `keep_recursive_entry` exactly as `collect_files` calls
    /// it, without depending on `collect_files`'s own file-collection logic.
    fn dir_entries_at_depth_1(root: &Path) -> Vec<walkdir::DirEntry> {
        WalkDir::new(root)
            .follow_links(false)
            .max_depth(1)
            .min_depth(1)
            .into_iter()
            .filter_map(std::result::Result::ok)
            .collect()
    }

    #[test]
    fn root_is_always_kept_even_when_its_own_name_starts_with_a_dot() {
        let dir = tempfile::tempdir().unwrap();
        let hidden_root = dir.path().join(".hidden-root");
        std::fs::create_dir(&hidden_root).unwrap();
        let root_entry = WalkDir::new(&hidden_root)
            .follow_links(false)
            .max_depth(0)
            .into_iter()
            .next()
            .unwrap()
            .unwrap();
        assert_eq!(root_entry.depth(), 0);
        assert!(keep_recursive_entry(&root_entry));
    }

    #[test]
    fn a_plain_ascii_dot_prefixed_subdirectory_is_pruned() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join(".git")).unwrap();
        std::fs::create_dir(dir.path().join("visible")).unwrap();
        let entries = dir_entries_at_depth_1(dir.path());
        let hidden = entries
            .iter()
            .find(|e| e.file_name() == std::ffi::OsStr::new(".git"))
            .unwrap();
        let visible = entries
            .iter()
            .find(|e| e.file_name() == std::ffi::OsStr::new("visible"))
            .unwrap();
        assert!(!keep_recursive_entry(hidden), "`.git` must be pruned");
        assert!(keep_recursive_entry(visible), "`visible` must be kept");
    }

    /// The regression this thread is about: on a filesystem that accepts
    /// arbitrary bytes in a name (Unix), a hidden directory whose name is
    /// not valid UTF-8 -- `.private\xff` -- must still be recognized as
    /// hidden. Building the name from raw bytes and never decoding it to a
    /// `str` is exactly the scenario `to_str()` handled wrong.
    #[cfg(unix)]
    #[test]
    fn a_non_utf8_hidden_subdirectory_is_pruned() {
        use std::os::unix::ffi::OsStrExt;
        let dir = tempfile::tempdir().unwrap();
        let name = std::ffi::OsStr::from_bytes(b".private\xff");
        assert!(
            name.to_str().is_none(),
            "the name must actually be invalid UTF-8 for this test to mean anything"
        );
        let hidden = dir.path().join(name);
        if std::fs::create_dir(&hidden).is_err() {
            // Some filesystems (notably macOS's APFS/HFS+) refuse to create
            // a directory entry with invalid UTF-8 bytes at all -- there is
            // then nothing here for a real `WalkDir` traversal to yield, so
            // this environment cannot exercise the regression and the test
            // is skipped rather than failed. Linux (ext4, tmpfs, and CI's
            // runners) accepts arbitrary bytes and does exercise it.
            eprintln!("skipping: this filesystem does not allow a non-UTF-8 directory name");
            return;
        }
        let entries = dir_entries_at_depth_1(dir.path());
        let entry = entries
            .iter()
            .find(|e| e.file_name() == name)
            .expect("the non-UTF-8 directory must still appear in the listing");
        assert!(
            !keep_recursive_entry(entry),
            "a non-UTF-8 name starting with `.` must be pruned, not kept"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_non_hidden_non_utf8_subdirectory_is_kept() {
        use std::os::unix::ffi::OsStrExt;
        let dir = tempfile::tempdir().unwrap();
        let name = std::ffi::OsStr::from_bytes(b"caf\xe9");
        let visible = dir.path().join(name);
        if std::fs::create_dir(&visible).is_err() {
            eprintln!("skipping: this filesystem does not allow a non-UTF-8 directory name");
            return;
        }
        let entries = dir_entries_at_depth_1(dir.path());
        let entry = entries
            .iter()
            .find(|e| e.file_name() == name)
            .expect("the non-UTF-8 directory must still appear in the listing");
        assert!(
            keep_recursive_entry(entry),
            "a non-UTF-8 name not starting with `.` must still be kept"
        );
    }
}

/// Whether a file's extension is one that identification can recognize at
/// all.
///
/// This is a fast pre-filter, not the identification itself. A file that
/// passes still goes through the exact same magic-number pipeline
/// single-file mode uses --
/// [`read_metadata_report_with_detector_and_options`](crate::core::operations::read_metadata_report_with_detector_and_options)
/// -- which is what actually decides whether the file can be read; a
/// recognized extension whose header disagrees, or whose format has no
/// parser, is still counted correctly afterward (as a read error or, per
/// `add_identity_tags`'s "detected is not parsed" fallback, as a success
/// carrying only identity tags). This function exists only to avoid
/// mmap'ing and probing every stray file (`.DS_Store`, a `.git` object) in
/// a large tree before it can find that out.
///
/// It used to be a hand-maintained ~90-entry list, which is exactly the
/// defect AGENTS.md warns against: it silently fell behind the ~40+
/// extensions OxiDex added real parsers for later (MP3, ZIP, DOCX, TXT,
/// HTML, EPUB, and more all have working parsers in
/// `crate::core::format_dispatch` yet were absent from that list), so
/// `oxidex -r` skipped them with no error, no warning and no count -- a
/// run over a directory of nothing but such files reported success having
/// read zero of them.
///
/// This is backed instead by `crate::filetype`'s extension table, which is
/// generated from ExifTool's own `%fileTypeLookup`/`%fileTypeExt` (see
/// `crate::core::operations::add_identity_tags`'s doc comment) and so
/// cannot drift the same way. ExifTool's own default recursive scan
/// applies the identical filter for the identical reason: `ScanDir`
/// (`exiftool:4370-4378`) skips a file when `GetFileType($file)`
/// (`ExifTool.pm:4214`, keyed on `%fileTypeLookup`) comes back empty,
/// unless `-ext` was given.
///
/// Files this returns `false` for are never queued for a read, but they
/// are never silently dropped either: [`collect_files`] counts every one
/// into `BatchStats::unidentified`, which is printed in the final summary.
pub fn is_supported_file(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| crate::filetype::identify_by_extension(ext).is_some())
}

/// Performs batch read operations on a collection of files.
///
/// Reads metadata from all files in parallel and outputs results.
/// Supports both JSON and human-readable output formats.
///
/// # Arguments
///
/// * `files` - Vector of file paths to process
/// * `args` - CLI arguments containing output format flags
///
/// # Returns
///
/// BatchStats with counts of successful reads and errors
pub fn batch_read(files: Vec<PathBuf>, args: &CliArgs) -> Result<BatchStats> {
    let file_count = files.len();

    // Create progress bar
    let progress = create_progress_bar(file_count, "Reading");

    // Atomic counters for thread-safe statistics
    let success_count = AtomicUsize::new(0);
    let error_count = AtomicUsize::new(0);

    // Step 21: the same `ReadOptions` single-file mode builds
    // (`main.rs::handle_read_operation`), from the same two inputs -- the
    // specific tags requested and `--extended-output` -- built once outside
    // the per-file loop since `args` is shared across every file in the
    // batch. Without this, a batch `-JPEGQualityEstimate` or
    // `--extended-output` request never reached the JPEG parser at all.
    let tag_filter = args.specific_tags();
    let read_options =
        crate::core::ReadOptions::new(tag_filter.as_deref().unwrap_or(&[]), args.extended_output);

    // Process files in parallel. Every file goes through the same
    // report-carrying read single-file mode uses
    // (`main.rs::handle_read_operation`), not the fail-fast
    // `read_metadata_with_detector_and_options`: a malformed JPEG or PNG
    // that reads alone as `ParseStatus::Partial` (filesystem and identity
    // tags plus ExifTool's `Warning: JPEG format error`) must read the same
    // way when another path is on the command line or when a directory walk
    // finds it, rather than turning into an error that drops its metadata
    // (#957, PRRT_kwDOQNbr5M6mTtBR). `--strict` refuses such a read here
    // exactly as it does for one file.
    let results: Vec<(PathBuf, Result<ReadReport>)> = files
        .par_iter()
        .map(|path| {
            let result =
                read_metadata_report_with_detector_and_options(path, args.detector, &read_options)
                    .and_then(|report| refuse_degraded_when_strict(report, args));

            match &result {
                Ok(_) => {
                    success_count.fetch_add(1, Ordering::Relaxed);
                }
                Err(e) => {
                    error_count.fetch_add(1, Ordering::Relaxed);
                    PathLine::new("Error reading ")
                        .path(path)
                        .text(&format!(": {e}"))
                        .eprint();
                }
            }

            progress.inc(1);

            (path.clone(), result)
        })
        .collect();

    progress.finish_and_clear();

    // Output results. Each file's raw read is resolved through
    // `cli::tag_resolution::resolve_file_output` -- the same function
    // `main.rs::handle_read_operation` uses -- so `-a`/`-G*`/
    // `--no-print-conv`/the unfiltered default listing behave identically
    // between batch and single-file mode for the same file (Step 21 closes
    // the Step 20 gap where batch instead fed `args.specific_tags()`
    // straight into each formatter's own exact/suffix `filter_tags`
    // matching, bypassing Step 20's group/priority-aware resolution
    // entirely).
    if args.csv {
        output_csv_results(&results, args)?;
    } else if args.json {
        output_json_results(&results, args)?;
    } else if args.short_level > 0 {
        output_short_results(&results, args);
    } else {
        output_human_readable_results(&results, args);
    }

    Ok(BatchStats {
        files_read: success_count.load(Ordering::Relaxed),
        files_updated: 0,
        files_unchanged: 0,
        write_mode: false,
        errors: error_count.load(Ordering::Relaxed),
        unidentified: 0,
        directories_scanned: 0,
    })
}

/// `--strict`'s refusal of a read that did not fully parse, as
/// `main.rs::handle_read_operation` applies it to one file: anything but
/// [`ParseStatus::Parsed`] or [`ParseStatus::IdentifiedOnly`] is an error,
/// reported with the first diagnostic and the status. Without `--strict`
/// the report passes through and its status travels into formatting.
fn refuse_degraded_when_strict(report: ReadReport, args: &CliArgs) -> Result<ReadReport> {
    if args.strict
        && !matches!(
            report.status,
            ParseStatus::Parsed | ParseStatus::IdentifiedOnly
        )
    {
        let reason = report
            .diagnostics
            .first()
            .map(|d| d.message.clone())
            .unwrap_or_else(|| report.status.to_string());
        return Err(ExifToolError::parse_error(format!(
            "{reason} (status: {})",
            report.status
        )));
    }
    Ok(report)
}

/// Performs batch write operations on a collection of files.
///
/// Applies the same tag modifications to all files in parallel.
///
/// # Arguments
///
/// * `files` - Vector of file paths to process
/// * `modifications` - Tag modifications to apply (tag_name, value pairs)
/// * `args` - CLI arguments containing file preservation flags
///
/// # Returns
///
/// BatchStats with counts of successful updates and errors
pub fn batch_write(
    files: Vec<PathBuf>,
    modifications: &[(String, OsString)],
    args: &CliArgs,
) -> Result<BatchStats> {
    let file_count = files.len();
    // Undefined names were warned about before dispatch; they are not
    // requests (ExifTool drops them in `SetNewValue`).
    let (_, modifications) = partition_defined(modifications);

    // Create progress bar
    let progress = create_progress_bar(file_count, "Writing");

    // Atomic counters for thread-safe statistics
    let updated_count = AtomicUsize::new(0);
    let unchanged_count = AtomicUsize::new(0);
    let error_count = AtomicUsize::new(0);

    // Process files in parallel
    files.par_iter().for_each(|path| {
        match apply_modifications(path, &modifications, args) {
            Ok(WriteOutcome::Updated) => {
                updated_count.fetch_add(1, Ordering::Relaxed);
            }
            Ok(WriteOutcome::Unchanged) => {
                unchanged_count.fetch_add(1, Ordering::Relaxed);
            }
            Err(e) => {
                error_count.fetch_add(1, Ordering::Relaxed);
                PathLine::new("Error writing ")
                    .path(path)
                    .text(&format!(": {e}"))
                    .eprint();
            }
        }

        progress.inc(1);
    });

    progress.finish_and_clear();

    Ok(BatchStats {
        files_read: 0,
        files_updated: updated_count.load(Ordering::Relaxed),
        files_unchanged: unchanged_count.load(Ordering::Relaxed),
        write_mode: true,
        errors: error_count.load(Ordering::Relaxed),
        unidentified: 0,
        directories_scanned: 0,
    })
}

/// Applies tag modifications to a single file.
///
/// The same transaction the single-file path uses
/// (`cli::write_transaction::write_file`): all requests or none, `-TAG=` is a
/// deletion, and a file whose bytes did not change is `Unchanged`, never
/// counted as updated. Handles file preservation options (backup, preserve
/// timestamps) around an actual update only.
fn apply_modifications(
    path: &Path,
    modifications: &[(String, OsString)],
    args: &CliArgs,
) -> std::result::Result<WriteOutcome, String> {
    // Every write target is checked as the single-file write checks it
    // (`main.rs`'s `prepare_write_target`) and as `-all=`/`-TagsFromFile`
    // over a file list does: a read-only file is refused, never replaced.
    // The atomic rename below would replace a 0444 file in a writable
    // directory, so `-Artist=x ro.jpg other.jpg` modified the very file
    // `-Artist=x ro.jpg` refuses.
    let target = fs::metadata(path)
        .map_err(|e| format!("Cannot access file '{}': {}", path.display(), e))?;
    if target.permissions().readonly() {
        return Err(format!("File is read-only: {}", path.display()));
    }
    // Preserve original file times if requested
    let original_metadata = args.preserve_file_times.then_some(target);

    // Create backup if requested, once the write is known to change the file.
    // `photo.jpg` -> `photo.jpg.bak`, `a` -> `a.bak` (the single-file
    // spelling, built on the `OsStr`; `with_extension` made `a..bak`).
    let backup = || {
        if !args.backup {
            return Ok(());
        }
        let mut backup_path = path.as_os_str().to_owned();
        backup_path.push(".bak");
        fs::copy(path, PathBuf::from(backup_path))
            .map(|_| ())
            .map_err(|e| e.to_string())
    };

    let outcome = write_file(path, modifications, !args.exiftool_compat(), backup)?;

    // Restore file times if requested
    if outcome == WriteOutcome::Updated
        && let Some(metadata) = original_metadata
        && let Ok(mtime) = metadata.modified()
    {
        use std::fs::File;
        if let Err(_e) = File::open(path).and_then(|f| f.set_modified(mtime)) {
            // Silently ignore errors - the write succeeded, only mtime restoration failed
            // Errors are expected on some filesystems or when permissions are restricted
        }
    }

    Ok(outcome)
}

/// Creates a progress bar for batch processing.
///
/// # Arguments
///
/// * `total` - Total number of files to process
/// * `action` - Action being performed ("Reading" or "Writing")
fn create_progress_bar(total: usize, action: &str) -> ProgressBar {
    let pb = ProgressBar::new(total as u64);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("[{bar:40.cyan/blue}] {pos}/{len} files ({msg})")
            .unwrap()
            .progress_chars("#>-"),
    );
    pb.set_message(action.to_string());
    pb
}

/// Resolves one file's raw read the same way `main.rs::handle_read_operation`
/// does (`cli::tag_resolution::resolve_file_output`) for a caller that only
/// ever wants the `MetadataMap` form. Both callers of this helper
/// (CSV and JSON output) already gate on `args.csv`/`args.json`, so
/// `resolve_file_output` never takes its `-Gn` + human/short `Lines` branch
/// here (that branch requires `!args.json && !args.csv`) -- the `Lines` arm
/// below is unreachable in practice and returns an empty map rather than
/// panicking, so a future change to that invariant fails loud (empty tags)
/// instead of crashing a batch run.
fn resolved_metadata_for_structured_output(metadata: &MetadataMap, args: &CliArgs) -> MetadataMap {
    match resolve_file_output(metadata, args) {
        ResolvedFileOutput::Metadata(m) => m,
        ResolvedFileOutput::Lines(_) => MetadataMap::new(),
    }
}

/// Emit the normal CSV header or silent JSON for a read matching no files.
/// The pinned oracle sends directory and zero-read counts to stderr here.
/// Reuse the CSV formatter so its fixed columns cannot drift.
pub fn print_structured_output_for_no_files(args: &CliArgs, stats: &BatchStats) -> Result<()> {
    if args.csv {
        output_csv_results(&[], args)?;
    }
    if stats.directories_scanned > 0 {
        eprintln!("{:5} directories scanned", stats.directories_scanned);
        eprintln!("{:5} image files read", stats.files_read);
    }
    Ok(())
}

fn output_csv_results(results: &[(PathBuf, Result<ReadReport>)], args: &CliArgs) -> Result<()> {
    let formatter = CsvFormatter;
    let mut writer = csv::Writer::from_writer(Vec::new());

    writer
        .write_record(["SourceFile", "Tag", "Value"])
        .map_err(|e| ExifToolError::parse_error(format!("CSV formatting failed: {e}")))?;

    for (path, result) in results {
        if let Ok(report) = result {
            let metadata = resolved_metadata_for_structured_output(&report.metadata, args);
            let rendered = formatter.format_with_mode(&metadata, None, !args.exiftool_compat());
            // The path's own bytes, as ExifTool's `-csv` prints them.
            let source_file = os_bytes(path.as_os_str());
            // Parse without implicit header handling and skip the formatter's
            // "Tag,Value" header row explicitly, so a formatter change cannot
            // silently swallow each file's first data row.
            let mut reader = csv::ReaderBuilder::new()
                .has_headers(false)
                .from_reader(rendered.as_bytes());

            for record in reader.records() {
                let record = record
                    .map_err(|e| ExifToolError::parse_error(format!("CSV parsing failed: {e}")))?;
                if record.get(0) == Some("Tag") && record.get(1) == Some("Value") {
                    continue;
                }
                writer
                    .write_record([
                        source_file,
                        record.get(0).unwrap_or_default().as_bytes(),
                        record.get(1).unwrap_or_default().as_bytes(),
                    ])
                    .map_err(|e| {
                        ExifToolError::parse_error(format!("CSV formatting failed: {e}"))
                    })?;
            }
        }
    }

    writer
        .flush()
        .map_err(|e| ExifToolError::parse_error(format!("CSV formatting failed: {e}")))?;
    let bytes = writer
        .into_inner()
        .map_err(|e| ExifToolError::parse_error(format!("CSV formatting failed: {e}")))?;
    // Bytes, not a `String`: a path need not be UTF-8.
    std::io::stdout()
        .lock()
        .write_all(&bytes)
        .map_err(ExifToolError::from)?;

    Ok(())
}

/// Outputs results at one of ExifTool's short levels (`-s`, `-s2`, `-s3`).
///
/// Each file read is introduced by ExifTool's own `======== FILE` line
/// (`exiftool`:2328-2332, printed whenever more than one file is processed),
/// including a file that has none of the requested tags; the caller prints
/// the `%5d image files read` summary after the last one.
fn output_short_results(results: &[(PathBuf, Result<ReadReport>)], args: &CliArgs) {
    let formatter = ShortFormatter;

    for (path, result) in results {
        if let Ok(report) = result {
            PathLine::new("======== ").path(path).print();
            match resolve_file_output(&report.metadata, args) {
                ResolvedFileOutput::Lines(lines) => print!("{}", lines),
                ResolvedFileOutput::Metadata(metadata) => {
                    let output =
                        formatter.format_with_mode(&metadata, None, !args.exiftool_compat());
                    print!("{}", output);
                }
            }
        }
    }
}

/// Outputs results in JSON format.
///
/// Creates a JSON array with one object per file containing:
/// - SourceFile: file path
/// - All metadata tags (for successful reads)
/// - Error message (for failed reads)
fn output_json_results(results: &[(PathBuf, Result<ReadReport>)], args: &CliArgs) -> Result<()> {
    let formatter = JsonFormatter;

    // Build each object as a `JsonNode` tree directly. Rendering to text and
    // parsing it back through `serde_json::Value` (as this once did) would
    // re-render every bare numeric token through f64 -- `2.00` back to `2.0`
    // -- undoing the verbatim spelling `EscapeJSON` preserves.
    let objects: Vec<BTreeMap<String, JsonNode>> = results
        .iter()
        .map(|(path, result)| {
            let mut map = match result {
                Ok(report) => {
                    let metadata = resolved_metadata_for_structured_output(&report.metadata, args);
                    let mut map =
                        formatter.build_json_map(&metadata, None, !args.exiftool_compat());
                    // The `Status` marker single-file `-j` carries for a read
                    // that did not fully parse (`JsonFormatter::
                    // format_with_status_and_mode`); absent for `Parsed`.
                    if report.status != ParseStatus::Parsed {
                        map.insert(
                            "Status".to_string(),
                            JsonNode::String(report.status.as_str().to_string()),
                        );
                    }
                    map
                }
                Err(e) => BTreeMap::from([("Error".to_string(), JsonNode::String(e.to_string()))]),
            };
            map.insert(
                "SourceFile".to_string(),
                // ExifTool's `-j` replaces each malformed byte of a path with
                // `?` (`FixUTF8`), keeping the output valid JSON.
                JsonNode::String(json_path(path)),
            );
            map
        })
        .collect();

    // The object map iterates keys alphabetically, so inserting "SourceFile"
    // cannot make it serialize first the way ExifTool's `-j` does. Render
    // the array by hand so SourceFile leads each object.
    println!("{}", json_array_with_source_file_first(&objects));
    Ok(())
}

/// Renders `objects` as a pretty-printed JSON array, printing each object's
/// `SourceFile` key first (as ExifTool's `-j` does) and every other key in
/// its existing (alphabetical) order after it.
fn json_array_with_source_file_first(objects: &[BTreeMap<String, JsonNode>]) -> String {
    let mut out = String::from("[\n");
    for (i, map) in objects.iter().enumerate() {
        out.push_str("  ");
        let entries = map
            .get_key_value("SourceFile")
            .into_iter()
            .chain(map.iter().filter(|(k, _)| k.as_str() != "SourceFile"))
            .map(|(k, v)| (k.as_str(), v));
        JsonNode::write_object(entries, &mut out, 2);
        if i + 1 < objects.len() {
            out.push(',');
        }
        out.push('\n');
    }
    out.push(']');
    out
}

#[cfg(test)]
mod json_ordering_tests {
    use super::*;
    use serde_json::json;

    fn object(pairs: &[(&str, JsonNode)]) -> BTreeMap<String, JsonNode> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect()
    }

    #[test]
    fn source_file_is_always_first_key() {
        let objects = vec![object(&[
            ("Zebra", JsonNode::String("z".into())),
            ("Apple", JsonNode::String("a".into())),
            ("SourceFile", JsonNode::String("photo.jpg".into())),
        ])];

        let rendered = json_array_with_source_file_first(&objects);
        let source_pos = rendered.find("\"SourceFile\"").unwrap();
        let apple_pos = rendered.find("\"Apple\"").unwrap();
        let zebra_pos = rendered.find("\"Zebra\"").unwrap();
        assert!(source_pos < apple_pos);
        assert!(source_pos < zebra_pos);
    }

    #[test]
    fn reindented_array_value_parses_back_identically() {
        let objects = vec![object(&[
            ("SourceFile", JsonNode::String("photo.jpg".into())),
            (
                "IPTC:Keywords",
                JsonNode::Array(vec![
                    JsonNode::String("ExifTool".into()),
                    JsonNode::String("Test".into()),
                ]),
            ),
        ])];

        let out = json_array_with_source_file_first(&objects);
        let reparsed: Vec<serde_json::Value> = serde_json::from_str(&out).unwrap();
        assert_eq!(reparsed[0]["IPTC:Keywords"], json!(["ExifTool", "Test"]));
        assert_eq!(reparsed[0]["SourceFile"], json!("photo.jpg"));
    }

    /// The batch writer must keep ExifTool's verbatim numeric spelling; it
    /// used to parse the formatter's text back through `serde_json::Value`,
    /// which printed `2.00` as `2.0`.
    #[test]
    fn batch_writer_keeps_numeric_literal_spelling() {
        let objects = vec![object(&[
            ("SourceFile", JsonNode::String("a.ntf".into())),
            ("NITF:NITFVersion", JsonNode::Literal("2.00".into())),
            ("Test:Exp", JsonNode::Literal("1.000e-06".into())),
        ])];
        assert_eq!(
            json_array_with_source_file_first(&objects),
            "[\n  {\n    \"SourceFile\": \"a.ntf\",\n    \"NITF:NITFVersion\": 2.00,\n    \"Test:Exp\": 1.000e-06\n  }\n]"
        );
    }
}

/// Outputs results in human-readable format.
///
/// Prints each file's metadata with a file path header.
fn output_human_readable_results(results: &[(PathBuf, Result<ReadReport>)], args: &CliArgs) {
    let formatter = HumanReadableFormatter;

    for (path, result) in results {
        match result {
            Ok(report) => {
                PathLine::new("File: ").path(path).print();
                match resolve_file_output(&report.metadata, args) {
                    ResolvedFileOutput::Lines(lines) => print!("{}", lines),
                    ResolvedFileOutput::Metadata(metadata) => {
                        let output =
                            formatter.format_with_mode(&metadata, None, !args.exiftool_compat());
                        print!("{}", output);
                    }
                }
            }
            Err(_) => {
                // Error already printed to stderr during processing
            }
        }
    }
}
