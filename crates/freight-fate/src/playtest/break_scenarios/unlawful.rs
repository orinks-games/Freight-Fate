//! Breaking the law on purpose, and the edges of what it costs (port of
//! `tools/playtest_break_scenarios/unlawful.py`).
//!
//! The rest of the battery drives badly. This drives ILLEGALLY -- twenty over
//! past a staffed crossover, straight through an open scale, a stop sign at
//! the end of a ramp treated as a suggestion -- and then checks that what the
//! game charges for it is coherent: the fine matches the one place fines are
//! priced, the record moves once rather than twice, and the words the driver
//! hears name the same numbers the ledger does.

use crate::playtest::breaker::{outcome, outcome_of, Outcome, Rig, RigOptions, DT};

use ff_core::models::business::WEIGH_STATION_TRANSPONDER_LEVEL;
use ff_core::models::career::LEVEL_XP;
use ff_core::models::enforcement::{
    citation_fine, speeding_citation_fine, CITATION_REPEAT_MAX_MULTIPLIER,
    CONSTRUCTION_ZONE_FINE_MULTIPLIER, WEIGH_STATION_BYPASS_FINE,
};
use ff_core::sim::enforcement_posts::{
    EnforcementPost, KIND_FIXED_SCALE, KIND_MEDIAN, METHOD_SCALE_SCREEN, METHOD_VISUAL,
};
use ff_core::sim::trip::Trip;
use ff_core::sim::trip_models::RoadStop;

/// Where the injected furniture goes. Far enough in that the truck is settled
/// at speed and any advance warning has room to land.
const SCALE_MI: f64 = 4.0;

/// A row of staffed posts, each able to look at the truck and act.
///
/// Several rather than one on purpose: whether a given post looks is a seeded
/// roll, so a scenario built on exactly one post is a scenario that reports
/// nothing on most seeds. A row makes the LOOK reliable without touching the
/// odds themselves, which is what the roll is there to model.
fn staffed_posts(trip: &mut Trip, kind: &str, miles: &[f64], reach_mi: f64) {
    trip.posts = miles
        .iter()
        .map(|mi| {
            let mut post = EnforcementPost::new(*mi, kind);
            post.method = METHOD_VISUAL.to_string();
            post.reach_mi = reach_mi;
            post.staffed = true;
            post
        })
        .collect();
}

/// Drive at a fixed number over whatever is posted, and keep it there.
///
/// Speeding is read as continuous DISTANCE over the limit, not a moment, so a
/// scenario has to actually hold the speed while miles pass rather than
/// setting a velocity and stepping once.
fn hold_over_limit(rig: &mut Rig, mph_over: f64, frames: usize) -> usize {
    let mut ran = 0;
    for _ in 0..frames {
        let position = rig.drive.trip.position_mi;
        let (limit, _) = rig.drive.trip.speed_limit_at(position);
        rig.drive.truck_mut().velocity_mps = (limit + mph_over) / 2.23693629;
        ran += rig.step(1, DT, None);
        if rig.drive.pull_over.is_some() {
            break;
        }
    }
    ran
}

/// Hold twenty over past a row of staffed crossovers; the citation must price
/// itself the same way the model does.
pub fn speeding_past_staffed_posts() -> Outcome {
    let mut rig = Rig::new(RigOptions {
        keep_patrols: true,
        ..RigOptions::default()
    });
    let mut findings: Vec<String> = Vec::new();
    staffed_posts(
        &mut rig.drive.trip,
        KIND_MEDIAN,
        &[2.0, 3.0, 4.0, 5.0, 6.0, 7.0],
        0.6,
    );
    rig.prepare(55.0, None);
    let citations_before = rig
        .app
        .ctx
        .profile
        .as_ref()
        .map_or(0, |p| p.driving_record.citations);

    hold_over_limit(&mut rig, 20.0, 4000);

    if rig.drive.pull_over.is_none() {
        // Not a finding on its own: the look is a seeded roll and a run where
        // nobody looked is a legitimate outcome of the model. It only means
        // this seed proves nothing about the pricing.
        return outcome(
            "speeding_past_staffed_posts",
            &rig,
            findings,
            "no post looked on this seed; twenty over went unobserved",
        );
    }

    if rig.said("Lights and siren") == 0 {
        findings.push("a pull-over began with no lights-and-siren line spoken".to_string());
    }

    // `_fine_charged(rig)` walked three possible attribute names for the fine
    // on the pending stop; here it is one field.
    let charged = rig.drive.pull_over_fine;
    let expected = speeding_citation_fine(20.0, citations_before, false);
    if charged > 0.0 && (charged - expected).abs() > 1.0 {
        findings.push(format!(
            "speeding stop charges {charged:.0} where the model prices twenty over at \
             {expected:.0} for {citations_before} priors"
        ));
    }
    let note = format!(
        "twenty over observed and priced at {:.0}",
        if charged > 0.0 { charged } else { expected }
    );
    outcome("speeding_past_staffed_posts", &rig, findings, &note)
}

