//! Real weather, the weather tablet, the limit-drop grace and the overspeed
//! alert (port of `tests/test_driving_cruise_weather.py`, lines 2427-2982).
//!
//! Fourth of the split; see `transcript_driving_cruise_weather.rs` for why the
//! Python file is split and `transcript_cruise_support` for what replaced each
//! monkeypatch.

use std::cell::RefCell;
use std::rc::Rc;

use ff_core::sim::enforcement_observe::OBSERVE_LEEWAY_MPH;
use ff_core::sim::trip_models::TripEventKind;
use ff_core::sim::weather::{WeatherKind, WeatherProvider};
use freight_fate::playtest::harness::PlaytestHarness;
use freight_fate::states::base::Key;
use freight_fate::states::driving_core::{ACC_LIMIT_OFFSET_MPH, OVERSPEED_WARN_MPH};

use crate::transcript_cruise_support::*;

// -- real weather ---------------------------------------------------------------

/// `_FakeWeatherProvider`: returns `kind` for any city; `None` models data not
/// yet fetched.
struct FakeWeatherProvider {
    kind: Option<WeatherKind>,
}

impl WeatherProvider for FakeWeatherProvider {
    fn request(&mut self, _city: &str, _lat: f64, _lon: f64) {}
    fn get(&mut self, _city: &str) -> Option<WeatherKind> {
        self.kind
    }
    // A mild station reading, so the real-calendar season model (read off
    // the wall clock) never turns this rain to ice on a cold night.
    fn get_temperature(&mut self, _city: &str) -> Option<f64> {
        Some(15.0)
    }
}

/// Install a fake provider on the live drive.
///
/// Python patched `ctx.real_weather_provider` before the drive was built, so
/// `DrivingState` picked the fake up through the same wiring the real provider
/// uses. `GameContext::real_weather_provider` hands back a concrete
/// `RealWeatherProvider`, so there is no fake to hand it here; instead the
/// wiring is ASSERTED (the setting really did give the trip a provider) and
/// then the provider is swapped for the fake.
fn install_provider(harness: &mut PlaytestHarness, provider: Box<dyn WeatherProvider>) {
    harness.with_drive(move |d, _| {
        assert!(
            d.weather().provider.is_some(),
            "real_weather was on, so the drive must have wired a live provider"
        );
        d.weather_mut().provider = Some(provider);
        d.weather_mut().live = false;
        d.weather_mut().current = WeatherKind::Clear;
    });
}

/// A delivery on a corridor long enough to cross several weather cells, with
/// real weather on before the drive is built (that is what wires a provider
/// into the trip's `WeatherSystem`).
fn a_live_weather_drive(name: &str) -> PlaytestHarness {
    use freight_fate::playtest::harness::RouteSetup;

    let mut harness = PlaytestHarness::new();
    harness.app.ctx.settings.real_weather = true;
    harness.start_route("Chicago", "Indianapolis", RouteSetup::seeded(0).named(name));
    harness.with_drive(|d, _| {
        d.departure_checked = true;
        d.truck_mut().set_air_ready(false);
    });
    harness
}

#[test]
fn test_real_weather_starts_clear_with_no_simulated_warmup() {
    // Regression: with real weather enabled, a drive starts neutral (clear)
    // and holds until live data arrives, instead of showing a provisional
    // simulated condition. So no momentary simulated rain can unlock an
    // achievement.
    let mut harness = a_live_weather_drive("Live Warmup");
    install_provider(&mut harness, Box::new(FakeWeatherProvider { kind: None }));
    assert_eq!(
        harness.read_drive(|d| d.weather().current),
        WeatherKind::Clear
    );
    assert!(!harness.read_drive(|d| d.weather().live));

    // While the fetch is still pending, weather holds clear -- no simulated
    // transitions, so no weather achievement fires.
    frames(&mut harness, 10, DT);
    assert_eq!(
        harness.read_drive(|d| d.weather().current),
        WeatherKind::Clear
    );
    assert!(!harness
        .app
        .ctx
        .profile
        .as_ref()
        .expect("a career")
        .achievements
        .iter()
        .any(|a| a == "rain_driver"));
}

