//! Port of the audio half of `tests/test_radio_now_playing.py`: what the
//! station says it is playing, read off the stream's ICY metadata. The
//! driving-state and status-screen tests of the Python file belong to the
//! states port.
//!
use std::time::Duration;

use freight_fate::audio::{
    parse_hls_stream_title, parse_icy_stream_title, parse_ogg_stream_title, Audio, AudioEngine,
    NullBackend,
};

use crate::audio_support::{hearken_rig, sine_wav, wait_for, IcyServer};

#[test]
fn test_icy_stream_title_parsing() {
    let cases: [(Option<&[u8]>, Option<&str>); 9] = [
        (
            Some(b"StreamTitle='Usher - U Remind Me';StreamUrl='';"),
            Some("Usher - U Remind Me"),
        ),
        (
            Some(b"StreamTitle='Darren Duff radio';"),
            Some("Darren Duff radio"),
        ),
        // UTF-8 from a modern Icecast mount.
        (
            Some("StreamTitle='Beyonc\u{e9} - Halo';".as_bytes()),
            Some("Beyonc\u{e9} - Halo"),
        ),
        // Latin-1 from an older Shoutcast one: not valid UTF-8, still a title.
        (
            Some(b"StreamTitle='Caf\xe9 del Mar';"),
            Some("Caf\u{e9} del Mar"),
        ),
        // Whitespace runs inside a title collapse; surrounding space goes.
        (
            Some(b"StreamTitle='  Artist   -   Title  ';"),
            Some("Artist - Title"),
        ),
        // An empty block between songs is "no information", not "".
        (Some(b"StreamTitle='';StreamUrl='';"), None),
        (Some(b"StreamUrl='http://x';"), None),
        (Some(b""), None),
        (None, None),
    ];
    for (raw, expected) in cases {
        assert_eq!(parse_icy_stream_title(raw).as_deref(), expected, "{raw:?}");
    }
}

#[test]
fn test_network_encoder_fields_are_not_read_out() {
    // Both shapes as captured off live stations on 2026-09-18: the song is
    // what the driver hears, never the ad-insertion fields around it.
    let cases: [(&[u8], Option<&str>); 4] = [
        (
            b"StreamTitle='title=\"18 And Life\",artist=\"SKID ROW\",url=\"song_spot=\"F\" MediaBaseId=\"0\"\"';",
            Some("SKID ROW - 18 And Life"),
        ),
        (
            b"StreamTitle='Beatles - text=\"This Boy\" song_spot=\"M\" length=\"00:02:13\"';",
            Some("Beatles - This Boy"),
        ),
        (
            b"StreamTitle='title=\"\",artist=\"\",url=\"song_spot=\"T\"\"';",
            None,
        ),
        // A key inside a longer word is not the key.
        (
            b"StreamTitle='Band - subtitle=\"x\"';",
            Some("Band - subtitle=\"x\""),
        ),
    ];
    for (raw, expected) in cases {
        assert_eq!(
            parse_icy_stream_title(Some(raw)).as_deref(),
            expected,
            "{}",
            String::from_utf8_lossy(raw)
        );
    }
}

#[test]
fn test_ogg_comments_give_artist_and_title() {
    let comments = |items: &[&str]| items.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    let cases: [(&[&str], Option<&str>); 6] = [
        (
            &["title=Policemen", "artist=Siistema Sangria"],
            Some("Siistema Sangria - Policemen"),
        ),
        // Upper-case keys and a trailing line ending, as one encoder sends.
        (
            &["ARTIST=The Weeknd", "TITLE=The Abyss\r\n"],
            Some("The Weeknd - The Abyss"),
        ),
        (
            &["TITLE=Rob Hubbard - Spellbound"],
            Some("Rob Hubbard - Spellbound"),
        ),
        (&["artist=Nobody", "ENCODER=Liquidsoap"], None),
        (&["title=  "], None),
        (&[], None),
    ];
    for (raw, expected) in cases {
        assert_eq!(
            parse_ogg_stream_title(&comments(raw)).as_deref(),
            expected,
            "{raw:?}"
        );
    }
}

#[test]
fn test_hls_segment_titles() {
    let cases: [(&str, Option<&str>); 6] = [
        (
            "5.01551,THIS STATION WILL CONTINUE AFTER THIS BREAK",
            Some("THIS STATION WILL CONTINUE AFTER THIS BREAK"),
        ),
        (
            "10,title=\"Armstrong & Getty\",artist=\"KOGA\",url=\"song_spot=\"T\"\"",
            Some("KOGA - Armstrong & Getty"),
        ),
        // A bare duration, and one encoder's placeholder, are no information.
        ("10.031,", None),
        ("9.984", None),
        ("4.99, no desc", None),
        ("", None),
    ];
    for (raw, expected) in cases {
        assert_eq!(parse_hls_stream_title(raw).as_deref(), expected, "{raw:?}");
    }
}

#[test]
fn test_the_audio_facade_answers_none_when_the_backend_cannot_know() {
    let engine = AudioEngine::with_backend(Box::new(NullBackend::new()));
    assert_eq!(engine.radio_now_playing(), None);
}

#[test]
fn test_the_hearken_backend_reads_the_title_off_the_stream() {
    // End to end against Hearken's ICY reader and the game's own parser: a
    // localhost station interleaves `StreamTitle='...'` blocks, and the
    // facade reports it.
    let mut r = hearken_rig();
    let server = IcyServer::start(sine_wav(8.0, 1), Some("Usher - U Remind Me"));
    assert_eq!(r.engine.radio_now_playing(), None); // nothing streaming yet
    r.engine.play_radio_stream_with(&server.url, 100).unwrap();
    let hearken = r.hearken_mut();
    hearken.settle_radio(Duration::from_secs(10));
    assert!(hearken.music_track().is_some(), "the station did not open");
    let engine = &r.engine;
    assert!(
        wait_for(Duration::from_secs(5), || engine
            .radio_now_playing()
            .is_some()),
        "no title arrived"
    );
    assert_eq!(
        r.engine.radio_now_playing().as_deref(),
        Some("Usher - U Remind Me")
    );
    r.engine.stop_music_with(0);
    assert_eq!(r.engine.radio_now_playing(), None);
}

#[test]
fn test_an_encoders_ad_fields_are_stripped_off_a_live_title() {
    // The same parser over the block as the station sent it, not Hearken's
    // own tidied reading of it: the song, never the fields around it.
    let mut r = hearken_rig();
    let server = IcyServer::start(
        sine_wav(8.0, 1),
        Some("Beatles - text=\"This Boy\" song_spot=\"M\" length=\"00:02:13\""),
    );
    r.engine.play_radio_stream_with(&server.url, 100).unwrap();
    r.hearken_mut().settle_radio(Duration::from_secs(10));
    let engine = &r.engine;
    assert!(
        wait_for(Duration::from_secs(5), || engine
            .radio_now_playing()
            .is_some()),
        "no title arrived"
    );
    assert_eq!(
        r.engine.radio_now_playing().as_deref(),
        Some("Beatles - This Boy")
    );
    r.engine.stop_music_with(0);
}
