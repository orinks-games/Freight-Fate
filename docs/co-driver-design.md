# The co-driver

Decided by the owner on 2026-10-17: the co-driver goes ahead. There is no attract mode and no demo item on the main menu. Co-driver miles earn the player less experience, but not none.

## What it is

You hand the wheel over from the pause menu (or from the shipper's dock, for the whole load), and the co-driver runs the truck while you play something else. It works the way real team driving works: while they drive, you are in the sleeper, so your own hours of service recover. Driving it yourself stays the way to earn the full rate and full experience.

Pausing never hands the truck over on its own. A paused truck that drives itself while you are at dinner breaks the one thing pause promises, and the career would pay for it.

## What the game already does on its own

On the All assists preset the truck holds the lane, steers the bends and street corners, and takes the exits. Cruise with the speed keeper and predictive cruise holds a speed through zones and over grades once it is armed. Facility stopping assistance brakes to the gate. Automatic braking stops for hazards. The drive also keeps running when the game window loses focus, so alt-tabbing to a flight sim already works for a stretch of interstate.

What the co-driver has to take over, because it is still the player's job on All assists:

- Starting from the gate and arming cruise.
- Open weigh stations: signal, stop on the scale, press T.
- Fuel, the 30-minute break and the 10-hour sleep. At a stop the co-driver fuels and takes their break without opening the stop's menu: the pump sound, one line in the message log (spoken too when you ride along), and back to the road. Only the end of their shift opens the menu, for you.
- Pulling over for a trooper, and roadside inspections.
- Confirming arrival and choosing Dock and deliver.

The playtest harness has a driving loop, but it cannot be reused for players: it refills the tank every frame and flattens every hill. The agent server shows the better model, because it plays through the normal controls and the real truck.

## How it plays

- **Handing over.** "Hand the wheel to Dan and ride along" or "... and sleep" in the pause menu, on a delivery whose rest would need a ten-hour break. At the shipper, "Depart with Dan driving, and ride along" or "... and sleep", with the pairing and the terms in the rows' help. Some long loads are booked as team loads: about a third more pay, and a deadline with time to change drivers but none for a ten-hour break.
- **Taking it back.** "Take the wheel back" in the pause menu. The co-driver also hands back on their own, with one sound and a line saying why: their hours are up (parked at a stop), the truck is at the receiver's gate, or something needs a decision only the driver can make. Scales, fuel, troopers and hazards are theirs.
- **While you are away.** The game goes quiet: no road speech, no limit lines, only the summon. The radio keeps playing if it was on. The status screens still answer if you come back and press Tab.
- **What it costs.** The co-driver takes a share of the settlement for the miles they drove. Those miles earn you reduced experience. On a load the co-driver drove any of, the badges for how a haul was driven are not earned, and the road journal entry names who drove; lifetime mileage still counts the truck's miles. Fuel, tolls and wear cost the same as when you drive.
- **What it does not cost.** The co-driver drives clean: no tickets, no preventable damage. They drive cautiously, so a tight deadline is better driven yourself.
- **Your hours.** Co-driver miles are sleeper-berth time for you, which counts toward the 10-hour break. You come back to the wheel rested.
- **Who drives when.** Real teams decide this between themselves; the law only limits each driver's own hours. So the player decides, and the co-driver keeps a logbook of their own. The co-driver hands back at the next stop with parking once the player's ten-hour rest is in (recommended and built 2026-10-19, after the owner asked whether they should stop short of their full hours). A fresh co-driver's 11 hours of driving outlast that rest, so they normally hand back with an hour or less of their own unused: the player pays the co-driver's share for no more miles than the rest needed, and sleeping through it lasts as long as a sleep. A player who has not driven since their last full rest gets the co-driver's whole shift. The co-driver plans their 30-minute break from five and a half hours of driving, stopping at the next rest area, so a full lot still leaves time to reach another. When their hours run out, or the break is due with no stop reached, they summon you. Taking the wheel back before your rest is in means you have not had your full rest.

## Riding along or sleeping through

Without a co-driver, sleeping parks the truck, shuts the engine down and moves the clock forward. Nothing drives during it. With a co-driver, the handover offers two choices:

- **Ride along.** The drive plays out at the normal pace with the co-driver at the wheel. You can listen, play something else, or take the wheel back with one key.
- **Sleep through it.** The co-driver's shift passes as time, the way a sports game simulates a match (owner, 2026-10-18). The road is worked out at once: the posted limits, the highway fuel burn, the tolls and both logbooks. Fuel and the 30-minute break cost their minutes and money where they fall, the radio is left alone, and the shift ends with the truck parked at a stop and its menu open. Only the merge onto the highway and the receiver's exit and gate are still driven, sped up with the sound down. You wake up where the truck got to, with a short summary: miles covered, where they fueled, anything that went wrong. If something needs the driver, it stops early and wakes you.

The choice changes how long you wait, not pay, experience or rest. Riding along is logged as sleeper-berth time and recovers hours and fatigue the same as sleeping through it: in the story you are in the bunk and hear the road, not sitting in the passenger seat. (Real rules count passenger-seat time as on duty, with a small allowance for team drivers, so a seat option would recover almost nothing.) Sleeping through it skips what the open road would have thrown at the co-driver between stops: weigh stations, inspections, hazards and traffic.

## Where the co-driver's information lives

No new hotkeys. Everything goes where players already look:

- **Dispatch board and job details.** A team load says so in its details. The pay terms are in the help of the handover rows.
- **Tab, Driver screen.** While teamed: who is driving, how much of the player's ten-hour rest is in, and when the co-driver hands back. After a slept-through shift, the wake-up summary stays here so it can be read again.
- **C readout.** The "nearest hours-of-service limit" clause becomes the rest and hand-back line while they drive ("Rest: 3.5 hours of 10 hours. Dan hands back in about 6.5 hours."). The pause menu's Take the wheel back row reads the same line in its help.
- **The first offer.** The first time a career is offered a co-driver, at the dock or in the pause menu, one line says what it is, where to choose it and when the co-driver hands back. Job details on the board say "Co-driver: offered at the shipper's dock or from the pause menu, on your current hours." when the dock would offer one on the clock the loaded truck leaves with.
- **ELD app.** The co-driver's logbook next to yours, the way a real team's electronic log shows both drivers.
- **Settlement.** One line for the co-driver's miles and their share, and the reduced experience in the experience line.
- **Manual and ontology.** A "Team driving" section in the manual, and rows for co-driver, team load, ride along and sleep through it.

Owner-operators keep the same hired co-driver from load to load, named in the hire. Company co-drivers are dispatch's pairing for one load.

## Build order

1. **The automatic driver.** Drives through the real controls and the real truck: fuel burns, hills climb, scales and breaks happen. Proven headless by delivering whole loads on the map sweeps, and by the adversarial battery with the co-driver at the wheel.
2. **The co-driver in careers.** The settlement share, reduced experience, hours of service while away, the summon, quiet speech, and a "co-driver" row in the ontology.
3. **Later: hired drivers for owner-operators.** Drivers who run loads off-screen for income, the way Euro Truck Simulator 2's hired drivers work, reusing the same automatic driver.

The time compression redesign (PR #451) fits alongside this. While the co-driver drives and nobody is listening, there is no speech for the road to outrun.

## Terms (decided by the owner, 2026-10-17)

These follow general patterns in real trucking. The patterns come from general knowledge and were not checked against sources for this write-up. Many large carriers put new drivers on teams, and a carrier's road training is itself team driving with a trainer. Company teams are usually paid a per-mile team rate that the two drivers split. An owner-operator who hires a co-driver pays them per mile or a percentage of the load, and still pays fuel and upkeep.

- **Available on any load too long for one shift.** Carriers team long, time-sensitive freight, not short runs, so the co-driver is offered when the rest of the trip would need you to stop for a 10-hour break. The route planner already works that out. There is no level unlock. Because of the distance caps (300 miles at level 1, 450 at level 2, 650 at level 3), it first comes up around level 3, the way a new hire earns long freight.
- **Company driver: co-driver miles pay you half.** The settlement for miles the co-driver drove is split evenly, the way a team splits its per-mile rate.
- **Owner-operator: you pay the co-driver 30 percent of the settlement for their miles.** You keep the rest, and fuel, tolls and upkeep are still yours. From the owner-operator buy-in at level 18, the co-driver is your hire instead of dispatch's pairing.
- **Experience: half for co-driver miles.**

## The cast: background and callbacks

A first draft for the owner to cut, rename and rewrite. Every callback below points at something already in the game. The song references use only each song's title and catalog description. I have not heard the lyrics, so any bio that touches a song needs checking against what the song actually says.

### How they came to be with you

**Everyone came through Great Lakes Training Transport.** It is the training fleet every career can start at, and the one a driver is "hired away" from at level 4. So the co-driver pool is your training class. Some stayed at Great Lakes and some moved on to Northstar, Prairie Link or Summit, the same moves the player can make. When dispatch pairs you, it is with someone from that class who works at your carrier now.

That gives three story beats that already line up with the career:

- **Company driver:** dispatch pairs you with a classmate for one load. You get to know the cast one load at a time.
- **Moving carriers:** a classmate who already works at the new carrier can be your first pairing there.
- **Owner-operator buy-in (level 18):** the co-driver you have teamed with most offers to come with you. That is your hire, and they stay from load to load.

### How the personality comes through

The co-driver is not chatty; the silence rule still holds. The personality shows up in four places only:

- **Their radio habits.** See "The radio while riding along" below.
- **The summon line**, when they need you back.
- **The wake-up summary**, which can carry one line in their voice.
- **Their bio**, one or two sentences on the Driver screen and in the pairing details.

### The radio while riding along

Decided by the owner on 2026-10-17:

- **Radio off stays off.** If the player's radio is off when they hand over, the co-driver never turns it on.
- **Radio on is the co-driver's to play with.** They may change station, flip through the dial, or turn it off.
- **The player always wins.** If the player changes the station or turns the radio off, the co-driver leaves it alone.

Each cast member has listening habits, not one fixed station: stations they lean toward, how often they flip through the dial, and how likely they are to want quiet for a while. A station fading out of range is a natural moment to flip. A player still learns who is driving by ear, from the kind of thing they land on and how restless they are with the dial.

### The cast (pitches)

**Dan "Dangerous Dan".** The most careful driver in the class, which is why the name stuck. He asked the Roadhouse for "Dangerous Dan" so often the other drivers named him after it. Leans toward the Freight Fate Roadhouse and rarely touches the dial. A good fit for the co-driver's cautious driving.

**Rusty Oxley.** Grandson of Grandpa Spuds Oxley, the voice of The Delta 94.3. He learned to drive listening to his grandfather on the air, which is a callback to "Grandpa's Radio". Leans toward The Delta, and blues wherever he can find it.

**June, from Della's.** Grew up waiting tables at Della's Blue Plate Diner, watching drivers come and go, and decided to be one. She knows every diner on the map. Leans toward country, Big Wheel 104.5 first, and hums along.

**Marisol.** Drove water tenders on a wildfire crew before trucking, a callback to "Wildfire Line". Calm on mountain grades. Likes rock, but turns the radio off for a long climb.

**Walt.** Will not leave a terminal until four forecasts agree, a callback to "Four Sources and the Truth". He is the one who reads you the weather in the wake-up summary. Leans toward Prairie Line 95.7, and drives long stretches in silence.

**Dee.** Spent one summer of the nineties online and never came back, a callback to "Dial-up Summer". Runs every driver app there is, LoadLasso and WeighAhead included. The most restless on the dial: scans constantly and never settles on one station for long.

**Nate.** Was a bicycle courier before trucks, and will tell you "The Lone Courier" on Channel 3000 is about him. It is not. Flips to Channel 3000 whenever it is in range.

### Surprise pairings at milestones

Dispatch pulls a specific co-driver once, unannounced, at a moment the career already marks. Rules: never hinted beforehand, never better or worse at the job than the regular cast, and each one counts as a hidden achievement so the player can find it again. Two ship: "The Classmate Came Back" for the Meridian classmate, and "Teacher in the Sleeper" for Ray.

- **The first team load:** Dan, so everyone meets the same face first.
- **Moving to another carrier:** a classmate who already works there.
- **A rare draw on any team load:** the classmate who "went to Meridian", back with a story.
- **Level 20:** Ray, the old trainer, for one load. He is never in the regular pool.
- **Owner-operator buy-in:** the co-driver offering to come along is already planned; it can land as the surprise of that level-up.

### Smaller easter eggs

- The co-driver feature's menu music: "Always Around When You Need Me" is already a "steady companionable country bed".
- A wake-up summary that mentions a stop at a Red Hawk travel center, a Black Kettle coffee or a bag of Smokestack Jerky, all businesses already advertised on the radio.
- Meridian Freight Lines advertises on the radio but is not a carrier you can join. A classmate who "went to Meridian" and is never heard from again is a running joke the cast can share.

### Decisions for the owner

- Which characters stay, and their names.
- Ray, the trainer, stays out of the regular pool (owner unsure, 2026-10-17). He turns up once, at level 20.
