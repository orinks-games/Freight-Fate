//! How far out an exit can be armed, and what the X key answers once the
//! destination exit has been called: `exit_window_mi` and `upcoming_exit_stop`
//! in `states/driving_events/exits.rs`, and the announcement wording in
//! `states/driving_events/destination_exit.rs`.
//!
//! Ported from `tests/test_driving_features.py`:
//! `test_exit_window_scales_with_speed_and_pacing`,
//! `test_destination_exit_announced_within_scaled_window`,
//! `test_announced_destination_exit_stays_actionable_when_window_shrinks`,
//! `test_destination_exit_response_queues_behind_intervening_safety_cue`,
//! `test_announced_destination_exit_grace_rejects_expired_and_passed_exit`,
//! `test_announced_destination_exit_wins_over_nearer_optional_stop`,
//! `test_exit_announcements_speak_each_name_once`,
//! `test_labeled_missed_exit_names_the_exit_once` and
//! `test_exit_key_is_a_toggle_and_needs_an_exit_nearby`.
//!
//! Where Python wrote `monkeypatch.setattr(driving.trip, "upcoming_stop",
//! lambda _window: None)` the trip's stop list is emptied instead, which is
//! what makes the real lookup answer nothing; where it stubbed
//! `_upcoming_exit_stop` to hand back an invented facility, the invented stop
//! is put on the trip and the destination exit is marked taken, so the real
//! lookup reaches it by the road it actually walks.

use ff_core::sim::trip_models::RoadStop;
use ff_core::sim::weather::{WeatherKind, WeatherProvider};

use freight_fate::playtest::harness::{key_event, PlaytestHarness, StartDelivery};
use freight_fate::states::base::Key;
use freight_fate::states::driving_core::{EXIT_WINDOW_MAX_MI, EXIT_WINDOW_MI};

const DT: f64 = 1.0 / 60.0;
const MPH_PER_MPS: f64 = 2.23694;

// -- rigging -------------------------------------------------------------------------

fn a_drive(name: &str) -> PlaytestHarness {
    a_drive_with_weather(name, None)
}

/// The rigging, with the trip's weather SOURCE named rather than left to
/// whatever the drive was built with.
///
/// Every case in this file counts what lands on the shared event-speech
/// channel, so anything else that can speak there has to be pinned, and the
/// weather source is one of them. Pinning `weather.current` alone -- which is
/// all this used to do -- pins today's reading and leaves the source open: a
/// live provider that answers, one that fails, and one whose fetch never
/// finishes each leave the weather system in a different state, and two of
/// those can put a condition line on this channel. That is not a hypothetical
/// difference between machines; it is the difference between a drive that is
/// asserting about exits and a drive that is also asserting about the weather
/// over Chicago.
///
/// So the source is an argument: `None` is no live source at all, and a
/// caller that wants one hands in a fake. Nothing here ever reaches the
/// weather service (see `network_guard.rs` for why it could not if it tried).
fn a_drive_with_weather(name: &str, provider: Option<Box<dyn WeatherProvider>>) -> PlaytestHarness {
    let mut harness = PlaytestHarness::new();
    harness.start_delivery(StartDelivery::named(name));
    harness.with_drive(move |drive, _| {
        drive.tutorial = None;
        drive.departure_checked = true;
        drive.trip.hazard_check_mi = 1e9;
        drive.trip.inspection_check_mi = 1e9;
        drive.trip.traffic_manager.rolling_bubble = false;
        drive.trip.set_npc_vehicles(Vec::new());
        drive.trip.traffic_pressures.clear();
        drive.trip.zones.retain(|z| z.aadt.is_none());
        drive.trip.weather.live = provider.is_some();
        drive.trip.weather.provider = provider;
        drive.trip.weather.current = WeatherKind::Clear;
        drive.trip.set_patrols(Vec::new());
        drive.trip.posts.clear();
    });
    harness.clear_speech();
    harness
}

