//! A pack read from disk one entry at a time, opened on first use.
//!
//! `channel3000.pak` is about 110 MB of Ogg Opus that most drives never
//! play, so unlike `sounds.pak` it is never read into memory and never
//! opened at startup. `music.pak` uses the same [`StreamedPack`], opened at
//! startup (the title theme needs it) but read one track at a time. [`LazyPack`] does nothing until the
//! first `c3k/` name is asked for; then [`StreamedPack`] opens the file and
//! reads only the zip directory at its end. Each clip is read and unmasked
//! from its own offset when it is played, so a 15-minute programme costs
//! its own few megabytes for as long as it plays, and nothing is ever
//! merged into the music pack's memory.
//!
//! The format is the same FFPK as the other packs (magic, then a zip
//! XOR-masked with the repeating key, see [`super::mask_in_place`]); the
//! mask is a pure function of the byte offset, which is what lets a reader
//! unmask any slice of the file without the bytes before it.
//!
//! If the file is missing or unreadable the pack answers "absent" to every
//! name, which is how the radio keeps the station off the dial
//! ([`channel3000_pack_available`]) instead of tuning to dead air.

use std::collections::HashSet;
use std::fs::File;
use std::io::{self, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};

use zip::ZipArchive;

use super::{pack_available, packs_disabled, PackError, PACK_MAGIC, XOR_KEY};

/// The pack file seen through the mask: offset 0 is the first byte after
/// [`PACK_MAGIC`], and every byte read comes back unmasked.
struct MaskedFile {
    inner: BufReader<File>,
    /// Where the next read starts, in payload offsets.
    pos: u64,
}

impl MaskedFile {
    const BASE: u64 = PACK_MAGIC.len() as u64;
    /// Read-ahead per system call. A track is read start to finish in one
    /// go, so a larger buffer means fewer calls for the same bytes.
    const READ_AHEAD: usize = 256 * 1024;
}

impl Read for MaskedFile {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = self.inner.read(buf)?;
        // One division per read, not per byte: a music track is megabytes.
        let mut key = (self.pos % XOR_KEY.len() as u64) as usize;
        for byte in &mut buf[..n] {
            *byte ^= XOR_KEY[key];
            key += 1;
            if key == XOR_KEY.len() {
                key = 0;
            }
        }
        self.pos += n as u64;
        Ok(n)
    }
}

impl Seek for MaskedFile {
    fn seek(&mut self, to: SeekFrom) -> io::Result<u64> {
        let file_pos = match to {
            SeekFrom::Start(offset) => self.inner.seek(SeekFrom::Start(offset + Self::BASE))?,
            SeekFrom::End(delta) => self.inner.seek(SeekFrom::End(delta))?,
            SeekFrom::Current(delta) => self.inner.seek(SeekFrom::Current(delta))?,
        };
        if file_pos < Self::BASE {
            // Inside the magic: not part of the zip. Put the file back on
            // the payload's first byte rather than leave it there.
            self.inner.seek(SeekFrom::Start(Self::BASE))?;
            self.pos = 0;
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "seek before the start of the pack payload",
            ));
        }
        self.pos = file_pos - Self::BASE;
        Ok(self.pos)
    }
}

/// A masked pack read entry by entry from its file.
pub struct StreamedPack {
    archive: Mutex<ZipArchive<MaskedFile>>,
    /// Entry names in archive order, and the same names for lookups.
    names: Vec<String>,
    name_set: HashSet<String>,
}

impl StreamedPack {
    /// Open `path`: check the magic, then read the zip directory only.
    pub fn open(path: &Path) -> Result<Self, PackError> {
        let mut file = File::open(path)?;
        let mut magic = [0u8; PACK_MAGIC.len()];
        if file.read_exact(&mut magic).is_err() || &magic != PACK_MAGIC {
            return Err(PackError::NotAPack(path.to_path_buf()));
        }
        let masked = MaskedFile {
            inner: BufReader::with_capacity(MaskedFile::READ_AHEAD, file),
            pos: 0,
        };
        let archive = ZipArchive::new(masked)?;
        let names: Vec<String> = archive.file_names().map(str::to_string).collect();
        let name_set = names.iter().cloned().collect();
        Ok(Self {
            archive: Mutex::new(archive),
            names,
            name_set,
        })
    }

