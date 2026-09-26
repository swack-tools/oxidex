//! OxiDex Command Line Interface
//!
//! Main entry point for the oxidex command-line application.

use oxidex::cli::args::CliArgs;
use oxidex::cli::batch_processor;
use oxidex::cli::non_utf8::PathLine;
use oxidex::cli::output_formatter::{
    CsvFormatter, HumanReadableFormatter, JsonFormatter, OutputFormatter, ShortFormatter,
};
use oxidex::cli::rename;
use oxidex::cli::write_transaction::{
    WriteOutcome, WritePlan, is_exiftool_refusal_message, write_plan_file,
};
use oxidex::core::operations::read_metadata_report_with_detector_and_options;
use oxidex::core::read_report::ParseStatus;
use std::process;

fn main() {
    if let Err(error) = oxidex::exiftool_tables::attribution::validate() {
        eprintln!("Error: {error}");
        process::exit(2);
    }

    // Parse command-line arguments after normalizing supported ExifTool-style options.
    let args = match CliArgs::parse() {
        Ok(args) => args,
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    };

    // Extract file path from arguments
    let file = match args.file() {
        Some(path) => path,
        None => {
            eprintln!("Error: No file or directory specified");
            eprintln!("Usage: oxidex [OPTIONS] [-TAG=VALUE ...] FILE|DIRECTORY");
            process::exit(1);
        }
    };

    // Every write request on the line, classified once (see
    // `cli::write_transaction::WritePlan`): combining modes applies all of
    // them or refuses, never dispatches on the first and drops the rest.
    let plan = match WritePlan::from_args(&args) {
        Ok(plan) => plan,
        Err(message) => {
            eprintln!("Error: {}", message);
            process::exit(1);
        }
    };
    let files = args.files();

    if let Some(pattern) = args.filename_pattern() {
        // Rename mode renames one file and writes no tags; anything else on
        // the line would be dropped, so it is refused instead.
        if plan.is_write() {
            eprintln!("Error: Combining -FileName< with tag writes is not supported");
            process::exit(1);
        }
        if files.len() > 1 {
            eprintln!("Error: -FileName< renames one file at a time");
            process::exit(1);
        }
        handle_rename_operation(&file, &pattern, &args);
        return;
    }

    if plan.is_write() {
        // ExifTool judges every `-TAG=VALUE` name before it opens a file
        // (`SetNewValue`, exiftool:1735-1813): an undefined name is a warning
        // and is dropped, and when no request is left it prints `Nothing to
        // do.` and exits 1 without touching anything.
        for warning in &plan.warnings {
            eprintln!("Warning: {}", warning);
        }
        if plan.nothing_to_do() {
            eprintln!("Nothing to do.");
            process::exit(1);
        }
        // `--readonly` refuses every write, whichever mode and however many
        // files (an explicit file list used to bypass it).
        if args.readonly {
            let what = if plan.copy_from.is_some() {
                "copy metadata"
            } else if plan.clear_all {
                "clear metadata"
            } else if !plan.shifts.is_empty() {
                "shift dates"
            } else {
                "modify file"
            };
            eprintln!(
                "Error: Cannot {} in read-only mode (--readonly flag set)",
                what
            );
            process::exit(1);
        }
        if let Some((src, _)) = &plan.copy_from
            && !src.exists()
        {
            PathLine::new("Error: Source file not found: ")
                .path(src)
                .eprint();
            process::exit(1);
        }
        if files.len() == 1 && !files[0].is_dir() {
            handle_single_write(&files[0], &plan, &args);
        } else if plan.sets_only() {
            // The plan's own sets, not the raw arguments re-parsed: every
            // write path applies one classification of the command line.
            if files.len() == 1 {
                handle_batch_processing(&files[0], &args, &plan.sets);
            } else {
                handle_multi_file_processing(&files, &args, &plan.sets);
            }
        } else if files.iter().any(|path| path.is_dir()) {
            eprintln!("Error: -all=, date shifts and -TagsFromFile take files, not directories");
            process::exit(1);
        } else {
            handle_multi_write(&files, &plan, &args);
        }
        return;
    }

    if files.len() > 1 {
        // args.file() only ever returns the *last* positional argument, so a
        // plain-read invocation with more than one path -- `oxidex -j a.jpg
        // b.jpg`, or a mix of files and directories -- used to see only the
        // last one: if it happened to be a directory, every earlier argument
        // was silently dropped (only that directory reached
        // `handle_batch_processing`); otherwise a directory among several
        // *non-trailing* arguments was handed to `handle_multi_file_processing`
        // as if it were a plain file, which fails to read it at all. Route
        // every multi-argument invocation through the same batch machinery,
        // which now expands any directory argument in the mix (see
        // `handle_multi_file_processing`).
        handle_multi_file_processing(&files, &args, &[]);
    } else if file.is_dir() {
        // Batch processing mode (single directory)
        handle_batch_processing(&file, &args, &[]);
    } else {
        // Read mode: display metadata
        handle_read_operation(&file, &args);
    }
}

