"""What road does each baked curve sit on? Ask a map matcher.

This replaces the old ``curve_osm_facts.py`` (deleted), which answered the
same question by streaming 14 GB of Geofabrik extracts and taking the nearest
way segment to each curve's apex. Nearest-distance is the wrong tool: at an interchange the
ramp and the mainline it leaves are metres apart, and a point has no way to
know which one the truck is on.

Map matching does know, because it snaps the WHOLE polyline at once and the
answer has to be a connected path. Valhalla's ``trace_attributes`` returns,
per matched edge, the road class, whether the edge is a ramp or turn channel,
its names and refs, and its speed limit -- so the readings the connector bake
needs come from one call to a purpose-built matcher.

Two fields carry the verdict, and they are separate on purpose where OSM
conflates them into ``highway=*_link``:

  ``edge.use``         ``ramp`` or ``turn_channel`` -- interchange geometry,
                       whatever class of road it carries. A ramp off a trunk
                       is still a ramp.
  ``edge.road_class``  ``motorway``, ``trunk``, ``primary`` ... the through
                       road's own importance, which is what the connector
                       rule compares a bend against.

Coverage is all or nothing per leg: a leg whose polyline leaves the built
tileset comes back partly unmatched, and an unmatched apex is recorded as
having NO reading rather than guessed at. Build tiles covering the whole
network before trusting a run.

Running the matcher
-------------------
The tileset is built by the ``valhalla-scripted`` image. Three things bite on
Windows and are worth writing down:

  * ``tile_urls`` must be passed even when empty -- the entrypoint runs under
    ``set -u`` and dies on the unbound variable.
  * Git Bash mangles the volume path. Use ``MSYS_NO_PATHCONV=1`` and a
    ``C:/...`` path, or the PBF is silently not mounted and the build reports
    "No local PBF files".
  * Do NOT disable ``build_admins``. The ``enhance`` stage uses the admin
    database and segfaults without it.

    MSYS_NO_PATHCONV=1 docker run -d --name valhalla -p 8002:8002 \\
      -v "C:/Users/joshu/.cache/valhalla/us:/custom_files" \\
      -e tile_urls= -e serve_tiles=True \\
      ghcr.io/valhalla/valhalla-scripted:latest

Usage
-----
    uv run python tools/curve_valhalla_facts.py --all
    uv run python tools/curve_valhalla_facts.py --legs a:b,c:d
    uv run python tools/curve_valhalla_facts.py --state co --out facts.jsonl
"""

from __future__ import annotations

import argparse
import json
import math
import sys
import time
import urllib.error
import urllib.request
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import straw_curve_sample as scs  # noqa: E402
from bake_curve_connectors import FACTS  # noqa: E402
from bake_divided import load_geometry_by_code  # noqa: E402
from world_source import load_world  # noqa: E402

# The production bake widens the straw margin; the archive was written with
# it, so re-detection has to use the same value or the rows will not line up.
scs.CURVE_PAD_M = 150.0

ROOT = Path(__file__).resolve().parent.parent
CURVES = ROOT / "data" / "world_data" / "us" / "gameplay" / "curves.jsonl"

# FOSSGIS runs a public, planet-wide Valhalla. It answers trace_attributes,
# which means the whole tileset build is optional: a 400-point chunk of I-70
# through Glenwood Canyon matched 400 of 400 in 0.7 seconds against it.
# Default to the public one and let --url point at a local build instead.
PUBLIC_URL = "https://valhalla1.openstreetmap.de/trace_attributes"
LOCAL_URL = "http://localhost:8002/trace_attributes"
USER_AGENT = "Freight-Fate map-matching (https://github.com/orinks-games/Freight-Fate)"
COSTING = "truck"  # the vehicle actually being routed, so its restrictions apply

