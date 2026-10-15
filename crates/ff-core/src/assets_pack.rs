//! Packed sound assets for frozen builds.
//!
//! Release builds ship the `assets/sounds` tree as two masked pack files
//! instead of a browsable folder: `freight_fate/music.pak` carries every entry
//! under `music/`, `freight_fate/sounds.pak` carries everything else. The
//! split keeps the small (tens-of-MB) gameplay SFX library out of the much
//! larger (several-hundred-MB) music payload, so a sound-only change does not
//! drag the whole music library with it. Each pack is a
//! deflated zip XOR-masked with a fixed key, so renaming one does not turn it
//! back into an openable archive; this deters casual editing, nothing more.
//! `sounds.pak` is read into memory at startup, because a gameplay cue must
//! never wait on the disk. `music.pak` is not: it is opened like
//! `channel3000.pak` (see [`streamed`]) and each track is read from its own
//! offset when it starts, so the several hundred megabytes of music stay on
//! disk instead of in the game's memory for the whole session.
//! In a source checkout `assets/sounds.pak` is an ordinary committed file and
//! `assets/music.pak` is downloaded by `tools/build_release.py`. Tests can explicitly disable the default packs and exercise the loose-file
//! fallback.
//!
//! A third pack, `channel3000.pak`, carries Channel 3000's clips under
//! `c3k/`. It is never read into memory or opened at startup: see
//! [`streamed`], which opens it on the first `c3k/` lookup.
//!
//! `tools/pack_sounds.py` writes both packs; the audio engine reads them
//! through [`open_default`], which returns one object that routes a lookup to
//! whichever pack carries that name -- callers do not need to know the packs
//! are split. The pack payload is deterministic for identical inputs.
//!
//! Port of `freight_fate/assets_pack.py`. This file also hosts the
//! process-global registry of runtime-synthesized sounds
//! (`audio.register_generated_sound` in Python) -- see the
//! [generated sounds](#generated-sounds) section -- because the asset lookup
//! consults it before either pack.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt;
use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use once_cell::sync::Lazy;
use zip::write::SimpleFileOptions;

mod streamed;
pub use streamed::{channel3000_pack_available, LazyPack, StreamedPack};
use zip::{CompressionMethod, DateTime, ZipArchive, ZipWriter};

pub const PACK_MAGIC: &[u8; 6] = b"FFPK1\0";
pub const DEFAULT_PACK_NAME: &str = "sounds.pak";
pub const DEFAULT_MUSIC_PACK_NAME: &str = "music.pak";

/// The first bytes of a Git LFS pointer file.
///
/// A pointer is a ~130 byte text stub standing in for the real object, and it
/// EXISTS -- which is the whole trap. An existence check alone reads an
/// unmaterialised pack as present, so a test guarded with "if the file is not
/// there, skip" never skips and asserts against 130 bytes of text instead.
///
/// The packs no longer live in Git LFS, but a checkout made while they did
/// can still hold a pointer where `assets/sounds.pak` should be, so the
/// guard stays.
pub const LFS_POINTER_MAGIC: &[u8] = b"version https://git-lfs";

/// Whether `path` is a Git LFS pointer standing in for the real file.
pub fn is_lfs_pointer(path: &Path) -> bool {
    let Ok(mut file) = std::fs::File::open(path) else {
        return false;
    };
    let mut head = [0u8; LFS_POINTER_MAGIC.len()];
    match file.read_exact(&mut head) {
        Ok(()) => head == LFS_POINTER_MAGIC,
        Err(_) => false,
    }
}

/// Whether a pack is really here, rather than an LFS pointer to it.
///
/// What a test that wants the shipped bytes has to ask: `Path::exists` is not
/// enough (see [`LFS_POINTER_MAGIC`]).
pub fn pack_available(path: &Path) -> bool {
    path.exists() && !is_lfs_pointer(path)
}

/// Fixed zip timestamp so identical inputs produce identical packs.
const EPOCH: (u16, u8, u8, u8, u8, u8) = (1980, 1, 1, 0, 0, 0);

const XOR_KEY: [u8; 64] = [
    0x8f, 0x3a, 0x51, 0xc7, 0xe2, 0x94, 0x6d, 0x0b, 0xb8, 0x5f, 0x13, 0xa6, 0xc9, 0x4e, 0x72, 0xd1,
    0x0d, 0x6b, 0x38, 0xf5, 0xa1, 0xc8, 0x4e, 0x97, 0x62, 0x5d, 0x0f, 0x3b, 0xb7, 0xa9, 0xc1, 0xe4,
    0x49, 0xe8, 0xd2, 0x76, 0x1b, 0x5f, 0xa3, 0xc0, 0x87, 0xd4, 0xe9, 0x1f, 0x6a, 0x2c, 0x53, 0xb8,
    0xf0, 0xb6, 0x24, 0x9d, 0xcd, 0x71, 0x83, 0xea, 0x5e, 0x40, 0xf9, 0x2c, 0x37, 0xa8, 0xd1, 0x65,
];

/// XOR `data` with the repeating pack key (symmetric), in place.
pub fn mask_in_place(data: &mut [u8]) {
    for chunk in data.chunks_mut(XOR_KEY.len()) {
        for (byte, key) in chunk.iter_mut().zip(XOR_KEY.iter()) {
            *byte ^= key;
        }
    }
}

/// XOR `data` with the repeating pack key (symmetric).
pub fn mask(data: &[u8]) -> Vec<u8> {
    let mut out = data.to_vec();
    mask_in_place(&mut out);
    out
}

#[derive(Debug)]
pub enum PackError {
    /// The file does not start with [`PACK_MAGIC`].
    NotAPack(PathBuf),
    /// Nothing under the sounds directory to pack.
    NothingToPack(PathBuf),
    Io(std::io::Error),
    Zip(zip::result::ZipError),
}

impl fmt::Display for PackError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotAPack(path) => write!(f, "Not a Freight Fate sound pack: {}", path.display()),
            Self::NothingToPack(dir) => {
                write!(f, "No sound assets to pack under {}", dir.display())
            }
            Self::Io(err) => write!(f, "{err}"),
            Self::Zip(err) => write!(f, "{err}"),
        }
    }
}

impl std::error::Error for PackError {}

impl From<std::io::Error> for PackError {
    fn from(err: std::io::Error) -> Self {
        Self::Io(err)
    }
}

impl From<zip::result::ZipError> for PackError {
    fn from(err: zip::result::ZipError) -> Self {
        Self::Zip(err)
    }
}

