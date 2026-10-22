//! Every module-level constant of `freight_fate/states/driving_core.py`, in
//! source order, with the comments that explain why each number is what it
//! is (they carry tester history). Values imported from the sim layer are
//! re-exported under the names the driving layer uses.

use ff_core::sim::enforcement_observe::OBSERVE_LEEWAY_MPH;
use ff_core::sim::hos;
use ff_core::sim::trip_models::{
    DESTINATION_LOCAL_APPROACH_MI, RAMP_MAX_MPH as TRIP_RAMP_MAX_MPH, SCALE_BYPASS_MPH,
};

pub const HAZARD_SAFE_MPH: f64 = 25.0;
// Weather that spans the road (a gust, fog, a squall) is braked for down to
// the speed the weather is safe at, this far under it, and never higher than
// the second figure: assumed values, a margin a driver keeps rather than a
// measurement.
pub const HAZARD_WEATHER_MARGIN_MPH: f64 = 5.0;
pub const HAZARD_SPANNING_MAX_MPH: f64 = 45.0;
// A fixed object in your lane -- debris, a stopped vehicle -- cannot be
// rolled over at 25: clearing it by brake alone means coming nearly to a
// stop and easing around. A lane change remains the no-time-lost answer.
pub const HAZARD_CREEP_MPH: f64 = 8.0;
// An animal struck at 65 mph, in truck damage points. Assumed, not
// measured: a deer or a dog dents the bumper and grille of a tractor built
// to shrug it off; an elk or a cow can take out the radiator. Scaled by the
// truck's speed in `animal_outcome`.
pub const ANIMAL_STRIKE_LARGE_PCT: f64 = 6.0;
pub const ANIMAL_STRIKE_DEER_PCT: f64 = 3.0;
pub const ANIMAL_STRIKE_SMALL_PCT: f64 = 1.0;
pub const MPH_PER_MPS: f64 = 2.23694;

// Roadside mechanic: a field patch, not a garage restoration.
pub const FIELD_REPAIR_DAMAGE_PCT: f64 = 25.0; // damage level the patch repairs down to
pub const MECHANIC_CALLOUT_FEE: f64 = 500.0;
pub const MECHANIC_RATE_PER_PCT: f64 = 110.0; // premium over the garage's 85 per percent
pub const MECHANIC_WAIT_MIN: f64 = 90.0; // game minutes waiting for the truck to be fixed
                                         // A full breakdown is the emergency version of that call-out: the truck is
                                         // dead where it stopped, so the fee is the premium and the repair only gets
                                         // the truck moving again -- it does not put it right. The rate per percent is
                                         // the same road rate; the difference is the call-out and the hours.
pub const BREAKDOWN_CALLOUT_FEE: f64 = 1200.0;
pub const BREAKDOWN_REPAIR_DAMAGE_PCT: f64 = 60.0; // still deep in reduced power afterwards
pub const BREAKDOWN_REPAIR_MIN: f64 = 180.0; // game minutes at the side of the road
pub const BREAKDOWN_REPUTATION_HIT: f64 = 5.0; // a company driver's record instead of their wallet
                                               // A carrier does not send a driver back out in equipment it just had to
                                               // recover: it grounds the tractor and covers the bill, and what the driver
                                               // loses is the day and their standing. Waiting on the yard to bring iron out
                                               // to a stranded truck is slower than a mechanic patching one, which is the
                                               // whole trade -- the company driver keeps their money and pays in hours.
pub const GROUNDED_SWAP_MIN: f64 = 300.0; // game minutes waiting on a replacement tractor
pub const GROUNDED_SPARE_DAMAGE_PCT: f64 = 15.0; // a yard spare is used equipment, not a new one
                                                 // How long a driver may crawl an out-of-service truck before road service
                                                 // reaches them anyway. Real seconds: it is a window to clear a live lane,
                                                 // not a way to finish the run at ten miles an hour.
pub const OUT_OF_SERVICE_RECOVERY_GRACE_S: f64 = 60.0;
// What the carrier charges a company driver at settlement for damage its own
// safety committee ruled preventable. The carrier still pays the repair --
// this is the deductible and the voided safety bonus, which is how a real
// company driver feels damage in the wallet without being handed the whole
// invoice. Scaled by the deepest band the run reached, because a driver who
// spent it in limp mode did something to get there.
pub const PREVENTABLE_DAMAGE_DEDUCTIBLE: f64 = 250.0; // per band reached, at full preventable share
pub const PREVENTABLE_REPUTATION_PER_BAND: f64 = 1.5; // standing lost per band reached
                                                      // How fast limp mode winds the road-speed cap down: the same "about 2 mph per
                                                      // second of comfortable braking" the dropped-speed-limit grace is built on, so
                                                      // the cap never snaps a speed out from under the driver.
pub const LIMP_CAP_RAMP_MPH_PER_S: f64 = 2.0;
// Chaining up is done kneeling on the shoulder in the weather that made it
// necessary. Real crews quote twenty to thirty minutes for a drive-axle set;
// doing it in the dark by headlamp costs more time and much more out of the
// driver. Removal is quick by comparison. A set is now five chains mounted
// (four drive tires and one trailer tire), so 30 minutes: about five a
// chain. UNVERIFIED: no official time source; trucker guidance runs 20 to 45.
pub const CHAIN_INSTALL_MIN: f64 = 30.0;
pub const CHAIN_INSTALL_NIGHT_MULT: f64 = 1.6;
pub const CHAIN_REMOVE_MIN: f64 = 10.0;
/// Stop planning's no-chains reminder plays this far before a chain-law
/// corridor in season: a reminder, well before the 1-mile chain-law cue.
pub const CHAIN_REMINDER_LEAD_MI: f64 = 15.0;
// A broken chain is one cross link or one chain, not the whole set, and it
// is fixed on the shoulder: Caltrans tells drivers to carry "chain repair
// links", and Washington requires two spare chains for this case (WAC
// 204-24-050(2)(f)). A repair puts the set back at 80 percent worn, under
// the slap warning, so a fresh repair doesn't slap at once.
pub const CHAIN_REPAIR_MIN: f64 = 15.0;
pub const CHAIN_REPAIR_WEAR_RESTORE: f64 = 20.0;
// A worn chain loosens and slaps the fender before it lets go. Tightening it
// takes about five minutes and buys back a little wear, once per install.
pub const CHAIN_SLAP_WARN_PCT: f64 = 85.0;
pub const CHAIN_TIGHTEN_MIN: f64 = 5.0;
pub const CHAIN_TIGHTEN_WEAR_RESTORE: f64 = 5.0;
pub const CHAIN_INSTALL_FATIGUE: f64 = 6.0;
pub const CHAIN_INSTALL_NIGHT_FATIGUE: f64 = 10.0;
pub const CHAIN_REMOVE_FATIGUE: f64 = 2.0;
// Rolling into an active chain law out of compliance: the checkpoint at the
// bottom of the grade is staffed often enough that gambling is a bad trade.
// What the citation costs is priced in models/enforcement with every other fine.
pub const CHAIN_LAW_CHECKPOINT_CHANCE: f64 = 0.6;
// California staffs every active R-control at a fixed checkpoint, and
// screening ("TS" on the Caltrans chain-control chart) stops every truck.
// The other states' staffing is UNVERIFIED and keeps the 0.6 roll.
pub const CHAIN_LAW_CHECKPOINT_CHANCE_CA: f64 = 1.0;

