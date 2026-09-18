//! Per-file stage timer + allocation counter for the dispatch-perf spike.
//!
//! Stages (each timed and alloc-counted in isolation, `REPS` repetitions,
//! median reported):
//!   fs      -- `extract_file_metadata` (File: group, stat)
//!   mmap    -- `MMapReader::new`
//!   detect  -- `parsers::detect_format` (signature detection)
//!   read    -- `read_metadata_with_detector_and_options` (the whole read:
//!              fs + mmap + detect + parse + conversions + composite)
//!   clone   -- `MetadataMap::clone` (allocations RETAINED by the map:
//!              key Strings, TagId::Named duplicates, value Strings)
//!   json    -- `JsonFormatter::format_with_status_and_mode` (the `-j` path)
//!
//! Usage: stages [--reps N] <file>...   (prints one TSV row per file + totals)

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use oxidex::cli::output_formatter::JsonFormatter;
use oxidex::core::read_report::ParseStatus;
use oxidex::core::{ReadOptions, read_metadata_with_detector_and_options};
use oxidex::io::MMapReader;
use oxidex::parsers::{DetectorMode, detect_format};
use oxidex_spike_dispatch_perf::{AllocSnapshot, Counting, delta, snapshot};

#[global_allocator]
static GLOBAL: Counting = Counting;

struct Stage {
    times: Vec<Duration>,
    allocs: AllocSnapshot,
}

fn measure<T>(reps: usize, mut f: impl FnMut() -> T) -> (Stage, T) {
    let mut times = Vec::with_capacity(reps);
    let mut last = None;
    let mut allocs = AllocSnapshot::default();
    for i in 0..reps {
        let a = snapshot();
        let t = Instant::now();
        let v = f();
        times.push(t.elapsed());
        if i == 0 {
            allocs = delta(a, snapshot());
        }
        last = Some(v);
    }
    times.sort();
    (Stage { times, allocs }, last.unwrap())
}

fn median(s: &Stage) -> Duration {
    s.times[s.times.len() / 2]
}

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let mut reps = 50usize;
    if args.first().map(String::as_str) == Some("--reps") {
        args.remove(0);
        reps = args.remove(0).parse().expect("--reps N");
    }
    let files: Vec<PathBuf> = args.iter().map(PathBuf::from).collect();
    let opts = ReadOptions::new(&[], false);

    println!(
        "file\ttags\tfs_us\tmmap_us\tdetect_us\tread_us\tjson_us\tread_allocs\tread_reallocs\tread_bytes\tretained_allocs\tjson_allocs\tjson_bytes"
    );
    let mut tot = [0f64; 5];
    let mut tot_allocs = [0usize; 4];
    let mut tot_tags = 0usize;
    for path in &files {
        let (fs, _) = measure(reps, || {
            oxidex::core::file_metadata::extract_file_metadata(path)
        });
        let (mm, reader) = measure(reps, || MMapReader::new(path).expect("mmap"));
        let (det, _) = measure(reps, || detect_format(&reader));
        let (rd, map) = measure(reps, || {
            read_metadata_with_detector_and_options(path, DetectorMode::Signature, &opts)
        });
        let map = match map {
            Ok(m) => m,
            Err(e) => {
                eprintln!("{}: read failed: {e}", path.display());
                continue;
            }
        };
        let (cl, _) = measure(reps, || map.clone());
        let (js, out) = measure(reps, || {
            JsonFormatter.format_with_status_and_mode(&map, None, Some(ParseStatus::Parsed), false)
        });
        let name = Path::new(path).file_name().unwrap().to_string_lossy();
        let row = [
            median(&fs).as_secs_f64() * 1e6,
            median(&mm).as_secs_f64() * 1e6,
            median(&det).as_secs_f64() * 1e6,
            median(&rd).as_secs_f64() * 1e6,
            median(&js).as_secs_f64() * 1e6,
        ];
        for (t, r) in tot.iter_mut().zip(row) {
            *t += r;
        }
        tot_allocs[0] += rd.allocs.allocs;
        tot_allocs[1] += rd.allocs.reallocs;
        tot_allocs[2] += cl.allocs.allocs;
        tot_allocs[3] += js.allocs.allocs;
        tot_tags += map.len();
        println!(
            "{name}\t{}\t{:.1}\t{:.1}\t{:.1}\t{:.1}\t{:.1}\t{}\t{}\t{}\t{}\t{}\t{}",
            map.len(),
            row[0],
            row[1],
            row[2],
            row[3],
            row[4],
            rd.allocs.allocs,
            rd.allocs.reallocs,
            rd.allocs.bytes,
            cl.allocs.allocs,
            js.allocs.allocs,
            js.allocs.bytes,
        );
        std::hint::black_box(out);
    }
    println!(
        "TOTAL\t{tot_tags}\t{:.1}\t{:.1}\t{:.1}\t{:.1}\t{:.1}\t{}\t{}\t-\t{}\t{}\t-",
        tot[0],
        tot[1],
        tot[2],
        tot[3],
        tot[4],
        tot_allocs[0],
        tot_allocs[1],
        tot_allocs[2],
        tot_allocs[3]
    );
}