#[test]
fn test_live_weather_calendar_off_does_not_announce_simulated_forecast_while_loading() {
    // V must not invent a forecast while the selected live source is loading.
    //
    // The calendar toggle changes seasonal plausibility, not the weather
    // source.
    let mut harness = a_live_weather_drive("Calendar Off");
    harness.app.ctx.settings.live_weather_controls_calendar = false;
    install_provider(&mut harness, Box::new(FakeWeatherProvider { kind: None }));
    harness.clear_speech();
    harness.with_drive(|d, ctx| d.speak_weather(ctx));
    let said = last(&harness);
    assert!(
        said.contains("Live weather is loading for your current route position"),
        "{said}"
    );
    assert!(!said.contains("Ahead:"), "{said}");
}

#[test]
fn test_real_weather_applies_and_awards_live_condition() {
    // Once live conditions arrive, they take over from clear and award their
    // achievement -- e.g. genuine live rain unlocks the rain achievement.
    let mut harness = a_live_weather_drive("Live Rain");
    install_provider(
        &mut harness,
        Box::new(FakeWeatherProvider {
            kind: Some(WeatherKind::Rain),
        }),
    );
    frames(&mut harness, 5, DT);
    assert!(harness.read_drive(|d| d.weather().live));
    assert_eq!(
        harness.read_drive(|d| d.weather().current),
        WeatherKind::Rain
    );
    assert!(harness
        .app
        .ctx
        .profile
        .as_ref()
        .expect("a career")
        .achievements
        .iter()
        .any(|a| a == "rain_driver"));
}

/// The recording provider `test_v_reports_live_weather_...` needs: a distinct
/// condition per route cell, and the keys it was asked for.
#[derive(Default)]
struct SpatialLog {
    requests: Vec<(String, f64, f64)>,
    conditions: Vec<(String, WeatherKind)>,
}

struct SpatialProvider(Rc<RefCell<SpatialLog>>);

impl WeatherProvider for SpatialProvider {
    fn request(&mut self, city: &str, lat: f64, lon: f64) {
        let mut log = self.0.borrow_mut();
        if log.conditions.iter().any(|(k, _)| k == city) {
            return;
        }
        let kinds = [
            WeatherKind::Clear,
            WeatherKind::Rain,
            WeatherKind::HeavyRain,
        ];
        let kind = kinds[log.conditions.len().min(2)];
        log.conditions.push((city.to_string(), kind));
        log.requests.push((city.to_string(), lat, lon));
    }
    fn get(&mut self, city: &str) -> Option<WeatherKind> {
        self.0
            .borrow()
            .conditions
            .iter()
            .find(|(k, _)| k == city)
            .map(|(_, kind)| *kind)
    }
    fn stale(&mut self, _city: &str) -> bool {
        false
    }
    fn unavailable(&mut self, _city: &str) -> bool {
        false
    }
    // A mild station reading, so the real-calendar season model (read off
    // the wall clock) never turns this rain to ice on a cold night.
    fn get_temperature(&mut self, _city: &str) -> Option<f64> {
        Some(15.0)
    }
}

