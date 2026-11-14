//! A game's data folder, read and written through the seal.

use std::collections::BTreeSet;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use crate::format;
use crate::key::Key;
use crate::name;

/// What reading a data file found.
#[derive(Debug)]
pub enum Opened {
    /// No file.
    Missing,
    /// An authentic sealed file: its bytes.
    Sealed(Vec<u8>),
    /// A plain file, in a build that does not seal (a developer build).
    Plain(Vec<u8>),
    /// A plain file in a sealing build that the witnesses still allow to be
    /// imported: there is none yet, or one lists this name as a plain file
    /// that was in the folder when this game first sealed for this player.
    /// Written by an older version of the game (or edited by hand before the
    /// first sealing run). The game decides whether to import it.
    Legacy(Vec<u8>),
    /// A sealed file in a build that cannot open it (a developer build
    /// running against a release's saves). The vault will not overwrite it.
    Locked,
    /// The file was changed, swapped or forged.
    Tampered(Tamper),
    /// The disk would not give the file up (permissions, I/O, a folder
    /// where the file should be).
    Failed(io::Error),
}

impl Opened {
    /// The bytes of `Sealed` or `Plain`: the only verdicts a game can trust
    /// as they are.
    pub fn trusted(self) -> Option<Vec<u8>> {
        match self {
            Opened::Sealed(bytes) | Opened::Plain(bytes) => Some(bytes),
            _ => None,
        }
    }
}

/// How a sealed file was tampered with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tamper {
    /// Does not authenticate: edited, cut short, another file's, another
    /// game's or another key's.
    Forged,
    /// A plain file that the witnesses do not allow to be imported: this
    /// game has sealed for this player, and the name was not one of the
    /// plain files left from before, or has been sealed since.
    Unsealed,
}

impl std::fmt::Display for Tamper {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Tamper::Forged => "it does not match its seal",
            Tamper::Unsealed => "a plain file is where only sealed files belong",
        })
    }
}

/// A game's data folder, read and written through the seal.
///
/// Every file goes in and out whole. A write lands atomically (a hidden
/// temporary file, flushed, then renamed over the old one), so a crash
/// never leaves half a file. Names are single file names (see
/// [`Vault::read`]); the vault keeps nothing in the folder but the game's
/// own files, and never touches anything outside it except its witnesses
/// (see [`Vault::with_witnesses`]).
pub struct Vault {
    dir: PathBuf,
    key: Option<Key>,
    witnesses: Vec<PathBuf>,
    checked: Vec<PathBuf>,
}

impl Vault {
    /// The folder at `dir`, sealing with this build's key, if it has one.
    pub fn new(dir: impl Into<PathBuf>) -> Vault {
        Vault::with_key(dir, crate::build_key())
    }

    /// The folder at `dir` with a given key, or none: for tests and tools.
    pub fn with_key(dir: impl Into<PathBuf>, key: Option<Key>) -> Vault {
        Vault {
            dir: dir.into(),
            key,
            witnesses: Vec::new(),
            checked: Vec::new(),
        }
    }

    /// The same vault, leaving a small sealed marker at each of `paths` (full
    /// file paths, outside the data folder) once it seals anything.
    ///
    /// A marker says "this game has sealed data for this player before",
    /// and holds, sealed, the names still importable: the plain files that
    /// were in the folder when the first one was written, less each name
    /// sealed since. While a marker is there, a plain file under any other
    /// name reads as `Tampered(Unsealed)`, not as `Legacy`: so swapping a
    /// sealed file for a hand-written plain one, or pointing the game at a
    /// fresh folder holding one, no longer gets it imported, while every
    /// genuine old save can still be imported one at a time. A marker that
    /// does not authenticate allows nothing. Writing a marker is best
    /// effort; a failure is logged.
    pub fn with_witnesses(mut self, paths: Vec<PathBuf>) -> Vault {
        self.witnesses = paths;
        self
    }

    /// The same vault, also heeding markers at `paths` that it never writes:
    /// where another way of running the same game (a portable copy rather
    /// than an installed one, or the reverse) leaves its own. So switching
    /// between them does not forget that this game has sealed.
    pub fn checking_witnesses(mut self, paths: Vec<PathBuf>) -> Vault {
        self.checked = paths;
        self
    }

    /// The marker files this vault may leave outside its folder.
    pub fn witnesses(&self) -> &[PathBuf] {
        &self.witnesses
    }

