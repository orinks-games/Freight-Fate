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

#[test]
fn a_deck_reading_covers_its_joined_structure_one_way() {
    let p = parse(
        json!({"structures": [
            "1 1.2 bridge deck=concrete nbi=12-000720001",
            "1.2 1.25 bridge forward",
            "3 3.4 bridge deck=open_grating nbi=12-000720002 grate=3.15:3.25",
            "5 5.3 bridge deck=open_grating",
            "7 7.2 bridge",
        ]}),
        10.0,
    );
    assert_eq!(p.deck_at(1.1, true), Some(Deck::Concrete));
    assert_eq!(p.deck_at(1.22, true), Some(Deck::Concrete), "the tail");
    assert_eq!(p.deck_at(1.22, false), None);
    assert_eq!(p.structures[0].nbi.as_deref(), Some("12-000720001"));
    assert_eq!(p.deck_at(7.1, true), None, "no record matched");
    assert_eq!(p.deck_at(2.0, true), None, "off any structure");
    assert!(!p.on_grating(3.1, true), "the fixed approach spans");
    assert!(p.on_grating(3.2, false), "the movable span");
    assert!(p.on_grating(5.01, true), "no span: the whole structure");
    assert!(!p.on_grating(1.1, true));
}

#[test]
fn contradictory_deck_and_year_readings_are_dropped_not_the_record() {
    let p = parse(
        json!({"surface": [
            "0 1 jointed_plain_concrete built=1850 improved=1990",
            "1 2 continuous_concrete built=2031",
        ], "structures": [
            "3 3.4 bridge deck=open_grating grate=3.3:3.6",
            "4 4.2 bridge deck=open_grating grate=4.15:4.05",
            "5 5.5 tunnel deck=concrete",
        ], "meta": {"hpms_year": 2023}}),
        10.0,
    );
    assert_eq!(p.surface.len(), 2);
    assert_eq!(
        (p.surface[0].built, p.surface[0].improved),
        (None, Some(1990))
    );
    assert_eq!(p.surface[1].built, None);
    assert_eq!(p.structures.len(), 3);
    assert!(p
        .structures
        .iter()
        .all(|s| s.deck.is_none() && s.grate.is_none()));
    assert_eq!(p.screened, 5);
}

#[test]
fn a_malformed_deck_reading_fails_the_load() {
    for raw in [
        json!({"structures": ["0 1 bridge deck=steel"]}),
        json!({"structures": ["0 1 bridge deck=open_grating grate=0.5"]}),
        json!({"surface": ["0 1 jointed_plain_concrete built=old"]}),
    ] {
        assert!(parse_pavement(Some(&raw), 10.0, "a", "b").is_err(), "{raw}");
    }
}

fn run(surface: Surface, built: Option<i64>, improved: Option<i64>) -> SurfaceRun {
    SurfaceRun {
        from_mi: 0.0,
        to_mi: 1.0,
        surface,
        ways: Ways::Both,
        iri: None,
        faulting: None,
        inventory_side: false,
        built,
        improved,
    }
}

/// A deck reading, the run, the state, the answer, and why.
type Case = (
    Option<(Deck, f64)>,
    Option<SurfaceRun>,
    &'static str,
    Option<Grooves>,
    &'static str,
);

