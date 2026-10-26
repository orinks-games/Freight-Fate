//! The cab around the driver: shutting the engine off, the status panel's
//! effect on the radio, the radio rotating under a pause, and what the pause
//! and the dial tell the drivers board.
//!
//! Ported from `tests/test_driving_features.py`:
//! `test_engine_shutdown_is_blocked_at_highway_speed`,
//! `test_engine_shutdown_is_allowed_once_stopped`,
//! `test_closing_status_panel_does_not_restart_drive_music`,
//! `test_drive_music_advances_to_next_track_while_paused`,
//! `test_pause_menu_reports_off_duty_to_the_drivers_board` and
//! `test_drivers_board_line_names_the_station_playing_in_the_cab`.

use ff_core::music::music_track_duration_s;
use ff_core::sim::weather::WeatherKind;

use freight_fate::online_presence::PAUSED_ACTIVITY;
use freight_fate::playtest::harness::{key_event, PlaytestHarness, StartDelivery};
use freight_fate::states::base::{Key, State};
use freight_fate::states::driving_menu_states::DrivingStatusState;
use freight_fate::states::driving_pause_states::PauseMenuState;

const DT: f64 = 1.0 / 60.0;

// -- rigging -------------------------------------------------------------------------

fn a_drive(name: &str) -> PlaytestHarness {
    let mut harness = PlaytestHarness::new();
    harness.start_delivery(StartDelivery::named(name));
    harness.with_drive(|drive, _| {
        drive.tutorial = None;
        drive.departure_checked = true;
        drive.trip.hazard_check_mi = 1e9;
        drive.trip.inspection_check_mi = 1e9;
        drive.trip.traffic_manager.rolling_bubble = false;
        drive.trip.set_npc_vehicles(Vec::new());
        drive.trip.traffic_pressures.clear();
        drive.trip.zones.retain(|z| z.aadt.is_none());
        drive.trip.weather.current = WeatherKind::Clear;
    });
    harness.clear_speech();
    harness
}

fn press(harness: &mut PlaytestHarness, key: Key) {
    harness.with_drive(move |drive, ctx| drive.handle_key_event(ctx, &key_event(key, None)));
}

fn last_main(harness: &PlaytestHarness) -> String {
    harness.app.main_lines().last().cloned().unwrap_or_default()
}

// -- the ignition ----------------------------------------------------------------------

#[test]
fn test_engine_shutdown_is_blocked_at_highway_speed() {
    let mut harness = a_drive("Shutdown Blocked");
    press(&mut harness, Key::E);
    assert!(harness.read_drive(|d| d.truck().engine_on));
    harness.app.ctx.audio.update(5.0); // let the ignition finish before toggling again
    harness.with_drive(|drive, _| drive.truck_mut().velocity_mps = 31.3);
    harness.clear_speech();

    press(&mut harness, Key::E);

    assert!(harness.read_drive(|d| d.truck().engine_on));
    let said = last_main(&harness);
    assert!(
        said.contains("Brake below 5 miles per hour first."),
        "{said}"
    );
    let status = harness.read_drive(|d| d.status_text.clone());
    assert!(status.contains("shutdown blocked"), "{status}");
}

#[test]
fn test_engine_shutdown_block_speaks_its_limit_in_metric() {
    let mut harness = a_drive("Shutdown Blocked Metric");
    harness.app.ctx.settings.imperial_units = false;
    press(&mut harness, Key::E);
    harness.app.ctx.audio.update(5.0);
    harness.with_drive(|drive, _| drive.truck_mut().velocity_mps = 11.2);
    harness.clear_speech();

    press(&mut harness, Key::E);

    let said = last_main(&harness);
    assert!(said.contains("kilometers per hour"), "{said}");
    assert!(
        said.contains("Brake below 8 kilometers per hour first."),
        "{said}"
    );
    assert!(!said.contains("miles"), "{said}");
}

#[test]
fn test_engine_shutdown_is_allowed_once_stopped() {
    let mut harness = a_drive("Shutdown Allowed");
    press(&mut harness, Key::E);
    assert!(harness.read_drive(|d| d.truck().engine_on));
    harness.app.ctx.audio.update(5.0); // let the ignition finish before toggling again
    harness.with_drive(|drive, _| drive.truck_mut().velocity_mps = 0.0);

    press(&mut harness, Key::E);

    assert!(!harness.read_drive(|d| d.truck().engine_on));
}

// -- the radio -------------------------------------------------------------------------

