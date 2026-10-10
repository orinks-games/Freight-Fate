//! The hub screen while parked at a company terminal or yard
//! (`CityMenuState`).

use ff_core::models::business::{
    display_rank_for, is_owner_operator, status_label, COMPANY_DRIVER,
};
use ff_core::models::career_objectives::career_objective;
use ff_core::models::career_training::{
    is_company_training_profile, training_guidance, TrainingStage,
};
use ff_core::models::carrier_fleet::fleet_assignment_text;
use ff_core::models::economy::{pay_advance_grant, pay_advance_unavailable_reason};
use ff_core::models::enforcement;
use ff_core::models::solvency;
use ff_core::music::{select_menu_music_sequence, MenuMusicProfile};
use ff_core::pyfmt::{fmt_f, fmt_grouped, round_py_n};
use ff_core::sim::hos::{clock_text, time_of_day};
use ff_core::sim::roadside_inspection::walk_around;
use ff_core::sim::timezones::{city_zone, to_local, TimeZone, EASTERN};

use crate::app::{GameContext, Say};
use crate::cloud_saves::{
    conflict_status, eviction_status, rejection_status, save_slot_name, AUTH_PAUSED_STATUS,
};
use crate::discord_presence::PresenceState;
use crate::impl_state_for_menu;
use crate::meaningful_play::MeaningfulPlayReason;
use crate::states::base::{Label, Menu, MenuCore, MenuItem, SimpleMenuState};
use crate::states::career_setback::CareerSetbackNoticeState;
use crate::states::career_stats::{fully_rested, CareerStatsState};
use crate::states::city::weather::time_and_weather_lines;
use crate::states::city::{
    base_menu_enter, board_candidates, first_day_guidance_active, first_day_orientation_lines,
    home_terminal, open_freight_market, profile, profile_mut, record_city_duty,
    terminal_objective_clause, warm_dispatch_board, BobtailDestState, BusinessStatusState,
    EndorsementCourseState, GarageState, PayDebtState, TruckShopState, BACKUP_RESULT_WAIT_S,
    BOBTAIL_RANGE_MI, DRIVING_SCHOOL_ENABLED,
};
use crate::states::driving_school::DrivingSchoolState;
use crate::states::logbook::LogbookState;
use crate::states::main_menu::{MainMenuState, SettingsState};

use super::close_out::CloseOutCareerState;
use super::truck_status::TruckStatusState;

/// The hub screen while parked at a company terminal or yard.
pub struct CityMenuState {
    menu: MenuCore<Self>,
    confirm_sleep_rested: bool,
    /// One-shot, set by the paths that speak a line the player must hear in
    /// full just before this state is pushed -- the welcome at career
    /// creation, the line answering "Not now" on the orinks.net offer. Those
    /// lines are spoken first and this state's own announcement queues behind
    /// them instead of cutting them off mid-word. Later re-entries into the
    /// same instance (coming back from the dispatch board, say) interrupt as
    /// usual, so stale speech never delays where-you-are.
    queue_entry_announcement: bool,
    /// A manual save watching for its cloud backup result:
    /// (slot name, attempt token, seconds left to wait), or None.
    backup_watch: Option<(String, i64, f64)>,
}

/// The wall-clock zone of the city the driver is standing in.
pub(crate) fn local_zone(ctx: &GameContext) -> TimeZone {
    ctx.world
        .city(&profile(ctx).current_city)
        .map(|city| city_zone(city))
        .unwrap_or(EASTERN)
}

impl CityMenuState {
    /// `CityMenuState(ctx, queue_entry_announcement=...)`.
    pub fn new(ctx: &GameContext, queue_entry_announcement: bool) -> Self {
        CityMenuState {
            menu: MenuCore::new(&Self::title_for(ctx)),
            confirm_sleep_rested: false,
            queue_entry_announcement,
            backup_watch: None,
        }
    }

    /// The Python `title` property: the terminal's name, or "Terminal"
    /// with no career loaded.
    pub fn title_for(ctx: &GameContext) -> String {
        if ctx.profile.is_none() {
            return "Terminal".to_string();
        }
        home_terminal(ctx).name
    }

    pub fn title(&self) -> &str {
        &self.menu.title
    }

    fn show_first_day_briefing(ctx: &GameContext) -> bool {
        let p = profile(ctx);
        if !first_day_guidance_active(p) {
            return false;
        }
        if !is_company_training_profile(p) {
            return true;
        }
        training_guidance(p).stage == TrainingStage::FirstDispatch
    }

