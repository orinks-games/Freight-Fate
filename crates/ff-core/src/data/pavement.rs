//! What the road is made of along a leg, and where it leaves the ground.
//!
//! `corridor.pavement` is written by `cargo tools hpms-pavement` from FHWA
//! HPMS Surface_Type and Structure_Type: runs of like surface, and bridges,
//! tunnels and causeways, each with the directions it holds for, in the
//! leg's native frame. The surface and the structure are HPMS's own
//! readings; the mileage is the builder's projection onto the archived
//! road. Where HPMS reads nothing the leg has no run, and nothing here
//! fills the gap: a caller that wants a fallback picks its own.
//!
//! Each record is one line of text, to keep the tree small and its diffs one
//! line per record: a run reads `"from_mi to_mi class"`, then `forward` or
//! `reverse` when it holds one way only, then `iri=`, `faulting=` and
//! `inventory_side` where they apply, and on exposed concrete `built=` and
//! `improved=` (HPMS's years of last construction and improvement); a
//! structure reads `"from_mi to_mi kind"`, the way, then `hpms=` (HPMS's own
//! length of it) and, where `cargo tools nbi-decks` matched a National
//! Bridge Inventory record, `deck=`, `nbi=` and on a movable grating
//! `grate=from:to`. The decks and the groove rule are in [`grooves`].
//!
//! Screened on the way in, never edited in the bake (see `CLAUDE.md`,
//! "Provenance"): a run that ends before it starts or runs off the leg, a
//! structure longer than any of its kind on the map's roads, and a stretch
//! where the surface changes class every few hundred feet for miles, which
//! is two carriageways' readings interleaved rather than a road. What was
//! dropped is counted in [`Pavement::screened`]. A structure whose derived
//! span disagrees with HPMS's own length of it is kept and flagged
//! ([`Structure::span_disagrees`]), since either could be the wrong one.
//! The deck readings are screened too: a `grate=` span that runs backwards
//! or outside its structure, a deck on a tunnel, and a `built=` or
//! `improved=` year before 1900 or after the layer's HPMS year each drop
//! that reading (not the record) and count in [`Pavement::screened`].

use serde_json::{Map, Value};

use crate::data::world_models::DataError;
use crate::data::world_parsing::{get_float, get_str};
use crate::pyfmt::py_str_float;

pub mod grooves;
pub use grooves::{cross_grooved, Deck, Grooves};

/// HPMS Surface_Type (HPMS Field Manual, Item 49), by its layer name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Surface {
    Unpaved,
    Asphalt,
    JointedPlainConcrete,
    JointedReinforcedConcrete,
    ContinuousConcrete,
    AsphaltOverAsphalt,
    AsphaltOverJointedConcrete,
    AsphaltOverContinuousConcrete,
    UnbondedJointedConcreteOverlay,
    BondedConcreteOverlay,
    Other,
}

/// The Field Manual's pavement groups for Surface_Type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SurfaceGroup {
    /// Codes 2, 6, 7 and 8: asphalt on top, whatever is under it.
    Asphalt,
    /// Codes 3, 4, 9 and 10: concrete slabs with transverse joints.
    JointedConcrete,
    /// Code 5: continuously reinforced concrete, no joints.
    ContinuousConcrete,
    /// Codes 1 and 11: unpaved, brick, plank and the like.
    Other,
}

const SURFACES: &[(Surface, &str)] = &[
    (Surface::Unpaved, "unpaved"),
    (Surface::Asphalt, "asphalt"),
    (Surface::JointedPlainConcrete, "jointed_plain_concrete"),
    (
        Surface::JointedReinforcedConcrete,
        "jointed_reinforced_concrete",
    ),
    (Surface::ContinuousConcrete, "continuous_concrete"),
    (Surface::AsphaltOverAsphalt, "asphalt_over_asphalt"),
    (
        Surface::AsphaltOverJointedConcrete,
        "asphalt_over_jointed_concrete",
    ),
    (
        Surface::AsphaltOverContinuousConcrete,
        "asphalt_over_continuous_concrete",
    ),
    (
        Surface::UnbondedJointedConcreteOverlay,
        "unbonded_jointed_concrete_overlay",
    ),
    (Surface::BondedConcreteOverlay, "bonded_concrete_overlay"),
    (Surface::Other, "other"),
];