/// Handles multiple positional arguments (e.g. `oxidex -j a.jpg b.jpg`, or a
/// mix of files and directories on one command line).
///
/// A plain file argument is never filtered by extension -- it was named
/// explicitly on the command line, so it is processed as given, exactly as
/// before this function had to consider directories at all. A directory
/// argument, anywhere in the list, is expanded with
/// `batch_processor::collect_paths` (walked non-recursively unless `-r`),
/// which also contributes to the `directories scanned` count every directory
/// among the arguments adds to the final summary. `modifications` are the
/// write plan's sets (`WritePlan::sets`); empty for a read.
fn handle_multi_file_processing(
    files: &[std::path::PathBuf],
    args: &CliArgs,
    modifications: &[(String, std::ffi::OsString)],
) {
    let has_directory = files.iter().any(|path| path.is_dir());
    let (expanded, unidentified, directories_scanned) = if has_directory {
        match batch_processor::collect_paths(files, args.recursive) {
            Ok(expansion) => expansion,
            Err(e) => {
                eprintln!("Error: Batch processing failed: {}", e);
                process::exit(1);
            }
        }
    } else {
        (files.to_vec(), 0, 0)
    };

    // A mix that expanded to nothing (every directory in it empty or holding
    // only unrecognized extensions, and no plain file argument) still
    // "scanned" those directories -- ExifTool's own fallback for this case
    // (`exiftool`:2076, `$countDir and not $totWr`) is the read-style `0
    // image files read` line, never `image files updated`, matching what
    // `batch_process_requests` already does for a single empty/unsupported
    // directory.
    if expanded.is_empty() {
        let stats = batch_processor::BatchStats {
            files_read: 0,
            files_updated: 0,
            files_unchanged: 0,
            write_mode: false,
            errors: 0,
            unidentified,
            directories_scanned,
        };
        stats.print();
        return;
    }

    let result = if !modifications.is_empty() {
        batch_processor::batch_write(expanded, modifications, args)
    } else {
        batch_processor::batch_read(expanded, args)
    };

    match result {
        Ok(mut stats) => {
            stats.unidentified = unidentified;
            stats.directories_scanned = directories_scanned;
            let is_read_mode = modifications.is_empty();
            // ExifTool prints its read summary after text output at every
            // level, `-s`/`-s3` included; only JSON/CSV keep stdout clean.
            if !(is_read_mode && (args.json || args.csv)) {
                stats.print();
            }

            if stats.errors > 0 {
                process::exit(1);
            }
        }
        Err(e) => {
            eprintln!("Error: Batch processing failed: {}", e);
            process::exit(1);
        }
    }
}

/// Handles every write to one file: `-TAG=VALUE`, `-all=`, date shifts and
/// `-TagsFromFile`, alone or combined, as one transaction
/// (`cli::write_transaction::write_plan_file`). The summary follows what
/// happened to the file: a file that did not change is `0 image files
/// updated` / `1 image files unchanged`, exactly as ExifTool 13.59 prints it.
fn handle_single_write(file: &std::path::Path, plan: &WritePlan, args: &CliArgs) {
    let (label, noun) = if plan.copy_from.is_some() {
        ("Destination file", "destination file")
    } else {
        ("File", "file")
    };
    let original_mtime = prepare_write_target(file, args, label, noun);
    let result = write_plan_file(file, plan, backup_before_commit(file, args));
    if let (Ok(done), Some((src, filters))) = (&result, &plan.copy_from)
        && let Some(copy) = &done.copy
    {
        report_copy(src, filters, copy);
    }
    // ExifTool prints the same line for a copy; the old `(N tags copied)`
    // suffix counted filter arguments, not tags written.
    finish_write(
        file,
        result.map(|done| done.outcome),
        original_mtime,
        "    1 image files updated",
    );
}