/// Every file under `root`, as `(pack-relative posix name, path)`, editor
/// backups (`*.bak`) left out.
fn walk_files(root: &Path) -> std::io::Result<BTreeMap<String, PathBuf>> {
    fn visit(root: &Path, dir: &Path, out: &mut BTreeMap<String, PathBuf>) -> std::io::Result<()> {
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                visit(root, &path, out)?;
            } else if path.is_file() {
                if path.extension().and_then(|e| e.to_str()) == Some("bak") {
                    continue;
                }
                let relative = path.strip_prefix(root).unwrap_or(&path);
                let name = relative
                    .components()
                    .map(|c| c.as_os_str().to_string_lossy().into_owned())
                    .collect::<Vec<_>>()
                    .join("/");
                out.insert(name, path);
            }
        }
        Ok(())
    }
    let mut out = BTreeMap::new();
    visit(root, root, &mut out)?;
    Ok(out)
}

/// `name.rsplit(".", 1)[0]`: the sound KEY a pack-relative name carries.
fn stem(name: &str) -> &str {
    name.rsplit_once('.').map(|(stem, _)| stem).unwrap_or(name)
}

/// Pack files under `sounds_dir` and return the pack path.
///
/// `overlay_dir` (the licensed-audio tree) is merged on top and wins by
/// sound KEY (path stem), not just exact path: the loader prefers ogg over
/// wav inside the pack, so a committed `engine/mid.ogg` fallback would
/// shadow a licensed `engine/mid.wav` if both shipped. A build made on a
/// machine that owns the licensed libraries ships them; a clean clone packs
/// the synthesized fallbacks alone. Editor backups (`*.bak`) never ship:
/// one already rode a builder's loose tree into a released pack.
///
/// `include`, when given, keeps only pack-relative names it accepts --
/// the base tree and overlay are still merged first (by full stem
/// precedence, exactly as with no filter), so an overlay entry routes to
/// whichever pack its own path belongs to. `tools/pack_sounds.py` calls
/// this twice, once per prefix, to split `music/` into its own pack.
pub fn write_pack(
    sounds_dir: &Path,
    output: &Path,
    overlay_dir: Option<&Path>,
    include: Option<&dyn Fn(&str) -> bool>,
) -> Result<PathBuf, PackError> {
    let mut entries = walk_files(sounds_dir)?;
    if let Some(overlay) = overlay_dir.filter(|dir| dir.is_dir()) {
        let overlay_entries = walk_files(overlay)?;
        let overlay_stems: HashSet<String> = overlay_entries
            .keys()
            .map(|name| stem(name).to_string())
            .collect();
        entries.retain(|name, _| !overlay_stems.contains(stem(name)));
        entries.extend(overlay_entries);
    }
    if let Some(include) = include {
        entries.retain(|name, _| include(name));
    }
    if entries.is_empty() {
        return Err(PackError::NothingToPack(sounds_dir.to_path_buf()));
    }
    let (year, month, day, hour, minute, second) = EPOCH;
    let options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .last_modified_time(
            DateTime::from_date_and_time(year, month, day, hour, minute, second)
                .expect("the pack epoch is a valid zip timestamp"),
        );
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    // BTreeMap iterates sorted, like Python's sorted(entries).
    for (name, path) in &entries {
        writer.start_file(name.as_str(), options)?;
        writer.write_all(&std::fs::read(path)?)?;
    }
    let payload = writer.finish()?.into_inner();
    if let Some(parent) = output.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut bytes = Vec::with_capacity(PACK_MAGIC.len() + payload.len());
    bytes.extend_from_slice(PACK_MAGIC);
    bytes.extend_from_slice(&mask(&payload));
    std::fs::write(output, bytes)?;
    Ok(output.to_path_buf())
}

/// Read-only view of a masked sound pack, held in memory.
pub struct SoundPack {
    archive: Mutex<ZipArchive<Cursor<Vec<u8>>>>,
    names: Vec<String>,
    name_set: HashSet<String>,
}

impl SoundPack {
    pub fn open(path: &Path) -> Result<Self, PackError> {
        let raw = std::fs::read(path)?;
        Self::from_bytes(raw).map_err(|err| match err {
            PackError::NotAPack(_) => PackError::NotAPack(path.to_path_buf()),
            other => other,
        })
    }

    /// A pack from its file bytes (magic plus masked payload).
    pub fn from_bytes(raw: Vec<u8>) -> Result<Self, PackError> {
        if !raw.starts_with(PACK_MAGIC) {
            return Err(PackError::NotAPack(PathBuf::from("<bytes>")));
        }
        let mut payload = raw;
        payload.drain(..PACK_MAGIC.len());
        mask_in_place(&mut payload);
        let archive = ZipArchive::new(Cursor::new(payload))?;
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

    /// Whether the pack carries `name`, without decompressing it.
    pub fn has(&self, name: &str) -> bool {
        self.name_set.contains(name)
    }

    /// Bytes for a pack-relative posix path, or None if absent.
    ///
    /// A damaged entry counts as absent, not as an error: the caller then
    /// falls back to the loose sound tree, so one corrupt member costs its
    /// own sound instead of every sound after it.
    pub fn read(&self, name: &str) -> Option<Vec<u8>> {
        if !self.has(name) {
            return None;
        }
        let mut archive = self.archive.lock().unwrap_or_else(|e| e.into_inner());
        let result = archive
            .by_name(name)
            .map_err(PackError::from)
            .and_then(|mut file| {
                let mut data = Vec::with_capacity(file.size() as usize);
                file.read_to_end(&mut data)?;
                Ok(data)
            });
        match result {
            Ok(data) => Some(data),
            Err(err) => {
                log::warn!("Damaged entry in sound pack: {name} ({err})");
                None
            }
        }
    }
}

/// Routes a lookup between the sounds pack, the music pack and Channel
/// 3000's pack by name.
///
/// `c3k/...` names go to Channel 3000's [`LazyPack`], which opens its file
/// on the first such lookup and never otherwise; [`CombinedPack::names`]
/// leaves it out for the same reason.
///
/// Offers the read-only slice of [`SoundPack`] (`names`/`has`/`read`) so a
/// caller that got a single pack back before keeps working unchanged: a
/// `music/...` name resolves from the music pack, any other name from the
/// sounds pack. Either side may be `None` (that pack is missing or
/// unreadable); a lookup that lands on the missing side reports absent, same
/// as a key the pack never carried, so the caller's existing loose-file
/// fallback takes it from there.
pub struct CombinedPack {
    sounds: Option<Arc<SoundPack>>,
    music: Option<MusicSide>,
    channel3000: Option<Arc<LazyPack>>,
}

/// Where the music side of a [`CombinedPack`] reads from. The shipped
/// `music.pak` is [`MusicSide::Streamed`]: about 380 MB that used to sit in
/// memory for the whole session (a quarter of an 8 GB Mac's free memory, for
/// one track playing at a time) now stays on disk until a track starts.
enum MusicSide {
    Memory(Arc<SoundPack>),
    Streamed(Arc<StreamedPack>),
}

impl MusicSide {
    fn names(&self) -> Vec<String> {
        match self {
            Self::Memory(pack) => pack.names(),
            Self::Streamed(pack) => pack.names(),
        }
    }

