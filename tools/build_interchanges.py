# ruff: noqa: F821,I001
"""Discover highway interchanges along each leg and curate them into the world source.

Development-time helper (never called at runtime). For every Interstate leg it:

1. Reads the leg's archived polyline (``leg_geometry``; OSRM through the
   checked-in ``route_points`` only for a leg with none) and projects each
   exit onto it for an accurate ``at_mi``.
2. Reads a local OSM PBF extract when ``--pbf`` is passed, streaming only
   ``highway=motorway_junction`` nodes and ``highway=motorway_link`` ways with
   ``destination`` tags. Without ``--pbf`` it falls back to the slower Overpass
   crawl.
3. Collapses the two per-direction junction nodes for an exit into one record,
   snaps it, merges the ramp destinations, and enforces a minimum spacing so
   the spoken cues stay readable.
4. Writes an additive ``corridor.interchanges`` array, leaving every other
   field untouched. Runtime gameplay reads only the checked-in result.

Interchanges are an *additive* layer like curated POIs; they are not a dispatch
requirement, so a leg with no clean OSM exit data simply gets none.

Run from the repo root:
    uv run python tools/build_interchanges.py            # report only
    uv run python tools/build_interchanges.py --write     # update the world source
    uv run python tools/build_interchanges.py --only "New York->Philadelphia" --write
    uv run --group tooling python tools/build_interchanges.py --pbf us.osm.pbf --write
"""

from __future__ import annotations

import argparse
import hashlib
import json
import math
import os
import re
import time
import urllib.error
import urllib.parse
import urllib.request
import importlib
import sys
from collections.abc import Callable
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any

import leg_geometry as lg
from build_interchanges_base import select_only
from world_source import WORLD_SOURCE_PATH, load_world, save_world

ROOT = Path(__file__).resolve().parents[1]
CACHE_DIR = ROOT / ".route-cache" / "interchanges"
PUBLIC_OVERPASS_MIRRORS = (
    "https://overpass-api.de/api/interpreter",
    "https://overpass.private.coffee/api/interpreter",
    "https://overpass.kumi.systems/api/interpreter",
)


def _overpass_mirrors() -> tuple[str, ...]:
    """Overpass endpoints, self-hosted first when OVERPASS_URL is set (same
    convention as enrich_routes_base); public mirrors stay as fallback. Read at
    call time -- not frozen at import -- so the env is always honored. A local
    instance turns the junction sweep from hours (public throttling) into minutes.
    """
    return tuple(url for url in (os.environ.get("OVERPASS_URL"), *PUBLIC_OVERPASS_MIRRORS) if url)


OSRM_ROUTE_URL = "https://router.project-osrm.org/route/v1/driving/{coords}"
USER_AGENT = "FreightFate interchange curation (https://github.com/orinks-games/Freight-Fate)"
ACCESSED_DATE = "2026-06-23"
EARTH_RADIUS_MI = 3958.7613

SAMPLE_SPACING_MI = 10.0  # how often to drop an Overpass probe along the leg
PROBE_RADIUS_M = 9_000  # search radius per probe
RAMP_NEAR_M = 350.0  # a ramp this close to a junction belongs to it
LOCAL_CORRIDOR_M = 200.0  # local PBF features must snap this close to a leg
LOCAL_PBF_PREFILTER_PAD_M = PROBE_RADIUS_M
# Polyline piece per prefilter box: about the route-point spacing the boxes
# used to follow, so a national run tests each OSM node against as many.
LOCAL_PBF_PREFILTER_CHUNK_MI = 25.0
MIN_EXIT_SPACING_MI = 2.0  # collapse exits closer than this (keep the richer)
MAX_DESTINATIONS = 3  # cap control cities per exit for speech brevity
LOCAL_INDEX_CACHE_VERSION = 1
LOCAL_INDEX_PROGRESS_INTERVAL_SEC = 60.0


@dataclass(frozen=True, slots=True)
class LocalOsmFeature:
    lat: float
    lon: float
    tags: dict[str, str]


@dataclass(slots=True)
class LocalOsmIndex:
    junctions: list[LocalOsmFeature]
    ramps: list[LocalOsmFeature]


@dataclass(frozen=True, slots=True)
class _LocalRampWay:
    node_ids: tuple[int, ...]
    tags: dict[str, str]


LocalBounds = tuple[float, float, float, float]


@dataclass(slots=True)
class _LocalIndexProgress:
    label: str
    interval_sec: float = LOCAL_INDEX_PROGRESS_INTERVAL_SEC
    started: float = field(init=False)
    last: float = field(init=False)

    def __post_init__(self) -> None:
        self.started = time.monotonic()
        self.last = self.started

    def maybe(self, message: str) -> None:
        now = time.monotonic()
        if self.interval_sec <= 0 or now - self.last < self.interval_sec:
            return
        self.last = now
        elapsed = now - self.started
        print(f"    {self.label}: {message} after {elapsed / 60.0:.1f} min", flush=True)


# --- geometry ---------------------------------------------------------------


def _haversine_mi(lat1: float, lon1: float, lat2: float, lon2: float) -> float:
    p1, p2 = math.radians(lat1), math.radians(lat2)
    dphi = math.radians(lat2 - lat1)
    dlmb = math.radians(lon2 - lon1)
    h = math.sin(dphi / 2) ** 2 + math.cos(p1) * math.cos(p2) * math.sin(dlmb / 2) ** 2
    return 2 * EARTH_RADIUS_MI * math.asin(math.sqrt(h))


