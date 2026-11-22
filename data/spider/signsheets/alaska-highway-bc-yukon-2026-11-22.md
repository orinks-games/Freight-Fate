# The Alaska Highway and Highway 97, British Columbia and the Yukon -- 2026-11-22

Copy by the director, 2026-11-22, waiting on Orinks's approval; only then
does `cargo tools bake-billboards --sheet <this file> --write` put it on the
legs. Both directions of an attraction read the same line.

Waiting for the West corridor: the thirteen signs on Highway 1 and 97
between Surrey and Prince George (Bridal Veil Falls, Hope, Yale, Hell's
Gate, One Hundred and Eight Mile, Barkerville, and The Exploration Place
from the Surrey side) are marked `skip` here. That corridor replaces the
Surrey to Prince George leg with a chain of shorter legs, so these move
to its sheet, with the same copy, once it merges.

Twenty-four attractions on the seven roads Career 2.0 has north of the
border: Highway 1 and 97 from Surrey to Prince George, the John Hart
Highway to Dawson Creek, and the Alaska Highway from Dawson Creek to Tok.
Forty-three signs, one in each direction a leg reaches the place; the
Kiskatinaw bridge's two are skipped (the bridge is closed). Every `leg:`
is written the way the driver reads the sign and `at_mi` counts from that
end; the bake mirrors the milepost onto a leg stored the other way round
and records which way the board faces.

How the places were found: OpenStreetMap's tourism, historic and museum
features within a few miles of each leg's archived road (the OSM API, read
2026-11-22), plus the Alaska Highway's well-known stops. Each attraction's
OSM element is named under it. Mileposts come from projecting it onto the
leg's road. A sign stands about eight to twelve miles before the place,
or before the city for a place in a destination city, and at least two
miles from any other callout heard the same way. The bake's dry run is
clean: no sign within two miles of another callout, none where the sign
law or a park bans it, no digits.

Law (section 11 of `docs/billboard-spider-methodology.md`): British
Columbia licenses boards off the right of way; the Yukon licenses only
boards for visitor services, facilities and attractions, which every
place here is; Alaska bans them, so the Tok legs carry signs only on their
Yukon side. Every provincial park, protected area and Kluane National Park
the road runs through is a billboard ban on the leg, so no sign stands
there; a few signs stand farther out than usual because of that, and each
says so.

For Orinks's ear: **Da Kų** (Haines Junction) is written with a u
with an ogonek, Southern Tutchone for "Our House"; also Tutchone, Aishihik,
Kluane, Liard, Teslin, Tetsa, Chetwynd and Quesnel. No French names on
these roads.

Facts that change what a board may promise: the Kiskatinaw bridge is
closed to vehicles; the Toad River hats may be gone; the SS Klondike
cannot be boarded; the Kluane museum is now the Kluane Museum of History;
the Airtram and several museums are summer only, and a board stands all
year.

## Mile Zero of the Alaska Highway -- Mile Zero post, Dawson Creek (102 Avenue and 10 Street)

The Dawson Creek Jaycees' sign painter Ellis Gislason built the first tall wooden post, presented to the city on Christmas Day 1946 and set in the city centre rather than at the road's exact start. It was not fastened down and suffered Halloween pranks (stolen, set alight, once swapped for an outhouse). A metal post of nearly the same design replaced it in the early 1960s. Outdoors, always there. Source: South Peace Historical Society (calverley.ca). OpenStreetMap node 5007856267.

### Mile Zero of the Alaska Highway (southbound, Prince George to Dawson Creek)
- treatment: billboard
- leg: prince_george_bc_ca -> dawson_creek_bc_ca
- at_mi: 244
- spoken: Billboard: Mile Zero of the Alaska Highway, ahead in Dawson Creek. The first post was stolen, set alight, and once swapped for an outhouse. The new one is metal.

### Mile Zero of the Alaska Highway (southbound, Fort St. John to Dawson Creek)
- treatment: billboard
- leg: fort_st_john_bc_ca -> dawson_creek_bc_ca
- at_mi: 37
- spoken: Billboard: Mile Zero of the Alaska Highway, ahead in Dawson Creek. The first post was stolen, set alight, and once swapped for an outhouse. The new one is metal.

