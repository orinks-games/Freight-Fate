//! The road under the wheels: which stretches have seams, how hard they
//! hit, and when each axle of the truck crosses one (a player's ask,
//! relayed by Orinks 2026-11-19).
//!
//! The surface comes from the pavement layer (`Leg::pavement`, FHWA HPMS
//! Surface_Type) wherever it reads the road under the truck. Where it reads
//! nothing, the road is invented: jointed and smooth stretches alternate at
//! lengths drawn from the drive's road texture RNG (one seed replays the same
//! road). [`RoadSurface`] says which, so the two are never mistaken for each
//! other. Every distance here is real wheel travel, not the trip model's
//! compressed route miles, so a stretch sounds the same at every pace.
//!
//! Bridge joints are in [`joints`]. Pure ambience: nothing here is spoken.

use crate::data::pavement::{Surface, SurfaceGroup};
use crate::pyrandom::PyRandom;

pub mod joints;

/// An invented jointed stretch runs this many metres of wheel travel.
pub const JOINTED_M: (f64, f64) = (400.0, 1_500.0);
/// An invented smooth stretch runs this many metres.
pub const SMOOTH_M: (f64, f64) = (800.0, 3_000.0);
/// Metres between two seams on a jointed stretch.
///
/// Assumed for the ear, not a slab length: Orinks approved it by ear
/// (2026-11-19). US practice puts jointed plain concrete panels at about
/// 15 feet (4.6 m), and many states cap them there (FHWA Technical Advisory
/// T 5040.30, "Concrete Pavement Joints", January 2019, question 6a), which
/// at highway speed is six seams a second.
pub const SEAM_SPACING_M: (f64, f64) = (14.0, 18.0);
/// Below this speed no new seam is heard.
pub const SEAM_MIN_SPEED_MPS: f64 = 5.0;
/// Steer axle to the drive tandem: an assumed tractor wheelbase, not a
/// measurement of any one truck.
pub const AXLE_GAP_M: f64 = 5.2;
/// Drive tandem to the trailer tandem, assumed for a 53-foot trailer with
/// its tandem slid back.
pub const TRAILER_GAP_M: f64 = 13.0;

/// Faulting under this many inches is good, over [`FAULTING_POOR_IN`] poor,
/// and fair between: the class boundaries are read from 23 CFR 490.313.
pub const FAULTING_FAIR_IN: f64 = 0.10;
pub const FAULTING_POOR_IN: f64 = 0.15;
/// How much harder a seam hits on fair and poor concrete than on good.
/// Assumed for the ear, not measured. A run with no faulting reading is
/// assumed good.
pub const FAIR_SEAM_FACTOR: f64 = 1.25;
pub const POOR_SEAM_FACTOR: f64 = 1.5;
/// Asphalt laid over jointed concrete: the old joints come back through as
/// cracks, softer than a slab edge. Assumed for the ear. HPMS barely reads
/// faulting through an overlay, so this level ignores it.
pub const OVERLAY_SEAM_FACTOR: f64 = 0.5;

/// What the pavement layer reads under the truck.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Reading {
    pub surface: Surface,
    /// Mean faulting between slabs, inches (derived by the layer).
    pub faulting: Option<f64>,
}

/// An invented stretch, used only where the layer reads nothing.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Invented {
    Smooth { left_m: f64 },
    Jointed { left_m: f64 },
}

/// Which road the wheels are on, and how the game knows it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RoadSurface {
    /// The pavement layer reads this surface here.
    Read(Reading),
    /// No reading here: the invented alternation.
    Invented(Invented),
    /// A staged scenario holds one kind for the whole drive; it wins over
    /// the layer.
    Held { jointed: bool },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RoadSurfaceEvent {
    /// The wheels rolled this many metres, onto road the layer reads as
    /// `reading` (`None` where it reads nothing).
    Travel {
        metres: f64,
        reading: Option<Reading>,
    },
}

impl Invented {
    /// A fresh stretch of either kind, at a drawn length.
    pub fn first(rng: &mut PyRandom) -> Invented {
        if rng.random() < 0.5 {
            Invented::Jointed {
                left_m: rng.uniform(JOINTED_M.0, JOINTED_M.1),
            }
        } else {
            Invented::Smooth {
                left_m: rng.uniform(SMOOTH_M.0, SMOOTH_M.1),
            }
        }
    }

