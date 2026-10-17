"""Name the short speed-limit postings that are not on the leg's own highway.

Background
----------
The baked maxspeed profile (``corridor.speed_limits``) was resampled from
OpenStreetMap ways snapped to the route geometry. Where that geometry runs
over an interchange ramp or turns onto a city street for a few hundred yards,
the bake picks up the ramp's or the street's limit, and the driver hears the
limit drop and come back with nothing on the highway to explain it (US-95 in
Las Vegas taking a 15 mph loop, I-80 in Reno a 45 and then a 25 ramp).

The load-time dwell filter (``dwell_filter_speed_limits``) already drops the
postings too short to be signs. What it keeps and this tool looks at are the
drop-and-return postings that hold long enough to be heard: the limit falls
from A to B and comes back to A within ``MAX_RUN_MI``.

The discriminator
-----------------
Length does not separate real zones from artifacts. A hand-checked sample of
100 postings (``speed-limit-artifacts.md`` in the project research notes,
2026-10-17) found real town zones from 0.2 to 2.3 miles and artifacts from
0.2 to 2.5; every length cut folded about as many real zones as artifacts.

What separates them is whether the leg's own highway posts that limit there.
Along the posting, every ``STEP_MI``, the tool finds the nearest highway way
carrying a maxspeed within ``NEAR_M`` of the route and asks:

* ``on_b`` -- it carries the leg's route number and posts B;
* ``on_other`` -- it carries the route number and posts something else;
* ``off_route`` -- it is a different road;
* ``no_road`` -- no tagged mainline way is that close (mostly ramps, which
  this index leaves out).

A posting is on its highway when ``on_b`` covers at least ``ON_HIGHWAY_MIN``
of it. Otherwise, when one of the other three covers at least
``VERDICT_MIN``, it is named here with that reason. Anything in between
("mixed") is left alone: no reading says it is wrong.

On the sample this flagged all 9 artifacts the game still played and 8 of 49
real zones (a highway whose ref OSM spells differently, or a town street
with no ref). The loader then keeps any named posting that has a village or
town within ``PLACE_GUARD_MI``, which brought the real zones folded to none
and still folded 5 of the 9 artifacts. Both numbers come from that sample,
not from the full set.

Output
------
``world_data/us/gameplay/limit_artifacts.jsonl``: a meta line, then one row
per named posting keyed by ``(leg, at_mi, mph)``. The corridor's
``speed_limits`` are never edited. ``ff_core::data::world_parsing::limits``
folds a named posting back into its neighbours at load, unless the map has a
village landmark or a town checkpoint within ``PLACE_GUARD_MI``, so the guard
can be re-judged without re-running this tool.

Inputs
------
Geofabrik state extracts in ``OSM_REGION_CACHE_DIR`` (``<slug>-latest.osm.pbf``),
read with pyosmium (the tooling dependency group). A state with no extract can
be read from a way cache in the same directory (``<slug>-latest.legspeed.json``:
``{"ways": [[id, tags, [[lat, lon], ...]], ...]}``) with ``--allow-way-cache``.
Legs touching a state with neither are skipped and listed in the meta line, so
a partial run says so.

Run after any change to the speed-limit bake, the route geometry, or the
dwell filter's constants::

    uv run python tools/screen_limit_artifacts.py --region-dir PATH
    uv run python tools/screen_limit_artifacts.py --check   # list matches the data
"""

from __future__ import annotations

import argparse
import hashlib
import json
import math
import re
import sys
from collections import Counter, defaultdict
from collections.abc import Iterable
from dataclasses import dataclass
from pathlib import Path
from typing import Any

sys.path.insert(0, str(Path(__file__).resolve().parent))

import leg_geometry as lg  # noqa: E402

ROOT = Path(__file__).resolve().parents[1]
LEGS_DIR = ROOT / "data" / "world_data" / "us" / "legs"
OUTPUT_PATH = ROOT / "data" / "world_data" / "us" / "gameplay" / "limit_artifacts.jsonl"
OSM_REGION_CACHE_DIR = Path.home() / ".cache" / "freight-fate-osm" / "regions"

