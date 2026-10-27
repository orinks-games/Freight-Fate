//! Application-local MSVC runtime staging for the Windows x64 portable build,
//! and the import audit that proves the payload starts on a clean Windows.
//!
//! Only Visual Studio's redistributable CRT directory is a source. Never copy
//! from System32, a Python environment, or an arbitrary DLL download site.
//! See <https://learn.microsoft.com/en-us/cpp/windows/redistributing-visual-cpp-files>
//! and <https://learn.microsoft.com/en-us/windows/win32/debug/pe-format>.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use regex::Regex;

use crate::util::{self, rel, Result};

/// msvcp140_atomic_wait.dll: Prism, linked into the executable, uses C++20
/// atomic waits.
pub const REQUIRED_CRT: &[&str] = &[
    "vcruntime140.dll",
    "vcruntime140_1.dll",
    "msvcp140.dll",
    "msvcp140_atomic_wait.dll",
];

/// What a clean Windows already has. An API set (`api-ms-win-`,
/// `ext-ms-win-`) is resolved by the loader from the OS schema, never from a
/// file on disk, and the classic DLLs below all ship in System32 on every
/// supported Windows -- including the Universal CRT (`api-ms-win-crt-*`),
/// which has been a Windows component since Windows 10.
/// <https://learn.microsoft.com/en-us/windows/win32/apiindex/windows-apisets>
/// <https://learn.microsoft.com/en-us/cpp/windows/universal-crt-deployment>
pub const WINDOWS_SYSTEM_DLLS: &[&str] = &[
    "advapi32.dll",
    "bcrypt.dll",
    "bcryptprimitives.dll",
    // COM's own runtime since Windows 8; the sound output reaches WASAPI
    // through it. Microsoft lists it as the DLL behind the COM functions,
    // e.g. https://learn.microsoft.com/en-us/windows/win32/api/combaseapi/nf-combaseapi-coincrementmtausage
    "combase.dll",
    "comctl32.dll",
    "comdlg32.dll",
    "crypt32.dll",
    "dbghelp.dll",
    "dwmapi.dll",
    "gdi32.dll",
    "imm32.dll",
    "iphlpapi.dll",
    "kernel32.dll",
    "msacm32.dll",
    // The Core Audio device API: cpal opens WASAPI devices through
    // ActivateAudioInterfaceAsync, which Microsoft lists in Mmdevapi.dll,
    // https://learn.microsoft.com/en-us/windows/win32/api/mmdeviceapi/nf-mmdeviceapi-activateaudiointerfaceasync
    "mmdevapi.dll",
    "msvcrt.dll", // the OS's own CRT, not a redistributable one
    "ntdll.dll",
    "ole32.dll",
    "oleaut32.dll",
    "powrprof.dll",
    "propsys.dll",
    "rpcrt4.dll",
    "secur32.dll",
    "setupapi.dll",
    "shcore.dll",
    "shell32.dll",
    "shlwapi.dll",
    "uiautomationcore.dll",
    "user32.dll",
    "userenv.dll",
    "uxtheme.dll",
    "version.dll",
    "windowscodecs.dll",
    "winmm.dll",
    "ws2_32.dll",
    "wtsapi32.dll",
    "xmllite.dll",
];

fn api_set() -> Regex {
    Regex::new(r"(?i)^(?:api|ext)-ms-win-[a-z0-9-]+-l\d+-\d+-\d+\.dll$").expect("valid regex")
}

fn vc_dll() -> Regex {
    Regex::new(r"(?i)^(?:vcruntime|msvcp|msvcr|concrt|vcomp)\d[^/\\]*\.dll$").expect("valid regex")
}

/// Whether a file name is one of the Visual C++ runtime family.
pub fn is_vc_dll(name: &str) -> bool {
    vc_dll().is_match(name)
}

fn version_key(path: &Path) -> Vec<u64> {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    Regex::new(r"\d+")
        .expect("valid regex")
        .find_iter(&name)
        .filter_map(|m| m.as_str().parse().ok())
        .collect()
}

