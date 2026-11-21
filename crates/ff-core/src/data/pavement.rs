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
//! `inventory_side` where they apply; a structure reads `"from_mi to_mi
//! kind"`, the way, then `hpms=` (HPMS's own length of it).
//!
//! Screened on the way in, never edited in the bake (see `CLAUDE.md`,
//! "Provenance"): a run that ends before it starts or runs off the leg, a
//! structure longer than any of its kind on the map's roads, and a stretch
//! where the surface changes class every few hundred feet for miles, which
//! is two carriageways' readings interleaved rather than a road. What was
//! dropped is counted in [`Pavement::screened`]. A structure whose derived
//! span disagrees with HPMS's own length of it is kept and flagged
//! ([`Structure::span_disagrees`]), since either could be the wrong one.

use serde_json::{Map, Value};

use crate::data::world_models::DataError;
use crate::data::world_parsing::{get_float, get_str};
use crate::pyfmt::py_str_float;

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
        let Some(value) = self
            .tags
            .iter()
            .find_map(|t| t.strip_prefix(key)?.strip_prefix('='))
        else {
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
        hpms_year: meta
            .and_then(|m| m.get("hpms_year"))
            .and_then(Value::as_i64),
        source: meta.map(|m| get_str(m, "source")).unwrap_or_default(),
        screened,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn parse(raw: Value, miles: f64) -> Pavement {
        parse_pavement(Some(&raw), miles, "a", "b")
            .unwrap()
            .unwrap()
    }

    #[test]
    fn a_run_resolves_by_direction() {
        let p = parse(
            json!({"surface": [
                "0 2 asphalt",
                "2 3 jointed_plain_concrete forward iri=80 faulting=0.1",
                "2 3 asphalt_over_jointed_concrete reverse inventory_side",
            ], "meta": {"read_share_forward": 0.3, "read_share_reverse": 0.3}}),
            10.0,
        );
        assert_eq!(p.surface_at(1.0, false).unwrap().surface, Surface::Asphalt);
        assert_eq!(p.surface_at(1.0, true).unwrap().ways, Ways::Both);
        let fwd = p.surface_at(2.5, true).unwrap();
        assert_eq!(fwd.surface.group(), SurfaceGroup::JointedConcrete);
        assert_eq!((fwd.iri, fwd.faulting), (Some(80.0), Some(0.1)));
        assert!(!fwd.inventory_side);
        let rev = p.surface_at(2.5, false).unwrap();
        assert!(rev.surface.over_jointed_concrete() && rev.inventory_side);
        assert!(p.surface_at(5.0, true).is_none(), "nothing is filled in");
    }

    #[test]
    fn a_twin_bridge_tail_joins_its_bridge_one_way_only() {
        let p = parse(
            json!({"structures": [
                "1 1.1 bridge hpms=0.1",
                "1.1 1.12 bridge forward",
                "4 4.5 tunnel",
            ]}),
            10.0,
        );
        assert_eq!(
            p.structures_for(true),
            vec![
                (1.0, 1.12, StructureKind::Bridge),
                (4.0, 4.5, StructureKind::Tunnel)
            ]
        );
        assert_eq!(
            p.structures_for(false)[0],
            (1.0, 1.1, StructureKind::Bridge)
        );
        assert_eq!(p.structure_at(1.11, true), Some(StructureKind::Bridge));
        assert_eq!(p.structure_at(1.11, false), None);
    }

    #[test]
    fn implausible_records_are_screened_not_kept() {
        let p = parse(
            json!({"surface": ["3 2 asphalt", "0 12 asphalt"],
                   "structures": ["1 5 tunnel", "1 1.2 bridge"]}),
            10.0,
        );
        assert!(p.surface.is_empty());
        assert_eq!(p.structures.len(), 1);
        assert_eq!(p.screened, 3);
    }

    #[test]
    fn a_span_off_its_hpms_length_is_flagged_and_kept() {
        // Tuscarora as the dry run placed it: 1.18 leg miles against HPMS's
        // 1.148, on a leg 5 percent longer than its measured road.
        let p = parse(
            json!({"structures": [
                "58.615 59.795 tunnel hpms=1.148",
                "1 1.105 bridge hpms=0.1",
                "2 2.5 bridge",
            ], "meta": {"mile_scale": 1.05}}),
            100.0,
        );
        let flags: Vec<bool> = p.structures.iter().map(|s| s.span_disagrees).collect();
        assert_eq!(flags, vec![false, false, false]);
        let p = parse(
            json!({"structures": ["58.615 59.795 tunnel hpms=0.99"]}),
            100.0,
        );
        assert!(p.structures[0].span_disagrees);
        assert_eq!(p.structures.len(), 1, "flagged, not dropped");
    }

    #[test]
    fn interleaved_carriageways_are_screened() {
        let mut runs: Vec<Value> = (0..30)
            .map(|i| {
                let class = if i % 2 == 0 {
                    "asphalt"
                } else {
                    "jointed_plain_concrete"
                };
                json!(format!(
                    "{} {} {class}",
                    i as f64 * 0.1,
                    (i + 1) as f64 * 0.1
                ))
            })
            .collect();
        runs.push(json!("3 8 asphalt"));
        // One lone tenth between long runs is a real reading and stays.
        runs.push(json!("8 8.1 continuous_concrete"));
        runs.push(json!("8.1 9 asphalt"));
        let p = parse(json!({ "surface": runs }), 10.0);
        assert_eq!(p.screened, 30);
        assert_eq!(p.surface.len(), 3);
    }

    #[test]
    fn a_malformed_record_fails_the_load() {
        for raw in [
            json!({"surface": ["0 1 gravel"]}),
            json!({"surface": ["0 asphalt"]}),
            json!({"surface": ["0 1 asphalt iri=rough"]}),
            json!({"structures": ["0 1 ramp"]}),
            json!({"meta": {"mile_scale": 7.0}}),
            json!({"meta": {"read_share_forward": 1.5}}),
        ] {
            assert!(parse_pavement(Some(&raw), 10.0, "a", "b").is_err(), "{raw}");
        }
        assert!(parse_pavement(None, 10.0, "a", "b").unwrap().is_none());
    }
}
