//! Download, unpack and apply an update: the archive handling, the
//! detached apply scripts (the exact `.bat` / `.sh` templates) and their
//! spawn. Re-exported from `crate::updater`.

use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use super::{
    install_target_in, running_appimage_path, Platform, UpdateInfo, UpdaterEnv, APP_NAME,
    USER_AGENT,
};
use crate::net::{self, NetError};

/// Why a download stopped short.
#[derive(Debug)]
pub enum DownloadError {
    /// `UpdateCancelled`: the player backed out.
    Cancelled,
    /// Nothing arrived for this long: the transfer went quiet (issue 266).
    Stalled(Duration),
    /// The file arrived whole but is not the one the release published.
    Corrupt,
    Net(NetError),
    Io(io::Error),
}

impl std::fmt::Display for DownloadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DownloadError::Cancelled => f.write_str("update canceled"),
            DownloadError::Stalled(idle) => {
                write!(f, "no data arrived for {} seconds", idle.as_secs())
            }
            DownloadError::Corrupt => f.write_str("the download does not match the release"),
            DownloadError::Net(e) => write!(f, "{e}"),
            DownloadError::Io(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for DownloadError {}

impl From<NetError> for DownloadError {
    fn from(e: NetError) -> Self {
        DownloadError::Net(e)
    }
}

impl From<io::Error> for DownloadError {
    fn from(e: io::Error) -> Self {
        DownloadError::Io(e)
    }
}

/// How long a download may go without a single byte before it fails.
///
/// An idle bound, never a total one: the download client deliberately has
/// no overall or response deadline, because a 420 MB snapshot cannot fit
/// one on a real line and the total deadline made every big update die
/// mid-transfer and be re-offered forever (issue 181). What it lost with
/// them was any bound on a connection that simply goes quiet, so a stalled
/// transfer blocked the worker for good (issue 266). Sixty seconds of
/// nothing is a dead transfer on any line that could finish one.
pub const DOWNLOAD_IDLE_TIMEOUT: Duration = Duration::from_secs(60);

/// How often the copy loop wakes to check for a cancel while it waits.
const CANCEL_POLL: Duration = Duration::from_millis(100);

/// Fetch the release archive into `dest_dir`.
///
/// `progress(done_bytes, total_bytes)` is called as data arrives. The
/// transfer fails with [`DownloadError::Stalled`] after
/// [`DOWNLOAD_IDLE_TIMEOUT`] without a byte, returns within a moment of
/// `cancelled` being set, and is checked against the release's published
/// SHA-256 when it has one.
pub fn download(
    info: &UpdateInfo,
    dest_dir: &Path,
    progress: Option<&mut dyn FnMut(u64, u64)>,
    cancelled: Option<&AtomicBool>,
) -> Result<PathBuf, DownloadError> {
    let dest = dest_dir.join(&info.asset_name);
    // The one send path in the crate that does not go through `net::request`
    // (the archive is streamed to disk, not buffered into memory), so it
    // carries the network capability itself.
    net::require_real_network("GET", &info.asset_url);
    // The download client, not the GitHub tier: a 294 MB snapshot can never
    // finish inside a total deadline, and failing here re-offers the same
    // update forever.
    let response = net::download_agent()
        .get(&info.asset_url)
        .header("User-Agent", USER_AGENT)
        .call()
        .map_err(NetError::from)?;
    let status = response.status().as_u16();
    if status >= 400 {
        return Err(NetError::http(status).into());
    }
    let total = response
        .headers()
        .get("content-length")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.trim().parse::<u64>().ok())
        .filter(|n| *n > 0)
        .unwrap_or(info.asset_size.max(0) as u64);
    let reader = response.into_body().into_reader();
    let file = fs::File::create(&dest)?;
    let digest = stream_to_file(
        reader,
        file,
        total,
        progress,
        cancelled,
        DOWNLOAD_IDLE_TIMEOUT,
    )?;
    if !info.asset_sha256.is_empty() && !digest.eq_ignore_ascii_case(&info.asset_sha256) {
        log::warn!(
            "Update download {} has SHA-256 {digest}, release says {}",
            info.asset_name,
            info.asset_sha256
        );
        let _ = fs::remove_file(&dest);
        return Err(DownloadError::Corrupt);
    }
    Ok(dest)
}

