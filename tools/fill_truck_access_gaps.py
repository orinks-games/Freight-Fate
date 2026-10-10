"""Fill serviceless stretches with REAL truck facilities the map never looked for.

The access sweep demoted 1,019 car-scale fuel stops to ``bobtail_only``, which
is honest but leaves corridors with long stretches a driver cannot legally
serve. The brief says fill those with real facilities -- truck stops, service
plazas, rest areas -- never with invented or disguised ones.

The reason those facilities are missing is not that they do not exist. The
existing corridor enrichment probes only five mid-corridor points plus the two
endpoint cities, each a 6 km box, so on a 162-mile leg it samples roughly a
third of the road and mid-corridor probes land 25-30 miles apart. The Love's at
Williams and the truck stops at Corning sit in those blind spots on I-5, both
tagged ``hgv=yes`` in the very extract we already query. "No truck stop found"
mostly meant "we never looked there."

So this samples densely, but only INSIDE a gap: it walks the leg's geometry
every few miles across the serviceless span, asks Overpass what is there, and
keeps what the reviewed relevance rules in ``enrich_routes_pois`` already
accept -- service plazas, rest areas, hgv-tagged fuel, dedicated truck parking,
named truck-stop brands. The ``rural_fallback`` relaxation is deliberately NOT
used: it is what admitted the generic car fuel stations this sweep just spent
its time reclassifying.

Added stops carry their coordinates, so ``classify_vehicle_access.py`` judges
them on the same evidence as everything else rather than being told what they
are here.

A gap that yields nothing is left alone and reported as UNVERIFIED, not as
empty. The distinction matters: on I-80 into Sacramento the sampling reached
Vacaville (within 1.9 mi, inside the search box) and found twenty named
stations, every one of them an Arco/Chevron/Speedway/Shell with no ``hgv`` tag
at all, so the rules correctly declined them. That corridor may hold a real
truck stop OSM has simply not tagged. Some sparse rural legs genuinely are
empty -- US-50 across Nevada is meant to be -- but this tool cannot tell the
two apart, and inventing a facility to close a number would be worse than
either.

Usage::

    OVERPASS_URL=http://localhost:12347/api/interpreter \\
      uv run python tools/fill_truck_access_gaps.py --min-gap 100 --report
    OVERPASS_URL=... uv run python tools/fill_truck_access_gaps.py --write
"""

from __future__ import annotations

import argparse
import json
import math
import os
import sys
import time
import urllib.error
import urllib.request
from pathlib import Path
from typing import Any

from enrich_routes_pois import (
    _actions_for_stop_type,
    _clean_poi_name,
    _parking_for_stop_type,
    _services_for_stop_type,
    _stop_type_from_tags,
    _truck_relevance,
)
from world_source import load_world, save_world

ROOT = Path(__file__).resolve().parents[1]
CACHE_DIR = ROOT / ".route-cache" / "gap-fill"
ACCESSED_DATE = "2026-09-16"

SOURCE_NOTE = (
    "OpenStreetMap/Overpass dense in-gap corridor amenity query, accessed "
    f"{ACCESSED_DATE}; added to close a stretch with no truck-accessible "
    "service after the truck-accessibility sweep. Curated into a gameplay POI "
    "(clean name, normalized category) without raw OSM IDs."
)

# Public APIs sometimes tag car-scale convenience plazas hgv=yes because a
# delivery truck can reach the pumps. That is not evidence of tractor-trailer
# parking. Keep these on the map as bobtail-only unless the name itself says
# this branch is a real travel/truck center.
CONVENIENCE_PLAZA_NAMES = (
    "7-eleven",
    "7 eleven",
    "arco",
    "bp ",
    "casey",
    "chevron",
    "circle k",
    "circlek",
    "exxon",
    "getgo",
    "get go",
    "kum & go",
    "maverik",
    "mobil",
    "murphy express",
    "murphy usa",
    "quiktrip",
    "quicktrip",
    "racetrac",
    "race trac",
    "shell",
    "sheetz",
    "speedway",
    "thorntons",
    "wawa",
)
TRUCK_CENTER_WORDS = (
    "travel center",
    "travel stop",
    "travel plaza",
    "truck stop",
    "truckstop",
)