impl Surface {
    pub fn from_name(name: &str) -> Option<Self> {
        SURFACES.iter().find(|(_, n)| *n == name).map(|(s, _)| *s)
    }

    pub fn name(self) -> &'static str {
        SURFACES
            .iter()
            .find(|(s, _)| *s == self)
            .map_or("other", |(_, n)| n)
    }

    pub fn group(self) -> SurfaceGroup {
        match self {
            Surface::Asphalt
            | Surface::AsphaltOverAsphalt
            | Surface::AsphaltOverJointedConcrete
            | Surface::AsphaltOverContinuousConcrete => SurfaceGroup::Asphalt,
            Surface::JointedPlainConcrete
            | Surface::JointedReinforcedConcrete
            | Surface::UnbondedJointedConcreteOverlay
            | Surface::BondedConcreteOverlay => SurfaceGroup::JointedConcrete,
            Surface::ContinuousConcrete => SurfaceGroup::ContinuousConcrete,
            Surface::Unpaved | Surface::Other => SurfaceGroup::Other,
        }
    }

    /// Asphalt laid over jointed concrete, whose joints come back through
    /// as cracks over the years (the brief's softer seam).
    pub fn over_jointed_concrete(self) -> bool {
        self == Surface::AsphaltOverJointedConcrete
    }
}

/// HPMS Structure_Type (HPMS Field Manual, Item 4). HPMS reports
/// bridge-sized culverts as bridges.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StructureKind {
    Bridge,
    Tunnel,
    Causeway,
}

impl StructureKind {
    pub fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "bridge" => StructureKind::Bridge,
            "tunnel" => StructureKind::Tunnel,
            "causeway" => StructureKind::Causeway,
            _ => return None,
        })
    }

    pub fn name(self) -> &'static str {
        match self {
            StructureKind::Bridge => "bridge",
            StructureKind::Tunnel => "tunnel",
            StructureKind::Causeway => "causeway",
        }
    }

    /// The longest of the kind a record may claim before it is screened.
    ///
    /// Above the longest of each on the lower 48's highways, with room for
    /// the builder's projection: the Manchac Swamp Bridge on I-55 (22.8 mi,
    /// which HPMS reads as 22.45), the Eisenhower-Johnson tunnels on I-70
    /// (1.7 mi), the Lake Pontchartrain Causeway (23.8 mi).
    pub fn max_mi(self) -> f64 {
        match self {
            StructureKind::Bridge => 25.0,
            StructureKind::Tunnel => 3.0,
            StructureKind::Causeway => 25.0,
        }
    }
}

/// Which travel directions along the leg a record holds for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Ways {
    #[default]
    Both,
    Forward,
    Reverse,
}

impl Ways {
    pub fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "both" => Ways::Both,
            "forward" => Ways::Forward,
            "reverse" => Ways::Reverse,
            _ => return None,
        })
    }

    pub fn name(self) -> &'static str {
        match self {
            Ways::Both => "both",
            Ways::Forward => "forward",
            Ways::Reverse => "reverse",
        }
    }

    pub fn holds(self, forward: bool) -> bool {
        match self {
            Ways::Both => true,
            Ways::Forward => forward,
            Ways::Reverse => !forward,
        }
    }
}

/// One run of like surface, native frame.
#[derive(Debug, Clone, PartialEq)]
pub struct SurfaceRun {
    pub from_mi: f64,
    pub to_mi: f64,
    pub surface: Surface,
    pub ways: Ways,
    /// Mean International Roughness Index, inches per mile.
    pub iri: Option<f64>,
    /// Mean faulting between slabs, inches.
    pub faulting: Option<f64>,
    /// The inventory carriageway's reading standing for a section whose
    /// other carriageway, the one the leg drives, HPMS reads nothing of.
    pub inventory_side: bool,
    /// HPMS Year_Last_Construction (read), on exposed concrete only.
    pub built: Option<i64>,
    /// HPMS Year_Last_Improvement (read), on exposed concrete only.
    pub improved: Option<i64>,
}

