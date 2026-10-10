"""Ask Overpass about a corridor, not about the box a corridor fits inside.

The per-leg bakes each asked one question: every ``maxspeed``-tagged (or
``lanes``-tagged) road inside the leg's bounding box. On a self-hosted server
holding a filtered extract that is fine. Against the public service it is not:
a 150-mile leg's bounding box is most of a state, and the answer is tens of
thousands of ways of which a few hundred are on the road. The service replies
"the server is probably too busy to handle your request" -- with a 200 and an
HTML body, which is its own trap; see ``TooBusy`` below.

The corridor is what was ever wanted. A way only governs a sample point if it
snaps within ``MATCH_CORRIDOR_M`` -- 90 metres -- so boxes strung along the
route and padded far wider than that contain every way that could match, and
nothing else. Same result, a fraction of the data, and each request small
enough to answer.

Requests name themselves (the public endpoints answer 406 to an unidentified
client), retry with backoff, and fall through to the mirrors, because a
free service refusing one request is not a fact about the map.
"""

from __future__ import annotations

import http.client
import json
import os
import time
import urllib.error
import urllib.parse
import urllib.request
from typing import Any

DEFAULT_MIRRORS = (
    "https://overpass-api.de/api/interpreter",
    "https://overpass.kumi.systems/api/interpreter",
    "https://overpass.private.coffee/api/interpreter",
)
# OVERPASS_URLS lets a bake choose reachable Overpass mirrors.
MIRRORS = (
    tuple(url.strip() for url in os.environ.get("OVERPASS_URLS", "").split(",") if url.strip())
    or DEFAULT_MIRRORS
)
USER_AGENT = "Freight-Fate world bake (https://github.com/orinks-games/Freight-Fate)"

# How far along the route one box may reach before the next one starts.
#
# MEASURED against the public service, on the same leg, same query, back to
# back: at 0.5 degrees it answered "the server is probably too busy" more
# often than it answered at all; at 0.25 it answered every time, in 4 to 25
# seconds. So the box size is the whole difference between a bake that
# finishes and one that spends the afternoon backing off -- it was never that
# the service could not be used. A quarter degree is roughly 17 miles, so a
# 150-mile leg is seven requests.
MAX_SPAN_DEG = 0.25
# Box padding. Two orders of magnitude more than the 90 m match corridor, so a
# way that could govern a sample point cannot fall outside the box that
# sample sits in.
PAD_DEG = 0.02

DELAY_S = 2.0  # between calls: a free community service, not a firehose
ATTEMPT_TIMEOUT_S = 300
RESPONSE_CHUNK_SIZE = 64 * 1024
# Overpass runs a small number of slots and says so (``/api/status``: "Rate
# limit: 2"). Over that line it answers 429, and under load 500 or 504 -- all
# three mean "later", not "no such data". So a refusal waits minutes rather
# than seconds before giving up: a bake that reads a busy server as an empty
# map writes an empty layer and reports success, which is the failure this
# whole job keeps hitting.
BACKOFF_S = (5.0, 20.0, 60.0, 120.0)


class TooBusy(Exception):
    """Overpass refused, in one of the several ways it does that.

    It does NOT always refuse with a status code. A busy dispatcher answers
    200 with an HTML page reading "The server is probably too busy to handle
    your request", and a query that runs out of time answers 200 with JSON
    carrying a ``remark``. Both parse as success to anything that only checks
    the status, and the HTML crashes a JSON decoder outright -- which is how
    a busy afternoon turns into either an empty layer or a dead bake.
    """


def _parse(body: str) -> dict[str, Any]:
    text = body.lstrip()
    if not text.startswith("{"):
        raise TooBusy(text.strip()[:200].replace("\n", " "))
    payload = json.loads(text)
    remark = str(payload.get("remark") or "")
    if remark and not payload.get("elements"):
        raise TooBusy(remark[:200])
    return payload