fn children_newest_first(dir: &Path, prefix: &str, suffix: &str) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            let name = p
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            name.starts_with(prefix) && name.ends_with(suffix)
        })
        .collect();
    found.sort_by_key(|p| std::cmp::Reverse(version_key(p)));
    found
}

/// Prefer the active toolchain, otherwise discover the latest installed VS.
pub fn find_crt_directory() -> Result<PathBuf> {
    let mut roots: Vec<PathBuf> = Vec::new();
    if let Ok(configured) = std::env::var("VCToolsRedistDir") {
        if !configured.is_empty() {
            roots.push(PathBuf::from(configured));
        }
    }
    if roots.is_empty() {
        let vc = std::env::var("VCINSTALLDIR").ok().filter(|s| !s.is_empty());
        let vs = std::env::var("VSINSTALLDIR").ok().filter(|s| !s.is_empty());
        if vc.is_some() || vs.is_some() {
            let base = match vc {
                Some(vc) => PathBuf::from(vc),
                None => PathBuf::from(vs.unwrap_or_default()).join("VC"),
            };
            roots.extend(children_newest_first(
                &base.join("Redist").join("MSVC"),
                "",
                "",
            ));
        }
        if roots.is_empty() {
            let program_files = std::env::var("ProgramFiles(x86)")
                .unwrap_or_else(|_| r"C:\Program Files (x86)".to_string());
            let vswhere = PathBuf::from(program_files)
                .join("Microsoft Visual Studio")
                .join("Installer")
                .join("vswhere.exe");
            if vswhere.is_file() {
                let installation = util::output(Command::new(&vswhere).args([
                    "-latest",
                    "-products",
                    "*",
                    "-requires",
                    "Microsoft.VisualStudio.Component.VC.Tools.x86.x64",
                    "-property",
                    "installationPath",
                ]))?;
                let installation = installation.trim();
                if !installation.is_empty() {
                    roots.extend(children_newest_first(
                        &PathBuf::from(installation)
                            .join("VC")
                            .join("Redist")
                            .join("MSVC"),
                        "",
                        "",
                    ));
                }
            }
        }
    }
    for root in roots {
        for directory in children_newest_first(&root.join("x64"), "Microsoft.VC", ".CRT") {
            let files: BTreeSet<String> = std::fs::read_dir(&directory)
                .into_iter()
                .flatten()
                .flatten()
                .filter(|e| e.path().is_file())
                .map(|e| e.file_name().to_string_lossy().to_lowercase())
                .collect();
            if REQUIRED_CRT.iter().all(|name| files.contains(*name)) {
                return Ok(directory);
            }
        }
    }
    Err(
        "Cannot find a complete Visual Studio x64 redistributable CRT. Install the \
         Visual Studio C++ build tools, or set VCToolsRedistDir to the toolchain's \
         VC/Redist/MSVC/<version> directory. The portable build cannot ship without it."
            .to_string(),
    )
}

struct Pe<'a> {
    data: &'a [u8],
}

