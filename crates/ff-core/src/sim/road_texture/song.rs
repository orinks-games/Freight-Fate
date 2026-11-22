//! The tires' song: the steady note a tire sings on concrete grooved across
//! the lane, and the low hum of an open steel grate bridge deck (Orinks,
//! 2026-11-22).
//!
//! Grooves cut or tined across the lane hit the tread at a steady rate, so
//! the pitch is speed over groove spacing; a grate's bars do the same a few
//! inches apart, so its note is low. Each is one loop made for a reference
//! speed and played at rate = speed / reference, the way the road loop is.
//!
//! [`TireSong::next`] is the only place the song moves. Nothing here is
//! spoken: a thing with its own sound is not announced.

use super::SEAM_MIN_SPEED_MPS;

/// How the game knows a stretch is grooved across the lane: derived from
/// the pavement's build year, or assumed for a concrete bridge deck.
pub use crate::data::pavement::Grooves;

/// How the game knows, as the agent server reports it.
pub fn grooves_origin(grooves: Grooves) -> &'static str {
    match grooves {
        Grooves::DerivedFromBuildYear => "derived from build year",
        Grooves::AssumedDeck => "assumed, concrete deck",
    }
}

/// What the layer reads under the steer axle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Underfoot {
    /// Nothing that sings: asphalt, concrete grooved along the lane or
    /// ground smooth, or road the layer says nothing about.
    Plain,
    Grooved(Grooves),
    /// An open steel grating (National Bridge Inventory Item 107, read).
    Grating,
}

/// A song a staged scenario holds for the whole drive.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HeldSong {
    Quiet,
    Whine,
    Hum,
}

impl HeldSong {
    pub fn from_name(name: &str) -> Option<HeldSong> {
        match name {
            "quiet" => Some(HeldSong::Quiet),
            "whine" => Some(HeldSong::Whine),
            "hum" => Some(HeldSong::Hum),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            HeldSong::Quiet => "quiet",
            HeldSong::Whine => "whine",
            HeldSong::Hum => "hum",
        }
    }
}

/// What the tires are singing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TireSong {
    Quiet,
    Whine(Grooves),
    Hum,
    /// A staged scenario holds one song; it wins over the layer.
    Held(HeldSong),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SongEvent {
    /// The steer axle is now on this.
    Under(Underfoot),
}

/// One of the two loops, with the speed it was made for.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SongLoop {
    pub key: &'static str,
    /// Metres a second at which the loop plays at its own pitch.
    pub reference_mps: f64,
    /// Its level at and above the reference speed, before the Road surface
    /// volume. First guesses for Orinks's ear, set from a game-mix recording
    /// (2026-11-22): the whine's note sits about 19 dB over the same band of
    /// the mix without it at 65 mph on jointed concrete, as in the previews.
    /// The hum sits about 10 dB over the mix in 140 to 175 Hz at 45 mph; it
    /// plays at full level, and its loop is raised 4 dB to get there.
    pub level: f64,
}

/// The whine, made for 65 mph: 1,529 Hz there, a groove every 19 mm.
pub const WHINE: SongLoop = SongLoop {
    key: "vehicle/tined_whine",
    reference_mps: 29.06,
    level: 0.35,
};
/// The grate, made for 45 mph: 158 Hz there, a bar every 5 inches.
pub const HUM: SongLoop = SongLoop {
    key: "vehicle/grate_hum",
    reference_mps: 20.12,
    level: 1.0,
};
/// The playback rate never leaves this range, so a crawl does not drop the
/// note into a growl or a sprint raise it to a squeal.
pub const SONG_RATE: (f64, f64) = (0.35, 1.5);
/// Milliseconds a loop takes to come in or go, so a deck starts on its
/// joint rather than before it.
pub const SONG_FADE_MS: u32 = 100;

impl TireSong {
    /// The song a drive starts with.
    pub fn first(under: Underfoot) -> TireSong {
        TireSong::Quiet.next(SongEvent::Under(under))
    }

    /// The only place the song moves. Grating wins over grooves, since the
    /// layer gives the grating first; a hold never moves.
    pub fn next(self, event: SongEvent) -> TireSong {
        let SongEvent::Under(under) = event;
        match (self, under) {
            (TireSong::Held(_), _) => self,
            (_, Underfoot::Plain) => TireSong::Quiet,
            (_, Underfoot::Grooved(g)) => TireSong::Whine(g),
            (_, Underfoot::Grating) => TireSong::Hum,
        }
    }

