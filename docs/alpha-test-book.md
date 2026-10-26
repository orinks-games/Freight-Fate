# Freight Fate 1.9 alpha test book

The 1.9 line is an opt-in alpha: a version with lots of changes, running
ahead of the calm nightly line. This book is for the people testing it —
it says what is different, how to hear each change for yourself, and when
to call it working. The physics side (jake, brakes, ice, chains) has its
own companion volume, `docs/physics-playtest-checklists.md`; this book
covers everything else and points there for the winter suite.

Written for a screen-reader playtest: everything to verify is spoken,
never visual. Each checklist says how to set it up, what to do, what to
listen for, and when to call it a pass. Work them in order or
cherry-pick — every one stands alone, except Chapter 0, which is setup
you need before the game will behave the way the rest of this book
describes.

When something fails, note three things: what you did, what you heard,
and what you expected to hear. That sentence pair is worth more than any
log file.

## Chapter 0. Before your first drive

Four things you cannot work out by playing, and one thing that is missing
on purpose. Read this chapter before you open the game; the rest of the
book assumes you have.

### 0.1 The alpha talks to a test server, not the real orinks.net

Every online feature in this alpha — the drivers board, profile sharing,
cloud backup — talks to a staging copy of the website
instead of the real one. It is a separate site, with separate accounts
and separate data.

Three things follow from that, and all three are intended:

- **Your real orinks.net sign-in will not work here.** Make a fresh
  account on the staging site and use that one for the whole alpha.
- **Nothing you do touches your real driver.** Your real profile, your
  real board listing, and your real backups are untouched no matter what
  you do in the alpha.
- **Everything you make on staging is disposable.** Staging careers and
  backups will not carry over when 1.9 releases. Never let a career you
  care about exist only as a staging backup.

Do not file "my orinks.net account is rejected" as a bug. What is worth
reporting is a spoken line that says orinks.net without making clear
which one you are being sent to, a refusal that leaves you no way to try
again, or anything you do in the alpha showing up on the real site.

### 0.2 Connecting an account is a code and a browser

Setup: the staging account from 0.1, and a browser you can reach.

Do: from the main menu, open Online, then "Set up orinks.net account".
Choose the first item. If the browser does not open by itself, use "Say
my activation code again" to hear it spelled out phonetically, or "Copy
my activation code" to put it on the clipboard, and go to the site
yourself. Sign in there and enter the code. Come back to the game and
wait.

Listen for: the game saying it is contacting the site, then reading you
a short activation code; the menu item itself changing to say it is
waiting for that code; a spoken re-orientation when you switch back to
the game from the browser; and a confirmation naming your driver once
the code is claimed.

Pass when: you got connected using speech alone, you could replay the
code as many times as you needed, and the confirmation said in so many
words that your careers are backing up and profile sharing is on.

The driver name in that confirmation is there to be checked, not
admired: it is how you would find out somebody else claimed the code you
just read aloud. If it is not your driver, say so.

One deliberate design worth knowing, and it changed in 1.9: connecting
an account now turns profile sharing and cloud backup on, because the
career statistics on your public profile are read out of the backup, and
an account that publishes nothing left new drivers looking at an empty
profile. Both are single items on the Online menu if you want either
off. A confirmation that claims otherwise, in either direction, is a
bug.

Also expected: right after you create your very first career, the game
offers this setup once, unprompted, with "Not now" already selected. It
is asked once ever and never again. That is not a stray prompt.

### 0.3 What is already on before you touch anything

Two things are on when you install: **Online services**, the master
switch, and **Discord presence**. Discord presence shares only broad
activity — main menu, driving a route, resting — and does nothing at all
unless Discord is running. The master switch is only a master switch:
everything underneath it is off or needs an account, so nothing is
published because of it.

Off until you turn each one on: live weather, traffic and parking. Profile sharing and cloud backup are off too until
you connect an account, which is what turns them on — see 0.2.

The radio plays its full dial out of the box, real public streams
included. The streamer-safe setting is the opt-out: turn it on while
streaming or recording and the radio keeps to built-in safe stations.

The sharing switches all live on one screen: Online, on the main menu.
The streamer-safe switch is under Settings, Audio. Live weather,
traffic and parking are under Settings, Speech and weather, and they
follow their own toggles — the master switch does not reach them.

### 0.4 Hear what gets shared before any of it is shared

Do: on the Online menu, before turning anything on, open account setup
and choose "Hear what gets shared". Then start turning profile sharing
on and choose the same item from the confirmation screen. Press F1 on
each row of the Online menu.

Listen for: one disclosure that says up front that connecting an account
turns profile sharing on and starts backing your careers up, and where
to turn either off; that names your driver name, broad on-duty board
activity, eligible profile details, achievements, road-journal posts,
the updates feed, and the game version you are running; and that says
plainly what is never published — your real name, your full save,
coordinates, cargo details, precise location, and the money you
currently have. Lifetime career earnings are public; current cash is
not.

Pass when: you could hear the whole disclosure twice, from two places,
without anything being transmitted, and every F1 row agreed with it.

### 0.5 There is no driving school in this alpha

The practice road where nothing counts is held back for a later
release. It is not in the terminal menu and it is not hiding behind a
level. **Do not file its absence as a bug.**

What you get instead: a first-drive tutorial that walks you through
starting the engine, waiting for air, releasing the parking brake and
pulling away, and then stops; and "Learn game sounds" on the main menu
and in the pause menu, which plays any road cue on demand in seven
groups with F1 explaining what each one means. Everything past pulling
away — lane changes, engine brake stages, long descents, exits — you
learn on a real load.

Worth reporting: a spoken prompt that tells you to go to the driving
school, or a cue you met at speed that has no entry in Learn game
sounds.

## Chapter 1. What the alpha changes, system by system