/// Copy `reader` into `file`, bounded by an idle timeout and a cancel flag;
/// returns the lowercase hex SHA-256 of what was written.
///
/// The blocking reads run on their own thread and hand chunks over a small
/// channel, so this loop can always give up: a socket read cannot be
/// interrupted, but it no longer has to be waited for. A reader left
/// blocked on a dead connection ends when that read returns (its next send
/// finds nobody listening) or with the process.
pub fn stream_to_file<R: Read + Send + 'static>(
    mut reader: R,
    mut file: fs::File,
    total: u64,
    mut progress: Option<&mut dyn FnMut(u64, u64)>,
    cancelled: Option<&AtomicBool>,
    idle_timeout: Duration,
) -> Result<String, DownloadError> {
    use sha2::{Digest, Sha256};
    use std::sync::mpsc::{sync_channel, RecvTimeoutError};

    let (tx, rx) = sync_channel::<io::Result<Vec<u8>>>(8);
    std::thread::Builder::new()
        .name("update-download-read".into())
        .spawn(move || loop {
            let mut buf = vec![0u8; 65536];
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    buf.truncate(n);
                    if tx.send(Ok(buf)).is_err() {
                        break;
                    }
                }
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => {
                    let _ = tx.send(Err(e));
                    break;
                }
            }
        })?;

    let mut hasher = Sha256::new();
    let mut done: u64 = 0;
    let mut last_data = Instant::now();
    loop {
        if cancelled.is_some_and(|flag| flag.load(Ordering::SeqCst)) {
            return Err(DownloadError::Cancelled);
        }
        match rx.recv_timeout(CANCEL_POLL) {
            Ok(Ok(chunk)) => {
                file.write_all(&chunk)?;
                hasher.update(&chunk);
                done += chunk.len() as u64;
                last_data = Instant::now();
                if let Some(progress) = progress.as_deref_mut() {
                    progress(done, total);
                }
            }
            Ok(Err(e)) => return Err(e.into()),
            Err(RecvTimeoutError::Disconnected) => break,
            Err(RecvTimeoutError::Timeout) => {
                if last_data.elapsed() >= idle_timeout {
                    log::warn!(
                        "Update download stalled: nothing for {} s after {done} bytes",
                        idle_timeout.as_secs()
                    );
                    return Err(DownloadError::Stalled(idle_timeout));
                }
            }
        }
    }
    file.flush()?;
    Ok(hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}

/// The longest an unpack may run. A 420 MB snapshot unpacks in well under
/// a minute on any Mac that can run the game; past this the unpacker is
/// wedged, and the player must be told rather than left on a silent screen.
pub const UNPACK_TIMEOUT: Duration = Duration::from_secs(600);

/// Run an external unpacker to completion, bounded: killed when `cancelled`
/// is set (`ErrorKind::Interrupted`) or when it outlives `timeout`
/// (`ErrorKind::TimedOut`). Its output is discarded; it has no terminal.
pub fn run_bounded(
    command: &mut Command,
    timeout: Duration,
    cancelled: Option<&AtomicBool>,
) -> io::Result<()> {
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(status) = child.try_wait()? {
            if status.success() {
                return Ok(());
            }
            return Err(io::Error::other(format!("unpacker failed: {status}")));
        }
        let stop = if cancelled.is_some_and(|flag| flag.load(Ordering::SeqCst)) {
            Some(io::Error::new(
                io::ErrorKind::Interrupted,
                "update canceled",
            ))
        } else if Instant::now() >= deadline {
            Some(io::Error::new(
                io::ErrorKind::TimedOut,
                format!("unpacking took longer than {} seconds", timeout.as_secs()),
            ))
        } else {
            None
        };
        if let Some(err) = stop {
            let _ = child.kill();
            let _ = child.wait();
            return Err(err);
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Unpack the release archive; returns the new app folder inside it.
pub fn extract(archive: &Path, staging: &Path, env: &UpdaterEnv) -> io::Result<PathBuf> {
    extract_with(archive, staging, env, None, UNPACK_TIMEOUT)
}

/// [`extract`], cancellable and bounded. The macOS unpacker is an external
/// `ditto`, so it is the one step that can be killed mid-way; the in-process
/// zip and tar readers check `cancelled` once they return.
pub fn extract_with(
    archive: &Path,
    staging: &Path,
    env: &UpdaterEnv,
    cancelled: Option<&AtomicBool>,
    timeout: Duration,
) -> io::Result<PathBuf> {
    fs::create_dir_all(staging)?;
    let name = archive
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    if name.ends_with(".tar.gz") {
        let file = fs::File::open(archive)?;
        let decoder = flate2::read::GzDecoder::new(file);
        let mut tar = tar::Archive::new(decoder);
        tar.set_preserve_permissions(true);
        tar.unpack(staging)?;
    } else if env.platform == Platform::MacOs {
        // ditto preserves the executable bits and bundle symlinks that a
        // plain unzip would drop. Bounded and killable: an unbounded
        // `status()` here could hold the download screen silent for as
        // long as ditto ran, with no way to leave it (issue 266).
        let mut ditto = Command::new("ditto");
        ditto.args(["-x", "-k"]).arg(archive).arg(staging);
        run_bounded(&mut ditto, timeout, cancelled)?;
    } else {
        let file = fs::File::open(archive)?;
        let mut zip = zip::ZipArchive::new(file)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        zip.extract(staging)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    }
    if cancelled.is_some_and(|flag| flag.load(Ordering::SeqCst)) {
        return Err(io::Error::new(
            io::ErrorKind::Interrupted,
            "update canceled",
        ));
    }
    extracted_root(staging, &name, env)
}

/// The new app folder inside an unpacked archive.
///
/// Windows and Linux archives hold a plain `FreightFate` folder; the
/// macOS archive holds the `FreightFate.app` bundle (`ditto
/// --keepParent` in `cargo tools build-release`).
pub fn extracted_root(staging: &Path, archive_name: &str, env: &UpdaterEnv) -> io::Result<PathBuf> {
    if env.platform == Platform::MacOs {
        let bundle = staging.join(format!("{APP_NAME}.app"));
        if bundle.is_dir() {
            return Ok(bundle);
        }
    }
    let new_root = staging.join(APP_NAME);
    if !new_root.is_dir() {
        let archive_name = if archive_name.is_empty() {
            "the archive"
        } else {
            archive_name
        };
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("{APP_NAME} folder missing from {archive_name}"),
        ));
    }
    Ok(new_root)
}