    /// The loop this song plays, if any.
    pub fn sound(self) -> Option<SongLoop> {
        match self {
            TireSong::Quiet | TireSong::Held(HeldSong::Quiet) => None,
            TireSong::Whine(_) | TireSong::Held(HeldSong::Whine) => Some(WHINE),
            TireSong::Hum | TireSong::Held(HeldSong::Hum) => Some(HUM),
        }
    }

    /// The song as the agent server's `status` reports it.
    pub fn describe(self) -> String {
        match self {
            TireSong::Quiet => "quiet".to_string(),
            TireSong::Whine(g) => format!("whine ({})", grooves_origin(g)),
            TireSong::Hum => "hum".to_string(),
            TireSong::Held(h) => format!("held {}", h.name()),
        }
    }

    /// What to play at `speed_mps`: the loop, its rate and its level.
    /// `None` when the song is quiet or the truck is under the speed at
    /// which seams stop.
    pub fn mix(self, speed_mps: f64) -> Option<(SongLoop, f64, f64)> {
        let sound = self.sound()?;
        if speed_mps <= SEAM_MIN_SPEED_MPS {
            return None;
        }
        let ratio = speed_mps / sound.reference_mps;
        let rate = ratio.clamp(SONG_RATE.0, SONG_RATE.1);
        let level = sound.level * ratio.powf(1.5).min(1.0);
        Some((sound, rate, level))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_the_song_transition_table() {
        use Grooves::*;
        use TireSong as S;
        use Underfoot as U;
        let held = [HeldSong::Quiet, HeldSong::Whine, HeldSong::Hum];
        let froms = [
            S::Quiet,
            S::Whine(DerivedFromBuildYear),
            S::Whine(AssumedDeck),
            S::Hum,
        ];
        // (under, after) for every song that is not held.
        let rows = [
            (U::Plain, S::Quiet),
            (
                U::Grooved(DerivedFromBuildYear),
                S::Whine(DerivedFromBuildYear),
            ),
            (U::Grooved(AssumedDeck), S::Whine(AssumedDeck)),
            (U::Grating, S::Hum),
        ];
        for from in froms {
            for (under, after) in rows {
                assert_eq!(
                    from.next(SongEvent::Under(under)),
                    after,
                    "{from:?} on {under:?}"
                );
            }
        }
        for h in held {
            for (under, _) in rows {
                assert_eq!(S::Held(h).next(SongEvent::Under(under)), S::Held(h));
            }
        }
    }

    #[test]
    fn test_one_loop_change_per_transition() {
        let whine = TireSong::Whine(Grooves::AssumedDeck);
        assert_eq!(TireSong::Quiet.sound(), None);
        assert_eq!(whine.sound(), Some(WHINE));
        assert_eq!(TireSong::Hum.sound(), Some(HUM));
        // Whine to hum swaps one loop for the other; how the game knows
        // the grooves never changes the sound.
        assert_eq!(
            TireSong::Whine(Grooves::DerivedFromBuildYear).sound(),
            whine.sound()
        );
        assert_eq!(TireSong::Held(HeldSong::Hum).sound(), Some(HUM));
        assert_eq!(TireSong::Held(HeldSong::Quiet).sound(), None);
    }

    #[test]
    fn test_the_mix_follows_speed() {
        let whine = TireSong::Whine(Grooves::DerivedFromBuildYear);
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
        let (_, rate, level) = TireSong::Hum.mix(40.0).unwrap();
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
            TireSong::Whine(Grooves::DerivedFromBuildYear).describe(),
            "whine (derived from build year)"
        );
        assert_eq!(
            TireSong::Whine(Grooves::AssumedDeck).describe(),
            "whine (assumed, concrete deck)"
        );
        assert_eq!(TireSong::Hum.describe(), "hum");
        assert_eq!(TireSong::Held(HeldSong::Whine).describe(), "held whine");
        for h in ["quiet", "whine", "hum"] {
            assert_eq!(HeldSong::from_name(h).map(HeldSong::name), Some(h));
        }
        assert_eq!(HeldSong::from_name("loud"), None);
    }
}
