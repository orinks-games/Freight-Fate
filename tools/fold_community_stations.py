"""Copy the radio stations players suggested and the owner accepted into the
shipped catalog.

Accepted stations reach players two ways. The game downloads the site's list
at launch (``crates/freight-fate/src/community_stations.rs``), which is how a
new one arrives without an update. And every build ships a copy, written here
to ``data/radio_community.json``, so a first launch, an offline one, or a
player with the orinks.net services switched off still has every station
accepted up to that build. The fold-community-stations workflow runs this
before each nightly and commits the result to dev when it changed.

The file mirrors the site exactly: a station the owner accepted appears, one
whose stream has died off the site's list disappears. Rows are checked the
way the game checks the download (``ff_core::radio::community``), which
repeats the check at load, so a bad row is refused here, where someone reads
the output, rather than only skipped in a player's log.

Run from the repository root::

    uv run python tools/fold_community_stations.py
    uv run python tools/fold_community_stations.py --check
    uv run python tools/fold_community_stations.py --input saved-list.json
"""

from __future__ import annotations

import argparse
import json
import sys
import urllib.error
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
OUTPUT = ROOT / "data" / "radio_community.json"
STATIONS_URL = "https://orinks.net/api/freight-fate/stations"
USER_AGENT = "FreightFate/1.9 (catalog build)"
ID_PREFIX = "community-"
# The game's ceiling on one list (MAX_COMMUNITY_STATIONS).
MAX_STATIONS = 2000

NOTES = (
    "Radio stations players suggested on orinks.net and the owner accepted, "
    "copied from the site by tools/fold_community_stations.py. Do not edit by "
    "hand: the next fold replaces this file with the site's list."
)

# The fields the game reads from a community row. Anything else the site
# sends is left behind rather than shipped unread.
FIELDS = (
    "id",
    "name",
    "call_sign",
    "format",
    "source",
    "source_type",
    "station_type",
    "stream_url",
    "stream_format",
    "real_stream",
    "safe_for_streaming",
    "supported",
    "always_available",
    "market",
    "region",
    "lat",
    "lon",
    "range_miles",
    "frequency_mhz",
)


class FoldError(Exception):
    """The site's answer is not a list this tool will ship."""


def fetch(url: str, timeout: float) -> dict:
    request = urllib.request.Request(
        url, headers={"User-Agent": USER_AGENT, "Accept": "application/json"}
    )
    try:
        with urllib.request.urlopen(request, timeout=timeout) as response:
            return json.loads(response.read().decode("utf-8"))
    except (urllib.error.URLError, TimeoutError, ValueError) as error:
        raise FoldError(f"could not read {url}: {error}") from error


def located(row: dict) -> bool:
    lat, lon, miles = row.get("lat"), row.get("lon"), row.get("range_miles")
    return (
        isinstance(lat, (int, float))
        and isinstance(lon, (int, float))
        and isinstance(miles, (int, float))
        and -90 <= lat <= 90
        and -180 <= lon <= 180
        and miles > 0
    )


def station(row: object) -> dict:
    """One site row as the shipped row, or FoldError naming what is wrong."""
    if not isinstance(row, dict):
        raise FoldError(f"a station row is not an object: {row!r}")
    station_id = row.get("id")
    if not isinstance(station_id, str) or not station_id.startswith(ID_PREFIX):
        raise FoldError(f"station id {station_id!r} does not start with {ID_PREFIX!r}")
    name = row.get("name")
    if not isinstance(name, str) or not name.strip():
        raise FoldError(f"{station_id} has no name")
    stream = row.get("stream_url")
    if not isinstance(stream, str) or not stream.strip().lower().startswith(
        ("http://", "https://")
    ):
        raise FoldError(f"{station_id} has no http or https stream: {stream!r}")
    kept = {field: row[field] for field in FIELDS if field in row}
    # What the game would force anyway, written down so the file says what
    # the game does with it.
    kept.update(real_stream=True, safe_for_streaming=False, supported=True)
    if row.get("source_type") == "imported" and located(row):
        kept.update(source_type="imported", station_type="imported", always_available=False)
    else:
        for field in ("lat", "lon", "range_miles", "frequency_mhz"):
            kept.pop(field, None)
        kept.update(source_type="web", station_type="web", always_available=True)
    return kept


def fold(reply: object) -> dict:
    """The file to ship for one reply from the site."""
    if not isinstance(reply, dict) or not isinstance(reply.get("stations"), list):
        raise FoldError("the reply is not a station list")
    stations = [station(row) for row in reply["stations"]]
    if len(stations) > MAX_STATIONS:
        raise FoldError(f"{len(stations)} stations is more than the game takes ({MAX_STATIONS})")
    ids = [s["id"] for s in stations]
    if len(set(ids)) != len(ids):
        raise FoldError("the list names one station twice")
    stations.sort(key=lambda s: s["id"])
    return {"schema": 1, "notes": NOTES, "stations": stations}


def render(catalog: dict) -> str:
    return json.dumps(catalog, indent=1, sort_keys=True, ensure_ascii=False) + "\n"


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--url", default=STATIONS_URL)
    parser.add_argument("--input", type=Path, help="read a saved reply instead of the site")
    parser.add_argument("--output", type=Path, default=OUTPUT)
    parser.add_argument("--timeout", type=float, default=60.0)
    parser.add_argument(
        "--check",
        action="store_true",
        help="exit 1 when the shipped file differs from the site; write nothing",
    )
    args = parser.parse_args(argv)

    try:
        reply = (
            json.loads(args.input.read_text(encoding="utf-8"))
            if args.input
            else fetch(args.url, args.timeout)
        )
        text = render(fold(reply))
    except FoldError as error:
        # The shipped file is left as it was: an unreachable site must never
        # empty the dial's accepted stations.
        print(f"Not folded: {error}", file=sys.stderr)
        return 2

    current = args.output.read_text(encoding="utf-8") if args.output.exists() else ""
    count = len(json.loads(text)["stations"])
    if text == current:
        print(f"{args.output.name} is current: {count} stations")
        return 0
    if args.check:
        print(f"{args.output.name} is out of date: the site lists {count} stations")
        return 1
    args.output.write_text(text, encoding="utf-8")
    print(f"Wrote {args.output.name}: {count} stations")
    return 0


if __name__ == "__main__":
    sys.exit(main())
