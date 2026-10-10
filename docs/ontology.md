# Freight Fate ontology

What kinds of things exist in Freight Fate, what the code calls them, and --
the part that reaches players -- what the game *says* out loud for each one.

Two audiences. A contributor adding a feature should be able to find out
whether the concept already exists under another name. A contributor writing
spoken text should be able to find out which word the game already uses, so a
screen reader user does not hear three nouns for one thing.

This is a contributor document. Nothing here is player-facing, so changes to it
alone take `[skip changelog]`.

## Two tiers

Freight Fate's entities divide into a portable model layer and a game layer.

**The portable layer** is the `ff-core` crate: world data, career models,
simulation, and spoken-text rules. It has no window, audio-device,
screen-reader, or network dependencies. Cargo enforces that boundary, so
these systems can run in headless tests.

**The game layer** is the `freight-fate` crate: the application and screen
states, audio and speech backends, saves, and online services. Career 1.9
uses this native Rust runtime. See [the contributor architecture guide](../CLAUDE.md#how-the-code-fits-together)
for the crate layout and service boundaries.

The Python module and class references below are **legacy port references**
to the deleted Python game (in git history; `v1.8.8.1` is its last release),
including `models/`, `sim/`, and `states/`.
They are not the current gameplay implementation. Rust
modules generally retain the names of their Python counterparts under
`crates/ff-core/src/` or `crates/freight-fate/src/`; consult those modules
for current behavior. The tables retain the reference names to help locate
port history. Their canonical spoken vocabulary still applies to the game.

The portable layer's vocabulary is documented alongside the code that owns it.
This file catalogues the game layer, and then the spoken vocabulary for both.

## Game-layer entities

### The save

| Concept | Class | Module |
| --- | --- | --- |
| Profile | `Profile` | `models/profile.py` |
| Truck condition | `TruckCondition` | `models/trucks.py` |
| Duty log | `DutyLog` | `sim/hos.py` |
| Loyalty account | `LoyaltyAccount` | `models/loyalty.py` |
| Radio favorite (spoken "favorites", a saved station) | `RadioState.favorite_ids` | `radio.py` |
| Radio app (the Driver apps tablet app that searches the dial, tunes by name, and keeps favorites; "Search stations", "Stations in range", "Favorites") | `RadioAppState`, `RadioState.search`, `RadioState.favorites` | `states/driving_radio_app.py`, `radio.py` |
| Now playing (spoken "Now playing on <station>: <song>"; what a stream says it is playing -- "song information" when there is none) | `_radio_now_playing_text`, `AudioEngine.radio_now_playing`, `parse_icy_stream_title` | `states/driving_updates.py`, `audio.py` |
| Personal playlist station (one of the player's own playlist files on the dial) | `PERSONAL_PLAYLIST_SOURCE_TYPE`, `load_personal_playlists` | `radio.py` |
| Station suggestion (spoken "Suggest a station"; a station a player sends from the Radio app for review) | `SuggestKindState`, `StationSuggestionEntryState`, `StationSuggestion` | `states/driving_radio_suggest.rs`, `community_stations.rs` |
| Community station (a suggested station the owner accepted; shipped in `radio_community.json` and downloaded at launch; an AM or FM one plays near its licensed transmitter, else on the web radio band) | `ff_core::radio::community`, `COMMUNITY_STATIONS_FILE`, `RADIO_COMMUNITY_RESOURCE` | `radio/community.rs`, `tools/fold_community_stations.py` |
| Save migration | `migrate_save_data`, `SAVE_VERSION` | `models/save_migration.py` |
| Career from an earlier version (the 1.9 cutover gate; never "legacy" in spoken text) | `created_line`, `LegacyCareerError` | `models/profile.py` |
| Integrity signature | `SIGNATURE_FIELD`, `ProfileIntegrityError` | `models/profile.py` |
| Profile invariants | -- | `profile_invariants.py`, `profile_integrity_invariants.py` |

`Profile` is the root of everything persisted. It *holds* a `Career`, a
`Market`, an `HosClock`, a `DutyLog`, a `LoyaltyAccount`, per-truck
`TruckCondition` records, the business standing (status, carrier, start mode,
authority readiness), owned and programme trailers, active buffs, the money,
the in-game clock, the achievement list, and an `active_trip` snapshot. It is
the one object whose shape is a compatibility contract in three directions at
once: older saves, the cloud validator, and the integrity signature. Add fields
with defaults; never reorder or repurpose.

The business fields are the ones to be most careful with, because they are read
by the cloud validator as well as by the game. The allow-list there is a
superset and the required set is only what the checks actually read -- exact
field matching rejects both older and newer builds.

### Carrier and equipment

| Concept | Class | Module |
| --- | --- | --- |
| Fleet tier | `FleetTier` | `models/carrier_fleet.py` |
| Trailer unit | `TrailerUnit` | `models/trailer_yard.py` |
| Pickup plan | `PickupPlan` | `models/trailer_yard.py` |
| Delivery plan | `DeliveryPlan` | `models/trailer_yard.py` |
| Truck dealer (spoken "truck dealer", or the source-backed local dealer name when one exists) | `TruckShopState` | `states/city_business.py` |

A `FleetTier` is what a carrier *assigns* at a level band: a company driver
does not choose a tractor, they are given one. An owner-operator owns theirs.
The same `truck` field on the profile means "assigned tractor" or "active owned
tractor" depending on business status, which is worth remembering before
writing any sentence about "your truck".

`TruckCondition` is per-truck persistent state -- fuel, damage, tire wear,
grime -- keyed by catalog key. It is deliberately *not* portable: condition
belongs to the truck you own in this game, not to the driver.

### The live drive

| Concept | Class | Module |
| --- | --- | --- |
| Trip | `Trip` | `sim/trip.py` |
| Route helpers | -- | `sim/trip_route_helpers.py` |
| Road events | `TripRoadEventMixin` | `sim/trip_road_events.py` |
| Traffic on the trip | `TripTrafficMixin` | `sim/trip_traffic.py` |
| Lane keeping | `LaneKeeping` | `sim/lane.py` |
| Lane keeping (the setting) | `lane_keeping` | `settings.py`, `states/main_menu.py` |
| The lane you came out of being open again | `LaneGapMixin` | `states/driving_lane_gap.py` |
| Driving mode tuning | `DrivingModeTuning` | `sim/driving_modes.py` |
| Driving mode (the setting): relaxed, standard, real time | `time_scale`, `TIME_SCALES` | `settings.py`, `states/main_menu.py` |
| Pedal latch | `PedalLatch` | `sim/pedal_latch.py` |
| A tank load and its sloshing wave | `LiquidLoad`, `SloshAxis` | `sim/surge.py` |
| Stop-cue geometry from stopping distance | -- | `states/driving_stops.py` |
| Planned rest-stop stopping assistance | `selected_stop_assist` | `settings.py`, `states/driving_events.py` |
| Destination approach assistance | `destination_approach_assist` | `settings.py`, `states/driving_updates.py` |
| Traffic vehicle | `TrafficVehicle`, `TrafficSituation`, `TrafficManager` | `sim/traffic_manager.py` |
| Live traffic | `TrafficEvent`, `TrafficData`, `RealTrafficProvider` | `sim/real_traffic.py` |
| Truck parking | `TruckParkingLocation`, `ParkingData`, `TruckParkingProvider` | `sim/truck_parking.py` |
| Enforcement post | `EnforcementPost` | `sim/enforcement_posts.py` |
| What a post noticed | `Observation`, `RoadSample` | `sim/enforcement_observe.py` |
| The enforcement watch on the drive | `EnforcementWatchMixin` | `states/driving_enforcement.py` |
| The held siren and its signature | `SirenLoop` | `states/driving_siren.py` |
| The safety record | -- | `models/safety_record.py` |
| Time zone | `TimeZone` | `sim/timezones.py` |
| Live weather | `RealWeatherProvider` | `sim/real_weather.py` |

Two kinds of traffic exist and they are not the same concept. `TrafficManager`
simulates the vehicles around the truck right now -- the ones a player hears
and reacts to. `RealTrafficProvider` fetches real-world incident data for the
route ahead. A sentence about "traffic" should make clear which one it means.

Spoken, the second one is a **live road report** -- never an "alert", and
never phrased as the state of the road ahead. It is a state DOT feed
describing the real road today, and the simulation does not act on it: a
reported closure does not close anything. Calling it an alert made a driver
believe the road in front of him was shut, and it was not (Brandon,
2026-08-21). The spoken frame carries the provenance the same way a data
`source` field does -- this came from outside, and the road under the wheels
is the game's own.

Three providers -- weather, traffic and parking -- reach the network. All of
them must degrade to the baked data silently; nothing in the drive may block on
one.

An enforcement post is a PLACE (a milepost with a body in it, or not) and an
observation is an EVENT (that body noticing one thing about your truck, with a
confidence). Keeping them apart is what lets presence be constant while
consequence stays rare: a run is full of posts and almost never produces an
observation. "Observed" is an internal word and is never spoken.

`Trip` is the binding entity: it joins a driver, a truck, a job and a route
into one moving thing, and it owns the clock. Everything the player hears while
driving is a projection of a `Trip`. Game hours are absolute Eastern; local
time is a view, via `Trip.local_hour`.

### Player-facing systems

| Concept | Class | Module |
| --- | --- | --- |
| Achievement | `Achievement`, `AchievementAward` | `achievements.py` |
| Achievement category | `AchievementCategory` | `achievements.py` |
| Message | `Message`, `MessageCategory`, `MessageLog` | `message_log.py` |
| Engine sound | `EngineVoice`, `EngineReading` | `engine_audio.py` |
| Settings | -- | `settings.py` |
| Speech | -- | `speech.py` |
| States | `State` and subclasses | `states/` |
| Playtest levers | -- | `playtest_levers.py` |
| Keyboard shortcuts and controller buttons | `Action`, `Chord`, `PadChord`, `KeyBindings` | `bindings.rs` (game crate) |

### Roadside colour

Curated flavour data. It has no mechanical effect except where noted, but it is
player-facing text and therefore bound by the vocabulary rules below.

| Concept | Class or catalogue | Module |
| --- | --- | --- |
| Buff | `Buff`, `BUFF_CATALOG` | `data/buffs.py` |
| Brand | `Brand`, `BRANDS` | `data/amenities.py` |
| Billboard | `CORRIDOR_BILLBOARDS` and the roadside sets | `data/billboards.py` |
| Curve | `CurveRecord`, `RouteCurve` | `data/curves.py` |
| Welcome sign | `WELCOME_SIGNS` | `data/state_welcome.py` |

A **buff** is a purchasable consumable -- a shower, a meal, an oil service --
that changes a fatigue, engine or tire accrual rate for a time or for a trip.
Curves are the exception to "no mechanical effect": a `RouteCurve` carries an
advisory speed, and hairpins, sharp and moderate bands have real limits.

**Hairpin** is reserved for a switchback and is spoken about nothing else.
The sharp, moderate and gentle bands come from the advisory speed, but a
hairpin is a SHAPE: the road turns back on itself (135 degrees or more,
MUTCD's Hairpin Curve sign W1-11) and has to be crawled (a Turn sign, 30 mph
or less). A tight corner taken slowly is a **sharp** bend, however slowly.
Spending "hairpin" on ordinary tight corners is what it used to do, and it
left the word meaning nothing when a real switchback arrived (2026-08-23).

### Online

| Concept | Class | Module |
| --- | --- | --- |
| Online identity | `OnlineIdentity`, `OnlinePresence` | `online_presence.py` |
| Journal outbox | `JournalOutbox`, `OutboxItem` | `online_journal.py` |
| Cloud saves | `CloudSaves`, `SyncState` | `cloud_saves.py`, `cloud_save_integrity.py` |

None of these are part of the world model. They project a career or a live trip
outward; nothing in the simulation may depend on them, and every one of them
has to degrade silently when offline.

### The data pipeline

Not classes, but part of the ontology, because the shape is load-bearing:

- Tools edit `data/world_source/`, never directly: go through
  `tools/world_source.py`, where `load_world()` returns the whole world as one
  dict and `save_world(data)` writes it back as per-state shards.
- The game loads the indexed `data/world_data/` tree.
- `tools/index_world.py` regenerates the index; `--check` verifies the two are
  in sync, and CI and tests expect that.

Both trees shard by the state a leg starts in (`legs/TX.json`), so a one-leg
edit is a small reviewable diff rather than a sixty-megabyte blob. There is no
automatic sync: a source edit that is not re-indexed is a change the game will
never see.

## Spoken vocabulary

The canonical player-facing noun for each concept. This is an accessibility
contract, not a style preference: a screen reader user builds a mental model
from the words, and synonyms cost them a re-read.

| Concept | Say | Avoid | Internal name |
| --- | --- | --- | --- |
| The haul contract | job | gig, run, assignment | `Job` |
| A driving control moved to another key | keyboard shortcut (the screen), the key it is on ("Engine on or off: E") | binding, mapping, keybind, hotkey | `bindings::Action`, `KeyBindings` |
| The same on the pad | controller button; right bumper plus «button» for the second layer | modified button, layer two, chord | `bindings::PadChord` |
| One booked citation or violation with its reason | citation / serious violation / major offense / safety incident, then day, clock, reason, fine, place | ticket entry, infraction, offense record, strike | `enforcement::RecordEntry`, `DrivingRecord.entries` |
| The freight itself | cargo, the load | payload, goods | `CargoType`, `Job.cargo` |
| The board of offers | dispatch board | job list, load board | `JobBoard` |
| Asking the carrier's dispatcher for help on a trip, from the pause menu while stopped | call dispatch; the answer starts "Dispatch:" | radio dispatch, phone home, check call, Qualcomm message | `DispatchCallState`, `DispatchCallRequest` |
| The vehicle | truck | rig (except as noted) | `TruckModel` |
| One city-to-city stretch | leg | segment, hop | `Leg` |
| The real highway a leg follows | corridor | -- | -- |
| How far the truck stays on the road it just turned or merged onto | then «distance» on it ("Turn right onto Palm Street, then half a mile on it") | a bare trailing distance ("; half a mile" right after a countdown to the turn reads as the turn moving away), for, stretch, segment length | `Trip::surface_distance_tail` |
| A populated place beside the corridor, spoken when the driver asks where they are | town ("In Pine", "Nearest town, Cicero") | village (that is the bake's category name, not a spoken word), settlement, locality, place | `Landmark.category == "village"`, `DrivingLocationMixin._speak_current_town` |
| Which way the truck is travelling, worded the way the shields sign it | direction, spoken as "Eastbound" | heading, bearing, compass direction (the signed direction is not the geometry) | `_leg_heading`, `DrivingLocationMixin._speak_current_direction` |
| One drive, start to finish | run | trip, haul | `Trip` |
| The pacing choice on the Driving mode row | relaxed, standard, real time | realistic (retired), 1x, normal speed, pacing level | `Settings.time_scale`, `TIME_SCALES`, `mode_name` |
| A truck stop or service POI | stop | POI, waypoint | `Stop`, `RoadStop` |
| The level band | rank | tier, grade | `CareerRank` |
| A group of related achievements | category | group, section, tab | `AchievementCategory` |
| Any rung of the license-and-training ladder, taken together | credential (menu name "Licenses and training") | qualification, license add-on | `CREDENTIALS` |
| A real CDL letter earned by written test (doubles T, tank N, hazmat H) | endorsement | certification, licence, permit | `CredentialTier::Endorsement` |
| Carrier-issued training (refrigerated, flatbed securement, heavy-haul, high-value, LCV) | certificate | certification, endorsement (a certificate is not on the CDL), card | `CredentialTier::Certificate`, `"lcv"` |
| Holding both the tank and hazmat endorsements | the X combination | X endorsement (it is a combination code, not a third test) | `X_COMBINATION_ANNOUNCEMENT` |
| The port security card | TWIC port card | TWIC alone (say what it opens), port pass | `CREDENTIALS`, `"twic"` |
| The federal vetting behind hazmat and the port card | background check | security threat assessment, STA, TSA check (the agency name is fine once, in help) | `PendingCredential` |
| A purchasable consumable | its own name: "shower", "energy drink" | buff, item, power-up | `Buff` |
| The tractor a carrier gave you | your assigned truck | your truck (when leased) | `FleetTier` |
| How much of the lane-holding work the truck does -- holding the truck between the lines against wander and wind. Steering THROUGH a mapped bend is curve assistance, a separate setting and a separate job | lane keeping, always with its value clause ("full, the truck holds the lane and takes your exits") | lane drift (retired 1.9), steering assist, lane keeping assist / lane centering assistance (retired 1.9 -- was a settings promise with no feature; never describe it as working), the bare value word alone -- "off" here is the hardest mode, the opposite of "off" on the rows around it | `settings.lane_keeping` |
| Vehicles around you now | traffic | NPCs, cars | `TrafficManager` |
| Room adaptive cruise leaves to the vehicle ahead | following gap, always with its seconds ("close, two and a half seconds") | following distance, headway, gap on its own (bare "gap" is the lane row below, and the two are different things) | `settings.acc_following_gap`, `ACC_GAP_CHOICES` |
| Room to move into the next lane over | the lane is open ("right lane open"); held by somebody, it is blocked ("right lane blocked by a semi"); at a hazard call the same words name the lane a dodge can go ("Left lane open.", "Either lane open.", "No lane open."; on a road three wide, the neighbour by the L key's name for it, "Middle lane open.") | clear (that is what the truck is clear OF -- the vehicle passed), safe, free, gap, "open lane on the left" | `Trip::open_side_at`, `Trip::lane_blocker_at`, `states/driving_lane_gap.rs` |
| Lane keeping on full moving into the open lane around a slow vehicle the hazard call named, and back to the right lane once it is behind | passing ("Passing on the left.", "In the left lane, passing the slow car.") | overtaking, going around, swerving (the driver's own dodge), dodge | `DrivingState::pass_for_hazard`, `update_pass_return`, `passing_hazard_call` |
| Incidents reported ahead | delays, road reports | traffic (unqualified) | `RealTrafficProvider` |
| A parking space at a stop | parking | slot, spot | `TruckParkingLocation` |
| The trailer liquid bulk rides in | tank trailer | tanker (as a noun for the trailer), tank truck | `TRAILER_CATALOG["tank"]` |
| Freight that is a liquid in bulk | liquid bulk | tanker freight, wet freight | `CargoType.tank` |
| Liquid moving inside the tank | surge | slosh, sloshing, wave action | `LiquidLoad`, `SloshAxis` |
| A tank with bulkheads that damp fore-aft surge | baffled | compartmented, bulkheaded | `CargoType.baffled` |
| A tank with nothing inside to damp surge | smooth bore | unbaffled, food-grade tank | `CargoType.baffled` False |
| How much liquid is in the tank | how full the tank is: "half full", "nearly full" | outage, ullage, headspace, fill level | `LiquidLoad.fill_fraction` |
| Road the truck needs to stop from here | stopping distance | braking distance, stop room | `TruckState.stopping_distance_m` |
| The short code a player reads and types into a browser to connect a computer | activation code | user code, device code, pairing code | `Activation.user_code` |
| The legal driving clock as a whole | hours of service | HOS, the ELD rules, your logbook hours | `HosClock`, `settings.hos_mode` |
| The 11-hour clock that limits time at the wheel | driving time left | drive clock, hours left, hours remaining | `HosClock.drive_time_summary` |
| Driving time already spent this shift | at the wheel | hours driven, drive time used, hours on the road (that is the run) | `HosClock.wheel_time_summary` |
| The 14-hour clock that starts when you go on duty | duty window | 14-hour clock, on-duty clock, shift window | `HosClock.duty_min` |
| The 30 minutes off the wheel the law requires | break | rest break, 30, half-hour rest | `HosClock.break_summary` |
| The in-game time of day (truck-local with a named zone in accelerated modes; computer-anchored and spoken as "local game time" in Real time) | clock | local time, game time | `Trip.local_hour`, `clock_text` |
| When the load is due | deadline | due time, drop time (the appointment is the named hour) | `Job.deadline_game_h` |
| The retarder | engine brake; "jake" in short control feedback ("Jake on, stage two") | retarder | `TruckState.engine_brake_stage` |
| A stretch of road where a town bans the engine brake | no engine brake zone | jake brake zone, engine brake restriction, quiet zone | `Trip.engine_brake_ban_at` |
| The speed this truck, at its weight, can hold a steep downgrade at without overheating its brakes ("Descent control holding 45 miles per hour." / D: "Safe speed 45 miles per hour for the grade.") | the number descent control holds; "safe speed ... for the grade" on D | weight-specific speed, grade speed, runaway speed, descent speed | `TruckState::safe_descent_mph`, `DrivingState::descent_safe_mph` |
| A posted height restriction on the corridor | low bridge | low clearance, height limit, clearance restriction | `RouteRestriction.kind_label` |
| A posted weight restriction on the corridor | weight limit | max weight, tonnage limit, weight restriction | `RouteRestriction.kind_label` |
| The facility entrance where a drive ends | facility gate; "gate" in short cues | entrance (as the noun for the thing), dock gate | `_handle_arrival_gate` |
| The highway exit for the delivery | destination exit | final exit, last exit, your exit | `_destination_exit_stop` |
| The driver's commitment to an exit, made with X wherever it is pressed; the gate a manual exit checks ("Signal set for exit 42", said farther out than the blinker runs; "Signal on for exit 42" once it clicks) | the signal: set, then on | exit armed, intent, confirm (there is no confirm action) | `exit_signal_on`, `exit_blinker_on`, `EXIT_BLINKER_MI` |
| The auxiliary lane with no through traffic that opens beside the right lane at its taper, just before the gore, and that a truck steers into and brakes in from road speed for the ramp ("Exit lane opening. Steer right into it.") | exit lane (the manual may add, once, that road engineers call it the deceleration lane) | deceleration lane (as the spoken noun), slip lane, off lane, exit lane for the right travel lane on the approach (that is "the right lane"; changed 2026-09-24) | `LaneKeeping::exit_lane_open`, `DrivingState::exit_lane_entered`, `driving_events/decel_lane.rs`, `ExitRampLayout::decel_mi`, `EXIT_TAPER_MI` |
| The bend at the end of the exit lane that the exit speed is for ("Ramp curve, too fast. Slow to 30 miles per hour.") | ramp curve | ramp bend, loop, the curve (unqualified, on a ramp) | `DrivingState::ramp_curve_radius_ft`, `ExitRampLayout::curve_mi` |
| The warning that a bend or ramp curve is being taken, or approached, faster than this load and this lane keeping take it at no cost, with the speed that does ("Sharp left, too fast. Slow to 37 miles per hour.") | too fast; slow to (the number) | drifting to the outside (the old line, retired 2026-09-24), rollover warning, safe speed, advisory (the number is not the sign's) | `driving_rollover::update_curve_warning`, `curve_safe_mph` |
| A truck going over on its side in a bend because the bend pulled harder than the load's rollover threshold ("The truck rolled over in the bend.") | rolled over | tipped over, flipped, jackknifed (a different crash), overturned, wreck | `DrivingState::roll_over`, `TruckState::roll_share`, `vehicle/roll.rs` |
| An accident on the driving record, such as a rollover: counted on the safety record and reputation like a serious violation, public as a count ("It goes on your driving record as a crash.") | crash | accident (the regulation's word, never spoken), wreck, incident, collision (that is the traffic hit) | `RECORD_CRASH`, `DrivingRecord::crashes`, `crash_times`, `crashes` on the public profile |
| The ramp's advisory, the speed its curve is built for, said once as the truck enters the exit lane ("Exit speed 45.") | exit speed | ramp speed, ramp limit, safe ramp speed, advisory (as the spoken noun) | `armed_ramp_mph`, `take_the_ramp`, `ExitRampLayout::curve_mph` |
| The assist that brakes and creeps the truck to the facility entrance, and, from a cleared ramp-end stop, drives it there hands off | Facility stopping assistance (the settings row's own name) | destination approach assistance (the setting's code name only), arrival assist, approach assist | `destination_approach_assist`, `update_destination_approach_assist`, `terminal_release_text` |
| The assist that takes a mapped bend for the driver: brakes to its advisory on the approach, holds that speed through it, brakes inside a bend entered too fast, and steers the wheel the bend wants | Curve assistance (the settings row's own name); "slowing" when it takes the brakes, "released" when it lets go | curve speed assistance (its name before it steered), turn assist, corner assist, curve braking, bend assist, the servo (code only) | `curve_speed_assist`, `driving_updates/curve_servo.rs`, `sim::lane::tracking_steer_rad`, `update_lane` |
| A driveway, delivery lane or parking aisle OSM holds no name for, and the approach to a facility the world generated rather than surveyed | a service road | unnamed public road, access road, service way, private road, local facility access road | `build_local_geometry.UNNAMED_SERVICE`, `build_local_approaches.fallback_road` |
| A residential or minor street OSM holds no name for | a side street | unnamed public road, unnamed street, local road, back road | `build_local_geometry.UNNAMED_STREET` |
| A street maneuver the route asks for | turn | corner, junction, intersection, manoeuvre | `_is_judged_turn`, `local_turn` cues |
| The speed a turn has to be taken under, from the turn's own measured angle | advise ("Advise 9", the pacenote word) | turn limit, corner advisory, max speed | `data::corners`, `turn_speed_mph` |
| The loop-back after missing the destination exit, the facility gate, the stop at the end of the destination ramp, or a turn | safe turnaround | U-turn, turnaround point, loop | `_handle_missed_destination_exit`, `_handle_missed_facility_gate`, `_loop_back_to_destination_terminal`, `_handle_missed_turn` |
| The give-way control at a ramp terminal: slow for the gap, stop only if the road is not clear | yield ("Yield at ramp end", "Through the yield in a gap") | give way (the OSM tag, not a spoken word), yield sign as the noun in short cues | `_ramp_control == "yield"`, `YIELD_ROLL_MPH` |
| The circular terminal a ramp can end at, played by yield rules against circulating traffic | roundabout | traffic circle, rotary, circle | `_ramp_control == "roundabout"` |
| A signal OSM maps at an intersection on a facility's streets, played by the ramp terminal's rules (off in 1.9, `STREET_CONTROLS_IN_PLAY`) | traffic light ("Traffic light ahead. Light red.") | signal, stoplight, street light, ramp light | `driving_events/street_controls.rs`, `street_control_kind == "signal"` |
| A public street between a ramp and a facility's or road stop's driveway, each at its own posted limit. Spoken by its own name, from the turn-by-turn, or not at all: a street is not a zone ("Speed limit 55.", "Speed keeper holding 40 miles per hour.") | the street's name | facility access road, access road, access road zone, facility access road zone (the zone reasons `STREET_ZONE` and `STOP_STREET_ZONE`, never spoken) | `spoken_zone`, `is_street_zone_reason`, `Trip::street_zones` |
| A stop sign every approach to the intersection stops at: the truck's turn comes after its own stop, with no gap to wait for | all-way stop ("All-way stop ahead.") | four-way stop, 4-way, multiway stop | `street_control_kind == "all_way_stop"` |
| Where a facility's streets leave the public road for its own service or private way (the manual's word; the drive names the turn onto it) | driveway | entrance, property line, gate (that is the check-in stop at the end of the yard) | `Driveway`, `Trip::driveway_mi`, `Leg::local_yard` |
| The facility's own way past the driveway, up to the check-in stop at the gate | the yard ("Into the yard.") | lot, facility grounds, premises, private road | `YARD_ZONE`, `Trip::street_zones` |
| The speed limit inside the yard: the operator's rule, not a law | yard limit ("Yard limit 15.") | gate speed, facility limit, lot limit (that is a road stop's) | `YARD_LIMIT_MPH` |
| The public road from a road stop's exit ramp to its driveway, in the manual and the docs; spoken by the street's own name, like a facility's (the row above). Off in 1.9 (`STOP_STREETS_IN_PLAY`): the stop's entrance is the ramp's end | access road | frontage road (unless the road is named one), service road (that is the driveway's), approach, access road zone | `STOP_STREET_ZONE`, `begin_stop_chain` |
| A truck stop's, travel center's or fuel station's own grounds past its driveway | the lot ("Into the lot.") | yard (that is a facility's), property, parking area | `LOT_ZONE`, `DrivingState::stop_chain` |
| The speed limit inside a road stop's lot: the same rule as a yard | lot limit ("Lot limit 15.") | yard limit (a facility's), parking lot speed | `LOT_ZONE`, `YARD_LIMIT_MPH` |
| The vehicles crossing in front of a stopped truck at a ramp terminal | cross traffic | crossing cars, the crossroad's traffic | `CrossTraffic`, `sim/cross_traffic.py` |
| The opening in cross traffic a driver pulls out into | gap, always yours ("wait for your gap", "Gap in traffic") | window, opening, break | `CrossTraffic.clear_to_cross` |
| The fine for engine braking in one | engine brake citation | jake ticket, noise fine | `EngineBrakeZoneMixin._fine_engine_braking` |
| An offense that counts toward losing the CDL | serious violation | strike, point, demerit, infraction | `DrivingRecord.record_serious_violation` |
| The career-long enforcement history | your record | rap sheet, history, file | `DrivingRecord` |
| A place on the road where an officer may be sitting, and the sounding entry that teaches it | enforcement post | patrol, speed trap, trap, checkpoint, bear | `EnforcementPost` |
| The officer | trooper on the highway; officer at a scale | cop, bear, smokey, unit, LEO | `EnforcementPost.agency` |
| The two rising tones an enforcement post makes before it can see you | Enforcement marker | signature, earcon, warning tone, post ping | `enforcement/signature`, `_mark_post_audible` |
| A marked police vehicle passing as the truck goes by an enforcement post -- the marker with a whoosh behind it | Police car going by | marked unit pass, patrol car pass, trooper pass | `traffic/trooper_pass`, `_play_marked_unit_pass` |
| The held siren a driver hears when a trooper is pulling them over | Siren | siren loop, cop siren, alarm tone | `events/police_siren`, `states/driving_siren.py` |
| Being pulled to the shoulder by one | pull-over | stop (already the POI, the act of stopping, and the command) | `_pull_over` |
| The checkpoint watching for chain-law compliance on a graded climb | chain control | chain checkpoint, chain law stop, chain station | `EnforcementPost` (`KIND_CHAIN`), `REASON_BY_KIND` |
| The inspection facility | weigh station; "the scale" in short cues | scale house, weigh point, chicken coop | `RoadStop(type="weigh_station")` |
| The certified commercial scale on a truck stop's lot a driver pays to weigh on | CAT Scale; always named, never "the scale" | truck scale, Cat scale, public scale, weighbridge | `RoadStop.services` `scale`, `rest_stop/cat_scale.rs`, `tools/cat_scales.py` |
| The printed result of a weigh, read aloud | CAT Scale ticket | weight ticket, scale ticket, weigh slip | `AxleLoads::ticket_text` |
| The axle groups a ticket names | steer axle, drive axles, trailer axles, gross | front axle, drives, tandems, rear axles, GVW | `TruckState::axle_loads`, `AxleLoads` |
| Whether it is working today | open / closed | active, manned, staffed, live | `KIND_FIXED_SCALE` vs `KIND_SCALE_APRON` |
| The fleet or self-purchased equipment that gets a weigh-in-motion verdict before an open scale, instead of every truck being demanded in | transponder | PrePass, bypass reader, weigh-in-motion unit | `business.has_weigh_station_transponder`, `business.WEIGH_STATION_TRANSPONDER_LEVEL` |
| The transponder's verdict clearing a truck to keep rolling past an open scale | Scale green light | bypass cleared, waved through | `events/scale_green`, `_resolve_transponder_verdict` |
| The transponder's verdict sending a truck into the scale anyway | Scale red light | red-lighted, called in | `events/scale_red`, `_resolve_transponder_verdict` |
| An officer going over the truck, the driver's papers, or both, at a scale or on the shoulder | roadside inspection; "Level 1 full inspection", "Level 2 walk-around inspection", "Level 3 driver inspection" for the kind | DOT inspection, safety check, CVSA inspection, audit | `sim/roadside_inspection::InspectionLevel`, `DrivingState::settle_inspection` |
| The item an inspector writes up | written up for ...; the finding itself is the noun ("brakes out of adjustment") | violation, defect code, OOS item | `roadside_inspection::Finding` |
| A critical item that parks the truck until it is fixed | out of service until repaired | OOS, red-tagged, grounded | `Finding::out_of_service`, `Repair` |
| An officer's order parking the driver, served in full on the spot and counted on the safety record (public as a count, never its reason) | out-of-service order | OOS, OOS order, put out of service | `Profile::out_of_service_events`, `DrivingRecord::out_of_service_times`, `outOfServiceOrders` on the public profile |
| The sticker a clean Level 1 earns, good for three months of being waved past open scales | inspection decal; "the decal on the windshield" | CVSA sticker, decal, bypass sticker | `DrivingRecord::decal_until_h`, `DECAL_VALID_HOURS` |
| The driver's own pre-trip check of the same items | walk-around; "Walk around the truck" is the row | pre-trip, DVIR, pre-trip inspection, vehicle check | `roadside_inspection::walk_around`, `WALK_AROUND_MIN` |
| The three days in May when every inspector is on the road | Roadcheck week | blitz, inspection blitz, Roadcheck event | `roadside_inspection::roadcheck_blitz`, `Trip::roadcheck_blitz` |
| Drivers talking about enforcement on the radio | CB chatter (the name in help and menus; the spoken report itself opens with the distance or "Somebody", never a "CB chatter" label -- the squelch marks it, owner 2026-10-01) | radio talk, scanner, traffic | `cb_patrol_message` |
| A CB report nobody has verified | unconfirmed | rumor, maybe, possible, unreliable | `_cb_confidence` |
| The last CB call said again because the driver asked for it | repeat the CB chatter | CB replay, rewind, play back the CB, last CB | `DrivingState::speak_last_cb_chatter` (Alt C) |
| How much police activity you hear | it is not a setting -- the road's own presence, from region, road class and the clock | enforcement presence (the player setting, removed 2026-08-16), police density, patrol frequency, difficulty | `Trip._post_density_at`, `EnforcementWatchMixin._ambience_scale` |
| How much dispatch and the shippers trust you: the delivery ledger less what your driving record still costs, 0 to 100 | reputation | standing, trust score, rating, rep (the raw ledger is never spoken; a citation or serious violation shows on this number for one game year, a major offense for good; the carrier's review and the insurer look back the same year; the CDL suspension ladder keeps three) | `Profile::standing`, `enforcement::standing_reputation`, `career.standing` in the save |
| How interesting you look to an inspector | safety record | ISS, CSA, SMS, score, rating | `Profile.selection_score` |
| What the licence file holds against you: citations, serious violations, major offenses | driving record; "Record:" in short status | MVR, rap sheet, points, history | `DrivingRecord`, `enforcement.standing_text` |
| The carrier deciding, from the driving record, whether to keep a company driver and on what equipment | the carrier's record review | MVR check, annual review, insurance screening | `enforcement.record_band`, 49 CFR 391.25 |
| What an owner-operator's insurer adds to the reserve for the driving record | insurance surcharge; "surcharged for your driving record" on the settlement line | rate hike, premium bump, points surcharge | `enforcement.record_insurance_surcharge` |
| The CDL being off the road for a set time | CDL suspension; "suspended" in short status | ban, revocation, lockout | `DrivingRecord.suspended` |
| The permanent version of it, after a second major offense | lifetime disqualification | permaban, blacklist | `DrivingRecord.lifetime_disqualified` |
| What a lifetime disqualification does to the career: the driving stops, the save stays readable | the career is over; "Career ended" on the public profile | game over, permadeath, deleted, wiped | `enforcement.career_ended`, `careerEnded` |
| The player's own removal of an ended career's save and cloud backups | close out this career | delete career (that is the title-menu action for any career), wipe, purge | `CloseOutCareerState` |
| An offense heavy enough to disqualify a CDL outright | major offense | felony (as the game's own noun), big one | `DrivingRecord.record_major_offense` |
| Running off the road asleep | fatigue event | microsleep (that is the warning, not the event), nod-off | `DrivingRecord.record_fatigue_event` |
| How far dispatch will work with you right now | dispatch trust | standing, rep level, tier | `enforcement.standing_band` |
| Everything the driver is behind by, cash and balance together | debt; "what you owe" in a sentence | negative balance, in the red, in the hole, arrears | `solvency.debt_owed` |
| Charges a settlement could not cover, carried to the next one | balance owed | tab, IOU, outstanding fines, arrears | `Profile.fines_owed` |
| The figure debt is not allowed to pass | ceiling | limit, cap, threshold, max | `solvency.debt_ceiling` |
| The share of every settlement that always reaches the driver | take-home | net after collection, what is left, remainder | `solvency.TAKE_HOME_SHARE` |
| The carrier ending a company driver's employment | ended your employment | terminated, fired, let go, sacked, dropped | `solvency.apply_company_termination` |
| The lender taking an owner-operator's tractor back | took the truck back; repossessed | seized, foreclosed, repo, impounded | `solvency.apply_repossession` |
| The level-18 step from company driver to leased-on owner-operator | the owner-operator buy-in; buy in | lease, lease-purchase, upgrade, promotion | `BusinessStatusState::become_owner_operator`, `OWNER_OPERATOR_BUY_IN` |
| Choosing the company seat with the buy-in open | stay a company driver; company driver by choice | decline, opt out, refuse, skip the lease | `Profile.owner_operator_declined`, `BusinessStatusState::stay_company_driver` |
| An owner-operator handing the equipment back for a company seat | go back to company driving | quit the lease, sell out, downgrade, revert | `solvency.apply_return_to_company_driving` |
| Equipment the level earns that dispatch trust is withholding | held back | locked, gated, downgraded, nerfed, demoted | `carrier_fleet.equipment_held_back` |
| Career experience arriving slower in low dispatch trust | reduced rate | XP penalty, multiplier, malus, nerf | `career.standing_xp_rate` |
| The first damage band: the engine holds power back | reduced power | derate, band two, power loss | `DAMAGE_BAND_REDUCED` |
| The deep damage band: reduced power plus a road-speed cap | limp mode | limp-home, speed governor, safe mode | `DAMAGE_BAND_LIMP` |
| Damage or component wear past the point where the truck may be driven | out of service | broken down, totaled, disabled, dead truck | `TruckState.out_of_service` |
| Component wear approaching the game's required repair threshold | service soon | legal defect, failed DOT inspection | `COMPONENT_SERVICE_WARNING_PCT` |
| Component wear at the game's required repair threshold | service required | regulatory defect limit, random breakdown | `COMPONENT_SERVICE_LIMIT_PCT` |
| The carrier taking a company tractor off the road | dispatch grounds it, grounded | benched, red-tagged, impounded | `_carrier_grounds_the_tractor` |
| The tractor a grounded company driver is moved into | yard spare | loaner, replacement truck, backup rig | `_draw_yard_spare` |
| The emergency call-out that gets an out-of-service truck moving | roadside repair | roadside rescue (that is the fuel one), tow | `_roadside_repair_out_of_pocket` |
| Miles the fuel in the tank reaches at this drive's own miles per gallon | fuel range, range | mileage left, distance to empty, endurance | `DrivingState::fuel_range_mi` |
| The once-per-shortfall cue when the fuel range will not reach the next fuel stop | fuel range warning | low fuel warning (that is the fifteen percent one), range alert | `DrivingState::check_fuel_range_warning` |
| The emergency call-out for an empty tank | roadside rescue | roadside repair (that is the damage one) | `_handle_out_of_fuel` |
| Losing the truck to speed, usually out of gear on a grade | runaway | overspeed (that is the posted-limit one) | `RUNAWAY_SPEED_MPH` |
| Backing along a travelled lane, away from the destination | driving the wrong way; backing | reversing (that is the gear), wrong-way driver, going backwards | `WRONG_WAY_WARN_MI` |
| The lane roadwork has coned off, named by the side of the road it is on | the right lane is closed; the left lane is closed | lane one, the outside/inside lane, the middle lane (a closure is always an edge lane), lane closure | `Zone.closed_side`, `Trip.closed_lane_at` |
| The mile of reduced speed ahead of a work zone | reduced speed for construction; "speed limit 55 from one mile out" | taper (that is the short merge at its end, MUTCD Part 6), merge zone, approach zone, transition area | `Zone.reason == "construction merge"`, `CONSTRUCTION_TAPER_MI` |
| Where to be instead of the closed lane | merge left / merge right (before the work zone); keep left / keep right (inside the work zone) | move over, get out of that lane, stay in the left lane (untrue where the road is three wide) | `Trip._closure_phrases` |
| What state the freight is in | the load; "freight" where "load" would be ambiguous | cargo condition, product, goods | `TruckState.cargo_damage_pct` |
| The receiver's note about damaged freight | exception on the bill of lading | OS and D, discrepancy, ding | `CARGO_OUTCOME_EXCEPTION` |
| What the carrier owes for freight it damaged | freight claim | damages, cargo insurance, write-off | `CargoSettlement.claim_value` |
| The receiver refusing a load outright | the receiver refused the load | bounced, returned, kicked back | `CARGO_OUTCOME_REJECTED` |
| Damage a safety committee rules the driver's fault | preventable damage | at-fault, chargeable, negligence | `TruckState.preventable_damage_pct` |
| The polling secret bound to this device | never spoken -- internal only | activation code | `Activation.device_code` |
| The engine leaning the way the wheel should go -- into a bend or a street corner only when that steering is the driver's (lane keeping off and curve assistance off; on full, for the road's shape), and, with lane keeping on partial or off and lane-departure warning on, back toward lane center on a drift. The one panned cue a driver steers TOWARD; the rumble strip is the opposite and is steered away from | The engine lean | the road lean (what it was called while it rode the road bed, before 2026-09-18), engine pan, steering lean, drift beep (that is the rumble strip, and it means the other direction) | `sim/turn_guide.rs`, `DrivingState::update_lane_guidance_audio`, `sim/lane_guidance.rs` |
| Where the truck is sitting across its lane, reported by panning the road noise; centered whenever lane keeping is doing the steering. A position readout, never a direction to steer | Where you sit in the lane | the road lean (that is the engine now), road bed, ambient road, tire hiss | `vehicle/road`, `DrivingState::update_lane_guidance_audio` |
| A tire just catching the edge line, still fully inside the lane | Rumble strip, clipped | edge clip, low rung of the edge ladder | `vehicle/edge_clip`, `sim/lane_guidance.EDGE_CLIP_KEY` |
| The whole tire riding the rumble strip on one side | Rumble strip | edge strip, full rumble, middle rung of the edge ladder | `vehicle/edge_strip`, `sim/lane_guidance.EDGE_STRIP_KEY` |
| Gravel under a tire that has left the road surface | Off the pavement | shoulder gravel, run-off, top rung of the edge ladder | `vehicle/edge_shoulder`, `sim/lane_guidance.EDGE_SHOULDER_KEY` |
| The soft chime confirming the truck is centered again after a drift | Back in the lane | lane-centered chime, drift recovered, all-clear | `vehicle/lane_centered` |
| Tires rolling over a painted line's raised markers, meaning a lane change happened whether meant or not | Lane line crossed | line cross, lane-change bump | `vehicle/lane_line_cross` |
| The held control that steers out the truck's heading so it points down the road, without moving it back to lane center; does nothing on full lane keeping | Straighten up (the shortcuts row), "hold slash" by default | center steering, recenter, auto-straighten, lane keeping (it is not: it leaves the lane position alone) | `Action::Straighten`, `LaneKeeping::straighten` |
| The tock that pans to where the truck sits inside its lane, toggled with I | Lane locator | position tick, lane ping, centering assist | `vehicle/lane_locator` |
| A turn signal relay's tick and softer tock, one flasher cycle per 0.9 s beat, following steering position or assisted lane-change direction; an armed exit repeats on the right from half a mile out (`EXIT_BLINKER_MI`) until ramp entry, cancellation, or a missed exit | Mechanical blinker | indicator tick, automatic locator | `vehicle/turn_signal` |
| The turn signal stalk clicking back, centred and quieter, when the steering cue ends, the exit position is set, or a running exit blinker is canceled; assisted lane changes instead finish with Lane line crossed | Blinker off | signal tone (a different sound since 2026-09-29), cancel chime, all-clear, "exit lane set" tone | `vehicle/turn_signal_off` |
| One rumble hit with nothing held after it, unattached to a steering correction -- fatigue or a momentary catch | Rumble strip, single hit | single tap, fatigue rumble | `vehicle/rumble_strip` |
| Grouped bars cut across a whole lane, placed only ahead of a curve that has killed people | Transverse strips | rumble bars, wake-up strips, dead-man's-curve strips | `vehicle/transverse_strips`, `sim/lane_guidance.TRANSVERSE_KEY` |
| The tick that speeds up as the truck closes on a ramp's stop bar, handing over to the stop bar tone once the bar is close enough that the truck must already be stopping | Stop bar countdown | curve chime (retired 2026-09-18 -- the bend chime no longer exists, and this sound is all that still uses the key), curve bink, bend warning, bar tick | `vehicle/curve_bink`, `DrivingState::update_ramp_bar_audio` |
| A short confirmation from the side of a deliberate lane crossing, shoulder pull-over, ramp merge, or route exit | Signal tone | confirmation tone, manoeuvre tone | `vehicle/signal_tone` |
| The compressor filling the air tanks before the truck can move | Air building | air pressurize, tank fill | `vehicle/air_pressurize` |
| The short sharp pop when the tanks reach full and the compressor cuts out | Air dryer purge | dryer pop, compressor cutout | `vehicle/air_dryer_purge` |
| Air pressure fallen too low to brake safely | Low air buzzer | low-pressure alarm, air warning | `vehicle/low_air_buzzer` |
| The parking brake engaging: a hard mechanical clunk of air dumping | Parking brake set | brake set, park brake on | `vehicle/brake_set` |
| The parking brake releasing, freeing the truck to roll | Parking brake released | brake release, park brake off | `vehicle/brake_release` |
| The hardest stop the truck has, for a hazard that cannot otherwise be missed | Emergency brake | e-brake (bare abbreviation), panic stop | `vehicle/ebrake` (fallback `vehicle/brake_air`) |
| The hazard assist taking the truck and stopping it on the service brakes | Automatic braking | AEB, auto brake, emergency braking (that names only the escalation, and the routine stop never reaches it) | `states/driving_updates.py._update_hazard` |
| Tires losing grip on the road | Tire screech | skid, traction loss | `vehicle/tire_screech` |
| The retarder's lightest setting: two cylinders of retard | Engine brake, stage one | jake stage one, retarder low | `engine/jake_1600`, `JAKE_STAGE_GAIN` in `states/driving_updates.py` |
| The retarder's usual working setting on a long descent: four cylinders of retard | Engine brake, stage two | jake stage two, retarder mid | `engine/jake_1600`, `JAKE_STAGE_GAIN` |
| The retarder at everything it has: six cylinders of retard | Engine brake, stage three | jake stage three, retarder max | `engine/jake_1600`, `JAKE_STAGE_GAIN` |
| A chime for being over the posted limit right now | Overspeed chime | speed warning, ticket chime | `vehicle/overspeed_chime` |
| A manual shift that did not take | Gear grind | missed shift, grinding gears | `vehicle/gear_grind` |
| The continuous tone meaning a stop bar is close enough that the truck must already be stopping | Stop bar tone | stop tone, bar tone | `vehicle/bar_solid` |
| The ramp signal at the bottom of the ramp going green | Green light | ramp green, signal green | `events/ramp_light_green` |
| The ramp signal going red | Red light | ramp red, stop signal | `events/ramp_light_red` |
| Something in the truck's path that needs a real reaction now | Hazard warning | obstacle warning, danger cue | `events/hazard_warning` |
| The hazard now behind the truck | Hazard clear | hazard passed, all clear | `events/hazard_clear` |
| The truck striking a hazard it did not clear (the failure half of the dodge outcome pair) | Collision | crash, impact, hit | `vehicle/collision` |
| Roadwork ahead: the posted limit drops and a lane may close | Construction zone | work zone, roadwork cue | `events/construction_zone` |
| A National Weather Service warning active on the road, spoken with the Service's own product name after it ("Weather alert: High Wind Warning, gusts to 60 miles per hour") | Weather alert | weather warning (as the noun), advisory, watch, bulletin, NWS alert | `sim/real_weather_alerts.rs`, `DrivingState::sync_weather_alerts`, on with the `real_weather` setting |
| The traffic ahead coming down in speed | Traffic slowing | slowdown cue, brake lights ahead | `events/traffic_slowing` |
| A street maneuver coming up on a local drive | Turn ahead | maneuver cue, upcoming turn | `events/turn_ahead` |
| The next maneuver is a left | Turn left | left-turn cue, left maneuver | `events/turn_left` |
| The next maneuver is a right | Turn right | right-turn cue, right maneuver | `events/turn_right` |
| Crossing into another state | State line | border cue, state-crossing chime | `events/state_crossing` |
| A toll gantry or plaza billing the truck | Toll charged | toll cue, gantry chime | `events/toll_charged` |
| The driver's own yawn as fatigue builds | Yawn | fatigue sound, drowsy cue | `driver/yawn` |
| The optional note that carries the lean instead of the engine, off by default; with it on the engine stays centered | Lane guide tone (the settings row is "Lane guide sound: engine" or "tone") | guide tone, steering tone, lean tone | `guide/lane_guide_tone`, `Settings.lane_guide_tone`, `DrivingState::lean_the_tone` |
| The setting for which way to steer when the engine leans: toward the lean by default, or away from it | Steering guide (the settings row reads "steer toward the lean" or "steer away from the lean") | invert steering, reverse guide, flipped lean | `Settings.steering_guide_inverted`, `guide_sign` |
| The one-time screen before the main menu that asks how much the truck should do, answered once by every player | Driving assistance (the same choice as the preset row in Settings) | setup wizard, onboarding, first-run screen | `states::assist_picker`, `Settings.assist_preset_chosen` |
| Being looked at for something other than speed: damage, missing chains, following too close | Inspection warning | inspection cue, roadside-check tone | `events/inspection_warning` |
| The earcon that fires with the open-scale approach notice, ahead of the ambient bed | Scale warning | weigh-station warning cue, scale earcon | `events/weigh_station_warning` |
| The ambient bed that swells as the truck comes up on an open scale | Weigh station | scale bed, weigh-lane loop | `poi/weigh_station_lane` |
| The end of a pursuit | Spike strip | tire-deflation device, pursuit ender | `events/spike_strip` |
| The sounding entry for liquid running back and forth inside a tank trailer (the concept itself is "surge", above) | Surge | slosh, sloshing, wave action | `vehicle/liquid_wash`, `sim/surge.LiquidLoad` |
| The load hitting the front or back of the tank | Surge strike | liquid hit, fore-aft strike | `vehicle/liquid_hit`, `sim/surge.SloshAxis` |
| The load hitting the side of the tank -- the one that rolls trucks | Surge strike, sideways | lateral hit, side slosh | `vehicle/liquid_hit_lateral`, `sim/surge.SloshAxis` |
| A station built from one of the player's own playlist files | playlist; what is in it is tracks, whether a track is a file or an internet station | mix, folder, media library, your files | `PERSONAL_PLAYLIST_SOURCE_TYPE`, `RadioStation.playlist_entries` |
| A personal playlist playing its tracks in a random order, every track once per lap | shuffle | random play, mix mode, randomize | `radio_shuffle_playlists`, `PlaylistShuffleLap` |
| Music the game composes itself, plus the restored 1.5 tracks; what Music source: Synthesized plays in menus and on the Roadhouse | synthesized music | no-AI music, generated music, procedural music | `Settings.synth_music`, `ff_core::music_synth` |
| The one career whose accepted backups front the player's public profile | public career | shared career, featured career, main save | `publicSaveName` |
| A career changed outside the game that was checked by hand and turned down, so it no longer backs up | "<career>: backup declined after review" | rejected, banned, blocked, quarantined | `cloud_saves::rejection_status` |
| A career's copy on orinks.net, and the act of sending it there | backed up / backup -- "<career> is backed up", "Backed up to the cloud"; the by-hand row on each career's Cloud backup screen is "Back up this career now"; the Speech settings row for how often the all-clear speaks is "Say when a career is backed up" | synced, uploaded, saved to the cloud, cloud save (the *menu* is Cloud backup, but a career is never "a cloud save") | `cloud_saves.backup_status`, `cloud_saves.recovery_status`, `city._backup_outcome_text` |
| The page on orinks.net where a player manages their driver name, their sharing, and the computers signed in to the account | driver setup page | account page, dashboard, my account, profile page (that name belongs to the public one) | `online_presence.setup_page_url`, the Online hub's "Open my driver setup page" |
| The other players out working right now, and the screen that shows them | drivers on duty; the thing itself is the drivers **list** ("this list", "the drivers list") | drivers board (board belongs to dispatch, one row above), drivers online (Online is the hub's name *and* a setting the player toggles, so it reads as "drivers who have online services on"), roster, who's online, live board | `DriversOnlineState`, `getLivePresenceBoard` and the `Drivers on duty` heading on orinks.net |
| Another driver appearing on or leaving the drivers list, and the optional spoken notice of it | "<driver> is on duty" / "<driver> went off duty"; the Online menu row that turns the notice on is "Say when drivers go on or off duty" | joined, logged in, came online, signed off, left the board, notification (the row says "say when") | `duty_watch.duty_change_text`, `settings.duty_notifications` |
| Every driver with a public profile, on duty or not, and when each was last on duty; the screen that lists them | driver directory; a row says "On duty" or "Last on duty three days ago", and "Not seen on duty yet" for a driver no session has ended for | roster, member list, all drivers, offline drivers, player list, last seen, last online (the stamp is when they went off DUTY, the board's own word) | `DriverDirectoryState`, `getDriverDirectory`, `GET /api/freight-fate/directory`, the Driver directory page on orinks.net |
| One driver's public page read aloud in the game -- opened with Enter on the drivers list, or as the player's own from the Online menu | driver profile; the player's own is "Your profile"; a driver whose page is hidden "has no public profile" | profile card, driver page, stats screen, player card, private profile (the game never says why a profile is hidden) | `DriverProfileState`, `GET /api/freight-fate/drivers/<driverId>` |
| Game sound lowering itself while the road voice speaks | game sounds step back for speech (the Settings > Audio row) | ducking, audio duck, attenuation | `settings.duck_audio_for_speech`, `AudioEngine.set_speech_duck` |
| Where the game's lines go: spoken, or to the screen reader's braille display with nothing spoken | Output (the Settings > Speech row), with speech and braille / braille only as its values; the display is "your braille display" | speech mode, silent mode, mute speech, braille backend, braille channel | `settings.braille_only`, `SpeechSink::set_braille_only`, `VoiceBackend::braille` |
| How loud the lane and edge cues are next to everything else | lane and edge cue volume (the Settings > Audio row), with quieter / standard / louder as its values | prominence (what the row said until 1.9, and what the owner could not decode), lane cue loudness, cue intensity | `settings.lane_cue_loudness`, `sim/lane_guidance.CUE_LOUDNESS` |

Notes on the entries that are not simple:

**"On duty" does double service, and that is a deliberate accepted cost.** It
is already the player's own regulatory duty status -- `sim/hos.rs` speaks
*"on duty, not driving"*, the logbook reads *"Today's totals: driving...,
on duty not driving..."*, and the duty window is *"the 14-hour clock that
starts when you go on duty"* two tables up. *Drivers on duty* uses the same
two words for a different subject: whether somebody ELSE is out working.

Both alternatives were worse. *Drivers board* takes a noun dispatch already
owns, and dispatch's board is the one a player hears about far more often.
*Drivers online* collides with the hub's own name and with the `Online
services` switch, so it reads as "drivers who have online services turned
on" -- close enough to true to mislead. The saving grace is that the HOS
sense the game actually speaks aloud is always the qualified *"on duty, not
driving"*, while bare "on duty" for "currently working" is ordinary English
before it is jargon, and the two never appear in the same screen. If a
tester ever reports confusing the two, this note is the place to start.

**"Standing" is a code word, never a spoken one.** Dispatch trust answers to
three things now -- the service record, the licence, and what the driver owes
-- and the code calls that combination standing (`enforcement.standing_band`).
The game still says *dispatch trust*, because it is one ladder with more
inputs, not a second status. A screen reader user should not have to hold two
words for one question, and "standing" was already ruled out as a synonym
above. `enforcement.standing_text` is a separate thing again: it speaks as
"Record: ...", and it is the enforcement record, not the trust band.

**Say the cause, not the ladder.** Because dispatch trust now has three
inputs, "clean on-time runs rebuild it" is false for a driver whose service is
fine and whose debt is the problem. Every place that says trust is down names
which of the three is holding it and what specifically clears that one.

**Neither ending is an ending.** Ending an employment and taking a truck back
are setbacks with a stated way forward, and the spoken text says so in the
same breath. Never write "game over", "career over", "you failed", "start
over", "bankrupt", or anything implying the save is gone -- none of that is
true, and all of it lands hardest on the player it is describing.

**Job and load are two concepts, not two names.** The job is the contract; the
load is the freight on the trailer. "Use Abandon job to drop the load" is
correct and should stay that way. What is wrong is using "load" for the
contract -- "no load, no pay" means the job.

**"Rig" is flavour, not function.** It never appears in a menu item, prompt,
status readout or warning. It appears in achievement prose, where a second word
for the truck reads as writing rather than as a new concept, and it is the
label of one truck model ("standard rig"). Keep it out of functional text.

**"Corridor" is an explanatory word.** It belongs in help and manual text
describing where the map comes from ("routes made from real highway
corridors"). Per-drive navigation says leg.

**"Device code" is never spoken.** It is the polling secret bound to this
device, not something a player ever reads back or types anywhere -- only the
activation code crosses to the player. Keep it out of every spoken and
transcript-bound string; if a screen reads it out loud, that is a bug.

**The damage bands are three nouns, not three ways of saying "damaged".**
Reduced power is the engine holding back; limp mode is that plus a road-speed
cap the driver cannot drive out of; out of service is the truck no longer being
legal or able to drive at all. Each one means something different is true about
what the truck will do next, so they are never used loosely or
interchangeably. Every readout that gives a damage number gives the band beside
it -- the number alone tells a player nothing about what they have lost.

**"Out of service" is the real-world term and carries its real meaning.** It is
not a colourful way of saying "very damaged": it is the state in which a truck
may not be driven, borrowed from the out-of-service criteria an inspector
applies at the roadside. The same words already name the hours-of-service
version (`_place_out_of_service`), which is correct -- both mean "this truck
does not move until something is put right" -- so never introduce a second
noun for either.

**Grounding is what the carrier does; out of service is what the truck is.** A
company driver hears both, in that order, because they are different facts: the
truck is unfit, and the carrier has taken it off the road as a result.

**Truck damage and load damage are never the same sentence.** The truck's
number is the driver's problem and prices at a garage; the load's is the
customer's and prices at the freight's value, which is far larger. A readout
that gives one must not let the player assume it covers the other, so the Tab
screen names the freight separately from the truck.

**The load's condition is spoken in words first, the number second.**
"Eighteen percent" tells a player nothing about whether the receiver will sign
for it; "damaged, eighteen percent" tells them both. The words are fixed --
secure, shifted but sound, damaged, badly damaged, ruined -- and each one maps
to exactly one thing the dock will do.

**"Roadside rescue" and "roadside repair" are two different call-outs.** The
rescue brings fuel; the repair gets an out-of-service truck moving. They cost
different money and different hours, so they keep different names.

**"Stop" is the most overloaded word in the game** -- the POI, the act of
stopping the truck, and the command to do so. Where the sentence could be read
either way, name the thing: "the truck stop ahead", "bring the truck to a
stop".

**"Buff" is an internal word and must never be spoken.** The catalogue calls
them buffs; the player buys a shower, an energy drink, an Iron Skillet dinner.
Every spoken string goes through `Buff.label`, which is what makes that work.
Game-shop jargon in a trucking sim breaks the fiction and, more practically,
tells a screen reader user nothing about what they just bought.

**The hours clocks are three separate nouns, and they never share a first
word.** "At the wheel" is time already driven this shift, "driving time left"
is what remains of the 11-hour clock, and the "duty window" is the 14-hour one
that can close first. The dedicated keys lead with those nouns so a driver
knows which key landed before any number arrives; "hours left", "hours driven",
and "hours until a break" all open on "hours" and are exactly what not to say.
Hours at the wheel are not "hours on the road" either: the run's clock is a
different number, and a shift can span several runs.

**"Traffic" needs a qualifier.** Simulated vehicles around the truck and
reported incidents on the road ahead are different systems the player can act
on differently. "Traffic is heavy" and "there is a wreck reported ahead" are
not interchangeable.

### "Bear" is CB voice only

"Bear" may appear in exactly one place: inside a clause the line attributes to
the CB, spoken by a driver on the radio. It is trade slang, and it is flavour.
In a warning, a menu item, a status readout, or anything the game says in its
own voice, the word is "trooper".

A sweep of every string literal in both crates' sources fails if the word
appears outside a CB clause
(`test_bear_is_cb_voice_only_in_every_player_facing_string` in
`crates/ff-core/tests/it/sim_enforcement_vocabulary.rs`); the song title
"Black Bear Road" is its one exception. The check exists because slang leaks: the word is
evocative, it reads well in a sentence, and one careless line teaches a screen
reader user a second noun for a thing that already had one.

### Driving speech rungs

How much of the road's *information* speaks. Three rungs, cutting whole
categories rather than shortening sentences; the player picks one and the
delivery layer decides per category. "Terse" survives only as the internal
name of the shorter rendering and is no longer a thing the player selects.

| Concept | Canonical spoken noun | Never say | Where |
| --- | --- | --- | --- |
| The working default | standard | normal, default | `DRIVING_SPEECH_MODES` |
| Short confirmations, lane openings, and status updates, without the traffic around the truck | quiet | terse (that is the rendering, not the rung), minimal | `DRIVING_SPEECH_MODES` |
| Safety warnings and directions requiring action | urgent only | emergency mode, critical only | `DRIVING_SPEECH_MODES` |

Roadside colour -- billboards, place names, landmarks -- is **not** governed
by these rungs. It answers to the chatter switches and the place-callouts
ladder, and a player may run the loudest colour with the quietest rung.

A line a rung leaves out is silent: no stand-in sound, and no tone of its
own (owner, 2026-10-03). The four ladder notes that used to stand in for
cut lines are retired. The road's own sounds -- traffic going by, the
engine, a siren -- are not announcements and always play.

### Terse speech grammar

Terse mode promises: the truck tells you what to *do* and what it *cost*, and
nothing else -- every safety call, route instruction, and money consequence
still speaks, in the shortest form this ontology allows, and everything that
is color, confirmation, coaching, or congratulation is an earcon or silence.
Two rules bound every terse rendering, and the pairs themselves live in
`speech_text.rs` (one definition, both forms side by side, pinned by the
tests in that module):

**Compress words, never certainty.** A qualifier that changes a decision
survives terse. Parking certainty is the worked example: all five values stay
distinguishable -- "Parking confirmed." / silence (meaning *likely*, exactly
as in normal mode) / "Parking limited." / "No truck parking." / "Parking not
verified." (the spoken form of *unknown*; never "unverified", which would be
a synonym).

**A fixed slot order.** A bare trailing number is only parseable because the
frame never shuffles, so no terse line may reorder its slots:

| Frame | Slots | Example |
| --- | --- | --- |
| Hazard-family cue (brake lights, slow traffic) | thing, distance, target speed | "Brake lights, 2.1 miles, 38." |
| Merging traffic cue | thing, distance | "Merging box truck, 0.4 miles." |
| Stop callout | name, exit, distance, qualifier | "Flying J Travel Center Corfu, exit 48A, 5 miles. Parking confirmed." |
| Money consequence | what, amount, who pays | "Toll, 15 dollars, carrier." |
| Speed limit | "Limit", the number | "Limit 65." |

A merging vehicle carries no target speed: it merges behind or passes on
its own, and naming a speed to "be ready for" read as an instruction to
slow down that the situation never asked for. The same rule holds for the
route-merge and construction-taper traffic-pressure advisories
(`Trip._traffic_pressure_message`, GPS_CUE): "Merging traffic in
«distance». Keep «direction»." and "Traffic squeezing at the construction
taper in «distance». Merge «direction» early." carry no speed either. The taper's actual
posted limit is a real sign, not a merge advisory, and is still spoken by
the separate zone warning/entry lines. Exit traffic keeps its speed --
the truck itself is slowing for the ramp, not reacting to someone else's
merge.

**The dodgeable hazard call is "Change lanes or brake!", and it ends by
naming the open lane.** The same phrase the help teaches, followed by the
lane answer in the L key's own words: "Left lane open.", "Right lane open.",
or "Either lane open." (owner, 2026-09-01, on I-90 with adaptive cruise on:
the bare call left a blind driver guessing whether a lane change was even
possible, and the game already knew). The side is read the way a lane change
is refused -- the lane exists, it is not coned off, and nobody is holding it
over the window the change takes -- so the call never names a lane a tap
would refuse. Where nothing is open, on a one-lane road or with both
neighbours held, the call is "Brake!" and ends "No lane open.", and the
hazard is not dodgeable at all. Terse keeps the thing and the lane answer
and drops the opener ("Slow car right ahead. Left lane open."): the named
lane says everything the opener did. "Brake or swerve!" was a terse-only
synonym for the game's most safety-critical cue and is exactly what this
table exists to prevent. The lane change leads the braking (owner,
2026-08-17): both actions stay on offer, because a driver who cannot see the
gap may reasonably prefer to slow, but at a hazard the first word is the one
that gets acted on. On partial or off, lane changes stay driver-initiated:
one tap of the arrow the call named, with adaptive cruise riding through the
dodge. On full, a slow VEHICLE ahead is passed by the truck itself (owner,
2026-10-01): the call drops the opener and the lane answer and says what the
truck is doing, "Slow car right ahead. Passing on the left.", the arrival is
"In the left lane, passing the slow car.", and the move back is reported by
"In the right lane." alone. "Passing" is the canonical word for it -- never
overtaking or going around. Objects in the lane keep the driver's call.

## Open naming decisions

Recorded rather than silently resolved, because changing any of them changes
what players hear.

- **run against trip.** Delivery summaries say "this run"; the class is `Trip`;
  help text uses both. The table above picks "run" for spoken text and leaves
  `Trip` as the internal name, which is the current majority behaviour rather
  than a decision anyone made.
- **job against load in older strings.** "job" leads roughly four to one across
  `states/`, but some surviving uses of "load" mean the contract, not the
  freight. Those are the ones worth fixing, and only where a player would
  actually hear the ambiguity.
- **`TruckCondition` lives in `models/trucks.py`**, a portable-layer module,
  despite being game-owned save state. It should probably move next to
  `Profile`. Cheap to do, and it removes a boundary exception.
- **One spoken "buff" leak exists.** `states/driving_updates.py` falls back to
  the literal word when a worn-off record has no label:
  `worn.get("label", "buff")`. Every catalogued buff has a label, so it should
  not fire, but if it ever does the player hears "The buff has worn off." A
  better fallback is the group name -- "the coffee", "the tire service" -- or
  saying nothing at all.

## Working rule

- Adding spoken text for an existing concept: use the word in the table.
- Adding a concept: add a row, in the same change.
- Renaming a concept players hear: that is a player-facing change. It needs a
  changelog entry and a note on what they will hear differently.
