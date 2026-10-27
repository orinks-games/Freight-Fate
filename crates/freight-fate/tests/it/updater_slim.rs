//! Smaller updates: when the install's sound packs match the ones in the
//! release archive, the updater fetches only the byte ranges around them and
//! writes a new index, so the result unpacks like the full archive minus the
//! packs. A Mac bundle then gets the player's packs copied in.

use std::fs;
use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};

use freight_fate::net::NetError;
use freight_fate::updater::slim::{self, PackRef};
use freight_fate::updater::{DownloadError, Platform, UpdateInfo, UpdaterEnv};
use serde_json::{json, Value};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

const FULL: &str = "FreightFate-v9.9.9-windows-portable.zip";
const URL: &str = "https://example.test/full.zip";

const MUSIC: &[u8] = b"music pack bytes, long enough to matter in the archive";
const RADIO: &[u8] = b"channel 3000 pack, also some length to it";

/// The other files in the archive, deflated and stored, around the packs.
fn others() -> Vec<(&'static str, Vec<u8>)> {
    vec![
        ("FreightFate/FreightFate.exe", b"exe ".repeat(500)),
        (
            "FreightFate/build_info.json",
            br#"{"tag":"v9.9.9"}"#.to_vec(),
        ),
        ("FreightFate/freight_fate/sounds.pak", b"sounds".repeat(300)),
        ("FreightFate/lang/es.po", b"msgid \"x\"\n".repeat(40)),
    ]
}

/// A release archive with the packs in the middle, stored like the real
/// build stores them.
fn archive(music: &[u8], radio: &[u8]) -> Vec<u8> {
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    let deflated = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    let others = others();
    let (first, rest) = others.split_at(2);
    for (name, data) in first {
        writer.start_file(*name, deflated).unwrap();
        writer.write_all(data).unwrap();
    }
    writer
        .start_file("FreightFate/freight_fate/channel3000.pak", stored)
        .unwrap();
    writer.write_all(radio).unwrap();
    writer
        .start_file("FreightFate/freight_fate/music.pak", stored)
        .unwrap();
    writer.write_all(music).unwrap();
    for (name, data) in rest {
        writer.start_file(*name, deflated).unwrap();
        writer.write_all(data).unwrap();
    }
    writer.finish().unwrap().into_inner()
}

fn crc(data: &[u8]) -> u32 {
    crc32fast::hash(data)
}

fn record(music: &[u8], radio: &[u8]) -> Value {
    json!({"packs": [
        {"path": "freight_fate/music.pak", "size": music.len(), "crc32": crc(music)},
        {"path": "freight_fate/channel3000.pak", "size": radio.len(), "crc32": crc(radio)},
    ]})
}

/// A packaged install: the executable, its packs, and optionally the
/// `build_info.json` packs record. Returns the executable's path.
fn install(root: &Path, music: &[u8], radio: &[u8], packs: Option<Value>) -> PathBuf {
    let dir = root.join("freight_fate");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("music.pak"), music).unwrap();
    fs::write(dir.join("channel3000.pak"), radio).unwrap();
    let mut info = json!({"tag": "v9.9.8", "channel": "stable"});
    if let Some(packs) = packs {
        info["packs"] = packs["packs"].clone();
    }
    fs::write(root.join("build_info.json"), info.to_string()).unwrap();
    let exe = root.join("FreightFate.exe");
    fs::write(&exe, b"exe").unwrap();
    exe
}

fn info_for(name: &str, size: usize) -> UpdateInfo {
    UpdateInfo {
        tag: "v9.9.9".into(),
        title: "Freight Fate v9.9.9".into(),
        notes: Vec::new(),
        asset_name: name.into(),
        asset_url: URL.into(),
        asset_size: size as i64,
        asset_sha256: String::new(),
        slim: None,
    }
}

/// Serve byte ranges of `bytes`, counting how many were asked for.
fn ranges_of(bytes: &[u8]) -> impl Fn(&str, u64, u64) -> Result<Vec<u8>, NetError> + '_ {
    move |url, start, end| {
        assert_eq!(url, URL);
        Ok(bytes[start as usize..end as usize].to_vec())
    }
}

fn resolved(bytes: &[u8], env: &UpdaterEnv) -> UpdateInfo {
    let mut info = info_for(FULL, bytes.len());
    slim::resolve_with(&mut info, env, &ranges_of(bytes));
    info
}

fn windows_install(
    music: &[u8],
    radio: &[u8],
    packs: Option<Value>,
) -> (tempfile::TempDir, UpdaterEnv) {
    let tmp = tempfile::tempdir().unwrap();
    let exe = install(tmp.path(), music, radio, packs);
    let env = UpdaterEnv::fake(Platform::Windows, &exe);
    (tmp, env)
}