# What the truck actually is. Valhalla's truck costing defaults to 21.77
# tonnes -- about 48,000 lb -- which is not a loaded US semi, and a weight
# limit it would clear at that figure it would not clear at 80,000. Measured
# on Newark to Hunts Point, truck costing already routes very differently from
# car (61.7 miles over the George Washington Bridge against 23 straight
# through the truck-banned tunnels), so the profile is doing real work; these
# numbers make it do it for the right vehicle.
#
# 80,000 lb gross and 13 ft 6 in are the federal maxima on the Interstate
# system (23 CFR 658.17 for weight, and the height every state signs to);
# 53 ft is the standard trailer, 8 ft 6 in the standard width, and 34,000 lb
# is the tandem-axle limit that goes with the 80,000.
TRUCK_OPTIONS = {
    "height": 4.11,  # metres, 13 ft 6 in
    "width": 2.59,  # 8 ft 6 in
    "length": 21.64,  # 71 ft tractor plus 53 ft trailer
    "weight": 36.29,  # tonnes, 80,000 lb
    "axle_load": 15.42,  # tonnes, 34,000 lb tandem
    "hazmat": False,
}


# What ``edge.use`` calls interchange geometry.
RAMP_USES = frozenset({"ramp", "turn_channel"})

# Valhalla caps a trace by PATH DISTANCE, not by point count: the public
# instance refuses anything over 200 km with error 154. Chunking by points
# misses that entirely -- a leg with 1 km vertex spacing blows the limit in
# 257 points, and the whole leg then comes back unmatched. So chunks are cut
# by distance, at three quarters of the limit because the matched path can run
# slightly longer than the shape it was given.
MAX_CHUNK_M = 150_000.0
MAX_SHAPE_POINTS = 1000  # a second bound, for dense city geometry
CHUNK_OVERLAP_M = 1_000.0  # context either side of a cut, so no edge is guessed at

# One mile between the route samples that say what a leg is MADE of.
COVERAGE_STEP_M = 1609.344


def load_curve_rows() -> dict[str, list[dict]]:
    out: dict[str, list[dict]] = {}
    for line in CURVES.read_text(encoding="utf-8").splitlines():
        if not line.strip() or line.startswith('{"meta"'):
            continue
        row = json.loads(line)
        out.setdefault(row["leg"], []).append(row)
    return out


def _post(url: str, body: dict, delay: float, attempts: int = 4) -> dict | None:
    """One trace, with backoff. Returns None once the retries are spent.

    The public instance is a free community service, so this throttles between
    calls and backs off rather than hammering it. A 400 is the matcher saying
    it could not snap the shape and is not worth retrying; anything else is
    treated as transient.
    """
    payload = json.dumps(body).encode("utf-8")
    for attempt in range(attempts):
        request = urllib.request.Request(
            url,
            data=payload,
            headers={"Content-Type": "application/json", "User-Agent": USER_AGENT},
        )
        try:
            with urllib.request.urlopen(request, timeout=300) as response:
                if delay:
                    time.sleep(delay)
                return json.loads(response.read())
        except urllib.error.HTTPError as exc:
            if exc.code == 400:
                return None  # unsnappable shape, not a transient failure
            time.sleep(delay + 2.0 * (attempt + 1))
        except (urllib.error.URLError, TimeoutError, OSError):
            time.sleep(delay + 2.0 * (attempt + 1))
    return None


