# Freight Fate 2.0 alpha test book

This book is for the people testing the first 2.0 alpha. It says what is
new since 1.9.7, how to hear each change for yourself, and when to call
it working. The physics side (engine brake, brakes, ice, chains) still has
its own companion volume, `docs/physics-playtest-checklists.md`.

Everything to check is spoken. Each checklist says how to set it up, what
to do, what to listen for, and when to call it a pass. Work them in order
or pick the ones you like. Each stands alone, except Chapter 0, which is
setup you need first.

When something fails, write down three things: what you did, what you
heard, and what you expected to hear. Quote a wrong spoken line as closely
as you can; the exact wording is often the whole bug.

## Chapter 0. Before your first drive

### 0.1 Back up your saves first

Do this before you start the alpha for the first time.

On Windows and Linux, Freight Fate keeps its saves in a folder called
`saves` inside the game's own folder. Copy that whole folder somewhere
safe. It holds your careers, your settings and the key that signs your
careers; a career copied without that key can't be opened. On a Mac, the
saves are in one folder for every copy of the game: copy
`~/Library/Application Support/FreightFate` instead.

### 0.2 Your 1.9 careers come with you, as copies

The first time 2.0 starts, it copies every 1.9 career it can read into a
`2.0` folder inside `saves`, and Continue picks the 1.9 career you played
last. The 1.9 originals are not changed: 1.9 still plays them, and 1.9.7
refuses to open a 2.0 career rather than damage it.

Each copy of the game owns only the saves inside its own folder. So if
you unzip the alpha into a new folder, it starts with no careers; copy
your 1.9 `saves` folder into the alpha's folder before its first start.
Switching Settings, Updates, Update channel to alpha in your 1.9 copy
updates that copy in place, which keeps the saves but leaves you no 1.9
build in that folder. Keep a separate 1.9 folder if you want to go on
playing 1.9.

A copied career keeps its name, and a new 2.0 career can't take a name a
1.9 career already has.

One known snag: 1.9 and 2.0 share the settings file when they share a
folder. If you set Units to Imperial in 2.0 and then run 1.9, it comes
back as Automatic in 2.0. Set it again; Metric survives.

### 0.3 The alpha talks to the real orinks.net

There is no test server this time. Online features, the drivers list,
profile sharing and cloud backup, go to the real orinks.net with your real
account. A 2.0 career backs up under its own name with "(Career 2)" on the
end, such as Road Star (Career 2), so it never replaces the 1.9 backup of
the same driver. Deleting a 2.0 career removes only the 2.0 backup.

Your public profile may not name a 2.0 career's carrier or its Alaska and
Canadian cities until the website's update goes live. Backups work either
way. Don't report a missing carrier on the profile; do report a backup
that is refused, or a 1.9 backup that changed.

### 0.4 Settings worth knowing

- **Units** (Settings, Gameplay, Controls) is Automatic in 2.0: miles in
  the United States, kilometers in Canada. A 1.9 choice of imperial
  becomes Automatic; metric stays metric.
- **Weather source** and **Live weather controls calendar** (Settings,
  Gameplay, World and traffic) decide the date and the weather. With live
  weather on, the date is today's, so it is winter now in the north.
  Live weather covers the United States only; in Canada the weather is
  simulated.
- **Speak billboards** (Settings, Speech) turns billboards on or off.

### 0.5 What isn't in this alpha

Don't report these as missing; they are planned:

- No ferries: Juneau, Kodiak, Newfoundland and the other places with no
  road are not on the map.
- Not yet built: Route 138 east of Sept-Îles, The Pas and Flin Flon,
  Route 389 and Labrador, the James Bay Road, the Dempster Highway to
  Inuvik, the Liard Highway, and the Taylor and Top of the World
  highways.
- No wipers, frost or ice roads yet.
- Cattle have no recorded sounds yet.
- All eleven languages have the new spoken lines translated. If you
  hear an English line while playing in another language, report it with
  the line quoted.