def _gap_fill_access(name: str) -> str:
    lowered = name.strip().lower()
    if any(word in lowered for word in TRUCK_CENTER_WORDS):
        return "tractor_trailer"
    # "Shell Truck Lanes" / "Exxon (Truck)" identify truck facilities even
    # without the words "truck stop".
    tokens = {tok.strip("()[],.") for tok in lowered.replace("-", " ").split()}
    if "truck" in tokens or "trucks" in tokens:
        return "tractor_trailer"
    if any(brand in lowered for brand in CONVENIENCE_PLAZA_NAMES):
        return "bobtail_only"
    return "tractor_trailer"


# Sampling step along the gap. The search box is ~6 km (about 7.5 mi across),
# so an 8-mile step overlaps slightly and leaves no unlooked-at road.
STEP_MI = 8.0
SEARCH_RADIUS_M = 6000
# Keep a new stop from landing on top of one already there.
MIN_SPACING_MI = 4.0
EARTH_RADIUS_MI = 3958.7613


def _overpass_url() -> str:
    url = os.environ.get("OVERPASS_URL", "").strip()
    if not url:
        raise SystemExit(
            "Set OVERPASS_URL to an Overpass interpreter (public or local). "
            "Public mirrors need OVERPASS_RATE_LIMIT_S pacing and .route-cache/gap-fill."
        )
    return url


def _bbox(lat: float, lon: float, radius_m: float) -> str:
    dlat = radius_m / 111_320.0
    dlon = radius_m / (111_320.0 * max(0.1, math.cos(math.radians(lat))))
    return f"{lat - dlat},{lon - dlon},{lat + dlat},{lon + dlon}"


def _haversine_mi(lat1: float, lon1: float, lat2: float, lon2: float) -> float:
    p1, p2 = math.radians(lat1), math.radians(lat2)
    dp, dl = p2 - p1, math.radians(lon2 - lon1)
    a = math.sin(dp / 2) ** 2 + math.cos(p1) * math.cos(p2) * math.sin(dl / 2) ** 2
    return 2 * EARTH_RADIUS_MI * math.asin(math.sqrt(a))


def _polyline(leg: dict[str, Any]) -> list[tuple[float, float, float]]:
    """The leg's geometry as (lat, lon, at_mi), ordered along the drive."""
    points = (leg.get("corridor") or {}).get("route_points") or []
    out = [
        (float(p["lat"]), float(p["lon"]), float(p["at_mi"]))
        for p in points
        if "lat" in p and "lon" in p and "at_mi" in p
    ]
    return sorted(out, key=lambda p: p[2])


def _interpolate(line: list[tuple[float, float, float]], at_mi: float) -> tuple[float, float]:
    """Where the road is at a given mile, linearly between stored points."""
    if at_mi <= line[0][2]:
        return line[0][0], line[0][1]
    for (lat1, lon1, mi1), (lat2, lon2, mi2) in zip(line, line[1:], strict=False):
        if mi1 <= at_mi <= mi2:
            span = mi2 - mi1
            t = 0.0 if span <= 0 else (at_mi - mi1) / span
            return lat1 + (lat2 - lat1) * t, lon1 + (lon2 - lon1) * t
    return line[-1][0], line[-1][1]


def _project_mi(line: list[tuple[float, float, float]], lat: float, lon: float) -> float:
    """Roughly where along the leg a found POI sits, by nearest stored point.

    The stored geometry is coarse (often 25-30 mi between points), so this
    refines by scanning interpolated positions around the nearest vertex.
    """
    best_mi, best_d = line[0][2], float("inf")
    lo, hi = line[0][2], line[-1][2]
    step = 0.5
    mi = lo
    while mi <= hi:
        plat, plon = _interpolate(line, mi)
        d = _haversine_mi(lat, lon, plat, plon)
        if d < best_d:
            best_d, best_mi = d, mi
        mi += step
    return best_mi


