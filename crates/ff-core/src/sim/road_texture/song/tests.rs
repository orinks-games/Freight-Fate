use super::*;

const DT: f64 = 0.1;

fn under(u: Underfoot) -> SongEvent {
    SongEvent::Under { under: u, dt: DT }
}

fn on(left_s: f64) -> Hold {
    Hold {
        left_s,
        past: false,
    }
}

fn past(left_s: f64) -> Hold {
    Hold { left_s, past: true }
}

#[test]
fn test_the_song_transition_table() {
    use Grooves::*;
    use TireSong as S;
    use Underfoot as U;
    let deck = U::Grooved(AssumedDeck);
    let pave = U::Grooved(DerivedFromBuildYear);
    let rows: Vec<(S, U, S)> = vec![
        (S::Quiet, U::Plain, S::Quiet),
        (S::Quiet, pave, S::Quiet),
        (S::Quiet, deck, S::Quiet),
        (S::Quiet, U::Grating, S::Hum(on(HUM_MIN_S))),
        (S::Hum(on(3.0)), U::Grating, S::Hum(on(2.9))),
        (S::Hum(on(2.0)), U::Plain, S::Hum(past(1.9))),
        (S::Hum(past(1.0)), pave, S::Hum(past(0.9))),
        (S::Hum(past(0.05)), U::Plain, S::Quiet),
        (S::Hum(past(0.05)), pave, S::Quiet),
        (
            S::Hum(past(1.0)),
            U::Grating,
            S::Hum(Hold {
                left_s: 0.9,
                past: false,
            }),
        ),
    ];
    for (from, u, after) in rows {
        let got = from.next(under(u));
        assert!(
            same(got, after),
            "{from:?} on {u:?}: {got:?}, wanted {after:?}"
        );
    }
    for h in [HeldSong::Quiet, HeldSong::Hum] {
        for u in [U::Plain, pave, deck, U::Grating] {
            assert_eq!(S::Held(h).next(under(u)), S::Held(h));
        }
    }
}

fn same(a: TireSong, b: TireSong) -> bool {
    let near = |h: Hold, k: Hold| (h.left_s - k.left_s).abs() < 1e-9 && h.past == k.past;
    match (a, b) {
        (TireSong::Hum(h), TireSong::Hum(j)) => near(h, j),
        _ => a == b,
    }
}

#[test]
fn test_grooved_concrete_stays_quiet_and_a_grating_hums_for_three_seconds() {
    let mut song = TireSong::Quiet;
    let step = |s: TireSong, u: Underfoot, dt: f64| s.next(SongEvent::Under { under: u, dt });
    for underfoot in [
        Underfoot::Grooved(Grooves::AssumedDeck),
        Underfoot::Grooved(Grooves::DerivedFromBuildYear),
    ] {
        song = step(song, underfoot, 0.1);
        assert_eq!(song.sound(), None);
        assert_eq!(song.describe(), "quiet");
    }
    song = step(song, Underfoot::Grating, 0.0);
    song = step(song, Underfoot::Grating, 0.5);
    assert_eq!(song.describe(), "hum");
    let mut hummed = 0.5;
    while song.sound() == Some(HUM) {
        song = step(song, Underfoot::Plain, 0.05);
        hummed += 0.05;
        if song.sound() == Some(HUM) {
            assert_eq!(song.describe(), "hum (holding)");
        }
    }
    assert!((hummed - HUM_MIN_S).abs() < 0.06, "{hummed}");
    song = step(song, Underfoot::Grating, 0.0);
    for _ in 0..100 {
        song = step(song, Underfoot::Grating, 0.1);
    }
    assert_eq!(song.sound(), Some(HUM));
    assert_eq!(step(song, Underfoot::Plain, 0.1), TireSong::Quiet);
}

#[test]
fn test_the_hum_is_the_only_tire_song_loop() {
    assert_eq!(TireSong::Quiet.sound(), None);
    assert_eq!(TireSong::Hum(on(0.0)).sound(), Some(HUM));
    assert_eq!(TireSong::Held(HeldSong::Hum).sound(), Some(HUM));
    assert_eq!(TireSong::Held(HeldSong::Quiet).sound(), None);
}

#[test]
fn test_the_mix_follows_speed() {
    let hum = TireSong::Hum(on(0.0));
    let (_, rate, level) = hum.mix(HUM.reference_mps).unwrap();
    assert!((rate - 1.0).abs() < 1e-9);
    assert!((level - HUM.level).abs() < 1e-9);
    let (_, rate, level) = hum.mix(15.65).unwrap();
    assert!((rate - 15.65 / HUM.reference_mps).abs() < 1e-9);
    assert!((level - HUM.level * (15.65_f64 / HUM.reference_mps).powf(1.5)).abs() < 1e-9);
    let (_, rate, level) = hum.mix(40.0).unwrap();
    assert_eq!(rate, SONG_RATE.1);
    assert!((level - HUM.level).abs() < 1e-9);
    assert_eq!(hum.mix(SEAM_MIN_SPEED_MPS), None);
    assert_eq!(TireSong::Quiet.mix(30.0), None);
    let (_, rate, _) = hum.mix(6.0).unwrap();
    assert_eq!(rate, SONG_RATE.0);
}

#[test]
fn test_status_names() {
    assert_eq!(TireSong::Quiet.describe(), "quiet");
    assert_eq!(TireSong::Hum(on(1.0)).describe(), "hum");
    assert_eq!(TireSong::Hum(past(1.0)).describe(), "hum (holding)");
    assert_eq!(TireSong::Held(HeldSong::Hum).describe(), "held hum");
    assert_eq!(TireSong::Held(HeldSong::Quiet).describe(), "held quiet");
    for name in ["quiet", "hum"] {
        assert_eq!(HeldSong::from_name(name).map(HeldSong::name), Some(name));
    }
    assert_eq!(HeldSong::from_name("whine"), None);
    assert_eq!(HeldSong::from_name("loud"), None);
}