# The longest artifact in the sample ran 2.5 miles, and that is also the span
# the arrival line still speaks as "for N tenths of a mile".
MAX_RUN_MI = 2.5
STEP_MI = 0.02
EDGE_MI = 0.05  # skip the posting's ends, where the bake's own sampling blurs
NEAR_M = 40.0
ON_HIGHWAY_MIN = 0.3
VERDICT_MIN = 0.5
PLACE_GUARD_MI = 0.25
WAY_CLASSES = ("motorway", "trunk", "primary", "secondary", "tertiary")
PLACE_CHECKPOINT_TYPES = ("place", "village", "town", "hamlet")

# Mirrors of the dwell filter in crates/ff-core/src/data/world_constants.rs;
# tests/test_screen_limit_artifacts.py pins them to the Rust values.
LIMIT_DWELL_REAL_S = 6.0
LIMIT_PLACE_DWELL_REAL_S = 3.0
LIMIT_PLACE_NEAR_MI = 1.0
LIMIT_PLACE_TOWN_MPH = 45.0
LIMIT_DWELL_REFERENCE_SCALE = 20.0
LIMIT_DWELL_LOW_SPEED_SCALE = 4.0
LIMIT_DWELL_FULL_COMPRESSION_MPH = 50.0
LIMIT_DWELL_FALLBACK_MPH = 55.0

REASONS = {
    "off_route": "the route there runs on a different road than the leg's highway",
    "other_limit": "the leg's highway posts a different limit there",
    "no_road": "no mainline way within 40 m of the route there (an interchange ramp)",
}
SOURCE = (
    "Derived: OpenStreetMap highway ways carrying maxspeed (maxspeed:hgv preferred), "
    "Geofabrik state extracts accessed 2026-09-24, matched to the checked-in route "
    "geometry archive; a posting is named when the nearest way along it is not the "
    "leg's highway posting that limit. Thresholds calibrated on a hand-checked "
    "sample of 100 postings, 2026-10-17. Development-time screen, see "
    "tools/screen_limit_artifacts.py."
)

_CELL_DEG = 0.005
_M_PER_DEG = 111_000.0


# --- the played profile (Python copy of the Rust dwell filter) ---------------


def dwell_floor_mi(seconds: float, mph: float | None) -> float:
    speed = max(mph if mph is not None else LIMIT_DWELL_FALLBACK_MPH, 5.0)
    ramp = min(speed / LIMIT_DWELL_FULL_COMPRESSION_MPH, 1.0)
    scale = (
        LIMIT_DWELL_LOW_SPEED_SCALE
        + (LIMIT_DWELL_REFERENCE_SCALE - LIMIT_DWELL_LOW_SPEED_SCALE) * ramp
    )
    return seconds * speed * scale / 3600.0


def dwell_filter(
    samples: list[dict[str, Any]], leg_miles: float, places: list[float]
) -> list[dict[str, Any]]:
    """The postings the game keeps, exactly as ``dwell_filter_speed_limits``."""
    if not samples:
        return []
    kept = [samples[0]]
    for i, sample in enumerate(samples[1:], start=1):
        run_end = samples[i + 1]["at_mi"] if i + 1 < len(samples) else leg_miles
        last = kept[-1]
        mph, last_mph = sample["mph"], last["mph"]
        lowers = mph is not None and last_mph is not None and mph < last_mph
        town = lowers and mph <= LIMIT_PLACE_TOWN_MPH
        explained = town and any(abs(sample["at_mi"] - at) <= LIMIT_PLACE_NEAR_MI for at in places)
        dwell_s = LIMIT_PLACE_DWELL_REAL_S if explained else LIMIT_DWELL_REAL_S
        if run_end - sample["at_mi"] < dwell_floor_mi(dwell_s, mph):
            continue
        if mph == last_mph:
            continue
        kept.append(sample)
    return kept


def village_miles(corridor: dict[str, Any]) -> list[float]:
    return [
        float(lm["at_mi"])
        for lm in corridor.get("landmarks", ())
        if lm.get("category") == "village"
    ]


