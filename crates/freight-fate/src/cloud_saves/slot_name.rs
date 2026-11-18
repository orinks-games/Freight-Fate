use ff_core::models::profile::CREATED_LINE;

use super::save_slot_name;

fn cloud_slot_suffix(line: &str) -> Option<String> {
    let (major, minor) = line.split_once('.')?;
    let release = (major.parse::<u64>().ok()?, minor.parse::<u64>().ok()?);
    (release > (1, 9)).then(|| format!(" (Career {})", release.0))
}

/// The remote backup key, isolated from careers on earlier release lines.
pub fn cloud_slot_name(profile_name: &str) -> String {
    let name = save_slot_name(profile_name);
    match cloud_slot_suffix(CREATED_LINE) {
        Some(suffix) => format!("{name}{suffix}"),
        None => name,
    }
}

/// The local career stem represented by this line's remote key or a 1.9 key.
pub fn local_name_for_slot(slot: &str) -> &str {
    match cloud_slot_suffix(CREATED_LINE) {
        Some(suffix) => slot.strip_suffix(&suffix).unwrap_or(slot),
        None => slot,
    }
}

#[cfg(test)]
mod tests {
    use super::cloud_slot_suffix;

    #[test]
    fn cloud_suffix_follows_the_numeric_release_line() {
        for (line, expected) in [
            ("1.8", None),
            ("1.9", None),
            ("1.10", Some(" (Career 1)")),
            ("2.0", Some(" (Career 2)")),
            ("20.0", Some(" (Career 20)")),
            ("", None),
            ("future", None),
            ("2.x", None),
            ("2.0.1", None),
        ] {
            assert_eq!(cloud_slot_suffix(line).as_deref(), expected, "{line}");
        }
    }
}
