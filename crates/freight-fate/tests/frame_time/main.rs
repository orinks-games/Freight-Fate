//! How long a frame takes while the game is being DRIVEN, and a gate that
//! fails when that regresses.
//!
//! `bench_drive.rs` next door times `DrivingState::update` alone on a short
//! plains route. This file times the whole per-frame path the shipped loop
//! runs -- `App::tick` (controller repeats, the speech poll, cloud notices,
//! the audio fades, the speech duck, the state update, presence, the
//! achievement notice) plus `App::visible_lines`, which is everything
//! `App::frame` does once the SDL window is out of the picture -- and it
//! does it over a long, varied mountain route rather than empty interstate.
//!
//! # The route
//!
//! Denver -> Silverthorne -> Edwards -> Glenwood Springs -> Grand Junction:
//! 246 miles of I-70 in four legs. It was chosen because it is the least
//! uniform drive in the world data rather than the longest -- the Front
//! Range climb, the Eisenhower bore, Vail Pass, Glenwood Canyon and the
//! run down to the Grand Valley. All four legs carry full corridor detail
//! (grade segments, elevation samples, lane segments, interchanges,
//! checkpoints, restrictions, speed limits, AADT), so the frames being
//! timed are frames doing the game's real work: grade and curve lookups,
//! zone transitions, traffic in a rolling bubble, hazards, the speech
//! ladder and the event pacer.
//!
//! The route is longer than the run reaches. [`DriveRig::steer`] is a
//! throttle and a brake, not a player, and 63.5 miles in -- most of the way
//! to Silverthorne, over the Divide and down the far side -- it has run the
//! air tanks down and the spring brakes park the truck. That is about
//! 35 000 timed frames, ten minutes of real play, and the report names
//! where and why it ended rather than leaving a reader to assume the road
//! ran out. Anyone extending this should teach the driver the retarder
//! before reaching for a longer route.
//!
//! Traffic and hazards are LEFT ON. `PlaytestHarness` neutralises both
//! (`neutralize_random_trip_friction`) because it is measuring what the
//! game SAYS and traffic makes that non-repeatable; here they are the
//! workload, and the trip seed is what makes them repeatable instead.
//!
//! # Determinism
//!
//! The trip seed is pinned ([`TRIP_SEED`]) and the start hour with it. A
//! seeded trip seeds the weather system too (`WeatherSystem::new`), so the
//! run cannot draw an ice day one morning and a clear one the next -- an
//! unseeded drive picks fresh weather every time and ice changes the whole
//! drive. The event pacer runs on a [`FakeClock`] advanced one frame per
//! frame, for the reason written up on `PlaytestHarness::clock`: on the
//! wall clock a simulated drive outruns real time by two orders of
//! magnitude and the pacer correctly drops almost every ambient line, so
//! the speech half of the frame would go unmeasured.
//!
//! # Which build the numbers come from
//!
//! The report's numbers are from a RELEASE build, because a frame time out
//! of an unoptimised build is not the game's frame time. In this workspace
//! that is a smaller distinction than usual: `[profile.test]` compiles test
//! binaries at `opt-level = 2` even for a plain `cargo test`, and the same
//! drive measured 59.9 us there against 54.1 us under `--release`
//! (2026-08-24). Run it:
//!
//! ```text
//! CARGO_TARGET_DIR=target/frame cargo test --release -p freight-fate \
//!     --test frame_time -- bench --ignored --nocapture
//! ```
//!
//! # Its own binary
//!
//! Not a module of `it`, because a wall-clock gate cannot share a process
//! with 3 000 other tests. Inside `it` the drive thread competes for the
//! CPU with every sibling test's thread and the helper threads those tests
//! start. Run oversubscribed (`--test-threads 16` on four cores,
//! 2026-10-02), the gate's frame was waiting to be scheduled for almost all
//! of its slow frames -- 7.2 ms runnable-but-waiting out of a 7.4 ms frame
//! -- and p99 read 7.4 ms and 10.1 ms against a median of 65 us, the same
//! shape the dev machine showed in a full run (p99 6.7-9.6 ms, median
//! 107 us). Cargo runs test binaries one after another, so here nothing
//! else from the suite is running while a gate times, and [`TIMING`] keeps
//! the gates in this file from timing over each other.

