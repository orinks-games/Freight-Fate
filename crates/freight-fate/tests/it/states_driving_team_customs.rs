//! The co-driver clears customs alone (owner ruling, 2026-11-28 13:00 UTC,
//! replacing the morning's hand-back). On All assists the truck stops at
//! the booth, the clearance is charged as on a solo crossing with no menu,
//! and one note tells the player. A slept shift's jump ends short of the
//! booth and of any line where the hours rules change, so both are driven
//! live. Agent drive, 2026-11-28: a team load out of Seattle handed over 5
//! miles before Abbotsford-Huntingdon woke 531 miles into British Columbia
//! with no stop and no clearance.

use ff_core::sim::hos::HosRules;
use ff_core::sim::trip_models::BorderBooth;
use ff_core::sim::weather::WeatherKind;
use ff_core::speech_pacing::SpeechCategory;
use ff_core::units::UnitsMode;
use freight_fate::playtest::{PlaytestHarness, RouteSetup};
use freight_fate::states::driving::DrivingState;
use freight_fate::states::driving_border::{border_secondary_referral, BorderClearanceState};
use freight_fate::states::driving_core::{BORDER_PRIMARY_MIN, BORDER_SECONDARY_EXTRA_MIN};
use freight_fate::states::driving_team::TeamMode;
use freight_fate::states::driving_updates::after_crossing::AfterCrossing;
use freight_fate::states::driving_updates::fatigue::hours_rules_line;

const DT: f64 = 1.0 / 60.0;
const MPS_PER_MPH: f64 = 1.0 / 2.236_936;
const CLEAR_ROW: &str = "Answer the officer's questions and hand over your documents";

/// A quiet road from the first to the last of `cities` on All assists with
/// Automatic units, with the truck at 60 on cruise at `at_mi`.
fn on_the_road(cities: &[&str], at_mi: f64) -> PlaytestHarness {
    let mut harness = PlaytestHarness::new();
    harness.app.ctx.settings.facility_streets = false;
    let origin = cities[0];
    let destination = cities[cities.len() - 1];
    harness.start_route(origin, destination, RouteSetup::default().cities(cities));
    harness.prepare_for_driving(60.0);
    assert!(harness
        .app
        .ctx
        .settings
        .apply_driving_assistance_preset("all"));
    harness
        .app
        .ctx
        .settings
        .set_units_mode(UnitsMode::Automatic);
    harness.app.ctx.settings.hos_mode = "realistic".to_string();
    harness.app.ctx.settings.place_callouts = "off".to_string();
    harness.with_drive(move |d, ctx| {
        d.tutorial = None;
        d.departure_checked = true;
        d.trip.hazard_check_mi = 1e9;
        d.trip.inspection_check_mi = 1e9;
        d.trip.traffic_manager.rolling_bubble = false;
        d.trip.set_npc_vehicles(Vec::new());
        d.trip.traffic_pressures.clear();
        d.trip.zones.retain(|z| z.aadt.is_none());
        d.trip.weather.current = WeatherKind::Clear;
        d.trip.set_patrols(Vec::new());
        d.trip.posts.clear();
        d.job.deadline_game_h = d.trip.game_minutes / 60.0 + 200.0;
        d.trip.position_mi = at_mi;
        d.enforcement_prev_mi = at_mi;
        d.truck_mut().velocity_mps = 60.0 * MPS_PER_MPH;
        d.engage_cruise(ctx, 60.0, false);
    });
    referred_at_every_booth(&mut harness, false);
    harness
}

/// A trip seed that sends the truck to secondary inspection at every booth
/// (`referred`), or through primary at every one.
fn referred_at_every_booth(harness: &mut PlaytestHarness, referred: bool) {
    let keys: Vec<String> =
        harness.read_drive(|d| d.trip.border_booths.iter().map(|b| b.key.clone()).collect());
    let seed = (0..2000)
        .find(|seed| {
            keys.iter()
                .all(|key| border_secondary_referral(*seed, key) == referred)
        })
        .expect("a seed for the referral");
    harness.with_drive(move |d, _| d.trip_seed = seed);
}

fn booth_named(harness: &PlaytestHarness, name: &str) -> BorderBooth {
    harness.read_drive(|d| {
        d.trip
            .border_booths
            .iter()
            .find(|booth| booth.name == name)
            .cloned()
            .unwrap_or_else(|| panic!("{name} on the drive: {:?}", d.trip.border_booths))
    })
}

