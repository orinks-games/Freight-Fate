# ruff: noqa: F401,F821,E402,I001
"""Build-time route enrichment helpers for corridor metadata.

Runtime gameplay stays offline. This tool either reads checked-in world data or
performs tiny live smoke checks for one representative corridor: no-key
OSRM/Open-Meteo, or OpenRouteService driving-hgv (truck) routing when an
``ORS_API_KEY`` is set in the environment.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import math
import os
import re
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
from pathlib import Path
from typing import Any

from world_source import load_world, save_world

ROOT = Path(__file__).resolve().parents[1]
CACHE_PATH = ROOT / ".route-cache"
USER_AGENT = "Freight-Fate route-enrichment smoke (https://github.com/orinks-games/Freight-Fate)"
OSRM_ROUTE_URL = "https://router.project-osrm.org/route/v1/driving/{coords}"
OPEN_METEO_ELEVATION_URL = "https://api.open-meteo.com/v1/elevation"
# OpenRouteService heavy-goods (truck) routing via the official `openrouteservice`
# SDK. Build-time only (the `tooling` dependency group); the key lives in the
# environment and is never bundled or read at runtime. One driving-hgv request
# returns truck-legal geometry with elevation plus steepness and tollway extras
# -- the inputs the corridor builders already consume.
ORS_HGV_PROFILE = "driving-hgv"
ORS_API_KEY_ENV = "ORS_API_KEY"
ORS_EXTRA_INFO = ("steepness", "tollways", "waytype")
# HeiGIT now serves the API at api.heigit.org; the SDK still defaults to the
# deprecated api.openrouteservice.org, so point it at the current host. The full
# endpoint becomes {base}/v2/directions/{profile}. Override with ORS_BASE_URL.
ORS_DEFAULT_BASE_URL = "https://api.heigit.org/openrouteservice"
OVERPASS_URL = "https://overpass-api.de/api/interpreter"
# Prefer a self-hosted Overpass via the OVERPASS_URL env var (see
# enrich_routes_base); public mirrors stay as fallback.
OVERPASS_URLS = tuple(
    dict.fromkeys(
        url
        for url in (
            os.environ.get("OVERPASS_URL"),
            OVERPASS_URL,
            "https://overpass.kumi.systems/api/interpreter",
        )
        if url
    )
)
CENSUS_STATES_URL = "https://www2.census.gov/geo/tiger/GENZ2023/shp/cb_2023_us_state_500k.zip"
CENSUS_STATES_GEOJSON_URL = (
    "https://raw.githubusercontent.com/plotly/datasets/master/geojson-counties-fips.json"
)
SIMPLE_STATES_GEOJSON_URL = (
    "https://raw.githubusercontent.com/PublicaMundi/MappingAPI/master/data/geojson/us-states.json"
)
OSRM_TIMEOUT_S = 12
# Dispatch gates on routing completeness only. Curated POIs are an additive
# quality layer (auto-sourced; reported via the non-blocking POI advisory), not a
# dispatch requirement -- the runtime HOS fallbacks keep a stop-less leg playable.
# Checkpoints are deliberately NOT required (speech-quality layer, same
# class as POIs): the old requirement forced 246 fake "corridor between"
# placeholder checkpoints into existence, which then spoke as places.
# Mirrors Leg.metadata_complete (world_models.py).
REQUIRED_METADATA_FIELDS = (
    "route_points",
    "state_miles",
    "elevation_samples",
    "grade_segments",
)
# A long leg with no fuel-capable curated stop leans on the roadside-fuel
# fallback; flag it for optional curation without blocking dispatch.
LONG_LEG_POI_ADVISORY_MI = 250.0
ELEVATION_SOURCE = "Open-Meteo Elevation API development-time sample from Copernicus DEM GLO-90."
CORRIDOR_SOURCE = (
    "Development-time OSRM route geometry over OpenStreetMap, with Open-Meteo "
    "elevation samples, Census/OpenStreetMap state context, and curated "
    "corridor POIs checked in for offline runtime use."
)
ORS_ELEVATION_SOURCE = (
    "OpenRouteService driving-hgv route elevation over OpenStreetMap "
    "(SRTM/elevation), sampled at development time."
)
ORS_CORRIDOR_SOURCE = (
    "Development-time OpenRouteService driving-hgv (truck) route geometry and "
    "elevation over OpenStreetMap, with Census/OpenStreetMap state context and "
    "curated corridor POIs checked in for offline runtime use."
)
ORS_GRADE_SOURCE = (
    "OpenRouteService route elevation profile segmented by terrain (development-time)."
)
HIGH_PRIORITY_REMAINING_CORRIDORS = (
    {
        "from": "philadelphia_pa_us",
        "to": "pittsburgh_pa_us",
        "label": "PA Turnpike / I-76 Allegheny corridor",
        "why": "major toll corridor with service plazas, grades, tunnels, and emergency service modeling",
    },
    {
        "from": "cleveland_oh_us",
        "to": "chicago_il_us",
        "label": "Ohio/Indiana Turnpike and I-80/I-90 corridor",
        "why": "major toll and service-plaza-heavy Midwest freight corridor",
    },
    {
        "from": "new_york_ny_us",
        "to": "boston_ma_us",
        "label": "I-95 / New England toll corridor",
        "why": "extends Northeast toll and service-plaza realism beyond the current NY-Philadelphia batch",
    },
    {
        "from": "philadelphia_pa_us",
        "to": "baltimore_md_us",
        "label": "I-95 Northeast Corridor south of Philadelphia",
        "why": "connects the current NJ/Philadelphia lane to the broader Northeast freight network",
    },
    {
        "from": "pittsburgh_pa_us",
        "to": "cleveland_oh_us",
        "label": "PA/Ohio Turnpike connector corridor",
        "why": "ties the PA Turnpike batch into the Ohio Turnpike network",
    },
)


import importlib

TOOLS_DIR = Path(__file__).resolve().parent
if str(TOOLS_DIR) not in sys.path:
    sys.path.insert(0, str(TOOLS_DIR))

_HELPER_MODULE_NAMES = (
    "enrich_routes_pipeline",
    "enrich_routes_pois",
    "enrich_routes_states",
    "enrich_routes_coverage",
    "enrich_routes_ors",
)
_HELPER_MODULES = [importlib.import_module(name) for name in _HELPER_MODULE_NAMES]
for _module in _HELPER_MODULES:
    for _name, _value in _module.__dict__.items():
        if not _name.startswith("__"):
            globals()[_name] = _value
for _module in _HELPER_MODULES:
    _module.__dict__.update(
        {name: value for name, value in globals().items() if not name.startswith("__")}
    )


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        description="Inspect or smoke-check offline corridor metadata."
    )
    parser.add_argument("--from-city", default="chicago_il_us")
    parser.add_argument("--to-city", default="indianapolis_in_us")
    parser.add_argument(
        "--live-smoke", action="store_true", help="Make tiny no-key OSRM and elevation requests."
    )
    parser.add_argument(
        "--ors-smoke",
        action="store_true",
        help="Make a tiny OpenRouteService driving-hgv (truck) "
        f"request for the corridor. Requires the "
        f"{ORS_API_KEY_ENV} environment variable (a free "
        "OpenRouteService key, build-time only).",
    )
    parser.add_argument(
        "--ors-compare",
        action="store_true",
        help="Read-only: compare the ORS driving-hgv corridor "
        "for --from-city/--to-city against the checked-in "
        f"data. Requires {ORS_API_KEY_ENV}.",
    )
    parser.add_argument(
        "--engine",
        choices=("osrm", "ors"),
        default="osrm",
        help="Routing engine for --enrich-all. 'ors' uses the "
        "driving-hgv truck profile (needs ORS_API_KEY and "
        "the tooling group); 'osrm' (default) is car.",
    )
    parser.add_argument(
        "--refresh-geometry",
        action="store_true",
        help="Re-derive route_points, elevation_samples, and "
        "grade_segments for selected legs from --engine, "
        "preserving curated miles, POIs, tolls, and named "
        "crossings/checkpoints. Use with --only and --write.",
    )
    parser.add_argument(
        "--only",
        default="",
        help="Semicolon-separated 'From:To' legs for "
        "--refresh-geometry / --adopt-ors-miles, e.g. "
        "'Denver:Salt Lake City;Philadelphia:Pittsburgh'. "
        "Semicolons (not commas) so comma-bearing city "
        "names like 'Charleston, West Virginia' parse.",
    )
    parser.add_argument(
        "--adopt-ors-miles",
        action="store_true",
        help="Rewrite leg mileage to the real ORS driving-hgv "
        "distance (rescaling corridor positions). Drives "
        "pay/deadlines. Needs ORS_API_KEY + tooling group; "
        "use with --write (and optionally --only).",
    )
    parser.add_argument(
        "--add-overpass-pois",
        action="store_true",
        help="Additively enrich legs with named OpenStreetMap "
        "truck POIs of any brand (Overpass). Use with "
        "--write, --per-leg, and optionally --only.",
    )
    parser.add_argument(
        "--per-leg",
        type=int,
        default=2,
        help="Max new mid-corridor POIs per leg for --add-overpass-pois "
        "(endpoint-city finds ride on top, at most one per leg end).",
    )
    parser.add_argument(
        "--rural-fuel-fallback",
        action="store_true",
        help="With --add-overpass-pois: on legs that would otherwise carry no "
        "stop, also accept a named diesel-selling fuel station (no HGV tag "
        "required) as a low-ranked fuel_station stop. Pair with --only on the "
        "stopless legs so legs with real truck stops are untouched.",
    )
    parser.add_argument(
        "--add-maxspeed",
        action="store_true",
        help="Bake real OpenStreetMap maxspeed onto legs as a "
        "speed_limits profile (Overpass). The runtime "
        "prefers it over the highway/region heuristic. Use "
        "with --write and optionally --only.",
    )
    parser.add_argument(
        "--prune-non-truck-pois",
        action="store_true",
        help="Remove auto-discovered non-truck POIs (generic car "
        "fuel) network-wide, keeping curated stops and "
        "truck-relevant ones. Use with --write.",
    )
    parser.add_argument(
        "--coverage-report",
        action="store_true",
        help="Report metadata coverage for every world leg.",
    )
    parser.add_argument(
        "--json", action="store_true", help="Emit machine-readable JSON with --coverage-report."
    )
    parser.add_argument(
        "--overpass-poi-smoke",
        action="store_true",
        help="Make one tiny Overpass POI query near the corridor.",
    )
    parser.add_argument(
        "--enrich-all",
        action="store_true",
        help="Enrich missing world legs from cached/no-key sources.",
    )
    parser.add_argument(
        "--write", action="store_true", help="Write enriched metadata back to world.json."
    )
    parser.add_argument(
        "--cache-dir",
        default=str(CACHE_PATH),
        help="Directory for resumable live API response cache.",
    )
    parser.add_argument(
        "--limit", type=int, default=0, help="Maximum number of legs to enrich in this run."
    )
    parser.add_argument(
        "--rate-limit",
        type=float,
        default=1.0,
        help="Seconds to wait after uncached live API requests.",
    )
    parser.add_argument(
        "--no-overpass",
        action="store_true",
        help="Skip live Overpass POI discovery during enrichment.",
    )
    args = parser.parse_args(argv)

    data = load_world()
    if args.enrich_all:
        api_key = None
        if args.engine == "ors":
            api_key = ors_api_key()
            if api_key is None:
                raise SystemExit(
                    f"--engine ors needs the {ORS_API_KEY_ENV} environment "
                    "variable and the tooling group "
                    "(uv run --group tooling ...)."
                )
        result = enrich_all_routes(
            data,
            cache_dir=Path(args.cache_dir),
            limit=args.limit or None,
            write=args.write,
            rate_limit_s=args.rate_limit,
            use_overpass=not args.no_overpass,
            engine=args.engine,
            api_key=api_key,
        )
        if args.write:
            save_world(data)
        print(
            json.dumps(result, indent=2, sort_keys=True)
            if args.json
            else format_enrichment_result(result)
        )
        return 0
    if args.adopt_ors_miles:
        api_key = ors_api_key()
        if api_key is None:
            raise SystemExit(
                f"--adopt-ors-miles needs the {ORS_API_KEY_ENV} environment "
                "variable and the tooling group (uv run --group tooling ...)."
            )
        result = adopt_ors_miles(
            data,
            cache_dir=Path(args.cache_dir),
            rate_limit_s=args.rate_limit,
            api_key=api_key,
            only=_parse_only(args.only),
        )
        if args.write:
            save_world(data)
        if args.json:
            print(json.dumps(result, indent=2, sort_keys=True))
        else:
            totals = result["coverage_totals"]
            print(
                f"Adopted ORS miles for {len(result['changed'])} leg(s); "
                f"playable {totals['playable']}/{totals['legs']}"
                f"{' (written)' if args.write else ' (dry run)'}"
            )
        return 0
    if args.add_overpass_pois:
        result = add_overpass_pois(
            data,
            cache_dir=Path(args.cache_dir),
            rate_limit_s=args.rate_limit,
            only=_parse_only(args.only),
            per_leg=args.per_leg,
            rural_fallback=args.rural_fuel_fallback,
        )
        if args.write:
            save_world(data)
        if args.json:
            print(json.dumps(result, indent=2, sort_keys=True))
        else:
            totals = result["coverage_totals"]
            print(
                f"Added {result['added_pois']} OSM POIs to "
                f"{result['updated_legs']} legs; playable "
                f"{totals['playable']}/{totals['legs']}; "
                f"{len(result['legs_without_any_poi'])} legs still without a POI"
                f"{' (written)' if args.write else ' (dry run)'}"
            )
        return 0
    if args.add_maxspeed:
        result = bake_maxspeed(
            data,
            cache_dir=Path(args.cache_dir),
            rate_limit_s=args.rate_limit,
            only=_parse_only(args.only),
        )
        if args.write:
            save_world(data)
        if args.json:
            print(json.dumps(result, indent=2, sort_keys=True))
        else:
            print(
                f"Baked OSM maxspeed onto {len(result['baked_legs'])} leg(s); "
                f"{len(result['legs_without_maxspeed'])} without a corridor "
                f"maxspeed tag"
                f"{' (written)' if args.write else ' (dry run)'}"
            )
        return 0
    if args.prune_non_truck_pois:
        result = prune_non_truck_pois(data)
        if args.write:
            save_world(data)
        if args.json:
            print(json.dumps(result, indent=2, sort_keys=True))
        else:
            totals = result["coverage_totals"]
            print(
                f"Pruned {result['removed_pois']} non-truck POIs; "
                f"{result['legs_emptied']} legs now stop-less; playable "
                f"{totals['playable']}/{totals['legs']}"
                f"{' (written)' if args.write else ' (dry run)'}"
            )
        return 0
    if args.refresh_geometry:
        api_key = None
        if args.engine == "ors":
            api_key = ors_api_key()
            if api_key is None:
                raise SystemExit(
                    f"--engine ors needs the {ORS_API_KEY_ENV} environment "
                    "variable and the tooling group "
                    "(uv run --group tooling ...)."
                )
        result = refresh_corridors(
            data,
            cache_dir=Path(args.cache_dir),
            rate_limit_s=args.rate_limit,
            engine=args.engine,
            api_key=api_key,
            only=_parse_only(args.only),
        )
        if args.write:
            save_world(data)
        if args.json:
            print(json.dumps(result, indent=2, sort_keys=True))
        else:
            totals = result["coverage_totals"]
            print(
                f"Refreshed {len(result['refreshed'])} corridor(s) via "
                f"{args.engine}; playable {totals['playable']}/{totals['legs']}"
                f"{' (written)' if args.write else ' (dry run)'}"
            )
        return 0
    if args.coverage_report:
        report = coverage_report(data)
        if args.json:
            print(json.dumps(report, indent=2, sort_keys=True))
        else:
            print(format_coverage_report(report))
        return 0

    leg = _find_leg(data, args.from_city, args.to_city)
    if leg is None:
        raise SystemExit(f"No direct world leg {args.from_city} to {args.to_city}")

    corridor = leg.get("corridor", {})
    print(_offline_summary(leg, corridor))
    if args.live_smoke:
        result = _osrm_smoke(data, args.from_city, args.to_city)
        print(
            "OSRM live smoke: "
            f"{result['miles']:.1f} miles, "
            f"{result['points']} geometry points, "
            f"code {result['code']}"
        )
        elevation = _open_meteo_elevation_smoke(corridor)
        print(
            "Open-Meteo elevation smoke: "
            f"{elevation['samples']} samples, "
            f"{elevation['min_ft']:.0f}-{elevation['max_ft']:.0f} feet"
        )
    if args.ors_smoke:
        api_key = ors_api_key()
        if api_key is None:
            raise SystemExit(
                f"ORS smoke needs the {ORS_API_KEY_ENV} environment variable "
                "(a free OpenRouteService key). It is build-time only and is "
                "never bundled or read at runtime."
            )
        ors = _ors_smoke(data, args.from_city, args.to_city, api_key)
        print(
            "OpenRouteService HGV smoke: "
            f"{ors['miles']:.1f} miles, "
            f"{ors['points']} geometry points, "
            f"elevation {ors['min_ft']:.0f}-{ors['max_ft']:.0f} feet, "
            f"{ors['steepness_segments']} steepness segments, "
            f"tollway {'yes' if ors['has_tollway'] else 'no'}"
        )
    if args.ors_compare:
        api_key = ors_api_key()
        if api_key is None:
            raise SystemExit(
                f"--ors-compare needs the {ORS_API_KEY_ENV} environment "
                "variable and the tooling group (uv run --group tooling ...)."
            )
        cmp = _ors_compare(
            data, args.from_city, args.to_city, api_key, Path(args.cache_dir), args.rate_limit
        )
        current_grade = (
            f"{cmp['current_avg_grade_pct']}%"
            if cmp["current_avg_grade_pct"] is not None
            else "n/a"
        )
        print(
            "ORS vs checked-in: "
            f"miles {cmp['ors_miles']:.1f} vs {cmp['leg_miles']:.0f}; "
            f"terrain {cmp['ors_terrain']} vs {cmp['current_terrain']}; "
            f"avg grade {cmp['ors_avg_grade_pct']}% vs {current_grade}; "
            f"elevation {cmp['ors_min_ft']:.0f}-{cmp['ors_max_ft']:.0f} ft; "
            f"{cmp['ors_points']} ORS points; "
            f"tollway {'yes' if cmp['ors_has_tollway'] else 'no'} "
            f"(checked-in toll events: {cmp['current_toll_events']})"
        )
    if args.overpass_poi_smoke:
        pois = _overpass_poi_smoke(corridor)
        print(
            "Overpass POI smoke: "
            f"{pois['elements']} elements in corridor bounding box, "
            f"{pois['actionable_candidates']} actionable candidates"
        )
    return 0


if __name__ == "__main__":
    sys.exit(main())