/// A live source that answers with `kind` for every city, or -- with `None` --
/// one whose fetch has not come back yet. The two live shapes the drive can
/// actually be in, without a network between them and the answer.
struct FixedWeather {
    kind: Option<WeatherKind>,
}

impl WeatherProvider for FixedWeather {
    fn request(&mut self, _city: &str, _lat: f64, _lon: f64) {}

    fn get(&mut self, _city: &str) -> Option<WeatherKind> {
        self.kind
    }
}

/// `monkeypatch.setattr(driving.trip, "upcoming_stop", lambda _window: None)`:
/// no optional stop anywhere on the road.
fn no_optional_stops(harness: &mut PlaytestHarness) {
    harness.with_drive(|drive, _| drive.trip.stops.clear());
}

fn press_x(harness: &mut PlaytestHarness) {
    harness.with_drive(|drive, ctx| drive.handle_key_event(ctx, &key_event(Key::X, None)));
}

fn transcript(harness: &PlaytestHarness) -> Vec<String> {
    harness.app.speech().lines()
}

fn last_said(harness: &PlaytestHarness) -> String {
    transcript(harness).last().cloned().unwrap_or_default()
}

// -- the window ------------------------------------------------------------------------

#[test]
fn test_exit_window_scales_with_speed_and_pacing() {
    let mut harness = a_drive("Exit Window");
    harness.with_drive(|drive, _| {
        drive.trip.time_scale = 20.0;
        drive.truck_mut().velocity_mps = 0.0; // crawling -> the minimum window
    });
    assert!((harness.read_drive(|d| d.exit_window_mi()) - EXIT_WINDOW_MI).abs() < 1e-6);

    harness.with_drive(|drive, _| drive.truck_mut().velocity_mps = 70.0 / MPH_PER_MPS);
    let fast = harness.read_drive(|d| d.exit_window_mi());
    assert!(fast > EXIT_WINDOW_MI, "{fast}");
    assert!(fast <= EXIT_WINDOW_MAX_MI, "{fast}");

    // Fast pacing compresses time -> even more.
    harness.with_drive(|drive, _| drive.trip.time_scale = 40.0);
    let faster = harness.read_drive(|d| d.exit_window_mi());
    assert!(faster >= fast, "{faster} vs {fast}");
    assert!(faster <= EXIT_WINDOW_MAX_MI, "{faster}");
}

#[test]
fn test_destination_exit_announced_within_scaled_window() {
    // At highway speed on fast pacing the callout fires beyond the base
    // 5-mile window, buying real seconds to hear it, arm, and brake.
    let mut harness = a_drive("Scaled Window");
    harness.with_drive(|drive, ctx| {
        drive.trip.time_scale = 40.0;
        drive.truck_mut().velocity_mps = 74.0 / MPH_PER_MPS;
        let destination = drive
            .destination_exit_stop(ctx)
            .expect("a delivery run has a destination exit");
        drive.trip.position_mi = destination.at_mi - (EXIT_WINDOW_MI + 3.0);
    });
    harness.clear_speech();

    harness.with_drive(|drive, ctx| drive.check_destination_exit(ctx));

    let said = transcript(&harness);
    assert!(!said.is_empty(), "no callout inside the scaled window");
    assert!(
        said.last().expect("a line").contains("destination exit"),
        "{said:#?}"
    );
}