/// How often the checkpoint chance for `state` (two-letter code).
pub fn chain_law_checkpoint_chance(state: Option<&str>) -> f64 {
    if state == Some("CA") {
        CHAIN_LAW_CHECKPOINT_CHANCE_CA
    } else {
        CHAIN_LAW_CHECKPOINT_CHANCE
    }
}
// Running the ramp-end red or stop sign is seen when someone is watching the
// crossroad -- a patrol car on the corner, a camera on the mast arm -- and
// most of the time nobody is. Same shape as CHAIN_LAW_CHECKPOINT_CHANCE: a
// flat, named, seeded roll, not a difficulty knob, priced in models/enforcement
// with every other fine. Under half, so the lesson of a blown light stays the
// cross traffic, not the paperwork.
pub const SIGNAL_RUN_CATCH_CHANCE: f64 = 0.35;
// Road wear service at branded travel centers -- the brand IS the capability
// (amenities.classify_brand): Love's and Speedco run dedicated tire bays at
// close to the terminal-garage rate and turn the truck around fast; TA and
// Petro full-service shops also reline brakes; every other major travel
// center can mount tires at a road markup. Engine overhauls stay in the
// terminal garage, and a landmark like Big Buck's fixes nothing.
pub const ROAD_TIRE_SPECIALIST_COST_PER_PCT: f64 = 50.0; // tire brands, near the garage's 45
pub const ROAD_TIRE_SPECIALIST_MIN: f64 = 45.0;
pub const ROAD_TIRE_COST_PER_PCT: f64 = 60.0; // everyone else marks tire work up
pub const ROAD_TIRE_MIN: f64 = 75.0;
pub const ROAD_BRAKE_COST_PER_PCT: f64 = 55.0; // road-shop premium over the garage's 40
pub const ROAD_BRAKE_MIN: f64 = 120.0;
/// A chain set bought on the road: the terminal garage's 750 dollars plus
/// the third road shops add to tire work (60 a percent against the
/// garage's 45). ASSUMED; truck stops on chain corridors stock sets, and
/// nobody publishes a single price.
pub const ROAD_CHAIN_SET_COST: f64 = 1000.0;
/// Paying at the counter and stowing the set in the side box, on duty.
/// ASSUMED, as the terminal garage's 10 minutes.
pub const ROAD_CHAIN_BUY_MIN: f64 = 10.0;
pub const FUEL_STOP_MIN: f64 = 20.0; // fueling is on-duty-not-driving work
/// Fraction of tank remaining that fires the once-per-threshold low-fuel cue.
/// Re-arms only after a refill climbs back above this line.
pub const LOW_FUEL_WARN_FRACTION: f64 = 0.15;
/// Miles per gallon the fuel range assumes before this run has burned
/// enough to measure its own. ASSUMED: the burn model is calibrated for
/// about 6.5 to 7 at a 60 mph cruise, and a loaded climb burns more.
pub const ASSUMED_RANGE_MPG: f64 = 6.0;
/// What a run must have driven and burned before its own average stands in
/// for [`ASSUMED_RANGE_MPG`], and the band that average is held to.
pub const RANGE_MEASURE_MIN_MI: f64 = 15.0;
pub const RANGE_MEASURE_MIN_GAL: f64 = 2.0;
pub const RANGE_MPG_FLOOR: f64 = 3.0;
pub const RANGE_MPG_CEILING: f64 = 10.0;
pub const INSPECTION_MIN: f64 = 15.0; // inspection lane: they take you in
/// The driver's own pre-trip walk-around, on duty. ASSUMED: 49 CFR 396.13
/// sets no time; carriers budget about fifteen minutes.
pub const WALK_AROUND_MIN: f64 = 15.0;
pub const WAVE_THROUGH_MIN: f64 = 2.0; // scale wave-through after you pull in
pub const OUT_OF_SERVICE_MIN: f64 = hos::SLEEP_MIN;
// Dynamiting the parking brake: pulling the valve at speed slams the spring
// brakes on and grinds flat spots into the tires. Above this speed the set
// is treated as the violent emergency move it really is; the tread cost
// scales with speed (55 mph costs about a percent and a half of tread).
pub const DYNAMITE_MIN_MPH: f64 = 5.0;
pub const FLAT_SPOT_WEAR_PCT_PER_MPH: f64 = 0.028;
pub const STOP_PULL_IN_MIN: f64 = 5.0;
pub const STOP_PULL_IN_WAIT_S: f64 = 1.0;

// Highway exits: signal inside the window, slow enough to make the ramp.
// The window is the *minimum*; at speed it grows so the spoken callout stays
// far enough out to hear, arm, and brake despite time compression -- see
// _exit_window_mi(), which mirrors the zone-warning lead scaling.
pub const EXIT_WINDOW_MI: f64 = 5.0; // how far out X can arm the upcoming exit, at minimum
pub const EXIT_WARNING_REAL_S: f64 = 25.0; // target real seconds from callout to the ramp
pub const EXIT_WINDOW_MAX_MI: f64 = 20.0;
// Keep the exact announced destination exit available for the same real-time
// budget even if coasting or automatic braking shrinks the dynamic window.
pub const DESTINATION_EXIT_RESPONSE_GRACE_S: f64 = EXIT_WARNING_REAL_S;
// Spoken distance anchors for an armed exit; a signal-on announcement miles
// out gets buried under canyon pacenotes without them.
pub const EXIT_COUNTDOWN_MILESTONES_MI: [f64; 3] = [2.0, 1.0, 0.5];
// Roadside chatter (billboards, landmarks) stays quiet inside this distance of
// an exit being taken: the countdown's last two calls and the gore's take line
// are the only things the driver needs to hear there. The countdown's own
// one-mile mark, so it is not a new number.
pub const EXIT_APPROACH_QUIET_MI: f64 = EXIT_COUNTDOWN_MILESTONES_MI[1];
// The pacenote cue tone leans hard toward the curve's side of the field.
pub const EXIT_COMMIT_WINDOW_MI: f64 = 0.4; // generous gore-window grace after the marker
/// Where the exit lane opens: the length of its taper ahead of the gore, in
/// miles. DERIVED: a parallel deceleration lane's taper runs 15:1 to 25:1
/// (AASHTO Green Book 2018, 10.9.6.6.2), so 180 to 300 feet to reach a 12-foot
/// lane; the long end, 300 feet. MUTCD 11th ed. 2E.25 stands the exit direction
/// sign at the start of this taper, which is where the cab calls the lane.
pub const EXIT_TAPER_MI: f64 = 300.0 / 5280.0;
/// How far out the exit's turn signal starts clicking, in miles. Owner ruling
/// (2026-09-24): a driver flicks it on a quarter to half a mile out, never
/// miles of blinker; the long end, which is also the last advance guide sign
/// (MUTCD 11th ed. 2E.23, 1/2 mi). X commits to the exit wherever it is
/// pressed; this is only when the blinker runs.
pub const EXIT_BLINKER_MI: f64 = EXIT_COUNTDOWN_MILESTONES_MI[2];
/// How far out lane keeping on full starts moving to the right lane for an
/// exit it is taking: the countdown's first anchor, where a manual driver is
/// asked for the same lane.
pub const EXIT_KEEP_RIGHT_MI: f64 = EXIT_COUNTDOWN_MILESTONES_MI[0];
pub const EXIT_CANCEL_GUARD_MI: f64 = 1.0; // inside this, X keeps the signal; a second press cancels
pub const EXIT_TAP_HOLD_S: f64 = 0.35; // a Right press this short is a tap, not held steering
pub const AEB_BUDGET_MARGIN: f64 = 1.2; // emergency braking leads the physics budget by this factor
pub const AEB_LEAD_S: f64 = 0.5; // plus this flat lead, covering brake heat added during the stop
                                 // The assist brakes on the SERVICE brakes, and the escalation to the emergency
                                 // application is judged on what the truck is actually doing rather than on what
                                 // a full application ought to deliver. The measured deceleration is smoothed
                                 // over this long, is not judged at all until the pedal has been down for one
                                 // smoothing constant, and the shortfall has to hold for the confirm window --
                                 // one noisy frame is not a losing stop.
