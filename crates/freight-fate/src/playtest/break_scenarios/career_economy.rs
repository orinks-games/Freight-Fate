//! Career and economy abuse: settlement, advances, endorsements, levels, XP
//! (port of `tools/playtest_break_scenarios/career_economy.py`).
//!
//! Covers whether spoken dollar amounts match the ledger at settlement,
//! whether an abandon-and-advance cycle can mint money, whether endorsement
//! purchases are exact-dollar honest, level-up bookkeeping at the settlement
//! boundary, the owner-operator buy-in gate at exactly level 18, and
//! short-haul streak XP farming.

use crate::app::testing::TestApp;
use crate::playtest::breaker::{outcome, outcome_of, Outcome, Rig, RigOptions, Verdict};
use crate::playtest::harness::{PlaytestHarness, StartDelivery};
use crate::playtest::MPH_PER_MPS;
use crate::states::city_business::{BusinessStatusState, EndorsementCourseState};
use crate::states::driving_engine_brake::{JAKE_ZONE_FINES, JAKE_ZONE_GRACE_S};
use crate::states::driving_menu_states::ArrivalState;
use crate::states::driving_pause_states::{AbandonJobConfirmationState, PauseMenuState};

use ff_core::models::business::{
    LEASED_OWNER_OPERATOR, OWNER_OPERATOR_BUY_IN, OWNER_OPERATOR_DELIVERIES, OWNER_OPERATOR_LEVEL,
    OWNER_OPERATOR_REPUTATION, OWNER_OPERATOR_WORKING_CAPITAL,
};
use ff_core::models::career::{
    Career, DELIVERY_COMPLETION_XP, LEVEL_XP, XP_CLEAN_BONUS, XP_PER_MILE_ON_TIME,
};
use ff_core::models::credentials::{credential, CREDENTIALS};
use ff_core::models::economy::{pay_advance_grant, PAY_ADVANCE_LIMIT};
use ff_core::models::jobs::MIN_JOB_DISTANCE_MI;
use ff_core::models::profile::Profile;
use ff_core::models::solvency::COLLECTION_SHARE;

use super::text::grouped_number_after;

/// Delivery carrying a balance owed larger than the load pays; the spoken
/// ledger must match the money, and the run must still pay the driver.
pub fn settlement_spoken_balance() -> Outcome {
    let mut findings: Vec<String> = Vec::new();
    let mut harness = PlaytestHarness::new();
    harness.start_delivery(StartDelivery::named("Breaker"));
    // A balance far bigger than a starter load's wage. This used to be billed
    // whole against the next settlement, which floored net pay at zero forever
    // and silently forgave the rest while the spoken ledger claimed it had
    // been paid. It is now recovered at a capped share.
    let (owed_before, money_before) = {
        let profile = harness.app.ctx.profile.as_mut().expect("a profile");
        profile.fines_owed = 2_000.0;
        (profile.fines_owed, profile.money())
    };
    harness.settle_current_delivery();
    harness.read_settlement_lines();
    let text = harness.transcript_text();
    let (money, owed, deliveries) = {
        let profile = harness.app.ctx.profile.as_ref().expect("a profile");
        (
            profile.money(),
            profile.fines_owed,
            profile.career.deliveries,
        )
    };
    if let Some(spoken) = grouped_number_after(&text, "you now have ") {
        if (spoken - money.round()).abs() > 1.0 {
            findings.push(format!(
                "settlement said 'you now have {spoken:.0}' but the balance is {money:.0}"
            ));
        }
    }
    if let Some(spoken) = grouped_number_after(&text, "Money after settlement: ") {
        if (spoken - money.round()).abs() > 1.0 {
            findings.push(format!(
                "settlement line said {spoken:.0} dollars after settlement, balance is {money:.0}"
            ));
        }
    }
    let collected = ((owed_before - owed) * 100.0).round() / 100.0;
    let delta = money - money_before;
    if collected <= 0.0 {
        findings.push(format!(
            "a balance of {owed_before:.0} dollars was carried into this settlement and none of \
             it was collected: working never pays it down"
        ));
    }
    if delta <= 0.0 {
        findings.push(format!(
            "the run paid the driver {delta:.0}: a balance owed took the whole settlement, which \
             is the zero-pay trap -- a driver with a truck, a board, and no reachable state \
             where working helps"
        ));
    }
    if collected > 0.0 && !text.contains("Balance owed") {
        findings.push(format!(
            "{collected:.0} dollars came off the balance but no spoken line says so: money \
             moved silently"
        ));
    }
    // The floor is the promise: never more than the capped share.
    let gross = delta + collected;
    if gross > 0.0 && collected > ((gross * COLLECTION_SHARE) * 100.0).round() / 100.0 + 1.0 {
        findings.push(format!(
            "collection took {collected:.0} of {gross:.0}, past the {:.0}% cap the game \
             promises out loud",
            COLLECTION_SHARE * 100.0
        ));
    }
    if deliveries != 1 {
        findings.push(format!(
            "career shows {deliveries} deliveries after one run"
        ));
    }
    let verdict = if findings.is_empty() {
        Verdict::Clean
    } else {
        Verdict::Odd
    };
    let note = findings
        .first()
        .cloned()
        .unwrap_or_else(|| format!("spoken settlement matches the ledger; balance {money:.0}"));
    let transcript = harness.transcript();
    Outcome {
        name: "settlement_spoken_balance".to_string(),
        verdict,
        note,
        findings,
        transcript,
    }
}

