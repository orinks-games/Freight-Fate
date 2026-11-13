# Fatigue redesign

Design for Freight Fate, written 2026-11-11 from the code on `dev` (be2fdbbd) and approved by Orinks. PR 1 (2026-11-13) built part 1 (the four bands, the band word in the readouts, the hysteresis), part 8 (the nod-off as a state machine saved with the trip), "The state machines" in `ff_core::sim::alertness`, and the play tester's `status`, `scenario` `fatigue` and `nod` tool. Parts 2 to 7 are not built yet. Numbers marked "assumed" are starting points for tuning, with no measured source behind them.

## Summary

Today fatigue has three effects, and all three are reaction tests: a shorter window to answer a hazard, a nod-off the player must catch by steering or braking, and a forced stop after three missed nods. Below 60 percent it does nothing at all.

The redesign keeps the one fatigue number and the nod-off at the top, and adds costs the player plans around instead of reacts to:

1. **Earlier warning and night hours.** A first yawn arrives at 40 percent, long before trouble, and a job's details say how much of the drive falls at night.
2. **Tired work is slow work.** Fueling, walk-arounds and chain work take longer as fatigue rises, which eats the 14-hour window and the deadline.
3. **Rest is earned by the hour.** Ten hours in a bunk clears 80 points, so a driver who went to bed past 80 wakes up with some left. Coffee and energy drinks lend alertness and take it back when they wear off.
4. **An officer can see it.** An inspection or traffic stop at severe fatigue ends in a fatigue out-of-service order.
5. **Dispatch will move the appointment.** A drowsy driver can call in and take the rest without a late delivery, at a price.

Underneath, the two fatigue state machines that are spread across six fields of the driving state become two enums in `ff-core`, saved with the trip and visible to the agent server.

Recommendation: build parts 1 to 5 in three pull requests. Hold the one control change (lane wander for manual steering, described under "Considered and left out") until Orinks has listened to the rest.

## What the game does today

Fatigue is one number, 0 to 100, on the driver profile.

**What raises it.** Driving faster than 5 mph: 0.115 points per game minute by day, 0.17 at night (9 PM to 5 AM). Relaxed pacing multiplies that by 0.8. A food or drink buff multiplies it by 0.7 to 0.85 while it lasts. Chain work adds 2 to 10 points. The wait at a facility gate and a classroom day add some. Parked time adds none, including on-duty time at a dock, a fuel island or an inspection (the August realism audit recorded this as item A4).

**What lowers it.** A 30-minute break takes off 35. A coffee break takes off 8. Meals, showers and drinks take off 1 to 15 at once. Ten hours of proper sleep sets it to 0 from any level. A night in a lot or on the shoulder cannot bring it below 30. A sleeper split takes off 18 or 55. Time in the bunk while a co-driver drives takes off 10 points an hour.

**What it does.**

| Fatigue | Effect |
| --- | --- |
| Below 60 | Nothing. |
| 60 to 80 | One yawn and "Getting drowsy.", a yawn every 15 game minutes, the fatigue line appears in the status readout, the hazard reaction window shrinks toward 60 percent of normal, tired billboards. |
| 80 and up | One rumble hit and "Dangerously drowsy and drifting out of your lane.", then a nod-off every 9 game minutes shrinking to 3. Each nod gives 2.2 real seconds to steer or brake. A miss costs 6 percent damage, speed, 6 reputation and a line on the safety record. The second miss on record is a serious violation. Three misses in a row park the truck out of service for 10 hours. |

### Findings from reading the code

These come from the source, not from a drive.