/// `tempfile.mkdtemp(prefix="freightfate-update-")`.
pub fn make_staging_dir() -> io::Result<PathBuf> {
    let base = std::env::temp_dir();
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    for attempt in 0..100u32 {
        let dir = base.join(format!(
            "{}-update-{}-{nanos}-{attempt}",
            APP_NAME.to_ascii_lowercase(),
            std::process::id()
        ));
        match fs::create_dir(&dir) {
            Ok(()) => return Ok(dir),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    Err(io::Error::other("could not create a staging directory"))
}

/// The runnable update staged from a downloaded release asset.
///
/// An .AppImage download IS the update -- one file, nothing to unpack.
/// Archives unpack into the staging dir and yield the new app folder.
pub fn stage_update(archive: &Path, staging: &Path, env: &UpdaterEnv) -> io::Result<PathBuf> {
    stage_update_with(archive, staging, env, None, UNPACK_TIMEOUT)
}

/// [`stage_update`], cancellable and bounded (see [`extract_with`]).
pub fn stage_update_with(
    archive: &Path,
    staging: &Path,
    env: &UpdaterEnv,
    cancelled: Option<&AtomicBool>,
    timeout: Duration,
) -> io::Result<PathBuf> {
    if path_name(archive).ends_with(".AppImage") {
        return Ok(archive.to_path_buf());
    }
    let new_root = extract_with(archive, &staging.join("unpacked"), env, cancelled, timeout)?;
    match fs::remove_file(archive) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => return Err(e),
    }
    Ok(new_root)
}

fn path_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn dir_writable(path: &Path) -> bool {
    let probe = path.join(format!(".{}-update-probe", APP_NAME.to_ascii_lowercase()));
    if fs::write(&probe, b"").is_err() {
        return false;
    }
    fs::remove_file(&probe).is_ok()
}

/// Whether `apply_and_restart` can install this staged update by itself.
///
/// False means the player must finish the install manually: an AppImage
/// swap needs the .AppImage's own folder to be writable, and a folder
/// update can never be applied to an AppImage run -- the mounted payload
/// is read-only and disposable; the .AppImage file is the install.
///
/// On macOS the swap renames the app bundle inside its folder, so that
/// folder must be writable, and the bundle must not be running from App
/// Translocation: an app opened straight out of a quarantined download runs
/// from a read-only copy under `AppTranslocation`, which no swap can touch,
/// and which is gone once the game quits -- the restart then opens nothing.
pub fn can_auto_apply(new_root: &Path, env: &UpdaterEnv) -> bool {
    let appimage = running_appimage_path(env.appimage.as_deref());
    if path_name(new_root).ends_with(".AppImage") && new_root.is_file() {
        return appimage
            .as_deref()
            .and_then(Path::parent)
            .is_some_and(dir_writable);
    }
    if env.platform == Platform::MacOs {
        return macos_bundle_swappable(&install_target_in(env));
    }
    appimage.is_none()
}

/// Whether the macOS apply script can swap the bundle at `install`.
pub fn macos_bundle_swappable(install: &Path) -> bool {
    if is_translocated(install) {
        return false;
    }
    install.parent().is_some_and(dir_writable)
}

/// True for a bundle macOS is running from an App Translocation mount.
pub fn is_translocated(path: &Path) -> bool {
    path.components()
        .any(|c| c.as_os_str() == std::ffi::OsStr::new("AppTranslocation"))
}

/// Park an update that needs a manual install somewhere describable.
///
/// The staging dir lives under the system temp folder; a single-file
/// update moves to the home folder instead, so the spoken location is
/// one the player can find again (and that survives a reboot). Folder
/// updates stay where they were unpacked.
///
/// A macOS `.app` bundle is a folder but is also the whole update, so it
/// moves to the home folder too; a bundle left under the system temp folder
/// is one the player could never find to drag into Applications.
pub fn stash_for_manual_install(new_root: &Path, home: Option<&Path>) -> PathBuf {
    let bundle = new_root.is_dir() && path_name(new_root).ends_with(".app");
    if !new_root.is_file() && !bundle {
        return new_root.to_path_buf();
    }
    let Some(home) = home else {
        return new_root.to_path_buf();
    };
    let dest = home.join(path_name(new_root));
    let moved = (|| -> io::Result<()> {
        if bundle {
            // Never delete a bundle already at the destination: it may be
            // the player's own copy. A rename either lands or fails whole.
            if dest.exists() {
                return Err(io::Error::from(io::ErrorKind::AlreadyExists));
            }
            return fs::rename(new_root, &dest);
        }
        if dest.exists() {
            fs::remove_file(&dest)?;
        }
        match fs::rename(new_root, &dest) {
            Ok(()) => Ok(()),
            Err(_) => {
                // shutil.move across devices: copy then remove
                fs::copy(new_root, &dest)?;
                fs::remove_file(new_root)
            }
        }
    })();
    if moved.is_err() {
        return new_root.to_path_buf();
    }
    dest
}

const WINDOWS_SCRIPT: &str = r#"@echo off
:wait
tasklist /FI "PID eq {pid}" 2>NUL | find "{pid}" >NUL
if not errorlevel 1 (
  ping -n 2 127.0.0.1 >NUL
  goto wait
)
robocopy "{src}\_internal" "{dst}\_internal" /MIR /R:10 /W:1 >NUL
robocopy "{src}" "{dst}" /E /XD _internal saves /R:10 /W:1 >NUL
start "" "{dst}\{exe}"
rmdir /s /q "{staging}"
del "%~f0"
"#;

const POSIX_SCRIPT: &str = r#"#!/bin/sh
# Keep portable saves under {dst}/saves intact even if a bad archive includes
# a top-level saves folder.
while kill -0 {pid} 2>/dev/null; do sleep 1; done
rm -rf "{dst}/_internal"
rm -rf "{src}/saves"
cp -a "{src}/." "{dst}/"
rm -rf "{staging}"
"{dst}/{exe}" &
rm -f "$0"
"#;

const APPIMAGE_SCRIPT: &str = r#"#!/bin/sh
# Swap the .AppImage file itself; the mounted payload it runs from is
# read-only (or a throwaway extraction) and must never be touched. The
# new file is staged next to the target so the final rename is atomic,
# and the relaunch runs the new AppImage, whose own AppRun rebuilds the
# library search path.
while kill -0 {pid} 2>/dev/null; do sleep 1; done
cp "{src}" "{dst}.update-new" || exit 1
chmod +x "{dst}.update-new"
mv -f "{dst}.update-new" "{dst}" || exit 1
rm -rf "{staging}"
"{dst}" &
rm -f "$0"
"#;

const MACOS_SCRIPT: &str = r#"#!/bin/sh
# Swap the whole app bundle. Saves live in ~/Library/Application Support,
# never inside the bundle. The old bundle is parked beside the install until
# the new one is in place, so a failed copy cannot leave the player with no
# game at all.
while kill -0 {pid} 2>/dev/null; do sleep 1; done
rm -rf "{dst}.old"
mv "{dst}" "{dst}.old"
if mv "{src}" "{dst}" 2>/dev/null || cp -R "{src}" "{dst}"; then
  rm -rf "{dst}.old"
else
  mv "{dst}.old" "{dst}"
fi
rm -rf "{staging}"
open "{dst}"
rm -f "$0"
"#;

/// A path made safe to sit between the double quotes of an apply script.
///
/// cmd expands `%NAME%` even inside quotes, so a folder such as
/// `C:\Games\100% Trucks` would copy the update somewhere else; a batch file
/// reads `%%` as one literal percent sign. A POSIX shell still expands `$`,
/// backticks and backslashes inside double quotes, so those are escaped.
pub fn quote_for_script(path: &str, windows: bool) -> String {
    if windows {
        return path.replace('%', "%%");
    }
    let mut out = String::with_capacity(path.len());
    for ch in path.chars() {
        if matches!(ch, '\\' | '"' | '$' | '`') {
            out.push('\\');
        }
        out.push(ch);
    }
    out
}

/// The helper script that swaps in the update once the game exits.
pub fn write_apply_script(
    new_root: &Path,
    install: &Path,
    staging: &Path,
    pid: u32,
    env: &UpdaterEnv,
) -> io::Result<PathBuf> {
    let windows = env.platform == Platform::Windows;
    let exe = format!("{APP_NAME}{}", if windows { ".exe" } else { "" });
    let template = if path_name(new_root).ends_with(".AppImage") {
        // install is the running .AppImage file itself, not a folder.
        APPIMAGE_SCRIPT
    } else if windows {
        WINDOWS_SCRIPT
    } else if env.platform == Platform::MacOs && install.extension().is_some_and(|e| e == "app") {
        MACOS_SCRIPT
    } else {
        POSIX_SCRIPT
    };
    let quote = |path: &Path| quote_for_script(&path.display().to_string(), windows);
    let text = template
        .replace("{pid}", &pid.to_string())
        .replace("{src}", &quote(new_root))
        .replace("{dst}", &quote(install))
        .replace("{staging}", &quote(staging))
        .replace("{exe}", &exe);
    let suffix = if windows { ".bat" } else { ".sh" };
    let parent = staging.parent().unwrap_or(staging);
    let script = parent.join(format!(
        "{}-apply-{pid}{suffix}",
        APP_NAME.to_ascii_lowercase()
    ));
    fs::write(&script, text)?;
    #[cfg(unix)]
    if !windows {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&script, fs::Permissions::from_mode(0o755))?;
    }
    Ok(script)
}