    fn has(&self, name: &str) -> bool {
        match self {
            Self::Memory(pack) => pack.has(name),
            Self::Streamed(pack) => pack.has(name),
        }
    }

    fn read(&self, name: &str) -> Option<Vec<u8>> {
        match self {
            Self::Memory(pack) => pack.read(name),
            Self::Streamed(pack) => pack.read(name),
        }
    }
}

impl CombinedPack {
    pub fn new(sounds: Option<Arc<SoundPack>>, music: Option<Arc<SoundPack>>) -> Self {
        Self {
            sounds,
            music: music.map(MusicSide::Memory),
            channel3000: None,
        }
    }

    /// The shipped arrangement: sounds in memory, music read from its file
    /// one track at a time.
    pub fn with_streamed_music(
        sounds: Option<Arc<SoundPack>>,
        music: Option<Arc<StreamedPack>>,
    ) -> Self {
        Self {
            sounds,
            music: music.map(MusicSide::Streamed),
            channel3000: None,
        }
    }

    /// Whether the music side reads from its file rather than memory.
    pub fn music_is_streamed(&self) -> bool {
        matches!(self.music, Some(MusicSide::Streamed(_)))
    }

    /// Route `c3k/` names to `pack`, unopened until one is asked for.
    pub fn with_channel3000(mut self, pack: Arc<LazyPack>) -> Self {
        self.channel3000 = Some(pack);
        self
    }

    fn is_channel3000(name: &str) -> bool {
        name.starts_with(crate::channel3000::CLIP_KEY_PREFIX)
    }

    fn is_music(name: &str) -> bool {
        name.starts_with("music/")
    }

    pub fn names(&self) -> Vec<String> {
        let mut names = self.sounds.as_ref().map(|p| p.names()).unwrap_or_default();
        if let Some(music) = &self.music {
            names.extend(music.names());
        }
        names
    }

    pub fn has(&self, name: &str) -> bool {
        if Self::is_channel3000(name) {
            return self.channel3000.as_ref().is_some_and(|pack| pack.has(name));
        }
        if Self::is_music(name) {
            return self.music.as_ref().is_some_and(|pack| pack.has(name));
        }
        self.sounds.as_ref().is_some_and(|pack| pack.has(name))
    }

    pub fn read(&self, name: &str) -> Option<Vec<u8>> {
        if Self::is_channel3000(name) {
            return self.channel3000.as_ref().and_then(|pack| pack.read(name));
        }
        if Self::is_music(name) {
            return self.music.as_ref().and_then(|pack| pack.read(name));
        }
        self.sounds.as_ref().and_then(|pack| pack.read(name))
    }
}

/// Read-and-unmask one pack file, or None when it is absent/unreadable.
fn load_one_pack(path: &Path, label: &str) -> Option<Arc<SoundPack>> {
    if !path.exists() {
        // Once per process: the loader reads the packs a single time. A
        // source run without music.pak (it is builder-local) says so here.
        log::info!(
            "No {label} pack at {}; its sounds come from loose files, if any",
            path.display()
        );
        return None;
    }
    match SoundPack::open(path) {
        Ok(pack) => {
            log::info!(
                "{} pack loaded: {} ({} entries)",
                capitalize(label),
                path.display(),
                pack.names.len()
            );
            Some(Arc::new(pack))
        }
        Err(err) => {
            log::warn!(
                "Unreadable {label} pack at {}; reading loose sound files instead ({err})",
                path.display()
            );
            None
        }
    }
}

/// Open one pack file for reading entry by entry (only its zip directory is
/// read now), or None when it is absent/unreadable. Logs exactly like
/// [`load_one_pack`], so a missing or broken music pack reads the same.
fn stream_one_pack(path: &Path, label: &str) -> Option<Arc<StreamedPack>> {
    if !path.exists() {
        log::info!(
            "No {label} pack at {}; its sounds come from loose files, if any",
            path.display()
        );
        return None;
    }
    match StreamedPack::open(path) {
        Ok(pack) => {
            log::info!(
                "{} pack opened: {} ({} entries, read from disk as they play)",
                capitalize(label),
                path.display(),
                pack.len()
            );
            Some(Arc::new(pack))
        }
        Err(err) => {
            log::warn!(
                "Unreadable {label} pack at {}; reading loose sound files instead ({err})",
                path.display()
            );
            None
        }
    }
}

fn capitalize(label: &str) -> String {
    let mut chars = label.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// The two shipped packs behind one lock.
///
/// Guards the read-and-unmask work and the loaded result. A plain lock
/// (rather than a future/event) doubles as the join: whichever thread -- the
/// background prefetch, or a caller of [`PackLoader::open`] that got there
/// first -- is inside this lock actually reading the packs off disk, every
/// other thread blocks on the lock itself instead of polling anything.
pub struct PackLoader {
    sounds_path: PathBuf,
    music_path: PathBuf,
    /// Channel 3000's pack: handed to every combined view unopened.
    channel3000: Option<Arc<LazyPack>>,
    /// `None` until a load has been attempted; then the combined view, or
    /// `None` when both packs are unusable.
    state: Mutex<Option<Option<Arc<CombinedPack>>>>,
    prefetch_started: AtomicBool,
    loads: AtomicUsize,
}

impl PackLoader {
    pub fn new(sounds_path: impl Into<PathBuf>, music_path: impl Into<PathBuf>) -> Self {
        Self {
            sounds_path: sounds_path.into(),
            music_path: music_path.into(),
            channel3000: None,
            state: Mutex::new(None),
            prefetch_started: AtomicBool::new(false),
            loads: AtomicUsize::new(0),
        }
    }

    /// Route `c3k/` names to `pack` in every view this loader opens.
    pub fn with_channel3000(mut self, pack: Arc<LazyPack>) -> Self {
        self.channel3000 = Some(pack);
        self
    }

    /// How many times the packs were actually read off disk (tests pin this
    /// at one however many threads race for them).
    pub fn loads(&self) -> usize {
        self.loads.load(Ordering::SeqCst)
    }

    /// Whether both packs have already been loaded or given up on.
    pub fn load_attempted(&self) -> bool {
        self.state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_some()
    }

    /// Read and unmask the sounds pack and open the music pack. Caller holds
    /// the lock.
    fn load_locked(&self, state: &mut Option<Option<Arc<CombinedPack>>>) {
        if state.is_some() {
            return; // someone else finished this while we waited for the lock
        }
        self.loads.fetch_add(1, Ordering::SeqCst);
        let sounds = load_one_pack(&self.sounds_path, "sound");
        let music = stream_one_pack(&self.music_path, "music");
        let channel3000 = self
            .channel3000
            .as_ref()
            .filter(|pack| pack.path().exists());
        let combined = if sounds.is_some() || music.is_some() || channel3000.is_some() {
            let mut combined = CombinedPack::with_streamed_music(sounds, music);
            if let Some(pack) = channel3000 {
                combined = combined.with_channel3000(Arc::clone(pack));
            }
            Some(Arc::new(combined))
        } else {
            None
        };
        *state = Some(combined);
    }

    /// Start loading the packs on a background thread.
    ///
    /// The sounds pack gets read fully into memory and XOR unmasked; the
    /// music pack only has its zip directory read (see the module docs).
    /// Read synchronously, that lands on whichever sound plays first
    /// (typically a main-menu sound), stalling it. Called as early as
    /// possible in app construction, this overlaps the reads with the rest
    /// of startup (world load especially) instead of adding to it.
    ///
    /// Safe to call more than once (a no-op once a load has started or
    /// finished) and safe even when there is no pack to load. Never blocks: the
    /// actual wait happens in [`PackLoader::open`], via the same lock, so a
    /// corrupt or half-written pack still logs exactly as it does today
    /// -- just on whichever thread ends up doing the work, main or background.
    pub fn prefetch(self: &Arc<Self>) {
        if self.load_attempted() {
            return;
        }
        if self.prefetch_started.swap(true, Ordering::SeqCst) {
            return;
        }
        let loader = Arc::clone(self);
        let spawned = std::thread::Builder::new()
            .name("ffpack-prefetch".into())
            .spawn(move || {
                let mut state = loader.state.lock().unwrap_or_else(|e| e.into_inner());
                loader.load_locked(&mut state);
            });
        if let Err(err) = spawned {
            // No thread to be had: the first open() pays for the read instead.
            log::warn!("Sound pack prefetch thread failed to start: {err}");
            self.prefetch_started.store(false, Ordering::SeqCst);
        }
    }

    /// The packs, combined behind one lookup, or None if both are unusable.
    ///
    /// An unreadable pack -- a truncated copy, a half-finished download, a file
    /// from another build -- is treated as no pack at all rather than raised.
    /// A source checkout still has its loose sound tree to fall back on, so the
    /// game keeps its sound; a frozen build has nothing to fall back to, but it
    /// says so in the log instead of failing on the first sound it plays. The
    /// two packs fail independently: a missing `music.pak` alongside a good
    /// `sounds.pak` still serves every non-music key from the pack, same as
    /// the reverse.
    ///
    /// If [`PackLoader::prefetch`] already has this underway on a background
    /// thread, this blocks on the lock until that finishes instead of
    /// redoing the read itself -- the first sound request pays only whatever
    /// time is left on the prefetch, not the whole read again.
    pub fn open(&self) -> Option<Arc<CombinedPack>> {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        self.load_locked(&mut state);
        state.as_ref().and_then(|combined| combined.clone())
    }
}

/// Where the shipped packs live: a packaged build puts them under
/// `<exe dir>/freight_fate/`, a source checkout under `assets/`.
/// `FREIGHT_FATE_PACK_DIR` overrides both.
pub fn default_pack_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("FREIGHT_FATE_PACK_DIR") {
        return PathBuf::from(dir);
    }
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(
                crate::data::data_resources::resource_dir_for_executable(
                    &exe,
                    cfg!(target_os = "macos"),
                )
                .join("freight_fate"),
            );
            candidates.push(dir.join("freight_fate"));
            candidates.push(dir.to_path_buf());
        }
    }
    candidates.push(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("assets"),
    );
    candidates
        .iter()
        .find(|dir| {
            dir.join(DEFAULT_PACK_NAME).exists() || dir.join(DEFAULT_MUSIC_PACK_NAME).exists()
        })
        .cloned()
        .unwrap_or_else(|| {
            candidates
                .into_iter()
                .last()
                .expect("at least one candidate")
        })
}