## Chetwynd chainsaw carvings -- Chetwynd (Highway 97)

More than 120 chainsaw carvings around town; the Chetwynd International Chainsaw Carving Championship, first held 2005, runs the second weekend of June. Source: District of Chetwynd (gochetwynd.com).

### Chetwynd chainsaw carvings (eastbound, Prince George to Dawson Creek)
- treatment: billboard
- leg: prince_george_bc_ca -> dawson_creek_bc_ca
- at_mi: 180
- spoken: Billboard: Chetwynd, ahead, with more than a hundred and twenty chainsaw carvings around town. Every June the carvers come back and make more. The trees are nervous.

### Chetwynd chainsaw carvings (westbound, Dawson Creek to Prince George)
- treatment: billboard
- leg: dawson_creek_bc_ca -> prince_george_bc_ca
- at_mi: 53
- spoken: Billboard: Chetwynd, ahead, with more than a hundred and twenty chainsaw carvings around town. Every June the carvers come back and make more. The trees are nervous.

## The Exploration Place -- Prince George, Lheidli T'enneh Memorial Park

Museum and science centre: a paleontology gallery, the Lheidli T'enneh gallery, and the Fort George Railway outside, which the museum calls Canada's shortest passenger railway. Opened as the Fort George Museum in 1972, burned 1975, The Exploration Place since 2001. Open year round. Sources: theexplorationplace.com; Waymarking. OpenStreetMap way 252066487.

### The Exploration Place (southbound, Dawson Creek to Prince George)
- treatment: billboard
- leg: dawson_creek_bc_ca -> prince_george_bc_ca
- at_mi: 242
- spoken: Billboard: The Exploration Place, ahead in Prince George. Fossils inside, and outside, what the museum calls Canada's shortest passenger railway. It hauls no freight.

### The Exploration Place (northbound, Surrey to Prince George)
- treatment: skip
- leg: surrey_bc_ca -> prince_george_bc_ca
- at_mi: 455
- spoken: Billboard: The Exploration Place, ahead in Prince George. Fossils inside, and outside, what the museum calls Canada's shortest passenger railway. It hauls no freight.

## Kiskatinaw curved bridge -- Old Alaska Highway, north of Dawson Creek

Curved timber bridge of 1942 to 1943, a nine-degree curve, about 162 m; bypassed in 1978. **Skipped: closed to vehicles since a landslide on June 4, 2021**, with no reopening found (news.gov.bc.ca 2021TRAN0083-001095; Energetic City, July 2022). A truck cannot go and see it. Revisit if it reopens.

### Kiskatinaw curved bridge (northbound, Dawson Creek to Fort St. John)
- treatment: skip
- leg: dawson_creek_bc_ca -> fort_st_john_bc_ca
- at_mi: 12
- spoken: Billboard: Kiskatinaw curved bridge.

### Kiskatinaw curved bridge (southbound, Fort St. John to Dawson Creek)
- treatment: skip
- leg: fort_st_john_bc_ca -> dawson_creek_bc_ca
- at_mi: 19
- spoken: Billboard: Kiskatinaw curved bridge.

## North Peace Museum -- Fort St. John (9323 100 Street)

Regional history from First Nations settlement to oil and gas: a tepee, trapper's cabin, blacksmith shop, a 1921 school room, an Alaska Highway exhibit. Current building opened February 16, 1984. Sources: City of Fort St. John; Prince George Citizen. OpenStreetMap way 159801244.

### North Peace Museum (northbound, Dawson Creek to Fort St. John)
- treatment: billboard
- leg: dawson_creek_bc_ca -> fort_st_john_bc_ca
- at_mi: 31
- spoken: Billboard: The North Peace Museum, ahead in Fort Saint John. A trapper's cabin, a blacksmith shop and a schoolroom from nineteen twenty-one. Class is dismissed.