/// Take a pay advance, abandon at 99% complete; the cycle must never mint
/// money.
pub fn abandon_and_advance_cycle() -> Outcome {
    let mut rig = Rig::new(RigOptions::default());
    let mut findings: Vec<String> = Vec::new();
    rig.drive.trip.position_mi = rig.drive.trip.total_miles() - 1.0; // one mile from the gate
    let rep_before = {
        let profile = rig.app.ctx.profile.as_mut().expect("a profile");
        profile.set_money(5.0);
        profile.career.reputation
    };
    let mut debt = 0.0;
    let mut minted = 0.0;
    for _ in 0..4 {
        let money_at_abandon = {
            let profile = rig.app.ctx.profile.as_mut().expect("a profile");
            let grant = pay_advance_grant(
                profile.money(),
                profile.pay_advance,
                profile.pay_advance_used_for_load,
            );
            if grant > 0.0 {
                profile.earn(grant);
                profile.pay_advance += grant;
                profile.pay_advance_used_for_load = true;
                minted += grant;
            }
            profile.money()
        };
        // Python called `AbandonJobConfirmationState._confirm()`; that method
        // is private here, so this presses the row that calls it -- the same
        // path the pause menu gives a player.
        let confirmed = rig.with_drive_on_stack(|rig, drive| {
            // The pause menu sits under the confirmation because `confirm`
            // pops BOTH on its way to the city; without it the pop would
            // take the drive early.
            rig.app
                .ctx
                .push_state_with(PauseMenuState::with_drive(drive.clone()), false);
            rig.app
                .ctx
                .push_state(AbandonJobConfirmationState::new(drive));
            rig.app.ctx.run_deferred();
            rig.select_menu_containing("abandon the job")
        });
        if !confirmed {
            findings.push("the abandon confirmation had no row that abandons".to_string());
            break;
        }
        let (money, advance) = {
            let profile = rig.app.ctx.profile.as_ref().expect("a profile");
            (profile.money(), profile.pay_advance)
        };
        if ((money_at_abandon - money) - 500.0).abs() > 0.01 {
            findings.push(format!(
                "abandon penalty took {:.0}, spoken text promises five hundred",
                money_at_abandon - money
            ));
        }
        minted -= 500.0;
        debt = advance;
    }
    if minted > 0.0 {
        findings.push(format!("advance-abandon cycle minted {minted:.0} dollars"));
    }
    if debt > PAY_ADVANCE_LIMIT + 0.01 {
        findings.push(format!(
            "outstanding advance {debt:.0} exceeds the {PAY_ADVANCE_LIMIT} cap"
        ));
    }
    let (money, advance, used, reputation) = {
        let profile = rig.app.ctx.profile.as_ref().expect("a profile");
        (
            profile.money(),
            profile.pay_advance,
            profile.pay_advance_used_for_load,
            profile.career.reputation,
        )
    };
    if pay_advance_grant(money, advance, used) > 0.0 && advance >= PAY_ADVANCE_LIMIT {
        findings.push("dispatcher kept advancing past the outstanding-advance cap".to_string());
    }
    if reputation > rep_before - 5.0 + 0.01 {
        findings.push("abandoning repeatedly cost no reputation".to_string());
    }
    if rig.said("Job abandoned") == 0 {
        findings.push("abandon confirmation spoke nothing".to_string());
    }
    for line in rig.lines_with("returned to") {
        if !line.contains("Buffalo") {
            findings.push(format!(
                "abandon said '{line}' but the career is in Buffalo"
            ));
        }
    }
    let note = format!(
        "4 abandons at 99%: cycle nets {minted:.0}, debt capped at {debt:.0}, rep down {:.0}",
        rep_before - reputation
    );
    outcome("abandon_and_advance_cycle", &rig, findings, &note)
}

