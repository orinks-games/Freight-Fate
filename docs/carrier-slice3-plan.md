# Carrier slice 3 plan — regional, local, and Alaska carriers

Status: **approved; names final.** The data lands in the carriers.json
data PR (`wip/carrier-s3-data`). The realism re-cut approved the structure;
the names are final after one swap (Catawba Ridge Transport became
Meherrin Transport, §10). The screen is in §10 and every decision is in
§11.

Parent: ROADMAP "Career carriers (slices 2–4)" — the row "The 16 regionals
plus an Alaska regional". Slices 1 and 2 (carrier-owned terminals, the
truthful parked-at line, the carrier-first start picker) are merged into
`feat/career-2.0`.

Branch: planned on `wip/carrier-s3-regionals` off `feat/career-2.0` at
`6e852f24`; that plan and the data land together on `wip/carrier-s3-data`.

---

## 0. Tier rules (approved)

| Tier | Hires | Runs (run band) | Notes |
| --- | --- | --- | --- |
| Local | within ~50 air mi of a terminal, same country | 25-150 mi | Metro cartage and drayage |
| Regional | within 250 air mi of any terminal, same country | 150-600 mi, inside its lane area | `hiring_radius_mi = 250` |
| National | every lower-48 city | 400-3000 mi | Unchanged: Northstar, Great Lakes Training, Summit Value |

The tighter of the run band and the level cap wins (today only the band max
folds into the distance cap; the band minimum stays deferred to slice 4).

Method for every number below: haversine air miles between world city
`lat`/`lon` in `data/world_data/{us,ca}/cities.json` (640 cities: 625
lower 48, 8 AK, 5 BC, 2 YT), same rule as `Carrier::hiring_terminal_city`.
Every terminal key listed here exists as a map city key today.

---

## 1. Carriers at a glance

Prairie Link Regional (`prairie_link`, KC / Omaha / Wichita) stays as it is.
The count is settled: **sixteen new lower-48 regionals plus Prairie Link,
17 lower-48 regionals in all**, one Alaska regional, and three locals
(Chicago, LA, Anchorage). Names below are final (§10).

| # | Name | Key | Tier | Terminal city keys | Cities in footprint | Only regional for |
| --- | --- | --- | --- | --- | --- | --- |
| 1 | Androscoggin Freight | `androscoggin_freight` | regional | `boston_ma_us`, `hartford_ct_us`, `portland_me_us`, `albany_ny_us` | 40 | 5 |
| 2 | Kittatinny Crossroads Freight | `kittatinny_crossroads` | regional | `harrisburg_pa_us`, `baltimore_md_us`, `pittsburgh_pa_us`, `newark_nj_us` | 85 | 0 |
| 3 | Meherrin Transport | `meherrin_transport` | regional | `charlotte_nc_us`, `greensboro_nc_us`, `richmond_va_us` | 82 | 7 |
| 4 | Alapaha Regional | `alapaha_regional` | regional | `atlanta_ga_us`, `birmingham_al_us`, `savannah_ga_us` | 82 | 0 |
| 5 | Sunpine Freight Lines | `sunpine_freight` | regional | `jacksonville_fl_us`, `orlando_fl_us`, `miami_fl_us` | 38 | 14 |
| 6 | Barataria Carriers | `barataria_carriers` | regional | `new_orleans_la_us`, `baton_rouge_la_us`, `mobile_al_us`, `jackson_ms_us` | 55 | 5 |
| 7 | Lone Mesa Freight | `lone_mesa_freight` | regional | `dallas_tx_us`, `houston_tx_us`, `san_antonio_tx_us`, `mcallen_tx_us` | 64 | 19 |
| 8 | Saint Francis River Lines | `saint_francis_river` | regional | `memphis_tn_us`, `little_rock_ar_us`, `nashville_tn_us` | 94 | 1 |
| 9 | Olentangy Valley Freight | `olentangy_valley` | regional | `columbus_oh_us`, `cincinnati_oh_us`, `louisville_ky_us`, `indianapolis_in_us`, `detroit_mi_us` | 101 | 13 |
| 10 | Loonwater Regional | `loonwater_regional` | regional | `minneapolis_mn_us`, `fargo_nd_us`, `bismarck_nd_us`, `duluth_mn_us`, `green_bay_wi_us` | 73 | 37 |
| 11 | Verdigris Transport | `verdigris_transport` | regional | `oklahoma_city_ok_us`, `tulsa_ok_us`, `amarillo_tx_us`, `lubbock_tx_us` | 85 | 9 |
| 12 | Musselshell Freight Lines | `musselshell_freight` | regional | `denver_co_us`, `cheyenne_wy_us`, `billings_mt_us` | 50 | 29 |
| 13 | Pinyon Basin Transport | `pinyon_basin` | regional | `salt_lake_city_ut_us`, `boise_id_us`, `las_vegas_nv_us` | 66 | 22 |
| 14 | San Simon Freight | `san_simon_freight` | regional | `phoenix_az_us`, `tucson_az_us`, `albuquerque_nm_us`, `el_paso_tx_us` | 51 | 17 |
| 15 | Tehachapi Motor Lines | `tehachapi_motor_lines` | regional | `los_angeles_ca_us`, `fresno_ca_us`, `sacramento_ca_us` | 53 | 20 |
| 16 | Chehalis Freight Lines | `chehalis_freight` | regional | `seattle_wa_us`, `portland_or_us`, `spokane_wa_us`, `medford_or_us` | 54 | 36 |
| AK | Chatanika Freight Lines | `chatanika_freight` | regional (AK) | `anchorage_ak_us`, `fairbanks_ak_us` | 8 (all AK) | 8 |
| L1 | Des Plaines River Cartage | `des_plaines_cartage` | local | `chicago_il_us`, `gary_in_us`, `aurora_il_us` | 4 | — |
| L2 | Basin Harbor Drayage | `basin_harbor_drayage` | local | `los_angeles_ca_us`, `riverside_ca_us` | 6 | — |
| L3 | Knik Arm Cartage | `knik_arm_cartage` | local (AK) | `anchorage_ak_us` | 3 | — |

Coverage with all sixteen plus Prairie Link, including the Green Bay
terminal on Loonwater Regional: **625 of 625** lower-48 cities have at least
one regional (248 have exactly one, 257 two, 101 three, 19 four). No city is
left with nationals only (§6).

