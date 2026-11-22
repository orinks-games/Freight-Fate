# Winter cab: climate that matters, wipers, winter trucks and ice roads

Design, 2026-11-22. Owner's ask the same day: the heat and air should do
something (frost when the heat is off in the cold), the truck needs
windshield wipers, and the catalog may need trucks for Alaska ice and frozen
lakes, on Canada's roads too. Status and order are in `ROADMAP.md` under
"Winter cab, wipers and ice roads". This file extends
`docs/weather-wiper-audio-notes.md` (July 2026), whose decisions stand
unless a section here says otherwise.

Nothing here is built. Each step is its own PR with its own agent-server
drive.

## What exists

- `ff_core::sim::cab`: window, fan (off, low, medium, high) and heat or air
  conditioning, as sound only. Z, Alt Z, Shift Z. No effect on the drive, by
  design at the time (a player who plays to relax asked for it). Not saved.
- No wipers, no windshield state, no headlight control
  (`enforcement_observe` assumes lights on).
- Temperature: `sim::season::temperature_c(region, hours)` from
  `REGION_CLIMATE` (sixteen United States regions), or the National Weather
  Service reading with live weather on. Alaska, Yukon and British Columbia
  map to `pacific_northwest`, whose coldest moment is 11 - 8 - 6 = -3 °C.
  Alberta and the Northwest Territories map to `rockies`, Saskatchewan and
  Manitoba to `heartland` (coldest -10 °C). Live weather does not cover
  Canada.
- Snow, freezing rain, grip, chain laws and chains exist. Visibility
  (`effects().visibility_mi`) already delays hazard calls and feeds the
  traffic and enforcement models.
- `models::trucks`: `TruckSpecs` is torque, tank, drag, fuel burn and mass.
  Four garage upgrades. Nothing about cold.

## Step 1: real winter temperatures (before the alpha)

A climate table for Alaska and Canada, separate from the freight region,
so `classify_region` keeps driving freight, hazards and market tags.

- A new `climate_region(state, lat, lon)` in `data/regions.rs` returns a
  climate key; United States states outside Alaska return their region
  unchanged. `temperature_c` and every caller that passes a region for
  temperature take the climate key.
- New `REGION_CLIMATE` rows, about ten: Alaska interior, Alaska south
  coast, Alaska panhandle, Alaska Arctic slope, Yukon, Northwest
  Territories, British Columbia interior (the coast stays
  `pacific_northwest`), prairies, the northern forest belt (northern
  Alberta to Quebec), Atlantic Canada. Split by latitude and longitude
  where one province spans two, with a test line per split.
- Provenance: each row is **derived**. Inputs are the 1991 to 2020 monthly
  normals for a named station in that climate (NOAA for Alaska,
  Environment and Climate Change Canada for Canada). Formula, written in
  the row's comment: mean = (January mean + July mean) / 2, seasonal
  amplitude = (July mean - January mean) / 2, daily amplitude = half the
  mean daily range averaged over January and July. Screen: no derived
  January mean may be warmer than the station's own published January
  mean by more than 1 °C.
- Check the knock-on: winter weather sampling, chain laws, the weather
  report, livestock heat stress, the outside ambience (birds and insects
  by temperature), the cab's starting mode. Run `cargo test-all` and the
  battery; drive Fairbanks to Tok in January and Whitehorse to Watson Lake
  in January with Facility streets off.
- Later, not this step: live weather for Canada.

## Step 2: the windshield and wipers

One state, in `ff-core`: how clear the windshield is, 0 to 1.

- `Wipers { Off, Intermittent, Low, High }` with `next()`, one key (pick a
  free one; the binding table and the touch layout both need it). Each
  press answers in two words: "Wipers off.", "Wipers intermittent.",
  "Wipers low.", "Wipers high." The dial rule applies: every press answers
  as it lands and cuts the last.
- Rain and wet snow lower clarity at a rate set by the weather kind and
  road speed; wipers raise it at a rate set by their speed. The numbers
  are **assumed** and labelled so, tuned only so that the matching wiper
  speed holds the glass clear in each rain strength.
- `Glass { Clear, Hazy, Blocked }` bands over clarity, with `next(event)`
  and a transition table test. The agent server's `status` reports the
  band and the wipers; `scenario` can set both.