An exhaustive tour of the gap between the nightly you are used to and
this alpha. Each entry names the chapter or companion checklist that
tests it.

**The truck is a different machine.** The engine brake is now a real
three-stage jake working through the gears — strong in a low gear at high
RPM, nearly useless in overdrive — and automatics pre-select down to put
it to work. Brakes heat like real drums: drag them down a grade and they
fade; jake-and-snub keeps them cool. Letting a downhill spin the engine
past its limit now wears the engine. Companion volume, checklists 5
and 9.

**Winter is real.** Freezing rain is its own weather and the one worth
parking for. Winter tires are a garage choice with trade-offs.
Chains ride in the side box, take real minutes and fatigue to hang (more
in the dark), transform ice stops and icy descents, and grind apart on
bare pavement. Chain laws activate over the steep passes with flashing
signs, warnings, and a citation that starts at 580 dollars and climbs if
you have a record or are inside a construction zone. Worn tires
hydroplane at lower speeds; the jake can break the drive wheels loose on
ice. Companion volume, checklists 1 through 8.

**Wear, and the truck it belongs to.** Tires, brakes, and engine each
have their own wear meter, driven by how you actually drive. Wear talks
back: bald tires grip less, worn brakes fade sooner, a tired engine loses
power and drinks fuel. Condition — wear, damage, fuel — now stays with
each truck instead of following your profile between tractors. Chapter 2.

**Truck stops earn their parking lots.** Meals, showers, and rig care are
purchasable at real branded stops, each good at what it is really known
for, and the effects are spoken buffs with clocks on them. Road shops fix
tires and brakes at brand-true prices; Big Buck's, famously, fixes
nothing. Chapter 3.

**The lane is a place.** Discrete lanes with real traffic in them:
dodgeable hazards, sideswipe risk, construction lane closures, keep-right
nags, and exits that gate on being in the right lane. Highway exits take
a real setup — signal with X, get to the right lane, steer into the exit
lane where it opens just before the gore, and slow down in it. Signalized ramp terminals run a stable green/yellow/red cycle with
dedicated earcons and a cross-traffic clearance before green. Chapter 4.

**Traffic has a clock.** Congestion is grounded in real federal traffic
counts per leg: metro stretches jam at rush hour and flow free at
midnight, and entering a live jam puts slow traffic in both lanes.
Chapter 5.

**Cities are driven, not teleported.** Tier-1 surface streets carry real
turn-by-turn cues with direction-shaped earcons panned to the maneuver
side. Arrivals flow off the ramp onto the destination's streets; loaded
departures drive the streets out to the on-ramp. The terminal's dispatch
board, garage, and truck dealer are all direct menu items, naming the
real local dealer where sourced data exists. Chapter 6.

**The law is watching more than your speed.** Weigh-station blow-pasts
and severe visible damage draw stops; running from lights escalates to a
felony stop with spike strips; construction zones stage a merge and
flagger before the barrels; CB chatter hints at troopers ahead. Traffic
stops read a real logbook — the in-cab Record of Duty Status — and dock
work takes spoken on-duty time. Chapter 7.

**Three trip-pacing modes.** Relaxed runs at ten times real time, Standard
at twenty times, and Real time at normal speed. The retired Realistic
pacing choice now uses Standard. Chapter 8 compares the accelerated modes.

**The career reads like employment.** New careers choose among fictional
starter carriers (assigned equipment, carrier-paid fuel and repairs,
different wage and freight personalities) or a riskier owner-operator
start. A 30-level business arc runs company ranks to the leased-on gate
at 18, own authority at 25, independent ranks to 30. Dispatch freedom is
earned: new hires run what dispatch assigns against a small decline
budget; the full board unlocks at level 8; route choice belongs to
owner-operators. Trailers, carrier accounts, reputation trust bonuses,
endorsement courses, and a first-day briefing that repeats until your
first load. Chapter 9.

**172 achievements, in seven categories.** State, region, and city
arrivals, cargo firsts, close calls, mishaps, and career milestones. The
badge wall opens on the categories and each one counts what you have
earned in it. Chapter 9 covers the spot checks.

**The law costs money now, and money has a floor under it.** Every fine
in the game lives on one schedule and most of them roughly tripled; a
fine earned inside a construction zone is doubled, and every citation
already on your record makes the next one dearer, up to twice the base.
An unpaid fine becomes a balance you carry, only a quarter of each
settlement goes to it, and the balance is not allowed to grow without
end. Chapter 9.6.

**The radio follows the map.** M toggles it, Page Down and Page Up tune
to the next and previous of the stations you can actually receive, and
Control with a tuning key jumps a whole category of the dial. The
thirteen Freight Fate regional stations are always receivable wherever
you are; real stations have real coverage and fade at the edge of it.
The full dial plays out of the box; streamer-safe mode is the opt-out
for anyone broadcasting their drive. The radio draws power from the
engine and falls silent with it. Chapter 10.

**The world got big and specific.** More than a hundred new dispatchable
cities fill the dead zones — the mountain West, the northern plains, the
Great Basin, Appalachia — on their real roads with real grades,
checkpoints, and truck stops. Roadside landmarks speak as ambient chatter
with per-kind switches in Settings. The career clock crosses real US time
zones, spoken, with deadlines in destination local time. Chapter 11.

**Online services point at a test server.** The drivers board, profile
sharing and cloud backup all talk to a staging copy of
orinks.net, with its own accounts and its own throwaway data. You need a
fresh account there and your real one will not work. Chapter 0.

**Cloud restores get a second integrity check.** Beyond the server's
signature, a restored profile must pass the game's own sanity rules, and
a file that fails is refused with a plainly spoken reason. Also shipping
on the nightly line. Chapter 12.