/// One open scale, staffed, screening.
fn inject_scale(trip: &mut Trip) -> RoadStop {
    let mut scale = RoadStop::new("Hamburg Scale", SCALE_MI, "weigh_station");
    scale.actions = vec!["inspect".to_string()];
    scale.parking = "none".to_string();
    trip.stops = vec![scale.clone()];
    let mut post = EnforcementPost::new(SCALE_MI, KIND_FIXED_SCALE);
    post.method = METHOD_SCALE_SCREEN.to_string();
    post.reach_mi = 1.0;
    post.staffed = true;
    post.anchor = scale.key();
    trip.posts = vec![post];
    scale
}

/// Blow an open scale and ride the failure-to-stop ladder out; every line must
/// still be true when it is spoken.
pub fn scale_bypass_to_the_end() -> Outcome {
    let mut rig = Rig::new(RigOptions::default());
    let mut findings: Vec<String> = Vec::new();
    inject_scale(&mut rig.drive.trip);
    // Lane keeping on full signals for an open scale itself; this driver
    // steers, and ignores the scale.
    rig.app.ctx.settings.lane_keeping = "partial".to_string();
    rig.prepare(62.0, None);

    // Never signal, never slow: the whole point is the driver who ignores
    // every instruction the scale gives.
    rig.step(
        20000,
        DT,
        Some(&|rig: &Rig| rig.drive.trip.position_mi > SCALE_MI + 2.0),
    );

    if rig.said("Open weigh station") == 0 {
        findings.push("the open scale never announced itself".to_string());
    }
    if rig.said("Scale bypass enforcement") == 0 {
        findings
            .push("driving straight through an open scale drew no bypass enforcement".to_string());
    }

    // The reminder is documented as firing once per scale. The pacer hands a
    // cut ROUTE line back when an urgent line interrupts it, and the bypass
    // ladder is three escalating warnings in a row -- so this counts what the
    // driver actually HEARD, not what the guard intended.
    let reminders = rig.said("Signal for the scale exit");
    if reminders > 2 {
        findings.push(format!(
            "\"Signal for the scale exit\" was spoken {reminders} times for one scale"
        ));
    }

    // And the harder half: a distance is a claim about now. Once the scale is
    // behind the truck, a line still offering its exit is telling the driver
    // to do something impossible.
    let past = rig.said("Hamburg Scale in");
    if past > 0 && rig.drive.trip.position_mi > SCALE_MI + 1.0 {
        findings.push(format!(
            "{past} scale-exit lines are still offering an exit the truck has passed"
        ));
    }
    outcome(
        "scale_bypass_to_the_end",
        &rig,
        findings,
        "bypass enforcement fired and the exit instruction was said once",
    )
}

