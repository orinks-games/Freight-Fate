"""Mark the truck stops that have a CAT Scale, from OpenStreetMap.

A CAT Scale is the certified commercial scale on a truck stop's lot where a
driver pays to learn their axle weights before a state scale does. Mappers tag
them ``amenity=weighbridge`` with ``brand=CAT Scale``: 2,127 in the United
States when this was first read, out of 3,819 weighbridges (the rest are farm,
quarry and enforcement scales; ``docs/data-sources.md`` section 5).

Two steps, so the second is deterministic and offline:

    uv run python tools/cat_scales.py --refresh [SAVED_JSON]
        Asks Overpass for every U.S. weighbridge (or reads a saved response to
        QUERY when the public server is busy), keeps the CAT-branded ones and
        writes ``tools/cat_scales_snapshot.json`` (committed). READ values,
        (c) OpenStreetMap contributors, ODbL.

    uv run python tools/cat_scales.py            # dry run, prints calibration
    uv run python tools/cat_scales.py --write    # apply, then tools/index_world.py
        Gives every truck stop within MATCH_MI of a CAT Scale the ``scale``
        service (the key the brand-page curation already uses for a CAT scale)
        and a provenance sentence in its ``source``. A stop this tool marked
        earlier that no longer matches loses both; a ``scale`` read from a
        brand's own location page is never removed.

MATCH_MI is calibrated, not tuned. Distance from each travel center that
carries a coordinate to its nearest CAT Scale, 2026-09-24 snapshot, 1,102
distinct records: 844 within 0.1 mi, 866 within 0.2, 868 within 0.25, 880
within 0.5, 891 within a mile. The scale sits on the lot, so the count climbs
steeply to about 0.2 mi and then flattens; past 0.25 mi the few extra matches
are a neighbouring stop's scale. A ``fuel_station`` within the same distance
(40 of 922) is a car pump beside a truck stop, so fuel stations never match.
"""

from __future__ import annotations

import argparse
import bisect
import json
import math
import sys
import urllib.parse
import urllib.request
from pathlib import Path
from typing import Any

sys.path.insert(0, str(Path(__file__).resolve().parent))

from world_source import load_world  # noqa: E402

SNAPSHOT_PATH = Path(__file__).resolve().parent / "cat_scales_snapshot.json"
OVERPASS_URL = "https://overpass-api.de/api/interpreter"
USER_AGENT = "Freight-Fate data tools (https://github.com/orinks-games/Freight-Fate)"
QUERY = (
    '[out:json][timeout:190];area["ISO3166-1"="US"][admin_level=2]->.us;'
    'nwr["amenity"="weighbridge"](area.us);out center tags;'
)
EARTH_RADIUS_MI = 3958.7613
MATCH_MI = 0.25
STOP_TYPES = ("travel_center", "truck_stop", "service_plaza")
SERVICE = "scale"
MARKER = "CAT Scale read from OpenStreetMap"
NOTE = (
    f"{MARKER} (amenity=weighbridge, brand CAT Scale) within {MATCH_MI} mi of the "
    "store; Overpass snapshot {accessed}, tools/cat_scales.py."
)


def haversine_mi(lat1: float, lon1: float, lat2: float, lon2: float) -> float:
    p1, p2 = math.radians(lat1), math.radians(lat2)
    dp = p2 - p1
    dl = math.radians(lon2 - lon1)
    a = math.sin(dp / 2) ** 2 + math.cos(p1) * math.cos(p2) * math.sin(dl / 2) ** 2
    return 2 * EARTH_RADIUS_MI * math.asin(math.sqrt(a))


def is_cat_scale(tags: dict[str, str]) -> bool:
    """A weighbridge whose brand, name or operator is CAT Scale."""
    text = " ".join(tags.get(k, "") for k in ("brand", "name", "operator")).lower()
    return "cat scale" in text or "catscale" in text


def cat_points(overpass: dict[str, Any]) -> list[list[float]]:
    """``[osm_id, lat, lon]`` for each CAT-branded element, sorted by id."""
    points = []
    for element in overpass.get("elements", []):
        if not is_cat_scale(element.get("tags", {})):
            continue
        where = element if "lat" in element else element.get("center", {})
        if "lat" not in where:
            continue
        osm_id = f"{element['type'][0]}{element['id']}"
        points.append([osm_id, round(where["lat"], 6), round(where["lon"], 6)])
    return sorted(points)