/// The trip mile of the first state or province line ahead into `into`.
fn line_into(harness: &PlaytestHarness, into: &str) -> f64 {
    harness.read_drive(|d| {
        d.trip
            .navigation_cues
            .iter()
            .find(|cue| cue.kind == "state_crossing" && cue.key.ends_with(into))
            .map(|cue| cue.at_mi)
            .unwrap_or_else(|| panic!("a line into {into}"))
    })
}

fn hand_over(harness: &mut PlaytestHarness, mode: TeamMode) {
    harness.with_drive(move |d, ctx| {
        d.hand_wheel_to_co_driver(ctx, mode);
        d.truck_mut().parking_brake = false;
    });
}

fn co_driver_at_wheel(harness: &PlaytestHarness) -> bool {
    harness.read_drive(|d| d.co_driver_driving())
}

fn co_driver_name(harness: &PlaytestHarness) -> &'static str {
    harness.read_drive(|d| d.team.as_ref().expect("a team").co_driver.name)
}

/// Tick the whole app until `done` or `max_frames` pass.
fn run_until(
    harness: &mut PlaytestHarness,
    max_frames: usize,
    mut done: impl FnMut(&PlaytestHarness) -> bool,
) -> bool {
    for _ in 0..max_frames {
        harness.advance_frame_clock();
        harness.app.frame(DT);
        if done(harness) {
            return true;
        }
    }
    false
}

/// Both logbooks' rules: the driver's, and the one off the wheel.
fn rules(harness: &PlaytestHarness) -> (HosRules, HosRules) {
    let driver = harness
        .app
        .ctx
        .profile
        .as_ref()
        .expect("a career")
        .hos
        .rules;
    let off = harness.read_drive(|d| d.team.as_ref().expect("a team").off_wheel_hos.rules);
    (driver, off)
}

const NORTH: &[&str] = &[
    "seattle_wa_us",
    "everett_wa_us",
    "bellingham_wa_us",
    "abbotsford_bc_ca",
    "chilliwack_bc_ca",
];
const HUNTINGDON: &str = "Abbotsford-Huntingdon Port of Entry";

const CA_RULES_LINE: &str =
    "Canadian hours rules: 13 hours of driving and 14 on duty in a 16-hour window.";
const KM_LINE: &str = "Speeds and distances are in kilometers now.";

fn booth_cleared(harness: &PlaytestHarness, booth: &BorderBooth) -> bool {
    let key = format!("border-booth:{}", booth.key);
    harness.read_drive(move |d| d.enforcement_events.contains(&key))
}

fn wake_notes(harness: &PlaytestHarness) -> Vec<String> {
    harness.read_drive(|d| {
        d.team
            .as_ref()
            .and_then(|team| team.wheel.as_ref())
            .map(|wheel| wheel.log.notes.clone())
            .unwrap_or_default()
    })
}

/// No line the officer, the booth or a hand-back for customs says.
fn assert_no_booth_lines(said: &str) {
    for line in [
        "Every truck stops at the customs booth.",
        "Stop at the booth.",
        "The officer asks for your passport",
        "Welcome to",
        "for customs",
        "at the customs booth",
        "stopped the truck for running",
    ] {
        assert!(!said.contains(line), "{line:?} was said: {said}");
    }
}