/// The same overspeed inside roadwork must cost exactly twice what it costs
/// anywhere else.
///
/// The doubling and the repeat step compound in one place on purpose
/// (`citation_fine`). This checks the two multipliers still meet there and
/// nowhere else: a second application anywhere in the call path shows up here
/// as four times the base rather than twice.
pub fn work_zone_speeding_doubles_the_fine() -> Outcome {
    let mut findings: Vec<String> = Vec::new();
    for over in [10.0, 20.0, 35.0] {
        let plain = speeding_citation_fine(over, 0, false);
        let zone = speeding_citation_fine(over, 0, true);
        if (zone - plain * CONSTRUCTION_ZONE_FINE_MULTIPLIER).abs() > 0.01 {
            findings.push(format!(
                "{over:.0} over: roadwork charges {zone:.0} against {plain:.0} plain, not the \
                 {CONSTRUCTION_ZONE_FINE_MULTIPLIER}x the model promises"
            ));
        }
    }

    // The repeat step is capped; the zone doubling is applied after the cap,
    // so a career offender inside roadwork pays cap x 2 and never more.
    let base = speeding_citation_fine(20.0, 0, false);
    let worst = speeding_citation_fine(20.0, 50, true);
    let ceiling = base * CITATION_REPEAT_MAX_MULTIPLIER * CONSTRUCTION_ZONE_FINE_MULTIPLIER;
    if worst > ceiling + 0.01 {
        findings.push(format!(
            "fifty priors inside roadwork charges {worst:.0}, past the {ceiling:.0} the cap and \
             the zone doubling allow together"
        ));
    }
    let note = format!("roadwork doubles cleanly and a career offender tops out at {ceiling:.0}");
    outcome_of("work_zone_speeding_doubles_the_fine", findings, &note)
}

/// A career offender's fines must climb to the cap and then stop, not compound
/// forever.
pub fn repeat_citations_stop_compounding() -> Outcome {
    let mut findings: Vec<String> = Vec::new();
    let priced: Vec<f64> = (0..40)
        .map(|n| citation_fine(WEIGH_STATION_BYPASS_FINE, n, false, None))
        .collect();
    if priced.windows(2).any(|pair| pair[1] < pair[0] - 0.01) {
        findings.push("a further prior citation made the fine go DOWN".to_string());
    }
    let cap = WEIGH_STATION_BYPASS_FINE * CITATION_REPEAT_MAX_MULTIPLIER;
    let last = *priced.last().expect("forty prices");
    if last > cap + 0.01 {
        findings.push(format!(
            "forty priors price a bypass at {last:.0}, past the cap {cap:.0}"
        ));
    }
    if last < priced[0] {
        findings.push("a repeat offender is charged less than a first offender".to_string());
    }
    let note = format!("the repeat step climbs to {cap:.0} and holds there");
    outcome_of("repeat_citations_stop_compounding", findings, &note)
}

/// Lean on the horn with the compressor gone: the protection valve must save
/// the brakes, not the horn.
///
/// Brandon's shared-air ask, and the realism audit that followed it. Two
/// halves of one claim, because half of it is about what a determined player
/// CANNOT do -- and those are the claims that quietly stop being true. With
/// the engine turning, the compressor out-earns the horn. With the engine dead
/// and the air already going, the horn draws the tanks down to the protection
/// valve and stops there: FMVSS 121 pressure-protects accessories, so honking
/// your way into a spring-brake lockdown is mechanically impossible on a
/// compliant tractor.
pub fn honk_the_air_down_to_the_valve() -> Outcome {
    use ff_core::sim::vehicle::TruckState;

    let mut rig = Rig::new(RigOptions::default());
    let mut findings: Vec<String> = Vec::new();
    rig.prepare(58.0, None);

    // Half one: engine running. The compressor should win, every time.
    rig.drive.truck_mut().primary_air_psi = 95.0;
    rig.drive.truck_mut().secondary_air_psi = 95.0;
    let before = rig
        .drive
        .truck()
        .primary_air_psi
        .min(rig.drive.truck().secondary_air_psi);
    for _ in 0..6000 {
        rig.drive.truck_mut().horn_on = true;
        rig.step(1, DT, None);
    }
    let with_engine = rig
        .drive
        .truck()
        .primary_air_psi
        .min(rig.drive.truck().secondary_air_psi);
    if with_engine < TruckState::HORN_PROTECTION_PSI {
        findings.push(format!(
            "a long blast with the engine running pulled the tanks from {before:.0} to \
             {with_engine:.0} psi, through the protection valve"
        ));
    }

    // Half two: engine dead, coasting, air already low. Now the horn is the
    // only draw and the valve is the only thing standing between it and the
    // brakes.
    rig.drive.truck_mut().horn_on = false;
    rig.drive.truck_mut().stop_engine();
    rig.drive.truck_mut().primary_air_psi = TruckState::HORN_PROTECTION_PSI + 4.0;
    rig.drive.truck_mut().secondary_air_psi = TruckState::HORN_PROTECTION_PSI + 4.0;
    rig.drive.truck_mut().horn_on = true;
    let mut floor = rig
        .drive
        .truck()
        .primary_air_psi
        .min(rig.drive.truck().secondary_air_psi);
    for _ in 0..30000 {
        if rig.drive.truck().horn_available() {
            rig.drive.truck_mut().horn_on = true;
        }
        rig.step(1, DT, None);
        floor = floor
            .min(rig.drive.truck().primary_air_psi)
            .min(rig.drive.truck().secondary_air_psi);
        if !rig.drive.truck().horn_available() && !rig.drive.truck().horn_on {
            break;
        }
    }

    if rig.drive.truck().horn_available() {
        findings.push(format!(
            "with no compressor the horn still never reached the valve ({:.0} psi)",
            rig.drive.truck().primary_air_psi
        ));
    } else if rig.said("Horn cut out") == 0 {
        findings.push("the horn cut out with no spoken reason; it just stopped".to_string());
    }
    if floor < TruckState::HORN_PROTECTION_PSI - 1.0 {
        findings.push(format!(
            "the horn pulled the tanks to {floor:.0} psi, past the {:.0} psi the protection \
             valve is supposed to hold",
            TruckState::HORN_PROTECTION_PSI
        ));
    }
    if rig.drive.truck().parking_brake {
        findings
            .push("honking alone locked the spring brakes, which FMVSS 121 forbids".to_string());
    }
    let note =
        format!("the compressor won while running; dead, the valve closed at {floor:.0} psi");
    outcome("honk_the_air_down_to_the_valve", &rig, findings, &note)
}