pub const AEB_DECEL_SMOOTHING_S: f64 = 0.4;
pub const AEB_ESCALATE_CONFIRM_S: f64 = 0.5;
// Floor on the driver's own time between hearing a hazard warning and the
// assist taking the truck. Long enough to hear the sentence out and get on
// the pedal: the warning is spoken, so a window shorter than the words is no
// window at all. A dodgeable hazard adds LANE_TAP_CHANGE_S on top, because
// "or change lanes" names a maneuver that takes that long to finish.
pub const HAZARD_MIN_REACTION_S: f64 = 3.0;
// How far a truck stopped short of the bar has to close before stopping again
// earns another "Stopped N feet short" line. Assumed, not measured: about a
// car length, so a crawl bobbing across the stopped line is one stop, and a
// real hop toward the bar is a new one.
pub const RAMP_CREEP_REARM_MI: f64 = 25.0 / 5280.0;
// Any faster and you blow past the exit. Defined in the portable layer,
// because the arrival speed zones are built from the same number: the
// destination approach must never cap below the speed the ramp needs.
pub const RAMP_MAX_MPH: f64 = TRIP_RAMP_MAX_MPH;
// How far under a ramp's own design speed automatic control aims. Was the
// gap between a flat 40 and a flat 45; now it is the gap itself, applied to
// whatever number the ramp earns.
pub const RAMP_CRUISE_HEADROOM_MPH: f64 = 5.0;
// Ceiling on the light throttle exit speed assistance uses to HOLD that target
// once it has slowed the truck to it. Deliberately small: the assist is keeping
// a truck rolling to its own gore, not driving it.
pub const EXIT_HOLD_MAX_THROTTLE: f64 = 0.45;
// READ: the most automatic control eases below road speed on the MAINLINE for
// an armed exit. TxDOT Roadway Design Manual 9.4.4 accepts up to 10 mph of
// slowing in the through lanes where the full deceleration length is
// impractical; NCHRP Research Report 1081 (2024) measured diverge speeds of
// 58 to 70 mph at 70 mph sites. The ramp's own number is reached past the
// gore, in the deceleration lane.
pub const EXIT_MAINLINE_EASE_MPH: f64 = 10.0;
// How far over what the gore accepts adaptive cruise or the keeper may be
// before exit speed assistance takes the pedals from it: the cruise loop's
// own two-mph brake deadband, which it answers inside anyway.
pub const EXIT_ASSIST_CONTROLLER_MARGIN_MPH: f64 = 2.0;
// A ramp's length is not a constant any more: `Trip::ramp_length_mi`.
// Ramp terminals: where the off-ramp meets the surface road there is usually
// a light or a stop sign (diamond interchanges), occasionally free flow
// (cloverleafs). The control comes from baked OSM traffic_signals/stop nodes
// on the ramp links when available, else a seeded urban/rural heuristic.
pub const RAMP_ACCESS_MI: f64 = 0.12; // terminal-to-driveway stretch at the ramp's end
                                      // Rolling stop-bar countdown milestones (spoken as each is crossed while
                                      // moving): the bar needs a position the way an exit does, or a driver
                                      // stops a quarter mile short and creeps blind (owner playtest, 2026-07-19).
pub const RAMP_GAP_MILESTONES_FT: [i64; 4] = [1000, 500, 300, 150];
pub const RAMP_GAP_MILESTONES_M: [i64; 4] = [300, 150, 100, 50];
// Parking-sensor tick for the stop bar (owner ask, 2026-07-19): inside
// this range a center tick speeds up as the bar closes -- rate carries
// the distance, silence means stopped. Placeholder ui/tick until the
// audio-design pass gives the bar its own voice (steering-sound RFC).
pub const RAMP_BAR_TICK_RANGE_MI: f64 = 300.0 / 5280.0;
// The bar's final leeway: inside this, still rolling, the ticks fuse into a
// continuous tone -- be nearly stopped or eat the intersection (owner spec,
// written straight into the manual, 2026-07-27). About sixty feet.
pub const RAMP_BAR_SOLID_MI: f64 = 0.012;
pub const RAMP_BAR_TICK_SLOW_S: f64 = 1.1; // period at the edge of the range
pub const RAMP_BAR_TICK_FAST_S: f64 = 0.15; // period at the bar
                                            // Ground covered while the driver hears the cue and gets to the pedal. The bar
                                            // is the one place in the game where the cue IS the instrument, so its range
                                            // has to pay for the listening as well as the stopping.
pub const RAMP_BAR_REACTION_S: f64 = 1.5;
// Safety-call re-arm: Ctrl always silences (a screen-reader reflex must
// never be fought), but a curve call cut inside this window re-speaks
// once, refreshed, after the delay -- IF the bend is still ahead and the
// truck is still hot. A stale warning re-spoken is worse than none.
pub const CRITICAL_CALL_WINDOW_S: f64 = 8.0;
pub const CRITICAL_RESPEAK_DELAY_S: f64 = 2.0;
pub const RAMP_CONTROL_ANNOUNCE_MI: f64 = 0.38; // where the terminal callout fires on the ramp

// Game tuning, not a claim about any field signal plan. FHWA's planning-level
// examples commonly begin at 60-second cycles; these four seeded profiles run
// 62, 68, 74, or 80 real seconds so every terminal does not feel metronomic.
// https://ops.fhwa.dot.gov/publications/fhwahop08024/chapter3.htm
pub const RAMP_LIGHT_RED_S: f64 = 30.0; // shortest profile; includes clearance below
pub const RAMP_LIGHT_RED_STEP_S: f64 = 4.0;
pub const RAMP_LIGHT_GREEN_S: f64 = 26.0; // shortest profile; room for a loaded departure
pub const RAMP_LIGHT_GREEN_STEP_S: f64 = 2.0;
pub const RAMP_LIGHT_PROFILE_COUNT: i64 = 4;
// The player's light remains red for this final part of its red interval while
// the cross street is also held. The MUTCD makes red clearance an engineering
// decision; this fixed game interval gives even this model's slowest vehicle
// enough time to accelerate from its 45-foot cross bar and clear the 55-foot
// conflict window rather than pretending to reproduce a real site.
// https://mutcd.fhwa.dot.gov/pdfs/11th_Edition/part4.pdf#page=120
pub const RAMP_LIGHT_RED_CLEARANCE_S: f64 = 7.0;
// MUTCD 4F.17 recommends 3 to 6 seconds and says a timing plan's yellow must
// not vary cycle by cycle, so one figure serves every game profile. The ITE
// kinematic interval, y = t + v / 2a, at a 35 mph ramp approach (51 ft/s)
// with a loaded truck's 8 ft/s2 is 1 + 3.2 = 4.2 s -- but this driver learns
// of the change by ear, and "Light yellow." takes about 1.5 s to say, so the
// perception term is 2.5 s and the interval 5.7. Four seconds put the red on
// the end of the spoken yellow (owner, exit 286A, 2026-09-23: "the light
// transitions seem way too short"). Rounded to the MUTCD ceiling.
pub const RAMP_LIGHT_YELLOW_S: f64 = 6.0;
pub const RED_STOP_MPH: f64 = 3.0; // at or under this you have honored a red or a stop sign
                                   // The stop bar's continuous tone level (BAR_SOLID_VOLUME) is re-exported from
                                   // ff_core::sound_catalog by the prelude, so the road and the Learn game
                                   // sounds screen can never drift apart.
                                   // A direction change engages only after the control is held this long at a
                                   // standstill: a confirm-tap on the brake must never grab reverse.
pub const DIRECTION_CHANGE_HOLD_S: f64 = 0.6;
pub const RAMP_TERMINAL_GRACE_MI: f64 = 0.02; // rolling this far past the bar commits the violation
                                              // Route-transition assistance at the terminal: the assist starts braking when
                                              // stopping at the bar needs this much deceleration, maps needed deceleration
                                              // to brake application against the nominal full-service figure, and holds the
                                              // stop once the truck is within the hold window short of the bar.