/// The co-driver drives up to `booth` in `mode` from `lead_mi` short of the
/// line into `into` and runs `after_mi` past it. Returns the harness with
/// the co-driver still at the wheel.
fn co_driver_drives_through(
    cities: &[&str],
    into: &str,
    booth_name: &str,
    mode: TeamMode,
    lead_mi: f64,
) -> (PlaytestHarness, BorderBooth) {
    let mut harness = on_the_road(cities, 3.0);
    let booth = booth_named(&harness, booth_name);
    let line = line_into(&harness, into);
    harness.with_drive(move |d, ctx| {
        d.trip.position_mi = line - lead_mi;
        d.enforcement_prev_mi = d.trip.position_mi;
        // At the posted speed there: a mountain road is no place to be
        // dropped at 60.
        let (posted, _) = d.trip.speed_limit_at(d.trip.position_mi);
        let mph = posted.min(60.0);
        d.truck_mut().velocity_mps = mph * MPS_PER_MPH;
        d.engage_cruise(ctx, mph, false);
    });
    hand_over(&mut harness, mode);
    let past = booth.at_mi + 0.5;
    let at_booth = booth.at_mi;
    // The slowest the truck went in the last tenth of a mile to the booth:
    // the booth's stop brought it there, not the catch for a truck that
    // reached it moving. Sampled once a frame, so only riding along: a
    // slept shift runs many ticks a frame and stops, clears and moves off
    // inside one.
    let mut slowest = f64::MAX;
    let through = run_until(&mut harness, 60 * 60 * 30, |h| {
        let (at, mph) = h.read_drive(|d| (d.trip.position_mi, d.truck().speed_mph()));
        if (at_booth - 0.1..=at_booth).contains(&at) {
            slowest = slowest.min(mph);
        }
        !co_driver_at_wheel(h) || at > past
    });
    let said = harness.transcript_text();
    assert!(through, "the truck never got past {}: {said}", booth.name);
    assert!(
        co_driver_at_wheel(&harness),
        "the co-driver handed back: {said}"
    );
    assert!(booth_cleared(&harness, &booth), "{said}");
    if mode == TeamMode::RideAlong {
        assert!(slowest <= 3.0, "reached the booth at {slowest} mph");
    }
    // Driving on under speed control again, not on the bare accelerator:
    // left paused at the booth, the truck ran 83 in a 50 work zone and drew
    // a ticket (agent drive, 2026-11-28).
    let controlled = harness.read_drive(|d| d.cruise_mph.is_some() || d.keeper_mph.is_some());
    assert!(
        controlled,
        "speed control still paused past the booth: {said}"
    );
    assert!(!harness.state_is::<BorderClearanceState>());
    (harness, booth)
}

#[test]
fn a_slept_shift_jump_ends_short_of_the_customs_booth() {
    let mut harness = on_the_road(NORTH, 3.0);
    let booth = booth_named(&harness, HUNTINGDON);
    let line = line_into(&harness, "British Columbia");
    hand_over(&mut harness, TeamMode::SleepThrough);
    let (jump, at) = harness.with_drive(|d, ctx| (d.sleep_ahead(ctx), d.trip.position_mi));
    let jump = jump.expect("the open road to the border passes as time");
    assert!(jump.miles > 50.0, "{jump:?}");
    // Short of where the booth's stop starts braking, and of the line.
    assert!(
        at < booth.at_mi - 2.0,
        "the jump ended at {at}, booth at {}",
        booth.at_mi
    );
    assert!(at < line, "the jump ended at {at}, past the line at {line}");
    assert!(co_driver_at_wheel(&harness));
    assert_eq!(rules(&harness), (HosRules::Us, HosRules::Us));
}

#[test]
fn a_slept_shift_jump_never_crosses_a_change_of_hours_rules() {
    // British Columbia into the Yukon: north of 60, no booth.
    let cities = ["fort_nelson_bc_ca", "watson_lake_yt_ca"];
    let mut harness = on_the_road(&cities, 3.0);
    let line = line_into(&harness, "Yukon");
    assert!(harness.read_drive(|d| d.trip.border_booths.is_empty()));
    hand_over(&mut harness, TeamMode::SleepThrough);
    let (jump, at, state) = harness.with_drive(|d, ctx| {
        (
            d.sleep_ahead(ctx),
            d.trip.position_mi,
            d.trip.state_at(None),
        )
    });
    assert!(jump.is_some(), "the road to the line passes as time");
    assert!(at < line, "jumped to {at}, past the Yukon line at {line}");
    assert_eq!(state, "British Columbia");
}

#[test]
fn a_cleared_booth_no_longer_holds_the_jump() {
    // Just past Huntingdon, cleared: the next jump runs on into British
    // Columbia.
    let mut harness = on_the_road(NORTH, 3.0);
    let booth = booth_named(&harness, HUNTINGDON);
    let start = booth.at_mi + 0.5;
    let key = format!("border-booth:{}", booth.key);
    harness.with_drive(move |d, _| {
        d.enforcement_events.insert(key);
        d.trip.position_mi = start;
        d.enforcement_prev_mi = start;
    });
    // The frame sets both logbooks to the rules of the place.
    run_until(&mut harness, 2, |_| false);
    hand_over(&mut harness, TeamMode::SleepThrough);
    let (jump, at) = harness.with_drive(|d, ctx| (d.sleep_ahead(ctx), d.trip.position_mi));
    let jump = jump.expect("the road past the booth passes as time");
    assert!(jump.miles > 5.0 && at > start + 5.0, "{jump:?} to {at}");
}

