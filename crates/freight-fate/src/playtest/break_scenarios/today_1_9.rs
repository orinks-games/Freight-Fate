//! The 1.9 tester-reported fixes, driven the way the reports were written
//! (port of `tools/playtest_break_scenarios/today_1_9.py`).
//!
//! Each of these exists because a tester found something by playing badly or
//! oddly, and the fix that followed made a promise. A promise in a changelog
//! is worth what the next change leaves of it, so these drive the promise
//! rather than the code path: what does the driver end up with, and does the
//! game's own speech agree with the ledger.

use crate::playtest::breaker::{force_grade, outcome, outcome_of, Outcome, Rig, RigOptions, DT};
use crate::states::base::Key;
use crate::states::driving_pause_states::{AbandonJobConfirmationState, PauseMenuState};

use ff_core::sim::traffic_manager::BRAKING_CAUSE_LINES;
use ff_core::sim::trip_models::HAZARDS;
use ff_core::sim::weather::WeatherKind;
use ff_core::speech_text::brake_lights_cue;

/// Walk the real confirmation state, exactly as the pause menu does.
fn abandon(rig: &mut Rig) -> bool {
    rig.with_drive_on_stack(|rig, drive| {
        rig.app
            .ctx
            .push_state_with(PauseMenuState::with_drive(drive.clone()), false);
        rig.app
            .ctx
            .push_state(AbandonJobConfirmationState::new(drive));
        rig.app.ctx.run_deferred();
        rig.select_menu_containing("abandon the job")
    })
}

/// Shane's bobtail turnaround: no load means no contract, so calling it off
/// can cost no money.
///
/// Shane P., 2026-08-20: turning back from an empty reposition was fined.
/// Three shapes of the same menu item, and the whole fix is that they are
/// priced differently: a loaded job breaches a paying contract (500 dollars
/// and 5 reputation), a self-serve bobtail breaches nothing at all, and a
/// dispatch-assigned reposition costs standing but not money.
pub fn abandon_an_empty_run_costs_nothing() -> Outcome {
    let mut findings: Vec<String> = Vec::new();
    for (label, bobtail, assigned, want_money, want_rep) in [
        ("self-serve bobtail", true, false, 0.0, 0.0),
        ("assigned reposition", true, true, 0.0, 3.0),
        ("loaded job", false, false, 500.0, 5.0),
    ] {
        let mut rig = Rig::new(RigOptions::default());
        rig.drive.job.bobtail = bobtail;
        rig.drive.job.assigned = assigned;
        rig.drive.trip.position_mi = rig.drive.trip.total_miles() * 0.5;
        let (money_before, rep_before, hours_before) = {
            let profile = rig.app.ctx.profile.as_ref().expect("a profile");
            (
                profile.money(),
                profile.career.reputation,
                profile.game_hours,
            )
        };

        if !abandon(&mut rig) {
            findings.push(format!(
                "{label}: the confirmation had no row that abandons"
            ));
            drop(rig);
            continue;
        }

        let (money, reputation, hours) = {
            let profile = rig.app.ctx.profile.as_ref().expect("a profile");
            (
                profile.money(),
                profile.career.reputation,
                profile.game_hours,
            )
        };
        let lost = money_before - money;
        let rep_lost = rep_before - reputation;
        if (lost - want_money).abs() > 0.01 {
            findings.push(format!(
                "{label}: abandoning cost {lost:.2} dollars, expected {want_money:.2}"
            ));
        }
        if (rep_lost - want_rep).abs() > 0.01 {
            findings.push(format!(
                "{label}: abandoning cost {rep_lost:.1} reputation, expected {want_rep:.1}"
            ));
        }
        // The hours already driven happened whatever the freight was.
        if hours < hours_before {
            findings.push(format!(
                "{label}: abandoning wound the career clock BACKWARDS"
            ));
        }

        // And the confirmation has to describe the branch it will take.
        let said = rig.transcript().join(" ");
        if want_money > 0.0 && !said.contains("five hundred") {
            findings.push(format!(
                "{label}: charges 500 without warning the driver first"
            ));
        }
        if want_money == 0.0 && said.contains("five hundred") {
            findings.push(format!(
                "{label}: promised a 500 dollar penalty it does not charge"
            ));
        }
        // A TestApp holds the environment lock until it is dropped.
        drop(rig);
    }

    outcome_of(
        "abandon_an_empty_run_costs_nothing",
        findings,
        "empty runs cost nothing, an assignment costs standing, a load costs 500 and 5",
    )
}

