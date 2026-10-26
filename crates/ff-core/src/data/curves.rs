//! Baked curve geometry for the pacenote layer (port of
//! `freight_fate/data/curves.py`).
//!
//! Reads `world_data/us/gameplay/curves.jsonl` -- the per-curve steering
//! rows from the dense geometry sweep (one bake direction per leg; the
//! runtime mirrors records when a route traverses a leg b-to-a). Connector
//! rows (interchange and ramp arcs) are excluded here: ramps carry their own
//! speech and the future curve-nav layer owns them.
//!
//! Which rows those are is READ from OSM at bake time, not guessed at here:
//! `tools/bake_curve_connectors.py` sets `connector` from the road class
//! under each curve's apex -- a `highway=*_link` way is a ramp, and an
//! Interstate leg's curve that is not on `highway=motorway` is not on the
//! Interstate. Each flagged row records which reading flagged it in
//! `connector_source`.
//!
//! Severity bands come from the advisory speed the bake computed at 0.3 g
//! lateral -- the same number a posted yellow diamond would show -- EXCEPT the
//! hairpin, which is a shape rather than a speed and is decided by deflection
//! alone (see `HAIRPIN_DEFLECTION_DEG`).
//!
//! Interstate mainline records are screened for sweep artifacts on the way in
//! (see `INTERSTATE_MIN_RADIUS_FT`); the raw bake keeps every row.
//!
//! US and state routes get a second, narrower screen: sweep artifacts also
//! land on non-interstate mainline (the same city-departure kink that hits an
//! interstate can ride the US-route out of the same city), but road class
//! alone cannot tell those apart from a real switchback -- US-550 and the Salt
//! River Canyon really are that sharp. `curve_artifacts.jsonl`
//! (`tools/screen_curve_artifacts.py`) names the specific `(leg, seq)`
//! records an offline check flagged: flat local ground where no real hairpin
//! can exist, city-departure geometry at a leg's ends off the mountains, and a
//! radius no through highway of any class can bend to. Everything else, sharp
//! or not, is left alone -- nothing in mountain terrain is ever flagged.
//!
//! A third screen applies to mainline of every class and asks only whether a
//! record agrees with itself: a bend's recorded span has to be able to hold
//! the arc its own radius and deflection describe, and opposite-direction
//! curves cannot meet with no straight between them. Terrain cannot excuse
//! either, so unlike the two above, this one does look at mountain roads --
//! which is where it was found (`ARC_CONSISTENCY_MIN`).

use std::collections::{HashMap, HashSet};

use once_cell::sync::OnceCell;
use serde::Deserialize;

use super::data_resources::{baked, read_data_text};
use super::world::{get_world, World};
use super::world_models::{Leg, Route};

/// The advisory at or below which a curve is an EXTREME CLAIM about the road,
/// used by the artifact screens to ask "could a road here really do this?".
/// Deliberately not the spoken hairpin test -- see `HAIRPIN_DEFLECTION_DEG`.
pub const HAIRPIN_MAX_MPH: i64 = 25;
pub const SHARP_MAX_MPH: i64 = 35;
pub const MODERATE_MAX_MPH: i64 = 50;
/// What actually makes a hairpin, from the sign that names one. MUTCD gives
/// the Hairpin Curve sign (W1-11) for a change in horizontal alignment of 135
/// degrees or more -- a switchback, where the road comes back on itself. The
/// advisory speed does not enter into it: MUTCD sorts curves by advisory
/// separately and much lower down, using the Turn sign (W1-1) instead of the
/// Curve sign at 30 mph or less.
///
/// This used to read "advisory <= 25 OR deflection >= 150", and the advisory
/// half was doing real damage. Across 33,930 baked curves it labelled 159
/// hairpins where only 99 turn 135 degrees or more; the other 60 were tight
/// little bends taken slowly, and the worst of them deflect TEN degrees. A
/// driver was being told "hairpin left" for a road that barely bends, which
/// spends the word on nothing and leaves it meaning less when a real
/// switchback arrives. Darren asked whether a 94-degree corner through
/// Norwich on NY-12 was "supposed to be there" (2026-08-23); the corner was,
/// the word was not.
///
/// The angle is necessary and NOT sufficient, which the data made plain. MUTCD
/// says the hairpin sign goes up INSTEAD OF A TURN OR CURVE SIGN, and it is
/// the Turn sign (advisory 30 or less) that a switchback would otherwise
/// carry. Taking the angle alone put 7 hairpins on interstate mainline --
/// among them a 143-degree bend on I-49 north of Fayetteville with an 811 ft
/// radius and a 60 mph advisory. That is a real half-circle of road and a
/// sweeping one; no driver calls it a hairpin, because you do not slow for it.
///
/// Both together give 46 hairpins in 33,930 curves and NONE on an interstate,
/// which is the check on the rule rather than a target it was fitted to:
/// interstates do not switchback, and the rule works that out on its own.
pub const HAIRPIN_TURN_MAX_MPH: i64 = 30;
pub const HAIRPIN_DEFLECTION_DEG: f64 = 135.0;

/// Interstate mainline geometry screen. The dense sweep baked some city
/// departure geometry and interchange vertices as mainline rather than as
/// connectors, which put 80-250 ft "hairpins" on roads that cannot bend that
/// hard. Anything under 300 ft, or turning more than a switchback's worth, is
/// a digitizing artifact.
///
/// That misclassification is now fixed where it happened -- the bake reads the
/// road class under each apex (see the module docstring) -- so this screen has
/// far less to catch than it did. It stays as the backstop for rows no extract
/// could speak to, and because a genuine digitizing kink on real interstate
/// pavement is still possible.
///
/// 300 is deliberately far below the DESIGN floor, and the difference is not
/// slack -- it is Glenwood Canyon. TxDOT Roadway Design Manual Table 4-7 puts
/// the minimum radius for a 50 mph design speed at 758 ft, and 50 mph is the
/// lowest the Interstate system designs to; raising this constant to 758
/// accordingly reads well and deletes real road, because I-70 through the
/// canyon really does bend tighter than standard under design exceptions
/// (tried and reverted, 2026-08-23 -- `test_glenwood_canyon_interstate_curves_survive`
/// is what caught it). A screen sized to the design floor cannot tell a design
/// exception from an artifact.
///
/// The design floor IS applied, by the fourth screen below, and only where the
/// ground cannot justify a tight bend: `screenable_legs` gives every LEVEL leg
/// a floor of `min_radius_ft(its own design speed)`. That is the same number,
/// spent where it is safe to spend.
///
/// Only the interstate class is screened here -- US and state routes really do
/// switch back (US-550 over Red Mountain Pass, US-40 in the Rockies) and their
/// sharp records are kept exactly as baked.
pub const INTERSTATE_MIN_RADIUS_FT: i64 = 300;
pub const INTERSTATE_MAX_DEFLECTION_DEG: f64 = 150.0;

