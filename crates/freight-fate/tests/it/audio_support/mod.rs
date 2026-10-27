//! Shared rigging for the audio integration tests.
//!
//! Every test that builds an `AudioEngine` from the environment takes
//! `AUDIO_LOCK` and sets `SDL_AUDIODRIVER=dummy` (the Python conftest did the
//! latter for the whole suite), so Hearken plays through its silent backend
//! and no test opens the machine's sound device. Tests that assert what the
//! backend did use [`hearken_rig`], Hearken over its recording backend,
//! which needs no device and runs everywhere.

#![allow(dead_code)]

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, Once};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use ff_core::assets_pack;
use freight_fate::audio::{assets_dir, Audio, AudioEngine, HearkenBackend};
use hearken::audio::{Recorder, RecordingBackend};

static AUDIO_LOCK: Mutex<()> = Mutex::new(());
static HEADLESS: Once = Once::new();

/// Serialise engine construction and route Hearken to its silent backend.
pub fn audio_lock() -> MutexGuard<'static, ()> {
    HEADLESS.call_once(|| std::env::set_var("SDL_AUDIODRIVER", "dummy"));
    AUDIO_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

/// An engine plus the lock that keeps it alone on the device. Shutdown runs
/// on drop too, so a failing test cannot leave the device initialised for
/// the next one.
pub struct Rig {
    pub engine: AudioEngine,
    _guard: MutexGuard<'static, ()>,
}

impl Drop for Rig {
    fn drop(&mut self) {
        self.engine.shutdown();
    }
}

/// The facade on whatever backend the environment gives: Hearken, silent
/// under the dummy driver.
pub fn rig() -> Rig {
    let guard = audio_lock();
    let engine = AudioEngine::new();
    Rig {
        engine,
        _guard: guard,
    }
}

/// The facade on the Hearken backend over Hearken's recording backend, and
/// the recorder that saw every call. No device, so no lock and no skip:
/// these run on every machine.
pub struct HearkenRig {
    pub engine: AudioEngine,
    pub rec: Recorder,
}

impl HearkenRig {
    pub fn hearken(&self) -> &HearkenBackend {
        self.engine.hearken().expect("the Hearken backend")
    }

    pub fn hearken_mut(&mut self) -> &mut HearkenBackend {
        self.engine.hearken_mut().expect("the Hearken backend")
    }

    /// The backend channel the latest play of `key` went to.
    pub fn channel(&self, key: &str) -> u64 {
        self.rec
            .channel_of(key)
            .unwrap_or_else(|| panic!("{key} was never played"))
    }
}

/// [`hearken_rig`] when the shipped recordings are here, or `None` (with a
/// note) when they are not: a test that measures a real recording needs the
/// recording, or it would fail on a missing horn, which says nothing about
/// the mixer, the fades or the jake routing it meant to pin.
pub fn recordings_rig() -> Option<HearkenRig> {
    shipped_sounds().then(hearken_rig)
}

pub fn hearken_rig() -> HearkenRig {
    let (backend, rec) = RecordingBackend::new();
    let engine =
        AudioEngine::with_backend(Box::new(HearkenBackend::with_backend(Box::new(backend))));
    HearkenRig { engine, rec }
}

// ---------------------------------------------------------------------------
// Shipped audio
//
// The same guard `ff_core::assets_pack`'s own tests use, asked from the game
// crate's side: a test that reads the real recordings needs the pack to be
// materialised, and a Git LFS pointer is a file that EXISTS -- so an
// existence check reads an unmaterialised pack as present and the lookup
// then fails against 130 bytes of pointer text.
//
// The predicate lives in `ff_core::assets_pack` (`pack_available`), which
// this crate already depends on, rather than being duplicated here: the two
// crates share one definition of what "the pack is here" means. What is
// local is only the skip note, because the two sides skip for different
// reasons -- ff-core cannot check a header it does not have, these tests
// cannot decode a recording they do not have.

/// Whether the shipped `sounds.pak` (or a loose sound tree standing in for
/// it) can answer a lookup, saying out loud why not when it cannot.
///
/// The note matters more than the skip: CI checks out without LFS on purpose
/// (fetching the packs on every push exhausted the repository's LFS budget),
/// so a silent skip would turn a green run into one that proved nothing about
/// the audio at all.
pub fn shipped_sounds() -> bool {
    shipped_pack(assets_pack::DEFAULT_PACK_NAME, "recordings")
}

/// Whether the shipped `music.pak` (or a loose tree) can answer a lookup.
pub fn shipped_music() -> bool {
    shipped_pack(assets_pack::DEFAULT_MUSIC_PACK_NAME, "music")
}

fn shipped_pack(name: &str, what: &str) -> bool {
    // A builder machine carries the loose sound tree instead, and the lookup
    // falls back to it: the pack being absent there is not a missing asset.
    // A clean clone has only `engine_classic/`, so `ui/` is the marker that
    // the real tree is present (the Python helper checks the same folder).
    if assets_dir().join("ui").is_dir() {
        return true;
    }
    let path = assets_pack::default_pack_dir().join(name);
    if assets_pack::pack_available(&path) {
        return true;
    }
    if assets_pack::is_lfs_pointer(&path) {
        eprintln!(
            "SKIPPING: no shipped {what}. {} is a leftover Git LFS pointer, not the \
             pack. The packs are plain files now: `git checkout -- assets/{name}` \
             restores sounds.pak, and cargo tools build-release downloads music.pak.",
            path.display()
        );
    } else {
        eprintln!(
            "SKIPPING: no shipped {what}. Neither the loose sound tree nor {} is here.",
            path.display()
        );
    }
    false
}

