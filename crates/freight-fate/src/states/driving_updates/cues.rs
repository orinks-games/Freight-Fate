//! What the lane sounds like: the pan, the off-pavement transitions, the
//! edge-boundary ladder, the curve run's verdict, the dead-man's-curve
//! strips, the locator and steering tocks, and the guidance director.

use crate::states::driving_turns::{TURN_COMMIT_TAIL_MI, TURN_GUIDE_LEAD_MI};
use ff_core::data::curves::{min_radius_ft, RouteCurve};
use ff_core::lane_guide_tone::LANE_GUIDE_TONE_KEY;
use ff_core::sim::lane::{CROSS_AT, OFF_ROAD};
use ff_core::sim::lane_guidance::{
    classify_boundaries, cue_loudness, edge_rung, GuidanceFrame, CURVE_LEAD_MI, TRANSVERSE_KEY,
};
use ff_core::sim::trip_models::{highway_class, RAMP_CURVE_DEFLECTION_RAD};
use ff_core::sim::turn_guide::{
    TurnInput, TurnShape, TurnSide, SLEW_PER_S as TURN_GUIDE_SLEW_PER_S,
};
use ff_core::speech_pacing::SpeechCategory;

use crate::app::{GameContext, SayEvent};
use crate::audio::{CH_EDGE, CH_LANE_GUIDE, CH_ROAD};
use crate::states::driving::DrivingState;
use crate::states::driving_core::*;
use crate::states::driving_updates::LANE_GUIDE_TONE_VOLUME;

/// Marks a street corner's identity apart from a mapped bend's in
/// `TurnInput::turn_id`. A bend's id is the bits of its start milepost, which
/// is never negative, so an f64's sign bit is the one bit no bend can set.
const CORNER_ID_BIT: u64 = 1 << 63;
/// The exit ramp's curve: a corner-side id no street corner's leg index
/// reaches.
const RAMP_CURVE_TURN_ID: u64 = CORNER_ID_BIT | (1 << 62);

impl DrivingState {
    /// Stereo pan for the rumble strip: it comes from the side you have
    /// drifted toward (negative left, positive right), so the side you hear it
    /// on is the side to steer away from.
    pub fn lane_pan(&self) -> f64 {
        self.lane.offset.clamp(-1.0, 1.0)
    }

    pub fn off_pavement(&self) -> bool {
        self.lane.edge_excursion() >= OFF_ROAD
    }

    /// A severity band that rises as the truck goes deeper off and faster,
    /// so the transition speech fires again when the condition worsens
    /// (research doc R12). Zero to two.
    pub fn off_pavement_band(&self) -> i32 {
        let depth = if self.lane.edge_excursion() < 1.4 {
            0
        } else {
            1
        };
        let fast = if self.trip.truck.speed_mph() >= 45.0 {
            1
        } else {
            0
        };
        depth + fast
    }

    /// Speak the off-pavement condition at its transitions only: on entry,
    /// and again when it worsens. A steady or easing band stays silent -- the
    /// panned edge-rumble loop carries where the truck is (research doc R12).
    pub fn announce_off_pavement(&mut self, ctx: &mut GameContext) {
        let band = self.off_pavement_band();
        if let Some(previous) = self.road_position_band {
            if band <= previous {
                // Still off, no worse: track the band so a later worsening speaks,
                // but say nothing now.
                self.road_position_band = Some(band);
                return;
            }
        }
        self.road_position_band = Some(band);
        let boundary = self.edge_boundary();
        let message = if boundary == "oncoming" {
            // Past an undivided centerline is not a shoulder: say the
            // real danger, on the side it lives.
            "Across the centerline, in the oncoming lane!".to_string()
        } else if boundary == "median" {
            "Off the pavement, into the median on the left!".to_string()
        } else {
            self.lane.describe()
        };
        // Position (the standing off-pavement condition) is STATUS; this
        // function only ever speaks on entry or worsening -- the transition
        // itself is the warning, so every line it emits is SAFETY.
        ctx.say_event_with(message, SayEvent::new().category(SpeechCategory::Safety));
    }