#[test]
fn matching_packs_fetch_only_the_rest_of_the_archive() {
    let bytes = archive(MUSIC, RADIO);
    let (_tmp, env) = windows_install(MUSIC, RADIO, None);
    let info = resolved(&bytes, &env);
    let slim = info.slim.as_ref().expect("smaller download offered");
    assert_eq!(slim.packs.len(), 2);
    let skipped = (MUSIC.len() + RADIO.len()) as u64;
    assert!(slim.fetch_len() + skipped <= bytes.len() as u64);
    assert_eq!(info.download_size(), slim.fetch_len() as i64);
    assert!(info.download_size() < bytes.len() as i64);
}

#[test]
fn a_different_pack_keeps_the_full_download() {
    let bytes = archive(MUSIC, RADIO);
    // Same size, different bytes: only the CRC-32 can tell.
    let changed: Vec<u8> = MUSIC.iter().map(|b| b ^ 1).collect();
    let (_tmp, env) = windows_install(&changed, RADIO, None);
    let info = resolved(&bytes, &env);
    assert_eq!(info.slim, None);
    assert_eq!(info.download_size(), bytes.len() as i64);
}

#[test]
fn a_missing_pack_or_unreadable_index_keeps_the_full_download() {
    let bytes = archive(MUSIC, RADIO);
    let (tmp, env) = windows_install(MUSIC, RADIO, None);
    fs::remove_file(tmp.path().join("freight_fate/channel3000.pak")).unwrap();
    assert_eq!(resolved(&bytes, &env).slim, None);

    let (_tmp, env) = windows_install(MUSIC, RADIO, None);
    let mut info = info_for(FULL, bytes.len());
    slim::resolve_with(&mut info, &env, &|_, _, _| Err(NetError::http(501)));
    assert_eq!(info.slim, None);

    // Garbage where the index should be.
    let mut broken = bytes.clone();
    let len = broken.len();
    broken[len - 22..].fill(0);
    assert_eq!(resolved(&broken, &env).slim, None);
}

#[test]
fn the_check_trusts_the_install_record() {
    let bytes = archive(MUSIC, RADIO);
    // The record says the install has the release's packs; the bytes on
    // disk differ at the same size, so a match proves no pack was read.
    let changed: Vec<u8> = MUSIC.iter().map(|b| b ^ 1).collect();
    let (_tmp, env) = windows_install(&changed, RADIO, Some(record(MUSIC, RADIO)));
    assert!(resolved(&bytes, &env).slim.is_some());
}

#[test]
fn tarballs_and_appimages_download_in_full() {
    let bytes = archive(MUSIC, RADIO);
    let (tmp, mut env) = windows_install(MUSIC, RADIO, None);
    let mut info = info_for("FreightFate-v9.9.9-linux-x64.tar.gz", bytes.len());
    slim::resolve_with(&mut info, &env, &ranges_of(&bytes));
    assert_eq!(info.slim, None);

    let appimage = tmp.path().join("FreightFate.AppImage");
    fs::write(&appimage, b"appimage").unwrap();
    env.appimage = Some(appimage.display().to_string());
    assert_eq!(resolved(&bytes, &env).slim, None);
}

#[test]
fn an_archive_without_both_packs_downloads_in_full() {
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    writer
        .start_file(
            "FreightFate/freight_fate/music.pak",
            SimpleFileOptions::default(),
        )
        .unwrap();
    writer.write_all(MUSIC).unwrap();
    let bytes = writer.finish().unwrap().into_inner();
    let (_tmp, env) = windows_install(MUSIC, RADIO, None);
    assert_eq!(resolved(&bytes, &env).slim, None);
}

fn open_ranges(
    bytes: &[u8],
) -> impl Fn(&str, u64, u64) -> Result<Box<dyn Read + Send>, DownloadError> + '_ {
    move |_, start, end| {
        Ok(
            Box::new(Cursor::new(bytes[start as usize..end as usize].to_vec()))
                as Box<dyn Read + Send>,
        )
    }
}