Names are fictional carriers under real geographic names, following how
real regional carriers are named: a river, valley, mountain range, highway,
or the region itself (no plants, animals, seaweed, or grasses, and no "Line
Freight" pattern). Each carrier's **Name** line says what the name refers
to. §10 has the screen for every name.

---

## 2. Regional carriers

### 1. Androscoggin Freight (`androscoggin_freight`)

- **Name:** Androscoggin River, which runs from the White Mountains of NH through western Maine to Merrymeeting Bay.
- **Tier:** regional; hires within 250 air mi of any terminal, same country; runs 150-600 mi.
- **Terminals:** `boston_ma_us`, `hartford_ct_us`, `portland_me_us`, `albany_ny_us` (widest terminal pair 190 air mi).
- **Lane area:** New England and the upper Hudson: Boston, Hartford, Portland ME, and Albany terminals; lanes Maine to the NYC metro and west to the Hudson.
- **Hiring footprint:** 40 map cities; states NY 7, PA 7, NJ 5, MA 4, CT 4, NH 3, VT 3, ME 3, RI 2, DE 2.
- **Only regional for:** Bangor ME, Lewiston ME, Portland ME, Burlington VT, Montpelier VT.

### 2. Kittatinny Crossroads Freight (`kittatinny_crossroads`)

- **Name:** Kittatinny Ridge (Kittatinny Mountain), the Appalachian ridge line across NJ and PA; the Delaware Water Gap cuts it.
- **Tier:** regional; hires within 250 air mi of any terminal, same country; runs 150-600 mi.
- **Terminals:** `harrisburg_pa_us`, `baltimore_md_us`, `pittsburgh_pa_us`, `newark_nj_us` (widest terminal pair 306 air mi).
- **Lane area:** Mid-Atlantic: Harrisburg, Baltimore, Pittsburgh, and Newark terminals; lanes along I-76/I-78/I-81/I-95 between the NJ ports, the PA distribution belt, and the Chesapeake.
- **Hiring footprint:** 85 map cities; states VA 13, OH 12, PA 11, NY 9, WV 6, NJ 5, MA 4, CT 4, MD 4, KY 4, MI 3, NH 3, RI 2, DE 2, DC 1, NC 1, VT 1.
- **Only regional for:** none (every city here is also reached by another regional).

### 3. Meherrin Transport (`meherrin_transport`)

- **Name:** Meherrin River, on the Virginia and North Carolina line between
  Richmond and Greensboro, inside this carrier's footprint.
- **Tier:** regional; hires within 250 air mi of any terminal, same country; runs 150-600 mi.
- **Terminals:** `charlotte_nc_us`, `greensboro_nc_us`, `richmond_va_us` (widest terminal pair 248 air mi).
- **Lane area:** Carolinas and Virginia: Charlotte, Greensboro, and Richmond terminals; lanes along I-85/I-40/I-95 from Richmond to upstate SC.
- **Hiring footprint:** 82 map cities; states VA 15, NC 14, GA 11, PA 7, SC 7, WV 6, KY 6, TN 5, MD 4, NJ 4, DE 2, DC 1.
- **Only regional for:** Durham NC, Greensboro NC, Greenville NC, Jacksonville NC, New Bern NC, Raleigh NC, Winston-Salem NC.

### 4. Alapaha Regional (`alapaha_regional`)

- **Name:** Alapaha River in south Georgia, a tributary of the Suwannee.
- **Tier:** regional; hires within 250 air mi of any terminal, same country; runs 150-600 mi.
- **Terminals:** `atlanta_ga_us`, `birmingham_al_us`, `savannah_ga_us` (widest terminal pair 347 air mi).
- **Lane area:** Deep South hub: Atlanta, Birmingham, and Savannah terminals; lanes between the Port of Savannah, metro Atlanta DCs, and Birmingham.
- **Hiring footprint:** 82 map cities; states GA 20, TN 13, AL 12, FL 11, MS 10, SC 7, NC 6, KY 2, VA 1.
- **Only regional for:** none (every city here is also reached by another regional).

### 5. Sunpine Freight Lines (`sunpine_freight`)

- **Name:** Coined (kept by the re-cut): Florida sun and pine flatwoods.
- **Tier:** regional; hires within 250 air mi of any terminal, same country; runs 150-600 mi.
- **Terminals:** `jacksonville_fl_us`, `orlando_fl_us`, `miami_fl_us` (widest terminal pair 328 air mi).
- **Lane area:** Florida: Jacksonville, Orlando, and Miami terminals; lanes along I-95/I-75/I-4 inside the peninsula and up to south Georgia.
- **Hiring footprint:** 38 map cities; states FL 23, GA 13, SC 1, AL 1.
- **Only regional for:** Cape Coral FL, Coral Springs FL, Fort Myers FL, Key West FL, Lakeland FL, Miami FL, Naples FL, North Port FL, Palm Bay FL, Port Saint Lucie FL, Sarasota FL, Spring Hill FL, Tampa FL, West Palm Beach FL.

### 6. Barataria Carriers (`barataria_carriers`)

- **Name:** Barataria Bay and the Barataria Basin, south of New Orleans between the Mississippi and Bayou Lafourche.
- **Tier:** regional; hires within 250 air mi of any terminal, same country; runs 150-600 mi.
- **Terminals:** `new_orleans_la_us`, `baton_rouge_la_us`, `mobile_al_us`, `jackson_ms_us` (widest terminal pair 188 air mi).
- **Lane area:** Central Gulf Coast: New Orleans, Baton Rouge, Mobile, and Jackson MS terminals; lanes along I-10/I-12/I-55/I-65 between the river ports and chemical belt.
- **Hiring footprint:** 55 map cities; states LA 12, MS 12, AL 10, AR 8, FL 4, GA 4, TX 3, TN 2.
- **Only regional for:** Baton Rouge LA, Hammond LA, Houma LA, New Orleans LA, Gulfport MS.

### 7. Lone Mesa Freight (`lone_mesa_freight`)

- **Name:** Coined landscape name (kept by the re-cut): a lone Texas mesa.
- **Tier:** regional; hires within 250 air mi of any terminal, same country; runs 150-600 mi.
- **Terminals:** `dallas_tx_us`, `houston_tx_us`, `san_antonio_tx_us`, `mcallen_tx_us` (widest terminal pair 462 air mi).
- **Lane area:** Texas Triangle and the border: Dallas, Houston, San Antonio, and McAllen terminals; lanes inside the I-35/I-45/I-10 triangle and down to the Rio Grande Valley crossings.
- **Hiring footprint:** 64 map cities; states TX 40, OK 13, LA 7, AR 4.
- **Only regional for:** Austin TX, Brownsville TX, College Station TX, Corpus Christi TX, Del Rio TX, Eagle Pass TX, Houston TX, Kerrville TX, Killeen TX, Lampasas TX, Laredo TX, Marble Falls TX, McAllen TX, Palestine TX, San Antonio TX, Temple TX, Uvalde TX, Victoria TX, Waco TX.

### 8. Saint Francis River Lines (`saint_francis_river`)

- **Name:** Saint Francis River, from the Missouri bootheel down eastern Arkansas to the Mississippi below Memphis. Spoken "Saint" in full so a screen reader does not read "St." as "street".
- **Tier:** regional; hires within 250 air mi of any terminal, same country; runs 150-600 mi.
- **Terminals:** `memphis_tn_us`, `little_rock_ar_us`, `nashville_tn_us` (widest terminal pair 325 air mi).
- **Lane area:** Mid-South: Memphis, Little Rock, and Nashville terminals; lanes along I-40/I-55/I-24 around the Memphis rail and river hub.
- **Hiring footprint:** 94 map cities; states AR 14, TN 12, KY 12, MS 10, OK 9, MO 6, GA 6, AL 6, LA 5, IN 5, TX 3, IL 3, OH 1, NC 1, KS 1.
- **Only regional for:** Poplar Bluff MO.

### 9. Olentangy Valley Freight (`olentangy_valley`)

- **Name:** Olentangy River valley, which runs through Columbus OH to the Scioto.
- **Tier:** regional; hires within 250 air mi of any terminal, same country; runs 150-600 mi.
- **Terminals:** `columbus_oh_us`, `cincinnati_oh_us`, `louisville_ky_us`, `indianapolis_in_us`, `detroit_mi_us` (widest terminal pair 316 air mi).
- **Lane area:** Ohio Valley and lower Great Lakes: Columbus, Cincinnati, Louisville, Indianapolis, and Detroit terminals; lanes along I-70/I-71/I-65/I-75 and into Chicagoland and lower Michigan.
- **Hiring footprint:** 101 map cities; states IN 15, KY 14, OH 13, IL 12, TN 11, MI 11, VA 6, WV 6, PA 4, MO 2, NY 2, WI 2, AL 1, GA 1, MD 1.
- **Only regional for:** Bloomington IL, Champaign IL, Decatur IL, Galesburg IL, Peoria IL, Springfield IL, Anderson IN, Fort Wayne IN, Indianapolis IN, Kokomo IN, Lafayette IN, Muncie IN, Richmond IN.

### 10. Loonwater Regional (`loonwater_regional`)

- **Name:** Coined North Woods lake name (kept by the re-cut).
- **Tier:** regional; hires within 250 air mi of any terminal, same country; runs 150-600 mi.
- **Terminals:** `minneapolis_mn_us`, `fargo_nd_us`, `bismarck_nd_us`, `duluth_mn_us`, `green_bay_wi_us` (widest terminal pair 636 air mi).
- **Lane area:** Upper Midwest and the western Great Lakes: Minneapolis, Fargo, Bismarck, Duluth, and Green Bay terminals; lanes along I-94/I-35/I-29/I-41/I-43 across MN, the Dakotas, Wisconsin, and the Upper Peninsula, and down the Mississippi to the Quad Cities.
- **Span note:** the Green Bay terminal makes Bismarck-Green Bay the widest pair of any regional (636 air mi, past the 600 mi run band max). No single run spans both ends; the terminals share one lane area the way the four approved wide spans do. KEEP (resolved): terminal span is not run length, and the 600 mi band applies to loads.
- **Hiring footprint:** 73 map cities; states MI 13, WI 12, MN 12, IA 11, SD 8, ND 8, IL 3, IN 3, MT 3.
- **Only regional for:** Davenport IA, Dubuque IA, Escanaba MI, Houghton MI, Iron Mountain MI, Marquette MI, Sault Ste. Marie MI, Bemidji MN, Duluth MN, Grand Rapids MN, Hibbing MN, Minneapolis MN, Rochester MN, St. Cloud MN, Willmar MN, Winona MN, Bismarck ND, Devils Lake ND, Dickinson ND, Fargo ND, Grand Forks ND, Jamestown ND, Minot ND, Williston ND, Aberdeen SD, Pierre SD, Watertown SD, Chippewa Falls WI, Eau Claire WI, Fond du Lac WI, Green Bay WI, La Crosse WI, Madison WI, Oshkosh WI, Rice Lake WI, Sheboygan WI, Wausau WI.

### 11. Verdigris Transport (`verdigris_transport`)

- **Name:** Verdigris River, KS and OK; its lower reach carries the McClellan-Kerr channel to the Tulsa Port of Catoosa.
- **Tier:** regional; hires within 250 air mi of any terminal, same country; runs 150-600 mi.
- **Terminals:** `oklahoma_city_ok_us`, `tulsa_ok_us`, `amarillo_tx_us`, `lubbock_tx_us` (widest terminal pair 377 air mi).
- **Lane area:** Southern Plains: Oklahoma City, Tulsa, Amarillo, and Lubbock terminals; lanes along I-40/I-44/I-35/I-27 and into the Permian edge.
- **Hiring footprint:** 85 map cities; states TX 30, OK 17, KS 12, AR 10, NM 9, MO 4, CO 3.
- **Only regional for:** Amarillo TX, Big Spring TX, Clarendon TX, Dumas TX, Lubbock TX, Midland TX, Pampa TX, Plainview TX, Stratford TX.

### 12. Musselshell Freight Lines (`musselshell_freight`)

- **Name:** Musselshell River in central Montana, north of the Billings terminal.
- **Tier:** regional; hires within 250 air mi of any terminal, same country; runs 150-600 mi.
- **Terminals:** `denver_co_us`, `cheyenne_wy_us`, `billings_mt_us` (widest terminal pair 454 air mi).
- **Lane area:** Front Range and high plains: Denver, Cheyenne, and Billings terminals; lanes along I-25/I-80/I-90 from Colorado Springs to Montana.
- **Hiring footprint:** 50 map cities; states CO 18, MT 11, WY 10, NE 4, SD 3, ID 1, KS 1, NM 1, OK 1.
- **Only regional for:** Burlington CO, Colorado Springs CO, Denver CO, Edwards CO, Fort Collins CO, Glenwood Springs CO, Limon CO, Silverthorne CO, Billings MT, Bozeman MT, Glasgow MT, Great Falls MT, Havre MT, Helena MT, Shelby MT, North Platte NE, Ogallala NE, Scottsbluff NE, Sidney NE, Hot Springs SD, Buffalo WY, Casper WY, Cheyenne WY, Gillette WY, Laramie WY, Lusk WY, Rawlins WY, Sheridan WY, Wheatland WY.

### 13. Pinyon Basin Transport (`pinyon_basin`)

- **Name:** Pinyon-juniper Great Basin country (kept by the re-cut).
- **Tier:** regional; hires within 250 air mi of any terminal, same country; runs 150-600 mi.
- **Terminals:** `salt_lake_city_ut_us`, `boise_id_us`, `las_vegas_nv_us` (widest terminal pair 517 air mi).
- **Lane area:** Great Basin: Salt Lake City, Boise, and Las Vegas terminals; lanes along I-15/I-80/I-84 between the Wasatch Front, southern Idaho, and Las Vegas.
- **Hiring footprint:** 66 map cities; states CA 18, AZ 11, UT 10, NV 10, ID 6, OR 4, WA 2, MT 2, CO 2, WY 1.
- **Only regional for:** Boise ID, Pocatello ID, Twin Falls ID, Alamo NV, Battle Mountain NV, Elko NV, Ely NV, Eureka NV, Wells NV, West Wendover NV, Winnemucca NV, Ontario OR, Cedar City UT, Green River UT, Logan UT, Moab UT, Nephi UT, Ogden UT, Provo UT, Richfield UT, Saint George UT, Salt Lake City UT.

### 14. San Simon Freight (`san_simon_freight`)

- **Name:** San Simon Valley on the AZ/NM line, which I-10 crosses between Tucson and El Paso.
- **Tier:** regional; hires within 250 air mi of any terminal, same country; runs 150-600 mi.
- **Terminals:** `phoenix_az_us`, `tucson_az_us`, `albuquerque_nm_us`, `el_paso_tx_us` (widest terminal pair 345 air mi).
- **Lane area:** Desert Southwest: Phoenix, Tucson, Albuquerque, and El Paso terminals; lanes along I-10/I-17/I-25/I-40 and the Nogales crossing.
- **Hiring footprint:** 51 map cities; states AZ 22, NM 14, CO 7, TX 5, CA 3.
- **Only regional for:** Casa Grande AZ, Douglas AZ, Globe AZ, Holbrook AZ, Nogales AZ, Payson AZ, Phoenix AZ, Show Low AZ, Sierra Vista AZ, Tucson AZ, Winslow AZ, Albuquerque NM, Farmington NM, Gallup NM, Las Cruces NM, Socorro NM, El Paso TX.

### 15. Tehachapi Motor Lines (`tehachapi_motor_lines`)

- **Name:** Tehachapi Mountains and Tehachapi Pass (CA-58), at the south
  end of the San Joaquin Valley inside the lane area. The main Los Angeles
  to Fresno lane runs I-5 over Tejon Pass, not CA-58; the name is for the
  range in the lane area, not a claim about the LA-Fresno route.
- **Tier:** regional; hires within 250 air mi of any terminal, same country; runs 150-600 mi.
- **Terminals:** `los_angeles_ca_us`, `fresno_ca_us`, `sacramento_ca_us` (widest terminal pair 361 air mi).
- **Lane area:** California and Reno: Los Angeles, Fresno, and Sacramento terminals; lanes along I-5/CA-99/I-80 from the LA basin through the Central Valley to Reno.
- **Hiring footprint:** 53 map cities; states CA 43, NV 7, AZ 3.
- **Only regional for:** Fairfield CA, Fresno CA, Merced CA, Modesto CA, Oxnard CA, Sacramento CA, Salinas CA, San Diego CA, San Francisco CA, San Jose CA, San Luis Obispo CA, Santa Barbara CA, Santa Maria CA, Santa Rosa CA, Stockton CA, Austin NV, Carson City NV, Fallon NV, Fernley NV, Reno NV.

### 16. Chehalis Freight Lines (`chehalis_freight`)

- **Name:** Chehalis River in southwest Washington; I-5 follows it between Seattle and Portland.
- **Tier:** regional; hires within 250 air mi of any terminal, same country; runs 150-600 mi.
- **Terminals:** `seattle_wa_us`, `portland_or_us`, `spokane_wa_us`, `medford_or_us` (widest terminal pair 454 air mi).
- **Lane area:** Pacific Northwest: Seattle, Portland, Spokane, and Medford terminals; lanes along I-5/I-84/I-90 from the Canadian line to southern Oregon and the Idaho panhandle.
- **Hiring footprint:** 54 map cities; states OR 20, WA 14, CA 11, ID 5, MT 4.
- **Only regional for:** Crescent City CA, Coeur d'Alene ID, Kellogg ID, Sandpoint ID, Kalispell MT, Libby MT, Superior MT, Albany OR, Astoria OR, Bend OR, Coos Bay OR, Corvallis OR, Eugene OR, Grants Pass OR, Klamath Falls OR, McMinnville OR, Medford OR, Newberg OR, Newport OR, Portland OR, Roseburg OR, Salem OR, The Dalles OR, Woodburn OR, Bellingham WA, Blaine WA, Everett WA, Longview WA, Moses Lake WA, Olympia WA, Port Angeles WA, Seattle WA, Spokane WA, Tacoma WA, Wenatchee WA, Yakima WA.

---

## 3. Local carriers

Chicago and LA are KEEP (resolved). The Anchorage local is new in this pass
(§4).

### L1. Des Plaines River Cartage (`des_plaines_cartage`)

- **Name:** Des Plaines River, which runs through the western Chicago suburbs to Joliet.
- **Tier:** local; hires within 50 air mi of a terminal; runs 25-150 mi.
- **Terminals:** `chicago_il_us`, `gary_in_us`, `aurora_il_us`.
- **Hiring footprint:** Chicago, Aurora, Gary, Kenosha.
- **Lane area:** Chicagoland rail ramps and DCs out to 150 mi — 28 map
  cities in band, including Milwaukee, Rockford, Peoria, South Bend, Fort
  Wayne, Indianapolis edge, Madison, and Grand Rapids.

### L2. Basin Harbor Drayage (`basin_harbor_drayage`)

- **Name:** Coined (kept by the re-cut): the LA basin and its harbor.
- **Tier:** local; hires within 50 air mi of a terminal; runs 25-150 mi.
- **Terminals:** `los_angeles_ca_us`, `riverside_ca_us`.
- **Hiring footprint:** Los Angeles, Riverside, Santa Ana, Valencia,
  Lancaster, Victorville.
- **Lane area:** Inland Empire DCs and metro cartage out to 150 mi — 17 map
  cities in band, including San Diego, Bakersfield, Barstow, Oxnard, and
  Indio. Port drayage into the LA/Long Beach marine terminals needs a TWIC
  card; that waits for real drayage loads (ROADMAP row, §9).

---

## 4. Alaska carriers and the picker rule

### Chatanika Freight Lines (`chatanika_freight`)

- **Name:** Chatanika River, Interior Alaska, north of Fairbanks along the Steese Highway.
- **Tier:** regional; `hiring_radius_mi = 250`, same country.
- **Terminals:** `anchorage_ak_us`, `fairbanks_ak_us`.
- **Hiring footprint:** all 8 AK map cities — Anchorage 0, Fairbanks 0,
  Wasilla 29, Palmer 37, Nenana 45, Healy 77, Glennallen 157, Tok 176 air
  mi from the nearer terminal.
- **Lane area: inside Alaska only.** The Parks, Glenn, Tok Cutoff, and
  Richardson corridors between Anchorage, Mat-Su, Fairbanks, and Tok. No
  loads leave AK. ALCAN through-freight is future work (§9).

### L3. Knik Arm Cartage (`knik_arm_cartage`)

- **Name:** Knik Arm of Cook Inlet, between Anchorage and the Mat-Su valley.
- **Tier:** local; hires within 50 air mi of its terminal, same country;
  runs 25-150 mi.
- **Terminal:** `anchorage_ak_us`.
- **Hiring footprint:** Anchorage (0 air mi), Wasilla (29), Palmer (37).
  The next AK city, Glennallen, is 157 mi out.
- **Run footprint (25-150 mi):** Wasilla 29 and Palmer 37 are the only map
  cities in band. Anchorage itself is under 25 mi, and every other AK city
  is past 150 (Glennallen 157, Healy 186, Nenana 233, Fairbanks 259, Tok
  266). KEEP thin (resolved): the carrier's main job is to be the
  Anchorage-area employer that is not Chatanika Freight Lines. The next AK
  map adds for an Anchorage local, measured by road rather than air, are
  the Kenai Peninsula (Soldotna/Kenai, about 147 road mi) and Seward (about
  125 road mi). Whittier waits until the Anton Anderson Memorial Tunnel
  schedule and its hazmat limits are modeled. ROADMAP debt row.
- **Lane area:** Port of Alaska and Ted Stevens cargo to Anchorage and
  Mat-Su distribution.

### How AK homes interact with the picker

- A home base is offerable when some carrier hires there
  (`is_offerable_home_city`). Nationals hire only in the lower 48, so today
  no AK city qualifies. With Chatanika Freight Lines in `data/carriers.json`, all
  eight AK cities become offerable. Anchorage, Wasilla, and Palmer list
  Chatanika Freight Lines and Knik Arm Cartage; the other five list Chatanika Freight Lines
  only.
- **BC and YT stay blocked with no code change:** regionals and locals hire
  in the same country only, and the nearest AK terminal is 490 air mi from
  Whitehorse and 688+ from every other BC/YT city. No other carrier hires
  there.
- No lower-48 carrier reaches AK (nationals stop at the lower 48, and no
  lower-48 terminal is within 250 mi), and neither AK carrier reaches a
  lower-48 city, so the two carrier lists never mix.
- Tests that pin Healy and Anchorage as not offerable flip to offerable;
  Whitehorse and Surrey stay excluded.

---

## 5. Termination and solvency fallback (rule change)

**Before this slice:** `check_carrier_termination` (`states/city/terminal.rs`) and the
two solvency paths (`solvency.rs`, `set_carrier` with
`LAST_CHANCE_CARRIER_KEY`) always move the driver to Great Lakes Training
(`enforcement::LAST_CHANCE_CARRIER_KEY`). `change_carrier` then makes a
Great Lakes Training terminal the driver's home terminal. For an Alaska
driver that means Milwaukee, which is wrong: **a fired driver never moves to
Milwaukee.**

**New rule.** The fallback carrier is picked from the driver's home city
(`home_city`, else `home_terminal_city`):

1. Candidates are carriers that hire in the home city (the same test as
   `is_offerable_home_city`), **never the carrier that just fired the
   driver.**
2. Prefer a regional or local that hires there, nearest hiring terminal
   first (ties by carrier key, so the pick is stable).
3. Otherwise, in the lower 48, Great Lakes Training, which hires in every
   lower-48 city and stays the designated last-chance carrier (it never
   fires; `carrier_termination_due` already skips it). Other nationals do
   not take a driver another carrier just fired.
4. Otherwise (no non-firing carrier hires there), the driver stays home and
   unassigned: no carrier and no assigned truck, at the home city, until
   they apply somewhere.

The same pick serves the solvency path (an owner-operator who loses the
business), minus the "not the firing carrier" filter.

**Sufferance.** The carrier that takes on a let-go driver does so knowing
the record, so the same record does not end that seat again: the driver is
kept on sufferance at that carrier (`DrivingRecord.sufferance_carrier_key`,
as Great Lakes Training always has) and their debt stops at the ceiling
like a Great Lakes Training driver's.

**Alaska outcomes:**

| Home | Fired by | Fallback |
| --- | --- | --- |
| Anchorage, Wasilla, Palmer | Chatanika Freight Lines | Knik Arm Cartage |
| Anchorage, Wasilla, Palmer | Knik Arm Cartage | Chatanika Freight Lines |
| Fairbanks, Nenana, Healy, Glennallen, Tok | Chatanika Freight Lines | none: home and unassigned |

**Lower 48:** the rule covers lower-48 fallbacks too. Every lower-48 city
has at least one regional (§6), so a driver fired by a national gets a
regional in the home city. The 248 cities with exactly one regional drop to
Great Lakes Training only when that regional is the one that fired them;
Chicago and LA homes also have a local to try first.

**If the rule does not land in this slice,** an AK termination keeps the
driver home and unassigned; it never moves them to Milwaukee. That minimum
guard (a same-country check before the Great Lakes Training fallback) is
small enough to land with the AK carriers either way.

Code touch points: `enforcement::LAST_CHANCE_CARRIER_KEY` (stays, as the
lower-48 last resort), `check_carrier_termination` in
`states/city/terminal.rs` (pick instead of hard-coding, and a new spoken
line naming the carrier that takes the driver on or saying they are home
without a seat), the two `set_carrier` calls in `solvency.rs`, and the
tests that pin Great Lakes Training (`states_city.rs`, `solvency/tests.rs`,
`carriers.rs`). An unassigned driver needs a save and hub state the menus
read clearly, which is the biggest part of the change.

---

## 6. Cities left with nationals only — resolved, none left

The Green Bay terminal on Loonwater Regional closes all six cities the
first pass left with nationals only:

| City | Air mi from Green Bay | Regional after the fix |
| --- | --- | --- |
| Davenport, IA | 244 | Loonwater Regional |
| Green Bay, WI | 0 | Loonwater Regional |
| Fond du Lac, WI | 56 | Loonwater Regional |
| Sheboygan, WI | 55 | Loonwater Regional |
| Escanaba, MI | 97 | Loonwater Regional |
| Sault Ste. Marie, MI | 224 | Loonwater Regional |

Davenport closes, but only by 6 mi, and only Loonwater reaches it. If a
later map or radius change drops it, the smallest real fix is a Quad Cities
(`davenport_ia_us`) terminal on Prairie Link or Loonwater, or a Des Moines
terminal on Prairie Link. The coverage test (§7) would catch the drop.

---

## 7. Coverage test (lands with the data)

`crates/ff-core/tests/it/data_world.rs` (or `models/carriers.rs` tests):

1. **National floor:** every lower-48 map city is hired into by at least
   one national (`tier == "national"`), and no national hires in AK, BC, or
   YT.
2. **Regional coverage:** every lower-48 city is hired into by at least one
   regional. The nationals-only list is empty, and the test pins it empty,
   so a map or carrier change that opens a gap fails loudly.
3. **Same country and radius:** every regional or local hire is within its
   `hiring_radius_mi` of a terminal in the same country.
4. **Alaska:** every AK city is offerable through Chatanika Freight Lines (and
   Anchorage, Wasilla, and Palmer through Knik Arm Cartage too); every
   BC/YT city is not offerable.
5. **Terminals:** every `terminal_city_key` is a world city (already
   validated) and no two carriers share a display name.
6. **Picker agreement:** every offerable city has at least one start option
   (already covered by the slice 2 app test; extended to the new carriers).
7. **Fallback:** for every offerable home city and every carrier hiring
   there, the fallback pick is never that carrier and never a carrier in
   another country; for AK homes it is never Great Lakes Training.
8. **No AK driver on a lower-48 carrier:** no path (new-career picker,
   termination fallback, solvency fallback, or a later carrier change)
   ever assigns a driver whose home is in AK to a carrier whose hiring
   terminal is in the lower 48. With no AK carrier available, the result is
   home and unassigned.

---

## 8. Implementation outline (after the cut)

1. `data/carriers.json`: the 20 new entries (16 regionals, Chatanika Freight Lines,
   3 locals). Pay and dispatch numbers per tier, reusing Prairie Link's
   regional shape and a local shape (higher `stop_pay`, a little more
   deadline slack; no `short_haul_bias`, whose spoken summary says
   "training loads"), tuned in review.
2. Tier `local` in `models/carriers.rs` (a non-national with
   `hiring_radius_mi = 50`; the regional code path already handles it).
3. One company start option per new carrier in
   `models/start_options.rs`, so the carrier-first picker lists them.
4. The fallback rule (§5), or at least the AK guard.
5. Flip the AK picker tests; keep BC/YT tests.
6. The §7 coverage test.
7. CHANGELOG bullet; ROADMAP rows checked off.

---

## 9. Future work (not this slice)

- **ALCAN through-freight** is a later long-haul lane program, not part of
  Chatanika Freight Lines: through moves only between AK and the lower 48, no
  domestic BC/YT moves (Canadian cabotage), and ACE/ACI eManifest filed at
  each border crossing.
- **TWIC for port drayage:** drayage into the LA/Long Beach marine
  terminals needs a TWIC card. Tracked as a ROADMAP row for when drayage
  loads are real.

---

## 10. Trademark screen

This is a **screen, not legal clearance.** It looks for names that match or
are confusingly close to a real motor carrier or a live transportation
mark. A name is **flagged** when its distinctive word or phrase is shared
with an FMCSA-registered entity whose name signals freight, trucking,
transport, logistics, towing, or rail, or with a live USPTO mark in class
039 (transportation), or when the web shows a carrier trading under it.
A **weak hit** is a shared word with a non-freight business, or a place
suffix ("... of Des Plaines"); weak hits are recorded but do not block.

Sources, all run 2026-09-26:

- **FMCSA SAFER company-name search**
  (`safer.fmcsa.dot.gov/keywordx.asp?searchstring=*TERM*`), wildcard, on
  the full name phrase and on the bare distinctive word. Every returned
  name was read, not only the transport-worded ones. SAFER keyword search
  is not exhaustive: it missed Medicine Bow Trucking LLC (Cheyenne WY) and
  Chinkapin Trucking Inc (Klamath Falls OR), which the web search found.
- **USPTO trademark search, reached this pass.** The tmsearch.uspto.gov
  front end calls a public JSON endpoint,
  `POST https://tmsearch.uspto.gov/prod-stage-v1-0-0/tmsearch`, with an
  Elasticsearch-style body (`query_string` on `wordmark`). It answers from
  the shell with no session; the first pass's HTTP 405 came from calling it
  with GET. Each term was checked for every mark, live or dead, and for live
  marks in IC 039 (transportation), IC 035 (business services), and IC 012
  (vehicles). TSDR (`tsdr.uspto.gov`) and `api.uspto.gov` answer 403
  without an API key and were not needed.
- **Web search:** "<word>" with trucking, freight, transport, carriers, or
  cartage, read for any carrier trading under the name.

### Final names

| # | Name | Key | SAFER (phrase; bare word) | USPTO wordmark | Web | Result |
| --- | --- | --- | --- | --- | --- | --- |
| 1 | Androscoggin Freight | `androscoggin_freight` | none; ANDROSCOGGIN 2 (a granite shop, a summer camp) | 9 marks, 2 live, none in IC 039/035/012 | only carriers located in Androscoggin County | Clean |
| 2 | Kittatinny Crossroads Freight | `kittatinny_crossroads` | none; KITTATINNY 1 (a canoe outfitter) | 2 marks, 1 live: a Harrisburg fire company slogan (IC 035/045) | canoe and campground only | Clean (KEEP) |
| 3 | Meherrin Transport | `meherrin_transport` | none; MEHERRIN 3 (an ag-chemical firm, a forest products firm; no freight names) | "meherrin" 2 marks, both dead | only carriers located in Meherrin VA | Clean (swapped in; see note) |
| 4 | Alapaha Regional | `alapaha_regional` | none; ALAPAHA none | 2 marks, 1 live, none in IC 039/035/012 | only carriers located in Alapaha GA | Clean |
| 5 | Sunpine Freight Lines | `sunpine_freight` | none; SUNPINE none | 5 marks, 3 live; TAHOE SUNPINE (IC 035/016, a lodging business), none in IC 039 | a lumber maker, a biofuel firm | Clean (KEEP) |
| 6 | Barataria Carriers | `barataria_carriers` | none; BARATARIA 1 (an electrician) | 0 marks | only carriers on Barataria Blvd, Marrero LA | Clean |
| 7 | Lone Mesa Freight | `lone_mesa_freight` | none; LONE MESA none | 0 marks | a publisher, a state park | Clean (KEEP) |
| 8 | Saint Francis River Lines | `saint_francis_river` | ST FRANCIS RIVER and SAINT FRANCIS RIVER none; ST FRANCIS 28 and SAINT FRANCIS 5 (hospitals, schools, churches, a lumber yard; no freight names) | "st francis river" 0; "st francis" 21 live, one in IC 039: ST. FRANCIS PET TAXI (pet transport) | no carrier under the name | Clean (weak hit: pet taxi mark) |
| 9 | Olentangy Valley Freight | `olentangy_valley` | none; OLENTANGY none | 4 marks, 1 live, none in IC 039/035/012 | only carriers on Olentangy River Rd, Columbus | Clean |
| 10 | Loonwater Regional | `loonwater_regional` | none; LOONWATER none | 0 marks | none | Clean (KEEP) |
| 11 | Verdigris Transport | `verdigris_transport` | none; VERDIGRIS 1 (an electric co-op) | 13 marks, 4 live, none in IC 039/035/012 | only carriers located in Verdigris OK | Clean |
| 12 | Musselshell Freight Lines | `musselshell_freight` | none; MUSSELSHELL 1 (an equipment dealer, private carrier) | 0 marks | only carriers located in Musselshell County MT | Clean |
| 13 | Pinyon Basin Transport | `pinyon_basin` | none; PINYON 8 (environmental and oilfield service firms) | "pinyon basin" 0; "pinyon" 7 live, none in IC 039/035/012 | none | Clean (KEEP) |
| 14 | San Simon Freight | `san_simon_freight` | none; SAN SIMON none | 11 marks, 1 live, none in IC 039/035/012 | only carriers located in San Simon AZ | Clean |
| 15 | Tehachapi Motor Lines | `tehachapi_motor_lines` | none; TEHACHAPI 12 (towing and road service, furniture, schools; no freight names) | 12 marks, 2 live; a city slogan in IC 035, none in IC 039 | a Landstar agency located in Tehachapi; no carrier under the name | Clean (weak hit: Tehachapi Towing) |
| 16 | Chehalis Freight Lines | `chehalis_freight` | none; CHEHALIS 12 (towing location, timber, gravel, fencing; no freight names) | 7 marks, 6 live, none in IC 039/035/012 | only carriers located in Chehalis WA | Clean |
| AK | Chatanika Freight Lines | `chatanika_freight` | none; CHATANIKA none | 0 marks | the Chatanika River bridge on the haul road only | Clean |
| L1 | Des Plaines River Cartage | `des_plaines_cartage` | DES PLAINES RIVER none; DES PLAINES 3 (place suffixes: a moving franchise, a truck dealer) | "des plaines river" 0; "des plaines" 2 live, one the City of Des Plaines seal (IC 035/037/039/045) | only carriers located in Des Plaines IL | Clean (weak hit: city seal mark) |
| L2 | Basin Harbor Drayage | `basin_harbor_drayage` | none; — | 1 mark, dead | a Vermont resort; a hauler on Basin Harbor Rd, not authorized | Clean (KEEP) |
| L3 | Knik Arm Cartage | `knik_arm_cartage` | none; KNIK 24, including KNIK TOWING & WRECKING and KNIK ROAD SERVICE (Mat-Su) | "knik arm" 0; "knik" 1 mark, dead | geography and port context only | Clean on the phrase (weak hit: Knik Towing, same metro) (KEEP) |

**Meherrin swap (final).** The re-cut first kept Catawba Ridge Transport.
"Catawba Ridge" screened clean, but the bare word "Catawba" is used by three
freight-named registrants in the carrier's own lane area (CATAWBA TRUCKING
LLC, CATAWBA HAULING LLC, CATAWBA WARRIOR TRUCKING LLC). Review swapped it
for Meherrin Transport, the best-screening fallback: the Meherrin River runs
on the VA/NC line between Richmond and Greensboro, inside the carrier's
footprint. Rivanna, Uwharrie, Haw River, Tar River, Deep River, Yadkin,
Roanoke, and Pee Dee all collide (below). With the swap, the names are final.