#[test]
fn test_announced_destination_exit_stays_actionable_when_window_shrinks() {
    // The spoken X instruction remains true through a human reaction delay.
    for (time_scale, approach_mph) in [(20.0, 54.0), (40.0, 56.0)] {
        let mut harness = a_drive(&format!("Shrink {time_scale}"));
        no_optional_stops(&mut harness);
        let destination = harness.with_drive(|drive, ctx| {
            // A bend inside its reaction window decompresses the clock, which
            // collapses the exit window to its floor; this case is about the
            // window shrinking with SPEED, so keep the curves out of it.
            drive.trip.curves.clear();
            drive.trip.time_scale = time_scale;
            drive.truck_mut().velocity_mps = approach_mph / MPH_PER_MPS;
            let destination = drive
                .destination_exit_stop(ctx)
                .expect("a delivery run has a destination exit");
            let announced_window = drive.exit_window_mi();
            drive.trip.position_mi = destination.at_mi - announced_window + 0.01;
            destination
        });
        harness.clear_speech();

        harness.with_drive(|drive, ctx| drive.check_destination_exit(ctx));

        assert!(
            last_said(&harness).contains("destination exit"),
            "{}",
            last_said(&harness)
        );
        // Reported case: automatic control is off.
        assert!(harness.read_drive(|d| d.cruise_mph).is_none());

        // Coasting while the player listens makes the dynamic window contract
        // below the still-ahead exit. Before the fix, the real X path answered
        // "No exit coming up" here.
        // Let the callout finish speaking before the next line lands. In one
        // instant the pacer reads it as still mid-sentence, and an
        // interrupting line hands it back to be requeued -- so the callout
        // would be heard twice and the assertion would be measuring that
        // rescue rather than what X did.
        harness.advance_clock(10.0);
        harness.with_drive(|drive, _| drive.truck_mut().velocity_mps = 30.0 / MPH_PER_MPS);
        let ahead = destination.at_mi - harness.read_drive(|d| d.trip.position_mi);
        assert!(harness.read_drive(|d| d.exit_window_mi()) < ahead);
        let stops_before = harness.app.speech().stop_event_calls();
        press_x(&mut harness);

        assert_eq!(
            harness.read_drive(|d| d.exit_stop.as_ref().map(|s| s.stop_type.clone())),
            Some("delivery_destination".to_string())
        );
        // "Signal set for ..." (the blinker waits for half a mile) is the 1.9
        // wording of the old "Signaling for ...".
        assert!(
            last_said(&harness).contains("Signal set for"),
            "{}",
            last_said(&harness)
        );
        assert!(
            !transcript(&harness)
                .join("\n")
                .contains("No exit coming up"),
            "{:#?}",
            transcript(&harness)
        );
        assert_eq!(harness.app.speech().stop_event_calls(), stops_before);
    }
}

/// Arm the destination exit, let a safety warning land, then press X, and
/// return every line that reached the event channel.
///
/// The whole subject is those lines: the warning first, the confirmation
/// second, and the confirmation queueing rather than cutting in. Nothing
/// about the road, the traffic or the weather is being measured here, which
/// is why `a_drive` pins all of it -- and why this body is shared with the
/// case below that runs it against a live weather source.
/// Advance the pacer's clock until it says the event voice has finished,
/// instead of guessing at a number of seconds.
///
/// The guess used to be ten. Ten is long enough for SOME destination-exit
/// callouts: the line names the exit number, the road, the places the road
/// serves, the facility, the town and the state, so how long the channel
/// stays busy follows whichever delivery the board handed out. When it was
/// not long enough the warning arrived on top of a callout still notionally
/// mid-sentence, the delivery layer flushed the backlog and requeued the
/// callout behind it, and a third line appeared -- which reads as this case
/// failing, and is nothing to do with what it is about. Asking the pacer is
/// the same question with an answer that does not depend on the wording.
fn wait_for_the_event_voice(harness: &mut PlaytestHarness) {
    let mut waited = 0.0;
    while harness.app.ctx.event_pacer.busy() && waited < 300.0 {
        harness.advance_clock(0.5);
        waited += 0.5;
    }
    assert!(
        !harness.app.ctx.event_pacer.busy(),
        "the event voice was still speaking after {waited} seconds"
    );
}

