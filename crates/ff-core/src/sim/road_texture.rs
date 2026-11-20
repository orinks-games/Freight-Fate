//! The road under the wheels: which stretches have seams, and when each axle
//! of the truck crosses one (a player's ask, relayed by Orinks 2026-11-19).
//!
//! Invented texture, not map data. The corridors carry no pavement layer, so
//! jointed and smooth stretches alternate at lengths drawn from the drive's
//! road texture RNG (one seed replays the same road). Every distance here is
//! real wheel travel, not the trip model's compressed route miles, so a
//! stretch sounds the same at every pace.
//!
//! Pure ambience: nothing here is spoken.

use crate::pyrandom::PyRandom;

/// A jointed stretch runs this many metres of wheel travel.
pub const JOINTED_M: (f64, f64) = (400.0, 1_500.0);
/// A smooth stretch runs this many metres.
pub const SMOOTH_M: (f64, f64) = (800.0, 3_000.0);
/// Metres between two seams on a jointed stretch.
pub const SEAM_SPACING_M: (f64, f64) = (14.0, 18.0);
/// Below this speed no new seam is heard.
pub const SEAM_MIN_SPEED_MPS: f64 = 5.0;
/// Steer axle to the drive tandem: an assumed tractor wheelbase, not a
/// measurement of any one truck.
pub const AXLE_GAP_M: f64 = 5.2;
/// Drive tandem to the trailer tandem, assumed for a 53-foot trailer with
/// its tandem slid back.
pub const TRAILER_GAP_M: f64 = 13.0;

/// Which kind of road the wheels are on, and how much of it is left.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RoadSurface {
    Smooth { left_m: f64 },
    Jointed { left_m: f64 },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RoadSurfaceEvent {
    /// The wheels rolled this many metres.
    Travel { metres: f64 },
}

impl RoadSurface {
    /// The first stretch of a drive: either kind, at a drawn length.
    pub fn first(rng: &mut PyRandom) -> RoadSurface {
        if rng.random() < 0.5 {
            RoadSurface::Jointed {
                left_m: rng.uniform(JOINTED_M.0, JOINTED_M.1),
            }
        } else {
            RoadSurface::Smooth {
                left_m: rng.uniform(SMOOTH_M.0, SMOOTH_M.1),
            }
        }
    }

    /// One kind for the rest of the drive, as a staged scenario asks.
    pub fn held(jointed: bool) -> RoadSurface {
        let left_m = f64::INFINITY;
        if jointed {
            RoadSurface::Jointed { left_m }
        } else {
            RoadSurface::Smooth { left_m }
        }
    }

    /// The only place the surface moves. A stretch that runs out hands the
    /// rest of the travel to the next one, drawn from `rng`.
    pub fn next(self, event: RoadSurfaceEvent, rng: &mut PyRandom) -> RoadSurface {
        let RoadSurfaceEvent::Travel { metres } = event;
        let mut metres = metres.max(0.0);
        let mut now = self;
        loop {
            let left = now.left_m();
            if metres < left {
                return now.with_left(left - metres);
            }
            metres -= left;
            now = match now {
                RoadSurface::Smooth { .. } => RoadSurface::Jointed {
                    left_m: rng.uniform(JOINTED_M.0, JOINTED_M.1),
                },
                RoadSurface::Jointed { .. } => RoadSurface::Smooth {
                    left_m: rng.uniform(SMOOTH_M.0, SMOOTH_M.1),
                },
            };
        }
    }

    pub fn is_jointed(self) -> bool {
        matches!(self, RoadSurface::Jointed { .. })
    }

    pub fn left_m(self) -> f64 {
        match self {
            RoadSurface::Smooth { left_m } | RoadSurface::Jointed { left_m } => left_m,
        }
    }

    fn with_left(self, left_m: f64) -> RoadSurface {
        match self {
            RoadSurface::Smooth { .. } => RoadSurface::Smooth { left_m },
            RoadSurface::Jointed { .. } => RoadSurface::Jointed { left_m },
        }
    }

    /// The state's name, as the agent server reports it.
    pub fn name(self) -> &'static str {
        match self {
            RoadSurface::Smooth { .. } => "smooth",
            RoadSurface::Jointed { .. } => "jointed",
        }
    }
}

/// The axle a hit comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axle {
    Steer,
    Drive,
    Trailer,
}

/// What the wheels cross.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Crossing {
    /// A seam in the pavement: the steer axle, then the drives.
    Seam,
    /// The expansion joint at a bridge end: all three axles.
    BridgeJoint,
}

/// One axle crossing one seam or joint.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hit {
    pub axle: Axle,
    pub crossing: Crossing,
}

/// The hits the truck has reached with its steer axle and not yet with the
/// axles behind it, each with the metres left before it lands.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AxleHits {
    pending: Vec<(f64, Hit)>,
}

impl AxleHits {
    /// The steer axle is on it now; its hit is returned to play at once, and
    /// the axles behind are queued.
    pub fn cross(&mut self, crossing: Crossing) -> Hit {
        self.pending.push((
            AXLE_GAP_M,
            Hit {
                axle: Axle::Drive,
                crossing,
            },
        ));
        if crossing == Crossing::BridgeJoint {
            self.pending.push((
                AXLE_GAP_M + TRAILER_GAP_M,
                Hit {
                    axle: Axle::Trailer,
                    crossing,
                },
            ));
        }
        Hit {
            axle: Axle::Steer,
            crossing,
        }
    }

    /// Roll `metres`; the hits that landed, in the order they landed.
    pub fn travel(&mut self, metres: f64) -> Vec<Hit> {
        let metres = metres.max(0.0);
        let mut landed: Vec<(f64, Hit)> = Vec::new();
        self.pending.retain_mut(|(left, hit)| {
            *left -= metres;
            if *left <= 0.0 {
                landed.push((*left, *hit));
                false
            } else {
                true
            }
        });
        // The one with the most travel past it landed first.
        landed.sort_by(|a, b| a.0.total_cmp(&b.0));
        landed.into_iter().map(|(_, hit)| hit).collect()
    }

    pub fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }
}

/// Real seconds between the steer axle's hit and the drives' at `speed_mps`.
pub fn axle_gap_s(speed_mps: f64) -> f64 {
    AXLE_GAP_M / speed_mps.max(f64::EPSILON)
}

#[cfg(test)]
mod tests;
