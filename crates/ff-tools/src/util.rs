//! Shared plumbing: the checkout root, running other programs, git, and the
//! handful of Python string rules the ported tools depend on for identical
//! output.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// A failure a tool reports and exits on. The message is for a person.
pub type Result<T> = std::result::Result<T, String>;

/// The repository checkout this binary was built from.
pub fn repo_root() -> PathBuf {
    let built = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf);
    match built {
        Some(root) if root.join("Cargo.toml").is_file() => root,
        _ => std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
    }
}

/// `path` relative to `base`, with forward slashes, for messages and refs.
pub fn rel(path: &Path, base: &Path) -> String {
    let shown = path.strip_prefix(base).unwrap_or(path);
    shown
        .components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

/// Render a command for an error message or a log line.
pub fn show_command(cmd: &Command) -> String {
    let mut parts = vec![cmd.get_program().to_string_lossy().into_owned()];
    parts.extend(cmd.get_args().map(|a| a.to_string_lossy().into_owned()));
    parts.join(" ")
}

/// Run a command to completion and fail unless it exits 0
/// (`subprocess.run(..., check=True)`).
pub fn run(cmd: &mut Command) -> Result<()> {
    let status = cmd
        .status()
        .map_err(|e| format!("could not start {}: {e}", show_command(cmd)))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{} failed ({status})", show_command(cmd)))
    }
}

/// Run a command and return its standard output, failing on a non-zero exit.
pub fn output(cmd: &mut Command) -> Result<String> {
    let out = cmd
        .stderr(Stdio::inherit())
        .output()
        .map_err(|e| format!("could not start {}: {e}", show_command(cmd)))?;
    if !out.status.success() {
        return Err(format!("{} failed ({})", show_command(cmd), out.status));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// `git <args>` in the checkout, stdout with surrounding whitespace removed.
pub fn git(root: &Path, args: &[&str]) -> Result<String> {
    let out = Command::new("git")
        .args(args)
        .current_dir(root)
        .stderr(Stdio::null())
        .output()
        .map_err(|e| format!("could not run git: {e}"))?;
    if !out.status.success() {
        return Err(format!("git {} failed", args.join(" ")));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Whether `git <args>` exits 0, output discarded.
pub fn git_ok(root: &Path, args: &[&str]) -> bool {
    Command::new("git")
        .args(args)
        .current_dir(root)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Python's `str.isspace()` for one character.
pub fn py_isspace(c: char) -> bool {
    c.is_whitespace() || matches!(c, '\u{1c}'..='\u{1f}')
}

/// Python's `str.strip()`.
pub fn py_strip(s: &str) -> &str {
    s.trim_matches(py_isspace)
}

/// Python's `str.lstrip()`.
pub fn py_lstrip(s: &str) -> &str {
    s.trim_start_matches(py_isspace)
}

/// Python's `str.rstrip()`.
pub fn py_rstrip(s: &str) -> &str {
    s.trim_end_matches(py_isspace)
}

/// Python's `str.splitlines()`: every line break Python knows, ends dropped.
pub fn py_splitlines(s: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let bytes: Vec<(usize, char)> = s.char_indices().collect();
    let mut start = 0;
    let mut i = 0;
    while i < bytes.len() {
        let (pos, c) = bytes[i];
        let is_break = matches!(
            c,
            '\n' | '\r'
                | '\u{b}'
                | '\u{c}'
                | '\u{1c}'
                | '\u{1d}'
                | '\u{1e}'
                | '\u{85}'
                | '\u{2028}'
                | '\u{2029}'
        );
        if is_break {
            out.push(&s[start..pos]);
            let mut next = pos + c.len_utf8();
            if c == '\r' && i + 1 < bytes.len() && bytes[i + 1].1 == '\n' {
                i += 1;
                next += 1;
            }
            start = next;
        }
        i += 1;
    }
    if start < s.len() {
        out.push(&s[start..]);
    }
    out
}

/// Sort paths the way Python sorts `pathlib.Path` objects: by component,
/// not by the joined string (`a/b.rs` sorts after `a/b/c.rs`).
pub fn sort_paths(paths: &mut [PathBuf]) {
    paths.sort_by(|a, b| {
        let ca: Vec<String> = a
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect();
        let cb: Vec<String> = b
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect();
        ca.cmp(&cb)
    });
}

/// Every file under `dir` (recursively), sorted as Python sorts paths.
pub fn walk_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            match entry.file_type() {
                Ok(t) if t.is_dir() => stack.push(path),
                Ok(t) if t.is_file() => out.push(path),
                Ok(t) if t.is_symlink() && path.is_file() => out.push(path),
                _ => {}
            }
        }
    }
    sort_paths(&mut out);
    out
}

/// Every file and directory under `dir`, sorted as Python sorts paths.
pub fn walk_all(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if entry.file_type().is_ok_and(|t| t.is_dir()) {
                stack.push(path.clone());
            }
            out.push(path);
        }
    }
    sort_paths(&mut out);
    out
}