#[test]
fn test_v_reports_live_weather_from_multiple_current_route_positions() {
    // Real V-key reports follow stable route cells instead of the destination.
    let mut harness = a_live_weather_drive("Spatial V");
    let log = Rc::new(RefCell::new(SpatialLog::default()));
    install_provider(&mut harness, Box::new(SpatialProvider(Rc::clone(&log))));
    // Python rebuilt the trip on the Chicago-Indianapolis corridor so the
    // route crosses several weather cells. The bench road is one leg between
    // one city and itself, so keep the drive's real dispatched route here and
    // just walk it; what the case is about is that the CELL follows the truck.
    let total = harness.read_drive(|d| d.trip.total_miles());
    assert!(
        total >= 80.0,
        "this case needs a route long enough to cross three weather cells"
    );
    log.borrow_mut().requests.clear();
    log.borrow_mut().conditions.clear();
    harness.with_drive(|d, _| {
        d.weather_mut().live = false;
        d.weather_mut().live_raw = None;
        d.weather_mut().live_city = None;
        d.weather_mut().live_kind = None;
    });

    for (position, condition) in [(0.0, "clear"), (40.0, "rain"), (80.0, "heavy rain")] {
        harness.with_drive(move |d, _| {
            d.trip.position_mi = position;
            d.trip.update(0.0);
        });
        press(&mut harness, Key::V, Some('v'));
        let said = last(&harness);
        assert!(
            said.starts_with(&format!("Live weather: {condition}")),
            "at {position}: {said}"
        );
    }
    let keys: std::collections::HashSet<String> = log
        .borrow()
        .requests
        .iter()
        .map(|(key, _, _)| key.clone())
        .collect();
    assert_eq!(keys.len(), 3, "{keys:?}");
    let destination =
        harness.read_drive(|d| d.trip.route.cities.last().cloned().unwrap_or_default());
    let city = harness
        .app
        .ctx
        .world
        .cities
        .get(&destination)
        .expect("the destination city");
    let first = log.borrow().requests[0].clone();
    assert!(
        (first.1, first.2) != (city.lat, city.lon),
        "the first cell was the destination, not the truck"
    );
    assert!(harness.state_is::<freight_fate::states::driving::DrivingState>());
}

// -- limit-drop grace and the overspeed alert -----------------------------------

#[test]
fn test_limit_drop_earns_braking_grace() {
    // A posted-limit drop gives braking time before strikes accrue -- real
    // enforcement tickets sustained disregard, not the transition (owner
    // struck 0.6 s after the 65-to-50 step in the Queen Creek canyon). Staying
    // on the throttle forfeits the grace.
    //
    // Python swapped `trip.speed_limit_at` three times. Here the three
    // postings are baked onto the bench road at the miles the rolls below
    // reach them: `roll` advances 0.01 miles per step before reading the
    // limit, so the first drop sits just past the seeding call and the second
    // just past the hundredth step.
    let mut harness = start_drive("Drop Grace");
    let p0 = harness.read_drive(|d| d.trip.position_mi);
    harness.with_drive(move |d, _| {
        bench_road_with(
            d,
            &[(0.0, 65.0), (p0 + 0.005, 50.0), (p0 + 1.005, 35.0)],
            0.0,
            1.0,
        );
        d.trip.position_mi = p0;
        d.trip.set_patrols(Vec::new());
        d.truck_mut().velocity_mps = 65.0 * MPS_PER_MPH;
        d.truck_mut().throttle = 0.0;
    });
    speeding_step(&mut harness, 0.1, false); // seed the previous limit

    fn roll(harness: &mut PlaytestHarness, steps: usize, accelerator_held: bool) {
        for _ in 0..steps {
            harness.advance_clock(0.1);
            harness.with_drive(move |d, ctx| {
                d.trip.position_mi += 0.01;
                d.update_speeding(ctx, 0.1, accelerator_held);
                d.update_enforcement_watch(ctx, 0.1);
            });
        }
    }

    roll(&mut harness, 70, false); // 7 s: inside the (65-50)/2 = 7.5 s grace
                                   // The transition itself is not a speed.
    assert!(approx(harness.read_drive(|d| d.over_limit_mi), 0.0));

    // Grace spent, still 15 over with no brake: the distance accrues.
    roll(&mut harness, 30, false);
    assert!(harness.read_drive(|d| d.over_limit_mi) > 0.0);

    // Second drop with the driver still on the throttle. For the first
    // ROUTE-budget seconds the zone-entry line may not have spoken yet, so the
    // held throttle is not yet disregard and the grace holds (the R1
    // demotion's coupled invariant)...
    harness.with_drive(|d, _| {
        d.truck_mut().throttle = 1.0;
        d.over_limit_mi = 0.0;
    });
    roll(&mut harness, 5, true); // 0.5 s: inside the speech-latency window
    assert!(approx(harness.read_drive(|d| d.over_limit_mi), 0.0));

    // ...but once the line has had time to speak, staying on the throttle
    // forfeits the grace and the distance accrues.
    roll(&mut harness, 10, true);
    assert!(harness.read_drive(|d| d.over_limit_mi) > 0.0);
}