/// [`apply_and_restart`] with the spawn injected: `spawn` receives the
/// command line (`cmd /c script` or `/bin/sh script`). Returns the script
/// path, or `None` when an AppImage update was refused because this is not
/// an AppImage run.
pub fn apply_and_restart_with(
    new_root: &Path,
    staging: &Path,
    env: &UpdaterEnv,
    spawn: &mut dyn FnMut(Vec<String>) -> io::Result<()>,
) -> io::Result<Option<PathBuf>> {
    let install = if path_name(new_root).ends_with(".AppImage") {
        match running_appimage_path(env.appimage.as_deref()) {
            Some(path) => path,
            None => {
                log::warn!(
                    "Not an AppImage run; cannot swap {} in place",
                    new_root.display()
                );
                return Ok(None);
            }
        }
    } else {
        install_target_in(env)
    };
    let script = write_apply_script(new_root, &install, staging, env.pid, env)?;
    let command = if env.platform == Platform::Windows {
        vec![
            "cmd".to_string(),
            "/c".to_string(),
            script.display().to_string(),
        ]
    } else {
        vec!["/bin/sh".to_string(), script.display().to_string()]
    };
    spawn(command)?;
    log::info!("Update staged; apply script {} spawned", script.display());
    Ok(Some(script))
}

/// Spawn the detached apply script. The caller must then quit the game;
/// the script waits for this process to exit before touching files.
pub fn apply_and_restart(new_root: &Path, staging: &Path) -> io::Result<()> {
    let env = UpdaterEnv::current();
    apply_and_restart_with(new_root, staging, &env, &mut spawn_detached).map(|_| ())
}

/// `subprocess.Popen(..., detached)`: no window, no inherited handles, its
/// own process group.
fn spawn_detached(command: Vec<String>) -> io::Result<()> {
    let (program, args) = command
        .split_first()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "empty command"))?;
    let mut cmd = Command::new(program);
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        use windows_sys::Win32::System::Threading::{CREATE_NEW_PROCESS_GROUP, CREATE_NO_WINDOW};
        cmd.creation_flags(CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // start_new_session=True
        cmd.process_group(0);
    }
    cmd.spawn().map(|_| ())
}
