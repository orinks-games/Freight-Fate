# ruff: noqa: F401,I001
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
# Prefer a self-hosted Overpass when OVERPASS_URL is set in the environment
# (e.g. a Docker instance on the LAN or tailnet, like the self-hosted ORS); it
# is tried first, with the public mirrors as fallback. Build-time only, mirroring
# ORS_BASE_URL -- never read at runtime.
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


def spoken_state(data, value):
    """Full state name for a city ``state`` value.

    Post-slug world data stores 2-letter codes on cities while corridor
    metadata (state crossings, state miles, Census/OSM context) keeps full
    names. Codes resolve through the world's geo lookup; full names and
    unknown values pass through unchanged.
    """
    value = str(value or "").strip()
    for country in data.get("geo", {}).get("countries", {}).values():
        states = country.get("states", {})
        if value in states:
            return str(states[value])
    return value


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


__all__ = [name for name in globals() if not name.startswith("__")]