pub const RAMP_ASSIST_DECEL_START_MPS2: f64 = 0.6;
// End a completed snub below the engagement threshold to avoid brake chatter.
pub const RAMP_ASSIST_DECEL_RELEASE_MPS2: f64 = 0.35;
// The destination approach: how hard the assist is willing to shed to ARRIVE
// stopped, and the brake it uses. Gentler than the ramp figure -- an approach
// is a street chain with a gate at the end, and the point is that the stop is
// comfortable rather than a grab.
// Seconds of road the arrival shed starts EARLY, so the brake has time to
// build before the stop profile binds. Not a change to where the truck
// stops -- the profile still aims at the arrival point -- only to when it
// begins. The exit approach prices its own road the same way
// (APPROACH_REACTION_S); this is the shorter arrival-speed version of the
// same budget, because a truck already down to street speed needs the
// pressure built, not a driver reaction.
pub const APPROACH_ASSIST_REACTION_S: f64 = 3.0;
pub const RAMP_ASSIST_FULL_DECEL_MPS2: f64 = 3.0;
pub const RAMP_ASSIST_HOLD_MI: f64 = 60.0 / 5280.0;
// How far the demand has to fall below the pedal the assist is already holding
// before it eases off. Easing costs nothing; coming back on is charged a whole
// brake application by the air system, so a servo that chases every dip in the
// demand empties the tanks on one approach.
pub const RAMP_ASSIST_RELEASE_BAND: f64 = 0.05;
// And how far it has to climb ABOVE that pedal before the servo presses
// harder. The air system charges a whole application for every RISE, so a
// demand that creeps up a ten-thousandth of a pedal per frame -- which is
// what a stop profile does while the truck is not quite holding its rate --
// is charged sixty applications a second. The 2026-09-20 approach sweep
// measured 31,556 of its 35,112 applications coming from that creep, and
// Albany Company Yard was the first chain whose shed ran long enough to put
// the tanks on the floor and set the spring brakes 2,992 feet short of the
// gate. Same size as the release band: the pedal moves when the move is
// worth an application, in either direction.
pub const RAMP_ASSIST_APPLY_BAND: f64 = RAMP_ASSIST_RELEASE_BAND;
// The destination approach assist's last lengths to the gate. The dock opens
// only AT the point, so the arrival profile aims to reach it at a walk, not at
// rest: an assist that targets zero speed at the point converges on a stop
// short of it the moment anything besides its own brake takes speed off --
// rolling resistance, drag, an upgrade, the servo's release band -- and with
// the throttle held to zero it can never roll the remainder (Jerry, Hobbs,
// 2026-08-22: nine metres short, brake held, dock never opened). Two miles an
// hour sits inside the gate's own creep band (DELIVERY_PARK_MPH) and well
// above docking speed, so the point still has something to stop.
pub const ARRIVAL_CREEP_MPH: f64 = 2.0;
// A facility lane is still road until the final truck-lengths. Hold a modest
// roll there instead of either coasting to zero short of the entrance or
// crawling the whole ramp at walking pace. The last 200 feet remain the
// established two-mile-an-hour gate creep.
pub const FACILITY_LANE_ROLL_MPH: f64 = 12.0;
pub const ARRIVAL_FINAL_CREEP_MI: f64 = 200.0 / 5280.0;
// The most pedal the creep may use to hold that walk against the road: enough
// for a grossed-out rig on the steepest gate approach, never a lunge.
pub const ARRIVAL_CREEP_THROTTLE_MAX: f64 = 0.35;
/// The pedal the assist may use on a chain ramp's roll from the bar to the
/// streets: road, not a gate, so it runs the truck up to the posted limit
/// instead of walking it (owner, 2026-09-01: "speed up to the limit to get
/// to that point as efficiently as it can"). Short of full so a loaded
/// truck still shifts cleanly through the ramp.
pub const APPROACH_ROLL_THROTTLE_MAX: f64 = 0.8;
// Proportional gain from creep shortfall (m/s) to throttle, on top of the
// feed-forward that balances the road (TruckState.hold_throttle).
pub const ARRIVAL_CREEP_THROTTLE_GAIN: f64 = 0.5;
pub const GREEN_ROLL_MPH: f64 = 25.0; // green lets you roll the terminal up to this
pub const STOP_ROLL_CLIP_MPH: f64 = 15.0; // blowing a stop sign this fast clips cross traffic
                                          // A yield taken in a real gap is legal at or under this -- the whole point of
                                          // the sign is that a clear road never demands the stop.
pub const YIELD_ROLL_MPH: f64 = 15.0;
pub const RED_RUN_DAMAGE: f64 = 0.3; // collision severity for running the red
pub const STOP_ROLL_DAMAGE: f64 = 0.2; // lighter clip for blowing the stop sign
                                       // A blown terminal met by a semi, a bus or a box truck is a broadside, not a
                                       // clip: the same violation scaled to what actually arrived, which is what
                                       // makes the roll dice rather than a fixed price (owner playtest 2026-07-15,
                                       // "sometimes a T-bone"). Cars and pickups keep the base severity.
pub const HEAVY_CROSS_HIT_MULTIPLIER: f64 = 2.5;
pub const HEAVY_CROSS_CLASSES: [&str; 3] = ["semi", "bus", "box truck"];
// Heuristic control mix when OSM has none baked: (signal, stop) cumulative
// weights; the remainder is free flow. Urban terminals are mostly signalized.
// A ramp onto ANOTHER FREEWAY is a system interchange: it ends in a merge,
// never a stop sign and never a light. Nothing stops traffic where an
// interstate meets an interstate. 4,999 of the world's 18,011 exits -- 27.8
// percent -- lead to one, and every single one of them was rolling the dice
// below, so half the rural ones were being given stop signs that cannot
// exist (owner, 2026-08-17: "no stop signs at the end of ramps"). Matched on
// the interchange's own `via`, which is baked from OSM. See
// `freeway_via_matches` for the compiled pattern.
pub const FREEWAY_VIA_PATTERN: &str = r"\bI[-\s]?\d";

// Cumulative (signal, stop) thresholds for a ramp terminal OSM never tagged:
// roll < signal_w is a light, < stop_w is a stop sign, above is free flow.
//
// The signal-to-stop SPLIT is calibrated, not invented. Of the 8,205 baked
// interchanges where OSM does tag a control, the split is 88.7 / 11.3 within
// URBAN_RADIUS_MI of a route city and 64.3 / 35.7 outside it -- both readings,
// since a signal and a stop sign are equally tagged when either exists. The
// old rural pair asserted 30 / 50, handing out roughly twice the stop signs
// reality has and half the lights, which is what made every country exit feel
// like a farm road.
//
// The free-flow SHARE is the assumed part and is left where it was (5 percent
// urban, 20 percent rural), because it cannot be read: OSM records controls
// that exist and says nothing at all where a ramp merges freely, so absence of
// a tag is not evidence of absence of a control. The far-end bake (2026-08-20)
// shrank the pool these weights govern: exits whose ramp chains provably merge
// onto another motorway carry ramp_control=none outright, and a proven surface
// terminal suppresses the via guess above, so the dice now decide only the
// exits topology could not judge plus the surface terminals OSM left untagged.
// The shares are NOT recalibrated for that smaller pool -- free flow at a
// surface terminal (a channelized merge onto a frontage road) still exists and
// still cannot be read, so the assumption stands, declared here.
const URBAN_FREE_FLOW: f64 = 0.05;
const RURAL_FREE_FLOW: f64 = 0.20;
pub const RAMP_CONTROL_URBAN_WEIGHTS: (f64, f64) = (0.843, 1.0 - URBAN_FREE_FLOW);
pub const RAMP_CONTROL_RURAL_WEIGHTS: (f64, f64) = (0.514, 1.0 - RURAL_FREE_FLOW);
// Grace past the end of the ramp before a taken-but-never-stopped exit counts
// as blown. Distance alone is not enough under trip pacing: at 40 mph the same
// half mile can pass in barely a second, before the driver can hear the arrival
// cue and set the brake. Require both this distance and a real-time reaction
// window. A driver who keeps rolling still misses the stop promptly.
pub const RAMP_OVERSHOOT_MI: f64 = 0.5;
// Blowing the destination terminal at the end of the ramp costs a scripted
// loop-back through the next safe turnaround, charged the same game minutes as
// the missed destination exit and the missed facility gate -- the same maneuver
// a road up or down. The lost time is the whole consequence; there is no fine.
pub const RAMP_TERMINAL_MISS_LOOP_MIN: f64 = 20.0;
// The missed destination exit's own loop-back, same maneuver and same clock.
pub const EXIT_MISS_LOOP_MIN: f64 = 20.0;
pub const RAMP_SPEECH_WPM_MIN: f64 = 30.0;
pub const RAMP_SPEECH_WPM_MAX: f64 = 60.0;
pub const RAMP_ARRIVAL_REACTION_S: f64 = 3.0;
pub const RAMP_ARRIVAL_GRACE_MIN_S: f64 = 8.0;
// Where the synthetic destination exit sits, and equally the local approach
// road the arrival zones assume behind it when the facility has no usable
// record of its own -- one road, described once (``sim.trip_models``).
pub const DESTINATION_EXIT_BEFORE_END_MI: f64 = DESTINATION_LOCAL_APPROACH_MI;
// A real interchange counts as the destination exit only inside this final
// approach window. Routes that finish on rural highways carry no baked
// interchanges, and without the floor the scan crowned the last labeled exit
// anywhere on the route -- one playtest got its "destination exit" on I-39 in
// Wisconsin, 1,158 miles from the Montana receiver, and taking it settled the
// load from there (transcripts, 2026-07-16). Past the window the synthetic
// end-of-route exit takes over.
pub const DESTINATION_EXIT_SCAN_WINDOW_MI: f64 = 25.0;
pub const UNLOADING_MIN: f64 = 45.0; // receiver dock work before settlement
pub const UNLOADING_WAIT_S: f64 = 1.5;