/// Geometry self-consistency screen, applied to mainline of every class.
///
/// The two screens above ask "could a road of this class bend this hard?" and
/// "does the ground under it allow a hairpin?". Both deliberately leave
/// mountain terrain alone, because that is where real switchbacks live. This
/// third screen asks something neither does and that terrain cannot excuse:
/// does the record agree with ITSELF?
///
/// A curve of radius R turning theta has an arc length of R*theta. The
/// recorded start-to-end span is measured along the same road, so a healthy
/// record spans at least its own arc -- the median mainline row comes in at
/// 1.22x. A row spanning a small fraction of that is not a bend the sweep
/// measured; it is two adjacent route points with a kink between them, and no
/// amount of real mountain justifies it.
///
/// Found on US-285 north of Santa Fe (owner playtest, 2026-08-17): a 160 ft
/// "hairpin" turning 79.9 degrees over 53 feet of road, where that geometry
/// needs 223. It sat in mountain terrain, so both existing screens correctly
/// passed it, and the pacenote layer called a 25 mph hairpin on a road posted
/// 35.
pub const ARC_CONSISTENCY_MIN: f64 = 0.1;
/// What a row with no usable radius/deflection reports: comfortably passing,
/// so an incomplete record is never dropped on arithmetic it never supplied.
const CONSISTENT_ENOUGH: f64 = 9.9;

/// The same artifact's other signature, and the one that caught the US-285
/// case: a digitized kink shows up as opposite-direction curves with NO
/// tangent between them. A real reversal at this radius has straight road in
/// between -- a through highway cannot swap lock at a point. Both sides of the
/// zig-zag go, not just the arithmetically worst one, because the wiggle is
/// spurious as a whole: the road does not really go left-right-left there.
pub const ZIGZAG_MAX_TANGENT_FT: f64 = 1.0;
pub const ZIGZAG_MAX_RADIUS_FT: i64 = 400;
/// One side must also fail arithmetic, at a looser bar than ARC_CONSISTENCY_MIN
/// -- an abrupt reversal alone is suggestive, not proof, and compound S-curves
/// are real.
pub const ZIGZAG_ARC_MAX: f64 = 0.5;

/// How near a way must be to count as the road under a curve apex, in metres.
///
/// DERIVED, from the archive's own precision rather than from taste. The
/// archival polyline is quantized at 1e-5 degrees (~1.1 m) and curve spans
/// are kept at eps=0, so an apex sits essentially on the way ORS routed it
/// along -- MEASURED over Alabama and Colorado, the nearest road of any kind
/// is within 0.6 m of the apex for 99 percent of curves, and beyond 25 m for
/// 4 of 1,245. So this is a sanity bound on "was anything read at all", not a
/// decision: the verdict comes from which way is nearest, and where a ramp and
/// the mainline it leaves are both in range they are a median 11 m apart,
/// twenty times the positioning error.
pub const CONNECTOR_CORRIDOR_M: f64 = 25.0;

/// What OSM calls a controlled-access freeway. An Interstate mainline mile is
/// one by statute (23 CFR 625 adopts the AASHTO Interstate design standards),
/// so this is the class an Interstate leg's mainline curves must ride -- not a
/// threshold, a definition.
pub const CONNECTOR_FREEWAY_CLASS: &str = "motorway";

/// OSM's own functional hierarchy, most important first. This is upstream's
/// ordering, not a weighting anybody chose: the wiki defines the tags as a
/// descending ladder of road importance, and every US mapper applies it that
/// way.
///
/// The rule compares a curve's road against the road ITS LEG IS MADE OF rather
/// than against a fixed class, and it is the comparison that does the work. A
/// leg made of motorway keeps only motorway bends, which is the Interstate
/// case. A leg made of trunk -- a curated I-65 whose route actually runs
/// US-231 end to end -- keeps its trunk bends, because those are that road's
/// real mainline, while still dropping the primary and residential kinks where
/// the route threads a town. Judging such a leg against `motorway` restored
/// 2,677 rows including Brickell Avenue in downtown Miami at a 38 ft radius;
/// judging it against its own class does not.
///
/// `service_other` is Valhalla's name for a service road, which OSM tags
/// `highway=service`. It covers the alleys, driveways and yard roads a route
/// threads at its very ends. Ranked last because a bend on one is never the
/// through road -- and it has to be HERE rather than absent: an unranked class
/// reads as "nothing may be concluded", which would have let service roads
/// through as mainline on every leg.
pub const CONNECTOR_CLASS_RANK: &[(&str, u8)] = &[
    ("motorway", 0),
    ("trunk", 1),
    ("primary", 2),
    ("secondary", 3),
    ("tertiary", 4),
    ("unclassified", 5),
    ("residential", 6),
    ("service_other", 7),
];

fn class_rank(class: &str) -> Option<u8> {
    CONNECTOR_CLASS_RANK
        .iter()
        .find(|(name, _)| *name == class)
        .map(|(_, rank)| *rank)
}

/// `(is connector, why)` for one curve, from its map-matched reading alone --
/// `tools/bake_curve_connectors.py::classify`.
///
/// The bake was a Python tool, retired with the rest of `tools/`, but the
/// RULE lives here, because it is what every `connector_source` in the shipped
/// data means and the only way those rows can be re-judged without re-baking.
///
/// Two readings:
///
/// * `osm:ramp` -- the apex rides a `highway=*_link` way, which is the tag OSM
///   uses for a ramp, slip road or interchange connector. True on every road
///   class.
/// * `osm:off-corridor` -- the apex rides a road LESS IMPORTANT than the one
///   its leg is made of: the town the route threads on its way in or out,
///   rather than the through road.
///
/// `made_of` is the class of road the LEG is built from, read per mile from
/// the route rather than taken from the leg's label. An Interstate leg is made
/// of `motorway` (every Interstate mainline mile is a controlled-access
/// freeway by statute, 23 CFR 625), so only motorway bends are its mainline. A
/// curated I-65 leg whose route actually runs US-231 end to end is made of
/// `trunk`, and its trunk bends are that road's real mainline.
///
/// A `near_m` or `near_hw` of `None` means the apex had no road within the
/// corridor: nothing was read, so nothing may be concluded, and the empty
/// reason tells the bake to keep whatever the positional sweep said. A
/// `made_of` that was never read concludes mainline rather than moving a row.
///
/// It never sees radius, deflection, advisory or severity, so it CANNOT tell a
/// sharp curve from a gentle one and therefore cannot delete a design exception
/// -- see `INTERSTATE_MIN_RADIUS_FT` for what happened the one time a screen
/// here was allowed to look at the geometry.
pub fn classify_connector(
    near_m: Option<f64>,
    near_hw: Option<&str>,
    made_of: Option<&str>,
) -> (bool, &'static str) {
    let (Some(near_m), Some(near_hw)) = (near_m, near_hw) else {
        return (false, "");
    };
    if near_m > CONNECTOR_CORRIDOR_M {
        return (false, "");
    }
    if near_hw.ends_with("_link") {
        return (true, "osm:ramp");
    }
    let (Some(corridor_rank), Some(here)) = (made_of.and_then(class_rank), class_rank(near_hw))
    else {
        return (false, "osm:mainline");
    };
    if here > corridor_rank {
        return (true, "osm:off-corridor");
    }
    (false, "osm:mainline")
}

/// One baked curve in bake direction, miles from leg city a.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CurveRecord {
    pub start_mi: f64,
    pub apex_mi: f64,
    pub end_mi: f64,
    /// 'L' | 'R'
    pub direction: char,
    pub advisory_mph: i64,
    pub min_radius_ft: i64,
    pub deflection_deg: f64,
    pub connector: bool,
}

/// A curve mapped onto route miles, in the direction of travel.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RouteCurve {
    pub start_mi: f64,
    pub apex_mi: f64,
    pub end_mi: f64,
    /// 'L' | 'R'
    pub direction: char,
    pub advisory_mph: i64,
    pub min_radius_ft: i64,
    pub deflection_deg: f64,
    pub connector: bool,
}