**Tehachapi note.** Los Angeles to Fresno runs I-5 over Tejon Pass, not
CA-58. CA-58 over Tehachapi Pass is inside the lane area, so the name
stays; no wording claims CA-58 is the LA-Fresno route.

### Candidates rejected this pass

| Candidate | Collision found |
| --- | --- |
| Catawba Ridge (swapped out) | bare word: CATAWBA TRUCKING LLC, CATAWBA HAULING LLC, CATAWBA WARRIOR TRUCKING LLC (SAFER) |
| Piscataqua | PISCATAQUA BROKERAGE INC (SAFER) |
| Casco Bay, Merrimack, Penobscot | CASCO BAY TRANSPORTATION LLC; MERRIMACK TRUCKING & EQUIPMENT LLC; PENOBSCOT HIGHLAND ENTERPRISES |
| Quinebaug | clean in SAFER; second choice behind Androscoggin |
| Ocmulgee, Coosa, Oconee, Tallapoosa, Etowah, Altamaha, Cahaba, Ogeechee, Chattahoochee, Alcovy, Ohoopee, Satilla, Black Warrior, Tombigbee | OCMULGEE TRUCKING LLC; COOSA RIVER FREIGHT and COOSA VALLEY FREIGHT; OCONEE TRANSPORT INC; TALLAPOOSA RIVER TRUCKING; ETOWAH EXPRESS; ALTAMAHA TRUCKING; CAHABA CARRIERS LLC; OGEECHEE RIVER HAULING; two Chattahoochee railroads; ALCOVY TRUCKING; OHOOPEE RIVER TRANSPORT; SATILLA TRANSPORT; BLACK WARRIOR TRANSPORTATION; TOMBIGBEE LOGISTICS |
| Oostanaula | clean; passed over as hard to say aloud |
| Tensaw, Atchafalaya, Pearl River, Pascagoula | TENSAW TRUCKING LLC; ATCHAFALAYA TRANSPORT LLC; PEARL RIVER TOWING & RECOVERY; Pascagoula clean but reads as the city, which has no terminal |
| Obion, Hatchie, Yazoo, Forked Deer | A & A TRUCKING OF OBION (place suffix) and the town; HATCHIE BOTTOM TRUCKING; YAZOO TRUCKING LLC; Forked Deer clean, second choice |
| Scioto, Wabash, Kanawha, Muskingum, Hocking, Miami Valley, Whitewater, Licking, Big Sandy, Mahoning | SCIOTO VALLEY TRUCKING; WABASH VALLEY TRANSPORT; KANAWHA TRUCKING; MUSKINGUM MOTOR CLUB; HOCKING TRUCKING; MIAMI VALLEY LOGISTICS; WHITEWATER FREIGHT; LICKING VALLEY TRUCKING; BIG SANDY TRANSPORT; MAHONING FARM LINES |
| Cimarron, Canadian, Canadian River, North Canadian, Washita, Salt Fork, Neosho | CIMARRON TRUCKING; many CANADIAN carriers; CANADIAN RIVER TRUCKING LLC (North Canadian shares it); WASHITA VALLEY TRANSIT; SALT FORK TRANSPORT; NEOSHO TRUCKING |
| Llano Estacado | clean; passed over because the Staked Plains do not reach the OKC and Tulsa terminals |
| Laramie Plains, Powder River, Medicine Bow, Bighorn, Sweetwater, Wind River, Absaroka, Beartooth, Crazy Mountain | LARAMIE VALLEY TRANSPORT and LARAMIE TRANSPORT share "Laramie"; POWDER RIVER TRUCKING; **Medicine Bow Trucking LLC, Cheyenne (web; SAFER missed it)**; BIGHORN FREIGHT; SWEETWATER TRANSPORT; WIND RIVER TRANSPORT; ABSAROKA TRUCKING; BEARTOOTH TRANSPORT; CRAZY MOUNTAIN TRANSPORT |
| Gila, Mogollon, Sonoran, Superstition, Mimbres | GILA BEND FREIGHT LINES; MOGOLLON LOGISTIC TRCKS LLC; SONORAN RAPID FREIGHT; SUPERSTITION TRANSPORT; MIMBRES TREE AND DEBRIS HAULING |
| Hassayampa | clean; passed over for San Simon, which sits on the I-10 lane |
| San Joaquin, Stanislaus, Mokelumne | SAN JOAQUIN FREIGHT LINES LLC; STANISLAUS ELECTRIC MOTOR WORKS; Mokelumne clean, second choice |
| Skagit, Willamette, Umpqua, Santiam, Deschutes, Snohomish, Nisqually, Snoqualmie, Siuslaw, Toutle, Sauk River, Palouse, Clackamas | SKAGIT FREIGHT LLC; WILLAMETTE TRANSPORT; UMPQUA FREIGHT LLC; SANTIAM TRANSPORT; DESCHUTES RIVER TRUCKING; SNOHOMISH TRUCKING; NISQUALLY TRANSPORT; SNOQUALMIE TRUCKING; SIUSLAW TRANSPORT; TOUTLE RIVER TRUCKING; SAUK RIVER TRANSPORTATION; PALOUSE COUNTRY TRUCKING; CLACKAMAS RIGGING & TRANSFER |
| Cowlitz, Klickitat, Grande Ronde, Nooksack | COLUMBIA & COWLITZ RAILWAY; Klickitat Valley Trucking LLC (web); Grande Ronde Transportation (web) and a live GRANDE RONDE mark in IC 012; NOOKSACK VALLEY DISPOSAL |
| Tanana | TANANA TRUCK AND TRACTOR (SAFER, Interior AK); no live USPTO marks |
| Chena, Goldstream, Susitna | CHENA TRUCKING INC; GOLDSTREAM LOGISTICS; SUSITNA ENTERPRISES |
| Calumet, Grand Calumet, Little Calumet, DuPage, Kankakee, Kinzie | CALUMET CARRIERS LLC and CALUMET TRANSPORTATION INC (Grand and Little Calumet share the word); DUPAGE FREIGHT COMPANY; GRAND KANKAKEE LOGISTICS; MCKINZIE TRUCKING |
| Sauganash | clean; passed over because it names a person and a neighborhood rather than a river |
| Rivanna, Uwharrie, Haw River, Tar River, Deep River, Yadkin, Roanoke, Pee Dee | Rivanna Transport LLC (web, inactive); UWHARRIE EXPRESS; HAW RIVER TRUCKING; TAR RIVER TRUCKING; DEEP RIVER FREIGHT LINES; YADKIN VALLEY TRANSPORTATION; ROANOKE TRANSPORT; PEE DEE LOGISTICS |