// Discrete lanes on top of the LaneKeeping drift model. With steering assist
// on, holding the wheel across the lane line is the lane change; with assist
// off, a Left/Right arrow tap runs a timed change with signal clicks.
pub const LANE_MIN_MPH: f64 = 10.0; // below this there is nothing to steer
                                    // A full-lane-keeping tap change, and the pace a held key crosses a lane at.
pub const LANE_TAP_CHANGE_S: f64 = ff_core::sim::lane::LANE_CHANGE_S;
pub const LANE_SIGNAL_CLICK_S: f64 = 0.45; // turn-signal cadence during a tap change
pub const MERGE_WINDOW_S: f64 = 8.0; // time to vacate a coned-off lane after the warning
pub const MERGE_BARRELS_DAMAGE: f64 = 0.25; // collision severity for riding into the barrels
pub const SIDESWIPE_DAMAGE: f64 = 0.35; // changing lanes into occupied space costs more
                                        // The mirror check's clearances live with the trip now, so the hazard call
                                        // that names the open side reads the same numbers the dodge's arrival does.
pub use ff_core::sim::traffic_manager::{DODGE_CLEARANCE_AHEAD_MI, DODGE_CLEARANCE_BEHIND_MI};
// The steering lane cue: the panned position tock, played on its own while a
// lane move is underway rather than waiting for the I key, and clicked off
// like a turn signal when the move is done. Owner request 2026-08-15: taking
// an exit with the lane work yours means HOLDING a position at the right of
// the lane, and that position was the one thing on the road a blind driver
// could not hear.
pub const STEER_CUE_MIN_MPH: f64 = 2.0; // same floor as the lane locator: stopped tires steer nothing
pub const STEER_CUE_ARM_S: f64 = 0.5; // a steering hold this long is a move, not a drift correction
pub const STEER_CUE_TOCK_S: f64 = 0.9; // the locator's own beat, so the two are one sound
pub const STEER_CUE_TOCK_FAST_S: f64 = 0.35; // the beat it closes to as the exit lane position fills
pub const STEER_CUE_HOLD: &str = "lane_steer"; // dead-man's-switch latch name, held on the audio clock
pub const STEER_CUE_CANCEL_VOL: f64 = 0.45; // the self-cancel click, quieter than the signal going on
pub const KEEP_RIGHT_NAG_S: f64 = 45.0; // left-lane camping before the CB calls you out
pub const KEEP_RIGHT_REPEAT_S: f64 = 75.0; // spacing for repeat nags while still camping
pub const KEEP_RIGHT_MIN_MPH: f64 = 45.0; // lane discipline only matters at highway speed
pub const PASSING_LOOKAHEAD_MI: f64 = 0.6; // slower right-lane traffic inside this justifies the left lane

pub const KEEPER_MIN_MPH: f64 = 2.0; // the speed keeper just needs the truck rolling
                                     // How far the keeper's own trim may pull away from the feed-forward. On the
                                     // flat at a zone speed the road itself asks for a tenth to a third of the
                                     // pedal, so half a pedal of trim on top is a generous authority and the
                                     // assist still reads unhurried -- which is what the number was chosen for.
                                     // It is NOT a ceiling on the whole pedal: read that way (as it was until
                                     // 2026-08-24) the keeper simply settled wherever half throttle balanced the
                                     // hill and never said so.
pub const KEEPER_MAX_THROTTLE: f64 = 0.5;
// How far under its number the keeper has to be falling before it owns up.
// The same fraction of a zone speed that CRUISE_DROOP_MPH is of a highway
// set speed: six in sixty-two is a tenth, and three in a twenty-five or
// thirty-five zone is about the same.
pub const KEEPER_DROOP_MPH: f64 = 3.0;
pub const KEEPER_GAP_SECONDS: f64 = 3.0; // follow queued traffic at this gap, down to a stop
pub const CRUISE_MIN_MPH: f64 = 20.0; // cruise control needs road speed to hold
/// The keeper's zone name while it bridges open road below cruise's holding
/// speed: a hazard or a stop that left the truck crawling with the session
/// still armed. It builds toward the posted limit behind whatever traffic
/// held the truck down and hands to adaptive cruise at `CRUISE_MIN_MPH`, the
/// way the acceleration lane does at its merge speed.
pub const KEEPER_OPEN_ROAD_BRIDGE: &str = "open road";
pub const CRUISE_STEP_MPH: f64 = 5.0; // set-point change per Accel/Coast (+/-) tap
pub const CRUISE_MAX_MPH: f64 = 85.0; // highest cruise set point (top US posted limits)

// Speed-hold gains. The feed-forward term (``Truck.hold_throttle``) carries
// the grade; P and I only trim from there. The old loop was integral-only at
// 0.08 per mph-second, which needed over ten seconds just to reach full
// throttle -- a 4 percent climb had already taken twenty mph off the truck by
// then (bench trace, 2026-07-25: 62 set, 31.9 mph low, and the sag never came
// back). Trim is bounded so a grade the engine genuinely cannot pull does not
// wind the integrator into a spike when the road levels out.
pub const CRUISE_P_GAIN: f64 = 0.055; // throttle per mph of error
pub const CRUISE_I_GAIN: f64 = 0.05; // throttle per mph-second of error
pub const CRUISE_TRIM_LIMIT: f64 = 0.4; // how far trim may pull away from the feed-forward
                                        // How fast the working setpoint eases toward the set speed. The loop is pure
                                        // proportional above, so a set speed far over the current one (resume to 85
                                        // from a crawl) used to land the whole error on the pedal at once and command
                                        // wide-open throttle -- governor-loud on the flat, and on a downgrade an
                                        // over-rev past redline during the automatic box's between-shift hold, which
                                        // charged engine wear (tester Shane, ~3 percent on a 12 percent grade). Cruise
                                        // now chases a working setpoint that ramps from the engage speed at this bounded
                                        // rate, so the per-frame error stays small and the throttle stays moderate
                                        // while the box upshifts normally. A loaded rig accelerates in the low single
                                        // digits of mph per second; 2.5 is brisk enough to feel like a resume yet inside
                                        // what the truck can comfortably do, so speed keeps up and the error never grows
                                        // into a governor slam.
pub const CRUISE_ACCEL_MPH_PER_S: f64 = 2.5;
// Belt and suspenders for the downgrade: even where gravity does the
// accelerating, cruise must not add throttle as the engine nears the governor.
// Demand tapers to nothing across this fraction of max RPM below the redline, so
// the descent-control and retarder staging own the grade and cruise is simply
// off the pedal -- never fighting the retarder, never feeding an over-rev.
pub const CRUISE_RPM_CEILING_BAND: f64 = 0.08;
pub const CRUISE_COAST_MPH: f64 = 2.0; // feed-forward eases to nothing across this much overspeed
                                       // The droop band: how far under its number cruise tolerates before the truck
                                       // counts as beaten by the hill rather than working through a dip. Fleet cruise
                                       // parameters use the same idea (a configurable underspeed), and it is what
                                       // keeps the spoken hand-back off a pull cruise recovers from on its own.