mod drive;

use std::time::Instant;

use freight_fate::app::FPS;

use drive::{
    drive_full_frames, drive_sim_only, drive_tick_phases, mean_of, phase_line, stats, Run, Stats,
};

/// What one frame is allowed to cost end to end if the loop is to hold the
/// rate it targets: `FrameClock::tick(FPS)` sleeps out the remainder of
/// this and no longer, so a frame that overruns it is a dropped frame.
const BUDGET_US: f64 = 1_000_000.0 / FPS as f64; // 16 666.7 us at 60 Hz

/// Frames thrown away before timing starts. The first ticks of a drive do
/// one-off work no later frame repeats -- the departure chain, the first
/// zone and corridor lookups, the first music selection.
const WARMUP: usize = 600;

/// Held by whatever is timing frames, so the gates in this binary (which
/// the harness would otherwise run on parallel threads) never compete with
/// each other for a core. Poison-tolerant: one failed gate must not fail
/// the next with a lock error instead of its own verdict.
static TIMING: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn timing() -> std::sync::MutexGuard<'static, ()> {
    TIMING.lock().unwrap_or_else(|e| e.into_inner())
}

fn env_usize(name: &str, fallback: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(fallback)
}

// -- the report ------------------------------------------------------------------------

fn report(title: &str, run: &Run) -> Stats {
    let totals: Vec<f64> = run.frames.iter().map(|f| f.total_us).collect();
    let s = stats(&totals);
    println!("\n{title}");
    println!("  frames timed       {}", s.n);
    println!(
        "  drove              {:.1} of {:.0} mi ({} legs), {:.0} game minutes{}",
        run.end_mi,
        run.total_mi,
        run.legs_covered,
        run.game_minutes,
        if run.finished {
            ", reached the gate"
        } else {
            ""
        }
    );
    println!("  end speed          {:.1} mph", run.end_speed_mph);
    println!("  lines spoken       {}", run.spoken);
    if !run.finished {
        println!("  ended on           {}", run.last_lines.join(" | "));
    }
    println!("  frames off-road    {} (a screen was pushed)", run.pushes);
    println!("  frame mean         {:.2} us", s.mean_us);
    println!("  frame median       {:.2} us", s.median_us);
    println!("  frame p95          {:.2} us", s.p95_us);
    println!("  frame p99          {:.2} us", s.p99_us);
    println!("  frame max          {:.2} us", s.max_us);
    println!("  timed total        {:.1} ms", s.total_ms);
    println!(
        "  sustainable rate   {:.0} fps if nothing else ran ({:.3}% of the \
         {:.0} us budget at the mean, {:.2}% at p99)",
        1e6 / s.mean_us,
        100.0 * s.mean_us / BUDGET_US,
        BUDGET_US,
        100.0 * s.p99_us / BUDGET_US,
    );
    let over = run.frames.iter().filter(|f| f.total_us > BUDGET_US).count();
    println!("  frames over budget {over} of {}", s.n);
    s
}