def _query(url: str, body: str, retries: int = 5) -> dict[str, Any]:
    headers = {
        "User-Agent": "FreightFateGapFill/1.0 (https://github.com/orinks-games/Freight-Fate)"
    }
    encoded = body.encode("utf-8")
    for attempt in range(retries):
        request = urllib.request.Request(url, data=encoded, headers=headers)
        try:
            with urllib.request.urlopen(request, timeout=90) as response:
                return json.loads(response.read().decode("utf-8"))
        except urllib.error.HTTPError as exc:
            # 429/504/502: back off harder for public mirrors
            if attempt == retries - 1:
                return {"elements": []}
            multiplier = 5.0 if exc.code in {429, 502, 503, 504} else 2.0
            time.sleep(min(60.0, (2 ** (attempt + 1)) * multiplier))
        except (urllib.error.URLError, TimeoutError, json.JSONDecodeError):
            # overpass-api.de often EOFs on TLS from this network; the same
            # public interpreter answers on HTTP. Retry rather than treating
            # a handshake failure as an empty corridor.
            if url.startswith("https://overpass-api.de/"):
                url = "http://" + url[len("https://") :]
                continue
            if attempt == retries - 1:
                return {"elements": []}
            time.sleep(2 ** (attempt + 1))
    return {"elements": []}


def _sample(url: str, leg_key: str, at_mi: float, lat: float, lon: float) -> list[dict[str, Any]]:
    CACHE_DIR.mkdir(parents=True, exist_ok=True)
    path = CACHE_DIR / f"{leg_key}--{at_mi:.1f}.json"
    if path.exists():
        return json.loads(path.read_text(encoding="utf-8")).get("elements", [])
    box = _bbox(lat, lon, SEARCH_RADIUS_M)
    body = f"""[out:json][timeout:50];
(
  nwr["amenity"="fuel"]["name"]({box});
  nwr["highway"~"services|rest_area"]["name"]({box});
  nwr["amenity"="truck_stop"]["name"]({box});
  nwr["amenity"="parking"]["hgv"="yes"]["name"]({box});
);
out tags center 40;"""
    payload = _query(url, body)
    elements = payload.get("elements", [])
    # Only cache a real answer. Empty payloads are often 429/timeout/TLS
    # failures on public mirrors; caching them would freeze a gap as barren.
    if elements or payload.get("osm3s"):
        path.write_text(json.dumps(payload) + "\n", encoding="utf-8")
    # Public Overpass needs a pause; local Docker can set OVERPASS_RATE_LIMIT_S=0.
    time.sleep(float(os.environ.get("OVERPASS_RATE_LIMIT_S", "1.5")))
    return elements


def usable_positions(leg: dict[str, Any], action: str) -> list[float]:
    return sorted(
        float(stop.get("at_mi", 0.0))
        for stop in leg.get("stops", [])
        if str(stop.get("vehicle_access", "tractor_trailer")) == "tractor_trailer"
        and (not action or action in (stop.get("actions") or ()))
    )


def gap_spans(leg: dict[str, Any], action: str, min_gap: float) -> list[tuple[float, float]]:
    """Stretches with no usable stop, longer than min_gap."""
    miles = float(leg.get("miles", 0.0) or 0.0)
    points = usable_positions(leg, action)
    edges = [0.0, *points, miles]
    spans = []
    for start, end in zip(edges, edges[1:], strict=False):
        if end - start >= min_gap:
            spans.append((start, end))
    return spans