/// One bridge, tunnel or causeway, native frame.
#[derive(Debug, Clone, PartialEq)]
pub struct Structure {
    pub from_mi: f64,
    pub to_mi: f64,
    pub kind: StructureKind,
    pub ways: Ways,
    /// HPMS's own length of the sections behind it (read), when known.
    pub hpms_mi: Option<f64>,
    /// The derived span, in measured miles, is more than
    /// [`SPAN_TOLERANCE_MI`] off `hpms_mi`. Flagged, never edited.
    pub span_disagrees: bool,
    /// The deck, from the National Bridge Inventory (read).
    pub deck: Option<Deck>,
    /// The NBI record behind `deck`, `<state FIPS>-<structure number>`.
    pub nbi: Option<String>,
    /// On a movable bridge with an open grating, the movable span (native
    /// frame), DERIVED from the record's span count and longest span
    /// centred on its point. `None` with an open grating: the whole
    /// structure is grating.
    pub grate: Option<(f64, f64)>,
}

/// A leg's whole layer.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Pavement {
    pub surface: Vec<SurfaceRun>,
    pub structures: Vec<Structure>,
    /// Share of the leg with a surface reading, each way.
    pub read_share_forward: f64,
    pub read_share_reverse: f64,
    /// Of that, the share each way ASSUMED from the inventory carriageway.
    pub inventory_share_forward: f64,
    pub inventory_share_reverse: f64,
    /// Leg miles per measured mile of the archived road.
    pub mile_scale: f64,
    pub hpms_year: Option<i64>,
    pub source: String,
    /// Records the load screens dropped.
    pub screened: usize,
}

/// A stretch at least this long...
pub const FLICKER_MI: f64 = 2.0;
/// ...made of runs each shorter than this is interleaved readings.
///
/// Calibrated on the five-leg dry run (2026-11-20, 524 miles in Ohio,
/// Texas, Pennsylvania, California and Wisconsin to Illinois): the shortest
/// real runs there are single tenth-mile sections between long runs, and no
/// stretch of two miles came close. HPMS sections are a tenth of a mile, so
/// two carriageways interleaving would make runs of a tenth or less.
pub const FLICKER_RUN_MI: f64 = 0.2;

/// How far a structure's derived span may sit from HPMS's own length before
/// it is flagged: the archived road is simplified to within 30 m of the
/// real one (`eps_tangent_m` in the geometry archive), which can move each
/// end by about 0.02 mi, plus a 0.005-mile bin at each end. On the five-leg
/// dry run 90 percent of 320 structures agreed within 0.025 mi.
pub const SPAN_TOLERANCE_MI: f64 = 0.05;

/// Two runs closer than this are touching (half a bin, for rounding).
const TOUCH_MI: f64 = 0.0051;

impl Pavement {
    /// The run under `offset_mi` (native frame) for a truck heading
    /// `forward` (from the leg's `from` toward its `to`).
    pub fn surface_at(&self, offset_mi: f64, forward: bool) -> Option<&SurfaceRun> {
        self.surface
            .iter()
            .find(|r| r.ways.holds(forward) && r.from_mi <= offset_mi && offset_mi < r.to_mi)
    }

    /// The structures a truck heading `forward` crosses, `(from_mi, to_mi,
    /// kind)` in the native frame and in mile order. Pieces of one kind
    /// that touch are one structure: a twin bridge whose far side runs a
    /// little longer is stored as a both-ways piece and a one-way tail.
    pub fn structures_for(&self, forward: bool) -> Vec<(f64, f64, StructureKind)> {
        let mut out: Vec<(f64, f64, StructureKind)> = Vec::new();
        let mut pieces: Vec<&Structure> = self
            .structures
            .iter()
            .filter(|s| s.ways.holds(forward))
            .collect();
        pieces.sort_by(|a, b| a.from_mi.total_cmp(&b.from_mi));
        for s in pieces {
            match out.last_mut() {
                Some(last) if last.2 == s.kind && s.from_mi <= last.1 + TOUCH_MI => {
                    last.1 = last.1.max(s.to_mi);
                }
                _ => out.push((s.from_mi, s.to_mi, s.kind)),
            }
        }
        out
    }

    /// The structure under `offset_mi` for a truck heading `forward`.
    pub fn structure_at(&self, offset_mi: f64, forward: bool) -> Option<StructureKind> {
        self.structures_for(forward)
            .into_iter()
            .find(|s| s.0 <= offset_mi && offset_mi < s.1)
            .map(|s| s.2)
    }

