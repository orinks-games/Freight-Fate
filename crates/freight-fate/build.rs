//! Stage the vendored SDL2 import library and DLL next to the build output.
//!
//! SDL2 is linked dynamically against the prebuilt libsdl-org release under
//! `vendor/sdl2/<os>-<arch>/`; building SDL from source needs a CMake/VS
//! pairing this machine does not have, and the prebuilt links in seconds.
//!
//! That is a Windows arrangement. macOS and Linux have no vendored SDL2: the
//! crate's `bundled` + `static-link` features compile it into the executable
//! (see Cargo.toml for why each platform does).
use std::{env, fs, path::PathBuf};

fn main() {
    pass_on_hearken_link_args();
    link_prism_system_libraries();
    build_ios_bridge();
    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let root = manifest.ancestors().nth(2).unwrap().to_path_buf();
    write_catalog_list(&root);
    let os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let arch = env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
    let dir = root
        .join("vendor")
        .join("sdl2")
        .join(format!("{os}-{arch}"));
    println!("cargo:rerun-if-changed={}", dir.display());
    if !dir.is_dir() {
        // macOS and Linux are not vendored on purpose: SDL2 is compiled in
        // statically there (see Cargo.toml), so there is nothing missing and
        // nothing to warn about.
        if os != "macos" && os != "linux" && os != "ios" {
            println!("cargo:warning=freight-fate: no vendored SDL2 for {os}-{arch} under vendor/sdl2; expecting a system SDL2");
        }
        return;
    }
    println!("cargo:rustc-link-search=native={}", dir.display());
    let out = PathBuf::from(env::var("OUT_DIR").unwrap());
    if let Some(profile) = out.ancestors().nth(3) {
        for entry in fs::read_dir(&dir).into_iter().flatten().flatten() {
            let p = entry.path();
            let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("");
            if matches!(ext, "dll" | "so" | "dylib") {
                let _ = fs::copy(&p, profile.join(p.file_name().unwrap()));
            }
        }
    }
}

/// Apply the link arguments Prism needs, as Hearken publishes them, to the
/// game's final link (copied from Echo Parlor's build.rs).
/// hearken-speech works out the arguments (none since Prism 0.1.4) and the
/// `hearken` facade passes them on; a link argument only reaches the final
/// link, which is this crate's, so a future argument reaches it with no
/// change here.
fn pass_on_hearken_link_args() {
    println!("cargo:rerun-if-env-changed=DEP_HEARKEN_LINK_ARGS");
    match env::var("DEP_HEARKEN_LINK_ARGS") {
        Ok(args) => {
            for arg in args.split(';').filter(|arg| !arg.is_empty()) {
                println!("cargo:rustc-link-arg={arg}");
            }
        }
        // Cargo hands a build script this list only from a direct dependency.
        Err(_) => println!(
            "cargo:warning=Prism's link arguments are missing. Fix: add hearken to this \
             package's [dependencies]."
        ),
    }
}

/// Link what Prism and SDL2 import on iOS.
///
/// On macOS and Linux hearken-speech's build script links Prism's system
/// libraries (the macOS frameworks; glibmm, giomm and a static libstdc++ on
/// Linux), and those reach this crate's final link through Cargo. It has no
/// iOS branch, so the iOS frameworks stay here.
fn link_prism_system_libraries() {
    if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("ios") {
        // Prism (VoiceOver announcements, AVSpeech) and the SDL2 UIKit
        // backend compiled in by sdl2-sys, whose static link names only
        // the macOS frameworks.
        for framework in [
            "Foundation",
            "UIKit",
            "AVFoundation",
            "AudioToolbox",
            "CoreAudio",
            "CoreBluetooth",
            "CoreFoundation",
            "CoreGraphics",
            "CoreHaptics",
            "CoreMotion",
            "GameController",
            "Metal",
            "OpenGLES",
            "QuartzCore",
            "Security",
        ] {
            println!("cargo:rustc-link-lib=framework={framework}");
        }
        println!("cargo:rustc-link-lib=objc");
        println!("cargo:rustc-link-lib=iconv");
        println!("cargo:rustc-link-lib=c++");
        link_clang_runtime();
    }
}

