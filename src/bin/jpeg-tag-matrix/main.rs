//! JPEG tag matrix pipeline: manifest generation, empirical read/write testing,
//! and report generation against a committed regression baseline.
//! Rust port of scripts/{generate_exiftool_manifest,jpeg_tag_matrix,jpeg_tag_report}.py.

mod manifest;
mod matrix;
mod report;
mod types;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "jpeg-tag-matrix")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Port of generate_exiftool_manifest.py
    Manifest(manifest::ManifestArgs),
    /// Port of jpeg_tag_matrix.py
    Run(matrix::RunArgs),
    /// Port of jpeg_tag_report.py
    Report(report::ReportArgs),
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Manifest(args) => manifest::run(args),
        Command::Run(args) => matrix::run(args),
        Command::Report(args) => report::run(args),
    }
}