def post(query: str, urls: tuple[str, ...] = MIRRORS) -> dict[str, Any]:
    """One Overpass query, retried across the mirrors. Raises when all fail."""
    data = urllib.parse.urlencode({"data": query}).encode("utf-8")
    last: Exception | None = None
    for wait in (0.0, *BACKOFF_S):
        if wait:
            time.sleep(wait)
        for url in urls:
            request = urllib.request.Request(url, data=data, headers={"User-Agent": USER_AGENT})
            try:
                # Bound the full attempt even when the server trickles response bytes.
                deadline = time.monotonic() + ATTEMPT_TIMEOUT_S
                with urllib.request.urlopen(request, timeout=ATTEMPT_TIMEOUT_S) as response:
                    chunks = []
                    while True:
                        if time.monotonic() >= deadline:
                            raise TimeoutError("Overpass attempt exceeded its wall-clock deadline")
                        chunk = response.read1(RESPONSE_CHUNK_SIZE)
                        if time.monotonic() >= deadline:
                            raise TimeoutError("Overpass attempt exceeded its wall-clock deadline")
                        if not chunk:
                            break
                        chunks.append(chunk)
                body = b"".join(chunks).decode("utf-8", "replace")
                payload = _parse(body)
                time.sleep(DELAY_S)
                return payload
            except (
                urllib.error.HTTPError,
                urllib.error.URLError,
                TimeoutError,
                OSError,
                # A chunked body that stops early. NOT an OSError, so it went
                # straight past this handler and killed an hour of baking on
                # its twenty-first leg. It means "ask again", same as a
                # timeout does.
                http.client.HTTPException,
                # ...and a body that arrives truncated is invalid JSON, which
                # is the same event seen from one layer up.
                json.JSONDecodeError,
                TooBusy,
            ) as exc:
                last = exc
                time.sleep(DELAY_S)
    raise RuntimeError(f"every Overpass mirror refused the query: {last}")


def corridor_box_tuples(
    coords: list[list[float]],
    max_span_deg: float = MAX_SPAN_DEG,
    pad_deg: float = PAD_DEG,
) -> list[tuple[float, float, float, float]]:
    """``(south, west, north, east)`` boxes covering a ``[[lon, lat], ...]`` route.

    Boxes overlap by a vertex at each seam, so nothing falls between two of
    them, and every box is padded far wider than the 90 m match corridor.
    """
    boxes: list[tuple[float, float, float, float]] = []
    run: list[list[float]] = []

    def close(chunk: list[list[float]]) -> None:
        if not chunk:
            return
        lats = [point[1] for point in chunk]
        lons = [point[0] for point in chunk]
        boxes.append(
            (
                min(lats) - pad_deg,
                min(lons) - pad_deg,
                max(lats) + pad_deg,
                max(lons) + pad_deg,
            )
        )

    for point in coords:
        candidate = run + [point]
        lats = [p[1] for p in candidate]
        lons = [p[0] for p in candidate]
        # Close BEFORE the point that would overshoot, not after it. The span
        # is a measured limit on what the service will answer, so a box that
        # is one vertex over it is a box that may not come back.
        if len(run) >= 2 and (
            max(lats) - min(lats) > max_span_deg or max(lons) - min(lons) > max_span_deg
        ):
            close(run)
            run = [run[-1], point]  # overlap by a vertex so no gap opens at the seam
            continue
        run = candidate
    close(run)
    return boxes


def corridor_boxes(
    coords: list[list[float]],
    max_span_deg: float = MAX_SPAN_DEG,
    pad_deg: float = PAD_DEG,
) -> list[str]:
    """The same boxes as Overpass bbox strings."""
    return [
        f"{south},{west},{north},{east}"
        for south, west, north, east in corridor_box_tuples(coords, max_span_deg, pad_deg)
    ]


def corridor_elements(coords: list[list[float]], query_for: Any) -> list[dict[str, Any]]:
    """Union of the elements every corridor box returns, deduped by id.

    ``query_for`` takes one box string and returns the Overpass QL to run for
    it, so each bake keeps its own filter and only the plumbing is shared.
    """
    found: dict[tuple[str, int], dict[str, Any]] = {}
    for box in corridor_boxes(coords):
        payload = post(query_for(box))
        for element in payload.get("elements", ()):
            found[(str(element.get("type")), int(element.get("id", 0)))] = element
    return list(found.values())