pub const CRUISE_DROOP_MPH: f64 = 6.0;
pub const CRUISE_FLOORED_THROTTLE: f64 = 0.98; // pedal genuinely on the floor, not merely deep
pub const CLIMB_CUE_COOLDOWN_S: f64 = 120.0; // a mountain is many pulls; say it once a hill
                                             // ...and only once the grade has genuinely won (dev fix f23a97ec, ported):
                                             // a road the G key calls level never counts as a climb, a shift's open
                                             // driveline is not evidence (drive_ratio is 0 mid-shift), and the condition
                                             // has to hold rather than catch one frame -- a limit rise raising the target
                                             // had cruise flooring the pedal on a slight grade and announcing defeat at
                                             // 71 mph while accelerating to 77 (playtest transcript, 2026-07-27).
pub const CRUISE_GRADE_BEATEN_PCT: f64 = 1.5;
pub const CRUISE_GRADE_BEATEN_S: f64 = 3.0;
// Holding the target from above. Cutting fuel was cruise's only answer, so any
// downgrade gentler than the descent assist's 2.5 percent trigger carried the
// truck past the set speed and kept it there (bench trace: 2 percent down, 62
// set, 67.2 held) -- a speeding strike cruise handed the driver. The retarder
// answers first because its heat goes out the exhaust; the drums only join in
// when the jake cannot hold, so a long grade does not fade them away.
pub const CRUISE_JAKE_OVER_MPH: f64 = 0.75; // over the target by this much and the jake steps in
pub const CRUISE_JAKE_STEP_S: f64 = 4.0; // quiet time between stage changes; the jake is loud
                                         // Under the target by this much and the retarder steps DOWN a stage -- and
                                         // inside it cruise does not fuel against a retarder holding a grade, since
                                         // any throttle cuts the jake. Assumed: wider than the snub's own band (it
                                         // ends half a mile an hour under), so the snub finishing never reads as
                                         // over-retarding, and inside the droop band that calls a climb beaten.
pub const CRUISE_JAKE_UNDER_MPH: f64 = 2.0;
// A step in the opposite direction to the last waits this long. Assumed:
// three of the steps above, so a stage just raised has a hill's worth of
// road to prove itself before it can come back off -- the owner heard
// 3, 0, 2, 1, 2, 1 in thirty seconds down a 5.8 percent grade (2026-09-24).
pub const CRUISE_JAKE_REVERSE_S: f64 = 12.0;
// Descent control engages at this grade (a fraction, downhill) and lets go
// where the road stops being a grade at all (`on_downgrade`).
pub const DESCENT_CONTROL_GRADE: f64 = 0.025;
// How close to the automatic's retarder ceiling (`JAKE_MAX_RPM`) the revs
// may come on a downgrade before cruise snubs to hold the gear. Assumed: a
// fraction of the ratio step between gears, so the snub lands well before
// the box's protective upshift and releases a guard band lower.
pub const DESCENT_RPM_GUARD: f64 = 50.0;
// The drums are the last resort, and they only come out in snubs: apply,
// recover the target, release. Dragging a light application down a long grade
// is how a real truck fades its brakes and empties its air tanks -- and the
// sim models both, so cruise did exactly that to itself (bench trace,
// 2026-07-25: 6 percent down, a tenth of brake held steady, 125 psi to 74 in
// twenty-two seconds, spring brakes on, an emergency stop on a downhill).
pub const CRUISE_BRAKE_OVER_MPH: f64 = 2.5; // retarder maxed and still this far over: snub
pub const CRUISE_SNUB_UNDER_MPH: f64 = 0.5; // snub runs until this far back under the target
pub const CRUISE_SNUB_BRAKE: f64 = 0.3; // a real application, not a drag
                                        // Interactive descent control's ceiling while a grade lasts. A cap on the
                                        // working target only -- it must never be written into the set speed.
pub const DESCENT_SAFE_MAX_MPH: f64 = 55.0;

/// Closing on a lower target, cruise's service trim fades in from this far
/// over and reaches its full over/30 a mile an hour later. A switched edge
/// pumps the pedal, and air is charged per application.
pub const CRUISE_CLOSE_FEATHER_FROM_MPH: f64 = 1.0;

// When has a hill BEATEN the descent control, as opposed to merely being held?
//
// The descent twin of CRUISE_GRADE_BEATEN_*, and it did not exist until
// 2026-08-25. "Descent control can't hold this grade. Brake."
// was said on the first frame the truck was this far over the ceiling
// interactive mode had just imposed -- which on a 75 mph road with cruise set
// at 80 is the instant a dip starts, before the control has done anything at
// all. Measured over the owner's own I-70 run (Silverthorne to Glenwood
// Springs, seed 4242): eight warnings, every one of them while the drums were
// applied between 0.44 and 0.70 and the truck was LOSING three to five miles
// an hour a second, on quarter-mile dips the G key called level three seconds
// later. The three guards mirror the climb side: still genuinely over, still
// genuinely gaining, and holding rather than catching one frame.
pub const DESCENT_BEATEN_MPH: f64 = 10.0; // this far over what the control works to
pub const DESCENT_BEATEN_S: f64 = 3.0; // ...and still losing it for this long
                                       // The line between "this is being held" and "this is getting away", in mph per
                                       // second of net acceleration. It is the G readout's own number: the spoken
                                       // "Jake stage 3 is not holding it" and "Speed is building" branches have judged
                                       // a descent by it since the readout shipped, and the descent control now asks
                                       // the same question of the same physics so the two can never disagree.
pub const GRADE_HOLDING_MPH_PER_S: f64 = 0.2;
// Predictive cruise: the road profile ahead, read the way a real system reads
// a stored 3D map (Volvo I-See, Detroit Intelligent Powertrain Management).
// The preview distance is what those systems use, and the baked grade segments
// resolve to a median half a mile, so a mile and a half is a real look ahead
// rather than a smoothed guess.
pub const PCC_PREVIEW_MI: f64 = 1.5;
pub const PCC_PREVIEW_STEP_MI: f64 = 0.1;
pub const PCC_GRADE_MIN: f64 = 0.015; // shallower than this is not a hill worth planning for
pub const PCC_GRADE_WINDOW_MI: f64 = 0.3; // a hill is a sustained window, not one spike
pub const PCC_PREBUILD_MPH: f64 = 3.0; // momentum banked before a climb, at a 4 percent pull
pub const PCC_CREST_SAG_MPH: f64 = 4.0; // speed given up rather than fought for at a summit
pub const PCC_DESCENT_SHAVE_MPH: f64 = 2.0; // taken off before a downgrade, at 5 percent
                                            // The crest gets its own, much shorter horizon: the summit is "close" when the
                                            // road inside this distance has already gone flat. Judged on the full preview
                                            // instead, a three-mile pull read as cresting from a mile and a half out.
pub const PCC_CREST_WINDOW_MI: f64 = 0.4;
// Grade advisories, spoken whether or not cruise is on. A downgrade this steep
// is the one a driver has to plan for -- gear and retarder before the hill, not
// halfway down it.
pub const GRADE_WARN_PCT: f64 = 3.0; // steep enough to call out, either direction
pub const GRADE_WARN_CLEAR_PCT: f64 = 2.0; // hysteresis: under this the grade is behind you
                                           // A grade that keeps its sign but gets materially worse is a new thing to plan
                                           // for: two percent down that becomes six is the hill gear goes in for, and the
                                           // road never flattens in between to announce it.
pub const GRADE_WARN_STEEPEN_PCT: f64 = 1.0;
pub const GRADE_WARN_LOOKAHEAD_MI: f64 = 0.75; // how far ahead the advisory reaches
pub const GRADE_WARN_SCAN_MI: f64 = 15.0; // how far a grade's run is measured before giving up
pub const GRADE_WARN_STEP_MI: f64 = 0.25; // sampling stride; matches the baked segment length
                                          // A grade gentler than the steep line's release runs, for the G key's "for
                                          // another", until the road stops going its way at all -- this close to
                                          // level. The number the readout has always used (0.002 as a fraction).