    pub fn names(&self) -> Vec<String> {
        self.names.clone()
    }

    pub fn has(&self, name: &str) -> bool {
        self.name_set.contains(name)
    }

    pub fn len(&self) -> usize {
        self.names.len()
    }

    pub fn is_empty(&self) -> bool {
        self.names.is_empty()
    }

    /// One entry's bytes, or `None` when the pack lacks it or it is
    /// damaged (that clip is lost, not the pack).
    pub fn read(&self, name: &str) -> Option<Vec<u8>> {
        if !self.has(name) {
            return None;
        }
        let mut archive = self.archive.lock().unwrap_or_else(|e| e.into_inner());
        let result = archive
            .by_name(name)
            .map_err(PackError::from)
            .and_then(|mut entry| {
                let mut data = Vec::with_capacity(entry.size() as usize);
                entry.read_to_end(&mut data)?;
                Ok(data)
            });
        match result {
            Ok(data) => Some(data),
            Err(err) => {
                log::warn!("Damaged entry in streamed pack: {name} ({err})");
                None
            }
        }
    }
}

/// A [`StreamedPack`] that is opened the first time a name is asked for.
pub struct LazyPack {
    path: PathBuf,
    pack: OnceLock<Option<StreamedPack>>,
    opens: AtomicUsize,
}

impl LazyPack {
    /// Remember `path`; touch nothing on disk.
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            pack: OnceLock::new(),
            opens: AtomicUsize::new(0),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Whether a first lookup has opened the file (or given up on it).
    pub fn opened(&self) -> bool {
        self.pack.get().is_some()
    }

    /// How many times the file was opened; at most one.
    pub fn opens(&self) -> usize {
        self.opens.load(Ordering::SeqCst)
    }

    fn pack(&self) -> Option<&StreamedPack> {
        self.pack
            .get_or_init(|| {
                self.opens.fetch_add(1, Ordering::SeqCst);
                if !self.path.exists() {
                    log::info!(
                        "No pack at {}; its sounds are not here",
                        self.path.display()
                    );
                    return None;
                }
                match StreamedPack::open(&self.path) {
                    Ok(pack) => {
                        log::info!(
                            "Pack opened on first use: {} ({} entries)",
                            self.path.display(),
                            pack.len()
                        );
                        Some(pack)
                    }
                    Err(err) => {
                        log::warn!("Unreadable pack at {} ({err})", self.path.display());
                        None
                    }
                }
            })
            .as_ref()
    }

    pub fn has(&self, name: &str) -> bool {
        self.pack().is_some_and(|pack| pack.has(name))
    }

    pub fn read(&self, name: &str) -> Option<Vec<u8>> {
        self.pack().and_then(|pack| pack.read(name))
    }
}

/// Whether this build can play Channel 3000: its pack is where the other
/// packs are, it is a real file rather than a pointer, and packs are not
/// switched off (`FREIGHT_FATE_IGNORE_SOUND_PACK=1`). Existence only; the
/// file is not opened. Without it the station stays off the dial.
pub fn channel3000_pack_available() -> bool {
    !packs_disabled()
        && pack_available(
            &super::default_pack_dir().join(crate::channel3000::CHANNEL_3000_PACK_NAME),
        )
}

#[cfg(test)]
mod tests {
    use super::super::{write_pack, CombinedPack, PackLoader};
    use super::*;

    /// A small Channel 3000 pack built here, never the real 110 MB one:
    /// two clips under `c3k/`, one long enough to cross many mask cycles.
    fn fixture_pack(tmp: &Path) -> (PathBuf, Vec<u8>) {
        let src = tmp.join("c3k_src");
        std::fs::create_dir_all(src.join("c3k")).unwrap();
        let long: Vec<u8> = (0..20_000u32).map(|i| (i * 7 % 251) as u8).collect();
        std::fs::write(src.join("c3k").join("day_weigh_in_01.opus"), &long).unwrap();
        std::fs::write(src.join("c3k").join("id_channel3000_01.opus"), b"ident").unwrap();
        let out = write_pack(&src, &tmp.join("channel3000.pak"), None, None).unwrap();
        (out, long)
    }