def place_miles(corridor: dict[str, Any]) -> list[float]:
    """Everything the loader's guard counts as a reason: village landmarks and
    town checkpoints."""
    return village_miles(corridor) + [
        float(cp["at_mi"])
        for cp in corridor.get("checkpoints", ())
        if cp.get("type") in PLACE_CHECKPOINT_TYPES
    ]


def played_profile(leg: dict[str, Any]) -> list[dict[str, Any]]:
    corridor = leg.get("corridor") or {}
    samples = sorted(
        (
            {"at_mi": float(s["at_mi"]), "mph": None if s.get("mph") is None else float(s["mph"])}
            for s in corridor.get("speed_limits", ())
        ),
        key=lambda s: s["at_mi"],
    )
    return dwell_filter(samples, float(leg["miles"]), village_miles(corridor))


@dataclass(frozen=True, slots=True)
class Candidate:
    leg: str
    highway: str
    at_mi: float
    mph: float
    from_mph: float
    run_mi: float


def candidates_of(leg: dict[str, Any]) -> list[Candidate]:
    """Played drop-and-return postings shorter than ``MAX_RUN_MI``."""
    profile = played_profile(leg)
    out: list[Candidate] = []
    for a, b, c in zip(profile, profile[1:], profile[2:], strict=False):
        if a["mph"] is None or b["mph"] is None or c["mph"] is None:
            continue
        if a["mph"] != c["mph"] or b["mph"] >= a["mph"]:
            continue
        run = c["at_mi"] - b["at_mi"]
        if run < MAX_RUN_MI:
            out.append(
                Candidate(
                    leg=lg.leg_id_of(leg),
                    highway=str(leg.get("highway", "")),
                    at_mi=b["at_mi"],
                    mph=b["mph"],
                    from_mph=a["mph"],
                    run_mi=round(run, 2),
                )
            )
    return out


def place_guarded(leg: dict[str, Any], cand: Candidate) -> bool:
    lo, hi = cand.at_mi - PLACE_GUARD_MI, cand.at_mi + cand.run_mi + PLACE_GUARD_MI
    return any(lo <= at <= hi for at in place_miles(leg.get("corridor") or {}))


# --- the OSM side --------------------------------------------------------------


def route_number(highway: str) -> str:
    digits = re.findall(r"\d+", str(highway))
    return digits[0] if digits else ""


def ref_carries(ref: str, number: str) -> bool:
    """Whether an OSM ``ref`` (``"US 19;GA 300"``) names this route number."""
    if not number:
        return False
    return any(re.findall(r"\d+", part)[:1] == [number] for part in re.split(r"[;/]", ref))


def way_mph(tags: dict[str, str]) -> float | None:
    from enrich_routes_pois import parse_osm_maxspeed

    hgv = parse_osm_maxspeed(tags.get("maxspeed:hgv"))
    return hgv if hgv is not None else parse_osm_maxspeed(tags.get("maxspeed"))


@dataclass(frozen=True, slots=True)
class Way:
    ref: str
    mph: float | None
    coords: tuple[tuple[float, float], ...]


def _cell(lat: float, lon: float) -> tuple[int, int]:
    return (math.floor(lat / _CELL_DEG), math.floor(lon / _CELL_DEG))


def _seg_m(lat: float, lon: float, a: tuple[float, float], b: tuple[float, float]) -> float:
    k = math.cos(math.radians(lat))
    px, py = lon * k, lat
    ax, ay, bx, by = a[1] * k, a[0], b[1] * k, b[0]
    dx, dy = bx - ax, by - ay
    length = dx * dx + dy * dy
    t = 0.0 if length == 0 else max(0.0, min(1.0, ((px - ax) * dx + (py - ay) * dy) / length))
    return math.hypot(px - (ax + t * dx), py - (ay + t * dy)) * _M_PER_DEG