/// ExifTool's copy warnings: nothing found to copy (13.59: `Warning: No
/// writable tags set from SRC`), and -- oxidex's own -- the source groups a
/// copy-all could not write, which are named rather than silently skipped.
fn report_copy(
    src: &std::path::Path,
    filters: &[String],
    copy: &oxidex::core::operations::CopyReport,
) {
    if !filters.is_empty() && copy.requested == 0 {
        PathLine::new("Warning: No writable tags set from ")
            .path(src)
            .eprint();
    }
    if !copy.uncopied_groups.is_empty() {
        eprintln!(
            "Warning: Not copied from {}: oxidex cannot write the {} group(s) here",
            src.display(),
            copy.uncopied_groups.join(", ")
        );
    }
}

/// Handles `-all=`, date shifts and `-TagsFromFile` (alone or with sets)
/// over several explicit files: every file gets its own transaction, and
/// the summary counts each one, as ExifTool's does. These modes used to act
/// on the last path only.
fn handle_multi_write(files: &[std::path::PathBuf], plan: &WritePlan, args: &CliArgs) {
    let mut stats = batch_processor::BatchStats::for_write();
    for file in files {
        let prepared = std::fs::metadata(file)
            .map_err(|e| format!("Cannot access file '{}': {}", file.display(), e))
            .and_then(|metadata| {
                if metadata.permissions().readonly() {
                    Err(format!("File is read-only: {}", file.display()))
                } else {
                    Ok(metadata.modified().ok())
                }
            });
        let result = prepared.and_then(|mtime| {
            write_plan_file(file, plan, backup_before_commit(file, args)).map(|done| (done, mtime))
        });
        match result {
            Ok((done, mtime)) => {
                if let (Some((src, filters)), Some(copy)) = (&plan.copy_from, &done.copy) {
                    report_copy(src, filters, copy);
                }
                // The library's outcome (ExifTool's WriteInfo 1 / 2) is the
                // count; `WriteOutcome` is non-exhaustive, and anything but
                // `Updated` left the file as it was.
                if done.outcome == WriteOutcome::Updated {
                    stats.files_updated += 1;
                    if args.preserve_file_times
                        && let Some(mtime) = mtime
                    {
                        let _ = std::fs::File::open(file).and_then(|f| f.set_modified(mtime));
                    }
                } else {
                    stats.files_unchanged += 1;
                }
            }
            Err(message) => {
                stats.errors += 1;
                PathLine::new("Error writing ")
                    .path(file)
                    .text(&format!(": {message}"))
                    .eprint();
            }
        }
    }
    stats.print();
    if stats.errors > 0 {
        process::exit(1);
    }
}

/// The checks every single-file write makes before touching anything: the
/// file exists and is writable. Returns its modification time when
/// `--preserve-file-times` asks for it to be restored after an update.
/// `label`/`noun` keep each caller's existing wording ("File"/"file", or
/// "Destination file"/"destination file").
fn prepare_write_target(
    file: &std::path::Path,
    args: &CliArgs,
    label: &str,
    noun: &str,
) -> Option<std::time::SystemTime> {
    // Verify file exists
    if !file.exists() {
        PathLine::new(&format!("Error: {label} not found: "))
            .path(file)
            .eprint();
        process::exit(1);
    }

    // Check if file is writable
    let file_metadata = match std::fs::metadata(file) {
        Ok(metadata) => {
            if metadata.permissions().readonly() {
                PathLine::new(&format!("Error: {label} is read-only: "))
                    .path(file)
                    .eprint();
                process::exit(1);
            }
            metadata
        }
        Err(e) => {
            PathLine::new(&format!("Error: Cannot access {noun} '"))
                .path(file)
                .text(&format!("': {e}"))
                .eprint();
            process::exit(1);
        }
    };

    // Save original modification time if preserve_file_times is enabled
    if args.preserve_file_times {
        match file_metadata.modified() {
            Ok(mtime) => Some(mtime),
            Err(e) => {
                eprintln!("Warning: Could not read file modification time: {}", e);
                None
            }
        }
    } else {
        None
    }
}