#[test]
fn what_changed_at_a_line_waits_for_the_player_to_have_the_wheel() {
    // Agent drive, 2026-11-28: the player cleared customs at a booth on the
    // line, handed the wheel straight back, and the units and hours rules
    // lines were spoken into the co-driver's silent road.
    let cities = ["fort_nelson_bc_ca", "watson_lake_yt_ca"];
    let mut harness = on_the_road(&cities, 3.0);
    hand_over(&mut harness, TeamMode::RideAlong);
    let rules_line = hours_rules_line(HosRules::CanadaNorth60);
    harness.with_drive(move |d, ctx| {
        d.queue_crossing_call("Crossing into Yukon.", None, None);
        d.queue_after_crossing(
            AfterCrossing::HoursRules,
            rules_line,
            Some(SpeechCategory::Status),
        );
        d.update_after_crossing(ctx, DT);
        d.update_after_crossing(ctx, DT);
    });
    let waiting: Vec<AfterCrossing> =
        harness.read_drive(|d| d.after_crossing.iter().map(|line| line.kind).collect());
    assert_eq!(waiting, vec![AfterCrossing::HoursRules]);
    assert!(!harness.transcript_text().contains(rules_line));
    harness.with_drive(|d, ctx| {
        d.take_wheel_back(ctx);
    });
    assert!(
        run_until(&mut harness, 1200, |h| h
            .transcript_text()
            .contains(rules_line)),
        "{}",
        harness.transcript_text()
    );
}

/// The truck stopped at `booth` with the co-driver at the wheel, and the
/// clearance opened as the booth's stop opens it. Returns the game minutes
/// it took and the co-driver's on-duty minutes it added.
fn co_driver_clears_at_the_booth(harness: &mut PlaytestHarness, booth: &BorderBooth) -> (f64, f64) {
    hand_over(harness, TeamMode::RideAlong);
    let booth = booth.clone();
    harness.with_drive(move |d, ctx| {
        d.trip.position_mi = booth.at_mi;
        d.enforcement_prev_mi = booth.at_mi;
        d.truck_mut().velocity_mps = 0.0;
        let minutes = d.trip.game_minutes;
        let duty = ctx.profile.as_ref().expect("a career").hos.duty_min;
        d.open_border_clearance(ctx, booth);
        (
            d.trip.game_minutes - minutes,
            ctx.profile.as_ref().expect("a career").hos.duty_min - duty,
        )
    })
}

#[test]
fn the_co_driver_clears_customs_with_no_menu_and_one_note() {
    let mut harness = on_the_road(NORTH, 3.0);
    let booth = booth_named(&harness, HUNTINGDON);
    let (minutes, duty) = co_driver_clears_at_the_booth(&mut harness, &booth);
    let name = co_driver_name(&harness);
    harness.expect_state::<DrivingState>("no clearance menu for the co-driver");
    assert!(co_driver_at_wheel(&harness), "no hand-back for customs");
    assert!((minutes - BORDER_PRIMARY_MIN).abs() < 0.01, "{minutes}");
    assert!(
        (duty - BORDER_PRIMARY_MIN).abs() < 0.01,
        "on the co-driver's logbook: {duty}"
    );
    let note = format!("{name} cleared customs into Canada at {HUNTINGDON}.");
    let said = harness.transcript_text();
    assert!(said.contains(&note), "the rider hears the note: {said}");
    assert_eq!(wake_notes(&harness), vec![note]);
    assert_no_booth_lines(&said);
    // Stopped at the booth on the brake the co-driver releases to drive on.
    assert!(harness.read_drive(|d| d.truck().parking_brake));
    // The crossing is the co-driver's, not the player's to earn.
    let profile = harness.app.ctx.profile.as_ref().expect("a career");
    assert!(!profile.achievements.iter().any(|a| a == "first_customs"));
}

#[test]
fn a_co_driver_who_reaches_the_booth_moving_stops_there_and_clears() {
    // Never a run: no chase, no penalty, the same clearance.
    let mut harness = on_the_road(NORTH, 3.0);
    let booth = booth_named(&harness, HUNTINGDON);
    hand_over(&mut harness, TeamMode::RideAlong);
    let name = co_driver_name(&harness);
    let at = booth.at_mi;
    let handled = harness.with_drive(move |d, ctx| {
        d.trip.position_mi = at + 0.01;
        d.truck_mut().velocity_mps = 55.0 * MPS_PER_MPH;
        d.check_border_booth_crossing(ctx, at - 0.01)
    });
    assert!(handled);
    let said = harness.transcript_text();
    assert!(
        said.contains(&format!(
            "{name} cleared customs into Canada at {HUNTINGDON}."
        )),
        "{said}"
    );
    assert_no_booth_lines(&said);
    let (mph, chased) = harness.read_drive(|d| (d.truck().speed_mph(), d.pull_over.is_some()));
    assert!(mph < 0.1 && !chased, "{mph} mph, chased {chased}");
    assert!(co_driver_at_wheel(&harness));
}

