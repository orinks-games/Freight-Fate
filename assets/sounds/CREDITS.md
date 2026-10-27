# Sound and Music Credits

Sound effects in this directory come from a few sources, documented per file in
the tables below: original effects created for Freight Fate (no third-party
recordings or samples), weather and road loops produced with ElevenLabs sound
effects tooling, and driving sounds cut from
commercially licensed sample libraries (see "Licensed Driving Sounds" below).
A number of in-cab cues were shared by Darren Duff early in the project; he
holds no license for that material (owner ruling 2026-07-22), so every Duff
row below is scheduled for replacement, not attribution -- most are already
superseded by the Career 1.9 sound pack. The main menu, Open
Road, and Night Haul themes are Suno remakes created by the project owner. The July 2026 radio-station batch
(fifty-two tracks marked "2026-07 Zero batch" below) was composed with Suno V5
through the
Zero capability marketplace, with original lyrics written for Freight Fate;
those tracks are subject to the applicable Suno terms for the generating
account.

The default sound_lib weather loops were generated for Freight Fate with
ElevenLabs sound effects tooling, then reviewed and selected for the in-cab
weather mix.

The original project sound effects, and the ElevenLabs-generated assets
(original project assets subject to the applicable ElevenLabs terms for the
generating account), are part of Freight Fate and covered by the project's
PolyForm Noncommercial License (see `LICENSE` at the repository root): free to
use and share for noncommercial purposes, all other rights reserved by the
project owner. They are not dedicated to the public domain.

