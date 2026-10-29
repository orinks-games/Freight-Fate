//! Dispatch board and save/load abuse (port of
//! `tools/playtest_break_scenarios/dispatch_saveload.py`).
//!
//! Declining loads until the budget runs dry (and checking the board doesn't
//! reroll on re-entry), and saving/reloading mid-hazard or mid-traffic-stop to
//! see whether the consequences survive the round trip.

use crate::playtest::breaker::{outcome, Outcome, Rig, RigOptions, Verdict};
use crate::playtest::harness::{key_event, PlaytestHarness};
use crate::states::base::Key;
use crate::states::city::{CityMenuState, JobBoardState};
use crate::states::driving::DrivingState;
use crate::states::driving_core::{hos_mut_of, hos_of};
use crate::states::main_menu::{MainMenuState, NameEntryState};
use crate::states::main_menu_career::{CareerStartState, HomeCityState, HomeTerminalState};

use ff_core::models::dispatch_policy::declines_remaining;

/// Decline assigned loads until dispatch runs dry; re-enter the board hunting
/// a reroll.
pub fn dispatch_decline_budget() -> Outcome {
    let mut findings: Vec<String> = Vec::new();
    let mut harness = PlaytestHarness::new();
    harness.app.ctx.push_state(MainMenuState::new());
    harness.app.ctx.run_deferred();
    harness.select_current_menu_text("New career");
    harness.expect_state::<NameEntryState>("naming a new career");
    for ch in "Breaker".chars() {
        harness.key(key_event(Key::from_char(ch), Some(ch)));
    }
    harness.key(key_event(Key::Return, None));
    harness.expect_state::<CareerStartState>("after the name");
    harness.key(key_event(Key::Return, None));
    harness.expect_state::<HomeTerminalState>("after the career start");
    harness.key(key_event(Key::Return, None));
    harness.expect_state::<HomeCityState>("after the home terminal");
    harness.key(key_event(Key::Return, None));
    harness.expect_state::<CityMenuState>("after the home city");
    harness.key(key_event(Key::Return, None));
    harness.expect_state::<JobBoardState>("the dispatch board");
    let assigned = harness.with_state::<JobBoardState, bool>(|board, _| board.assigned_mode());
    if !assigned {
        findings.push("a brand-new hire was not in assigned-load mode".to_string());
    }
    let budget = declines_remaining(harness.app.ctx.profile.as_ref().expect("a profile"));
    let mut declined = 0;
    while declined <= budget + 5 {
        let label = harness
            .menu_labels()
            .into_iter()
            .find(|item| item.starts_with("Decline"));
        let Some(label) = label else { break };
        harness.select_menu_item(&label);
        declined += 1;
    }
    if declined > budget {
        findings.push(format!(
            "decline budget said {budget} but the board accepted {declined} declines"
        ));
    }
    let remaining = declines_remaining(harness.app.ctx.profile.as_ref().expect("a profile"));
    if remaining != 0.max(budget - declined) {
        findings.push("declines_remaining does not match the declines actually spent".to_string());
    }
    let jobs_before = board_offers(&mut harness);
    harness.key(key_event(Key::Escape, None));
    harness.expect_state::<CityMenuState>("back out of the board");
    harness.select_current_menu_text("Dispatch board");
    harness.expect_state::<JobBoardState>("back on the board");
    let jobs_after = board_offers(&mut harness);
    if jobs_before != jobs_after {
        findings.push(
            "leaving and re-entering the dispatch board rerolled the offers -- board-reroll \
             farming is open (dispatch_board_cache failed)"
                .to_string(),
        );
    }
    if budget > 0
        && harness
            .menu_labels()
            .iter()
            .any(|item| item.starts_with("Decline"))
    {
        findings.push("spent declines came back after re-entering the board".to_string());
    }
    let verdict = if findings.is_empty() {
        Verdict::Clean
    } else {
        Verdict::Odd
    };
    let note = findings.first().cloned().unwrap_or_else(|| {
        format!("budget of {budget} enforced; board and spent declines survive re-entry")
    });
    let transcript = harness.transcript();
    Outcome {
        name: "dispatch_decline_budget".to_string(),
        verdict,
        note,
        findings,
        transcript,
    }
}