def refresh(saved: str | None = None) -> None:
    """Fetch, or read ``saved``: the public server is often too busy, and the
    same QUERY run by hand (curl --data-urlencode) is the same reading."""
    if saved:
        overpass = json.loads(Path(saved).read_text(encoding="utf-8"))
    else:
        body = urllib.parse.urlencode({"data": QUERY}).encode()
        headers = {"User-Agent": USER_AGENT}
        request = urllib.request.Request(OVERPASS_URL, data=body, headers=headers)
        with urllib.request.urlopen(request, timeout=240) as response:
            overpass = json.load(response)
    accessed = str(overpass["osm3s"]["timestamp_osm_base"])[:10]
    points = cat_points(overpass)
    snapshot = {
        "source": f"OpenStreetMap via Overpass ({OVERPASS_URL}), amenity=weighbridge in the "
        f"United States, kept where brand, name or operator is CAT Scale. Data as of {accessed}.",
        "licence": "(c) OpenStreetMap contributors, ODbL 1.0",
        "accessed": accessed,
        "weighbridges": len(overpass.get("elements", [])),
        "points": points,
    }
    # One point per line, so a refresh diffs as the scales that moved.
    head = json.dumps(snapshot, indent=1).split('\n "points"')[0]
    rows = ",\n".join(f"  {json.dumps(p)}" for p in points)
    SNAPSHOT_PATH.write_text(f'{head}\n "points": [\n{rows}\n ]\n}}\n', encoding="utf-8")
    print(
        f"{len(points)} CAT Scales of {snapshot['weighbridges']} weighbridges -> {SNAPSHOT_PATH.name}"
    )


def nearest_mi(lat: float, lon: float, points: list[list[float]]) -> float:
    # ponytail: linear scan, 2,127 points x ~1,300 stops is a second; grid it if the map grows 10x.
    return min(haversine_mi(lat, lon, p[1], p[2]) for p in points)


def strip_note(source: str) -> str:
    at = source.find(MARKER)
    return source[:at].rstrip() if at >= 0 else source


def apply(data: dict[str, Any], snapshot: dict[str, Any]) -> dict[str, int]:
    points = snapshot["points"]
    note = NOTE.format(accessed=snapshot["accessed"])
    counts = {"marked": 0, "unmarked": 0, "already listed": 0}
    for leg in data["legs"]:
        for stop in leg.get("stops", []):
            if stop.get("type") not in STOP_TYPES:
                continue
            services = stop.setdefault("services", [])
            source = str(stop.get("source", ""))
            ours = MARKER in source
            near = "lat" in stop and nearest_mi(stop["lat"], stop["lon"], points) <= MATCH_MI
            if near and not ours:
                if SERVICE in services:
                    counts["already listed"] += 1
                    continue
                services.append(SERVICE)
                stop["source"] = f"{source} {note}".strip()
                counts["marked"] += 1
            elif ours and not near:
                if SERVICE in services:
                    services.remove(SERVICE)
                stop["source"] = strip_note(source)
                counts["unmarked"] += 1
            elif ours:
                stop["source"] = f"{strip_note(source)} {note}".strip()
    return counts


def calibration(data: dict[str, Any], points: list[list[float]]) -> None:
    seen: set[tuple[str, float, float]] = set()
    by_type: dict[str, list[float]] = {}
    for leg in data["legs"]:
        for stop in leg.get("stops", []):
            if "lat" not in stop:
                continue
            key = (stop["name"], round(stop["lat"], 4), round(stop["lon"], 4))
            if key in seen:
                continue
            seen.add(key)
            by_type.setdefault(stop.get("type", ""), []).append(
                nearest_mi(stop["lat"], stop["lon"], points)
            )
    for kind in (*STOP_TYPES, "fuel_station"):
        dists = sorted(by_type.get(kind, []))
        steps = [(mi, bisect.bisect_right(dists, mi)) for mi in (0.1, 0.2, 0.25, 0.5, 1.0)]
        print(
            f"  {kind}: {len(dists)} distinct, within "
            + ", ".join(f"{mi} mi {n}" for mi, n in steps)
        )


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--refresh",
        nargs="?",
        const="",
        metavar="SAVED_JSON",
        help="re-read Overpass (or a saved response of QUERY) into the snapshot",
    )
    parser.add_argument("--write", action="store_true", help="apply to data/world_source")
    args = parser.parse_args(argv)
    if args.refresh is not None:
        refresh(args.refresh or None)
        return 0
    snapshot = json.loads(SNAPSHOT_PATH.read_text(encoding="utf-8"))
    data = load_world()
    print(
        f"{len(snapshot['points'])} CAT Scales, snapshot {snapshot['accessed']}. Nearest CAT Scale:"
    )
    calibration(data, snapshot["points"])
    counts = apply(data, snapshot)
    print(f"Stop records: {counts}")
    if args.write:
        from nonchain_plazas import save_keeping_spelling

        print(f"Wrote {save_keeping_spelling(data)} shard(s). Now run tools/index_world.py.")
    else:
        print("Dry run. Re-run with --write.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