static DEFAULT_LOADER: Lazy<Arc<PackLoader>> = Lazy::new(|| {
    let dir = default_pack_dir();
    Arc::new(
        PackLoader::new(
            dir.join(DEFAULT_PACK_NAME),
            dir.join(DEFAULT_MUSIC_PACK_NAME),
        )
        .with_channel3000(Arc::new(LazyPack::new(
            dir.join(crate::channel3000::CHANNEL_3000_PACK_NAME),
        ))),
    )
});

fn packs_disabled() -> bool {
    std::env::var("FREIGHT_FATE_IGNORE_SOUND_PACK").as_deref() == Ok("1")
}

/// Start loading the shipped packs on a background thread (see
/// [`PackLoader::prefetch`]). `FREIGHT_FATE_IGNORE_SOUND_PACK=1` disables
/// both packs.
pub fn prefetch_default() {
    if packs_disabled() {
        return;
    }
    DEFAULT_LOADER.prefetch();
}

/// The shipped packs, combined behind one lookup, or None if both are
/// unusable or `FREIGHT_FATE_IGNORE_SOUND_PACK=1` (see [`PackLoader::open`]).
pub fn open_default() -> Option<Arc<CombinedPack>> {
    if packs_disabled() {
        return None;
    }
    DEFAULT_LOADER.open()
}

// ---------------------------------------------------------------------------
// Generated sounds
//
// Runtime-synthesized cues (the lane guide tone, the
// enforcement signature) are published under ordinary sound keys and win
// over every pack and loose file: `audio._asset_bytes` checks `_GENERATED`
// first, so a synthesized cue plays through the same path as a packed asset
// on every backend. The Python registry lived in `audio.py`; it sits here so
// the pure synthesis modules can register without an audio device.

type Generated = HashMap<String, (Arc<Vec<u8>>, String)>;

static GENERATED: Lazy<Mutex<Generated>> = Lazy::new(|| Mutex::new(HashMap::new()));
static GENERATED_VERSION: AtomicU64 = AtomicU64::new(0);
/// Per key, the [`GENERATED_VERSION`] at which it last changed. Entries are
/// kept after an unregister so a stamp never repeats for a key.
static KEY_VERSIONS: Lazy<Mutex<HashMap<String, u64>>> = Lazy::new(|| Mutex::new(HashMap::new()));

fn stamp(key: &str) {
    let version = GENERATED_VERSION.fetch_add(1, Ordering::SeqCst) + 1;
    KEY_VERSIONS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(key.to_string(), version);
}