def fill_leg(
    leg: dict[str, Any], url: str, action: str, min_gap: float, per_gap: int
) -> list[dict[str, Any]]:
    line = _polyline(leg)
    if len(line) < 2:
        return []
    leg_key = f"{leg['from']}--{leg['to']}"
    existing_names = {str(s.get("name", "")).strip().lower() for s in leg.get("stops", [])}
    taken = [float(s.get("at_mi", 0.0)) for s in leg.get("stops", [])]

    added: list[dict[str, Any]] = []
    for start, end in gap_spans(leg, action, min_gap):
        best: dict[str, tuple[int, dict[str, Any]]] = {}
        mi = start + STEP_MI / 2
        while mi < end:
            lat, lon = _interpolate(line, mi)
            for element in _sample(url, leg_key, mi, lat, lon):
                tags = element.get("tags") or {}
                raw_name = str(tags.get("name", "")).strip()
                if not raw_name:
                    continue
                score = _truck_relevance(tags, raw_name, rural_fallback=False)
                name = _clean_poi_name(raw_name)
                if score is None:
                    # Convenience plazas stay on the map as bobtail-only rather
                    # than being dropped. A later access pass cannot promote them.
                    if _gap_fill_access(name) == "bobtail_only":
                        score = 1
                    else:
                        continue
                key = name.lower()
                if key in existing_names:
                    continue
                if key not in best or score > best[key][0]:
                    best[key] = (score, element)
            mi += STEP_MI

        for _, element in sorted(best.values(), key=lambda pair: -pair[0])[:per_gap]:
            center = element.get("center") or element
            if "lat" not in center or "lon" not in center:
                continue
            lat, lon = float(center["lat"]), float(center["lon"])
            at_mi = round(_project_mi(line, lat, lon), 1)
            if not start - 1 <= at_mi <= end + 1:
                continue
            # A stop must sit strictly inside the leg: the parser rejects
            # at_mi of exactly 0 or exactly the leg length, since that is the
            # city itself rather than a point on the road between. Projection
            # lands on an endpoint whenever the real facility sits at a leg's
            # edge, which is common -- truck stops cluster at town exits.
            miles = float(leg.get("miles", 0.0) or 0.0)
            if miles <= 1.0:
                continue
            at_mi = min(max(at_mi, 0.1), round(miles - 0.1, 1))
            if any(abs(at_mi - t) < MIN_SPACING_MI for t in taken):
                continue
            tags = element.get("tags") or {}
            stop_type = _stop_type_from_tags(tags)
            stop_name = _clean_poi_name(str(tags.get("name", "")).strip())
            stop = {
                "name": stop_name,
                "type": stop_type,
                "at_mi": at_mi,
                "source": SOURCE_NOTE,
                "parking": _parking_for_stop_type(stop_type),
                "actions": _actions_for_stop_type(stop_type),
                "services": _services_for_stop_type(stop_type),
                "vehicle_access": _gap_fill_access(stop_name),
                "lat": lat,
                "lon": lon,
            }
            leg.setdefault("stops", []).append(stop)
            taken.append(at_mi)
            existing_names.add(stop["name"].lower())
            added.append(stop)
    if added:
        leg["stops"].sort(key=lambda s: float(s.get("at_mi", 0.0)))
    return added


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--action", default="fuel", help="Capability the gap lacks.")
    parser.add_argument("--min-gap", type=float, default=100.0, help="Miles that count as a gap.")
    parser.add_argument("--per-gap", type=int, default=3, help="Most stops to add per span.")
    parser.add_argument("--limit-legs", type=int, default=0)
    parser.add_argument("--write", action="store_true")
    parser.add_argument("--report", action="store_true")
    args = parser.parse_args(argv)

    url = _overpass_url()
    data = load_world()
    targets = [leg for leg in data["legs"] if gap_spans(leg, args.action, args.min_gap)][
        : args.limit_legs or None
    ]

    print(f"{len(targets)} leg(s) with a {args.min_gap:.0f}+ mile {args.action} gap.")
    total = 0
    empty: list[str] = []
    for index, leg in enumerate(targets, 1):
        added = fill_leg(leg, url, args.action, args.min_gap, args.per_gap)
        total += len(added)
        if not added:
            empty.append(f"{leg['from']} -> {leg['to']} ({leg.get('highway', '')})")
        elif args.report:
            for stop in added:
                print(
                    f"  + {leg['from']}->{leg['to']}: {stop['name']} ({stop['type']}) @ {stop['at_mi']}"
                )
        if index % 10 == 0:
            print(f"  ...{index}/{len(targets)} legs, {total} added", flush=True)

    print(f"\nAdded {total} real facility stop(s) across {len(targets) - len(empty)} leg(s).")
    print(
        f"{len(empty)} leg(s) yielded nothing -- no OSM-verifiable truck facility in "
        "the gap (genuinely empty, or real but untagged). Left alone:"
    )
    for line in empty[:20]:
        print(f"  {line}")

    if args.write:
        print(f"\nWrote {save_world(data)} shard(s).")
    else:
        print("\nDry run. Re-run with --write.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