### North Peace Museum (southbound, Fort Nelson to Fort St. John)
- treatment: billboard
- leg: fort_nelson_bc_ca -> fort_st_john_bc_ca
- at_mi: 226
- spoken: Billboard: The North Peace Museum, ahead in Fort Saint John. A trapper's cabin, a blacksmith shop and a schoolroom from nineteen twenty-one. Class is dismissed.

## Fort Nelson Heritage Museum -- Fort Nelson (5553 Alaska Highway)

Opened 1987; curator Marl Brown began saving discarded things in 1957. Highway construction artifacts and old vehicles, among them a 1908 McLaughlin Buick that was driven to Whitehorse and back in 2008. Summer season. Sources: fortnelsonmuseum.ca; Curbside Classic. OpenStreetMap way 161731473.

### Fort Nelson Heritage Museum (northbound, Fort St. John to Fort Nelson)
- treatment: billboard
- leg: fort_st_john_bc_ca -> fort_nelson_bc_ca
- at_mi: 228
- spoken: Billboard: The Fort Nelson Heritage Museum, ahead. Old highway machinery, and a nineteen oh-eight Buick that drove to Whitehorse and back at a hundred years old. It did not complain.

### Fort Nelson Heritage Museum (southbound, Watson Lake to Fort Nelson)
- treatment: billboard
- leg: watson_lake_yt_ca -> fort_nelson_bc_ca
- at_mi: 308
- spoken: Billboard: The Fort Nelson Heritage Museum, ahead. Old highway machinery, and a nineteen oh-eight Buick that drove to Whitehorse and back at a hundred years old. It did not complain.

## Tetsa River Lodge -- Historic Mile 375, Alaska Highway

Family-run log lodge, bakery, campground, fuel; known for its cinnamon buns (The Milepost calls it the Cinnamon Bun Center of the Galactic Cluster; about 200 a day in summer, Real Food Traveler 2017). Listed open year round (Tourism Northern Rockies); this year's status not confirmed. OpenStreetMap node 11897539785. The eastbound sign stands 25 miles out because Stone Mountain Provincial Park and the Northern Rocky Mountains protected area take the last 21 miles.

### Tetsa River Lodge (westbound, Fort Nelson to Watson Lake)
- treatment: billboard
- leg: fort_nelson_bc_ca -> watson_lake_yt_ca
- at_mi: 60
- spoken: Billboard: Tetsa River Lodge, ahead on the Alaska Highway. A log lodge famous for its cinnamon buns, baked by the hundred in summer. One is a meal. Nobody buys one.

### Tetsa River Lodge (eastbound, Watson Lake to Fort Nelson)
- treatment: billboard
- leg: watson_lake_yt_ca -> fort_nelson_bc_ca
- at_mi: 224
- spoken: Billboard: Tetsa River Lodge, ahead on the Alaska Highway. A log lodge famous for its cinnamon buns, baked by the hundred in summer. One is a meal. Nobody buys one.

## Toad River Lodge -- Historic Mile 422, Alaska Highway

Lodge, restaurant, fuel, campground, open since 1947. Famous for a ceiling of donated hats, begun 1979, more than 11,000. **Caution:** Atlas Obscura says the hats were no longer on display as of July 2022, and no second source settles it; copy should not promise the hats. Sources: Atlas Obscura; Wikipedia. OpenStreetMap way 1313927561.

### Toad River Lodge (westbound, Fort Nelson to Watson Lake)
- treatment: billboard
- leg: fort_nelson_bc_ca -> watson_lake_yt_ca
- at_mi: 109
- spoken: Billboard: Toad River Lodge, ahead on the Alaska Highway. Feeding highway travelers since nineteen forty-seven. Toad is not on the menu.

### Toad River Lodge (eastbound, Watson Lake to Fort Nelson)
- treatment: billboard
- leg: watson_lake_yt_ca -> fort_nelson_bc_ca
- at_mi: 199
- spoken: Billboard: Toad River Lodge, ahead on the Alaska Highway. Feeding highway travelers since nineteen forty-seven. Toad is not on the menu.

## Northern Rockies Lodge -- Historic Mile 462, Muncho Lake