/// The `--backup` copy (`photo.jpg` -> `photo.jpg.bak`), taken only once a
/// write is known to change the file: ExifTool makes no `_original` for an
/// unchanged file.
fn backup_before_commit<'a>(
    file: &'a std::path::Path,
    args: &'a CliArgs,
) -> impl FnOnce() -> Result<(), String> + 'a {
    move || {
        if !args.backup {
            return Ok(());
        }
        let mut backup_path = file.as_os_str().to_owned();
        backup_path.push(".bak");
        let backup_path = std::path::PathBuf::from(backup_path);
        std::fs::copy(file, &backup_path).map(|_| ()).map_err(|e| {
            format!(
                "Failed to create backup file '{}': {}",
                backup_path.display(),
                e
            )
        })
    }
}

/// Prints a single-file write's summary from what actually happened to the
/// bytes (`cli::write_transaction::transact`), restoring the modification
/// time after an update when asked. `updated_line` is the caller's success
/// line (the copy path appends its tag count).
fn finish_write(
    file: &std::path::Path,
    outcome: Result<WriteOutcome, String>,
    original_mtime: Option<std::time::SystemTime>,
    updated_line: &str,
) {
    match outcome {
        Ok(WriteOutcome::Updated) => {
            // Restore original modification time if requested
            if let Some(mtime) = original_mtime {
                use std::fs::File;
                if let Err(e) = File::open(file).and_then(|f| f.set_modified(mtime)) {
                    eprintln!("Warning: Could not restore file modification time: {}", e);
                    // Don't exit - the write succeeded, only mtime restoration failed
                }
            }
            // Print success message (matching ExifTool format)
            println!("{}", updated_line);
        }
        Ok(_) => {
            println!("    0 image files updated");
            println!("    1 image files unchanged");
        }
        Err(message) => {
            // A refusal that is ExifTool's own warning text (`Warning: Sorry,
            // <tag> doesn't exist or isn't writable` / `Nothing to do.`, the
            // PNG `XMP` literal-text-chunk case) is printed as ExifTool
            // itself would print it, not wrapped in oxidex's own `Error:`
            // diagnosis.
            if is_exiftool_refusal_message(&message) {
                eprintln!("{}", message);
            } else {
                eprintln!("Error: {}", message);
            }
            process::exit(1);
        }
    }
}

/// Handles read operations (displaying metadata)
fn handle_read_operation(file: &std::path::Path, args: &CliArgs) {
    // Step 21: build request-awareness (`ReadOptions`) from the specific
    // tags requested and `--extended-output` *before* reading, so a
    // specifically-requested `-JPEGQualityEstimate` (or `--extended-output`)
    // actually reaches the JPEG parser instead of being computed
    // unconditionally or not at all. See `core::read_options`.
    let tag_filter = args.specific_tags();
    let read_options =
        oxidex::core::ReadOptions::new(tag_filter.as_deref().unwrap_or(&[]), args.extended_output);
    match read_metadata_report_with_detector_and_options(file, args.detector, &read_options) {
        Ok(report) => {
            // `--strict` opts back into the old fail-fast behavior: a read
            // that didn't fully parse is an error, not partial output. See
            // `CliArgs::strict` and `read_metadata_report_with_detector`'s
            // doc comment for why the default is otherwise to degrade
            // gracefully (ExifTool.pm:8483's `Warn('JPEG format error')`
            // model) rather than discard the whole read.
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
                PathLine::new("Error: Failed to read metadata from '")
                    .path(file)
                    .text(&format!("': {reason} (status: {})", report.status))
                    .eprint();
                process::exit(1);
            }

            let status = report.status;
            let raw_metadata = report.metadata;

            // Check if any metadata was found
            if raw_metadata.is_empty() {
                PathLine::new("No metadata found in file: ")
                    .path(file)
                    .print();
                return;
            }

            // A specific `-TAG` request goes through Step 20's group/priority-
            // aware resolution (`cli::tag_resolution::resolve_requested_tags`)
            // instead of the exact/suffix match `tag_matches_filter` used to
            // apply; the unfiltered default listing applies Step 21's
            // extended-namespace filter. See
            // `cli::tag_resolution::resolve_file_output`'s doc comment for
            // the full rationale, including why `-Gn` + human/short output
            // is rendered directly rather than through a formatter.
            //
            // Shared with batch mode (`cli::batch_processor`) via
            // `cli::tag_resolution::resolve_file_output`, so the two CLI
            // paths agree on `-a`/`-G*`/`--no-print-conv`/the extended
            // namespace for the same file (Step 21 closes the Step 20 gap
            // where batch bypassed this module entirely). `show_header`
            // (the human-readable `"File: ..."`/`"Found N ..."` preamble)
            // only ever applies to the unfiltered full listing.
            match oxidex::cli::tag_resolution::resolve_file_output(&raw_metadata, args) {
                oxidex::cli::tag_resolution::ResolvedFileOutput::Lines(output) => {
                    print!("{}", output);
                }
                oxidex::cli::tag_resolution::ResolvedFileOutput::Metadata(metadata) => {
                    print_resolved_metadata(file, &metadata, args, status, tag_filter.is_none());
                }
            }
        }
        Err(e) => {
            PathLine::new("Error: Failed to read metadata from '")
                .path(file)
                .text(&format!("': {e}"))
                .eprint();
            process::exit(1);
        }
    }
}

