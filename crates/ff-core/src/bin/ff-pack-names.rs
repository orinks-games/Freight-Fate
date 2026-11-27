//! `ff-pack-names` -- print every name a sound pack holds, one per line.
//!
//! ```text
//! ff-pack-names assets/sounds.pak
//! ```
//!
//! The release builder runs this beside `ff-bake` to prove a staged pack
//! carries what the game reads. Both come out of the same `cargo build` as
//! the game, so the builder itself needs no copy of ff-core and starts in
//! seconds.

use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

use ff_core::assets_pack::SoundPack;

fn main() -> ExitCode {
    let mut args = std::env::args_os().skip(1);
    let (Some(path), None) = (args.next(), args.next()) else {
        eprintln!("usage: ff-pack-names <pack>");
        return ExitCode::from(2);
    };
    let path = PathBuf::from(path);
    let pack = match SoundPack::open(&path) {
        Ok(pack) => pack,
        Err(err) => {
            eprintln!("{}: {err}", path.display());
            return ExitCode::FAILURE;
        }
    };
    let mut out = std::io::BufWriter::new(std::io::stdout().lock());
    for name in pack.names() {
        if writeln!(out, "{name}").is_err() {
            return ExitCode::FAILURE;
        }
    }
    if out.flush().is_err() {
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}