impl Pe<'_> {
    fn bytes(&self, offset: usize, len: usize) -> std::result::Result<&[u8], String> {
        self.data
            .get(offset..offset.checked_add(len).ok_or("truncated PE structure")?)
            .ok_or_else(|| "truncated PE structure".to_string())
    }

    fn u16(&self, offset: usize) -> std::result::Result<u16, String> {
        let b = self.bytes(offset, 2)?;
        Ok(u16::from_le_bytes([b[0], b[1]]))
    }

    fn u32(&self, offset: usize) -> std::result::Result<u32, String> {
        let b = self.bytes(offset, 4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    fn u64(&self, offset: usize) -> std::result::Result<u64, String> {
        let b = self.bytes(offset, 8)?;
        let mut a = [0u8; 8];
        a.copy_from_slice(b);
        Ok(u64::from_le_bytes(a))
    }
}

fn parse_imports(data: &[u8], delayed: bool) -> std::result::Result<BTreeSet<String>, String> {
    let pe = Pe { data };
    if data.get(..2) != Some(b"MZ") {
        return Err("missing DOS signature".into());
    }
    let header = pe.u32(0x3C)? as usize;
    if data.get(header..header + 4) != Some(b"PE\0\0") {
        return Err("missing PE signature".into());
    }
    let machine = pe.u16(header + 4)?;
    let section_count = pe.u16(header + 6)? as usize;
    if machine != 0x8664 {
        return Err(format!("expected x64 machine, got 0x{machine:04x}"));
    }
    let optional_size = pe.u16(header + 20)? as usize;
    let optional = header + 24;
    if optional_size < 112 || pe.u16(optional)? != 0x20B {
        return Err("expected PE32+ optional header".into());
    }
    let image_base = pe.u64(optional + 24)?;
    let directory_count = pe.u32(optional + 108)? as usize;
    if directory_count > (optional_size - 112) / 8 {
        return Err("data directories exceed optional header".into());
    }
    let mut sections = Vec::with_capacity(section_count);
    for index in 0..section_count {
        let offset = optional + optional_size + index * 40;
        let address = pe.u32(offset + 12)? as i64;
        let raw_size = pe.u32(offset + 16)? as i64;
        let raw_offset = pe.u32(offset + 20)? as i64;
        sections.push((address, raw_size, raw_offset));
    }
    let rva_offset = |rva: i64, size: i64| -> std::result::Result<usize, String> {
        for &(address, raw_size, raw_offset) in &sections {
            let delta = rva - address;
            if delta >= 0 && delta + size <= raw_size {
                let offset = raw_offset + delta;
                if offset + size <= data.len() as i64 {
                    return Ok(offset as usize);
                }
            }
        }
        Err(format!("unmapped RVA 0x{rva:x}"))
    };
    let dll_name = |rva: i64| -> std::result::Result<String, String> {
        let mut value = Vec::new();
        for index in 0..260 {
            let byte = data[rva_offset(rva + index, 1)?];
            if byte == 0 {
                let name = String::from_utf8(value)
                    .ok()
                    .filter(|n| n.is_ascii())
                    .ok_or("invalid imported DLL name")?
                    .to_lowercase();
                if name.is_empty() || name.contains('/') || name.contains('\\') {
                    return Err("invalid imported DLL name".into());
                }
                return Ok(name);
            }
            value.push(byte);
        }
        Err("unterminated DLL name".into())
    };

    let mut imports = BTreeSet::new();
    let tables: &[(usize, i64, usize)] = if delayed {
        &[(1, 20, 12), (13, 32, 4)]
    } else {
        &[(1, 20, 12)]
    };
    for &(index, size, name_field) in tables {
        if directory_count <= index {
            continue;
        }
        let rva = pe.u32(optional + 112 + index * 8)? as i64;
        let length = pe.u32(optional + 112 + index * 8 + 4)? as i64;
        if rva == 0 && length == 0 {
            continue;
        }
        if rva == 0 || length < size {
            return Err("invalid import directory".into());
        }
        let mut terminated = false;
        let mut delta = 0;
        while delta <= length - size {
            let offset = rva_offset(rva + delta, size)?;
            if data[offset..offset + size as usize].iter().all(|b| *b == 0) {
                terminated = true;
                break;
            }
            let mut name_rva = pe.u32(offset + name_field)? as i64;
            if index == 13 {
                let attributes = pe.u32(offset)?;
                if attributes & !1 != 0 {
                    return Err("unknown delay-import attributes".into());
                }
                if attributes & 1 == 0 {
                    name_rva -= image_base as i64;
                }
            }
            imports.insert(dll_name(name_rva)?);
            delta += size;
        }
        if !terminated {
            return Err("unterminated import directory".into());
        }
    }
    Ok(imports)
}

/// The DLLs an x64 PE imports; malformed inputs are refused.
///
/// `delayed` off returns only the NORMAL import table -- what the loader
/// must resolve before the process starts. A delay import is resolved on
/// first call, so a missing one is a feature that does not run rather than a
/// game that does not launch. This is a static runtime audit, not a general
/// Windows loader emulator: every read must map to bytes in the file.
pub fn pe_imports(path: &Path, delayed: bool) -> Result<BTreeSet<String>> {
    let data = std::fs::read(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    parse_imports(&data, delayed)
        .map_err(|e| format!("Cannot audit Windows binary {}: {e}", path.display()))
}

/// Prove the payload starts on a clean Windows: every VC++ runtime import
/// has an application-local DLL, and every other NORMAL import is a DLL
/// Windows itself ships, or one the payload carries.
pub fn verify_windows_runtime(root: &Path) -> Result<()> {
    let available: BTreeSet<String> = std::fs::read_dir(root)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().is_file())
        .map(|e| e.file_name().to_string_lossy().to_lowercase())
        .collect();
    let missing: Vec<&str> = REQUIRED_CRT
        .iter()
        .copied()
        .filter(|n| !available.contains(*n))
        .collect();
    if !missing.is_empty() {
        return Err(format!(
            "Windows payload is missing VC++ runtime: {}",
            missing.join(", ")
        ));
    }
    let api = api_set();
    let vc = vc_dll();
    let mut unprovided = Vec::new();
    for path in util::walk_files(root) {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let is_dll = path
            .extension()
            .is_some_and(|e| e.to_string_lossy().eq_ignore_ascii_case("dll"));
        if !is_dll && name != "FreightFate.exe" {
            continue;
        }
        for import in pe_imports(&path, true)? {
            if vc.is_match(&import) && !available.contains(&import) {
                return Err(format!(
                    "{} requires unbundled VC++ runtime {import}",
                    rel(&path, root)
                ));
            }
        }
        for import in pe_imports(&path, false)? {
            if available.contains(&import)
                || WINDOWS_SYSTEM_DLLS.contains(&import.as_str())
                || api.is_match(&import)
            {
                continue;
            }
            unprovided.push(format!("{} needs {import}", rel(&path, root)));
        }
    }
    // Every unprovided import at once, so one package build names them all.
    if !unprovided.is_empty() {
        return Err(format!(
            "{} before it can start, and neither Windows nor this payload provides it. \
             Ship the library beside the executable, or add it to WINDOWS_SYSTEM_DLLS \
             with the Microsoft page that says Windows includes it.",
            unprovided.join("; ")
        ));
    }
    Ok(())
}

/// Copy one complete official CRT family beside the executable and audit.
pub fn stage_windows_runtime(root: &Path) -> Result<()> {
    let source = find_crt_directory()?;
    let mut files: Vec<PathBuf> = std::fs::read_dir(&source)
        .map_err(|e| format!("cannot list {}: {e}", source.display()))?
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.is_file()
                && p.extension()
                    .is_some_and(|e| e.to_string_lossy().eq_ignore_ascii_case("dll"))
        })
        .collect();
    util::sort_paths(&mut files);
    for path in &files {
        pe_imports(path, true)?; // A wrong-architecture/corrupt redist must not be copied.
    }
    // Replace any CRT left in the Cargo directory; never mix runtime versions.
    for entry in std::fs::read_dir(root).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.is_file() && is_vc_dll(&entry.file_name().to_string_lossy()) {
            std::fs::remove_file(&path)
                .map_err(|e| format!("cannot remove {}: {e}", path.display()))?;
        }
    }
    let mut names = Vec::new();
    for path in &files {
        let name = path.file_name().unwrap_or_default();
        util::copy_file(path, &root.join(name))?;
        names.push(name.to_string_lossy().into_owned());
    }
    println!(
        "Staged Visual C++ runtime from {}: {}",
        source.display(),
        names.join(", ")
    );
    verify_windows_runtime(root)
}

