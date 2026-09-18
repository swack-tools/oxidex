//! Dispatch microbench for the "interpreter in disguise" claim.
//!
//! Four ways to resolve an IFD tag id against a generated table:
//!   linear   -- `table.tags.iter().find(|t| t.id == id)` (the critique's
//!               picture; it is what `ifd_engine.rs:1479` does on one side path)
//!   bsearch  -- `IfdTable::tag(id)` (what `ifd_engine::resolve` actually does)
//!   matcharm -- a generated `match id { 0x.. => Some(&T.tags[i]) }` (what
//!               "compile the table to a native jump table" would produce)
//!   hashmap  -- `HashMap<u16, &IfdTag>` built once (for scale)
//!
//! Workloads: every id of the table in order (`all`), a realistic per-file mix
//! (the ids ExifTool 13.59 walks for t/images/Canon.jpg, Nikon.nef), and
//! misses (ids the table does not declare).

mod match_tables;

use std::collections::HashMap;
use std::hint::black_box;

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use match_tables::*;
use oxidex::exiftool_tables::ifd_schema::{IfdTable, IfdTag};
use oxidex::exiftool_tables::ifd_tables::{IFD_CANON_MAIN, IFD_EXIF_MAIN};

#[inline(never)]
fn linear(table: &'static IfdTable, id: u16) -> Option<&'static IfdTag> {
    table.tags.iter().find(|t| t.id == id)
}

#[inline(never)]
fn bsearch(table: &'static IfdTable, id: u16) -> Option<&'static IfdTag> {
    table.tag(id)
}

fn run_workload(
    c: &mut Criterion,
    group: &str,
    table: &'static IfdTable,
    matcharm: fn(u16) -> Option<&'static IfdTag>,
    workload: &str,
    ids: &[u16],
) {
    let map: HashMap<u16, &'static IfdTag> = table.tags.iter().map(|t| (t.id, t)).collect();
    let mut g = c.benchmark_group(format!("{group}/{workload}"));
    g.throughput(Throughput::Elements(ids.len() as u64));
    g.bench_with_input(BenchmarkId::new("linear", ids.len()), ids, |b, ids| {
        b.iter(|| {
            let mut hits = 0usize;
            for &id in ids {
                hits += linear(table, black_box(id)).is_some() as usize;
            }
            hits
        })
    });
    g.bench_with_input(BenchmarkId::new("bsearch", ids.len()), ids, |b, ids| {
        b.iter(|| {
            let mut hits = 0usize;
            for &id in ids {
                hits += bsearch(table, black_box(id)).is_some() as usize;
            }
            hits
        })
    });
    g.bench_with_input(BenchmarkId::new("matcharm", ids.len()), ids, |b, ids| {
        b.iter(|| {
            let mut hits = 0usize;
            for &id in ids {
                hits += matcharm(black_box(id)).is_some() as usize;
            }
            hits
        })
    });
    g.bench_with_input(BenchmarkId::new("hashmap", ids.len()), ids, |b, ids| {
        b.iter(|| {
            let mut hits = 0usize;
            for &id in ids {
                hits += map.get(&black_box(id)).is_some() as usize;
            }
            hits
        })
    });
    g.finish();
}

fn all_ids(table: &'static IfdTable) -> Vec<u16> {
    table.tags.iter().map(|t| t.id).collect()
}

fn misses(table: &'static IfdTable, n: usize) -> Vec<u16> {
    // Deterministic ids the table does not declare, spread over the id space.
    let mut out = Vec::new();
    let mut x: u32 = 0x9e37;
    while out.len() < n {
        x = x.wrapping_mul(1103515245).wrapping_add(12345) & 0xffff;
        let id = x as u16;
        if table.tag(id).is_none() {
            out.push(id);
        }
    }
    out
}

fn bench(c: &mut Criterion) {
    // Sanity: all four agree on every id of both tables.
    for id in 0..=u16::MAX {
        let a = linear(&IFD_EXIF_MAIN, id).map(|t| t as *const _);
        assert_eq!(a, bsearch(&IFD_EXIF_MAIN, id).map(|t| t as *const _));
        assert_eq!(a, exif_main_match(id).map(|t| t as *const _));
        let a = linear(&IFD_CANON_MAIN, id).map(|t| t as *const _);
        assert_eq!(a, bsearch(&IFD_CANON_MAIN, id).map(|t| t as *const _));
        assert_eq!(a, canon_main_match(id).map(|t| t as *const _));
    }
    run_workload(
        c,
        "exif_main",
        &IFD_EXIF_MAIN,
        exif_main_match,
        "all",
        &all_ids(&IFD_EXIF_MAIN),
    );
    run_workload(
        c,
        "exif_main",
        &IFD_EXIF_MAIN,
        exif_main_match,
        "canon_jpg",
        CANON_JPG_EXIF_IDS,
    );
    run_workload(
        c,
        "exif_main",
        &IFD_EXIF_MAIN,
        exif_main_match,
        "nikon_nef",
        NIKON_NEF_EXIF_IDS,
    );
    run_workload(
        c,
        "exif_main",
        &IFD_EXIF_MAIN,
        exif_main_match,
        "miss",
        &misses(&IFD_EXIF_MAIN, 64),
    );
    run_workload(
        c,
        "canon_main",
        &IFD_CANON_MAIN,
        canon_main_match,
        "all",
        &all_ids(&IFD_CANON_MAIN),
    );
    run_workload(
        c,
        "canon_main",
        &IFD_CANON_MAIN,
        canon_main_match,
        "canon_jpg",
        CANON_JPG_CANON_IDS,
    );
    run_workload(
        c,
        "canon_main",
        &IFD_CANON_MAIN,
        canon_main_match,
        "miss",
        &misses(&IFD_CANON_MAIN, 64),
    );
}

criterion_group!(benches, bench);
criterion_main!(benches);