#[test]
fn a_secondary_referral_charges_its_minutes_and_says_so() {
    let mut harness = on_the_road(NORTH, 3.0);
    referred_at_every_booth(&mut harness, true);
    let booth = booth_named(&harness, HUNTINGDON);
    let (minutes, duty) = co_driver_clears_at_the_booth(&mut harness, &booth);
    let name = co_driver_name(&harness);
    let expected = BORDER_PRIMARY_MIN + BORDER_SECONDARY_EXTRA_MIN;
    assert!((minutes - expected).abs() < 0.01, "{minutes}");
    assert!((duty - expected).abs() < 0.01, "{duty}");
    let note = format!(
        "{name} cleared customs into Canada at {HUNTINGDON} after a {:.0}-minute secondary inspection.",
        BORDER_SECONDARY_EXTRA_MIN
    );
    assert_eq!(wake_notes(&harness), vec![note]);
}

#[test]
fn the_solo_clearance_is_unchanged() {
    let mut harness = on_the_road(NORTH, 3.0);
    let booth = booth_named(&harness, HUNTINGDON);
    let minutes = harness.with_drive(move |d, ctx| {
        d.trip.position_mi = booth.at_mi;
        d.truck_mut().velocity_mps = 0.0;
        let minutes = d.trip.game_minutes;
        d.open_border_clearance(ctx, booth);
        minutes
    });
    harness.expect_state::<BorderClearanceState>("the officer's questions");
    harness.select_menu_item(CLEAR_ROW);
    harness.expect_state::<DrivingState>("cleared at Huntingdon");
    let said = harness.transcript_text();
    assert!(
        said.contains("The officer asks for your passport"),
        "{said}"
    );
    assert!(said.contains("Cleared. Welcome to Canada."), "{said}");
    let spent = harness.read_drive(|d| d.trip.game_minutes) - minutes;
    assert!((spent - BORDER_PRIMARY_MIN).abs() < 0.01, "{spent}");
    let profile = harness.app.ctx.profile.as_ref().expect("a career");
    assert!(profile.achievements.iter().any(|a| a == "first_customs"));
}

#[test]
#[cfg_attr(
    not(feature = "slow-tests"),
    ignore = "drives the last miles to the border and through customs"
)]
fn a_slept_shift_clears_customs_and_drives_on() {
    let (mut harness, _) = co_driver_drives_through(
        NORTH,
        "British Columbia",
        HUNTINGDON,
        TeamMode::SleepThrough,
        8.0,
    );
    let name = co_driver_name(&harness);
    let note = format!("{name} cleared customs into Canada at {HUNTINGDON}.");
    assert_eq!(wake_notes(&harness), vec![note.clone()]);
    let said = harness.transcript_text();
    assert_no_booth_lines(&said);
    assert!(
        !said.contains(KM_LINE) && !said.contains(CA_RULES_LINE),
        "{said}"
    );
    assert_eq!(
        rules(&harness),
        (HosRules::CanadaSouth60, HosRules::CanadaSouth60)
    );
    let reason = harness.read_drive(|d| d.team.as_ref().and_then(|t| t.last_handback.clone()));
    assert_eq!(reason, None, "no hand-back for customs");
    // The shift ends: the wake summary carries the note, and the lines the
    // line carried follow once the player drives.
    harness.with_drive(|d, ctx| d.co_driver_summon(ctx, "rested"));
    run_until(&mut harness, 60 * 60, |h| {
        let said = h.transcript_text();
        said.contains(KM_LINE) && said.contains(CA_RULES_LINE)
    });
    let said = harness.transcript_text();
    let summary = said.find("You slept").unwrap_or_else(|| panic!("{said}"));
    let heard = said.find(&note).unwrap_or_else(|| panic!("{said}"));
    assert!(summary < heard, "{said}");
    for line in [KM_LINE, CA_RULES_LINE] {
        assert!(said.contains(line), "missing {line:?}: {said}");
    }
}

