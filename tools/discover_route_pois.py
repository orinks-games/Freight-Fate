"""Discover truck-relevant POI candidates near checked-in route geometry.

This is a manual curation helper. It performs live Overpass requests for one
corridor and prints clean candidate summaries. Runtime gameplay never calls it.
"""

from __future__ import annotations

import argparse
import json
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
from typing import Any

from world_source import load_world

OVERPASS_URL = "https://overpass-api.de/api/interpreter"
USER_AGENT = "Freight-Fate route POI curation smoke (https://github.com/orinks-games/Freight-Fate)"


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        description="Query Overpass for truck-relevant POIs near a Freight Fate corridor."
    )
    parser.add_argument("--from-city")
    parser.add_argument("--to-city")
    parser.add_argument(
        "--all",
        action="store_true",
        help="Query every leg in the world source, optionally capped by --max-legs.",
    )
    parser.add_argument("--max-legs", type=int, default=0)
    parser.add_argument("--radius-m", type=int, default=12_000)
    parser.add_argument("--limit-points", type=int, default=0)
    parser.add_argument("--rate-limit", type=float, default=1.0)
    parser.add_argument("--json", action="store_true")
    args = parser.parse_args(argv)

    data = load_world()
    if not args.all and (not args.from_city or not args.to_city):
        raise SystemExit("--from-city and --to-city are required unless --all is used")
    legs = data["legs"] if args.all else [_require_leg(data, args.from_city, args.to_city)]
    if args.max_legs:
        legs = legs[: args.max_legs]
    reports = [
        discover_leg(
            leg,
            radius_m=args.radius_m,
            limit_points=args.limit_points,
            rate_limit=args.rate_limit,
        )
        for leg in legs
    ]
    if args.json:
        print(json.dumps({"endpoint": OVERPASS_URL, "legs": reports}, indent=2, sort_keys=True))
    else:
        for report in reports:
            _print_report(report)
    return 0


def discover_leg(
    leg: dict[str, Any],
    *,
    radius_m: int,
    limit_points: int,
    rate_limit: float,
) -> dict[str, Any]:
    points = _sample_points(leg, limit_points)
    candidates: dict[str, dict[str, Any]] = {}
    queries = []
    errors = []
    for point in points:
        query = _query(point["lat"], point["lon"], radius_m)
        queries.append(
            {
                "at_mi": point["at_mi"],
                "lat": point["lat"],
                "lon": point["lon"],
                "radius_m": radius_m,
            }
        )
        try:
            payload = _post_overpass(query)
        except (TimeoutError, OSError, urllib.error.URLError, urllib.error.HTTPError) as exc:
            errors.append(
                {
                    "at_mi": point["at_mi"],
                    "lat": point["lat"],
                    "lon": point["lon"],
                    "error": f"{type(exc).__name__}: {exc}",
                }
            )
            continue
        for element in payload.get("elements", []):
            candidate = _candidate_from_element(element, point["at_mi"])
            if candidate is None:
                continue
            key = candidate["key"]
            existing = candidates.get(key)
            if existing is None or abs(candidate["sample_at_mi"] - point["at_mi"]) < abs(
                existing["sample_at_mi"] - point["at_mi"]
            ):
                candidates[key] = candidate
        if rate_limit > 0:
            time.sleep(rate_limit)

    return {
        "from": leg["from"],
        "to": leg["to"],
        "highway": leg["highway"],
        "miles": leg["miles"],
        "endpoint": OVERPASS_URL,
        "queries": queries,
        "errors": errors,
        "candidate_count": len(candidates),
        "candidates": sorted(candidates.values(), key=lambda item: item["sample_at_mi"]),
    }


def _find_leg(data: dict[str, Any], from_city: str, to_city: str) -> dict[str, Any] | None:
    for leg in data["legs"]:
        if leg["from"] == from_city and leg["to"] == to_city:
            return leg
    return None


def _require_leg(data: dict[str, Any], from_city: str, to_city: str) -> dict[str, Any]:
    leg = _find_leg(data, from_city, to_city)
    if leg is None:
        raise SystemExit(f"No direct world leg {from_city} to {to_city}")
    return leg


def _print_report(report: dict[str, Any]) -> None:
    print(
        f"{report['from']} to {report['to']} via {report['highway']}: "
        f"{report['candidate_count']} candidates"
    )
    print(f"Endpoint: {report['endpoint']}")
    for query in report["queries"]:
        print(
            "Query around "
            f"{query['lat']:.5f},{query['lon']:.5f} "
            f"at {query['at_mi']:.1f} mi radius {query['radius_m']} m"
        )
    for error in report["errors"]:
        print(
            "Overpass error around "
            f"{error['lat']:.5f},{error['lon']:.5f} "
            f"at {error['at_mi']:.1f} mi: {error['error']}"
        )
    for candidate in report["candidates"]:
        print(
            f"- {candidate['sample_at_mi']:.1f} mi sample: "
            f"{candidate['name']} [{candidate['type']}] "
            f"{candidate['lat']:.5f},{candidate['lon']:.5f} "
            f"services={','.join(candidate['services'])}"
        )