/// Poll `cond` every 10 ms for up to `timeout`.
pub fn wait_for(timeout: Duration, mut cond: impl FnMut() -> bool) -> bool {
    let start = Instant::now();
    while start.elapsed() < timeout {
        if cond() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    cond()
}

/// A 16-bit PCM WAV of `seconds` of a 440 Hz sine at 44.1 kHz, `channels`
/// wide.
pub fn sine_wav(seconds: f64, channels: u16) -> Vec<u8> {
    let rate: u32 = 44100;
    let frames = (seconds * rate as f64).round() as u32;
    let block = channels as u32 * 2;
    let data_len = frames * block;
    let mut out = Vec::with_capacity(44 + data_len as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVE");
    out.extend_from_slice(b"fmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&channels.to_le_bytes());
    out.extend_from_slice(&rate.to_le_bytes());
    out.extend_from_slice(&(rate * block).to_le_bytes());
    out.extend_from_slice(&(block as u16).to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for i in 0..frames {
        let t = i as f64 / rate as f64;
        let s = ((t * 440.0 * std::f64::consts::TAU).sin() * 0.5 * i16::MAX as f64) as i16;
        for _ in 0..channels {
            out.extend_from_slice(&s.to_le_bytes());
        }
    }
    out
}

/// A tiny Shoutcast-style server on localhost: every connection gets the
/// same body, with ICY metadata interleaved every `metaint` bytes when a
/// title is given. Stands in for the station the Python tests faked.
pub struct IcyServer {
    pub url: String,
    pub connections: Arc<AtomicUsize>,
    /// The head of every request, in arrival order.
    pub requests: Arc<Mutex<Vec<String>>>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

const METAINT: usize = 8192;

fn icy_body(audio: &[u8], title: Option<&str>) -> Vec<u8> {
    let Some(title) = title else {
        return audio.to_vec();
    };
    let mut block = format!("StreamTitle='{title}';StreamUrl='';").into_bytes();
    let pad = (16 - block.len() % 16) % 16;
    block.resize(block.len() + pad, 0);
    let len_byte = (block.len() / 16) as u8;
    let mut out = Vec::with_capacity(audio.len() + audio.len() / METAINT * (block.len() + 1));
    for chunk in audio.chunks(METAINT) {
        out.extend_from_slice(chunk);
        if chunk.len() == METAINT {
            out.push(len_byte);
            out.extend_from_slice(&block);
        }
    }
    out
}

fn serve_one(mut socket: TcpStream, headers: &str, body: &[u8], requests: &Mutex<Vec<String>>) {
    let _ = socket.set_read_timeout(Some(Duration::from_secs(2)));
    // A listener that stops reading (a full buffer nobody plays) must not
    // hold the server's shutdown forever.
    let _ = socket.set_write_timeout(Some(Duration::from_secs(2)));
    let mut request = Vec::new();
    let mut buf = [0u8; 1024];
    while !request.windows(4).any(|w| w == b"\r\n\r\n") {
        match socket.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => request.extend_from_slice(&buf[..n]),
        }
    }
    requests
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .push(String::from_utf8_lossy(&request).into_owned());
    let _ = socket.write_all(headers.as_bytes());
    let _ = socket.write_all(body);
    let _ = socket.flush();
    // Keep the connection open a moment so a reader that wants more sees a
    // clean end, not a reset mid-buffer.
    std::thread::sleep(Duration::from_millis(200));
    let _ = socket.shutdown(std::net::Shutdown::Both);
}

impl IcyServer {
    pub fn start(audio: Vec<u8>, title: Option<&str>) -> IcyServer {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind localhost");
        listener
            .set_nonblocking(true)
            .expect("nonblocking listener");
        let port = listener.local_addr().unwrap().port();
        let body = icy_body(&audio, title);
        let mut headers = String::from(
            "ICY 200 OK\r\nicy-name: Freight Fate test station\r\nContent-Type: audio/wav\r\n",
        );
        if title.is_some() {
            headers.push_str(&format!("icy-metaint: {METAINT}\r\n"));
        }
        headers.push_str("\r\n");
        let connections = Arc::new(AtomicUsize::new(0));
        let requests = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let thread = {
            let connections = Arc::clone(&connections);
            let requests = Arc::clone(&requests);
            let stop = Arc::clone(&stop);
            std::thread::spawn(move || {
                let mut workers: Vec<JoinHandle<()>> = Vec::new();
                while !stop.load(Ordering::SeqCst) {
                    match listener.accept() {
                        Ok((socket, _)) => {
                            connections.fetch_add(1, Ordering::SeqCst);
                            let _ = socket.set_nonblocking(false);
                            let headers = headers.clone();
                            let body = body.clone();
                            let requests = Arc::clone(&requests);
                            workers.push(std::thread::spawn(move || {
                                serve_one(socket, &headers, &body, &requests)
                            }));
                        }
                        Err(_) => std::thread::sleep(Duration::from_millis(10)),
                    }
                }
                for worker in workers {
                    let _ = worker.join();
                }
            })
        };
        IcyServer {
            url: format!("http://127.0.0.1:{port}/live"),
            connections,
            requests,
            stop,
            thread: Some(thread),
        }
    }

    pub fn connections(&self) -> usize {
        self.connections.load(Ordering::SeqCst)
    }

    /// The head of the latest request, or "" before any.
    pub fn last_request(&self) -> String {
        let requests = self.requests.lock().unwrap_or_else(|e| e.into_inner());
        requests.last().cloned().unwrap_or_default()
    }
}

impl Drop for IcyServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