fn queue_behind_a_safety_cue(harness: &mut PlaytestHarness) -> Vec<(String, bool)> {
    no_optional_stops(harness);
    harness.with_drive(|drive, ctx| {
        drive.trip.time_scale = 20.0;
        drive.truck_mut().velocity_mps = 54.0 / MPH_PER_MPS;
        let destination = drive
            .destination_exit_stop(ctx)
            .expect("a delivery run has a destination exit");
        drive.trip.position_mi = destination.at_mi - drive.exit_window_mi() + 0.01;
        drive.check_destination_exit(ctx);
    });
    harness.clear_speech();
    let stops_before = harness.app.speech().stop_event_calls();
    // The callout has finished speaking; see the sibling case above for why
    // that matters before an interrupting line lands.
    wait_for_the_event_voice(harness);

    harness.app.ctx.say_event("Brake now. Hazard ahead.");
    // The player hears the warning, then reaches for X. Without the pause the
    // confirmation lands while the warning is still notionally mid-sentence,
    // and the delivery layer flushes the backlog and requeues the warning
    // behind it -- the pacer's own rescue, not anything X did, and its own
    // suite's business.
    harness.advance_clock(5.0);
    press_x(harness);

    assert_eq!(harness.app.speech().stop_event_calls(), stops_before);
    harness.app.event_calls()
}

fn assert_warning_then_queued_confirmation(calls: &[(String, bool)]) {
    assert_eq!(calls.len(), 2, "{calls:#?}");
    assert_eq!(calls[0].0, "Brake now. Hazard ahead.");
    assert!(calls[1].0.contains("Signal set for"), "{calls:#?}");
    assert!(
        !calls[1].1,
        "the confirmation queues rather than cutting in"
    );
}

#[test]
fn test_destination_exit_response_queues_behind_intervening_safety_cue() {
    // X must not silence a newer warning on the shared event-speech channel.
    let mut harness = a_drive("Queue Behind");
    let calls = queue_behind_a_safety_cue(&mut harness);
    assert_warning_then_queued_confirmation(&calls);
}

/// The same two lines, whatever the weather is doing.
///
/// The case above counts the lines on the event channel, so a weather source
/// that also speaks there changes its answer -- and until the drive's weather
/// source was pinned, which source it had was decided by a setting and by
/// whether a fetch happened to come back. A live reading that lands, a live
/// fetch that has not come back, and no live source at all are the three
/// states the drive can be in; the exits behave identically in all three, and
/// that is what this pins so nothing has to be inferred from a run that
/// happened to be offline.
#[test]
fn test_the_exit_confirmation_queue_does_not_depend_on_the_weather_source() {
    for (label, provider) in [
        ("answering", Some(WeatherKind::Rain)),
        ("still loading", None),
    ] {
        let mut harness = a_drive_with_weather(
            &format!("Queue Behind {label}"),
            Some(Box::new(FixedWeather { kind: provider })),
        );
        let calls = queue_behind_a_safety_cue(&mut harness);
        assert_warning_then_queued_confirmation(&calls);
        assert!(
            harness.read_drive(|d| d.trip.weather.provider.is_some()),
            "{label}: the live source was supposed to still be attached"
        );
    }
}

