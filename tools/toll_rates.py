"""Curated 5-axle commercial toll rates, with the source for every figure.

The map's 46 toll events were all ``estimated`` -- plausible numbers nobody had
checked. This is the researched replacement: each entry names the authority's
own schedule or calculator it came from, so a later reader can re-verify rather
than trust.

Fields per entry:
  transponder  what a rig with a working tag pays
  plate        what it costs without one; equal to ``transponder`` where the
               authority gives no discount, which is commoner than expected
  directions   ("both",) unless the crossing collects one way only
  src          the authority document or calculator the figure came from
  verified     True when read off a primary source; False when DERIVED (say
               so in ``src``) and therefore not safe to ship as fact

Three rules learned the hard way while assembling this:

* A figure no pass could confirm does NOT go in with ``verified=True`` to look
  tidy. Oklahoma sat unverified through two attempts before a third read the
  live pikepass.com calculator -- its 2025 annual report genuinely stops at a
  2024 rate column, which is why document searches kept failing. The estimates
  held in the meantime were 2 to 7 percent off in BOTH directions: close enough
  to look right, wrong enough to matter.
* Adopted schedules list future years too. BATA Resolution 0184 steps the
  five-axle bridge toll to $40.50 by 2030, but the 2026 rate is $30.50 for
  FasTrak and plate alike (plate surcharges start 2027-01-01). Read the column
  for the career year, not the last one in the table.
* An axle multiplier is not a constant, even inside one authority. Florida's
  own schedule effective 2026-04-21 charges a five-axle rig $3.44 where a car
  pays $0.86 at the NW 27th Avenue plaza -- four times -- and $1.50 where a
  car pays $0.60 at Lantana, two and a half times. The first is priced at a
  flat rate for every axle after the first; the second at a lower rate for
  every axle after the second. So a five-axle per-mile rate CANNOT be derived
  by scaling the published two-axle per-mile table, which is what a pass
  through this file nearly shipped for all 49 Florida crossings. Read the
  five-axle column, or price the plazas.
* Ask where the tolled road actually GOES before pricing a leg. A confident
  source said I-64 leaves Charleston toll-free; it does not, it runs concurrent
  with I-77 down the tolled Turnpike for 60 miles first. The mirror of that:
  I-70 tolling stops at Breezewood, so pricing Pittsburgh to Hagerstown as a
  full turnpike run would overcharge for free road. Route definition causes
  more error here than arithmetic does.
"""

from __future__ import annotations

BOTH = ("both",)