class WayIndex:
    """Way segments bucketed by grid cell, for nearest-way lookups."""

    def __init__(self) -> None:
        self.ways: list[Way] = []
        self.grid: dict[tuple[int, int], list[tuple[int, int]]] = defaultdict(list)

    def add(self, way: Way) -> None:
        wi = len(self.ways)
        self.ways.append(way)
        for si, (a, b) in enumerate(zip(way.coords, way.coords[1:], strict=False)):
            for gx in range(
                math.floor(min(a[0], b[0]) / _CELL_DEG), math.floor(max(a[0], b[0]) / _CELL_DEG) + 1
            ):
                for gy in range(
                    math.floor(min(a[1], b[1]) / _CELL_DEG),
                    math.floor(max(a[1], b[1]) / _CELL_DEG) + 1,
                ):
                    self.grid[(gx, gy)].append((wi, si))

    def nearest(self, lat: float, lon: float, tol_m: float = NEAR_M) -> Way | None:
        best: tuple[float, int] | None = None
        gx, gy = _cell(lat, lon)
        for dx in (-1, 0, 1):
            for dy in (-1, 0, 1):
                for wi, si in self.grid.get((gx + dx, gy + dy), ()):
                    c = self.ways[wi].coords
                    d = _seg_m(lat, lon, c[si], c[si + 1])
                    if d < tol_m and (best is None or d < best[0]):
                        best = (d, wi)
        return None if best is None else self.ways[best[1]]


def classify(
    cand: Candidate, point_at: Any, index: WayIndex
) -> tuple[str | None, dict[str, float]]:
    """``(reason or None, fractions)`` for one candidate posting."""
    number = route_number(cand.highway)
    counts: Counter[str] = Counter()
    mile = cand.at_mi + EDGE_MI
    end = cand.at_mi + cand.run_mi - EDGE_MI
    steps = 0
    while mile <= end + 1e-9:
        lat, lon = point_at(mile)
        way = index.nearest(lat, lon)
        if way is None:
            counts["no_road"] += 1
        elif ref_carries(way.ref, number):
            counts["on_b" if way.mph == cand.mph else "other_limit"] += 1
        else:
            counts["off_route"] += 1
        steps += 1
        mile += STEP_MI
    if steps == 0:
        return None, {}
    fractions = {k: round(v / steps, 3) for k, v in sorted(counts.items())}
    if fractions.get("on_b", 0.0) >= ON_HIGHWAY_MIN:
        return None, fractions
    for reason in REASONS:
        if fractions.get(reason, 0.0) >= VERDICT_MIN:
            return reason, fractions
    return None, fractions


def point_interpolator(geom: list[tuple[float, float, float]]) -> Any:
    miles = [g[2] for g in geom]

    def at(mile: float) -> tuple[float, float]:
        from bisect import bisect_left

        i = min(max(bisect_left(miles, mile), 1), len(geom) - 1)
        (a0, b0, m0), (a1, b1, m1) = geom[i - 1], geom[i]
        t = 0.0 if m1 <= m0 else min(max((mile - m0) / (m1 - m0), 0.0), 1.0)
        return (a0 + t * (a1 - a0), b0 + t * (b1 - b0))

    return at


def _wanted_cells(points: Iterable[tuple[float, float]]) -> set[tuple[int, int]]:
    cells: set[tuple[int, int]] = set()
    for lat, lon in points:
        gx, gy = _cell(lat, lon)
        cells.update((gx + dx, gy + dy) for dx in (-1, 0, 1) for dy in (-1, 0, 1))
    return cells


def _keep_way(tags: dict[str, str]) -> bool:
    return tags.get("highway") in WAY_CLASSES and ("maxspeed" in tags or "maxspeed:hgv" in tags)