def densify(coords: list[list[float]], step_m: float = COVERAGE_STEP_M / 2.0):
    """``(coords with a point at least every step_m, original index -> new)``.

    The archive keeps a vertex wherever the road BENDS and drops them where it
    runs straight, which is exactly right for reading a curve and exactly
    wrong for reading what a leg is made of: the surviving vertices cluster on
    interchanges, ramps, city approaches and mountain bends, so a coverage
    sample taken only at vertices barely sees the long straight interstate
    running between them.

    Measured across the network, the bias is monotonic in how much the
    simplifier collapsed -- legs whose longest hop is under 5 percent of their
    length read a median 85 percent on their own shield, legs over 20 percent
    read 62 -- and it is worst on the freeway-heaviest legs, which are the
    ones with the longest tangents to collapse.

    Points go in at HALF the sample step. At exactly the step the sampler
    skips every other one -- it only takes a sample once the distance since
    the last one reaches the step, so a point landing a metre short is passed
    over and the next is a whole step late.

    Interpolating along the collapsed tangents costs nothing in fidelity: the
    chord IS the road there (a route point sits a median 0.009 miles off the
    archived line), which is why the simplifier was allowed to drop them.
    """
    out: list[list[float]] = [coords[0]]
    index_of: list[int] = [0]
    for a, b in zip(coords, coords[1:], strict=False):
        span = _haversine_m(a[1], a[0], b[1], b[0])
        for k in range(1, int(span // step_m) + 1):
            t = k * step_m / span
            out.append([a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t])
        out.append(list(b))
        index_of.append(len(out) - 1)
    return out, index_of


def _haversine_m(lat1: float, lon1: float, lat2: float, lon2: float) -> float:
    return scs._haversine_m(lat1, lon1, lat2, lon2)


def match_leg(coords: list[list[float]], url: str, delay: float) -> dict[int, dict]:
    """``vertex index -> matched edge`` for one leg's polyline.

    Chunked with overlap so a cut never robs the matcher of the context it
    needs to resolve which way a point belongs to. A vertex the matcher could
    not place is simply absent, and the caller records that as no reading.
    """
    out: dict[int, dict] = {}
    cum = scs._cumulative_m(coords)
    start = 0
    while start < len(coords):
        stop = start + 1
        while (
            stop < len(coords)
            and cum[stop] - cum[start] < MAX_CHUNK_M
            and stop - start < MAX_SHAPE_POINTS
        ):
            stop += 1
        chunk = coords[start:stop]
        if len(chunk) < 2:
            break
        result = _post(
            url,
            {
                "shape": [{"lat": lat, "lon": lon} for lon, lat in chunk],
                "costing": COSTING,
                "costing_options": {COSTING: TRUCK_OPTIONS},
                "shape_match": "map_snap",
                "filters": {
                    "attributes": [
                        "edge.road_class",
                        "edge.use",
                        "edge.names",
                        "edge.speed_limit",
                        "edge.way_id",
                        "matched.point_index",
                        "matched.edge_index",
                        "matched.type",
                    ],
                    "action": "include",
                },
            },
            delay,
        )
        if result:
            edges = result.get("edges") or []
            for offset, point in enumerate(result.get("matched_points") or []):
                index = point.get("edge_index")
                if index is None or index >= len(edges):
                    continue
                if str(point.get("type")) == "unmatched":
                    continue
                out[start + offset] = edges[index]
        if stop >= len(coords):
            break
        back = stop - 1
        while back > start + 1 and cum[stop - 1] - cum[back] < CHUNK_OVERLAP_M:
            back -= 1
        start = back
    return out


def facts_for(leg_id: str, seq: int, edge: dict | None) -> dict:
    """One curve's reading, in the shape ``bake_curve_connectors`` expects."""
    if edge is None:
        return {"leg": leg_id, "seq": seq, "near_m": None}
    use = str(edge.get("use") or "")
    road_class = str(edge.get("road_class") or "")
    names = edge.get("names") or []
    # The connector rule reads ``near_hw`` as an OSM highway value, so a ramp
    # is reported the way OSM would tag it. The matcher's own words are kept
    # alongside, because they are the evidence.
    near_hw = f"{road_class}_link" if use in RAMP_USES else road_class
    return {
        "leg": leg_id,
        "seq": seq,
        "near_m": 0.0,  # a matched edge IS the road; there is no offset to report
        "near_hw": near_hw,
        "near_ref": ";".join(str(n) for n in names),
        "near_name": names[0] if names else "",
        "valhalla_use": use,
        "valhalla_road_class": road_class,
        "valhalla_way_id": edge.get("way_id"),
        "valhalla_speed_limit": edge.get("speed_limit"),
    }


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    group = ap.add_mutually_exclusive_group(required=True)
    group.add_argument("--all", action="store_true")
    group.add_argument("--state", help="comma-separated from-state codes")
    group.add_argument("--legs", help="comma-separated leg ids (from:to)")
    ap.add_argument("--out", help=f"facts file to write (default {FACTS})")
    ap.add_argument("--url", default=PUBLIC_URL, help=f"matcher endpoint (default {PUBLIC_URL})")
    ap.add_argument("--local", action="store_true", help=f"shorthand for --url {LOCAL_URL}")
    ap.add_argument(
        "--delay",
        type=float,
        default=None,
        help="seconds between calls; defaults to 0.4 against the public service, 0 locally",
    )
    args = ap.parse_args()
    url = LOCAL_URL if args.local else args.url
    delay = args.delay if args.delay is not None else (0.0 if url == LOCAL_URL else 0.4)

    status = url.rsplit("/", 1)[0] + "/status"
    try:
        probe = urllib.request.Request(status, headers={"User-Agent": USER_AGENT})
        actions = json.loads(urllib.request.urlopen(probe, timeout=20).read())
    except Exception as exc:
        print(f"{status} is not answering ({exc}) -- see this module's docstring.")
        return 1
    if "trace_attributes" not in (actions.get("available_actions") or []):
        print(f"{status} does not offer trace_attributes.")
        return 1
    print(f"matching against {url} (delay {delay}s between calls)")

    out_path = Path(args.out) if args.out else FACTS
    wanted = None if args.all or args.legs else {s.strip().lower() for s in args.state.split(",")}
    wanted_legs = {item.strip() for item in args.legs.split(",")} if args.legs else None
    world = load_world()
    cities = world["cities"]
    rows_by_leg = load_curve_rows()

    geom_cache: dict[str, dict[str, list]] = {}
    lines: list[str] = []
    coverage: list[str] = []
    done = skipped = unread = 0
    legs = sorted(world["legs"], key=lambda L: (L["from"], L["to"]))
    for n, leg in enumerate(legs, 1):
        code = str(cities[leg["from"]]["state"]).lower()
        if wanted and code not in wanted:
            continue
        leg_id = f"{leg['from']}:{leg['to']}"
        if wanted_legs is not None and leg_id not in wanted_legs:
            continue
        rows = rows_by_leg.get(leg_id)
        if not rows:
            continue
        if code not in geom_cache:
            geom_cache[code] = load_geometry_by_code(code)
        coords = geom_cache[code].get(leg_id)
        if not coords or len(coords) < 3:
            skipped += 1
            continue
        cum = scs._cumulative_m(coords)
        detected = scs.analyse_curvature(coords, cum)["curves"]
        if len(detected) != len(rows):
            skipped += 1
            continue
        dense, index_of = densify(coords)
        matched = match_leg(dense, url, delay)
        for curve, row in zip(detected, rows, strict=False):
            fact = facts_for(leg_id, row["seq"], matched.get(index_of[curve["_apex"]]))
            unread += fact.get("near_m") is None
            lines.append(json.dumps(fact, sort_keys=True))
        # What the leg is MADE of, from the same matching: one sample a mile.
        classes: dict[str, int] = {}
        refs: dict[str, int] = {}
        samples = 0
        last = -math.inf
        dense_cum = scs._cumulative_m(dense)
        for i in range(len(dense)):
            if dense_cum[i] - last < COVERAGE_STEP_M and i != len(dense) - 1:
                continue
            last = dense_cum[i]
            samples += 1
            edge = matched.get(i)
            if edge is None or str(edge.get("use")) in RAMP_USES:
                continue
            road_class = str(edge.get("road_class") or "")
            if road_class:
                classes[road_class] = classes.get(road_class, 0) + 1
            for name in edge.get("names") or []:
                refs[str(name)] = refs.get(str(name), 0) + 1
        coverage.append(
            json.dumps(
                {
                    "leg": leg_id,
                    "coverage_samples": samples,
                    "coverage_on_motorway": classes.get("motorway", 0),
                    # Class AND number: matching on the digits alone credits
                    # US 95 to a leg named I-95 and reports it faithful to a
                    # road it never touches.
                    "coverage_on_shield": sum(
                        count
                        for ref, count in refs.items()
                        if scs.matches_shield(ref, str(leg.get("highway", "")))
                    ),
                    "ridden_refs": dict(sorted(refs.items(), key=lambda kv: -kv[1])[:8]),
                    "ridden_classes": dict(sorted(classes.items(), key=lambda kv: -kv[1])),
                },
                sort_keys=True,
            )
        )
        done += 1
        if done % 25 == 0:
            print(f"  matched {done} legs ({n}/{len(legs)} scanned)", flush=True)

    out_path.parent.mkdir(parents=True, exist_ok=True)
    out_path.write_text("\n".join(coverage + lines) + "\n", encoding="utf-8")
    total = len(lines)
    print(f"\n{done} legs matched, {skipped} skipped (rows did not re-detect or no geometry)")
    print(
        f"{total} curve readings, {unread} with no matched edge ({100 * unread / max(1, total):.1f}%)"
    )
    print(f"wrote {out_path}")
    if unread > total * 0.02:
        print("\nWARNING: more than 2 percent unmatched. The tileset probably does not")
        print("cover the whole network -- check the build before trusting this.")
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