impl RouteCurve {
    pub fn severity(&self) -> &'static str {
        curve_severity(self.advisory_mph, self.deflection_deg)
    }
}

/// The severity band for an advisory speed and deflection (shared by the
/// route curve and tests over plain records).
pub fn curve_severity(advisory_mph: i64, deflection_deg: f64) -> &'static str {
    if deflection_deg >= HAIRPIN_DEFLECTION_DEG && advisory_mph <= HAIRPIN_TURN_MAX_MPH {
        "hairpin"
    } else if advisory_mph <= SHARP_MAX_MPH {
        "sharp"
    } else if advisory_mph <= MODERATE_MAX_MPH {
        "moderate"
    } else {
        "gentle"
    }
}

/// The AASHTO point-mass control, which is what every state design manual
/// republishes: Rmin = V^2 / \[15(0.01*emax + fmax)\]  (FHWA-HRT-17-098, ch. 3;
/// Green Book table 3-7 supplies fmax). This is COMPUTED from the published
/// formula rather than a table typed in from memory, and the values it
/// produces are checked against the TxDOT Roadway Design Manual's own tables
/// 4-6 and 4-7 by `test_the_radius_floor_matches_published_design_tables` --
/// an earlier pass of this screen used 1,330 ft as the interstate floor, which
/// is in fact the SIXTY mph minimum, and let a design-speed step of bad
/// geometry through.
pub const AASHTO_SIDE_FRICTION: &[(i64, f64)] = &[
    (20, 0.27),
    (25, 0.23),
    (30, 0.20),
    (35, 0.18),
    (40, 0.16),
    (45, 0.15),
    (50, 0.14),
    (55, 0.13),
    (60, 0.12),
    (65, 0.11),
    (70, 0.10),
    (75, 0.09),
    (80, 0.08),
];
/// 8 percent is the most permissive superelevation any state builds to, so a
/// curve under this floor is under EVERY standard, not merely under a strict
/// one. Snow states cap at 6 percent and could justify a stricter screen; the
/// looser number is the one that keeps this a screen for the impossible
/// rather than a screen for the unusual.
pub const SUPERELEVATION_MAX: f64 = 0.08;

/// The table row nearest a design speed: `(speed, side friction)`.
///
/// Python's `min(speeds, key=lambda s: abs(s - design))` over the sorted keys,
/// which keeps the FIRST of two equally-near speeds.
fn nearest_friction(design_speed_mph: f64) -> (f64, f64) {
    let (v, friction) = AASHTO_SIDE_FRICTION
        .iter()
        .copied()
        .fold(None, |best: Option<(i64, f64)>, (s, f)| match best {
            Some((bs, _))
                if ((bs as f64) - design_speed_mph).abs()
                    <= ((s as f64) - design_speed_mph).abs() =>
            {
                best
            }
            _ => Some((s, f)),
        })
        .expect("the friction table is not empty");
    (v as f64, friction)
}

/// Tightest radius a road of this design speed may legally bend to.
pub fn min_radius_ft(design_speed_mph: f64) -> f64 {
    let (v, friction) = nearest_friction(design_speed_mph);
    (v * v) / (15.0 * (SUPERELEVATION_MAX + friction))
}

/// What the bake priced its advisory at: a comfortable lateral for a loaded
/// truck, on a FLAT road (tools/straw_curve_sample.py, A_LAT_G).
/// The fastest speed this whole model has anything to say about, and so the
/// ceiling on any advisory it produces.
///
/// The advisory is AASHTO's point-mass control solved for V, and the friction
/// table is the model: it is published for 20 through 80 mph and stops there,
/// because no US road is designed above 80 (the highest posted limit in the
/// country is the 85 on Texas SH-130). Run unclamped on a gentle bend the
/// formula happily reads out 115 mph, which is not a number about roads at
/// all -- it is arithmetic past the edge of its own table. 33 percent of the
/// baked rows were above 80 and the worst read 115.
///
/// Nothing observable moves under a truck: an advisory above the posted limit
/// never fires a pacenote, never counts as corner overspeed and never eases
/// cruise, so 115 and 80 behave identically. The point is that the stored
/// number means what it says, and a future consumer cannot read one of these
/// as a real advisory.
pub const ADVISORY_MAX_MPH: i64 = 80;

pub const ADVISORY_LATERAL_G: f64 = 0.30;

/// The most lateral acceleration any advisory is established at, in g. READ,
/// FHWA-SA-11-22, "Procedures for Setting Advisory Speeds on Curves" (2011),
/// 3.7, the accelerometer method: "A measurement of 0.26 g ... to 0.30 g ...
/// is considered an acceptable range for establishing advisory speeds." Past
/// this a bend is being taken faster than any posted advisory would ever have
/// asked, which is where the roll model starts charging a full trailer
/// (`sim::vehicle::ROLL_WARN_SHARE`).
pub const ADVISORY_ACCEPTABLE_MAX_G: f64 = 0.30;

/// The bank a mapped bend on a leg built for `design_mph` is credited with:
/// [`superelevation_at`] on roads designed at [`BANKED_DESIGN_MIN_MPH`] and
/// above, and none below it, where town streets are left unbanked -- the same
/// rule the advisory is repriced and screened with.
pub fn bend_bank(radius_ft: f64, design_mph: Option<f64>) -> f64 {
    design_mph
        .filter(|mph| *mph >= BANKED_DESIGN_MIN_MPH)
        .map(|mph| superelevation_at(radius_ft, mph))
        .unwrap_or(0.0)
}

/// The bank a road is actually BUILT with, which is not the same question as
/// the bank a road is ALLOWED. `SUPERELEVATION_MAX` above is 8 percent because
/// the screen asks "is this curve under every standard anywhere" and wants the
/// most permissive number. Crediting a curve with the full 8 would assume the
/// steepest bank any state permits on every road in the country, and reading
/// more bank than a road has reads out a higher safe speed than it has -- an
/// error in the one direction that matters.
///
/// Both manuals name the built rate and both say 6: TxDOT 4.7.3, "The
/// Department normally uses a maximum superelevation rate of 6 percent",
/// 8 only "where higher superelevation rates or sharper curves are desired"
/// and only with a District Design Engineer's sign-off. Iowa DOT 2B-2, "For
/// new construction, the superelevation rate is limited to 6%", reserving 8
/// as the state ceiling.
pub const SUPERELEVATION_BUILT: f64 = 0.06;

/// The bank a designer had to build to hold the design speed here.
///
/// Zero where friction alone carries it -- a gentle curve gets normal crown
/// -- and never above `SUPERELEVATION_BUILT`, the rate roads are built to
/// rather than the steeper one they are permitted.
///
/// DERIVED, not surveyed: nothing in the bake records a real cross-slope, so
/// this is the bank the governing equation demands, e = V^2/15R - f. AASHTO
/// Method 5, which TxDOT Tables 4-6 and 4-7 are computed with, lays MORE
/// bank than this on gentle curves and the same at the minimum radius -- so
/// this reads low where it barely matters and true where it does, and every
/// error runs toward caution.
pub fn superelevation_at(radius_ft: f64, design_speed_mph: f64) -> f64 {
    if radius_ft <= 0.0 || design_speed_mph <= 0.0 {
        return 0.0;
    }
    let (_, friction) = nearest_friction(design_speed_mph);
    let needed = design_speed_mph * design_speed_mph / (15.0 * radius_ft) - friction;
    // Deliberately not `clamp`: this mirrors Python's
    // `max(0.0, min(SUPERELEVATION_BUILT, needed))` exactly, and `clamp`
    // panics on a NaN input where the pair quietly returns a number.
    #[allow(clippy::manual_clamp)]
    needed.min(SUPERELEVATION_BUILT).max(0.0)
}