/// Brandon's climb report: the retarder holds a load back downhill and does
/// nothing else.
///
/// Brandon, 2026-08-20: cruise reached for the jake on a CLIMB, where the hill
/// was about to take that speed for free. The same week's fix stopped it on
/// soaked level pavement, where no real driver would allow it. Both are the
/// same rule -- the retarder exists to hold a load back on a downgrade, and
/// slowing anywhere else is the service brakes' job.
pub fn jake_stays_off_where_it_does_not_belong() -> Outcome {
    let mut rig = Rig::new(RigOptions::default());
    let mut findings: Vec<String> = Vec::new();
    rig.prepare(62.0, None);
    rig.press(Key::K); // cruise on, holding this speed

    // A sustained climb, with the truck over its set speed: the shape that
    // used to reach for the retarder.
    force_grade(&mut rig.drive.trip, 0.05);
    rig.drive.truck_mut().grade = 0.05;
    for climbed in 1..=1800 {
        rig.drive.truck_mut().grade = 0.05;
        rig.step(1, DT, None);
        if rig.drive.truck().engine_brake_stage > 0 {
            findings.push(format!(
                "the jake came up on a 5 percent CLIMB after {climbed} frames at {:.0} mph",
                rig.drive.truck().speed_mph()
            ));
            break;
        }
    }

    // Slick, level pavement. Same demand to lose speed, and the drums are the
    // only right answer.
    force_grade(&mut rig.drive.trip, 0.0);
    rig.drive.truck_mut().grade = 0.0;
    rig.drive.truck_mut().engine_brake_stage = 0;
    rig.drive.weather_mut().forced = Some(WeatherKind::Rain);
    rig.drive.weather_mut().current = WeatherKind::Rain;
    for _ in 0..1800 {
        rig.drive.truck_mut().grade = 0.0;
        rig.step(1, DT, None);
        if rig.drive.truck().engine_brake_stage > 0 {
            findings.push(format!(
                "the jake came up on wet LEVEL road at {:.0} mph",
                rig.drive.truck().speed_mph()
            ));
            break;
        }
    }
    outcome(
        "jake_stays_off_where_it_does_not_belong",
        &rig,
        findings,
        "the retarder stayed down on a climb and on wet level road",
    )
}

/// Brandon's naming asks: debris and animals say what they are, and are no
/// more common than before.
///
/// Splitting one hazard into six is a frequency change unless it is not. The
/// family's weights have to sum to what the single entry carried, or the split
/// quietly makes debris commoner and the road busier than it was.
pub fn named_hazards_keep_their_frequency() -> Outcome {
    let mut findings: Vec<String> = Vec::new();

    let debris_family = [
        "a ladder fallen from a truck in the lane",
        "loose lumber dropped across the lane",
        "a mattress lying in the lane",
        "spilled cargo boxes across the lane",
        "a shredded truck tarp in the lane",
        "debris on the road",
    ];
    let animal_family = [
        "a dog loose on the road",
        "a coyote crossing the road",
        "loose livestock on the road",
        "a raccoon in the lane",
        "an animal on the road",
    ];

    let weight_of = |text: &str| -> Option<f64> {
        HAZARDS
            .iter()
            .find(|hazard| hazard.text == text)
            .map(|hazard| hazard.weight)
    };

    for (label, family, expected, generic) in [
        ("debris", &debris_family[..], 1.2, "debris on the road"),
        (
            "nationwide animals",
            &animal_family[..],
            0.7,
            "an animal on the road",
        ),
    ] {
        let missing: Vec<&str> = family
            .iter()
            .copied()
            .filter(|text| weight_of(text).is_none())
            .collect();
        if !missing.is_empty() {
            findings.push(format!(
                "{label}: these entries are gone from the table: {missing:?}"
            ));
            continue;
        }
        let total: f64 = family.iter().filter_map(|text| weight_of(text)).sum();
        if (total - expected).abs() > 0.001 {
            findings.push(format!(
                "{label} now weigh {total:.3} against the {expected} the split promised to \
                 preserve; the road got busier without anyone deciding to"
            ));
        }
        // The anonymous fallback is meant to be the rare unidentifiable one,
        // not the common case wearing a family as cover.
        let share = if total != 0.0 {
            weight_of(generic).unwrap_or(0.0) / total
        } else {
            1.0
        };
        if share > 0.2 {
            findings.push(format!(
                "{label}: the unnamed fallback is {:.0}% of the family, so 'what is it?' still \
                 usually has no answer",
                share * 100.0
            ));
        }
    }

    // And the contract the naming exists for: a hazard the driver has to clear
    // must be nameable when they clear it.
    for hazard in HAZARDS {
        if (hazard.in_lane || hazard.animal) && hazard.name.is_empty() {
            findings.push(format!(
                "hazard {:?} has no name to clear it by",
                hazard.text
            ));
        }
    }

    outcome_of(
        "named_hazards_keep_their_frequency",
        findings,
        "debris and animals are named, and exactly as common as they were",
    )
}

