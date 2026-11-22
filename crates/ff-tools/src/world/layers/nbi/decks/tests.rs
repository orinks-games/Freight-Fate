use super::*;

fn rec(id: &str, len_m: f64, deck_107: &str, surface_108a: &str) -> Record {
    Record {
        id: id.into(),
        length_m: Some(len_m),
        deck_107: deck_107.into(),
        surface_108a: surface_108a.into(),
        ..Default::default()
    }
}

fn on(record: usize, at_mi: f64, off_m: f64) -> OnRoad {
    OnRoad {
        record,
        at_mi,
        off_m,
    }
}

fn lines(texts: &[&str]) -> Vec<Line> {
    texts.iter().map(|t| Line::parse(t).unwrap()).collect()
}

#[test]
fn the_deck_codes() {
    assert_eq!(rec("a", 1.0, "3", "N").deck(), "open_grating");
    assert_eq!(rec("a", 1.0, "1", "0").deck(), "concrete");
    assert_eq!(rec("a", 1.0, "2", "4").deck(), "concrete");
    assert_eq!(rec("a", 1.0, "1", "6").deck(), "asphalt");
    assert_eq!(rec("a", 1.0, "1", "5").deck(), "other", "epoxy");
    assert_eq!(
        rec("a", 1.0, "4", "6").deck(),
        "asphalt",
        "closed grating paved"
    );
    assert_eq!(rec("a", 1.0, "8", "7").deck(), "other", "timber");
}

#[test]
fn a_line_round_trips_and_drops_old_deck_tags() {
    let l = Line::parse("1.5 1.62 bridge forward hpms=0.12 deck=asphalt nbi=x grate=1:2").unwrap();
    assert_eq!(l.ways, "forward");
    assert_eq!(l.tags, vec!["hpms=0.12".to_string()]);
    assert_eq!(l.text("forward", None), "1.5 1.62 bridge forward hpms=0.12");
}

#[test]
fn an_overpass_and_a_misfit_are_rejected_and_a_fit_is_kept() {
    let ls = lines(&["1 1.2 bridge hpms=0.2"]);
    let records = vec![
        rec("17-over", 40.0, "1", "6"),
        rec("17-long", 900.0, "1", "1"),
        rec("17-good", 0.19 * M_PER_MI, "1", "1"),
        rec("17-far", 0.2 * M_PER_MI, "1", "1"),
    ];
    let placed = vec![
        on(0, 3.0, 2.0),
        on(1, 1.1, 5.0),
        on(2, 1.21, 30.0),
        on(3, 1.1, 90.0),
    ];
    let m = match_direction(&ls, true, &placed, &records, 1.0);
    assert_eq!(
        m.fate,
        vec![
            Some(Reject::NoStructure),
            Some(Reject::Length),
            None,
            Some(Reject::Far)
        ]
    );
    let kept = m.kept[0].1.as_ref().unwrap();
    assert_eq!((kept.id.as_str(), kept.deck), ("17-good", "concrete"));
}

#[test]
fn twin_records_pick_the_better_length_and_count_the_tie() {
    let ls = lines(&["1 1.2 bridge"]);
    let records = vec![
        rec("nb", 0.17 * M_PER_MI, "1", "1"),
        rec("sb", 0.2 * M_PER_MI, "1", "1"),
    ];
    let placed = vec![on(0, 1.0, 1.0), on(1, 1.0, 40.0)];
    let m = match_direction(&ls, true, &placed, &records, 1.0);
    assert_eq!(m.kept[0].1.as_ref().unwrap().id, "sb");
    assert_eq!((m.ties, m.tie_disagree), (1, 0));
    assert_eq!(m.fate, vec![Some(Reject::Outfitted), None]);
}

#[test]
fn the_span_is_measured_miles() {
    // 0.21 leg miles on a leg 5 percent long is 0.2 measured miles.
    let ls = lines(&["1 1.21 bridge"]);
    let records = vec![rec("a", 0.2 * M_PER_MI, "1", "1")];
    let m = match_direction(&ls, true, &[on(0, 1.1, 1.0)], &records, 1.05);
    assert!(m.kept[0].1.as_ref().unwrap().len_err_mi < 1e-9);
}

#[test]
fn a_movable_grating_gets_its_span_centred_on_the_structure() {
    let ls = lines(&["10 10.3 bridge"]);
    let mut lift = rec("12-720022", 0.3 * M_PER_MI, "3", "0");
    lift.type_43b = 15;
    lift.main_spans = Some(1);
    lift.max_span_m = Some(0.1 * M_PER_MI);
    let mut fixed = lift.clone();
    fixed.type_43b = 2;
    let m = match_direction(&ls, true, &[on(0, 10.0, 0.0)], &[lift], 1.0);
    let (a, z) = m.kept[0].1.as_ref().unwrap().grate.unwrap();
    assert!(
        (a - 10.1).abs() < 1e-9 && (z - 10.2).abs() < 1e-9,
        "{a} {z}"
    );
    let m = match_direction(&ls, true, &[on(0, 10.0, 0.0)], &[fixed], 1.0);
    assert_eq!(m.kept[0].1.as_ref().unwrap().grate, None, "whole deck");
}

#[test]
fn a_shared_line_splits_only_when_its_directions_disagree() {
    let ls = lines(&[
        "1 1.2 bridge hpms=0.2",
        "1.2 1.25 bridge forward",
        "3 3.1 bridge",
        "5 5.5 tunnel",
    ]);
    let records = vec![
        rec("tail-fit", 0.25 * M_PER_MI, "1", "1"),
        rec("plain", 0.2 * M_PER_MI, "1", "6"),
        rec("short", 0.1 * M_PER_MI, "1", "1"),
    ];
    let placed = vec![on(0, 1.0, 1.0), on(1, 1.0, 30.0), on(2, 3.0, 1.0)];
    let fwd = match_direction(&ls, true, &placed, &records, 1.0);
    let rev = match_direction(&ls, false, &placed, &records, 1.0);
    let (out, split) = rewrite(&ls, &fwd, &rev);
    assert_eq!(split, 1);
    assert_eq!(
        out,
        vec![
            "1 1.2 bridge forward hpms=0.2 deck=concrete nbi=tail-fit",
            "1 1.2 bridge reverse hpms=0.2 deck=asphalt nbi=plain",
            "1.2 1.25 bridge forward deck=concrete nbi=tail-fit",
            "3 3.1 bridge deck=concrete nbi=short",
            "5 5.5 tunnel",
        ]
    );
}