/// Dispatches an already display-ready `metadata` (PrintConv applied or
/// not, filtered to exactly what should show) to whichever formatter
/// `args` selects. Shared tail of [`handle_read_operation`]'s two paths --
/// a resolved specific-tag request and the full-listing default -- which
/// differ only in how `metadata` was built, not in how it is printed.
/// `show_header` gates the human-readable `"File: ..." / "Found N ..."`
/// preamble, which only ever applied to the unfiltered full listing.
fn print_resolved_metadata(
    file: &std::path::Path,
    metadata: &oxidex::core::MetadataMap,
    args: &CliArgs,
    status: ParseStatus,
    show_header: bool,
) {
    if args.csv {
        let formatter = CsvFormatter;
        let output = formatter.format_with_mode(metadata, None, !args.exiftool_compat());
        print!("{}", output);
    } else if args.json {
        // Carries `Status` for any read that didn't fully parse (see
        // `JsonFormatter::format_with_status`); identical to plain `format`
        // for a healthy `Parsed` read.
        let formatter = JsonFormatter;
        let output = formatter.format_with_status_and_mode(
            metadata,
            None,
            Some(status),
            !args.exiftool_compat(),
        );
        println!("{}", output);
    } else if args.short_level > 0 {
        let formatter = ShortFormatter;
        let output = formatter.format_with_mode(metadata, None, !args.exiftool_compat());
        print!("{}", output);
    } else {
        if show_header {
            PathLine::new("File: ").path(file).print();
            println!("Found {} metadata tag(s):", metadata.len());
            println!();
        }
        let formatter = HumanReadableFormatter;
        let output = formatter.format_with_mode(metadata, None, !args.exiftool_compat());
        print!("{}", output);
    }
}

/// Handles batch processing (multiple files or directories)
fn handle_batch_processing(
    path: &std::path::Path,
    args: &CliArgs,
    modifications: &[(String, std::ffi::OsString)],
) {
    match batch_processor::batch_process_requests(path, args, modifications) {
        Ok(stats) => {
            let is_read_mode = modifications.is_empty();
            // ExifTool prints its read summary after text output at every
            // level, `-s`/`-s3` included; only JSON/CSV keep stdout clean.
            if !(is_read_mode && (args.json || args.csv)) {
                stats.print();
            }

            // Exit with error code if there were any errors
            if stats.errors > 0 {
                process::exit(1);
            }
        }
        Err(e) => {
            eprintln!("Error: Batch processing failed: {}", e);
            process::exit(1);
        }
    }
}

/// Handles rename operations (renaming files based on metadata)
fn handle_rename_operation(file: &std::path::Path, pattern: &str, args: &CliArgs) {
    // Verify file exists
    if !file.exists() {
        PathLine::new("Error: File not found: ").path(file).eprint();
        process::exit(1);
    }

    // If it's a directory, we could support batch rename in the future
    // For now, only support single files
    if file.is_dir() {
        eprintln!("Error: Directory renaming not yet supported");
        eprintln!("Please specify a single file for renaming");
        process::exit(1);
    }

    // Extract date format if provided
    let date_format = args.date_format.as_deref();

    // Perform rename
    match rename::rename_file(file, pattern, date_format, args.dry_run) {
        Ok(_new_path) => {
            if !args.dry_run {
                // Print success message
                println!("    1 image files renamed");
            }
        }
        Err(e) => {
            PathLine::new("Error: Failed to rename file '")
                .path(file)
                .text(&format!("': {e}"))
                .eprint();
            process::exit(1);
        }
    }
}
