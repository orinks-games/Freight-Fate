//! The macOS app bundle: Info.plist, library relocation, the native
//! dependency audit, Developer ID signing and notarization.

use std::path::{Path, PathBuf};
use std::process::Command;

use super::plist::{self, Plist};
use super::{label_package_version, APP_NAME};
use crate::pyjson;
use crate::util::{self, Result};

const SIGN_IDENTITY_ENV: &str = "MACOS_SIGN_IDENTITY";
const SIGN_KEYCHAIN_ENV: &str = "MACOS_SIGN_KEYCHAIN";
const NOTARY_ENV: [&str; 3] = [
    "MACOS_NOTARY_KEY_PATH",
    "MACOS_NOTARY_KEY_ID",
    "MACOS_NOTARY_ISSUER_ID",
];

fn env_trimmed(name: &str) -> String {
    std::env::var(name).unwrap_or_default().trim().to_string()
}

/// The install names recorded in a Mach-O file (`otool -L`). A fat Mach-O
/// repeats an unindented `<file> (architecture ...):` header per slice; only
/// the indented rows are install names.
pub fn linked_libraries(executable: &Path) -> Result<Vec<String>> {
    let out = util::output(Command::new("otool").arg("-L").arg(executable))?;
    let mut names = Vec::new();
    for line in out.lines() {
        if !line.starts_with(char::is_whitespace) {
            continue;
        }
        let name = line.trim().split(" (").next().unwrap_or("").to_string();
        if !name.is_empty() && !names.contains(&name) {
            names.push(name);
        }
    }
    Ok(names)
}

/// The SDL2 install name recorded in `executable`, if any. `None` is the
/// only shippable answer: since Homebrew retired the real SDL2, a dynamic
/// SDL2 is sdl2-compat, which loads SDL3 where no audit can see it and dies
/// on every player Mac without Homebrew ("failed to load sdl3", 2026-08-30).
pub fn dynamic_sdl_dependency(executable: &Path) -> Result<Option<String>> {
    Ok(linked_libraries(executable)?.into_iter().find(|n| {
        let leaf = Path::new(n)
            .file_name()
            .map(|f| f.to_string_lossy().into_owned())
            .unwrap_or_default();
        leaf.starts_with("libSDL2") && leaf.ends_with(".dylib")
    }))
}