    /// Whether files are sealed (a release built with `seal = true`).
    pub fn is_sealing(&self) -> bool {
        self.key.is_some()
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Reads `name`; never trusts a file that fails the seal.
    ///
    /// A name is one file name: Unicode letters, digits, spaces, `.`, `_` and `-`, not
    /// starting with `.`, at most 240 bytes. Names are compared exactly, so
    /// a game should not use two that differ only in case. Any other name
    /// reads as `Failed` with `InvalidInput`.
    ///
    /// A plain file reads as `Legacy` only while the witnesses allow its
    /// name to be imported (see [`Vault::with_witnesses`]). Otherwise it is
    /// `Tampered(Unsealed)`.
    pub fn read(&mut self, name: &str) -> Opened {
        if let Err(e) = name::check(name) {
            return Opened::Failed(e);
        }
        let found = match read_file(&self.dir.join(name)) {
            Ok(found) => found,
            Err(e) => return Opened::Failed(e),
        };
        let Some(bytes) = found else {
            return Opened::Missing;
        };
        let Some(key) = &self.key else {
            return if format::is_sealed(&bytes) {
                Opened::Locked
            } else {
                Opened::Plain(bytes)
            };
        };
        if !format::is_sealed(&bytes) {
            let importable = match self.importable(key) {
                None => true,
                Some(names) => names.contains(name),
            };
            return if importable {
                Opened::Legacy(bytes)
            } else {
                Opened::Tampered(Tamper::Unsealed)
            };
        }
        match format::open(&key.file_key(), name, &bytes) {
            Some(plain) => Opened::Sealed(plain),
            None => Opened::Tampered(Tamper::Forged),
        }
    }

    /// Writes `name` atomically: sealed when sealing, plain otherwise. A
    /// build that does not seal refuses (`PermissionDenied`) to overwrite a
    /// sealed file, or to add a new file to a folder that holds one (a
    /// release's folder, which would take the plain file for tampering). A
    /// sealing build overwriting a plain file first keeps it
    /// as `<name>.plain-backup`, unless one is there already.
    pub fn write(&mut self, name: &str, bytes: &[u8]) -> io::Result<()> {
        name::check(name)?;
        std::fs::create_dir_all(&self.dir)?;
        let path = self.dir.join(name);
        let existing = read_file(&path)?;
        let Some(key) = self.key.clone() else {
            if existing.as_deref().is_some_and(format::is_sealed) {
                return Err(locked(name));
            }
            if existing.is_none() && holds_sealed(&self.dir) {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    format!(
                        "{} holds a release's sealed files; this build will not add {name} to it",
                        self.dir.display()
                    ),
                ));
            }
            return atomic_write(&self.dir, name, bytes);
        };
        if let Some(old) = existing.filter(|old| !format::is_sealed(old)) {
            let backup = self.dir.join(format!("{name}{}", name::BACKUP));
            if std::fs::symlink_metadata(&backup).is_err() {
                std::fs::write(&backup, old)?;
            }
        }
        let sealed = format::seal(&key.file_key(), name, bytes)?;
        atomic_write(&self.dir, name, &sealed)?;
        self.leave_witnesses(&key, name);
        Ok(())
    }

    /// Deletes `name` on purpose (a reset the player chose). A build that
    /// does not seal will not delete a sealed file.
    pub fn remove(&mut self, name: &str) -> io::Result<()> {
        name::check(name)?;
        let path = self.dir.join(name);
        if !self.is_sealing() && read_file(&path)?.as_deref().is_some_and(format::is_sealed) {
            return Err(locked(name));
        }
        match std::fs::remove_file(&path) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        }
    }

    /// Moves a file the game will not use to `<name>.tampered` (or
    /// `.tampered-2`, `-3`...), so it can still be looked at, and the next
    /// read is `Missing`. Returns where it went (`None` when there was no
    /// file). A build that does not seal will not move a sealed file.
    pub fn set_aside(&mut self, name: &str) -> io::Result<Option<PathBuf>> {
        name::check(name)?;
        let path = self.dir.join(name);
        if std::fs::symlink_metadata(&path).is_err() {
            return Ok(None);
        }
        if !self.is_sealing() && read_file(&path)?.as_deref().is_some_and(format::is_sealed) {
            return Err(locked(name));
        }
        let to = free_name(&self.dir, name);
        std::fs::rename(&path, &to)?;
        log::warn!("seal: set {name} aside as {}", to.display());
        Ok(Some(to))
    }

    /// Brings every witness up to date after `sealed` was written: one that
    /// is missing is written, and `sealed` is no longer importable.
    fn leave_witnesses(&self, key: &Key, sealed: &str) {
        if self.witnesses.is_empty() {
            return;
        }
        let mut names = match self.importable(key) {
            Some(names) => names,
            // The first witness: every plain file still in the folder may be
            // imported later.
            None => plain_names(&self.dir),
        };
        names.remove(sealed);
        for path in &self.witnesses {
            if read_witness(key, path).as_ref() == Some(&Some(names.clone())) {
                continue;
            }
            if let Err(e) = write_witness(key, path, &names) {
                log::warn!("seal: could not leave a witness at {}: {e}", path.display());
            }
        }
    }

    /// The names a plain file may be imported under: `None` while no
    /// witness exists (anything may), else those every witness allows.
    fn importable(&self, key: &Key) -> Option<BTreeSet<String>> {
        let mut out: Option<BTreeSet<String>> = None;
        for path in self.witnesses.iter().chain(&self.checked) {
            let Some(found) = read_witness(key, path) else {
                continue;
            };
            let names = found.unwrap_or_default();
            out = Some(match out {
                None => names,
                Some(prev) => prev.intersection(&names).cloned().collect(),
            });
        }
        out
    }
}