    fn first_day_briefing(&mut self, ctx: &mut GameContext) {
        let lines = first_day_orientation_lines(ctx, "");
        ctx.push_state(SimpleMenuState::readout("First-day briefing", lines));
    }

    fn career_plan(&mut self, ctx: &mut GameContext) {
        let lines = career_objective(profile(ctx)).spoken_lines();
        ctx.push_state(SimpleMenuState::readout("Career plan", lines));
    }

    fn truck_dealer(&mut self, ctx: &mut GameContext) {
        ctx.push_state(TruckShopState::new(true));
    }

    /// `_job_board`: open the dispatch board.
    pub fn job_board(&mut self, ctx: &mut GameContext) {
        open_freight_market(ctx);
    }

    fn bobtail(&mut self, ctx: &mut GameContext) {
        // Driving empty is still driving: the same pulled CDL that stops the
        // board stops a bobtail (ported from 2.0, PR #259).
        if let Some(line) = crate::states::city::cdl_drive_refusal(ctx) {
            ctx.audio.play("ui/error");
            ctx.say(&line);
            return;
        }
        let mut cands = board_candidates(ctx.world, &profile(ctx).current_city);
        cands.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
        let mut nearby: Vec<String> = cands
            .iter()
            .filter(|c| c.1 <= BOBTAIL_RANGE_MI)
            .take(8)
            .map(|c| c.0.clone())
            .collect();
        if nearby.is_empty() {
            // never strand a remote start: offer the nearest few
            nearby = cands.iter().take(3).map(|c| c.0.clone()).collect();
        }
        if nearby.is_empty() {
            ctx.audio.play("ui/error");
            ctx.say("No nearby cities are reachable from here.");
            return;
        }
        ctx.push_state(BobtailDestState::new(nearby));
    }

    fn garage_label(ctx: &GameContext) -> String {
        let region = ctx
            .world
            .city(&profile(ctx).current_city)
            .map(|c| c.region.clone())
            .unwrap_or_default();
        let price = ctx.economy.fuel_price(&region);
        let live = if ctx.economy.live_national_price().is_some() {
            ", this week's price"
        } else {
            ""
        };
        format!("Garage: fuel {} per gallon{live}", fmt_f(price, 2))
    }

    fn garage(&mut self, ctx: &mut GameContext) {
        ctx.push_state(GarageState::new());
    }

    fn business_status(&mut self, ctx: &mut GameContext) {
        ctx.push_state(BusinessStatusState::new());
    }

    fn pay_debt_label(ctx: &GameContext) -> String {
        let owed = solvency::money_text(solvency::debt_owed(profile(ctx)));
        format!("Pay down what you owe: {owed} owed")
    }

    fn pay_debt(&mut self, ctx: &mut GameContext) {
        ctx.push_state(PayDebtState::new());
    }

    fn driving_school(&mut self, ctx: &mut GameContext) {
        ctx.push_state(DrivingSchoolState::new());
    }

    fn endorsement_courses(&mut self, ctx: &mut GameContext) {
        ctx.push_state(EndorsementCourseState::new());
    }

    fn pay_advance_label(ctx: &GameContext) -> String {
        let p = profile(ctx);
        let grant = pay_advance_grant(p.money(), p.pay_advance, p.pay_advance_used_for_load);
        if grant > 0.0 {
            return format!("Request pay advance: {} dollars", fmt_grouped(grant, 0));
        }
        "Request pay advance".to_string()
    }

    /// `_pay_advance_available`.
    pub fn pay_advance_available(ctx: &GameContext) -> bool {
        let p = profile(ctx);
        // An advance is only ever offered below ten dollars of cash, so a
        // driver already having a balance collected would be offered one after
        // every single run, forever, borrowing against money that is already
        // spoken for. Dispatch stops offering instead.
        if !solvency::advance_refused_reason(p).is_empty() {
            return false;
        }
        pay_advance_grant(p.money(), p.pay_advance, p.pay_advance_used_for_load) > 0.0
    }

