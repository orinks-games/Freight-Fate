//! The tires' song: the low hum of an open steel grate bridge deck.
//!
//! Its pitch follows speed. Grooved concrete is quiet.
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
    /// A surface that does not produce a tire song, or road the layer says
    /// nothing about.
    Plain,
    Grooved(Grooves),
    /// An open steel grating (National Bridge Inventory Item 107, read).
    Grating,
}

/// A song a staged scenario holds for the whole drive.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HeldSong {
    Quiet,
    Hum,
}

impl HeldSong {
    pub fn from_name(name: &str) -> Option<HeldSong> {
        match name {
            "quiet" => Some(HeldSong::Quiet),
            "hum" => Some(HeldSong::Hum),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            HeldSong::Quiet => "quiet",
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
/// What the tires are singing.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TireSong {
    Quiet,
    Hum(Hold),
    /// A staged scenario holds one song; it wins over the layer.
    Held(HeldSong),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SongEvent {
    /// The steer axle is now on this, `dt` real seconds after the last.
    Under { under: Underfoot, dt: f64 },
}

/// The loop, with the speed it was made for.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SongLoop {
    pub key: &'static str,
    /// Metres a second at which the loop plays at its own pitch.
    pub reference_mps: f64,
    /// Its level at and above the reference speed, before the Road surface
    /// volume. The hum sits about 10 dB over the mix in 140 to 175 Hz at 45
    /// mph; it plays at full level, and its loop is raised 4 dB to get there.
    pub level: f64,
}

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

    /// The only place the song moves. A hold never moves; a grating starts
    /// the hum, which sounds out its hold after the surface is behind the
    /// truck; other surfaces are quiet.
    pub fn next(self, event: SongEvent) -> TireSong {
        let SongEvent::Under { under, dt } = event;
        let dt = dt.max(0.0);
        if let TireSong::Held(_) = self {
            return self;
        }
        match (self, under) {
            (TireSong::Hum(h), Underfoot::Grating) => TireSong::Hum(h.after(dt, false)),
            (_, Underfoot::Grating) => TireSong::Hum(Hold::fresh(HUM_MIN_S)),
            (TireSong::Hum(h), _) if h.after(dt, true).holds() => TireSong::Hum(h.after(dt, true)),
            _ => TireSong::Quiet,
        }
    }

    /// The loop this song plays, if any.
    pub fn sound(self) -> Option<SongLoop> {
        match self {
            TireSong::Quiet | TireSong::Held(HeldSong::Quiet) => None,
            TireSong::Hum(_) | TireSong::Held(HeldSong::Hum) => Some(HUM),
        }
    }

    /// The song as the agent server's `status` reports it.
    pub fn describe(self) -> String {
        match self {
            TireSong::Quiet => "quiet".to_string(),
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