/// The release's payload check refuses an executable that imports a DLL
/// neither Windows nor the payload provides, but it only runs in the
/// snapshot build. This runs the same check on the executable the PR CI's
/// Windows test build already compiled, so a new import fails the pull
/// request that brings it in. It stages what the release stages beside the
/// executable (the executable under its shipped name, the libraries the
/// build scripts left, and Visual Studio's CRT) into a scratch folder.
pub fn check_windows_imports(argv: &[String]) -> Result<i32> {
    let profile_dir = argv
        .first()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("target/debug"));
    let exe = profile_dir.join(crate::release::cargo_exe_name("win32"));
    if !exe.is_file() {
        return Err(format!(
            "no executable at {}; build the game first",
            exe.display()
        ));
    }
    let scratch = tempfile::tempdir().map_err(|e| e.to_string())?;
    let root = scratch.path();
    util::copy_file(&exe, &root.join(crate::release::rust_exe_name("win32")))?;
    for lib in crate::release::rust_native_libraries(&profile_dir, &["dll"]) {
        util::copy_file(&lib, &root.join(lib.file_name().unwrap_or_default()))?;
    }
    stage_windows_runtime(root)?;
    println!(
        "{} imports only DLLs Windows or the payload provides.",
        exe.display()
    );
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal PE32+ with one section and one normal import.
    fn tiny_pe(import: &str) -> Vec<u8> {
        let mut data = vec![0u8; 0x400];
        data[0..2].copy_from_slice(b"MZ");
        data[0x3C..0x40].copy_from_slice(&0x80u32.to_le_bytes());
        let pe = 0x80;
        data[pe..pe + 4].copy_from_slice(b"PE\0\0");
        data[pe + 4..pe + 6].copy_from_slice(&0x8664u16.to_le_bytes());
        data[pe + 6..pe + 8].copy_from_slice(&1u16.to_le_bytes());
        let optional_size: u16 = 112 + 16 * 8;
        data[pe + 20..pe + 22].copy_from_slice(&optional_size.to_le_bytes());
        let opt = pe + 24;
        data[opt..opt + 2].copy_from_slice(&0x20Bu16.to_le_bytes());
        data[opt + 108..opt + 112].copy_from_slice(&16u32.to_le_bytes());
        // Import directory at RVA 0x1000, two descriptors (one + terminator).
        data[opt + 112 + 8..opt + 112 + 12].copy_from_slice(&0x1000u32.to_le_bytes());
        data[opt + 112 + 12..opt + 112 + 16].copy_from_slice(&40u32.to_le_bytes());
        // Section: virtual 0x1000, raw size 0x200 at file offset 0x200.
        let sec = opt + optional_size as usize;
        data[sec + 12..sec + 16].copy_from_slice(&0x1000u32.to_le_bytes());
        data[sec + 16..sec + 20].copy_from_slice(&0x200u32.to_le_bytes());
        data[sec + 20..sec + 24].copy_from_slice(&0x200u32.to_le_bytes());
        // Descriptor name RVA -> 0x1100.
        data[0x200 + 12..0x200 + 16].copy_from_slice(&0x1100u32.to_le_bytes());
        data[0x300..0x300 + import.len()].copy_from_slice(import.as_bytes());
        data
    }

    #[test]
    fn reads_a_normal_import() {
        let imports = parse_imports(&tiny_pe("KERNEL32.dll"), true).unwrap();
        assert_eq!(
            imports.into_iter().collect::<Vec<_>>(),
            vec!["kernel32.dll"]
        );
    }

    #[test]
    fn refuses_garbage() {
        assert!(parse_imports(b"not a pe", true).is_err());
        let mut pe = tiny_pe("x.dll");
        pe[0x84] = 0x4c; // i386
        assert!(parse_imports(&pe, true)
            .unwrap_err()
            .contains("expected x64"));
    }

    #[test]
    fn classifies_runtime_names() {
        assert!(is_vc_dll("VCRUNTIME140_1.dll"));
        assert!(is_vc_dll("msvcp140_atomic_wait.dll"));
        assert!(!is_vc_dll("kernel32.dll"));
        assert!(api_set().is_match("api-ms-win-crt-runtime-l1-1-0.dll"));
    }

    /// The installed Visual Studio runtime stages and passes the audit. Runs
    /// on the Windows CI runner, which has Visual Studio.
    #[cfg(windows)]
    #[test]
    fn stages_the_installed_runtime() {
        let dir = tempfile::tempdir().unwrap();
        stage_windows_runtime(dir.path()).unwrap();
    }
}
