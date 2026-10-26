//! `ff-tools`: Freight Fate's build, release, data and translation tooling.
//!
//! One binary with one subcommand per job, run through the cargo alias in
//! `.cargo/config.toml`:
//!
//! ```text
//! cargo tools <command> [options]
//! cargo tools help
//! ```
//!
//! The commands that only read git and text (the changelog gate, release
//! notes, the test-target check, translations) build without the `world`
//! feature, so a CI job that runs one of them does not compile the game's
//! core crate: `cargo run -p ff-tools --no-default-features -- <command>`.

mod check_test_targets;
mod cli;
mod http;
mod l10n;
mod manual_html;
mod prune_snapshots;
mod pyjson;
mod radio;
mod release;
mod release_notes;
mod translation_kit;
mod util;
mod windows_runtime;

#[cfg(feature = "world")]
mod pack_sounds;
mod playtest_watch;
#[cfg(feature = "world")]
mod world;

type Command = fn(&[String]) -> util::Result<i32>;

/// Every command: its name, what it does, and its entry point.
const COMMANDS: &[(&str, &str, Command)] = &[
    (
        "build-release",
        "build, verify and archive the player release for this platform",
        release::main,
    ),
    (
        "build-appimage",
        "package the staged Linux build as an AppImage",
        release::appimage::main,
    ),
    (
        "build-ios",
        "build the iPhone and iPad app (Simulator, device or TestFlight .ipa)",
        release::ios::main,
    ),
    (
        "ios-signing",
        "find the TestFlight signing profile and manage uploads (App Store Connect)",
        release::ios_signing::main,
    ),
    (
        "windows-imports",
        "audit a Cargo build's Windows executable the way the release does",
        windows_runtime::check_windows_imports,
    ),
    (
        "release-notes",
        "write release notes, decide nightlies, and run the changelog gate",
        release_notes::main,
    ),
    (
        "prune-snapshots",
        "delete old 1.9 tester snapshot releases, keeping the newest few",
        prune_snapshots::main,
    ),
    (
        "translation-kit",
        "zip lang/ as the translator's kit release download",
        translation_kit::main,
    ),
    (
        "version",
        "print the project version (--release-line drops a .devN suffix)",
        release::version_main,
    ),
    (
        "test-targets",
        "check that every crate's integration tests stay in one test binary",
        check_test_targets::main,
    ),
    (
        "l10n-extract",
        "rebuild lang/freight-fate.pot and every language file (--check: fail if stale)",
        l10n::extract::main,
    ),
    (
        "l10n-check",
        "check every language file for mistakes the game cannot fix (--stats)",
        l10n::check::main,
    ),
    (
        "fold-community-stations",
        "copy the accepted community radio stations into data/radio_community.json",
        radio::fold_community::main,
    ),
    (
        "check-radio-streams",
        "probe every real stream on the dial and write the health report",
        radio::check_streams::main,
    ),
    (
        "audit-radio-streams",
        "listen to every stream: answer, station identity, dead air (needs curl and ffmpeg)",
        radio::audit_streams::main,
    ),
    (
        "playtest-watch",
        "watch a live manual playtest's log and report what is worth interrupting for",
        playtest_watch::main,
    ),
    #[cfg(feature = "world")]
    (
        "pack-sounds",
        "pack assets/sounds into the masked sounds.pak and music.pak",
        pack_sounds::main,
    ),
    #[cfg(feature = "world")]
    (
        "index-world",
        "regenerate data/world_data/ from data/world_source/ (--check: verify in sync)",
        world::index::main,
    ),
];

fn usage() -> String {
    let width = COMMANDS.iter().map(|(n, _, _)| n.len()).max().unwrap_or(0);
    let mut out = String::from("usage: cargo tools <command> [options]\n\ncommands:\n");
    for (name, about, _) in COMMANDS {
        out.push_str(&format!("  {name:width$}  {about}\n"));
    }
    out
}

fn main() {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let Some((name, rest)) = argv.split_first() else {
        eprint!("{}", usage());
        std::process::exit(2);
    };
    if matches!(name.as_str(), "help" | "--help" | "-h") {
        print!("{}", usage());
        return;
    }
    let Some((_, _, run)) = COMMANDS.iter().find(|(n, _, _)| n == name) else {
        eprintln!("unknown command {name:?}");
        eprint!("{}", usage());
        std::process::exit(2);
    };
    match run(rest) {
        Ok(code) => std::process::exit(code),
        Err(message) => {
            eprintln!("{message}");
            std::process::exit(1);
        }
    }
}