#[test]
fn test_limit_drop_grace_uses_released_key_not_smoothed_throttle() {
    // Releasing Up keeps grace even while applied throttle ramps down.
    //
    // Python swapped `trip.speed_limit_at` from 65 to 50 between the two
    // frames. Here both numbers are baked on the road and the truck is moved
    // onto the second one, which is the same change from the drive's side.
    let mut harness = start_drive("Released Key");
    harness.with_drive(|d, _| {
        bench_road_with(d, &[(0.0, 65.0), (1.0, 50.0)], 0.0, 1.0);
        d.trip.set_patrols(Vec::new());
        d.truck_mut().velocity_mps = 65.0 * MPS_PER_MPH;
        d.truck_mut().throttle = 1.0;
    });
    hold(&mut harness, &[Key::Up]);

    frame(&mut harness, DT);
    assert!(harness.read_drive(|d| d.truck().throttle) > 0.0);

    release_keys(&mut harness);
    harness.with_drive(|d, _| d.trip.position_mi = 1.5);
    frame(&mut harness, DT);

    assert!(harness.read_drive(|d| d.truck().throttle) > 0.0);
    assert!(harness.read_drive(|d| d.limit_drop_grace_s) > 0.0);
}

/// `driving._update_speeding(dt)` with the pacer's clock kept honest.
///
/// The Python file recorded ABOVE the event pacer, so it never met the
/// repeat window. At this seam an identical line inside
/// `EventSpeechPacer::REPEAT_WINDOW_S` of the last one is correctly
/// suppressed -- and a test that calls `update_speeding(0.1)` two hundred
/// times without moving the clock is telling the pacer that all of it
/// happened in the same instant. These steps are 0.1 s of the drive, so the
/// clock gets 0.1 s.
fn speeding_step(harness: &mut PlaytestHarness, dt: f64, accelerator_held: bool) {
    harness.advance_clock(dt);
    harness.with_drive(move |d, ctx| d.update_speeding(ctx, dt, accelerator_held));
}

/// How many overspeed chimes the audio backend was asked for.
fn chimes(log: &freight_fate::app::testing::AudioLog) -> usize {
    log.borrow()
        .played
        .iter()
        .filter(|(key, _, _)| key == "vehicle/overspeed_chime")
        .count()
}

#[test]
fn test_overspeed_warning_speaks_then_chimes_until_compliant() {
    // The dash overspeed alert: spoken once when armed, chiming on an interval
    // while over, disarmed by settling back under the limit -- and a fresh
    // episode speaks again.
    let mut harness = bench_drive("Overspeed Chime", 50.0, 0.0);
    harness.with_drive(|d, _| d.trip.set_patrols(Vec::new()));
    let log = harness.app.record_audio();
    harness.clear_speech();
    harness.with_drive(|d, _| {
        d.truck_mut().throttle = 0.3;
        // 58 in a 50: over the 7-over warn threshold, inside the 9-over strike
        // leeway -- the band where the dash still gets you back down for free.
        d.truck_mut().velocity_mps = 58.0 * MPS_PER_MPH;
    });

    speeding_step(&mut harness, 0.1, false);
    assert!(said_any(&harness, "Over the limit of"));
    assert_eq!(chimes(&log), 1);

    for _ in 0..52 {
        // past one repeat interval
        speeding_step(&mut harness, 0.1, false);
    }
    assert_eq!(chimes(&log), 2);
    assert_eq!(
        spoken(&harness)
            .iter()
            .filter(|e| e.contains("Over the limit of"))
            .count(),
        1
    ); // spoken once

    // Settling under the limit disarms; the next episode speaks again.
    harness.with_drive(|d, _| d.truck_mut().velocity_mps = 50.0 * MPS_PER_MPH);
    speeding_step(&mut harness, 0.1, false);
    harness.with_drive(|d, _| d.truck_mut().velocity_mps = 58.0 * MPS_PER_MPH);
    speeding_step(&mut harness, 0.1, false);
    assert_eq!(
        spoken(&harness)
            .iter()
            .filter(|e| e.contains("Over the limit of"))
            .count(),
        2
    );

    // Way over, the cadence escalates: at 25 over the ding runs about every
    // 1.5 seconds instead of every 5.
    harness.with_drive(|d, _| d.truck_mut().velocity_mps = 75.0 * MPS_PER_MPH);
    log.borrow_mut().played.clear();
    for _ in 0..40 {
        // 4 seconds
        speeding_step(&mut harness, 0.1, false);
    }
    assert!(chimes(&log) >= 2);
}