    /// `_request_pay_advance`.
    pub fn request_pay_advance(&mut self, ctx: &mut GameContext) {
        let refused = solvency::advance_refused_reason(profile(ctx));
        if !refused.is_empty() {
            ctx.audio.play("ui/error");
            ctx.say(&refused);
            return;
        }
        let (grant, reason) = {
            let p = profile(ctx);
            (
                pay_advance_grant(p.money(), p.pay_advance, p.pay_advance_used_for_load),
                pay_advance_unavailable_reason(
                    p.money(),
                    p.pay_advance,
                    p.pay_advance_used_for_load,
                ),
            )
        };
        if grant <= 0.0 {
            ctx.audio.play("ui/error");
            ctx.say(&reason);
            return;
        }
        let (money, advance) = {
            let p = profile_mut(ctx);
            p.earn(grant);
            p.pay_advance = round_py_n(p.pay_advance + grant, 2);
            p.pay_advance_used_for_load = true;
            (p.money(), p.pay_advance)
        };
        ctx.save_profile();
        ctx.audio.play("ui/notify");
        ctx.say(&format!(
            "Pay advance approved: {} dollars against your next load, repaid at delivery. You \
             have {} dollars, {} dollars of advance to repay.",
            fmt_grouped(grant, 0),
            fmt_grouped(money, 0),
            fmt_grouped(advance, 0)
        ));
        self.refresh(ctx, true);
    }

    fn stats(&mut self, ctx: &mut GameContext) {
        ctx.push_state(CareerStatsState::new());
    }

    /// Open the active tractor's reviewable status screen.
    pub fn truck_status(&mut self, ctx: &mut GameContext) {
        ctx.push_state(TruckStatusState::new());
    }

    fn time_weather(&mut self, ctx: &mut GameContext) {
        let lines = time_and_weather_lines(ctx);
        ctx.push_state(SimpleMenuState::readout("Time and weather", lines));
    }

    /// The driver's own pre-trip on the tractor, before a load is hooked:
    /// the same items a Level 1 inspector reads, fifteen minutes on duty.
    fn walk_around(&mut self, ctx: &mut GameContext) {
        let (start, lines) = {
            let p = profile(ctx);
            (
                p.game_hours,
                walk_around(
                    p.tire_wear_pct(),
                    p.brake_wear_pct(),
                    p.truck_damage_pct(),
                    None,
                ),
            )
        };
        let end = {
            let p = profile_mut(ctx);
            p.game_hours += crate::states::driving_core::WALK_AROUND_MIN / 60.0;
            p.hos.on_duty(crate::states::driving_core::WALK_AROUND_MIN);
            p.game_hours
        };
        record_city_duty(
            ctx,
            "on_duty_not_driving",
            start,
            end,
            "pre-trip walk-around",
        );
        ctx.save_profile();
        let body = if lines.is_empty() {
            "Nothing to write up: tires, brakes and the body would all pass.".to_string()
        } else {
            lines.join(" ")
        };
        ctx.audio.play("ui/notify");
        ctx.say(&format!(
            "Walk-around done, 15 minutes. {body} No trailer is hooked yet; walk around it again \
             at the first stop after pickup."
        ));
    }

    /// `_sleep`: a full night in the terminal bunk room.
    pub fn sleep(&mut self, ctx: &mut GameContext) {
        if fully_rested(profile(ctx)) && !self.confirm_sleep_rested {
            self.confirm_sleep_rested = true;
            ctx.audio.play("ui/warning");
            ctx.say(
                "You are already rested: fresh hours of service and no fatigue. Sleeping only \
                 moves the clock forward 10 hours. Enter again to sleep anyway.",
            );
            return;
        }
        self.confirm_sleep_rested = false;
        let (before_fatigue, start, end) = {
            let p = profile_mut(ctx);
            let before_fatigue = p.fatigue;
            let start = p.game_hours;
            p.game_hours += 10.0;
            (before_fatigue, start, p.game_hours)
        };
        record_city_duty(ctx, "sleeper_berth", start, end, "terminal sleep");
        {
            let p = profile_mut(ctx);
            p.hos.sleep();
            p.fatigue = 0.0;
            let day = p.market_day();
            p.market.advance_to(day);
        }
        ctx.save_profile();
        ctx.audio.play("ui/notify");
        let zone = local_zone(ctx);
        let hour = to_local(profile(ctx).game_hours, zone).rem_euclid(24.0);
        ctx.say(&format!(
            "You slept 10 hours and woke rested. It is \
             {}, {}. \
             Hours of service reset.",
            clock_text(hour),
            time_of_day(hour)
        ));
        if before_fatigue < 70.0 {
            ctx.award_achievement("sleep_before_exhaustion");
        }
    }