The in-cab truck sounds used for selected vehicle cues are authorized sounds
that [Darren Duff](https://darrenduff.com/) found, liked, and sent along for the
project. Darren also shared the louder/doubled-volume versions used for the
current idle, start, and shutdown cues.

## Vehicle Sounds

| Sound | File | Description |
| --- | --- | --- |
| Truck idle, interior | `engine_classic/idle.ogg` | In-cab idle loop, shared by Darren Duff; ships in-repo as the classic engine sound (was `engine/idle.ogg` until the rebuilt engine landed 2026-07-27) |
| Truck start, interior | `engine/start.ogg` | In-cab engine-start cue, shared by Darren Duff |
| Truck shutdown, interior | `engine/shutdown.ogg` | In-cab shutdown cue, shared by Darren Duff |
| Truck horn, interior | `vehicle/horn.ogg` | In-cab horn cue (trimmed from Take 2), shared by Darren Duff |
| Gear shift, interior | `vehicle/gear_shift.ogg` | In-cab gear-shift cue (split from a two-shift source), shared by Darren Duff |
| Parking brake release, interior | `vehicle/brake_release.ogg` | In-cab air-brake cue, shared by Darren Duff |
| Parking brake set, interior | `vehicle/brake_set.ogg` | In-cab brake-set cue, shared by Darren Duff |
| Driver yawn | `driver/yawn.ogg` | Drowsiness yawn cue, shared by Darren Duff |
| Air dryer purge | `vehicle/air_dryer_purge.ogg` | ElevenLabs-generated compressor cut-out purge, played when air pressure is ready |
| Low air buzzer | `vehicle/low_air_buzzer.ogg` | ElevenLabs-generated low-air-pressure / spring-brake warning buzzer |
| Highway road bed | `vehicle/road.ogg` | ElevenLabs-generated in-cab road ambience; raised 6 dB on 2026-10-08, when it sat about 15 dB under the engine |
| Road seam thump | `vehicle/road_joint.ogg` | Short low thump synthesized in-repo for routine road texture |
| Car pass-by | `traffic/car_pass.ogg` | ElevenLabs-generated nearby passenger-car pass-by cue |
| Box truck pass-by | `traffic/box_truck_pass.ogg` | ElevenLabs-generated nearby medium-truck pass-by cue |
| Semi pass-by | `traffic/semi_pass.ogg` | ElevenLabs-generated nearby tractor-trailer pass-by cue |
| State trooper pass-by | `traffic/trooper_pass.ogg` | ElevenLabs-generated patrol-car pass-by cue without siren |
| Pickup, motorcycle, bus and tractor pass-bys | `traffic/pickup_pass.ogg`, `traffic/motorcycle_pass.ogg`, `traffic/bus_pass.ogg`, `traffic/tractor_pass.ogg` | ElevenLabs Sound Effects API pass-by cues, 2026-09-11; replaced the deterministic numpy stand-ins (tools/generate_sounds.py, still the no-key fallback) that shipped under the same keys from 2026-08-20 |
| Cross traffic at ramp ends (car, pickup, box truck, semi, motorcycle, bus, tractor) | `traffic/car_cross.ogg`, `traffic/pickup_cross.ogg`, `traffic/box_truck_cross.ogg`, `traffic/semi_cross.ogg`, `traffic/motorcycle_cross.ogg`, `traffic/bus_cross.ogg`, `traffic/tractor_cross.ogg` | ElevenLabs Sound Effects API crossing cues (a vehicle driving through in front of the stopped truck, panned by the game), 2026-09-11; replaced the numpy stand-ins that shipped from 2026-08-20 |
| Traffic sounds (car, pickup, box truck, semi, motorcycle, bus, tractor, patrol car) | `traffic/car_loop.ogg`, `traffic/pickup_loop.ogg`, `traffic/box_truck_loop.ogg`, `traffic/semi_loop.ogg`, `traffic/motorcycle_loop.ogg`, `traffic/bus_loop.ogg`, `traffic/tractor_loop.ogg`, `traffic/trooper_loop.ogg` | Original synthesized steady loops of a vehicle holding its distance at road speed (tires, wind and a modelled engine), rendered with the genny CLI from `sound-test/traffic_sounds.json` (2026-10-08), then lifted 12 dB around 1.2 kHz and cut 3 dB around 300 Hz at the same loudness (2026-10-09) so the tires stand out from the cab's engine; the game sets each one's level, pan and pitch from a real NPC vehicle every frame |
| Freeway traffic bed | `traffic/highway_bed.ogg` | Original synthesized distant-traffic roar, steady shaped-noise bands and soft far-off passes, from `sound-test/highway_bed.py` (2026-10-09); replaced a genny render that swelled like surf |
| Lane drift | `vehicle/lane_drift.ogg` | ElevenLabs-generated directional lane drift beep |
| Lane centered | `vehicle/lane_centered.ogg` | ElevenLabs-generated centered-lane confirmation chime |
| Turn signal | `vehicle/turn_signal.ogg` | Original synthesized flasher cycle, a relay tick then a softer tock, rendered with the genny CLI from `sound-test/turn_signal.json` (2026-09-29); replaced an ElevenLabs-generated click recording |
| Blinker off | `vehicle/turn_signal_off.ogg` | Original synthesized turn signal stalk click, rendered with the genny CLI from `sound-test/turn_signal.json` (2026-09-29) |
| Tire screech | `vehicle/tire_screech.ogg` | ElevenLabs-generated emergency-braking skid for microsleep forced stops |
| Brake squeal | `vehicle/brake_squeal.ogg` | ElevenLabs-generated overheated-brake squeal past the fade temperature |
| Air pressurization fill | `vehicle/air_pressurize.ogg` | Original parametric DSP loop (seeded frequency-domain synthesis, `sound-test/air_fallback.py`); superseded by the Career 1.9 sound-pack version |
| Jake brake growl | `engine/jake_1200.ogg` ... `engine/jake_2200.ogg` | Original synthesized engine-brake loops at six rpm points (deterministic pulse-train DSP, `sound-test/jake_v2.py` family; approved by ear 2026-07-18) |
| Edge-boundary ladder | `vehicle/edge_clip.wav`, `vehicle/edge_strip.wav`, `vehicle/edge_shoulder.wav` | Original synthesized boundary loops (deterministic pulse-train DSP from the `sound-test/edge_nav.py` audition machinery, baked by `sound-test/edge_ladder.py`); intermittent / periodic / aperiodic structural states |
| Turn-signal tone | `vehicle/signal_tone.wav` | Original synthesized indicator tick-tone (`sound-test/signal_tone.py`), the designed modern-cab indicator sound; supersedes the relay click for signaling (the click remains for a future vintage-equipment option) |
| Dead-man's-curve strips | `vehicle/transverse_strips.wav` | Original synthesized transverse rumble-bar crossing (`sound-test/transverse_strips.py`): three grouped bar sets under all axles, the real DOT hairpin warning |
| Lane locator tock | `vehicle/lane_locator.wav` | Original synthesized soft position tock (`sound-test/transverse_strips.py`), panned by the game to the truck's place in its lane |
| Curve cue bink | `vehicle/curve_bink.wav` | Original synthesized bright cue tone (`sound-test/transverse_strips.py`); carries the pacenote call, the curve entry, and the stop-bar closing tick -- the beep the old ui/tick placeholder note asked for |
| Lane-line marker roll | `vehicle/lane_line_cross.wav` | Original synthesized raised-marker crossing (`sound-test/transverse_strips.py`): the five-hit axle roll of a tractor-trailer crossing a lane line, panned to the crossed side |
| Stop-bar solid tone | `vehicle/bar_solid.wav` | Original synthesized continuous alert (`sound-test/transverse_strips.py`): the parking-sensor beeps going solid in the bar's final leeway, a calm 400 Hz you can hold for seconds at a time (pitched down from 1250 by ear, 2026-08-03) |
| Liquid surge, tank trailer | `vehicle/liquid_wash.wav`, `vehicle/liquid_hit.wav`, `vehicle/liquid_hit_lateral.wav` | Original synthesized tank-surge layer (deterministic seeded spectral-domain DSP, `sound-test/liquid_surge.py`): a wash bed held only while the liquid is running, plus the arrival slap through the shell's modal ring, fore-aft and lateral distinguished by ring. Banded 1.5-4 kHz on purpose -- the engine and road beds own everything below a kilohertz and the jake runs at full gain during exactly the deceleration that produces surge |

## Licensed Driving Sounds

The encrypted Career 1.9 sound pack includes commercially licensed sounds
obtained through Splice. The original source recordings remain local and are
not distributed in this repository.

## Weather

| Sound | File | Description |
| --- | --- | --- |
| Light rain | `weather/rain_light.ogg` | Isolated in-cab light rain layer |
| Heavy rain | `weather/rain_heavy.ogg` | Isolated in-cab heavy rain layer |
| Snow and wind | `weather/snow_wind.ogg` | Dry snow against the truck cab |
| Fog horn | `weather/fog_horn.ogg` | Distant fog-horn ambience without engine bed |
| Wind | `weather/wind.ogg` | Isolated wind around the truck cab shell |
| Thunder | `weather/thunder.ogg` | Filtered in-cab thunder one-shot |

## Route Events And POIs

| Sound | File | Description |
| --- | --- | --- |
| Hazard warning | `events/hazard_warning.ogg` | Short in-drive hazard cue |
| Construction zone | `events/construction_zone.ogg` | Short construction-zone cue |
| Traffic slowing | `events/traffic_slowing.ogg` | Short traffic-slowing cue |
| Toll charged | `events/toll_charged.ogg` | ElevenLabs-generated low, soft dashboard bong |
| State crossing | `events/state_crossing.ogg` | Short route milestone cue |
| Inspection warning | `events/inspection_warning.ogg` | Short inspection/weigh-station cue |
| Scale warning | `events/weigh_station_warning.ogg` | Procedural (numpy) two-part low-thump-then-beep cue for the open-scale approach notice, generated with `tools/generate_sounds.py --weigh-station-warning`, no ElevenLabs credits |
| Local turn left | `events/turn_left.ogg` | ElevenLabs-generated falling two-note turn chime, panned left at playback |
| Local turn right | `events/turn_right.ogg` | Rising mirror of the left-turn chime (note order swapped via `tools/mirror_turn_chime.py`), panned right at playback |
| Local turn ahead | `events/turn_ahead.ogg` | ElevenLabs-generated single-tone straight-ahead cue, loudness-normalized |
| Ramp light red | `events/ramp_light_red.ogg` | ElevenLabs-generated low two-tone stop cue for a red ramp-terminal light |
| Ramp light green | `events/ramp_light_green.ogg` | ElevenLabs-generated go cue for a green ramp-terminal light, loudness-normalized |
| Police siren | `events/police_siren.ogg` | ElevenLabs-generated trooper pull-over siren wail |
| CB radio chatter | `events/cb_radio_chatter.ogg` | Original synthesized CB squelch tail, the burst a receiver makes when the other driver lets go of the mic, rendered with the genny CLI from `sound-test/cb_squelch.json` (2026-10-01); replaced an ElevenLabs-generated squelch-and-chatter recording |
| Spike strip | `events/spike_strip.ogg` | ElevenLabs-generated spike-strip puncture/air-hiss for felony stops |
| Hazard clear | `events/hazard_clear.ogg` | ElevenLabs-generated confirmation cue when a hazard has been safely passed |
| Rest stop at night | `poi/rest_stop_night.ogg` | Parked rest-stop ambience loop |
| Weigh station lane | `poi/weigh_station_lane.ogg` | Parked weigh-station lane ambience loop |
| Facility gate | `poi/facility_gate.ogg` | ElevenLabs-generated loading-dock gate ambience loop |
| Dock and deliver | `poi/dock_and_deliver.ogg` | Destination docking action cue |
| Main menu, night | `ambient/menu_night_lot.ogg` | ElevenLabs-generated truck-stop-at-night ambience loop |
| Main menu, owner-operator by day | `ambient/menu_yard.ogg` | ElevenLabs-generated loop of one rig idling in a truck yard |
| Main menu, company driver by day | `ambient/menu_dispatch_office.ogg` | ElevenLabs-generated trucking dispatch office ambience loop |
| Truck stop by day | `ambient/truck_stop.ogg` | ElevenLabs-generated daytime truck-stop lot ambience loop |
| Warehouse interior | `ambient/warehouse.ogg` | ElevenLabs-generated warehouse dock interior ambience loop |
| Night driving | `ambient/night.ogg` | In-cab night ambience loop, shared by Darren Duff |

## Music

| Track | File | Description |
| --- | --- | --- |
| Headlights West | `music/menu_theme.ogg` | Suno remake for the main menu |
| Keys To The Rig | `music/menu_first_rig.ogg` | First-owned-truck career menu bed |
| Regional Lines | `music/menu_regional_carrier.ogg` | Regional carrier career menu bed |
| Yard Lights | `music/menu_fleet_owner.ogg` | Fleet-owner career menu bed |
| Coast To Coast Ledger | `music/menu_coast_to_coast.ogg` | Coast-to-coast career menu bed |
| Million Mile Morning | `music/menu_legendary_haul.ogg` | Late-career menu bed |
| Midnight Keys | `music/menu_theme_night.ogg` | Night piano ballad menu bed (career loaded after dark) |
| Open Road | `music/open_road.ogg` | Suno remake for daytime driving |
| Desert Two-Lane | `music/drive_desert_two_lane.ogg` | Spacious daytime desert drive bed |
| Mountain Grade | `music/drive_mountain_grade.ogg` | Measured daytime mountain drive bed |
| Rain-Day Cruise | `music/drive_rain_day_cruise.ogg` | Gentle rainy daytime drive bed |
| Urban Roll | `music/drive_urban_roll.ogg` | Light heavy-traffic daytime drive bed |
| Dawn Push | `music/drive_dawn_push.ogg` | Soft early-morning daytime drive bed |
| Night Haul | `music/night_haul.ogg` | Suno remake for night driving |
| Midnight Interstate | `music/night_midnight_interstate.ogg` | Quiet nighttime highway bed |
| Neon Truck Stop | `music/night_neon_truck_stop.ogg` | Soft night truck-stop approach bed |
| Rainy Night Miles | `music/night_rainy_miles.ogg` | Sparse rainy night drive bed |
| Lonely Plains | `music/night_lonely_plains.ogg` | Open nighttime plains drive bed |
| Mountain Night Pass | `music/night_mountain_pass.ogg` | Quiet mountain night drive bed |
| Small Hours | `music/night_small_hours.ogg` | Late-night piano ballad drive bed |
| Headlights West, from Freight Fate 1.5 | `music/classic_menu_theme.ogg` | Procedurally generated by the game's own `tools/generate_audio.py` (removed in e751db73); restored byte for byte from v1.5.0 |
| Open Road, from Freight Fate 1.5 | `music/classic_open_road.ogg` | Procedurally generated by the game's own `tools/generate_audio.py` (removed in e751db73); restored byte for byte from v1.5.0 |
| Night Haul, from Freight Fate 1.5 | `music/classic_night_haul.ogg` | Procedurally generated by the game's own `tools/generate_audio.py` (removed in e751db73); restored byte for byte from v1.5.0 |
| Backroads Sunrise | `music/radio_country_backroads.ogg` | ElevenLabs-composed country song for regional radio |
| Two-Lane Towns | `music/radio_country_two_lane.ogg` | ElevenLabs-composed classic country song for regional radio |
| Diesel Heart | `music/radio_country_diesel_heart.ogg` | ElevenLabs-composed country rock song for regional radio |
| Open Throttle | `music/radio_rock_open_throttle.ogg` | ElevenLabs-composed highway rock anthem for regional radio |
| Night Shift | `music/radio_rock_night_shift.ogg` | ElevenLabs-composed organ-driven rock song for regional radio |
| Chrome Horizon | `music/radio_rock_chrome_horizon.ogg` | ElevenLabs-composed heartland rock song for regional radio |
| Delta Mile | `music/radio_blues_delta_mile.ogg` | ElevenLabs-composed delta blues song for regional radio |
| Crossroad Coffee | `music/radio_blues_crossroad_coffee.ogg` | ElevenLabs-composed soul blues song for regional radio |
| Low Beams | `music/radio_night_low_beams.ogg` | ElevenLabs-composed late-night jazz instrumental for the Night Line and the night menu rotation |
| High Plains Wind | `music/drive_high_plains_wind.ogg` | Suno-composed warm high-plains Americana bed for the in-game radio (2026-07 Zero batch) |
| Open Sky Run | `music/drive_open_sky_run.ogg` | Suno-composed breezy open-sky Americana bed for the in-game radio (2026-07 Zero batch) |
| Golden Hour Freeway | `music/drive_golden_hour_freeway.ogg` | Suno-composed golden-hour heartland drive bed for the in-game radio (2026-07 Zero batch) |
| Amber Lanes | `music/drive_amber_lanes.ogg` | Suno-composed warm sunset freeway drive bed for the in-game radio (2026-07 Zero batch) |
| River Valley Roll | `music/drive_river_valley_roll.ogg` | Suno-composed rolling folk-rock valley bed for the in-game radio (2026-07 Zero batch) |
| Green Mile Bend | `music/drive_green_mile_bend.ogg` | Suno-composed easy fingerpicked river-road bed for the in-game radio (2026-07 Zero batch) |
| County Line Cruise | `music/drive_county_line_cruise.ogg` | Suno-composed laid-back twangy cruising bed for the in-game radio (2026-07 Zero batch) |
| Two-Lane Daydream | `music/drive_two_lane_daydream.ogg` | Suno-composed relaxed two-lane country-rock bed for the in-game radio (2026-07 Zero batch) |
| Chrome Creek | `music/drive_chrome_creek.ogg` | Suno-composed breezy slide-guitar roots bed for the in-game radio (2026-07 Zero batch) |
| Silver Current | `music/drive_silver_current.ogg` | Suno-composed sparkling slide-guitar morning bed for the in-game radio (2026-07 Zero batch) |
| Quiet Mile | `music/night_quiet_mile.ogg` | Suno-composed calm electric-piano night bed for the in-game radio (2026-07 Zero batch) |
| Soft Shoulder | `music/night_soft_shoulder.ogg` | Suno-composed soft ambient night-highway bed for the in-game radio (2026-07 Zero batch) |
| Starlight Grade | `music/night_starlight_grade.ogg` | Suno-composed gentle piano mountain-night bed for the in-game radio (2026-07 Zero batch) |
| High Beam Hush | `music/night_high_beam_hush.ogg` | Suno-composed hushed strings-and-piano night bed for the in-game radio (2026-07 Zero batch) |
| Last Diner Open | `music/radio_night_last_diner.ogg` | Suno-composed quiet late-night diner ballad for the in-game radio (2026-07 Zero batch) |
| Third Shift Waltz | `music/radio_night_third_shift_waltz.ogg` | Suno-composed gentle waltz for night workers for the in-game radio (2026-07 Zero batch) |
| More Nashville Jazzicals | `music/radio_jazz_nashville_jazzicals.opus` | Suno-composed smoky Nashville crossover jazz instrumental for the in-game radio (2026-08 originals batch) |
| When Some Nashville Cats Got Jazzy | `music/radio_jazz_nashville_cats.opus` | Suno-composed playful swing-country jazz instrumental for the in-game radio (2026-08 originals batch) |
| That Nashville Sound in the 80's | `music/radio_jazz_eighties_sound.opus` | Suno-composed smooth eighties countrypolitan instrumental for the in-game radio (2026-08 originals batch) |
| That Nashville Sound in the 90's | `music/radio_jazz_nineties_sound.opus` | Suno-composed polished nineties Nashville-pop instrumental for the in-game radio (2026-08 originals batch) |
| Happens in Nashville Tonight | `music/radio_jazz_nashville_tonight.opus` | Suno-composed after-hours downtown jazz instrumental for the in-game radio (2026-08 originals batch) |
| A Caring Touch | `music/radio_jazz_caring_touch.opus` | Suno-composed gentle late-evening ballad instrumental for the in-game radio (2026-08 originals batch) |
| Penny for Your Thoughts | `music/radio_jazz_penny_thoughts.opus` | Suno-composed thoughtful brushed-drums jazz instrumental for the in-game radio (2026-08 originals batch) |
| Texico Station Fill Up | `music/radio_country_texico_fill_up.opus` | Suno-composed bouncy truck-stop country instrumental for the in-game radio (2026-08 originals batch) |
| Crucial Load Needed in Arkansas | `music/radio_country_arkansas_load.opus` | Suno-composed urgent dispatch-run country instrumental for the in-game radio (2026-08 originals batch) |
| Kentucky Rain Called Me Home | `music/radio_country_kentucky_rain.opus` | Suno-composed rain-soaked homecoming country instrumental for the in-game radio (2026-08 originals batch) |
| Texian Style | `music/radio_country_texian_style.opus` | Suno-composed swaggering Texas country instrumental for the in-game radio (2026-08 originals batch) |
| Texas Country on a Tuesday Evening | `music/radio_country_tuesday_texas.opus` | Suno-composed easygoing weeknight Texas country instrumental for the in-game radio (2026-08 originals batch) |
| Thursday Night in Fort Worth | `music/radio_country_fort_worth_thursday.opus` | Suno-composed honky-tonk Thursday-night country instrumental for the in-game radio (2026-08 originals batch) |
| Texas Wants You Back, and So Do I | `music/radio_country_texas_wants_you.opus` | Suno-composed yearning Texas ballad instrumental for the in-game radio (2026-08 originals batch) |
| Alabama Called | `music/radio_country_alabama_called.opus` | Suno-composed southbound homesick country instrumental for the in-game radio (2026-08 originals batch) |
| Carolina Groovin | `music/radio_country_carolina_groovin.opus` | Suno-composed funky Carolina country-groove instrumental for the in-game radio (2026-08 originals batch) |
| Over Yonder | `music/radio_country_over_yonder.opus` | Suno-composed front-porch country stroll instrumental for the in-game radio (2026-08 originals batch) |
| Canoe Trail | `music/drive_canoe_trail.opus` | Suno-composed easy pastoral fingerpicked bed for the daytime driving rotation (2026-08 originals batch) |
| On the Gunflint | `music/drive_gunflint.opus` | Suno-composed wide northwoods Americana bed for the daytime driving rotation (2026-08 originals batch) |
| A Little Boat Trip I Took Once | `music/drive_little_boat_trip.opus` | Suno-composed lazy riverside country bed for the daytime driving rotation (2026-08 originals batch) |
| Dancing Firelight | `music/drive_dancing_firelight.opus` | Suno-composed warm campfire country-folk bed for the daytime driving rotation (2026-08 originals batch) |
| Always Around When You Need Me | `music/drive_always_around.opus` | Suno-composed steady companionable country bed for the daytime driving rotation (2026-08 originals batch) |
| Under the Starlight | `music/night_under_starlight.opus` | Suno-composed open-sky nighttime country bed for the night driving rotation (2026-08 originals batch) |
| Gettin Ever So Slightly Darker Tonight | `music/night_slightly_darker.opus` | Suno-composed moody dusk-into-night country bed for the night driving rotation (2026-08 originals batch) |
| Why the Stars Said I Love You That Night | `music/night_stars_said_love.opus` | Suno-composed tender starlit country waltz bed for the night driving rotation (2026-08 originals batch) |
| Her Real Words to Me That Night | `music/night_her_real_words.opus` | Suno-composed slow confessional night ballad bed for the night driving rotation (2026-08 originals batch) |
| When You Were on My Mind | `music/night_on_my_mind.opus` | Suno-composed wistful late-night country bed for the night driving rotation (2026-08 originals batch) |
| Call Me When You Get This | `music/night_call_me.opus` | Suno-composed quiet waiting-by-the-phone night bed for the night driving rotation (2026-08 originals batch) |
| Maroon Coloured Scarf | `music/night_maroon_scarf.opus` | Suno-composed soft keepsake night ballad bed for the night driving rotation (2026-08 originals batch) |
| When We Took That Train I Knew It Was It | `music/night_train_knew.opus` | Suno-composed rolling nighttime train-memory bed for the night driving rotation (2026-08 originals batch) |
| Progress for Progress's Sake | `music/menu_progress.opus` | Suno-composed seasoned late-career country bed for the menu milestone rotation (2026-08 originals batch) |
| County Fair | `music/radio_country_county_fair.ogg` | Suno-composed upbeat county-fair country song for the in-game radio (2026-07 Zero batch) |
| Porch Light | `music/radio_country_porch_light.ogg` | Suno-composed warm homecoming country song for the in-game radio (2026-07 Zero batch) |
| Wildflower Mile | `music/radio_country_wildflower_mile.ogg` | Suno-composed hopeful springtime country song for the in-game radio (2026-07 Zero batch) |
| Dust and Daylight | `music/radio_country_dust_and_daylight.ogg` | Suno-composed gritty outlaw country song for the in-game radio (2026-07 Zero batch) |
| Blue Ridge Morning | `music/radio_country_blue_ridge_morning.ogg` | Suno-composed upbeat bluegrass instrumental for the in-game radio (2026-07 Zero batch) |
| Appalachian Sunrise | `music/radio_country_appalachian_sunrise.ogg` | Suno-composed bright mountain bluegrass instrumental for the in-game radio (2026-07 Zero batch) |
| Steel String Sunday | `music/radio_country_steel_string_sunday.ogg` | Suno-composed lazy pedal-steel instrumental for the in-game radio and the day menu rotation (2026-07 Zero batch) |
| Dobro Dusk | `music/radio_country_dobro_dusk.ogg` | Suno-composed mellow dobro country instrumental for the in-game radio and the day menu rotation (2026-07 Zero batch) |
| Mile Marker Moon | `music/radio_country_mile_marker_moon.ogg` | Suno-composed moonlit homesick country waltz for the in-game radio (2026-07 Zero batch) |
| Paper Town | `music/radio_country_paper_town.ogg` | Suno-composed wistful small-town country song for the in-game radio (2026-07 Zero batch) |
| Tailgate Summer | `music/radio_country_tailgate_summer.ogg` | Suno-composed rowdy lakeside party country song for the in-game radio (2026-07 Zero batch) |
| Grandpa's Radio | `music/radio_country_grandpas_radio.ogg` | Suno-composed tender heirloom-radio country ballad for the in-game radio (2026-07 Zero batch) |
| Dust on the Highway | `music/radio_country_dust_on_the_highway.opus` | Suno-composed driving outlaw country-rock instrumental for the in-game radio (2026-08) |
| Thunder County | `music/radio_rock_thunder_county.ogg` | Suno-composed storm-charged seventies rock anthem for the in-game radio (2026-07 Zero batch) |
| Midnight Arcade | `music/radio_rock_midnight_arcade.ogg` | Suno-composed neon eighties arena rock song for the in-game radio (2026-07 Zero batch) |
| Neon Avenue | `music/radio_rock_neon_avenue.ogg` | Suno-composed late-night organ-driven rock groove for the in-game radio (2026-07 Zero batch) |
| Ember Sky | `music/radio_rock_ember_sky.ogg` | Suno-composed hopeful heartland rock song for the in-game radio (2026-07 Zero batch) |
| Glass Highway | `music/radio_rock_glass_highway.ogg` | Suno-composed melodic highway rock instrumental for the in-game radio and the day menu rotation (2026-07 Zero batch) |
| Mercury Miles | `music/radio_rock_mercury_miles.ogg` | Suno-composed soaring lead-guitar rock instrumental for the in-game radio (2026-07 Zero batch) |
| Switchback | `music/radio_rock_switchback.ogg` | Suno-composed funky seventies rock instrumental for the in-game radio (2026-07 Zero batch) |
| Hairpin | `music/radio_rock_hairpin.ogg` | Suno-composed wah-driven mountain rock instrumental for the in-game radio (2026-07 Zero batch) |
| Wildfire Line | `music/radio_rock_wildfire_line.ogg` | Suno-composed driving fire-crew hard rock anthem for the in-game radio (2026-07 Zero batch) |
| Silver Falcon | `music/radio_rock_silver_falcon.ogg` | Suno-composed female-fronted muscle-car rocker for the in-game radio (2026-07 Zero batch) |
| Last Ferry Home | `music/radio_rock_last_ferry_home.ogg` | Suno-composed warm harbor-dusk rock song for the in-game radio (2026-07 Zero batch) |
| Static and Stars | `music/radio_rock_static_and_stars.ogg` | Suno-composed wide-open night-sky heartland rock for the in-game radio (2026-07 Zero batch) |
| Greywater Quay | `music/radio_rock_greywater_quay.ogg` | Suno-composed folk-rock tribute to Saltwake's Greywater Quay for the in-game radio (2026-07 Zero batch) |
| Inland Sea | `music/radio_rock_inland_sea.ogg` | Suno-composed heartland rock song about the Great Salt Lake for the in-game radio (2026-07 Zero batch) |
| Lights Over Superior | `music/radio_rock_lights_over_superior.ogg` | Suno-composed northern-lights rock instrumental for the in-game radio and the day menu rotation (2026-09 batch) |
| Raincheck | `music/radio_blues_raincheck.ogg` | Suno-composed slow rained-out electric blues for the in-game radio (2026-07 Zero batch) |
| Magnolia Porch | `music/radio_blues_magnolia_porch.ogg` | Suno-composed warm porch-evening southern soul for the in-game radio (2026-07 Zero batch) |
| Neon and Bourbon | `music/radio_blues_neon_bourbon.ogg` | Suno-composed smoky Chicago bar-band blues for the in-game radio (2026-07 Zero batch) |
| Freight Yard Moon | `music/radio_blues_freight_yard_moon.ogg` | Suno-composed midnight rail-yard blues instrumental for the in-game radio and the night menu rotation (2026-07 Zero batch) |
| Midnight Siding | `music/radio_blues_midnight_siding.ogg` | Suno-composed slow-burning night blues instrumental for the in-game radio and the night menu rotation (2026-07 Zero batch) |
| Slow Train Shuffle | `music/radio_blues_slow_train_shuffle.ogg` | Suno-composed rolling harmonica blues instrumental for the in-game radio (2026-07 Zero batch) |
| Boxcar Stroll | `music/radio_blues_boxcar_stroll.ogg` | Suno-composed easy boxcar harmonica instrumental for the in-game radio (2026-07 Zero batch) |
| Grits and Gasoline | `music/radio_blues_grits_and_gasoline.ogg` | Suno-composed greasy roadside blues rocker for the in-game radio (2026-07 Zero batch) |
| Paycheck Friday | `music/radio_blues_paycheck_friday.ogg` | Suno-composed swinging horn-section jump blues for the in-game radio (2026-07 Zero batch) |
| Levee Moon | `music/radio_blues_levee_moon.ogg` | Suno-composed smoky riverside delta soul for the in-game radio (2026-07 Zero batch) |
| Jukebox in the Corner | `music/radio_oldies_jukebox_in_the_corner.opus` | ElevenLabs Eleven Music-composed bright doo-wop number about a truck-stop jukebox for the in-game radio (2026-08-14 station-identity batch) |
| Milkshake Moon | `music/radio_oldies_milkshake_moon.opus` | ElevenLabs Eleven Music-composed slow-dance doo-wop ballad for the in-game radio (2026-08-14 station-identity batch) |
| Chrome Fins | `music/radio_oldies_chrome_fins.opus` | ElevenLabs Eleven Music-composed surf-rock instrumental about chrome tailfins for the in-game radio (2026-08-14 station-identity batch) |
| Hop in the Hardtop | `music/radio_oldies_hop_in_the_hardtop.opus` | ElevenLabs Eleven Music-composed jump-blues rock and roll dance number for the in-game radio (2026-08-14 station-identity batch) |
| Soda Shop Sweetheart | `music/radio_oldies_soda_shop_sweetheart.opus` | ElevenLabs Eleven Music-composed girl-group pop song with wall-of-sound sparkle for the in-game radio (2026-08-14 station-identity batch) |
| Drive-In Picture Show | `music/radio_oldies_drive_in_picture_show.opus` | ElevenLabs Eleven Music-composed twist-beat rocker about drive-in movie nights for the in-game radio (2026-08-14 station-identity batch) |
| Transistor Summer | `music/radio_oldies_transistor_summer.opus` | ElevenLabs Eleven Music-composed sunny beach-pop tune about a pocket radio for the in-game radio (2026-08-14 station-identity batch) |
| Sock Hop Saturday | `music/radio_oldies_sock_hop_saturday.opus` | ElevenLabs Eleven Music-composed piano-pounding rock and roll dance-floor filler for the in-game radio (2026-08-14 station-identity batch) |
| Wide Is the River | `music/radio_gospel_wide_is_the_river.opus` | ElevenLabs Eleven Music-composed full-choir southern gospel song about crossing over for the in-game radio (2026-08-14 station-identity batch) |
| Set My Burden Down | `music/radio_gospel_set_my_burden_down.opus` | ElevenLabs Eleven Music-composed slow-building spiritual of release and rest for the in-game radio (2026-08-14 station-identity batch) |
| Morning Will Come | `music/radio_gospel_morning_will_come.opus` | ElevenLabs Eleven Music-composed gentle piano gospel ballad of hope before dawn for the in-game radio (2026-08-14 station-identity batch) |
| Camino de Flores | `music/radio_tejano_camino_de_flores.opus` | ElevenLabs Eleven Music-composed flower-strewn Tejano cumbia for the road home, sung in Spanish, for the in-game radio (2026-08-14 station-identity batch) |
| Mi Troquita | `music/radio_tejano_mi_troquita.opus` | ElevenLabs Eleven Music-composed affectionate Tejano conjunto polka about a beloved truck, sung in Spanish, for the in-game radio (2026-08-14 station-identity batch) |
| Luna de Laredo | `music/radio_tejano_luna_de_laredo.opus` | ElevenLabs Eleven Music-composed romantic border-moon Tejano ranchera ballad, sung in Spanish, for the in-game radio (2026-08-14 station-identity batch) |
| Afterglow Avenue | `music/radio_synthwave_afterglow_avenue.opus` | ElevenLabs Eleven Music-composed dreamwave song with soft vocals about the last mile home for the in-game radio (2026-08-14 station-identity batch) |
| Hands on the Wheel | `music/radio_gospel_hands_on_the_wheel.opus` | ElevenLabs Eleven Music (music_v2_5) trucker gospel shuffle of faith on the road, for the in-game radio (2026-09-11 song batch) |
| Hallelujah Highway | `music/radio_gospel_hallelujah_highway.opus` | ElevenLabs Eleven Music (music_v2_5) hand-clapping up-tempo gospel celebration, for the in-game radio (2026-09-11 song batch) |
| Old Church Bell | `music/radio_gospel_old_church_bell.opus` | ElevenLabs Eleven Music (music_v2_5) close-harmony gospel quartet remembering a country church, for the in-game radio (2026-09-11 song batch) |
| Sunday Sunrise | `music/radio_gospel_sunday_sunrise.opus` | ElevenLabs Eleven Music (music_v2_5) instrumental organ-and-steel Sunday meditation, for the in-game radio (2026-09-11 song batch) |
| Carry Me Home | `music/radio_gospel_carry_me_home.opus` | ElevenLabs Eleven Music (music_v2_5) slow spiritual with a deep soloist and choir, for the in-game radio (2026-09-11 song batch) |
| Baila Conmigo | `music/radio_tejano_baila_conmigo.opus` | ElevenLabs Eleven Music (music_v2_5) dance-floor Tejano cumbia invitation, sung in Spanish, for the in-game radio (2026-09-11 song batch) |
| Acordeón del Alma | `music/radio_tejano_acordeon_del_alma.opus` | ElevenLabs Eleven Music (music_v2_5) Tejano accordion showcase instrumental, for the in-game radio (2026-09-11 song batch) |
| Polvo del Camino | `music/radio_tejano_polvo_del_camino.opus` | ElevenLabs Eleven Music (music_v2_5) road-dust conjunto tune for long hauls, sung in Spanish, for the in-game radio (2026-09-11 song batch) |
| Corazón de Cromo | `music/radio_tejano_corazon_de_cromo.opus` | ElevenLabs Eleven Music (music_v2_5) modern Tejano pop song with a chrome heart, sung in Spanish, for the in-game radio (2026-09-11 song batch) |
| Cumbia del Mercado | `music/radio_tejano_cumbia_del_mercado.opus` | ElevenLabs Eleven Music (music_v2_5) market-day cumbia instrumental, for the in-game radio (2026-09-11 song batch) |
| Midnight Grid | `music/radio_synthwave_midnight_grid.opus` | ElevenLabs Eleven Music (music_v2_5) pulsing retrowave instrumental for empty city freeways, for the in-game radio (2026-09-11 song batch) |
| Sodium Lights | `music/radio_synthwave_sodium_lights.opus` | ElevenLabs Eleven Music (music_v2_5) amber-lit synthwave nocturne of highway lamps, for the in-game radio (2026-09-11 song batch) |
| Starlight Odometer | `music/radio_synthwave_starlight_odometer.opus` | ElevenLabs Eleven Music (music_v2_5) glittering night-drive arpeggio instrumental, for the in-game radio (2026-09-11 song batch) |
| Neon Mirage | `music/radio_synthwave_neon_mirage.opus` | ElevenLabs Eleven Music (music_v2_5) desert-heat retrowave shimmer instrumental, for the in-game radio (2026-09-11 song batch) |
| Taillight River | `music/radio_synthwave_taillight_river.opus` | ElevenLabs Eleven Music (music_v2_5) flowing red-light synthwave meditation for slow lanes, for the in-game radio (2026-09-11 song batch) |
| Analog Heart | `music/radio_synthwave_analog_heart.opus` | ElevenLabs Eleven Music (music_v2_5) synthpop love song for machines that feel, for the in-game radio (2026-09-11 song batch) |
| Desert Satellites | `music/radio_synthwave_desert_satellites.opus` | ElevenLabs Eleven Music (music_v2_5) wide-sky ambient synthwave finale, for the in-game radio (2026-09-11 song batch) |
| Paper Cup Moon | `music/radio_night_paper_cup_moon.opus` | ElevenLabs Eleven Music (music_v2_5) quiet ballad of vending-machine coffee at midnight, for the Night Line (2026-09-11 song batch) |
| Idle Hearts | `music/radio_night_idle_hearts.opus` | ElevenLabs Eleven Music (music_v2_5) slow duet for two trucks idling side by side, for the Night Line (2026-09-11 song batch) |

## Radio Hosts And Static

The 2026-08-14 station-identity batch gave all 19 in-game stations a real
host, three station IDs (two sung jingles and a spoken legal ID), and a
shared pool of fictional ads. Every host line, ad read, and legal-ID script
is original writing for Freight Fate; no real station, host, or business is
represented.

| Sound | File | Source |
| --- | --- | --- |
| Roadhouse host breaks | `music/host_roadhouse_01.opus` … `music/host_roadhouse_08.opus` | ElevenLabs TTS host segments for the Freight Fate Roadhouse; regenerated 2026-08-14 with a new host voice |
| Night Line host breaks | `music/host_nightline_01.opus` … `music/host_nightline_08.opus` | ElevenLabs TTS host segments for the Freight Fate Night Line; regenerated 2026-08-14 |
| Host breaks, other 17 stations (The Rawhide, Big Wheel Country, Prairie Line, Big Sky Country, The Grind, Desert Rock, Chrome, The Ridge, The Sound, The Delta, Bayou Soul, Southern Soul, Nashville After Hours, Cruisin' Gold, Glory Road, Puro Tejano, Neon Drive) | `music/host_<station>_01.opus` … `music/host_<station>_08.opus` | ElevenLabs TTS host segments, eight per station, 2026-08-14 station-identity batch |
| Station jingles, all 19 stations | `music/id_<station>_01.opus`, `music/id_<station>_02.opus` | ElevenLabs Eleven Music sung station jingles, two per station, 2026-08-14 station-identity batch |
| Station legal IDs, all 19 stations | `music/id_<station>_03.opus` | ElevenLabs TTS spoken legal ID, run through the in-repo imaging chain (tools/radio_generate_content.py) and mixed with the imaging beds below, 2026-08-14 station-identity batch |
| Radio ad reads (18 fictional ads: travel centers, a diner, tire and truck-wash shops, insurance, boots, headsets, and more) | `music/ad_*.opus` | ElevenLabs TTS ad reads for the shared radio ad rotation, 2026-08-14 station-identity batch |
| Radio imaging beds | `radio/imaging/radio_imaging_impact.ogg`, `radio_imaging_riser.ogg`, `radio_imaging_shimmer.ogg`, `radio_imaging_stinger.ogg`, `radio_imaging_whoosh_long.ogg`, `radio_imaging_whoosh_short.ogg` | ElevenLabs Sound Effects API beds, mixed under the spoken station legal IDs by the in-repo imaging chain, 2026-08-14 station-identity batch |
| Radio static burst | `radio/static_burst.ogg` | Procedurally generated band-limited noise (tools/generate_radio.py) |
| Overspeed dash chime | `vehicle/overspeed_chime.ogg` | Procedurally synthesized two-partial bell strike (deterministic numpy/soundfile recipe, 2026-07-14) |