#[test]
fn test_closing_status_panel_does_not_restart_drive_music() {
    let mut harness = a_drive("Status Music");
    let log = harness.app.record_audio();
    log.borrow_mut().music.clear();

    press(&mut harness, Key::Tab);
    assert!(
        harness.state_is::<DrivingStatusState>(),
        "Tab opened no status menu"
    );
    harness.key(key_event(Key::Escape, None));

    assert!(
        harness.has_drive() && harness.state_is::<freight_fate::states::driving::DrivingState>()
    );
    assert!(log.borrow().music.is_empty(), "{:#?}", log.borrow().music);
}

#[test]
fn test_drive_music_advances_to_next_track_while_paused() {
    // The drive's music is the in-cab radio now, and a paused rig is still a
    // cab with the radio on: the station keeps rotating under the pause menu
    // instead of going silent when the current song runs out.
    let mut harness = a_drive("Paused Radio");
    harness.with_drive(|drive, ctx| {
        drive.truck_mut().start_engine(); // the radio has power only with the engine on
        drive.update_frame(ctx, DT); // lets the tuned station start its rotation
    });
    assert!(harness.read_drive(|d| d.radio.enabled));
    assert!(!harness.read_drive(|d| d.radio_playlist.is_empty()));

    press(&mut harness, Key::Escape);
    assert!(harness.state_is::<PauseMenuState>(), "Escape did not pause");
    let (current, index_before, break_before) = harness.read_drive(|d| {
        let track = d.radio_playlist[d.radio_track_index % d.radio_playlist.len()].clone();
        (track, d.radio_track_index, !d.radio_break_queue.is_empty())
    });
    let log = harness.app.record_audio();
    log.borrow_mut().music.clear();

    // Sit on the pause menu until the current song's playback ends.
    let seconds = music_track_duration_s(&current) + 1.0;
    harness.advance_clock(seconds);
    harness.with_state::<PauseMenuState, _>(|state, ctx| state.update(ctx, seconds));

    assert!(
        !log.borrow().music.is_empty(),
        "the radio went silent under the pause menu"
    );
    let (index_after, break_after) =
        harness.read_drive(|d| (d.radio_track_index, !d.radio_break_queue.is_empty()));
    assert!(
        index_after != index_before || break_after != break_before,
        "the rotation never moved on"
    );
}

// -- the drivers board -------------------------------------------------------------------

#[test]
fn test_pause_menu_stays_on_the_drivers_board_as_paused() {
    let mut harness = a_drive("Board Pause");
    let drive_detail = harness.with_drive(|d, ctx| {
        d.online_presence_state(ctx)
            .expect("a rolling drive is on the public board")
            .detail
    });

    harness.with_drive(|drive, ctx| drive.push_pause_menu(ctx));
    assert!(harness.state_is::<PauseMenuState>());

    // A pause is not the end of a shift: the player stays on the public
    // board, shown as paused over the drive's own detail, so nobody's duty
    // watch calls them off duty for a bathroom break...
    let (board, discord) = harness.with_state::<PauseMenuState, _>(|state, ctx| {
        (state.online_presence(ctx), state.presence(ctx))
    });
    let board = board.expect("a paused drive stays on the public board");
    assert_eq!(board.activity, PAUSED_ACTIVITY);
    assert_eq!(board.detail, drive_detail);
    // ...and Discord presence tells friends the game is paused too.
    assert_eq!(
        discord.expect("Discord presence stays on").activity,
        PAUSED_ACTIVITY
    );
}

#[test]
fn test_drivers_board_line_names_the_station_playing_in_the_cab() {
    let mut harness = a_drive("Board Radio");

    let (board_detail, discord_detail, stream_url, listening) = harness.with_drive(|drive, ctx| {
        drive.radio.enabled = true;
        let station = drive.radio.clone().current_station();
        let listening = format!("listening to {}", station.display_name());
        let board = drive
            .online_presence_state(ctx)
            .expect("a rolling drive is on the board");
        let discord = drive
            .presence_state(ctx)
            .expect("Discord presence is built too");
        (
            board.detail,
            discord.detail,
            station.stream_url.clone(),
            listening,
        )
    });

    assert!(board_detail.ends_with(&listening), "{board_detail}");
    // The clause is board-only color: Discord presence keeps the plain
    // route/cargo detail, and no stream URL ever leaves the game.
    assert!(!discord_detail.contains(&listening), "{discord_detail}");
    assert!(
        stream_url.is_empty() || !board_detail.contains(&stream_url),
        "{board_detail}"
    );

    // Radio off, clause gone -- the board hears only what the cab plays.
    let off_detail = harness.with_drive(|drive, ctx| {
        drive.radio.enabled = false;
        drive
            .online_presence_state(ctx)
            .expect("still on the board")
            .detail
    });
    assert!(!off_detail.contains("listening to"), "{off_detail}");
}