TOLL_RATES: dict[str, dict] = {
    # -- Pennsylvania Turnpike, 2026 schedule, class 5H -----------------------
    "pa_gateway_pittsburgh": {
        "transponder": 53.65,
        "plate": 107.94,
        "directions": BOTH,
        "verified": True,
        "src": "PA Turnpike 2026 E-ZPass and Toll By Plate schedules, Gateway #2 to "
        "Pittsburgh #57; files.paturnpike.com",
    },
    "pa_harrisburg_philadelphia": {
        "transponder": 50.28,
        "plate": 100.56,
        "directions": BOTH,
        "verified": True,
        "src": "PA Turnpike 2026 schedules, Harrisburg East #247 to Valley Forge #326",
    },
    "pa_harrisburg_pittsburgh": {
        "transponder": 114.40,
        "plate": 228.80,
        "directions": BOTH,
        "verified": True,
        "src": "PA Turnpike 2026 schedules, Harrisburg East #247 to Pittsburgh #57",
    },
    "pa_philadelphia_pittsburgh": {
        "transponder": 164.68,
        "plate": 329.36,
        "directions": BOTH,
        "verified": True,
        "src": "PA Turnpike 2026 schedules, Valley Forge #326 to Pittsburgh #57",
    },
    "pa_pittsburgh_carlisle": {
        "transponder": 94.68,
        "plate": 189.36,
        "directions": BOTH,
        "verified": True,
        "src": "PA Turnpike 2026 schedules, Pittsburgh #57 to Carlisle #226",
    },
    # -- Ohio Turnpike, 2026 schedule, class 5 -------------------------------
    "oh_cleveland_toledo": {
        "transponder": 25.75,
        "plate": 32.25,
        "directions": BOTH,
        "verified": True,
        "src": "Ohio Turnpike 2026 Schedule of Tolls, Cleveland MP173 to Maumee MP59",
    },
    "oh_full_westbound": {
        "transponder": 58.75,
        "plate": 74.00,
        "directions": ("forward",),
        "verified": True,
        "src": "Ohio Turnpike 2026 news release, PA line to IN line westbound; the "
        "Eastgate westbound barrier is a ROUND-TRIP charge, eastbound is free",
    },
    "oh_full_eastbound": {
        "transponder": 49.75,
        "plate": 62.75,
        "directions": ("reverse",),
        "verified": True,
        "src": "Ohio Turnpike 2026 news release, IN line to PA line eastbound",
    },
    "oh_gateway_cleveland": {
        "transponder": 20.00,
        "plate": 25.00,
        "directions": BOTH,
        "verified": True,
        "src": "Ohio Turnpike 2026, Eastgate barrier plus segment, summed by the "
        "authority's own documented method",
    },
    # -- New York State Thruway, 2026, class 5H ------------------------------
    # NOTE: a non-NY E-ZPass pays the Tolls-by-Mail rate, so "plate" here is
    # what any out-of-state carrier actually pays.
    "ny_buffalo_rochester": {
        "transponder": 16.53,
        "plate": 28.95,
        "directions": BOTH,
        "verified": True,
        "src": "NY Thruway toll calculator 5H, exit 53 to exit 45, eff. 2026-01-01",
    },
    "ny_syracuse_albany": {
        "transponder": 33.77,
        "plate": 59.12,
        "directions": BOTH,
        "verified": True,
        "src": "NY Thruway calculator 5H, exit 39 to exit 24",
    },
    "ny_syracuse_buffalo": {
        "transponder": 31.19,
        "plate": 54.60,
        "directions": BOTH,
        "verified": True,
        "src": "NY Thruway calculator 5H, exit 39 to exit 53",
    },
    "ny_syracuse_rochester": {
        "transponder": 14.66,
        "plate": 25.65,
        "directions": BOTH,
        "verified": True,
        "src": "NY Thruway calculator 5H, exit 39 to exit 45",
    },
    "ny_utica_albany": {
        "transponder": 20.27,
        "plate": 35.47,
        "directions": BOTH,
        "verified": True,
        "src": "NY Thruway calculator 5H, exit 31 to exit 24",
    },
    "ny_erie_buffalo": {
        "transponder": 15.87,
        "plate": 27.77,
        "directions": BOTH,
        "verified": True,
        "src": "NY Thruway calculator 5H, PA state line to exit 53",
    },
    "ny_buffalo_nyc": {
        "transponder": 167.53,
        "plate": 293.21,
        "directions": BOTH,
        "verified": True,
        "src": "NY Thruway calculator 5H, exit 53 to NYC line, incl. Cuomo Bridge",
    },
    "ny_rochester_nyc": {
        "transponder": 151.00,
        "plate": 264.26,
        "directions": BOTH,
        "verified": True,
        "src": "NY Thruway calculator 5H, exit 45 to NYC line",
    },
    "ny_nyc_albany": {
        "transponder": 46.43,
        "plate": 81.25,
        "directions": BOTH,
        "verified": True,
        "src": "NY Thruway calculator 5H, NYC line to exit 24, incl. Spring Valley",
    },
    "ny_new_england_thruway": {
        "transponder": 7.98,
        "plate": 13.97,
        "directions": ("forward",),
        "verified": True,
        "src": "NY Thruway calculator 5H, New Rochelle gantry, NORTHBOUND ONLY",
    },
    "ny_berkshire_connector": {
        "transponder": 10.48,
        "plate": 18.34,
        "directions": BOTH,
        "verified": True,
        "src": "NY Thruway calculator 5H, exit 23 to Canaan gantry at the MA line, "
        "eff. 2026-01-01; non-NY E-ZPass pays the Tolls-by-Mail rate",
    },
    # -- Indiana Toll Road, eff. 2026-07-01, class 5 -------------------------
    "in_full_length": {
        "transponder": 91.37,
        "plate": 91.30,
        "directions": BOTH,
        "verified": True,
        "src": "Indiana Toll Road calculator and published rate table, Eastpoint to "
        "Westpoint, eff. 2026-07-01",
    },
    "in_gary_illinois": {
        "transponder": 11.93,
        "plate": 11.90,
        "directions": BOTH,
        "verified": True,
        "src": "Indiana Toll Road Class 5, Gary East to Westpoint",
    },
    "in_gary_south_bend": {
        "transponder": 39.08,
        "plate": 39.10,
        "directions": BOTH,
        "verified": True,
        "src": "Indiana Toll Road Class 5, Gary East to South Bend West",
    },
    "in_south_bend_illinois": {
        "transponder": 44.80,
        "plate": 44.80,
        "directions": BOTH,
        "verified": True,
        "src": "Indiana Toll Road Class 5, South Bend West to Westpoint",
    },
    # -- Massachusetts Turnpike, all-electronic ------------------------------
    "ma_boston_worcester": {
        "transponder": 10.85,
        "plate": 12.95,
        "directions": BOTH,
        "verified": True,
        "src": "MassDOT EZDriveMA calculator, I-93 to Auburn I-290/I-395",
    },
    "ma_springfield_ny_line": {
        "transponder": 6.00,
        "plate": 6.90,
        "directions": BOTH,
        "verified": True,
        "src": "MassDOT calculator, I-291 Springfield to West Stockbridge NY line",
    },
    "ma_ny_line_worcester": {
        "transponder": 11.70,
        "plate": 13.50,
        "directions": BOTH,
        "verified": True,
        "src": "MassDOT calculator, West Stockbridge NY line to Auburn",
    },
    # -- New Jersey / Delaware / Maryland corridor ---------------------------
    "nj_turnpike_full": {
        "transponder": 71.66,
        "plate": 78.55,
        "directions": BOTH,
        "verified": True,
        "src": "NJ Turnpike Authority 2026 Class 5 schedule, interchange 1 to 18E/18W",
    },
    "nj_western_spur_newark_ny": {
        "transponder": 15.09,
        "plate": 16.55,
        "directions": BOTH,
        "verified": True,
        "src": "NJTA 2026 Class 5, entry 15E Newark to exit 18E GWB approach; PEAK "
        "figure, off-peak is 14.34. Verified against both the PDF matrix and "
        "the authority's own calculator",
    },
    "nj_newark_pa_extension": {
        "transponder": 43.44,
        "plate": 47.50,
        "directions": BOTH,
        "verified": True,
        "src": "NJTA 2026 Class 5, entry 14 Newark Airport to exit 6 PA Turnpike at "
        "Florence; PEAK figure, off-peak is 41.27",
    },
    # -- Pennsylvania Turnpike extensions, 2026, class 5H --------------------
    # Toll By Plate is exactly double E-ZPass on every PA segment, which is the
    # Commission's published policy rather than an approximation.
    "pa_ne_extension_scranton_allentown": {
        "transponder": 53.64,
        "plate": 107.28,
        "directions": BOTH,
        "verified": True,
        "src": "PA Turnpike 2026 schedules, Northeast Extension I-476, Clarks Summit "
        "#131 to Lehigh Valley #56, eff. 2026-01-04",
    },
    "pa_ne_extension_allentown_philadelphia": {
        "transponder": 24.12,
        "plate": 48.24,
        "directions": BOTH,
        "verified": True,
        "src": "PA Turnpike 2026 schedules, Lehigh Valley #56 to Mid-County #20",
    },
    "pa_i70_pittsburgh_breezewood": {
        "transponder": 57.56,
        "plate": 115.12,
        "directions": BOTH,
        "verified": True,
        "src": "PA Turnpike 2026 schedules, Pittsburgh #57 to Breezewood #161. NOTE: "
        "I-70 tolling ENDS at Breezewood -- the run on to the Maryland line and "
        "Hagerstown is free, so never price the full leg",
    },
    "pa_i70_new_stanton_breezewood": {
        "transponder": 43.24,
        "plate": 86.48,
        "directions": BOTH,
        "verified": True,
        "src": "PA Turnpike 2026 schedules, New Stanton #75 (where I-70 actually joins "
        "the Turnpike) to Breezewood #161",
    },
    "de_i95_plaza": {
        "transponder": 10.00,
        "plate": 10.00,
        "directions": BOTH,
        "verified": True,
        "src": "DelDOT I-95 Newark plaza calculator; cash and E-ZPass are equal here",
    },
    "md_jfk_highway": {
        "transponder": 48.00,
        "plate": 63.00,
        "directions": ("forward",),
        "verified": True,
        "src": "MDTA toll rate tables, I-95 JFK Memorial Highway, ONE DIRECTION ONLY; "
        "plate figure is the video toll",
    },
    "md_fort_mchenry": {
        "transponder": 24.00,
        "plate": 36.00,
        "directions": BOTH,
        "verified": True,
        "src": "MDTA toll rate tables, Fort McHenry Tunnel I-95; hazmat prohibited, "
        "and the Key Bridge detour has been unavailable since the 2024 collapse",
    },
    "md_chesapeake_bay_bridge": {
        "transponder": 24.00,
        "plate": 36.00,
        "directions": ("forward",),
        "verified": True,
        "src": "MDTA Bay Bridge rates, US-50/301, EASTBOUND ONLY; plate figure is the "
        "video toll, registered pay-by-plate is 24.00",
    },
    "drjtbc_i78_bridge": {
        "transponder": 32.50,
        "plate": 40.00,
        "directions": ("forward",),
        "verified": True,
        "src": "DRJTBC current tolls, Class 5 at 6.50/axle E-ZPass and 8.00/axle plate, "
        "WESTBOUND (Pennsylvania-bound) ONLY, eff. 2026-01-01; no cash",
    },
    # -- California bridges, BATA -------------------------------------------
    # 2026 rate; 40.50 is Resolution 0184's 2030 step.
    "ca_carquinez_bridge": {
        "transponder": 30.50,
        "plate": 30.50,
        "directions": ("forward",),
        "verified": True,
        "src": "BATA Resolution 0184 schedule, 5 axles, 2026 rate (40.50 is the 2030 step), "
        "EASTBOUND ONLY",
    },
    "ca_benicia_martinez_bridge": {
        "transponder": 30.50,
        "plate": 30.50,
        "directions": ("forward",),
        "verified": True,
        "src": "BATA Resolution 0184 schedule, 5 axles, 2026 rate (40.50 is the 2030 step), "
        "NORTHBOUND ONLY",
    },
    # -- Illinois Tollway, barrier system, cashless, 2026 --------------------
    # Truck rates split by time of day: daytime 6am-10pm, overnight 10pm-6am.
    # The overnight figure is roughly 25 percent cheaper and is a real reason
    # to run at night. Stored as the daytime rate; overnight kept in the note
    # until the sim models clock-dependent tolls.
    "il_i90_rockford_chicago": {
        "transponder": 42.30,
        "plate": 42.30,
        "directions": BOTH,
        "verified": True,
        "src": "Illinois Tollway 2026, four I-90 mainline barriers summed; cashless",
    },
    "il_i90_rockford_wi_line": {
        "transponder": 27.55,
        "plate": 27.55,
        "directions": BOTH,
        "verified": True,
        "src": "Illinois Tollway 2026, Belvidere plus South Beloit barriers",
    },
    "il_tristate_full": {
        "transponder": 60.30,
        "plate": 60.30,
        "directions": BOTH,
        "verified": True,
        "src": "Illinois Tollway 2026, Tri-State I-294 WI line to IN line, six mainline "
        "barriers summed, DAYTIME; overnight 10pm-6am is 45.30",
    },
    "il_edens_spur": {
        "transponder": 10.60,
        "plate": 10.60,
        "directions": BOTH,
        "verified": True,
        "src": "Illinois Tollway 2026, Edens Spur mainline plaza 24, DAYTIME; overnight 7.95",
    },
    "il_i88_dixon": {
        "transponder": 20.10,
        "plate": 20.10,
        "directions": BOTH,
        "verified": True,
        "src": "Illinois Tollway 2026, Dixon mainline plaza 69, DAYTIME; overnight "
        "15.15. I-88 does not reach Rockford; a truck leaves at I-39",
    },
    # -- Kansas Turnpike, flat per-mile, cashless since July 2024 ------------
    "ks_emporia_wichita": {
        "transponder": 11.66,
        "plate": 23.32,
        "directions": BOTH,
        "verified": True,
        "src": "Kansas Turnpike cashless rates, 0.138/mi K-TAG and 0.276/mi image, "
        "84.5 mi; the authority's own per-mile method",
    },
    "ks_topeka_emporia": {
        "transponder": 6.93,
        "plate": 13.86,
        "directions": BOTH,
        "verified": True,
        "src": "Kansas Turnpike cashless per-mile rate, 50.2 mi",
    },
    "ks_wichita_kansas_city": {
        "transponder": 25.08,
        "plate": 50.16,
        "directions": BOTH,
        "verified": True,
        "src": "Kansas Turnpike cashless per-mile rate, 181.7 mi",
    },
    # -- Maine Turnpike and New Hampshire -----------------------------------
    "me_lewiston_portland": {
        "transponder": 9.00,
        "plate": 9.00,
        "directions": BOTH,
        "verified": True,
        "src": "Maine Turnpike E-ZPass Class 5 chart, eff. 2021-11-01",
    },
    "me_gardiner_portland": {
        "transponder": 14.20,
        "plate": 16.00,
        "directions": BOTH,
        "verified": True,
        "src": "Maine Turnpike E-ZPass Class 5 chart; Bangor is NORTH of the tolled "
        "section, so a Bangor run pays only the Gardiner-to-Portland portion",
    },
    "nh_hampton_plaza": {
        "transponder": 4.95,
        "plate": 5.50,
        "directions": BOTH,
        "verified": True,
        "src": "NH DOT Turnpike System Toll Rate Schedule, Hampton main plaza, class 8 "
        "(5 axles dual tires), eff. 2020-01-01; NH E-ZPass only -- an "
        "out-of-state transponder pays the cash rate",
    },
    # -- Florida, Chesapeake Bay Bridge-Tunnel, West Virginia ----------------
    "fl_alligator_alley": {
        "transponder": 25.44,
        "plate": 30.00,
        "directions": BOTH,
        "verified": True,
        "src": "Florida Turnpike Alligator Alley, TWO plazas at 12.72 SunPass and "
        "15.00 cash each; no toll-by-plate option, unpaid is a violation",
    },
    "cbbt_us13": {
        "transponder": 48.00,
        "plate": 48.00,
        "directions": BOTH,
        "verified": True,
        "src": "CBBT Class 12 (5 axles under 84,000 lb), flat for all payment types, "
        "charged each direction, eff. 2024-01-01",
    },
    "wv_turnpike_barrier": {
        "transponder": 12.00,
        "plate": 15.00,
        "directions": BOTH,
        "verified": True,
        "src": "WV Parkways Class 8, PER MAINLINE BARRIER (Ghent, Pax, Chelyan); "
        "non-WV E-ZPass is 13.00",
    },
    # Charleston to Virginia is NOT toll-free, though a second opinion said it
    # was. I-64 does not leave Charleston eastbound: it merges into I-77 and the
    # two run concurrently SOUTH as the tolled Turnpike for about 60 miles,
    # splitting east only at Exit 40 below Beckley. So the run crosses Chelyan
    # and Pax -- two of the three barriers -- and only then reaches free road.
    "wv_charleston_to_i64_split": {
        "transponder": 24.00,
        "plate": 30.00,
        "directions": BOTH,
        "verified": True,
        "src": "WV Parkways Class 8 at 12.00 E-ZPass / 15.00 cash per barrier, TWO "
        "barriers (Chelyan MP83, Pax MP56) on the I-64/I-77 concurrency from "
        "Charleston to the Exit 40 split; Ghent lies south of the split and is "
        "not crossed. turnpike.wv.gov toll rates + AARoads I-64/I-77 guides",
    },
    # -- Oklahoma Turnpike Authority, live calculator, class 5 "Large" --------
    # OTA publishes NO current dollar table -- its 2025 ACFR genuinely stops at
    # a 2024 column -- so the live calculator is the authoritative source.
    "ok_turner_okc_tulsa": {
        "transponder": 22.12,
        "plate": 44.03,
        "directions": BOTH,
        "verified": True,
        "src": "OTA toll calculator, Turner Turnpike, I-35/Kilpatrick to Tulsa, "
        "class 5; pikepass.com, accessed 2026-07-19",
    },
    "ok_will_rogers": {
        "transponder": 22.12,
        "plate": 44.03,
        "directions": BOTH,
        "verified": True,
        "src": "OTA toll calculator, Will Rogers Turnpike, Tulsa to the Missouri line",
    },
    "ok_he_bailey_tx_okc": {
        "transponder": 16.34,
        "plate": 37.73,
        "directions": BOTH,
        "verified": True,
        "src": "OTA toll calculator, H.E. Bailey, Texas line to Oklahoma City",
    },
    "ok_he_bailey_lawton_okc": {
        "transponder": 11.70,
        "plate": 24.26,
        "directions": BOTH,
        "verified": True,
        "src": "OTA toll calculator, H.E. Bailey, Lawton to Oklahoma City",
    },
    "ok_cimarron": {
        "transponder": 12.40,
        "plate": 25.98,
        "directions": BOTH,
        "verified": True,
        "src": "OTA toll calculator, Cimarron Turnpike full length, I-35 near Perry to "
        "US-64/SH-48 into Tulsa. NOTE: the Cimarron does NOT reach Enid, so a "
        "Tulsa-to-Enid leg does not run its full length",
    },
    "ok_cherokee": {
        "transponder": 10.72,
        "plate": 22.06,
        "directions": BOTH,
        "verified": True,
        "src": "OTA toll calculator, Cherokee Turnpike full length, US-69 to the "
        "Arkansas line at Flint Bridge",
    },
    "va_elizabeth_river_tunnels": {
        "transponder": 7.25,
        "plate": 12.08,
        "directions": BOTH,
        "verified": True,
        "src": "Elizabeth River Crossings, heavy vehicle 3+ axles, OFF-PEAK, eff. "
        "2026-01-01; peak 5:30-9am and 2:30-7pm is 13.59 and 18.42. Tunnels "
        "are height-limited to 13ft6in and ban several hazmat classes",
    },
    "va_pocahontas_parkway": {
        "transponder": 10.50,
        "plate": 10.50,
        "directions": BOTH,
        "verified": True,
        "src": "pocahontas895.com main toll plaza, 5 axle, eff. 2026-04-01",
    },
}