def ways_from_pbf(pbf: Path, cells: set[tuple[int, int]]) -> list[Way]:
    try:
        import osmium  # type: ignore[import-not-found]
    except ImportError as exc:  # pragma: no cover - depends on the tooling group
        raise SystemExit("Reading PBFs needs pyosmium: uv sync --group dev") from exc
    out: list[Way] = []
    processor = (
        osmium.FileProcessor(str(pbf), osmium.osm.NODE | osmium.osm.WAY)
        .with_locations("flex_mem")
        .with_filter(osmium.filter.KeyFilter("highway"))
    )
    for obj in processor:
        if not obj.is_way():
            continue
        tags = {k: v for k, v in obj.tags}
        if not _keep_way(tags):
            continue
        coords = tuple(
            (nd.location.lat, nd.location.lon) for nd in obj.nodes if nd.location.valid()
        )
        if len(coords) >= 2 and any(_cell(*c) in cells for c in coords):
            out.append(Way(ref=tags.get("ref", ""), mph=way_mph(tags), coords=coords))
    return out


def ways_from_cache(path: Path, cells: set[tuple[int, int]]) -> list[Way]:
    payload = json.loads(path.read_text(encoding="utf-8"))
    out: list[Way] = []
    for _wid, tags, pts in payload["ways"]:
        if not _keep_way(tags):
            continue
        coords = tuple((float(p[0]), float(p[1])) for p in pts)
        if len(coords) >= 2 and any(_cell(*c) in cells for c in coords):
            out.append(Way(ref=tags.get("ref", ""), mph=way_mph(tags), coords=coords))
    return out


# --- driver --------------------------------------------------------------------


def state_slug(state: str) -> str:
    return "-".join(str(state).strip().lower().split())


def leg_states(leg: dict[str, Any]) -> set[str]:
    return {
        str(m["state"])
        for m in (leg.get("corridor") or {}).get("state_miles", ())
        if m.get("state")
    }


def load_legs() -> list[dict[str, Any]]:
    legs: list[dict[str, Any]] = []
    for path in sorted(LEGS_DIR.glob("*.json")):
        legs.extend(json.loads(path.read_text(encoding="utf-8"))["legs"])
    return legs


def screen(
    legs: list[dict[str, Any]], region_dir: Path, allow_way_cache: bool
) -> tuple[list[dict[str, Any]], dict[str, Any]]:
    """Every named posting, plus the counts the meta line carries."""
    per_leg: list[tuple[dict[str, Any], list[Candidate], Any]] = []
    states_wanted: dict[str, set[tuple[int, int]]] = defaultdict(set)
    for leg in legs:
        cands = candidates_of(leg)
        if not cands:
            continue
        geom = lg.corridor_geometry(leg)
        if not geom:
            continue
        at = point_interpolator(geom)
        pts = [at(c.at_mi + i * STEP_MI) for c in cands for i in range(int(c.run_mi / STEP_MI) + 1)]
        for state in leg_states(leg):
            states_wanted[state].update(_wanted_cells(pts))
        per_leg.append((leg, cands, at))

    index = WayIndex()
    inputs: dict[str, str] = {}
    for state in sorted(states_wanted):
        slug = state_slug(state)
        pbf = region_dir / f"{slug}-latest.osm.pbf"
        cache = region_dir / f"{slug}-latest.legspeed.json"
        if pbf.exists():
            ways, inputs[state] = ways_from_pbf(pbf, states_wanted[state]), "extract"
        elif allow_way_cache and cache.exists():
            ways, inputs[state] = ways_from_cache(cache, states_wanted[state]), "way cache"
        else:
            continue
        for way in ways:
            index.add(way)
        print(f"  {state}: {len(ways):,} ways ({inputs[state]})", flush=True)

    missing = sorted(set(states_wanted) - set(inputs))
    rows: list[dict[str, Any]] = []
    tally: Counter[str] = Counter()
    for leg, cands, at in per_leg:
        if leg_states(leg) & set(missing):
            tally["skipped"] += len(cands)
            continue
        for cand in cands:
            tally["candidates"] += 1
            reason, fractions = classify(cand, at, index)
            if reason is None:
                tally["on_highway" if fractions.get("on_b", 0) >= ON_HIGHWAY_MIN else "mixed"] += 1
                continue
            guarded = place_guarded(leg, cand)
            tally["named"] += 1
            tally["place_guarded" if guarded else "folds"] += 1
            rows.append(
                {
                    "leg": cand.leg,
                    "highway": cand.highway,
                    "at_mi": cand.at_mi,
                    "mph": cand.mph,
                    "from_mph": cand.from_mph,
                    "run_mi": cand.run_mi,
                    "why": reason,
                    "fractions": fractions,
                }
            )
    rows.sort(key=lambda r: (r["leg"], r["at_mi"]))
    counts = {
        "candidates": tally["candidates"],
        "on_highway": tally["on_highway"],
        "mixed": tally["mixed"],
        "named": tally["named"],
        "place_guarded": tally["place_guarded"],
        "folds": tally["folds"],
        "skipped_candidates": tally["skipped"],
        "fold_ratio": round(tally["folds"] / tally["candidates"], 3)
        if tally["candidates"]
        else 0.0,
    }
    meta_inputs = {"states_missing": missing, "inputs": dict(sorted(inputs.items()))}
    return rows, {"counts": counts, **meta_inputs}


