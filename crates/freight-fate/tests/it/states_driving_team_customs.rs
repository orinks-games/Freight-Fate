//! The co-driver never crosses a border (owner ruling, 2026-11-28). Short
//! of the line into a customs booth's country they stop the truck and hand
//! the wheel back, so the player crosses, hears the new units and hours
//! rules and clears customs as on a solo crossing. A slept shift's jump
//! ends short of that point, and short of any line where the hours rules
//! change. Agent drive, 2026-11-28: a team load out of Seattle handed over
//! 5 miles before Abbotsford-Huntingdon woke 531 miles into British
//! Columbia with no stop and no clearance.

use ff_core::sim::hos::HosRules;
use ff_core::sim::trip_models::BorderBooth;
use ff_core::sim::weather::WeatherKind;
use ff_core::units::UnitsMode;
use freight_fate::playtest::{PlaytestHarness, RouteSetup};
use freight_fate::states::driving::DrivingState;
use freight_fate::states::driving_border::{border_secondary_referral, BorderClearanceState};
use freight_fate::states::driving_team::TeamMode;

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
    // A seed that sends the truck through primary at every booth.
    let keys: Vec<String> =
        harness.read_drive(|d| d.trip.border_booths.iter().map(|b| b.key.clone()).collect());
    let seed = (0..200)
        .find(|seed| {
            keys.iter()
                .all(|key| !border_secondary_referral(*seed, key))
        })
        .expect("a seed with no referral");
    harness.with_drive(move |d, _| d.trip_seed = seed);
    harness
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