/// Buy a course broke, then with the exact dollar; refusals and balances must
/// be honest.
pub fn endorsement_wallet_edges() -> Outcome {
    let mut rig = Rig::new(RigOptions::default());
    let mut findings: Vec<String> = Vec::new();
    let (key, cost) = {
        let career = &rig.app.ctx.profile.as_ref().expect("a profile").career;
        let earned = career.endorsements();
        let level = career.level();
        // A course this fresh career can actually book: unheld, level-open,
        // no prerequisites, and no background-check wait -- the wallet is
        // the only thing under test here.
        CREDENTIALS
            .iter()
            .find(|c| {
                !earned.contains(c.key)
                    && level >= c.min_level
                    && c.prereqs.is_empty()
                    && c.wait_days == 0.0
                    && !c.needs_clean_record
            })
            .map(|c| (c.key, c.course_cost))
            .expect("a course the driver does not hold")
    };
    if let Some(profile) = rig.app.ctx.profile.as_mut() {
        profile.set_money(cost - 1.0);
    }
    rig.app.ctx.push_state(EndorsementCourseState::new());
    rig.app.ctx.run_deferred();
    let row = course_row(&rig, key);
    let Some(row) = row else {
        findings.push(format!("no course row for {key} on the endorsement menu"));
        return outcome("endorsement_wallet_edges", &rig, findings, "");
    };
    rig.select_menu_containing(&row);
    let holds_now = |rig: &Rig| {
        rig.app
            .ctx
            .profile
            .as_ref()
            .expect("a profile")
            .career
            .endorsements()
            .contains(key)
    };
    if holds_now(&rig) {
        findings.push("a course sold itself one dollar short".to_string());
    }
    let money = rig.app.ctx.profile.as_ref().map_or(0.0, |p| p.money());
    if money != cost - 1.0 {
        findings.push("a refused course still took money".to_string());
    }
    let refusal = rig.transcript().last().cloned().unwrap_or_default();
    if !refusal.contains(&fmt_dollars(cost)) || !refusal.contains(&fmt_dollars(cost - 1.0)) {
        findings.push(format!(
            "refusal does not state both numbers honestly: {refusal}"
        ));
    }
    if let Some(profile) = rig.app.ctx.profile.as_mut() {
        profile.set_money(cost);
    }
    let lines_before_purchase = rig.transcript().len();
    rig.select_menu_containing(&row);
    if !holds_now(&rig) {
        findings.push("exact-money purchase was refused".to_string());
    }
    let money = rig.app.ctx.profile.as_ref().map_or(0.0, |p| p.money());
    if money != 0.0 {
        findings.push(format!(
            "exact-money purchase left {money}, expected exactly 0"
        ));
    }
    // The purchase confirmation is followed by an achievement announcement, so
    // check every line this call actually spoke, not just the last one.
    let transcript = rig.transcript();
    let purchase_lines = &transcript[lines_before_purchase.min(transcript.len())..];
    if !purchase_lines
        .iter()
        .any(|line| line.contains("You have 0 dollars left"))
    {
        findings.push(format!(
            "purchase did not speak the zero balance: {purchase_lines:?}"
        ));
    }
    // Re-buying a credential already held: the row is now the "earned" one.
    rig.select_menu_containing("earned, self-paid course");
    let money = rig.app.ctx.profile.as_ref().map_or(0.0, |p| p.money());
    if money != 0.0 {
        findings.push("re-buying an owned endorsement charged money".to_string());
    }
    outcome(
        "endorsement_wallet_edges",
        &rig,
        findings,
        "broke refusal, exact-dollar purchase, and re-buy guard all honest",
    )
}