/// A weigh-in-motion green light must clear the scale; driving on cannot then
/// be charged as a bypass.
///
/// The bypass ladder and the weigh-in-motion verdict are separate systems that
/// both watch the same gore point. If the verdict clears a driver and the
/// ladder has not been told, the game tells you to keep rolling and then
/// tickets you 1,800 dollars for doing it.
pub fn prepass_green_is_not_a_bypass_charge() -> Outcome {
    let mut rig = Rig::new(RigOptions::default());
    let mut findings: Vec<String> = Vec::new();
    // A company driver the fleet trusts with a transponder. The level is
    // derived from XP, so this is the honest way to grant it.
    if let Some(profile) = rig.app.ctx.profile.as_mut() {
        profile.career.xp = LEVEL_XP[WEIGH_STATION_TRANSPONDER_LEVEL as usize - 1];
    }
    let scale = inject_scale(&mut rig.drive.trip);
    rig.prepare(62.0, None);
    rig.step(
        20000,
        DT,
        Some(&|rig: &Rig| rig.drive.trip.position_mi > SCALE_MI + 2.0),
    );

    // The verdict map is keyed the way the enforcement watch keys a scale, not
    // by the stop's own key -- ask the state to build it rather than guessing.
    let key = rig.drive.weigh_station_key(&scale);
    let verdict = rig
        .drive
        .weigh_station_transponder_verdict
        .get(&key)
        .cloned();
    match verdict.as_deref() {
        None => findings.push(format!(
            "a level {WEIGH_STATION_TRANSPONDER_LEVEL} company driver got no weigh-in-motion \
             verdict at an open scale"
        )),
        Some("green") => {
            let spoke = rig.said("Cleared past the scale");
            if spoke == 0 {
                findings.push("a green verdict never told the driver to keep rolling".to_string());
            } else if spoke > 1 {
                // One verdict, one line. A ROUTE line cut by a hazard is
                // handed back by the pacer, and nothing caps how many times
                // that can happen, so a busy moment turns one clearance into
                // a chant.
                findings.push(format!(
                    "the green-light clearance was spoken {spoke} times for one scale"
                ));
            }
            if rig.said("Scale bypass enforcement") > 0 {
                findings.push(
                    "the transponder cleared the scale and the bypass ladder charged for it anyway"
                        .to_string(),
                );
            }
            if rig.said("Signal for the scale exit") > 0 {
                findings.push(
                    "a cleared driver was still told to signal for an exit they do not need"
                        .to_string(),
                );
            }
        }
        Some(_) => {}
    }
    let note = format!("weigh-in-motion verdict {verdict:?}, and the ladder agreed with it");
    outcome(
        "prepass_green_is_not_a_bypass_charge",
        &rig,
        findings,
        &note,
    )
}