#[test]
fn a_slept_shift_jump_ends_short_of_the_customs_handback() {
    let mut harness = on_the_road(NORTH, 3.0);
    let booth = booth_named(&harness, HUNTINGDON);
    let line = line_into(&harness, "British Columbia");
    hand_over(&mut harness, TeamMode::SleepThrough);
    let (jump, at, stop_by) = harness.with_drive(|d, ctx| {
        let jump = d.sleep_ahead(ctx);
        (jump, d.trip.position_mi, d.customs_handback_mi(ctx))
    });
    let jump = jump.expect("the open road to the border passes as time");
    assert!(jump.miles > 50.0, "{jump:?}");
    let stop_by = stop_by.expect("Huntingdon is still ahead");
    assert!(
        stop_by < line && stop_by < booth.at_mi,
        "{stop_by} {line} {}",
        booth.at_mi
    );
    assert!(at < stop_by, "the jump ended at {at}, past {stop_by}");
    assert!(co_driver_at_wheel(&harness), "nothing handed back yet");
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
fn a_co_driver_who_reaches_the_booth_hands_back_before_the_officer_speaks() {
    let mut harness = on_the_road(NORTH, 3.0);
    let booth = booth_named(&harness, HUNTINGDON);
    hand_over(&mut harness, TeamMode::RideAlong);
    let name = co_driver_name(&harness);
    harness.with_drive(move |d, ctx| {
        d.trip.position_mi = booth.at_mi;
        d.truck_mut().velocity_mps = 0.0;
        d.open_border_clearance(ctx, booth);
    });
    harness.expect_state::<BorderClearanceState>("the clearance after the handback");
    assert!(!co_driver_at_wheel(&harness));
    let said = harness.transcript_text();
    let summon = said
        .find(&format!("{name} has the truck at the customs booth."))
        .unwrap_or_else(|| panic!("{said}"));
    let officer = said
        .find("The officer asks for your passport")
        .unwrap_or_else(|| panic!("{said}"));
    assert!(summon < officer, "{said}");
    let reason = harness.read_drive(|d| d.team.as_ref().and_then(|t| t.last_handback.clone()));
    assert_eq!(reason.as_deref(), Some("customs booth"));
}

/// The co-driver drives up to the border live in `mode` and hands back
/// stopped short of the line; the player then drives across and clears
/// customs on All assists. Returns the transcript.
fn hands_back_and_player_clears(mode: TeamMode, lead_mi: f64) -> String {
    let mut harness = on_the_road(NORTH, 3.0);
    let booth = booth_named(&harness, HUNTINGDON);
    let line = line_into(&harness, "British Columbia");
    harness.with_drive(move |d, _| {
        d.trip.position_mi = line - lead_mi;
        d.enforcement_prev_mi = d.trip.position_mi;
    });
    hand_over(&mut harness, mode);
    let name = co_driver_name(&harness);
    let handed_back = run_until(&mut harness, 60 * 60 * 20, |h| !co_driver_at_wheel(h));
    let said = harness.transcript_text();
    assert!(handed_back, "the co-driver never handed back: {said}");
    let (at, mph) = harness.read_drive(|d| (d.trip.position_mi, d.truck().speed_mph()));
    assert!(mph < 0.6, "handed back at {mph} mph");
    assert!(at < line, "handed back at {at}, past the line at {line}");
    assert!(
        said.contains(&format!(
            "{name} stopped short of the border and hands you the wheel for customs."
        )),
        "{said}"
    );
    assert_eq!(
        said.contains("You slept"),
        mode == TeamMode::SleepThrough,
        "{said}"
    );
    assert!(!said.contains("Crossing into British Columbia"), "{said}");
    assert_eq!(rules(&harness), (HosRules::Us, HosRules::Us));
    let reason = harness.read_drive(|d| d.team.as_ref().and_then(|t| t.last_handback.clone()));
    assert_eq!(reason.as_deref(), Some("customs"));

    // The player drives on, feet off on All assists.
    harness.with_drive(|d, ctx| {
        d.truck_mut().parking_brake = false;
        d.truck_mut().velocity_mps = 30.0 * MPS_PER_MPH;
        d.engage_cruise(ctx, 30.0, false);
    });
    let cleared = run_until(&mut harness, 60 * 60 * 20, |h| {
        h.state_is::<BorderClearanceState>()
    });
    let said = harness.transcript_text();
    assert!(cleared, "the clearance never opened: {said}");
    harness.select_menu_item(CLEAR_ROW);
    harness.expect_state::<DrivingState>("cleared at Huntingdon");
    // Past the booth, on the road again: the lines the crossing carries,
    // and both logbooks under Canadian rules.
    harness.with_drive(|d, ctx| {
        d.truck_mut().parking_brake = false;
        d.truck_mut().velocity_mps = 30.0 * MPS_PER_MPH;
        d.engage_cruise(ctx, 30.0, false);
    });
    run_until(&mut harness, 60 * 30, |_| false);
    let said = harness.transcript_text();
    for line in [
        "Every truck stops at the customs booth.",
        "The officer asks for your passport",
        "Cleared. Welcome to Canada.",
        "Speeds and distances are in kilometers now.",
        "Canadian hours rules: 13 hours of driving and 14 on duty in a 16-hour window.",
    ] {
        assert!(said.contains(line), "missing {line:?}: {said}");
    }
    assert!(!said.contains("stopped the truck for running"), "{said}");
    assert_eq!(
        rules(&harness),
        (HosRules::CanadaSouth60, HosRules::CanadaSouth60)
    );
    // Handed over again past the booth: the co-driver drives on.
    hand_over(&mut harness, TeamMode::RideAlong);
    run_until(&mut harness, 600, |_| false);
    assert!(
        co_driver_at_wheel(&harness),
        "summoned again for {}",
        booth.name
    );
    said
}

#[test]
#[cfg_attr(
    not(feature = "slow-tests"),
    ignore = "drives the last miles to the border and through customs"
)]
fn a_slept_shift_hands_back_short_of_the_border_and_the_player_clears_customs() {
    hands_back_and_player_clears(TeamMode::SleepThrough, 8.0);
}

#[test]
#[cfg_attr(
    not(feature = "slow-tests"),
    ignore = "drives the last miles to the border and through customs"
)]
fn a_ride_along_hands_back_short_of_the_border_and_the_player_clears_customs() {
    hands_back_and_player_clears(TeamMode::RideAlong, 5.0);
}

#[test]
#[cfg_attr(
    not(feature = "slow-tests"),
    ignore = "drives the last miles to the border"
)]
fn southbound_the_co_driver_hands_back_short_of_the_line_into_the_united_states() {
    let cities = [
        "chilliwack_bc_ca",
        "abbotsford_bc_ca",
        "bellingham_wa_us",
        "everett_wa_us",
    ];
    let mut harness = on_the_road(&cities, 3.0);
    let line = line_into(&harness, "Washington");
    harness.with_drive(move |d, _| {
        d.trip.position_mi = line - 8.0;
        d.enforcement_prev_mi = d.trip.position_mi;
    });
    hand_over(&mut harness, TeamMode::SleepThrough);
    let name = co_driver_name(&harness);
    let handed_back = run_until(&mut harness, 60 * 60 * 20, |h| !co_driver_at_wheel(h));
    let said = harness.transcript_text();
    assert!(handed_back, "{said}");
    let at = harness.read_drive(|d| d.trip.position_mi);
    assert!(at < line, "handed back at {at}, past the line at {line}");
    assert!(
        said.contains(&format!(
            "{name} stopped short of the border and hands you the wheel for customs."
        )),
        "{said}"
    );
}