/// The credential ladder's gates: a level-locked course must refuse without
/// charging, a prerequisite must be named, and a background-checked course
/// must take the money but withhold the credential until the wait is served.
pub fn credential_ladder_gates() -> Outcome {
    let mut rig = Rig::new(RigOptions::default());
    let mut findings: Vec<String> = Vec::new();
    let hazmat = credential("hazmat").expect("hazmat is on the ladder");
    let lcv = credential("lcv").expect("lcv is on the ladder");
    if let Some(profile) = rig.app.ctx.profile.as_mut() {
        profile.set_money(50_000.0);
    }
    rig.app.ctx.push_state(EndorsementCourseState::new());
    rig.app.ctx.run_deferred();

    // 1. Level 1 books nothing federal: the refusal must name the level and
    //    charge nothing.
    let money_before = rig.app.ctx.profile.as_ref().map_or(0.0, |p| p.money());
    if let Some(row) = course_row(&rig, "hazmat") {
        rig.select_menu_containing(&row);
        let refusal = rig.transcript().last().cloned().unwrap_or_default();
        if !refusal.contains(&format!("level {}", hazmat.min_level)) {
            findings.push(format!(
                "a level-1 hazmat booking did not name the level gate: {refusal}"
            ));
        }
    } else {
        findings.push("no hazmat course row on the menu".to_string());
    }
    if rig.app.ctx.profile.as_ref().map_or(0.0, |p| p.money()) != money_before {
        findings.push("a refused course still took money".to_string());
    }

    // 2. LCV at level, without the T endorsement: the prerequisite is named.
    if let Some(profile) = rig.app.ctx.profile.as_mut() {
        profile.career.xp = LEVEL_XP[(lcv.min_level - 1) as usize];
    }
    rig.app.ctx.run_deferred();
    if let Some(row) = course_row(&rig, "lcv") {
        rig.select_menu_containing(&row);
        let refusal = rig.transcript().last().cloned().unwrap_or_default();
        if !refusal.contains("doubles endorsement") {
            findings.push(format!(
                "the LCV refusal does not name the doubles prerequisite: {refusal}"
            ));
        }
    } else {
        findings.push("no LCV course row on the menu".to_string());
    }

    // 3. Book hazmat for real: money out, credential NOT on the license yet.
    let money_before = rig.app.ctx.profile.as_ref().map_or(0.0, |p| p.money());
    if let Some(row) = course_row(&rig, "hazmat") {
        rig.select_menu_containing(&row);
    }
    let (holds, pending, money) = {
        let p = rig.app.ctx.profile.as_ref().expect("a profile");
        (
            p.career.endorsements().contains("hazmat"),
            p.career
                .pending_credentials
                .iter()
                .any(|pc| pc.key == "hazmat"),
            p.money(),
        )
    };
    if holds {
        findings.push(
            "hazmat granted the moment the course was paid: the TSA wait is real \
             and the endorsement cannot issue until the check clears"
                .to_string(),
        );
    }
    if !pending {
        findings.push("the paid hazmat course left no pending background check".to_string());
    }
    if (money_before - money - hazmat.course_cost).abs() > 0.01 {
        findings.push(format!(
            "hazmat course charged {} instead of {}",
            money_before - money,
            hazmat.course_cost
        ));
    }

    // 4. Serve the wait on the clock; the credential activates and is heard.
    if let Some(profile) = rig.app.ctx.profile.as_mut() {
        profile.game_hours += hazmat.wait_days * 24.0 + 1.0;
    }
    let cleared = {
        let p = rig.app.ctx.profile.as_mut().expect("a profile");
        let now = p.game_hours;
        p.career.activate_pending(now, false)
    };
    let holds_now = rig
        .app
        .ctx
        .profile
        .as_ref()
        .is_some_and(|p| p.career.endorsements().contains("hazmat"));
    if !holds_now {
        findings.push("the served background check never activated the endorsement".to_string());
    }
    if !cleared.iter().any(|line| line.contains("hazmat")) {
        findings.push(format!(
            "activation spoke nothing about the hazmat endorsement: {cleared:?}"
        ));
    }
    outcome(
        "credential_ladder_gates",
        &rig,
        findings,
        "level gate, prerequisite, paid wait, and activation all honest",
    )
}

