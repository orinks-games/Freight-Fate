"""Tests for tools/screen_limit_artifacts.py: naming off-highway limit postings.

No OSM read. The way index is built by hand, so what is pinned here is how a
posting becomes a candidate (the dwell filter copy), how a reading along it
becomes a verdict, and that the copied constants still match the Rust loader.
"""

from __future__ import annotations

import importlib.util
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def _load_tool():
    sys.path.insert(0, str(ROOT / "tools"))
    spec = importlib.util.spec_from_file_location(
        "screen_limit_artifacts", ROOT / "tools" / "screen_limit_artifacts.py"
    )
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


tool = _load_tool()


def _rust_const(name: str) -> float:
    text = (ROOT / "crates" / "ff-core" / "src" / "data" / "world_constants.rs").read_text(
        encoding="utf-8"
    )
    match = re.search(rf"pub const {name}: f64 = ([0-9.]+);", text)
    assert match, name
    return float(match.group(1))


def test_copied_constants_match_the_rust_loader():
    for name in (
        "LIMIT_DWELL_REAL_S",
        "LIMIT_PLACE_DWELL_REAL_S",
        "LIMIT_PLACE_NEAR_MI",
        "LIMIT_PLACE_TOWN_MPH",
        "LIMIT_DWELL_REFERENCE_SCALE",
        "LIMIT_DWELL_LOW_SPEED_SCALE",
        "LIMIT_DWELL_FULL_COMPRESSION_MPH",
        "LIMIT_DWELL_FALLBACK_MPH",
    ):
        assert getattr(tool, name) == _rust_const(name), name
    assert _rust_const("LIMIT_ARTIFACT_MAX_RUN_MI") == tool.MAX_RUN_MI
    assert _rust_const("LIMIT_ARTIFACT_PLACE_GUARD_MI") == tool.PLACE_GUARD_MI


def _leg(samples, miles=50.0, landmarks=(), checkpoints=()):
    return {
        "from": "a_nv_us",
        "to": "b_nv_us",
        "miles": miles,
        "highway": "US-95",
        "corridor": {
            "speed_limits": [{"at_mi": at, "mph": mph} for at, mph in samples],
            "landmarks": list(landmarks),
            "checkpoints": list(checkpoints),
        },
    }


def test_a_played_drop_and_return_is_a_candidate():
    leg = _leg([(0.0, 65), (10.0, 45), (11.5, 65)])
    (cand,) = tool.candidates_of(leg)
    assert (cand.leg, cand.at_mi, cand.mph, cand.from_mph, cand.run_mi) == (
        "a_nv_us:b_nv_us",
        10.0,
        45.0,
        65.0,
        1.5,
    )


def test_the_dwell_filter_runs_first():
    # Four tenths of a mile of a 45 is under six real seconds: the game
    # already drops it, so there is nothing for this screen to name.
    assert tool.candidates_of(_leg([(0.0, 80), (10.0, 45), (10.4, 80)])) == []
    # A step down that never comes back, and a span past the cap, are not
    # drop-and-return postings.
    assert tool.candidates_of(_leg([(0.0, 65), (10.0, 45), (11.5, 55)])) == []
    assert tool.candidates_of(_leg([(0.0, 65), (10.0, 45), (12.6, 65)])) == []


def test_ref_matching_reads_each_route_in_a_concurrency():
    assert tool.ref_carries("US 95", "95")
    assert tool.ref_carries("I 80;US 95", "95")
    assert not tool.ref_carries("US 395", "95")
    assert not tool.ref_carries("NV 950", "95")
    assert not tool.ref_carries("", "95")


def _straight_route():
    # Due east along the 36th parallel; a mile of longitude there is ~0.0179 deg.
    deg_per_mi = 1.0 / (69.172 * 0.809)
    geom = [(36.0, -115.0 + m * deg_per_mi, float(m)) for m in range(0, 21)]
    return geom, deg_per_mi


def _index(*ways):
    index = tool.WayIndex()
    for way in ways:
        index.add(way)
    return index


def _cand(mph=45.0):
    return tool.Candidate(
        leg="a_nv_us:b_nv_us", highway="US-95", at_mi=10.0, mph=mph, from_mph=65.0, run_mi=1.0
    )


def test_verdicts_follow_the_nearest_tagged_way():
    geom, dpm = _straight_route()
    at = tool.point_interpolator(geom)
    on_route = tuple((36.0, -115.0 + m * dpm) for m in (9.0, 12.0))
    beside = tuple((36.0 + 0.0002, -115.0 + m * dpm) for m in (9.0, 12.0))  # ~22 m north

    def verdict(way):
        return tool.classify(_cand(), at, _index(way))[0]

    assert verdict(tool.Way(ref="US 95", mph=45.0, coords=on_route)) is None
    assert verdict(tool.Way(ref="US 95", mph=65.0, coords=on_route)) == "other_limit"
    assert verdict(tool.Way(ref="", mph=45.0, coords=beside)) == "off_route"
    far = tuple((36.01, -115.0 + m * dpm) for m in (9.0, 12.0))  # ~1.1 km away
    assert verdict(tool.Way(ref="US 95", mph=45.0, coords=far)) == "no_road"


def test_the_highway_carrying_part_of_the_posting_keeps_it():
    # On the highway for a third of the posting is enough to leave it alone.
    geom, dpm = _straight_route()
    at = tool.point_interpolator(geom)
    highway = tool.Way(
        ref="US 95", mph=45.0, coords=((36.0, -115.0 + 9.9 * dpm), (36.0, -115.0 + 10.4 * dpm))
    )
    reason, fractions = tool.classify(_cand(), at, _index(highway))
    assert reason is None
    assert fractions["on_b"] >= tool.ON_HIGHWAY_MIN


def test_a_town_within_the_guard_is_reported_as_kept():
    leg = _leg(
        [(0.0, 65), (10.0, 45), (11.5, 65)],
        checkpoints=[{"type": "place", "at_mi": 11.7, "name": "Beatty"}],
    )
    (cand,) = tool.candidates_of(leg)
    assert tool.place_guarded(leg, cand)
    leg["corridor"]["checkpoints"][0]["at_mi"] = 11.8
    assert not tool.place_guarded(leg, cand)


def test_check_reports_rows_the_data_no_longer_has():
    legs = [_leg([(0.0, 65), (10.0, 45), (11.5, 65)])]
    live = '{"leg": "a_nv_us:b_nv_us", "at_mi": 10.0, "mph": 45.0}'
    gone = '{"leg": "a_nv_us:b_nv_us", "at_mi": 20.0, "mph": 45.0}'
    text = '{"meta": {}}\n' + live + "\n" + gone + "\n"
    assert [r["at_mi"] for r in tool.stale_rows(text, legs)] == [20.0]


def test_the_shipped_list_matches_the_data():
    assert tool.main(["--check"]) == 0