Lodge on Muncho Lake with a floatplane base: flightseeing, fly-in fishing, day trips to Virginia Falls in Nahanni National Park. Source: the lodge's site. OpenStreetMap way 904220274. The lodge is inside Muncho Lake Provincial Park, so both signs stand outside the park: 36 miles out westbound and 26 eastbound.

### Northern Rockies Lodge (westbound, Fort Nelson to Watson Lake)
- treatment: billboard
- leg: fort_nelson_bc_ca -> watson_lake_yt_ca
- at_mi: 119
- spoken: Billboard: Northern Rockies Lodge, ahead at Muncho Lake. Floatplanes at the dock, for fishing where no road goes. The truck stays behind.

### Northern Rockies Lodge (eastbound, Watson Lake to Fort Nelson)
- treatment: billboard
- leg: watson_lake_yt_ca -> fort_nelson_bc_ca
- at_mi: 138
- spoken: Billboard: Northern Rockies Lodge, ahead at Muncho Lake. Floatplanes at the dock, for fishing where no road goes. The truck stays behind.

## Liard River Hot Springs -- Alaska Highway km 765, Liard River Hot Springs Provincial Park

Alpha pool 42 to 52 degrees C, the second largest hot springs in Canada, reached by a boardwalk through a warm-water swamp. Open year round; park since 1957. Sources: BC Parks. OpenStreetMap way 621109456. Both signs stand outside the park and outside Muncho Lake's.

### Liard River Hot Springs (westbound, Fort Nelson to Watson Lake)
- treatment: billboard
- leg: fort_nelson_bc_ca -> watson_lake_yt_ca
- at_mi: 180
- spoken: Billboard: Liard River Hot Springs, ahead. Canada's second largest hot springs, down a boardwalk through a warm swamp. Open all year. At forty below, nobody gets out first.

### Liard River Hot Springs (eastbound, Watson Lake to Fort Nelson)
- treatment: billboard
- leg: watson_lake_yt_ca -> fort_nelson_bc_ca
- at_mi: 120
- spoken: Billboard: Liard River Hot Springs, ahead. Canada's second largest hot springs, down a boardwalk through a warm swamp. Open all year. At forty below, nobody gets out first.

## Sign Post Forest -- Watson Lake, Yukon

Started 1942 by Private Carl K. Lindley of the 341st Engineers with a sign for Danville, Illinois. More than 75,000 signs when it became a Yukon Historic Site in June 2014, around 100,000 now (undated). Outdoors, year round. Sources: Town of Watson Lake; Travelweek; Travel Yukon. OpenStreetMap way 494217182.

### Sign Post Forest (westbound, Fort Nelson to Watson Lake)
- treatment: billboard
- leg: fort_nelson_bc_ca -> watson_lake_yt_ca
- at_mi: 309
- spoken: Billboard: The Sign Post Forest, ahead in Watson Lake. An army private put up a sign for Danville, Illinois, in nineteen forty-two. Tens of thousands have followed. Yours is missing.

### Sign Post Forest (eastbound, Whitehorse to Watson Lake)
- treatment: billboard
- leg: whitehorse_yt_ca -> watson_lake_yt_ca
- at_mi: 262
- spoken: Billboard: The Sign Post Forest, ahead in Watson Lake. An army private put up a sign for Danville, Illinois, in nineteen forty-two. Tens of thousands have followed. Yours is missing.

## George Johnston Museum -- Teslin, Yukon

George Johnston, a Teslin Tlingit trapper, trader and photographer, brought the first car to Teslin, a 1928 Chevrolet, by riverboat before there were roads, and drove it on the frozen lake in winter, painted white. The car is in the museum. June 1 to Labour Day. Sources: Heritage Yukon; Yukon Nuggets. OpenStreetMap way 1292774119.

### George Johnston Museum (westbound, Watson Lake to Whitehorse)
- treatment: billboard
- leg: watson_lake_yt_ca -> whitehorse_yt_ca
- at_mi: 152
- spoken: Billboard: The George Johnston Museum, ahead in Teslin. He shipped in Teslin's first car by riverboat in nineteen twenty-eight, before any road. He drove it on the frozen lake.

