//! Personal playlists: files dropped into the Playlists folder become
//! stations on the dial. A folder, not a file picker, on purpose -- screen
//! reader users manage folders in their file manager far more comfortably
//! than in any in-game browse dialog. Both ubiquitous playlist formats are
//! read, because which one a player has is decided by whatever exported it.
//!
//! The playlist half of `freight_fate/radio.py`. Paths are normalised the
//! way `pathlib` spells them on the host, because the entries are compared
//! against what the player's file manager shows.

use std::path::Path;

use once_cell::sync::Lazy;
use regex::Regex;

use super::RadioStation;

pub const PERSONAL_PLAYLIST_SOURCE_TYPE: &str = "playlist";
pub const PLAYLISTS_DIR_NAME: &str = "Playlists";
pub const PLAYLIST_SUFFIXES: &[&str] = &["*.m3u", "*.m3u8", "*.pls"];

/// Absolute on the machine that wrote the playlist, not just on this one.
///
/// A playlist copied off a Windows machine carries entries like
/// `C:\music\song.flac`, which POSIX does not read as absolute -- joining
/// one to the playlist's own folder would invent a path nobody ever meant, and
/// hide the real one when the track cannot be found. A drive letter or a UNC
/// share is absolute wherever the playlist is read.
pub fn absolute_anywhere(line: &str) -> bool {
    // PurePosixPath(line).is_absolute()
    if line.starts_with('/') {
        return true;
    }
    // PureWindowsPath(line).is_absolute(): a drive letter with a root, or a
    // UNC share (`\\server\share`, either separator).
    let bytes = line.as_bytes();
    if bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && (bytes[2] == b'\\' || bytes[2] == b'/')
    {
        return true;
    }
    let is_sep = |c: char| c == '\\' || c == '/';
    let mut chars = line.chars();
    if let (Some(a), Some(b)) = (chars.next(), chars.next()) {
        if is_sep(a) && is_sep(b) {
            let rest: &str = &line[2..];
            let mut parts = rest.split(is_sep).filter(|p| !p.is_empty());
            return parts.next().is_some() && parts.next().is_some();
        }
    }
    false
}

/// Whether a playlist entry names an internet station, not a file.
pub fn is_stream_entry(entry: &str) -> bool {
    let lower = entry.to_lowercase();
    lower.starts_with("http://") || lower.starts_with("https://")
}

/// `str(pathlib.Path(s))` on this host: the separator folded to the
/// native one, repeated separators collapsed, `.` components dropped and
/// a trailing separator stripped.
fn py_path_str(s: &str) -> String {
    if cfg!(windows) {
        let folded = s.replace('/', "\\");
        let unc = folded.starts_with("\\\\");
        let body = if unc { &folded[2..] } else { folded.as_str() };
        let rooted = !unc && body.starts_with('\\');
        let parts: Vec<&str> = body
            .split('\\')
            .filter(|p| !p.is_empty() && *p != ".")
            .collect();
        let mut out = String::new();
        if unc {
            out.push_str("\\\\");
        } else if rooted {
            out.push('\\');
        }
        out.push_str(&parts.join("\\"));
        if out.is_empty() {
            ".".to_string()
        } else {
            out
        }
    } else {
        let rooted = s.starts_with('/');
        let parts: Vec<&str> = s
            .split('/')
            .filter(|p| !p.is_empty() && *p != ".")
            .collect();
        let mut out = String::new();
        if rooted {
            out.push('/');
        }
        out.push_str(&parts.join("/"));
        if out.is_empty() {
            ".".to_string()
        } else {
            out
        }
    }
}

/// One playlist line as a stream URL or a fully resolved file path.
fn resolve_entry(line: &str, path: &Path) -> String {
    if is_stream_entry(line) {
        return line.to_string();
    }
    if absolute_anywhere(line) {
        return py_path_str(line);
    }
    let parent = path
        .parent()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();
    let sep = if cfg!(windows) { "\\" } else { "/" };
    if parent.is_empty() {
        py_path_str(line)
    } else {
        py_path_str(&format!("{parent}{sep}{line}"))
    }
}

/// A playlist's text: UTF-8 with or without a byte order mark, or UTF-16
/// when the file starts with one. Windows Media Player and some Winamp
/// exports save UTF-16, which read as UTF-8 had no `File1=` line anyone
/// could find, so the folder looked empty (issue #355).
fn read_playlist_text(path: &Path) -> Option<String> {
    match std::fs::read(path) {
        Ok(bytes) => Some(decode_playlist_bytes(&bytes)),
        Err(err) => {
            log::warn!(
                "Could not read playlist {} ({err})",
                path.file_name()
                    .map(|n| n.to_string_lossy())
                    .unwrap_or_default()
            );
            None
        }
    }
}