/// The advisory the bake would have given had it known about the bank.
///
/// The bake reads every curve as flat -- `sqrt(0.30 g R)` -- which throws
/// away the e in the manual's own e + f = V^2/15R and understates every
/// banked curve. On a 1,000 ft radius that is 67 mph read as safe against a
/// designed-and-banked 75, and it is why trucks braked for interstate curves
/// built to be taken at speed.
///
/// Rounded to 5 like the bake's own, so the two are directly comparable.
pub fn advisory_with_bank_mph(radius_ft: f64, design_speed_mph: f64) -> i64 {
    if radius_ft <= 0.0 {
        return 0;
    }
    let e = superelevation_at(radius_ft, design_speed_mph);
    let raw =
        crate::pyfmt::round_py_int((15.0 * radius_ft * (e + ADVISORY_LATERAL_G)).sqrt() / 5.0) * 5;
    raw.min(ADVISORY_MAX_MPH)
}

/// Which legs the screen may judge at all. It needs to know whether a tight
/// bend is the road or an artifact, and that is a question about terrain.
///
/// TWO PROXIES WERE TRIED AND BOTH FAILED, which is why this reads a bake:
///
///   1. The world's own `terrain` field. Derived from NET elevation change
///      end to end, so a road that climbs and drops all the way along without
///      getting anywhere reads as flat -- I-70 through Glenwood Canyon is
///      tagged "flat", and screening on it took 21 real curves off the canyon.
///   2. Feet of elevation range per mile. Calibrated against HPMS Terrain_Type
///      over a 92-leg sample and measured WEAK: Youden's J of 0.29, and at the
///      threshold it suggested it mislabelled 54 percent of rolling and
///      mountainous legs. A number tuned until it looked right, which is
///      exactly what AGENTS.md says not to ship.
///
/// So the terrain class is read rather than guessed: FHWA HPMS Terrain_Type,
/// baked per leg by tools/build_terrain_type.py. Only HPMS level ground is
/// screened. A leg HPMS calls rolling or mountainous keeps every curve it has,
/// and so does a leg HPMS has nothing to say about -- absence is not evidence
/// of flatness, and the safe direction here is a curve too many.
pub const HPMS_TERRAIN_LEVEL: i64 = 1;

/// True only where HPMS itself says the ground is level.
pub fn leg_is_level(leg: &Leg) -> bool {
    leg.hpms_terrain()
        .is_some_and(|terrain| terrain.terrain_type == HPMS_TERRAIN_LEVEL)
}

/// The speed this leg is built for, from its own baked limits.
///
/// The posted limit is the honest stand-in for design speed and it is real
/// data here -- OSM maxspeed swept per corridor -- so the floor follows the
/// road instead of a guess about its class. The fastest posting on the leg
/// is the one that matters: a curve has to be safe at the speed the road
/// lets a truck reach. Legs with no baked limit fall back to the class
/// default the runtime already uses elsewhere.
pub fn leg_design_speed(leg: &Leg) -> f64 {
    // A sample can carry a null mph (a posting the sweep found but could not
    // read); those say nothing about design speed and must not be compared.
    let fastest = leg
        .speed_limits()
        .iter()
        .filter_map(|s| s.mph)
        .fold(None, |best: Option<f64>, mph| {
            Some(best.map_or(mph, |b| b.max(mph)))
        });
    if let Some(limit) = fastest {
        return limit;
    }
    let highway = leg.highway.to_uppercase();
    if highway.starts_with("I-") {
        70.0
    } else if highway.starts_with("US") {
        55.0
    } else {
        45.0
    }
}

/// `"a:b"` -> radius floor, for legs flat enough to judge. Absent key
/// means "do not screen this leg's curves".
pub fn screenable_legs() -> HashMap<String, f64> {
    screenable_legs_of(get_world())
}

/// [`screenable_legs`] against a world handed in, so the baker can screen
/// against the world it just loaded instead of the process singleton.
pub fn screenable_legs_of(world: &World) -> HashMap<String, f64> {
    let mut out = HashMap::new();
    for leg in &world.legs {
        if !leg_is_level(leg) {
            continue;
        }
        let floor = min_radius_ft(leg_design_speed(leg));
        out.insert(format!("{}:{}", leg.a, leg.b), floor);
        out.insert(format!("{}:{}", leg.b, leg.a), floor);
    }
    out
}

/// Below this the road is built to normal crown and the flat reading is the
/// right one. TxDOT Table 4-3 splits exactly here: low-speed facilities (45 and
/// below) distribute superelevation by AASHTO Method 2, which "only introduces
/// superelevation after the maximum side friction has been used" and in
/// practice leaves town streets unbanked; 50 and above use Method 5 and are
/// banked as a matter of course.
pub const BANKED_DESIGN_MIN_MPH: f64 = 50.0;

/// `"a:b"` -> the speed the leg is built for, for EVERY leg.
///
/// [`screenable_legs_of`] answers the same question but only for level ground,
/// because that screen has no business judging a mountain road. The bank
/// applies everywhere, so it needs its own pass.
pub fn leg_design_speeds_of(world: &World) -> HashMap<String, f64> {
    let mut out = HashMap::new();
    for leg in &world.legs {
        let design = leg_design_speed(leg);
        out.insert(format!("{}:{}", leg.a, leg.b), design);
        out.insert(format!("{}:{}", leg.b, leg.a), design);
    }
    out
}

/// The row's advisory, corrected for the bank its road must carry.
fn banked_advisory(row: &CurveRow, design_mph: Option<f64>) -> i64 {
    let baked = row.advisory_mph;
    let Some(design_mph) = design_mph else {
        return baked;
    };
    if design_mph < BANKED_DESIGN_MIN_MPH {
        return baked;
    }
    let radius = row.min_radius_ft as f64;
    if radius <= 0.0 {
        return baked;
    }
    // Only ever upward: the bake read the road flat, and a bank can only help.
    // A curve that somehow reads slower banked than flat keeps the flat number
    // rather than being quietly slowed by a correction meant to speed it up.
    // Both sides are capped, so a gentle bend cannot be repriced past the top
    // of the friction table the repricing is built on (ADVISORY_MAX_MPH).
    baked
        .max(advisory_with_bank_mph(radius, design_mph))
        .min(ADVISORY_MAX_MPH)
}

/// The most cornering a loaded combination's tires will supply, in g.
///
/// The static rollover threshold a loaded five-axle combination is built to,
/// NHTSA DOT HS 811 734 -- the same number the lane model corners against
/// (`sim::lane::MAX_ROAD_LATERAL_G`), which is the point: a number the game
/// speaks must be one the truck it is spoken to can drive.
pub const HOLDABLE_LATERAL_G: f64 = 0.35;

/// The slowest an advisory is ever spoken as, mph.
///
/// The lowest advisory plaque the MUTCD provides (2C.08, Table 2C-5 starts at
/// 15), and the floor the curve bake already writes. A screen may step an
/// advisory down to here and no further: below it the bend is not a curve
/// warning any more, it is a corner, which `data::corners` prices.
pub const ADVISORY_MIN_MPH: i64 = 15;

