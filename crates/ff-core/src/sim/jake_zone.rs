//! What the driver has been told about a no engine brake zone on this pass.
//!
//! One enum, [`ZoneNotice`], with one [`ZoneNotice::next`] and nothing else
//! allowed to move it. Each transition yields at most one [`ZoneCue`]; the
//! driving state speaks it and keeps the fines, the sounds and the assists.
//!
//! A zone is quiet on the way in when the engine brake is off: with nothing
//! to switch, the sign is noise (module doc of `driving_engine_brake.rs` in
//! the game). A driver who did hear about the zone (the heads-up before it,
//! the assist letting go of the retarder at the town line, or the warning to
//! switch it off) hears once when it ends, so they know the retarder is
//! legal again. Terse speech skips the heads-up and the end alike.

/// Where the notice for the zone ahead or under the truck stands.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum ZoneNotice {
    /// Nothing said about a zone ahead, and not inside one.
    #[default]
    Untold,
    /// The heads-up before the zone starting at `start_tenths` (its start
    /// mile, in tenths) has been read.
    HeadsUp { start_tenths: i64 },
    /// Inside the zone around `city`. `told` once any zone line reached the
    /// driver on this pass; `assist_cued` once the assist's line has been
    /// said for this city.
    Inside {
        city: String,
        told: bool,
        assist_cued: bool,
    },
}

/// What happened around a zone this frame.
#[derive(Clone, Debug, PartialEq)]
pub enum ZoneEvent {
    /// Outside any zone, with the driver's own retarder on and a zone
    /// starting at `start_mi` close enough for the heads-up.
    Approaching { start_mi: f64 },
    /// Inside the zone around `city`.
    Inside { city: String },
    /// Inside a zone, an assist just let go of a retarder it had raised.
    AssistReleased,
    /// Inside a zone, the warning to switch the engine brake off was spoken.
    Warned,
    /// Outside any zone.
    Outside,
}

/// The one line a transition gives the driving state to speak.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ZoneCue {
    /// "No engine brake zone in {distance}, coming into {city}."
    HeadsUp,
    /// "No engine brake zone, {city}."
    AssistReleased,
    /// "End of no engine brake zone."
    Ends,
}

impl ZoneNotice {
    /// The only place the zone notice moves. `terse` is the terse speech
    /// setting: it skips the heads-up, the assist line and the end line.
    pub fn next(self, event: ZoneEvent, terse: bool) -> (ZoneNotice, Option<ZoneCue>) {
        use ZoneNotice::*;
        match (self, event) {
            (HeadsUp { start_tenths }, ZoneEvent::Approaching { start_mi })
                if start_tenths == tenths(start_mi) =>
            {
                (HeadsUp { start_tenths }, None)
            }
            (state, ZoneEvent::Approaching { start_mi }) => {
                if terse || matches!(state, Inside { .. }) {
                    (state, None)
                } else {
                    let start_tenths = tenths(start_mi);
                    (HeadsUp { start_tenths }, Some(ZoneCue::HeadsUp))
                }
            }
            (
                Inside {
                    city: here,
                    told,
                    assist_cued,
                },
                ZoneEvent::Inside { city },
            ) => {
                // One town's zone running straight into the next keeps what
                // the driver was told; the assist line is per town.
                let assist_cued = assist_cued && here == city;
                (
                    Inside {
                        city,
                        told,
                        assist_cued,
                    },
                    None,
                )
            }
            (state, ZoneEvent::Inside { city }) => {
                let told = matches!(state, HeadsUp { .. });
                (
                    Inside {
                        city,
                        told,
                        assist_cued: false,
                    },
                    None,
                )
            }
            (
                Inside {
                    city,
                    assist_cued: false,
                    ..
                },
                ZoneEvent::AssistReleased,
            ) if !terse => (
                Inside {
                    city,
                    told: true,
                    assist_cued: true,
                },
                Some(ZoneCue::AssistReleased),
            ),
            (
                Inside {
                    city, assist_cued, ..
                },
                ZoneEvent::Warned,
            ) => (
                Inside {
                    city,
                    told: true,
                    assist_cued,
                },
                None,
            ),
            (Inside { told, .. }, ZoneEvent::Outside) => {
                let cue = (told && !terse).then_some(ZoneCue::Ends);
                (Untold, cue)
            }
            (state, _) => (state, None),
        }
    }

    /// The state's name, as the agent server's `status` reports it.
    pub fn key(&self) -> &'static str {
        match self {
            ZoneNotice::Untold => "untold",
            ZoneNotice::HeadsUp { .. } => "heads_up",
            ZoneNotice::Inside { told: false, .. } => "inside_untold",
            ZoneNotice::Inside { told: true, .. } => "inside_told",
        }
    }
}

fn tenths(mile: f64) -> i64 {
    (mile * 10.0).round() as i64
}

#[cfg(test)]
mod tests;