#[test]
#[cfg_attr(
    not(feature = "slow-tests"),
    ignore = "drives the last miles to the border and through customs"
)]
fn riding_along_the_player_hears_the_co_driver_clear_customs() {
    let (mut harness, _) = co_driver_drives_through(
        NORTH,
        "British Columbia",
        HUNTINGDON,
        TeamMode::RideAlong,
        5.0,
    );
    let name = co_driver_name(&harness);
    let said = harness.transcript_text();
    assert!(
        said.contains(&format!(
            "{name} cleared customs into Canada at {HUNTINGDON}."
        )),
        "{said}"
    );
    assert_no_booth_lines(&said);
    assert!(!said.contains(CA_RULES_LINE), "{said}");
    harness.with_drive(|d, ctx| {
        d.take_wheel_back(ctx);
    });
    assert!(
        run_until(&mut harness, 60 * 60, |h| h
            .transcript_text()
            .contains(CA_RULES_LINE)),
        "{}",
        harness.transcript_text()
    );
}

#[test]
#[cfg_attr(
    not(feature = "slow-tests"),
    ignore = "drives the last miles to the border and through customs"
)]
fn southbound_the_co_driver_clears_customs_into_the_united_states() {
    let cities = [
        "chilliwack_bc_ca",
        "abbotsford_bc_ca",
        "bellingham_wa_us",
        "everett_wa_us",
    ];
    let probe = on_the_road(&cities, 3.0);
    let booth_name = probe.read_drive(|d| {
        d.trip
            .border_booths
            .iter()
            .find(|booth| booth.entering_country == "US")
            .map(|booth| booth.name.clone())
            .expect("a booth into the United States")
    });
    drop(probe);
    let (harness, _) = co_driver_drives_through(
        &cities,
        "Washington",
        &booth_name,
        TeamMode::SleepThrough,
        8.0,
    );
    let name = co_driver_name(&harness);
    assert_eq!(
        wake_notes(&harness),
        vec![format!(
            "{name} cleared customs into the United States at {booth_name}."
        )]
    );
    assert_no_booth_lines(&harness.transcript_text());
    assert_eq!(rules(&harness), (HosRules::Us, HosRules::Us));
}

/// A slept shift from well short of the line on a road where the booth
/// stands miles past it: the line and the booth are both driven live, the
/// booth is cleared, and the co-driver drives on.
fn slept_through_a_far_booth(cities: &[&str], into: &str, entering: &str) {
    let probe = on_the_road(cities, 3.0);
    let line = line_into(&probe, into);
    let entering = entering.to_string();
    let booth = probe.read_drive(move |d| {
        d.trip
            .border_booths
            .iter()
            .find(|booth| booth.entering_country == entering)
            .cloned()
            .expect("a booth")
    });
    drop(probe);
    assert!(
        booth.at_mi > line + 5.0,
        "{} at {} past {line}",
        booth.name,
        booth.at_mi
    );
    let lead = (line - 5.0).min(30.0);
    let (harness, _) =
        co_driver_drives_through(cities, into, &booth.name, TeamMode::SleepThrough, lead);
    let name = co_driver_name(&harness);
    let country = if booth.entering_country == "CA" {
        "Canada"
    } else {
        "the United States"
    };
    assert_eq!(
        wake_notes(&harness),
        vec![format!(
            "{name} cleared customs into {country} at {}.",
            booth.name
        )]
    );
    assert_no_booth_lines(&harness.transcript_text());
    let (driver, off) = rules(&harness);
    assert_eq!(driver, off);
    assert_eq!(driver.is_canadian(), booth.entering_country == "CA");
}

#[test]
#[cfg_attr(
    not(feature = "slow-tests"),
    ignore = "drives the miles between the Alaska line and Beaver Creek"
)]
fn a_slept_shift_clears_beaver_creek_miles_past_the_line() {
    slept_through_a_far_booth(&["tok_ak_us", "haines_junction_yt_ca"], "Yukon", "CA");
}

#[test]
#[cfg_attr(
    not(feature = "slow-tests"),
    ignore = "drives the miles between the line and Fraser"
)]
fn a_slept_shift_clears_fraser_miles_past_the_line() {
    slept_through_a_far_booth(
        &["skagway_ak_us", "whitehorse_yt_ca"],
        "British Columbia",
        "CA",
    );
}

#[test]
#[cfg_attr(
    not(feature = "slow-tests"),
    ignore = "drives the miles between the line and Skagway"
)]
fn a_slept_shift_clears_skagway_miles_past_the_line() {
    slept_through_a_far_booth(&["whitehorse_yt_ca", "skagway_ak_us"], "Alaska", "US");
}