#[test]
fn test_overspeed_warning_stops_dinging_once_back_under_its_own_threshold() {
    // Slowing down must end the episode, not carry it down to the limit.
    //
    // The disarm was measured from the posted limit, six mph below the point
    // the alert arms at. So one honest trigger at nine over went on chiming at
    // six, five, four and three over while the driver was slowing -- speeds
    // the alert must never speak at, and exactly what a tester heard as "it
    // dings at five over" (Shane, 2026-08-15; reproduced in a logged
    // playtest).
    let mut harness = bench_drive("Overspeed Disarm", 50.0, 0.0);
    harness.with_drive(|d, _| d.trip.set_patrols(Vec::new()));
    let log = harness.app.record_audio();
    harness.clear_speech();
    harness.with_drive(|d, _| {
        d.truck_mut().throttle = 0.3;
        d.truck_mut().velocity_mps = 60.0 * MPS_PER_MPH; // 10 over: armed
    });
    speeding_step(&mut harness, 0.1, false);
    assert_eq!(chimes(&log), 1);

    // Backed off to five over -- under the threshold, still above the limit.
    // Nothing more may sound, however long it is held.
    harness.with_drive(|d, _| d.truck_mut().velocity_mps = 55.0 * MPS_PER_MPH);
    log.borrow_mut().played.clear();
    for _ in 0..200 {
        // 20 seconds, four repeat intervals
        speeding_step(&mut harness, 0.1, false);
    }
    let played = log.borrow().played.clone();
    assert_eq!(chimes(&log), 0, "{played:?}");

    // And the episode really ended: going back over speaks again.
    harness.with_drive(|d, _| d.truck_mut().velocity_mps = 60.0 * MPS_PER_MPH);
    speeding_step(&mut harness, 0.1, false);
    assert_eq!(
        spoken(&harness)
            .iter()
            .filter(|e| e.contains("Over the limit of"))
            .count(),
        2
    );
}

#[test]
fn test_adaptive_cruise_at_its_own_pace_never_arms_the_overspeed_warning() {
    // The bug the setting existed to work around.
    //
    // Predictive cruise holds ACC_LIMIT_OFFSET_MPH (5) over the posted limit
    // by design. The warning used to arm at exactly 5 over too, so the truck
    // chimed at the driver for the pace it had chosen itself, and the only fix
    // on offer was a setting to silence the whole alert. The threshold now
    // sits above cruise's pace: driving at, and a little past, the speed
    // cruise picks is silent, and no setting is needed to make it so.
    //
    // The threshold has to live in the gap, or one of the two failures is
    // back: chiming at cruise's own pace, or staying silent until the driver
    // is already ticketable.
    const { assert!(ACC_LIMIT_OFFSET_MPH < OVERSPEED_WARN_MPH) };
    const { assert!(OVERSPEED_WARN_MPH < OBSERVE_LEEWAY_MPH) };

    let mut harness = bench_drive("ACC Pace Silent", 50.0, 0.0);
    harness.with_drive(|d, _| d.trip.set_patrols(Vec::new()));
    let log = harness.app.record_audio();
    harness.clear_speech();
    harness.with_drive(|d, _| {
        d.truck_mut().throttle = 0.3;
        // Exactly the pace predictive cruise holds, plus a mile an hour of the
        // control-loop wobble a real grade produces.
        d.truck_mut().velocity_mps = (50.0 + ACC_LIMIT_OFFSET_MPH + 1.0) * MPS_PER_MPH;
    });
    for _ in 0..100 {
        // ten seconds of it
        speeding_step(&mut harness, 0.1, false);
    }
    assert_eq!(chimes(&log), 0);
    assert!(!said_any(&harness, "Over the limit of"));
}