#[test]
fn test_announced_destination_exit_grace_rejects_expired_and_passed_exit() {
    // The reaction buffer never turns an old or passed announcement into an
    // exit.
    let mut harness = a_drive("Grace Window");
    no_optional_stops(&mut harness);
    let destination = harness.with_drive(|drive, ctx| {
        // See the sibling window-shrink case: a bend in its reaction window
        // decompresses the clock and collapses the exit window to its floor.
        drive.trip.curves.clear();
        drive.trip.time_scale = 40.0;
        drive.truck_mut().velocity_mps = 56.0 / MPH_PER_MPS;
        let destination = drive
            .destination_exit_stop(ctx)
            .expect("a delivery run has a destination exit");
        drive.trip.position_mi = destination.at_mi - drive.exit_window_mi() + 0.01;
        drive.check_destination_exit(ctx);
        destination
    });

    // The callout arms the destination exit itself, so the reaction buffer
    // only has to keep the spoken confirmation honest -- the exit can no
    // longer go missing while the player is still reacting.
    assert_eq!(
        harness.read_drive(|d| d.exit_stop.as_ref().map(|s| s.stop_type.clone())),
        Some("delivery_destination".to_string())
    );

    harness.with_drive(|drive, _| {
        drive.truck_mut().velocity_mps = 0.0;
        drive.destination_exit_response_s = 1.0 / 120.0;
    });
    harness.advance_clock(DT);
    harness.with_drive(|drive, ctx| drive.update_frame(ctx, DT));
    assert_eq!(harness.read_drive(|d| d.destination_exit_response_s), 0.0);
    harness.clear_speech();
    press_x(&mut harness);
    assert!(harness.read_drive(|d| d.exit_signal_on));
    assert!(
        !transcript(&harness)
            .iter()
            .any(|line| line.starts_with("No route exit to signal")),
        "{:#?}",
        transcript(&harness)
    );

    // Even a live response timer cannot resurrect an exit behind the truck.
    //
    // Python froze `_destination_exit_stop` on the announced exit so that
    // rolling past it left nothing ahead. The real scan does not freeze: a
    // truck past the announced interchange still has the end-of-route
    // fallback exit in front of it, and arming THAT is correct. So the truck
    // is put past every remaining destination exit instead -- inside the last
    // mile, where the fallback is behind too -- which is the condition the
    // case is really about.
    let _ = destination;
    harness.with_drive(|drive, _| {
        drive.exit_stop = None;
        drive.exit_signal_on = false;
        drive.destination_exit_response_s = 10.0;
        drive.trip.position_mi = drive.trip.total_miles() - 0.5;
    });
    harness.clear_speech();
    press_x(&mut harness);
    assert!(harness.read_drive(|d| d.exit_stop.is_none()));
    // "No route exit to signal for yet" is the 1.9 wording of the older
    // "No exit coming up" refusal.
    assert!(
        last_said(&harness).starts_with("No route exit to signal"),
        "{}",
        last_said(&harness)
    );
}

#[test]
fn test_announced_destination_exit_wins_over_nearer_optional_stop() {
    // X responds to the destination callout, not a newly nearby truck stop.
    let mut harness = a_drive("Wins Over Stop");
    harness.with_drive(|drive, ctx| {
        drive.trip.time_scale = 20.0;
        drive.truck_mut().velocity_mps = 54.0 / MPH_PER_MPS;
        let destination = drive
            .destination_exit_stop(ctx)
            .expect("a delivery run has a destination exit");
        drive.trip.position_mi = destination.at_mi - drive.exit_window_mi() + 0.01;
        // A nearer optional stop, right where the truck can still reach it.
        let mut nearer = RoadStop::new(
            "Nearby Travel Plaza",
            drive.trip.position_mi + 2.0,
            "truck_stop",
        );
        nearer.actions = ["fuel", "sleep"].iter().map(|a| a.to_string()).collect();
        drive.trip.stops = vec![nearer];
        drive.check_destination_exit(ctx);
    });
    harness.with_drive(|drive, _| drive.truck_mut().velocity_mps = 30.0 / MPH_PER_MPS);
    harness.clear_speech();

    press_x(&mut harness);

    assert_eq!(
        harness.read_drive(|d| d.exit_stop.as_ref().map(|s| s.stop_type.clone())),
        Some("delivery_destination".to_string())
    );
    let said = last_said(&harness);
    assert!(said.contains("destination exit"), "{said}");
    assert!(!said.contains("Nearby Travel Plaza"), "{said}");
}

// -- naming the exit once -------------------------------------------------------------