/// `[(j.origin, j.destination, round(j.pay)) for j in board.jobs]`.
fn board_offers(harness: &mut PlaytestHarness) -> Vec<(String, String, i64)> {
    harness.with_state::<JobBoardState, _>(|board, _| {
        board
            .jobs
            .iter()
            .map(|job| {
                (
                    job.origin.clone(),
                    job.destination.clone(),
                    job.pay.round() as i64,
                )
            })
            .collect()
    })
}

/// Save and reload during a traffic stop and a live hazard; do the
/// consequences survive?
pub fn save_scum_enforcement() -> Outcome {
    let mut rig = Rig::new(RigOptions::default());
    let mut findings: Vec<String> = Vec::new();
    rig.drive.trip.position_mi = 12.0;
    rig.prepare(70.0, None);
    rig.drive.jake_zone_fines = 2;
    rig.drive.jake_fines_paid = 450.0;
    rig.drive.ticket_fines_paid = 150.0;
    hos_mut_of(&mut rig.app.ctx).drive(200.0);
    rig.drive.begin_pull_over(&mut rig.app.ctx, 55.0);
    rig.app.ctx.run_deferred();
    rig.drive.hazard_deadline = Some(6.0);
    let deadline_before = rig.drive.job.deadline_game_h;
    let snap = rig.drive.snapshot(&rig.app.ctx);
    let Some(resumed) = DrivingState::from_snapshot(&mut rig.app.ctx, &snap) else {
        findings.push("snapshot failed to round-trip at all".to_string());
        return outcome("save_scum_enforcement", &rig, findings, "");
    };
    if rig.drive.pull_over.is_some() && resumed.pull_over.is_none() {
        findings.push(
            "save-and-reload during a traffic stop erases the stop: the trooper, the ticket, and \
             the felony ladder all vanish -- quit-to-menu is a get-out-of-jail-free card \
             (pull-over state is not in the snapshot)"
                .to_string(),
        );
    }
    // A live hazard is deliberately NOT in the snapshot, and that is not a
    // save-scum hole because no save can be written while one is running.
    // Every player-reachable save needs a parked truck or an open menu, and
    // the one save taken mid-roll is the traffic stop, which cannot begin
    // during a hazard. So what this scenario must hold is the gate, not the
    // round-trip.
    if !rig.drive.enforcement_busy() {
        findings.push(
            "a live hazard no longer makes the cab busy, so a traffic stop can begin mid-hazard \
             -- that stop snapshots itself, which would put a live hazard into a save the reload \
             cannot speak"
                .to_string(),
        );
    }
    if resumed.speeding_tickets != rig.drive.speeding_tickets {
        findings
            .push("the on-the-spot ticket count was lost in the snapshot round-trip".to_string());
    }
    if resumed.jake_zone_fines != 2 || resumed.jake_fines_paid != 450.0 {
        findings.push("jake citations lost in the snapshot round-trip".to_string());
    }
    if resumed.ticket_fines_paid != 150.0 {
        findings.push("ticket ledger lost in the snapshot round-trip".to_string());
    }
    if (resumed.job.deadline_game_h - deadline_before).abs() > 1e-6 {
        findings.push(format!(
            "deadline drifted across save/reload: {deadline_before} -> {} (free hours)",
            resumed.job.deadline_game_h
        ));
    }
    if (resumed.trip.position_mi - rig.drive.trip.position_mi).abs() > 1e-6 {
        findings.push("position drifted across save/reload".to_string());
    }
    // The shift clock lives on the profile, which the snapshot restores into
    // the context rather than onto the state: read it where it lives.
    if (hos_of(&rig.app.ctx).driving_min - 200.0).abs() > 1e-6 {
        findings.push("HOS driving clock drifted across save/reload".to_string());
    }
    outcome(
        "save_scum_enforcement",
        &rig,
        findings,
        "snapshot preserved every ledger and live consequence",
    )
}