/// Compile the UIKit gesture and VoiceOver bridge (`ios/ff_touch.m`).
///
/// It is the one piece of the iOS port that has to be Objective-C: a view
/// over SDL's own, carrying the gesture recognizers and the accessibility
/// element VoiceOver focuses. Everything it sees is queued as a plain gesture
/// code that `touch` turns into the same key events the desktop game reads.
fn build_ios_bridge() {
    let source = "ios/ff_touch.m";
    println!("cargo:rerun-if-changed={source}");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("ios") {
        return;
    }
    cc::Build::new()
        .file(source)
        .flag("-fobjc-arc")
        .flag("-fmodules")
        .compile("ff_touch");
}

/// Link clang's own runtime for iOS. SDL2's and Prism's `@available` checks
/// compile to `__isPlatformVersionAtLeast`, which lives there, and rustc links
/// with `-nodefaultlibs`, so clang does not add it by itself.
fn link_clang_runtime() {
    let simulator = env::var("CARGO_CFG_TARGET_ABI").as_deref() == Ok("sim");
    let Ok(output) = std::process::Command::new("xcrun")
        .args(["clang", "-print-resource-dir"])
        .output()
    else {
        println!("cargo:warning=freight-fate: xcrun clang not found; not linking clang_rt");
        return;
    };
    let resource_dir = String::from_utf8_lossy(&output.stdout).trim().to_string();
    // A fat archive, which rustc cannot bundle into the rlib, so it goes to
    // the final link directly.
    println!(
        "cargo:rustc-link-arg={resource_dir}/lib/darwin/libclang_rt.{}.a",
        if simulator { "iossim" } else { "ios" }
    );
}

/// Compile every `lang/*.po` that translates something into the game, so a
/// translator's new language needs its file and nothing else:
/// `$OUT_DIR/catalogs.rs` is the `(code, text)` list `app::language`
/// includes. A file with no translations yet is left out: Settings would not
/// offer it, and each one is most of a megabyte of English in the executable.
fn write_catalog_list(root: &std::path::Path) {
    let lang = root.join("lang");
    println!("cargo:rerun-if-changed={}", lang.display());
    let mut codes: Vec<(String, PathBuf)> = fs::read_dir(&lang)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|e| e.to_str()) == Some("po"))
        .filter_map(|path| {
            let code = path.file_stem()?.to_str()?.to_string();
            Some((code, path))
        })
        .filter(|(_, path)| fs::read_to_string(path).is_ok_and(|text| translates_something(&text)))
        .collect();
    codes.sort();
    let mut out = String::from("&[\n");
    for (code, path) in &codes {
        println!("cargo:rerun-if-changed={}", path.display());
        out.push_str(&format!(
            "    ({code:?}, include_str!({:?})),\n",
            path.display().to_string()
        ));
    }
    out.push(']');
    let dest = PathBuf::from(env::var("OUT_DIR").unwrap()).join("catalogs.rs");
    fs::write(dest, out).expect("catalog list written");
}

/// Whether a PO file has any entry past the header with a non-empty
/// translation, on one `msgstr` line or its continuation lines.
fn translates_something(po: &str) -> bool {
    let mut past_header = false;
    let mut in_msgstr = false;
    for line in po.lines().map(str::trim) {
        if line.starts_with("msgid ") {
            in_msgstr = false;
        } else if let Some(rest) = line.strip_prefix("msgstr") {
            // `msgstr "..."` or a plural form, `msgstr[0] "..."`.
            let text = rest.trim_start_matches(|c: char| c != '"');
            if past_header && text.len() > 2 {
                return true;
            }
            in_msgstr = true;
        } else if line.starts_with('"') {
            if in_msgstr && past_header && line.len() > 2 {
                return true;
            }
        } else if line.is_empty() && in_msgstr {
            past_header = true;
            in_msgstr = false;
        }
    }
    false
}