- **A legal daytime driver never gets drowsy.** Eight hours of day driving reaches 55. The required break takes that to 20, and the last three hours end the shift at about 41. Only night runs and hours violations reach 60, so for most careers fatigue is a number in a readout.
- **Every consequence is a reaction test.** This is the complaint that started this proposal, and the code agrees with it.
- **Sleep is flat.** Ten hours zeroes fatigue whether the driver went to bed at 30 or at 100, so there is no reason to stop early.
- **Coffee has no limit.** A coffee break takes 15 minutes and removes 8 points, and parked time adds none. Nothing I found stops a driver from taking coffee breaks back to back to erase any amount of fatigue.
- **The band is two booleans.** `drowsy_said` and `severe_said` reset the moment fatigue dips under the threshold, so a 3-point energy drink at 61 followed by two more miles speaks "Getting drowsy." again.
- **The nod-off is four loose fields** (`microsleep_deadline`, `microsleep_gm`, `microsleep_cooldown_gm`, `microsleep_misses`), and none of them is in the trip snapshot. Saving and resuming a drive resets the time to the next nod and the count of misses toward the forced stop.
- **Five other files read `microsleep_deadline.is_some()`** to decide whether they may speak or act (turns, enforcement, lane gap, keys, team driving). The rule "one demand on the driver at a time" lives in those scattered checks.
- **Two rest screens appear to judge "slept before it got bad" against different numbers**: 70 at the city terminal, 80 at a rest stop.

## Design rules

1. **No information is withheld.** A tired driver hears every callout, at the same time, in the same words. Fatigue costs time, money, rest and standing. It never costs a spoken line.
2. **Every cost can be read before it lands.** The rest preview and the stop menus say the number first.
3. **One line or sound per change of state**, and nothing repeated on a key press. No "X ahead" lines.
4. **The one number stays.** Saves, readouts, buffs and tests keep `fatigue` as it is.
5. **Relaxed stays relaxed.** Relaxed pacing keeps its slower build and gets the softer version of each new cost, listed under "Modes".

## The redesign

### 1. Four bands

| Band | Fatigue | Entering it |
| --- | --- | --- |
| Rested | below 40 | silent |
| Worn | 40 to 60 | one yawn, no speech |
| Drowsy | 60 to 80 | yawn and "Getting drowsy." |
| Severe | 80 and up | one rumble strip hit and "Dangerously drowsy and drifting out of your lane." |

A band is left downward only 5 points below where it was entered, so a small lift does not replay the line a mile later. Dropping a band is silent on the road; the rest screens already say "Fatigue 62 to 27."

The status readout names the band with the number from Worn up: "Fatigue: 64 percent, drowsy."

### 2. What builds fatigue

