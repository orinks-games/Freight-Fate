//! Bakes the game's seal key into the build.
//!
//! `hearken release` builds a game that sets `seal = true` with the game's
//! own secret in `HEARKEN_SEAL_KEY` (64 hex digits). Every other build has
//! none and reads and writes plain files. The key is never written into the
//! program whole: `hearken-bake` splits it into shares.

use std::path::PathBuf;

fn main() {
    let code = hearken_bake::from_env("HEARKEN_SEAL_KEY");
    let out = PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR"));
    std::fs::write(out.join("seal_key.rs"), code).expect("write seal_key.rs");
}