#[test]
#[ignore = "benchmark: run with --release --ignored --nocapture"]
fn bench_frame_time_on_a_mountain_route() {
    let _timing = timing();
    let warmup = env_usize("FF_FRAME_WARMUP", WARMUP);
    // High enough to reach Grand Junction; the run stops itself at the gate.
    let max_frames = env_usize("FF_FRAME_FRAMES", 200_000);

    if cfg!(debug_assertions) {
        println!(
            "WARNING: this is a DEBUG build (opt-level 0). The numbers below \
             are not the game's frame time. Re-run with --release."
        );
    }

    let t0 = Instant::now();
    let full = drive_full_frames(warmup, max_frames);
    let full_wall = t0.elapsed().as_secs_f64();
    let full_stats = report("full frame (App::tick + the line build)", &full);
    println!("  wall clock         {full_wall:.1} s for the whole drive");

    // -- phase attribution -------------------------------------------------------------
    let tick: Vec<f64> = full.frames.iter().map(|f| f.tick_us).collect();
    let lines: Vec<f64> = full.frames.iter().map(|f| f.lines_us).collect();
    let tick_s = stats(&tick);
    let lines_s = stats(&lines);

    let sim = drive_sim_only(warmup, max_frames);
    let sim_stats = report("sim only (DrivingState::update_frame)", &sim);

    let overhead = (tick_s.mean_us - sim_stats.mean_us).max(0.0);
    println!("\nwhere the frame goes (means, us)");
    println!(
        "  sim step           {:8.2}  {:5.1}%",
        sim_stats.mean_us,
        100.0 * sim_stats.mean_us / full_stats.mean_us
    );
    println!(
        "  rest of App::tick  {overhead:8.2}  {:5.1}%  (speech poll, audio \
         fades, the duck, controller, presence, cloud notices)",
        100.0 * overhead / full_stats.mean_us
    );
    println!(
        "  line build         {:8.2}  {:5.1}%  (the 18 rows App::render \
         hands the window)",
        lines_s.mean_us,
        100.0 * lines_s.mean_us / full_stats.mean_us
    );
    println!("  line build p99     {:8.2}", lines_s.p99_us);
    println!(
        "  cross-check        the two runs ended at {:.1} mi and {:.1} mi; \
         the split is fair only where those agree",
        full.end_mi, sim.end_mi
    );

    // -- inside the tick ----------------------------------------------------------------
    let (phases, phase_end_mi) = drive_tick_phases(warmup, max_frames);
    println!("\ninside App::tick, phase by phase (us per frame)");
    phase_line("controller", &phases.controller, full_stats.mean_us);
    phase_line("speech poll", &phases.speech_poll, full_stats.mean_us);
    phase_line("cloud notices", &phases.cloud, full_stats.mean_us);
    phase_line("audio", &phases.audio, full_stats.mean_us);
    phase_line("speech duck", &phases.duck, full_stats.mean_us);
    phase_line("state update (sim)", &phases.sim, full_stats.mean_us);
    phase_line(
        "presence: build",
        &phases.presence_build,
        full_stats.mean_us,
    );
    phase_line(
        "presence: hand off",
        &phases.presence_push,
        full_stats.mean_us,
    );
    phase_line("online: build", &phases.online_build, full_stats.mean_us);
    phase_line("online: hand off", &phases.online_push, full_stats.mean_us);
    println!("  (this pass ended at {phase_end_mi:.1} mi)");

    // Does any phase get more expensive the longer the drive runs? A frame
    // cost that climbs with time driven is a different bug from a frame
    // cost that is merely high, and only one of them gets worse on a long
    // haul.
    let drift = |name: &str, samples: &[f64]| {
        if samples.len() < 200 {
            return;
        }
        let tenth = samples.len() / 10;
        let first: f64 = samples[..tenth].iter().sum::<f64>() / tenth as f64;
        let last: f64 = samples[samples.len() - tenth..].iter().sum::<f64>() / tenth as f64;
        println!(
            "  {name:<22} first tenth {first:9.2} us -> last tenth {last:9.2} us  \
             ({:.2}x)",
            if first > 0.0 { last / first } else { 0.0 }
        );
    };
    println!("\ndoes it get worse as the drive goes on?");
    drift("state update (sim)", &phases.sim);
    drift("presence: build", &phases.presence_build);
    drift("presence: hand off", &phases.presence_push);
    drift("online: build", &phases.online_build);
    drift("online: hand off", &phases.online_push);

    // -- what makes a frame expensive ---------------------------------------------------
    println!("\nwhat a slow frame was doing");
    if let Some((n, mean)) = mean_of(&full.frames, |f| f.spoke > 0) {
        println!("  frames that spoke      {n:6}  mean {mean:8.2} us");
    }
    if let Some((n, mean)) = mean_of(&full.frames, |f| f.spoke == 0) {
        println!("  frames that did not    {n:6}  mean {mean:8.2} us");
    }
    let mut leg_change = vec![false; full.frames.len()];
    for (flag, pair) in leg_change.iter_mut().skip(1).zip(full.frames.windows(2)) {
        *flag = pair[1].leg != pair[0].leg;
    }
    if let Some((n, mean)) = mean_of(&full.frames, |f| {
        leg_change[f.index.min(leg_change.len() - 1)]
    }) {
        println!("  frames changing leg    {n:6}  mean {mean:8.2} us");
    }
    if let Some((n, mean)) = mean_of(&full.frames, |f| f.depth > 1) {
        println!("  frames on a pushed screen {n:3}  mean {mean:8.2} us");
    }

    let mut worst = full.frames.clone();
    worst.sort_by(|a, b| b.total_us.partial_cmp(&a.total_us).expect("finite"));
    println!("\n  the twelve slowest frames");
    println!(
        "    {:>8}  {:>10}  {:>9}  {:>5}  {:>5}  {:>5}",
        "frame", "total us", "lines us", "leg", "said", "depth"
    );
    for f in worst.iter().take(12) {
        println!(
            "    {:>8}  {:>10.1}  {:>9.1}  {:>5}  {:>5}  {:>5}",
            f.index, f.total_us, f.lines_us, f.leg, f.spoke, f.depth
        );
    }
}