/// Fight the storm cap with the cruise key, then clear the sky: the cap must
/// hold, then let go.
///
/// Two failures a clean unit test does not reach. First, a driver who simply
/// refuses the cap must not be able to ratchet past the safe speed; if they
/// could, the cap is advice with extra steps. Second, and worse, a cap that
/// fails to RELEASE leaves the truck governed on a dry road with nothing to
/// explain it, which is indistinguishable from a broken speed keeper.
pub fn weather_cap_releases_when_the_sky_does() -> Outcome {
    let mut rig = Rig::new(RigOptions::default());
    let mut findings: Vec<String> = Vec::new();
    rig.prepare(68.0, None);
    rig.press(Key::K); // cruise on around 68

    rig.drive.weather_mut().forced = Some(WeatherKind::Thunderstorm);
    rig.drive.weather_mut().current = WeatherKind::Thunderstorm;
    // How far over the storm's number the truck ran where the posted limit
    // was above it: under a lower posted limit the limit is what slows it.
    let mut storm_top = 0.0f64;
    for _ in 0..20 {
        rig.run_frames(30);
        let here = rig.drive.trip.position_mi;
        let (posted, _) = rig.drive.trip.speed_limit_at(here);
        let safe_now = rig.drive.weather().effects().safe_speed_mph;
        if posted > safe_now {
            storm_top = storm_top.max(rig.drive.truck().speed_mph());
        }
    }
    let safe = rig.drive.weather().effects().safe_speed_mph;
    if safe >= 68.0 {
        let note = format!("this storm's safe speed is {safe:.0}, no cap to test below 68");
        return outcome(
            "weather_cap_releases_when_the_sky_does",
            &rig,
            findings,
            &note,
        );
    }

    // Argue with it: twenty presses of cruise-up in the storm, on an empty
    // road so the only thing holding the truck down is the cap.
    let mut top = 0.0f64;
    for _ in 0..20 {
        rig.drive.trip.set_npc_vehicles(Vec::new());
        rig.drive.trip.traffic_pressures.clear();
        rig.press(Key::Equals);
        rig.run_frames(30);
        top = top.max(rig.drive.truck().speed_mph());
    }
    if top > safe + 4.0 {
        findings.push(format!(
            "holding the cruise-up key through the storm reached {top:.0} mph against a safe \
             speed of {safe:.0}"
        ));
    }
    // The easing line speaks only when the cap really slows the truck. A rig
    // that climbs out of town under the storm's number is never over it, and
    // silence there is the rule, not a missing line.
    if rig.said("easing to") == 0 && storm_top.max(top) > safe + 2.0 {
        findings.push("the storm cap was applied with nothing said about it".to_string());
    }

    // Now the sky clears. The cap has to let go on its own.
    //
    // Judged on a road with nothing else on it: the first version of this ran
    // into a hazard, read the resulting near-stop as a cap that never
    // released, and reported a bug that was a braking event.
    rig.drive.weather_mut().forced = Some(WeatherKind::Clear);
    rig.drive.weather_mut().current = WeatherKind::Clear;
    let mut recovered = 0.0f64;
    for _ in 0..240 {
        rig.drive.trip.set_npc_vehicles(Vec::new());
        rig.drive.trip.traffic_pressures.clear();
        rig.run_frames(25);
        recovered = recovered.max(rig.drive.truck().speed_mph());
        if recovered > safe + 5.0 {
            break;
        }
    }

    let clear_safe = rig.drive.weather().effects().safe_speed_mph;
    if clear_safe < 68.0 {
        findings.push(format!(
            "clear weather still reports a safe speed of {clear_safe:.0}"
        ));
    } else if recovered < safe + 5.0 {
        findings.push(format!(
            "the sky cleared and the truck never got above {recovered:.0} mph, about the \
             storm's {safe:.0}; the cap looks stuck"
        ));
    }
    let note =
        format!("the cap held at {safe:.0} against twenty presses, then let go when it cleared");
    outcome(
        "weather_cap_releases_when_the_sky_does",
        &rig,
        findings,
        &note,
    )
}