### First-pass screen (kept for the record)

The first pass (commit `86997afa`) replaced Granite Coast, Allegheny,
Piedmont, Red Clay, Bayou Gulf, Delta River, Riverbend, North Woods,
Crosstimber, Pronghorn, Wasatch, Saguaro, Tule Valley, Timberline Cascade,
Ptarmigan Line, and Lakefront over SAFER collisions (for example GRANITE
STATE HAULING, PIEDMONT CARRIERS LLC, RED CLAY FREIGHT, NORTH WOODS
TRANSPORT LLC, PRONGHORN FREIGHT BROKERS, SAGUARO TRUCKING CO, TIMBERLINE
FREIGHT SERVICES LLC, PTARMIGAN TRANSPORT SERVICES LLC, LAKEFRONT TRUCK
LINES) and Delta River over Delta Air Lines. The re-cut then retired its
plant, animal, seaweed, and grass replacements (Rockweed Coast, Sweetgum,
Marsh Hen, Buttonbush River, Hellbender Valley, Sandplum Plains, Blue
Grama, Brittlebush Sun, Tarweed Valley, Salmonberry, Spruce Hen Line,
Bubbly Creek) under the name rule. The USPTO pass this time confirmed one
of the retirements: MARSH HEN is a live mark of Marsh Hen Mill LLC.

