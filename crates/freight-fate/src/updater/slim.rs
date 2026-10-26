//! Smaller updates: keep the sound packs the player already has.
//!
//! Most of a release archive is two packs (`music.pak`, `channel3000.pak`)
//! that rarely change between builds. A zip release (Windows, Mac) lists
//! every file's offset, size and CRC-32 in its index at the end, and
//! GitHub serves byte ranges of a release asset. So the update check reads
//! that index (a few kilobytes), and when the player's packs have the same
//! size and CRC-32 as the archive's, the download fetches only the byte
//! ranges around them and appends a new index ([`zip_index`]): a complete
//! archive minus the packs, unpacked as usual.
//!
//! The check trusts the install's `build_info.json`, which records its
//! packs' sizes and CRC-32s, when the file sizes agree, so it reads no
//! pack; it reads a pack only when the install predates that record. The
//! download only confirms each pack is still there at the same size.
//! Anything off, before or during the download, takes the full archive.
//!
//! A Linux tarball has no index and an AppImage is one file, so both always
//! download in full.
//!
//! Applying needs nothing new on Windows: the apply script copies the update
//! over the install folder, which keeps its packs. A Mac update swaps the
//! whole bundle, so [`complete_staged`] copies the player's packs into the
//! staged bundle first.

use std::fs;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::AtomicBool;

use serde_json::Value;

use super::zip_index::{self, Entry};
use super::{
    resource_root_in, running_appimage_path, stream_to_file, DownloadError, Platform, UpdateInfo,
    UpdaterEnv, DOWNLOAD_IDLE_TIMEOUT, USER_AGENT,
};
use crate::net::{self, NetError, Tier};

/// The packs an update skips, relative to the payload root (beside the
/// executable, or a Mac bundle's `Contents/Resources`).
pub const SKIPPED_PACKS: [&str; 2] = ["freight_fate/music.pak", "freight_fate/channel3000.pak"];

/// One pack an update leaves out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackRef {
    /// Relative to the payload root, `/`-separated: `freight_fate/music.pak`.
    pub path: String,
    pub size: u64,
    pub crc32: u32,
}

/// The parts of the release archive to fetch, when the player's packs match.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SlimUpdate {
    /// Half-open byte ranges of the full archive, in order.
    pub ranges: Vec<(u64, u64)>,
    /// The index to append after the ranges.
    pub directory: Vec<u8>,
    /// Where each kept file's local header lands, to check the result.
    pub local_headers: Vec<u64>,
    /// The packs left out.
    pub packs: Vec<PackRef>,
}

impl SlimUpdate {
    /// Bytes the download fetches.
    pub fn fetch_len(&self) -> u64 {
        self.ranges.iter().map(|(start, end)| end - start).sum()
    }
}

/// A safe relative path: plain names only, so a record can never point
/// outside the install.
fn relative_path(path: &str) -> Option<PathBuf> {
    let relative = PathBuf::from(path);
    let plain = !path.is_empty()
        && relative
            .components()
            .all(|component| matches!(component, Component::Normal(_)));
    plain.then_some(relative)
}

fn parse_pack(value: &Value) -> Option<PackRef> {
    let path = value.get("path")?.as_str()?;
    relative_path(path)?;
    Some(PackRef {
        path: path.to_string(),
        size: value.get("size")?.as_u64()?,
        crc32: u32::try_from(value.get("crc32")?.as_u64()?).ok()?,
    })
}

/// The packs `build_info.json` records; `None` when the list is missing or
/// any entry is malformed (an install from before the record had CRC-32s).
pub fn parse_packs(value: &Value) -> Option<Vec<PackRef>> {
    value
        .get("packs")?
        .as_array()?
        .iter()
        .map(parse_pack)
        .collect::<Option<Vec<_>>>()
        .filter(|packs| !packs.is_empty())
}

/// CRC-32 of the file at `path`.
pub fn file_crc32(path: &Path) -> io::Result<u32> {
    let mut file = fs::File::open(path)?;
    let mut hasher = crc32fast::Hasher::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher.finalize())
}

/// Whether the install still has `pack` at its expected size.
fn installed_size_matches(root: &Path, pack: &PackRef) -> bool {
    relative_path(&pack.path).is_some_and(|relative| {
        fs::metadata(root.join(relative)).map(|m| m.len()).ok() == Some(pack.size)
    })
}

/// Whether the install's copy of `pack` is the one in the release. With
/// `recorded` (the install's own `build_info.json` packs), a matching size
/// and recorded CRC-32 are trusted without reading the pack.
fn installed_matches(root: &Path, pack: &PackRef, recorded: Option<&[PackRef]>) -> bool {
    if !installed_size_matches(root, pack) {
        return false;
    }
    if recorded.is_some_and(|recorded| recorded.contains(pack)) {
        return true;
    }
    let path = root.join(&pack.path);
    match file_crc32(&path) {
        Ok(crc) => crc == pack.crc32,
        Err(e) => {
            log::warn!("Could not read {}: {e}", path.display());
            false
        }
    }
}