// -- the V readout and the weather tablet ---------------------------------------
//
// Python read `tablet._weather_lines()` directly. That helper is private here,
// and the rows it builds ARE the tablet's menu items, so the tablet goes on the
// stack and the rows are read the way a player reaches them.

/// `StatefulProvider`: every answer a flag the test flips.
#[derive(Default)]
struct WeatherFlags {
    kind: Option<WeatherKind>,
    is_stale: bool,
    is_unavailable: bool,
    is_refreshing: bool,
    is_failed: bool,
}

struct StatefulProvider(Rc<RefCell<WeatherFlags>>);

impl WeatherProvider for StatefulProvider {
    fn request(&mut self, _city: &str, _lat: f64, _lon: f64) {}
    fn get(&mut self, _city: &str) -> Option<WeatherKind> {
        self.0.borrow().kind
    }
    fn stale(&mut self, _city: &str) -> bool {
        self.0.borrow().is_stale
    }
    fn unavailable(&mut self, _city: &str) -> bool {
        self.0.borrow().is_unavailable
    }
    fn refreshing(&mut self, _city: &str) -> bool {
        self.0.borrow().is_refreshing
    }
    fn refresh_failed(&mut self, _city: &str) -> bool {
        self.0.borrow().is_failed
    }
    fn observation_age_s(&mut self, _city: &str) -> Option<f64> {
        self.0.borrow().kind.map(|_| 12.0 * 60.0)
    }
    // A mild station reading, so the real-calendar season model (read off
    // the wall clock) never turns this rain to ice on a cold night.
    fn get_temperature(&mut self, _city: &str) -> Option<f64> {
        Some(15.0)
    }
}

/// `FreshOldProvider`: rain, twelve minutes old, and nothing else going on.
struct FreshOldProvider;

impl WeatherProvider for FreshOldProvider {
    fn request(&mut self, _city: &str, _lat: f64, _lon: f64) {}
    fn get(&mut self, _city: &str) -> Option<WeatherKind> {
        Some(WeatherKind::Rain)
    }
    fn observation_age_s(&mut self, _city: &str) -> Option<f64> {
        Some(12.0 * 60.0)
    }
    // A mild station reading, so the real-calendar season model (read off
    // the wall clock) never turns this rain to ice on a cold night.
    fn get_temperature(&mut self, _city: &str) -> Option<f64> {
        Some(15.0)
    }
}

/// Push the weather tablet over the drive and hand back its rows.
fn open_weather_tablet(harness: &mut PlaytestHarness) -> Vec<String> {
    use freight_fate::states::driving_menu_states::{DriveRef, DriverAppScreenState};

    let drive = harness.shared_driving().expect("a drive on the stack");
    let state = DriverAppScreenState::new(&mut harness.app.ctx, DriveRef::of(&drive), "weather");
    harness.app.ctx.push_state(state);
    harness.app.ctx.run_deferred();
    // The menu carries a "Back to Driver apps" row the Python
    // `_weather_lines()` never had; the weather lines are the rest.
    harness
        .menu_labels()
        .into_iter()
        .filter(|row| !row.starts_with("Back to"))
        .collect()
}

/// The rows of the tablet already on the stack.
///
/// Python's `tablet.items` were built once, at `enter()`, and every later
/// assertion read that same list; so does this.
fn tablet_rows(harness: &PlaytestHarness) -> Vec<String> {
    harness
        .menu_labels()
        .into_iter()
        .filter(|row| !row.starts_with("Back to"))
        .collect()
}

/// Step the open tablet one frame (`tablet.update(0.0)`).
fn tablet_update(harness: &mut PlaytestHarness) {
    let Some(state) = harness.app.ctx.state() else {
        return;
    };
    state.borrow_mut().update(&mut harness.app.ctx, 0.0);
    harness.app.ctx.run_deferred();
}