The full list of rough edges is [Known gaps](#chapter-11-known-gaps).

## Chapter 1. What 2.0 changes

**The map reaches Alaska and Canada.** Alaska from Tok to Fairbanks,
Deadhorse, Anchorage, the Kenai Peninsula, Valdez, Whittier, Skagway and
Haines. Canada across British Columbia, the Yukon, the Northwest Territories,
Alberta, Saskatchewan, Manitoba, Ontario and Quebec, and New Brunswick,
Nova Scotia and Prince Edward Island. The north reaches Fort McMurray,
Yellowknife, Prince Rupert, the Stewart-Cassiar Highway with Stewart and
Hyder, Alaska, Dawson City, Thompson, Baie-Comeau, Sept-Îles and
Chibougamau. Chapters 3 and 7.

**The border is a stop.** Every crossing has a customs booth both ways,
with a chance of secondary inspection and a penalty for driving past.
Loads go into Canada and back. Chapter 4.

**Units follow the country.** Automatic speaks miles in the United States
and kilometers in Canada, fuel in liters and loads in tonnes there, and
Canadian prices at pumps and booths. Chapter 5.

**Hours follow the place.** Alaska, Canada south of 60 and north of 60
each have their own limits and cycles. Chapter 6.

**Carriers decide where you start.** Fifty-four carriers. A new career
picks a region, a home city and a carrier, and starts at that carrier's
terminal. Chapter 2.

**Northern weather is cold.** Simulated winters in Alaska and Canada match
the real ones. Chapter 8.

**Billboards follow sign law,** with new boards and landmarks for Canada
and Alaska, and holidays by country. Chapter 8.

**New freight.** Live cattle, doubles on the Interstates and the
turnpikes, hazardous loads routed around tunnels that ban them, bridges
that close in high wind, the Laredo yards, and the reefer and APU keys.
Chapter 9.

**Twenty-six new achievements.** Chapter 10.

## Chapter 2. Carriers and the career start

### 2.1 The career start list

Setup: none.

Do: New career, enter a name, pick a region, then a home city. Try a big
city (Chicago, Dallas), a small one in the plains (Billings), and
Anchorage.

Listen for: "Career start. 12 carriers hire in Chicago, Illinois." (the
count changes by city), then local carriers first, then regional, then
national, nearest terminal first, with the owner-operator start last.
Each row names the carrier's kind, what freight it leans toward, its
longest loads, and the terminal where your truck starts. F1 on a row says
more.

Pass when: every lower-48 city has at least eight carriers, Anchorage has
two (Chatanika Freight Lines and Knik Arm Cartage), and the distances in
the rows are in your units.

### 2.2 The first day at your carrier's terminal

Setup: finish 2.1 with a carrier whose terminal is in another city.

Listen for: the briefing at "{carrier} {city} terminal", saying the
carrier paid your travel from your home city; at the terminal menu, where
your truck is parked; a level 1 board that isn't empty.

Pass when: the terminal names the carrier and its city, and your first
assigned load leaves from there.

### 2.3 A career in Canada

Do: New career, then pick a Canadian region (Alberta, Atlantic provinces,
British Columbia, Ontario, Quebec, Saskatchewan and Manitoba, Yukon and
Northwest Territories) and a home city. Try Toronto, Regina and
Whitehorse.

Listen for: only Canadian carriers in the list; in Toronto, Port Lands
Cartage, Speed River Freight, Mattawa River Freight, Longspur Transport,
Great Shield Transport, then the owner-operator start leased to Longspur;
in Whitehorse, Liard River Freight alone. With Units on Automatic the rows
give kilometers.

Pass when: no US city sits under a Canadian row and no Canadian city
under a US one, every carrier listed has a terminal in Canada, the first
board holds a load you can take, and the Licenses and training list has
no doubles or LCV course.

### 2.4 Losing a job and getting a seat back

Setup: a company career near the reputation floor or the debt ceiling.
`FREIGHT_FATE_FORCE_PERSIST` off, so nothing sticks.

Do: drive badly until the carrier ends your employment.

Listen for: the news as you enter the terminal, before the terminal menu;
a regional or local carrier near your home taking you on, never the one
that ended it; Great Lakes Training Transport only when no regional or
local hires there. With no carrier at all, the terminal offers Apply to a
carrier where the dispatch board was.

Pass when: the new carrier hires near your home, and Get Back is earned
at the next terminal visit once you're on another carrier's payroll.

### 2.5 Moving up from Great Lakes

Setup: a Great Lakes Training Transport career at level 4 with full
dispatch trust.

Do: open Business status and choose Move to another carrier.

Listen for: carriers that hire where you live, and "You start at their
{city} terminal" before you confirm.

Pass when: the move names your new terminal, the truck waits there, and
Takin' Care of Business is earned.

### 2.6 A 1.9 career carried over

Setup: Chapter 0.2, with at least one 1.9 career.

Do: start 2.0. Choose Continue. Then start 1.9 from its own folder.

Listen for: Continue naming the 1.9 career you played last; the
terminal saying where its truck is.

Pass when: the career plays in 2.0 with its money, level and record, and
1.9 still opens the original unchanged.

## Chapter 3. Alaska

### 3.1 The Alaska Highway

Setup: a load or an empty run between Dawson Creek or Fort Nelson and
Tok, Fairbanks or Delta Junction. `FREIGHT_FATE_FORCE_CITY` can park you
in Whitehorse (`whitehorse_yt_ca`).

Listen for: town names and lower limits through the towns along the
highway; fuel stops, lodges and rest areas that say Fuel or No fuel;
"Crossing into Yukon." near Lower Post, said once; the Alcan port on the
way into Alaska and the Beaver Creek post on the way into Canada.

Pass when: you can plan fuel and sleep from Dawson Creek to Fairbanks
with T and the Truck stops app, and every crossing call is said once.
Drive the whole road, in pieces over several loads if you like, for The
Long and Winding Road.

### 3.2 The Dalton to Deadhorse

Setup: a load to Deadhorse, in winter for Cold as Ice.

Listen for: Coldfoot, the Arctic Circle, Atigun Pass, the steep grades
called with G, and Deadhorse selling diesel.

Pass when: the grades over Atigun Pass are called, fuel at Coldfoot is
offered, and the delivery earns Top of the World.

### 3.3 Whittier and the tunnel

Setup: a drive to Whittier from Bear Valley.

Listen for: the Anton Anderson Memorial Tunnel's one-lane gate, its
schedule and the toll.

Pass when: the gate holds you until your direction opens, the toll is
charged once, and the arrival earns Tunnel of Love.

### 3.4 Spring weight limits

Setup: a career date between April and mid-June, a Fairbanks or Anchorage
board.

Listen for: offers on thawing highways that name the road setting the
limit, and lighter loads on them.

Pass when: the limit is named on the offer, and a load booked under it
earns Travelin' Light on delivery.

### 3.5 Skagway and Haines

Setup: a load into Skagway, or a run from Haines to Haines Junction.

Listen for: the Klondike or Haines Highway into the Yukon, with a customs
booth at each crossing, and fuel at Haines Junction.

Pass when: each crossing has its booth and its hours line, and the
distance and fuel readouts make sense for a road this empty.

## Chapter 4. The border

### 4.1 The line, in order

Setup: Units on Automatic, a drive across any crossing. Blaine to Surrey
is short.

Listen for, in this order and each once: the crossing call ("Crossing
into British Columbia near the Pacific Highway border crossing."), the
time zone if it changes, "Speeds and distances are in kilometers now.",
then "Canadian hours rules: 13 hours of driving and 14 on duty in a
16-hour window." Coming back, "Speeds and distances are in miles now." and
the US hours line. The new country's speed limit from the line.

Pass when: nothing is dropped and nothing is cut off. A line cut by
another line and said again in full is known; note it if it bothers you.

### 4.2 The customs booth

Listen for: the port's name, its distance and agency, and "Every truck
stops at the customs booth."; then "Customs booth in half a mile. Stop at
the booth." (800 meters in kilometers).

Do: stop at the booth. Choose Answer the officer's questions and hand
over your documents.

Listen for: "The officer asks for your passport and checks the eManifest
filed for this load.", then "Cleared. Welcome to Canada." or "Welcome to
the United States."

Pass when: you had room to stop from the reminder, clearing took 10
minutes on duty (check Alt+A before and after), Escape at the booth says
"Customs has to clear you before you drive on.", and your first clearance
earns Borderline.

### 4.3 Secondary inspection

Do: cross several times. About one truck in ten is referred.

Listen for: "Referred to secondary inspection. Officers examine the
trailer for 45 minutes, then clear you."

Pass when: the 45 minutes show on the clock and in the logbook as on
duty.

### 4.4 Driving past the booth

Do: drive through the booth at speed.

Listen for: "An RCMP cruiser is behind you with its lights on." going
into Canada, or "A Border Patrol vehicle..." going south, then the stop,
"Officers escort you and the truck back to the port.", and the penalty:
5,000 US dollars the first time going south, 2,000 Canadian dollars going
north, charged in US dollars.

Pass when: a company driver hears the carrier pays; an owner-operator
pays it; customs then clears you as in 4.2.

### 4.5 Loads into Canada and back

Setup: a career at a national carrier other than Great Lakes, on a board
near the border (Bellingham, Detroit, Buffalo, Minot).

Listen for: loads to Canadian cities named with their province, "from
Bellingham, Washington to Surrey, British Columbia". On the board in
Canada, only loads to the United States.

Pass when: no Canadian board offers a load to another Canadian city; no
twin trailer, cattle or placarded load crosses; Great Lakes and local
carriers offer no Canadian loads; and the deadline leaves time for
customs.

### 4.6 Alaska freight and loads in bond

Setup: a career at a national carrier other than Great Lakes, at level 20,
on a board in Seattle, Portland or Spokane; then an owner-operator board in
Anchorage.

Listen for: loads to the TOTE, Matson and Alaska Marine Lines terminals at
Tacoma and Seattle; a rare load to Alaska by road, always steel, lumber,
machinery, building materials, food or refrigerated freight. In Anchorage,
nearly every load stays in Alaska. On the road to Alaska: "checks the
in-transit manifest and bond for this load" and "Cleared in bond. Welcome
to Canada." going into Canada, then "Cleared. The bond is closed. Welcome to
the United States." at the Alcan port.

Pass when: Great Lakes never offers Alaska by road; no general, retail or
placarded load runs between Alaska and the lower 48; a save made in the
Yukon resumes with the bond lines at the Alcan port.

### 4.7 A town that ships nothing

Setup: a company career.

Do: deliver to Watson Lake or Fort Nelson, which ship nothing, then open
the dispatch board.

Listen for: an assigned empty run to the nearest city with a load, with
its distance and pay.

Pass when: the empty run parks and settles when you arrive, with no dock
menu, and the next board has a load. An empty board with no way to leave
is a bug.

## Chapter 5. Units, fuel and money

### 5.1 Automatic at the line

Setup: Automatic, cruise set before a crossing.

Do: press Space before and after the line; press Plus and Minus in
Canada.

Listen for: speed in miles per hour before and kilometers per hour after,
with the truck's speed unchanged; the cruise dial stepping 95, 100, 105.

Pass when: every readout after the line is in kilometers: distances, R,
F (liters), the load (tonnes), low bridges (meters).

### 5.2 Imperial and Metric

Do: drive the same crossing with Imperial, then with Metric.

Pass when: Imperial stays in miles everywhere and a 100 sign reads 62;
Metric stays in kilometers in the United States too; neither says a
units line at the border.

### 5.3 Pumps, tolls and settlement

Setup: a Canadian fuel stop (McLeod Lake on the Alaska Highway, or
Whitewood on Regina to Brandon), and a toll bridge such as the Blue Water
Bridge.

Listen for: the price in Canadian dollars a liter and "N liters fueled";
the booth saying the Canadian price, such as 35 Canadian dollars at the
Blue Water Bridge.

Pass when: the pump and the booth speak Canadian dollars, and the
settlement and your cash stay in US dollars. With Units on Imperial, a
fuel row that gives the price a liter gives the fuel in liters too.

## Chapter 6. Hours of service

### 6.1 The rules lines

Listen for, as you cross into each place:

- "US hours rules: 11 hours of driving in a 14-hour window, and a
  30-minute break after 8 hours of driving."
- "Alaska hours rules: 15 hours of driving and 20 on duty, with no
  30-minute break."
- "Canadian hours rules: 13 hours of driving and 14 on duty in a 16-hour
  window."
- "North of 60 hours rules: 15 hours of driving and 18 on duty in a
  20-hour window." (the Yukon and the Northwest Territories)

Pass when: each is said once at its line, after the crossing call.

### 6.2 The readouts agree

Do: in each place, press C, Alt+A, Alt+S and Alt+D, and open the ELD app.

Pass when: each reads the rules you are under ("Canadian hours rules. 13
hours of driving, 14 hours on duty, and 16 hours of shift left."). Report
any readout that names a 30-minute break in Alaska or Canada.

### 6.3 The cycle and the restart

Setup: a career late in its week (60 or more hours on duty in the last
8 days).

Listen for: the ELD status line saying a day of cycle hours is left; the
sleep that finishes 34 hours off (36 in Canada) saying "Cycle clock
reset."

Pass when: the restart clears the cycle, and Sunday Morning Coming Down
is earned; at your home terminal, Take Me Home, Country Roads too.

### 6.4 Sleeping in Canada

Do: at a Canadian stop, read the sleep rows before choosing one, then
sleep.

Listen for: "Sleep 8 hours", "Sleep 8 hours in the lot" and "Motel room:
sleep 8 hours for 95 dollars", and in the preview "A full 8-hour sleep
resets your driving hours and legal driving window."

Pass when: the preview says "Afterward, driving allowance is 13.0 hours.
Your legal driving window closes in 16.0 hours", the Canadian numbers, the
clock moves 8 hours, and back in the United States the US clocks still
count, since the US rules want 10 hours off.

## Chapter 7. Canadian roads

### 7.1 Speed limits

Listen for: a 100 sign read as 100 kilometers per hour; British Columbia
holding heavy trucks to 105; an unsigned stretch taking the province's
default, such as 80 in Ontario, never an American figure like 97.

Pass when: no limit sounds like a conversion ("19", "64", "113") and
cruise holds the posted number.

### 7.2 Exits and directions in Ontario and Quebec

Listen for: exits naming the road they lead to, such as Route 255 or
Highway 8; Highway 400 to Barrie as north and the 401 to Kingston as east;
French place names as signed.

Pass when: no exit says a bare number, no direction is in French, and
place names are said in a way you would recognize. Note any French name
the voice gets badly wrong.

### 7.3 Time zones

Listen for: the local time line crossing into Saskatchewan and Manitoba
(Central), Alberta (Mountain), Kenora (Central to Eastern), the Peace
country around Dawson Creek (Mountain), the Yukon and the Northwest
Territories (Mountain), and New Brunswick, Nova Scotia and Prince Edward
Island (Atlantic).

Pass when: the clock after each line matches the zone, and the delivery
appointment still uses the receiver's time.

### 7.4 Atlantic Canada

Setup: a load across Houlton or Calais into Nova Scotia, then one to
Prince Edward Island and back.

Listen for: the customs booth, the Cobequid Pass toll in each direction,
and the Confederation Bridge toll on the way off the island.

Pass when: the Cobequid Pass toll is charged each way, the bridge charges
nothing going onto the island and once leaving it, and each toll is in
Canadian dollars at the booth.

### 7.5 The Deh Cho Bridge toll

Setup: a load from the United States to Yellowknife
(`FREIGHT_FATE_FORCE_DEST` set to `yellowknife_nt_ca`, with a carrier
that crosses the border), as a company driver.

Listen for: "Crossing into the Northwest Territories." and the north-of-60
hours rules on the Mackenzie Highway. North of Hay River, "Crossing the
Mackenzie River." then "Transponder toll at Deh Cho Bridge, 168 Canadian
dollars, billed to carrier settlement."

Pass when: the toll is charged once going north and not at all on the
way back south.

### 7.6 Leaving Hyder

Setup: a company driver parked in Hyder (`FREIGHT_FATE_FORCE_CITY` set
to `hyder_ak_us`). Hyder has no freight, so dispatch sends you out
empty, and the only road out runs through Stewart.

Listen for: "Stewart Port of Entry in half a mile, Canada Border
Services Agency. Every truck stops at the customs booth.", then
"Crossing into British Columbia.", "Crossing into Pacific Time.",
"Speeds and distances are in kilometers now." and the Canadian hours
rules. At the booth: "Stewart Port of Entry, Canada Border Services
Agency. The officer asks for your passport and checks the eManifest
filed for this load."

Pass when: you can't drive on from Stewart without stopping at the
booth. Going the other way, into Hyder, nothing stops you, because there
is no US port.

## Chapter 8. Weather and billboards

### 8.1 Northern cold

Setup: simulated weather (or live weather in Canada),
January.

Listen for: Fairbanks below zero Fahrenheit on a January morning,
Whitehorse well below freezing, Seattle unchanged.

Pass when: temperatures fall as you drive north, and a July day in
Fairbanks is mild.

### 8.2 Live weather at the border

Setup: Weather source live.

Pass when: past the line the weather is simulated, alerts stay quiet,
and live weather returns at the US line.

### 8.3 Billboards and sign law

Setup: Speak billboards on.

Listen for: new boards and plain landmark lines along the Canadian and
Alaska roads; no boards at all in Maine, Vermont, Alaska, Alberta, Prince
Edward Island, the Northwest Territories or parks the road crosses, so in
Alberta places are named plainly as you pass; only boards for places to
visit in the Yukon, New Brunswick and Nova Scotia; holiday boards by
country, Canada Day and October's Thanksgiving only in Canada. Between
Barrie and North Bay, listen for "Passing Bethune Memorial House, in
Gravenhurst." in place of the town's own name.

Pass when: a board names a real place you are near, none plays where
boards are banned, and a US holiday board never plays in Canada.

## Chapter 9. Freight

### 9.1 Live cattle

Setup: a career at level 5 or more with the livestock handling
certificate (free course from level 5).

Listen for: the head count in job details; the 2-hour, 1-hour and
30-minute warnings before the 28-hour limit; cattle losing their footing
after hard braking; Check the cattle when one goes down; the shrink at
delivery.

Pass when: a clean load earns Rawhide, and the hours readouts name no
30-minute break while cattle are aboard.

### 9.2 Doubles

Setup: the doubles endorsement; for turnpike doubles, the LCV certificate.

Listen for: hook-up time once at the shipper and once at the receiver, a
25-minute walk-around, the rear trailer whipping on a hard swerve at 40
miles per hour, reverse refused.

Pass when: the board posts the miles of the road the doubles must take,
and no doubles load is offered into Canada.

### 9.3 Hazardous loads, tunnels and wind

Listen for: the material in job details ("Gasoline, UN1203, Class 3"),
routes around tunnels that ban the load, the Mackinac escort wait, a
bridge closed in high wind.

Pass when: a load turned away hears only why, and wind and escort waits
count as on duty.

### 9.4 Laredo

Setup: a southbound load to Laredo.

Pass when: the load names its Mexican consignee, the trailer drops at
the yard, and South of the Border is earned.

### 9.5 A refused load

Do: deliver a badly damaged load until the receiver refuses it.

Listen for: "Load refused" as the title, "Pay for the load: none. The
receiver refused it.", no on-time bonus.

Pass when: the settlement doesn't contradict itself, and a company driver
is charged their own pay for the load, not its full price, plus the damage
deductible.

## Chapter 10. Achievements

Twenty-six new achievements. Seventeen come from Alaska, the border and
the new freight: North to Alaska, Anchorage, Top of the World, Tunnel of
Love, The Long and Winding Road, When It's Springtime in Alaska, Cold as
Ice, Travelin' Light, Borderline, Sunday Morning Coming Down, Take Me Home,
Country Roads, Workin' for a Livin', Get Back, Takin' Care of Business,
Rawhide, Cattle Call and South of the Border. Nine come from Canada:
Across the Border, Alberta Bound, Runnin' Back to Saskatoon, Sudbury
Saturday Night, Farewell to Nova Scotia, Mon Pays, Northwest Passage, This
Land Is Your Land, and one hidden. The checklists above say where most of
them come from.

Pass when: each is earned once, at the moment its description says, and
Achievements reads its description. Past deliveries earn nothing; only
Get Back reads your saved record.

### 10.1 Across the Border

Setup: a load from Detroit to Windsor.

Do: deliver it.

Listen for: "Across the Border" in the list of new achievements on the
delivery summary. In Achievements, Places on the Map, the row reads
"Earned: Across the Border. A load delivered in Canada."

Pass when: it is earned on that first Canadian delivery and not again.

### 10.2 Farewell to Nova Scotia

Setup: a load from Halifax or Truro to the United States.

Do: deliver it.

Listen for: "Farewell to Nova Scotia" on the delivery summary, not at
pickup.

Pass when: it is earned when the load is delivered.

### 10.3 Mon Pays

Setup: a winter clock, and a load to anywhere in Quebec.

Do: deliver it. Then try a Quebec delivery outside winter on a fresh
career.

Pass when: only the winter delivery earns it.

### 10.4 The hidden one

Listen for: Deep Cuts in Achievements, where locked hidden rows read
"Locked: A Secret the Manifest Is Keeping".

Pass when: no locked row gives a hidden achievement's name or
description.

## Chapter 11. Known gaps

Known and recorded. Report them only if one costs you a load or a drive.

- Some northern links are thin on stops: Tok to Haines Junction has fuel
  but no long rest stop; Timmins to Sudbury has fuel at Gogama but no
  sourced overnight stop; five Prairie and Rockies links under 200 miles
  have no 10-hour stop; Slave Lake to Peace River has fuel at High
  Prairie but no sleep stop; Fort Providence to Yellowknife has no fuel
  on the road, as in life, and one rest area; Highway 6 has no 10-hour
  stop with a source between Ashern and Thompson; Route 167 has no fuel
  for 118 miles south of Chibougamau, and Route 113 none for 128 miles
  between Senneterre and Waswanipi.
- Several northern stops are town stations or unnamed rest areas whose
  truck access is assumed.
- Kenora, Hearst and Kapuskasing use unnamed industrial land for freight.
- About 75 Ontario and Quebec exits still sign a bare number.
- Canada has no county lines, so Alt+5 says less there.
- Canadian traffic is estimated, not read from provincial counts.
- Canadian diesel prices are fixed 2025 averages.
- Canada's daily off-duty rules and cycle 2 are not modeled.
- The deadline plans the whole route on US rules, so the board's trip
  hours, the deadline plan and the team hand-over lines still say 10
  hours in Canada.
- A Canadian career still hears US license, endorsement and police
  words: CDL and its suspensions, the hazmat background check, the tank
  endorsement letter, the TWIC port card and troopers.
- Watson Lake, Haines Junction, Fort St. John, Dawson Creek, Fort Nelson,
  Terrace, Smithers, Kitimat and Sept-Îles are not offered as homes:
  nothing there ships a load a new driver can take.
- Money stays in US dollars for a Canadian career.
- Passports and FAST cards are not modeled; Huntingdon clears trucks on
  weekends, when the real port is closed.
- No Ambassador Bridge, Peace Bridge, Fort Frances crossing or Highway 407.
- French and other place names have not all been checked by ear.
- Twin parcel loads stay off Alaska and Canadian lanes.

## Appendix A. Levers

Set these before launching the game, load your career from the main
menu, and unset them when you're done. A lever session is a sandbox: you
hear "Playtest sandbox: nothing this session is saved" and your real save
resumes untouched next time. Add `FREIGHT_FATE_FORCE_PERSIST = "1"` only
when you want the changes to stick.

In PowerShell:

    $env:FREIGHT_FATE_FORCE_CITY = "bellingham_wa_us"
    $env:FREIGHT_FATE_FORCE_CLOCK = "8"
    $env:FREIGHT_FATE_FORCE_DEST = "surrey_bc_ca"

- `FREIGHT_FATE_FORCE_CITY`: park the career in a city, by key or plain
  name.
- `FREIGHT_FATE_FORCE_CLOCK`: roll the clock to the next time it reads
  that hour (0 to 23), logged off duty.
- `FREIGHT_FATE_FORCE_DEST`: the next new dispatch board offers a load to
  that city when a road reaches it. A Canadian destination needs a board
  that may send loads there.
- `FREIGHT_FATE_FORCE_WEATHER`: see the companion volume.

City keys are the city, the state or province, and the country, in lower
case with underscores: `fairbanks_ak_us`, `whitehorse_yt_ca`,
`yellowknife_nt_ca`, `hyder_ak_us`, `quebec_city_qc_ca`.

Unset with `Remove-Item Env:FREIGHT_FATE_FORCE_CITY`, and the same for
the others.

## Appendix B. Reporting a run

Report a problem on the main menu opens the report form with your game
log attached. Say which chapter and checklist, your units and driving
mode, whether weather was live or simulated, what you did, what you
heard, and what you expected.