def _sample_points(leg: dict[str, Any], limit: int) -> list[dict[str, float]]:
    points = list(leg.get("corridor", {}).get("route_points", ()))
    if not points:
        raise SystemExit("Leg has no route_points to query around.")
    if limit and len(points) > limit:
        if limit == 1:
            return [points[len(points) // 2]]
        indexes = sorted({round(i * (len(points) - 1) / (limit - 1)) for i in range(limit)})
        return [points[index] for index in indexes]
    return points


def _query(lat: float, lon: float, radius_m: int) -> str:
    return f"""
    [out:json][timeout:25];
    (
      node["highway"~"services|rest_area"](around:{radius_m},{lat},{lon});
      way["highway"~"services|rest_area"](around:{radius_m},{lat},{lon});
      relation["highway"~"services|rest_area"](around:{radius_m},{lat},{lon});
      node["amenity"="fuel"]["hgv"~"yes|designated"](around:{radius_m},{lat},{lon});
      way["amenity"="fuel"]["hgv"~"yes|designated"](around:{radius_m},{lat},{lon});
      node["amenity"="parking"]["hgv"~"yes|designated"](around:{radius_m},{lat},{lon});
      way["amenity"="parking"]["hgv"~"yes|designated"](around:{radius_m},{lat},{lon});
      node["amenity"="weighbridge"](around:{radius_m},{lat},{lon});
      way["amenity"="weighbridge"](around:{radius_m},{lat},{lon});
      node["name"~"Love's|Pilot|Flying J|TravelCenters|TA |Petro|Road Ranger|truck stop|travel center",i](around:{radius_m},{lat},{lon});
      way["name"~"Love's|Pilot|Flying J|TravelCenters|TA |Petro|Road Ranger|truck stop|travel center",i](around:{radius_m},{lat},{lon});
    );
    out tags center 40;
    """


def _post_overpass(query: str) -> dict[str, Any]:
    body = urllib.parse.urlencode({"data": query}).encode("utf-8")
    request = urllib.request.Request(
        OVERPASS_URL,
        data=body,
        headers={
            "User-Agent": USER_AGENT,
            "Content-Type": "application/x-www-form-urlencoded",
        },
    )
    with urllib.request.urlopen(request, timeout=45) as response:
        return json.loads(response.read().decode("utf-8"))


def _candidate_from_element(element: dict[str, Any], sample_at_mi: float) -> dict[str, Any] | None:
    tags = element.get("tags", {})
    name = _clean_name(tags.get("name") or tags.get("brand") or tags.get("operator") or "")
    if not name:
        highway = tags.get("highway", "")
        if highway == "rest_area":
            name = "Unnamed rest area"
        elif highway == "services":
            name = "Unnamed service area"
        else:
            return None
    lat = element.get("lat") or element.get("center", {}).get("lat")
    lon = element.get("lon") or element.get("center", {}).get("lon")
    if lat is None or lon is None:
        return None
    candidate_type = _candidate_type(tags, name)
    services = _services(tags, candidate_type)
    return {
        "key": f"{element.get('type')}:{element.get('id')}",
        "name": name,
        "type": candidate_type,
        "lat": float(lat),
        "lon": float(lon),
        "sample_at_mi": float(sample_at_mi),
        "services": services,
        "tags": {
            key: value
            for key, value in tags.items()
            if key in {"amenity", "highway", "hgv", "parking", "brand", "operator", "name"}
        },
    }


def _candidate_type(tags: dict[str, str], name: str) -> str:
    if tags.get("amenity") == "weighbridge" or "weigh" in name.lower():
        return "weigh_station"
    if tags.get("highway") == "services":
        return "service_plaza"
    if tags.get("highway") == "rest_area":
        return "public_rest_area"
    if tags.get("amenity") == "parking":
        return "truck_parking"
    if any(text in name.lower() for text in ("pilot", "flying j", "love's", "ta ", "petro")):
        return "travel_center"
    return "fuel_station"


def _services(tags: dict[str, str], candidate_type: str) -> list[str]:
    services = []
    if candidate_type in {"travel_center", "fuel_station", "service_plaza"}:
        services.append("diesel")
    if candidate_type in {"travel_center", "service_plaza"}:
        services.append("food")
    if candidate_type != "weigh_station":
        services.append("parking")
    if tags.get("amenity") == "weighbridge" or candidate_type == "weigh_station":
        services.append("inspection")
    return services


def _clean_name(value: str) -> str:
    name = " ".join(str(value).replace("\n", " ").split()).strip()
    lowered = name.lower()
    if any(marker in lowered for marker in ("osm", "amenity=", "highway=", "node/", "way/")):
        return ""
    return name[:80]


if __name__ == "__main__":
    sys.exit(main())