#[test]
fn the_partial_download_unpacks_to_everything_but_the_packs() {
    let bytes = archive(MUSIC, RADIO);
    let (_tmp, env) = windows_install(MUSIC, RADIO, None);
    let info = resolved(&bytes, &env);
    let slim = slim::still_matches(&info, &env).expect("packs unchanged");
    let dest = tempfile::tempdir().unwrap();
    let mut seen = 0;
    let mut on_progress = |done: u64, total: u64| {
        assert!(done <= total);
        seen = done;
    };
    let path = slim::download_with(
        &info,
        slim,
        dest.path(),
        Some(&mut on_progress),
        None,
        &open_ranges(&bytes),
    )
    .expect("download");
    assert_eq!(seen, slim.fetch_len());
    assert_eq!(path, dest.path().join(FULL));

    let mut zip = ZipArchive::new(fs::File::open(&path).unwrap()).unwrap();
    let mut names: Vec<String> = zip
        .file_names()
        .map(|name| name.unwrap().into_owned())
        .collect();
    names.sort();
    let mut expected: Vec<String> = others().iter().map(|(n, _)| n.to_string()).collect();
    expected.sort();
    assert_eq!(names, expected);
    for (name, data) in others() {
        let mut out = Vec::new();
        // Reading to the end checks each file's CRC-32.
        zip.by_name(name).unwrap().read_to_end(&mut out).unwrap();
        assert_eq!(out, data, "{name}");
    }
}

#[test]
fn a_short_range_is_a_damaged_download() {
    let bytes = archive(MUSIC, RADIO);
    let (_tmp, env) = windows_install(MUSIC, RADIO, None);
    let info = resolved(&bytes, &env);
    let slim = info.slim.as_ref().unwrap();
    let dest = tempfile::tempdir().unwrap();
    let short = |_: &str, start: u64, end: u64| {
        let end = end - 1;
        Ok(
            Box::new(Cursor::new(bytes[start as usize..end as usize].to_vec()))
                as Box<dyn Read + Send>,
        )
    };
    let result = slim::download_with(&info, slim, dest.path(), None, None, &short);
    assert!(matches!(result, Err(DownloadError::Corrupt)), "{result:?}");
}

#[test]
fn a_pack_gone_since_the_check_takes_the_full_download() {
    let bytes = archive(MUSIC, RADIO);
    let (tmp, env) = windows_install(MUSIC, RADIO, None);
    let info = resolved(&bytes, &env);
    assert!(info.slim.is_some());
    fs::remove_file(tmp.path().join("freight_fate/music.pak")).unwrap();
    assert!(slim::still_matches(&info, &env).is_none());
}

#[test]
fn a_record_cannot_point_outside_the_install() {
    for path in ["../music.pak", "/etc/passwd", "freight_fate/../../x", ""] {
        let packs = json!({"packs": [{"path": path, "size": 1, "crc32": 1}]});
        assert_eq!(slim::parse_packs(&packs), None, "{path:?}");
    }
    // A record from before CRC-32s were kept is not trusted.
    let old = json!({"packs": [{"path": "freight_fate/music.pak", "size": 1, "sha256": "ab"}]});
    assert_eq!(slim::parse_packs(&old), None);
}

fn packs() -> Vec<PackRef> {
    slim::parse_packs(&record(MUSIC, RADIO)).unwrap()
}

#[test]
fn a_mac_update_bundle_gets_the_players_packs_copied_in() {
    let tmp = tempfile::tempdir().unwrap();
    let resources = tmp
        .path()
        .join("Applications/FreightFate.app/Contents/Resources");
    install(&resources, MUSIC, RADIO, None);
    let exe = tmp
        .path()
        .join("Applications/FreightFate.app/Contents/MacOS/FreightFate");
    fs::create_dir_all(exe.parent().unwrap()).unwrap();
    fs::write(&exe, b"exe").unwrap();
    let env = UpdaterEnv::fake(Platform::MacOs, &exe);
    let staged = tmp.path().join("staging/FreightFate.app");
    fs::create_dir_all(staged.join("Contents/Resources/freight_fate")).unwrap();

    assert!(slim::complete_staged(&staged, &packs(), &env).unwrap());

    let staged_packs = staged.join("Contents/Resources/freight_fate");
    assert_eq!(fs::read(staged_packs.join("music.pak")).unwrap(), MUSIC);
    assert_eq!(
        fs::read(staged_packs.join("channel3000.pak")).unwrap(),
        RADIO
    );
}

#[test]
fn a_folder_update_leaves_the_packs_where_they_are() {
    let tmp = tempfile::tempdir().unwrap();
    let exe = install(&tmp.path().join("game"), MUSIC, RADIO, None);
    let env = UpdaterEnv::fake(Platform::Windows, &exe);
    let staged = tmp.path().join("staging/FreightFate");
    fs::create_dir_all(&staged).unwrap();

    assert!(
        !slim::complete_staged(&staged, &packs(), &env).unwrap(),
        "the staged folder still leaves the packs out"
    );

    // The apply script copies over the install, which keeps its packs.
    assert!(!staged.join("freight_fate/music.pak").exists());
}