**Careers from before 1.9 do not carry over.** 1.9 rebalances pay,
trucks and levels, so every driver starts fresh. Your old save is left
untouched on this computer and still works in 1.8, and the game explains
this when you try to continue one. This is intended, not a bug —
Chapter 12 covers what is worth reporting about it.

## Chapter 2. Wear meters and the truck they belong to

### 2.1 The three meters move for three reasons

Setup: an owner-operator profile with a loaded, longish leg — a few
hundred miles of ordinary interstate.

Do: before departing, open the truck status readout (Tab while driving,
or the garage's condition readout in town) and note tire, brake, and
engine wear. Drive the leg with some deliberate sins: ride the service
brakes on one descent, hold a low gear past the shift point for a minute,
run loaded the whole way. Deliver, and listen to the delivery summary.

Listen for: the delivery summary telling you what the run added to each
meter, separately. The status readout speaking all three meters any time
you ask.

Pass when: each meter moved and the summary attributed wear where you
earned it — brake wear from the dragged descent, engine wear from the
lugging and the loaded hours, tire wear from the miles.

### 2.2 Wear talks back

Setup: wear a truck through many legs or use an established
high-mileage profile.

Do: compare a hard stop and a fuel readout against a fresh truck's.

Listen for: worn brakes pulling weaker and overheating sooner (the fade
warning arrives earlier than the companion volume's fresh-truck
anchors); a tired engine burning noticeably more fuel on the F readout
over the same leg.

Pass when: the worn truck is audibly worse in the way each meter
promises, and the garage quotes a repair for exactly the worn item.

### 2.3 Condition follows the truck, not you

Setup: an owner-operator with enough money for a second tractor.

Do: run your current truck until it carries obvious wear and a
half-empty tank; note the readouts. Buy a second truck at the dealer and
drive it. Then swap back.

Listen for: the new truck rolling out fresh with a full tank — none of
your first truck's wear or fuel state carried over. On the swap back,
the first truck exactly as you left it: same wear, same tank.

Pass when: nothing teleported between tractors, and the garage in town
fixes the truck you actually drove in, not the one parked.

### 2.4 The garage sells more than tires

Setup: an owner-operator in town with worn brakes and a worn engine.

Do: open the terminal garage and walk the service list.

Listen for: brake jobs and engine overhauls offered alongside tire
replacement, each quoting shop time and a real price. On a company
driver profile, the same services billing the carrier instead.

Pass when: each service restores its meter, costs an owner-operator real
money, and takes spoken shop time.

## Chapter 3. Truck stops: meals, showers, and rig care

### 3.1 One food buff at a time, and it never adds hours

Setup: any loaded run past branded truck stops; note your fatigue in the
Tab status menu, and your time at the wheel with Alt+A.

Do: pull into a stop (T at an announced stop) and buy a hot meal. Drive
an hour, then buy an energy drink somewhere else.

Listen for: the meal easing fatigue immediately and the status readout
(Tab) naming the active food buff and how long it has left; the energy
drink replacing the meal buff rather than stacking on it. Petro's Iron
Skillet dinner beating a no-name diner's effect.

Pass when: one food buff exists at a time, the readout tracks it, and
your legal driving hours never grew — buffs touch fatigue, never the
clock.

### 3.2 The shower deal is real

Setup: a run past a Pilot or Flying J with fuel below full.

Do: buy fuel there, then check the shower price. Compare with a stop
where you bought nothing.

Pass when: the fuel purchase made the shower free, like real life, and
without fuel it costs money.

### 3.3 Rig care buffs, one per system

Setup: an owner-operator mid-trip near a Speedco or Love's.

Do: buy the lube-bay service, then a tire rotation, then a bottle of
diesel additive at any fuel stop.

Listen for: each purchase spoken with what it slows (engine wear for the
lube bay, tread for the rotation, a little of both for the additive) and
the status readout carrying one buff per rig system — a new lube
purchase replaces the old one instead of stacking.

Pass when: the delivery summary's wear lines come in lower than an
unbuffed run of the same leg, and no purchase ever added driving hours.

### 3.4 Road repairs are brand-true

Setup: a worn truck and a route with a spread of branded stops (the
interstate picks in the companion volume all qualify).

Do: at a Love's or Speedco, ask for tires. At a TA or Petro, ask for a
brake job. At some other major travel center, ask for tires. At Big
Buck's, ask for anything.

Listen for: Love's and Speedco replacing tires fast at close to the
garage price; TA and Petro running full service shops that also do
brakes on the road; other majors mounting tires at a spoken road markup;
an engine overhaul always deferred to your terminal garage; Big Buck's
fixing nothing, famously. Road shops selling the whole job or none of
it — no partial repairs when cash is short.

Pass when: every brand behaved by its real-world reputation and the
prices said so out loud.

## Chapter 4. Lanes, exits, and ramp lights

### 4.1 The lane is discrete and the taps are real

Setup: Settings, Driving assistance, Lane keeping set to off — that is
the hardest setting on this row, not the mildest, so read the value
clause before you commit. Any multi-lane interstate leg.

Do: press L to hear your lane. Tap Left and Right arrows to change
lanes; try one while on an exit ramp.

Listen for: signal clicks with each timed change, L describing the new
lane, and the ramp refusing: "You are on the exit ramp. No lanes to
change."

Pass when: every change is announced, lands in the adjacent lane, and
the lane readout always agrees with what you last did.

### 4.2 Change lanes or brake

Setup: a busy leg with Standard trip pacing.

Do: when a dodgeable hazard calls "Change lanes or brake!", dodge with a
lane change instead of braking — but press L first to know where the
traffic is.

Listen for: a clean dodge when the adjacent lane is clear; sideswipe
risk spoken when it is not; the CB nagging you to keep right if you camp
the hammer lane afterward.

Pass when: the lane change resolves the hazard and sideswiping
real traffic carries real consequences.

### 4.2a The warning leaves you time to answer it

New on 2026-08-11, from Munchkinbear's report and Darren's logs. The
truck used to take over almost the instant it warned you — often inside
half a second, sometimes before the sentence had finished — and on hot
brakes or a downgrade it fired on the same breath. Every recorded lane
change had automatic braking cut in mid-move.

Setup: automatic emergency braking on (it is on by default), Standard
pressure, highway speed. Worth repeating on a long downgrade and again
with brakes you have already cooked on a descent, because those are the
cases that used to be worst.

Do: when a hazard calls, count before you act. Then run it three ways —
answer immediately with a lane change; answer late, a good two or three
seconds in; and do nothing at all.

Listen for: enough silence after the warning to hear it out and act. If
you start a lane change, the change should finish and clear the hazard on
its own; "Automatic braking" should not land on top of you part-way
through the move. Doing nothing should end in the assist braking for you
and actually stopping the truck — that changed the same day, and it is
the second half of this checklist.

Worth its own run: the assist says "Automatic braking" and uses the
normal brakes, one steady application it holds to the end, and it only
adds "Emergency braking engaged" when the stop it is actually making has
stopped getting there in the time left. So take a hazard at the bottom of
a long descent, with brakes you have already cooked and rain or ice
underneath. That combination used to end in a collision after the truck
had told you it was braking. On good brakes on level ground you should
never hear the second line, and the air gauge should barely move.

Pass when: you can hear the whole warning and still beat it, on every
road and whatever state the brakes are in; and when you do nothing, the
truck stops short of the hazard rather than announcing the stop and
hitting it anyway.

Report if: the assist engages while you are mid-lane-change; the gap
between the warning and the truck acting is shorter on one kind of road
than another; or the truck brakes hard, says so, and still collides. The
first two were the original bug and the third is the one found alongside
it.

Turn automatic emergency braking off in Settings and the whole safety net
goes with it — ignoring a hazard then is a collision, as it should be.
That is the setting doing its job, not a regression.

### 4.3 Construction closes a real lane

Setup: any leg that announces a construction zone.

Do: obey the merge — flagger and taper first, then the barrels. On a
second pass, stay in the closing lane too long on purpose. Press L
before and after, so you know how many lanes you had.

Listen for: the staged approach in order (merge warning, flagger,
taper), then, if you ignored it, plowing through the barrels: truck
damage, a citation of 1,000 dollars or more, and a plain statement that
it goes on your safety record. Enforcement pressure hinted on the CB a
few miles out.

Pass when: complying is calm, ignoring it costs both damage and money,
and you were told the money was a citation and not a repair bill.

Two rules here are deliberate, so do not report them as bugs:

- **The barrels are charged once per construction zone**, however many
  times they catch you inside it. That is one refusal to merge. The
  damage still lands every time.
- **The barrel citation is not doubled for being in a construction
  zone**, unlike every other fine. Its amount is already the roadwork
  penalty. Other citations you collect inside the zone are doubled, and
  the game says so out loud when it doubles one.

### 4.3a A construction zone never closes your only lane

Setup: a leg that narrows to one lane your side — the road speaks the
change ("Down to one lane your side", "Road widens to three lanes your
side"), and L always answers with the lane count you actually have.

Do: drive construction zones on stretches where the lane count changes.

Listen for: no merge demand and no barrel citation anywhere a second
lane your side does not exist for the whole zone, taper included. You
should never be ordered out of a lane you cannot leave.

Pass when: every merge you were asked to make had somewhere to go.
Being pinned in a coned-off lane with no open lane beside you is worth
reporting immediately — note the two cities and roughly how far into the
leg it happened.

Also listen for: the side never changing its mind. The lane named in the
advance warning, in U, at the taper, inside the work zone, and by any
refusal when you ask to change lanes is one and the same lane. If you are
told the right lane is closed and anything afterwards treats the left one
as shut, report it. Where the road drops a lane under you, expect to hear
which lane closed and which one you are now in — never silence followed by
barrels.

### 4.4 Exits take a setup

Setup: any leg with announced exits; know your destination exit (R for
progress, Shift+R for what the next exit offers).

Do: when your exit is announced, press X to commit to it. Follow the
GPS: the right lane, then the exit lane where it opens. Once, miss the
exit on purpose.

Listen for: the GPS asking for the right lane only while you are out of
it, and for the exit lane once, where it opens; checking your speed at the
gore point, and explaining plainly what happens now that you missed —
and merge/exit traffic pressuring the maneuver while you do it.

Pass when: a set-up exit flows clean, a missed exit costs real miles and
is explained, and X while a pull-over is pending signals that instead.

### 4.5 The ramp light is an instrument

Setup: exits whose ramps end at a signal (thousands do — they are baked
from real intersection data).

Do: take a ramp to its stop bar on a red; wait for the cycle. On another
ramp, run the red deliberately.

Listen for: the dedicated red-light earcon and spoken callout at the
bar, the distinct green earcon on release, and cross-traffic punishing a
run red with more than words.

Pass when: red means stop, green frees you, and running it hurt.

### 4.6 The ramp's ending is announced before the ramp

Setup: any leg with announced exits.

Do: signal for an exit with X and listen to the whole announcement.
Press U on the mainline before the exit. Then take the ramp and watch
the clock: press C once on the open highway and again while braking
down a ramp that ends in a light or sign.

Listen for: the signal-on announcement naming the ending -- "The ramp
ends at a stop sign." -- with a mile-plus still to drive, U carrying
the same phrase, and time slowing to real seconds from the gore of a
controlled ramp: the half mile down to the sign takes as long as it
would take a real truck, not a compressed blink.

Pass when: you always know how the ramp ends before you are on it, and
you always have real time to brake for it. A free-flow ramp still
passes in compressed time.

### 4.7 Latching brake

Setup: Settings, Driving assistance, Latching brake on (the default).

Do: stop, shift to reverse, then hold the throttle to go forward.
Pump the pedal -- tap, then hold -- and do the same. On a long
downgrade, tap the brake, then press it again and hold for half a
second. Let go. Later, press it once to take it back. Latch it
again and tap the accelerator.

Listen for: getting out of reverse with a normal hold, including
after pumping, with no "Throttle latched." and no catch click on
the throttle. A brake catch click clearly different from the gear
click, then "Brake latched."; the drums holding with your hands
off; "Brake released." on the single press or instantly on the
accelerator. A latched brake on a grade heats and fades exactly
like a held one -- the latch never edits physics.

Pass when: the throttle key never latches, reverse lets go with a
normal hold, the brake gesture never fires from ordinary pumping
taps, and every brake latch and release is spoken.

## Chapter 5. Traffic with a clock

### 5.1 Rush hour is a place and a time

Setup: a metro leg (into or out of Denver, Portland, or any big-city
corridor). Two runs: one arriving in the commute window, one near
midnight — the C key tells you the local clock.

Do: drive the same stretch at both hours.

Listen for: the rush-hour run jamming — spoken congestion, slow traffic
injected in both lanes, your following distance doing real work — and
the midnight run flowing free on the same asphalt.

Pass when: the difference between the two runs is unmistakable by ear
alone, and rural stretches never fake a jam.

## Chapter 6. Streets, the truck dealer, and both ends of the trip

### 6.1 Turn-by-turn on real streets

Setup: a delivery to a facility in a city with street-level data (big
terminals usually qualify; the arrival announces itself street by
street).

Do: follow the spoken maneuvers off the ramp to the gate.

Listen for: each junction spoken with block-aware distance ("Turn right
onto..."), a direction-shaped earcon panned to the maneuver side —
falling chime left, rising chime right, steady tone ahead — and highway
pressure language gone quiet on the streets.

Pass when: you can drive gate-to-gate by ear, and near-straight name
changes say "Continue onto" instead of inventing a turn.

### 6.2 Departures drive out, too

Setup: a loaded run out of a facility that announced street-level
arrival (chain-capable both ways).

Do: accept the load and start driving from the gate.

Listen for: the same streets outbound with every turn direction
correctly flipped, then the on-ramp merge onto the highway with your
clock and tolls continuous — no teleport to the interstate.

Pass when: the outbound chain mirrors the inbound one and the trip
odometer never jumps.

### 6.3 The truck dealer is a direct menu item

Setup: any terminal city with a source-backed local dealer.

Do: open **Truck dealer** from the terminal menu.

Listen for: the intro naming the real local dealer where sourced data
exists, with no drive or menu teleport in between -- the dispatch board
and the garage are the same way, each its own direct terminal item.

Pass when: the dealer name matches the city it claims to be, and the
menu opens immediately.

## Chapter 7. Enforcement and the working day

### 7.1 The logbook is real and the law reads it

Setup: a career a few deliveries old.

Do: in town, open the city menu's Logbook item and arrow through the
entries. Then get pulled over on the road (speeding works) and listen.

Listen for: Record of Duty Status lines that match what you actually did
— drive blocks, on-duty dock time, rests — and the officer's stop
referencing your logbook rather than ignoring it.

Pass when: the book matches your day and the stop reads it.

### 7.2 Weigh stations and visible damage

Setup: a leg with an announced weigh station; separately, a truck
carrying severe damage.

Do: blow past the open scale once. Drive the damaged truck past patrol
presence.

Listen for: the blow-past drawing a roadside stop with consequences;
severe visible damage attracting a stop all by itself.

Pass when: both stop types trigger from their true causes and resolve
with spoken outcomes (fines, orders, or a clean release).

### 7.3 Running is a felony

Setup: courage, and a save you do not love. Get lit up, then do not
stop.

Listen for: escalating warnings, then the felony stop — spike strips,
the arrest, and a loaded run cancelled out from under you.

Pass when: the escalation is staged and spoken all the way down, and
the consequences land on your career, not just your ears.

### 7.3a A trooper who saw you does not forget

New on 2026-08-11, from JamminJerry's report of running whole routes over
the limit with nothing ever happening. The game deliberately gives you
one thing at a time, so an officer's look was held back whenever
something else already had your attention — a hazard, an exit, a stop in
progress — and then it was never given back. The look is now kept and
acted on the moment the cab goes quiet.

Setup: a long run, Standard pressure. There is nothing to set for
enforcement any more — how loud the policed country sounds now comes from
where you are and what time it is.

Do: hold nine or ten over for a long stretch, on purpose, and keep
driving through hazards rather than around them. The point is to be busy
exactly when a trooper is looking.

Listen for: the enforcement earcon before any post can see you — that
never changes, and it is the rule that an officer you were never told
about may not cost you anything. Then, after a hazard resolves, lights
and siren arriving a little late rather than not at all.

Pass when: a long over-the-limit run produces traffic stops at a rate
that feels like a road with police on it, and every one of them was
audible first.

Worth knowing before you report: under nine over accrues nothing at all,
by design. If you sit at eight over for a thousand miles and nothing ever
happens, that is the game working. Ten over is the first speed anything
can be built on.

### 7.4 Docks take time

Setup: any pickup and delivery.

Do: listen through the loading and unloading at both ends; check the
clock (C) and your time on duty this shift (Alt+A) before and after.

Listen for: spoken on-duty time passing at the dock, pull-ins taking
real minutes, and loaded launches ramping in heavy instead of leaping
off the line.

Pass when: dock time shows up in the logbook as on-duty and your
delivery windows price it in.

## Chapter 8. Three pressures, one truck

### 8.1 Relaxed is calmer, not smaller

Setup: the same leg, same load, driven twice: once in Relaxed, once in
Standard (Settings, driving mode).

Do: drive the leg in each mode with the same load.

Listen for: Relaxed keeping every system — weather, traffic, air brakes,
fatigue, hazards, consequences — but spacing hazards farther apart,
allowing more response time, building damage and fatigue more gently,
and speaking routine matters more quietly. Standard keeping the
quicker decision cadence. Safety warnings staying ahead of hours and
fatigue chatter in both.

Pass when: the difference is pacing and breathing room, never missing
systems — nothing in Relaxed feels like a feature was removed.

## Chapter 9. The career is a job

### 9.1 The first day lands

Setup: a brand-new career.

Do: walk the start: pick among the starter carriers (or the
owner-operator start), listen to the first-day briefing, then stall —
visit menus without accepting a load.

Listen for: each carrier pitched with its real trade-offs (wage,
dispatch personality, freight mix, assigned equipment); the briefing
repeating until your first dispatch is accepted; the Career plan
terminal item naming your next practical step in plain words.

Pass when: a brand-new player who only listens knows exactly what to do
next.

### 9.2 Dispatch freedom is earned

Setup: a new company-driver career.

Do: try to refuse work. Decline dispatched loads until the budget runs
dry; note what the board offers before level 8 and after.

Listen for: declines drawing down a spoken budget that refills on
promotion; declined loads staying declined; the full freight board
unlocking at level 8; route choice refused until you are an
owner-operator or run your own authority.

Pass when: the freedom ladder matches your level and the game says why
each rung is locked.

Numbers to expect, so you can tell a wrong count from a design you did
not like: a new hire gets three refusals per level band, four from level
five. Each refusal costs two reputation, and one on-time delivery wins
that back.

**Declining hard has a second, slower consequence, and it is intended.**
Dispatch trust slides down a four-rung ladder — full, guarded, poor, last
chance — and each rung down takes a refusal off your budget, holds back
freight, and slows how fast career experience arrives. Below guarded,
picking your own loads is revoked even past level 8. Dispatch trust also
answers to your CDL and to money you owe, not just to refusals, so a
clean driver in debt can see it fall too. Ask for it any time: it is on
the Career stats screen in town and on Tab, Driver while driving, and it
names which of the three is holding it down and what brings it back.

Report the slowdown only if the game never told you it was happening.
The rate is deliberately never spoken as a number.

### 9.3 The money is a carrier's money

Setup: a company-driver career and an owner-operator career,
side by side if you keep two profiles.

Do: fuel and repair on both. Deliver specialty cargo on time and watch
the streaks.

Listen for: the carrier covering a company driver's road fuel and
routine repairs; the owner-operator paying real operating costs from day
one; on-time streaks and specialty cargo compounding experience;
reputation paying a continuous dispatch trust bonus you can hear in the
offers.

Pass when: the same purchase sounds different in the two careers, and
personal money still buys endorsement courses and motel rest in both.

### 9.4 Trailers and the long arc

Setup: a career at or past the level-18 leased-on gate (an established
profile is fine).

Do: walk the leased-on trailer program, then dispatch rows with trailer
previews.

Listen for: dispatch rows previewing trailer fit and estimated take-home
before you accept; owned trailers arriving with own authority at 25;
guidance voices changing with the level band; haul-length caps growing
through the arc instead of maxing early.

Pass when: the trailer economics are spoken before commitment and the
arc still has somewhere to go at level 25.

### 9.5 Achievement spot checks

Do: earn any three obvious ones — a first delivery, a state arrival, a
close call — and check the badge wall.

Pass when: each lands once, speaks its line, and nothing awards twice.

### 9.6 Owing money, and the floor underneath it

Fines got much bigger in 1.9 and testers have already done the
arithmetic and got frightened. Here is the shape the game is actually
built to, so you can tell us when it does not hold.

**What is intended.** A fine you cannot pay does not simply take your
cash negative and leave you there. It becomes a **balance owed** that
you carry, and four rules govern it:

- **Only a quarter of any settlement goes to the balance.** Three
  quarters always reaches you. You can never finish a run with nothing
  because of what you owe, so working always helps.
- **There is a ceiling, and you are told what it is.** For a company
  driver it is what the carrier will carry — roughly eight of your own
  settlements, never less than 6,000 dollars. For an owner-operator it
  is what the tractor would fetch at sale, never less than 12,000.
- **You get three spoken warnings on the way there**: when a balance
  first exists, when you pass halfway, and a last warning that leaves
  real room — at least a couple of your own settlements — before
  anything happens.
- **No single citation can reach the ceiling on its own.** The worst
  fine in the game, a repeat offender doing thirty over inside a
  construction zone, tops out at 10,000 dollars against a 12,000 floor.
  One traffic stop is never meant to end a career.

**What happens at the ceiling.** A company driver's employment ends and
they move to another fleet on shorter freight and lesser equipment. An
owner-operator's lender takes the tractor back and they go on a payroll
again. Both settle the balance to zero, both keep level, experience,
endorsements, driving record and everything else owned, and both land as
a screen you can arrow through line by line and re-read before you leave
it. Neither is a game over and neither deletes a save.

If you are already at the fleet that hires anyone, there is nowhere
further to fall, so the carrier holds the balance at the ceiling and
writes off anything past it. It cannot grow.

Setup: a career you do not mind wrecking. An owner-operator start is the
fastest route — 18,000 dollars of working capital against a 12,000
repossession floor.

Do: collect citations on purpose. Blow an open scale, run fifteen or
more over, ignore a merge into the barrels. Spend past zero at the
garage. Between each one, check what you owe: Career stats in town, Tab
then Driver while driving, or Business status. Ask dispatch for an
advance once a balance exists. Keep going to the ceiling.

Listen for: every citation naming its amount and your remaining cash;
the doubling spoken as a reason whenever a fine is doubled for being in
a construction zone; the balance turning up in the delivery summary with
what the settlement paid toward it; the three warning rungs arriving in
order, each naming the ceiling and what happens there; a refused pay
advance explaining that a share of every settlement is already spoken
for; and the ending screen reading in order — the money, what it cost,
what you keep, where you go next.

Pass when: you were never surprised. Every rung was spoken before the
next one, the ceiling was a number you had heard, three quarters of each
settlement really did reach you, and the ending screen let you re-read
every line before you left it.

Worth reporting: a settlement that leaves you with less than three
quarters of the net; a balance still growing after the ceiling on a
last-chance carrier; an ending that arrives with no final warning before
it, or that arrives while you still had a couple of runs of room; any
line that calls you failed, bankrupt, fired, or says a save was lost;
and any fine amount you cannot get repeated after the fact.

## Chapter 10. Radio

### 10.1 The dial follows the map

Setup: a long leg crossing regions; radio on (M).

Do: tune with Page Down and Page Up (semicolon and apostrophe still
work), ask for status with Y, open the
Radio screen
from Tab. Drive a long way and keep checking what the dial offers.

Listen for: only receivable stations in the dial rotation, and the list
changing as you cross the country; hosts on the Roadhouse and Night Line
at their hours.

Pass when: what you can tune matches where you are, and the Radio screen
agrees with what Page Down actually gives you.

The thirteen Freight Fate regional stations — The Rawhide, Big Wheel
Country, Prairie Line and the rest — are receivable everywhere by
design, so they do not fade at a market edge. That is not a broken range
check. Real coverage, fringe static and the handover to the Roadhouse
belong to real stations, which play out of the box on a fresh install.

### 10.2 Streamer-safe on demand

Setup: a fresh install or reset settings.

Do: check that real public streams and personal playlists are on the
dial without touching anything — the full dial is the default. Then
turn on the one streamer-safe switch under Settings, Audio, and check
the dial again.

Pass when: streamer-safe on leaves only built-in safe stations, and
turning it back off restores the full dial.

### 10.3 A dial this big needs jumping, not walking

Setup: a fresh install — the whole dial is live by default: terrestrial,
AFN, satellite, international, web radio, and your own playlists if you
have any. Terrestrial lists the strongest signal first.

Do: hold Control and press a tuning key repeatedly to walk the
categories instead of the stations. Find a station you like and press O.
Press O again on the same station. Jump to the Favorites category and
check it is there. Keep tuning until something refuses to play.

Listen for: each Control jump naming the category it landed in before
naming the station; O confirming a station saved and then unsaved; a
station that will not play handing over to another one in the same
category rather than leaving you in silence, and then staying off the
dial for the rest of the session.

Pass when: you could cross a dial of thousands of stations in a handful
of presses, favorites survive a restart, and a dead stream never traps
you twice in one session.

A retired dead stream comes back next time you play. That is deliberate:
a stream that was down for an hour is not gone forever.

## Chapter 11. The world speaks

### 11.1 New country

Setup: dispatch into the new territory — the Great Basin on I-80, the
Hi-Line on US-2, I-70 over the Rockies, I-75 through the Kentucky
mountains.

Do: drive a leg that did not exist on the nightly line.

Listen for: real roads with real checkpoints, grades that match the
terrain out the window, truck stops (or a spoken rural-diesel fallback)
on every leg, and fuel planning that actually matters on the empty
stretches.

Pass when: the new corridor plays as fully as an old one — no silent
miles, no missing fuel plan.

### 11.1a The road holds its shape and its number

Two map fixes landed on 2026-08-11, both reported from this alpha. Sharp
switchback calls were turning up on ordinary US and state routes away
from the mountains, most often a mile or two out of a town; and the
posted limit would drop and come straight back with nothing on the road
to explain it.

Setup: a long run on US and state routes rather than interstate, and at
least one leg that leaves a town onto a two-lane. Standard pressure.

Do: drive the first few miles out of a town paying attention to the curve
calls, then settle in and just listen to the speed limit for half an hour.
Press the limit key whenever something sounds wrong, so you can tell a
real sign from a phantom.

Listen for: curve calls that match the road you are on — a hairpin only
where the country is steep enough for one. And a posted limit
that changes when the road changes and holds otherwise, rather than
dipping and recovering for no reason you can hear.

Pass when: no switchback is called on flat or gently rolling ground, and
a limit change is always something you can attribute — a town, a canyon,
a state line, a work zone.

Worth knowing before you report: real mountain country was left entirely
alone, so US-550 over Red Mountain Pass and the Salt River Canyon still
bend as hard as they ever did — those calls are correct. And a village
main street really is posted low for half a mile, so a short drop with a
town named around it is a real sign, not the bug.

### 11.2 The chatter switches work

Setup: Settings, the Roadside chatter group.

Do: drive a landmark-rich leg (national forests, named rivers, passes)
with chatter on; flip the master switch off; then re-enable just one
kind; then set verbosity to terse.

Listen for: ambient callouts for landmarks and parody billboards when
on; total silence from the whole group when the master switch is off;
only the chosen kind returning; terse verbosity muting it all
regardless.

Pass when: every switch does exactly what its label says, spoken
settings round-trip included.

### 11.3 Time is local

Setup: a leg crossing a zone boundary (Denver westbound works).

Do: check the clock before and after the crossing; take a load whose
deadline lies across a boundary.

Listen for: the zone change spoken at the line, C reporting the new
local time, and deadlines always read in the destination's local time.

Pass when: you can never be surprised by an hour you were not told
about.

### 11.4 The comma walks back through what you heard

Setup: any busy drive -- callouts, status keys, a warning or two.

Do: press comma once and you get the newest line, as always. Now press
it again quickly, and again: each press steps one line older, spoken as
"2 back:", "3 back:". Wait ten seconds and press once more. Then press
Space (a fresh status line) and comma again.

Listen for: the walk stepping through both voices' recent lines --
menu speech and driving events share one history -- the position
prefix on every older line, the pause snapping the key back to the
newest line, and any fresh announcement doing the same.

Pass when: a warning you missed two announcements ago is reachable in
three presses, and a comma pressed cold always answers "what did it
just say?".

## Chapter 12. Saves, restores, and the integrity gate

### 12.1 Old careers are turned away kindly

1.9 does not continue careers made before it. This is deliberate: 1.9
rebalances pay, trucks and levels, so an old high-level driver would
arrive on a scale that no longer exists. Your old save is not touched
and still works in 1.8. **Do not file the refusal as a bug** — what is
worth reporting is a refusal that is confusing, silent, or that loses
anything.

Setup: a career made in 1.8 or on the nightly line, in place before you
start the alpha.

Do: open the career list and try to continue it.

Listen for: the driver being named, a plain explanation that 1.9
rebalances the whole career so everyone starts fresh, a clear statement
that the old save is still on this computer and still works in 1.8, and
an offer to start a new career.

Pass when: you understood why without needing to ask, nothing suggested
the save was deleted or damaged, and you could get to a new career from
where the message left you.

### 12.2 Cloud restores still restore

Setup: a cloud backup made normally. If you have not set one up yet,
Chapter 0 has the account and the backup switch — connecting the account
turns cloud backup on, and it needs the staging account, not your real
one. Remember that a staging backup is disposable: it is not a safe
place for the only copy of a career.

Do: restore it.

Listen for: a clean, ordinary restore. The new integrity layer runs
behind the server's signature on every restore; a valid save should
restore without an integrity warning. If a restore is refused, the spoken
line names the first problem in plain words. Include that sentence in the bug report.

Pass when: valid saves restore without an integrity warning and any refusal
you can provoke says exactly why.

## Appendix A. Levers and keys

Weather forcing (`FREIGHT_FATE_FORCE_WEATHER`), the winter route picks,
and the physics key list live in the companion volume,
`docs/physics-playtest-checklists.md`. The README has the full key
table. The keys this book leans on beyond the physics ones:

Driving: X commit to the exit (or signal a pull-over), L lane readout,
I lane locator on and off, Left/Right steer or, with lane keeping on
full, tap to change lanes, B the emergency brake, K automatic speed
control, Shift+K resume it after braking cancelled it, Alt+T swap
automatic and manual shifting, Alt+J whether J runs the automatic engine
brake, T stop at an announced service, Enter accept a city-service
arrival.

Asking: R progress, Shift+R next exit, U upcoming, S posted limit, D the
one safe speed for here, G the grade and the next one, C clock,
deadline, and the nearest hours limit, Alt+A time at the wheel this
shift, Alt+S when the break is due, Alt+D what ends the shift and where
you can stop before it, F fuel, A repeat last announcement, F1 the full
key help.

Radio: M on and off, Page Down and Page Up to tune (semicolon and
apostrophe still work), Control with a tuning key to jump a category, O
to save or unsave a favorite, Y for status.

Messages, on every screen: comma repeats the last spoken line and
presses again walk back through the last 200, period moves forward,
Control with comma or period jumps to the oldest or newest, the bracket
keys switch between all messages, general messages and driving events,
and Control C copies the one you are on.

### Scenario levers

Three more environment variables move a career into position for a
scenario without hours of setup driving. All three speak what they did
in plain words, move no miles and no money, and only touch a career
that is parked with no load in progress.

**A lever session is a sandbox by default.** The whole run — the
relocation, any loads you take, money won or lost, damage, abandoned
jobs — plays out in memory and is never saved. You hear "Playtest
sandbox: nothing this session is saved" as the career loads, and your
real save resumes untouched the next time you play normally. For the
rare run whose changes should stick (moving your career home, say), set
`FREIGHT_FATE_FORCE_PERSIST = "1"` alongside the levers and the session
saves like normal play.

Set the levers before launching the game, load your career from the
main menu, and unset them when the scenario is done. In PowerShell:

    $env:FREIGHT_FATE_FORCE_CITY = "denver_co_us"
    $env:FREIGHT_FATE_FORCE_CLOCK = "21"
    $env:FREIGHT_FATE_FORCE_DEST = "silverthorne_co_us"

- `FREIGHT_FATE_FORCE_CITY` — relocate the parked career to a city
  (slug or plain name) as the career loads. You hear "Playtest lever:
  relocated to..." after the terminal announcement.
- `FREIGHT_FATE_FORCE_CLOCK` — roll the career clock forward to the
  next time the local wall clock reads that hour (0 to 23). The wait is
  logged as off duty; ten or more hours counts as a full break and
  resets hours of service, exactly like sleeping at the terminal.
- `FREIGHT_FATE_FORCE_DEST` — the next freshly built dispatch board is
  guaranteed to offer a load to that city when a supported corridor
  reaches it, and assigned dispatch hands you that load first. If the
  board was cached before you set the lever, sleep once or deliver a
  load to refresh it.

Unset with `Remove-Item Env:FREIGHT_FATE_FORCE_CITY` (and the same for
the other two) before normal play. When shared profiles arrive, the
event ledger must record forced relocations and clock moves so a
shared save records those changes (see docs/profile-invariants.md).

## Appendix B. Reporting a run

One line of setup, what you heard, what you expected. Say which chapter
and checklist, your driving mode, and whether weather was forced. If a
spoken line was wrong, quote it as closely as you can — the exact
wording is often the whole bug.