// -- the gate --------------------------------------------------------------------------

/// Frames each gate times. Enough that p99 has a real tail (its rank is the
/// 20th-slowest frame) and cheap enough to sit in the ordinary suite: about
/// a second per gate, release or debug.
const GATE_FRAMES: usize = 2_000;
const GATE_WARMUP: usize = 300;

/// Ground the gates must cover before their numbers mean anything.
///
/// Both would pass trivially on a parked truck: a frame that simulates
/// nothing is cheap, and its presence string cheaper still. So each checks
/// the drive went somewhere first. From a standing start, 2 300 frames of
/// this route put the truck about 2.7 miles along; a mile is comfortably
/// below that and far above anything a truck that never released its
/// brakes could reach.
const MOVED_MI: f64 = 1.0;

/// The share of one 60 Hz frame the driven frame is allowed at p99.
///
/// Derived, not fitted to this machine:
///
/// * The loop runs at `app::FPS` = 60, and `FrameClock::tick` sleeps out
///   only the remainder of `1/FPS`. So the whole frame budget is
///   [`BUDGET_US`] = 16 667 us, and anything over it is a dropped frame.
/// * This test measures the headless frame. A player's frame also carries
///   the SDL event pump and the window render, the mixer playing its channels,
///   and Prism handing text to a screen reader -- none of which exist here.
///   The measured part therefore cannot be allowed the whole budget.
/// * What it may have is set by the slowest hardware the game has to hold
///   60 fps on, not by what this desktop happens to do. Single-thread
///   throughput across the machines a Windows player plausibly runs spans
///   roughly four to one between a current desktop part and a low-end
///   mobile one (PassMark single-thread ratings, same generation). Giving
///   the measured frame a quarter of the budget here is giving it the whole
///   budget on a machine four times slower, which is the floor of support.
///
/// So: p99 <= 16 667 / 4 = 4 167 us, in release and in debug alike.
/// `[profile.test]` compiles test binaries at `opt-level = 2` even for a
/// plain `cargo test`, which is what CI runs, and the two measure the same
/// drive within a tenth (59.9 us against 54.1 us, 2026-08-24). There is no
/// second, looser number for an unoptimised run because there is no
/// meaningfully slower run to give one to; and if `[profile.test]` ever
/// went back to `opt-level = 0`, this ceiling still sits about seventy
/// times over what the frame costs.
///
/// This is a CATASTROPHE gate, not a drift gate, and the margin is large on
/// purpose: a tighter wall-clock number on a shared CI box fails when a
/// neighbouring job runs, and a perf test that fails for that reason is a
/// perf test somebody deletes.
/// [`a_frames_bookkeeping_never_out_costs_its_simulation`] is the tight
/// half of the pair, and the one that would have caught the bug this file
/// was written to find. Drift is for the report, not for the gate.
const BUDGET_SHARE: f64 = 0.25;