/// An advisory the truck cannot actually hold, stepped down until it can.
///
/// A SELF-CONTRADICTION screen, not a taste adjustment. Every advisory is
/// priced at [`ADVISORY_LATERAL_G`] plus the bank and then rounded to the
/// nearest five, and rounding UP is what does the damage: 141 feet at 6
/// percent prices out at 27.6 and is posted 30, which asks 0.37 g of the
/// tires against the 0.35 they have. Seven bends in the shipped bake were over
/// that line, and a truck taking any of them at the number the cab had just
/// called out left its lane whatever the driver or the assists did (sweep over
/// all 25,761 signed bends, 2026-09-19).
///
/// Downward only, to the next multiple of five, and never below the 15 the
/// bake floors at: the row's geometry is not in question, only the rounding
/// applied on top of it, and every correction runs toward caution.
fn holdable_advisory(advisory_mph: i64, radius_ft: f64, bank: f64) -> i64 {
    if radius_ft <= 0.0 {
        return advisory_mph;
    }
    let mut mph = advisory_mph;
    while mph > ADVISORY_MIN_MPH {
        let demand = (mph * mph) as f64 / (15.0 * radius_ft) - bank;
        if demand <= HOLDABLE_LATERAL_G {
            break;
        }
        mph -= 5;
    }
    mph.max(ADVISORY_MIN_MPH)
}

/// A bend tighter than its own road may legally hold, on level ground.
///
/// The owner's report, 2026-08-19: "when I'm cruising down the highway or
/// turnpike in a car, it hardly ever curves." Measured, the map disagreed
/// with the road: interstate curve callouts ran 5.7 per hundred miles and
/// the median interstate curve radius sat at 1,342 ft against an 1,815 ft
/// floor for a 70 mph road.
///
/// The tell is the record disagreeing with ITSELF, which is why this needs
/// both halves -- a radius under the floor AND ground HPMS calls level.
/// Rough country earns a tight bend and keeps it.
fn is_flat_ground_artifact(row: &CurveRow, floor: f64) -> bool {
    (row.min_radius_ft as f64) < floor
}

/// `"a:b"` keys, both directions, for interstate-class legs.
fn interstate_leg_keys(world: &World) -> HashSet<String> {
    let mut keys = HashSet::new();
    for leg in &world.legs {
        if leg.highway.to_uppercase().starts_with("I-") {
            keys.insert(format!("{}:{}", leg.a, leg.b));
            keys.insert(format!("{}:{}", leg.b, leg.a));
        }
    }
    keys
}

/// True for a record no interstate mainline could physically hold.
fn is_interstate_artifact(row: &CurveRow) -> bool {
    row.min_radius_ft < INTERSTATE_MIN_RADIUS_FT
        || row.deflection_deg >= INTERSTATE_MAX_DEFLECTION_DEG
}

/// Recorded span as a multiple of the arc this bend's own geometry needs.
///
/// 1.0 means the span exactly holds the curve; the median mainline record is
/// 1.22. Below 1 the record is describing a turn sharper than the road it
/// claims to occupy. Returns a large number when radius or deflection is
/// missing, so an incomplete row is never screened out on arithmetic it
/// never supplied.
pub fn arc_consistency(record: &CurveRecord) -> f64 {
    let need = (record.min_radius_ft as f64) * record.deflection_deg.to_radians();
    if need <= 0.0 {
        return CONSISTENT_ENOUGH;
    }
    let span_ft = (record.end_mi - record.start_mi) * 5280.0;
    span_ft / need
}

fn by_position(a: &CurveRecord, b: &CurveRecord) -> std::cmp::Ordering {
    (a.start_mi, a.apex_mi)
        .partial_cmp(&(b.start_mi, b.apex_mi))
        .unwrap_or(std::cmp::Ordering::Equal)
}

/// Drop mainline rows that contradict their own geometry.
///
/// Two rules, both blind to road class and terrain -- see
/// `ARC_CONSISTENCY_MIN` and `ZIGZAG_MAX_TANGENT_FT`. Connectors are
/// exempt, matching the screens above: a ramp really does bend that hard,
/// and its arcs are recorded against a different baseline.
///
/// Together these drop about 2.3% of surviving mainline rows.
pub fn screen_geometry(records: &[CurveRecord]) -> Vec<CurveRecord> {
    let mut mainline: Vec<CurveRecord> = records.iter().filter(|r| !r.connector).copied().collect();
    mainline.sort_by(by_position);
    let mut doomed: HashSet<usize> = HashSet::new();
    for (i, record) in mainline.iter().enumerate() {
        if arc_consistency(record) < ARC_CONSISTENCY_MIN {
            doomed.insert(i);
        }
    }
    for (i, pair) in mainline.windows(2).enumerate() {
        let (left, right) = (&pair[0], &pair[1]);
        if left.direction == right.direction {
            continue;
        }
        if (right.start_mi - left.end_mi) * 5280.0 > ZIGZAG_MAX_TANGENT_FT {
            continue;
        }
        if left.min_radius_ft.min(right.min_radius_ft) > ZIGZAG_MAX_RADIUS_FT {
            continue;
        }
        if arc_consistency(left).min(arc_consistency(right)) >= ZIGZAG_ARC_MAX {
            continue;
        }
        doomed.insert(i);
        doomed.insert(i + 1);
    }
    let mut kept: Vec<CurveRecord> = mainline
        .iter()
        .enumerate()
        .filter(|(i, _)| !doomed.contains(i))
        .map(|(_, r)| *r)
        .collect();
    kept.extend(records.iter().filter(|r| r.connector).copied());
    kept.sort_by(by_position);
    kept
}

/// One line of `curves.jsonl` (the `meta` line is skipped before this).
#[derive(Deserialize)]
struct CurveRow {
    leg: String,
    #[serde(default)]
    seq: Option<i64>,
    start_mi: f64,
    apex_mi: f64,
    end_mi: f64,
    direction: String,
    advisory_mph: i64,
    min_radius_ft: i64,
    deflection_deg: f64,
    #[serde(default)]
    connector: bool,
}

#[derive(Deserialize)]
struct ArtifactRow {
    leg: String,
    seq: i64,
}

fn is_meta_line(line: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(line)
        .ok()
        .is_some_and(|v| v.get("meta").is_some())
}

/// `(leg, seq)` pairs the US/state-route artifact screen flagged.
///
/// Baked offline by `tools/screen_curve_artifacts.py` -- see that tool's
/// docstring for the three discriminators (flat local ground under a
/// hairpin-severity curve, city-departure geometry at a leg's ends off the
/// mountains, and a radius no through highway of any class can bend to).
/// Missing file reads as "nothing flagged" rather than an error, the same
/// fail-open the interstate screen takes when curves.jsonl itself is absent.
fn flagged_artifact_keys_from(text: &str) -> HashSet<(String, i64)> {
    let mut keys = HashSet::new();
    for line in text.lines() {
        if line.trim().is_empty() || is_meta_line(line) {
            continue;
        }
        let row: ArtifactRow = serde_json::from_str(line).expect("curve_artifacts.jsonl row");
        keys.insert((row.leg, row.seq));
    }
    keys
}

static CACHE: OnceCell<HashMap<String, Vec<CurveRecord>>> = OnceCell::new();