def _osrm_geometry(
    route_points: list[dict[str, Any]], rate_limit: float, cached_only: bool = False
) -> list[tuple[float, float, float]] | None:
    """Dense [(lat, lon, cumulative_mi), ...] following the leg's waypoints.

    With ``cached_only`` it never makes a live request, returning ``None`` on a
    cache miss so a batch can fall back to local interpolation instead of risking
    a hang on the public OSRM router."""
    if len(route_points) < 2:
        return None
    coords = ";".join(f"{p['lon']},{p['lat']}" for p in route_points)
    params = urllib.parse.urlencode(
        {
            "overview": "full",
            "geometries": "geojson",
            "alternatives": "false",
            "steps": "false",
        }
    )
    url = OSRM_ROUTE_URL.format(coords=coords) + "?" + params
    payload = _cached_get(url, "osrm", rate_limit, cached_only=cached_only)
    if payload is None:
        return None
    try:
        geom = payload["routes"][0]["geometry"]["coordinates"]
    except (KeyError, IndexError):
        return None
    out: list[tuple[float, float, float]] = []
    cum = 0.0
    prev: tuple[float, float] | None = None
    for lon, lat in geom:
        if prev is not None:
            cum += _haversine_mi(prev[0], prev[1], lat, lon)
        out.append((lat, lon, cum))
        prev = (lat, lon)
    return out


_ENRICH_MODULE = None


def _ors_geometry(
    data: dict[str, Any], leg: dict[str, Any], rate_limit: float, api_key: str
) -> list[tuple[float, float, float]] | None:
    """Dense ``[(lat, lon, cumulative_mi), ...]`` for a leg from the self-hosted
    OpenRouteService instead of public OSRM -- so the whole interchange sweep can
    run locally. Reuses ``enrich_routes``' cached ORS fetch/parse (importing the
    aggregate module triggers the cross-module namespace wiring that resolves its
    private helpers); the ORS route is already cached for enriched legs, so this
    is usually a free cache read, and a miss hits the local ORS (unlimited)."""
    global _ENRICH_MODULE
    if _ENRICH_MODULE is None:
        tools_dir = str(Path(__file__).resolve().parent)
        if tools_dir not in sys.path:
            sys.path.insert(0, tools_dir)
        _ENRICH_MODULE = importlib.import_module("enrich_routes")
    parsed = _ENRICH_MODULE._cached_ors_route(data, leg, ROOT / ".route-cache", rate_limit, api_key)
    coords = parsed.get("coordinates") or []  # dense [[lon, lat], ...]
    if len(coords) < 2:
        return None
    out: list[tuple[float, float, float]] = []
    cum = 0.0
    prev: tuple[float, float] | None = None
    for point in coords:
        lat, lon = float(point[1]), float(point[0])
        if prev is not None:
            cum += _haversine_mi(prev[0], prev[1], lat, lon)
        out.append((lat, lon, cum))
        prev = (lat, lon)
    return out


SNAP_CELL_DEG = 0.1  # grid cell of the snapper's segment index
INTERCHANGE_SOURCE = (
    "Exit ref, name and destination signs read from OpenStreetMap "
    "highway=motorway_junction nodes and motorway_link destination tags, "
    f"accessed {ACCESSED_DATE}. at_mi derived: the mean of the exit's junction "
    "nodes projected onto the leg's route polyline (its archived geometry; "
    "OSRM only where none is archived), rescaled to the leg's miles: "
    "https://www.openstreetmap.org/"
)


def _polyline_snapper(
    geom: list[tuple[float, float, float]], leg_miles: float
) -> Callable[[float, float], tuple[float, float]]:
    """(lat, lon) -> (at_mi in the leg's frame, dist_mi) at the nearest point
    ON the polyline.

    Projects onto segments, not vertices: the archive keeps every vertex a
    curve needs and thins the straights (the longest gap on the median one of
    the 81 legs re-derived on 2026-09-25 was 6 miles), so a junction halfway
    down a tangent sits miles from any vertex. Snapping to vertices lost 60
    percent of those legs' junctions at 200 m and put the rest up to half a
    gap off. Segments are indexed by grid cell, padded one cell, so any
    segment within about 7 km of the query is tried; farther answers inf.
    """
    total = geom[-1][2] or leg_miles
    scale = leg_miles / total if total else 1.0
    cells: dict[tuple[int, int], list[int]] = {}
    for i in range(len(geom) - 1):
        (a_lat, a_lon, _), (b_lat, b_lon, _) = geom[i], geom[i + 1]
        y0, y1 = sorted((math.floor(a_lat / SNAP_CELL_DEG), math.floor(b_lat / SNAP_CELL_DEG)))
        x0, x1 = sorted((math.floor(a_lon / SNAP_CELL_DEG), math.floor(b_lon / SNAP_CELL_DEG)))
        for y in range(y0 - 1, y1 + 2):
            for x in range(x0 - 1, x1 + 2):
                cells.setdefault((y, x), []).append(i)

    def snap(lat: float, lon: float) -> tuple[float, float]:
        best_d, best_cum = float("inf"), 0.0
        key = (math.floor(lat / SNAP_CELL_DEG), math.floor(lon / SNAP_CELL_DEG))
        for i in cells.get(key, ()):
            (a_lat, a_lon, a_mi), (b_lat, b_lon, b_mi) = geom[i], geom[i + 1]
            # Local equirectangular plane around the query; exact enough for
            # a segment tens of miles long and a match radius under two.
            kx = math.cos(math.radians(lat))
            ax, ay = (a_lon - lon) * kx, a_lat - lat
            dx, dy = (b_lon - a_lon) * kx, b_lat - a_lat
            seg2 = dx * dx + dy * dy
            t = 0.0 if seg2 == 0 else min(1.0, max(0.0, -(ax * dx + ay * dy) / seg2))
            p_lat, p_lon = a_lat + (b_lat - a_lat) * t, a_lon + (b_lon - a_lon) * t
            d = _haversine_mi(lat, lon, p_lat, p_lon)
            if d < best_d:
                best_d, best_cum = d, a_mi + (b_mi - a_mi) * t
        return best_cum * scale, best_d

    return snap


def _geometry_bounds(
    geom: list[tuple[float, float, float]], pad_m: float
) -> tuple[float, float, float, float]:
    min_lat = min(p[0] for p in geom)
    max_lat = max(p[0] for p in geom)
    min_lon = min(p[1] for p in geom)
    max_lon = max(p[1] for p in geom)
    mid_lat = (min_lat + max_lat) / 2.0
    lat_pad = pad_m / 111_320.0
    lon_scale = max(0.2, math.cos(math.radians(mid_lat)))
    lon_pad = pad_m / (111_320.0 * lon_scale)
    return min_lat - lat_pad, max_lat + lat_pad, min_lon - lon_pad, max_lon + lon_pad