    /// The structure under `offset_mi` heading `forward`, as joined by
    /// [`Self::structures_for`], with the piece of it that carries a deck
    /// reading, if any does.
    fn decked_at(&self, offset_mi: f64, forward: bool) -> Option<((f64, f64), &Structure)> {
        let (a, z, _) = self
            .structures_for(forward)
            .into_iter()
            .find(|s| s.0 <= offset_mi && offset_mi < s.1)?;
        self.structures
            .iter()
            .find(|s| s.deck.is_some() && s.ways.holds(forward) && s.from_mi < z && s.to_mi > a)
            .map(|s| ((a, z), s))
    }

    /// The deck under `offset_mi`, for a truck heading `forward`. `None`
    /// off a structure, or on one no NBI record matched.
    pub fn deck_at(&self, offset_mi: f64, forward: bool) -> Option<Deck> {
        self.decked_at(offset_mi, forward).and_then(|(_, s)| s.deck)
    }

    /// Whether the truck is on the open grating itself: the `grate=` span,
    /// or the whole structure when the deck is open grating with no span.
    pub fn on_grating(&self, offset_mi: f64, forward: bool) -> bool {
        self.decked_at(offset_mi, forward).is_some_and(|(_, s)| {
            s.deck == Some(Deck::OpenGrating)
                && s.grate.is_none_or(|(a, z)| a <= offset_mi && offset_mi < z)
        })
    }

    /// Whether the surface here is grooved across the lane, and how the
    /// game knows; `state` is the state the truck is in, by name. `None`
    /// where it is not, or where nothing says. The rule is
    /// [`cross_grooved`].
    pub fn cross_grooved_at(&self, offset_mi: f64, forward: bool, state: &str) -> Option<Grooves> {
        let deck = self
            .decked_at(offset_mi, forward)
            .and_then(|((a, z), s)| Some((s.deck?, z - a)));
        cross_grooved(deck, self.surface_at(offset_mi, forward), state)
    }
}

fn object<'a>(
    raw: &'a Value,
    what: &str,
    from_city: &str,
    to_city: &str,
) -> Result<&'a Map<String, Value>, DataError> {
    raw.as_object().ok_or_else(|| {
        DataError::value(format!(
            "{from_city} to {to_city} pavement {what} must be an object"
        ))
    })
}

/// One record line, split: its two miles, its name, the way it holds, and
/// the `key=value` and bare-word tags after them.
struct Line<'a> {
    from_mi: f64,
    to_mi: f64,
    name: &'a str,
    ways: Ways,
    tags: Vec<&'a str>,
}

impl<'a> Line<'a> {
    fn parse(text: &'a str, what: &str, from_city: &str, to_city: &str) -> Result<Self, DataError> {
        let bad = |why: &str| {
            DataError::value(format!(
                "{from_city} to {to_city} pavement {what} {text:?}: {why}"
            ))
        };
        let mut words = text.split_whitespace();
        let mut mile = || -> Result<f64, DataError> {
            words
                .next()
                .and_then(|w| w.parse::<f64>().ok())
                .filter(|m| m.is_finite())
                .ok_or_else(|| bad("does not start with two miles"))
        };
        let (from_mi, to_mi) = (mile()?, mile()?);
        let name = words.next().ok_or_else(|| bad("has no name"))?;
        let mut ways = Ways::Both;
        let mut tags = Vec::new();
        for w in words {
            match Ways::from_name(w) {
                Some(way) => ways = way,
                None => tags.push(w),
            }
        }
        Ok(Line {
            from_mi,
            to_mi,
            name,
            ways,
            tags,
        })
    }

    fn number(&self, key: &str) -> Result<Option<f64>, DataError> {
        let Some(value) = self.value(key) else {
            return Ok(None);
        };
        value
            .parse::<f64>()
            .ok()
            .filter(|v| v.is_finite())
            .map(Some)
            .ok_or_else(|| DataError::value(format!("pavement {key}={value} is not a number")))
    }

    fn flag(&self, word: &str) -> bool {
        self.tags.contains(&word)
    }

