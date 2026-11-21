//! Builds and links the vendored prism library.

use std::{
	env, fs,
	path::{Path, PathBuf},
};

use cmake::Config;

/// The Windows system libraries prism links itself, in `PrismPlatformWindows.cmake`.
///
/// A static prism leaves these to the final link too, and nothing else names them there. An
/// application only linked them by luck, when another dependency happened to pull them in;
/// otherwise it failed with unresolved UI Automation, RPC and Windows Runtime symbols.
const WINDOWS_SYSTEM_LIBS: &[&str] = &["ole32", "onecore", "runtimeobject", "uiautomationcore", "rpcrt4", "powrprof"];

fn main() {
	println!("cargo:rerun-if-env-changed=PRISM_LIB_DIR");
	let static_link = env::var("CARGO_FEATURE_STATIC").is_ok();
	let vendored = env::var("PRISM_LIB_DIR").map_or_else(
		|_| Some(build_vendored(static_link)),
		|dir| {
			println!("cargo:rustc-link-search=native={dir}");
			None
		},
	);
	let kind = if static_link { "static" } else { "dylib" };
	println!("cargo:rustc-link-lib={kind}=prism");
	if static_link {
		if let Some(dst) = vendored {
			link_pkg_config_deps(&dst);
		}
		link_cpp_runtime();
		link_windows_system_libs();
		link_apple_frameworks();
	}
}

/// Links the Windows system libraries prism imports, which a shared prism
/// links itself and a static one leaves to whoever is doing the final link.
fn link_windows_system_libs() {
	if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
		return;
	}
	for lib in WINDOWS_SYSTEM_LIBS {
		println!("cargo:rustc-link-lib=dylib={lib}");
	}
}

/// Links the pkg-config modules prism builds against on Linux, such as giomm, which a static prism also leaves to the final link.
///
/// They're read from the `CMake` package prism installs, since which ones it uses depends on what the build machine has.
fn link_pkg_config_deps(dst: &Path) {
	let Ok(config) = fs::read_to_string(dst.join("share/prism/prism-config.cmake")) else {
		return;
	};
	for line in config.lines().filter(|line| line.trim_start().starts_with("pkg_check_modules(")) {
		let Some(spec) = line.split('"').nth(1) else {
			continue;
		};
		let name = spec.split(['>', '<', '=']).next().unwrap_or(spec);
		let mut probe = pkg_config::Config::new();
		if let Some(version) = spec[name.len()..].strip_prefix(">=") {
			let _ = probe.atleast_version(version);
		}
		if let Err(e) = probe.probe(name) {
			panic!("a static prism needs {spec} from pkg-config: {e}");
		}
	}
	if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("linux") {
		println!("cargo:rustc-link-lib=dylib=dl");
	}
}

/// Links the frameworks prism links on Apple platforms, in `PrismPlatformApple.cmake`, which a static prism leaves to the final link too.
fn link_apple_frameworks() {
	let platform: &[&str] = match env::var("CARGO_CFG_TARGET_OS").as_deref() {
		Ok("macos") => &["AppKit", "CoreServices", "Security", "IOKit", "CoreFoundation"],
		Ok("ios" | "tvos" | "visionos") => &["UIKit"],
		Ok("watchos") => &["WatchKit"],
		_ => return,
	};
	for framework in ["Foundation", "AVFoundation"].iter().chain(platform) {
		println!("cargo:rustc-link-lib=framework={framework}");
	}
	// prism's Objective-C++ uses ARC, whose runtime functions clang adds only when it does the final link itself.
	println!("cargo:rustc-link-lib=dylib=objc");
}

fn build_vendored(static_link: bool) -> PathBuf {
	let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("cargo sets CARGO_MANIFEST_DIR"));
	let source = manifest_dir.join("prism");
	assert!(
		source.join("CMakeLists.txt").exists(),
		"vendored prism source not found at {}; run `git submodule update --init` \
		 or set PRISM_LIB_DIR to a directory containing a prebuilt prism library",
		source.display()
	);
	println!("cargo:rerun-if-changed=prism/CMakeLists.txt");
	println!("cargo:rerun-if-changed=prism/cmake");
	println!("cargo:rerun-if-changed=prism/source");
	println!("cargo:rerun-if-changed=prism/include");
	let mut cfg = Config::new(&source);
	let _ = cfg.define("BUILD_SHARED_LIBS", if static_link { "OFF" } else { "ON" });
	if env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
		// prism defaults to the static CRT, but the objects must use the same
		// CRT rustc links: the non-debug one, static or dynamic per the
		// crt-static target feature.
		let crt_static =
			env::var("CARGO_CFG_TARGET_FEATURE").is_ok_and(|features| features.split(',').any(|f| f == "crt-static"));
		let _ = cfg.define("CMAKE_MSVC_RUNTIME_LIBRARY", if crt_static { "MultiThreaded" } else { "MultiThreadedDLL" });
	}
	let dst = cfg.build();
	println!("cargo:rustc-link-search=native={}", dst.join("lib").display());
	println!("cargo:rustc-link-search=native={}", dst.join("bin").display());
	dst
}

fn link_cpp_runtime() {
	let os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
	let target_env = env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
	match (os.as_str(), target_env.as_str()) {
		// MSVC objects carry /DEFAULTLIB directives for their C++ runtime.
		(_, "msvc") => {}
		("macos" | "ios" | "tvos" | "watchos" | "visionos", _) => println!("cargo:rustc-link-lib=c++"),
		("android", _) => println!("cargo:rustc-link-lib=c++_shared"),
		_ => println!("cargo:rustc-link-lib=stdc++"),
	}
}