#[test]
fn test_exit_announcements_speak_each_name_once() {
    // Fallback phrasing must not repeat the facility or exit label -- the
    // sentence is heard, not read.
    let mut harness = a_drive("Name Once");
    // A miss needs a driver who can miss: full lane keeping takes the exit.
    harness.app.ctx.settings.lane_keeping = "partial".to_string();
    let facility = "grocery warehouse Trenton Distribution in Trenton";
    harness.with_drive(|drive, _| {
        let mut stop = RoadStop::new(facility, 10.0, "delivery_destination");
        stop.actions = vec!["deliver".to_string()];
        drive.trip.stops = vec![stop];
        // The real destination exit is out of the way, so `upcoming_exit_stop`
        // reaches the invented facility the way it reaches any route stop.
        drive.destination_exit_taken = true;
        drive.trip.position_mi = 9.0;
    });
    harness.clear_speech();

    harness.with_drive(|drive, ctx| drive.take_exit(ctx));
    let said = last_said(&harness);
    assert_eq!(said.matches(facility).count(), 1, "{said}");
    assert!(said.contains("destination exit for"), "{said}");

    let stop = harness.read_drive(|d| d.trip.stops[0].clone());
    let announcement =
        harness.with_drive(|d, ctx| d.destination_exit_announcement(ctx, &stop, 1.2));
    assert_eq!(announcement.matches(facility).count(), 1, "{announcement}");
    // Singular, not "1 miles".
    assert!(announcement.contains("In 1 mile,"), "{announcement}");

    // Inside a mile the whole-mile form rounds to nothing, and the last call
    // before the gore came out as "In 0 miles, the destination exit" while
    // there was still a third of a mile to use it (owner playtest,
    // 2026-08-15).
    let close = harness.with_drive(|d, ctx| d.destination_exit_announcement(ctx, &stop, 0.3));
    assert!(!close.contains("0 miles"), "{close}");
    assert!(close.contains("a quarter mile"), "{close}");

    harness.with_drive(move |drive, _| {
        // Past the end of the gore window, never having taken the exit lane.
        drive.trip.position_mi = stop.at_mi + 0.5;
        drive.truck_mut().velocity_mps = 29.0;
    });
    harness.clear_speech();
    harness.with_drive(|drive, ctx| drive.update_exit(ctx, 0.0, DT));
    let said = last_said(&harness);
    assert!(said.contains("Missed"), "{said}");
    assert_eq!(said.matches(facility).count(), 1, "{said}");
}

#[test]
fn test_labeled_missed_exit_names_the_exit_once() {
    let mut harness = a_drive("Labeled Miss");
    harness.with_drive(|drive, _| {
        let mut stop = RoadStop::new("grocery warehouse in Trenton", 10.0, "delivery_destination");
        stop.actions = vec!["deliver".to_string()];
        stop.exit_label = "exit 5B".to_string();
        drive.exit_stop = Some(stop);
        drive.trip.position_mi = 10.0;
        drive.truck_mut().velocity_mps = 29.0;
    });
    harness.clear_speech();

    harness.with_drive(|drive, ctx| drive.update_exit(ctx, 0.0, DT));

    let said = last_said(&harness);
    assert!(said.contains("Missed exit 5B"), "{said}");
    assert_eq!(said.matches("exit 5B").count(), 1, "{said}");
}

// -- the key itself ---------------------------------------------------------------------

#[test]
fn test_exit_key_is_a_toggle_and_needs_an_exit_nearby() {
    let mut harness = a_drive("X Toggle");
    let first_stop_mi = harness.read_drive(|d| d.trip.stops[0].at_mi);
    // Far from any stop: X does not arm.
    harness.with_drive(|drive, _| drive.trip.position_mi = 0.0);
    if first_stop_mi > 6.0 {
        press_x(&mut harness);
        assert!(harness.read_drive(|d| d.exit_stop.is_none()));
    }

    // In range it arms; pressing X again cancels.
    harness.with_drive(move |drive, _| drive.trip.position_mi = first_stop_mi - 2.0);
    press_x(&mut harness);
    assert_eq!(
        harness.read_drive(|d| d.exit_stop.as_ref().map(|s| s.at_mi)),
        Some(first_stop_mi)
    );
    assert!(harness.read_drive(|d| d.exit_signal_on));
    press_x(&mut harness);
    assert!(!harness.read_drive(|d| d.exit_signal_on));
    assert!(harness.read_drive(|d| d.exit_signal_canceled));
}