#[test]
fn test_v_distinguishes_loading_last_known_and_fallback() {
    let mut harness = a_live_weather_drive("V States");
    let flags = Rc::new(RefCell::new(WeatherFlags::default()));
    install_provider(&mut harness, Box::new(StatefulProvider(Rc::clone(&flags))));
    harness.clear_speech();

    harness.with_drive(|d, _| {
        d.trip.update(0.0);
    });
    press(&mut harness, Key::V, Some('v'));
    let said = last(&harness);
    assert!(
        said.starts_with("Live weather is loading for your current route position"),
        "{said}"
    );
    assert!(!said.contains("Ahead:"), "{said}");

    flags.borrow_mut().kind = Some(WeatherKind::HeavyRain);
    harness.with_drive(|d, _| {
        d.trip.update(0.0);
    });
    flags.borrow_mut().is_stale = true;
    press(&mut harness, Key::V, Some('v'));
    let said = last(&harness);
    assert!(
        said.starts_with("Last-known live weather: heavy rain"),
        "{said}"
    );
    assert!(!said.contains("minutes old"), "{said}");
    assert!(!said.to_lowercase().contains("updating"), "{said}");
    assert!(!said.contains("Ahead:"), "{said}");
    let status_weather = harness
        .with_drive(|d, ctx| d.status_lines(ctx))
        .into_iter()
        .find(|line| line.starts_with("Weather:"))
        .expect("a weather status line");
    assert!(
        status_weather.contains("Last-known live weather"),
        "{status_weather}"
    );

    let rows = open_weather_tablet(&mut harness);
    let heads: Vec<&str> = rows
        .iter()
        .map(|line| line.split_once(':').map(|(head, _)| head).unwrap_or(line))
        .collect();
    assert_eq!(
        heads,
        [
            "Weather source",
            "Observation age",
            "Conditions",
            "Safe speed guidance",
            "Forecast ahead",
        ]
    );
    assert!(rows[0]
        .starts_with("Weather source: Last-known live weather for your current route position"));
    assert_eq!(rows[1], "Observation age: 12 minutes old.");
    let selected = harness.focused_label().expect("a focused row");
    assert!(selected.starts_with("Weather source: Last-known live weather"));

    flags.borrow_mut().is_refreshing = true;
    press(&mut harness, Key::V, Some('v'));
    assert!(
        last(&harness).contains("Live weather is updating for your current location"),
        "{}",
        last(&harness)
    );
    flags.borrow_mut().is_refreshing = false;

    flags.borrow_mut().is_failed = true;
    harness.advance_clock(DT);
    tablet_update(&mut harness);
    assert!(
        said_any(&harness, "The latest live weather check failed"),
        "{:#?}",
        spoken(&harness)
    );
    assert!(harness.read_drive(|d| d.trip.weather_refresh_issue_announced));
    let duplicate_events = harness.with_drive(|d, _| d.trip.update(0.0));
    assert!(!duplicate_events.iter().any(|event| {
        event.kind == TripEventKind::WeatherChange
            && event.text().contains("latest live weather check failed")
    }));
    flags.borrow_mut().is_failed = false;

    flags.borrow_mut().kind = None;
    flags.borrow_mut().is_unavailable = true;
    harness.with_drive(|d, _| {
        d.trip.update(0.0);
    });
    press(&mut harness, Key::V, Some('v'));
    // The session has heard live weather, so an unavailable provider holds
    // last-known conditions -- simulated fallback never takes over mid-run
    // (owner ruling, 2026-08-08).
    assert!(
        last(&harness).starts_with("Last-known live weather"),
        "{}",
        last(&harness)
    );
    let rows = tablet_rows(&harness);
    assert!(
        rows[0].starts_with("Weather source: Last-known live weather"),
        "{rows:#?}"
    );
    harness.key(freight_fate::playtest::harness::key_event(
        Key::Return,
        None,
    ));
    assert!(
        last(&harness).starts_with("Weather source: Last-known live weather"),
        "{}",
        last(&harness)
    );
    assert_eq!(harness.focused_label().expect("a focused row"), selected);
}