    fn value(&self, key: &str) -> Option<&'a str> {
        self.tags
            .iter()
            .find_map(|t| t.strip_prefix(key)?.strip_prefix('='))
    }

    fn year(&self, key: &str) -> Result<Option<i64>, DataError> {
        self.value(key)
            .map(|v| {
                v.parse::<i64>()
                    .map_err(|_| DataError::value(format!("pavement {key}={v} is not a year")))
            })
            .transpose()
    }

    fn deck(&self) -> Result<Option<Deck>, DataError> {
        self.value("deck")
            .map(|v| {
                Deck::from_name(v)
                    .ok_or_else(|| DataError::value(format!("pavement deck={v} is not a deck")))
            })
            .transpose()
    }

    /// `key=from:to`, two miles.
    fn span(&self, key: &str) -> Result<Option<(f64, f64)>, DataError> {
        let Some(v) = self.value(key) else {
            return Ok(None);
        };
        let mile = |w: &str| w.parse::<f64>().ok().filter(|m| m.is_finite());
        v.split_once(':')
            .and_then(|(a, z)| Some((mile(a)?, mile(z)?)))
            .map(Some)
            .ok_or_else(|| DataError::value(format!("pavement {key}={v} is not two miles")))
    }
}

/// Drop deck and year readings that contradict themselves or their
/// record; the records stay. Returns how many readings were dropped.
fn screen_readings(
    surface: &mut [SurfaceRun],
    structures: &mut [Structure],
    hpms_year: Option<i64>,
) -> usize {
    let mut dropped = 0;
    let latest = hpms_year.unwrap_or(i64::MAX);
    for r in surface.iter_mut() {
        for year in [&mut r.built, &mut r.improved] {
            if year.is_some_and(|y| !(1900..=latest).contains(&y)) {
                *year = None;
                dropped += 1;
            }
        }
    }
    for s in structures.iter_mut() {
        if s.kind == StructureKind::Tunnel && s.deck.is_some() {
            s.deck = None;
            s.grate = None;
            dropped += 1;
        }
        if s.grate
            .is_some_and(|(a, z)| !(a < z && a >= s.from_mi - TOUCH_MI && z <= s.to_mi + TOUCH_MI))
        {
            // Without its span an open grating would read as the whole
            // structure, which is the claim the bad span did not make.
            s.grate = None;
            s.deck = None;
            dropped += 1;
        }
    }
    dropped
}

fn lines<'a>(raw: &'a Map<String, Value>, key: &str) -> Vec<&'a str> {
    raw.get(key)
        .and_then(Value::as_array)
        .map(|items| items.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default()
}

/// On the leg, starting before it ends, and no longer than `max_mi`.
fn sane(from_mi: f64, to_mi: f64, leg_miles: f64, max_mi: f64) -> bool {
    from_mi >= 0.0 && to_mi > from_mi && to_mi <= leg_miles + 0.01 && to_mi - from_mi <= max_mi
}

/// Drop every run inside a stretch of [`FLICKER_MI`] or more made of runs
/// shorter than [`FLICKER_RUN_MI`] touching end to end, one direction at a
/// time. Returns how many were dropped.
fn screen_flicker(runs: &mut Vec<SurfaceRun>) -> usize {
    let mut drop = vec![false; runs.len()];
    for forward in [true, false] {
        let mut order: Vec<usize> = (0..runs.len())
            .filter(|&i| runs[i].ways.holds(forward))
            .collect();
        order.sort_by(|&a, &b| runs[a].from_mi.total_cmp(&runs[b].from_mi));
        let short = |i: usize| runs[i].to_mi - runs[i].from_mi < FLICKER_RUN_MI;
        let mut start = 0;
        while start < order.len() {
            let mut end = start;
            while end < order.len()
                && short(order[end])
                && (end == start
                    || runs[order[end]].from_mi - runs[order[end - 1]].to_mi < TOUCH_MI)
            {
                end += 1;
            }
            if end > start && runs[order[end - 1]].to_mi - runs[order[start]].from_mi >= FLICKER_MI
            {
                for &i in &order[start..end] {
                    drop[i] = true;
                }
            }
            start = end.max(start + 1);
        }
    }
    let before = runs.len();
    let mut keep = drop.iter().map(|d| !d);
    runs.retain(|_| keep.next().unwrap_or(true));
    before - runs.len()
}