/// Brandon's why: a slowdown names its cause only where the road really has
/// one, and never guesses.
///
/// A zone reason the generator produces but the cause table has never heard of
/// simply loses its explanation, and nobody notices because the line still
/// reads fine. A cause-table key no zone ever carries is dead vocabulary that
/// looks like coverage. This checks both directions against the real Zone
/// reasons, and then checks the shape of the line an unknown cause produces.
pub fn brake_lights_never_invent_a_cause() -> Outcome {
    let mut findings: Vec<String> = Vec::new();

    // The reasons the trip generator actually stamps on a Zone. A literal, so
    // that adding a zone kind is a visible diff here rather than a silently
    // uncovered case -- but checked against the REAL table, not against
    // another copy of itself.
    let generated = ["construction", "construction merge", "heavy traffic"];

    let unexplained: Vec<&str> = generated
        .iter()
        .copied()
        .filter(|reason| {
            !BRAKING_CAUSE_LINES
                .iter()
                .any(|(key, line)| key == reason && !line.is_empty())
        })
        .collect();
    if !unexplained.is_empty() {
        findings.push(format!(
            "these zone reasons reach the road with no cause line: {unexplained:?}"
        ));
    }
    let dead: Vec<&str> = BRAKING_CAUSE_LINES
        .iter()
        .map(|(key, _)| *key)
        .filter(|key| !generated.contains(key))
        .collect();
    if !dead.is_empty() {
        findings.push(format!(
            "the cause table explains reasons nothing ever produces: {dead:?}"
        ));
    }

    // An unknown cause must add no clause at all -- no dangling sentence, no
    // doubled space where the clause would have gone.
    let bare = brake_lights_cue("half a mile", "forty miles per hour", "40", "");
    for terse in [false, true] {
        let rendering = bare.render(terse);
        if rendering.is_empty() {
            continue;
        }
        if rendering.contains("  ") {
            findings.push(format!(
                "an unexplained slowdown leaves a doubled space: {rendering:?}"
            ));
        }
        if has_empty_sentence(rendering) {
            findings.push(format!(
                "an unexplained slowdown leaves an empty sentence: {rendering:?}"
            ));
        }
    }

    // And a known cause must actually appear, in the full form only: terse
    // slots stay compact by design.
    let named = brake_lights_cue(
        "half a mile",
        "forty miles per hour",
        "40",
        "Road work is the cause.",
    );
    if !named.render(false).contains("Road work is the cause.") {
        findings.push("a known cause never reaches the spoken line".to_string());
    }
    let terse = named.render(true);
    if !terse.is_empty() && terse.contains("Road work is the cause.") {
        findings.push(
            "the cause clause leaked into the terse rendering, which must stay compact".to_string(),
        );
    }

    outcome_of(
        "brake_lights_never_invent_a_cause",
        findings,
        "every mapped reason has a cause line, and an unmapped one stays honestly silent",
    )
}

/// `re.search(r"\.\s*\.", text)`: a full stop with nothing but space before
/// the next one.
fn has_empty_sentence(text: &str) -> bool {
    let bytes: Vec<char> = text.chars().collect();
    for (i, ch) in bytes.iter().enumerate() {
        if *ch != '.' {
            continue;
        }
        let mut j = i + 1;
        while bytes.get(j).is_some_and(|c| c.is_whitespace()) {
            j += 1;
        }
        if bytes.get(j) == Some(&'.') {
            return true;
        }
    }
    false
}