#[test]
fn test_old_but_freshly_fetched_weather_is_live_across_v_status_and_tablet() {
    let mut harness = a_live_weather_drive("Fresh Old");
    install_provider(&mut harness, Box::new(FreshOldProvider));
    harness.clear_speech();
    harness.with_drive(|d, _| {
        d.trip.update(0.0);
    });
    press(&mut harness, Key::V, Some('v'));
    let said = last(&harness);
    assert!(said.starts_with("Live weather: rain"), "{said}");
    assert!(!said.contains("minutes old"), "{said}");
    assert!(!said.to_lowercase().contains("updating"), "{said}");

    let weather_status = harness
        .with_drive(|d, ctx| d.status_lines(ctx))
        .into_iter()
        .find(|line| line.starts_with("Weather:"))
        .expect("a weather status line");
    assert!(
        weather_status.contains("Live weather: rain"),
        "{weather_status}"
    );
    assert!(!weather_status.contains("minutes old"), "{weather_status}");
    assert!(
        !weather_status.to_lowercase().contains("updating"),
        "{weather_status}"
    );

    let rows = open_weather_tablet(&mut harness);
    assert!(rows[0].starts_with("Weather source: Live weather for your current route position"));
    assert_eq!(rows[1], "Observation age: 12 minutes old.");
    assert!(!rows.join(" ").to_lowercase().contains("updating"));
}

#[test]
fn test_persistent_weather_fallback_is_said_once_and_recovery_once() {
    // Agent drive, 2026-09-24: with the route cell's station failing, "Live
    // weather unavailable. Simulated weather in use." came back after every
    // failed retry, once a minute. A fallback that persists is said once;
    // live weather coming back is said once.
    use ff_core::sim::real_weather::{Observation, RealWeatherProvider, RETRY_AFTER_S};
    use std::sync::{mpsc, Arc, Mutex};
    use std::time::Duration;

    let mut harness = a_live_weather_drive("Fallback Once");
    let clock = Arc::new(Mutex::new(0.0_f64));
    let live = Arc::new(Mutex::new(false));
    let (started_tx, started_rx) = mpsc::channel::<()>();
    let (release_tx, release_rx) = mpsc::channel::<()>();
    let release_rx = Mutex::new(release_rx);
    let live_for_fetch = Arc::clone(&live);
    let clock_for_provider = Arc::clone(&clock);
    let provider = RealWeatherProvider::new(Arc::new(move |_, _| {
        started_tx.send(()).unwrap();
        release_rx
            .lock()
            .unwrap()
            .recv_timeout(Duration::from_secs(5))
            .expect("released");
        if *live_for_fetch.lock().unwrap() {
            Ok(Observation::new("Clear", 0.0, Some(20.0), Some(10.0)))
        } else {
            Err("The server answered with error 404.".to_string())
        }
    }))
    .with_clock(Arc::new(move || *clock_for_provider.lock().unwrap()));
    install_provider(&mut harness, Box::new(provider));
    harness.clear_speech();

    // One fetch: a frame starts it, frames run while it is in flight, it
    // answers, and the provider's clock moves on to the next retry.
    let fetch = |harness: &mut PlaytestHarness| {
        frames(harness, 1, DT);
        started_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("a fetch started");
        frames(harness, 30, DT);
        release_tx.send(()).unwrap();
        for _ in 0..500 {
            let busy = harness.with_drive(|d, _| {
                let city = d.weather().city.clone().expect("a weather cell");
                d.weather_mut()
                    .provider
                    .as_deref_mut()
                    .expect("a provider")
                    .refreshing(&city)
            });
            if !busy {
                break;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        frames(harness, 30, DT);
        *clock.lock().unwrap() += RETRY_AFTER_S + 1.0;
    };

    // Thirty failed retries, one a minute: half an hour of fallback.
    for _ in 0..30 {
        fetch(&mut harness);
    }
    let unavailable: Vec<String> = spoken(&harness)
        .into_iter()
        .filter(|line| line.contains("unavailable"))
        .collect();
    assert_eq!(unavailable.len(), 1, "{:#?}", spoken(&harness));

    *live.lock().unwrap() = true;
    harness.with_drive(|d, _| d.weather_mut().current = WeatherKind::Clear);
    fetch(&mut harness);
    frames(&mut harness, 120, DT);
    let ready = spoken(&harness)
        .into_iter()
        .filter(|line| line.contains("Live weather ready"))
        .count();
    assert_eq!(ready, 1, "{:#?}", spoken(&harness));
}