---

## 11. Resolved decisions

1. **Count:** sixteen new regionals plus Prairie Link, 17 lower-48
   regionals in all. No regional is cut.
2. **Wide terminal spans:** Pinyon Basin (SLC-Boise-Las Vegas, 517 mi),
   Lone Mesa (Dallas-McAllen 462), Musselshell (Denver-Billings 454), and
   Chehalis (Seattle-Medford 454) are all KEEP.
3. **Loonwater span:** Bismarck-Green Bay (636 air mi) is KEEP, and
   Bismarck stays. Terminal span is not run length; the 600 mi band applies
   to loads.
4. **Eastern Wisconsin, the UP, and the Quad Cities:** Loonwater Regional
   gets a Green Bay terminal; all six nationals-only cities close (§6).
5. **Termination fallback (§5):** KEEP. A fired driver never moves to
   Milwaukee. The fallback is a carrier hiring in the home city, never the
   firing carrier; without the rule, an AK termination leaves the driver
   home and unassigned. The coverage test adds: no AK driver is ever
   assigned to a lower-48 carrier (§7, test 8).
6. **Knik Arm Cartage:** KEEP thin. The next AK map adds for an Anchorage
   local are the Kenai Peninsula (Soldotna/Kenai, about 147 road mi) and
   Seward (about 125 road mi), measured by road. Whittier waits for the
   Anton Anderson tunnel schedule and hazmat limits. ROADMAP debt row.
