//! `prune-snapshots`: delete old Career 1.9 tester snapshot releases, keeping
//! the newest few.
//!
//! Only releases tagged `1.9-tester-YYYYMMDD` are considered: stable
//! releases, the `nightly-*` bridge for 1.8 players and anything else are
//! never touched. The git tags stay, because the snapshot workflow writes
//! each build's notes against the previous `1.9-tester-*` tag, and a kept
//! tag keeps the history. Needs `gh` and `GH_TOKEN`.

use std::process::Command;

use crate::cli::{Args, Spec};
use crate::pyjson;
use crate::util::{self, Result};

const DEFAULT_KEEP: usize = 7;

fn is_tester_tag(tag: &str) -> bool {
    tag.strip_prefix("1.9-tester-")
        .is_some_and(|d| d.len() == 8 && d.bytes().all(|b| b.is_ascii_digit()))
}

/// The tester snapshot tags beyond the newest `keep`, newest first.
pub fn releases_to_prune(tags: &[String], keep: usize) -> Result<Vec<String>> {
    if keep < 1 {
        return Err("keep must be at least 1".into());
    }
    let mut testers: Vec<String> = tags.iter().filter(|t| is_tester_tag(t)).cloned().collect();
    testers.sort();
    testers.dedup();
    testers.reverse();
    Ok(testers.into_iter().skip(keep).collect())
}

fn list_release_tags() -> Result<Vec<String>> {
    let out = util::output(
        Command::new("gh").args(["release", "list", "--limit", "1000", "--json", "tagName"]),
    )?;
    let parsed = pyjson::loads(&out).map_err(|e| format!("gh release list: {e}"))?;
    Ok(parsed
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|r| {
                    r.get("tagName")
                        .and_then(|t| t.as_str())
                        .map(str::to_string)
                })
                .collect()
        })
        .unwrap_or_default())
}

pub fn main(argv: &[String]) -> Result<i32> {
    let args = Args::parse(
        argv,
        &Spec {
            values: &["keep"],
            flags: &["dry-run"],
        },
    )
    .map_err(|e| e.to_string())?;
    let keep = args
        .number("keep", DEFAULT_KEEP)
        .map_err(|e| e.to_string())?;
    let dry_run = args.flag("dry-run");
    let doomed = releases_to_prune(&list_release_tags()?, keep)?;
    if doomed.is_empty() {
        println!("Nothing to prune; {keep} or fewer tester snapshots.");
        return Ok(0);
    }
    let mut failed = 0;
    for tag in doomed {
        println!(
            "{} release {tag} (tag kept)",
            if dry_run { "Would delete" } else { "Deleting" }
        );
        if dry_run {
            continue;
        }
        let ok = Command::new("gh")
            .args(["release", "delete", &tag, "--yes"])
            .status()
            .is_ok_and(|s| s.success());
        if !ok {
            failed += 1;
        }
    }
    Ok(if failed > 0 { 1 } else { 0 })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_the_newest_testers_and_ignores_other_tags() {
        let tags: Vec<String> = [
            "1.9-tester-20261001",
            "v1.9.4",
            "1.9-tester-20261003",
            "nightly-20260601",
            "1.9-tester-20261002",
            "1.9-tester-2026100",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        assert_eq!(
            releases_to_prune(&tags, 1).unwrap(),
            vec!["1.9-tester-20261002", "1.9-tester-20261001"]
        );
        assert!(releases_to_prune(&tags, 0).is_err());
    }
}