    fn sounds_pack(tmp: &Path) -> PathBuf {
        let src = tmp.join("sounds_src");
        std::fs::create_dir_all(src.join("ui")).unwrap();
        std::fs::write(src.join("ui").join("menu_select.ogg"), b"menu select").unwrap();
        write_pack(&src, &tmp.join("sounds.pak"), None, None).unwrap()
    }

    #[test]
    fn a_streamed_pack_reads_what_the_writer_packed() {
        let tmp = tempfile::tempdir().unwrap();
        let (path, long) = fixture_pack(tmp.path());
        let pack = StreamedPack::open(&path).unwrap();
        assert_eq!(pack.len(), 2);
        assert_eq!(pack.read("c3k/day_weigh_in_01.opus").unwrap(), long);
        assert_eq!(pack.read("c3k/id_channel3000_01.opus").unwrap(), b"ident");
        assert!(pack.read("c3k/not_there.opus").is_none());
        // Out of order, and twice: every read seeks to its own entry.
        assert_eq!(pack.read("c3k/day_weigh_in_01.opus").unwrap(), long);
    }

    #[test]
    fn a_file_that_is_not_a_pack_is_refused() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("channel3000.pak");
        std::fs::write(&path, b"PK\x03\x04 a plain zip").unwrap();
        assert!(matches!(
            StreamedPack::open(&path),
            Err(PackError::NotAPack(_))
        ));
    }

    #[test]
    fn the_pack_opens_on_the_first_c3k_lookup_and_only_then() {
        let tmp = tempfile::tempdir().unwrap();
        let (c3k, long) = fixture_pack(tmp.path());
        let sounds = sounds_pack(tmp.path());
        let lazy = std::sync::Arc::new(LazyPack::new(&c3k));
        let loader = PackLoader::new(&sounds, tmp.path().join("no_music.pak"))
            .with_channel3000(std::sync::Arc::clone(&lazy));
        let combined = loader.open().unwrap();
        // Startup, the menus and every other sound: the pack stays shut.
        assert_eq!(combined.read("ui/menu_select.ogg").unwrap(), b"menu select");
        assert!(combined.read("music/open_road.ogg").is_none());
        let _ = combined.names();
        assert!(!lazy.opened());
        assert_eq!(lazy.opens(), 0);
        // Tuning in: the first clip opens it, once.
        assert_eq!(combined.read("c3k/day_weigh_in_01.opus").unwrap(), long);
        assert!(combined.has("c3k/id_channel3000_01.opus"));
        assert_eq!(
            combined.read("c3k/id_channel3000_01.opus").unwrap(),
            b"ident"
        );
        assert!(lazy.opened());
        assert_eq!(lazy.opens(), 1);
    }

    #[test]
    fn a_missing_pack_answers_absent_and_is_not_available() {
        let tmp = tempfile::tempdir().unwrap();
        let sounds = sounds_pack(tmp.path());
        let lazy = std::sync::Arc::new(LazyPack::new(tmp.path().join("channel3000.pak")));
        let combined = CombinedPack::new(
            Some(std::sync::Arc::new(
                super::super::SoundPack::open(&sounds).unwrap(),
            )),
            None,
        )
        .with_channel3000(std::sync::Arc::clone(&lazy));
        assert!(combined.read("c3k/day_weigh_in_01.opus").is_none());
        assert!(!combined.has("c3k/day_weigh_in_01.opus"));
        assert_eq!(lazy.opens(), 1, "a missing file is looked for once");
        assert!(combined.read("c3k/day_weigh_in_01.opus").is_none());
        assert_eq!(lazy.opens(), 1);
        // The sounds side is untouched.
        assert_eq!(combined.read("ui/menu_select.ogg").unwrap(), b"menu select");
        assert!(!pack_available(lazy.path()));
    }

    #[test]
    fn c3k_names_never_reach_the_sounds_pack() {
        // Without a Channel 3000 pack wired in, a c3k name is absent even if
        // some other pack happened to carry it.
        let tmp = tempfile::tempdir().unwrap();
        let (c3k, _) = fixture_pack(tmp.path());
        let combined = CombinedPack::new(
            Some(std::sync::Arc::new(
                super::super::SoundPack::open(&c3k).unwrap(),
            )),
            None,
        );
        assert!(combined.read("c3k/id_channel3000_01.opus").is_none());
    }
}