### George Johnston Museum (eastbound, Whitehorse to Watson Lake)
- treatment: billboard
- leg: whitehorse_yt_ca -> watson_lake_yt_ca
- at_mi: 101
- spoken: Billboard: The George Johnston Museum, ahead in Teslin. He shipped in Teslin's first car by riverboat in nineteen twenty-eight, before any road. He drove it on the frozen lake.

## SS Klondike -- Whitehorse riverbank

Sternwheeler launched 1937, last trip August 26, 1955, National Historic Site since 1967. **Caution:** closed to visitors during a long restoration and lead removal; seen from the grounds only (Travel Yukon). Sources: Parks Canada. OpenStreetMap way 214563318.

### SS Klondike (westbound, Watson Lake to Whitehorse)
- treatment: billboard
- leg: watson_lake_yt_ca -> whitehorse_yt_ca
- at_mi: 254
- spoken: Billboard: The sternwheeler Klondike, ahead on the riverbank in Whitehorse. She made her last trip in nineteen fifty-five and has been parked ever since. Longest layover in the Yukon.

## Miles Canyon -- South of Whitehorse

Basalt canyon on the Yukon River that wrecked Klondike stampeders' boats; the Robert Lowe Suspension Bridge, a footbridge of 1922, crosses it. Year round. Sources: Bridgemeister; ExploreNorth. OpenStreetMap node 5076441325.

### Miles Canyon (westbound, Watson Lake to Whitehorse)
- treatment: billboard
- leg: watson_lake_yt_ca -> whitehorse_yt_ca
- at_mi: 260
- spoken: Billboard: Miles Canyon, ahead near Whitehorse. Its rapids wrecked the boats of the Klondike gold rush. A footbridge crosses it now. Take the bridge.

## Yukon Beringia Interpretive Centre -- Whitehorse, beside the airport

Ice Age Beringia; a full-size cast of the largest woolly mammoth found in North America. Opened 1997. Summer season (May 5 to September 26 in 2026). Sources: Heritage Yukon; Wikipedia. OpenStreetMap way 422415747.

### Yukon Beringia Interpretive Centre (westbound, Watson Lake to Whitehorse)
- treatment: billboard
- leg: watson_lake_yt_ca -> whitehorse_yt_ca
- at_mi: 248
- spoken: Billboard: The Yukon Beringia Interpretive Centre, ahead in Whitehorse. Ice Age Yukon, and a full-size cast of the largest woolly mammoth found in North America. He would not fit in your trailer.

### Yukon Beringia Interpretive Centre (eastbound, Tok to Whitehorse)
- treatment: billboard
- leg: tok_ak_us -> whitehorse_yt_ca
- at_mi: 377
- spoken: Billboard: The Yukon Beringia Interpretive Centre, ahead in Whitehorse. Ice Age Yukon, and a full-size cast of the largest woolly mammoth found in North America. He would not fit in your trailer.

## Yukon Transportation Museum -- Whitehorse, beside the airport

Its DC-3, CF-CPY, is billed as the world's largest weathervane: it turns on its pedestal in about a five-knot wind. Flew 31,851 hours, last flight 1970. Sources: goytm.ca; Atlas Obscura. OpenStreetMap way 158988875.

### Yukon Transportation Museum (eastbound, Tok to Whitehorse)
- treatment: billboard
- leg: tok_ak_us -> whitehorse_yt_ca
- at_mi: 381
- spoken: Billboard: The Yukon Transportation Museum, ahead in Whitehorse. Out front, an old airliner on a pedestal turns to face the wind, billed as the world's largest weathervane. It is never wrong.

## Da Kų Cultural Centre -- Haines Junction, Yukon

Written **Da Kų** (Southern Tutchone for Our House), owned by the Champagne and Aishihik First Nations; opened June 1, 2013; also the Kluane National Park visitor centre. Mid-May to September 30. Sources: Travel Yukon; Mapping the Way. OpenStreetMap way 1410347610. The eastbound sign stands before the Kluane span.