fn decode_playlist_bytes(bytes: &[u8]) -> String {
    let utf16 = |body: &[u8], unit: fn([u8; 2]) -> u16| {
        let units: Vec<u16> = body.as_chunks::<2>().0.iter().copied().map(unit).collect();
        String::from_utf16_lossy(&units)
    };
    if let Some(body) = bytes.strip_prefix(b"\xFF\xFE") {
        return utf16(body, u16::from_le_bytes);
    }
    if let Some(body) = bytes.strip_prefix(b"\xFE\xFF") {
        return utf16(body, u16::from_be_bytes);
    }
    let body = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    String::from_utf8_lossy(body).into_owned()
}

/// Python `str.splitlines()`: every line boundary it recognises.
fn py_splitlines(text: &str) -> Vec<&str> {
    let mut lines = Vec::new();
    let mut start = 0;
    let mut iter = text.char_indices().peekable();
    while let Some((i, c)) = iter.next() {
        let boundary = matches!(
            c,
            '\n' | '\r'
                | '\x0b'
                | '\x0c'
                | '\x1c'
                | '\x1d'
                | '\x1e'
                | '\u{85}'
                | '\u{2028}'
                | '\u{2029}'
        );
        if !boundary {
            continue;
        }
        lines.push(&text[start..i]);
        let mut end = i + c.len_utf8();
        if c == '\r' {
            if let Some((j, '\n')) = iter.peek().copied() {
                iter.next();
                end = j + 1;
            }
        }
        start = end;
    }
    if start < text.len() {
        lines.push(&text[start..]);
    }
    lines
}

/// Entries and the optional #PLAYLIST title from one M3U file.
///
/// Relative entries resolve against the M3U's own folder, so a playlist
/// exported next to its music keeps working when the folder moves. Stream
/// URLs are entries like any other: a playlist exported from an internet
/// radio app is nothing but stream URLs, and skipping them made the whole
/// station vanish. They need no extra licensing gate -- personal playlist
/// stations are already not safe_for_streaming, so they ride exactly the
/// same streamer-safe switch the curated real streams do.
pub fn parse_m3u(path: &Path) -> (Vec<String>, String) {
    let Some(text) = read_playlist_text(path) else {
        return (Vec::new(), String::new());
    };
    let mut title = String::new();
    let mut entries = Vec::new();
    for raw in py_splitlines(&text) {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(rest) = line.strip_prefix('#') {
            if line.to_uppercase().starts_with("#PLAYLIST:") {
                title = rest
                    .split_once(':')
                    .map(|(_, t)| t.trim())
                    .unwrap_or("")
                    .to_string();
            }
            continue;
        }
        entries.push(resolve_entry(line, path));
    }
    (entries, title)
}

static PLS_KEY_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?i)^(file|title)(\d+)$").unwrap());

/// Entries and a title from one PLS file.
///
/// The format internet radio directories hand out: `File1=`/`File2=`
/// lines, numbered rather than ordered by position, with a matching
/// `Title1=`. A one-entry PLS is a single station and its Title1 is that
/// station's own name, which is the best name the dial can give it; a
/// multi-entry PLS titles each track instead, so there the file name wins.
pub fn parse_pls(path: &Path) -> (Vec<String>, String) {
    let Some(text) = read_playlist_text(path) else {
        return (Vec::new(), String::new());
    };
    let mut files: std::collections::BTreeMap<u64, String> = Default::default();
    let mut titles: std::collections::BTreeMap<u64, String> = Default::default();
    for raw in py_splitlines(&text) {
        let line = raw.trim();
        if line.is_empty() || line.starts_with(';') || !line.contains('=') {
            continue;
        }
        let (key, value) = line.split_once('=').unwrap();
        let (key, value) = (key.trim(), value.trim());
        let Some(caps) = PLS_KEY_RE.captures(key) else {
            continue;
        };
        if value.is_empty() {
            continue;
        }
        let Ok(n) = caps[2].parse::<u64>() else {
            continue;
        };
        let target = if caps[1].to_lowercase() == "file" {
            &mut files
        } else {
            &mut titles
        };
        target.insert(n, value.to_string());
    }
    let entries: Vec<String> = files.values().map(|f| resolve_entry(f, path)).collect();
    let title = if entries.len() == 1 && !titles.is_empty() {
        files
            .keys()
            .next()
            .and_then(|first| titles.get(first))
            .cloned()
            .unwrap_or_default()
    } else {
        String::new()
    };
    (entries, title)
}

pub fn parse_playlist_file(path: &Path) -> (Vec<String>, String) {
    let suffix = path
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    if suffix == "pls" {
        return parse_pls(path);
    }
    parse_m3u(path)
}

fn is_playlist_file(path: &Path) -> bool {
    let Some(name) = path.file_name().map(|n| n.to_string_lossy().into_owned()) else {
        return false;
    };
    // pathlib.glob matches case-insensitively on Windows, exactly elsewhere.
    let name = if cfg!(windows) {
        name.to_lowercase()
    } else {
        name
    };
    PLAYLIST_SUFFIXES
        .iter()
        .any(|pattern| name.ends_with(&pattern[1..]) && name.len() > pattern.len() - 1)
}