/// A numeric Finder build identity, including the snapshot date.
pub fn bundle_version(label: &str) -> String {
    if let Some(date) = label.strip_prefix("1.9-tester-") {
        if date.len() == 8 && date.bytes().all(|b| b.is_ascii_digit()) {
            return format!("{}.{}.{}", &date[..4], &date[4..6], &date[6..]);
        }
    }
    let numeric: Vec<&str> = label
        .trim_start_matches('v')
        .split('.')
        .filter(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
        .collect();
    if numeric.is_empty() {
        "1".into()
    } else {
        numeric.join(".")
    }
}

/// Write the minimal metadata Finder and assistive technology need.
pub fn write_info_plist(app: &Path, label: &str) -> Result<()> {
    let version = label_package_version(label)?;
    let short = version.split(".dev").next().unwrap_or(&version).to_string();
    let info = Plist::dict(vec![
        ("CFBundleDevelopmentRegion", Plist::string("en")),
        ("CFBundleDisplayName", Plist::string("Freight Fate")),
        ("CFBundleExecutable", Plist::string(APP_NAME)),
        (
            "CFBundleIdentifier",
            Plist::string("net.orinks.freight-fate"),
        ),
        ("CFBundleInfoDictionaryVersion", Plist::string("6.0")),
        (
            "CFBundleGetInfoString",
            Plist::string(format!("Freight Fate {short} ({label})")),
        ),
        ("CFBundleName", Plist::string("Freight Fate")),
        ("CFBundlePackageType", Plist::string("APPL")),
        ("CFBundleShortVersionString", Plist::string(short.clone())),
        ("CFBundleVersion", Plist::string(bundle_version(label))),
    ]);
    // No NSAppleEventsUsageDescription, on purpose: without it macOS refuses
    // Prism's AppleScript route to VoiceOver at once instead of asking. The
    // question blocked the game's main thread until answered, freezing the
    // game at launch (issue 266). Speech still reaches VoiceOver through
    // accessibility announcements, which need no permission.
    util::write_text(
        &app.join("Contents").join("Info.plist"),
        &plist::to_xml(&info),
    )
}

/// Mach-O files whose dependencies must stay inside the app or macOS.
pub fn bundle_binaries(app: &Path) -> Vec<PathBuf> {
    let mut out = vec![app.join("Contents").join("MacOS").join(APP_NAME)];
    let mut dylibs: Vec<PathBuf> = util::walk_files(&app.join("Contents").join("Frameworks"))
        .into_iter()
        .filter(|p| p.extension().is_some_and(|e| e == "dylib"))
        .collect();
    util::sort_paths(&mut dylibs);
    out.extend(dylibs);
    out
}

fn system_install_name(name: &str) -> bool {
    [
        "@rpath/",
        "@loader_path/",
        "@executable_path/",
        "/usr/lib/",
        "/System/Library/",
    ]
    .iter()
    .any(|p| name.starts_with(p))
}

fn leaf(name: &str) -> String {
    Path::new(name)
        .file_name()
        .map(|f| f.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Rewrite every bundled dependency away from the builder's filesystem.
pub fn relocate_libraries(app: &Path) -> Result<()> {
    let executable = app.join("Contents").join("MacOS").join(APP_NAME);
    let bundled: Vec<String> = bundle_binaries(app)
        .iter()
        .skip(1)
        .map(|p| leaf(&p.to_string_lossy()))
        .collect();
    for binary in bundle_binaries(app) {
        if binary.extension().is_some_and(|e| e == "dylib") {
            util::run(
                Command::new("install_name_tool")
                    .arg("-id")
                    .arg(format!("@rpath/{}", leaf(&binary.to_string_lossy())))
                    .arg(&binary),
            )?;
        }
        for dependency in linked_libraries(&binary)? {
            if system_install_name(&dependency) {
                continue;
            }
            let name = leaf(&dependency);
            if bundled.contains(&name) {
                util::run(
                    Command::new("install_name_tool")
                        .arg("-change")
                        .arg(&dependency)
                        .arg(format!("@rpath/{name}"))
                        .arg(&binary),
                )?;
            }
        }
    }
    util::run(
        Command::new("install_name_tool")
            .args(["-add_rpath", "@executable_path/../Frameworks"])
            .arg(&executable),
    )
}

/// Reject any Mach-O dependency the player's Mac cannot satisfy: an absolute
/// path into the builder's disk (the classic Homebrew leak), or an `@rpath`
/// name whose dylib was never bundled.
pub fn verify_native_dependencies(app: &Path) -> Result<()> {
    let binaries = bundle_binaries(app);
    let bundled: Vec<String> = binaries
        .iter()
        .skip(1)
        .map(|p| leaf(&p.to_string_lossy()))
        .collect();
    let mut forbidden = Vec::new();
    for binary in &binaries {
        let own = leaf(&binary.to_string_lossy());
        for dependency in linked_libraries(binary)? {
            if dependency.starts_with('/') && !system_install_name(&dependency) {
                forbidden.push(format!("{own}: {dependency}"));
            } else if dependency.starts_with("@rpath/") {
                let name = leaf(&dependency);
                if name != own && !bundled.contains(&name) {
                    forbidden.push(format!("{own}: {dependency} (not bundled)"));
                }
            }
        }
    }
    if !forbidden.is_empty() {
        return Err(format!(
            "macOS app contains a builder-local dependency: {}",
            forbidden.join("; ")
        ));
    }
    Ok(())
}

fn developer_id_codesign(identity: &str, target: &Path, keychain: &str) -> Command {
    let mut cmd = Command::new("codesign");
    cmd.args(["--force", "--options", "runtime", "--timestamp"]);
    if !keychain.is_empty() {
        cmd.args(["--keychain", keychain]);
    }
    cmd.args(["--sign", identity]).arg(target);
    cmd
}

/// Sign the finished app bundle: Developer ID when configured, else ad hoc.
pub fn sign_distribution(app: &Path) -> Result<()> {
    verify_native_dependencies(app)?;
    let identity = env_trimmed(SIGN_IDENTITY_ENV);
    if identity.is_empty() {
        util::run(
            Command::new("codesign")
                .args(["--force", "--deep", "--sign", "-"])
                .arg(app),
        )?;
    } else {
        let keychain = env_trimmed(SIGN_KEYCHAIN_ENV);
        // Inside out: nested libraries first, then the bundle seals them.
        // Notarization rejects --deep signing, so each dylib is signed alone.
        let executable = app.join("Contents").join("MacOS").join(APP_NAME);
        for binary in bundle_binaries(app) {
            if binary != executable {
                util::run(&mut developer_id_codesign(&identity, &binary, &keychain))?;
            }
        }
        util::run(&mut developer_id_codesign(&identity, app, &keychain))?;
    }
    util::run(
        Command::new("codesign")
            .args(["--verify", "--deep", "--strict"])
            .arg(app),
    )
}

/// Notarize and staple a Developer ID signed app, when credentials are set.
pub fn notarize_distribution(app: &Path) -> Result<()> {
    let values: Vec<String> = NOTARY_ENV.iter().map(|n| env_trimmed(n)).collect();
    if values.iter().all(String::is_empty) {
        println!("Skipped notarization: no App Store Connect key configured.");
        return Ok(());
    }
    if values.iter().any(String::is_empty) {
        let missing: Vec<&str> = NOTARY_ENV
            .iter()
            .zip(&values)
            .filter(|(_, v)| v.is_empty())
            .map(|(n, _)| *n)
            .collect();
        return Err(format!(
            "notarization is half configured; missing {}",
            missing.join(", ")
        ));
    }
    if env_trimmed(SIGN_IDENTITY_ENV).is_empty() {
        return Err("notarization needs a Developer ID signature; set MACOS_SIGN_IDENTITY".into());
    }
    let (key_path, key_id, issuer) = (&values[0], &values[1], &values[2]);
    let scratch = tempfile::tempdir().map_err(|e| e.to_string())?;
    let stem = app.file_stem().unwrap_or_default().to_string_lossy();
    let upload = scratch.path().join(format!("{stem}-notarize.zip"));
    util::run(
        Command::new("ditto")
            .args(["-c", "-k", "--keepParent"])
            .arg(app)
            .arg(&upload),
    )?;
    let result = Command::new("xcrun")
        .args(["notarytool", "submit"])
        .arg(&upload)
        .args(["--key", key_path, "--key-id", key_id, "--issuer", issuer])
        .args(["--wait", "--timeout", "45m", "--output-format", "json"])
        .output()
        .map_err(|e| format!("could not run xcrun notarytool: {e}"))?;
    let submission =
        pyjson::loads(&String::from_utf8_lossy(&result.stdout)).unwrap_or(pyjson::Value::Null);
    let field = |k: &str| {
        submission
            .get(k)
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string()
    };
    let status = field("status");
    if status != "Accepted" {
        let id = field("id");
        if !id.is_empty() {
            let _ = Command::new("xcrun")
                .args(["notarytool", "log", &id])
                .args(["--key", key_path, "--key-id", key_id, "--issuer", issuer])
                .status();
        }
        return Err(format!(
            "Apple notarization returned {} (exit {}): {}",
            if status.is_empty() {
                "no status"
            } else {
                &status
            },
            result.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&result.stderr).trim()
        ));
    }
    println!("Notarization accepted: {}", field("id"));
    util::run(Command::new("xcrun").args(["stapler", "staple"]).arg(app))?;
    util::run(Command::new("xcrun").args(["stapler", "validate"]).arg(app))?;
    util::run(
        Command::new("spctl")
            .args(["--assess", "--type", "execute", "--verbose=2"])
            .arg(app),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundle_versions() {
        assert_eq!(bundle_version("1.9-tester-20261017"), "2026.10.17");
        assert_eq!(bundle_version("v1.9.4.1"), "1.9.4.1");
        assert_eq!(bundle_version("1.9.0.dev0"), "1.9.0");
        assert_eq!(bundle_version("nightly-x"), "1");
    }
}
