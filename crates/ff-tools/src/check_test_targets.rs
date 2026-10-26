//! `test-targets`: keep every crate's integration tests in one test binary.
//!
//! Each test binary links the whole dependency graph again, so a crate builds
//! its integration tests as ONE binary, `it` (tests/it/main.rs), and a new
//! test file goes in as a `mod` line there rather than a new top-level file.
//! A few tests need a process of their own and keep their own binary; they
//! are listed in [`SEPARATE_BINARIES`] with the reason, and CLAUDE.md names
//! them too.
//!
//! This fails when:
//!
//! - a crate's tests/ holds a top-level test file or directory that is not
//!   `it` and not listed in [`SEPARATE_BINARIES`] (a new binary nobody
//!   decided on);
//! - a file or directory directly under tests/it/ is neither a `mod` of
//!   tests/it/main.rs nor pulled in by a `#[path]` attribute (a test that
//!   never compiles, so never runs).

use std::collections::HashSet;
use std::path::Path;

use regex::Regex;

use crate::util::{self, rel};

/// crate -> (top-level entry under tests/, why it is its own binary).
pub const SEPARATE_BINARIES: &[(&str, &[(&str, &str)])] = &[
    (
        "ff-core",
        &[
            (
                "data_baked.rs",
                "points the process at the baked container's data root",
            ),
            (
                "data_map_correction.rs",
                "points the process at a corrected data root",
            ),
        ],
    ),
    (
        "freight-fate",
        &[
            (
                "classic_startup.rs",
                "needs a process where the 1.5 classics are not yet registered",
            ),
            (
                "frame_time",
                "times frames against the wall clock, which sibling tests would contend for",
            ),
            (
                "agent_server.rs",
                "agent tooling, `test = false`; CI runs it by name",
            ),
            (
                "real_window.rs",
                "opens the real game window, `harness = false` and `test = false`; run by name on a desktop",
            ),
        ],
    ),
];

fn allowed(crate_name: &str) -> HashSet<&'static str> {
    SEPARATE_BINARIES
        .iter()
        .filter(|(name, _)| *name == crate_name)
        .flat_map(|(_, entries)| entries.iter().map(|(entry, _)| *entry))
        .collect()
}

fn sorted_entries(dir: &Path) -> Vec<std::path::PathBuf> {
    let mut entries: Vec<_> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .collect();
    util::sort_paths(&mut entries);
    entries
}

fn name_of(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn stem_of(path: &Path) -> String {
    path.file_stem()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn is_rs(path: &Path) -> bool {
    path.extension().is_some_and(|e| e == "rs")
}

/// Every problem with one crate's test layout.
pub fn check_crate(root: &Path, crate_dir: &Path) -> Vec<String> {
    let path_attr = Regex::new(r#"#\[path\s*=\s*"([^"]+)"\]"#).expect("valid regex");
    let mod_line =
        Regex::new(r"(?m)^\s*(?:#\[[^\]]*\]\s*)*(?:pub(?:\([^)]*\))?\s+)?mod\s+(\w+)\s*;")
            .expect("valid regex");

    let mut problems = Vec::new();
    let tests = crate_dir.join("tests");
    if !tests.is_dir() {
        return problems;
    }
    let crate_name = name_of(crate_dir);
    let allowed = allowed(&crate_name);
    for entry in sorted_entries(&tests) {
        let name = name_of(&entry);
        if name == "it" || allowed.contains(name.as_str()) {
            continue;
        }
        if entry.is_file() && !is_rs(&entry) {
            continue;
        }
        if entry.is_dir() && !entry.join("main.rs").exists() {
            continue; // a fixture directory, not a test target
        }
        problems.push(format!(
            "{} is a test binary of its own; make it a `mod` of {crate_name}/tests/it/main.rs, \
             or list it in SEPARATE_BINARIES with the reason",
            rel(&entry, root)
        ));
    }
    let it = tests.join("it");
    let main = it.join("main.rs");
    if it.is_dir() {
        if !main.exists() {
            problems.push(format!("{} has no main.rs", rel(&it, root)));
            return problems;
        }
        let main_src = std::fs::read_to_string(&main).unwrap_or_default();
        let mut listed: HashSet<String> = mod_line
            .captures_iter(&main_src)
            .map(|c| c[1].to_string())
            .collect();
        // A sibling module may pull a file in with `#[path = "x.rs"]` to keep
        // its own file under the size limit; that file compiles too.
        for src in util::walk_files(&it).into_iter().filter(|p| is_rs(p)) {
            let text = std::fs::read_to_string(&src).unwrap_or_default();
            for target in path_attr.captures_iter(&text) {
                listed.insert(stem_of(Path::new(&target[1])));
            }
        }
        for entry in sorted_entries(&it) {
            if name_of(&entry) == "main.rs" {
                continue;
            }
            if entry.is_file() && !is_rs(&entry) {
                continue;
            }
            let name = if entry.is_file() {
                stem_of(&entry)
            } else {
                name_of(&entry)
            };
            if !listed.contains(&name) {
                problems.push(format!(
                    "{} is not a `mod` of {}, so it never compiles or runs",
                    rel(&entry, root),
                    rel(&main, root)
                ));
            }
        }
    }
    problems
}

pub fn main(_argv: &[String]) -> util::Result<i32> {
    let root = util::repo_root();
    let mut problems = Vec::new();
    for crate_dir in sorted_entries(&root.join("crates")) {
        if crate_dir.join("Cargo.toml").is_file() {
            problems.extend(check_crate(&root, &crate_dir));
        }
    }
    for p in &problems {
        println!("{p}");
    }
    if !problems.is_empty() {
        return Ok(1);
    }
    println!("test targets OK");
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flags_a_stray_binary_and_an_unlisted_module() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let krate = root.join("crates").join("ff-core");
        let it = krate.join("tests").join("it");
        std::fs::create_dir_all(&it).unwrap();
        std::fs::write(krate.join("Cargo.toml"), "").unwrap();
        std::fs::write(it.join("main.rs"), "mod listed;\n#[cfg(x)] pub mod also;\n").unwrap();
        std::fs::write(it.join("listed.rs"), "#[path = \"pulled.rs\"] mod p;").unwrap();
        std::fs::write(it.join("also.rs"), "").unwrap();
        std::fs::write(it.join("pulled.rs"), "").unwrap();
        std::fs::write(it.join("orphan.rs"), "").unwrap();
        std::fs::write(krate.join("tests").join("stray.rs"), "").unwrap();
        std::fs::write(krate.join("tests").join("data_baked.rs"), "").unwrap();
        let problems = check_crate(root, &krate);
        assert_eq!(problems.len(), 2, "{problems:?}");
        assert!(problems[0].starts_with("crates/ff-core/tests/stray.rs is a test binary"));
        assert!(problems[1].starts_with("crates/ff-core/tests/it/orphan.rs is not a `mod`"));
    }
}
