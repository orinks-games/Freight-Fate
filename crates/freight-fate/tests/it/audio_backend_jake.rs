use super::*;

#[test]
fn test_every_jake_rpm_band_routes_to_the_recorded_cut() {
    let r = rig();
    let bands: Vec<String> = [1200, 1400, 1600, 1800, 2000, 2200]
        .iter()
        .map(|b| format!("engine/jake_{b}"))
        .collect();
    let recorded: std::collections::BTreeSet<String> =
        bands.iter().map(|k| r.engine.voice_key(k)).collect();
    assert_eq!(
        recorded,
        [JAKE_RECORDED_KEY.to_string()].into_iter().collect()
    );
    assert_eq!(
        r.engine.voice_key("engine/jake_1600_synth"),
        JAKE_RECORDED_KEY
    );
}

#[test]
fn test_jake_band_loop_plays_recorded_cut_on_every_band() {
    let Some(mut r) = recordings_rig() else {
        return;
    };
    r.engine
        .start_loop_with(CH_JAKE, "engine/jake_1400", 0.5, 300);
    let entry = r.engine.backend().loop_entry(CH_JAKE).unwrap();
    assert_eq!(entry.0, JAKE_RECORDED_KEY);
    assert_eq!(entry.1, 0.5);
    r.engine.stop_loop(CH_JAKE);
}

#[test]
fn test_jake_is_not_restarted_by_every_rpm_band() {
    let r = rig();
    let bands: Vec<String> = [1200, 1400, 1600, 1800, 2000, 2200]
        .iter()
        .map(|b| format!("engine/jake_{b}"))
        .collect();
    let distinct: std::collections::BTreeSet<String> =
        bands.iter().map(|b| r.engine.voice_key(b)).collect();
    assert_eq!(
        distinct.len(),
        1,
        "every jake band must resolve to the single recorded cut"
    );
}