fn recorded_packs(root: &Path) -> Option<Vec<PackRef>> {
    let text = fs::read_to_string(root.join("build_info.json")).ok()?;
    parse_packs(&serde_json::from_str(&text).ok()?)
}

/// The index entry for each skipped pack, by its path's ending: the archive
/// holds them under `FreightFate/` or `FreightFate.app/Contents/Resources/`.
fn pack_entries(entries: &[Entry]) -> Option<Vec<(usize, PackRef)>> {
    SKIPPED_PACKS
        .iter()
        .map(|pack| {
            let suffix = format!("/{pack}");
            let mut found = entries
                .iter()
                .enumerate()
                .filter(|(_, entry)| entry.name.ends_with(&suffix));
            let (i, entry) = found.next()?;
            if found.next().is_some() {
                return None;
            }
            Some((
                i,
                PackRef {
                    path: (*pack).to_string(),
                    size: entry.size,
                    crc32: entry.crc32,
                },
            ))
        })
        .collect()
}

/// Read a byte range of a release asset: `fetch(url, start, end)`.
pub type RangeFetch<'a> = &'a dyn Fn(&str, u64, u64) -> Result<Vec<u8>, NetError>;

/// The archive's index entries and end record, read from its last bytes.
fn read_index(info: &UpdateInfo, fetch: RangeFetch) -> Option<(Vec<Entry>, zip_index::End)> {
    let size = u64::try_from(info.asset_size).ok().filter(|n| *n > 0)?;
    let tail_start = size.saturating_sub(zip_index::TAIL_LEN);
    let tail = fetch(&info.asset_url, tail_start, size)
        .map_err(|e| log::warn!("Could not read the update's index: {e:?}"))
        .ok()?;
    let end = zip_index::find_end(&tail)?;
    let stop = end.directory_offset.checked_add(end.directory_len)?;
    let directory = if end.directory_offset >= tail_start && stop <= size {
        let from = usize::try_from(end.directory_offset - tail_start).ok()?;
        tail.get(from..from + usize::try_from(end.directory_len).ok()?)?
            .to_vec()
    } else {
        fetch(&info.asset_url, end.directory_offset, stop)
            .map_err(|e| log::warn!("Could not read the update's index: {e:?}"))
            .ok()?
    };
    Some((zip_index::parse_directory(&directory, end)?, end))
}

/// Set `info.slim` when the release is a zip whose packs match this
/// install's; `fetch` reads byte ranges (injectable for tests).
pub fn resolve_with(info: &mut UpdateInfo, env: &UpdaterEnv, fetch: RangeFetch) {
    info.slim = None;
    // An AppImage is one file, and a tarball has no index to skip by.
    if !info.asset_name.ends_with(".zip")
        || running_appimage_path(env.appimage.as_deref()).is_some()
    {
        return;
    }
    let Some((entries, end)) = read_index(info, fetch) else {
        log::info!("Offering the full update; its index could not be used");
        return;
    };
    let Some(packs) = pack_entries(&entries) else {
        log::info!("Offering the full update; it does not carry both sound packs");
        return;
    };
    let root = resource_root_in(env);
    let recorded = recorded_packs(&root);
    if !packs
        .iter()
        .all(|(_, pack)| installed_matches(&root, pack, recorded.as_deref()))
    {
        log::info!("Offering the full update; this install's sound packs differ");
        return;
    }
    let skip: Vec<usize> = packs.iter().map(|(i, _)| *i).collect();
    let Some(plan) = zip_index::plan(&entries, end, &skip) else {
        log::info!("Offering the full update; its index could not be planned");
        return;
    };
    let slim = SlimUpdate {
        ranges: plan.ranges,
        directory: plan.directory,
        local_headers: plan.local_headers,
        packs: packs.into_iter().map(|(_, pack)| pack).collect(),
    };
    log::info!(
        "Offering {} without its sound packs ({} of {} bytes); this install's packs match",
        info.asset_name,
        slim.fetch_len(),
        info.asset_size
    );
    info.slim = Some(slim);
}

/// [`resolve_with`] over the network.
pub fn resolve(info: &mut UpdateInfo, env: &UpdaterEnv) {
    resolve_with(info, env, &|url, start, end| {
        let headers = vec![
            ("User-Agent".to_string(), USER_AGENT.to_string()),
            ("Range".to_string(), format!("bytes={start}-{}", end - 1)),
        ];
        let response = net::request(Tier::GitHub, Some("GET"), url, None, &headers)?;
        let expected = usize::try_from(end - start).unwrap_or(usize::MAX);
        if response.status != 206 || response.body.len() != expected {
            return Err(NetError::Other {
                type_name: "RangeNotServed".to_string(),
                message: format!(
                    "asked for {expected} bytes, got status {} with {}",
                    response.status,
                    response.body.len()
                ),
            });
        }
        Ok(response.body)
    });
}