/// The witness at `path`: `None` when there is none, `Some(None)` when one
/// is there but does not authenticate (it allows nothing), else the names
/// it allows.
#[allow(clippy::option_option)]
fn read_witness(key: &Key, path: &Path) -> Option<Option<BTreeSet<String>>> {
    std::fs::symlink_metadata(path).ok()?;
    let names = std::fs::read(path)
        .ok()
        .and_then(|bytes| format::open(&key.file_key(), WITNESS, &bytes))
        .and_then(|plain| String::from_utf8(plain).ok())
        .map(|text| {
            text.lines()
                .filter(|n| name::check(n).is_ok())
                .map(str::to_owned)
                .collect()
        });
    Some(names)
}

/// Seals `names` into the witness at `path`, through a temporary file
/// beside it so a crash never leaves half a witness.
fn write_witness(key: &Key, path: &Path, names: &BTreeSet<String>) -> io::Result<()> {
    let text = names
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join("\n");
    let sealed = format::seal(&key.file_key(), WITNESS, text.as_bytes())?;
    let (Some(dir), Some(file)) = (path.parent(), path.file_name()) else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "a witness needs a file name",
        ));
    };
    std::fs::create_dir_all(dir)?;
    let mut tmp = std::ffi::OsString::from(".");
    tmp.push(file);
    tmp.push(".tmp");
    write_through(&dir.join(tmp), path, &sealed)
}

/// The valid names in `dir` that hold plain (unsealed) files.
fn plain_names(dir: &Path) -> BTreeSet<String> {
    std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            name::check(&name).ok()?;
            let bytes = std::fs::read(entry.path()).ok()?;
            (!format::is_sealed(&bytes)).then_some(name)
        })
        .collect()
}

/// Whether any file in `dir` under a valid name is sealed.
fn holds_sealed(dir: &Path) -> bool {
    std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .any(|entry| {
            let named = entry
                .file_name()
                .to_str()
                .is_some_and(|n| name::check(n).is_ok());
            named && starts_sealed(&entry.path())
        })
}

/// Whether the file at `path` begins like a sealed file.
fn starts_sealed(path: &Path) -> bool {
    use std::io::Read;
    let mut head = [0u8; 4];
    std::fs::File::open(path)
        .and_then(|mut f| f.read_exact(&mut head))
        .is_ok()
        && format::is_sealed(&head)
}

/// The name a witness is sealed under: one [`name::check`] refuses, so no
/// data file can ever authenticate as a witness, or a witness as one.
const WITNESS: &str = ".hearken-seal.witness";

fn locked(name: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        format!("{name} is sealed by a release of the game; this build will not change it"),
    )
}

/// The file's bytes, or `None` when there is no file. A folder where the
/// file should be is an error, not a missing file.
fn read_file(path: &Path) -> io::Result<Option<Vec<u8>>> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            // A dangling link is a file that is gone.
            Ok(None)
        }
        Err(e) => Err(e),
    }
}

/// Writes `bytes` to `dir/name` through a hidden temporary file, flushed
/// before it is renamed over the old one.
fn atomic_write(dir: &Path, name: &str, bytes: &[u8]) -> io::Result<()> {
    write_through(&dir.join(name::temp(name)), &dir.join(name), bytes)
}

/// Writes `bytes` to `tmp`, flushes it and renames it over `to`. Whatever
/// was at `tmp` (a leftover file, a folder, a link to some other file) is
/// cleared first, and the file is made new, so a write never follows a
/// link to touch anything but `tmp` itself.
fn write_through(tmp: &Path, to: &Path, bytes: &[u8]) -> io::Result<()> {
    clear(tmp)?;
    let written = (|| {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(tmp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(tmp, to)
    })();
    if written.is_err() {
        let _ = std::fs::remove_file(tmp);
    }
    written
}

/// Removes whatever is at `path` (never what a link there points to).
fn clear(path: &Path) -> io::Result<()> {
    let removed = match std::fs::symlink_metadata(path) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e),
        Ok(meta) if meta.is_dir() => std::fs::remove_dir_all(path),
        Ok(_) => std::fs::remove_file(path),
    };
    match removed {
        Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

/// `dir/<name>.tampered`, or the first of `.tampered-2`, `-3`... not taken.
fn free_name(dir: &Path, name: &str) -> PathBuf {
    let mut n = 1u32;
    loop {
        let candidate = if n == 1 {
            dir.join(format!("{name}{}", name::TAMPERED))
        } else {
            dir.join(format!("{name}{}-{n}", name::TAMPERED))
        };
        if std::fs::symlink_metadata(&candidate).is_err() {
            return candidate;
        }
        n += 1;
    }
}

#[cfg(test)]
mod tests;