    /// Roll `metres`; a stretch that runs out hands the rest of the travel
    /// to the next one, drawn from `rng`.
    pub fn roll(self, metres: f64, rng: &mut PyRandom) -> Invented {
        let mut metres = metres.max(0.0);
        let mut now = self;
        loop {
            let left = now.left_m();
            if metres < left {
                return now.with_left(left - metres);
            }
            metres -= left;
            now = match now {
                Invented::Smooth { .. } => Invented::Jointed {
                    left_m: rng.uniform(JOINTED_M.0, JOINTED_M.1),
                },
                Invented::Jointed { .. } => Invented::Smooth {
                    left_m: rng.uniform(SMOOTH_M.0, SMOOTH_M.1),
                },
            };
        }
    }

    pub fn is_jointed(self) -> bool {
        matches!(self, Invented::Jointed { .. })
    }

    pub fn left_m(self) -> f64 {
        match self {
            Invented::Smooth { left_m } | Invented::Jointed { left_m } => left_m,
        }
    }

    fn with_left(self, left_m: f64) -> Invented {
        match self {
            Invented::Smooth { .. } => Invented::Smooth { left_m },
            Invented::Jointed { .. } => Invented::Jointed { left_m },
        }
    }
}

impl RoadSurface {
    /// The surface a drive starts on: the reading under the truck, or an
    /// invented stretch. The invented stretch is drawn either way, so the
    /// rest of the drive's road texture replays the same.
    pub fn first(reading: Option<Reading>, rng: &mut PyRandom) -> RoadSurface {
        let invented = Invented::first(rng);
        match reading {
            Some(r) => RoadSurface::Read(r),
            None => RoadSurface::Invented(invented),
        }
    }

    /// One kind for the rest of the drive, as a staged scenario asks.
    pub fn held(jointed: bool) -> RoadSurface {
        RoadSurface::Held { jointed }
    }

    /// The only place the surface moves. A reading wins; leaving read road
    /// starts a fresh invented stretch; invented road keeps alternating; a
    /// hold never moves.
    pub fn next(self, event: RoadSurfaceEvent, rng: &mut PyRandom) -> RoadSurface {
        let RoadSurfaceEvent::Travel { metres, reading } = event;
        match (self, reading) {
            (RoadSurface::Held { .. }, _) => self,
            (_, Some(r)) => RoadSurface::Read(r),
            (RoadSurface::Invented(s), None) => RoadSurface::Invented(s.roll(metres, rng)),
            (RoadSurface::Read(_), None) => RoadSurface::Invented(Invented::first(rng)),
        }
    }

    /// How hard a seam hits here, 1.0 for a good jointed slab; `None` where
    /// the road has no seams (asphalt, continuous concrete, unpaved).
    pub fn seam_factor(self) -> Option<f64> {
        match self {
            RoadSurface::Read(r) if r.surface.over_jointed_concrete() => Some(OVERLAY_SEAM_FACTOR),
            RoadSurface::Read(r) => match r.surface.group() {
                SurfaceGroup::JointedConcrete => Some(faulting_factor(r.faulting)),
                _ => None,
            },
            RoadSurface::Invented(s) => s.is_jointed().then_some(1.0),
            RoadSurface::Held { jointed } => jointed.then_some(1.0),
        }
    }

    pub fn is_jointed(self) -> bool {
        self.seam_factor().is_some()
    }

    /// The kind of road, as the agent server reports it.
    pub fn name(self) -> &'static str {
        if self.is_jointed() {
            "jointed"
        } else {
            "smooth"
        }
    }

    /// How the game knows, as the agent server reports it: `read` and the
    /// layer's class, `invented`, or `held`.
    pub fn origin(self) -> String {
        match self {
            RoadSurface::Read(r) => format!("read {}", r.surface.name()),
            RoadSurface::Invented(_) => "invented".to_string(),
            RoadSurface::Held { .. } => "held".to_string(),
        }
    }
}

/// A jointed slab's seam level by its faulting class (23 CFR 490.313); no
/// reading counts as good.
pub fn faulting_factor(faulting_in: Option<f64>) -> f64 {
    match faulting_in {
        Some(f) if f > FAULTING_POOR_IN => POOR_SEAM_FACTOR,
        Some(f) if f >= FAULTING_FAIR_IN => FAIR_SEAM_FACTOR,
        _ => 1.0,
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
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hit {
    pub axle: Axle,
    pub crossing: Crossing,
    /// The seam's level factor where it was crossed (1.0 for a joint), so
    /// the axles behind hit as hard as the steer axle did.
    pub factor: f64,
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
    pub fn cross(&mut self, crossing: Crossing, factor: f64) -> Hit {
        let hit = |axle| Hit {
            axle,
            crossing,
            factor,
        };
        self.pending.push((AXLE_GAP_M, hit(Axle::Drive)));
        if crossing == Crossing::BridgeJoint {
            self.pending
                .push((AXLE_GAP_M + TRAILER_GAP_M, hit(Axle::Trailer)));
        }
        hit(Axle::Steer)
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
