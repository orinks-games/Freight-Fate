use super::*;

#[test]
fn test_the_classic_jake_voice_covers_every_rpm_band() {
    // One voice per setting, whatever the rpm (owner, 2026-08-17: "it is
    // playing both").
    let mut r = rig();
    let bands: Vec<String> = [1200, 1400, 1600, 1800, 2000, 2200]
        .iter()
        .map(|b| format!("engine/jake_{b}"))
        .collect();
    r.engine.set_jake_voice(true);
    let classic: std::collections::BTreeSet<String> =
        bands.iter().map(|k| r.engine.voice_key(k)).collect();
    assert_eq!(
        classic,
        [JAKE_CLASSIC_KEY.to_string()].into_iter().collect()
    );

    r.engine.set_jake_voice(false);
    let recorded: std::collections::BTreeSet<String> =
        bands.iter().map(|k| r.engine.voice_key(k)).collect();
    // Real must collapse to the one recording too; the other bands are
    // synths.
    assert_eq!(
        recorded,
        [JAKE_RECORDED_KEY.to_string()].into_iter().collect()
    );
    assert!(!recorded.contains(JAKE_CLASSIC_KEY));
}

#[test]
fn test_the_jake_voice_switch_applies_on_every_band_not_just_1600() {
    let Some(mut r) = recordings_rig() else {
        return;
    };
    r.engine.set_jake_voice(false);
    // Growling on a band that is NOT 1600 -- the case the guard missed.
    r.engine
        .start_loop_with(CH_JAKE, "engine/jake_1400", 0.5, 300);
    assert_eq!(
        r.engine.backend().loop_entry(CH_JAKE).unwrap().0,
        JAKE_RECORDED_KEY
    );

    r.engine.set_jake_voice(true); // classic, live, mid-growl on 1400
    let entry = r.engine.backend().loop_entry(CH_JAKE).unwrap();
    assert_eq!(
        entry.0, JAKE_CLASSIC_KEY,
        "the switch did nothing off the 1600 band"
    );
    assert_eq!(entry.1, 0.5); // level carries across

    r.engine.set_jake_voice(false); // back to real, live
    assert!(r
        .engine
        .backend()
        .loop_entry(CH_JAKE)
        .unwrap()
        .0
        .starts_with(JAKE_BAND_PREFIX));
    r.engine.stop_loop(CH_JAKE);
}

#[test]
fn test_the_classic_jake_is_not_restarted_by_every_rpm_band() {
    // On classic every band maps to one synth cut, so a caller caching
    // the BAND key saw each rpm crossing as a new sound and restarted the
    // same file over itself. `voice_key` exists so the drive can cache
    // what will actually sound.
    let mut r = rig();
    let bands: Vec<String> = [1200, 1400, 1600, 1800, 2000, 2200]
        .iter()
        .map(|b| format!("engine/jake_{b}"))
        .collect();
    r.engine.set_jake_voice(true);
    let distinct: std::collections::BTreeSet<String> =
        bands.iter().map(|b| r.engine.voice_key(b)).collect();
    assert_eq!(
        distinct.len(),
        1,
        "the classic voice should resolve every band to one cut"
    );
    r.engine.set_jake_voice(false);
    let distinct: std::collections::BTreeSet<String> =
        bands.iter().map(|b| r.engine.voice_key(b)).collect();
    assert_eq!(
        distinct.len(),
        1,
        "real must resolve to one voice too -- there is only one recording"
    );
}

#[test]
fn test_one_jake_voice_sounds_whatever_the_rpm_in_both_directions() {
    let mut r = rig();
    let bands: Vec<String> = [1200, 1400, 1600, 1800, 2000, 2200]
        .iter()
        .map(|b| format!("engine/jake_{b}"))
        .collect();
    r.engine.set_jake_voice(false);
    let keys: std::collections::BTreeSet<String> =
        bands.iter().map(|b| r.engine.voice_key(b)).collect();
    assert_eq!(keys, [JAKE_RECORDED_KEY.to_string()].into_iter().collect());

    r.engine.set_jake_voice(true);
    let keys: std::collections::BTreeSet<String> =
        bands.iter().map(|b| r.engine.voice_key(b)).collect();
    assert_eq!(keys, [JAKE_CLASSIC_KEY.to_string()].into_iter().collect());

    // The Learn game sounds entry demos the classic cut by name, so asking
    // for it explicitly must never be re-voiced into the other one.
    for classic in [false, true] {
        r.engine.set_jake_voice(classic);
        assert_eq!(r.engine.voice_key(JAKE_CLASSIC_KEY), JAKE_CLASSIC_KEY);
    }
}