/// The slim update to download, when the install still has every pack the
/// update check matched; `None` means fetch the full archive.
pub fn still_matches<'a>(info: &'a UpdateInfo, env: &UpdaterEnv) -> Option<&'a SlimUpdate> {
    let slim = info.slim.as_ref()?;
    let root = resource_root_in(env);
    if slim
        .packs
        .iter()
        .all(|pack| installed_size_matches(&root, pack))
    {
        Some(slim)
    } else {
        log::warn!("A sound pack changed since the update check; downloading the full update");
        None
    }
}

/// Open a byte range of a release asset as a stream:
/// `open(url, start, end)`.
pub type RangeOpen<'a> = &'a dyn Fn(&str, u64, u64) -> Result<Box<dyn Read + Send>, DownloadError>;

/// Fetch `slim`'s ranges into `dest_dir/<asset name>` and append its index:
/// the release archive without the packs. Progress counts against the
/// ranges' total; each range is bounded by the download's idle timeout.
pub fn download_with(
    info: &UpdateInfo,
    slim: &SlimUpdate,
    dest_dir: &Path,
    mut progress: Option<&mut dyn FnMut(u64, u64)>,
    cancelled: Option<&AtomicBool>,
    open: RangeOpen,
) -> Result<PathBuf, DownloadError> {
    let dest = dest_dir.join(&info.asset_name);
    let total = slim.fetch_len();
    // Read back at the end to check the kept files' headers.
    let file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(true)
        .open(&dest)?;
    let mut done = 0u64;
    for &(start, end) in &slim.ranges {
        let reader = open(&info.asset_url, start, end)?;
        let base = done;
        let mut on_range = |got: u64, _: u64| {
            if let Some(progress) = progress.as_deref_mut() {
                progress(base + got, total);
            }
        };
        stream_to_file(
            reader,
            file.try_clone()?,
            total,
            Some(&mut on_range),
            cancelled,
            DOWNLOAD_IDLE_TIMEOUT,
        )?;
        done += end - start;
        if file.metadata()?.len() != done {
            log::warn!("Update range {start}..{end} arrived short");
            return Err(DownloadError::Corrupt);
        }
    }
    let mut file = file;
    io::Write::write_all(&mut file, &slim.directory)?;
    io::Write::flush(&mut file)?;
    // Every kept file must start where the new index says it does.
    let mut signature = [0u8; 4];
    for &at in &slim.local_headers {
        file.seek(SeekFrom::Start(at))?;
        file.read_exact(&mut signature)?;
        if signature != zip_index::LOCAL_SIG {
            log::warn!("Update archive has no file header at {at}");
            return Err(DownloadError::Corrupt);
        }
    }
    Ok(dest)
}

/// [`download_with`] over the network.
pub fn download(
    info: &UpdateInfo,
    slim: &SlimUpdate,
    dest_dir: &Path,
    progress: Option<&mut dyn FnMut(u64, u64)>,
    cancelled: Option<&AtomicBool>,
) -> Result<PathBuf, DownloadError> {
    download_with(
        info,
        slim,
        dest_dir,
        progress,
        cancelled,
        &|url, start, end| {
            // Streamed off the download client, like the full archive, so it
            // carries the network capability itself.
            net::require_real_network("GET", url);
            let response = net::download_agent()
                .get(url)
                .header("User-Agent", USER_AGENT)
                .header("Range", format!("bytes={start}-{}", end - 1))
                .call()
                .map_err(NetError::from)?;
            let status = response.status().as_u16();
            if status != 206 {
                return Err(NetError::http(status).into());
            }
            Ok(Box::new(response.into_body().into_reader()))
        },
    )
}

/// Make the staged update whole before it is applied, and say whether it
/// now is. A Mac bundle is swapped whole, so the player's packs are copied
/// into it. On Windows the apply script copies over the install, which
/// keeps its packs, so nothing is copied and the staged folder stays
/// without them (`false`).
pub fn complete_staged(new_root: &Path, packs: &[PackRef], env: &UpdaterEnv) -> io::Result<bool> {
    if env.platform != Platform::MacOs || new_root.extension().is_none_or(|ext| ext != "app") {
        return Ok(packs.is_empty());
    }
    let installed = resource_root_in(env);
    let staged = new_root.join("Contents").join("Resources");
    for pack in packs {
        let relative = relative_path(&pack.path).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("bad pack path {}", pack.path),
            )
        })?;
        let target = staged.join(&relative);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(installed.join(&relative), &target)?;
    }
    Ok(true)
}