static SLUG_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"[^a-z0-9]+").unwrap());

/// One dial station per playlist file in the player's Playlists folder.
///
/// Creating the folder here is the feature's discoverability: an empty
/// Playlists directory next to the saves invites dropping files in.
/// Missing media is skipped at play time, not here -- a NAS that is asleep
/// when the drive starts should not erase the station.
pub fn load_personal_playlists(directory: &Path) -> Vec<RadioStation> {
    let listing = std::fs::create_dir_all(directory).and_then(|_| std::fs::read_dir(directory));
    let entries = match listing {
        Ok(entries) => entries,
        Err(err) => {
            log::warn!(
                "Could not read the Playlists folder {} ({err})",
                directory.display()
            );
            return Vec::new();
        }
    };
    let mut candidates: Vec<std::path::PathBuf> = entries
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|path| path.is_file() && is_playlist_file(path))
        .collect();
    candidates.sort_by_key(|path| {
        path.file_name()
            .map(|n| n.to_string_lossy().to_lowercase())
            .unwrap_or_default()
    });
    let mut stations = Vec::new();
    let mut used: std::collections::HashSet<String> = Default::default();
    for path in candidates {
        let file_name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let (entries, title) = parse_playlist_file(&path);
        if entries.is_empty() {
            // Silence used to be the whole diagnosis here: no station on the
            // dial, nothing in the log, nothing spoken.
            log::warn!("Playlist {file_name} has no usable entries; it gets no station");
            continue;
        }
        let name = if title.is_empty() {
            path.file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default()
        } else {
            title
        };
        let streams = entries.iter().filter(|e| is_stream_entry(e)).count();
        log::info!(
            "Playlist {file_name} loaded as {name:?}: {} entries, {} files, {streams} streams",
            entries.len(),
            entries.len() - streams
        );
        let slug = SLUG_RE
            .replace_all(&name.to_lowercase(), "-")
            .trim_matches('-')
            .to_string();
        let slug = if slug.is_empty() {
            "playlist".to_string()
        } else {
            slug
        };
        let mut sid = format!("playlist-{slug}");
        let mut n = 2;
        while used.contains(&sid) {
            sid = format!("playlist-{slug}-{n}");
            n += 1;
        }
        used.insert(sid.clone());
        stations.push(RadioStation {
            id: sid,
            name,
            call_sign: "Playlist".to_string(),
            format: "personal playlist".to_string(),
            source: format!("your playlist file {file_name}"),
            source_type: PERSONAL_PLAYLIST_SOURCE_TYPE.to_string(),
            // The streamer-safe promise is that nothing licensed can
            // reach the speakers; the game cannot vouch for personal
            // media, so these ride the same gate as real streams.
            safe_for_streaming: false,
            always_available: true,
            playlist_entries: entries,
            ..Default::default()
        });
    }
    stations
}

#[cfg(test)]
pub(crate) fn py_path_string(s: &str) -> String {
    py_path_str(s)
}

#[cfg(test)]
mod encoding_tests {
    use super::*;

    fn utf16(text: &str, little_endian: bool) -> Vec<u8> {
        let mut bytes = if little_endian {
            vec![0xFF, 0xFE]
        } else {
            vec![0xFE, 0xFF]
        };
        for unit in text.encode_utf16() {
            bytes.extend(if little_endian {
                unit.to_le_bytes()
            } else {
                unit.to_be_bytes()
            });
        }
        bytes
    }

    /// A UTF-16 playlist, the way Windows Media Player saves one, loads
    /// as a station like any other (issue #355).
    #[test]
    fn a_utf16_playlist_gets_its_station() {
        let tmp = tempfile::tempdir().unwrap();
        let text = "[playlist]\r\nFile1=https://example.com/radio\r\nTitle1=Radio Café\r\nNumberOfEntries=1\r\n";
        std::fs::write(tmp.path().join("little.pls"), utf16(text, true)).unwrap();
        std::fs::write(
            tmp.path().join("big.m3u"),
            utf16("#EXTM3U\nhttps://example.com/other\n", false),
        )
        .unwrap();
        let stations = load_personal_playlists(tmp.path());
        let names: Vec<&str> = stations.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["big", "Radio Café"]);
        assert_eq!(
            stations[1].playlist_entries,
            ["https://example.com/radio".to_string()]
        );
    }

    #[test]
    fn utf8_with_and_without_a_mark_reads_the_same() {
        assert_eq!(decode_playlist_bytes(b"\xEF\xBB\xBFFile1=a"), "File1=a");
        assert_eq!(decode_playlist_bytes(b"File1=a"), "File1=a");
    }
}