def dumps(rows: list[dict[str, Any]], summary: dict[str, Any]) -> str:
    payload = "\n".join(json.dumps(row, sort_keys=True) for row in rows)
    meta = {
        "meta": {
            "schema": 1,
            "layer": "limit_artifacts",
            "data_version": "sha256:" + hashlib.sha256(payload.encode("utf-8")).hexdigest()[:12],
            "kind": "derived",
            "source": SOURCE,
            "reasons": REASONS,
            "params": {
                "max_run_mi": MAX_RUN_MI,
                "step_mi": STEP_MI,
                "near_m": NEAR_M,
                "on_highway_min": ON_HIGHWAY_MIN,
                "verdict_min": VERDICT_MIN,
                "place_guard_mi": PLACE_GUARD_MI,
            },
            **summary,
        }
    }
    lines = [json.dumps(meta, sort_keys=True)]
    lines.extend(json.dumps(row, sort_keys=True) for row in rows)
    return "\n".join(lines) + "\n"


def stale_rows(text: str, legs: list[dict[str, Any]]) -> list[dict[str, Any]]:
    """Named rows that no longer match a played candidate in the data."""
    live = {(c.leg, round(c.at_mi, 2), c.mph) for leg in legs for c in candidates_of(leg)}
    out = []
    for line in text.splitlines():
        if not line.strip() or line.startswith('{"meta"'):
            continue
        row = json.loads(line)
        if (row["leg"], round(float(row["at_mi"]), 2), float(row["mph"])) not in live:
            out.append(row)
    return out


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    parser.add_argument("--region-dir", type=Path, default=OSM_REGION_CACHE_DIR)
    parser.add_argument(
        "--allow-way-cache",
        action="store_true",
        help="Read <slug>-latest.legspeed.json for a state with no extract.",
    )
    parser.add_argument(
        "--check",
        action="store_true",
        help="No OSM read: exit 1 if a named posting no longer matches the data.",
    )
    args = parser.parse_args(argv)

    legs = load_legs()
    if args.check:
        text = OUTPUT_PATH.read_text(encoding="utf-8") if OUTPUT_PATH.exists() else ""
        stale = stale_rows(text, legs)
        for row in stale:
            print(f"  stale: {row['leg']} mile {row['at_mi']} ({row['mph']:g} mph)")
        if stale:
            print(f"{len(stale)} named posting(s) no longer in the data; re-run the screen.")
            return 1
        print(f"{OUTPUT_PATH.name} matches the data.")
        return 0

    rows, summary = screen(legs, args.region_dir, args.allow_way_cache)
    c = summary["counts"]
    print(
        f"{c['candidates']} short drop-and-return postings checked: {c['on_highway']} on "
        f"their highway, {c['mixed']} mixed, {c['named']} named "
        f"({c['place_guarded']} kept by a nearby place, {c['folds']} fold at load)."
    )
    if summary["states_missing"]:
        print(
            f"No OSM input for {', '.join(summary['states_missing'])}: "
            f"{c['skipped_candidates']} postings on legs touching them were not checked."
        )
    OUTPUT_PATH.write_text(dumps(rows, summary), encoding="utf-8")
    print(f"Wrote {OUTPUT_PATH}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