/// The screened, per-leg curve table, built once per process (Python's
/// module `_CACHE`). Public so sweep tests can walk every leg.
pub fn load() -> &'static HashMap<String, Vec<CurveRecord>> {
    CACHE.get_or_init(|| {
        // A baked build already holds the screened table: the screens ran at
        // bake time, against this same world, through the code below.
        if let Some(container) = baked() {
            return container.curves().expect("baked curve table decodes");
        }
        let curve_text = read_data_text("world_data/us/gameplay/curves.jsonl");
        let artifact_text =
            read_data_text("world_data/us/gameplay/curve_artifacts.jsonl").unwrap_or_default();
        build_from_sources(
            curve_text.as_deref().unwrap_or(""),
            &artifact_text,
            get_world(),
        )
    })
}

/// The screened per-leg curve table built from the two JSONL texts and the
/// world they were baked against.
///
/// This is the body of [`load`], lifted out so the baker can call it with the
/// world it loaded and the files it read. One implementation of the four
/// screens, so the container and the JSON tree cannot screen differently.
pub fn build_from_sources(
    curves_jsonl: &str,
    artifacts_jsonl: &str,
    world: &World,
) -> HashMap<String, Vec<CurveRecord>> {
    {
        let mut by_leg: HashMap<String, Vec<CurveRecord>> = HashMap::new();
        {
            let text = curves_jsonl;
            let interstate_legs = interstate_leg_keys(world);
            let flagged_artifacts = flagged_artifact_keys_from(artifacts_jsonl);
            let screenable = screenable_legs_of(world);
            let design_speeds = leg_design_speeds_of(world);
            for line in text.lines() {
                if line.trim().is_empty() || is_meta_line(line) {
                    continue;
                }
                let row: CurveRow = serde_json::from_str(line).expect("curves.jsonl row");
                let connector = row.connector;
                // Screen sweep artifacts off interstate mainline before they
                // can reach any consumer: a bogus hairpin call, a physics
                // shove, and a time-decompression stall all read the same
                // records. Ramps are exempt -- they really are that sharp.
                if !connector && interstate_legs.contains(&row.leg) && is_interstate_artifact(&row)
                {
                    continue;
                }
                // Second screen: US/state-route mainline records an offline
                // terrain check flagged as sitting on flat ground (see
                // flagged_artifact_keys). Keyed by (leg, seq) rather than a
                // runtime rule, because the discriminator needs the dense
                // elevation archive this loader has no reason to carry.
                if !connector
                    && row
                        .seq
                        .is_some_and(|seq| flagged_artifacts.contains(&(row.leg.clone(), seq)))
                {
                    continue;
                }
                // Fourth screen: any class, any route -- a bend tighter than
                // the road's own design speed allows, on ground with no
                // relief to justify it. See is_flat_ground_artifact.
                let floor = screenable.get(&row.leg).copied();
                if !connector && floor.is_some_and(|floor| is_flat_ground_artifact(&row, floor)) {
                    continue;
                }
                // Connector arcs (interchange ramps) stay in the data with
                // their flag: curve physics wants them, spoken layers skip
                // them -- ramps carry their own speech.
                let design_mph = design_speeds.get(&row.leg).copied();
                let advisory_mph = banked_advisory(&row, design_mph);
                // Last, because it judges the number the other corrections
                // produced: an advisory the truck cannot hold is a row that
                // disagrees with itself, whatever priced it.
                let bank = bend_bank(row.min_radius_ft as f64, design_mph);
                let advisory_mph = holdable_advisory(advisory_mph, row.min_radius_ft as f64, bank);
                by_leg
                    .entry(row.leg.clone())
                    .or_default()
                    .push(CurveRecord {
                        start_mi: row.start_mi,
                        apex_mi: row.apex_mi,
                        end_mi: row.end_mi,
                        direction: row.direction.chars().next().unwrap_or('L'),
                        advisory_mph,
                        min_radius_ft: row.min_radius_ft,
                        deflection_deg: row.deflection_deg,
                        connector,
                    });
            }
        }
        // Third screen, needing neighbours and so running per leg once the rows
        // are gathered rather than line by line above.
        by_leg
            .into_iter()
            .map(|(key, rows)| (key, screen_geometry(&rows)))
            .collect()
    }
}

/// Baked curves for `"a_slug:b_slug"`, bake direction. Connector arcs are
/// excluded unless asked for.
pub fn leg_curves(leg_key: &str, mainline_only: bool) -> Vec<CurveRecord> {
    let rows = load().get(leg_key).map(Vec::as_slice).unwrap_or(&[]);
    if mainline_only {
        return rows.iter().filter(|r| !r.connector).copied().collect();
    }
    rows.to_vec()
}

fn mirror(direction: char) -> char {
    match direction {
        'L' => 'R',
        'R' => 'L',
        other => other,
    }
}