pub const GRADE_READOUT_LEVEL_PCT: f64 = 0.2;
pub const GRADE_WARN_MIN_MPH: f64 = 25.0; // no advisories while crawling; nothing to plan for
                                          // A grade has to last to be worth planning for. The baked segments are around
                                          // half a mile each and the mountain corridors are full of short punchy dips: a
                                          // 4 percent blip lasting a third of a mile costs a couple of mph and warning
                                          // about it buried the hills that matter. Unfiltered, Knoxville to Asheville
                                          // spoke 76 advisories in 116 miles; at three quarters of a mile it speaks 4.
pub const GRADE_WARN_MIN_RUN_MI: f64 = 0.75;
pub const GRADE_WARN_RESCAN_MI: f64 = 0.1; // how far the truck rolls between advisory scans
pub const ACC_BASE_GAP_SECONDS: f64 = 3.0; // clear-weather adaptive cruise gap, normal setting
                                           // The driver's choice of cushion, in seconds of clear-weather gap. Real
                                           // adaptive cruise offers three, and so does this. Weather still ADDS to
                                           // whichever one is chosen (see _acc_gap_seconds), so picking "close" never
                                           // stops the truck opening up on ice.
                                           //
                                           // Every one of them sits far clear of enforcement_observe.TAILGATE_GAP_S
                                           // (1.2 seconds), and that is a floor rather than a coincidence: the closest
                                           // setting the game offers must never be a setting that gets the driver
                                           // ticketed for choosing it. A test pins the margin.
                                           //
                                           // `ff_core::settings` replicated this table for its own validation; it is the
                                           // one definition now and the prelude re-exports it (`ACC_GAP_CHOICES`,
                                           // `ACC_GAP_DEFAULT`, `acc_gap_seconds`).
pub const ACC_LIMIT_OFFSET_MPH: f64 = 5.0; // predictive ACC holds this far over the posted
                                           // limit -- a with-traffic pace, sized to sit right at OVERSPEED_WARN_MPH
                                           // without arming it, and comfortably under the 9 mph speeding-strike
                                           // threshold. Cruise used to overshoot it on every downgrade and chime at the
                                           // driver for a speed cruise itself had picked; the grade band is bounded now
                                           // (see CRUISE_JAKE_OVER_MPH and the snub constants) rather than the pace
                                           // being cut.
                                           // Zones the driver is warned about in advance and that cruise pre-brakes for,
                                           // holding their limit exactly rather than the usual with-traffic offset. The
                                           // construction merge taper is deliberately absent: it posts a higher limit
                                           // ahead of the work zone, and aiming at it reached the barrels too fast.
pub const RESTRICTED_ZONE_REASONS: [&str; 2] = ["construction", "heavy traffic"];
pub const ACC_LIMIT_LOOKAHEAD_MIN_MI: f64 = 0.25;
pub const ACC_LIMIT_LOOKAHEAD_MAX_MI: f64 = 1.5;
pub const ACC_LIMIT_LOOKAHEAD_STEP_MI: f64 = 0.1;
pub const ACC_LIMIT_COMFORT_DECEL_MPS2: f64 = 1.0;
pub const ACC_FOLLOW_DECEL_MPS2: f64 = 0.35; // gentle planned deceleration while closing on a lead
pub const ACC_FOLLOW_CUE_COOLDOWN_S: f64 = 30.0; // minimum quiet time between "Traffic ahead" cues
pub const ACC_STOPPED_CANCEL_S: f64 = 20.0; // hand control back this many seconds before stopped traffic
pub const ENGINE_SHUTDOWN_SAFE_MPH: f64 = 5.0; // prevent accidental kill-switch use at speed
pub const DELIVERY_PARK_MPH: f64 = 3.0; // within this, the gate prompts you to stop
pub const DOCKING_MAX_MPH: f64 = 0.5; // dock/settle/rest actions need a complete stop
pub const PARKING_BRAKE_SETTLE_MAX_MPH: f64 = 3.0; // spring brakes finish a walking-pace stop immediately
                                                   // How often a facility gate re-speaks its stop instruction while the truck is
                                                   // still rolling past it. The one-shot warnings latch, so without a cadence a
                                                   // player who overshot the gate at speed heard them once, minutes ago, and got
                                                   // silence for the rest of the drive (playtest 2026-07-22: six minutes and the
                                                   // on-time bonus lost three miles past a delivery entrance).
pub const GATE_REMINDER_INTERVAL_S: f64 = 10.0;
// Minimum quiet time between curve-assist spoken cues. The assist state can
// legitimately cycle when cruise fights the curve brake; the cues must not
// (playtest 2026-07-22: 23 slowing/released flips in four seconds).
pub const CURVE_ASSIST_CUE_COOLDOWN_S: f64 = 15.0;

/// The three drive phases (`DRIVE_PHASE_*`): plain strings in Python, kept
/// as `&'static str` so `phase == DRIVE_PHASE_PICKUP` reads the same.
pub const DRIVE_PHASE_PICKUP: &str = "pickup";
pub const DRIVE_PHASE_DELIVERY: &str = "delivery";
pub const DRIVE_PHASE_SCHOOL: &str = "school"; // sandbox practice drive, never persisted

// Microsleeps: once fatigue is severe, the driver involuntarily nods off and
// must respond (steer or brake) within a short window or drift off the road.
// They come faster the more exhausted you are, and escalate to a forced stop.
pub const MICROSLEEP_REACTION_S: f64 = 2.2; // real seconds to respond before drifting off
pub const MICROSLEEP_BASE_GM: f64 = 9.0; // game-minutes between nods at the severe threshold
pub const MICROSLEEP_MIN_GM: f64 = 3.0; // ...shrinking to this nearer total exhaustion
pub const MICROSLEEP_COOLDOWN_GM: f64 = 4.0; // quiet period after one resolves
pub const MICROSLEEP_SHOULDER_DAMAGE_PCT: f64 = 6.0;
pub const MICROSLEEP_FORCE_STOP_MISSES: i64 = 3; // consecutive misses that force a stop

// Turn earcons come from the side of the maneuver, the same convention as
// the lane-guidance beeps: hear it on the side you are about to steer toward.
pub const TURN_CUE_PAN: f64 = 0.6;

// Tolerance over the posted limit before a speed is a speed at all -- roughly
// real-world ticketing tolerance, judged against the leg's real OSM maxspeed
// rather than a flat number. Canonical in the sim layer, because the officer
// who reads the speed lives there; this name is kept because half the driving
// layer already asks for it.
pub const SPEEDING_LEEWAY_MPH: f64 = OBSERVE_LEEWAY_MPH;
// The dash overspeed alert speaks up before enforcement does: it arms over the
// limit (under the strike leeway), then chimes on an interval until the truck
// settles back under. Real carrier trucks nag exactly like this, which is why
// nobody in one is surprised by their own speed.
//
// 7 is the only value that sits in the gap, and the gap is narrow:
//   - ACC_LIMIT_OFFSET_MPH (5.0) is the pace predictive cruise itself holds.
//     Arming AT 5 gave the warning zero headroom over the speed the game's own
//     automation picks, so ordinary control-loop wobble -- a downgrade, a
//     traffic adjustment, the grade band -- chimed at a driver for a speed
//     they did not choose. That was patched once by bounding the grade band
//     rather than by moving this number; this is the real fix.
//   - OBSERVE_LEEWAY_MPH (9.0) is where a trooper can act. Arming below it is
//     the whole point: the dash warns while compliance is still free, never
//     after the driver is already ticketable.
// Anything at or above 9 would let a driver become ticketable in silence,
// which inverts what the alert is for.
pub const OVERSPEED_WARN_MPH: f64 = 7.0; // over the limit where the warning arms
                                         // Hysteresis measured from the arm point, NOT from the limit. Measured from the
                                         // limit it was six mph deep: one honest trigger at nine over went on chiming
                                         // through six, five, four and three over while the driver was slowing down, so
                                         // a driver who blipped over once heard the alert at speeds it must never speak
                                         // at (playtest, 2026-08-15, and the tester report behind it). Back under the
                                         // threshold by this much and the episode is over.