def _inside_bounds(lat: float, lon: float, bounds: LocalBounds) -> bool:
    min_lat, max_lat, min_lon, max_lon = bounds
    return min_lat <= lat <= max_lat and min_lon <= lon <= max_lon


def _inside_any_bounds(lat: float, lon: float, bounds: list[LocalBounds]) -> bool:
    return any(_inside_bounds(lat, lon, item) for item in bounds)


def _segment_bounds(a: dict[str, Any], b: dict[str, Any], pad_m: float) -> LocalBounds:
    min_lat = min(float(a["lat"]), float(b["lat"]))
    max_lat = max(float(a["lat"]), float(b["lat"]))
    min_lon = min(float(a["lon"]), float(b["lon"]))
    max_lon = max(float(a["lon"]), float(b["lon"]))
    mid_lat = (min_lat + max_lat) / 2.0
    lat_pad = pad_m / 111_320.0
    lon_scale = max(0.2, math.cos(math.radians(mid_lat)))
    lon_pad = pad_m / (111_320.0 * lon_scale)
    return min_lat - lat_pad, max_lat + lat_pad, min_lon - lon_pad, max_lon + lon_pad


def _route_corridor_bounds(
    route_points: list[dict[str, Any]], pad_m: float = LOCAL_PBF_PREFILTER_PAD_M
) -> list[LocalBounds]:
    if len(route_points) < 2:
        return []
    return [
        _segment_bounds(start, end, pad_m)
        for start, end in zip(route_points, route_points[1:], strict=False)
    ]


def _polyline_bounds(
    geom: list[tuple[float, float, float]],
    pad_m: float = LOCAL_PBF_PREFILTER_PAD_M,
    chunk_mi: float = LOCAL_PBF_PREFILTER_CHUNK_MI,
) -> list[LocalBounds]:
    """Padded boxes over consecutive ~chunk_mi pieces of a polyline. Each
    piece shares its end vertex with the next, so no stretch falls between."""
    out: list[LocalBounds] = []
    start = 0
    for i in range(1, len(geom)):
        if geom[i][2] - geom[start][2] >= chunk_mi or i == len(geom) - 1:
            out.append(_geometry_bounds(geom[start : i + 1], pad_m))
            start = i
    return out


def _local_prefilter_bounds(legs: list[dict[str, Any]]) -> list[LocalBounds]:
    """PBF prefilter boxes along the road each leg drives: its archived
    polyline, route_points only for a leg with none. Route points can leave
    the polyline entirely -- on 7 of the 81 legs re-derived on 2026-09-25
    they boxed as little as 23 percent of it -- and a junction outside every
    box is never read."""
    bounds: list[LocalBounds] = []
    for leg in legs:
        geom = lg.corridor_geometry(leg)
        if geom:
            bounds.extend(_polyline_bounds(geom))
            continue
        route_points = list(leg.get("corridor", {}).get("route_points", ()))
        bounds.extend(_route_corridor_bounds(route_points))
    return bounds


def _bounds_digest(bounds: list[LocalBounds]) -> str:
    payload = json.dumps(
        [[round(value, 7) for value in item] for item in bounds],
        separators=(",", ":"),
    )
    return hashlib.sha256(payload.encode("utf-8")).hexdigest()


def _pbf_metadata(pbf_path: Path) -> dict[str, Any]:
    resolved = pbf_path.resolve()
    stat = resolved.stat()
    return {
        "path": str(resolved),
        "size": stat.st_size,
        "mtime_ns": stat.st_mtime_ns,
    }


def _pbf_set_metadata(pbf_paths: list[Path]) -> list[dict[str, Any]]:
    return [_pbf_metadata(path) for path in pbf_paths]


def _default_local_index_cache(pbf_path: Path) -> Path:
    name = pbf_path.name
    for suffix in (".osm.pbf", ".pbf"):
        if name.endswith(suffix):
            name = name[: -len(suffix)]
            break
    return pbf_path.with_name(f"{name}.interchanges.json")


def _feature_to_json(feature: LocalOsmFeature) -> dict[str, Any]:
    return {"lat": feature.lat, "lon": feature.lon, "tags": feature.tags}


def _feature_from_json(raw: dict[str, Any]) -> LocalOsmFeature:
    return LocalOsmFeature(
        lat=float(raw["lat"]),
        lon=float(raw["lon"]),
        tags={str(k): str(v) for k, v in raw.get("tags", {}).items()},
    )


def _write_local_index_cache(
    cache_path: Path,
    index: LocalOsmIndex,
    pbf_paths: list[Path],
    bounds: list[LocalBounds],
) -> None:
    cache_path.parent.mkdir(parents=True, exist_ok=True)
    payload = {
        "version": LOCAL_INDEX_CACHE_VERSION,
        "pbfs": _pbf_set_metadata(pbf_paths),
        "bounds_digest": _bounds_digest(bounds),
        "junctions": [_feature_to_json(feature) for feature in index.junctions],
        "ramps": [_feature_to_json(feature) for feature in index.ramps],
    }
    cache_path.write_text(json.dumps(payload, indent=2) + "\n", encoding="utf-8")