    /// Sit out the rest of a CDL suspension in one go.
    ///
    /// Serving a 60-day suspension ten hours at a time would be an
    /// accessibility problem dressed up as realism, so the terminal lets the
    /// driver wait it out and says exactly what that costs in game time.
    pub fn wait_out_suspension(&mut self, ctx: &mut GameContext) {
        let (hours, days) = {
            let p = profile(ctx);
            let record = &p.driving_record;
            (
                record.hours_left(p.game_hours),
                enforcement::days_text(record.days_left(p.game_hours)),
            )
        };
        if hours <= 0.0 {
            ctx.say("Your CDL is clear. Nothing to wait out.");
            return;
        }
        let (start, end) = {
            let p = profile_mut(ctx);
            let start = p.game_hours;
            p.game_hours += hours;
            (start, p.game_hours)
        };
        record_city_duty(ctx, "off_duty", start, end, "CDL suspension");
        {
            let p = profile_mut(ctx);
            let now = p.game_hours;
            p.driving_record.serve_until(now);
            p.hos.sleep();
            p.fatigue = 0.0;
            let day = p.market_day();
            p.market.advance_to(day);
        }
        ctx.save_profile();
        ctx.audio.play("ui/notify");
        let zone = local_zone(ctx);
        let hour = to_local(profile(ctx).game_hours, zone).rem_euclid(24.0);
        ctx.say(&format!(
            "You sat out the {days} of your suspension. Your CDL is clear and the dispatch \
             board is open again. It is {}, {}, and you are rested.",
            clock_text(hour),
            time_of_day(hour)
        ));
    }

    /// Background checks that cleared, and grants worth hearing twice.
    ///
    /// A grant announced inside the delivery-summary chatter is easy to
    /// miss (the owner once declined a reefer load he was cleared for), so
    /// each is repeated once here, somewhere quiet. A hazmat or TWIC check
    /// that came due activates here too -- the terminal is where paperwork
    /// catches up with the driver.
    fn check_credentials(&mut self, ctx: &mut GameContext) {
        let (activated, reminders) = {
            let p = profile_mut(ctx);
            let now = p.game_hours;
            let activated = p.career.activate_pending(now, false);
            let reminders = p.career.take_unacknowledged_grants();
            if !activated.is_empty() {
                p.dispatch_board_cache = None;
            }
            (activated, reminders)
        };
        if !activated.is_empty() {
            ctx.save_profile();
            ctx.audio.play("ui/notify");
        } else if !reminders.is_empty() {
            ctx.save_profile();
        }
        for line in activated.into_iter().chain(reminders) {
            ctx.say_with(line, Say::queued());
        }
    }

    /// Speak a trust-band change, once, when it changes -- never on a timer.
    ///
    /// The band now answers to the licence and to what the driver owes as
    /// well as to their service, so the line names whichever of the three is
    /// actually holding it, what the yard is doing about the equipment, and
    /// whether the career has slowed. All of it is available on demand from
    /// Career stats; none of it is ever repeated on a timer.
    fn check_standing(&mut self, ctx: &mut GameContext) {
        let line = {
            let p = profile_mut(ctx);
            let band = enforcement::standing_band(p);
            let heard = p.driving_record.trust_band_heard.clone();
            if band == heard {
                return;
            }
            let first_time = heard.is_empty();
            let improved = !first_time && enforcement::worst_band(&[band, &heard]) == heard;
            p.driving_record.trust_band_heard = band.to_string();
            if first_time && band == enforcement::TRUST_FULL {
                return; // a clean driver is never told they are fine
            }
            let mut line = enforcement::dispatch_trust_line(p);
            if improved && band == enforcement::TRUST_FULL && !p.owns_equipment() {
                line = format!("{line} {}", fleet_assignment_text(p));
            }
            line
        };
        ctx.say_with(line, Say::queued());
    }

    /// Take the seat or the truck when a balance has passed the ceiling.
    ///
    /// Only ever here, at the terminal, and never out on the road: both of
    /// these remove the tractor the driver is sitting in, and doing that
    /// mid-run would take the truck out from under them. Returns True when a
    /// notice is now owed, so the caller can stop talking about the terminal
    /// and put the notice on screen instead.
    fn check_career_setback(&mut self, ctx: &mut GameContext) -> bool {
        let applied = {
            let p = profile_mut(ctx);
            if solvency::setback_pending(p) {
                false
            } else if solvency::company_termination_due(p) {
                solvency::apply_company_termination(p);
                true
            } else if solvency::repossession_due(p) {
                solvency::apply_repossession(p);
                true
            } else {
                return false;
            }
        };
        if applied {
            ctx.mark_meaningful_play(MeaningfulPlayReason::BusinessChanged);
            ctx.save_profile();
        }
        let notice = CareerSetbackNoticeState::new(ctx);
        ctx.push_state(notice);
        true
    }

