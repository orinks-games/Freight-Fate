"""Propose candidate new city nodes for the freight map (read-only).

Build-time helper for Workstream C (see docs/osm-routing-plan.md). Ranks US
populated places from GeoNames by population, drops cities already in the world
source and near-duplicate suburbs, and prints a candidate list with each
city's derived freight region so the regional balance is visible before anyone
adds legs. This never modifies the world source; choosing nodes, wiring
adjacencies, and ORS leg generation are separate, deliberate steps.

GeoNames is free under CC BY; keep attribution in any committed result. Data:
- https://download.geonames.org/export/dump/cities15000.zip
- https://download.geonames.org/export/dump/admin1CodesASCII.txt
"""

from __future__ import annotations

import argparse
import io
import json
import math
import urllib.request
import zipfile
from pathlib import Path
from typing import Any

from world_source import load_world

ROOT = Path(__file__).resolve().parents[1]
CACHE_PATH = ROOT / ".route-cache"
USER_AGENT = "Freight-Fate node-picker (https://github.com/orinks-games/Freight-Fate)"
GEONAMES_CITIES_URL = "https://download.geonames.org/export/dump/cities15000.zip"
GEONAMES_ADMIN1_URL = "https://download.geonames.org/export/dump/admin1CodesASCII.txt"
EARTH_RADIUS_MI = 3958.8
# Non-contiguous states/territories cannot join the continental truck network.
NON_CONTIGUOUS_STATES = frozenset(
    {
        "Alaska",
        "Hawaii",
        "Puerto Rico",
        "Guam",
        "American Samoa",
        "United States Virgin Islands",
        "Northern Mariana Islands",
    }
)


def _haversine_miles(lat1: float, lon1: float, lat2: float, lon2: float) -> float:
    phi1, phi2 = math.radians(lat1), math.radians(lat2)
    dphi = math.radians(lat2 - lat1)
    dlmb = math.radians(lon2 - lon1)
    a = math.sin(dphi / 2) ** 2 + math.cos(phi1) * math.cos(phi2) * math.sin(dlmb / 2) ** 2
    return EARTH_RADIUS_MI * 2 * math.asin(math.sqrt(a))


def parse_admin1(text: str) -> dict[str, str]:
    """Map GeoNames US admin1 codes (e.g. 'US.CA') to state names."""
    states: dict[str, str] = {}
    for line in text.splitlines():
        parts = line.split("\t")
        if len(parts) < 2 or not parts[0].startswith("US."):
            continue
        states[parts[0].split(".", 1)[1]] = parts[1]
    return states


def parse_geonames(cities_text: str, admin1: dict[str, str]) -> list[dict[str, Any]]:
    """Normalize the GeoNames cities dump to US populated places.

    Columns (tab-separated): 1 name, 4 lat, 5 lon, 8 country, 10 admin1, 14 pop.
    """
    rows: list[dict[str, Any]] = []
    for line in cities_text.splitlines():
        cols = line.split("\t")
        if len(cols) < 15 or cols[8] != "US":
            continue
        state = admin1.get(cols[10])
        if state is None or state in NON_CONTIGUOUS_STATES:
            continue
        try:
            rows.append(
                {
                    "name": cols[1],
                    "state": state,
                    "lat": float(cols[4]),
                    "lon": float(cols[5]),
                    "population": int(cols[14] or 0),
                }
            )
        except ValueError:
            continue
    return rows


def rank_candidates(
    rows: list[dict[str, Any]],
    existing_names: set[str],
    existing_coords: list[tuple[float, float]],
    *,
    top_n: int = 25,
    min_population: int = 100_000,
    dedupe_mi: float = 30.0,
) -> list[dict[str, Any]]:
    """Highest-population US cities not already on the map and not within
    ``dedupe_mi`` of an existing node (so we add real new metros, not suburbs)."""
    candidates: list[dict[str, Any]] = []
    chosen_coords = list(existing_coords)
    for row in sorted(rows, key=lambda r: r["population"], reverse=True):
        if row["population"] < min_population or row["name"] in existing_names:
            continue
        if any(
            _haversine_miles(row["lat"], row["lon"], lat, lon) <= dedupe_mi
            for lat, lon in chosen_coords
        ):
            continue
        candidates.append(row)
        chosen_coords.append((row["lat"], row["lon"]))
        if len(candidates) >= top_n:
            break
    return candidates


def _cached_bytes(cache_dir: Path, name: str, url: str) -> bytes:
    path = cache_dir / "geonames" / name
    if path.exists():
        return path.read_bytes()
    req = urllib.request.Request(url, headers={"User-Agent": USER_AGENT})
    with urllib.request.urlopen(req, timeout=60) as resp:
        payload = resp.read()
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(payload)
    return payload


def load_geonames(cache_dir: Path) -> list[dict[str, Any]]:
    admin1 = parse_admin1(
        _cached_bytes(cache_dir, "admin1CodesASCII.txt", GEONAMES_ADMIN1_URL).decode("utf-8")
    )
    zip_bytes = _cached_bytes(cache_dir, "cities15000.zip", GEONAMES_CITIES_URL)
    with zipfile.ZipFile(io.BytesIO(zip_bytes)) as zf:
        cities_text = zf.read("cities15000.txt").decode("utf-8")
    return parse_geonames(cities_text, admin1)


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        description="Propose candidate new city nodes from GeoNames (read-only)."
    )
    parser.add_argument("--top", type=int, default=25)
    parser.add_argument("--min-pop", type=int, default=100_000)
    parser.add_argument("--dedupe-mi", type=float, default=30.0)
    parser.add_argument("--cache-dir", default=str(CACHE_PATH))
    parser.add_argument("--json", action="store_true")
    args = parser.parse_args(argv)

    data = load_world()
    existing_names = set(data["cities"])
    existing_coords = [(float(c["lat"]), float(c["lon"])) for c in data["cities"].values()]

    rows = load_geonames(Path(args.cache_dir))
    candidates = rank_candidates(
        rows,
        existing_names,
        existing_coords,
        top_n=args.top,
        min_population=args.min_pop,
        dedupe_mi=args.dedupe_mi,
    )

    from ffworld.regions import classify_region

    for candidate in candidates:
        candidate["region"] = classify_region(
            candidate["state"], candidate["lat"], candidate["lon"]
        )

    if args.json:
        print(json.dumps(candidates, indent=2))
    else:
        print(
            f"Top {len(candidates)} candidate nodes (not in the current "
            f"{len(existing_names)}-city map), by population:"
        )
        for i, c in enumerate(candidates, 1):
            print(f"{i:2}. {c['name']}, {c['state']} ({c['population']:,}) -> {c['region']}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