/// Publish synthesized audio under `key` for every backend.
///
/// Moves [`generated_sound_version`] for `key` alone: anything measured or
/// rendered from its old bytes (the audio engine's clip-length and
/// cab-sealed caches) was measuring nothing and must be dropped, which the
/// game crate does by checking that key's version. Other keys' cached
/// entries stand.
pub fn register_generated_sound(key: &str, data: Vec<u8>, ext: &str) {
    GENERATED
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(key.to_string(), (Arc::new(data), ext.to_string()));
    stamp(key);
}

/// Drop a generated sound, e.g. a synthesized piece that has played.
pub fn unregister_generated_sound(key: &str) {
    let removed = GENERATED
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(key)
        .is_some();
    if removed {
        stamp(key);
    }
}

/// The bytes and extension published under `key`, if any.
pub fn generated_sound(key: &str) -> Option<(Arc<Vec<u8>>, String)> {
    GENERATED
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(key)
        .cloned()
}

/// Every published key, sorted.
pub fn generated_sound_keys() -> Vec<String> {
    let mut keys: Vec<String> = GENERATED
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .keys()
        .cloned()
        .collect();
    keys.sort();
    keys
}

/// A stamp that moves whenever `key` is registered or unregistered (0 if it
/// never was), for caches of anything derived from that key's sound.
pub fn generated_sound_version(key: &str) -> u64 {
    KEY_VERSIONS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(key)
        .copied()
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    //! Tests for the masked sound pack used by frozen release builds.
    //!
    //! The `audio._asset_bytes` / `_asset_path` / `verify_sound_assets` tests
    //! of the Python file (pack-then-loose resolution, the engine-ring rule,
    //! the licensed overlay on lookup) drive the audio module, which is the
    //! game crate's; the pack format, split, loader and prefetch are pinned
    //! here. The `tools/pack_sounds.py` split test stays Python.
    use super::*;

    fn write_fixture_sounds(tmp: &Path) -> PathBuf {
        let sounds = tmp.join("sounds");
        std::fs::create_dir_all(sounds.join("ui")).unwrap();
        std::fs::create_dir_all(sounds.join("music")).unwrap();
        std::fs::write(
            sounds.join("ui").join("menu_select.ogg"),
            b"fake ogg for menu select",
        )
        .unwrap();
        std::fs::write(
            sounds.join("music").join("open_road.wav"),
            b"fake wav for open road",
        )
        .unwrap();
        sounds
    }

    fn pack(sounds: &Path, out: &Path) -> SoundPack {
        SoundPack::open(&write_pack(sounds, out, None, None).unwrap()).unwrap()
    }

    #[test]
    fn test_pack_round_trips_files() {
        let tmp = tempfile::tempdir().unwrap();
        let sounds = write_fixture_sounds(tmp.path());
        let pack = pack(&sounds, &tmp.path().join("sounds.pak"));
        let mut names = pack.names();
        names.sort();
        assert_eq!(names, vec!["music/open_road.wav", "ui/menu_select.ogg"]);
        assert_eq!(
            pack.read("ui/menu_select.ogg").unwrap(),
            b"fake ogg for menu select"
        );
        assert_eq!(
            pack.read("music/open_road.wav").unwrap(),
            b"fake wav for open road"
        );
        assert!(pack.read("ui/not_there.ogg").is_none());
    }

    #[test]
    fn test_pack_is_not_a_plain_zip_after_renaming() {
        let tmp = tempfile::tempdir().unwrap();
        let sounds = write_fixture_sounds(tmp.path());
        let out = write_pack(&sounds, &tmp.path().join("sounds.pak"), None, None).unwrap();
        let raw = std::fs::read(&out).unwrap();
        assert!(ZipArchive::new(Cursor::new(raw.clone())).is_err());
        assert!(raw.starts_with(PACK_MAGIC));
        // entry names are masked too
        assert!(!raw.windows(11).any(|w| w == b"menu_select"));
    }

    #[test]
    fn test_pack_overlay_replaces_and_adds() {
        let tmp = tempfile::tempdir().unwrap();
        let sounds = write_fixture_sounds(tmp.path());
        let overlay = tmp.path().join("licensed");
        std::fs::create_dir_all(overlay.join("ui")).unwrap();
        std::fs::create_dir_all(overlay.join("engine")).unwrap();
        std::fs::write(
            overlay.join("ui").join("menu_select.ogg"),
            b"licensed menu select",
        )
        .unwrap();
        std::fs::write(
            overlay.join("engine").join("low.ogg"),
            b"licensed engine low",
        )
        .unwrap();
        let out = write_pack(
            &sounds,
            &tmp.path().join("sounds.pak"),
            Some(&overlay),
            None,
        )
        .unwrap();
        let pack = SoundPack::open(&out).unwrap();
        assert_eq!(
            pack.read("ui/menu_select.ogg").unwrap(),
            b"licensed menu select"
        ); // replaced
        assert_eq!(pack.read("engine/low.ogg").unwrap(), b"licensed engine low"); // added
        assert_eq!(
            pack.read("music/open_road.wav").unwrap(),
            b"fake wav for open road"
        ); // untouched
    }

    #[test]
    fn test_pack_overlay_wins_by_key_across_extensions() {
        // The loader tries ogg before wav inside the pack, so a committed ogg
        // fallback must not ship beside a licensed wav for the same key.
        let tmp = tempfile::tempdir().unwrap();
        let sounds = write_fixture_sounds(tmp.path());
        let overlay = tmp.path().join("licensed");
        std::fs::create_dir_all(overlay.join("ui")).unwrap();
        std::fs::write(overlay.join("ui").join("menu_select.wav"), b"licensed wav").unwrap();
        let out = write_pack(
            &sounds,
            &tmp.path().join("sounds.pak"),
            Some(&overlay),
            None,
        )
        .unwrap();
        let pack = SoundPack::open(&out).unwrap();
        assert_eq!(pack.read("ui/menu_select.wav").unwrap(), b"licensed wav");
        assert!(pack.read("ui/menu_select.ogg").is_none()); // stale-extension twin dropped
    }

    #[test]
    fn test_pack_excludes_editor_backups() {
        // A jake .bak from a builder's loose tree once rode into a released pack;
        // backups stay out of the payload, from the committed tree and the
        // licensed overlay both.
        let tmp = tempfile::tempdir().unwrap();
        let sounds = write_fixture_sounds(tmp.path());
        std::fs::write(
            sounds.join("ui").join("menu_select.ogg.bak"),
            b"stale backup",
        )
        .unwrap();
        let overlay = tmp.path().join("licensed");
        std::fs::create_dir_all(overlay.join("engine")).unwrap();
        std::fs::write(
            overlay.join("engine").join("low.ogg"),
            b"licensed engine low",
        )
        .unwrap();
        std::fs::write(
            overlay.join("engine").join("jake.synth-original.wav.bak"),
            b"synth original",
        )
        .unwrap();
        let out = write_pack(
            &sounds,
            &tmp.path().join("sounds.pak"),
            Some(&overlay),
            None,
        )
        .unwrap();
        let names = SoundPack::open(&out).unwrap().names();
        assert!(!names.iter().any(|n| n.ends_with(".bak")));
        assert!(names.contains(&"engine/low.ogg".to_string()));
        assert!(names.contains(&"ui/menu_select.ogg".to_string()));
    }

    #[test]
    fn test_pack_missing_overlay_dir_is_fine() {
        let tmp = tempfile::tempdir().unwrap();
        let sounds = write_fixture_sounds(tmp.path());
        let out = write_pack(
            &sounds,
            &tmp.path().join("sounds.pak"),
            Some(&tmp.path().join("not_there")),
            None,
        )
        .unwrap();
        let mut names = SoundPack::open(&out).unwrap().names();
        names.sort();
        assert_eq!(names, vec!["music/open_road.wav", "ui/menu_select.ogg"]);
    }

    #[test]
    fn test_pack_is_deterministic() {
        let tmp = tempfile::tempdir().unwrap();
        let sounds = write_fixture_sounds(tmp.path());
        let first =
            std::fs::read(write_pack(&sounds, &tmp.path().join("a.pak"), None, None).unwrap())
                .unwrap();
        let second =
            std::fs::read(write_pack(&sounds, &tmp.path().join("b.pak"), None, None).unwrap())
                .unwrap();
        assert_eq!(first, second);
    }

    #[test]
    fn test_pack_include_filter_splits_by_prefix() {
        // What tools/pack_sounds.py does twice to split music/ into its own pack.
        let tmp = tempfile::tempdir().unwrap();
        let sounds = write_fixture_sounds(tmp.path());
        let is_music = |name: &str| name.starts_with("music/");
        let not_music = |name: &str| !name.starts_with("music/");
        let music = pack_with(&sounds, &tmp.path().join("music.pak"), &is_music);
        let rest = pack_with(&sounds, &tmp.path().join("sounds.pak"), &not_music);
        assert_eq!(music.names(), vec!["music/open_road.wav"]);
        assert_eq!(rest.names(), vec!["ui/menu_select.ogg"]);
    }

    fn pack_with(sounds: &Path, out: &Path, include: &dyn Fn(&str) -> bool) -> SoundPack {
        SoundPack::open(&write_pack(sounds, out, None, Some(include)).unwrap()).unwrap()
    }

    #[test]
    fn test_mask_is_symmetric_and_chunked() {
        let data: Vec<u8> = (0..=255u8).cycle().take(1000).collect();
        let masked = mask(&data);
        assert_ne!(masked, data);
        assert_eq!(mask(&masked), data);
        assert!(mask(&[]).is_empty());
    }

    fn committed_pack_dir() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets")
    }

    /// Whether the shipped pack at `path` is really here, saying out loud
    /// why it is not when it is not.
    ///
    /// The skip has to be audible. A checkout without LFS leaves a pointer
    /// where the pack should be, and a pointer is a file that EXISTS -- so
    /// the old "if it is not there, skip" guard never fired and these
    /// assertions ran against 130 bytes of pointer text. Now they skip, and
    /// a run that quietly tested nothing would be the worse failure, so the
    /// reason goes in the log naming the pointer.
    fn committed_pack(path: &Path) -> bool {
        if pack_available(path) {
            return true;
        }
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        if is_lfs_pointer(path) {
            eprintln!(
                "SKIPPING {}: it is a leftover Git LFS pointer, not the pack. The \
                 packs are plain files now: `git checkout -- assets/{name}` restores \
                 sounds.pak, and tools/build_release.py downloads music.pak.",
                path.display()
            );
        } else {
            eprintln!(
                "SKIPPING {}: not present (sounds.pak is committed; music.pak is \
                 builder-local and tools/build_release.py downloads it)",
                path.display()
            );
        }
        false
    }

    /// A Git LFS pointer, byte for byte what a checkout without LFS leaves
    /// behind: three lines, about 130 bytes, where the object should be.
    fn write_lfs_pointer(path: &Path, oid: &str, size: u64) {
        std::fs::write(
            path,
            format!("version https://git-lfs.github.com/spec/v1\noid sha256:{oid}\nsize {size}\n"),
        )
        .unwrap();
    }

    #[test]
    fn test_an_lfs_pointer_is_not_an_available_pack() {
        // The trap this guard exists for: a pointer is a file that EXISTS,
        // so `Path::exists` alone reads an unmaterialised pack as present.
        //
        // Written into a temp directory, never over the shipped packs: the
        // working tree holds the only local copy (music.pak is a 250 MB
        // download).
        let tmp = tempfile::tempdir().unwrap();
        let pointer = tmp.path().join(DEFAULT_PACK_NAME);
        write_lfs_pointer(&pointer, &"a".repeat(64), 7_781_859);

        assert!(pointer.exists(), "the trap: the pointer is a file");
        assert!(pointer.metadata().unwrap().len() < 200, "~130 bytes");
        assert!(is_lfs_pointer(&pointer));
        assert!(!pack_available(&pointer), "a pointer is not the pack");

        // So the header tests refuse to run against it rather than asserting
        // a byte length over 130 bytes of pointer text.
        assert!(!committed_pack(&pointer));
        // Opening it as a pack fails, which is what every asset lookup would
        // have done if the guard had stayed an existence check.
        assert!(SoundPack::open(&pointer).is_err());
    }

    #[test]
    fn test_a_real_pack_and_a_missing_one_are_both_read_correctly() {
        let tmp = tempfile::tempdir().unwrap();
        let sounds = write_fixture_sounds(tmp.path());
        let real = write_pack(&sounds, &tmp.path().join(DEFAULT_PACK_NAME), None, None).unwrap();
        assert!(!is_lfs_pointer(&real));
        assert!(pack_available(&real), "a real pack is available");
        assert!(committed_pack(&real));

        // Absent is not a pointer, and still not available: the two reasons
        // stay distinguishable, which is what lets the skip note say which.
        let missing = tmp.path().join("never_written.pak");
        assert!(!is_lfs_pointer(&missing));
        assert!(!pack_available(&missing));
        assert!(!committed_pack(&missing));
    }

    #[test]
    fn test_committed_pack_has_freight_fate_header() {
        let path = committed_pack_dir().join(DEFAULT_PACK_NAME);
        if !committed_pack(&path) {
            return;
        }
        let pack_bytes = std::fs::read(&path).unwrap();
        assert!(pack_bytes.starts_with(PACK_MAGIC));
        // The release ships THIS pack, so what has to hold is that it carries
        // every cue the game teaches -- the failure that happened was cues
        // missing from it (the scale lights, 2026-08-29) and a re-bake from
        // the incomplete loose tree dropping 60 effects. A deliberate swap of
        // one sound is not a failure, so size and hash are not pinned.
        let pack = SoundPack::open(&path).unwrap();
        // Synthesized cues are not packed: the guide tone publishes itself
        // here, and enforcement/ is the game crate's siren signature.
        crate::lane_guide_tone::register_lane_guide_tone();
        let ships = |key: &str| {
            !key.is_empty()
                && (key.starts_with("enforcement/")
                    || ["ogg", "wav"]
                        .iter()
                        .any(|ext| pack.has(&format!("{key}.{ext}")))
                    || generated_sound(key).is_some())
        };
        let missing: Vec<&str> = crate::sound_catalog::catalog_entries()
            .flat_map(|entry| entry.plays.iter())
            .filter(|cue| !ships(cue.key) && !ships(cue.fallback))
            .map(|cue| cue.key)
            .collect();
        assert!(
            missing.is_empty(),
            "taught but not in sounds.pak: {missing:?}"
        );
    }

    #[test]
    fn test_committed_music_pack_has_freight_fate_header() {
        let path = committed_pack_dir().join(DEFAULT_MUSIC_PACK_NAME);
        if !committed_pack(&path) {
            return;
        }
        // Only the header: tools/build_release.py checks the download against
        // DEFAULT_MUSIC_SHA256 before every release build.
        let mut head = [0u8; 6];
        std::fs::File::open(&path)
            .unwrap()
            .read_exact(&mut head)
            .unwrap();
        assert_eq!(&head, PACK_MAGIC);
    }

    #[test]
    fn test_unreadable_pack_falls_back_to_loose_files() {
        // A truncated or half-copied pack must not take the sound with it: a
        // source checkout still has the real tree, and it has to keep playing.
        let tmp = tempfile::tempdir().unwrap();
        let broken = tmp.path().join("sounds.pak");
        let mut bytes = PACK_MAGIC.to_vec();
        bytes.extend_from_slice(b"not a zip, only noise");
        std::fs::write(&broken, bytes).unwrap();
        let loader = PackLoader::new(&broken, tmp.path().join("__no_music_pack__.pak"));
        assert!(loader.open().is_none());
        assert!(loader.load_attempted());
    }

    #[test]
    fn test_pack_from_another_program_falls_back_to_loose_files() {
        // Wrong magic entirely -- someone renamed an unrelated file into place.
        let tmp = tempfile::tempdir().unwrap();
        let stranger = tmp.path().join("sounds.pak");
        std::fs::write(&stranger, b"PK\x03\x04 whatever this is").unwrap();
        let loader = PackLoader::new(&stranger, tmp.path().join("__no_music_pack__.pak"));
        assert!(loader.open().is_none());
        assert!(matches!(
            SoundPack::open(&stranger),
            Err(PackError::NotAPack(_))
        ));
    }

    #[test]
    fn test_prefetch_default_loads_once_for_concurrent_callers() {
        // The background prefetch and every racing open() caller must all see
        // the one real load: the read-and-unmask work runs exactly once, and
        // everyone gets the same pack instance back.
        let tmp = tempfile::tempdir().unwrap();
        let sounds = write_fixture_sounds(tmp.path());
        let out = write_pack(&sounds, &tmp.path().join("sounds.pak"), None, None).unwrap();
        let loader = Arc::new(PackLoader::new(
            &out,
            tmp.path().join("__no_music_pack__.pak"),
        ));

        loader.prefetch();

        let workers: Vec<_> = (0..5)
            .map(|_| {
                let loader = Arc::clone(&loader);
                std::thread::spawn(move || loader.open())
            })
            .collect();
        let results: Vec<Option<Arc<CombinedPack>>> =
            workers.into_iter().map(|w| w.join().unwrap()).collect();

        assert_eq!(loader.loads(), 1); // read off disk exactly once, not once per caller
        let first = results[0].as_ref().unwrap();
        assert!(results
            .iter()
            .all(|r| Arc::ptr_eq(r.as_ref().unwrap(), first)));
        assert_eq!(
            first.read("ui/menu_select.ogg").unwrap(),
            b"fake ogg for menu select"
        );
    }

    #[test]
    fn test_prefetch_default_is_a_harmless_noop_when_called_twice() {
        let tmp = tempfile::tempdir().unwrap();
        let sounds = write_fixture_sounds(tmp.path());
        let out = write_pack(&sounds, &tmp.path().join("sounds.pak"), None, None).unwrap();
        let loader = Arc::new(PackLoader::new(
            &out,
            tmp.path().join("__no_music_pack__.pak"),
        ));
        loader.prefetch();
        loader.prefetch(); // must not start a second thread/load
        let pack = loader.open().unwrap();
        assert_eq!(
            pack.read("ui/menu_select.ogg").unwrap(),
            b"fake ogg for menu select"
        );
        assert_eq!(loader.loads(), 1);
    }

    #[test]
    fn test_prefetch_with_unreadable_pack_still_falls_back_via_open_default() {
        // A corrupt pack found by the background prefetch must land exactly
        // where it does today: no pack, loose files answer, nothing raised.
        let tmp = tempfile::tempdir().unwrap();
        let broken = tmp.path().join("sounds.pak");
        let mut bytes = PACK_MAGIC.to_vec();
        bytes.extend_from_slice(b"not a zip, only noise");
        std::fs::write(&broken, bytes).unwrap();
        let loader = Arc::new(PackLoader::new(
            &broken,
            tmp.path().join("__no_music_pack__.pak"),
        ));
        loader.prefetch();
        assert!(loader.open().is_none());
    }

    #[test]
    fn test_damaged_entry_costs_only_its_own_sound() {
        // A member whose deflate stream is corrupt reads as absent; its
        // neighbours are unharmed. Built by hand: a valid pack, then the
        // compressed bytes of one member scribbled over.
        let tmp = tempfile::tempdir().unwrap();
        let sounds = write_fixture_sounds(tmp.path());
        std::fs::write(
            sounds.join("ui").join("menu_select.ogg"),
            b"fake ogg for menu select, long enough to deflate into something scribblable"
                .repeat(8),
        )
        .unwrap();
        let out = write_pack(&sounds, &tmp.path().join("sounds.pak"), None, None).unwrap();
        let mut raw = std::fs::read(&out).unwrap();
        let mut payload = raw.split_off(PACK_MAGIC.len());
        mask_in_place(&mut payload);
        // Local file header of the first (sorted: music/...) entry is at 0; the
        // second member (ui/menu_select.ogg) follows it. Scribble a run of bytes
        // well inside the second member's compressed data.
        let marker = b"ui/menu_select.ogg";
        let header_at = payload
            .windows(marker.len())
            .position(|w| w == marker)
            .expect("local header name");
        let data_start = header_at + marker.len();
        for byte in &mut payload[data_start + 4..data_start + 24] {
            *byte = 0xA5;
        }
        let pack =
            SoundPack::from_bytes([PACK_MAGIC.as_slice(), &mask(&payload)].concat()).unwrap();
        assert!(pack.read("ui/menu_select.ogg").is_none()); // damaged, reported as absent
        assert_eq!(
            pack.read("music/open_road.wav").unwrap(),
            b"fake wav for open road"
        ); // unharmed
    }

    fn write_split_fixture_packs(tmp: &Path) -> (PathBuf, PathBuf) {
        let sounds_src = tmp.join("sounds_src");
        std::fs::create_dir_all(sounds_src.join("engine")).unwrap();
        std::fs::write(
            sounds_src.join("engine").join("y.ogg"),
            b"engine sound bytes",
        )
        .unwrap();
        let sounds_out = write_pack(&sounds_src, &tmp.join("sounds.pak"), None, None).unwrap();

        let music_src = tmp.join("music_src");
        std::fs::create_dir_all(music_src.join("music")).unwrap();
        std::fs::write(music_src.join("music").join("x.ogg"), b"music track bytes").unwrap();
        let music_out = write_pack(&music_src, &tmp.join("music.pak"), None, None).unwrap();
        (sounds_out, music_out)
    }

    #[test]
    fn test_loader_routes_music_names_to_music_pack() {
        let tmp = tempfile::tempdir().unwrap();
        let (sounds_out, music_out) = write_split_fixture_packs(tmp.path());
        let loader = PackLoader::new(&sounds_out, &music_out);
        let combined = loader.open().unwrap();
        assert!(combined.music_is_streamed(), "music.pak must stay on disk");
        assert_eq!(combined.read("music/x.ogg").unwrap(), b"music track bytes");
        assert_eq!(
            combined.read("engine/y.ogg").unwrap(),
            b"engine sound bytes"
        );
        assert!(combined.has("music/x.ogg") && !combined.has("engine/x.ogg"));
        assert!(combined.has("engine/y.ogg") && !combined.has("music/y.ogg"));
        let mut names = combined.names();
        names.sort();
        assert_eq!(names, vec!["engine/y.ogg", "music/x.ogg"]);
    }

    #[test]
    fn test_a_streamed_track_longer_than_the_read_ahead_is_byte_exact() {
        // A track is unmasked as it comes off the disk, buffer by buffer, so
        // every buffer has to pick the key up at the right byte. Stored, not
        // deflated, would hide nothing either way: a pseudo-random payload
        // does not compress, so the masked bytes are the track's own.
        let tmp = tempfile::tempdir().unwrap();
        let (sounds_out, _) = write_split_fixture_packs(tmp.path());
        let music_src = tmp.path().join("long_music_src");
        std::fs::create_dir_all(music_src.join("music")).unwrap();
        let mut state = 0x2545_f491_4f6c_dd1du64;
        let track: Vec<u8> = (0..1_300_007)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                (state >> 24) as u8
            })
            .collect();
        std::fs::write(music_src.join("music").join("long.ogg"), &track).unwrap();
        let music_out = write_pack(&music_src, &tmp.path().join("music.pak"), None, None).unwrap();
        let combined = PackLoader::new(&sounds_out, &music_out).open().unwrap();
        assert!(combined.music_is_streamed());
        let read = combined.read("music/long.ogg").unwrap();
        assert_eq!(read.len(), track.len());
        assert!(
            read == track,
            "the streamed track differs from what was packed"
        );
        // And again, from wherever the first read left the file.
        assert!(combined.read("music/long.ogg").unwrap() == track);
    }

    #[test]
    fn test_missing_music_pack_falls_back_while_sounds_pack_still_serves() {
        // The music side of the combined pack answers nothing when music.pak
        // itself is missing, while the sounds side is untouched -- the audio
        // engine takes it from there to the loose tree.
        captured_logs(); // install the capture before the load
        let tmp = tempfile::tempdir().unwrap();
        let (sounds_out, _music_out) = write_split_fixture_packs(tmp.path());
        let loader = PackLoader::new(&sounds_out, tmp.path().join("no_music_here.pak"));
        let combined = loader.open().unwrap(); // the sounds side is still good
        assert_eq!(
            combined.read("engine/y.ogg").unwrap(),
            b"engine sound bytes"
        );
        assert!(combined.read("music/x.ogg").is_none());
        assert!(!combined.has("music/x.ogg"));
        loader.open();
        // The session log says the music pack is missing, once.
        let missing = tmp.path().join("no_music_here.pak").display().to_string();
        let said: Vec<String> = captured_logs()
            .into_iter()
            .filter(|line| line.contains(&missing))
            .collect();
        assert_eq!(said.len(), 1, "{said:?}");
        assert!(said[0].starts_with("INFO No music pack at "), "{said:?}");
    }

    /// Every log record this test binary has emitted since the capture was
    /// installed (on first call), as "LEVEL message". No other ff-core unit
    /// test installs a logger.
    fn captured_logs() -> Vec<String> {
        struct Capture;
        static LINES: Lazy<Mutex<Vec<String>>> = Lazy::new(|| Mutex::new(Vec::new()));
        impl log::Log for Capture {
            fn enabled(&self, _: &log::Metadata) -> bool {
                true
            }
            fn log(&self, record: &log::Record) {
                LINES
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .push(format!("{} {}", record.level(), record.args()));
            }
            fn flush(&self) {}
        }
        static CAPTURE: Capture = Capture;
        if log::set_logger(&CAPTURE).is_ok() {
            log::set_max_level(log::LevelFilter::Info);
        }
        LINES.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    #[test]
    fn test_generated_sounds_registry_wins_and_lists_sorted() {
        let before = generated_sound_version("test_registry/alpha");
        let other = generated_sound_version("test_registry/untouched");
        register_generated_sound("test_registry/zeta", b"zz".to_vec(), "wav");
        register_generated_sound("test_registry/alpha", b"aa".to_vec(), "wav");
        let (data, ext) = generated_sound("test_registry/alpha").unwrap();
        assert_eq!(data.as_slice(), b"aa");
        assert_eq!(ext, "wav");
        assert!(generated_sound("test_registry/none").is_none());
        let keys = generated_sound_keys();
        let a = keys
            .iter()
            .position(|k| k == "test_registry/alpha")
            .unwrap();
        let z = keys.iter().position(|k| k == "test_registry/zeta").unwrap();
        assert!(a < z);
        assert!(generated_sound_version("test_registry/alpha") > before);
        // Only the registered key moves; a cache of any other stays valid.
        assert_eq!(generated_sound_version("test_registry/untouched"), other);
    }
}
