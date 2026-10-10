# Changelog

## Unreleased

### Added

- **Interstates now have the sound of distant traffic, louder on busier stretches.**

- **On busy freeways, traffic now comes up beside you and passes.**

- **The radio status screen says where your own playlist files go until you have one on the dial.**

- **A fuel range warning speaks when your fuel will not reach the next fuel stop.** It names your range and how far the stop is.

- **The Radio app can suggest a station that is missing from the dial.** Accepted stations join every driver's dial, and AM and FM ones play near home.

- **Text fields now paste with Control V, and name punctuation as you type it.** A colon or slash is spoken as a word instead of a silent click.

- **Drivers can call dispatch while stopped for trip-specific help.** Report delays, hours, road conditions, truck trouble or load trouble.

### Changed

- **The player manual and How to play now explain menus, driving, rest, radio, and career choices more clearly.**

- **How to play announces controller controls, including how to stop speech.**

- **Update checks, the bug report page and the manual download link now go to the game's new GitHub home, orinks-games.**

- **A slow vehicle ahead in your lane is now called at half a mile, when you start to hear it.**

- **Tire noise on the road is louder at highway speed.**

- **The fuel range now comes from your truck's own mileage on this drive, load and hills included.**

- **The Engine voice and Engine brake voice settings are now called Engine sound and Engine brake sound.**

- **The distant interstate traffic is now a steady roar.**

- **A slow vehicle ahead in your lane can now be heard from half a mile away.**

- **A slower vehicle ahead now plays the slowing-traffic sound, not a passing sound.**

### Fixed

- **Resume speed control now brings back the speed keeper's speed after you brake out of it.**

- **Channel 3000's shows are now as loud as the other stations, and their theme music no longer jumps out over the talk.**
- **Been Everywhere, For Real now needs all sixteen regions on the map, not fourteen.**

- **Grossed Out at the Scale House can now be earned.** It needs a load within a ton of the legal limit; dispatch never offered the old 24 tons.

- **The Route 66 and redwood badges count the newer towns on the map,** like Joplin, Gallup and Kingman on the Mother Road, and Eureka in the redwoods.

- **Menus accept Enter, Space, and letters from braille notetakers and automation tools.** Like the BrailleNote Evolve, they could arrow but not select.

- **Traffic is now loud enough to hear over the engine, from about a quarter mile away.**

- **Passing traffic now sounds where the vehicle really is.** It comes up on its own side, then fades as it pulls ahead.

- **Traffic sounds on exit ramps now match the cars.** The freeway fades off to your left, and cross traffic at the ramp end sweeps across in front of you.

- **Bobtail to a nearby city is refused while your CDL is suspended or disqualified.** You hear when the suspension ends instead.

- **Training classes now leave you tired, about half as fast as driving does.** If a class leaves you drowsy, you are told to sleep before you drive.

- **The last weigh station warning names its scale and leaves time to take the exit in every driving mode.** Closed scales say so.

- **Weigh stations and truck stops around West Memphis are now where they really are.** The scale that was never there is gone, and the Marion and Lehi scales are added.

- **The clock no longer sticks at real time after a slow merge.** Changing Driving mode while moving now waits until you stop, and says so.

- **A pickup dispatched from another city now calls each street turn by its real direction.** It no longer says keep right at every street.

- **The weigh station bypass charge now applies only when you really skip an open scale.** A hazard near the scale no longer costs you; slowing, then speeding past, still does.

- **A closed weigh station says it is closed, once, and nothing more.** It no longer chimes, saves, or counts as an inspection.

- **The state line on the St. Louis to Memphis drive now says I-40, over the Hernando de Soto Bridge.**

- **The update check no longer offers a preview snapshot that is the same as your copy.**

- **What's new, when updating from an older preview snapshot, now lists every change since your copy.**

## 1.9.3 - 2026-10-06

### Added

- **LWorks Radio is on the dial.** Upbeat hits from the nineties and two thousands, with an eighties lunch hour, wherever you drive.

### Fixed

- **The load you sleep for after an hours warning stays on the dispatch board.** It used to vanish when the rest ran past midnight.

- **On a three-lane road, a hazard call names the lane you can move into.** From the right lane it now says middle lane open, matching the lane change.

- **Roadside billboards end with a single period.** Some used to end with two, which a screen reader could read as dot dot.

## 1.9.2 - 2026-10-05

### Added

- **The truck warns you once when fuel drops to about 15 percent.** It speaks again only after you refill above that line and the tank drops again.

### Fixed

- **The Roadhouse no longer keeps restarting around dusk and dawn outside Eastern time.** It now changes between its day and night music by the local clock.

- **Rush-hour traffic follows the local clock.** West of Eastern time, jams and the words rush hour now come at the local rush, not hours early.

- **Mac launch no longer freezes waiting on the keychain.** After a newly signed build, macOS can ask before handing over a saved online token; the game now waits a few seconds, then continues and says so, instead of sitting unresponsive with a log that stops after "world".

## 1.9.1 - 2026-10-04

### Fixed

- **The update check no longer offers the version you already have, and the welcome no longer says development build.**

## 1.9.0 - 2026-10-04

### Compatibility

- **Careers from 1.8 and earlier do not carry over; every driver starts a new career.** Old saves stay listed, untouched and playable in 1.8.

- **Your settings come across from 1.8.**

- **Freight Fate runs on Windows, Apple Silicon Macs and Linux.** Linux comes for x64 or ARM64, as an archive or an AppImage.

- **On Linux, speech uses the Speech Dispatcher installed on your computer.** Without it the game still starts, but stays silent.

### Highlights

- **Freight Fate is rebuilt from the ground up, and starts noticeably faster.**

- **The biggest map yet: 100 new cities, real speed limits coast to coast, and real streets into hundreds of facilities.**

- **Highways drive like highways.** Real lanes, exit ramps at their real lengths, deceleration lanes, ramp-end lights and cross traffic at the bar.

- **The GPS reads the road.** Spoken curve calls, steep-grade warnings and speed-limit drops, with bends that can move your load or roll the truck.

- **Choose how much the truck does for you.** The game asks once: All assists, Balanced or Realistic.

- **The road has rules and someone enforcing them.** Troopers, roadside inspections, real weigh stations, and a driving record your carrier and insurer read.

- **The radio is a real dial.** Thousands of real stations, dozens of new original songs, your own playlists, and Synthesized music the game makes itself.

- **The truck sounds like a real truck.** A three-stage engine brake, real brakes and gear changes, and traffic that sounds like what it is.

- **Weather and seasons matter.** Snow chains and chain laws on the western grades, winter tires and hydroplaning.

- **Truck stops sell more than fuel.** Meals, showers, repairs, loyalty points, live parking counts and CAT scales.

### New features

- **Each truck keeps its own condition.** Worn brakes, tires or engine can put it out of service, so walk around it before you pull out.

- **Your freight can be damaged, and the receiver can refuse it.** Tank loads slosh, and hard bends and stops show it.

- **Drop and hook, detention pay, and receivers with a drop yard.**

- **Hours of service has its own keys.** T plans your next sleep stop, and opt-in hints name a comfortable break.

- **Cruise reads the road ahead.** It drives the hill before it arrives, has a resume button, and adaptive cruise slows for bad weather.

- **You can choose how much room the truck leaves to the vehicle ahead.**

- **Latch the brake and give your hands a rest.** Turn it off under Settings, Driving assistance, Latching brake.

- **Hold slash to straighten up.** With lane keeping on partial or off, the truck points down the road.

- **Alt T switches between automatic and manual shifting on the road.**

- **You can change which key or controller button each driving control uses.** F2 lists every driving command by name.

- **Learn game sounds plays what every sound means before you meet it at speed.**

- **Comma and period review your recent spoken messages while you drive.** Comma re-reads the last line anywhere in the game.

- **You can play from a braille display with speech off.** Driving readouts are short enough to fit one.

- **Driving keys work with JAWS without the pass-through key.** Settings, Speech has a JAWS arrow keys row when JAWS is running.

- **Live road reports, real construction zones and live truck parking cover most of the map.**

- **The pumps charge this week's real diesel price.**

- **Real time joins the Driving mode row.** The game clock follows your computer's.

- **Licenses and training replaces the endorsement menu.** Train out of the automatic-only restriction, and add steel and lumber to flatbed securement.

- **You can stay a company driver, and owner-operators can go back to company driving.** Owner-operators start with a brand-new truck.

- **Dispatch picks the truck to fit the load, and relays one from a nearby town when your board is thin.**

- **Online, you choose which career is public.** A driver directory sits next to Drivers on duty, and profiles say what each achievement was for.

- **Dozens of new achievements, browsed by category.** Careers that reach level 21 get a menu theme of their own.

- **The driver tablet has a Radio app.** Search the dial, tune by name, and save favorites with O.

- **Music source can be set to Synthesized.** A music seed changes every piece, and the original 1.5 soundtrack is back.

- **About three thousand new billboards, state welcome signs, and real roadside attractions where they really stand.**

### Fixes

- **A stalled or damaged update download is never installed.** Escape leaves the download at once.

- **Closing the game hands your screen reader back right away.** A stuck screen reader can no longer freeze the game.

- **Speech comes back on its own when a voice locks up mid-drive.**

- **Safety calls, hazard instructions, turn calls and exit calls are never lost or talked over.**

- **Automatic braking stops the truck in time, even on hot or worn brakes.**

- **The assists no longer run your air tanks dry by pumping the brakes.**

- **Hills and bends match the real road, and interstate curve warnings are rare.**

- **Exits, state lines and rivers are announced where the truck really is.**

- **Every stop the game announces is one your truck can enter.** Convenience stores no longer pose as truck stops.

- **Town speed limits no longer follow you out of town, and a lower limit is always announced in time.**

- **Checking in at a weigh station no longer gets you fined for bypassing it.**

- **A dead radio stream lands on a live station instead of silence.**

- **A damaged settings file or sound file no longer takes the game down.**

### Changes

- **New installs start on All assists.** Switch to Balanced or Realistic under Driving assistance.

- **Spoken lines are shorter everywhere.** Quiet and Urgent only leave out traffic chatter, and a key you press always answers.

- **The game stops repeating key prompts you have mastered, and Escape acts instead of explaining.**

- **The radio dial moved to Page Down and Page Up.**

- **Four keys answer one question each about where you are.** R now answers just "where am I".

- **Fines match what they cost a real trucker, and your driving record costs you with the carrier and the insurer.**

- **Company drivers no longer see the truck dealer, upgrades or tire choice.** The carrier handles the truck.

- **Pausing no longer takes you off duty.**

- **Every music track and radio segment plays at the same loudness.**

- **The Mac app is signed and notarized by Apple, so it opens without the Open Anyway step.**

- **The driving school steps out of this release to finish training.**

- **Lane centering assistance is gone from Driving assistance.** Lane keeping on full already holds the lane.

## 1.9.0 complete change list

Every change since 1.8.8.1, as each tester snapshot listed it.

### Added

- **Before a stable release, the billboards grow: about three thousand new ones, from Ocean City's boardwalk to Big Bend.** Big Jim notices your driving.

- **You can pull in at The Thing, on Interstate 10 in Arizona.** Only a tractor without a trailer can fuel there.

- **The game asks how much the truck should do for you before the main menu, once.** Choose All assists, Balanced or Realistic; Escape keeps what you have.

- **F2 lists every driving command by name.** Pick one and it runs as its key would, then you are back on the road.

- **Settings, Speech has a JAWS arrow keys row when JAWS is running.** Faster makes menus and held arrows answer at once; Default removes the script.

- **Start the game with `--key-probe` to see what your screen reader sends it.** It reports how held arrow keys arrive, for support.

- **Space and U say how far away your signalled exit is.** Space ends with it, and U names it first.

- **Truck stops with a CAT Scale let you weigh.** The ticket reads steer, drive and trailer axles and gross, and says which are over.

- **Opt-in hours of service planning hints guide T to a comfortable break or sleep stop.** Speech and Alt+D name the last legally reachable fallback.
- **Hold slash to straighten up.** With lane keeping on partial or off, the truck turns to point down the road and leaves your lane position to you. (Suggested by flight)

- **Bananavision channels are on the dial.** Nine HTTPS Icecast mounts from bananavision.tv — Main, Bananalodeon 80s, Bananalodeon, Bananarang, Educational, Brain, Star Trek Vision, MST3K, and Over the Edge — and they come in anywhere you drive.

- **The game lists every screen reader it finds on your computer.** Start it with `--list-speech-backends` and it names the one it would speak through.

- **Country, classic rock, blues, and Night Line stations play new songs.**

- **The radio has additional songs, station IDs and ads.**

- **Two new achievements, both hidden.** One is earned at the wheel and one only on a particular day of the year.

- **A new hidden achievement belongs to one week of the real year.**

- **Truckers Radio USA sounds better.** It now plays the station's full-quality stream under Web radio.

- **The Terrestrial dial has 156 more music stations.** Country, rock, hits, hip-hop and Spanish stations across 35 states.

- **Dodge Radio is on the dial.** A United Kingdom station playing a bit of everything, and it comes in anywhere you drive.

- **Six AFN 360 channels are back on the dial for players outside the United States.** AFN blocks them inside the US, so they won't play there.

- **X Transmission FM is on the dial.** Rock, metal and country from live DJs, and it names the song playing.

- **Music source can be set to Synthesized, music the game makes with no AI.** Menus and the Roadhouse play it, with no voiced breaks.

- **A music seed changes every synthesized piece.** Settings, Audio, Music seed rolls a new one and says it, so you can share one you like.

- **You can type in a music seed.** Enter on Music seed opens a field for a seed someone shared; Left or Right rolls a new one.

- **Synthesized music plays more instruments.** Each style adds some of a strummed guitar, an organ, a bell and a harmonica-like reed.

- **The original 1.5 soundtrack is back in Synthesized mode.** Headlights West, Open Road and Night Haul return.

- **Synthesized mode takes Freight Fate's own stations off the radio.** The dial keeps the synthesized Roadhouse, local and web stations, and your playlists.

- **In Synthesized mode, streamer-safe keeps the radio on the Roadhouse.** Station keys do nothing; the radio key turns it on and off.

- **Radio playlists can play tracker modules.** Music made in OpenMPT and similar trackers plays like any other file in your playlist.

- **Real construction reports now cover California.** With real traffic on, lane closures Caltrans has in place right now appear on your route.

- **Past a facility's driveway you are in the yard.** "Into the yard. Yard limit 15." The gate where you check in is at its end.

- **Taking a bend or ramp curve too fast can roll the truck over.** A full trailer or part-filled tank goes first, and the load is lost.

### Security

- **A career balance edited outside the game stays marked.** Money changed by a memory tool saves with the modified flag.

### Changed

- **Every music track and radio segment now plays at the same loudness.** The music and radio volume settings no longer need adjusting from one song to the next.

- **Light rain, snow, night rest stops and the traffic slowing cue are louder.** They were far quieter than the sounds around them.

- **The Mac app is signed and notarized by Apple, so it opens without the Open Anyway step.**

- **Quiet and Urgent only driving speech no longer call out traffic.** Slow cars ahead, cruise settling in behind them and lane keeping passing are spoken only on Standard.

- **Anything Quiet or Urgent only leaves out now makes no sound.** The stand-in notes and the notification tones of silenced lines are gone.

- **Quiet driving speech says cruise, speed keeper, work zone and collision updates in a few words.** For example, Cruise easing to 50.

- **Urgent only no longer announces achievements while you drive.** They wait in message review.

- **Urgent only no longer speaks CB reports, toll heads-ups or traffic packs.** Lane closures and the directions you must act on still speak.

- **Radio stations whose name already includes the call sign say it once.** Tuning now says "KXLU 88.9", not "KXLU, KXLU 88.9".

- **CB reports now open with a short squelch and go straight to the news.** "CB chatter" is no longer said before each one.

- **Lane keeping on full now passes slow vehicles.** You hear "Passing on the left", and the truck moves back right once past.

- **Letting go of the steering keys now straightens the truck.** It stops drifting where you leave it, so there is no wheel to unwind after a turn.

- **Holding the arrow toward a bend or street corner now takes the turn.** The truck follows the road until you let go, instead of steering past it.

- **Holding a steering key now moves the truck across at a steady pace.** A lane change takes about two and a half seconds however long you hold, and a tap is a nudge.

- **New installs start on All assists.** The truck steers, street corners included, until you switch to Balanced under Driving assistance.

- **The Lane keeping help describes the lean your Steering guide and Lane guide sound settings give you.**

- **The turn signal ticks and tocks like a real flasher, and clicks off when the move ends.**

- **Downshifting a manual with the clutch held matches revs.** The engine follows the truck's speed down instead of dropping to idle, so letting the clutch out is smooth.

- **A manual no longer stalls while you brake to a stop in a tall gear.** Coming off the brake at a standstill in that gear still stalls it.

- **Choosing Back to the road at an open weigh station checks you in first.**

- **Dispatch trust now shapes the board, refusals and load choice whatever lowers it.** A driving record or debt that holds trust down now costs freight too.

- **Company drivers no longer see the truck dealer, upgrades, trucks, trailer programs or tire choice.** The owner-operator buy-in is still under Business status.

- **The loyalty desk offers only the shower, at stops that sell one.** Parking, food and laundry rewards did nothing, so they are gone for now.

- **The exit lane opens just before the gore, and you steer into it there.** Already in the right lane, you hear no lane instructions before that.

- **The exit blinker starts clicking half a mile out.** X still sets the signal from miles away, and the game says "Signal set" until then.

- **Each city street keeps its own speed limit.** The cab says the new number when the street changes, and delivery times are planned on them.

- **City streets start on the road your exit ramp meets.** The streets named and the miles to the gate are the ones from that exit.

- **The gate's 15 mph no longer applies on the public street.** The street keeps its limit up to the driveway, which is a turn like any other.

- **The speed keeper holds a corner's speed when the next corner is close.** It no longer speeds up between them and brakes hard.
- **The too-fast warning for a bend now comes before it costs you anything.** It names the speed your load can take the bend at.

- **Curve and exit speed assistance slow below the sign when your load needs it.** A heavy load or part-filled tank takes some curves slower.

- **A bend is never called faster than your load can take it.** Where the sign asks too much of a heavy load or part-filled tank, you hear a lower number.

- **Partial lane keeping now steers the truck through bends, as curve assistance does.** Lane changes and speed stay yours.

- **A rollover goes on your driving record as a crash.** It counts against your safety record and reputation like a serious violation.

- **Open scales judge your safety record on the last game year only.** Old citations and out-of-service orders stop getting you pulled in.

- **A driver's public profile counts their out-of-service orders.** The safety record row names how many, never why.

- **Taking an exit keeps road speed until the gore.** You brake for the ramp after it.

- **The exit speed is now said as you enter the deceleration lane.** A ramp curve taken faster than that shifts the load.

- **Exit ramps are now as long as the real ones.** A short ramp gives you less room to stop at the bottom.

- **A loaded delivery's deadline now covers a legal sleep forced by your current hours.** Dispatch checks again at departure and tells you when it adjusts the time.

- **Sleep choices now read your projected driving hours, fatigue, and delivery time before you commit.** Press Enter again to rest.

- **A Driving mode change made mid-drive starts when the truck next stops.** (Reported by flight.)

- **City street turns keep the game's pace until it is time to brake.** Real time starts just before the turn, not a mile out. (Reported by flight.)

- **About twenty billboards were rewritten so they make sense read aloud.** Jokes that relied on a picture or trucker shorthand now say what the sign is selling.

- **Yellow lights at the end of a ramp last longer.** A yellow now holds six seconds, enough to hear it and brake.

- **Each ramp-end traffic light keeps its own steady cycle.** Lights run about a minute or more, and cross traffic clears before your green.

- **Achievement category menu help text got an owner voice pass.** Five category descriptions now read the way the road says them.

- **A career declined after review stops backing up.** You hear once, by name, that its backup was declined.

- **Discord presence comes down when you step away.** A paused game used to show your run all evening. It now hides after half an hour.

- **Quiet speaks short updates, including lane openings.** Urgent only keeps essential warnings and directions; suppressed speech stays out of the event buffer.

- **Curve assistance now steers through a bend as well as slowing for it.** Lane keeping holds your line the rest of the time.

- **Leave the wheel alone in a bend and the truck runs wide.** Steering turns the truck now instead of sliding it sideways.

- **The engine leans the way you have to steer.** Follow it into the bend and it comes back to centre as you come round.

- **Steering the wrong way in a turn leans the engine harder.** You hear the mistake straight away, not once you reach the lane line.

- **The engine still leans through turns when the assists are driving.** You hear the road's shape even when the truck takes the corner.

- **Steering guide, under Settings, Audio, flips which way to steer when the engine leans.** For drivers used to steering away from the sound.

- **The road noise now tells you where you are in your lane.** It used to carry the steering guide; the engine does that now.

- **Bends no longer chime.** The spoken call and the engine's lean carry the bend, so there is nothing beeping over them.

- **Route-transition assistance stops announcing itself over and over on a slow ramp.** It used to bury the countdown to the stop bar.

- **Pressing the accelerator now lets the speed keeper's brake off.** The keeper used to hold its brake against you until you lifted.

- **Street corners are now called at the speed a loaded truck can take.** That is about nine miles an hour, not the posted limit.

- **A turn is called out even when you are already crawling.** Turns used to go unannounced while the speed keeper held you at fifteen.

- **HKC Radio tells you it plays rock.** It used to give no format at all.


- **F1 on a course under Licenses and training says what it unlocks.** Also the sponsor level, or the level you can pay early.

- **More facilities start and end with street directions, beginning on the yard's own road.** That first link is spoken as a service road.

- **A career edited outside the game is always marked as modified now.** The notice plays once when you open it.

- **When a station fades out of range, the radio tunes to the strongest local station still in range.** The Roadhouse is the fallback only when nothing else is on the air. (Suggested by Brandon.)

- **Hazmat, doubles, TWIC and LCV holders see their freight on the board.** Bulk fuel and placarded loads move between many more shippers and receivers.

- **More California, New York and Texas pickups and drops start with street-by-street directions out of the facility.** Fewer departures leave straight onto the highway.

- **Steel works, energy terminals, auto supplier parks and more warehouses in every state now start with street-by-street directions.**

- **Street directions at a facility now cover the streets nearest the facility.** Long routes used to name streets near the city centre instead.

- **The shared Duff sound cues stay as they are.** No replacements are planned for them.

- **Chain truck stops are announced with their town.** You hear Love's Travel Stop Rolla where you heard only Love's Travel Stop.

### Fixed

- **A ramp light that turns green as you roll up announces green once.** Crossing it now says only to stop at the entrance, without a second chime.

- **Urgent only now says when a work zone turns adaptive cruise off.** It used to let go of the pedals without a word.

- **Quiet speaks every truck stop heads-up in its short form.** One that had to wait was read out in full.

- **On a Mac, the game no longer freezes at launch asking to control VoiceOver.** VoiceOver also speaks the first screen now. (Reported by Jason)

- **Radio stations that were silent, gone, listed twice or playing a different station are fixed or off the dial.** Stations that moved play from their new address.

- **Your Discord status keeps up on long sessions.** If Discord turns down an update, the game reconnects and sends it again instead of leaving the old status showing.

- **A truck parked with the radio on now leaves the live drivers board after half an hour.** A new song no longer counts as driving.

- **The game keeps answering your computer while it closes, after an update too, and finishes its last sentence first.** macOS no longer reports it as not responding.

- **A stalled update no longer freezes the download screen.** After a quiet minute the game says so and names where to download it yourself.

- **Escape now leaves an update download at once, and the game says when it is unpacking.**

- **An update download that arrives damaged is no longer installed.** The game says so and keeps the version you have.

- **On a Mac, Report a problem names the folder that really holds your game log.**

- **Chain controls now stand only while a chain law is in effect.** On dry roads the CB no longer calls one, and no chain-control trooper watches you.

- **Chain laws now apply only in the western states that post them.** A steep grade in Texas or Wisconsin no longer brings a chain law in snow.

- **A chain citation now comes only on a grade where the chain law is posted.** Snow elsewhere no longer lets a scale or patrol write one.

- **Install snow chains appears in the pause menu only when snow, ice or a chain law calls for them.**

- **With live weather setting the calendar, real snow now stays snow in any month.** An October snow in Denver is no longer driven as rain.

- **Simulated weather no longer says rain below freezing out of season.** A cold March night is overcast instead.

- **A real winter storm warning no longer reaches a career that is in summer on its own calendar.**

- **On the real calendar, dates now name the right weekday.** A leap year no longer shifts every date a day late.

- **Lane keeping on full now moves to the right lane for your exit.** Pulling out to pass no longer costs you the exit.

- **State lines, rivers and exits on several highways now match where the truck is.** The New York Thruway no longer announces Pennsylvania.

- **A CDL suspended mid-drive now ends the run.** The truck stops on the shoulder, the way a roadside stop ends it.

- **More route cues match the roads trucks drive.** Repaired checkpoints, river callouts and signs follow their routes; scenic stretches keep billboards quiet.

- **Escape on a roadside stop that suspends your CDL no longer drives on.** It returns to the terminal, like the Return to terminal row.

- **A saved run on a suspended CDL no longer resumes.** Continuing the career closes it out and opens the terminal.

- **The Dalles is no longer read as the The Dalles.**

- **The truck no longer tells you to release a parking brake that is already off.** A late "Press P to release it" could send you to set the brake while rolling.

- **The engine no longer leans into bends and corners the truck is already taking.** With curve assistance or partial lane keeping, it leans only when you drift.

- **Steering into a street corner no longer pushes the truck across its lane.** The truck turns with the corner, and curve assistance takes it for you.

- **The manual now lists Slash, Straighten up.** Hold it to stop the truck drifting across the lane.

- **The Lane keeping help now teaches the engine lean.** It used to say steer by the road sound, which only tells you where you sit in your lane.

- **The day a CDL suspension clears now matches the calendar you hear.** It never names a day already past, and a year-long one says next year.

- **With JAWS, a held arrow keeps working when you tap another key.** Hold Up and tap Space for your speed, and the truck keeps accelerating.

- **Dispatch no longer says assigned loads last until level 8 once you are past it.** It now says they last until your standing recovers.

- **The manual transmission bonus is paid only for a run driven in manual.** Switching to manual near the end no longer earns it.

- **A running engine burns fuel while you wait at a stop.** Breaks, meals and shop work idle the engine as the dock always did.

- **Exit and hours of service prompts name your own keys.** A moved steering or hours key is named as you set it.

- **More prompts and settings help name your own keys.** The cruise resume reply, driving assistance help and the controller's hours tip follow your shortcuts.

- **A key you moved onto Slash keeps working.** Hold slash to straighten stays off it until you move that action elsewhere.

- **Checking in at a weigh station no longer gets you fined for bypassing it.** Steering into the exit lane early used to leave the scale ahead of you.

- **Leaving a closed weigh station no longer runs an inspection.**

- **Exit speed assistance no longer speeds you past a lower limit before your exit.**

- **Lane keeping takes the destination exit after you cancel and miss it.** The loop-back's promise now holds.

- **Space and U say the signal is set, not on, until the blinker starts.**

- **Holding the clutch or sitting in neutral no longer leaves cruise's brake on.** Once the truck drops under its number, the brake lets go.

- **A wrecked load stays wrecked when the carrier moves you to a spare truck.** A tank load keeps its surge too.

- **A rollover with no load aboard no longer mentions a load or a receiver.**

- **The sleep preview counts only the rest you still owe.** Finishing a reset you started gives the real wake time and deadline.

- **The rest key waits for the highway when you leave a shipper.** On the streets to the on-ramp it no longer plans a stop it then drops.

- **The loyalty desk no longer says you need more points when you have them.**

- **The lane guide tone comes back after the pause menu.**

- **Route calls are no longer lost when the voice is busy.** A turn or exit call waiting behind a repeated line was dropped unheard.

- **The departure line about a required sleep names the rest stop key.** It named the hours readout, which cannot plan a stop.

- **Job details give a delivery time counted from loading.** A load waiting in a nearby town no longer shows a time before you could arrive.

- **Suspension and disqualification notices no longer mention the truck dealer to company drivers.**

- **The clock keeps the right time zone name after you change the driving pace on the move.**

- **Live construction reports cover your whole route.** Closures between towns and past a state line used to be missed.

- **A closure on the other side of a divided highway no longer slows your side.**

- **Route status on city streets reads street and city names as written.** A road like US 83 is no longer read as the word "us".

- **Resuming a save on the streets to a facility warns about the first turn again.**

- **Unposted back roads in Virginia and Montana carry the truck speed limit.** You hear 45 on Virginia back roads and 65 in Montana.

- **The dispatch board opens without a pause in a new city.** It used to take about a second the first time.

- **Adaptive cruise keeps its set speed through a pickup again.** Setting the parking brake or stopping assistance at the gate no longer switches it off.

- **A shower that came free with your fuel stays free if you leave the truck stop menu and open it again.**

- **A weigh station check-in is not offered twice when you reopen the stop.**

- **After loading, the fuel you burned idling before shutting the engine down is named as such.** It no longer sounds like the engine ran through the load.

- **At a shipper or receiver you hear a running engine idle, and shutting it down makes its sound.**

- **Loyalty shower credits and points now make the shower free.** They used to be spent while the shower still cost full price.

- **Loyalty points and shower credits from a fill-up are saved right away.**

- **Leaving a stop with the engine running no longer tells you to start it.**

- **Motel and repair lines at a stop give local time.**

- **Canceling a pickup frees the pay advance for your next load.**

- **After a manual stalls, the game walks you back into first gear.** Its old advice shifted up a gear and stalled the truck again.

- **Descent control no longer asks for a lower gear during every manual shift.**

- **Cruise no longer revs the engine while a manual is in neutral.**

- **Exit and facility stopping assistance leave the throttle alone while you shift a manual.**

- **The engine brake no longer counts against a no-jake zone, or answers G, while the clutch is in.**

- **A speeding ticket no longer takes your driving record's points out of your reputation for good.**

- **A record hold now names the day it lifts.** It used to name the day the oldest entry left, even when the rest still held it.

- **A relayed load's delivery deadline no longer counts the drive to the shipper.**

- **A failed log check no longer counts as a passed inspection.**

- **Credential courses count as off-duty time on your hours of service.**

- **While cruise is paused, plus and minus set the speed it resumes at.** They used to say cruise was off and point at the key that turns it off.

- **In Synthesized mode, a station fading out of range is announced again.**

- **The Tab radio screen names your own keys for saving a favorite and turning the radio on or off.**

- **Synthesized Roadhouse pieces come back on the next pass instead of staying replaced by a 1.5 track.**

- **Trip status says whole hours as whole numbers.**

- **Opening a screen from the pause menu no longer signs you off the drivers board.**

- **Two stops at one exit always hear the same control at the end of the ramp.**

- **Work zones no longer raise the speed limit on roads posted under 55.**

- **Turning descent control off and back on during a grade holds the safe speed again.**

- **The controls help says cruise holds at most five over the limit.**

- **The siren, weigh station, and lane guide tone now stop when you leave the drive.** They used to keep playing until you closed the game.

- **J turns the engine brake on while cruise is holding the throttle.** It no longer asks you to release an accelerator you are not pressing.

- **Exits on several routes are announced at the right mile.** Charlotte to Knoxville and Dallas to St. Louis no longer name another highway's exits.
- **Descent control holds a steep downgrade at a safe speed for your load.** It slows before the steep part, holds the gear, and names the speed once.

- **The engine brake no longer steps up and down every few seconds on a long descent.**

- **The G key gives a steep grade one length.** On a two-mile pitch it no longer says ten miles once you are on it.

- **D names a steep downgrade's safe speed for your truck and load.** You hear "for the grade" after the number.
- **City streets are no longer called a facility access road zone.** A street keeps its own name, and the speed keeper just says its number.

- **G no longer says "the next 0 miles" on the last streets to a gate.** It says nothing steep is ahead.

- **Lines the cab says in the same moment are each said once, in order.** The first is no longer cut off and started again by the next.

- **Each street turn's sound plays as you make that turn.** Turns close together off the ramp no longer save their sounds and play them all at once.

- **Exit announcements no longer name your own Interstate as the exit.** Southbound at Ardmore, exit 31B is toward Waurika, not I-35 North.

- **On facility streets the next turn is called after you are round the last.** The turn chime and the words agree again.

- **A ramp-end yield names the car the crossing sounds are for.** The words and the panned traffic come from the same vehicle.

- **Cruise says what grade it is easing for, and stays quiet over a crest.**

- **Route status on city streets starts with a capital, and the live weather Conditions line ends with a period.**

- **Curve assistance slows for any bend your load cannot take at your speed.** A full trailer or part-filled tank no longer rolls over on mountain bends.

- **The cab calls a bend whenever it would move your load.** You hear the number to slow to before the freight shifts, not after.

- **Adaptive cruise stops pushing against curve assistance in a bend.** It no longer holds the throttle while the brakes slow you.

- **A half-full tank no longer rolls over entering a bend right after another.**

- **Surge left over from one bend now lowers the number spoken for the next.** A half-full tank no longer nears a rollover unwarned.

- **Surge in a tank trailer settles instead of building on its own.** Adaptive cruise on a climb no longer damages a half-full load.

- **Every bend in a tight run is called before you reach it.** Warnings no longer wait behind calls or repeat for bends behind you.

- **The too-fast warning covers the next bend while you are still in one.** Downhill, it comes before the load moves.

- **Leaving the pavement damages the truck with lane-departure warning off.** The setting only silences the warning.

- **The engine lean leads you through an exit ramp's curve with lane-departure warning off.**
- **Adaptive cruise and the speed assists no longer drain the air tanks by pumping the brakes.** They hold the pedal steady while they slow you.

- **Losing live weather is announced once.** You no longer hear that simulated weather is in use every minute while it stays that way.

- **The stop bar countdown and stop bar tone stay quiet on a green light.**

- **Exit announcements are no longer said twice or cut off.** The exit you take, the ramp's light and a hold at the sign are each heard once.

- **A highway speed limit change is no longer spoken as you take an exit.**

- **Stopping where the stop bar tone starts counts as stopping at the sign.** A stop farther back is said once, with the distance.

- **Route-transition assistance says once that it is slowing for the light, sign or yield.**

- **U names the destination exit, and gives the distance to the facility gate itself.**

- **C's arrival estimate uses the road left to the gate once you are off the highway.**

- **The facility gate warning stays quiet when an assist is already holding you to its speed.**

- **The speed keeper says it is easing only when it is slowing the truck down.**

- **Billboards wait until you are past the exit you are taking.**

- **The downgrade warning no longer tells you to set the engine brake when it is already on.**

- **The engine brake's growl no longer restarts over and over as the throttle comes and goes.**
- **Live weather no longer drops to simulated weather when the nearest weather station is silent.** The next station over is used instead.

- **The Every Sky in the Logbook achievement now names freezing rain.** Its description lists all nine kinds of weather it takes to earn.

- **The You Get What You Get achievement waits for your walk-around to find the problem.** Hooking the trailer no longer gives the defect away.

- **Refusing a bad trailer at pickup no longer earns Somebody Else's Turn at delivery.** You hauled the sound replacement, not the bad trailer.

- **A yield is judged where you cross the road.** A gap that holds while your whole rig gets across counts as clean.