    /// A company driver the carrier will no longer keep on the insurance.
    pub fn check_carrier_termination(&mut self, ctx: &mut GameContext) {
        if !enforcement::carrier_termination_due(profile(ctx)) {
            return;
        }
        let former = {
            let p = profile_mut(ctx);
            let former = p.carrier_name.clone();
            p.driving_record.carrier_terminations += 1;
            p.carrier_key = enforcement::LAST_CHANCE_CARRIER_KEY.to_string();
            p.carrier_name = enforcement::LAST_CHANCE_CARRIER_NAME.to_string();
            p.dispatch_board_cache = None;
            former
        };
        ctx.mark_meaningful_play(MeaningfulPlayReason::BusinessChanged);
        ctx.save_profile();
        ctx.audio.play("ui/error");
        ctx.say(&format!(
            "{former} has ended your employment. Your safety record is past what their \
             insurance will carry, so your seat and your assigned truck go back to the yard. \
             {} will take you on: lower pay and shorter freight. Your money, your levels, and \
             everything you own stay as they are. Once your dispatch trust is full again, \
             Business status lets you move to another carrier.",
            enforcement::LAST_CHANCE_CARRIER_NAME
        ));
    }

    fn logbook(&mut self, ctx: &mut GameContext) {
        ctx.push_state(LogbookState::new(None));
    }

    /// `_save`: write the career and, with cloud backup on, back it up now.
    pub fn save(&mut self, ctx: &mut GameContext) {
        ctx.save_profile();
        ctx.audio.play("ui/notify");
        // A manual save is the player asking for certainty (Shane's report,
        // 2026-08-14): the cloud backup runs right away and the result is
        // spoken, because a silent background upload is indistinguishable
        // from no backup for a screen reader user. update() speaks exactly
        // one result line when the attempt lands.
        // Sandbox runs (driving school, forced playtest scenarios) never reach
        // disk in save_profile, so their throwaway profile must never reach
        // the cloud either -- it would overwrite the real career's slot.
        let sandbox = ctx.school_sandbox || ctx.playtest_sandbox;
        let cloud = ctx.cloud_saves_service();
        if cloud.enabled() && ctx.profile.is_some() && !sandbox {
            let p = profile(ctx);
            let token = cloud.backup_now(&p.name, serde_json::Value::Object(p.to_dict()));
            if let Some(token) = token {
                self.backup_watch = Some((save_slot_name(&p.name), token, BACKUP_RESULT_WAIT_S));
                ctx.say("Game saved. Backing up.");
                return;
            }
        }
        ctx.say("Game saved.");
        let cloud = ctx.cloud_saves_service();
        if cloud.identity().is_some() && !cloud.enabled() {
            // The player set up an account but backup is off: say so here,
            // where they asked to save, instead of only in the Online menu.
            // With no account configured, saving stays local and quiet.
            let status = cloud.status();
            ctx.say_with(status, Say::queued());
        }
    }

    /// One spoken line per cloud backup outcome family, reusing the
    /// standing status wording wherever one already exists.
    fn backup_outcome_text(name: &str, outcome: &str) -> String {
        if outcome == "accepted" {
            return "Backed up to the cloud.".to_string();
        }
        if let Some(name) = outcome.strip_prefix("accepted:evicted:") {
            return eviction_status(name);
        }
        if outcome == "unchanged" {
            // "Already backed up" alone reads as a refusal to a driver who
            // just fuelled and bought tires: they know the career changed, so
            // being told there is nothing to send sounds like the game is
            // wrong (Shane, 2026-08-15). It is not -- every one of those
            // actions saves, and a save backs up on its own, so the server
            // really is current. Say the part that answers his worry: the
            // copy up there matches the one on this computer.
            return "Already backed up. The cloud copy matches this computer's save.".to_string();
        }
        if let Some(reason) = outcome.strip_prefix("rejected:") {
            return rejection_status(name, Some(reason));
        }
        if outcome == "conflict" {
            return conflict_status(name);
        }
        if outcome == "auth" {
            return AUTH_PAUSED_STATUS.to_string();
        }
        "The backup will keep retrying in the background.".to_string()
    }

    fn settings(&mut self, ctx: &mut GameContext) {
        ctx.push_state(SettingsState::new());
    }