### Da Ku Cultural Centre (westbound, Whitehorse to Tok)
- treatment: billboard
- leg: whitehorse_yt_ca -> tok_ak_us
- at_mi: 86
- spoken: Billboard: Da Kų Cultural Centre, ahead in Haines Junction. The name is Southern Tutchone for Our House, and it belongs to the Champagne and Aishihik First Nations. Guests welcome in summer.

### Da Ku Cultural Centre (eastbound, Tok to Whitehorse)
- treatment: billboard
- leg: tok_ak_us -> whitehorse_yt_ca
- at_mi: 282
- spoken: Billboard: Da Kų Cultural Centre, ahead in Haines Junction. The name is Southern Tutchone for Our House, and it belongs to the Champagne and Aishihik First Nations. Guests welcome in summer.

## Kluane Museum of History -- Burwash Landing, Historic Mile 1093

Formerly the Kluane Museum of Natural History: wildlife dioramas, First Nations clothing and tools, Yukon minerals. May 15 to September 15. Source: Heritage Yukon. OpenStreetMap way 507390559 (still tagged with the old name).

### Kluane Museum of History (westbound, Whitehorse to Tok)
- treatment: billboard
- leg: whitehorse_yt_ca -> tok_ak_us
- at_mi: 163
- spoken: Billboard: The Kluane Museum of History, ahead in Burwash Landing. Yukon wildlife in glass cases, holding very still. The ones outside do not.

### Kluane Museum of History (eastbound, Tok to Whitehorse)
- treatment: billboard
- leg: tok_ak_us -> whitehorse_yt_ca
- at_mi: 206
- spoken: Billboard: The Kluane Museum of History, ahead in Burwash Landing. Yukon wildlife in glass cases, holding very still. The ones outside do not.

## Bridal Veil Falls -- Highway 1 exit 135, east of Chilliwack

A 60 m fall sliding over smooth rock, a half-hour walk return; provincial park since 1965. Source: BC Parks. OpenStreetMap node 930576261.

### Bridal Veil Falls (eastbound, Surrey to Prince George)
- treatment: skip
- leg: surrey_bc_ca -> prince_george_bc_ca
- at_mi: 46
- spoken: Billboard: Bridal Veil Falls, ahead near Chilliwack. A tall fall of water sliding down smooth rock like a veil. Nobody has located the groom.

### Bridal Veil Falls (westbound, Prince George to Surrey)
- treatment: skip
- leg: prince_george_bc_ca -> surrey_bc_ca
- at_mi: 408
- spoken: Billboard: Bridal Veil Falls, ahead near Chilliwack. A tall fall of water sliding down smooth rock like a veil. Nobody has located the groom.

## Fort Hope -- Hope, BC

First Blood (1982) shot its town scenes in Hope; a wooden Rambo statue stands in town. More than 80 chainsaw carvings around the centre. Sources: Wikipedia; CBC; Tourism Hope Cascades and Canyons. OpenStreetMap node 4858150133. Name the town and the carvings; the film's character is a trademark, so the copy should stay nominative.

### Fort Hope (eastbound, Surrey to Prince George)
- treatment: skip
- leg: surrey_bc_ca -> prince_george_bc_ca
- at_mi: 66
- spoken: Billboard: Hope, ahead. More than eighty chainsaw carvings stand around town. Every driver says they are beyond Hope on the way out. The locals have heard it.

### Fort Hope (southbound, Prince George to Surrey)
- treatment: skip
- leg: prince_george_bc_ca -> surrey_bc_ca
- at_mi: 384
- spoken: Billboard: Hope, ahead. More than eighty chainsaw carvings stand around town. Every driver says they are beyond Hope on the way out. The locals have heard it.

## Yale Historic Site -- Yale, Fraser Canyon

Creighton House, and the Church of St. John the Divine (1863), the oldest Anglican church in British Columbia on its original site; gold panning. Yale was the head of navigation in the 1858 rush and the start of the Cariboo Wagon Road. April to October. Sources: Historic Places Days; britishcolumbia.com. OpenStreetMap way 815488646.

### Yale Historic Site (northbound, Surrey to Prince George)
- treatment: skip
- leg: surrey_bc_ca -> prince_george_bc_ca
- at_mi: 79
- spoken: Billboard: Yale Historic Site, ahead. In the gold rush of eighteen fifty-eight the steamboats stopped here and the wagon road began. Gold panning in summer. Keep the day job.