7. **AK lane area:** inside AK only. ALCAN through-freight is future work
   (§9).
8. **Locals:** Chicago and LA are KEEP; TWIC for LA/Long Beach port drayage
   is a ROADMAP row.
9. **Regionals overlapping a national's home city** (Olentangy Valley
   reaching Chicago and Milwaukee): KEEP.
10. **Structure:** approved for the data PR.
11. **Names:** the rule is real geographic names (river, valley, range,
    highway, region, or a founder), no plants, animals, seaweed, or grasses,
    and no "Line Freight" pattern. Eight names are KEEP as they were; twelve
    are replaced and screened (§10).
12. **Names final:** Catawba Ridge Transport becomes Meherrin Transport
    (`meherrin_transport`); every other name is KEEP. Tehachapi Motor Lines
    stays: CA-58 is in its lane area even though LA-Fresno runs I-5 over
    Tejon Pass (§10).

---

## 12. Open questions

None open.

1. **Catawba Ridge Transport:** resolved. Swapped to Meherrin Transport
   (§10, §11 item 12).
2. **Unassigned driver state (§5):** resolved with the simplest honest
   version in the data PR. A company driver with no carrier saves an empty
   `carrier_key` and `carrier_name`. The hub says "You have no carrier right
   now" on arrival, and "Apply to a carrier" takes the dispatch board's
   place. It lists carriers that hire in the home city, never the one that
   let the driver go; when none do, carriers with a terminal in the same
   state, each saying it moves the driver's home there. Picking one puts the
   driver at that terminal on sufferance if the record would otherwise end
   the seat. Opening the dispatch board with no carrier says why there is
   none.