    fn close_out(&mut self, ctx: &mut GameContext) {
        let state = CloseOutCareerState::new(ctx);
        ctx.push_state(state);
    }

    fn quit_to_main_menu(&mut self, ctx: &mut GameContext) {
        ctx.save_profile();
        ctx.say("Progress saved.");
        MainMenuState::arm_update_check(&ctx.settings);
        ctx.reset_to(MainMenuState::new());
    }
}

impl Menu for CityMenuState {
    fn menu(&self) -> &MenuCore<Self> {
        &self.menu
    }

    fn menu_mut(&mut self) -> &mut MenuCore<Self> {
        &mut self.menu
    }

    fn enter(&mut self, ctx: &mut GameContext) {
        self.confirm_sleep_rested = false;
        // Entering -- first arrival or coming back from a submenu -- drops any
        // backup announcement still owed to an earlier save: spoken text is
        // the interface, and a stale "Backed up to the cloud" landing minutes
        // later would describe a save the player has moved past.
        self.backup_watch = None;
        self.menu.title = Self::title_for(ctx);
        let sequence =
            select_menu_music_sequence(ctx.profile.as_ref().map(|p| p as &dyn MenuMusicProfile));
        let refs: Vec<&str> = sequence.iter().map(String::as_str).collect();
        ctx.play_music_sequence("menu", &refs);
        ctx.audio.set_ambient(Some("poi/facility_gate"));
        // Parked at the terminal the truck's location is known: warm the live
        // weather now so the next drive starts on real conditions, not
        // "loading" (the provider shares observations per station).
        let city = profile(ctx).current_city.clone();
        ctx.warm_real_weather(&city);
        warm_dispatch_board(ctx.world, &city);
        base_menu_enter(self, ctx);
    }

    // Moving off the Sleep item withdraws its pending double-press
    // confirmation, so a stale "press Enter again" can never sleep you
    // silently later.
    fn move_by(&mut self, ctx: &mut GameContext, delta: i64) {
        self.confirm_sleep_rested = false;
        let core = self.menu_mut();
        if core.items.is_empty() {
            return;
        }
        let n = core.items.len() as i64;
        core.index = ((core.index as i64 + delta).rem_euclid(n)) as usize;
        ctx.audio.play("ui/menu_move");
        self.speak_current(ctx);
    }

    fn jump(&mut self, ctx: &mut GameContext, index: usize) {
        self.confirm_sleep_rested = false;
        let core = self.menu_mut();
        if core.items.is_empty() {
            return;
        }
        core.index = index.min(core.items.len() - 1);
        ctx.audio.play("ui/menu_move");
        self.speak_current(ctx);
    }

    fn exit(&mut self, ctx: &mut GameContext) {
        self.backup_watch = None;
        ctx.audio.set_ambient(None);
    }

    fn presence(&self, ctx: &GameContext) -> Option<PresenceState> {
        let city = match ctx.profile.as_ref() {
            Some(p) if !p.current_city.is_empty() => ctx.world.spoken_city(&p.current_city, None),
            _ => String::new(),
        };
        let detail = if city.is_empty() {
            String::new()
        } else {
            format!("{city} service area")
        };
        Some(PresenceState::new("At the terminal", &detail))
    }

    fn announce_entry(&mut self, ctx: &mut GameContext) {
        let interrupt = !self.queue_entry_announcement;
        self.queue_entry_announcement = false;
        let line = {
            let p = profile(ctx);
            let world = ctx.world;
            let (city_name, city_state) = world
                .city(&p.current_city)
                .map(|c| (c.name.clone(), c.state.clone()))
                .unwrap_or_else(|_| (p.current_city.clone(), String::new()));
            let terminal = home_terminal(ctx);
            let business = status_label(&p.business_status);
            let rank = display_rank_for(p);
            let first_day = terminal_objective_clause(p);
            // A licence that is not clear is said here, every time, because it
            // decides what the rest of this screen can do. A career that is
            // over says so in full: what stopped, what stays, and the one
            // way to remove it.
            let cdl = if enforcement::career_ended(p) {
                format!(" {}", enforcement::career_ended_text())
            } else if p.driving_record.suspended(p.game_hours) {
                format!(" {}.", enforcement::career_menu_status(p))
            } else {
                String::new()
            };
            // What the record is costing right now -- the carrier's review
            // hold or the insurer's surcharge -- said here because the
            // terminal is where both are decided, and nothing else said it
            // outside the stats screen (owner, 2026-09-12).
            let record = enforcement::record_consequence_text(p);
            let record = if record.is_empty() {
                String::new()
            } else {
                format!(" {record}")
            };
            format!(
                "Parked at {} in {} \
                 service area, {city_state}. {} with \
                 level {}, {}.{cdl}{record} \
                 You have {} dollars. \
                 {first_day}",
                terminal.spoken_name(),
                ff_core::speech_text::the_city(&city_name),
                crate::states::city::py_capitalize(business),
                rank.level,
                rank.title,
                fmt_grouped(p.money(), 0)
            )
        };
        ctx.say_with(line, Say::new().interrupt(interrupt));
        let current = self.current_text(ctx);
        ctx.say_with(current, Say::queued().review(false));
        // A career-changing setback goes first and takes the screen: nothing
        // else the terminal has to say survives being read over the top of it.
        if self.check_career_setback(ctx) {
            return;
        }
        self.check_carrier_termination(ctx);
        self.check_standing(ctx);
        self.check_credentials(ctx);
    }