/// The menu row that books `key`'s course, by the part of its label that does
/// not move.
fn course_row(rig: &Rig, key: &str) -> Option<String> {
    let gate_label = credential(key).map(|c| c.gate_label).unwrap_or(key);
    let needle = format!("{gate_label} course:");
    rig.menu_labels()
        .into_iter()
        .find(|label| label.to_lowercase().starts_with(&needle.to_lowercase()))
        .map(|label| label.split(':').next().unwrap_or(&label).to_string())
}

/// `f"{value:,.0f}"`.
fn fmt_dollars(value: f64) -> String {
    ff_core::pyfmt::fmt_grouped(value, 0)
}

/// Fines that land the balance on exactly $0, then below; escalation and
/// ledger must agree.
pub fn money_exact_zero_and_below() -> Outcome {
    let mut rig = Rig::new(RigOptions::default());
    let mut findings: Vec<String> = Vec::new();
    // Fines, not the new-driver warning.
    rig.app.seasoned_driver();
    let start = JAKE_ZONE_FINES[0] + JAKE_ZONE_FINES[1]; // 450: two fines to zero
    if let Some(profile) = rig.app.ctx.profile.as_mut() {
        profile.set_money(start);
    }
    rig.drive.trip.position_mi = 2.0;
    rig.drive.truck_mut().engine_on = true;
    rig.drive.truck_mut().transmission.gear = 8;
    rig.drive.truck_mut().velocity_mps = 55.0 / MPH_PER_MPS;
    crate::playtest::breaker::force_grade(&mut rig.drive.trip, 0.0);
    for _ in 0..3 {
        rig.drive.truck_mut().set_engine_brake(true);
        rig.drive.update_engine_brake_zone(&mut rig.app.ctx, 0.1);
        rig.drive
            .update_engine_brake_zone(&mut rig.app.ctx, JAKE_ZONE_GRACE_S + 1.0);
        rig.drive.truck_mut().set_engine_brake(false);
        rig.drive.update_engine_brake_zone(&mut rig.app.ctx, 0.1);
        rig.app.ctx.run_deferred();
    }
    if rig.drive.jake_zone_fines != 3 {
        findings.push(format!(
            "expected 3 citations, ledger has {}",
            rig.drive.jake_zone_fines
        ));
    }
    let money = rig.app.ctx.profile.as_ref().map_or(0.0, |p| p.money());
    let expected_money = 450.0 - JAKE_ZONE_FINES.iter().sum::<f64>();
    if (money - expected_money).abs() > 0.001 {
        findings.push(format!(
            "balance {money} after 3 fines, expected {expected_money}"
        ));
    }
    for (fine, amount) in rig.lines_with("dollar fine").iter().zip(JAKE_ZONE_FINES) {
        if !fine.contains(&fmt_dollars(amount)) {
            findings.push(format!("citation spoke the wrong amount: {fine}"));
        }
    }
    if !money.is_finite() {
        findings.push("negative balance went non-finite".to_string());
    }
    let note =
        format!("450 -> 0 -> {money:.0}: escalation spoken right, negative balance survives");
    outcome("money_exact_zero_and_below", &rig, findings, &note)
}