### Yale Historic Site (southbound, Prince George to Surrey)
- treatment: skip
- leg: prince_george_bc_ca -> surrey_bc_ca
- at_mi: 369
- spoken: Billboard: Yale Historic Site, ahead. In the gold rush of eighteen fifty-eight the steamboats stopped here and the wagon road began. Gold panning in summer. Keep the day job.

## Hell's Gate Airtram -- Fraser Canyon, Highway 1

Aerial tram down 157 m into the narrowest point of the Fraser canyon, opened July 21, 1971. May 1 to Canadian Thanksgiving; closed now. Sources: hellsgateairtram.com; Wikipedia. OpenStreetMap way 30689315. Copy should not promise it is open in winter.

### Hell's Gate Airtram (northbound, Surrey to Prince George)
- treatment: skip
- leg: surrey_bc_ca -> prince_george_bc_ca
- at_mi: 96
- spoken: Billboard: Hell's Gate Airtram, ahead in the Fraser Canyon. A cable car down to where the whole river squeezes through its narrowest gap. Summer only. The river never closes.

### Hell's Gate Airtram (southbound, Prince George to Surrey)
- treatment: skip
- leg: prince_george_bc_ca -> surrey_bc_ca
- at_mi: 348
- spoken: Billboard: Hell's Gate Airtram, ahead in the Fraser Canyon. A cable car down to where the whole river squeezes through its narrowest gap. Summer only. The river never closes.

## One Hundred and Eight Mile Heritage Site -- 108 Mile Ranch, Highway 97

Thirteen buildings from the 1860s on, among them Cariboo Wagon Road roadhouses, and the 1908 Clydesdale barn, about 160 feet long, called the largest log structure of its kind left in Canada. Summer. Sources: Cariboo Regional District; Historic Places Canada. OpenStreetMap way 1424667835. Spoken as One Hundred and Eight Mile.

### One Hundred and Eight Mile Heritage Site (northbound, Surrey to Prince George)
- treatment: skip
- leg: surrey_bc_ca -> prince_george_bc_ca
- at_mi: 266
- spoken: Billboard: One Hundred and Eight Mile Heritage Site, ahead. Wagon road roadhouses and a huge log barn built in nineteen oh-eight for Clydesdales. The original horsepower lived well.

### One Hundred and Eight Mile Heritage Site (southbound, Prince George to Surrey)
- treatment: skip
- leg: prince_george_bc_ca -> surrey_bc_ca
- at_mi: 184
- spoken: Billboard: One Hundred and Eight Mile Heritage Site, ahead. Wagon road roadhouses and a huge log barn built in nineteen oh-eight for Clydesdales. The original horsepower lived well.

## Barkerville Historic Town -- Off route: 80 km east of Quesnel on Highway 26

Gold rush town: Billy Barker struck gold on Williams Creek on August 17, 1862; burned 1868 and rebuilt in six weeks; more than 125 heritage buildings. Main season May 30 to September 13 in 2026; the gate is open year round. Sources: barkerville.ca; Dictionary of Canadian Biography. OpenStreetMap way 94666787. The only attraction off the route: both signs stand before the Quesnel turn, the way Barkerville is actually signed.

### Barkerville Historic Town (northbound, Surrey to Prince George)
- treatment: skip
- leg: surrey_bc_ca -> prince_george_bc_ca
- at_mi: 395
- spoken: Billboard: Barkerville Historic Town, east of Quesnel. Billy Barker struck gold in eighteen sixty-two. The town burned in eighteen sixty-eight and was rebuilt in six weeks. Contractors, take note.

### Barkerville Historic Town (southbound, Prince George to Surrey)
- treatment: skip
- leg: prince_george_bc_ca -> surrey_bc_ca
- at_mi: 48
- spoken: Billboard: Barkerville Historic Town, east of Quesnel. Billy Barker struck gold in eighteen sixty-two. The town burned in eighteen sixty-eight and was rebuilt in six weeks. Contractors, take note.