- Cost of a glass that is not clear, two parts, never speech:
  1. the lane-keeping bed blurs (July decision);
  2. effective visibility falls toward the fog values (new; waits on the
     owner's answer to question 1 below). Radar-based assists are not
     affected.
- Sounds: one swipe per wiper pass; a squeak and drag when the glass is
  dry for the setting; rain on glass at three strengths. Synthesized
  placeholders first, under final keys, as the cab sounds were.
- Speech: none for the wipers beyond the key's answer; the swipe is their
  sound. The status screen names the windshield only when it is not clear.
- Driving assistance: a wipers row. The most assisted preset runs them
  (rain-sensing); Realistic leaves them to the driver. Company and owned
  trucks behave the same.
- `docs/ontology.md` rows: windshield, wipers.

## Step 3: frost, fog and the heat

- Frost lowers clarity when the outside is below freezing and the fan is
  off or on air conditioning; faster the colder it is. Fog on the inside
  lowers it in cold wet weather (rain, snow or fog, below about 10 °C)
  with the fan off. Thresholds get a cited source or the label assumed.
- The fan on heat clears frost and prevents it; the fan on heat or air
  clears fog. Fan speed sets the rate. No separate defrost mode.
- Window down in freezing weather: the heat cannot hold, frost returns.
- Cold start: a truck parked below freezing with the engine off starts
  with the glass `Blocked` by frost. A truck that idled starts clear.
- Calls at standard speech, one per transition, each over its own sound:
  "Windshield frosting.", "Windshield fogging.", "Windshield clear."
  Quiet and urgent-only verbosity play the sound alone.
- Driving assistance: a climate row, as for wipers.

## Step 4: resting in the cold, the winter package, winter trucks

- A rest taken in the truck when the outside is below or above the
  comfort band idles the engine for the whole rest. Fuel is charged at an
  idle rate with a cited source (a published idling study), and one line
  at waking says so: "Idled ten hours for heat. Eight gallons." (liters
  under metric units). A motel rest idles nothing.
- Fuel that would run out during the rest: say so before the rest starts,
  in the rest menu's own text, not as a new warning at the wheel.
- Garage upgrade `winter_package`, three tiers: block heater (clean start
  after a cold night), bunk heater (rest without idling; burn rate from
  the maker's published figure), heated windshield and mirrors (no frost
  while the engine runs).
- Two catalog trucks: a northern spec conventional (full winter package
  from the factory) and a tri-drive heavy hauler (traction on snow and ice
  when pulling away and climbing; heavier, thirstier). `TruckSpecs` gains
  the winter flags and a drive-axle count. Names, descriptions and prices
  are the director's.
- Carriers with terminals in Alaska or northern Canada list the northern
  spec in their fleets.
- Later, gently if at all: a hard start after a night off in deep cold
  (long crank, slow warm-up). Never a dead truck on a remote road.
- Not building: fuel gelling, frozen air lines, frozen exhaust fluid.

## Step 5: ice roads

A leg kind with a season, a surface that punishes speed by sound, and a
convoy. First road: the Tibbitt to Contwoyto Winter Road, from the end of
the Ingraham Trail east of Yellowknife to the diamond mines. Needs
Yellowknife (Canada corridor 5).

Read, with sources (the joint venture's site, jvtcwinterroad.ca, and the
road's Wikipedia page, both fetched 2026-11-22):

- About 400 km to the mines; about 85 percent on lake ice; 64 portages.
- Opens about February 1; 67 days in an average year.
- Loaded on ice 25 km/h, 10 km/h in marked areas; empty 60 km/h.
  Portages 30 km/h, 10 km/h on and off the ice. Radar enforced.
- Convoys of four leave every twenty minutes; no truck runs alone.
- Full loads at 100 cm of ice; the road opens at 74 cm.
- Freight north: mostly diesel; cement, tires, ammonium nitrate prill,
  construction materials. Mines: Ekati, Gahcho Kué (Diavik ceased
  production in March 2026).

Design:

- Seasonal legs: a leg carries an open window (read per year from the
  operator's published dates, or the average where a year is missing,
  labelled). Out of season it is not routed and its loads are not posted.
  Reuse the spring weight-limit window machinery.
- Spans: each leg carries ice and portage spans, **derived** from
  OpenStreetMap (`ice_road=yes` ways against lake polygons) and checked
  against the operator's portage count.
- The ice as a state machine in `ff-core`: `Ice { Quiet, Working,
  Cracking }` from speed over the limit and distance to shore, one sound
  bed per state. No spoken line for the ice itself.
- Enforcement: the road's security. First time over the limit by a set
  margin: one spoken warning. Second: off the road, load lost. No truck
  sinks in the first version (owner question 4).
- Convoy: three other trucks, a gap to hold, calls on the CB
  (`docs/cb-radio-design.md`). Whiteouts on open lakes use the visibility
  model.
- Cruise needs a floor low enough to hold 25 km/h.
- Tires sound different on ice and on packed snow, so a portage is heard.
- Rest at the road's camps; read their places from the operator's map.
- Achievements and billboards are the director's, afterwards.

## Step 6: more winter roads

One at a time, as their starting points get roads:

- An oil field ice road out of Deadhorse. North Slope ice roads are built
  each winter by the oil companies with state approval, for 90 to 100
  days (Alaska Public Media 2017, Anchorage Daily News 2014). Pick one
  that recurs on a mapped line; none is confirmed yet.
- Northwest Territories public winter roads: the Mackenzie Valley winter
  road north of Wrigley, the Aklavik ice road, and the Peel and Mackenzie
  ice crossings on the Dempster (ferries in summer, closed at freeze-up
  and break-up). Dates and weight limits from the territory's lists.
- Manitoba: over 2,356 km to 22 communities, late January to mid-March
  (province's release, 2025-01-31).
- Northern Ontario: about 30 communities, mid-January to the thaw
  (ontario.ca winter roads page).

## Open questions for the owner (director's pick in brackets)

1. Does a blocked windshield also shorten how far the driver sees, or
   only blur the lane bed as decided in July? [Both.]
2. Automatic wipers and climate: a driving assistance choice, or bought
   equipment as in the July notes? [Assistance choice.]
3. When does frost form? [Below freezing with the fan off, thicker and
   faster below 0 °F.]
4. Can a truck go through the ice? [No.]
5. Does a freezing or sweltering cab tire the driver? [No.]