- **Route-transition assistance no longer stops short of a clear yield.** It rolls through at a walk and says when your gap comes.

- **Facility stopping assistance now slows for the exit ramp's curve.** The whole ramp also runs in real time.

- **The steering lean on an exit ramp now leans only for its curve.** Running wide there no longer says you crossed into oncoming traffic.

- **Exit speed assistance no longer takes over from cruise for a fraction of a mile per hour.**

- **Adaptive cruise no longer damages the freight when it brakes downhill for traffic, a ramp, or a lower limit.**

- **The Map Route line no longer repeats the deadhead city.** A facility approach that used the same city on every local leg is spoken once. (#205)

- **Holding the cruise target key no longer races the set speed.** Only a fresh press steps it; the operating system's key-repeat is ignored.

- **Trucks build speed at a realistic rate again, loaded or empty.** The automatic shifts through each gear once, with one clunk per shift.

- **Route-transition assistance brakes smoothly to a stop sign.** It slows steadily to the line instead of arriving fast and stopping hard.

- **Switching the music source during a drive now changes the radio straight away.** Choosing Original ends the synthesized playlist.

- **A lower speed limit is always announced.** The warning and the new limit no longer go unspoken when the road is busy.

- **Street turns are called once.** After the quarter-mile notice, the turn's own call adds only the advise speed.

- **A turn right after another is called before you reach it.** Turns taken slowly chime as you make them, too.

- **The end of an acceleration lane says you have merged.** You hear "Lane ended" instead of being told to merge left into the passing lane.

- **City streets to a delivery keep the game's pace.** Real time starts near the gate, not for the whole drive in from the ramp.

- **The route readout on an exit ramp counts to the facility gate.** When streets follow the ramp, it no longer reads the ramp's end as the destination.

- **The upcoming readout stops listing highway stops once you leave the highway.**

- **Following the engine lean out of a drift no longer swings you across the lane.** It centres as the truck straightens, not once it reaches the middle. (Reported by flight.)

- **Back-to-back sleeps at one stop count as one rest.** After 3 hours and 3 more, the menu offers the last 4 to finish your 10-hour reset. (Reported by Vol4life.)

- **Right at the destination exit, the cab no longer announces a second one further on.** The exit you are taking is the only destination exit you hear.

- **Where a ramp leads onto city streets, the handoff at its end says so.** You hear "onto the streets" instead of "to the entrance".

- **Route-transition assistance slows for a green light, not just a red.** It takes you through at rolling speed instead of too fast.

- **A light the assist stopped you at is no longer called "stopped short".** You hear only that assistance is holding for green.

- **You have to shut the tractor off before the fuel island will fill you.** Same bar as a car pump. The rest-stop and full-lot menus offer the engine kill switch so you are not stuck leaving the stop to turn it off.

- **Deleting or resetting a career now always takes effect.** A save whose file was renamed or copied said it was deleted and stayed. (Jerry)

- **Manage careers lists careers from earlier versions, so you can delete them.** Before, one stayed on the Choose career list for good.

- **Deleting a backed-up career asks about its cloud backups.** Remove them too, or keep them; a new career with the same name no longer writes over kept ones.

- **Restoring a career you had deleted no longer mentions a fallback file.** There was no save to replace, so none was kept.

- **The truck drives off again after a stop.** Leaving a fuel or rest stop no longer leaves the brakes held, with the truck stuck at 0 miles per hour. (Shane, Jerry and Jessie)

- **A deleted career no longer asks which copy to keep.** The Online menu named a career you no longer have, and nothing could clear it.

- **Grain elevators, quarries, lumber yards and materials yards now start with street directions.** Those four kinds of stop never had them.

- **Rail ramps, air cargo docks, food terminals and industrial parks start with street directions too.**

- **A way in to a facility never runs through a locked gate or up a street signed against trucks.**

- **Two badges can finally be earned.** One is for a whole run at better than eight miles to the gallon, the other for twenty-five proper breaks; neither had any way to be awarded.

- **The dispatch board recommends a load again.** It had stopped naming one for every driver except a company trainee.

- **Learn game sounds no longer explains itself with a speech setting that was removed.** Four entries pointed at terse; they name Urgent only now.

- **The manual lists the key that repeats the last CB call, and the one that names what the radio is playing.** Both keys worked already.

- **An empty trailer takes a street corner faster than a loaded one.** Every corner used to be advised as though you were carrying a full load.

- **Facility stopping assistance now stops you at a pickup.** It braked and held at the entrance on a delivery, but only talked you in at a pickup.

- **The way in to a facility sticks to real streets.** Approaches used to cut through parking lanes, a fire lane, and one coffee shop's drive-through.

- **A turn chimes once, as you take it.** The same corner used to sound up to three times on the way in, and at Urgent only it sounded where nothing was said about the turn at all.

- **An assist holding the brake no longer empties your air tanks.** The held pedal was billed as a fresh application sixty times a second, and the spring brakes could set short of the gate.

- **Towns with no real freight site now hold one yard instead of four invented warehouses.** Every other town keeps the businesses the map can back.

- **The assists brake earlier with a tank load.** Cruise, the curve assist and the stop at the gate now allow for the liquid running forward.

- **Short, false walls of hill on flat road are gone.** Over a thousand of them, most only a few hundred yards long, were never there.

- **Mountain climbs pull like mountain climbs again.** Wolf Creek Pass, the redwood coast and the Santiam were all being flattened to a gentler grade than the road has.

- **A station in one of your playlists now names the song it is playing.** The cab used to answer that your playlist sends no song information.

- **A tight bend can now be held at the speed it is signed for.** The truck used to run wide at the number curve assistance had just braked to.

- **The approach to your delivery exit now asks you to signal.** It used to name the lane and the ramp speed only.

- **The truck now says when you lose the exit lane.** It used to say the lane was set, then go quiet until you missed the exit.

- **The engine leans only for bends that need steering, and leans as hard as the bend asks.** A winding highway no longer swings it side to side the whole way.

- **Curve assistance no longer stamps on the brakes just before a bend.** The last few miles an hour come off gently now.

- **Curve assistance no longer runs you out of air on a downhill bend.** It holds one steady brake through the bend instead of pumping.

- **The speed keeper no longer runs you out of air approaching a gate.** It used to strand you short of the yard.

- **z103.com plays music again.** The station moved its stream and the old address played only a sign-off message.

- **Now playing works on more radio stations.** Some used to say they had no song information. (Reported by js730.)

- **Rest areas on roads with no truck-usable stop are now on the map.** Twenty-two of them, including Heath Creek on I-35 north of Owatonna.

- **Street directions now end at the facility.** Hundreds used to end at a railway line, a power substation or a shop nearby.

- **A truck stop that was on the map twice is announced once.** Signalling for it arms the real exit, with its exit number and the stop at the end of the ramp.

- **Chain truck stops like Love's, Pilot and Flying J are announced as travel centers.** Many were called service plazas, which are on toll roads.

- **Street directions name one road, not a list of route numbers.** A street with several numbers is spoken by its first.

- **Businesses that are not truck stops are off the map.** Independent truck stops are announced as travel centers, not service plazas.

- **Station names keep their apostrophes.** Fifteen imported stations no longer read as, for example, Birmingham s Beautiful QEZ.

- **Stepping the dial visits each terrestrial station once.** The band no longer re-sorts by signal under a moving truck between presses.

- **Real roadside attractions on billboards stay near the real thing.** The twine ball, rocking chair, ketchup bottle and petrified wood signs no longer appear just anywhere.

- **Alt 3 names the town you just heard the game pass.** Route towns and the leg's own cities count now, not only villages off the corridor.

- **A facility placed only by estimate now says so.** Its approach miles come from that estimate.

- **Wall Drug corridor boards again cover Wyoming as well as South Dakota and western Minnesota.** Montana stays out.

- **Interstate curve warnings are rare again, about one slow-down per hundred-plus miles.** Ramps and town streets no longer count as highway bends; Glenwood Canyon and the US-550 switchbacks stay.

- **Twenty-five more runs got the same curve cleanup.** Glenwood Canyon and the US-550 switchbacks stay.

- **Opening the pause menu now stops the tank surge sound.** It no longer keeps playing through the menu and back into the drive.
- **More truck stops announce their own exit number and the light or stop sign on their own ramp.** Many had a neighbouring exit's number.

### Compatibility

- **Careers from 1.8 and earlier do not carry over; every driver starts a new career.** Old saves stay listed, untouched and playable in 1.8.

- **On Linux, speech uses the Speech Dispatcher installed on your computer.** Without it the game still starts, but stays silent.


### Changed

- **Lane centering assistance is gone from Driving assistance.** It never steered; lane keeping full already holds the lane, and old saves lose the row on their next save.

- **Circle K, Exxon, QuikTrip, and similar plazas already on the map are bobtail-only.** A familiar name is not a promise a tractor-trailer can pull in.

- **The map's fuel-gap search no longer hammers the shared map server.** It backs off when asked and never records a failed lookup as an empty corridor; convenience plazas stay bobtail-only unless the name says travel center.

- **Traffic lights announce only their color, and the approach countdown says only the distance.**
  Contributed by Tim ([@trssharp](https://github.com/trssharp)) in [PR #191](https://github.com/orinks-games/Freight-Fate/pull/191).

- **Route status starts with your location or arrival information instead of saying "Route status."**
  Contributed by Tim ([@trssharp](https://github.com/trssharp)) in [PR #192](https://github.com/orinks-games/Freight-Fate/pull/192).

- **The exit blinker keeps clicking until you take, cancel, or miss the exit.**
  Contributed by Tim ([@trssharp](https://github.com/trssharp)) in [PR #190](https://github.com/orinks-games/Freight-Fate/pull/190).

- **Canceling an exit stops its countdown and lane guidance until you signal again.**
  Contributed by Tim ([@trssharp](https://github.com/trssharp)) in [PR #190](https://github.com/orinks-games/Freight-Fate/pull/190).

### Added

- **Your playlists can shuffle.** Settings, Audio, Shuffle personal playlists plays every track once in a random order before any repeats. (Suggested by Hailey.)

- **A trooper passing you can see worn tires and a dead trailer lamp.** Seeing one pulls you in for a Level 2 walk-around, so walk around the truck first.

- **Gilley's and the Longhorn Ballroom are on the Texas billboards.** Houston remembers the Urban Cowboy honky-tonk; Dallas advertises the bull and Bob Wills' ballroom.

- **The scale's inspection lane is a real Level 1 inspection.** Worn tires, brakes, damage and trailer defects are written up; a critical one parks you.

- **A clean Level 1 earns an inspection decal.** Open scales wave you through for three months unless your record is targeted.

- **Troopers now pull legal drivers in for routine Level 3 inspections.** Licence and paperwork, fifteen minutes; a clean one costs nothing else.

- **Roadcheck week in May triples inspection odds, and the CB warns you.**

- **Walk around the truck before an inspector does.** The new row at the terminal and every stop reads out what an inspector would find.

- **Pilot, Love's and Flying J travel centers now stand on ten thin US highway runs.** Clovis, Rapid City, Dodge City, Norfolk, Benson, Tucson, Lufkin, Columbia Falls, Mayer and Missoula each gain a tractor-trailer fuel stop.

- **Love's and Pilot travel centers now sit at Corbin on I-75, between London and Knoxville.** Both take a tractor-trailer and both offer truck parking.

- **Little America travel centers at Cheyenne and Flagstaff are pull-ins.** Cheyenne serves the I-80 runs to Laramie, Rock Springs and Salt Lake City; Flagstaff serves the runs to Camp Verde, Kingman and Winslow.

- **Little America's fuel center on I-80 west of Rock Springs is a pull-in.** It serves the runs from Rock Springs and Cheyenne toward Salt Lake City.

- **A truck stop known driving one way on a highway now shows up driving the other way too.** Flying J, Petro, Love's and the rest are copied only where the same highway overlaps.

- **Midway Service Center is a pull-in on US-45 between Mobile and Meridian.** Circle K on that corridor and Birmingham to Opelika is bobtail-only.

- **Wall Drug on Rapid City to Sioux Falls is park-capable only.** Same Exit 110 pull-in as the westbound side, with no diesel.

- **Wall Drug on the Mitchell to Rapid City run is park-capable only.** Truck parking at the Wall exit, with no diesel.

- **Wall Drug sells its famous five-cent coffee and free ice water.** Both ease fatigue at the park-only stops on the Mitchell to Rapid City and Rapid City to Sioux Falls runs.

- **More billboards along the road, without crowding it.** New jokes, attorney and travel-plaza ads, music landmarks, casino and Tejano approaches and dated movie promos, at the same spacing as before; Maine, Vermont, Alaska and Hawaii stay quiet.


- **Career stats opens your citations and violations, newest first.** Each
  says what it was, why, what it cost, when, and where.

- **You can change which key or pad button each driving control uses.**
  Settings, Gameplay, Controls, then Keyboard shortcuts or Controller
  buttons.

- **After you change a key, the F1 help, How to play, and the spoken
  prompts say the new one.** With a controller in use, How to play names
  the pad button instead.

- **A driver directory sits under Online, next to Drivers on duty.** It
  lists every driver with a public profile, and Enter on a driver reads
  their profile.

- **A lifetime disqualification ends the career, and only you can remove
  it.** A new last row on the terminal menu, Close out this career, removes
  the save and every cloud backup of it.

- **The owner-operator buy-in now waits for a clear CDL.**

- **Your driving record now costs you with the carrier and the insurer.** A
  bad record can hold your equipment back or end your employment; an
  owner-operator pays an insurance surcharge instead.

- **The record line on Career stats and the status screen says what your
  record is costing you.**

- **Nothing on your record from before this build counts toward the
  carrier's record review or the insurance surcharge.**

- **Your public profile now says what each achievement was for.** Every
  badge carries its description and category, on orinks.net and in the
  in-game driver profile.

- **Dispatch checks the state construction reports before choosing your
  route.** With real traffic on, it tells you what roadwork is on the way
  and takes the next route when a road is closed.

- **Real construction reports now cover fifteen more states.** Live work
  zones there appear on your route when real traffic is on.

- **Weather alerts from the National Weather Service reach dispatch and the
  cab.** With real weather on, dispatch routes around the worst of them,
  and the cab reads each one out as you drive into it.

- **Dispatch relays a load from a nearby town when your board is thin.**
  You hear one assignment, the deadhead first, then the load itself.

- **Empty repositions no longer appear on the dispatch board.**

- **The pumps charge this week's real diesel price.** A new Settings, World
  row, Fuel prices, switches back to the simulated price.

- **Nineteen new songs join the in-house stations.** Glory Road, Puro
  Tejano and Neon Drive each have a full rotation now, and the Night Line
  has two more ballads.

- **Passing and crossing traffic sounds like the vehicle it is.** A pickup,
  a motorcycle, a bus or a farm tractor going by now plays its own cue.

- **Game sound can step back while the road talks.** It is off by default;
  turn it on under Settings, Audio, "Game sounds step back for speech."

- **Sixty new billboards along the highways.** Fresh roadside jokes join the
  rotation everywhere, and tribute signs mark the corridors that raised the
  music.

- **Learn what every sound means before you meet it at speed.** The Learn game
  sounds screen on the main menu and the pause menu plays any cue: Enter hears
  it, F1 explains it.

- **The game now tells you when you are backing the wrong way.** Backing along
  a travelled lane away from your destination is called out, and if you keep
  going, traffic finds you.

- **The truck now tells you when the lane you passed in is open again.** Once
  you are past with room to spare, you hear "Clear of the box truck. Right
  lane open."

- **Press L any time to hear your lane and whether each neighbouring lane is
  open, blocked, or closed.**

- **You now choose which career is your public one.** Every backed-up career
  on the Cloud backup menu offers "Make this your public career," with a
  spoken confirmation.

- **Liquid bulk opens late in your career, and it drives like nothing
  else.** You hear the surge run forward when you brake and come back into
  you, so brake early and brake once.

- **Troopers are out on the road now, whoever you are.** A clean driver
  hears them often and pays nothing, and speeding nobody saw no longer
  bills you at the dock.

- **A wrecked truck now drives like one, and eventually not at all.** Damage
  brings reduced power, then limp mode, then out of service, each announced
  before you feel it.

- **Bring a company truck back wrecked and the carrier notices.** Dispatch
  grounds the tractor, moves you to a yard spare, charges a deductible,
  voids the safety bonus and logs preventable damage on your record.

- **Your freight can be damaged, and the receiver can refuse it.** Hard
  stops, fast corners and collisions hurt the load; the dock notes an
  exception, files a freight claim, or refuses the load outright.

- **Traffic law now follows you through your whole career.** Serious
  violations go on your driving record, enough of them suspends your CDL,
  and running from a traffic stop can end the career for good.

- **Dispatch trusts you less as your reputation falls.** Lower reputation
  means fewer, worse-paying jobs and no load choice; at the bottom the
  carrier ends your employment.

- **Falling asleep at the wheel is now on your record.** Doing it again is a
  fatigued-driving violation, and when you cannot stay awake at all you are
  placed out of service.

- **Blow past the facility gate and you now miss it.** The approach warns
  you with the speed to slow to; carry past too fast and you loop back
  through a safe turnaround with the clock running.

- **Facility stopping assistance always makes the gate for you.** Miss it
  twice and the game offers it.

- **Weigh stations are on the map.** Real scales are now stops on the
  routes that sign them.

- **Towns now enforce their engine brake ordinances.** Coming up on a no
  engine brake zone with the engine brake on you hear a heads-up; keep it
  barking inside and a local officer fines you on the spot.

- **Cruise and curve speed assistance switch the engine brake off inside a
  no engine brake zone and hold speed on the service brakes.**

- **Live road reports and truck parking counts cover far more of the map.**
  They now arrive wherever a state publishes a feed.

- **Public profiles now tell more of your career, and the drivers list
  hears your radio.** Your row names the station you are tuned to, and
  your public statistics add lifetime earnings, badges, endorsements and
  fleet tier.

- **The whole map got its roadside signs.** Real-place billboards now
  stand in every region, each at its attraction's real milepost.

- **Achievements are now browsed by category.** The achievements screen
  opens on seven categories, each spoken with how many you have earned;
  Enter opens its badges.

- **Thirty-one new original songs and beds across the game.** New driving
  beds, ten more country songs, and a new station, Nashville After Hours,
  playing late-night Nashville jazz.

- **Careers that reach level 21 get a menu theme of their own.**

- **Thousands more real stations on the dial.** Local commercial stations
  come in near their real transmitters, and a new Web radio band carries
  internet stations from everywhere (station catalog contributed by
  CatalystForChaos, [@CatalystForChaos](https://github.com/CatalystForChaos),
  [PR #150](https://github.com/orinks-games/Freight-Fate/pull/150)).

- **Save your stations with O.** O keeps the current station as a favorite
  and lets it go on a second press; Favorites are their own dial category
  right after your playlists.

- **Tuning a dead station no longer freezes the cab.** The radio connects in
  the background and gives up on a station that will not answer.

- **Roadside sleep now works from T when the truck is fully stopped.** Away
  from a route stop, T opens the emergency shoulder-sleep warning; route
  stops still take priority.

- **Five more AFN bases on the dial.** Iwakuni, Kunsan, Misawa, Okinawa and
  Sasebo join the AFN section, all carrying The Eagle.

- **Steer through curves by ear.** The road lean slides into a bend and sits
  toward lane center when you drift; drift to the edge and the rumble
  strip, then gravel, answer from that side.

- **Transverse strips warn ahead of a true hairpin.** Three grouped rumbles
  under all your tires, far enough out that braking still makes the curve.

- **A demanding bend chimes from its side on entry, and the co-driver gives
  a verdict on the way out.**

- **I toggles the lane locator, a soft tock from where you sit in your
  lane.**

- **A new Audio setting, lane and edge cue volume, sets how loud the lane
  and edge cues are: quieter, standard or louder.**

- **The road tells you how many lanes it has.** Your road status says the
  lanes on your side, the trip briefing sums it up, and you hear the road
  widen or narrow as you drive.

- **A warning before every steep grade.** A long climb or descent is called
  out before you reach it, with how steep, how far, and going down, what
  to do before it starts.

- **A new song on the country stations.** "Dust on the Highway", an
  instrumental, plays on The Rawhide, Big Wheel Country, Prairie Line and
  Big Sky Country.

- **G also names the next grade ahead.** One press says what you are on
  and what is coming, how far off and how long it runs.

- **Review recent spoken messages while driving.** Comma and period step
  through what has been said, the brackets switch message groups, and
  Ctrl C copies the current one (thanks to Day Garwood,
  [@day-garwood](https://github.com/day-garwood),
  [PR #122](https://github.com/orinks-games/Freight-Fate/pull/122)).

- **Walk around the trailer before you pull out.** Hooking from a drop yard,
  a new pickup option checks lamps, brakes and tires, and you can refuse a
  bad trailer and wait for a sound one.

- **Receivers with a drop yard take the whole trailer.** Back the loaded
  trailer into their yard, hook a clean empty and go, faster than a dock.

- **Drop and hook.** Busy shippers keep loaded trailers in a yard, so you
  drop your empty, hook a loaded one and go; you get the trailer you get,
  and the game tells you its shape.

- **Detention pay.** A shipper who holds you at the dock past two hours is
  billed for the wait, and it shows on your settlement as money in.

- **Owning your trailer costs you the fast turn.** Nobody swaps an
  owner-operator's own trailer, so you load at a dock every time.

- **The yard has thirty-five tractors in it now.** Day cabs and sleepers,
  light and heavy, aero and long-hood, so every band of the fleet holds a
  real spread.

- **Dispatch picks the truck to fit the load.** Early on you slip-seat, and
  dispatch says which truck and why; make level nine and you get a truck
  of your own to keep.

- **Twenty-one new achievements, including several that should not count.**
  Badges for the radio, for the driving craft, for Christmas Day and
  Friday the thirteenth, and one for holding exactly sixty-nine miles an
  hour.

- **Cruise reads the road ahead and drives the hill before it arrives.**
  It banks speed before a climb and eases near the crest, saying so the
  first time; the switch is Predictive cruise under Driving assistance.

- **Cruise says so when a hill has beaten it.** With the accelerator on the
  floor and the truck still losing the grade, it tells you once what speed
  it is holding.

- **Setting the parking brake at speed now dynamites the brakes.** The
  spring brakes slam on, the tires screech, your tread takes flat spots,
  and you are told to save it for emergencies.

- **New hires can review the rest of the day's board.** Below level 8, a
  new board row, "Review the rest of today's board", reads out the other
  postings dispatch put up.

- **FM radio now behaves like FM radio.** Height is range, so a grade
  brings distant stations in, and at the edge of coverage hiss creeps in
  and the signal flutters with your speed.

- **Cold starts build their air out loud.** With low tanks the engine holds
  a fast idle over a fill hiss; the dryer's purge pop and the idle
  settling are your cue to release the parking brake.

- **Parked high idle is on the cruise button.** With the parking brake set,
  K latches a fast idle and plus and minus step it; on a controller it is
  the Y button.

- **The engine brake finally has its voice.** Switch it on and you hear the
  growl, deeper with more cylinders and higher revs, cutting out through
  each shift and going quiet on the throttle.

- **Cruise has a resume button now, like a car.** Shift K brings back the
  set speed a brake tap cancelled; K by itself still sets a fresh target.

- **The automatic box now manages its own engine brake, like a real one.**
  J arms it and it steps its own stage; 1, 2 or 3 take the stage back, and
  Alt J turns it off.

- **Alt T switches between automatic and manual shifting on the road.**

- **Low-gear shifts are quick now, like a real automated box.** Shifts
  through the bottom gears take about half the time, and the launch rhythm
  matches a real truck.

- **Curve speed assistance slows you for a bend.** Engine brake on a steep
  downgrade, service brakes anywhere else, gentle braking on ice.

- **Brakes and gear changes sound like the real mechanisms now.** The brake
  valve clunks and releases its air, louder the harder you press, and
  every shift is a real recorded shift.

- **Real stations reach a lot more of the map.** With streamer-safe mode
  off, the radio picks up public, community and college stations in dozens
  more places.

- **The radio dial now fills out like a real city.** With streamer-safe mode
  off, each town carries its whole public and community band, not one
  station.

- **Radio reading services for blind listeners are on the dial.** Wherever a
  city has one, it sits on the band with everything else.

- **Community radio joins the dial.** With streamer-safe mode off,
  volunteer-run, college, Pacifica, and tribal stations play alongside the
  public ones.

- **The loneliest highways have a signal now.** With streamer-safe mode off,
  the emptiest stretches of Nevada and West Texas catch their local public
  station.

- **The engine brake has a cylinder selector.** J turns it on at the stage
  you last picked, and 1, 2, and 3 pick the stage while it is on, spoken as
  you change it.

- **On a controller, the modifier with the engine brake button steps through
  the stages.**

- **Your own music can play on the in-cab radio.** Drop M3U playlists into
  the Playlists folder next to your saves and each becomes a station under
  Your playlists.

- **Your playlists play only when streamer-safe mode is off.**

- **The radio dial now jumps by category.** Control with a dial key leaps to
  the previous or next section of the dial and announces where you landed.

- **International public broadcasters are always on the dial.** With
  streamer-safe mode off, a new International section carries English
  public radio from Australia, Ireland, New Zealand, France, and Canada.

- **The road now names the towns that change your speed limit.** You hear
  "Entering Strawberry" before the limit drops.

- **A new Place callouts setting controls how much you hear.** Sparse, the
  default, names only the towns that explain a limit change; all adds the
  towns you pass through; off silences place names.

- **Roadside chatter has its own switches.** Landmarks, billboards, and
  truck stop flavor each have a setting, so you choose what the road tells
  you. Contributed by Noel Romey ([@nromey](https://github.com/nromey)) in
  [PR #54](https://github.com/orinks-games/Freight-Fate/pull/54).

- **Every stop the game announces is one your truck can enter.** Car-scale
  gas stations are no longer announced or offered as exits, and hundreds of
  real truck stops and rest areas join thin corridors.

- **The stop bar at the end of a delivery ramp now has a position.** The
  distance counts down toward the light, and S on the ramp speaks the
  light's color and how far the bar is.

- **The co-driver now warns you before the speed limit drops.** She calls it
  while there is still room to brake, and a limit that lasts only through a
  short town says so.

- **An armed exit now counts itself down.** Once your signal is on, the exit
  calls again as it nears, and each call says if you are not in the right
  lane.

- **Curve calls now open with a tone on the curve's side.** A curve chime
  panned to the bend's side lands just before the spoken call.

- **S now speaks the bend's advisory speed with the posted limit in bend
  country.**

- **A co-driver now reads the road: spoken curve callouts.** Bends that need
  slowing are called before they arrive, like "Sharp left, half a mile.
  Advise 35."

- **U lists the next few bends with their advisory speeds, and D folds the
  bend into its safe-speed number.**

- **Curve callouts can be turned off under Settings, Driving assistance,
  Curve callouts.**

- **Rest stops can now tell you how big the lot is.** Where the lot has been
  counted, the parking note adds the spaces, like confirmed truck parking,
  45 spaces.

- **The GPS now calls out posted low bridges and weight limits.** You hear
  them ahead of the point, the way toll plazas are called.

- **The menus borrow a few songs from the radio.** Six radio instrumentals
  join the menu music by day and after dark, and your milestone theme still
  plays first.

- **The radio finally sounds like radio: fifty-two new original songs.** The
  fictional stations now have real rotations of sung songs and instrumentals
  in each format.

- **Route-transition assistance now handles the light at the end of the
  ramp.** With it on, the truck brakes for a red, stops at the bar, and
  holds until the green; pulling ahead is still yours.

- **Driving assistance reduces workload without driving for you.** One
  preset coordinates the assists; you still steer, confirm routes and exits,
  leave long stops, and handle every yard and dock task.

- **Choose how much driving assistance the truck provides.** A new Driving
  assistance settings category offers Realistic, Balanced, All assists, and
  Custom presets.

- **The truck now sounds like a real truck.** The engine voice comes from a
  real cab recording and follows the rpm through its range.

- **A new Engine voice setting under Settings, Audio switches between real
  and classic, even while driving.**

- **A bobtail truck is finally just the tractor.** Deadheads drop the
  trailer's weight, so the truck pulls away quicker and stops shorter.

### Changed

- **Running from the police is judged by how you drive, with no key to
  press.** Hold speed unbraked for twelve seconds after the final warning and
  it is a pursuit; a touch of the brake is a forced stop.

- **Whole hours are spoken as whole numbers.** Three hours is "3 hours"
  now, never "three point zero".

- **Time and weather, Trip status, Career plan, and the first-day briefing
  are screens of lines now.** Up and Down read one line at a time, and Enter
  repeats it.

- **The logbook opens with what you are doing and since when, then your
  limits one per line.** Entries read newest first, each led by what you were
  doing.

- **The construction warning says the speed drops one mile before the work
  zone, then again inside it.** The mile ahead of the work is no longer
  called the taper.

- **Pausing no longer takes you off duty.** You stay on the drivers list,
  shown as paused.

- **A bad driving record now shows on your reputation.** Citations and
  serious violations cost points for a game year; a major offense costs
  them for good.

- **The carrier's record review and the insurer look back one game year.**

- **The engine brake now follows your revs.** Its growl rises and falls
  with the engine, so on a manual you can hear the shift point coming.

- **Manual shifts clunk twice, like the automatic.** One clunk when you
  select the gear, a softer one when the gear takes.

- **Fog is silent now.** The distant fog horn is gone; you hear fog as the
  visibility calls and the wet road.

- **Station breaks run more like radio.** A station now airs host breaks,
  station IDs and commercials between songs.

- **Steering and assisted lane changes use the mechanical blinker sound.**
  The clicks follow your position and stop when the move ends. (thanks to
  Tim, @trssharp, for [the original audio contribution](https://github.com/trssharp/Freight-Fate/commit/d3140964ec376a8ba0d9e5cf14d3f23052b73e60))

- **The engine follows your position within the lane.** Its sound moves
  left and right with the truck unless lane keeping is full.

- **You can read another driver's profile without leaving the game.** Press
  Enter on a driver in Drivers on duty, or choose Your profile on the
  Online menu for your own.

- **You can choose to stay a company driver.** In Business status, choose
  Stay a company driver to stop the owner-operator reminders; Reopen the
  owner-operator plan restores them.

- **Owner-operators can go back to company driving.** Go back to company
  driving, in Business status, sells your tractors and trailers to the
  carrier and puts you back on company wages.

- **You can play from a braille display with speech off.** A new Output row
  in Settings, Speech, offers speech and braille or braille only; braille
  only works with NVDA and JAWS. (asked for on AppleVis)

- **Career 1.9 now has a Linux download.** The release ships a Linux
  tarball and an AppImage; speech comes through Speech Dispatcher.

- **Career 1.9 runs on the Blazie BT Speak and BT Braille, and other ARM
  Linux computers.** Career 1.9 has 64-bit ARM Linux builds, and the
  manual's download table says which file to pick.

- **The game can say when other drivers go on or off duty.** Turn on Say
  when drivers go on or off duty, on the Online menu; it is off by default.

- **You choose how often you hear that a career is backed up.** A new
  Settings, Speech row, Say when a career is backed up, offers every time,
  once a session, or never. (asked for by MariahL)

- **Each career's Cloud backup screen has a Back up this career now row.**
  It sends this computer's save to your orinks.net account straight away
  and tells you the result.

- **Account achievements now have their own Online menu browser.** It lists
  achievements earned across every career on this computer, by category.

- **When the cloud limit removes your least recently played backup, the
  game names it and says the local career stays.**

- **Career 1.9 runs on Windows and Apple Silicon Macs.** The Windows zip
  and the Mac app include the music and audio libraries; Intel Macs are
  not supported.

- **Licenses and training replaces the endorsement menu at every terminal.**
  Carrier certificates come first, then the CDL endorsements by written
  test, then the TWIC port card and the LCV certificate.

- **Flatbed securement now covers steel and lumber.**

- **Hazmat needs a paid background check that clears on its own while you
  keep driving.**

- **Bulk fuel now asks for the X combination, tank and hazmat together.**

- **A sponsored course can be booked at most one level early.**

- **Career stats lists every credential you hold, every one in the works,
  and how many days each check has left.**

- **You can train out of the automatic-only restriction.** Book manual
  transmission training at any terminal; afterwards, runs on the manual
  gearbox pay a manual-spec differential, named in the settlement.

- **New credentials are announced twice.** Every new grant is repeated the
  next time you walk into a terminal, and an unlocked specialty job on the
  board says you are cleared for it.

- **The drivers list keeps itself up to date while you read it.** Drivers
  who set off or sign off turn up on their own.

- **Live internet radio starts sooner after you tune it.**

- **Truck status is now a list you can review.** At the terminal, arrow
  through fuel, condition, each wear reading, grime and chains one at a
  time; Enter repeats the line.

- **"Four Sources and the Truth" joins the country stations.** A country
  song about trusting the forecast.

- **"Dangerous Dan" and "Dial-up Summer" join the country stations.** An
  outlaw country song and a ballad about a summer spent online.

- **Alt C says the CB chatter again.** It repeats the last CB call with the
  distance from where you are now, at every speech setting. (thanks to
  marrie for asking)

- **Exit ramps now use real advisory signs where the map has them.** A read
  sign can slow the truck's ramp target but never raise it.

- **Trucks climb hills like trucks.** A loaded tractor loses speed on a
  long grade while a light box truck holds it, so traffic strings out on a
  climb.

- **Real time joins the driving mode row.** Under Settings, Gameplay,
  Difficulty and hours of service, Driving mode offers Relaxed, Standard
  and Real time, which runs the clock at the speed of a real one.

- **In Real time the date and time start in step with your computer's
  clock.**

- **The driver tablet has a Radio app.** Under Tab, Driver apps, Radio: it
  says what is playing, keeps favorites, lists stations in range, and
  Search stations finds any station on the dial by name.

- **Shift and Y says what the station is playing.** A stream that sends no
  song information says so.

- **Phoenix Fire FM is on the dial as a named station.** (requested by
  Jerry)

- **Sunny 1100 WGPA is on the dial everywhere.** The Bethlehem,
  Pennsylvania station plays classic country, rockabilly and western swing.

- **City streets into a facility now run at your state's own speed limit.**

- **Ramp ends have cross traffic now, so listen for your gap.** At a stop
  sign, "clear, pull ahead" waits for a gap and names what is crossing while
  you hold.

- **Ramps that end at a yield or a roundabout now say so and play by yield
  rules.** Slow, listen for your gap, roll through if the road is clear,
  and stop only when it is not.

- **Live road reports now cover Florida and New York.** With Traffic source
  set to real time, you hear crashes, closures and other incidents from
  each state's 511 service as you approach them.

- **A weigh station transponder can wave a clean truck past the scale.**
  Company drivers get one once dispatch trusts them; owner-operators
  subscribe from Business status.

- **With a transponder, an open scale gives a green light to keep rolling
  or a red light to pull in.**

- **The horn runs on brake air, like a real truck's.** Run the air low and
  the horn goes silent while the brakes keep what is left.

- **The horn can move an animal off the road.** One blast may clear an
  animal hazard, but deer and elk often freeze, so braking stays the
  instruction. (thanks to Shane for the idea, and Brandon for the rebuttal)

- **State welcome signs are read at the border.** Crossing a state line
  reads the nickname and a bit of roadside trivia; the billboard chatter
  switch governs them. (thanks to Brandon for asking)

- **Dispatch now routes through freight around the Million Dollar
  Highway.** A new corridor runs from Durango through Cortez and Monticello
  to Moab; loads that serve Durango or Montrose still drive the passes.
  (thanks to Jerry)

- **Adaptive cruise slows to the safe speed in bad weather and says so.**
  It climbs back as the weather lifts. (thanks to Brandon for the
  suggestion)

- **Traffic jams where the real road jams.** Rush hour on I-5 through Los
  Angeles is stop-and-go, and a quiet rural highway never jams.

- **You can choose how much room the truck leaves to the vehicle ahead.** A
  new Following gap setting in Settings, Gameplay, Driving assistance
  offers close, normal or far. (asked for after Darren was pulled over)

- **Distant stations no longer clutter the dial.** Big-city stations stop
  reaching you from hundreds of miles away, though in empty country the
  far-off station is still there.

- **Hundreds more real stations are on the dial, in the places they
  broadcast from.** Country, classic rock, hits, oldies and top forty
  across all fifty states, each reaching as far as its licensed power
  carries.

- **Some stations that were in web radio have moved to the dial position
  they broadcast on.**

- **Stations that had gone off the air are off the dial.** Tuning no longer
  lands you on silence, and a few hand-picked stations that are down are
  set aside to come back later.

- **Classical KDFC, WMFO, WRBH Reading Radio, WHYR Baton Rouge and KFMG Des
  Moines play again.**

- **Live weather changes at the state line instead of following you across
  it.** (reported by Brandon)

- **A Florida landmark that no longer exists is off the road.** The
  Astronaut Memorial Planetarium and Observatory, destroyed in a hurricane,
  is gone from the map. (reported by Brandon)

- **Darren Duff radio is on the web radio band.** A rock station for the
  long hauls. (asked for by Brandon, with the stream address found by
  Shane)

- **Synthwave City FM is on the web radio band.** A synthwave station for
  the night runs. (requested by Sarah R.)

- **Radiostorm's four channels are on the web radio band under their own
  names.** At Work 104, Rock 104 Classic Rock, Oldies 104 and Comedy 104
  are named the way the station names them, each on the dial once.

- **A station listed under more than one address now lands on the dial
  once.**

- **You can shut the engine off while you wait at a facility.** A row in
  both facility menus shuts it down and turns into Start the engine; idling
  burns fuel. (asked for by a new player and Jake)

- **Elberton, Georgia is on the map, and it hauls granite.** Its docks are
  quarries and monument plants, reached from Athens, Augusta and
  Greenville. (requested by William)

- **The controller's left trigger, pressed all the way down, is now the
  emergency brake.**

- **A controller can answer the nodding-off warning.** Steering with the
  stick or pressing the left trigger now counts.

- **Pressing a key cuts the speech in progress again.** You get the new
  answer straight away, on the controller buttons too. (reported by Sarah
  R.)

- **The dial does nothing while the radio is switched off.** The dial keys
  say "Radio off" and leave the tuning alone. (reported by Darren)

- **Pulling out no longer drops you straight into a hazard.** A
  construction zone no longer starts around you before you have moved, and
  the opening miles of a run carry no merging traffic. (reported by Josh)

- **Learn game sounds has a Back option on both screens.** Leaving a group
  stops a sound that was still running.

- **The controller's Back button stops an announcement while it is being
  read.** When nothing is being said, Back still reads the controller help.
  (reported by Sarah R.)

- **Right bumper plus X reads the posted speed limit and how far over you
  are.** (reported by Sarah R.)

- **Four keys now answer one question each about where you are.** Alt 1 to
  Alt 4, or the keypad numbers, speak the state, the road, the town and the
  direction in turn. (suggested by Tim K.)

- **The Online menu can open your driver setup page for you.** Open my
  driver setup page opens it in your browser, or puts the address on your
  clipboard if it cannot.

- **You will sometimes hear a trooper run somebody else down.** A siren
  past you, the stop on the shoulder as you pass, and a trooper who has
  somebody stopped is not watching you.

- **That siren now says whose stop it is.** A short line tells you a
  trooper has somebody else on the shoulder, not you, and sometimes names
  why.

- **The scale ahead now has its own sound.** A heavy thump and a quick beep
  play when an open weigh station is announced.

- **You can now pay down what you owe from your own cash.** The terminal
  and every truck stop offer to pay all, half, or everything above a fuel
  cushion.

- **Every Freight Fate station now sounds like a real station.** Each
  regional station has its own host, sung jingles, station IDs and
  commercials for the road.

- **Four new stations join the dial.** Cruisin' Gold plays oldies, Glory
  Road plays southern gospel, Puro Tejano plays Tejano and Neon Drive plays
  synthwave.

- **The engine brake sound is now choosable.** A new Engine brake voice row
  in Settings, Audio offers recorded or classic, and switches even mid
  descent.

- **Radio volume now changes from the wheel.** Hold Shift with Page Up or
  Page Down, or Shift with semicolon or apostrophe, to raise or lower it.

- **You can hear where you are in the lane while you steer.** Holding a
  steering direction brings up the lane position cue on its own, stopping
  with a click when the exit lane is set.

- **Balanced and All assists now turn Facility stopping assistance on;
  Realistic leaves it off.** Changing it by hand reads as Custom.

- **Fuel counts toward your truck's weight.** Load offers and fuel menus say
  how much room remains under the gross-weight limit.

- **Worn brakes, tires, or engine can put your truck out of service.** You
  hear service warnings first; at the service limit the truck needs repair.

- **Rest advice names the last suitable stop within your hours.** You hear a
  warning before passing the last reachable stop, or that none is reachable.

- **The speech library behind every voice in the game is updated.** Microsoft
  Speech Platform voices now appear in the SAPI voice list, and two voices at
  once no longer trip NVDA.

- **Spoken lines are shorter everywhere.** Road cues, menus, readouts, help
  screens and settings descriptions say the fact and stop.

- **The Drivers on duty list no longer has a Refresh row.** It keeps itself up
  to date while it is open.

- **Curve speed assistance now slows the truck for a bend before you reach it,
  in every driving mode.** With curve callouts on, the call ends "Curve speed
  assistance slowing", and your own brake cancels it.

- **The hazard warning now tells you which lane is open.** It ends "Left lane
  open", "Right lane open", "Either lane open", or "Brake!" and "No lane
  open".

- **Facility stopping assistance now drives you from the ramp-end stop to the
  facility gate, hands off.** The cab says it is taking you to the entrance,
  and your own brake hands the truck back.

- **Bulk fuel now requires the hazmat endorsement alongside tank vehicle.**
  Book the hazmat course under Licenses and training; the background check
  runs while you keep driving.

- **Steel and lumber now ride on the flatbed securement certificate.** It is
  its own course under Licenses and training.

- **"Drivers board" is now "Drivers on duty".** The Online menu, the pause
  menu, the screen itself and the website all use the one name.

- **Detention, lumpers, washouts, and tolls now come out of an owner-
  operator's settlement.** Detention pays you and the rest charge you; a
  company driver's are billed to the carrier.

- **The dispatch board weighs each load against the 80,000-pound limit.**
  Your fuel counts toward it, and an overweight truck draws a Scale red
  light at a transponder scale.

- **Relaxed hours of service keep the same 11-hour driving time, 14-hour duty
  window, and 30-minute break as Realistic.** Fines and inspection odds are
  lighter.

- **A scale wave-through after you pull in takes a couple of minutes, not
  fifteen.**

- **A missed break costs thirty minutes on the shoulder, not a ten-hour
  reset.**

- **Standard pacing no longer multiplies roadside chatter.** Road cues still
  speak once per place, and CB chatter, weather color and roadside flavor no
  longer scale with the clock.

- **The truck says what kind of road you are turning onto.** A street the map
  has no name for is "a service road" or "a side street", never "unnamed
  public road".

- **A slow station is no longer written off as dead.** The radio waits longer,
  says "is slow to answer, trying again", and retries once before moving on.

- **Every exit now asks for its own ramp speed, and you may leave the highway
  at road speed.** The ramp's speed is what you hear in "slow to" and "stay
  under", and what adaptive cruise eases to.

- **A line cut off by urgent warnings finishes once, and only while it is
  still true.** A Scale green light no longer tells you to signal for the
  scale exit.

- **Owner-operators can see the weigh station transponder before they can
  afford it.** The Business status row reads as locked and says what it is
  waiting on.

- **Speech about the truck's own automation always arrives now.** An assist
  taking or releasing the pedals, "Jake off", or where you landed after a lane
  change waits its turn instead of being dropped.

- **Slow vehicles on the highway are traffic you catch and pass.** A slow
  vehicle you catch is spoken with its type, gap, and speed, and merging
  vehicles build back up to road speed.

- **How much traffic you meet now comes from how busy the real road is, hour
  by hour.** A rural highway at three in the morning is empty; at five in the
  afternoon it is not.

- **Realistic is retired from the Driving mode row; Standard is now the
  quickest pacing.** A save set to Realistic is on Standard now, and the row
  says so the next few times you open it.

- **The speed keeper no longer promises a speed the road will not let you
  reach.** On a short facility approach it holds the street's own limit
  instead of the gate's crawl.

- **No more stop signs where one interstate meets another.** A ramp onto
  another interstate merges.

- **Your career readout says how much experience the next level needs.** It
  says "two hundred more to level four" right after the level. (asked for by
  Brandon)

- **Lane guide sound, a new Audio settings row, switches the lane guide to a
  soft note.** Off by default; in Learn game sounds as "Lane guide tone".
  (thanks to Darren, who sent in the sound)

- **The R key no longer repeats the turn you were just given.** On city
  streets it answers where you are and how far to the gate.

- **Shift+R is gone.** Plain R answers route status whether or not you hold
  Shift, and the next exit stays on the status screen.

- **On a controller, the right bumper with D-pad up answers route status.**

- **The police marker no longer turns the radio down when "game sounds step
  back for speech" is off.** The marker still sounds at full volume.

- **Confirmations no longer play the Hazard clear sound.** They have their own
  short note, listed in Learn game sounds as "Confirmation note".

- **On a one-lane road the hazard warning says "Brake" again at the quieter
  settings.**

- **The cruise dial answers with just the number at Quiet and Urgent only.**
  Tapping the speed up or down says "62" instead of a full sentence.

- **A key you press always answers, whatever your driving speech setting.**
  The reply gets shorter at the quieter settings but never goes missing.

- **Traffic warnings have a short form at Quiet.** "Exit traffic, 2 miles.
  Hold right, 45", and the merge and traffic-pack warnings shorten the same
  way.

- **Urgent only now turns heads-up lines into a short sound.** What stays in
  words is the safety calls, what things cost, and the directions you cannot
  take back.

- **The new sound is two short notes falling, listed in Learn game sounds as
  "Road ahead note".**

- **Nothing ahead of you is ever "in 0 miles" any more.** Anything under half
  a mile counts down in quarter miles and then says "just ahead".

- **Being stopped short of a stop bar is always said out loud.** The line
  telling you to drive up and close the gap waits its turn instead of being
  dropped.

- **Roving patrols can pull you over now.** A trooper running with traffic
  falls in behind you and clocks you over a stretch of road.

- **The enforcement presence setting is gone; the road decides how much police
  activity you hear.** An empty enforcement post is silent now, so a police
  car going by can act.

- **The overspeed warning no longer chimes at you for adaptive cruise's own
  speed.** The Overspeed warning setting is gone; there is nothing left to
  turn off.

- **Speed keeper moved from Controls to Driving assistance.** If you have
  played before, the Gameplay menu tells you once where things went.

- **Lane and edge cue prominence is now Lane and edge cue volume, in Audio
  under Gameplay cues volume.** Its values are quieter, standard, and louder.

- **The Driving assistance help no longer promises lane centering that does
  not happen.** Lane centering assistance now says plainly that it makes no
  difference to how the truck steers today.

- **Coming up on your exit no longer slows you to a crawl miles out.**
  Automatic speed control holds road speed until the exit is close, and the
  delivery approach works the same way.

- **Your roadside chatter switches now work at the quieter speech settings.**
  Leave a kind on and you get it, kept short: the river's name, the park, the
  billboard itself.

- **Traffic at exits you are not taking has gone quiet.** The exit traffic
  call comes only for an exit you have signalled for, or one lane keeping is
  taking for you.

- **A speed limit drop now says why, when the road knows.** It names the town,
  a weigh station just ahead, or a downgrade starting there; otherwise it
  stays the plain number.

- **Pressing Escape now acts instead of explaining.** At the terminal it takes
  you straight to the main menu; at the main menu it asks whether to quit,
  then does it.

- **Blowing past an open weigh station can now get you pulled over.** Caught,
  it is a citation on the spot, more with priors or in roadwork, plus a full
  roadside inspection.

- **The truck dealer opens straight from the terminal menu.** The drive to
  city services is retired; fuel, repairs, rest, and food stay at truck stops
  and the terminal garage.

- **Routine road announcements now keep a few seconds apart, in every driving
  mode.** Warnings that need your hands never wait.

- **Radio stations hold their signal like real ones.** A station plays clean
  through most of its range and only smears into static at the edge.

- **Clearance and weight signs now say what they mean.** "In 13 miles, a low
  bridge, signed 13 feet 6 inches. Your route clears it."

- **Cruise speed now steps onto the fives.** From 32, plus gives 35, then 40;
  the controller's cruise buttons step the same way.

- **Holding Control with cruise plus or minus changes the target by exactly
  one mile per hour.**

- **The engine now sounds like you are sitting in the cab, not standing beside
  the truck.** Contributed by Noel Romey
  ([@nromey](https://github.com/nromey)) in [PR #162](https://github.com/orinks-games/Freight-Fate/pull/162).

- **Running off the pavement no longer talks in a loop.** It speaks when it
  happens, again if it gets worse, and once more when you are back on the
  road.

- **Facility names are said in full the first time on a leg, then kept
  short.** A pickup no longer says "cross-dock Chicago Cross-Dock" over and
  over.

- **The game stops repeating key prompts you have mastered.** "Press E to
  start the engine" falls away after a few uses, and returns if you remap the
  control or switch to a controller.

- **The Learn game sounds screen now lists the Collision sound, right after
  Hazard clear in the Hazards group.**

- **A worsening load stops repeating the same advice.** Later warnings speak
  only the new figure and what it will cost at the dock.

- **Achievements stay out of your way while you drive.** Earning a badge at
  speed is its sound and its name; the full write-up waits in the message log
  and the achievements menu.

- **The first drive no longer buries you in badges.** Your first dispatch, air
  pressure, and first trailer fold into one "First Day" badge, and the rookie
  chain spreads across your first runs.

- **The delivery summary skips the rows that say nothing.** No new damage, an
  undamaged truck, a healthy tank, and no new messages are no longer read out.

- **Gameplay settings now open a submenu of four screens: Driving assistance,
  Difficulty and hours of service, World and traffic, and Controls.** Speech
  and weather is now just Speech; its weather, traffic, and parking sources
  live in World and traffic.

- **The speed keeper and lane keeping rows each appear once now.**

- **At the quieter speech settings the truck says what to do and what it cost,
  and nothing else.** Traffic coaching shrinks to "Brake lights, 2 miles, 38",
  and a charged toll is "Toll, 15 dollars, carrier".

- **Road announcements no longer cut each other off unless it is a real
  emergency.** Speed zone entries, checkpoint notices, and traffic warnings
  wait their short turn; only act-now calls interrupt.

- **While you drive, screen reader speech waits its turn too.** Achievements,
  assist notices, and status key answers queue instead of cutting your screen
  reader off; menus still cancel speech instantly.

- **Out-of-date roadside chatter is dropped instead of barging in.** It is
  kept in the message log if you want to read what you missed.

- **The game starts noticeably faster.** Reaching the main menu is quicker on
  every launch.

- **Chaining very short hops no longer fast-tracks a career.** The on-time
  streak bonus is capped at what the miles themselves teach.

- **The radio plays its full dial out of the box.** The two radio stream
  settings are now one switch, streamer-safe mode, off by default; on, the
  radio keeps to built-in safe stations.

- **The terrestrial category now lists the strongest signal first.**

- **Turning the radio on lands on a station that plays clean.** If the station
  you left it on has gone to static, it retunes to the strongest signal
  around.

- **The radio draws power from the engine.** It cuts off when the engine shuts
  down, comes back when it starts, and a radio key in a dead cab says "The
  engine is off. The radio has no power."

- **Merging traffic is now announced without a speed to slow to.** The call is
  "Hold your lane and leave a gap".

- **CB chatter now reports work zones and the scale, not only troopers.**
  Drivers on the radio talk about a trooper working a work zone and logs
  being checked at the scale.

- **Selecting reverse no longer speaks a line on top of the reverse beep.**
  Coming back out to a forward gear still says so.

- **Real-world traffic reports are spoken as a live road report.** With
  Traffic source set to real time, they describe the real road today and do
  not change the one you drive.

- **The semis out there are governed now, so you can get around one.**
  Heavy trucks vary a little from one another; cars still pass at their own
  limit.

- **Less talking on the way down to a stop sign or a light.** On standard you
  get two distance calls instead of four, as bare distances like "one thousand
  feet".

- **On quiet, the second stop bar call lands where the tick starts, so the
  words hand you over to the beeps.**

- **Connecting your orinks.net account now turns Profile sharing and
  cloud backup on.** Each stays a single row on the Online menu whenever
  you want it off.

- **Owner-operators now start with a brand-new truck.** A full tank, no
  damage and nothing worn.

- **Driving through the barrels in a construction zone is now a fine and a
  serious mark on your record.** Once per zone, and never where the road
  left you no open lane.

- **Fines now match what they cost a real trucker.** Nearly every penalty
  except speeding rises.

- **A fine inside a construction zone is doubled, and every prior citation
  raises the next one.** You are told when a fine was doubled, and every
  spoken fine says the amount taken.

- **You never pay a fine into a hole.** A fine you cannot cover becomes a
  balance owed, and part of every settlement still reaches you.

- **New careers start on the Realistic preset.** Lane keeping starts off, so
  you hold the lane and take your own destination exit; the Lane keeping row
  in Driving assistance settings changes it.

- **Lane drift is now called Lane keeping, with the values full, partial
  and off.** Your saved setting carries over to the value that behaves the
  same.

- **With Lane keeping on full, the truck says so the first time it sets an
  exit lane and the first time it takes your destination exit.**

- **The owner-operator start now begins at level one.** Buying in changes
  who pays, and your level stays where it was.

- **Three new keys answer one hours question each.** Alt A says how long
  you have been at the wheel, Alt S when your break is due, and Alt D how
  much driving time and duty window you have left.

- **C now ends with only the limit that matters next.** On a controller,
  the clock button still reads the full report.

- **Street corners now ask you to slow down for them.** The approach names
  the turn, the street, the distance and the speed to be under; arrive too
  fast and you loop back through a safe turnaround.

- **The road lean now follows you through turns and exit ramps.**

- **The radio dial moved to Page Down and Page Up.** Page Down tunes the
  next station, Page Up the previous, Control still jumps a category, and
  semicolon and apostrophe keep working.

- **Every Freight Fate music station now plays everywhere.** The game's own
  stations no longer fade past their home cities, and they play in
  streamer-safe mode, in the Freight Fate stations category.

- **Turn signals play a clear tone instead of a soft click.** The blinker,
  the exit signal, and the pull-over signal use an indicator tone panned to
  the side you are signaling.

- **Truck status now explains whose truck you are in.** Company drivers hear
  which carrier fleet their assigned truck comes from, and junior drivers
  hear that they slip-seat from the yard's spare tractors.

- **The driving school steps out of this release to finish training.** The
  Driving school item leaves the terminal menu until the next major version.

- **Comma and period review your messages while driving.** From the cab
  they walk the message log, kept by category; everywhere else they still
  repeat and step through recent speech.

- **The R key now answers just "where am I".** How far along you are and how
  far is left, then the road, its direction, the state, and the city ahead.

- **The nearest town, the grade, the zone, and the next maneuver left the R
  readout.** Each has its own key or lives in the Tab status menu.

- **Shorter driving readouts that fit a braille display.** The clock, route,
  weather, and fuel reports put the answer in the first few words, and C
  leads with the time and whether you are on schedule.

- **Rest stops no longer let you sleep twice for nothing.** Already fully
  rested, a sleep option warns it would only move the clock and your
  deadline forward and asks for Enter again.

- **A truck parked with nothing changing for half an hour takes you off duty
  on the drivers list.** Roll again or pull into a stop and you are back on
  within seconds.

- **Deadhead drives now say how far along they are, on the drivers list and
  in Discord.**

- **The streets into town speak their real speed limit now.** Heading in to
  a customer or a stop, each approach street's posted limit comes from the
  real road.

- **Gear changes are quicker, like a modern automated box.** Power upshifts
  are faster; downshifts keep their rev-matched pace.

- **The game is about a third smaller to download.** The music uses a more
  efficient format that sounds the same.

- **Dispatch stops handing you the same job over and over.** Assigned
  dispatch leads with a load to somewhere you have not just been, whenever
  the board has one.

- **Kilometer mode stays kilometers everywhere.** Every spoken distance and
  speed follows your units setting, and one mile is "1 mile", not "1 miles"
  (thanks to the forum for the report).

- **The player manual caught up with the alpha.** New sections cover mountain
  and winter driving, truck-stop meals and showers, wear meters, chatter
  switches, and every new key.

- **Ramp endings are announced early.** When you signal for an exit, you
  hear how the ramp ends, like "The ramp ends at a stop sign", with a mile
  still to plan.

- **The clock drops to real time on a ramp that ends in a light or a stop
  sign, until you are through.**

- **Latch the brake and give your hands a rest.** Tap the brake, then press
  and hold half a second to latch it; the same key or the accelerator lets it
  go.

- **Turn latching off under Settings, Driving assistance, Latching brake.**

- **One key now answers "how fast should I be going?"** Press D while
  driving for a single safe-speed number for right now, with weather, the
  grade and the next bend already in it.

- **Career stats now list your endorsements.** An Endorsements line on the
  Career stats screen shows which you hold, any time.

- **Real streets reach the whole map now.** Most home-terminal yards start
  every job with spoken turn-by-turn directions on named local streets.

- **Hundreds more facilities now start and end on real city streets.**
  Deadheads and final approaches at many more warehouses, cross-docks, and
  company yards speak real turn-by-turn directions.

- **Truck stops now track loyalty points and rewards.** Fueling earns points
  by stop type, and a new loyalty menu at each truck stop redeems them for
  showers, parking, food, and laundry discounts.

- **Live road reports can be announced while driving.** Turn it on in
  Settings, Gameplay, to hear accidents and construction zones reported
  near your route.

- **Construction zones ahead now come from real state road reports.** Where
  a state provides them, real roadwork appears where it really is, with its
  real speed reduction; elsewhere the game makes its own.

- **Live construction zones now cover twenty-four states.**

- **Blowing a bend's advisory now costs you.** Enter a bend well above its
  advisory and the truck is shoved toward the outside, with a spoken slip
  warning as the tires complain.

- **Curve speed assistance can ease you down before the bend, and adaptive
  cruise drops out with a spoken reason when its set speed is too hot for
  the curve ahead.**

- **Live truck parking availability is announced at stops.** Turn it on in
  Settings, Gameplay, to hear how many spaces are open nearby when you
  arrive at a truck stop.

- **Truck stops now offer more amenities.** CAT scales, laundry, game rooms,
  barber shops, premium wifi, check cashing, DEF lanes, and ATMs can be
  spoken at Pilot and Flying J stops.

- **Dispatch assigns your tractor now, and better equipment follows
  seniority.** Every new hire gets the same trainer-spec truck, then a
  better assigned truck at levels 4, 9, 13, and 17, spoken at settlement.

- **Ten new tractor models fill the fleet, and after the owner-operator
  buy-in the same models are yours to buy at the dealer.**

- **Thirty-one new achievements for the bigger map and the longer career.**
  Badges now mark career levels 5 through 30, the owner-operator buy-in,
  map-wide progress, and deliveries into cities the jukebox got to first.

- **The dash now warns you about your own speed, like a real company
  truck.** Over the posted limit, a chime and "Watch your speed" repeat
  until you settle back under.

- **The new Gameplay setting has three positions: on, urgent only, which
  keeps just the runaway alarm, and off.**

- **G speaks the grade under the wheels and what it is doing to the truck.**
  You hear the slope, how far it runs, and whether the engine brake is
  holding.

- **Comma re-reads the last spoken line, anywhere in the game.** In menus,
  in the truck, everywhere; A still replays the last route announcement
  while driving.

- **The in-cab radio picks up forty-three more real stations.** With real
  streams allowed, live local radio reaches many more cities and the wide
  public networks across the empty country.

- **The desert Southwest gets its real radio dial.** Six more live stations
  light up the Four Corners and border country, including KTNN, the Voice of
  the Navajo Nation, with real public streams on in Settings.

- **Winter tires are now sold at the garage.** They bite harder on snow and
  ice but wear faster; the truck readout and the garage say which set is
  mounted.

- **Snow chains and chain laws have arrived on the steep grades.** Buy a
  chain set at the garage, and chain up from the pause menu when a sign
  calls the chain law. Contributed by Noel Romey
  ([@nromey](https://github.com/nromey)) in [PR #75](https://github.com/orinks-games/Freight-Fate/pull/75).

- **Chains grind apart if you run them fast or on bare pavement.** Rolling
  into an active chain law out of compliance gets a warning and maybe a
  citation.

- **Freezing rain is now its own weather, and it is the one worth parking
  for.** It glazes the road far slicker than snow; the forecast and weather
  reports call it out, and live weather recognizes it.

- **Hydroplaning now depends on your tires as well as the rain.** Worn tires
  float at lower speeds; the truck calls it out, and steering and braking go
  soft until you ease off.

- **The engine brake can now break the drive wheels loose on ice.** The
  truck warns you when they slide, and the right move is a lighter stage or
  none.

- **Your truck now wears with how you drive it: tires, brakes, and engine
  each have their own meter.** Wear talks back in grip, braking, power, and
  fuel, and the truck status readouts speak all three meters.

- **The terminal garage now offers brake jobs and engine overhauls alongside
  tires, and your delivery summary tells you what each run added.**

- **Truck stops now sell more than fuel: meals, showers, and truck care.** A
  meal eases fatigue, a lube bay or tire rotation slows wear for the run,
  and your status readout says what is active.

- **The big-name truck stops along your route now fix your truck.** Love's
  and Speedco replace tires, TA and Petro also do brake jobs, and an engine
  overhaul still means the terminal garage.

- **The biggest map update yet: 100 new cities to pick up and deliver in.**
  New corridors reach the mountain West, the northern plains, the Great
  Basin, the Pacific coast, and Appalachia (thanks to nromey).

- **The All assists preset now sets Lane keeping to full.** The truck keeps
  its lane for you, and a tap of Left or Right changes lanes.

- **Speed limits follow the real road now, all the way across the map.**
  Every route carries its actual posted limits, so you hear the zones a
  mountain highway really steps through.

- **The career is a months-long arc now, and every level up unlocks
  something.** An extra load refusal, say, or a deeper dispatch board.

- **The engine brake now works like the real thing, in three stages.** It
  pulls hardest in a low gear and does little in top gear, so set your gear
  before the hill.

- **Brakes now heat and cool like real drums.** Dragging the service brakes
  down a long grade fades them; short firm applications with the engine
  brake carrying the load keep them cool.

- **Each truck now keeps its own condition.** Wear, damage, and fuel stay
  with the truck they happened to, so swapping tractors no longer carries
  them onto the next one.

- **Careers from 1.9 test builds load unchanged: your current wear settles
  onto every truck you own.**

- **Relaxed driving now leaves real breathing room without removing the
  truck.** Hazards are spaced farther apart, warnings allow more time,
  damage and fatigue build more gently, and routine speech is calmer.

- **The truck revs each gear out when you accelerate hard.** You hear the
  engine climb and pull through the gears the way a loaded truck should.

- **Truck speed limits now match each state's law.** Some stretches run
  slower than before and some faster.

- **Pressing S says when a speed limit is a truck limit.** You hear "Truck
  limit 55. California holds trucks to this" instead of the bare figure.

- **Driving speech now has three settings: Standard, Quiet and Urgent
  only.** The Driving speech row is under Speech settings.

- **Standard speaks every confirmation and status readout in words.** A
  driving tip is said once per leg, and a status readout repeats only when
  it changes.

- **Quiet turns tips, confirmations and status readouts into a short
  sound.** Safety calls, costs and route instructions stay as words.

- **Urgent only turns the heads-up lines into a sound too.** Safety calls,
  costs and the directions you cannot take back stay as words.

- **Everything the quieter settings stop saying is still in the message
  log.** Billboards, place names and landmarks keep their own switches.

- **If you were playing on terse you are now on Quiet, and normal lands on
  Standard.**

### Removed

- **The Shift and exit key hold to run from a trooper is gone.** Running is judged
  by how you drive after the final warning.

### Fixed

- **Quiet-mode confirmation notes get the same room as every other cue.** With Game sounds step back for speech on, cruise and stop confirmations no longer play against the full road bed.

- **Driving out of a station's range now says so and retunes the radio.**

- **Automatic speed control comes back on its own when a hazard leaves you
  below 20 miles per hour.** The speed keeper builds speed and hands to
  adaptive cruise; it used to stay paused with nothing said.

- **The manual says what ignoring a trooper's lights costs.** A forced stop
  and a serious violation; a felony only if you choose to run.

- **The percent there counts the whole run, streets included, and never
  says 100 before the gate.**

- **Control now silences the last-stop warning and wear warnings for good.**

- **Your public profile and cloud backup name the truck you drive now.**

- **Automatic speed control eases to the taper speed first and meets the work
  zone speed at the barrels.**

- **The game no longer uses up Windows a little at a time while it sits
  open.** A session that runs all day stays the size it started at.

- **A chain-law ticket now goes on your safety record.**

- **Being placed out of service now counts on your safety record.**

- **G announces an upcoming grade directly.** When a gentler hill or short
  steep stretch is ahead, the terrain report names it without first saying
  there is nothing steep ahead. Contributed by trssharp
  ([@trssharp](https://github.com/trssharp)) in [PR #189](https://github.com/orinks-games/Freight-Fate/pull/189).

- **Running the light or the stop sign at a ramp end can now cost you a
  ticket.** Cross traffic varies each time, and a trooper at the crossroad may
  write you up.

- **The S key no longer names a bend inside a highway connector.** It reads
  mainline bends only; the D key still gives the safe speed through the
  connector.

- **Convenience stores no longer pose as truck stops.** With a trailer on
  they are not announced and do not count as a rest stop, though bobtailing
  they still sell fuel.

- **Cruise stops slowing for a car you have left behind once you cross the
  lane line.** On the crossing it watches the lane you entered instead.

- **The dispatch board warns about a load that only just fits your hours.**
  The warning now counts the drive to the pickup as well, and Enter again
  still takes the load.

- **The clock key counts the whole run while you are still on the streets
  out of the pickup.** The arrival time and the on-schedule verdict now
  include the highway.

- **The speed readout says when a slow car is setting the speed keeper's
  number.** Space now says "holding 51 for the traffic ahead, set 58".

- **A long sleeper-berth rest no longer runs your duty window down while you
  sleep.** You wake with the window you went to bed with, and the wake-up
  line says when it closes.

- **A voice that cannot start no longer blocks the next one.** The game
  skips it and tries the next voice.

- **Bobtail trips to find work use your driving hours.** Stops during that
  travel count as on-duty time.

- **Braking estimates use the brakes each situation calls for.** Planning and
  hazard warnings allow for service braking and time to react.

- **The manual gives consistent speed-warning, police, and backup
  instructions.** It clarifies the U key, speeding charges, and where Linux
  saves are stored.

- **Speech recovery keeps its replacement voices connected.** Restoring your
  speech settings after a voice stalls no longer reconnects the stalled
  voice, and your event voice and braille only choice are kept.

- **Exit assistance slows early enough and gives the pedals back.** Ramp-end
  assistance no longer crawls toward a distant stop, and holding the
  accelerator overrides it (thanks to Tower, [@TowerAlphaTheta15](https://github.com/TowerAlphaTheta15), for the
  report and original fix in
  [PR #185](https://github.com/orinks-games/Freight-Fate/pull/185)).

- **Heavy traffic follows local road conditions.** Clear stretches stay
  clear, and a narrow bottleneck no longer slows the wider road around it.

- **Moving between equally slow traffic sections no longer repeats the same
  traffic warning.**

- **Windows downloads include the files needed to start the game.** You no
  longer need to install the Visual C++ runtime separately.

- **Speech comes back on its own when a voice locks up mid-drive.** The game
  abandons the stuck voice and starts a fresh one with your speech settings
  already applied.

- **The automatic now rev-matches its downshifts.** You hear the engine blip
  up to meet the lower gear before it takes.

- **The engine no longer sounds like it cuts out on every gear change.** It
  eases off through the shift instead of going silent.

- **The "slow down for the gate" warning comes once, on the last street.**
  It waits until you are coming up on the street the gate is on.

- **Route-transition assistance says "braking for the light" every time it
  brakes at the end of a ramp.**

- **Facility stopping assistance no longer crawls the whole ramp.** It drives
  the lane at its own speed and creeps only the last lengths to the
  entrance, and a safe turnaround starts a fresh approach and says so.

- **A load staged at your own yard no longer sends you on a drive to it.**
  The pickup opens straight away and you check in from there.

- **The speed keeper hands the pedals back at a facility gate instead of
  being looped for its speed.** The gate says it is handing off and gives
  you a moment to brake.

- **Rolling the last stretch to a gate a little quick after stopping short
  gets the gate's warning, not an instant safe turnaround.**

- **The rest key a truck length short of a gate now names the gate.**
  Pressing T there says how far ahead the gate is and to roll up to it.

- **The paperwork at a delivery quotes a company driver's wages.** The
  estimated net driver pay line now matches the figure the dispatch board
  quoted.

- **Adaptive cruise no longer surges on the way down to a lower limit.** It
  keeps aiming at a limit it has started slowing for until it reaches it.

- **"Pickup ahead" and "Destination ahead" are no longer repeated after you
  are at the gate.**

- **The route status distance and the toll estimate now read as their own
  sentence.**

- **Your choice of driving event voice now survives closing the game.**
  Picking a voice on the Driving event voice row no longer reverts the next
  time you start (reported by MariahL).

- **On a road with one lane your side, traffic behind you stays behind
  you.** A faster car or trooper falls in behind you instead of driving
  through the cab.

- **Adaptive cruise starts slowing for a lower speed limit early enough at
  every Driving mode.** It is at the new number when it reaches the sign.

- **Pulling onto the highway, the speed keeper builds speed down the
  acceleration lane again.** It drops the last street corner's speed the
  moment the truck is on the ramp.

- **Crossing a state line no longer asks the weather service over and
  over.**

- **Choosing Quit closes the game at once.**

- **Curve speed assistance still slows for bends with curve callouts switched
  off.** You hear nothing about the bend, but cruise eases for it, or pauses
  and comes back.

- **An announcement cut off in its last words is no longer repeated from the
  start.** Only an instruction cut early is said again.

- **Facility approaches no longer say "unnamed public road".** Nameless turns
  now say "a side street", like every other nameless road.

- **The first-run walkthrough no longer tells you to wait for air that is
  already up.** Once air is ready, it and the driving school lesson tell you
  to press the parking brake key.

- **A drive no longer opens with the traffic-slowing sound.** Entering a yard
  road, a facility gate, or a destination approach now plays the plain
  notice sound.

- **No more phantom hills at flat city gates.** Where the ground is level,
  the grade line and the truck's pull at the gate now match it.

- **A hazard stop, a missed turn, or a tight bend now pauses automatic speed
  control instead of cancelling it.** Cruise comes back at your set speed
  once you are rolling again, and K still switches it off.

- **On a facility approach the speed keeper takes the corners itself.**

- **The exit ramp length is read in your units.** Drivers using kilometers no
  longer hear "half a mile of ramp".

- **Turn callouts no longer sound like the turn is getting farther away.**
  The distance after a turn or merge now says "then 1 kilometer on it".

- **Ramp traffic now yields instead of forcing its way in front of the
  truck.** A slower vehicle waits beside the highway until you have passed,
  then joins behind you.

- **Manual facility arrivals now wait until you have secured the truck.**
  With Facility stopping assistance off, stop, set the parking brake, then
  press T to enter.

- **Facility stopping assistance's final prompt names Enter or controller A
  according to your controls.**

- **Roadside achievements now match where you stopped.** A weigh station or
  repair shop no longer earns Sweetheart of the Truck Stop, and a motel room
  never counts as sleeping in the bunk.

- **The destination and planned-rest-stop options are now one Facility
  stopping assistance setting.** It covers pickups, deliveries, planned rest
  stops and required weigh stations, and either old opt-in carries over.

- **Open scales give you real time to use their lane.**

- **Checking in at a scale cannot be repeated to advance the clock.**

- **The game no longer rescans every speech backend while it is open.** That
  eases the load on NVDA on lower-powered computers.

- **Closing the game hands control back to NVDA immediately on Windows.**
  Long or often Alt-Tabbed sessions no longer leave your screen reader
  waiting after sound has stopped.

- **Quitting while the truck is moving now asks first and tells you the
  cost.** It says what you would lose since your last stop, with Keep
  driving as the first choice.

- **The voice keeps up with the game again.** Newer announcements silence
  stale ones still waiting in line, so speech stays current and your screen
  reader stops feeling sluggish.

- **Closing the game hands your screen reader back right away.** Quitting
  drops whatever was left unsaid after the sentence already being spoken.

- **A radio stream that will not play lands on a live station.** The radio
  tries the next station on the same band, then AFN Humphreys The Eagle, and
  says so; streamer-safe mode still lands on the silent channel.

- **The drivers list names the truck you are in.** A company driver's line
  and Discord now show the assigned truck instead of the old yard mule.

- **Updating the game works now.** The download keeps going as long as it
  makes progress, the game announces every quarter, and Escape still cancels.

- **The Mac build starts again.** Everything the game needs is built into the
  app, so it no longer depends on anything installed on your Mac.

- **The game answers your keys the moment it starts.** The sound device opens
  in the background while the menu comes up, and your volume settings kick
  in once it is ready.

- **Heavy-traffic jams now clear when the slow zone does.** Jam traffic
  disperses past the end of the zone and the road opens back up.

- **The approach countdown counts down again after you take your destination
  exit.** Off the highway, R now counts the approach itself down, half a
  mile, a quarter mile, then feet.

- **A stuck screen reader can no longer freeze the whole game.** If the voice
  stops responding you lose a few sentences, the truck keeps driving, and
  speech comes back when the voice does.

- **The first-day briefing says your city's name properly.** It says "the
  Chicago service area" instead of the map's internal name.

- **Real time driving no longer leaves the road empty for minutes at a
  time.** A vehicle now enters ahead in your lane, where the traffic warning
  and status readout can identify it.

- **Corridor billboards that were lying got pulled or moved.** Wall Drug
  stays on South Dakota and the Minnesota approach, the Meramec caverns sign
  now sits on Interstate 44 in Missouri, and a handful of radio-memory
  tributes are gone.

- **Route-transition assistance no longer rides the service brake down a
  routine ramp.** It eases off the throttle for the ramp speed instead of
  draining the air tanks.

- **The throttle key no longer latches.** Holding the accelerator never
  catches a throttle latch, and pumping the pedal out of reverse works with
  a normal hold.

- **The Latching pedals setting is now Latching brake, on or off.** An older
  value that was on stays a latched brake, and off stays off.

- **The Scale green light and Scale red light sounds play at the weigh
  station again.**

- **The Back in the lane chime waits until the truck is actually centered.**
  With lane keeping off, a manual correction gets one confirmation only once
  the truck is near lane center.

- **Traffic status now names the vehicle it is describing.** The Route screen
  and Driver Traffic app say car, semi, box truck, service vehicle, or
  trooper instead of "lead vehicle".

- **Acceleration-lane assistance keeps pulling until cruise can take the
  truck safely.** The speed keeper uses full throttle down the on-ramp, then
  hands to adaptive cruise once traffic speed is reachable.

- **A red light on the destination ramp no longer leaves the truck crawling
  after green.** Route-transition assistance handles the light first, and
  Facility stopping assistance slows again only when the entrance is close.

- **A slow loaded truck gets a safe highway handoff after an on-ramp.** Speed
  assistance stays on the real-time clock until the truck is close enough to
  merge.

- **A start with no game sounds now tells you so.** The main menu says once
  that game sounds could not start and you will hear the voice but no
  engine, traffic, or alerts.

- **After a missed destination exit, the game no longer asks you to signal
  for an exit lane keeping takes itself.** With lane keeping on full, the
  safe turnaround says to hold the right lane and slow down.

- **The speed warning stays on after you miss your destination exit.** It
  works the whole way round the safe turnaround (reported by Tyler Rodick).

- **Missing your destination exit no longer sends you hunting for an exit
  that is not there.** The cab now says to carry on to the safe turnaround,
  and the exit comes around again.

- **Freight Fate's own stations keep playing while you are tuned away.**
  Tuning back finds the station wherever it has got to, not the top of its
  song list (reported by Marie).

- **Receiver unloading no longer turns an on-time arrival into a late
  delivery.** The appointment, pay, and on-time bonus use the time you
  checked in at the receiver.

- **Traffic stops crawling in front of you for a jam that is not there.**
  Once the congestion is behind it, a vehicle comes back up to road speed
  and its warning stops.

- **"Descent control cannot hold this grade" is only said when it cannot.**
  It waits until the truck is getting away, and the warning and the G
  readout now agree.

- **The automatic engine brake waits for a hill that needs it.** On
  a shallow descent the service brakes hold your speed, and once on it stays
  on until the road is level again.

- **The engine brake lets go the moment the hill runs out.** On a climb it
  comes off altogether so the truck can build speed, and it no longer keeps
  two cylinders cut on the flat.

- **The automatic engine brake works to the cruise set speed, not the speed
  you were doing when you armed it.**

- **Billboards now stand where the sign says they do.** A sign that names a
  town or region only appears near it.

- **The speed keeper holds the posted number up a hill, and says so when it
  cannot.** Where the hill beats a loaded truck it says once that it is flat
  out and cannot make the number.

- **On a road with no way past, the truck brakes sooner and harder.**
  Automatic braking on a single lane no longer waits out the time a lane
  change would take.

- **Debris on a wide road can be gone around now, and the warning says so.**
  Where a lane is open the warning names it, and you can move over.

- **The truck no longer brakes for a stop it does not have to make.** The
  warning about a vehicle ahead now gives you the time that slowing to its
  speed needs.

- **Traffic drives the speed of the road it is on, so far fewer slow vehicles
  are in your way.**

- **No more phantom brake lights.** Traffic only brakes where the road gives
  it a reason: a jam, roadwork, or a ramp.

- **Braking down behind a slower vehicle is confirmed as slowing to match
  it.** Swerving into an open lane still has its own line.

- **The traffic placed along your route when a run begins takes an exit like
  everybody else.**

- **Nobody passes you in a lane the road does not have.** Overtaking traffic
  uses the lanes the road really has.

- **You get the whole on-ramp to build speed when you pull out of a yard.**
  The merging lane now takes the time it really takes.

- **Facility stopping assistance no longer brakes you on the way out of a
  yard.** It leaves a departure alone and slows only on the way in.

- **Facility stopping assistance stops you at the gate on every delivery.**
  The last city streets no longer go by faster than the brakes can answer.

- **An automatic dock pull-in no longer also tells you to press Enter.** The
  arrival says the dock menu is opening and leaves it there.

- **Driving keys now work with JAWS without the pass-through key.** Hold Up,
  Down, Left, or Right to drive, the same as everyone else. Contributed by
  Noel Romey ([@nromey](https://github.com/nromey)) in [PR #167](https://github.com/orinks-games/Freight-Fate/pull/167) and
  [PR #168](https://github.com/orinks-games/Freight-Fate/pull/168).

- **Through JAWS, letting go of a pedal or the wheel lands a moment late, and
  the double tap and hold that latches a pedal cannot be caught.**

- **The drive costs the computer far less, so speech stays prompt on a slower
  machine.**

- **Quitting the game no longer stalls for two silent seconds with Discord
  running.** Turning Discord status off under Settings, Online, no longer
  freezes the game either.

- **The speed key and the status screen tell you the speed adaptive cruise is
  holding.** You hear "adaptive cruise holding thirty-three for the ramp,
  set eighty" until nothing holds it down.

- **A turn or exit instruction cut off by the next one is read straight after
  it.** A line you were already hearing still gives way, and one no longer
  true is dropped.

- **No black terminal window beside the game any more.** The game starts as a
  single window.

- **Your own career stops being called a copy from another computer.**

- **A career already marked as modified this way stays marked until you back
  it up online and restore it, which lifts the mark.**

- **The same jam is not waiting in the same place every single run.** A
  stretch that only just backs up on an average day can flow freely on a quiet
  one.

- **Roadworks that land near a jam are moved instead of cancelled, so a run
  with traffic can still have roadworks.**

- **Twenty-four legs go back on the interstate they are named for.** Chicago
  to St Louis, Knoxville to Atlanta and Albuquerque to Phoenix are among them,
  and everything you hear along them matches the new road.

- **Those twenty-four legs pay and take differently now, because their
  distance changed.** A leg you know well quotes a different rate and delivery
  window.

- **Ten legs stop calling themselves an interstate they never touch.**
  Winston-Salem to Greensboro is US-421, Evansville to Nashville is US-431 and
  Allentown to New York is I-78, among others.

- **The interstate stops warning you about bends that are not on it.** Ramps,
  business routes and town streets are no longer called out as highway curves.

- **Asheville to Hickory is now called a mountain road.**

- **Five runs are named for the road you are on.** Hickory to Charlotte is
  now NC-16 and West Palm Beach to Cape Coral is SR-80, among others.

- **Legs that were booked shorter than the road really runs now carry their
  true distance.** Expect them to pay more and be given more time.

- **The Pigeon River Gorge is called a mountain road again.** I-40 between
  Asheville and Knoxville and I-70 through Glenwood Canyon now say what they
  are.

- **Bends are judged against the road you are on.** You hear the corners on
  roads like US-231 again, and never a town square called as a highway
  curve.

- **A hot exit ramp warns you once.** The line that survives names what the
  assist is braking for.

- **The delivery clock now counts the bends you have to slow for.** A mountain
  run like US-550 over Red Mountain Pass is given the driving time it takes.

- **A tank load is described in tank words at the dock too.** The receiver
  uses the same words the road did.

- **Facility stopping assistance delivers the truck all the way to the
  gate.** It rolls the last few lengths and stops there, so the pull-in opens
  on its own.

- **T plans the next sleep stop however far ahead it is.** Inside signalling
  range it tells you to press X; further out, it tells you to wait for the
  exit call.

- **The speed readout tells you what the speed keeper is holding.** You hear
  "speed keeper holding fifteen for the corner, set twenty-five" when the two
  differ.

- **Coming off the ramp onto city streets, you hear the first corner and the
  speed keeper takes the streets at once.** The off-the-ramp line carries the
  first turn in the same breath.

- **The destination ramp no longer stops you a mile short of the gate when
  city streets follow it.** The assist waits for the real arrival at the end
  of the streets.

- **Adaptive cruise no longer pauses on the run-in to an exit with nothing to
  brake for.** Exit speed assistance leaves cruise holding until it has work.

- **Facility stopping assistance now stops the truck at the facility, and
  says so when it starts.** It takes the pedals a block out and holds them to
  the gate.

- **City turns give you time to hear them again.** Every corner on the streets
  into a facility takes its real seconds, so turns arrive one at a time.

- **You can always get out of reverse again.** Holding the throttle at a
  standstill shifts you forward now, even after a tap, and the pedal latch
  steps aside.

- **A trooper stops fining you for a gap the assists are managing.** Following
  too close only counts against you when you are the one on the throttle.

- **Construction warnings stop telling you to brake for something eight miles
  away.** The advance warning now opens with the distance: "in eight miles,
  construction ahead."

- **Brake lights ahead no longer make you stop dead.** A vehicle ahead clears
  once you are down to its speed.

- **The upcoming readout stops saying "in 0 miles".** U reads a decimal now,
  like "facility gate in 0.4 miles".

- **No more "Limit." with nothing after it at a ramp end.** On quiet, the
  ramp-end light call says the limit when it has one and stops cleanly when it
  does not.

- **Braking on a hill no longer lowers your cruise speed for the rest of the
  run.** You hear "descent control holding X for this grade," and your set
  speed returns when the road levels out.

- **Fewer needless slowdowns for interstate curves.** Curves are read with the
  bank the road is built with.

- **"Hairpin" now means a hairpin.** Only a switchback gets the word; a tight
  corner taken slowly is a sharp bend.

- **Curve speed assistance slows for the whole chain of bends.** It holds the
  slowest speed in the chain until the last bend is behind you.

- **You can get past a box truck now.** It no longer runs at a loaded semi's
  governed speed.

- **Closing the window asks first, instead of taking your drive with it.** Alt
  F4 or the close button raises the same yes or no question Escape does; press
  it again to close without asking.

- **Billboards and roadside signs read in full on quiet.** If you find them
  too talkative, the billboards switch turns them off outright.

- **Career stats now tells you what you are driving and what earns the next
  truck.** It names your tractor and its fleet, and what is holding better
  equipment back or which level earns the next.

- **Canceling a planned stop gives you the road back straight away.** The
  drive picks its pace back up the moment you call the stop off.

- **A curve call no longer comes back after the bend.** A cut-off curve call
  returns only while the bend is still ahead and you are still too fast for
  it.

- **The message review filter stays where you put it.** Review now tells you
  how many newer messages sit outside the category you picked.

- **Texas billboards stay in Texas.** Song tributes that name a real place
  have moved to the road they are about.

- **A trooper stop no longer repeats its demand after you have pulled over.**
  The pull-over line and the final warning only speak while the stop is
  unresolved.

- **The speed warning stops nagging once you have slowed down.**

- **"Press Enter to continue into the facility" stops asking once you have
  pressed it.**

- **Moving over to avoid something no longer drags it into your new lane.** A
  hazard stays in the lane it was in, so you are not told to change lanes
  again.

- **A dodge call no longer comes back after you are already clear.** A hazard
  line only returns while the hazard is still live.

- **A cut exit instruction no longer comes back after you have passed the
  exit.**

- **If the yard hands you a lesser truck than your level earned, taking a load
  tells you why.** It names what is held back and the exact thing that gives
  it back to you.

- **Pulling out of a warehouse or yard now gets you a real run-up to the
  highway.** The on-ramp now has an acceleration lane, and the speed keeper
  drives it up to road speed.

- **If you are still under the speed of traffic when the merging lane runs
  out, the truck says so and tells you to take a big gap.**

- **A facility approach posts one speed limit, and changes it once, at the
  gate.** The access road holds one limit the whole way in; the fifteen at the
  gate is the only change.

- **"Slow to fifteen for the gate" is now a limit your truck really has.**
  Once you have taken your exit, the assists slow for the gate's fifteen.

- **A heads-up about the limit ahead no longer talks over the limit you just
  entered.** The warning waits a moment for the arrival to finish speaking.

- **Pressing the rest key on a scale ramp sends you to the scale, not to
  bed.** On the ramp it says to stop at the scale first, then press it to
  check in.

- **Heavy traffic moves at the speed the warning promised.** Traffic in a jam
  settles at the zone's number, so the truck does too (thanks to Brandon).

- **Experience to the next level now reads in the driving status browse and
  on the career stats screen.** (thanks to Brandon)

- **The Back to the Holler billboard explains itself.** It says what a holler
  is and what the song is before the joke.

- **The all-clear after a dodge is spoken again.** "You swerve around the
  brake lights. Well done." and "Clear of the semi. Right lane open." wait
  their turn and always speak.

- **Brake lights say why, when the road knows.** A slowdown in a work zone or
  a rated jam names its cause: "Road work is the cause."

- **The animal you brake for has a name too.** A dog, a coyote, loose
  livestock, or a raccoon, alongside the deer and elk.

- **Road debris says what it is.** A ladder, loose lumber, a mattress, spilled
  boxes, a shredded tarp, or tire retread, named on the warning and again when
  you clear it (thanks to Brandon).

- **A delayed stop announcement says the distance that is true when it
  speaks.** A waiting notice stays silent if you have already passed the stop.

- **Adaptive cruise says the speed it will hold after traffic clears.** In a
  zone you hear "resuming at 20 miles per hour through the heavy traffic."

- **The Five-by-Two and Out badge means the bunk again.** Motel nights no
  longer count; a night in your own sleeper does.

- **Weigh stations work like real weigh stations.** Stay at road speed and
  signal for the scale exit; the ramp brings you down to the scale.

- **Pulling into a scale announces an inspection station, not a truck stop.**
  It no longer offers a motel room or reads a loyalty points balance.

- **The scale's ramp no longer invents a stop sign or quotes the highway's
  limit.** Ramp-end callouts everywhere skip a limit the road ahead has none
  on record.

- **Cruise control only uses the engine brake on a real downgrade.** It no
  longer raises it on flat road or on a climb.

- **One imported radio station stopped spelling its web address.** The station
  announcing itself as TheRadioStorm dot com is now The Radio Storm.

- **The speed keeper settles at a lowered limit instead of surging past it.**
  It coasts to the new number, and starts slowing early enough to arrive at
  it.

- **Exit ramps end at what the map's real road connections say is there.** A
  ramp straight onto another freeway gets a clean merge, with no invented
  light or stop sign.

- **Country exits no longer end at a stop sign far too often.** Rural
  off-ramps the map does not describe now mostly end at a light.

- **Facility stopping assistance now stops you at the door.** With it on, you
  stop at the arrival point and hear "stopped and holding, press Enter to
  continue into the facility".

- **The last half mile to a delivery no longer flies past.** The ramp down to
  your destination runs on the real clock, so you have time to brake.

- **Each engine brake setting plays one voice.** It keeps that voice all the
  way down a grade.

- **The engine brake voice switches over wherever you are in the rev range.**
  On the classic voice the growl no longer restarts each time the revs cross a
  band.

- **The assists take your destination exit again.** With lane keeping
  steering, the truck eases down for the ramp instead of holding highway speed
  through it.

- **Lane callouts never name a lane you cannot be in.**

- **The first instruction of a run cannot go missing.** "Merge onto I-70 west"
  and other act-now directions are always spoken.

- **A safety call is never talked over.** A warning still being spoken
  finishes behind whatever interrupted it.

- **A speed limit warning no longer stutters.** Approaching a lower limit
  announces the drop once, not twice.

- **Arrival and turn calls cannot go missing.** The dock check-in, the
  facility gate warning, the merge off a ramp and the hold at a red light are
  always spoken.

- **Most cars are just travelling now.** Merging traffic only comes from
  where a ramp feeds in, hard braking only where traffic is backed up.

- **Hazard instructions cannot go missing on a busy road.** The follow-up that
  a hazard is still in your lane, and the still-reversing reminder, always
  speak.

- **The truck always tells you why it will not move.** Pressing the
  accelerator against a set parking brake, or before the air is up, always
  explains itself.

- **Cues in quiet mode are no longer buried under the road.** Those sounds now
  get the same room the words would have had. (reported by Shane)

- **With game sounds step back for speech off, nothing steps back at all.**
  Not speech, not the quiet-mode sounds, and not the radio.

- **A ticket you have already paid no longer sounds like a second one.**
  Hearing an enforcement stop read out again now leads with the stop being
  settled.

- **The truck says why it is slowing down.** Adaptive cruise easing for a work
  zone, opening its following gap for weather, or handing back at stopped
  traffic always says so.

- **No more tickets for a following gap adaptive cruise chose.** A trooper
  needs the close gap held over a stretch of road, never while an assist is
  braking. (reported by Darren)

- **The Radio Storm was on the dial twice.** It is one entry now, and the four
  stations wrongly listed as Radiostorm go by their own names: Star104,
  Country104 and Christmas104.

- **A handful of station names cut off mid-word now read properly.**

- **Every lane has a name you can tell apart.** Roads now offer at most three
  lanes to drive in: right, middle and left.

- **State lines, billboards and lane counts stop going missing.** The road's
  quieter announcements queue and wait their turn, and a late one stays in
  message review. (reported by Sarah A.)

- **The truck went quiet on the standard speech setting.** Your speed, a limit
  change and the lane beside you coming open speak again whenever the number
  changes. (reported by Darren)

- **Exits now end at the control that is really there.** What waits at the
  bottom of an off-ramp is looked up for every state.

- **A few stretches of road were far steeper than any road really is.** Those
  grades are now held to what a road of that kind is built to.

- **A career waiting on your decision now says so on the menu.** Restore a
  cloud backup names the waiting career and says nothing backs up until you
  pick.

- **The "no" on a confirmation no longer sounds like the thing you wanted.**
  Every confirmation in Cloud backup now says "No, cancel and change nothing".

- **When you pick between two copies of a career, you hear what each one is.**
  Each choice names what it keeps: "Keep this computer's save and back it up:
  Bear Cub, level 4, 3,294 dollars".

- **The weigh station sound can be heard in Learn game sounds.** It plays
  louder and longer there than on the road, and the description says why.

- **You now hear when a career is backed up, not only when it is not.** After
  a save, the game says your career is backed up once the server has it.

- **Two backup refusals now tell you what is wrong.** A full set of backups
  says to remove one from the Cloud backup menu; a server fault says backups
  resume on their own.

- **The exit calls stop being talked over.** The exit ahead, the exit lane,
  the gore and lane keeping's exit notice now come ahead of ordinary road
  talk.

- **The ramp's traffic light stops going quiet on you.** The light changing,
  the distance to the stop bar and the braking notice now take priority over
  ordinary road talk.

- **The last call before your exit no longer says "in 0 miles".** Inside a
  mile you hear a quarter mile, half a mile, or one mile.

- **Other traffic now drives at the speed of the road it is on.** It keeps up
  on an interstate, slows through a town, and cars pass faster where the law
  lets them.

- **The speed alert stops dinging once you have slowed down.** It goes quiet
  as soon as you are back under the speed it armed at.

- **A refused backup now says when the server is the one that is behind.**
  Your career on this computer is safe, and backups resume on their own once
  orinks.net catches up.

- **The truck no longer builds speed for a hill the grade key says is not
  there.** The cue names the grade it is working for, and G names the same
  pull.

- **G now tells you when a grade you are already on is about to get worse.**
  It says the grade steepens, how far off that is, and how long it runs.

- **When orinks.net stops accepting this computer, the game points you
  somewhere that exists.** It walks you to the Online menu row that asks for a
  fresh activation code.

- **A one-lane road no longer calls itself the right lane.** L now says "In
  the lane", and the call that moves you when the road narrows stops naming a
  side.

- **Automatic speed control comes back on its own after a ramp's stop bar.**
  Once you have honored the light or sign and are rolling, it picks up with
  nothing to press.

- **Signalling for an exit early no longer slows you down early.** The truck
  holds road speed until it needs to slow, and the approach runs at real-time
  pace.

- **Exit speed assistance holds ramp speed all the way to the exit.** Brake
  and it stands aside.

- **Your own playlists work again, and can carry internet stations as well as
  music files.** They play in your order, and PLS is read alongside M3U and
  M3U8.

- **A playlist with nothing playable in it says so and names the Playlists
  folder.**

- **Opening the Radio status screen re-reads the Playlists folder.** A
  playlist added or repaired mid-drive shows up without starting a new run.

- **The route report gives you a real distance as you close on a gate.** R
  counts down in quarter miles and then in feet, right to the entrance.

- **On city streets, R names the street under your wheels, and leaving a gate
  it counts you down to the on-ramp.**

- **The coming-up key is shorter, and no longer reads out police activity.** U
  answers only the ramp control ahead, the next speed limit, the next stop and
  the next demanding bend.

- **Saving the game backs up your career right away and tells you how it
  went.** One line follows: backed up, already backed up, or what went wrong
  and where to fix it.

- **Saving with cloud backup switched off says so.**

- **A refused cloud backup now says so wherever you are.** The first refusal
  names the career and the reason out loud, and it says when that career is
  backed up again.

- **A highway narrowing to one lane tells you when it moves you.** You hear
  it, and are told which lane you landed in.

- **A road message that rings its chime always reaches you, spoken or waiting
  in message review.** Lane closures and work-zone warnings now wait in line
  instead of being thrown away.

- **Changing lanes no longer makes cruise slow down for the car you are
  leaving behind.** While the lane change is underway, the truck drives toward
  the lane you are entering.

- **Changing lanes no longer switches automatic speed control off; only
  braking does.** Cruise and the speed keeper ride through a dodge; your
  brake, the automatic brake or a collision ends the session.

- **When the server refuses a cloud backup, the game says so instead of
  blaming your connection.** A sign-in problem says reconnect; a flat refusal
  says so and points you at reporting it.

- **A refused cloud backup names the career and the reason.** A build mismatch
  reads as one, and a save whose numbers do not add up says it was flagged for
  review.

- **When two hazards stack, the all-clear names both.** "Past the deer and the
  slowed traffic. Well done."

- **"Lane open" now means open long enough to take it.** The lane call and the
  L readout look far enough ahead that the lane stays open until you are in
  it.

- **Choosing the classic engine voice brings back the original engine.**
  Classic on the Engine voice row under Settings, Audio always means the
  engine from earlier versions.

- **Resuming cruise to a high speed no longer floors the engine to get
  there.** Shift+K eases up to the set speed and lifts off before redline.

- **A quieter speech setting no longer silences your first-drive
  walkthrough.** First-run guidance speaks in full whatever your speech
  setting.

- **What something cost is now always said.** A toll, a fine or a citation is
  never dropped when the road is talkative.

- **A ten-hour out-of-service order now shuts the engine down.** If the air
  bled down while parked, it tells you to build pressure before releasing the
  brake.

- **A big delivery announces every rank it passes through.** Each rank gets
  its own "Level up" line, in order.

- **Running off the road asleep a third or fourth time now says so.** The line
  keeps counting instead of sticking at "twice now".

- **Docking and checking in no longer freeze the engine sound.** The engine
  settles to idle the moment you stop at a dock or gate.

- **A missed destination exit's loop-back now costs what it says it does.** It
  charges driving hours, fatigue and a sip of fuel, like the facility gate's.

- **A missed facility gate's loop-back now costs what it says it does.** It
  spends hours of service and fatigue and burns a little fuel, as the line
  always said.

- **Radio station names got their apostrophes back.** The Big Buffalo's Oldies
  reads naturally instead of with a stray broken s.

- **Important road announcements that get talked over repeat themselves
  instead of vanishing.** The urgent line speaks first, then the weigh station
  notice or planned stop announcement it cut off speaks again.

- **The weigh station announcement and the T key now tell the same story.** It
  tells you to signal for the scale exit, and to press the rest key once
  stopped at the scale.

- **Near an open scale, the rest key reminds you the scale comes first, and
  the exit key prefers the scale over a planned sleep stop.**

- **A pull-over stands down any armed exit.**

- **Turning streamer-safe mode on takes the station off the air at once.**
  The cab says so, and the radio lands on the Roadhouse.

- **The static dies with the station it belongs to.** The handover to the
  Roadhouse cuts the old station's hiss the moment it speaks.

- **The Skip on the Far End badge means what it says again.** It now takes a
  catch from beyond the station's normal reach, not any fading signal.

- **The speed keeper comes back up to street speed.** A new posted number
  hands it back up, and it says so: "Speed keeper holding 25 miles per hour
  through the facility access road zone."

- **The speed keeper no longer eases for a slower vehicle miles ahead, or for
  a drop a stretch too early.**

- **Automatic braking stops the truck on the service brakes, and stops
  draining your air.** One full application, held to the end of the stop,
  announced as "Automatic braking"; the emergency application now really
  happens.

- **A dropping speed limit is one announcement, heard in time.** The advance
  call comes early enough to act on, and the arrival confirmation stays quiet
  once you have heard the number.

- **Hazard warnings stop telling you to change lanes when there is no lane.**
  On a one-lane road, or beside a closed lane, the truck says "Brake!".

- **Rolling past the stop at your destination's entrance loops you back
  instead of stranding you.** You hear what happened and loop back through a
  safe turnaround, and cruise stays out until you have stopped.

- **The cloud backup list tells you how to fix a conflict.** A career whose
  backups stopped says opening it lets you choose which copy to keep.

- **Cloud backup no longer goes quiet after the cloud copy of a career
  disappears.** When nothing is left in the cloud to protect, the career's
  backups start over fresh with your next save.

- **One dip in air pressure is one warning.** The low air warning speaks once
  and stays quiet until pressure has properly recovered.

- **Driver name entry now has a caret you can steer.** Left and right walk the
  name a character at a time, Home and End jump to either end, and edits land
  at the caret.

- **The speed keeper no longer misses the second corner of a short block.**
  Every corner close enough to matter now bids, and the slowest one wins.

- **The engine settles to idle while you are pulled over, for the whole
  stop.**

- **Sleeping at a motel now shuts the engine off.** The wake-up message no
  longer tells you to start an engine that was already running.

- **Live weather no longer drops to neutral conditions when a weather
  station goes quiet.** The game asks the next-nearest station, or holds the
  last real weather it knew.

- **Date badges now follow the calendar you are told.** Christmas, New
  Year, Friday the thirteenth and the seasonal badges use the date you
  hear.

- **Automatic braking now stops the truck in time.** When the normal brakes
  alone will not make the stop, it uses the same hardest stop as the
  emergency brake.

- **A hazard warning now comes early enough to hear it out and act before
  automatic braking takes over.** If you are already changing lanes, the
  assist waits for you to finish.

- **The posted speed limit no longer flickers.** A limit that changes and
  changes straight back with no sign to explain it is gone.

- **Hairpin bends that no road could hold are gone from ordinary roads.**

- **A trooper who saw you no longer forgets because you were busy.** The
  pull-over comes as soon as the cab is quiet.

- **The reconnect advice now covers a driver account gone from orinks.net as
  well as a signed-out computer.** It tells you how to tell the two apart
  and what to do for each.

- **The assists no longer empty the air tanks stopping for a light or a
  stop sign.** Route transition assistance sets the brake once and holds it.

- **The speed keeper no longer stalls the truck on a downhill.** It takes
  one firm brake application and holds it.

- **When an assist cannot hold your speed, it says so.** You hear once
  that the assist cannot hold the speed here and you need the service
  brakes yourself.

- **Low air warnings no longer give parked-truck advice to a moving
  driver.** Rolling, you are told to get stopped and let the compressor
  build.

- **The speed keeper now slows down before a street corner instead of at
  it.** The corner call tells you when the keeper is taking it.

- **A construction zone can no longer close the only lane you have.** Work
  zones only close a lane where the road has another to give you.

- **Pulling into a weigh station no longer counts as driving past it.**
  Taking the scale's own exit is no longer fined as a bypass.

- **A fine you have already paid is not charged again when you come back.**
  A settled roadside stop stays settled, speeding tickets included.

- **The highway has traffic on it now, and you can hear it go by.**
  Vehicles come up behind and overtake, and you hear each one go by on the
  side it passed.

- **Traffic no longer thins out because of your hazard or Driving mode
  settings.**

- **A full truck stop no longer means you cannot fuel there.** A full lot
  still costs you the parking, but you can fill up and then decide where
  to sleep.

- **Stopping hard no longer wrecks a load that was tied down properly.**
  Only the emergency brake, or a hard stop on a downgrade, still reaches
  the freight.

- **Sharp bends are now the ones that hurt the load.** The tighter the
  corner, the less room you have over the advisory; a bend taken at its
  advisory is free.

- **A roadside stop that takes your CDL now ends the run.** The load goes
  back to dispatch and you are released to the terminal to wait out the
  suspension.

- **Cloud backups of 1.9 careers are accepted again.** Your next save
  backs up normally, with no game update needed.

- **The Cloud backup menu now says when cloud backup is off, and offers to
  turn it on.**

- **Live weather is ready when you start driving, and a dropped connection
  keeps the last real conditions.** The game says they are last-known and
  keeps trying.

- **The Online services switch no longer touches weather, traffic, or
  parking.** Real-world weather, traffic and parking follow their own rows
  under Settings, whatever the Online switch says.

- **A dead radio stream no longer drops you to silence.** The radio says
  which station went off the air and moves you to the next one on the dial.

- **Rest-stop sleep no longer turns into a dead end at the exit.** Pressing
  T toward a sleep-capable stop names that stop and tells you to press X
  before leaving the highway.

- **A new stopping aid, off by default, can finish a planned rest stop at
  the entrance after you take the exit.** It never picks an exit for you,
  and missing the stop clears it.

- **The stop bar's steady tone is calmer.** It is now a much lower tone that
  still carries over the engine.

- **The stop bar's steady tone always stops.** It ends the moment the bar is
  behind you, and pausing or arriving silences it. Contributed by Noel Romey
  ([@nromey](https://github.com/nromey)) in [PR #149](https://github.com/orinks-games/Freight-Fate/pull/149).

- **Rumble strip sound no longer keeps playing when you pause on the
  strip.**

- **A damaged settings file no longer takes the game down with it.**
  Anything the game cannot read falls back to that setting's normal value.

- **The engine keeps one voice.** Every layer of the engine sound comes from
  the same recording, so it sounds like one engine.

- **A bad sound file no longer costs you every sound.** The game plays
  whatever it can still find, and one unreadable sound costs only that
  sound.

- **Cruise no longer claims a hill has beaten it while it is winning.** The
  call waits for a real grade that holds for a few seconds.

- **The engine's voice is now longer than your memory.** Each layer of the
  engine sound is longer now, so there is no pattern left to learn.

- **Public radio stations that had gone silent play again.** KUAR, WBFO,
  Maine Public, South Dakota Public Broadcasting and Texas Public Radio are
  back; stations with no working stream leave the dial.

- **The engine never repeats itself exactly anymore.** Each layer of the
  engine voice wanders slightly in speed and level, so there is no fixed
  cycle to notice.

- **The repeat key gives back the hazard warning, not the assist that
  talked over it.** One step back from a collision reaches the warning
  itself; the automatic braking line stays in message review.

- **Cruise control holds your speed down a hill instead of running away
  with it.** It steps up the engine brake as the hill needs and snubs the
  service brakes when that is not enough.

- **Cruise control answers a hill as you reach it, not ten seconds later.**
  It gives the grade the throttle it needs right away.

- **The automatic gearbox downshifts for a hill instead of lugging up it.**
  With the accelerator floored and the truck losing ground, it looks for a
  lower gear the way a driver does.

- **Route-transition assistance no longer traps the truck short of a stop
  bar.** Stopping short of the line hands the pedals back and says how far
  ahead the bar is.

- **Long local approaches step down like real streets.** A long facility
  approach runs faster on the wide-out stretch, then slows for the last
  miles and again at the gate.

- **No more 35-mile deadheads to a local pickup.** Local approaches now stay
  at cross-town distance.

- **A same-city dispatch no longer zones the whole interstate at 25.**
  Highway miles keep highway rules and their curve and limit warnings.

- **Automatic shifts sound like a real gear change now: clunk, sigh,
  clunk.** The revs fall away between gears, and the gear taking hold gets
  its own soft clunk.

- **The engine stopped ticking and the engine brake stopped breathing.** No
  click at any speed, no pulse in the growl.

- **Curve warnings now come with time to act on them.** Any bend the game
  warns you about plays out in real time from the warning until the curve
  is behind you.

- **A sleep that does not reset your hours says so, first and loudly.**
  Waking with the split still pending leads with "This sleep did NOT reset
  your hours" and when your duty window closes.

- **The roadside out-of-service stop explains which limit you blew and that
  the deadline kept counting while you sat.**

- **A serious log-check violation now pulls you over.** Lights and siren
  behind you, signal and brake to the shoulder, and the out-of-service hours
  pass while the truck is parked.

- **Dispatch deadlines now respect the hours already on your clock.** When a
  deadline stretches to cover a rest, the offer says "planned around the
  10-hour rest your hours will force."

- **Rolling a green light at the ramp end takes you straight onto the
  streets.** The street chain begins at whatever speed the light let
  through.

- **The radio finds its station again after a stop.** It reconnects by
  itself as you roll out and tells you if the station cannot be reached.

- **Fringe static finally sounds like FM.** The static at the edge of a
  station's range is now a smooth hiss.

- **Speed-limit drop warnings no longer double up.** The advance warning
  before a big posted-limit drop speaks once now.

- **The game no longer says you have arrived while handing you two miles of
  streets.** When streets follow the ramp, the arrival announcement waits
  for the gate.

- **No more "corridor between" announcements pretending to be towns.** Place
  callouts and the route report no longer speak those placeholder names.

- **The gear change comes at the top of the rev, not a second after.** The
  moment the engine crests its shift point on a hard pull, the gear comes.

- **Ramp-end callouts now tell you the speed limit at the bar.** "Light red,
  about 800 feet to the stop bar, speed limit 25," and the repeating status
  line carries it too.

- **Manual shifting answers the moment the clutch comes out.** A clean shift
  pulls again as soon as you let the pedal out.

- **The updater no longer hides a developer snapshot released the same day
  as a stable build.** Whichever build is newest is the one offered.

- **The route report tells the truth on the facility approach.** After your
  destination exit, R answers with the street you are on and how far to the
  gate.

- **The engine revs freely when you sit with the parking brake set.** It
  answers the throttle across its whole range and settles back to idle when
  you let off.

- **Stop signs at ramp ends finally tell you where to stop.** The sign gets
  the distance countdown, the closing tick, an answer from S, and guidance
  when you stop short.

- **Driving past a facility entrance no longer goes silent.** The gate
  repeats its instruction while you are still moving, cruise drops each
  time, and S answers with the gate itself.

- **The route report knows when you have arrived.** At a facility, R says
  "You have arrived. At Chicago Port Terminal. Stop to dock."

- **Adaptive cruise now drives the bends instead of quitting on them.**
  Cruise eases to the bend's advisory speed and climbs back to your set
  speed once the bend is behind you.

- **Curve speed assistance stopped talking over itself.** It holds its
  decision through the bend instead of flipping between slowing and
  released.

- **Turning on real-time traffic no longer crashes the game.** With Traffic
  source set to real time, live road reports now name crashes and closures
  near your truck. (thanks to Stickbear)

- **Curve calls arrive on time and tell the truth about distance.** Curve
  calls cut ahead of everything less urgent, and a bend just ahead says so
  instead of rounding up.

- **A curve call silenced by your stop-speech key comes back once with a
  fresh distance.** It stays quiet if you have already slowed for the bend.

- **Chained bends are one call now.** A bend covered by a "then" tail gets
  no call of its own: "Sharp left, half a mile. Advise 35. Then hairpin
  right, advise 25."

- **Driving to a local dock no longer sounds like driving to town.** Local
  facility runs name where you are going: "toward dry warehouse Camp Verde
  Dry Warehouse."

- **Speed-limit calls now know which way the town is.** If the town is
  behind you, the call says "leaving" instead of approaching.

- **Town speed limits no longer follow you out of town.** Past the last
  posted reading, the road goes back to a normal open-road limit for its
  type.

- **Live weather no longer flips between rain and freezing rain on its
  own.** Real rain is matched to your career season once when it arrives,
  then holds.

- **Downloaded builds no longer crash when you continue a career.**
  Truck-stop purchases, city-service errands, facility driveways, the radio
  catalog and curve callouts all ship inside the game now.

- **Taking an exit no longer talks you out of it.** The prompt now says to
  hold Right for the exit lane and keep slowing.

- **Within the last mile of an exit, a stray press of the exit key keeps
  your signal on.** Canceling there takes a deliberate second press.

- **A tap of Left or Right near an exit now says that taps only nudge the
  wheel and to hold the key instead.**

- **After a missed exit, the short turnaround announcement reminds you to
  signal again for the next pass.**

- **You cannot roll over a ladder at 25 anymore.** A fixed object in your
  lane takes a lane change or braking nearly to a stop, and automatic
  braking brings you down to a crawl for it.

- **Automatic braking now saves you even on hot or worn brakes.** It engages
  early enough on tired brakes, and hazard warnings arrive earlier when the
  truck needs more stopping room.

- **The ramp light now says where you are as well as its color.** The game
  says when you are stopped short, and yellow and green announcements say
  whether you have reached the bar.

- **Missing the destination exit twice no longer strands you at the end of
  the road.** Dispatch reroutes you every time, and the turnaround drops
  you far enough out to hear the callout, signal and brake.

- **The driving event voice no longer narrates the past.** When queued
  announcements fall too far behind, the stale ones are dropped and the
  newest speaks.

- **Street directions come one turn at a time now.** The navigator speaks
  only the next turn, announcing each as you approach it.

- **Street names no longer read out raw map codes.** Spoken street names
  keep just the street and its primary route number.

- **The merge instruction is the first thing you hear when you depart.**
  Travel plaza and rest stop notices wait a moment when another road
  announcement just played.

- **Adaptive cruise no longer crawls behind traffic it has not caught up
  to.** Cruise holds your set speed until you are closing in, eases down to
  your following gap, and speaks the traffic warning once.

- **Braking to a stop no longer drops the truck into reverse.** Every
  direction change takes one deliberate gesture: stop, release the control,
  then press and hold it for a moment.

- **A quick tap of the brake at a standstill just brakes.** Confirming the
  truck is holding never grabs a gear.

- **Hold-through reversing no longer works.** The spoken key help teaches
  the new gesture.

- **Seven real radio streams play again after a full dial checkup.** KJZZ,
  KCRW, KUNM, KUTX, KERA, KCUR and WBUR are back; WABE Atlanta leaves the
  dial until it has a working stream.

- **Ramp-end traffic lights now have a yellow phase and speak every
  change.** Every green, yellow and red is spoken as it happens; yellow
  means stop unless you are already at the light.

- **Interstate speed limits no longer drop to city speeds at the ends of a
  leg.** The limit you hear is the one the road posts, and speeding
  enforcement matches it.

- **US highways and parkways got the same speed-limit cleanup.** Small-town
  limits stay as posted, and a town's street speed no longer rules a whole
  route.

- **A dropped speed limit now gives you braking time before a strike.**
  Enforcement waits while you slow; staying on the throttle through the
  drop forfeits the grace.

- **Touching the brake now switches cruise control off, like a real
  truck.** Any press of the service brake or emergency brake drops cruise
  and announces it.

- **Port terminals only show up in cities that really have a port now.**
  Inland towns no longer offer port loads, and small towns far from a rail
  yard no longer list an intermodal ramp.

- **Toledo, Detroit, Chicago and Green Bay now have working docks.**

- **An old port offer still on a saved dispatch board is withdrawn when you
  accept it.**

- **An empty truck no longer machine-guns up through the gears.** Running
  light, the truck skip-shifts, starts in a higher gear, and keeps that
  launch gear at a stop.

- **Updating the game no longer flags your save as changed outside the
  game.** A save from an earlier version loads without that mark.

- **Quick manual downshifts now respect the clutch the moment you press
  it.** Holding Shift while tapping Q or W counts as clutch down at once.
  (thanks to corykad, [@corykad](https://github.com/corykad),
  [PR #157](https://github.com/orinks-games/Freight-Fate/pull/157))

- **Paying down what you owe never empties your wallet anymore.** Every
  payment option at the yard keeps fuel money in your pocket, so Pay half
  cannot leave you at zero.

- **"The scale comes first" now reaches you.** Pressing the rest
  key with an open weigh station ahead always gets that line through.

- **The driving speech settings explain themselves again.** The Driving
  event voice row's help says the rate, pitch, volume and voice rows only
  appear when your voice supports them.

- **A stalled engine, a carrier-grounded tractor, and a snapped tire chain
  now speak on Quiet and Urgent only.** All three speak as words on every
  driving speech setting.

- **"Back on the pavement" now shows up in message review on Urgent only.**
  It always reaches the log now.

## 1.8.8.1 - 2026-08-08

### Added

- **Creating your first career now offers to connect this computer to an
  orinks.net account.** Right after you hear "Welcome aboard" and where your
  truck is parked, the game asks once whether to connect this computer to an
  orinks.net account, with a code and a browser confirmation. It only asks the
  once -- decline with Not now, or just press Escape, and driving starts
  right away with nothing else to answer. Connecting links the computer to an
  account; it does not turn anything on by itself, and you can connect any
  time later from Online on the main menu, where cloud backup and the
  drivers board each stay off until you turn them on yourself.

### Fixed

- **Live weather no longer gives up on a healthy weather service.** Weather
  stations file their reports once an hour, but the game treated any report
  more than thirty minutes old as a failure -- so for most of every hour it
  quietly switched to simulated fallback weather even when the service was
  fine. A report now stays current until it is well past the hourly cycle,
  so live weather stays live, and the weather app still tells you exactly
  how old the reading is.

- **Real-world weather now follows the truck instead of the next city.** Live
  National Weather Service conditions update as you move along a route, and
  weather reports say when conditions are live, still loading, last known, or
  simulated because the live service is unavailable. Old failed-refresh rain
  now expires instead of lingering as if it were current.

- **Loading a career no longer cuts off its own welcome.** Choosing a saved
  career from Continue latest career or Choose career used to say "Welcome
  back" and then get cut off mid-sentence by the terminal announcing where
  you are parked, so you never heard your money or which terminal you loaded
  into. The welcome is now heard in full before the terminal speaks.

- **Backing all the way out of Settings now speaks "Settings saved."**
  Pressing Escape from the settings categories used to say it and then get
  cut off by the main menu announcing itself again, so it was never actually
  heard. It now plays after the main menu's own announcement, so you get the
  confirmation.

## 1.8.8 - 2026-08-05

### Changed

- **Connecting a computer to orinks.net no longer involves copying and
  pasting anything.** Setting up online features used to mean copying a
  Driver ID and a token from the website and pasting each one into the
  game. Now the game shows an activation code, reads it out loud, and can
  spell it letter by letter or copy it to the clipboard if you would
  rather type it into the browser yourself. Once you confirm the code on
  orinks.net, the game finishes connecting on its own.

- **Automatic Mastodon posts moved to their own hashtag.** Deliveries shared
  to your Mastodon account now carry the FreightFateRuns hashtag instead of
  the FreightFate one. Players use the FreightFate tag to share their own
  thoughts about the game, so anyone who had muted it to keep the automatic
  posts out of their timeline was quietly losing those conversations too.
  The two are separate now: FreightFate is for people talking, FreightFateRuns
  is for the game posting. There is nothing to change in the game. If you
  muted the FreightFate tag because of the delivery posts, you can unmute it
  and mute FreightFateRuns instead, and if you liked following other drivers'
  runs, follow FreightFateRuns to keep seeing them. The change is on the
  orinks.net side, so it already applies to every version of the game.

- **The automatic transmission works each gear harder before shifting up.**
  Pulling a load, the engine now runs into a higher, more realistic RPM range
  in every gear instead of grabbing the next one early, so climbs sound and
  feel like the truck is actually working. Running empty, the two-gear jumps
  the box likes on flat ground now land far enough above the downshift point
  that it no longer shifts up and immediately drops back down. Contributed by
  corykad ([@corykad](https://github.com/corykad)) in
  [PR #144](https://github.com/orinks-games/Freight-Fate/pull/144).

### Fixed

- **Picking a lower gear with the clutch held no longer damages the engine.**
  Downshifting several gears at highway speed used to bring the redline
  warning and engine damage the moment the gear was selected, even with the
  clutch to the floor and the engine disconnected from the wheels. The
  warning and the damage now begin only if you release the clutch while the
  gear is too low for your road speed. Contributed by corykad
  ([@corykad](https://github.com/corykad)) in
  [PR #144](https://github.com/orinks-games/Freight-Fate/pull/144).

- **Copying to the clipboard works on Linux.** On Linux, copying a delivery
  summary, a reviewed message or a link always said the copy did not take.
  Linux desktops offer clipboard text under a different name than Windows
  does, and the game was only ever asking for the Windows one. It now asks for
  the names Linux really uses, so copying out of the game works, including
  copying your activation code during online setup. Windows and Mac are
  unchanged. Thanks to a player report.

- **Continuing a saved run announces the right date and season again.** When
  you picked a run back up, the calendar went back to the day you set out on
  rather than the day you had driven into, so a haul that had rolled past
  midnight came back on yesterday's date, and a long run that had carried you
  into a new season heard the old one, with the weather to match. Continuing a
  drive now puts the calendar at the same moment your trip clock is at, so the
  date, the season and the weather all agree. Contributed by Day Garwood
  ([@day-garwood](https://github.com/day-garwood)) in
  [PR #146](https://github.com/orinks-games/Freight-Fate/pull/146).

- **Continuing a saved run no longer pushes your deadline further out.** Every
  time a delivery was picked back up, the game worked the deadline out again
  from where you were and how long you had been going, which quietly gave you
  more hours than dispatch had agreed to. A run you were running late on could
  be rescued just by saving at a stop and continuing. Deadlines now stay where
  dispatch set them. A run already under way gets one last recalculation the
  first time you continue it after this update, so nobody loses hours they had
  been counting on, and it is fixed from then on. Contributed by Day Garwood
  ([@day-garwood](https://github.com/day-garwood)) in
  [PR #146](https://github.com/orinks-games/Freight-Fate/pull/146).

- **Quitting mid-drive writes a save that agrees with itself.** Quitting to the
  title mid-drive puts you back at the stop you last saved at, but the save was
  still being written with the hours of service and the fatigue you had built
  up since leaving that stop. Continuing always put them back, so the drive you
  returned to was correct either way, and this only mattered to the cloud
  backup, which was storing a shift that never happened. The save now records
  the stop you will actually resume from. Contributed by Day Garwood
  ([@day-garwood](https://github.com/day-garwood)) in
  [PR #146](https://github.com/orinks-games/Freight-Fate/pull/146).

## 1.8.7 - 2026-07-30

### Added

- **The drivers board is now reachable from the pause menu.** You can hear
  who is hauling right now without quitting to the main menu. The new
  "Drivers board" item sits between Settings and Abandon job in the pause
  menu. Viewing the board shares nothing about you. Contributed by wleicht
  ([@wleicht](https://github.com/wleicht)) in
  [PR #136](https://github.com/orinks-games/Freight-Fate/pull/136).

### Changed

- **Reviewing what the game said now works the same way everywhere.** Comma
  and period stepped through recent speech on every screen, while a fuller set
  of review keys only ever worked while driving, and the two kept separate
  histories. There is one history now, and every review key works on every
  screen. Comma still repeats what was just said and steps back from there,
  period moves forward, and you can now also jump to the oldest or newest
  message with Ctrl and those same keys, switch between all messages, general
  messages and driving events with the bracket keys, and copy the message you
  are on with Ctrl+C, whether you are driving or sitting in a menu. Each
  press reads the message and nothing else, the last 200 are kept, and moving
  through menus is no longer mixed into the history, so what you step back
  through is what actually happened. Checking the pause menu
  mid-run no longer leaves a "Paused" and a "Resumed" between every pair of
  announcements. New announcements do not move your place while you are
  reviewing, and once you have left the keys alone for ten seconds the next
  press starts fresh from the newest message with all categories showing, so
  comma always repeats what was just said instead of picking up where you
  left off earlier in the run. Typing a driver name still takes punctuation
  as punctuation. Raised by wleicht
  ([@wleicht](https://github.com/wleicht)) in
  [issue #134](https://github.com/orinks-games/Freight-Fate/issues/134).

- **Your online driver token is now kept in your computer's password store.**
  If you have linked the game to an Orinks account, the secret half of those
  credentials used to sit in a plain text file alongside your saves, readable
  by anything that could reach the folder. It now lives in Windows Credential
  Manager, the macOS Keychain, or your Linux keyring, the same places your
  browser keeps saved passwords. The change happens by itself the next time
  the game starts, and your Driver ID stays where it was, so there is nothing
  to re-enter and nothing to set up. If secure storage is unavailable, the
  old credentials remain intact instead of being removed, and Windows will
  not put a new token into a plain text fallback file. The setup menu explains
  how to retry without claiming that unsaved credentials connected. Thanks
  to Tyler Rodick, [@trodick](https://github.com/trodick),
  [PR #133](https://github.com/orinks-games/Freight-Fate/pull/133).

### Fixed

- **Automatic speed control no longer says it is resuming as you pull up to
  a pickup.** Rolling up to the gate with cruise or the speed keeper on, you
  were told speed control was paused and would resume after you departed with
  the load, and then a moment later heard it announce that it was resuming
  after all, before it switched off again. It was genuinely re-engaging for a
  fraction of a second. It now stays put until you actually depart, so the
  announcement you hear at the gate is the one about arriving.

## 1.8.6.2 - 2026-07-29

### Fixed

- **Accepting a dispatch after an update no longer closes the game.** The
  dispatch board is kept in your career, so the board you were looking at
  before an update could still list a pickup that the update had closed.
  Pressing Enter on one of those loads shut the game down without a word. The
  board is now rebuilt from the current world the first time you open it after
  an update, so you see loads you can actually take. If you somehow reach a
  closed pickup anyway, the game says so and sends you back for a fresh board
  instead of quitting.

- **Kilometers are now used everywhere, not just out on the road.** With units
  set to metric, the driving cues spoke kilometers but several other screens
  still read miles, so the same trip was measured two different ways depending
  on where you asked. The dispatch board's job details, the distance and rate
  it quotes, the summary you hear when you take a load or deadhead to a
  pickup, the exit and hazard callouts, your remaining distance to a pickup,
  the delivery summary's credited distance, your lifetime distance in career
  stats, and the on-screen speed and trip readouts now all use the unit you
  chose. Career stats also says "Lifetime kilometers" instead of labelling
  kilometers as miles, and the pay rate reads as dollars per kilometer with
  the figure recalculated to match. Nothing changes if you play in miles.
  Contributed by otaviols ([@otaviols](https://github.com/otaviols)) in
  [PR #142](https://github.com/orinks-games/Freight-Fate/pull/142).

## 1.8.6.1 - 2026-07-28

### Fixed

- **Quitting to the main menu no longer closes the game.** Choosing "Quit to
  main menu" from the pause menu brought you back to the title screen and then
  shut the game down a moment later. You now land on the title screen and stay
  there, free to continue your career or start another. Reported by smeveriss
  in [issue #132](https://github.com/orinks-games/Freight-Fate/issues/132).

## 1.8.6 - 2026-07-28

### Fixed

- **Roadside fuel rescue now leaves your truck safely stopped.** After running
  dry, rescue refuels the truck but clears its previous highway motion before
  you restart, so you pull away normally instead of resuming at road speed.

### Added

- **A warning before every steep grade.** Any climb or descent of three percent
  or more that runs for at least three quarters of a mile is now called out
  before you reach it, with how steep it is, how far it runs, and -- going down
  -- what to do about it before it starts. In a manual that is picking your
  gear and setting the engine brake; in an automatic it is setting the engine
  brake and braking down to speed, which is what puts the transmission in a
  lower gear for you. Short dips stay quiet, so on a mountain route you hear
  the hills that matter and nothing else. Terse speech does not get these
  announcements at all -- press G whenever you want the grade instead. The
  advisory is there so you can decide before the hill starts: when in doubt,
  take manual control of the speed rather than leaving it to cruise.

- **G speaks the grade.** Press G while driving for the slope under the wheels,
  how much further it runs, whether the truck is holding it or losing ground --
  and the next steep grade ahead with how far off it is. It answers "why is my
  speed building" without waiting for anything to be announced.

- **Review recent spoken messages while driving.** Use the message review
  controls to move through general and critical announcements, jump to the
  first or latest message, switch categories, and copy the current message to
  the clipboard. Contributed by Day Garwood
  ([@day-garwood](https://github.com/day-garwood)) in
  [PR #122](https://github.com/orinks-games/Freight-Fate/pull/122) and
  [PR #124](https://github.com/orinks-games/Freight-Fate/pull/124).

- **Tire sounds now react to your speed.** On supported audio systems, the tire
  hum rises and falls as you accelerate or brake. Above a crawl, soft road-seam
  thumps add texture through sound and controller vibration. Contributed by
  Swarup Baral ([@swarup-developer](https://github.com/swarup-developer)) in
  [PR #114](https://github.com/orinks-games/Freight-Fate/pull/114).

- **Linux players get an AppImage.** Alongside the tarball, each release now
  ships `FreightFate-<version>-linux-x86_64.AppImage`: one file you mark
  executable and run, with no extraction step. It carries the libraries the
  Ubuntu build needs, so it also runs on Fedora, Arch, and openSUSE, and
  every build boots on Fedora before it ships. Because the AppImage itself
  is read-only, saves live in `~/.local/share/FreightFate` instead of a
  `saves` folder beside the game. In-game updates work too: the game
  downloads the new AppImage, swaps the file in place, and restarts — and
  if the AppImage sits somewhere your user account cannot write to, the
  game tells you where the downloaded update was saved instead of failing
  quietly.

- **An armed exit now counts itself down.** Once your signal is on, the
  exit calls out again at two miles, one mile, and half a mile. No more
  hearing about an exit once, five miles early, and never again until
  you have missed it. On the terse speech setting the countdown stays
  quiet, so terse drivers hear only the original announcement.

- **Delete a career's cloud backups.** Each career in the Cloud backup menu
  now has a Delete item that removes every kept backup of that career from
  your orinks.net account, after a spoken confirmation. Your saves on this
  computer are never touched, and a career that is still on this computer
  with cloud backup turned on simply starts a fresh backup the next time it
  saves. Handy if a save was backed up by mistake, such as someone else's
  career you had copied onto your computer.
- **Copy your delivery summary to the clipboard.** The delivery complete
  screen has a new item, just before Continue, that copies every settlement
  line as plain text so you can paste the whole run into a message or a
  forum post. The game confirms out loud once the text is really on the
  clipboard, and tells you if the copy did not take.
- **Share notable deliveries to your own Mastodon account.** A new Settings,
  Online option posts a short public summary, with the FreightFate hashtag,
  when a delivery earns you an achievement, a level, or a perfect streak milestone.
  Routine runs are never posted. It is off until you turn it on, and linking
  your Mastodon account happens in your browser on orinks.net using the same
  sign-in as driver setup; the Mastodon account item walks you through it
  and can check whether the link took.

- **The R key now tells you how far along you are.** The route report leads
  with your trip progress, like 53 percent there, followed by the miles left.
  It is the same figure the online drivers board shows for you, and the Tab
  status menu has a matching Progress line. Deadhead drives count too.

### Changed

- **The R key now answers just "where am I".** Two short sentences: how far
  along you are and how far is left, then the road you are on with its
  direction, the state you are in, and the city it is taking you toward. If
  you have planned a stop, the distance counts down to that stop instead of
  the destination, so R tells you how far to the place you are actually
  driving at. The nearest named place, the grade, the zone, and the next
  maneuver are gone from it, because each of those already has its own key
  or lives in the Tab status menu. U still reads what is coming up, and
  Shift R still reads the next exit.
- **Shorter driving readouts that fit a braille display.** The clock, route,
  weather, and fuel reports now put the answer in the first few words, so a
  one-line braille display shows what matters without panning. The C key
  leads with the time and whether you are on schedule instead of burying the
  verdict at the end, and on the terse speech setting it skips the calendar,
  the appointment restatement, and the stop-planning advice, all of which
  the Tab status menu still carries.
- **Speech verbosity is now a simple choice between terse and normal.** The
  chatty level never said anything normal did not; it only repeated your
  speed a little more often. If you had chatty selected, the game now uses
  normal, and everything you heard before is still there.
- **Rest stops no longer let you sleep twice for nothing.** When you are
  already fully rested at a rest stop, choosing a sleep option now warns you
  that it would only move the clock and your deadline forward, and asks you to
  press Enter again to confirm. This is the same safeguard the terminal bunk
  room already had. Contributed by Brandon Cross
  ([@ironcross32](https://github.com/ironcross32)) in [PR #112](https://github.com/orinks-games/Freight-Fate/pull/112).
- **Everything online now lives in one Online menu on the main menu.** The
  drivers board, orinks.net account setup, Profile sharing, cloud backup and
  restore, Mastodon sharing, and Discord presence moved out of Settings into
  a single Online menu, so restoring a save on a new computer or checking who
  is hauling no longer means hunting through settings categories. The board
  sits first because viewing it shares nothing about you. Choosing Online
  inside Settings still works and opens the same menu, and every toggle keeps
  the familiar Enter, Right, and Left arrow controls.
- **Walking away from a parked truck now takes you off the drivers board.**
  If your truck sits stopped with nothing changing for half an hour, you
  leave the public board just as if you had paused the game, and the board
  stops calling you a driver who is on duty. The moment anything changes,
  like rolling again or pulling into a stop, you are back on the board within
  seconds. Deadhead drives also now say how far along they are, on the board
  and in Discord, so a long empty run never looks like a parked truck.
- **Test builds now introduce themselves as development builds.** The main
  menu welcome and the update screen used to read a bare version number that
  looked like a stable release. A nightly or source build now says, for
  example, "version 1.8.6 development build", so you always know which kind
  of build is talking. Stable releases sound the same as before, and version
  numbers no longer skip when a stable release comes out.

### Fixed

- **Adaptive cruise no longer runs away down a hill.** Cruise could only ever
  add throttle, so on a downgrade it simply came off the fuel and let gravity
  carry the truck -- fifteen miles per hour or more over your set speed, with
  nothing said about it, and a speeding fine at the bottom. It now reaches for
  the engine brake when the truck starts to gain on a descent and snubs the
  service brakes when the engine brake is not enough, holding your set speed
  on grades as steep as eight percent. It hands the engine brake back when the
  hill ends, and never touches the switch when you set it yourself. Climbing,
  cruise now answers a hill as the wheels reach it instead of taking ten
  seconds to work up to full throttle. Cruise is still an assistant, not a
  driver: on a steep or long grade, when in doubt, take manual control of the
  speed. Braking hands the truck straight back to you.

- **Cruise tells you when a grade has beaten it.** If the truck is running well
  past your set speed down a hill, or has lost a lot of it climbing, cruise
  says so once, names the speed you are actually doing, and tells you to brake
  or gear down -- or just to brake, if you are driving an automatic. Before
  this it drifted in silence. It waits until the grade has genuinely won and
  stayed won, so a gear change or a stretch of nearly level road never sets it
  off. Treat it as your cue to take over: when in doubt, brake rather than
  waiting to see whether cruise recovers the speed.

- **Automatic speed control now slows in time for construction zones.** At
  highway speed, adaptive cruise begins braking when the advance warning is
  announced and reaches the work-zone limit before the speed keeper takes over,
  so the game no longer fines you while its own controls are still slowing down.

- **Automatic speed control now slows in time for heavy traffic zones.** The
  same early braking that construction zones already received now applies to
  heavy traffic zones. When the advance warning sounds, adaptive cruise begins
  easing down to the posted limit and hands off to the speed keeper at the
  zone boundary, so the transition is smooth and you enter at the right speed.
  Contributed by wleicht ([@wleicht](https://github.com/wleicht)) in
  [PR #127](https://github.com/orinks-games/Freight-Fate/pull/127).

- **The rest-stop arrival cue now leaves real time to set the brake.** Trip
  pacing no longer consumes the whole stopping buffer while even a slow voice
  is still speaking. Terse speech now says "Stop now." If you set the parking
  brake when the stop announces your arrival, the truck can finish stopping
  and open the rest-stop menu; continuing past without stopping still misses
  the stop.

- **A destination exit stays ready after it is announced.** On Standard or
  Fast trip pacing, slowing down while the callout spoke could shrink the
  action window, so pressing X answered "No exit coming up." The exact
  announced exit now remains available through a human reaction window, while
  expired and already-passed exits still cannot be armed. Braking, inspection,
  and other safety warnings also finish before the signaling confirmation.

- **Short hauls no longer pay several times more per mile than long ones.**
  A guaranteed minimum meant a fifty mile hop could pay over a thousand
  dollars, four to five times the per-mile rate of a real cross country run,
  so short hops were always the best money. Short jobs still pay a premium
  per mile, the way real freight does, but it now eases down smoothly as the
  distance grows. Pay for medium and long routes is essentially unchanged.

- **The updater no longer hides a developer snapshot released the same
  day as a stable build.** On the developer snapshots channel, a stable
  release published in the small hours used to mask that morning's
  snapshot -- even when the snapshot carried newer fixes -- because the
  two were compared by date alone. Updates now compare by the actual
  publish moment, so whichever build is genuinely newest is the one
  offered.

- **Driving past a pickup or delivery entrance no longer goes silent.**
  Arriving at a facility used to announce itself once; if you rolled on --
  easy to do with cruise re-engaged -- the game said nothing more for the
  rest of the drive, and the delivery quietly went late. Now the gate
  repeats its instruction every ten seconds while you are still moving,
  cruise drops each time so the truck is never held at speed past a dead
  end, and the S key answers with the gate itself -- "At the receiver.
  Stop to dock." -- instead of a speed limit that stopped mattering when
  the route ended.

- **The road tells you the truth about the terrain.** Stretches that were
  quietly called mountain in flat country -- the East Texas piney woods, the
  Hill Country's gentle dips -- now read as the flat or rolling ground they
  really are, so the status readout no longer puts you in the mountains where
  no Texan would. The real climbs you brace for still call out as mountain
  grades, and every famous grade -- the Grapevine, Monteagle, the Siskiyous,
  the run up to the Continental Divide -- keeps its name. Thanks to Noel
  Romey, [@nromey](https://github.com/nromey),
  [PR #107](https://github.com/orinks-games/Freight-Fate/pull/107).

- **Starting a new career no longer talks over itself.** Naming your driver
  and picking a home region and city used to leave those screens announcing
  themselves again on the way out, each one cutting off the last, before the
  terminal menu finally spoke. The screens you have finished with now stay
  quiet, so you hear your new career's terminal and nothing else -- and the
  stray announcements no longer fill up the message review history either.
  Contributed by Day Garwood ([@day-garwood](https://github.com/day-garwood))
  in [PR #129](https://github.com/orinks-games/Freight-Fate/pull/129).

## 1.8.5.1 - 2026-07-22

### Fixed

- **Each extracted copy of the game now keeps its saves strictly to itself.**
  Previously, a copy of the game could look one folder up on its first run
  and adopt the saves it found there, so keeping two versions side by side,
  for example a stable install next to a test build, could make one copy
  take over the other's careers, and deleting a save in one copy could make
  it disappear from the other. A copy of the game now only ever reads and
  writes the saves folder inside its own game folder, so you can test as
  many extracted copies as you like without them touching each other.

## 1.8.5 - 2026-07-22

### Added

- **The game can now tell you where its log file is.** Settings has a new
  Problem reports category, and Where the game log is saved reads out the full
  path and shows it in the window. The game has always kept a log of the
  session, including everything it said out loud, but nothing ever pointed you
  at it. Attaching it to a bug report shows exactly what you heard. The
  previous session is kept beside it, so restarting the game to check something
  does not lose it, and both files stay on your computer.
- **The route report now tells you where you are.** Press R, or D-pad up on a
  controller, to hear the current road and direction, state, nearest named city,
  checkpoint, or road stop, along with your progress and upcoming guidance.
- **Live weather can now leave your career calendar running.** The new Live
  weather controls calendar setting keeps today's real date when on. Turn it
  off to keep live city conditions while your career date advances at midnight
  and the seasons pass normally. Live conditions are adjusted to the career
  season so summer snow and cold-season thunderstorms do not slip through. An
  established career begins its independent calendar from today's date when
  you turn the setting off, avoiding a jump back to its old hidden date; a new
  career still begins on March 21. Thanks to TowerAlphaTheta15,
  [@TowerAlphaTheta15](https://github.com/TowerAlphaTheta15),
  [PR #88](https://github.com/orinks-games/Freight-Fate/pull/88).
- **Map stops now open a full details view, and you can plan your next
  stop.** On the driving Map screen, pressing Enter on a stop now opens its
  details instead of repeating the line: the exit, distance, what it offers,
  parking, and an estimate of how long it will take to reach at your pace,
  including whether you would arrive before your next break, driving limit,
  or duty window. From there, plan to stop at it: approach announcements and
  the upcoming and clock readouts then call it your planned stop, so you know
  exactly when to signal for the exit. Plans can be canceled or replaced any
  time, survive saving and resuming, and clear themselves when you pull in or
  drive past. Thanks to Brandon Cross,
  [@ironcross32](https://github.com/ironcross32),
  [PR #94](https://github.com/orinks-games/Freight-Fate/pull/94).
- **Keep speed assistance active across the whole job.** Pressing K now starts one automatic speed-control session: the speed keeper handles facility roads, gate queues, work zones, and congestion, then adaptive cruise takes over on the open road. If started during the deadhead, it pauses through pickup check-in and loading, survives a save there, and resumes once the loaded truck is rolling. It restores your earlier cruise target or uses the new road's limit, and switches back to the keeper for the next restricted zone. Braking outside that planned pickup, a hazard, or pressing K again cancels the whole session so it cannot restart unexpectedly. Plus and Minus adjust the remembered open-road target in either mode.

### Changed

- **The drivers board now checks in a little less often.** With online
  presence on, the game reports you every two and a half minutes instead of
  every minute, and the board waits six minutes before it calls a driver gone.
  You still appear the moment you go on duty, and going off duty, pulling
  over, or starting a new leg still reaches the board within seconds; it is
  only the quiet keep-alive that slowed down. A driver who closes the game may
  sit on the board a few minutes longer than before.
- **Career saves are now sealed files instead of plain text.** Saved careers
  use a new packed format that ordinary text editors cannot open, and every
  save is signed by your own installation. Your existing careers convert
  automatically the first time they load, and a copy of the old file is kept
  beside the new one in case you ever roll back to an older version of the
  game. A save that was changed outside the game, or copied over from another
  computer, still loads and plays normally, but the game tells you once and
  marks that career as modified; shared features may not accept a modified
  career. Thanks to Noel Romey, [@nromey](https://github.com/nromey),
  [PR #96](https://github.com/orinks-games/Freight-Fate/pull/96).

- **Moving a planned stop now asks first, and each stop only cancels its own
  plan.** A stop's details screen shows the cancel option only when that stop
  is the one you planned. Choosing "Plan to stop" while another stop is already
  planned now tells you which stop is planned and how far ahead it is, and asks
  whether to move your plan here before changing it.

- **Air pressure now bleeds down while the truck is parked with its engine
  off.** After a full night's sleep, you must start the engine and rebuild air
  before releasing the parking brake instead of waking to fully charged tanks.

- **Each truck you own now keeps its own fuel, damage, tire wear, and road
  grime.** Refueling, repairing, or dirtying one truck no longer affects the
  others, and a newly bought truck arrives with a full tank and a clean bill
  of health. Switching trucks no longer moves fuel between them. Careers from
  older versions are updated automatically the first time they load, and the
  game tells you once that the updated save can no longer be opened by older
  versions: the truck you were driving keeps its condition, and your other
  trucks start fueled and fresh. Back up your career save before opening it in
  this version. If the conversion causes a problem, include both the original
  backup and the updated save with your issue report. Thanks to Brandon
  Cross, [@ironcross32](https://github.com/ironcross32),
  [PR #91](https://github.com/orinks-games/Freight-Fate/pull/91).

- **On-time deliveries now pay a real bonus.** Delivering on time used to add
  only a sliver of extra pay unless you raced in absurdly far ahead of the
  deadline, so the bonus line at settlement rarely felt like one. Pay now
  works the way real shipper scorecards do: hit the delivery window and you
  earn a flat ten percent on-time bonus every time, and arriving hours early
  pays no more than making the appointment. The settlement summary now calls
  it an on-time delivery bonus. Late and damaged deliveries still pay less,
  exactly as before.

- **You can sleep at turnpike service plazas now.** Service plazas used to
  offer fuel, food, and a short break but never a proper rest, so a tired
  driver had to push on to the next truck stop even while parked at a big
  Thruway or Turnpike plaza with overnight truck parking. Every service
  plaza's stop menu now offers sleep like truck stops and travel centers do,
  and route planning counts them as places you can end your day.

- **The driving assistance presets have been set aside for version 1.9.** Recent developer snapshots briefly offered a Driving assistance settings category with emergency braking, lane support, stop-and-go, and descent control presets. That work now ships complete with version 1.9 instead. The speed keeper stays, now under Settings, Gameplay, and your other settings are untouched.

### Fixed

- **Automatic cruise now leaves enough speed in hand for an exit.** After you
  signal for an exit, cruise aims for 40 miles per hour instead of balancing on
  the 45-mile-per-hour ramp limit. It now stays safely under that limit on a
  downhill approach instead of creeping up to 46 and missing the exit.

- **Distances and speeds no longer say "1 miles".** Anything that comes out to
  exactly one is now read as "1 mile" or "1 kilometer", on map points, stop
  distances, speed readouts, and everywhere else the game speaks a measurement.

- **The Map screen now names the cities on your route.** Its first line read
  the internal data names aloud, so a run down the east coast opened with
  "new underscore york underscore n y underscore u s" and kept going for every
  city on the route. It now says "New York to Philadelphia to Washington", the
  same way the delivery summary and the dispatch board already did.

- **Getting pulled over now judges whether you are actually stopping, not how
  far you rolled.** A loaded truck at highway speed cannot halt on a dime, and
  the old rule could flag a felony stop even when you braked correctly the whole
  way. Now, once the lights come on, the trooper watches your behavior: signal
  and brake down to a stop and you get the ordinary roadside check, and pulling
  over promptly and cleanly gives a small chance a ticket is waived to a warning.
  Keep accelerating, coast along without slowing, or ignore the lights and it
  still ends in a felony stop. You no longer have to hold the emergency brake
  the entire time; braking steadily is enough. Thanks to Brandon Cross,
  [@ironcross32](https://github.com/ironcross32),
  [PR #103](https://github.com/orinks-games/Freight-Fate/pull/103).

- **Taking your planned stop's exit no longer warns that you drove past it.**
  When you signal a planned stop and brake down the exit ramp, the game used to
  announce that you had driven past it and cancelled the plan, even though you
  were stopping there. Now the warning only speaks when the stop is truly out of
  reach: if you drive past the exit without signaling, if you signal but are
  going too fast to make the ramp, or if you take the exit but never stop and
  roll on past the end of the ramp. Thanks to Brandon Cross,
  [@ironcross32](https://github.com/ironcross32),
  [PR #102](https://github.com/orinks-games/Freight-Fate/pull/102).
- **Taking an exit and never stopping no longer strands you on the ramp.**
  If you took an exit but kept driving without ever coming to a stop, the game
  would quietly wait forever -- speed enforcement stayed off and, miles later,
  stopping would still open that stop's menu. Now, once you roll well past the
  end of the ramp without stopping, the game tells you that you never stopped
  and hands the highway back.
- **The trip mile marker holds while you are on an exit ramp.** The ramp is off
  the highway, so your progress along the route no longer ticks up while you
  brake down it; the highway picks back up where you left it once you rejoin.
- **Live weather no longer announces a simulated forecast while loading.**
  Pressing V before the first live observation arrives now says that live
  weather is still loading instead of describing an invented forecast. This
  also works when Live weather controls calendar is turned off. Route weather
  now names cities whose observations are loading or unavailable instead of
  silently skipping them, and weather-change announcements identify live
  observations and simulated fallback conditions. Thanks to TowerAlphaTheta15,
  [@TowerAlphaTheta15](https://github.com/TowerAlphaTheta15),
  [PR #99](https://github.com/orinks-games/Freight-Fate/pull/99).
- **Cloud Backup works again on test builds.** Careers from a recent test
  build were being turned away with a message about the backup being
  unreadable. Nothing was wrong with those careers: the game had changed what
  it records about each truck, and the backup service was still expecting the
  older shape. Both sides now agree, and they are kept in step automatically
  so this cannot drift apart again. Released builds were never affected.
- **A career carried between computers can lose its "modified" mark.** Moving
  a save to another machine marks it as changed outside the game, because the
  new machine cannot recognise the old one's signature. When you restore that
  career from Cloud Backup and everything about it checks out, the mark is
  now cleared instead of following you forever.
- **Finishing a delivery no longer risks losing what it reported online.**
  A settlement can report the delivery, a level up, and several badges at
  once, and the game was sending them all at the same moment. They could
  crowd each other out, be sent twice, or in one case be dropped entirely.
  They now go out one after another, so your road journal records everything
  the run earned.
- **Taking a pay advance no longer makes your cloud backup look edited.** A
  pay advance is money from your next load paid early, but your career was
  only crediting the part of the settlement left after the advance came back
  out. Those advanced dollars sat in your bank with nothing in your lifetime
  earnings to account for them, so cloud backup upload could refuse the save
  and mark the driver for review. Lifetime earnings now count the whole
  settlement, and taking an advance no longer counts against you.
- **Planning a stop now plays a single confirmation sound.** Choosing "Plan to
  stop" from a stop's details no longer stacks a menu click and a confirmation
  chime back to back; you hear just the confirmation.
- **Back-to-back road alerts no longer crash the Windows event voice.** Urgent
  hazards still interrupt the current event announcement immediately, without
  making a separate redundant stop call that could crash inside SAPI.
- **Lower speed zones now give you time to brake.** When a construction zone
  or other posted limit drops, releasing the accelerator gives a loaded truck
  a fair braking window before speeding enforcement begins. Staying on the
  throttle forfeits that grace.
- **Missing the destination exit twice no longer strands the delivery.** Every
  missed attempt now loops you back far enough to hear the exit, signal, and
  slow down again instead of leaving the route stuck at zero miles remaining.
- **Automatic speed control now slows you for the destination exit.** When the
  signed delivery exit is announced, adaptive cruise eases the truck to ramp
  speed instead of switching off at highway speed. Press X to take the exit;
  automatic control releases as you enter the ramp, ready for you to brake to
  the facility stop without collecting an unwanted speeding fine.
- **Driving through a city no longer announces the same truck stop twice.**
  A city's stops were counted once for the road coming in and once for the
  road going out, so passing through called out a single Pilot or Love's
  twice, two miles apart, as though they were two places. On a Chicago to Los
  Angeles run that was eight doubled stops. Each facility is now listed once,
  keeping whichever exit number was recorded for it.
- **Signaling for one stop no longer hides that you missed a same-named
  one.** The game works out whether you are taking a stop's exit or blowing
  past it. It compared stops by name, so signaling for any Love's Travel Stop
  counted as taking the exit for the Love's you had planned, and driving past
  your real planned stop went unmentioned. It now checks the actual stop.
- **A planned stop is no longer cancelled by a different stop with the same
  name.** Plans were remembered by name only, so on a route with four Love's
  Travel Stops, driving past the first one told you that you had driven past
  your planned stop and cancelled the plan -- even when the stop you actually
  planned was three hundred miles further on. Every same-name stop also
  announced itself as your planned stop, and pulling into any of them counted
  as arriving at your plan. Your plan now follows the one stop you chose.
  Plans saved by an earlier version still load, pointing at the soonest stop
  of that name you can still reach.
- **Every stop on your route is announced now, not just the first of each
  name.** Chains repeat -- a run from New York to Miami passes four different
  Love's Travel Stops -- and the game only ever called out the first one,
  then went quiet for the rest of the trip. On that route six of its
  twenty-five stops were never mentioned at all. Each stop is now tracked by
  where it actually sits, so all of them announce as you approach.
- **The delivery run no longer warns about a 15 mile per hour limit you
  never reach.** On the way in to a receiver you were told a facility gate
  was coming with a 15 limit, then that limit never arrived, because the gate
  sits in the last half mile of the highway while your destination exit comes
  a mile or more before it -- you left the road before the sign existed.
  Delivery runs now only warn about slower zones you will actually drive
  into. Pickups and facility approach roads still drive to the gate, and
  still get the warning.
- **Signaling for the destination exit no longer says the same thing twice.**
  When the exit callout had already announced adaptive cruise easing to ramp
  speed, pressing X repeated the whole sentence. It now just confirms the
  exit.
- **Entering a slower zone no longer sounds exactly like the warning for
  it.** Both announcements said the same thing -- "facility gate ahead, speed
  limit 15" -- so on the run in to a delivery you heard the gate limit two
  miles out, found the limit had not actually changed, and had no way to tell
  that one was a heads-up and the other was the change itself. The warning
  still says how far ahead the zone is; entering it now says "Entering
  facility gate zone, speed limit 15 now," matching the "End of" call you
  already hear on the way out. Construction, heavy traffic, and destination
  approach zones all read the same way.
- **Signaling for any exit now slows the truck, not just the delivery one.**
  Pressing X told you to slow to ramp speed, but adaptive cruise kept holding
  highway speed, so a truck stop or rest area exit went by at seventy while
  automatic control did nothing. Signaling now eases cruise to ramp speed for
  every exit, and a posted limit above ramp speed no longer cancels that out.
  Pressing X again to stay on the highway hands the open-road speed straight
  back.
- **State-line announcements no longer repeat at the next major city.** When
  the GPS calls out the real state boundary, passing the next city now says
  only the city and onward route instead of claiming that you crossed the same
  state line again.
- **The game no longer crashes when it is installed in a protected folder.**
  If you put Freight Fate somewhere Windows guards, such as Program Files, the
  game could not write next to itself and would crash the moment it tried to
  save, often right as you reached a facility. Freight Fate now notices when
  its own folder is read only and keeps your saves in your personal user
  folder instead, so the game saves and plays normally wherever you install
  it. Saves that are already beside the game are still used as before. Thanks
  to Ryan Bishop, [@ryanb96](https://github.com/ryanb96),
  [PR #92](https://github.com/orinks-games/Freight-Fate/pull/92).
- **The engine load now follows throttle smoothly.** Engine effort remains
  audible when you accelerate or ease off, while manual releases and
  adaptive-cruise corrections blend gradually instead of making the engine
  volume jump. Automatic shifts retain a brief, gentle unload and recovery.
  Thanks to TowerAlphaTheta15, [@TowerAlphaTheta15](https://github.com/TowerAlphaTheta15),
  [PR #89](https://github.com/orinks-games/Freight-Fate/pull/89).
- **Terminal weather now agrees with the live report on the road.** Time and
  weather uses the real station temperature even when live weather does not
  control the career calendar. If the first observation is still loading, the
  terminal says so instead of announcing a modeled temperature that may change
  when you start driving.
- **Interstates stop enforcing a side street's speed limit mid-route.** A
  handful of highway legs still carried a 25 to 40 mile per hour sample in
  the middle of the corridor, picked up from a nearby city street or ramp
  during the map sweep -- so the GPS would suddenly announce a 35 limit on
  the open interstate and enforcement would treat highway speed as
  speeding. Every such sample is gone (no US interstate mainline posts
  below 45), and an automatic check now keeps them from ever coming back.
  Small-town limits on US and state highways are real and unchanged. Thanks
  to Noel Romey, [@nromey](https://github.com/nromey),
  [PR #86](https://github.com/orinks-games/Freight-Fate/pull/86).

- **The destination exit can no longer show up a state early.** On routes
  that finish on rural highways, the game could announce the destination
  exit at the last big interchange anywhere along the way -- one delivery to
  Havre, Montana offered its "destination exit" in Wisconsin, over a
  thousand miles out, and taking it ended the trip and paid the load right
  there. The destination exit now only appears within the final miles of
  your route; where the last stretch has no signed interchange, you get the
  normal final approach to the facility instead.

- **Live weather no longer turns light haze into thick fog.** With real-world
  weather on, weather stations report haze or mist whenever they can see less
  than about seven miles, and the game treated every such report as dense fog:
  fog horns, a forty mile per hour safe speed, and near-zero visibility, often
  for an entire route on a humid summer night. The game now checks how far the
  station can actually see, and only genuinely low visibility becomes fog;
  ordinary haze plays as an overcast sky instead.
- **Highways no longer inherit city-street speed limits near their start
  and end.** On six hundred eighty routes, the posted limit spoken and
  enforced at the edges of a drive could come from a city street beside
  the highway instead of the highway itself, so an interstate might hold
  you to thirty miles per hour for miles of open road. Those stray
  readings are gone: the limit you hear at the wheel now matches the
  road you are actually on, and speeding enforcement judges you against
  that honest number. Thanks to Noel Romey, [@nromey](https://github.com/nromey),
  [PR #82](https://github.com/orinks-games/Freight-Fate/pull/82).
- **Careers from older versions now trade every cargo type at real market
  prices.** A career started before the cargo list grew to sixteen classes
  kept freight-market prices only for the original eight, so pay for the
  newer cargo types never rose or fell. Loading such a career now fills in
  the missing market prices, which also keeps cloud backups of these older
  careers in step with orinks.net.

## 1.8.3 - 2026-07-14

### Added

- **Cloud restores now get a second integrity check.** Beyond the server's
  signature, a restored profile has to pass the game's own sanity rules --
  wear between zero and one hundred, honest delivery counts, a fuel tank
  that fits in a truck. A file that fails is refused with a plainly spoken
  reason instead of being loaded, and saves from newer versions of the
  game still restore fine. Contributed by Noel Romey
  ([@nromey](https://github.com/nromey)) in [PR #76](https://github.com/orinks-games/Freight-Fate/pull/76).

### Fixed

- **Restoring a cloud backup works again.** Every new server-verified backup
  was wrongly refused with "failed its integrity check" the moment you tried
  to restore it. The refusal was the game's mistake, not a problem with your
  save. Restores of verified backups now complete normally.
- **Cloud backup now tells you when this computer needs to reconnect.** If
  orinks.net stops accepting this computer's sign-in, the cloud backup menu
  now says so and explains the fix, instead of wrongly reporting that your
  backups could not be reached.
- **Long deliveries are easier on the game while you drive.** The destination
  exit is now worked out once and remembered instead of being recalculated
  every moment of the drive, removing a heavy background load tied to a
  reported crash on coast-to-coast routes.

### Changed

- **Playing on more than one computer no longer signs the other one out.**
  orinks.net now gives each of your computers its own token: add a computer
  from the driver setup page and your other machines keep working. If the
  game says your sign-in is no longer accepted, it now points you to the
  computer list on the setup page to get a fresh token for that computer.

## 1.8.1 - 2026-07-13

### Fixed

- **The Mountain Grade driving track sounds right again.** The daytime
  mountain music bed has been replaced with a corrected recording, normalized
  to sit at the same volume as the rest of the soundtrack.

- **Controllers are left alone when controller support is off.** With the setting disabled, the game no longer starts up the controller system or grabs a connected pad; turning support on in Settings, Gameplay activates it, and turning it back off releases the controller again.
  Contributed by Brandon Cross ([@ironcross32](https://github.com/ironcross32)) in
  [PR #67](https://github.com/orinks-games/Freight-Fate/pull/67).

- **Engine sound now stays present through automatic gear changes.** Shifts still ease the engine tone briefly, without the repeated volume pumping that could sound like the engine was dropping out.

- **Starting the engine no longer dips in volume.** The running engine sound now
  meets the tail of the ignition sound at the same level, then settles smoothly
  down to idle instead of briefly dropping out. Contributed by Brandon Cross
  ([@ironcross32](https://github.com/ironcross32)) in [PR #66](https://github.com/orinks-games/Freight-Fate/pull/66).

- **Manual and automatic transmissions behave reliably on steep grades.** The
  diesel governor now holds a safe low-gear road speed without quietly damaging
  the engine, and automatic trucks avoid shifts that cannot pull the hill.
- **Transmission changes now apply when you return to an active drive.** The
  game announces the new automatic or manual mode instead of waiting until the
  next trip.
- **Destination signs no longer send you down an early exit.** Navigation now
  favors the interchange nearest the destination over an earlier sign that
  happens to mention the same city.
- **Speeding fines now follow you on bobtail runs.** Empty repositioning trips
  charge accumulated speeding-strike fines and announce the cost in the arrival
  summary instead of silently letting the fines disappear.

### Changed

- **The engine no longer jumps in volume the instant an automatic shift
  finishes.** It now eases back up to full pull over a brief moment, so completed
  shifts sound smooth instead of abruptly snapping back under load.
- **Route alerts no longer repeat at one mile.** Fuel stops, rest stops, and
  other actionable exits now speak once at five miles. State lines speak once
  as you cross them.
- **The soundtrack now uses the finished music throughout the game.** Menu,
  daytime driving, and nighttime driving tracks have been replaced with their
  full-quality versions, normalized to match the existing music. Urban Roll
  also joins the menu rotation as a separate track from its driving version.
- **Automatic shifting now follows real heavy-truck strategy.** Lower gears use
  progressive shift points, the starting gear responds to load and grade,
  light trucks can skip unneeded gears, and braking selects a useful lower gear
  instead of stepping through every ratio. Engine audio now unloads between
  gears instead of sweeping upward as one continuous high-pitched tone.
- **Freight Fate checks for updates again when you leave a terminal.** Returning
  to the main menu from a city terminal or pickup facility now starts a quiet
  background check, so an available update can be installed before you finish
  the session.

### Added

- **Online sharing now tells orinks.net which game version you are running.**
  When Profile sharing or cloud backup is on, each post carries the release the
  game was built from, such as a stable version or a nightly date. It is used
  only for moderation and troubleshooting, is never shown publicly, and the
  spoken "Hear what gets shared" disclosure now mentions it.

- **The major toll turnpikes now charge realistic tolls.** Running the Kansas
  Turnpike, the Oklahoma turnpikes, the New York Thruway, the Pennsylvania and
  Ohio turnpikes, the Indiana Toll Road, the Illinois Tollway, the Mass Pike, the
  Maine and West Virginia turnpikes now adds an estimated commercial toll to the
  run -- so a toll route is a real cost to weigh against the free way around.

- **The map explodes from 249 cities to 623, coast to coast.** Since the last
  stable release the drivable network has more than doubled: 623 cities to
  pick up and deliver in, joined by about 139,000 miles of real truck routes.
  Dead zones that used to have nothing drivable for hundreds of miles -- the
  mountain West, the northern plains, the Nevada Great Basin, Appalachia, the
  Gulf coast -- now connect city to city on the real roads, town by town.
  The entries below tour the new country region by region; each nightly
  snapshot's notes carried the town-by-town detail. Special thanks to Noel
  Romey, [@nromey](https://github.com/nromey), for the mapping work behind
  it in [PR #50](https://github.com/orinks-games/Freight-Fate/pull/50),
  [PR #51](https://github.com/orinks-games/Freight-Fate/pull/51),
  [PR #52](https://github.com/orinks-games/Freight-Fate/pull/52),
  [PR #58](https://github.com/orinks-games/Freight-Fate/pull/58), and
  [PR #68](https://github.com/orinks-games/Freight-Fate/pull/68). And watch your fuel
  out there -- some of the new country is a long way between diesel pumps.

- **New England and the Northeast fill in.** Rutland, Keene, Lewiston, and
  Barnstable bring Vermont, New Hampshire, Maine, and Cape Cod onto the map;
  Watertown and Jamestown open New York's north country and Southern Tier;
  Williamsport, Altoona, State College, and Meadville put the Pennsylvania
  mountains on real routes; and fourteen short-haul runs stitch the corridor
  from Boston and Manchester down through Providence, Hartford, and
  Philadelphia to the Chesapeake, including the Bay Bridge run to Dover and
  Salisbury and the New York Thruway up the Hudson Valley.

- **The Mid-Atlantic and Appalachia connect through the mountains.** The whole
  Interstate 81 freight run is drivable -- Staunton, Wytheville, Marion,
  Abingdon, Bristol, and Kingsport -- with Interstate 64 east over Afton
  Mountain into Richmond. The Kentucky parkways and the coalfields open
  thirteen storied mountain runs, from Pound Gap and the Cumberland Gap to the
  New River Gorge road to Beckley, with Paducah and Owensboro on the western
  parkways; Cumberland lands on the Interstate 68 climb over the Alleghenies;
  Lynchburg anchors US-460; and Jackson and Cookeville break the long
  Interstate 40 haul clear across Tennessee into real stops.

- **The Carolinas and the Southeast coast come together.** Durham and
  Spartanburg finish Interstate 85 through the Piedmont; Petersburg, Florence,
  and Lumberton close the Interstate 95 gap, so the East Coast's busiest
  freight run finally drives city to city; eastern North Carolina adds
  Greenville, Jacksonville, New Bern, and Rocky Mount; and Myrtle Beach brings
  the Grand Strand onto coastal US-17.

- **Georgia, Alabama, and Mississippi fill in from the mountains to the
  Gulf.** Interstates 75 and 85 stop in real towns the whole way -- Dalton,
  Cartersville, Valdosta, Tifton, Cordele, Opelika, and LaGrange -- Columbus
  and Albany open the wiregrass, and Dothan, the Peanut Capital, ties three
  states together. The Delta and the Blues Highway open at Greenville,
  Clarksdale, Oxford, Tupelo, Grenada, and Hattiesburg, while Gadsden,
  Cullman, Selma, Natchez, and Panama City round out the Deep South. Louisiana
  fills in too, from Ruston and Natchitoches to Hammond and bayou-country
  Houma, with Alexandria anchoring the middle of Interstate 49.

- **Florida runs border to border.** Pensacola and Crestview break up the
  Panhandle, Ocala and Palm Coast fill the peninsula's spine, and Daytona
  Beach, Sarasota, North Port, Fort Myers, and Naples line both coasts -- with
  the run from Naples to Miami crossing the Everglades on Interstate 75's
  Alligator Alley, no services for eighty miles.

- **Arkansas and the Ozarks open up.** Fayetteville and Bentonville climb the
  real Boston Mountains, Jonesboro reaches the rice-country Delta, Harrison
  and Mountain Home carry the winding Ozark truck routes, Hot Springs crosses
  the Ouachita ridges, the Interstate 49 line finishes across the state from
  Fort Smith to Texarkana, and Pine Bluff, El Dorado, Stuttgart, and
  Russellville tie the farm and timber country into Texas, Louisiana, and
  Tennessee.

- **Texas and Oklahoma become town-by-town country.** The US-287
  Ports-to-Plains spine runs from San Antonio clear to Denver through Vernon,
  Childress, Dumas, and the Oklahoma panhandle; US-281 and US-75 open
  north-south routes beside the crowded interstates; Temple completes the
  Interstate 35 spine; Uvalde and Eagle Pass open the border country; the
  plains add Plainview, Big Spring, Brownwood, and Pampa; Longview takes its
  place on Interstate 20 toward Shreveport; and Oklahoma links up through
  Stillwater, McAlester, Muskogee, Durant, Ardmore, Bartlesville, and Ada.

- **The Great Plains ladder is complete.** Interstate 80 across Nebraska is
  now continuous past the hundredth-meridian marker, Kansas adds Lawrence,
  Emporia, Hutchinson, Great Bend, and Liberal, the Dakotas add Jamestown,
  Pierre, and Aberdeen, a Black Hills freight run links Cheyenne through
  Scottsbluff country to Rapid City, and the Missouri and Iowa heartland
  fills in from Sedalia and Saint Joseph up through Ames, Fort Dodge, and
  Mason City, with Cape Girardeau and Poplar Bluff anchoring southeast
  Missouri.

- **The Midwest and Great Lakes lattice comes together.** Twenty-nine new
  cities across five states -- from Springfield, Flint, and Kalamazoo to the
  Upper Peninsula's Marquette and Sault Ste. Marie and the Iron Range's
  Hibbing -- plus central Indiana around Indianapolis, Ohio's Zanesville,
  Mansfield, and Youngstown, Wisconsin's Fox Valley, and nineteen short runs
  lacing Detroit, Toledo, Fort Wayne, and Milwaukee together. Terre Haute and
  Effingham break the long Indianapolis-to-St. Louis drive, Dubuque anchors
  the US-20 Mississippi crossing, and every city comes with real, named
  freight facilities: haul taconite pellets from the Hibbing mine, steel out
  of Gary Works, and new Subarus from Lafayette. Contributed by Liam Erven
  ([@liamerven](https://github.com/liamerven)) in [PR #43](https://github.com/orinks-games/Freight-Fate/pull/43).

- **The Rockies and the Great Basin connect end to end.** Wolf Creek Pass and
  the Million Dollar Highway open Colorado's steepest crossings, with grades
  past eleven percent over the San Juans; Durango and Farmington meet at the
  Four Corners; fifteen runs link the northern Rockies from Missoula to Miles
  City; the Silver Valley opens Interstate 90 over Lookout Pass; Rawlins
  lands on Interstate 80 across Wyoming, with Logan and Moab opening Utah;
  New Mexico adds Hobbs, Alamogordo, Roswell, Carlsbad, and Socorro; and
  Nevada's US-93 and US-50 -- the Loneliest Road in America -- cross the
  Great Basin through Ely, Austin, and Fallon.

- **Arizona and the desert Southwest fill in.** The Verde Valley, the Beeline
  Highway, and Route 66 country open Camp Verde, Sedona, Payson, Winslow, and
  Holbrook, with Prescott in the highlands; copper country climbs US-60
  through Globe and the Salt River Canyon to Show Low; the border adds
  Nogales, Sierra Vista, and Douglas; the Colorado River runs from Lake
  Havasu City down to Yuma; and US-89 reaches Page and Lake Powell across
  the Navajo Nation.

- **California and the Pacific Northwest round out the coast.** San Luis
  Obispo and Santa Barbara complete the US-101 coast run, Modesto and Merced
  fill Highway 99 through the Central Valley, the Redwood Highway reaches
  Eureka, the eastern Sierra opens the long US-395 run beneath Mount
  Whitney, the Cajon Pass climb connects Riverside to Victorville, and
  fourteen Cascade-pass runs -- Stevens, White, and Santiam, real grades with
  brake checks -- tie Seattle, Tacoma, and Salem to the Columbia Basin, with
  The Dalles seating the Columbia Gorge.

- **Drive the Overseas Highway to Key West.** Key West joins the map at the very
  end of the road, reached from Miami down US-1 through the Florida Keys -- Key
  Largo, Islamorada, Marathon, Big Pine Key -- across the Seven Mile Bridge, all the
  way to the southernmost point in the continental United States.

- **You can now cross the Chesapeake Bay Bridge-Tunnel.** Cape Charles joins the
  map on Virginia's Eastern Shore, and the run north from Norfolk takes you out
  across the seventeen-mile Bridge-Tunnel -- diving into two tunnels beneath the
  shipping channels, past Sea Gull Island, out to where no land is visible in any
  direction, and up the Delmarva peninsula to Salisbury. It carries a hefty truck
  toll, because of course it does.
- **Cloud backups now prove they were accepted by orinks.net before restore.**
  orinks.net validates and signs each private revision, and Freight Fate verifies
  that signature before touching a local career. Public Profile sharing stays
  separate: detailed career statistics come only from an accepted backup and
  are omitted when no verified revision exists.

- **Optional Profile sharing stays quiet during driving.** With Profile sharing on,
  Freight Fate can queue automatic road-journal posts,
  achievements, and updates for the public driver profile. Detailed career
  statistics come only from the latest private Cloud Backup accepted by
  orinks.net. Offline posting retries in the background and never adds a spoken
  interruption while driving.

- **Exits now come straight from real-world maps -- with the correct exit names
  and numbers.** On the Interstates, your stops and your destination exit are
  announced with their actual exit number and name and the places they point to --
  "Exit 33, Yemassee," "toward Beaufort and Port Royal," "Durham" -- taken directly
  from real map data, so you always know the right exit to take. This now covers
  the whole Interstate network.

- **Routes now carry the real posted speed limits.** Instead of estimating a
  limit from the road type, every leg on the map now carries the actual posted
  speed limits from map data (interstates, US highways, and more), so your truck
  runs the real limit on the road it is driving. Rural roads without published
  limits still fall back to a sensible estimate.

- **Truck-stop names read cleanly now.** Spoken stop names across the map no
  longer include bare initials like "T A" or leftover store numbers.

- **Every run now names the real towns and country you pass.** Those are
  checkpoints -- the actual places along a route, spoken as you reach them -- and
  the map went from about 550 of them to over 2,500. Instead of empty miles, a
  haul now names the towns you pass and the state lines along the way, all from
  real geography, and real elevation data means the grades are felt and not
  smoothed flat. Thanks to Noel Romey, [@nromey](https://github.com/nromey),
  [PR #50](https://github.com/orinks-games/Freight-Fate/pull/50).

- **Over 1,700 truck stops are now named along your routes.** Real travel centers, truck
  stops, and rest areas -- Love's, Pilot, Flying J, TA, Petro, and independents
  -- each pinned to its real location, so every route now has at least one place
  to fuel or park, and even the emptiest rural stretches point you to a real
  diesel pump you can pull a rig into. For now these are just named on the map;
  making them do something -- rest, showers, repairs, and buffs -- comes in a
  later update. Thanks to Noel Romey, [@nromey](https://github.com/nromey),
  [PR #50](https://github.com/orinks-games/Freight-Fate/pull/50).

- **Some hauls now offer more than one way to drive them.** Where two real truck
  routes reach the same place, the map keeps both, so a run can offer a choice --
  a faster interstate or a shorter back road -- instead of a single fixed path.
  Is it winter, and you'd rather take a southern route than a mountainous
  northern one? We've got you covered. Thanks to Noel Romey,
  [@nromey](https://github.com/nromey), [PR #50](https://github.com/orinks-games/Freight-Fate/pull/50).

- **See who else is hauling right now with the new drivers board.** A new
  Drivers online item in the main menu reads the live board from orinks.net:
  each driver's name, what they are doing, their route and cargo, and how
  fresh the report is. If you want to appear there yourself, set up sharing
  under Settings, Online. Drivers are Orinks accounts now: the game opens
  the orinks.net setup page where you sign in, pick your driver name and
  whether the public board lists you at all, and copy a Driver ID and a
  one-time posting token; back in the game you paste each from the
  clipboard and choose Connect and save. Nothing is ever shared before
  that, the game speaks exactly what gets shared, and only broad in-game
  activity goes out, like "Driving: Chicago to Dallas, steel coils", never
  your save files, real name, or location. You leave the board within
  minutes of going off duty or turning sharing off.

- **Your careers can now back up to the cloud.** Turn on Back up saves to
  your Orinks account under Settings, Online, and after each game save your
  career quietly uploads to your own orinks.net account -- so a dead hard
  drive no longer means a dead career, and you can pick up the same driver
  on another computer. It uses the same one-time sign-in as the drivers
  board, nothing extra to set up, and backups are private to your account:
  they never appear on the drivers board or anywhere public. The new
  Restore a cloud backup menu reads your backups aloud, newest first, and
  brings one onto this computer -- keeping the save it replaces beside it
  as a fallback. Played the same career on two computers? The game notices
  and asks which copy should win instead of silently overwriting either.
  Cloud backup is off until you turn it on.

- **The map now has real time zones, and your clock changes as you cross
  them.** Drive west out of Tennessee on I-40 and you will hear "Crossing
  into Central Time. It is now 2:15 PM." With terse speech on, it is just
  "Central Time." Every spoken clock -- rest stops, sleep, city arrivals, the driving
  status screens -- now reads the local time where your truck is, and the
  clock readouts name the zone, like "2:15 PM Central Time". Delivery
  deadlines are also quoted the way a real receiver would say them: in the
  destination's local time, like "deliver by 6 PM Central Time tomorrow", on
  the dispatch job details and in the driving deadline readouts. Hours of
  service, deadlines, and pay are untouched; only what the wall clock says
  changes. Boundaries follow the real lines, including split states like
  Tennessee, Kentucky, Indiana, the Florida panhandle, and far west Texas.

### Changed

- **Pausing now takes you off the live drivers board.** The pause menu used to
  keep you listed as "Paused"; now it counts as going off duty, so the public
  board only shows drivers who are actively hauling. A quick pause and resume
  will not bounce you off the board, and Discord presence still shows
  "Paused" to your friends while the menu is open.

- **Dispatches and route planning now always name the state with each
  city.** A job reads as "to McCall, Idaho" even when no other McCall
  exists, so an unfamiliar town still tells you roughly where you are
  headed. And each route option now says which cities it passes through
  right in the option itself -- "through Boise, Idaho, then McCall" --
  instead of only in the F1 help, so you can weigh routes the same way
  the end-of-trip summary describes them. Thanks to a player suggestion.

- **Automatic direction changes can now be simple or deliberate.** Simple is the
  casual default: keep holding the control after the truck stops to change
  between forward and reverse. Deliberate keeps the safer two-step behavior from
  the previous snapshot: stop, release the control, then press it again. Choose
  the style you prefer under Settings, Gameplay. Manual shifting is unchanged.

- **Online settings are now gathered in one place.** The Discord presence
  toggle moved from Settings, Gameplay to Settings, Online, alongside the
  drivers board and the new cloud backup options. And before you have set
  up your Orinks sign-in, the first Online item now says "Driver profile:
  not set up" -- setting it up is one step that unlocks both the drivers
  board and cloud backup.

- **The horn sounds like a real horn held down.** Instead of restarting the
  same short honk over and over, holding the horn now sustains one steady blast
  for as long as you press it, and when you let go the horn rings out and fades
  the way a real one does rather than cutting off abruptly. Pressing the horn
  again while it is still sounding no longer layers a second horn on top.
  Contributed by Brandon Cross ([@ironcross32](https://github.com/ironcross32)) in
  [PR #55](https://github.com/orinks-games/Freight-Fate/pull/55).

- **Abandoning a job now asks you to confirm.** Choosing Abandon job from the
  pause menu opens a Yes or No prompt that starts on No, so you have to arrow
  down to Yes to actually give up the load and pay the penalty. Choosing No
  takes you straight back to the pause menu with the job intact. Contributed
  by Brandon Cross ([@ironcross32](https://github.com/ironcross32)) in
  [PR #53](https://github.com/orinks-games/Freight-Fate/pull/53).
- **Cities that share a name now always say their state.** With two Jacksons,
  two Portlands, and three Springfields on the map, dispatch offers, route
  planning, GPS announcements, and delivery summaries now say "Jackson,
  Mississippi" or "Jackson, Michigan" wherever the bare name would be
  ambiguous. Cities with a unique name keep their short spoken form, and a few
  places that used to stutter their state twice, like "toward Jackson,
  Michigan, Michigan", now say it once. Existing careers and saved trips carry
  over unchanged. Contributed by Noel Romey
  ([@nromey](https://github.com/nromey)) in [PR #46](https://github.com/orinks-games/Freight-Fate/pull/46).

- **Job details always tell you the state.** Not sure where Baton Rouge is?
  Open a job's detail view from the dispatch board and the origin and
  destination lines now always include the state, like "in Baton Rouge,
  Louisiana", even for cities with a unique name. Board offers stay short.

### Added

- **The Great Lakes split into three regions that each feel like
  themselves.** The Upper Midwest covers Minnesota, Wisconsin, and Michigan's
  Upper Peninsula; the Great Lakes keeps the lower-lakes industrial belt from
  Chicago through Detroit to Buffalo; and the new Corn Belt takes interior
  Illinois, Indiana, and southern Ohio. Each has its own weather, fuel
  prices, freight market flavor, and road hazards, so a winter run out of
  Duluth no longer sounds like a summer haul into Cincinnati. Contributed by
  Liam Erven ([@liamerven](https://github.com/liamerven)) in [PR #43](https://github.com/orinks-games/Freight-Fate/pull/43).

### Fixed

- **A few routes now name the right highway.** On the runs from Denver to Salt
  Lake City, Santa Rosa to Stockton, and Clarksville to Huntsville, the game
  announced a highway the route never actually takes; it now names the road you
  are really driving.
- **The truck now warns you while the engine is over-revving, instead of
  surprising you with damage at delivery.** Holding the engine at redline --
  easiest to do by backing up fast for a long stretch -- quietly ground the
  truck down, and the first you heard of it was a big damage number on the
  end screen. Now a warning sounds and the game tells you the engine is
  taking damage and the current total, repeating while it goes on, so you
  can ease off and slow down before the repair bill grows. Thanks to a
  player report.

- **Online setup now tells you when orinks.net refuses your pasted
  credentials, instead of blaming your connection.** If the server answered
  but did not accept the Driver ID and token, the game said "could not reach
  orinks.net, check your connection," sending you off to troubleshoot a
  network that was fine. It now says the credentials were not accepted and
  asks you to re-copy them from the setup page. The token paste item also
  checks that the pasted text looks like a real driver token -- they always
  start with the letters F F D and an underscore -- and says so when it does
  not, catching a wrong copy before anything is sent. Thanks to a player
  report.

- **Music keeps playing while the game is paused.** If a music track ended
  while you sat on the pause menu -- or in settings, help, or any other menu
  over a drive -- the music went silent until you resumed driving. The next
  track now starts on its own, so a long pause no longer means a quiet cab.

- **Pasting your Driver ID and token now works on Mac.** Setting up the
  online drivers board no longer crashes the game, or silently does
  nothing in the downloadable app, when you paste your Driver ID or
  driver token from the clipboard on a Mac. Thanks to a player report.

- **No more "brake now" ambushes on the way to a pickup.** The short
  facility access road you deadhead down to reach a shipper no longer
  springs road hazards or emergency-braking events; those belong on the
  open road, not on a two-minute crawl at yard speeds. Thanks to a player
  report.

- **Reconnecting a controller no longer crashes the game or leaves it
  half-working.** Unplugging a pad -- or having it change to another device and
  come back over Bluetooth -- could crash the game outright, or bring the
  controller back with the triggers and bumpers dead so you could steer but not
  brake. The game now recovers from the hot-plug instead of crashing, and
  fully re-acquires the controller when it returns -- even when the system hands
  it back under a new identity -- so braking, throttle, and the bumpers work
  again right away. Contributed by Brandon Cross
  ([@ironcross32](https://github.com/ironcross32)) in [PR #60](https://github.com/orinks-games/Freight-Fate/pull/60).

- **Controller toggle actions no longer fire twice.** On some controllers --
  notably the Xbox Elite -- setting or releasing the parking brake, or starting
  or shutting down the engine, could trigger twice from a single press, so the
  action immediately undid itself. Each button press now counts once, even when
  the controller reports itself to the system more than once. Contributed by
  Brandon Cross ([@ironcross32](https://github.com/ironcross32)) in [PR #60](https://github.com/orinks-games/Freight-Fate/pull/60).

- **Construction zones no longer stack or chain together.** Slow zones were
  placed independently, so a construction zone could land inside another
  one, or two could start back to back with no open road between. Zones now
  keep at least eight miles apart, so "end of construction" always means
  open road ahead. Thanks to a player report.

- **Metric mode now covers the whole weather report.** With units set to
  kilometers, pressing V mid-drive still read the temperature in Fahrenheit
  and low visibility in miles. Temperatures now speak in Celsius and
  visibility in kilometers everywhere weather is described: the V report,
  weather-change announcements while driving, trip resume summaries, and
  the terminal weather check. Thanks to a player report.

- **The engine sound now stops when you shut down to sleep.** Going to sleep
  at a rest stop, motel, or on the shoulder shuts the engine down, but the
  engine sound kept playing over the night and after you woke, as if the
  truck were still idling with the engine off. The shutdown is now heard
  when it happens, and the idle goes quiet with it. Thanks to Darren Duff
  for the report.

- **Using the accelerator to brake in reverse no longer speeds you up.** In an
  automatic, pressing the accelerator while rolling backward is meant to slow
  and stop the truck, but at higher reverse speeds it could push you faster
  instead. It now brakes reliably all the way to a stop. Contributed by
  Brandon Cross ([@ironcross32](https://github.com/ironcross32)) in [PR #59](https://github.com/orinks-games/Freight-Fate/pull/59).

- **Adaptive cruise no longer revs the engine when you press the clutch to
  shift.** With a manual gearbox, holding the clutch under cruise control used
  to send the engine screaming toward the redline. Now cruise eases off the
  moment the clutch goes in, the engine settles back toward idle, and the speed
  is picked back up smoothly once you let the clutch out. Contributed by
  Brandon Cross ([@ironcross32](https://github.com/ironcross32)) in [PR #59](https://github.com/orinks-games/Freight-Fate/pull/59).

- **The engine no longer re-cranks when you pick a trip back up.** Resuming a
  saved haul with the engine already running -- or coming back from a menu
  mid-drive -- used to replay the ignition sound as if you had just turned the
  key. Now the running engine simply fades back in, and the starter is heard
  only when you actually start the engine yourself. When you do start it, the
  crank now blends smoothly into the running engine instead of being drowned
  out the instant it catches. Contributed by Brandon Cross
  ([@ironcross32](https://github.com/ironcross32)) in [PR #55](https://github.com/orinks-games/Freight-Fate/pull/55).

- **Your truck no longer idles all night while you sleep.** Bedding down for
  the night -- at a rest stop, in the sleeper berth, in a cramped lot, or on
  the shoulder -- now shuts the engine down first, and you will hear "You
  shut down the engine" as you turn in. When you head back to the road,
  start the engine as usual. Thanks to Bartholomue.

- **Updating the game on Mac now works.** Downloading an update used to end
  with "the download failed" and nothing installed, leaving Mac players to
  fetch each new version by hand. The updater now understands the Mac app
  bundle: it swaps in the new app after the game closes and reopens it for
  you, just like on Windows and Linux. Your saves are untouched. Thanks to
  vlad-a-c.

- **Asking for job details on Back to terminal no longer crashes the game.**
  On the dispatch board, pressing F1 while on the Back to terminal entry used
  to crash; it now simply reads the entry back, like any other menu item.
  Thanks to Brandon Cross, [@ironcross32](https://github.com/ironcross32),
  [PR #47](https://github.com/orinks-games/Freight-Fate/pull/47).

- **Resuming a trip no longer repeats a stop it already called out.** When you
  continued a saved run, the game could re-announce a truck stop or rest area
  just ahead that it had already told you about before you saved. It now
  remembers what it said and stays quiet. Thanks to Noel Romey,
  [@nromey](https://github.com/nromey), [PR #50](https://github.com/orinks-games/Freight-Fate/pull/50).

## 1.8.0 - 2026-07-05

### Added

- **Report a problem straight from the main menu.** A new Report a problem
  option, just above Quit, opens the Freight Fate bug report page on GitHub
  in your web browser and tells you where to find your game log: the file
  game.log in the logs folder next to the game. The game now also keeps the
  previous run's log as game.prev.log, so if the game crashes, the evidence
  survives restarting it to file the report. Crashes inside the game's audio
  and video engines, which used to vanish without a trace, are now written
  into the log as well.

- **Game controllers are now supported, alongside the keyboard.** Plug in an
  Xbox, PlayStation, or other compatible controller and drive by feel: the right
  and left triggers are the gas and brake, the left stick steers, the left bumper
  is the clutch, and the A and X buttons shift up and down. Menus map to the
  D-pad, the A button confirms, the B button goes back, and the Back button reads
  controller help. The first controller is picked up automatically, hot-plugging
  and unplugging are detected mid-game (unplugging pauses the drive), and spoken
  prompts name controller buttons when you are on a pad and keys when you are on
  the keyboard. Turn it off under Settings, Gameplay, Controller. The keyboard
  always stays active. Thanks to Brandon Cross, [@ironcross32](https://github.com/ironcross32),
  [PR #28](https://github.com/orinks-games/Freight-Fate/pull/28).

- **Set the parking brake to let time pass while you wait.** Pressing your
  parking brake while stopped now means deliberate waiting: the clock runs at
  double your trip pacing -- weather blows through, daylight comes, and dock
  time passes without the game ever dropping to real time. Pressing it again
  to leave returns to normal pacing instantly. Only your own brake press arms
  the fast-forward; the brake the game sets for you at trip start or after a
  rest stop never does, so pre-trip setup stays cheap. Each pacing setting
  keeps its relative feel while waiting: relaxed 20 times, standard 40,
  fast 80.

- **The Pacific Northwest fills in with eight new cities.** Tacoma, Everett,
  Olympia, Bellingham, and Yakima in Washington and Medford, Roseburg, and
  Pendleton in Oregon join the map with truck-routed corridors, real named
  ports, mills, and freight facilities, and real truck stops along the way.
  The region finally has short local runs -- Seattle to Tacoma is a
  34-mile hop instead of nothing closer than Portland -- and the empty I-84
  corridor gets its first stop at Pendleton. Thanks to Liam Erven,
  [@liamerven](https://github.com/liamerven), [PR #32](https://github.com/orinks-games/Freight-Fate/pull/32).

- **Appalachia, the Heartland, and the Southern Plains grow by eighteen
  cities.** Appalachia becomes a real Valley-and-Ridge region: Asheville,
  Johnson City, Beckley, Harrisonburg, Winchester, and Hagerstown line the
  I-81, I-77, and I-40 mountain corridors, Roanoke gains its rail yard and
  distribution work, and the western reaches of Virginia, North Carolina, and
  Maryland now count as Appalachian country. The Heartland adds Sioux City,
  Grand Island, North Platte, Columbia, Joplin, and Rolla along I-70, I-29,
  I-80, and I-44; the Southern Plains add Salina, Dodge City, Garden City,
  Enid, Lawton, and San Angelo with their grain, beef, and oilfield freight.
  Every new city carries real named facilities and every corridor has named
  truck stops. Thanks to Liam Erven, [@liamerven](https://github.com/liamerven),
  [PR #36](https://github.com/orinks-games/Freight-Fate/pull/36),
  [PR #37](https://github.com/orinks-games/Freight-Fate/pull/37), and
  [PR #38](https://github.com/orinks-games/Freight-Fate/pull/38).

### Fixed

- **Switching screen readers no longer leaves the game silent.** The game now
  notices within a few seconds when your screen reader closes or changes, for
  example going from NVDA to Narrator and back to NVDA, and reconnects its
  speech to whichever voice is running, telling you which one it picked.
  While Narrator is running, the game keeps its own Windows voice so that
  moving through menus still cuts speech off crisply; Narrator itself only
  carries the game's speech as a last resort when no other voice on the
  machine works. This also
  works if you start the game before your screen reader: speech simply
  begins once the screen reader is up. Your speech rate, voice, and separate
  event voice settings carry over to the reconnected voice automatically.

- **Release archives no longer ship the build machine's log.** The packaging
  smoke check writes a log inside the build folder; it is now stripped
  alongside saves before archiving, so a fresh download starts with an empty
  logs folder instead of a confusing leftover run.
- **Save migration now explains itself.** When the game folds an old save
  folder into the active one on first run, it writes what moved from where
  to the game log and leaves a small saves-moved.txt breadcrumb at the old
  location, so an unexpectedly familiar career is traceable instead of
  haunted.
- **Spoken help now teaches the W and Q gear keys everywhere.** The engine
  start walkthrough, the transmission setting, and the manual-transmission
  page of How to play still told manual drivers to shift with the number
  row; they now describe holding the clutch and pressing W to shift up and
  Q to shift down, matching how the truck actually shifts. The left and
  right arrows also now toggle the Haptics setting like every other
  gameplay setting row, instead of doing nothing there.
- **Getting up to highway speed no longer costs an hour of game time.** Truck
  physics runs in real time so shifting and braking stay playable, but the
  clock billed every real second at full trip pacing -- so the couple of real
  minutes a loaded rig needs to work through the gears cost most of a game
  hour, burning daylight, deadline, and duty clock. Clock compression now
  ramps with road speed: near real time while stopped or maneuvering, your
  full pacing setting once at cruise. Distance, fuel, fatigue, and the hours
  of service ledger all follow the same effective rate, so the simulation
  stays consistent -- acceleration now costs about five game minutes instead
  of forty-five.
- **The dispatch board no longer offers trivially short hauls.** Because each
  city stands for a whole freight area, a job to a neighbor under 25 miles was a
  pointless across-town hop; the board now skips those destinations and fills
  from real routes instead. Contributed by Liam Erven
  ([@liamerven](https://github.com/liamerven)) in [PR #29](https://github.com/orinks-games/Freight-Fate/pull/29).
- **The dispatch hours warning now respects a fresh clock.** Sleeping off your
  hours before visiting the dispatch board no longer leaves every long haul
  flagged with "may not fit your duty clock." The warning compared your time
  until the next HOS limit against the route's full legal plan -- including the
  overnight sleeps every multi-day run needs anyway -- so it fired even right
  after a reset. It now only warns when hours already spent this shift would
  force an extra rest that fresh hours would avoid, and the board note says
  sleeping first will clear it.
- **Trucks into New York now take the George Washington Bridge, not the Holland
  Tunnel.** New York freight now routes to the Hunts Point market in the Bronx
  over the GWB on I-95 -- the Hudson crossing a full-height rig can legally use
  -- instead of the height-restricted Holland Tunnel that I-78 feeds into. Trips
  from New Jersey and Pennsylvania have realistic mileage and exit cues as a
  result.
- **Truck speed limits are now capped in Oregon and Idaho too.** Posted limits
  on those states' fastest roads are held to the legal truck maximum (65 in
  Oregon, 70 in Idaho), matching the existing handling for California and other
  truck-restricted states.
- **Control now stops speech in menus too, not just while driving.** Left or
  Right Control already silenced the driving event voice; it now also stops the
  current speech in every menu and in the how-to-play reader, so a long readout
  -- job details, cargo loading, a full help page -- can be cut short with the
  same key everywhere. Contributed by Liam Erven
  ([@liamerven](https://github.com/liamerven)) in [PR #26](https://github.com/orinks-games/Freight-Fate/pull/26).
- **Dispatch, garage, and driving tools feel clearer.** F1 on a dispatch job now opens a
  reviewable job-detail view with line-by-line facts, long-haul pay has a stronger
  floor, drive-start speech is shorter in terse mode, the horn loops while held,
  truck and upgrade wording is clearer, and the garage can service tire wear and
  wash road grime.
- **Reverse now has its own backing cue.** Shifting into reverse with the engine
  running now starts a backing loop through the main audio backend, and automatic
  reverse selection still gets a spoken confirmation. Thanks to ashleygrobler04,
  [@ashleygrobler04](https://github.com/ashleygrobler04), for the original
  reverse-loop [PR #24](https://github.com/orinks-games/Freight-Fate/pull/24).
- **Lane drift now cues direction before the rumble strip.** When lane drift is
  enabled, a short beep now plays from the side you drift toward, and a dedicated
  centered-lane chime confirms when you are back in the lane.
- **Hazard clears are easier to hear, and speech backs off faster.** Passing a
  road hazard now plays a short achievement-like confirmation sound, and urgent
  events plus driving warnings clear stale spoken messages so old alerts do not
  keep piling up. The brake-now hazard warning cue was also remade as a short,
  louder alert.
- **First-rig menu music refreshed.** The first-owned-truck menu bed now uses
  a cleaner, longer copy and plays for its full length before the menu rotation
  advances.
- **Driving realism polish.** Metric speed warnings,
  speeding strikes, trooper stops, cruise messages, and the speed-limit key now
  use the selected units consistently. Missed destination exits reroute you via
  a safe turnaround instead of telling you to reverse down the highway, and
  recovery no longer loops gate-speed tickets. Dispatch warns when your current
  hours are too short for a load, including when every listed job is risky.
  Bobtail repositioning now counts as off-duty personal conveyance, dispatch
  board facility names are less repetitive, impossible short delivery summaries
  are floored to a practical road time, and automatic shift audio no longer
  flares at full throttle during gear changes.
- **Engine brake and throttle no longer fight each other.** The engine brake now
  refuses to switch on while you are accelerating, and pressing the accelerator
  turns it back off so the truck can make power normally.
- **Destination exits keep the route status honest.** Taking a delivery exit now
  clears the remaining route miles before the dock menu opens, and the GPS no
  longer repeats the destination exit with a second generic interchange cue.
- **Real posted speed limits win near cities.** City approaches still use a
  slower fallback when the route has no posted speed-limit sample, but real
  baked `maxspeed` data is no longer capped just because the route is near a
  city.
- **Truck speed limits now respect state caps.** Baked route speed-limit data
  now applies lower truck maximums in states that cap commercial trucks below
  the general posted limit, and reversed routes read the correct limit profile.
- **Stops no longer announce speculative truck parking.** If a stop's parking
  is confirmed, that still gets spoken; otherwise speculative parking wording
  is dropped from route cues so the game just announces the stop.
- **Adaptive cruise starts slowing before big speed-limit drops.** When the
  posted limit ahead falls sharply, adaptive cruise now looks far enough ahead
  to begin braking before the lower-limit point instead of waiting until the
  truck is already in the slower stretch. Pressing Space while cruise is on now
  also includes the cruise set speed in the speed readout.
- **Adaptive cruise no longer gets you fined while braking for a lower limit.**
  When the posted limit drops sharply, cruise now gets a clean chance to slow
  the truck instead of letting the speeding timer fire while it is already
  braking down.
- **Route status explains road grade clearly.** Pressing R now reports the
  current grade as a percent and uphill, downhill, or level instead of saying
  the vague phrase "Grade level."
- **Delivery windows match the slower, real route model.** New dispatch
  deadlines now use the route's posted-limit profile, city approaches, facility
  gates, HOS breaks, sleep, and practical slack instead of a flat mileage
  average. Older active trips that were saved under the faster estimate get a
  one-time fair deadline floor when they resume, so a source update does not
  make an in-progress load suddenly late.
- **Metric weather readouts use metric safe speed.** Pressing V with metric
  units enabled now reports the weather safe speed in kilometers per hour.
  Contributed by ashleygrobler04 ([@ashleygrobler04](https://github.com/ashleygrobler04))
  in [PR #22](https://github.com/orinks-games/Freight-Fate/pull/22).
- **No more "dot dot" at the end of menu items.** A menu or list item that was
  already a full sentence (like a settlement summary line) got a second period
  appended before its "N of M" position, which a screen reader voiced as "dot
  dot". The readout now adds a period only when the text does not already end
  in one.
- **You can always find somewhere to sleep.** A sleep option is now reachable
  at any time, so the hours-of-service clock can never strand you with nowhere
  legal to stop. Stopped on the open road with no route stop nearby, you can
  pull over and sleep on the shoulder (poor rest, possible parking ticket);
  the wording escalates when an HOS limit is closing in with no reachable stop.
  Any break/fuel stop you reach -- even one with no sleeper facility -- now
  offers an emergency sleep in the lot: a legal 10-hour reset with poor, cramped
  rest. The "no stop visible" warning also names the shoulder-sleep out, so it
  is a plan rather than a panic. (Proper sleeper stops still give the best,
  fully-rested 10-hour sleep.)
- **The automatic no longer gears up while you brake.** Braking from speed could
  trigger an upshift because the box only watched engine RPM; it now holds the
  gear for engine braking and downshifts cleanly as you slow to a stop.
- **"Air pressure ready" no longer repeats back to back.** The parking-brake
  release threshold sat exactly at the compressor cut-in pressure, so the ready
  state flickered every 100-125 psi cycle and re-announced. The cue now fires
  once, only while the parking brake is actually set (its whole purpose is
  "you can release it now"), and only re-arms after a genuine low-air depletion.
- **Snapshot players move to stable when it catches up.** On the preview
  snapshot channel, the game now offers the stable release whenever it is as
  new as -- or newer than -- the latest nightly, so once those changes ship in
  a stable build you converge back onto stable instead of being left on an
  equivalent nightly.
- **The low-air alarm now sounds on a cold start.** Starting the engine for
  the first time with the air tanks low used to stay silent; the warning now
  plays as soon as the engine is running with pressure below the threshold,
  so you know to wait for the compressor before releasing the brakes. Thanks
  to hannes16, [@hannes16](https://github.com/hannes16),
  [PR #35](https://github.com/orinks-games/Freight-Fate/pull/35).
- **Erie and Evansville moved to their right regions.** Erie sits on the Lake
  Erie shore between Buffalo and Cleveland, so it is now Great Lakes country
  rather than Appalachia; Evansville, down on Indiana's Ohio River border, is
  now the Mid-South rather than the Great Lakes. Spoken region names, weather
  flavor, and regional hazards on runs through both cities now match the
  geography. Thanks to Liam Erven, [@liamerven](https://github.com/liamerven),
  [PR #33](https://github.com/orinks-games/Freight-Fate/pull/33).

### Fixed
- **Exit warnings now arrive early enough to act on.** At highway speed on
  standard or fast pacing, the destination exit callout used to fire so close
  that by the time it finished speaking the ramp was gone. The warning
  distance now grows with your speed and pacing, so you always get roughly
  the same amount of real listening and braking time, and the exit can be
  armed as soon as you hear the callout.
- **Exit announcements no longer say the same name twice.** Messages like
  "missed exit 5B for exit 5B" and "Signaling for the exit for the warehouse,
  destination exit for the warehouse" now speak each exit and facility name
  exactly once. Distances also read naturally: "in 1 mile" instead of
  "in 1 miles".

### Changed
- **Career stats at the terminal is now a browsable list.** Instead of one
  long spoken paragraph, arrow through your level, reputation, deliveries,
  lifetime miles, and earnings one line at a time; Enter repeats a line. The
  screen also gains your rest status: whether you are fully rested or how
  tired you are, plus your hours of service at a glance.
- **Sleeping at the terminal no longer swallows 10 hours by accident.** If
  your hours of service are fresh and you are not tired, choosing Sleep 10
  hours now warns that sleeping would only move the clock forward, and asks
  you to press Enter again to sleep anyway. So an extra press on the sleep
  option can never quietly cost you a rested clock.
- **New installs now start at relaxed trip pacing.** Fresh installs default to
  the relaxed pace, which gives you the most real time to hear and react to
  spoken warnings like exits and hazards. Existing players keep whatever
  pacing they already chose, and standard and fast are still one setting away
  under Settings, Gameplay, Trip pacing.
- **All music now plays at the same volume.** Six tracks, including the main
  menu themes, Open Road, Night Haul, and Small Hours, were much louder than
  the rest of the soundtrack. They have been brought down to match, so the
  music volume slider now behaves the same no matter which track is playing,
  and the menu no longer greets you louder than the drive that follows.
- **Real-world weather now refreshes three times as often.** With the real
  weather source turned on, the game checks the live conditions for your
  destination every five minutes instead of every fifteen, so fog rolling in,
  a storm firing up, or skies clearing reach your drive much sooner. If your
  connection drops, the game holds the last known weather for the same half
  hour as before switching to simulated conditions.
- **Downloaded builds no longer expose the game's world data files.** The
  world now ships built into the game program itself, so there is no data
  folder to browse or accidentally edit next to the game. Nothing changes
  about how the game plays, and source checkouts keep their editable data
  files.
- **Downloaded builds now ship their sounds as a single packed file.** The
  browsable sounds folder is gone from the download; all sound effects and
  music travel in one pack file the game reads directly. Every sound plays
  exactly as before, the sound and music credits ship as a readable file
  next to the game, and source checkouts keep their editable sound files.
- **During a manual drive.** hold down the clutch (shift) then press W to shift up gears, and q to shift down gears .
  Contributed by ashleygrobler04 ([@ashleygrobler04](https://github.com/ashleygrobler04))
  in [PR #27](https://github.com/orinks-games/Freight-Fate/pull/27) and
  [PR #31](https://github.com/orinks-games/Freight-Fate/pull/31).
- **Hours-of-service rules are more realistic.** Realistic mode now tracks the
  11-hour driving limit, 14-hour duty window, 30-minute break requirement,
  60/70-hour weekly limits, roadside inspections, and legal sleeper-berth split
  rest. Rest menus now make the choice explicit: short breaks, poor emergency
  sleep, full sleeper sleep, or sleeper split planning where the stop supports
  it.
- **Menus can read just the option, not its place.** A new Speech setting,
  "Menu position announcements," turns off the "N of 10" position spoken after
  each menu option, so menus read only the option itself. On by default.
- **In-game help and manual now cover the new systems.** The how-to-play pages,
  the F1 driving help, and the user manual were brought in line: the calendar
  and seasons, weather that bites (traction loss, drag, visibility), the
  always-available shoulder and lot sleep, cruise that declines low-speed local
  roads, and -- newly documented anywhere -- state-trooper speeding pull-overs
  (signal with X) and real changing posted limits.
- **The calendar reads as a real date, in more places.** The career clock now
  speaks an actual date that advances as time passes -- "March 21," then "April
  1," and so on (a new career starts March 21) -- instead of only a day number.
  It is announced on the C clock readout, the Tab status menu, and the on-screen
  status, not just at the terminal, with the season alongside it. With live
  weather on, the date and season follow the real-world calendar.
- **Weather you have to drive to, not just hear.** Three conditions that used to
  be flavor now bite. High wind and storms add real aerodynamic drag, so they
  cost top speed and fuel. Driving well over the conditions-safe speed on a
  slick road risks a traction-loss incident -- hydroplaning in rain, sliding on
  snow -- so the safe-speed readout finally has teeth. And low visibility (fog,
  heavy rain) shortens how much warning you get before a hazard, so you have to
  actually slow down to see and react in time.
- **Speed-limit changes now say why.** A changing posted limit is announced as
  "Speed limit reduced to X" or "raised to X" instead of a bare number, and an
  urban drop names the city ("reduced to 55 approaching Boston"), so a mid-drive
  change is no longer a mystery.
- **No cruise on low-speed local roads.** Adaptive cruise will not engage on a
  facility access road, gate, construction zone, or heavy-traffic stretch -- the
  low-speed local roads a real driver takes manually -- and says so if you try.
- **Relaxed mode now feels emptier on the road.** Relaxed hours-of-service mode
  already made random hazards and trooper patrols rarer; it now also thins
  ambient traffic and the odds of a random roadside log check, so a relaxed run
  centers on driver responsibility -- hours, fuel, fatigue -- with fewer
  interruptions. Fixed checkpoints (weigh stations) and construction-zone
  enforcement are unchanged: a real violation still catches you. Realistic mode
  is untouched.
- **Live weather now reports the real temperature.** With live weather on, the
  cab speaks the actual temperature from the nearest National Weather Service
  station instead of the modeled seasonal estimate, so the degrees match the
  conditions it is already pulling in. The seasonal climate model stays the
  fallback whenever live data is unavailable or a station omits its reading.
- **Dial your cruise speed with Plus and Minus.** Once adaptive cruise is set,
  Plus and Minus raise and lower the target by 5 -- the accelerate and coast
  buttons on a real truck -- so you can engage as soon as you are rolling and
  dial the speed up to where you want it instead of having to reach it manually
  first. The truck accelerates up to a higher target on its own, and the posted
  limit cap still applies, so a higher set speed never makes it speed.
- **Adaptive cruise now respects the posted limit.** Cruise eases off to hold a
  with-traffic pace (about 5 over the posted limit) instead of carrying your set
  speed straight through an urban drop or a lower-limit stretch -- so it keeps
  you moving naturally without driving you into speeding strikes, tickets, and
  trooper stops. It still follows slower traffic and widens its gap in bad
  weather, and a short cue says when it eases off for a lower limit (the
  "Speed limit X" sign cue still names the number).
- **The air-brake system has real sounds now.** When pressure builds, you hear
  an air-dryer purge as the compressor cuts out instead of a generic beep, and
  low-air and spring-brake warnings sound a proper low-air buzzer. The spoken
  cues are unchanged, so nothing is lost if you rely on them.
- **Speeding now costs you out loud, the moment it happens.** When a speeding
  strike is recorded, the cab calls out the running fine ("Speeding strike. The
  limit is 65. Speeding fines now total 160 dollars, due at delivery.") instead
  of the cost landing silently on your settlement. Judged against the corridor's
  real posted limit, with the usual ~10 mph leeway before a strike lands.
- **Posted speed limits can now come from real map data.** Where a corridor
  carries an OpenStreetMap `maxspeed` tag, the game uses that real posted limit
  instead of the highway/region approximation -- and falls back to the
  approximation only on stretches OSM has not tagged. Limits are baked at build
  time (truck-specific `maxspeed:hgv` preferred where present); the spoken
  limit-change cue still calls out posted-limit changes as you drive.
- **The lane-drift rumble is now directional.** When you wander toward a lane
  edge, the rumble strip plays from that side -- drift right and you hear it on
  the right -- so the ear it lands in tells you which way to steer back.
- **Safety announcements no longer get buried, and you get more warning.** Zone
  entries, construction and traffic warnings, and checkpoints now preempt
  ambient chatter (weather, tolls, state lines) on the event voice instead of
  queuing behind it -- so a "construction ahead" never arrives after you have
  already entered the zone. Zone warnings also lead by real time now, not a
  flat distance: the heads-up scales with your speed and pacing, so 70 mph at
  high time compression gets a usefully earlier callout instead of a couple of
  seconds.

### Added
- **Repeat the market watch on the dispatch board.** The board speaks which
  freight is tight or loose when you open it; pressing Tab now repeats just that
  market watch, so you can re-check it without leaving and reopening the board.
  Contributed by Liam Erven ([@liamerven](https://github.com/liamerven)) in
  [PR #30](https://github.com/orinks-games/Freight-Fate/pull/30).
- **State troopers can pull you over for speeding.** Routes now have patrol
  windows -- hotter on busy interstates, in construction, and in dense regions,
  cooler out on the plains, with a night DUI bump. Speed badly inside one and a
  trooper lights you up: signal with X, brake to a stop on the shoulder, and sit
  through a license and logbook check that ends in an on-the-spot ticket (paid
  immediately, escalating with each stop) or a warning if it's a first, marginal
  stop or your reputation is strong. Run from the stop and it's logged as
  evasion -- a heavier fine and a serious reputation hit. Speeding the patrols
  don't catch still accrues the quieter safety-record cost at settlement.
  Relaxed mode keeps patrols light.
- **Consult the controls without leaving a drive.** The pause menu now has a
  "Controls and help" entry that opens the how-to-play reference straight to the
  driving keys -- page through it, read it line by line, then escape back to the
  road. The keys list also now includes S, A, and U.
- **HTML player manual.** Portable builds now ship `USER_MANUAL.html` alongside
  the Markdown one: the same manual rendered as a clean, accessible web page
  (semantic headings and real tables) you can open in any browser.
- **Three new on-demand driving keys.** **S** reads the posted speed limit where
  you are -- the zone if any, and how far over you are -- so you no longer have
  to dig into the status menu. **A** repeats the last route announcement, for
  the one you missed before you could react. **U** reads what is coming up:
  imposed speed limits, stops, and exits ahead, so a zone or gate never blindsides
  you. All three are listed in F1 help and the manual.
- **Drowsiness has real consequences now.** Push past severe fatigue and you
  start to nod off: a rumble-strip jolt and a warning give you a moment to steer
  or brake and stay awake. Catch it and you carry on; miss it and you drift onto
  the shoulder for damage and lost speed. Keep driving exhausted and the nods
  come faster and harder until a third miss forces you off the road. Sleep is no
  longer optional once you are running on empty -- and in relaxed mode, where
  hazards are rare, managing fatigue becomes the heart of the drive.
- **Posted speed limits that change by corridor.** The flat 70 everywhere is
  gone. The limit now comes from the highway and region -- rural Interstates run
  70 in the Midwest and East, 75-80 across the West, US highways and state
  routes slower -- and drops to an urban limit on the city stretches. Crossing
  into a new limit is spoken like a sign ("Speed limit 75"), the limit restores
  correctly when you leave a construction zone, and speeding is judged against
  the corridor you are actually on.
- **Seasons and temperature.** Your career now moves through the year, and the
  weather follows. A regional temperature model (a seasonal swing plus a
  day-night swing, warmer in the desert and Gulf, colder across the northern
  tier) decides whether precipitation falls as rain or snow and whether storms
  can brew, so snow is a cold-season risk, thunderstorms a warm-season one, and
  a Great Lakes January night freezes while a Gulf Coast one does not. Because
  hazards are weather-gated, snow squalls and ice now show up in winter and
  hail in summer, on their own. The terminal time-and-weather readout names the
  season, and weather reports include the temperature in your units. With live
  weather turned on, the season follows the real-world calendar so it matches
  the live conditions you are pulling in; otherwise it follows your career clock.
- **Cargo weight now changes how the truck drives.** Gross weight is the
  tractor-and-trailer tare plus the actual payload, so a heavy load pulls away
  gently, lugs harder on grades, and burns more fuel, while a light load or an
  empty pickup deadhead is noticeably brisker. Heavier freight is now a real
  trade-off, not just a number on the dispatch board. The driving status screen
  shows gross tonnage alongside the cargo weight.
- **Load-sensitive braking.** The foundation brakes have a fixed force ceiling
  sized for the rated gross, so a load heavier than the rated weight is
  brake-capacity limited: it decelerates more gently, takes longer to stop, and
  heats and fades the brakes sooner. Loads at or below the rated gross brake
  exactly as before. Overloading a run now bites on a downgrade or a panic stop.
- **Grounded, context-aware road hazards.** Hazards now only happen where and
  when they plausibly would. Standing water and hydroplaning need wet weather;
  snow squalls, bridge-deck ice, and black ice on shaded grades need snow;
  dense-fog brake-lights need fog; crosswind shoves and dust storms need high
  wind in open country; rockfall and runaway-truck hazards need mountain
  terrain. Deer and elk are biased to dawn, dusk, and night, with regional
  species. The implausible ones are gone -- no more farm equipment merging
  onto the interstate or a dust devil on a clear, calm day.

## 1.7.0 - 2026-06-26

### Added

- **Relaxed mode now actually relaxes the road.** In relaxed hours-of-service
  mode, random road hazards are much rarer, so the drive centers on driver
  responsibility -- hours, fueling, repairs, and fatigue -- instead of constant
  emergency braking. Realistic mode is unchanged. The Settings help for Hours
  of service spells out the difference.
- **Dispatcher pay advances (no more soft lock).** A broke driver who can no
  longer afford fuel can now draw a cash advance against the next load -- from
  the terminal hub or any in-trip rest stop -- and it is repaid automatically
  out of the next delivery settlement. The advance is offered only while cash
  is low and is capped, so it stays a recovery line rather than free money. A
  negative balance is no longer a dead end.
- **Discord Rich Presence (optional).** When Discord is running, your profile
  can show broad game activity -- the main menu, the terminal, driving a route,
  resting, or delivering -- with high-level route and cargo context. Only
  general game status is shared, never save files or personal details. It is on
  by default and can be switched off in Settings → Gameplay → Discord presence,
  and the game starts, plays, and exits cleanly whether or not Discord is open.
- **Bigger freight map.** The playable network grows to 194 cities and
  437 routed legs, adding many more regional hubs, shorter connector lanes,
  and route-backed freight choices across the country.
- **Highway exit callouts.** Interstate drives now announce upcoming
  interchanges the way a real sign reads them -- "In 2 miles, exit 7 for
  US-1 North toward Trenton and New York" -- with the exit number, the route
  you would take, and its control cities. Exit data is sourced from
  OpenStreetMap and snapped onto each corridor.
- **Grounded exits and onramps.** When a rest stop sits at a real interchange,
  the exit prompt and ramp now name its number ("Signaling for exit 113, the
  Petro Stopping Centers"; "You take exit 113"). Each run also opens with an
  onramp callout -- "Merge onto I-65 South toward Indianapolis" -- and highway
  changes name the new road and direction.
- **Optional lane drift.** Gameplay settings now include off, light, and
  realistic drift so players can add a gentle steering task, rumble-strip
  warnings, and off-road consequences without making the default drive harder.
- **Packaged changelog and manual.** Portable builds now include
  `CHANGELOG.md` and `USER_MANUAL.md` in the game folder so release notes and
  the player manual are available offline.
- **Player manual.** A new public manual now gathers install, career,
  dispatch, driving, saves, settings, accessibility, and troubleshooting
  guidance in one linkable place.
- **Music remakes.** The main menu theme, Open Road, and Night Haul now use
  new Suno remakes while keeping their familiar Freight Fate music slots.
- **Music rotation.** All menu and driving music beds now play once and rotate
  through their active pool instead of looping.
- **Quieter music by default.** New settings now start background music at half
  volume so speech and driving cues stay comfortably in front.
- **Expanded music beds.** Freight Fate now includes longer menu, facility,
  daytime driving, and nighttime driving music. Menus and freight facility
  screens use a career-aware pool, and active drives use stable day/night
  pools that rotate without reshuffling abruptly while you are on the road.
- **Truck cab sound refresh.** Engine start, idle, shutdown, horn, gear shift,
  parking-brake set and release, and highway road ambience now use an updated
  in-cab vehicle sound set, thanks to [Darren Duff](https://darrenduff.com/).
  The start cue is trimmed so the idle loop takes over cleanly.
- **Night driving ambience.** Night drives now play a new recorded in-cab
  night ambience loop.
- **More music.** New night beds: a menu theme for careers loaded after dark,
  and a late-night driving piece.
- **New drowsiness yawn.** The fatigue yawn cue uses a fresh sound, thanks to
  [Darren Duff](https://darrenduff.com/).
- **New achievement system.** Careers now track achievements across a range
  of categories, with a spoken main-menu viewer and a chime when you unlock
  one. Existing careers carry over. Note: a career saved on a preview snapshot
  may not load on an older stable release.

### Changed

- **Safety announcements no longer get buried, and you get more warning.** Zone
  entries, construction and traffic warnings, and checkpoints now preempt
  ambient chatter (weather, tolls, state lines) on the event voice instead of
  queuing behind it -- so a "construction ahead" never arrives after you have
  already entered the zone. Zone warnings also lead by real time now, not a
  flat distance: the heads-up scales with your speed and pacing, so 70 mph at
  high time compression gets a usefully earlier callout instead of a couple of
  seconds.
- **Truck-legal routing everywhere.** Every corridor's geometry, elevation, and
  grades are now derived from OpenRouteService's heavy-goods (driving-hgv)
  profile. The original cross-country legs (NY-Boston, the I-70/I-80 spine, and
  about a hundred others) were still on the car-routing engine; they now match
  the rest of the network with truck-legal paths and real truck elevation. Their
  grade profiles are finer too -- the old car-engine legs had a single grade per
  corridor, where the truck engine breaks each into the real run of climbs and
  descents -- though no leg's overall terrain rating changed. Distances were
  already accurate, so pay and deadlines are unchanged. The refreshed route
  data is included in the game, so driving still works fully offline.
- **Real weather now uses the National Weather Service.** Optional live weather
  switched from Open-Meteo to the U.S. National Weather Service API
  (api.weather.gov). It is still free and needs no API key, reads each city's
  nearest official station for current conditions, and keeps the same seamless
  fallback to simulated weather when offline.

### Fixed

- **The truck can no longer roll away while you rest.** Opening a truck
  stop or rest-stop menu now sets the parking brake and cuts the throttle, the
  same way pulling into a pickup or delivery does. Before, a rig that crept in
  just under the stop threshold (or idled in gear) could keep drifting down the
  road while the driver slept. Returning to the road now reminds you to release
  the parking brake with P.
- **No more implausible interstate hazards.** The random road-hazard pool no
  longer surfaces things that can't happen on a limited-access interstate, or
  that are really weather rather than a brake-now event: farm equipment merging
  onto the highway, sudden downpours and thunderstorm downpours, and hail. Real
  weather still arrives through the weather system, and genuine road hazards --
  standing water, whiteout squalls, debris, stopped traffic, crosswinds,
  wildlife, rockfall -- stay.
- **Phantom state-line crossings.** Highways that run alongside a river border
  -- I-84 down the Columbia Gorge most of all -- no longer announce a flurry of
  back-and-forth state crossings the driver never makes. I-84 hugs the Oregon
  bank of the Columbia (the Oregon/Washington line) for about 100 miles without
  ever crossing it, but corridor sampling against a simplified boundary used to
  flicker across the line and fabricate the crossings; a Portland run could call
  the Oregon/Washington line four times before the real Oregon/Idaho border. The
  baked route data is now scrubbed of these round trips (71 across 20 legs,
  including I-5, I-24, I-29, I-79, and I-90 corridors), and the enrichment
  pipeline guards against re-introducing them.
- **Salem connected to Portland.** Salem now has a direct I-5 leg to Portland
  (about 46 miles). Before, Salem was wired to Seattle and Tri-Cities but not to
  Portland right next door, so a Salem-to-Portland run routed 176 miles the
  wrong way -- south to Eugene and back north through Salem -- and long hauls out
  of Salem were labeled I-84 from the start even though they leave on I-5. The
  redundant direct Salem-Seattle and Salem-Tri-Cities legs are gone; those trips
  now compose through Portland with correct per-highway signage (I-5 out of
  Salem, I-84 only once you reach the Columbia).
- **Real weather warm-up.** With real weather enabled, a drive now starts in
  neutral clear conditions and waits for live data, instead of briefly showing a
  simulated condition that the live data immediately replaced. That warm-up
  flicker could also wrongly unlock a weather achievement (for example, a rain
  achievement for weather you never drove in). Simulated weather still runs as
  the offline fallback when live data cannot be reached.
- **macOS save location.** Saves now live in
  `~/Library/Application Support/FreightFate` instead of beside the app in
  Applications, matching macOS conventions. Existing saves found next to or
  inside the app bundle are moved into the new location on first launch.
- **Empty reposition arrivals.** Finishing a bobtail (empty reposition) run no
  longer crashes on arrival. The "Repositioned" summary screen now opens and
  reads its relocation summary instead of failing as you reach the new city.
  Contributed by Shane Popplestone ([@stickbear2015](https://github.com/stickbear2015))
  in [PR #16](https://github.com/orinks-games/Freight-Fate/pull/16).
- **Speech setting previews.** Adjusting speech rate, pitch, volume, or voice
  now previews with the voice being changed, so a selected SAPI or OneCore
  voice speaks its own new setting.
- **Truck idling.** The diesel now stays running through pickup check-in,
  loading, route planning, loaded departure, and active-drive resume instead
  of forcing a fresh engine start.
- **Destination exits.** Delivery routes now require taking the real signed
  exit for the destination when one is listed, instead of completing just by
  driving to the end of the highway corridor.
- **Destination exit callouts.** Destination exits now announce the signed exit
  and toward cities before the ramp, then tell you to press X; adaptive cruise
  cancellation includes that exit guidance.
- **OneCore pitch.** Windows OneCore speech now keeps its native default pitch
  unless the player changes the pitch setting.
- **Metric driving status.** Metric mode now reports driving status,
  speed limits, traffic, pickup distance, and legal-stop distance in metric
  units instead of mixing in mph or miles.
- **Metric traffic speed.** The traffic-queue speed shown in the route line now
  reads in kilometers per hour in metric mode, instead of staying in miles per
  hour next to the already-metric distance. Contributed by Shane Popplestone
  ([@stickbear2015](https://github.com/stickbear2015)) in [PR #16](https://github.com/orinks-games/Freight-Fate/pull/16).
- **Metric navigation cues.** Spoken GPS guidance -- onramp, continue, stop,
  exit, traffic, and construction-zone callouts -- and the Map status screen now
  give distances in kilometers in metric mode instead of miles, matching the
  rest of the metric driving readouts.
- **Metric speed limits.** Construction and traffic zone callouts now speak the
  posted speed limit as a metric value in metric mode instead of the mph number.
- **Live unit switching.** Switching between miles and kilometers mid-drive now
  updates spoken navigation guidance right away, including the distances already
  laid out along the current route.
- **Packaged update checks.** The updater now recognizes standalone packaged
  folders more reliably, so switching to preview snapshots does not leave the
  update screen confused about how the game was installed.
- **Quieter exit guidance.** Ordinary highway exits now stay available in the
  route screen without being announced during the drive unless they lead to a
  stop you can actually take.
- **Route key priority.** Pressing R now keeps the next actionable route detail
  first, while Shift+R reports the next listed highway exit.
- **State-line timing.** State crossing previews now speak about 10 miles out
  instead of 2 miles out, giving the preview and crossing announcements more
  room at highway speed.
- **Upper gear spacing.** Automatic shifting now holds 9th gear longer before
  entering overdrive 10th, so the truck no longer reaches top gear around
  city-road speeds.
- **Portable save folders.** Snapshot builds now move nearby duplicate
  portable save folders into the active `FreightFate\saves` folder instead of
  leaving players with two likely save locations after extraction or updates.
- **Clearer help.** F1 help now focuses on what the selected item does for the
  player instead of repeating menu controls, and garage upgrade help explains
  how each upgrade changes the truck.
- **Updater works in packaged builds again.** Packaged copies are now detected
  correctly, restoring update checks, install, and crash logging.
- **Facility approach speed cues.** Pickup deadheads now use lower-speed
  facility access roads, deliveries slow through a final receiver approach,
  and the last gate prompts are shorter so stopping instructions land faster.
- **Facility gate ambience.** Pickup and destination facility screens now use a
  quieter loading-dock ambience that stays away from truck-idle rumble.
- **Preview sound volume.** The refreshed truck, road, weather, route, and
  facility sounds now play at full source strength before the player's volume
  settings are applied, so lowering and raising sound effects behaves more
  predictably.
- **Achievement speech routing.** Achievement unlocks now speak through the
  screen reader voice instead of the separate driving-event voice, so players
  who miss or interrupt an unlock can still review it later from the
  Achievements menu.
- **Facility and settings audio fixes.** Terminal and yard screens now use
  the new facility-gate ambience, delivery completion no longer buries the
  dock and settlement cues under a generic menu sound, and volume settings
  persist into the next game session.
- **Status and settings navigation.** The driving status panel now opens into
  clear route, driver, truck, and map-style status screens, and Settings uses
  category menus for gameplay, audio, speech, weather, and updates.
- **Menu navigation polish.** Delivery completion now presents settlement,
  route, truck, and career details in one continuous list, while Settings keeps
  its category menus for easier browsing.

## 1.6.0 - 2026-06-19

### Added
- **Contextual route and weather audio.** Driving now uses in-cab rain, snow,
  wind, fog horn, and thunder cues plus short route-event sounds for hazards,
  construction zones, inspections, tolls, state crossings, rest stops, weigh
  stations, facility gates, and docking. The road bed is back in the mix so
  the cab does not feel dry while moving. The experimental vehicle engine sound
  redesign is still being tuned and is not part of this release.
- **Route rest, toll, and settlement realism.** Route planning now uses richer
  truck-stop data, handles shoulder-sleep edge cases more cleanly, and accounts
  for toll and settlement details more explicitly.
- **Air-brake startup and reservoir behavior.** Trucks now build air
  pressure before departure, keep spring brakes engaged until the system is
  ready, and model service and emergency reservoir pressure while driving so
  braking feels more like a heavy truck without stranding new careers.
- **Driving status menu.** Pressing Tab while driving now opens a spoken status
  menu with load, trip, truck, route, and route-stop details from the road.
- **Better route stops.** Dispatch-supported freight now
  relies on curated truck-relevant route stops only: placeholder midpoint
  POIs no longer count as real route support, long-haul lanes must include
  explicit fuel-capable stops, and route summaries/GPS stop details
  now give clearer parking certainty.
- **Auto-updater.** The packaged game now checks GitHub for new releases
  when you reach the main menu. When one is found, a fully spoken prompt
  offers "Download and restart" (downloads the update, swaps it in, and
  relaunches the game for you), "What's new" (reads the update's changelog
  line by line), "Remind me later", and "Skip this version". A new
  Settings entry, "Update channel", picks between stable releases and preview
  builds, and "Check for updates" checks immediately.
- **Real pickup and loading flow.** Job offers now name the origin
  facility as an actual stop on the trip instead of flavor text. After
  accepting a load, you check in at the listed facility, load only while
  stopped, then plan the loaded trip to the destination.
- **Company terminal dispatch flow.** New careers and continued drives now
  frame the service-area hub as a company terminal or yard instead of a
  generic city spawn. Dispatches start with a local deadhead move from the
  terminal to the shipper, and delivery settlement parks the truck at the
  destination area's terminal or yard for the next assignment.
- **Destination facility docking.** Deliveries no longer settle just
  because the truck reached the destination city. The game now warns at
  speed, keeps you in control until a full stop, opens a facility menu
  with a dock/yard cue, and requires "Dock and deliver" before payment.
  "Check paperwork" previews facility, cargo, payout, deadline, and damage
  details without completing the load.
- **Real freight facilities on job boards.** Cities now offer freight from
  classified locations such as terminals, warehouses, ports, intermodal
  yards, air cargo areas, manufacturing plants, food terminals, industrial
  parks, retail distribution hubs, and bulk facilities. Cargo is filtered
  by plausible facility type.
- **Highway exits.** Rest stops now sit at proper exits. They are
  announced a few miles out ("Press X to take the exit for it"); X
  signals for the exit (and X again cancels), you slow to 45 or less for
  the ramp — any faster and you blow past it — then half a mile of ramp
  and brake to a stop, and the rest stop menu opens by itself. The ramp
  is off the highway: hazards and speeding checks pause while you are on
  it. Pressing T while stopped on the highway at a stop still works.
- **Explicit highway stop positions.** Route data now stores named highway
  amenities with explicit mile positions instead of spreading rest stops
  evenly across a leg. The first curated offline stop set uses sourced rest
  areas and travel centers, keeping the game playable without live map lookups.
- **Reverse gear and missed-stop recovery.** Trucks can now back up.
  Automatic players can hold Down while stopped to reverse slowly, then
  touch Up to brake and return to forward drive; manual players can press
  the clutch and Backspace for reverse. If you miss a rest stop, slow
  down, back up carefully, stop, and press T.
- **Cruise control.** K sets cruise at your current speed, matching common
  highway driving expectations, and holds it with a slow throttle governor
  through grades.
  K again, any braking, the emergency brake, a stall, or taking an exit
  cancels it — and a hazard warning hands control straight back to you.
  Space reports speed.
- **Region-flavored road hazards.** The hazard pool now mixes nationwide
  staples with local flavor for the region you are driving through: dust
  devils and tumbleweeds in the Southwest, deer and farm equipment in
  the Midwest, rockfall in the Rockies, elk and standing water in the
  Pacific Northwest, and more.
- **Separate voice for driving events.** Road events — hazard warnings,
  collisions, weather changes, rest stop and city announcements, HOS and
  fatigue warnings, speeding, inspections, speed callouts — now speak
  through a dedicated Windows SAPI voice, so a screen reader reading menus
  or echoing keystrokes can no longer cut off a "Brake now!" mid-sentence.
  A new Settings entry, "Driving event voice" (default: separate SAPI
  voice), switches events back to the screen reader. When SAPI is
  unavailable, or is already the main voice, events fall back to the main
  channel automatically.
- **Emergency brake.** Hold B while driving for the hardest possible stop:
  instant full application plus the spring brakes (about 1.6 times the
  service brakes, still subject to weather grip and brake fade), with a
  loud air-dump cue. Use it for hazards and for rest stops you would
  otherwise overshoot. Mentioned in the tutorial, F1 controls, and the
  manual.
- **Roadside mechanic.** The pause menu while driving now offers "Call a
  roadside mechanic" once damage is past 25 percent: a field patch back
  down to 25 percent damage for a 500-dollar callout plus 110 dollars per
  percent repaired (a steep premium over the garage). The repair takes 90
  in-game minutes against your deadline and duty window, and the bill is
  due even if it puts you in debt — never a dead end.
- **Time and weather in the city.** A new city menu entry speaks the
  clock, the time of day, the day of your career, and current conditions
  in town (live Open-Meteo data when real weather is enabled).
- **Sleep in any city.** A new city menu entry, "Sleep 10 hours", gives a
  full night at your terminal: fresh hours of service, zero fatigue, and
  the clock advances 10 hours. Previously a spent duty window followed
  you into the city with no remedy except driving — illegally — to the
  first rest stop of the next run.

### Fixed
- **Pickup facility sounds.** Pickup gates and loading now use the new facility
  ambience and dock cues instead of the older generic menu notification sounds.
- **Preview builds stay in sync with release notes.** Preview builds now pick up
  player-facing changes that have already been prepared for the next stable
  release, so their "What's new" text no longer falls behind.
- **Save resume keeps traffic zones stable.** Continuing a saved drive now
  seeds trip weather from the saved trip seed too, so traffic and
  construction-zone layouts regenerate consistently across operating
  systems.
- **Updater connections on macOS and Linux.** The packaged game's Python
  runtime looks for certificate authorities at paths that only exist on
  the build machine, so on macOS and Linux every secure connection — the
  update check, the download, and the real-weather fetch (which silently
  fell back to simulated weather) — could fail certificate verification.
  The game now ships its own certificate bundle (certifi) and uses it
  alongside the system store on every connection.
- **Update errors now say what went wrong.** "Could not reach the update
  server" covered everything from a dropped connection to a blocked DNS
  lookup. The check and download now speak the actual reason — "The
  secure connection could not be verified", "The server answered with
  error 403", "The server address could not be found", and so on. The
  packaged game also writes a session log to logs/game.log, so a
  player can share the full error when reporting a problem.
- **Hazard warnings were unbeatable at highway speed.** The reaction
  window was a fixed 3 to 4.5 seconds, but a full-service stop from 65
  to the safe 25 miles per hour takes about 5 — even the emergency brake
  could not make it once you add the time to hear the warning. The
  deadline is now the braking time the truck actually needs from its
  current speed (on the current surface and grade) plus the rolled
  reaction window, so hitting the brakes promptly always succeeds — in
  rain or snow you get the longer stop those surfaces really take.
  Drowsiness now eats into the reaction part only instead of the whole
  window, since a tired driver reacts late but the truck stops no
  slower. Warnings also lead with "Brake now!" instead of ending with
  it, so you can be on the brakes before the sentence finishes.
- **Collision stall soft-lock.** A hard collision could stop the truck
  while the automatic transmission was still in a high gear; the engine
  then stalled the instant it was restarted, every time, stranding the
  player (it read as "too damaged to start", since the same crashes also
  max out damage). The automatic now returns to first gear whenever the
  truck is stopped in a higher gear, and restarting after a stall recovers
  cleanly.
- Pressing E with a bone-dry tank no longer dead-ends on "the engine will
  not start": the out-of-fuel roadside rescue now triggers from there too.
- **The C key's arrival estimate was a constant.** It always assumed
  55 miles per hour, so it never responded to how fast you were actually
  driving. It now tracks your current speed once you are meaningfully
  rolling (and says so), falling back to a typical highway pace while
  parked, and names the basis either way.
- **Abandoning a job lost the hours you drove.** The world clock snapped
  back to the departure time while hours of service and fatigue kept the
  accrued wear, and the freight market did not advance. The time spent on
  the failed run now counts.
- **Trip pacing now applies mid-trip.** Changing "Trip pacing" from the
  pause menu's settings was silently ignored until the next delivery; the
  active trip now picks it up immediately.
- **Unsafe engine shutdown blocked.** Pressing E at road speed no longer
  shuts off the engine. The game now gives spoken feedback and requires a
  safe low-speed stop before shutdown.
- **Delivery at speed blocked.** Arriving at the destination at highway
  speed no longer completes the job. Settlement now requires the full
  stopped facility docking flow.
- **Tampered saves are quarantined.** Career saves now carry an integrity
  signature. Old unsigned saves migrate forward, but edited or corrupted
  saves are moved aside instead of being loaded as valid career data.
- **Implausible route detours filtered.** Route options now reject obvious
  short-haul detours that send drivers far out of the way, while still
  allowing meaningful alternate long-haul routes.
- **State progress announcements improved.** Trips now announce state
  crossings and nearby cities along the route, not only the destination
  state.
- **Construction-zone warnings are actionable again.** Construction zones
  now give a spoken GPS warning about 2 miles before the slowdown begins,
  and troopers will not clock construction-zone speeding until you have
  had about a mile inside the zone to react. Speech-first players can
  slow down in time again instead of being fined on the same update that
  first announces the zone. Contributed by Shane Popplestone
  ([@stickbear2015](https://github.com/stickbear2015)) in [PR #9](https://github.com/orinks-games/Freight-Fate/pull/9).

### Changed
- **How-to-play driving guidance.** The main-menu guidance for driving controls
  is shorter and more direct.
- **Early career progression and pay.** Low-level jobs now pay enough to
  make early progress feel worthwhile after operating costs, and higher
  levels unlock clearer differences in range, cargo, endorsements, and
  long-haul opportunities.
- **Truck acceleration and shifting.** Loaded trucks reach safe highway
  speeds more plausibly, top gear behaves more like mild overdrive, and
  automatic shift cues are easier to hear without adding air-brake sounds
  to gear changes.
- **Freight market terminology.** Player-facing market wording now uses
  trucking language: tight, loose, and steady, replacing the old generic
  market labels.
- **Real terrain on real highways.** A geography audit corrected 20 of
  the 106 legs. The famous grades are now mountains: Monteagle on I-24
  (Nashville-Atlanta), the Cumberland Plateau on I-40
  (Knoxville-Nashville), the Pennsylvania Turnpike's Allegheny crossings
  (Philadelphia-Pittsburgh and Baltimore-Pittsburgh), and US-95's Idaho
  canyon country (Spokane-Boise). Rolling country stopped pretending to
  be flat: I-70's Missouri River hills, the Flint Hills and Arbuckles on
  I-35, Tennessee's Highland Rim on I-40, Wisconsin's driftless coulees
  on I-94, the Carolinas' piedmont, Connecticut on I-95, and the desert
  passes on I-10 (San Gorgonio, Texas Canyon) among others. Genuinely
  flat country — the high plains, the Gulf coast, Florida, and the Illinois
  prairie — stays flat.
- **Realistic deadlines.** Dispatch can no longer ask for the
  impossible. Deadlines are now built from the hours a law-abiding
  trucker actually needs — driving at an achievable 55 mph average, plus
  the 30-minute break every 8 driving hours and a 10-hour sleep for
  every 11-hour shift the distance demands — with 20 to 50 percent
  shipper slack and a flat hour for fuel on top. San Antonio to Dallas
  now quotes a workable 7-to-8-hour window instead of a sprint.
- **State trooper groundwork.** The next law-enforcement milestone is outlined:
  patrol intensity by corridor, CB chatter warnings, pull-overs, immediate
  fines, and an enforcement setting.
- **Portable saves.** Profiles and settings now live in a `saves` folder
  inside the game's own directory (next to the executable in release
  builds) instead of the per-user data directory. Existing saves are migrated
  over automatically on first launch; the originals are left in place.

## 1.5.0 — 2026-06-10

"On the Clock": hours of service, fatigue, day and night, and overnight
parking. Everything runs on the in-game clock (`settings.time_scale`
compresses it as usual), never wall time.

### Added
- **Hours of service.** Simplified FMCSA rules per shift: 11 hours of
  driving inside a 14-hour duty window, a 30-minute break required after
  8 hours at the wheel, and a 10-hour sleep to reset. Spoken warnings at
  2 hours, 1 hour, and 30 minutes before each limit (each fires once),
  and at the violation itself. The C key now reports the clock time and
  HOS status alongside the deadline; Tab includes it at normal and chatty
  verbosity. Driving past a limit risks roadside inspections with
  escalating fines (200 to 2,000 dollars) and reputation hits — never a
  game over. A new Settings entry, "Hours of service", picks realistic,
  relaxed (every limit 25 percent longer), or off.
- **Rest stop menu.** Pressing T at a rest stop now opens a fully spoken
  menu: refuel (as before), take a 30-minute break, or sleep 10 hours.
  Resting advances the in-game clock, so the delivery deadline keeps
  counting — that is the tension.
- **Fatigue.** Builds with continuous driving (faster at night), eases
  with breaks, and clears with sleep. A drowsy driver yawns, drifts onto
  the rumble strip, hears spoken drowsiness warnings, and reacts late to
  hazards (the reaction window shrinks up to 40 percent). Deterministic
  under the trip seed.
- **Day/night cycle.** Dawn, day, dusk, and night derived from the career
  clock (new careers still start at 6 AM). Nights bring sparser traffic
  zones, a higher hazard chance, a cricket-and-air night ambience layer,
  and the previously unused "Night Haul" track while driving. V, Tab, and
  C mention the time of day, and arrivals speak the clock ("It is 11 PM").
- **Overnight truck parking.** Arriving at a rest stop between 8 PM and
  4 AM, the lot may be full — more likely as the evening wears on,
  deterministic per trip seed. A spoken menu offers driving on to the next
  stop or shoulder parking: a full HOS reset but poor rest (fatigue floor
  of 30) and a 15 percent chance of a 150-dollar ticket.
- New manual page "Hours and rest"; F1 help on all new menus.
- New procedural sounds: `ambient/night` and `driver/yawn`
  (regenerate with `tools/generate_audio.py`).

### Fixed
- **Speech backend selection.** Prism's registry ranks NVDA above every
  other backend whether or not NVDA is running, so on machines without it
  the game bound to a dead NVDA connection and stayed silent. The backend
  choice is now validated against actual runtime support and falls down
  the priority list (JAWS, One Core, SAPI, Speech Dispatcher, ...) to the
  best backend that can really speak. A new
  `FREIGHT_FATE_SPEECH_BACKEND=<name>` environment variable forces a
  specific backend for troubleshooting.

### Compatibility
- Save format version is now 3. Old v2 profiles and pre-1.5 mid-trip
  snapshots load cleanly, defaulting to a fresh HOS clock and a rested
  driver.

## 1.4.0 — 2026-06-10

### Added
- **Home terminal picker.** A new career now asks where it should begin:
  after name entry, a fully spoken menu lists every city labeled by region
  ("Atlanta, the South"), with the usual arrow, Home/End, and first-letter
  navigation plus F1 help. Defaults to Chicago; Escape returns to name
  entry with the typed name intact. Existing profiles are untouched.
- **A real interstate network.** The map grows from 21 cities and 27 legs
  to 59 cities and 106 legs along real corridors (I-95, I-90, I-80, I-75,
  I-70, I-65, I-40, I-35, I-10, I-5, and more), so neighboring cities sit
  roughly 100-250 miles apart. Every new city has real coordinates for the
  live-weather feature, a weather region, and freight locations with
  regional identity: produce out of the Central Valley, autos around
  Detroit, electronics at the container ports, grain and livestock across
  the plains, machinery in the rust belt. Boston and Seattle are no longer
  dead ends; no city has fewer than two highways.
- **Career-arc job generation.** Rookie boards (levels 1-2) offer short
  regional work: mostly single-leg hops to neighboring cities, capped
  around 280-340 miles, with destinations weighted toward nearby cities so
  freight follows plausible lanes. The distance cap grows with level and
  cross-country hauls (600+ miles) unlock around level 4-5 as a dedicated
  long-haul slot on the board. A flat hookup fee keeps short early runs
  profitable after fuel.

### Compatibility
- All 21 original cities and all 27 original direct legs are preserved
  verbatim, so old profiles and mid-trip snapshots load and resume unchanged.

## 1.2.1 — 2026-06-09

### Added
- **Mid-trip save and resume.** "Save and quit to main menu" while driving
  now snapshots the delivery — job, route, position on the route, clock,
  speeding strikes, and trip damage baseline — into the profile. Continue
  (and Load driver) resume the drive right where you left off, parked with
  the engine off, with a spoken recap of cargo, destination, remaining
  miles, and hours used. Construction and traffic zones reappear in the
  same places thanks to a persisted trip seed, and stops or cities already
  passed are not re-announced. The Load driver list shows mid-delivery
  profiles as "on the road to <city>".

### Fixed
- "Save and quit to main menu" no longer silently discards the delivery
  (previously Continue always returned to the city with the job gone).

## 1.2.0 — 2026-06-09

### Added
- **Smoother truck engine audio.** Engine sound now follows RPM more naturally,
  with smoother transitions as you accelerate, shift, and settle into highway
  speed.
- **Garage upgrades** (Garage → Upgrades), money-gated and saved on the
  profile: engine tune (+10% torque per tier, two tiers), aerodynamic kit
  (−12% drag), long-range tank (+50 gallons), and reinforced brakes (fade
  onset pushed 150 degrees hotter). Upgrades feed straight into the driving
  physics.
- **A second truck**: the heavy hauler (Garage → Trucks) — a quarter more
  torque and a 200-gallon tank, but blunter aerodynamics and a thirstier
  engine. Buy it once, then switch between owned trucks at any garage.
- **Freight market**: every cargo class carries a pay multiplier (0.8–1.3)
  that drifts each in-game day on a seeded random walk persisted in the
  profile. Job descriptions call out tight and loose markets,
  and the job board opens with a spoken market watch headline.

### Changed
- Truck status and garage refueling respect the active truck's actual tank
  size instead of assuming 150 gallons.
- Save format version is now 2 (older saves load fine; new fields get
  defaults).

### Notes
- BASS is proprietary software, free for non-commercial use. If Freight Fate
  is ever sold commercially, a paid license from
  [un4seen developments](https://www.un4seen.com/bass.html#license) is
  required. See the README's license section.

## 1.1.0 — 2026-06-09

### Added
- **Real-world weather** (Settings → Weather source): live current
  conditions for each city from the free
  [Open-Meteo](https://open-meteo.com) API (no key required). WMO weather
  codes map onto the game's conditions, including strong-wind promotion.
  Fetches run in background threads with a 15-minute cache; offline or on
  any failure the simulated weather takes over seamlessly.
- City coordinates in the world data.
- With real weather enabled, route planning's W key speaks live conditions
  for the cities along the route, and the V key while driving reports
  "live conditions" for the city you are heading toward.

## 1.0.0 — 2026-06-09

First release. Complete rewrite of the prototype.

### Added
- Career mode: jobs, route planning, deliveries, money, experience levels,
  reputation, and cargo endorsements (refrigerated at level 2, high-value at
  level 4).
- Tuned Class 8 truck physics: ten-speed transmission (manual with clutch or
  automatic), torque curve, grades, traction limits, stalling, brake fade,
  engine braking, and realistic fuel economy (~6 mpg loaded).
- 21-city, 27-leg interstate network with Dijkstra route finding and multiple
  route options per job.
- Dynamic regional weather (eight conditions) affecting grip, drag, and safe
  speed, with forecasts and thunder.
- Trip events: construction and traffic zones, road hazards with reaction
  windows, rest stop refueling, out-of-fuel roadside rescue, speeding fines.
- Screen reader output through Prism (`prismatoid`): NVDA, JAWS, SAPI,
  VoiceOver, Speech Dispatcher, and more, with silent fallback.
- Fully synthesized CC0 sound library (43 effects) and three original music
  tracks, all reproducible from `tools/generate_audio.py`.
- RPM-crossfaded engine audio, speed-tracking road noise, weather ambience.
- Accessible UI: spoken menus with wrap-around and first-letter navigation,
  contextual F1 help, accessible text entry, three speech verbosity levels,
  imperial/metric units, and a visible text mirror of all speech.
- First-drive tutorial, six-page in-game manual.
- Atomic JSON saves with multiple driver profiles.
- Packaged builds for Windows and Linux.

### Removed
- SRAL DLL dependency (replaced by the Prism Python package).
- Legacy prototype files and duplicate data files.