    fn build_items(&mut self, ctx: &mut GameContext) -> Vec<MenuItem<Self>> {
        let p = profile(ctx);
        let status = if p.business_status.is_empty() {
            COMPANY_DRIVER
        } else {
            p.business_status.as_str()
        };
        let mut items: Vec<MenuItem<Self>> =
            vec![
                MenuItem::new("Dispatch board", |s: &mut Self, ctx| s.job_board(ctx)).help(
                    "Loads from local freight facilities. New company hires get an assigned \
                     load, load choice opens with seniority.",
                ),
            ];
        // Owner-operator business stays off a company driver's menus (owner,
        // 2026-09-28): the carrier assigns the tractor, and the buy-in lives
        // under Business status.
        if is_owner_operator(status) {
            items.push(
                MenuItem::new("Truck dealer", |s: &mut Self, ctx| s.truck_dealer(ctx))
                    .help("Tractors at the local dealer. Buy a truck or switch between yours."),
            );
        }
        // Owner-operators only (owner ruling, 2026-08-20): a company
        // driver's tractor goes where dispatch sends it -- repositioning
        // on a whim is the owner's privilege because it is the owner's
        // fuel. Company drivers will get ASSIGNED repositions from
        // dispatch instead (ROADMAP).
        if is_owner_operator(status) {
            items.push(
                MenuItem::new("Bobtail to a nearby city", |s: &mut Self, ctx| {
                    s.bobtail(ctx)
                })
                .help(
                    "Drive empty to a nearby city for its dispatch board. Costs fuel and hours \
                     of service, no load, no pay.",
                ),
            );
        }
        items.push(
            MenuItem::new(
                Label::dynamic(|_s: &Self, ctx| Self::garage_label(ctx)),
                |s: &mut Self, ctx| s.garage(ctx),
            )
            .help(
                "Fuel and repairs. Company drivers bill the carrier account, owner-operators \
                 pay their own.",
            ),
        );
        items.push(
            MenuItem::new("Business status", |s: &mut Self, ctx| {
                s.business_status(ctx)
            })
            .help("Carrier, rank, next business unlock, and owner-operator buy-in."),
        );
        items.push(
            MenuItem::new("Career stats", |s: &mut Self, ctx| s.stats(ctx)).help(
                "Level, reputation, dispatch trust, driving record and CDL, balance and debt, \
                 endorsements, lifetime numbers, and rest.",
            ),
        );
        items.push(
            MenuItem::new("Licenses and training", |s: &mut Self, ctx| {
                s.endorsement_courses(ctx)
            })
            .help(
                "Carrier certificates, CDL endorsements, the TWIC port card and the LCV \
                 certificate. Courses cost money and game time, some add a background check \
                 wait.",
            ),
        );
        if DRIVING_SCHOOL_ENABLED {
            items.push(
                MenuItem::new("Driving school", |s: &mut Self, ctx| s.driving_school(ctx)).help(
                    "Spoken lessons on a practice road where nothing counts: no money, no \
                     wear, no hours.",
                ),
            );
        }
        items.push(
            MenuItem::new("Truck status", |s: &mut Self, ctx| s.truck_status(ctx))
                .help("Assignment, eligibility, fuel, condition, wear, grime, and snow chains."),
        );
        items.push(
            MenuItem::new("Walk around the truck", |s: &mut Self, ctx| {
                s.walk_around(ctx)
            })
            .help(
                "A pre-trip walk-around: what a roadside inspector would find on the tractor. \
                 Fifteen minutes on duty.",
            ),
        );
        items.push(
            MenuItem::new("Time and weather", |s: &mut Self, ctx| s.time_weather(ctx))
                .help("Clock, career day, and the conditions outside."),
        );
        items.push(
            MenuItem::new("Logbook", |s: &mut Self, ctx| s.logbook(ctx))
                .help("Recent Record of Duty Status entries."),
        );
        items.push(
            MenuItem::new("Sleep 10 hours", |s: &mut Self, ctx| s.sleep(ctx))
                .help("Fresh hours of service and zero fatigue. Clock advances 10 hours."),
        );
        items.push(
            MenuItem::new("Save game", |s: &mut Self, ctx| s.save(ctx)).help("Saves the career."),
        );
        items.push(
            MenuItem::new("Settings", |s: &mut Self, ctx| s.settings(ctx))
                .help("Units, transmission, volumes, weather, voices, update channel, and pacing."),
        );
        items.push(
            MenuItem::new("Quit to main menu", |s: &mut Self, ctx| {
                s.quit_to_main_menu(ctx)
            })
            .help("Save your career and return to the title menu."),
        );
        if Self::show_first_day_briefing(ctx) {
            items.insert(
                1,
                MenuItem::new("First-day briefing", |s: &mut Self, ctx| {
                    s.first_day_briefing(ctx)
                })
                .help("Repeats the carrier, terminal, business costs, and first objective."),
            );
        } else {
            items.insert(
                1,
                MenuItem::new("Career plan", |s: &mut Self, ctx| s.career_plan(ctx))
                    .help("The next career objective and the dispatch it suggests."),
            );
        }
        let record = &p.driving_record;
        if record.suspended(p.game_hours) && !record.lifetime_disqualified {
            items.insert(
                1,
                MenuItem::new("Wait out the CDL suspension", |s: &mut Self, ctx| {
                    s.wait_out_suspension(ctx)
                })
                .help(
                    "Sits out the suspension in one go. The clock jumps to the day it clears, \
                     money, truck, and record untouched.",
                ),
            );
        }
        if record.lifetime_disqualified {
            // Last on the list, after Quit: the one destructive act on this
            // screen, never where a driver arrowing for the board lands on it.
            items.push(
                MenuItem::new("Close out this career", |s: &mut Self, ctx| {
                    s.close_out(ctx)
                })
                .help(
                    "Removes this career's save from this computer and its cloud backups from \
                     your account, for good. A confirmation follows.",
                ),
            );
        }
        if Self::pay_advance_available(ctx) {
            items.insert(
                3,
                MenuItem::new(
                    Label::dynamic(|_s: &Self, ctx| Self::pay_advance_label(ctx)),
                    |s: &mut Self, ctx| s.request_pay_advance(ctx),
                )
                .help("Cash against your next load, repaid from the next delivery settlement."),
            );
        }
        if !solvency::out_of_pocket_options(p).is_empty() {
            // right behind the garage: the money cluster
            let at = items
                .iter()
                .position(|item| matches!(&item.label, Label::Text(t) if t == "Business status"))
                .unwrap_or(items.len());
            items.insert(
                at,
                MenuItem::new(
                    Label::dynamic(|_s: &Self, ctx| Self::pay_debt_label(ctx)),
                    |s: &mut Self, ctx| s.pay_debt(ctx),
                )
                .help("Your own cash toward the balance you owe. Cash never goes below zero."),
            );
        }
        items
    }

    fn update(&mut self, ctx: &mut GameContext, dt: f64) {
        ctx.update_music_rotation(dt);
        let Some((name, token, remaining)) = self.backup_watch.clone() else {
            return;
        };
        let outcome = match ctx.cloud_saves_service().outcome_for(&name, token) {
            Some(outcome) => outcome,
            None => {
                let remaining = remaining - dt;
                if remaining > 0.0 {
                    self.backup_watch = Some((name, token, remaining));
                    return;
                }
                // Still in flight after the bounded wait: the worker keeps
                // retrying on its own, and the player is told so once.
                "network".to_string()
            }
        };
        self.backup_watch = None;
        ctx.audio.play("ui/notify");
        ctx.say_with(Self::backup_outcome_text(&name, &outcome), Say::queued());
    }

    fn go_back(&mut self, ctx: &mut GameContext) {
        ctx.audio.play("ui/menu_back");
        self.quit_to_main_menu(ctx);
    }
}

impl_state_for_menu!(CityMenuState);