#[test]
fn a_driven_frame_stays_well_inside_the_sixty_hertz_budget() {
    let _timing = timing();
    let run = drive_full_frames(GATE_WARMUP, GATE_FRAMES);
    let totals: Vec<f64> = run.frames.iter().map(|f| f.total_us).collect();
    let s = stats(&totals);

    // The gate is only a gate if the frames it timed were a drive. A change
    // that parks the truck (or ends the leg early) would make every number
    // below meaninglessly small and the assertion would still pass.
    assert!(
        run.end_mi > MOVED_MI,
        "the bench drive covered only {:.2} mi in {} frames -- it was not \
         driving, so its frame times measure nothing",
        run.end_mi,
        s.n
    );
    assert!(
        run.spoken > 0,
        "the bench drive spoke nothing in {} frames; the speech half of the \
         frame went unmeasured",
        s.n
    );
    assert_eq!(s.n, GATE_FRAMES, "the drive ended before the gate's frames");

    let ceiling = BUDGET_US * BUDGET_SHARE;
    println!(
        "driven frame: mean {:.2} us, median {:.2} us, p95 {:.2} us, p99 \
         {:.2} us, max {:.2} us over {} frames ({:.1} mi); ceiling {:.0} us \
         ({:.0}% of the {:.0} us budget, {} build)",
        s.mean_us,
        s.median_us,
        s.p95_us,
        s.p99_us,
        s.max_us,
        s.n,
        run.end_mi,
        ceiling,
        BUDGET_SHARE * 100.0,
        BUDGET_US,
        if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        },
    );
    assert!(
        s.p99_us < ceiling,
        "a driven frame's p99 is {:.1} us, past the {:.1} us this build is \
         allowed ({:.0}% of the {:.0} us frame at {FPS} fps). The frame got \
         an order of magnitude more expensive; see \
         bench_frame_time_on_a_mountain_route in tests/frame_time/main.rs for where it went.",
        s.p99_us,
        ceiling,
        BUDGET_SHARE * 100.0,
        BUDGET_US,
    );
}

/// The gate with real teeth, and the one that does not care how fast the
/// machine is.
///
/// Both sides of the comparison are measured in the same process, in the
/// same frames, so a loaded CI box slows them together and the RATIO holds.
/// That is what makes it safe to state tightly where a wall-clock number
/// cannot be.
///
/// The rule it enforces is a design one, not a measurement: **building the
/// status text for a presence panel must never cost more than simulating
/// the truck.** The sim is the frame's work -- physics, the corridor, the
/// traffic bubble, the hazards, the speech ladder. Presence is a short
/// string handed to Discord and to the drivers board, both of which
/// throttle to seconds. If the string costs more than the truck, something
/// in the builder is doing work it has no business doing per frame.
///
/// That is exactly the shape of the bug this file found: the online
/// presence builder was cloning the whole radio catalog (757 stations plus
/// the identity map) every frame to read one station name, at 2 395 us a
/// frame against the sim's 84 -- twenty-eight times over this line, and
/// ninety-seven per cent of the frame. The absolute gate above did not
/// notice, because 2.4 ms still fits in a 60 Hz frame. This one fails at
/// 1.0x, so it would have failed on the first frame.
#[test]
fn a_frames_bookkeeping_never_out_costs_its_simulation() {
    let _timing = timing();
    let (phases, end_mi) = drive_tick_phases(GATE_WARMUP, GATE_FRAMES);
    assert!(
        end_mi > MOVED_MI,
        "the bench drive covered only {end_mi:.2} mi -- it was not driving"
    );
    let sim = stats(&phases.sim);
    let presence = stats(&phases.presence_build);
    let online = stats(&phases.online_build);
    println!(
        "per frame: sim {:.2} us, Discord presence {:.2} us ({:.3}x the sim), \
         drivers board {:.2} us ({:.3}x the sim)",
        sim.mean_us,
        presence.mean_us,
        presence.mean_us / sim.mean_us,
        online.mean_us,
        online.mean_us / sim.mean_us,
    );
    for (what, built) in [
        ("Discord presence", &presence),
        ("the drivers board line", &online),
    ] {
        assert!(
            built.mean_us < sim.mean_us,
            "building {what} costs {:.2} us a frame against the simulation's \
             {:.2} us ({:.1}x). A presence string is throttled to seconds and \
             must not out-cost the truck; something in that builder is doing \
             per-frame work it does not need to. Run \
             bench_frame_time_on_a_mountain_route in tests/frame_time/main.rs for the \
             breakdown.",
            built.mean_us,
            sim.mean_us,
            built.mean_us / sim.mean_us,
        );
    }
}