/// Every curve on the route, in travel order and direction.
///
/// `cities` is the route's city sequence; each leg is mirrored when the
/// route runs it b-to-a (offsets flip across the leg, left becomes right).
/// Pass `mainline_only=false` to keep connector arcs for physics.
pub fn route_curves(route: &Route, cities: &[String], mainline_only: bool) -> Vec<RouteCurve> {
    let mut out: Vec<RouteCurve> = Vec::new();
    let mut leg_start = 0.0;
    for (from_city, leg) in cities.iter().zip(route.legs.iter()) {
        let forward = *from_city == leg.a;
        for rec in leg_curves(&format!("{}:{}", leg.a, leg.b), mainline_only) {
            let (start, apex, end, direction) = if forward {
                (rec.start_mi, rec.apex_mi, rec.end_mi, rec.direction)
            } else {
                (
                    leg.miles - rec.end_mi,
                    leg.miles - rec.apex_mi,
                    leg.miles - rec.start_mi,
                    mirror(rec.direction),
                )
            };
            out.push(RouteCurve {
                start_mi: leg_start + start,
                apex_mi: leg_start + apex,
                end_mi: leg_start + end,
                direction,
                advisory_mph: rec.advisory_mph,
                min_radius_ft: rec.min_radius_ft,
                deflection_deg: rec.deflection_deg,
                connector: rec.connector,
            });
        }
        leg_start += leg.miles;
    }
    out.sort_by(|a, b| {
        a.start_mi
            .partial_cmp(&b.start_mi)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    out
}

#[cfg(test)]
mod tests {
    //! The pure parts of `tests/test_curve_management.py` (the geometry
    //! screen and the radius floor); the world-backed sweeps live in
    //! `crates/ff-core/tests/data_curve_management.rs`.
    use super::*;

    fn rec(
        start_mi: f64,
        end_mi: f64,
        direction: char,
        radius_ft: i64,
        deflection_deg: f64,
        advisory: i64,
        connector: bool,
    ) -> CurveRecord {
        CurveRecord {
            start_mi,
            apex_mi: (start_mi + end_mi) / 2.0,
            end_mi,
            direction,
            advisory_mph: advisory,
            min_radius_ft: radius_ft,
            deflection_deg,
            connector,
        }
    }

    #[test]
    fn test_a_curve_too_short_to_hold_its_own_arc_is_dropped() {
        // Owner playtest, 2026-08-17: "is a hairpin supposed to be on this
        // route?" on US-285 north of Santa Fe.
        //
        // A 160 ft radius turning 79.9 degrees needs 223 ft of road; that record
        // claimed 53. Both existing screens correctly passed it -- it sits in real
        // mountain terrain, where real switchbacks live -- so the pacenote layer
        // called a 25 mph hairpin on a road posted 35. This screen asks a question
        // terrain cannot excuse: does the record agree with itself?

        // 500 ft radius, 30 degrees: needs 262 ft of arc.
        let honest = rec(1.00, 1.10, 'L', 500, 30.0, 25, false); // 528 ft of span, comfortable
        assert!(arc_consistency(&honest) > 1.0);
        // The same bend claiming to happen in 11 ft.
        let impossible = rec(2.00, 2.002, 'L', 500, 30.0, 25, false);
        assert!(arc_consistency(&impossible) < 0.1);

        let kept = screen_geometry(&[honest, impossible]);
        assert!(kept.contains(&honest));
        assert!(!kept.contains(&impossible));
    }

    #[test]
    fn test_a_zero_tangent_reversal_takes_both_sides_with_it() {
        // A digitized kink reads as opposite-direction curves meeting at a
        // point. A through highway cannot swap lock with no straight between, so
        // the whole wiggle goes -- including the side whose own arithmetic happens
        // to be fine, because the road does not really go left-right there.

        // Left is arithmetically fine on its own; right contradicts itself.
        let left = rec(10.00, 10.05, 'L', 160, 35.4, 25, false);
        let right = rec(10.05, 10.06, 'R', 160, 79.9, 25, false);
        let far = rec(12.00, 12.20, 'R', 900, 20.0, 45, false);

        let kept = screen_geometry(&[left, right, far]);
        assert!(
            !kept.contains(&left),
            "the fine-looking half of a zig-zag stays a zig-zag"
        );
        assert!(!kept.contains(&right));
        assert!(
            kept.contains(&far),
            "an unrelated curve down the road is untouched"
        );
    }

    #[test]
    fn test_a_real_switchback_pair_with_road_between_them_survives() {
        // US-550 over Red Mountain Pass really does reverse. The screen must
        // only take reversals with NO tangent -- otherwise it eats the mountain
        // roads the previous two screens were careful to leave alone.

        // Two tight, honest bends with a tenth of a mile of straight between.
        let first = rec(5.00, 5.10, 'L', 200, 90.0, 25, false);
        let second = rec(5.20, 5.30, 'R', 200, 90.0, 25, false);
        let kept = screen_geometry(&[first, second]);
        assert!(kept.contains(&first) && kept.contains(&second));
    }

    #[test]
    fn test_ramps_are_exempt_like_the_other_screens() {
        // Connectors really are that sharp and are recorded against a different
        // baseline; the screens above exempt them and so does this one.
        let ramp = rec(1.00, 1.001, 'L', 500, 30.0, 25, true);
        let kept = screen_geometry(&[ramp]);
        assert!(kept.contains(&ramp));
    }

    #[test]
    fn test_a_record_missing_its_geometry_is_never_screened_on_arithmetic() {
        let blank = rec(1.00, 1.001, 'L', 0, 0.0, 25, false);
        assert!(arc_consistency(&blank) > 1.0);
        assert!(screen_geometry(&[blank]).contains(&blank));
    }

    #[test]
    fn test_the_radius_floor_matches_published_design_tables() {
        // The floor is computed, so it has to be checked against print.
        //
        // `min_radius_ft` evaluates the AASHTO point-mass control
        // (Rmin = V^2 / [15(0.01*emax + fmax)], FHWA-HRT-17-098 chapter 3) using
        // the Green Book table 3-7 side-friction factors. These expected values
        // are the TxDOT Roadway Design Manual's own tables 4-6 and 4-7, which
        // republish the Green Book controls -- so if either the formula or a
        // friction factor is mistyped, this fails against a published source
        // rather than against my arithmetic.
        //
        // An earlier pass of this screen used 1,330 ft as the interstate floor.
        // That is the SIXTY mph minimum at 6 percent superelevation; a 70 mph road
        // needs 1,815 ft at 8 percent and 2,040 at 6. The screen was letting a
        // whole design-speed step of bad geometry through.
        assert_eq!(SUPERELEVATION_MAX, 0.08); // the most permissive standard built
        for (speed, expected) in [
            (40, 444.0),
            (50, 758.0),
            (60, 1200.0),
            (70, 1810.0),
            (75, 2210.0),
        ] {
            let computed = min_radius_ft(speed as f64);
            assert!(
                (computed - expected).abs() <= 5.0,
                "{speed} mph: computed {computed:.0} ft, TxDOT table 4-7 says {expected} ft"
            );
        }
        // And it really is a floor that rises with speed.
        assert!(min_radius_ft(50.0) < min_radius_ft(60.0));
        assert!(min_radius_ft(60.0) < min_radius_ft(70.0));
    }

    #[test]
    fn test_a_hairpin_is_a_shape_not_a_speed() {
        // MUTCD gives the Hairpin Curve sign (W1-11) for a change in horizontal
        // alignment of 135 degrees or more -- a switchback, where the road comes
        // back on itself. Advisory speed does not enter into it; MUTCD sorts by
        // advisory separately and much lower, swapping the Turn sign (W1-1) for
        // the Curve sign at 30 mph or less.
        //
        // The old rule said "advisory <= 25 OR deflection >= 150", and the
        // advisory half called tight little bends taken slowly hairpins -- the
        // worst of them deflecting ten degrees. That spends the word on nothing
        // and leaves it meaning less when a real switchback turns up.
        let curve = |advisory: i64, deflection: f64| RouteCurve {
            start_mi: 1.0,
            apex_mi: 1.05,
            end_mi: 1.1,
            direction: 'L',
            advisory_mph: advisory,
            min_radius_ft: 150,
            deflection_deg: deflection,
            connector: false,
        };

        // MUTCD's own numbers, not rounder ones chosen nearby.
        assert_eq!(HAIRPIN_DEFLECTION_DEG, 135.0);
        assert_eq!(HAIRPIN_TURN_MAX_MPH, 30);

        // A switchback comes back on itself AND has to be crawled...
        assert_eq!(curve(25, 170.0).severity(), "hairpin");
        assert_eq!(curve(30, 135.0).severity(), "hairpin");
        // ...and a slow little kink is not one, however slowly it is taken.
        assert_ne!(curve(25, 10.6).severity(), "hairpin");
        assert_ne!(curve(20, 94.0).severity(), "hairpin");

        // Nor is a sweeper, however far round it goes. The angle is necessary
        // and not sufficient: MUTCD puts the hairpin sign up instead of a TURN
        // sign, and a 60 mph bend was never getting one. This is I-49 north of
        // Fayetteville, 811 ft radius through 143 degrees -- real road, and the
        // reason taking the angle alone put hairpins on seven interstate curves.
        assert_ne!(curve(60, 142.9).severity(), "hairpin");

        // Darren's Norwich corner: real, tight, and correctly no longer a
        // switchback (NY-12, 2026-08-23).
        assert_eq!(curve(25, 94.0).severity(), "sharp");
    }

    #[test]
    fn test_the_friction_table_is_aashto_table_3_7() {
        // Pinned against the published source, not against itself.
        //
        // Iowa DOT Design Manual 2B-2 Table 2B.1 reproduces these and names its
        // origin: "Adapted from Table 3-7: AASHTO Greenbook, 7th edition, 2018".
        // TxDOT Table 4-7 agrees independently -- back-solve its minimum radii
        // through the manual's own e + f = V^2/15R and these are the f values
        // that fall out. Three sources, one table.
        let published: &[(i64, f64)] = &[
            (15, 0.32),
            (20, 0.27),
            (25, 0.23),
            (30, 0.20),
            (35, 0.18),
            (40, 0.16),
            (45, 0.15),
            (50, 0.14),
            (55, 0.13),
            (60, 0.12),
            (65, 0.11),
            (70, 0.10),
            (75, 0.09),
            (80, 0.08),
        ];
        // Every value we carry must be the published one. The table starts at 20
        // rather than 15 because nothing in the game is designed for 15 mph, and
        // a row nothing reads is a row nobody maintains.
        for (speed, factor) in AASHTO_SIDE_FRICTION {
            let want = published
                .iter()
                .find(|(s, _)| s == speed)
                .unwrap_or_else(|| panic!("{speed} mph is not in the published table"))
                .1;
            assert_eq!(want, *factor, "{speed} mph");
        }
        for speed in (20..85).step_by(5) {
            assert!(
                AASHTO_SIDE_FRICTION.iter().any(|(s, _)| *s == speed),
                "{speed} mph is missing from the table"
            );
        }
    }

    #[test]
    fn test_minimum_radii_match_the_published_table() {
        // `min_radius_ft` against TxDOT Roadway Design Manual Table 4-7
        // (Minimum Radii and Superelevation Rates, emax = 8%), e = 8.0% row.
        //
        // The derivation and the table are independent -- one is arithmetic from
        // AASHTO's friction factors, the other is printed in a state design
        // manual -- so agreeing to rounding is a real check rather than a
        // tautology.
        for (speed, table_ft) in [
            (50.0, 758.0),
            (55.0, 960.0),
            (60.0, 1200.0),
            (65.0, 1480.0),
            (70.0, 1810.0),
            (75.0, 2210.0),
            (80.0, 2670.0),
        ] {
            let derived = min_radius_ft(speed);
            assert!(
                (derived - table_ft).abs() / table_ft < 0.01,
                "{speed} mph: derived {derived:.0} ft vs published {table_ft} ft"
            );
        }
    }

    #[test]
    fn test_the_bank_is_the_rate_roads_are_built_to_not_the_rate_allowed() {
        // 8 percent is the ceiling; 6 percent is what gets built.
        //
        // Crediting every curve with the steepest bank any state permits would
        // read out a higher safe speed than the road has, which is the one
        // direction an error must not run. TxDOT 4.7.3 normally builds 6 and
        // needs a District Design Engineer to go to 8; Iowa DOT 2B-2 limits new
        // construction to 6 and keeps 8 as the state ceiling.
        assert_eq!(SUPERELEVATION_BUILT, 0.06);
        assert_eq!(
            SUPERELEVATION_MAX, 0.08,
            "the SCREEN still asks the permissive question"
        );

        // Never steeper than what is built, however tight the curve.
        for radius in [200.0, 500.0, 758.0, 1200.0] {
            assert!(superelevation_at(radius, 70.0) <= SUPERELEVATION_BUILT);
        }
        // A gentle curve is not banked at all -- normal crown, as built.
        assert_eq!(superelevation_at(20_000.0, 70.0), 0.0);
        // And the bank grows as the curve tightens, never the other way.
        let banks: Vec<f64> = [12_000.0, 6_000.0, 3_000.0, 1_500.0, 800.0]
            .into_iter()
            .map(|r| superelevation_at(r, 70.0))
            .collect();
        let mut sorted = banks.clone();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
        assert_eq!(banks, sorted);
    }

    #[test]
    fn test_counting_the_bank_only_ever_raises_an_advisory() {
        // The correction exists because the bake read every road as flat.
        //
        // A bank can only help a truck hold a curve, so this must never hand
        // back a number lower than the flat one -- a correction meant to speed a
        // curve up must not quietly slow a different one down.
        //
        // Inside the model's own domain, which is what `ADVISORY_MAX_MPH`
        // marks: the friction table this is solved from is published for 20 to
        // 80 mph and stops there, so an advisory is only a statement about a
        // road up to 80. A 20,000 ft bend reads 300 mph unclamped, which is
        // arithmetic rather than a road, and both the flat and the banked
        // number are capped alike -- so the "never lower" property is checked
        // where both are real.
        for radius in [400.0, 758.0, 1000.0, 1810.0, 5000.0, 20_000.0] {
            let flat = (15.0 * radius * ADVISORY_LATERAL_G).sqrt();
            let capped = (crate::pyfmt::round_py_int(flat / 5.0) * 5).min(ADVISORY_MAX_MPH);
            assert!(advisory_with_bank_mph(radius, 70.0) >= capped - 1);
        }

        // The cap binds, and it is the top of the friction table rather than a
        // number anybody picked: no US road is designed above 80.
        assert_eq!(ADVISORY_MAX_MPH, 80);
        assert_eq!(advisory_with_bank_mph(20_000.0, 70.0), ADVISORY_MAX_MPH);
        assert!(
            advisory_with_bank_mph(600.0, 70.0) < ADVISORY_MAX_MPH,
            "a real bend must still read its own speed, not the cap"
        );

        // A town street is built to normal crown, so it keeps the flat reading:
        // TxDOT Table 4-3 and Iowa DOT 2B-1 both put the Method 2 / Method 5
        // line at 45 mph.
        assert_eq!(BANKED_DESIGN_MIN_MPH, 50.0);
    }

    #[test]
    fn test_the_design_floor_is_spent_where_the_ground_cannot_excuse_a_bend() {
        // The design minimum is real, and applying it flat to interstates is not.
        //
        // TxDOT Table 4-7 puts a 50 mph design speed at 758 ft and 50 is the
        // lowest the Interstate system designs to, so a tighter interstate curve
        // is below every standard. Screening on that number directly deletes
        // I-70 through Glenwood Canyon, which really is tighter than standard
        // under design exceptions -- a floor sized to the design minimum cannot
        // tell an exception from an artifact (tried and reverted, 2026-08-23).
        //
        // So the flat interstate screen stays deliberately loose, and the design
        // floor is spent by the terrain-gated screen instead, which only judges
        // ground with no relief to justify the bend.
        assert_eq!(INTERSTATE_MIN_RADIUS_FT, 300);
        assert!(
            min_radius_ft(50.0) > INTERSTATE_MIN_RADIUS_FT as f64,
            "the blunt screen must stay below the design floor, or it eats real road"
        );
    }

    #[test]
    fn test_absence_of_a_terrain_class_is_never_read_as_level() {
        // The safe direction, and the reason this reads a bake rather than a rule.
        //
        // Screening treats level ground as license to delete geometry, so a leg
        // HPMS never classified must keep every curve it has. Absence of evidence
        // is not evidence of flatness -- getting this backwards would quietly
        // strip curves from exactly the legs we know least about.
        use crate::data::world_models::{CorridorDetail, HpmsTerrain};
        let with = |terrain_type: Option<i64>| {
            Leg::new("a", "b", 10.0, "US-1", "flat", Vec::new()).with_detail(CorridorDetail {
                hpms_terrain: terrain_type.map(|t| HpmsTerrain {
                    terrain_type: t,
                    ..Default::default()
                }),
                ..Default::default()
            })
        };
        assert!(!leg_is_level(&with(None)));
        assert!(!leg_is_level(&with(Some(2))));
        assert!(!leg_is_level(&with(Some(3))));
        assert!(leg_is_level(&with(Some(1))));
    }
}