/// XP one point under a level threshold, settle a delivery; the level-up must
/// land exactly.
///
/// Level-ups only announce at delivery settlement, never mid-drive -- so the
/// adversarial move is landing XP exactly on a threshold and checking the
/// announcement, the audio cue, and the level count are all honest, including
/// a delivery big enough to jump two thresholds at once.
///
/// Python read the private `ArrivalState._announcements`. The Rust field is
/// private too, but every line in it is folded into the public
/// `summary_lines` -- the rows the arrival screen reads out -- so that is
/// what this checks, which is also the stricter question: did the level-up
/// reach the screen a player actually hears, not just a list.
pub fn level_up_at_settlement_boundary() -> Outcome {
    let mut rig = Rig::new(RigOptions::default());
    let mut findings: Vec<String> = Vec::new();
    let target_level: usize = 5;
    if let Some(profile) = rig.app.ctx.profile.as_mut() {
        profile.career.xp = LEVEL_XP[target_level - 1] - 1.0; // one XP short of the threshold
        profile.career.deliveries = 0;
    }
    rig.drive.trip.position_mi = rig.drive.trip.total_miles();
    rig.drive.trip.game_minutes = 0.0; // comfortably on time
    let audio = rig.app.record_audio();
    let settled = rig.with_drive_on_stack(|rig, _drive| {
        let shared = rig.app.ctx.state().expect("the drive is on the stack");
        let mut arrival = {
            let mut borrowed = shared.borrow_mut();
            let driving = borrowed
                .as_any_mut()
                .downcast_mut::<crate::states::driving::DrivingState>()
                .expect("the rig's drive");
            // `ArrivalState::new` settles synchronously, as `__init__` did.
            ArrivalState::new(&mut rig.app.ctx, driving)
        };
        let settled = arrival.summary_lines.clone();
        // The audio cue and the spoken arrival screen only fire on enter(),
        // which is what push_state does for a real player landing here.
        arrival.enter_over_drive(&mut rig.app.ctx);
        rig.app.ctx.push_state_with(arrival, false);
        rig.app.ctx.run_deferred();
        settled
    });
    let level = rig.app.ctx.profile.as_ref().map_or(0, |p| p.career.level());
    if level != target_level as i64 {
        findings.push(format!(
            "one XP over the level-{target_level} threshold, career.level reads {level}"
        ));
    }
    let level_up_lines: Vec<&String> = settled
        .iter()
        .filter(|line| line.contains("Level up"))
        .collect();
    if level_up_lines.is_empty() {
        findings.push("crossing a level threshold at settlement announced nothing".to_string());
    } else if !level_up_lines[0].contains(&format!("level {target_level}")) {
        findings.push(format!(
            "level-up line does not name level {target_level}: {}",
            level_up_lines[0]
        ));
    }
    if !audio
        .borrow()
        .played
        .iter()
        .any(|(name, _, _)| name == "ui/level_up")
    {
        findings.push("level-up audio cue (ui/level_up) did not play on arrival".to_string());
    }

    // Now a delivery huge enough to skip levels entirely, the way
    // record_delivery itself computes the jump.
    // level 2, one short of level 3
    let mut skip_career = Career {
        xp: LEVEL_XP[2] - 1.0,
        ..Career::default()
    };
    let level_before_skip = skip_career.level();
    let skip_messages = skip_career.record_delivery(3000.0, 5000.0, true, 0.0, 1.0, 1.0);
    let levels_crossed = skip_career.level() - level_before_skip;
    let level_up_count = skip_messages
        .iter()
        .filter(|message| message.contains("Level up"))
        .count() as i64;
    if levels_crossed >= 2 && level_up_count < levels_crossed {
        findings.push(format!(
            "one delivery pushed career level {level_before_skip} -> {} ({levels_crossed} levels \
             in one settlement) but only {level_up_count} 'Level up!' line(s) were announced -- \
             record_delivery's level-up message only ever reports the level it lands on, so \
             every intermediate rank (and that rank's own unlock text) goes completely unspoken",
            skip_career.level()
        ));
    }
    let note =
        format!("exact-threshold level-up announced level {target_level} with its audio cue");
    outcome("level_up_at_settlement_boundary", &rig, findings, &note)
}