    /// What lies past the road edge the truck is drifting toward.
    ///
    /// The divided flag prefers the baked lane segment at the current mile,
    /// then the leg's carriageway-geometry flag (Track D2), then the
    /// classifier's honest inference (interstates are divided by
    /// definition; one lane per side means a centerline).
    pub fn edge_boundary(&self) -> &'static str {
        // An exit ramp is one way: both of its edges are road edges. Read
        // off the mainline's divided flag, a truck running wide on the ramp
        // off an undivided road was told it was "in the oncoming lane"
        // (every-assist audit, 2026-09-24).
        if self.on_laid_out_ramp() {
            return "shoulder";
        }
        let baked = self.trip.lanes_at(None);
        let leg = &self.trip.route.legs[self.trip.current_leg_index()];
        let divided = match baked {
            Some((_, divided)) => Some(divided),
            None => leg.divided,
        };
        let (left, right) = classify_boundaries(
            self.lane.lane,
            self.lane.lane_count,
            divided,
            highway_class(&leg.highway) == "interstate",
        );
        if self.lane.offset < 0.0 {
            left
        } else {
            right
        }
    }

    /// Close the loop the pacenote opens: a soft tick on the bend's side
    /// as the curve begins, and a verdict once you are through -- the
    /// centred earcon for a clean bend, words for caught the edge or hot. The windshield
    /// gives a sighted driver this for free; the co-driver owes it to ours
    /// (owner ask 2026-07-27: "nothing tells you that you made it through
    /// well"). Chained bends hold their verdict for the last link.
    pub fn update_curve_run(&mut self, ctx: &mut GameContext, active: Option<&RouteCurve>) {
        let active = active.filter(|curve| !curve.connector);
        if let Some(curve) = active {
            let fresh = self
                .curve_run
                .as_ref()
                .map(|run| run.curve != *curve)
                .unwrap_or(true);
            if fresh {
                let (limit, _) = self.trip.speed_limit_at(self.trip.position_mi);
                let mut demanding =
                    (curve.advisory_mph as f64) < limit && curve.severity() != "gentle";
                let mut touched = false;
                let mut hot = false;
                if let Some(run) = self.curve_run.as_ref() {
                    // A chained link: carry what the earlier bends earned.
                    demanding = demanding || run.demanding;
                    touched = run.touched;
                    hot = run.hot;
                }
                self.curve_run = Some(CurveRun {
                    curve: *curve,
                    demanding,
                    touched,
                    hot,
                });
                // No chime at the bend's start: the engine is already leaning
                // into it, and it leads the bend rather than marking a moment
                // that has already passed (owner, 2026-09-18).
            }
            let rumbling = self.lane.rumble_level() > 0.0;
            let speed = self.trip.truck.speed_mph();
            if let Some(run) = self.curve_run.as_mut() {
                if rumbling {
                    run.touched = true;
                }
                if speed > run.curve.advisory_mph as f64 + 15.0 {
                    run.hot = true;
                }
            }
            return;
        }
        if self.curve_run.is_none() {
            return;
        }
        if self.trip.curve_ahead_mi(0.2).is_some() {
            return; // linked "then right": the verdict waits for the last bend
        }
        let run = self.curve_run.take().expect("checked above");
        if !run.demanding || !ctx.settings.curve_callouts {
            return;
        }
        // A clean bend is the earcon alone; words only for what went wrong.
        if self.terse_speech(ctx) || !(run.touched || run.hot) {
            ctx.audio.play_with("vehicle/lane_centered", 0.5, 0.0);
            return;
        }
        let text = if run.touched {
            "Caught the edge."
        } else {
            "Too hot."
        };
        ctx.say_event_with(
            text,
            SayEvent::queued().category(SpeechCategory::Confirmation),
        );
    }

    /// Lanes on our side at this mile.
    ///
    /// One answer, kept on the trip, so the lane the truck steers in and the
    /// lane a work zone may cone off can never disagree -- two readings of
    /// the road is how a closure landed on a one-lane stretch.
    pub fn lane_count_here(&self) -> i64 {
        self.trip.lane_count_at(None)
    }

    pub fn cue_loudness(&self, ctx: &GameContext) -> f64 {
        cue_loudness(&ctx.settings.lane_cue_loudness).unwrap_or(1.0)
    }

    /// Fixed dead-man's-curve bars ahead of hairpins: cross them, hear
    /// them -- at any speed, in any assist mode, because they are cut into
    /// the road. Louder when faster, like the real hits.
    pub fn update_transverse_strips(&mut self, ctx: &mut GameContext) {
        if self.trip.truck.speed_mph() < 2.0 {
            return;
        }
        let position = self.trip.position_mi;
        let strips = self.transverse_strip_miles.clone();
        for strip_mi in strips {
            if self.transverse_fired.contains(&strip_mi) || position < strip_mi {
                continue;
            }
            if position - strip_mi > 0.5 {
                self.transverse_fired.push(strip_mi); // resumed past it; stay quiet
                continue;
            }
            self.transverse_fired.push(strip_mi);
            let volume =
                1.0f64.min((0.65 + self.trip.truck.speed_mph() / 150.0) * self.cue_loudness(ctx));
            ctx.audio.play_with(TRANSVERSE_KEY, volume, 0.0);
            ctx.controller.rumble.impact(0.5);
        }
    }

    /// The I-key locator: a soft tock every beat, panned to where the
    /// truck sits in its lane. Player-summoned, so it keeps ticking until
    /// they shut it off or lane keeping takes the lane over.
    pub fn update_lane_locator_audio(&mut self, ctx: &mut GameContext, dt: f64) {
        if !self.lane_locator_on {
            return;
        }
        if ctx.settings.lane_is_automated() || self.trip.truck.speed_mph() < 2.0 {
            return;
        }
        self.lane_locator_timer -= dt;
        if self.lane_locator_timer > 0.0 {
            return;
        }
        self.lane_locator_timer = 0.9;
        let pan = self.lane.offset.clamp(-1.0, 1.0);
        let volume = 1.0f64.min(0.5 * self.cue_loudness(ctx));
        ctx.audio.play_with("vehicle/lane_locator", volume, pan);
    }

    /// How far across into the exit lane the truck is, 0 to 1: nothing
    /// until the lane opens at its taper, then the way to its line.
    pub fn exit_alignment_progress(&self) -> f64 {
        if self.exit_stop.is_none() || !self.exit_signal_on {
            return 0.0;
        }
        if self.exit_lane_ready() {
            return 1.0;
        }
        if !self.lane.exit_lane_open {
            return 0.0;
        }
        (self.lane.offset / CROSS_AT).clamp(0.0, 1.0)
    }

    /// Is a lane move underway that the driver should hear their position for?
    ///
    /// An armed exit owns the blinker until the ramp or cancellation. Otherwise
    /// a sustained steering hold summons the ordinary lane-position cue.
    pub fn steering_lane_cue_armed(&mut self, ctx: &GameContext, dt: f64) -> bool {
        if self.lane.steering != 0.0 {
            self.steer_cue_hold_s += dt;
        } else {
            self.steer_cue_hold_s = 0.0;
        }
        if self.exit_blinker_on() {
            return true;
        }
        if self.ramp_mi.is_some() {
            return false;
        }
        if self.lane_locator_on {
            return false; // the driver already has this tock running; one is enough
        }
        if ctx.settings.lane_is_automated() {
            return false; // the truck holds the lane and takes the exit itself
        }
        if self.lane.is_following() {
            // A hold taking a turn keeps the lane, so there is no move across
            // it to hear -- and the relay at every corner read as a blinker
            // nobody switched on (owner, 2026-09-30).
            return false;
        }
        if self.trip.truck.speed_mph() < STEER_CUE_MIN_MPH {
            return false;
        }
        self.steer_cue_hold_s >= STEER_CUE_ARM_S
    }

    /// Hear where you are in the lane while you steer across it.
    ///
    /// The lane locator answers "where am I" on demand. This answers it for
    /// the length of a move being made right now, with no key to remember:
    /// a panned turn-signal relay, keeping time from the moment the wheel goes
    /// over until the move is done.
    ///
    /// An exit signal has a steady beat on the right, independent of steering
    /// and lane readiness. The audio hold expires while a menu owns the frame.
    pub fn update_steering_lane_cue(&mut self, ctx: &mut GameContext, dt: f64) {
        if self.exit_blinker_on() {
            self.exit_blinker_active = true;
        } else if self.exit_blinker_active {
            // Finishing or abandoning an exit must not turn the still-held
            // Right key straight into another blinker for ordinary steering.
            ctx.audio.release_cue("vehicle/turn_signal");
            ctx.audio.release_cue(STEER_CUE_HOLD);
            self.steer_cue_active = false;
            self.steer_cue_timer = 0.0;
            self.steer_cue_hold_s = 0.0;
            self.exit_blinker_active = self.lane.steering != 0.0;
            return;
        }
        if !self.steering_lane_cue_armed(ctx, dt) {
            if self.lane_change_target.is_none() {
                ctx.audio.release_cue("vehicle/turn_signal");
            }
            if !self.steer_cue_active {
                return;
            }
            self.steer_cue_active = false;
            self.steer_cue_timer = 0.0;
            // Click off only if we still held the frame a moment ago. A menu
            // over the drive lets the latch lapse on the audio clock, and the
            // move ends in silence rather than a signal cancelling at the
            // pause screen -- the dead man's switch, same as a held alert.
            if ctx.audio.cue_held(STEER_CUE_HOLD) {
                ctx.audio.release_cue(STEER_CUE_HOLD);
                let volume = 1.0f64.min(STEER_CUE_CANCEL_VOL * self.cue_loudness(ctx));
                // centred and quieter: the stalk clicking back, not the signal on
                ctx.audio.play_with("vehicle/turn_signal_off", volume, 0.0);
            }
            return;
        }
        let resumed = !ctx.audio.cue_held("vehicle/turn_signal");
        ctx.audio.hold_cue(STEER_CUE_HOLD);
        let volume = 1.0f64.min(0.5 * self.cue_loudness(ctx));
        let pan = if self.exit_blinker_on() {
            0.6
        } else {
            self.lane.offset.clamp(-1.0, 1.0)
        };
        ctx.audio.update_cue("vehicle/turn_signal", volume, pan);
        if !self.steer_cue_active || resumed {
            self.steer_cue_active = true;
            self.steer_cue_timer = 0.0; // first tock lands on the frame the move starts
        }
        self.steer_cue_timer -= dt;
        if self.steer_cue_timer > 0.0 {
            return;
        }
        let span = STEER_CUE_TOCK_S - STEER_CUE_TOCK_FAST_S;
        self.steer_cue_timer = if self.exit_blinker_on() {
            STEER_CUE_TOCK_S
        } else {
            STEER_CUE_TOCK_S - span * self.exit_alignment_progress()
        };
        ctx.audio.play_if_idle("vehicle/turn_signal", volume, pan);
    }

    /// Whether the turn signal is clicking for an exit.
    ///
    /// Signalled with X, or taken by lane keeping on full, and only from
    /// `EXIT_BLINKER_MI` out. X commits the truck wherever it is pressed, but
    /// a real driver flicks the signal on a quarter to half a mile out; eight
    /// miles of blinker is what gets a trucker flashed (owner ruling,
    /// 2026-09-24, after agent drives that blinked 7.3 miles to the gore).
    pub fn exit_blinker_on(&self) -> bool {
        let Some(stop) = self.exit_stop.as_ref() else {
            return false;
        };
        self.ramp_mi.is_none()
            && !self.exit_signal_canceled
            && (self.exit_signal_on || self.exit_lane_entered)
            && stop.at_mi - self.trip.position_mi <= EXIT_BLINKER_MI
    }

    /// Run the edge-boundary ladder: structural loops, not louder beeps.
    ///
    /// Clipping the strip is intermittent, fully on it is periodic, off the
    /// pavement is aperiodic gravel -- states a driver can tell apart under
    /// engine noise. Panned to the drift side. Past an undivided centerline
    /// the strip stays the outermost texture (there is no gravel out there;
    /// the spoken warning carries the oncoming danger).
    pub fn update_edge_ladder_audio(&mut self, ctx: &mut GameContext) {
        let rung = if ctx.settings.lane_is_automated() || self.trip.truck.speed_mph() < 2.0 {
            None // tires that are not rolling make no groove noise
        } else {
            edge_rung(
                self.lane.edge_excursion(),
                self.edge_boundary(),
                self.cue_loudness(ctx),
            )
        };
        let Some((key, volume)) = rung else {
            if self.edge_loop_key.is_some() {
                ctx.audio.stop_loop_with(CH_EDGE, 150);
                self.edge_loop_key = None;
            }
            return;
        };
        ctx.audio.start_loop_with(CH_EDGE, key, volume, 120);
        ctx.audio.set_loop_volume(CH_EDGE, volume);
        let pan = self.lane_pan();
        ctx.audio.set_loop_pan(CH_EDGE, pan);
        self.edge_loop_key = Some(key.to_string());
    }

    /// Whether a mapped bend asks the driver for enough wheel to lean the
    /// engine for.
    ///
    /// The lean is a warning device -- it says "there is steering to do here"
    /// -- and the road already states which of its bends need one. A bend
    /// posted at or above the speed of the road it sits on is taken without
    /// slowing and without a conscious steering input, which is why no state
    /// DOT signs one: the MUTCD warrants the Horizontal Alignment sign and its
    /// advisory plaque (sections 2C.06 and 2C.08) where the advisory speed is
    /// BELOW the approach speed, and nowhere else. It is the same test
    /// `update_curve_run` already uses to decide a bend is worth a spoken
    /// verdict, so the engine and the co-driver now agree on what a bend is.
    ///
    /// The guide had no such test, and the lean's DEPTH is flat: every turn
    /// opens `TURN_LEAN` whatever it asks for, because only the timing is read
    /// off the road. Driven on AZ-260 Camp Verde to Payson (agent drive,
    /// 2026-09-19) that meant fifty-eight miles of sixty-five to eighty mph
    /// sweepers, none of them wanting any wheel at thirty-seven, each opening
    /// the lean to its full depth in turn -- and with a bend every fifth of a
    /// mile the engine swung hard left, hard right, hard left for the whole
    /// drive while the truck sat dead centre in its lane. An instrument at
    /// full deflection telling a driver to steer when the road wants nothing
    /// is worse than a silent one: it is the same reading the bend that
    /// destroyed a load gives.
    pub fn bend_asks_for_the_wheel(&mut self, bend: &RouteCurve) -> bool {
        let (limit, _) = self.trip.speed_limit_at(self.trip.position_mi);
        (bend.advisory_mph as f64) < limit
    }

    /// Signed steer the active bend asks for, -1 full left .. 1 full right.
    ///
    /// Direction leads into the curve (a left bend wants left); magnitude
    /// follows the same tightness/overspeed shape the curve push uses, so
    /// the guide leans harder exactly when the bend pulls harder.
    ///
    /// A bend the road does not warn about is not one of them. The gate has to
    /// be here as well as in [`Self::turn_guide_input`] or it buys nothing:
    /// this demand is the FALLBACK the engine takes when no turn is in play,
    /// and it leans for the whole length of a bend rather than closing as the
    /// bend is used up, so leaving the sweepers to it would have made the
    /// swing worse rather than quieter.
    pub fn curve_steer_demand(&mut self) -> f64 {
        let active = self.trip.curve_at(self.trip.position_mi);
        // A gated sweeper is not handed on to the maneuver demand as though it
        // were a connector: it is simply not a bend, so the road under the
        // truck reads as whatever it would with no curve record at all.
        let mainline = active.as_ref().filter(|curve| !curve.connector).copied();
        let asks = mainline.is_some_and(|curve| self.bend_asks_for_the_wheel(&curve));
        let Some(bend) = mainline.filter(|_| asks) else {
            // Ramp connectors and street maneuvers carry no mainline curve
            // record, and returning 0.0 here left the panned road bed dead
            // centre through every exit and every turn. The maneuver demand
            // keeps the guide leaning (see driving_turns.py).
            return self.maneuver_steer_demand(active.as_ref().filter(|c| c.connector));
        };
        let tightness = 0.2f64.max(1.0 - bend.min_radius_ft as f64 / 5000.0);
        let excess = 0.0f64.max(self.trip.truck.speed_mph() - bend.advisory_mph as f64);
        let magnitude = 1.0f64.min(tightness * (1.0 + excess * 0.04));
        if bend.direction == 'L' {
            -magnitude
        } else {
            magnitude
        }
    }

    /// The turn the engine should be leaning for, and how far off it is.
    ///
    /// A mapped bend and a street corner are the same thing to the guide --
    /// a direction, an angle and a radius -- so both are read into one
    /// `TurnShape`. The bend's come from the curve bake; the corner's angle is
    /// the one `build_local_geometry` measures at the junction and its radius
    /// from `data::corners`, which is the same geometry that sets its advise
    /// speed.
    ///
    /// Which of them the engine leans for is decided by road, not by which
    /// has the smaller number. The rule used to be the smallest SIGNED
    /// distance to the start, and a turn already begun has a negative one that
    /// keeps falling -- so a corner coasting out its commit tail, its lean
    /// long closed, outranked a bend fifty yards ahead and the bend got no
    /// lead at all (review S5, 2026-09-19). Each turn now CLAIMS the engine by
    /// how much lean the road itself gives it: a turn under the wheels claims
    /// what is left of it, a turn ahead claims how far into its lead the truck
    /// is. One falls as the other rises, so the engine changes hands exactly
    /// once, where the two leans would be equally loud, and never back.
    ///
    /// `hear_drift` is whether the DRIFT half of the lean may speak: false
    /// under full lane keeping, and false with the lane-departure warning off
    /// (owner ruling, 2026-09-19: "turns yes, drift no"). The turn half is
    /// never gated.
    pub fn turn_guide_input(&mut self, hear_drift: bool) -> TurnInput {
        let position = self.trip.position_mi;
        let speed = self.trip.truck.speed_mph();
        let steering = self.lane.steering;
        // Automation holds the lane, so there is no drift for the lean to
        // report -- only the turn itself. The lane model already pins the
        // offset to centre there; reading it as zero here as well means a
        // stale value can never be heard as a drift the truck is not making.
        //
        // And a driver who switched the lane-departure warning off asked not
        // to be told about drift. The old road lean went quiet for them; when
        // the guide moved to the engine the drift half came along ungated and
        // kept correcting a driver who had declined it (review I10).
        let lane_offset = if hear_drift { self.lane.offset } else { 0.0 };
        let lane_heading_rad = if hear_drift { self.lane.yaw_rad } else { 0.0 };
        // `(claim, distance to its start, identity, shape, progress)`.
        let mut best: Option<(f64, f64, u64, TurnShape, f64)> = None;
        let mut consider = |to_start_mi: f64, turn_id: u64, shape: TurnShape, progress: f64| {
            let claim = if to_start_mi <= 0.0 {
                1.0 - progress
            } else {
                (1.0 - to_start_mi / TURN_GUIDE_LEAD_MI).max(0.0)
            };
            let wins = best.is_none_or(|(held, seen, ..)| {
                claim > held || (claim == held && to_start_mi < seen)
            });
            if wins {
                best = Some((claim, to_start_mi, turn_id, shape, progress));
            }
        };
        let bend_shape = |curve: &RouteCurve| {
            TurnSide::parse(&curve.direction.to_string()).map(|side| TurnShape {
                side,
                deflection_deg: curve.deflection_deg,
                radius_ft: curve.min_radius_ft as f64,
            })
        };
        // A bend is the same bend for as long as its start milepost is, and
        // no two bends of one route share one. Mileposts are never negative,
        // so the top bit is free to mark the street corners below.
        let bend_id = |curve: &RouteCurve| curve.start_mi.to_bits() & !CORNER_ID_BIT;

        // The bend under the wheels AND the next one inside the lead. They
        // were an either/or, so the second half of an S-bend was never looked
        // at until the first had ended and opened with no lead.
        //
        // Both go through `bend_asks_for_the_wheel` first: a sweeper the road
        // does not warn about is not a turn the engine has anything to say
        // about, and a corridor of them left the lean with no rest at all
        // (agent drive, AZ-260, 2026-09-19).
        let mainline = self.trip.curve_at(position).filter(|c| !c.connector);
        let under_the_wheels = mainline.filter(|c| self.bend_asks_for_the_wheel(c));
        if let Some(curve) = under_the_wheels {
            if let Some(shape) = bend_shape(&curve) {
                let lo = curve.start_mi.min(curve.end_mi);
                let hi = curve.start_mi.max(curve.end_mi);
                let span = (hi - lo).max(1e-6);
                consider(
                    curve.start_mi - position,
                    bend_id(&curve),
                    shape,
                    ((position - lo) / span).clamp(0.0, 1.0),
                );
            }
        }
        let next_bend = self.trip.next_curve_within(TURN_GUIDE_LEAD_MI);
        let ahead_of_us = next_bend.filter(|(_, curve)| self.bend_asks_for_the_wheel(curve));
        if let Some((ahead, curve)) = ahead_of_us {
            if let Some(shape) = bend_shape(&curve) {
                consider(ahead, bend_id(&curve), shape, 0.0);
            }
        }

        // And the exit ramp's curve, which is a turn like any bend: its lean
        // leads in from the deceleration lane and closes as the curve is used
        // up. It rode only the lane guide's fallback, which the
        // lane-departure warning switches off, so with the warning off a
        // truck running wide on a ramp curve heard a centred engine (agent
        // drive, 2026-09-24); a turn's lean is never gated ("turns yes, drift
        // no", 2026-09-19). It claims the engine over a street turn waiting
        // past the ramp's end by the same claim a bend makes.
        if let (Some(layout), Some(travelled)) = (self.ramp_layout, self.ramp_travelled_mi()) {
            let into_mi = travelled - layout.decel_mi;
            let in_play = -into_mi <= TURN_GUIDE_LEAD_MI && into_mi < layout.curve_mi;
            if !self.surface_chain && in_play && layout.curve_mi > 0.0 {
                consider(
                    -into_mi,
                    RAMP_CURVE_TURN_ID,
                    TurnShape {
                        side: TurnSide::Right,
                        deflection_deg: RAMP_CURVE_DEFLECTION_RAD.to_degrees(),
                        radius_ft: min_radius_ft(layout.curve_mph).max(1.0),
                    },
                    (into_mi / layout.curve_mi).clamp(0.0, 1.0),
                );
            }
        }

        // And the street corner the route is asking for -- once it is inside
        // the lead. `turn_cue_in_play` has no upper bound, so a corner two
        // miles off used to count as a turn in play for the whole approach;
        // harmless while the engine only listened when the lean was non-zero,
        // but "a turn is in play" now decides who owns the engine, and a
        // corner that far away must not take it from the exit ramp's lean.
        let corner = self
            .turn_cue_in_play()
            .filter(|cue| cue.at_mi - position <= TURN_GUIDE_LEAD_MI);
        if let Some(cue) = corner {
            if let Some(side) = TurnSide::parse(&cue.direction) {
                let index = self.turn_leg_index(&cue);
                // A street corner has no footprint of its own, so it is
                // taken as used up across the commit tail past its milepost --
                // the same stretch `turn_cues_in_play` keeps it alive for.
                let through = ((position - cue.at_mi) / TURN_COMMIT_TAIL_MI).clamp(0.0, 1.0);
                consider(
                    cue.at_mi - position,
                    // A corner is its leg of the street chain, which is what
                    // its cue key already ends in.
                    CORNER_ID_BIT | index as u64,
                    self.corner_shape(&cue, side),
                    through,
                );
            }
        }

        match best {
            Some((_, to_start_mi, turn_id, shape, progress)) => TurnInput {
                shape: Some(shape),
                turn_id,
                to_start_mi,
                past: false,
                steering,
                speed_mph: speed,
                lane_offset,
                lane_heading_rad,
                progress,
            },
            None => TurnInput {
                shape: None,
                turn_id: 0,
                to_start_mi: f64::INFINITY,
                past: true,
                steering,
                speed_mph: speed,
                lane_offset,
                lane_heading_rad,
                progress: 1.0,
            },
        }
    }

    /// Run the guidance director: the ENGINE leans toward where the wheel
    /// should go (pursuit guide -- follow the sound), wakes for drift or a
    /// bend, and slews home on the centered straight, while the road bed
    /// sits where the truck actually is in its lane.
    ///
    /// The guide rode the road bed until 2026-09-18 and the owner's ruling
    /// moved it: the bed is a quiet, textureless surface and a lean across it
    /// is hard to place, where the engine is the loudest continuous thing in
    /// the cab and the one whose pitch a driver is already tracking. It
    /// carries a pan far better, which is the whole job. The two swapped
    /// rather than the position readout being dropped -- knowing where you
    /// sit in the lane is still worth a channel, it is just worth the quieter
    /// one.
    ///
    /// Still never a new tone: the community ruling holds, and panning an
    /// engine that was already running is not a tone the soundscape did not
    /// have.
    pub fn update_lane_guidance_audio(&mut self, ctx: &mut GameContext, dt: f64) {
        let warned = ctx.settings.lane_departure_warning;
        // The lean asks for the wheel only where the wheel is the driver's
        // (owner, 2026-09-30). With curve assistance or partial lane keeping
        // steering the road's turns, a lean into them asked for the same
        // steering a second time, and a driver who followed it changed lanes
        // into the bend (owner's drive on US-83). There it carries the
        // driver's own drift and nothing else.
        let road_steers = ctx.settings.lane_is_manual() && ctx.settings.road_steers_the_bend();
        let curve_steer = if warned && !road_steers {
            self.curve_steer_demand()
        } else {
            0.0
        };
        let frame = if !warned {
            self.lane_guidance
                .update(&self.lane, 0.0, dt, false, 0.0, None)
        } else {
            let assist_on =
                ctx.settings.lane_is_manual() && self.trip.truck.speed_mph() >= LANE_MIN_MPH;
            let curve_ahead_mi = self.trip.curve_ahead_mi(CURVE_LEAD_MI);
            self.lane_guidance.update(
                &self.lane,
                self.trip.truck.speed_mph(),
                dt,
                assist_on,
                curve_steer,
                curve_ahead_mi,
            )
        };
        // The turn's own lean comes first: it is the one the owner asked for,
        // and it says how much wheel is still owed rather than how far off
        // centre the truck has wandered. Its drift half speaks only to a
        // driver who is holding the lane themselves AND left the
        // lane-departure warning on ("turns yes, drift no", 2026-09-19).
        let hear_drift = warned && ctx.settings.lane_is_manual();
        let mut turn_input = self.turn_guide_input(hear_drift);
        if road_steers {
            turn_input.shape = None;
            turn_input.past = true;
        }
        let turn_pan = self.turn_guide.update(turn_input, dt);
        // On full lane keeping the engine still pans for the road's shape
        // (owner, 2026-09-18): its keys change lanes and never steer, so the
        // lean cannot be followed into a bend, and it closes as the turn is
        // used up rather than as a wheel answers it. Only the opt-in tone,
        // which leans instead of the engine, silences it.
        //
        // WHO owns the engine is decided by whether a turn is in play, never
        // by whether its lean happens to read zero. The selector used to be
        // "the turn's pan unless it is 0.0, else the lane guide's" -- and 0.0
        // is exactly what a driver earns by finishing the turn, while the
        // lane guide's pan is `curve_steer - offset`, non-zero for the whole
        // bend. So steering a bend correctly snapped the engine from centred
        // back into the bend: "keep steering", said to the one driver who had
        // just finished (review I3, 2026-09-19). A turn in play now keeps the
        // engine through its honest 0.0. The lane guide is left the two
        // maneuvers the turn guide has no shape for -- the exit ramp's peel
        // and a connector's arc, both of which reach it as `curve_steer` --
        // and everything else is the turn guide's own drift-only lean.
        let lean = if ctx.settings.lane_guide_tone {
            0.0
        } else if turn_input.shape.is_some() {
            turn_pan
        } else if curve_steer != 0.0 {
            frame.pan
        } else {
            turn_pan
        };
        let wanted = guide_sign(lean, ctx.settings.steering_guide_inverted);
        // Eased across the hand-over, so the engine never jumps when the
        // ramp's lean gives way to the turn guide's or the driver flips the
        // steering guide mid-bend. Each producer already slews at or under
        // this rate, so inside one producer this changes nothing. The first
        // frame of a drive has nothing to ease from and is written as it is:
        // the backend keeps the engine's pan across stops and drives, so a
        // tracker that ASSUMED centre left the next drive's engine leaning
        // down a straight road until something moved it (review I7).
        let guide_pan = match self.engine_guide_pan_applied {
            Some(applied) => {
                let step = TURN_GUIDE_SLEW_PER_S * dt.max(0.0);
                if (wanted - applied).abs() <= step {
                    wanted
                } else if wanted > applied {
                    applied + step
                } else {
                    applied - step
                }
            }
            None => wanted,
        };
        if self.engine_guide_pan_applied != Some(guide_pan) {
            ctx.audio.set_engine_pan(guide_pan);
            self.engine_guide_pan_applied = Some(guide_pan);
        }
        if ctx.settings.lane_guide_tone {
            self.lean_the_tone(ctx, frame);
        }
        // And the position readout, on the bed the guide used to ride.
        let seat_pan = if ctx.settings.lane_is_automated() {
            0.0
        } else {
            self.lane.offset.clamp(-1.0, 1.0)
        };
        if self.road_pan_applied != Some(seat_pan) {
            ctx.audio.set_loop_pan(CH_ROAD, seat_pan);
            self.road_pan_applied = Some(seat_pan);
        }
        if frame.centered {
            // The drift settled: the old centered earcon still says so.
            ctx.audio.play_with("vehicle/lane_centered", 0.45, 0.0);
        }
    }

    /// The opt-in alternative: lean a tone of the guide's own.
    ///
    /// Same frame, same pan, same wake and sleep -- only the thing being
    /// panned differs. The bed is left flat while the tone is in use, or a
    /// driver who switched would get both leaning at once and the bed would
    /// stay wherever the last drift left it.
    ///
    /// Silence is still centered: the loop starts on the wake and stops on
    /// the settle, so a straight road is as quiet as it has always been.
    /// That is what keeps this inside the community ruling rather than
    /// merely beside it -- the objection was to a CONTINUOUS tone, and this
    /// one only exists while the truck is actually off center.
    pub fn lean_the_tone(&mut self, ctx: &mut GameContext, frame: GuidanceFrame) {
        // The bed is NOT flattened here any more. It used to carry the guide,
        // so leaving it leaning while the tone led as well would have given a
        // driver two guides at once; now it carries lane position, which the
        // tone does not replace and which a driver who switched guides still
        // wants.
        if frame.awake {
            if !self.lane_guide_tone_on {
                let volume = LANE_GUIDE_TONE_VOLUME * self.cue_loudness(ctx);
                ctx.audio
                    .start_loop_with(CH_LANE_GUIDE, LANE_GUIDE_TONE_KEY, volume, 120);
                self.lane_guide_tone_on = true;
            }
            // The Steering guide row reverses the tone exactly as it reverses
            // the engine. It used to reach only the turn guide, so with the
            // tone on the setting did nothing at all (review I3).
            let pan = guide_sign(frame.pan, ctx.settings.steering_guide_inverted);
            if pan != self.lane_guide_pan_applied {
                ctx.audio.set_loop_pan(CH_LANE_GUIDE, pan);
                self.lane_guide_pan_applied = pan;
            }
        } else if self.lane_guide_tone_on {
            ctx.audio.stop_loop_with(CH_LANE_GUIDE, 180);
            self.lane_guide_tone_on = false;
            self.lane_guide_pan_applied = 0.0;
        }
    }
}

/// A guide pan as THIS driver follows it: toward the sound by default, away
/// from it with the Steering guide row reversed.
///
/// The one place the reversal happens, applied to whatever the engine or the
/// tone is about to carry, so every producer -- the turn guide, the ramp's
/// lean, the drift correction -- lands in the same convention. Centre stays a
/// plain 0.0 rather than a negative zero, which compares equal but reads as
/// "-0" in a session log.
fn guide_sign(pan: f64, inverted: bool) -> f64 {
    if inverted && pan != 0.0 {
        -pan
    } else {
        pan
    }
}