/// Parse `corridor.pavement`; `None` when the leg has none.
pub fn parse_pavement(
    raw: Option<&Value>,
    leg_miles: f64,
    from_city: &str,
    to_city: &str,
) -> Result<Option<Pavement>, DataError> {
    let Some(raw) = raw.filter(|v| !v.is_null()) else {
        return Ok(None);
    };
    let raw = object(raw, "layer", from_city, to_city)?;
    let mut screened = 0;
    let mut surface = Vec::new();
    for text in lines(raw, "surface") {
        let line = Line::parse(text, "run", from_city, to_city)?;
        let kind = Surface::from_name(line.name).ok_or_else(|| {
            DataError::value(format!(
                "{from_city} to {to_city} pavement surface {:?} is not an HPMS surface",
                line.name
            ))
        })?;
        if !sane(line.from_mi, line.to_mi, leg_miles, leg_miles) {
            screened += 1;
            continue;
        }
        surface.push(SurfaceRun {
            from_mi: line.from_mi,
            to_mi: line.to_mi,
            surface: kind,
            ways: line.ways,
            iri: line.number("iri")?,
            faulting: line.number("faulting")?,
            inventory_side: line.flag("inventory_side"),
            built: line.year("built")?,
            improved: line.year("improved")?,
        });
    }
    surface.sort_by(|a, b| a.from_mi.total_cmp(&b.from_mi));
    screened += screen_flicker(&mut surface);
    let mut structures = Vec::new();
    for text in lines(raw, "structures") {
        let line = Line::parse(text, "structure", from_city, to_city)?;
        let kind = StructureKind::from_name(line.name).ok_or_else(|| {
            DataError::value(format!(
                "{from_city} to {to_city} pavement structure {:?} is not a bridge, tunnel or \
                 causeway",
                line.name
            ))
        })?;
        if !sane(line.from_mi, line.to_mi, leg_miles, kind.max_mi()) {
            screened += 1;
            continue;
        }
        structures.push(Structure {
            from_mi: line.from_mi,
            to_mi: line.to_mi,
            kind,
            ways: line.ways,
            hpms_mi: line.number("hpms")?,
            span_disagrees: false,
            deck: line.deck()?,
            nbi: line.value("nbi").map(str::to_string),
            grate: line.span("grate")?,
        });
    }
    structures.sort_by(|a, b| a.from_mi.total_cmp(&b.from_mi));
    let meta = raw.get("meta").and_then(Value::as_object);
    let meta_float = |key: &str, default: f64| -> Result<f64, DataError> {
        match meta {
            Some(m) => get_float(m, key, default),
            None => Ok(default),
        }
    };
    let share = |key: &str| meta_float(key, 0.0);
    let mile_scale = meta_float("mile_scale", 1.0)?;
    if !(mile_scale > 0.5 && mile_scale < 2.0) {
        return Err(DataError::value(format!(
            "{from_city} to {to_city} pavement mile_scale {} is not near 1",
            py_str_float(mile_scale)
        )));
    }
    let hpms_year = meta
        .and_then(|m| m.get("hpms_year"))
        .and_then(Value::as_i64);
    screened += screen_readings(&mut surface, &mut structures, hpms_year);
    for s in &mut structures {
        s.span_disagrees = s
            .hpms_mi
            .is_some_and(|mi| ((s.to_mi - s.from_mi) / mile_scale - mi).abs() > SPAN_TOLERANCE_MI);
    }
    let (read_share_forward, read_share_reverse) =
        (share("read_share_forward")?, share("read_share_reverse")?);
    let (inventory_share_forward, inventory_share_reverse) = (
        share("inventory_share_forward")?,
        share("inventory_share_reverse")?,
    );
    for s in [
        read_share_forward,
        read_share_reverse,
        inventory_share_forward,
        inventory_share_reverse,
    ] {
        if !(0.0..=1.0).contains(&s) {
            return Err(DataError::value(format!(
                "{from_city} to {to_city} pavement read share {} is not between 0 and 1",
                py_str_float(s)
            )));
        }
    }
    Ok(Some(Pavement {
        surface,
        structures,
        read_share_forward,
        read_share_reverse,
        inventory_share_forward,
        inventory_share_reverse,
        mile_scale,
        hpms_year,
        source: meta.map(|m| get_str(m, "source")).unwrap_or_default(),
        screened,
    }))
}

#[cfg(test)]
mod tests;