/// Buy into owner-operator at exactly level 18 with exact capital; refuse it at
/// level 17.
pub fn owner_op_buyin_at_level_18_boundary() -> Outcome {
    let mut findings: Vec<String> = Vec::new();
    {
        let mut app = TestApp::new();
        // Slug, not display name -- see the note in `breaker`.
        let origin = app.ctx.world.resolve_city_key("Chicago");
        app.ctx.profile = Some(Profile::named_in("Owner Boundary", &origin));
        {
            let profile = app.ctx.profile.as_mut().expect("a profile");
            profile.career.xp = LEVEL_XP[OWNER_OPERATOR_LEVEL as usize - 1]; // exactly level 18
            profile.career.deliveries = OWNER_OPERATOR_DELIVERIES;
            profile.career.reputation = OWNER_OPERATOR_REPUTATION;
            profile.set_money(OWNER_OPERATOR_BUY_IN + OWNER_OPERATOR_WORKING_CAPITAL);
            // exact dollar
        }
        let level = app.ctx.profile.as_ref().map_or(0, |p| p.career.level());
        if level != OWNER_OPERATOR_LEVEL {
            findings.push(format!(
                "LEVEL_XP boundary did not land level {OWNER_OPERATOR_LEVEL} exactly"
            ));
        }
        app.ctx.push_state(BusinessStatusState::new());
        app.ctx.run_deferred();
        let labels = crate::playtest::menu::menu_labels_of(
            &*app.ctx.state().expect("a menu").borrow(),
            &app.ctx,
        );
        if !labels
            .iter()
            .any(|label| label.contains("Buy into leased-on"))
        {
            findings.push("level 18 with exact capital: no buy-in menu item offered".to_string());
        } else {
            select_containing(&mut app, "Buy into leased-on");
            let (status, money) = {
                let profile = app.ctx.profile.as_ref().expect("a profile");
                (profile.business_status.clone(), profile.money())
            };
            if status != LEASED_OWNER_OPERATOR {
                findings.push("exact-level, exact-capital buy-in was refused".to_string());
            }
            if (money - OWNER_OPERATOR_WORKING_CAPITAL).abs() > 0.01 {
                findings.push(format!(
                    "buy-in left {money:.0}, expected exactly the \
                     {OWNER_OPERATOR_WORKING_CAPITAL:.0} working capital"
                ));
            }
        }
        drop(app); // a TestApp holds the environment lock until dropped
    }

    // One level short, plenty of money: must be refused and name level 18.
    let mut app = TestApp::new();
    let origin = app.ctx.world.resolve_city_key("Chicago");
    app.ctx.profile = Some(Profile::named_in("One Short", &origin));
    let money_before = {
        let profile = app.ctx.profile.as_mut().expect("a profile");
        profile.career.xp = LEVEL_XP[OWNER_OPERATOR_LEVEL as usize - 2]; // level 17
        profile.career.deliveries = OWNER_OPERATOR_DELIVERIES;
        profile.career.reputation = OWNER_OPERATOR_REPUTATION;
        profile.set_money(OWNER_OPERATOR_BUY_IN + OWNER_OPERATOR_WORKING_CAPITAL + 50_000.0);
        profile.money()
    };
    let level = app.ctx.profile.as_ref().map_or(0, |p| p.career.level());
    if level != OWNER_OPERATOR_LEVEL - 1 {
        findings.push("LEVEL_XP boundary did not land level 17 for the refusal case".to_string());
    }
    app.clear_speech();
    app.ctx.push_state(BusinessStatusState::new());
    app.ctx.run_deferred();
    let labels = crate::playtest::menu::menu_labels_of(
        &*app.ctx.state().expect("a menu").borrow(),
        &app.ctx,
    );
    if labels
        .iter()
        .any(|label| label.contains("Buy into leased-on"))
    {
        findings.push(
            "level 17 (one short) still offered the owner-operator buy-in menu item".to_string(),
        );
    } else {
        // The locked row is the refusal path a player reaches; press it and
        // check nothing moved and the gate is named.
        select_containing(&mut app, "Owner-operator path locked");
        let (status, money) = {
            let profile = app.ctx.profile.as_ref().expect("a profile");
            (profile.business_status.clone(), profile.money())
        };
        if status == LEASED_OWNER_OPERATOR {
            findings.push("level 17 buy-in succeeded despite the level-18 gate".to_string());
        }
        if money != money_before {
            findings.push("a refused buy-in at level 17 still took money".to_string());
        }
        let spoken = app.speech().transcript_lines();
        if let Some(last) = spoken.last() {
            if !last.contains(&format!("level {OWNER_OPERATOR_LEVEL}")) {
                findings.push(format!(
                    "refusal does not name level {OWNER_OPERATOR_LEVEL}: {last}"
                ));
            }
        }
    }
    drop(app);
    outcome_of(
        "owner_op_buyin_at_level_18_boundary",
        findings,
        "level 18 with exact capital bought in for exactly the working capital left over; \
         level 17 was refused and named the gate",
    )
}

/// Arrow to the first row containing `needle` on a bare `TestApp` and press it.
fn select_containing(app: &mut TestApp, needle: &str) {
    let labels = crate::playtest::menu::menu_labels_of(
        &*app.ctx.state().expect("a menu").borrow(),
        &app.ctx,
    );
    for _ in 0..=labels.len() {
        let focused = {
            let state = app.ctx.state().expect("a menu");
            let borrowed = state.borrow();
            crate::playtest::menu::menu_rows(&*borrowed, &app.ctx)
                .and_then(|(rows, index)| rows.get(index).cloned())
        };
        let event = crate::playtest::harness::key_event(
            if focused.as_deref().is_some_and(|row| row.contains(needle)) {
                crate::states::base::Key::Return
            } else {
                crate::states::base::Key::Down
            },
            None,
        );
        let pressing = focused.is_some_and(|row| row.contains(needle));
        app.dispatch_to_state(&event);
        app.ctx.run_deferred();
        if pressing {
            return;
        }
    }
}

