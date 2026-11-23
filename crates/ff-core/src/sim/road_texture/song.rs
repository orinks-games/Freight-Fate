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

/// The real seconds a song has left to sound once its surface is behind
/// the truck, and whether it is behind.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hold {
    pub left_s: f64,
    /// The truck is past the surface; only the hold keeps the song.
    pub past: bool,
}

impl Hold {
    fn fresh(left_s: f64) -> Hold {
        Hold {
            left_s,
            past: false,
        }
    }

    fn after(self, dt: f64, past: bool) -> Hold {
        Hold {
            left_s: (self.left_s - dt.max(0.0)).max(0.0),
            past,
        }
    }

    fn holds(self) -> bool {
        self.left_s > 0.0
    }
}

/// A grating hums for at least this many real seconds from when the hum
/// starts. Route miles pass 10 or 20 times faster than the wheels turn, so
/// a movable span of 0.07 mile would otherwise be a 0.6 s burst. Assumed
/// for the ear, in the manner of the river's hold.
pub const HUM_MIN_S: f64 = 3.0;
/// A concrete deck whines for at least this many real seconds: the bridge
/// joints' own gap ([`super::joints::MIN_JOINT_GAP_S`]), so a short deck is
/// thump, whine, thump and a long one still ends on its far joint. Pavement
/// whine has no hold. Assumed for the ear.
pub const DECK_WHINE_MIN_S: f64 = 1.5;
/// Once a concrete deck's whine ends, the next deck stays quiet for this
/// many real seconds. Route miles pass 10 to 20 times faster than the
/// wheels turn, so a city's overpasses, a few tenths of a mile apart, would
/// otherwise whine every two or three seconds and sound like a fault (a
/// player's report, 2026-11-23). Grooved pavement and gratings are not
/// held off. Assumed for the ear.
pub const DECK_WHINE_REST_S: f64 = 10.0;

/// What the tires are singing.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TireSong {
    Quiet,
    /// Quiet after a concrete deck's whine: another deck stays quiet for
    /// these real seconds ([`DECK_WHINE_REST_S`]).
    Rest(f64),
    Whine(Grooves, Hold),
    Hum(Hold),
    /// A staged scenario holds one song; it wins over the layer.
    Held(HeldSong),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SongEvent {
    /// The steer axle is now on this, `dt` real seconds after the last.
    Under { under: Underfoot, dt: f64 },
}

fn whine_hold(grooves: Grooves) -> f64 {
    match grooves {
        Grooves::AssumedDeck => DECK_WHINE_MIN_S,
        Grooves::DerivedFromBuildYear => 0.0,
    }
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
        TireSong::Quiet.next(SongEvent::Under { under, dt: 0.0 })
    }

    /// The only place the song moves. A hold never moves; a grating wins
    /// over grooves, and a hum still holding wins over a whine; a song whose
    /// surface is behind the truck sounds out its hold, then goes quiet; a
    /// deck's whine is followed by a rest in which another deck is quiet.
    pub fn next(self, event: SongEvent) -> TireSong {
        let SongEvent::Under { under, dt } = event;
        let dt = dt.max(0.0);
        let hold = match self {
            TireSong::Held(_) => return self,
            TireSong::Quiet | TireSong::Rest(_) => None,
            TireSong::Whine(_, h) | TireSong::Hum(h) => Some(h),
        };
        match (self, under) {
            (TireSong::Hum(h), Underfoot::Grating) => TireSong::Hum(h.after(dt, false)),
            (_, Underfoot::Grating) => TireSong::Hum(Hold::fresh(HUM_MIN_S)),
            (TireSong::Hum(h), _) if h.after(dt, true).holds() => TireSong::Hum(h.after(dt, true)),
            (TireSong::Whine(was, h), Underfoot::Grooved(g)) if was == g => {
                TireSong::Whine(g, h.after(dt, false))
            }
            (TireSong::Rest(left), Underfoot::Grooved(Grooves::AssumedDeck)) if left > dt => {
                TireSong::Rest(left - dt)
            }
            (_, Underfoot::Grooved(g)) => TireSong::Whine(g, Hold::fresh(whine_hold(g))),
            (_, Underfoot::Plain) => match (self, hold.map(|h| h.after(dt, true))) {
                (TireSong::Whine(g, _), Some(h)) if h.holds() => TireSong::Whine(g, h),
                (TireSong::Whine(Grooves::AssumedDeck, _), _) => TireSong::Rest(DECK_WHINE_REST_S),
                (TireSong::Rest(left), _) if left > dt => TireSong::Rest(left - dt),
                _ => TireSong::Quiet,
            },
        }
    }

    /// The loop this song plays, if any.
    pub fn sound(self) -> Option<SongLoop> {
        match self {
            TireSong::Quiet | TireSong::Rest(_) | TireSong::Held(HeldSong::Quiet) => None,
            TireSong::Whine(..) | TireSong::Held(HeldSong::Whine) => Some(WHINE),
            TireSong::Hum(_) | TireSong::Held(HeldSong::Hum) => Some(HUM),
        }
    }

    /// The song as the agent server's `status` reports it.
    pub fn describe(self) -> String {
        let holding = |h: Hold| if h.past { ", holding" } else { "" };
        match self {
            TireSong::Quiet => "quiet".to_string(),
            TireSong::Rest(_) => "quiet (resting after a deck)".to_string(),
            TireSong::Whine(g, h) => format!("whine ({}{})", grooves_origin(g), holding(h)),
            TireSong::Hum(h) if h.past => "hum (holding)".to_string(),
            TireSong::Hum(_) => "hum".to_string(),
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
mod tests;