pub const OVERSPEED_RESET_MPH: f64 = 1.0;
// The cadence carries the magnitude: slightly over dings politely, a real
// runaway dings twice a second. Interval slides between these ends as the
// overage grows.
pub const OVERSPEED_CHIME_REPEAT_S: f64 = 5.0; // cadence just past the warn threshold
pub const OVERSPEED_CHIME_FAST_S: f64 = 0.5; // cadence at OVERSPEED_URGENT_MPH over and beyond
pub const OVERSPEED_URGENT_MPH: f64 = 20.0;
// Speeding tickets are priced by how far over the limit you were, how many
// citations the career already carries, and whether it happened in a
// construction zone -- see models/enforcement.speeding_citation_fine, which is
// anchored to the real state fine schedules. Paid on the spot when a trooper
// pulls you over, and that is the ONLY way speeding costs anything.
//
// There used to be a second, invisible charge: hold nine over for six seconds
// with no patrol anywhere and the drive banked a "speeding strike", billed at
// the dock hours later as a driver-responsibility charge. It was a placeholder
// for enforcement that did not exist -- a fine from an officer who was never
// there -- and it is gone (owner ruling, 2026-08-09). Speeding nobody saw now
// costs nothing, which is both honest and what happens on a real road. The
// presence model is what stands between a speeder and impunity.
// Travel this far still moving after the lights come on and it counts as
// ignoring the stop -- a heavier fine and a bigger reputation hit.
pub const PULL_OVER_IGNORE_MI: f64 = 2.0;
// The staged warnings run on real seconds, not trip miles: compression could
// burn two miles before the first warning had a chance to speak.
pub const PULL_OVER_FIRST_WARNING_S: f64 = 8.0;
pub const PULL_OVER_FINAL_WARNING_S: f64 = 16.0;
// After the final warning, this long before troopers force the stop.
pub const PULL_OVER_FORCED_STOP_S: f64 = 10.0;
// Running is judged by conduct after the final warning: this many real
// seconds holding speed or accelerating, unbraked, at or above the floor,
// and troopers call it a pursuit. Any brake resets it, so a driver who is
// trying to stop can never reach a felony; the slow roll and the coast-down
// end in the forced stop instead. Doubled when the next one would be a
// lifetime disqualification.
pub const PURSUIT_RUN_S: f64 = 12.0;
pub const PURSUIT_RUN_MIN_MPH: f64 = 30.0;
pub const FAILURE_TO_STOP_DAMAGE_PCT: f64 = 12.0;
pub const FAILURE_TO_STOP_PROCESSING_MIN: f64 = 180.0;
pub const WEIGH_STATION_NOTICE_MI: f64 = 2.0;
// The sim layer owns the number: the scale reminder's real-time hold lets go
// at the same speed a crossing stops counting as a bypass.
pub const WEIGH_STATION_BYPASS_MPH: f64 = SCALE_BYPASS_MPH;
// A bypass is caught, not certain. The scale house has plate readers and
// weigh-in-motion sensors watching the bypass lane, and dispatches a unit up
// the corridor after a truck that ran it -- but a unit still has to catch up,
// so real bypass enforcement is steep, not perfect. Same shape as
// CHAIN_LAW_CHECKPOINT_CHANCE: a flat, named, seeded roll, not a difficulty
// knob -- the enforcement-presence setting governs ambience only and never
// reaches this number (owner ruling, 2026-08-14: "pretty steep"). What a
// caught bypass costs is priced in models/enforcement, with every other fine.
pub const WEIGH_STATION_BYPASS_CATCH_CHANCE: f64 = 0.85;
// A transponder-equipped truck gets a weigh-in-motion verdict instead of the
// blanket "all trucks must pull in" demand (see EnforcementWatchMixin's scale
// handling in driving_updates.py / driving_enforcement.py, and
// models.business.has_weigh_station_transponder for who has one). PrePass
// publicly reports the large majority of transponder-equipped, in-compliance
// trucks bypassing an open scale -- this is a design constant informed by
// that public figure, not a measurement of anything in this game, and it
// only ever applies to a truck that is not overweight (see
// WEIGH_STATION_TRANSPONDER_BYPASS_SHARE's use in
// _resolve_transponder_verdict). The remainder are red-lighted anyway,
// matching how real weigh-in-motion programs still spot-check compliant
// carriers rather than waving every one through every time.
pub const WEIGH_STATION_TRANSPONDER_BYPASS_SHARE: f64 = 0.9;
pub const UNSAFE_DAMAGE_STOP_PCT: f64 = 65.0;
pub const AMBIENT_EVENT_SPACING_S: f64 = 2.5; // keep low-priority chatter from stacking
                                              // Once the lights come on, a compliance tracker (0..1) judges whether you are
                                              // actually pulling over -- signaling and slowing -- rather than how far you
                                              // rolled. It seeds at PULL_OVER_START_COMPLIANCE, rises with braking, falls with
                                              // accelerating/coasting/ignoring, and a felony stop fires the instant it hits 0.
                                              // Disobedient rates outpace the compliant one so it always zeroes faster than it
                                              // fills, and their deductions stack when several apply at once.
pub const PULL_OVER_START_COMPLIANCE: f64 = 0.5;
pub const PULL_OVER_ACCEL_RATE: f64 = 0.34; // per s of rising speed; full 1.0 -> 0.0 in ~3 s
pub const PULL_OVER_ACCEL_EPS_MPH_S: f64 = 0.4; // speed must genuinely rise (past jitter) to count
pub const PULL_OVER_COAST_RATE: f64 = 0.12; // per s of coasting; lighter than accelerating
pub const PULL_OVER_BRAKE_RATE: f64 = 0.15; // per s of braking; the only thing that raises it
pub const PULL_OVER_SIGNAL_GRACE_S: f64 = 5.0; // plenty of time to react before the no-signal drain
pub const PULL_OVER_COAST_GRACE_S: f64 = 3.0; // coasting is only flagged after this many s
pub const PULL_OVER_SIGNAL_BOOST: f64 = 0.20; // one-time bump the first time you signal
pub const PULL_OVER_NOSIGNAL_HIT: f64 = 0.25; // one-time 1/4 hit once past the signal grace unsignaled
pub const PULL_OVER_NOSIGNAL_RATE: f64 = 0.03; // per s small drain while still unsignaled past grace
pub const PULL_OVER_FULL_COMPLIANCE: f64 = 0.95; // at/above this a stop counts as prompt and clean
pub const PULL_OVER_CLEAN_STOP_WARN_CHANCE: f64 = 0.25; // chance a clean stop downgrades a ticket to a warning

/// `_pull_over` stage names: `None` (no stop), lights on, and pulling in.
pub const PULL_OVER_LIGHTS: &str = "lights";
pub const PULL_OVER_STOPPING: &str = "stopping";

/// `driving_updates.OVERREV_GRACE_S`, needed at construction time for
/// `overrev_warn_due`; the updates module re-exports it.
pub const OVERREV_GRACE_S: f64 = 1.5;

pub const FACILITY_ENGINE_SHUT_DOWN_ITEM: &str = "Shut down the engine";
pub const FACILITY_ENGINE_START_ITEM: &str = "Start the engine";

/// `POI_ACTION_LABELS`: what a stop's action keys are called aloud.
pub const POI_ACTION_LABELS: [(&str, &str); 10] = [
    ("park", "parking"),
    ("save", "save point"),
    ("fuel", "fuel"),
    ("food", "food and coffee"),
    ("break", "30-minute rest break"),
    ("sleep", "sleep or long rest"),
    ("repair", "repairs"),
    ("roadside_assistance", "roadside assistance"),
    ("towing", "towing"),
    ("inspect", "inspection check-in"),
];

/// `POI_SERVICE_LABELS`: listed services by key; anything else is the key
/// with underscores turned to spaces.
pub const POI_SERVICE_LABELS: [(&str, &str); 9] = [
    ("diesel", "diesel"),
    ("food", "food"),
    ("parking", "truck parking"),
    ("truck_parking", "truck parking"),
    ("restrooms", "restrooms"),
    // Only truck stops list it: the certified scale a driver pays to use,
    // never the weigh station ("the scale" in short cues).
    ("scale", "CAT Scale"),
    ("repair", "repair"),
    ("roadside_assistance", "roadside assistance"),
    ("towing", "towing"),
];