/// Chain board-minimum on-time deliveries and check the streak cannot mint XP
/// off the flat completion award.
///
/// `DELIVERY_COMPLETION_XP` is a flat 150 whatever the distance, and that is
/// by design (2026-08-12, reaffirmed 2026-09-02): every settled load teaches
/// docks, paperwork and people, which is what lets a rookie's short early
/// hauls move the career at all, and each hop costs a full dispatch and dock
/// cycle of real playtime that miles do not show. What the design promises
/// instead is that the on-time streak can at most double the ROAD lesson --
/// `streak_bonus_xp` caps the bonus at the mileage XP -- so chaining trivial
/// hops earns the flat lessons and nothing more on top of them.
///
/// Two bars, both derived from that promise rather than tuned: no single hop
/// may earn more than its flat lesson plus twice its road lesson, and the XP
/// ten hops earn BEYOND their flat lessons, per mile, may not exceed twice
/// what one honest long haul earns beyond its own. Dropping the cap puts the
/// second ratio over two; the first catches any per-hop leak.
pub fn short_hop_streak_xp_farming() -> Outcome {
    let mut findings: Vec<String> = Vec::new();
    let short_miles = MIN_JOB_DISTANCE_MI; // the shortest a real dispatch board offers
    let long_miles = 500.0;
    let hops = 10;

    // A clean delivery: the flat award and the road lesson, each with the
    // clean-cargo bonus on top, are the design's own arithmetic.
    let flat_xp = DELIVERY_COMPLETION_XP * (1.0 + XP_CLEAN_BONUS);
    let road_xp = |miles: f64| miles * XP_PER_MILE_ON_TIME * (1.0 + XP_CLEAN_BONUS);

    let mut short_career = Career::default();
    for hop in 1..=hops {
        let before = short_career.xp;
        short_career.record_delivery(short_miles, 300.0, true, 0.0, 1.0, 1.0);
        let gained = short_career.xp - before;
        let ceiling = flat_xp + 2.0 * road_xp(short_miles);
        if gained > ceiling + 1e-6 {
            findings.push(format!(
                "hop {hop} at streak {} earned {gained:.1} XP, more than its flat lesson plus \
                 twice its road lesson ({ceiling:.1}): the streak is minting XP off the flat \
                 completion award",
                short_career.on_time_streak
            ));
        }
    }
    let short_xp = short_career.xp;
    let short_real_miles = short_miles * hops as f64;

    let mut long_career = Career::default();
    long_career.record_delivery(long_miles, 1800.0, true, 0.0, 1.0, 1.0);
    let long_xp = long_career.xp;

    // The edge beyond the flat lessons, per mile. With the cap the hops come
    // in at about 1.7x the haul; without it, above 2x.
    let short_beyond_flat = (short_xp - flat_xp * hops as f64) / short_real_miles;
    let long_beyond_flat = (long_xp - flat_xp) / long_miles;
    let ratio = if long_beyond_flat > 0.0 {
        short_beyond_flat / long_beyond_flat
    } else {
        f64::INFINITY
    };
    if ratio > 2.0 {
        findings.push(format!(
            "{hops} legal {short_miles:.0}-mile hops earn {short_beyond_flat:.2} XP/mi beyond \
             their flat lessons versus one {long_miles:.0}-mile haul's {long_beyond_flat:.2} \
             XP/mi beyond its own -- {ratio:.1}x, more than the doubled road lesson the \
             streak cap allows"
        ));
    }
    if short_career.on_time_streak != hops {
        findings.push(format!(
            "streak should read {hops} after {hops} clean on-time hops, reads {}",
            short_career.on_time_streak
        ));
    }
    let flat_ratio = (short_xp / short_real_miles) / (long_xp / long_miles);
    let note = format!(
        "beyond the flat lessons, short hops earn {short_beyond_flat:.2} XP/mi to the long \
         haul's {long_beyond_flat:.2} ({ratio:.1}x, within the doubled road lesson); counting \
         the flat lessons the hops are {flat_ratio:.1}x per mile, which is the per-delivery \
         award by design"
    );
    outcome_of("short_hop_streak_xp_farming", findings, &note)
}