- Driving rates stay as they are.
- On-duty time that is not driving builds fatigue at one third of the driving rate (assumed; the realism audit's suggestion). This covers fueling, dock time, inspections and repairs waited out at the truck. A four-hour dock delay now costs about 9 points by day instead of none.
- Off-duty and sleeper time builds none.

With the Worn band at 40, a legal day driver now crosses a threshold about six hours in. Taking the 30-minute break at hour five instead of hour eight keeps the driver out of Worn for the whole shift, which makes break timing a choice.

### 3. Night hours on the trip sheet

An earlier draft had the game predict a clock time and place for getting drowsy ("Drowsy about 2 AM, near Des Moines."). Orinks questioned that on 2026-11-11 and it is dropped. Fleets do run fatigue models over schedules, but no driver is told the minute they will get sleepy. A driver knows two things: the schedule, and how they feel.

**The schedule.** Where a job's details and the departure line already give trip hours, they add how much of the drive falls at night, using the night hours the game already has (9 PM to 5 AM): "About 4 hours of night driving." Nothing is said for a daytime run. This is a fact about the trip, not a prediction about the driver.

**How the driver feels.** The band cues are the body: the first yawn at Worn, the yawn and "Getting drowsy." at Drowsy, the yawns that keep coming every 15 game minutes. The Worn yawn is new and arrives well before anything goes wrong, so it is the early warning the forecast was meant to be.

The rest preview is unchanged. It already reads fatigue before and after a rest.

### 4. Tired work is slow work

Work the driver does by hand takes longer (all assumed):

| Band | Extra time |
| --- | --- |
| Worn | 10 percent |
| Drowsy | 25 percent |
| Severe | 50 percent |

This applies to fueling (20 minutes), the walk-around (15), chain install, removal and repair, and pulling into a stop. It does not apply to time someone else controls: the receiver's unloading, a mechanic, an officer's inspection.

The stop menus already read each task's minutes, so the longer figure is read there and no new line is needed. At Severe, a 30-minute chain install becomes 45, all of it inside the 14-hour window.

### 5. Rest by the hour

- **Proper sleep** (sleeper parking, motel, terminal) removes 8 points an hour. Ten hours clears 80. A driver who beds down at 95 wakes at 15, and the rest preview says so before they choose.
- **Poor sleep** (lot, shoulder, chain control) removes 5 an hour and keeps today's floor of 30.
- **The bunk while a co-driver drives** drops from 10 to 8 an hour, the same as parked sleep.
- **Breaks and splits** keep today's values.
- **Stimulants are a loan.** The instant lift from a coffee break, a coffee or an energy drink comes back when it wears off (two hours for the coffee break). The existing worn-off line carries it: "The energy drink has worn off." A second stimulant while one is active adds nothing. Meals and showers stay real relief.

Coffee still gets a driver to the next stop. It no longer gets them through the night.

### 6. Enforcement

49 CFR 392.3 forbids driving while impaired by fatigue, and an officer can place a fatigued driver out of service. The game already cites that rule for run-off-road events.

- A traffic stop or inspection while Severe ends in a fatigue out-of-service order: 10 hours off duty, the existing order and wording.
- Drowsy and below: nothing.
- It is certain, not a roll, so a player at Severe knows what the next scale house means.

### 7. Calling dispatch for rest

The dispatch call gets one row while the driver is Drowsy or Severe on a load: **Too tired to drive safely.**

- Dispatch moves the appointment by the length of a 10-hour rest plus the drive to the nearest stop with parking. Once per load.
- The load pays its base rate with no on-time bonus and no late penalty.
- The third call inside seven game days costs dispatch trust, with a line from the dispatcher about planning.
- The row is absent on a team load while the co-driver has hours, since handing over the wheel is the answer there.

This gives a tired driver three priced options instead of one: push on and risk the record, sleep and deliver late, or call it in and give up the bonus.

### 8. The nod-off

Unchanged in what the player hears and does. Two fixes come with the state machine: the timer and the miss count are saved with the trip, and a co-driver taking the wheel or the truck stopping clears a nod in one place.

### Modes

- **Relaxed pacing**: fatigue builds at 0.8 as now. Slow work is halved (5, 12 and 25 percent). The inspection at Severe gives a warning the first time in a career and the order after that.
- **Hours enforcement off**: fatigue is the only limit on the day, as now. The night-hours line and the yawns are the planning tools there. The inspection rule does not apply, matching how enforcement is bypassed today.
- **Real time**: same as Standard.

## The state machines

Both live in a new `ff_core::sim::alertness` module. Each has one `next` function and a transition test table, per the owner rule of 2026-11-09.

### Alertness band

```rust
pub enum Alertness { Rested, Worn, Drowsy, Severe }

impl Alertness {
    /// The only place the band moves.
    pub fn next(self, fatigue: f64) -> (Alertness, Option<BandCue>);
}
```

| From | Fatigue | To | Cue |
| --- | --- | --- | --- |
| Rested | 40 or more | Worn | yawn |
| Worn | 60 or more | Drowsy | yawn, "Getting drowsy." |
| Drowsy | 80 or more | Severe | rumble hit, "Dangerously drowsy and drifting out of your lane." |
| Severe | below 75 | Drowsy | none |
| Drowsy | below 55 | Worn | none |
| Worn | below 35 | Rested | none |

A jump across two bands (a save loaded at 85, a scenario) gives only the cue of the band landed in. The band is derived state: it is saved with the trip so a resume does not replay a cue, and rebuilt from the number if missing.

Every reader of the thresholds (status readout, billboards, the classroom cue, the shoulder-sleep hint, the rest achievements, slow work, enforcement) asks the band, which also settles the 70 against 80 question.

### Nod

```rust
pub enum Nod {
    Awake { since_gm: f64 },      // counting toward the next nod
    Nodding { left_s: f64 },      // reaction window open
    Recovering { left_gm: f64 },  // quiet period after one resolves
}

pub struct NodState { pub nod: Nod, pub misses: u8 }

pub enum NodEvent {
    Tick { gm: f64, dt_s: f64, band: Alertness, fatigue: f64, moving: bool, crawling: bool, busy: bool },
    Reacted,
    WheelHandedOver,
}

pub enum NodOutcome { None, Began, Caught, CaughtSilent, DriftedOff, ForcedStop }
```

| From | Event | To | Outcome |
| --- | --- | --- | --- |
| Awake | Tick, band below Severe or not moving | Awake, counter reset | None |
| Awake | Tick, `busy` (a hazard is waiting on the driver) | Awake, counter held | None |
| Awake | Tick, counter reaches the interval | Nodding 2.2 s | Began |
| Nodding | Reacted | Recovering 4 min, misses 0 | Caught |
| Nodding | Tick, `crawling` | Recovering 4 min, misses 0 | CaughtSilent |
| Nodding | Tick, time runs out, misses below 2 | Recovering 4 min, misses plus 1 | DriftedOff |
| Nodding | Tick, time runs out, misses at 2 | Recovering 4 min, misses 0 | ForcedStop |
| Recovering | Tick, time runs out | Awake | None |
| any | WheelHandedOver | Awake, counter reset | None |

`DrivingState` keeps the side effects (sound, speech, damage, the record, the out-of-service order) and matches on the outcome. The five files that read `microsleep_deadline.is_some()` call one `nod.demands_driver()` instead.

### For the play tester

- Agent server `status` reports `alertness` (the band), `nod` (the variant and its timer) and `nod_misses`.
- `scenario` takes `fatigue`, and may take `nod` to stage a nod directly, the way the `hazard` tool stages a hazard.
- Both machines go in the trip snapshot.

## Considered and left out

- **Dropping or delaying callouts for a tired driver.** It would read as the game breaking, and it takes away the one channel the player drives by. Rule 1 forbids it.
- **Lane wander for manual steering.** At Drowsy and Severe the truck could drift harder with lane keeping off. It is a real control effect, but it does nothing for players on lane keeping and cruise, and whether it feels like fatigue or like a fault can only be judged by ear. Worth a later experiment after the rest has been driven.
- **A forced missed exit.** The same objection as dropped callouts.
- **A second "sleep debt" number.** Rest by the hour and the poor-sleep floor carry most of it without another readout to learn.
- **Fuel economy or cab climate effects.** No source to cite, and the cab climate was made ambience on purpose.
- **A spoken drowsiness forecast.** Dropped after review; see part 3.
- **An afternoon dip in the day rate.** Real, but it would put Worn inside most legal days twice and needs its own tuning pass.

## Tradeoffs

- **More to track.** Three priced choices are the point, and they are also more for a new player to learn. The band costs are silent on the road and read only where the player already looks.
- **Players who drive to unwind will feel it.** A legal day now touches Worn, where today it touches nothing. Worn costs minutes at stops and nothing behind the wheel, and Relaxed halves it.
- **Most numbers are assumed.** The slow-work percentages, the on-duty rate and the 8 points an hour need tuning against real careers. Each is one constant.
- **Stimulants get weaker.** Players who lean on energy drinks through a night run will meet Severe sooner than they do now. The help text and worn-off lines have to say so plainly, or it reads as a bug.
- **Spoken strings change.** Rest previews, buff help, the manual's hours section, the dispatch call and several pinned tests move, and the translation files with them.
- **Old saves.** The fatigue number carries over. The band is rebuilt on load, and a trip saved in the middle of a nod resumes awake once.

## Implementation estimate

Three pull requests against `dev`, each driven through the agent server with facility streets off.

| PR | Contents | Size |
| --- | --- | --- |
| 1 | The `alertness` module with both enums, transition tables, snapshot, agent server `status` and `scenario`, hysteresis, the band word in the readout. No new costs. | Medium. About 400 lines of new core code plus rewiring the driving state. One session. |
| 2 | The night-hours line (job details, departure line), on-duty build, rest by the hour, stimulants as a loan, manual and help text, translations. | Medium to large. The most spoken text and the most pinned tests. One session. |
| 3 | Slow work, the inspection rule, the dispatch rest call, ontology rows, changelog and roadmap. | Medium. One session. |

Each PR is safe to ship without the next. After PR 2 the numbers want a listening and tuning pass from Orinks before PR 3 adds more costs on top.
