//! `ff-bake` -- turn the JSON data tree into the shipped `world.ffdata`.
//!
//! ```text
//! ff-bake --data-dir data --out dist/freight_fate/data/world.ffdata
//! ff-bake --data-dir data --out <same path> --check
//! ```
//!
//! `--check` re-bakes to a temp file and compares bytes with the container
//! already at `--out`, which is how CI and `tools/build_release.py` prove a
//! committed or staged container matches the tree it claims to come from.
//!
//! The baker lives in the library (`ff_core::data::baked::bake`) so the tests
//! can call it; this is argument parsing and a size table.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use ff_core::data::baked::{bake, bake_bytes, BAKED_FILE_NAME};

const USAGE: &str = "\
usage: ff-bake --data-dir <dir> --out <file> [--check] [--quiet]

  --data-dir <dir>  the world data folder (data/ in a checkout)
  --out <file>      where to write world.ffdata; a directory is allowed and
                    the file is named world.ffdata inside it
  --check           do not write: re-bake and compare bytes with --out
  --quiet           suppress the size table
";

struct Args {
    data_dir: PathBuf,
    out: PathBuf,
    check: bool,
    quiet: bool,
}

fn parse_args() -> Result<Args, String> {
    let mut data_dir: Option<PathBuf> = None;
    let mut out: Option<PathBuf> = None;
    let mut check = false;
    let mut quiet = false;
    let mut argv = std::env::args().skip(1);
    while let Some(arg) = argv.next() {
        match arg.as_str() {
            "--data-dir" => {
                data_dir = Some(PathBuf::from(
                    argv.next().ok_or("--data-dir needs a directory")?,
                ));
            }
            "--out" => {
                out = Some(PathBuf::from(argv.next().ok_or("--out needs a path")?));
            }
            "--check" => check = true,
            "--quiet" => quiet = true,
            "-h" | "--help" => return Err(USAGE.to_string()),
            other => return Err(format!("unknown argument {other}\n\n{USAGE}")),
        }
    }
    let data_dir = data_dir.ok_or_else(|| format!("--data-dir is required\n\n{USAGE}"))?;
    let mut out = out.ok_or_else(|| format!("--out is required\n\n{USAGE}"))?;
    if out.is_dir() {
        out = out.join(BAKED_FILE_NAME);
    }
    Ok(Args {
        data_dir,
        out,
        check,
        quiet,
    })
}

fn run(args: &Args) -> Result<(), String> {
    if !args.data_dir.join("world_data").is_dir() {
        return Err(format!(
            "{} has no world_data/ -- is that the package data folder?",
            args.data_dir.display()
        ));
    }
    if args.check {
        return check(&args.data_dir, &args.out, args.quiet);
    }
    let report = bake(&args.data_dir, &args.out).map_err(|e| e.to_string())?;
    if !args.quiet {
        print!("{}", report.table());
        println!(
            "{} legs, {} with corridor detail -> {}",
            report.legs,
            report.legs_with_corridor,
            args.out.display()
        );
        println!(
            "{} short speed-limit postings folded as mapping artifacts",
            report.speed_limits_folded
        );
    }
    Ok(())
}

fn check(data_dir: &Path, out: &Path, quiet: bool) -> Result<(), String> {
    let existing = std::fs::read(out)
        .map_err(|e| format!("{}: {e} (bake it first, then --check)", out.display()))?;
    let (fresh, report) = bake_bytes(data_dir).map_err(|e| e.to_string())?;
    if !quiet {
        print!("{}", report.table());
    }
    if existing == fresh {
        if !quiet {
            println!("{} matches {}", out.display(), data_dir.display());
        }
        return Ok(());
    }
    Err(format!(
        "{} is stale: {} bytes on disk, {} bytes from {}. Re-bake it.",
        out.display(),
        existing.len(),
        fresh.len(),
        data_dir.display()
    ))
}

fn main() -> ExitCode {
    let args = match parse_args() {
        Ok(args) => args,
        Err(message) => {
            eprintln!("{message}");
            return ExitCode::FAILURE;
        }
    };
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("ff-bake: {message}");
            ExitCode::FAILURE
        }
    }
}