# Nothing is currently held back. Oklahoma sat here through two failed
# verification passes -- its 2025 ACFR genuinely stops at a 2024 rate column,
# which is why static-document searches kept coming up empty -- until a third
# pass read the live pikepass.com calculator, the only authoritative current
# source. The derived estimates had been 2 to 7 percent off in BOTH directions,
# close enough to look right and wrong enough not to ship. Keep this dict: the
# next unverifiable figure belongs here, not in TOLL_RATES.
UNVERIFIED: dict[str, dict] = {}

# Facilities the research positively established are NOT tolled for us, so the
# scan's sighting must be discarded rather than priced.
NOT_TOLLED: dict[str, str] = {
    # WARNING, a correction: Charleston to Richmond and Charleston to Roanoke
    # were briefly listed here on one source's say-so that I-64 leaves Charleston
    # eastbound free. It does not -- it runs concurrent with I-77 down the tolled
    # Turnpike to the Exit 40 split, crossing two barriers. They are priced above
    # as wv_charleston_to_i64_split. Do not re-add them here.
    "louisville_ky_us->elizabethtown_ky_us": (
        "Elizabethtown is south of Louisville, so the run never crosses the tolled "
        "Ohio River bridges"
    ),
    "va_coleman_bridge": (
        "US-17 Coleman Bridge tolls were eliminated 2025-08-08 and the plaza is being "
        "demolished. Never model this as tolled"
    ),
}