/// Files directly in `dir` whose name ends with `suffix`, sorted.
pub fn glob_suffix(dir: &Path, suffix: &str) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file())
        .filter(|p| {
            p.file_name()
                .is_some_and(|n| n.to_string_lossy().ends_with(suffix))
        })
        .collect();
    sort_paths(&mut out);
    out
}

/// SHA-256 of a file, hex, read in bounded chunks.
pub fn file_sha256(path: &Path) -> Result<String> {
    use sha2::{Digest, Sha256};
    let mut file =
        std::fs::File::open(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = file
            .read(&mut buf)
            .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex::encode(hasher.finalize()))
}

/// Read a UTF-8 text file, naming it in the error.
///
/// CRLF reads as LF, like Python's universal newlines: a Windows checkout
/// converts line endings, and a spoken line or a byte compare against
/// freshly rendered text must not see them.
pub fn read_text(path: &Path) -> Result<String> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    Ok(if text.contains('\r') {
        text.replace("\r\n", "\n")
    } else {
        text
    })
}

/// [`read_text`], or `None` when the file cannot be read.
pub fn read_text_lf(path: &Path) -> Option<String> {
    read_text(path).ok()
}

/// Write a text file, creating its folder, naming it in the error.
pub fn write_text(path: &Path, text: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
    }
    std::fs::write(path, text).map_err(|e| format!("cannot write {}: {e}", path.display()))
}

/// Copy a file (with its permissions), creating the destination folder.
pub fn copy_file(from: &Path, to: &Path) -> Result<()> {
    if let Some(parent) = to.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
    }
    std::fs::copy(from, to)
        .map(|_| ())
        .map_err(|e| format!("cannot copy {} to {}: {e}", from.display(), to.display()))
}

/// `"{n:.0f}"` with Python's round-half-even.
pub fn py_round0(x: f64) -> String {
    let r = x.round_ties_even();
    format!("{}", r as i64)
}

/// Run a command, killing it after `limit`; `Err` on a failed start, a
/// timeout, or a non-zero exit (`subprocess.run(..., check=True, timeout=)`).
pub fn run_with_timeout(cmd: &mut Command, limit: std::time::Duration) -> Result<()> {
    let shown = show_command(cmd);
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("could not start {shown}: {e}"))?;
    let deadline = std::time::Instant::now() + limit;
    loop {
        if let Some(status) = child
            .try_wait()
            .map_err(|e| format!("waiting for {shown}: {e}"))?
        {
            return if status.success() {
                Ok(())
            } else {
                Err(format!("{shown} failed ({status})"))
            };
        }
        if std::time::Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!(
                "{shown} timed out after {} seconds",
                limit.as_secs()
            ));
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}

/// The current platform under Python's `sys.platform` names.
pub fn platform_name() -> &'static str {
    if cfg!(windows) {
        "win32"
    } else if cfg!(target_os = "macos") {
        "darwin"
    } else {
        "linux"
    }
}

/// `platform.machine()`, lowercased.
pub fn machine() -> String {
    match std::env::consts::ARCH {
        "x86_64" => "x86_64".into(),
        "aarch64" => {
            if cfg!(target_os = "macos") {
                "arm64".into()
            } else {
                "aarch64".into()
            }
        }
        other => other.to_string(),
    }
}