def _read_local_index_cache(
    cache_path: Path,
    pbf_paths: list[Path] | None,
    bounds: list[LocalBounds],
) -> LocalOsmIndex | None:
    if not cache_path.exists():
        return None
    try:
        payload = json.loads(cache_path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as exc:
        print(f"    ignoring unreadable local index cache {cache_path}: {exc}", flush=True)
        return None
    if payload.get("version") != LOCAL_INDEX_CACHE_VERSION:
        return None
    if pbf_paths is not None and payload.get("pbfs") != _pbf_set_metadata(pbf_paths):
        return None
    if payload.get("bounds_digest") != _bounds_digest(bounds):
        return None
    return LocalOsmIndex(
        junctions=[_feature_from_json(item) for item in payload.get("junctions", [])],
        ramps=[_feature_from_json(item) for item in payload.get("ramps", [])],
    )


def load_or_build_local_index(
    pbf_paths: list[Path],
    bounds: list[LocalBounds],
    cache_path: Path,
    rebuild: bool = False,
) -> LocalOsmIndex:
    if not rebuild:
        cached = _read_local_index_cache(cache_path, pbf_paths, bounds)
        if cached is not None:
            print(
                f"Loaded local OSM interchange index cache: {cache_path} "
                f"({len(cached.junctions)} junctions, {len(cached.ramps)} ramps)",
                flush=True,
            )
            return cached
    index = build_local_index(pbf_paths, bounds)
    _write_local_index_cache(cache_path, index, pbf_paths, bounds)
    print(f"    wrote local OSM interchange index cache: {cache_path}", flush=True)
    return index


def load_local_index_cache_only(cache_path: Path, bounds: list[LocalBounds]) -> LocalOsmIndex:
    cached = _read_local_index_cache(cache_path, None, bounds)
    if cached is None:
        raise SystemExit(f"Local index cache is missing, stale, or incompatible: {cache_path}")
    print(
        f"Loaded local OSM interchange index cache: {cache_path} "
        f"({len(cached.junctions)} junctions, {len(cached.ramps)} ramps)",
        flush=True,
    )
    return cached


# --- Overpass ---------------------------------------------------------------


def _shield_pattern(highway: str) -> str | None:
    """'I-95' -> regex 'I 95([^0-9]|$)'. Interstate shields only (clean OSM
    motorway_junction tagging); returns None for non-Interstate legs."""
    primary = highway.replace(",", "/").split("/")[0].strip()
    m = re.match(r"^I-(\d+)$", primary)
    if not m:
        return None
    return f"I {m.group(1)}([^0-9]|$)"


def _eligible_for_interchange_discovery(
    leg: dict[str, Any], local_mode: bool, explicitly_selected: bool
) -> bool:
    if _shield_pattern(str(leg.get("highway", ""))) is not None:
        return True
    corridor = leg.get("corridor", {})
    return local_mode and (explicitly_selected or bool(corridor.get("interchanges")))


def _sample_points(geom: list[tuple[float, float, float]]) -> list[tuple[float, float]]:
    points: list[tuple[float, float]] = []
    next_at = 0.0
    total = geom[-1][2]
    for lat, lon, cum in geom:
        if cum >= next_at:
            points.append((lat, lon))
            next_at += SAMPLE_SPACING_MI
    if total - (next_at - SAMPLE_SPACING_MI) > 1.0:  # always probe the tail
        points.append((geom[-1][0], geom[-1][1]))
    return points


def _overpass_query(shield_rx: str, lat: float, lon: float) -> str:
    r = PROBE_RADIUS_M
    return (
        f"[out:json][timeout:60];"
        f'way["highway"="motorway"]["ref"~"{shield_rx}"]'
        f"(around:{r},{lat},{lon})->.m;"
        f'node(w.m)["highway"="motorway_junction"]'
        f"(around:{r},{lat},{lon})->.jx;"
        f".jx out body;"
        f'way(around.jx:{int(RAMP_NEAR_M)})["highway"="motorway_link"]'
        f'["destination"]->.r;'
        f".r out tags center;"
    )


def _post_overpass(query: str, rate_limit: float) -> dict[str, Any] | None:
    return _cached_post(query, rate_limit)


# --- local OSM extracts -----------------------------------------------------


def _dedupe_features(features: list[LocalOsmFeature]) -> list[LocalOsmFeature]:
    seen: set[tuple[float, float, tuple[tuple[str, str], ...]]] = set()
    out: list[LocalOsmFeature] = []
    for feature in features:
        key = (
            round(feature.lat, 6),
            round(feature.lon, 6),
            tuple(sorted(feature.tags.items())),
        )
        if key in seen:
            continue
        seen.add(key)
        out.append(feature)
    return out


def build_local_index(
    pbf_paths: Path | list[Path],
    bounds: list[LocalBounds],
    progress_interval_sec: float = LOCAL_INDEX_PROGRESS_INTERVAL_SEC,
) -> LocalOsmIndex:
    paths = [pbf_paths] if isinstance(pbf_paths, Path) else list(pbf_paths)
    junctions: list[LocalOsmFeature] = []
    ramps: list[LocalOsmFeature] = []
    for i, pbf_path in enumerate(paths, 1):
        part = _build_local_index_from_pbf(
            pbf_path,
            bounds,
            progress_interval_sec,
            label=f"{i}/{len(paths)}",
        )
        junctions.extend(part.junctions)
        ramps.extend(part.ramps)
    return LocalOsmIndex(
        junctions=_dedupe_features(junctions),
        ramps=_dedupe_features(ramps),
    )


def _build_local_index_from_pbf(
    pbf_path: Path,
    bounds: list[LocalBounds],
    progress_interval_sec: float = LOCAL_INDEX_PROGRESS_INTERVAL_SEC,
    label: str = "1/1",
) -> LocalOsmIndex:
    try:
        import osmium  # type: ignore[import-not-found]
    except ImportError as exc:
        raise SystemExit(
            "Reading --pbf requires the tooling dependency group: "
            "uv sync --group dev --group tooling"
        ) from exc

    tag_filters = [
        osmium.filter.TagFilter(  # type: ignore[attr-defined]
            ("highway", "motorway_junction"),
            ("highway", "motorway_link"),
        )
    ]
    pass1 = _LocalIndexProgress(f"PBF {label} pass 1", progress_interval_sec)

    class InterchangeHandler(osmium.SimpleHandler):  # type: ignore[name-defined]
        def __init__(self) -> None:
            super().__init__()
            self.junctions: list[LocalOsmFeature] = []
            self.ramp_ways: list[_LocalRampWay] = []
            self.ramp_node_ids: set[int] = set()
            self.nodes_seen = 0
            self.ways_seen = 0

        def node(self, node: Any) -> None:
            self.nodes_seen += 1
            pass1.maybe(
                f"{self.nodes_seen:,} nodes, {self.ways_seen:,} ways; "
                f"retained {len(self.junctions):,} junctions and "
                f"{len(self.ramp_ways):,} ramp ways"
            )
            tags = {str(k): str(v) for k, v in node.tags}
            if tags.get("highway") != "motorway_junction":
                return
            if not node.location.valid():
                return
            lat = float(node.location.lat)
            lon = float(node.location.lon)
            if not _inside_any_bounds(lat, lon, bounds):
                return
            self.junctions.append(
                LocalOsmFeature(
                    lat=lat,
                    lon=lon,
                    tags=tags,
                )
            )

        def way(self, way: Any) -> None:
            self.ways_seen += 1
            pass1.maybe(
                f"{self.nodes_seen:,} nodes, {self.ways_seen:,} ways; "
                f"retained {len(self.junctions):,} junctions and "
                f"{len(self.ramp_ways):,} ramp ways"
            )
            tags = {str(k): str(v) for k, v in way.tags}
            if tags.get("highway") != "motorway_link" or not tags.get("destination"):
                return
            node_ids: list[int] = []
            for node_ref in way.nodes:
                node_id = getattr(node_ref, "ref", None)
                if node_id is not None:
                    node_ids.append(int(node_id))
            if not node_ids:
                return
            self.ramp_ways.append(
                _LocalRampWay(
                    node_ids=tuple(node_ids),
                    tags={
                        "highway": "motorway_link",
                        "destination": str(tags.get("destination", "")),
                        "destination:ref": str(tags.get("destination:ref", "")),
                    },
                )
            )
            self.ramp_node_ids.update(node_ids)

    pass2 = _LocalIndexProgress(f"PBF {label} pass 2", progress_interval_sec)

    class RampNodeHandler(osmium.SimpleHandler):  # type: ignore[name-defined]
        def __init__(self, wanted: set[int]) -> None:
            super().__init__()
            self.wanted = wanted
            self.coords: dict[int, tuple[float, float]] = {}
            self.nodes_seen = 0

        def node(self, node: Any) -> None:
            self.nodes_seen += 1
            pass2.maybe(
                f"{self.nodes_seen:,} nodes; resolved "
                f"{len(self.coords):,}/{len(self.wanted):,} ramp node locations"
            )
            node_id = int(node.id)
            if node_id not in self.wanted:
                return
            if not node.location.valid():
                return
            self.coords[node_id] = (
                float(node.location.lat),
                float(node.location.lon),
            )

    handler = InterchangeHandler()
    try:
        print(
            f"    building local index from PBF {label} pass 1: {pbf_path}",
            flush=True,
        )
        handler.apply_file(str(pbf_path), filters=tag_filters)
    except RuntimeError as exc:
        raise SystemExit(f"Could not read OSM PBF {pbf_path}: {exc}") from exc

    ramps: list[LocalOsmFeature] = []
    if handler.ramp_node_ids:
        ramp_nodes = RampNodeHandler(handler.ramp_node_ids)
        id_filters = [osmium.filter.IdFilter(handler.ramp_node_ids)]  # type: ignore[attr-defined]
        try:
            print(
                f"    resolving {len(handler.ramp_node_ids):,} ramp node locations "
                f"from PBF {label} pass 2",
                flush=True,
            )
            ramp_nodes.apply_file(str(pbf_path), filters=id_filters)
        except RuntimeError as exc:
            raise SystemExit(f"Could not read OSM PBF {pbf_path}: {exc}") from exc
        for ramp in handler.ramp_ways:
            coords = [
                ramp_nodes.coords[node_id]
                for node_id in ramp.node_ids
                if node_id in ramp_nodes.coords
            ]
            if not coords:
                continue
            lat = sum(p[0] for p in coords) / len(coords)
            lon = sum(p[1] for p in coords) / len(coords)
            if _inside_any_bounds(lat, lon, bounds):
                ramps.append(LocalOsmFeature(lat=lat, lon=lon, tags=ramp.tags))

    print(
        f"    retained {len(handler.junctions):,} route-bbox junctions and "
        f"{len(ramps):,} route-bbox destination ramps from local extract",
        flush=True,
    )
    return LocalOsmIndex(junctions=handler.junctions, ramps=ramps)


def _local_candidates(
    index: LocalOsmIndex, geom: list[tuple[float, float, float]], leg_miles: float
) -> tuple[dict[int, dict[str, Any]], list[dict[str, Any]]]:
    bounds = _geometry_bounds(geom, max(PROBE_RADIUS_M, 2_000.0))
    snap = _polyline_snapper(geom, leg_miles)
    junctions: dict[int, dict[str, Any]] = {}
    ramps: list[dict[str, Any]] = []
    for i, feature in enumerate(index.junctions):
        if not _inside_bounds(feature.lat, feature.lon, bounds):
            continue
        _, dist = snap(feature.lat, feature.lon)
        if dist * 1609.34 > LOCAL_CORRIDOR_M:
            continue
        junctions[i] = {
            "lat": feature.lat,
            "lon": feature.lon,
            "ref": str(feature.tags.get("ref", "")).strip(),
            "name": _clean_place(feature.tags.get("name", "")),
        }
    for feature in index.ramps:
        if not _inside_bounds(feature.lat, feature.lon, bounds):
            continue
        _, dist = snap(feature.lat, feature.lon)
        if dist * 1609.34 > max(RAMP_NEAR_M, LOCAL_CORRIDOR_M):
            continue
        ramps.append(
            {
                "lat": feature.lat,
                "lon": feature.lon,
                "destinations": _split_destinations(feature.tags.get("destination", "")),
                "via": str(feature.tags.get("destination:ref", "")).strip(),
            }
        )
    return junctions, ramps


# --- caching ----------------------------------------------------------------


def _cache_file(tag: str, key: str) -> Path:
    digest = hashlib.md5(key.encode("utf-8")).hexdigest()[:16]
    return CACHE_DIR / f"{tag}-{digest}.json"


def _cached_get(
    url: str, tag: str, rate_limit: float, cached_only: bool = False
) -> dict[str, Any] | None:
    cf = _cache_file(tag, url)
    if cf.exists():
        return json.loads(cf.read_text(encoding="utf-8"))
    if cached_only:
        return None
    req = urllib.request.Request(url, headers={"User-Agent": USER_AGENT})
    try:
        with urllib.request.urlopen(req, timeout=30) as resp:
            payload = json.loads(resp.read().decode("utf-8"))
    except (TimeoutError, OSError, urllib.error.URLError) as exc:
        print(f"    OSRM error: {type(exc).__name__}: {exc}")
        return None
    CACHE_DIR.mkdir(parents=True, exist_ok=True)
    cf.write_text(json.dumps(payload), encoding="utf-8")
    if rate_limit > 0:
        time.sleep(rate_limit)
    return payload


def _cached_post(query: str, rate_limit: float) -> dict[str, Any] | None:
    cf = _cache_file("overpass", query)
    if cf.exists():
        return json.loads(cf.read_text(encoding="utf-8"))
    body = urllib.parse.urlencode({"data": query}).encode("utf-8")
    # Public Overpass throttles bulk use (429) and sheds load (504). Sweep the
    # mirrors a few times with exponential backoff so a long crawl rides out a
    # rate-limit window instead of dropping the leg. Cache makes a re-run free.
    for attempt in range(4):
        for url in _overpass_mirrors():
            req = urllib.request.Request(
                url,
                data=body,
                headers={
                    "User-Agent": USER_AGENT,
                    "Content-Type": "application/x-www-form-urlencoded",
                },
            )
            try:
                with urllib.request.urlopen(req, timeout=120) as resp:
                    payload = json.loads(resp.read().decode("utf-8"))
            except (TimeoutError, OSError, urllib.error.URLError, ValueError) as exc:
                # ValueError catches an empty / non-JSON 200 body -- the local
                # single-dispatcher Overpass sheds those under load, and an
                # uncaught JSONDecodeError would kill the whole crawl.
                code = getattr(exc, "code", "")
                print(
                    f"    Overpass {url.split('/')[2]} -> {type(exc).__name__} "
                    f"{code}; trying next mirror"
                )
                continue
            CACHE_DIR.mkdir(parents=True, exist_ok=True)
            cf.write_text(json.dumps(payload), encoding="utf-8")
            if rate_limit > 0:
                time.sleep(rate_limit)
            return payload
        backoff = 15.0 * (attempt + 1)
        print(f"    all mirrors busy; backing off {backoff:.0f}s (attempt {attempt + 1}/4)")
        time.sleep(backoff)
    return None


# --- discovery --------------------------------------------------------------

RAW_MARKERS = ("node/", "way/", "relation/", "amenity=", "highway=")
# Lane-control / vehicle-class words that ride in OSM destination tags. A
# destination made up *only* of these (e.g. "Cars Only", "Trucks - Buses",
# "Buses And Cars Only") is lane signage, not a place to head "toward".
VEHICLE_CONTROL_WORDS = {
    "cars",
    "car",
    "trucks",
    "truck",
    "buses",
    "bus",
    "vehicles",
    "only",
    "no",
    "hov",
    "express",
    "local",
    "left",
    "right",
    "lane",
    "lanes",
    "exit",
    "toll",
    "ezpass",
}
_CONNECTOR_WORDS = {"and"}


def _clean_place(value: str) -> str:
    text = " ".join(str(value).replace("\n", " ").split()).strip()
    lowered = text.lower()
    if not text or any(marker in lowered for marker in RAW_MARKERS):
        return ""
    words = [w for w in re.findall(r"[a-z]+", lowered) if w not in _CONNECTOR_WORDS]
    if words and all(w in VEHICLE_CONTROL_WORDS for w in words):
        return ""
    return text


def _split_destinations(raw: str) -> list[str]:
    out: list[str] = []
    for piece in str(raw).split(";"):
        place = _clean_place(piece)
        if place and place not in out:
            out.append(place)
    return out


def discover_leg(
    leg: dict[str, Any],
    rate_limit: float,
    local_index: LocalOsmIndex | None = None,
    geom: list[tuple[float, float, float]] | None = None,
) -> list[dict[str, Any]]:
    highway = str(leg.get("highway", ""))
    shield_rx = _shield_pattern(highway)
    if shield_rx is None and local_index is None:
        return []
    if geom is None:
        # The archived polyline is the road this leg drives; ask OSRM only for
        # a leg that has none. A live OSRM answer is a CAR route over today's
        # OSM, which on a rerouted leg agrees with neither the archive nor the
        # truck.
        geom = lg.corridor_geometry(leg)
    if geom is None:
        route_points = list(leg.get("corridor", {}).get("route_points", ()))
        geom = _osrm_geometry(route_points, rate_limit)
    if not geom:
        return []
    leg_miles = float(leg["miles"])

    if local_index is not None:
        junctions, ramps = _local_candidates(local_index, geom, leg_miles)
        return _assemble(junctions, ramps, geom, leg_miles, highway)

    junctions: dict[int, dict[str, Any]] = {}  # node id -> {lat, lon, ref, name}
    ramps: list[dict[str, Any]] = []  # {lat, lon, destination, via}
    for lat, lon in _sample_points(geom):
        payload = _post_overpass(_overpass_query(shield_rx, lat, lon), rate_limit)
        if payload is None:
            continue
        for el in payload.get("elements", []):
            tags = el.get("tags", {})
            if el.get("type") == "node" and tags.get("highway") == "motorway_junction":
                junctions[el["id"]] = {
                    "lat": el["lat"],
                    "lon": el["lon"],
                    "ref": str(tags.get("ref", "")).strip(),
                    "name": _clean_place(tags.get("name", "")),
                }
            elif el.get("type") == "way" and tags.get("highway") == "motorway_link":
                center = el.get("center") or {}
                if "lat" not in center:
                    continue
                ramps.append(
                    {
                        "lat": center["lat"],
                        "lon": center["lon"],
                        "destinations": _split_destinations(tags.get("destination", "")),
                        "via": str(tags.get("destination:ref", "")).strip(),
                    }
                )

    return _assemble(junctions, ramps, geom, leg_miles, highway)


def _assemble(
    junctions: dict[int, dict[str, Any]],
    ramps: list[dict[str, Any]],
    geom: list[tuple[float, float, float]],
    leg_miles: float,
    highway: str,
) -> list[dict[str, Any]]:
    # Collapse the two per-carriageway junction nodes that share an exit ref
    # into one logical exit; ref-less nodes are grouped by their snapped mile.
    snap = _polyline_snapper(geom, leg_miles)
    by_key: dict[str, list[dict[str, Any]]] = {}
    for node in junctions.values():
        at_mi, dist = snap(node["lat"], node["lon"])
        if dist > 1.5 or not (1.0 < at_mi < leg_miles - 1.0):
            continue  # off-corridor match or too close to an endpoint
        node["at_mi"] = at_mi
        key = f"ref:{node['ref']}" if node["ref"] else f"mi:{round(at_mi / 0.5)}"
        by_key.setdefault(key, []).append(node)

    # A leg across a state line can pass two exits with one number, since
    # each state counts its own. Nodes of one ref farther apart than exits
    # may stand are two exits; averaged, they were one exit at a mile
    # between them where there is none.
    groups: list[list[dict[str, Any]]] = []
    for same_key in by_key.values():
        same_key.sort(key=lambda n: n["at_mi"])
        groups.append([same_key[0]])
        for node in same_key[1:]:
            if node["at_mi"] - groups[-1][-1]["at_mi"] > MIN_EXIT_SPACING_MI:
                groups.append([])
            groups[-1].append(node)

    exits: list[dict[str, Any]] = []
    for group in groups:
        at_mi = sum(n["at_mi"] for n in group) / len(group)
        # Collapse stray internal spaces in OSM exit refs ("103 B" -> "103B").
        ref = re.sub(r"\s+", "", next((n["ref"] for n in group if n["ref"]), ""))
        name = next((n["name"] for n in group if n["name"]), "")
        # Gather destinations from ramps near any node in this group.
        dests: list[str] = []
        via = ""
        for node in group:
            for ramp in ramps:
                if (
                    _haversine_mi(node["lat"], node["lon"], ramp["lat"], ramp["lon"]) * 1609.34
                    > RAMP_NEAR_M
                ):
                    continue
                for d in ramp["destinations"]:
                    if d not in dests:
                        dests.append(d)
                if not via and ramp["via"]:
                    via = ramp["via"]
        if not (ref or dests or name):
            continue
        exits.append(
            {
                "at_mi": round(at_mi, 1),
                "exit_ref": ref,
                "name": name,
                "destinations": dests[:MAX_DESTINATIONS],
                "via": via,
                "highway": highway,
                "source": INTERCHANGE_SOURCE,
            }
        )

    return _space_out(exits)


def _richness(ex: dict[str, Any]) -> int:
    return (1 if ex["exit_ref"] else 0) + len(ex["destinations"])


def _space_out(exits: list[dict[str, Any]]) -> list[dict[str, Any]]:
    """Greedily keep the richest exits, dropping any within MIN_EXIT_SPACING_MI
    of one already kept, so the spoken cues do not crowd each other."""
    kept: list[dict[str, Any]] = []
    for ex in sorted(exits, key=lambda e: (-_richness(e), e["at_mi"])):
        if all(abs(ex["at_mi"] - k["at_mi"]) >= MIN_EXIT_SPACING_MI for k in kept):
            kept.append(ex)
    kept.sort(key=lambda e: e["at_mi"])
    return kept


# --- maxspeed ---------------------------------------------------------------
#
# Bakes real OSM `maxspeed` onto each leg as a `corridor.speed_limits` step
# profile (mph), which the runtime prefers over the highway/region heuristic.
# Reuses this module's local-PBF reader and corridor snapping; the per-state
# extracts under ~/.cache/freight-fate-osm/regions are selected automatically
# from the states each leg touches, so the slow 12GB national file is not needed.


TOOLS_DIR = Path(__file__).resolve().parent
if str(TOOLS_DIR) not in sys.path:
    sys.path.insert(0, str(TOOLS_DIR))

_MAXSPEED_MODULE = importlib.import_module("build_interchanges_maxspeed")
for _name, _value in _MAXSPEED_MODULE.__dict__.items():
    if not _name.startswith("__"):
        globals()[_name] = _value
_MAXSPEED_MODULE.__dict__.update(
    {name: value for name, value in globals().items() if not name.startswith("__")}
)

_RAMPCONTROL_MODULE = importlib.import_module("build_interchanges_rampcontrols")
for _name, _value in _RAMPCONTROL_MODULE.__dict__.items():
    if not _name.startswith("__") and _name not in globals():
        globals()[_name] = _value
_RAMPCONTROL_MODULE.__dict__.update(
    {
        name: value
        for name, value in globals().items()
        if not name.startswith("__") and name not in _RAMPCONTROL_MODULE.__dict__
    }
)
run_ramp_controls = _RAMPCONTROL_MODULE.run_ramp_controls

_RESTRICTIONS_MODULE = importlib.import_module("build_interchanges_restrictions")
for _name, _value in _RESTRICTIONS_MODULE.__dict__.items():
    if not _name.startswith("__") and _name not in globals():
        globals()[_name] = _value
_RESTRICTIONS_MODULE.__dict__.update(
    {
        name: value
        for name, value in globals().items()
        if not name.startswith("__") and name not in _RESTRICTIONS_MODULE.__dict__
    }
)
run_restrictions = _RESTRICTIONS_MODULE.run_restrictions


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="Curate OSM interchanges into the world source.")
    parser.add_argument(
        "--write",
        action="store_true",
        help="Write discovered interchanges back into the world source.",
    )
    parser.add_argument(
        "--only",
        default="",
        help="Legs to rebuild, as SLUG pairs -- "
        "'corpus_christi_tx_us->san_antonio_tx_us', semicolons between several. "
        "Spoken city names do not match anything.",
    )
    parser.add_argument("--max-legs", type=int, default=0)
    parser.add_argument("--rate-limit", type=float, default=1.0)
    parser.add_argument(
        "--ors",
        action="store_true",
        help="Source dense route geometry from the self-hosted OpenRouteService "
        "(ORS_BASE_URL/ORS_API_KEY) instead of public OSRM, so the whole "
        "interchange sweep runs locally. Pair with OVERPASS_URL for the "
        "junction queries.",
    )
    parser.add_argument(
        "--force", action="store_true", help="Re-discover legs that already have interchanges."
    )
    parser.add_argument(
        "--pbf",
        type=Path,
        action="append",
        default=[],
        help=(
            "Read OSM interchange candidates from a local Geofabrik/"
            "OpenStreetMap .osm.pbf extract instead of Overpass. "
            "May be passed more than once."
        ),
    )
    parser.add_argument(
        "--local-index-cache",
        type=Path,
        help=(
            "Reusable JSON cache for route-filtered local OSM "
            "interchange features. Defaults beside --pbf."
        ),
    )
    parser.add_argument(
        "--rebuild-local-index",
        action="store_true",
        help="Ignore any existing --local-index-cache and rebuild it.",
    )
    parser.add_argument(
        "--maxspeed",
        action="store_true",
        help="Bake real OSM maxspeed onto legs as a speed_limits "
        "profile (the runtime prefers it over the heuristic) "
        "instead of discovering interchanges. Reads local "
        "per-state extracts, auto-selected from --osm-region-dir "
        "unless --pbf is given.",
    )
    parser.add_argument(
        "--restrictions",
        action="store_true",
        help="Bake posted low-clearance (maxheight) and weight-limit "
        "(maxweight) advisories onto legs as corridor.restrictions, read "
        "from local per-state extracts like --maxspeed. An empty baked "
        "list records a clean sweep.",
    )
    parser.add_argument(
        "--ramp-controls",
        action="store_true",
        help="Bake ramp-terminal controls (traffic light / stop sign) onto "
        "baked interchanges from highway=traffic_signals and highway=stop "
        "nodes sitting on motorway_link ways, read from local per-state "
        "extracts like --maxspeed.",
    )
    parser.add_argument(
        "--osm-region-dir",
        type=Path,
        default=OSM_REGION_CACHE_DIR,
        help=(
            "Directory of per-state Geofabrik extracts "
            "(<state>-latest.osm.pbf) for --maxspeed auto-selection. "
            f"Default: {OSM_REGION_CACHE_DIR}."
        ),
    )
    args = parser.parse_args(argv)

    data = load_world()
    if args.ramp_controls:
        return run_ramp_controls(data, args)
    if args.restrictions:
        return run_restrictions(data, args)
    if args.maxspeed:
        return run_maxspeed(data, args)
    legs = data["legs"]
    if args.only:
        legs = select_only(legs, args.only)

    eligible = 0
    index_legs: list[dict[str, Any]] = []
    process_legs: list[dict[str, Any]] = []
    local_mode = bool(args.pbf or args.local_index_cache)
    for leg in legs:
        corridor = leg.setdefault("corridor", {})
        if not _eligible_for_interchange_discovery(
            leg, local_mode=local_mode, explicitly_selected=bool(args.only)
        ):
            continue
        eligible += 1
        index_legs.append(leg)
        if corridor.get("interchanges") and not args.force:
            continue
        if not args.max_legs or len(process_legs) < args.max_legs:
            process_legs.append(leg)

    local_index: LocalOsmIndex | None = None
    pbf_paths = list(args.pbf)
    if pbf_paths:
        missing = [path for path in pbf_paths if not path.exists()]
        if missing:
            raise SystemExit("OSM PBF not found: " + ", ".join(str(path) for path in missing))
        bounds = _local_prefilter_bounds(index_legs)
        cache_path = args.local_index_cache or (
            _default_local_index_cache(pbf_paths[0])
            if len(pbf_paths) == 1
            else pbf_paths[0].with_name("freight-fate-interchanges.json")
        )
        print(
            f"Reading {len(pbf_paths)} local OSM extract(s) "
            f"({len(bounds)} route segment bbox filters, cache {cache_path})",
            flush=True,
        )
        local_index = load_or_build_local_index(
            pbf_paths,
            bounds,
            cache_path,
            rebuild=args.rebuild_local_index,
        )
        print(
            f"    using {len(local_index.junctions)} motorway junctions and "
            f"{len(local_index.ramps)} destination-tagged motorway ramps",
            flush=True,
        )
    elif args.local_index_cache:
        if args.rebuild_local_index:
            raise SystemExit("--rebuild-local-index requires at least one --pbf")
        bounds = _local_prefilter_bounds(index_legs)
        local_index = load_local_index_cache_only(args.local_index_cache, bounds)

    ors_key = os.environ.get("ORS_API_KEY") or "selfhosted"
    total_added = 0
    updated_legs = 0
    processed = 0
    for leg in process_legs:
        corridor = leg.setdefault("corridor", {})
        processed += 1
        label = f"{leg['from']}->{leg['to']} ({leg['highway']})"
        print(f"[{processed}] {label}", flush=True)
        geom = None
        if args.ors:
            geom = _ors_geometry(data, leg, args.rate_limit, ors_key)
            if not geom:
                print("    (no ORS geometry; skipped)", flush=True)
                continue
        found = discover_leg(leg, args.rate_limit, local_index, geom=geom)
        print(f"    {len(found)} interchanges", flush=True)
        if not found and local_index is not None and corridor.get("interchanges"):
            # The local index answers for every junction within reach of the
            # polyline, so nothing found means the leg drives no exits; what
            # it carried belongs to a road it left. (An Overpass miss can be a
            # busy server, so that path keeps what it had.)
            print(
                f"    CLEARED {len(corridor['interchanges'])} exits: no junction on the polyline",
                flush=True,
            )
            corridor["interchanges"] = []
            updated_legs += 1
        if found:
            corridor["interchanges"] = found
            total_added += len(found)
            updated_legs += 1
        # Flush periodically so a long crawl is crash-safe and resumable: a
        # re-run without --force skips legs already written here.
        if args.write and updated_legs and processed % 10 == 0:
            save_world(data)
            print(f"    ...checkpointed the world source ({updated_legs} legs so far)", flush=True)

    print(
        f"\n{eligible} legs eligible; "
        f"{processed} processed, {updated_legs} populated, "
        f"{total_added} interchanges total."
    )
    if args.write and updated_legs:
        save_world(data)
        print(f"Wrote {WORLD_SOURCE_PATH}")
    elif not args.write:
        print("(dry run; pass --write to update the world source)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