#[test]
fn the_groove_rule_table() {
    use Grooves::{AssumedDeck, DerivedFromBuildYear};
    let jpc = Surface::JointedPlainConcrete;
    let derived = Some(DerivedFromBuildYear);
    let cases: &[Case] = &[
        (
            None,
            Some(run(jpc, Some(1979), None)),
            "Ohio",
            None,
            "before the 1979 advisory",
        ),
        (
            None,
            Some(run(jpc, Some(1980), None)),
            "Ohio",
            derived,
            "first year",
        ),
        (
            None,
            Some(run(jpc, Some(2005), None)),
            "Ohio",
            derived,
            "last year",
        ),
        (
            None,
            Some(run(jpc, Some(2006), None)),
            "Ohio",
            None,
            "after the 2005 advisory",
        ),
        (
            None,
            Some(run(jpc, Some(1990), Some(1990))),
            "Ohio",
            derived,
            "improved the year built",
        ),
        (
            None,
            Some(run(jpc, Some(1990), Some(2012))),
            "Ohio",
            None,
            "ground since",
        ),
        (
            None,
            Some(run(jpc, None, Some(1990))),
            "Ohio",
            None,
            "no build year",
        ),
        (None, None, "Ohio", None, "no run"),
        (
            None,
            Some(run(Surface::ContinuousConcrete, Some(1995), None)),
            "Texas",
            derived,
            "continuous",
        ),
        (
            None,
            Some(run(Surface::BondedConcreteOverlay, Some(1995), None)),
            "Texas",
            derived,
            "overlay",
        ),
        (
            None,
            Some(run(Surface::AsphaltOverJointedConcrete, Some(1995), None)),
            "Texas",
            None,
            "asphalt on top",
        ),
        (
            None,
            Some(run(Surface::Asphalt, Some(1995), None)),
            "Texas",
            None,
            "asphalt",
        ),
        (
            None,
            Some(run(jpc, Some(1995), None)),
            "California",
            None,
            "California never tined across",
        ),
        (
            None,
            Some(run(jpc, Some(1976), None)),
            "Michigan",
            derived,
            "Michigan from 1975",
        ),
        (
            None,
            Some(run(jpc, Some(1974), None)),
            "Michigan",
            None,
            "Michigan before 1975",
        ),
        (
            None,
            Some(run(jpc, Some(1978), None)),
            "Wisconsin",
            derived,
            "Wisconsin from 1978",
        ),
        (
            None,
            Some(run(jpc, Some(1977), None)),
            "Wisconsin",
            None,
            "Wisconsin before 1978",
        ),
        (
            Some((Deck::Concrete, 0.2)),
            None,
            "California",
            Some(AssumedDeck),
            "California decks",
        ),
        (
            Some((Deck::Concrete, 0.2)),
            Some(run(jpc, Some(1995), None)),
            "Iowa",
            None,
            "Iowa decks along",
        ),
        (
            Some((Deck::Concrete, 0.05)),
            None,
            "Ohio",
            Some(AssumedDeck),
            "the bar itself",
        ),
        (
            Some((Deck::Concrete, 0.03)),
            None,
            "Ohio",
            None,
            "short, nothing around",
        ),
        (
            Some((Deck::Concrete, 0.03)),
            Some(run(jpc, Some(1995), None)),
            "Ohio",
            derived,
            "short, defers",
        ),
        (
            Some((Deck::OpenGrating, 0.3)),
            Some(run(jpc, Some(1995), None)),
            "Florida",
            None,
            "grating",
        ),
        (
            Some((Deck::Asphalt, 0.3)),
            Some(run(jpc, Some(1995), None)),
            "Ohio",
            None,
            "asphalt deck",
        ),
        (
            Some((Deck::Other, 0.3)),
            Some(run(jpc, Some(1995), None)),
            "Ohio",
            None,
            "timber deck",
        ),
    ];
    for (deck, r, state, want, why) in cases {
        assert_eq!(cross_grooved(*deck, r.as_ref(), state), *want, "{why}");
    }
}

#[test]
fn cross_grooved_at_reads_the_deck_before_the_run() {
    let p = parse(
        json!({"surface": [
            "0 10 jointed_plain_concrete built=1990",
        ], "structures": [
            "2 2.5 bridge deck=asphalt",
            "4 4.3 bridge deck=concrete",
            "6 6.02 bridge deck=concrete",
            "8 8.4 bridge",
        ]}),
        10.0,
    );
    let g = |mi: f64| p.cross_grooved_at(mi, true, "Ohio");
    assert_eq!(g(1.0), Some(Grooves::DerivedFromBuildYear));
    assert_eq!(g(2.2), None, "an asphalt deck");
    assert_eq!(g(4.1), Some(Grooves::AssumedDeck));
    assert_eq!(g(6.01), Some(Grooves::DerivedFromBuildYear), "a culvert");
    assert_eq!(
        g(8.2),
        Some(Grooves::DerivedFromBuildYear),
        "no deck reading"
    );
}
