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
    // (from, under, after), each step DT real seconds.
    let rows: Vec<(S, U, S)> = vec![
        // From quiet: one loop per surface, with its hold.
        (S::Quiet, U::Plain, S::Quiet),
        (S::Quiet, pave, S::Whine(DerivedFromBuildYear, on(0.0))),
        (S::Quiet, deck, S::Whine(AssumedDeck, on(DECK_WHINE_MIN_S))),
        (S::Quiet, U::Grating, S::Hum(on(HUM_MIN_S))),
        // Still on the surface: the hold counts down.
        (
            S::Whine(AssumedDeck, on(1.5)),
            deck,
            S::Whine(AssumedDeck, on(1.4)),
        ),
        (S::Hum(on(3.0)), U::Grating, S::Hum(on(2.9))),
        // Past it with hold left: the song stays, holding.
        (
            S::Whine(AssumedDeck, on(1.0)),
            U::Plain,
            S::Whine(AssumedDeck, past(0.9)),
        ),
        (S::Hum(on(2.0)), U::Plain, S::Hum(past(1.9))),
        // Past it with the hold spent: quiet.
        (S::Whine(AssumedDeck, past(0.05)), U::Plain, S::Quiet),
        (S::Whine(DerivedFromBuildYear, on(0.0)), U::Plain, S::Quiet),
        (S::Hum(past(0.05)), U::Plain, S::Quiet),
        // A grating during a whine's hold: the hum, with a fresh hold.
        (
            S::Whine(AssumedDeck, past(1.0)),
            U::Grating,
            S::Hum(on(HUM_MIN_S)),
        ),
        // Grooves while a hum still holds: the hum keeps sounding.
        (S::Hum(past(1.0)), pave, S::Hum(past(0.9))),
        // Grooved pavement once the hum's hold ends: the whine, no hold.
        (
            S::Hum(past(0.05)),
            pave,
            S::Whine(DerivedFromBuildYear, on(0.0)),
        ),
        // Off a deck straight onto grooved pavement: the pavement's whine.
        (
            S::Whine(AssumedDeck, on(1.0)),
            pave,
            S::Whine(DerivedFromBuildYear, on(0.0)),
        ),
        // A whine's hold that ran out on its deck lasts to the deck's end.
        (
            S::Whine(AssumedDeck, on(0.0)),
            deck,
            S::Whine(AssumedDeck, on(0.0)),
        ),
    ];
    for (from, u, after) in rows {
        let got = from.next(under(u));
        assert!(
            same(got, after),
            "{from:?} on {u:?}: {got:?}, wanted {after:?}"
        );
    }
    // A hold never moves.
    for h in [HeldSong::Quiet, HeldSong::Whine, HeldSong::Hum] {
        for u in [U::Plain, pave, deck, U::Grating] {
            assert_eq!(S::Held(h).next(under(u)), S::Held(h));
        }
    }
}

/// Equal, with the holds' seconds compared to rounding.
fn same(a: TireSong, b: TireSong) -> bool {
    let near = |h: Hold, k: Hold| (h.left_s - k.left_s).abs() < 1e-9 && h.past == k.past;
    match (a, b) {
        (TireSong::Whine(g, h), TireSong::Whine(k, j)) => g == k && near(h, j),
        (TireSong::Hum(h), TireSong::Hum(j)) => near(h, j),
        _ => a == b,
    }
}

#[test]
fn test_a_short_deck_whines_its_hold_and_a_grating_hums_three_seconds() {
    let mut song = TireSong::Quiet;
    let step = |s: TireSong, u: Underfoot, dt: f64| s.next(SongEvent::Under { under: u, dt });
    // A 0.05 mile deck passes in a fifth of a second at route pacing.
    song = step(song, Underfoot::Grooved(Grooves::AssumedDeck), 0.0);
    song = step(song, Underfoot::Grooved(Grooves::AssumedDeck), 0.2);
    let mut whined = 0.2;
    while song.sound() == Some(WHINE) {
        song = step(song, Underfoot::Plain, 0.05);
        whined += 0.05;
    }
    assert!((whined - DECK_WHINE_MIN_S).abs() < 0.06, "{whined}");
    // A grating of half a second hums its three.
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
    // At real-time pacing a long grating plays its own length.
    song = step(song, Underfoot::Grating, 0.0);
    for _ in 0..100 {
        song = step(song, Underfoot::Grating, 0.1);
    }
    assert_eq!(song.sound(), Some(HUM));
    assert_eq!(step(song, Underfoot::Plain, 0.1), TireSong::Quiet);
}

#[test]
fn test_one_loop_change_per_transition() {
    let whine = TireSong::Whine(Grooves::AssumedDeck, on(0.0));
    assert_eq!(TireSong::Quiet.sound(), None);
    assert_eq!(whine.sound(), Some(WHINE));
    assert_eq!(TireSong::Hum(on(0.0)).sound(), Some(HUM));
    // How the game knows the grooves never changes the sound.
    assert_eq!(
        TireSong::Whine(Grooves::DerivedFromBuildYear, on(0.0)).sound(),
        whine.sound()
    );
    assert_eq!(TireSong::Held(HeldSong::Hum).sound(), Some(HUM));
    assert_eq!(TireSong::Held(HeldSong::Quiet).sound(), None);
}

#[test]
fn test_the_mix_follows_speed() {
    let whine = TireSong::Whine(Grooves::DerivedFromBuildYear, on(0.0));
    // At its reference speed the loop plays at its own pitch and level.
    let (_, rate, level) = whine.mix(WHINE.reference_mps).unwrap();
    assert!((rate - 1.0).abs() < 1e-9);
    assert!((level - WHINE.level).abs() < 1e-9);
    // 35 mph after 65: the note drops with the speed, and so does the
    // level, by the 1.5 power.
    let (_, rate, level) = whine.mix(15.65).unwrap();
    assert!((rate - 15.65 / 29.06).abs() < 1e-9);
    assert!((level - WHINE.level * (15.65_f64 / 29.06).powf(1.5)).abs() < 1e-9);
    // Above the reference the level is capped and the rate clamped.
    let (_, rate, level) = TireSong::Hum(on(0.0)).mix(40.0).unwrap();
    assert_eq!(rate, SONG_RATE.1);
    assert!((level - HUM.level).abs() < 1e-9);
    // Under the seam speed it is silent; a quiet song never plays.
    assert_eq!(whine.mix(SEAM_MIN_SPEED_MPS), None);
    assert_eq!(TireSong::Quiet.mix(30.0), None);
    let (_, rate, _) = whine.mix(6.0).unwrap();
    assert_eq!(rate, SONG_RATE.0);
}

#[test]
fn test_status_names() {
    assert_eq!(TireSong::Quiet.describe(), "quiet");
    assert_eq!(
        TireSong::Whine(Grooves::DerivedFromBuildYear, on(0.0)).describe(),
        "whine (derived from build year)"
    );
    assert_eq!(
        TireSong::Whine(Grooves::AssumedDeck, on(1.0)).describe(),
        "whine (assumed, concrete deck)"
    );
    assert_eq!(
        TireSong::Whine(Grooves::AssumedDeck, past(1.0)).describe(),
        "whine (assumed, concrete deck, holding)"
    );
    assert_eq!(TireSong::Hum(on(1.0)).describe(), "hum");
    assert_eq!(TireSong::Hum(past(1.0)).describe(), "hum (holding)");
    assert_eq!(TireSong::Held(HeldSong::Whine).describe(), "held whine");
    for h in ["quiet", "whine", "hum"] {
        assert_eq!(HeldSong::from_name(h).map(HeldSong::name), Some(h));
    }
    assert_eq!(HeldSong::from_name("loud"), None);
}
