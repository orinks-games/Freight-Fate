"""Tests for folding accepted community stations into the shipped catalog.

The fold runs unattended before every nightly, so the cases that matter are
the ones nobody is watching: a site that is down must not empty the shipped
list, and a row the game would refuse must stop the fold, not ship.
"""

from __future__ import annotations

import importlib.util
import json
import sys
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[1]


def _load():
    spec = importlib.util.spec_from_file_location(
        "fold_community_stations", ROOT / "tools" / "fold_community_stations.py"
    )
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


fcs = _load()


def _row(station_id: str, **extra) -> dict:
    row = {
        "id": station_id,
        "name": "Night Owl Radio",
        "call_sign": "",
        "format": "jazz",
        "source": "suggested by a player, reviewed by Freight Fate",
        "source_type": "web",
        "station_type": "web",
        "stream_url": "https://s.example/live",
        "stream_format": "mp3",
        "real_stream": True,
        "safe_for_streaming": False,
        "supported": True,
        "always_available": True,
        "market": "",
        "region": "",
    }
    row.update(extra)
    return row


def test_the_shipped_file_is_a_fold_of_something() -> None:
    shipped = json.loads((ROOT / "data" / "radio_community.json").read_text(encoding="utf-8"))
    assert fcs.render(fcs.fold(shipped)) == fcs.render(shipped)


def test_rows_are_sorted_and_trimmed_to_what_the_game_reads() -> None:
    folded = fcs.fold(
        {"schema": 1, "stations": [_row("community-b", note="private"), _row("community-a")]}
    )
    assert [s["id"] for s in folded["stations"]] == ["community-a", "community-b"]
    assert "note" not in folded["stations"][1]


def test_a_placed_station_keeps_its_transmitter_and_an_unplaced_one_plays_everywhere() -> None:
    placed = _row(
        "community-t",
        source_type="imported",
        station_type="imported",
        always_available=False,
        call_sign="KWSC",
        lat=42.24,
        lon=-97.01,
        range_miles=32.0,
        frequency_mhz=91.9,
    )
    half = dict(placed, id="community-h", range_miles=0)
    by_id = {s["id"]: s for s in fcs.fold({"stations": [placed, half]})["stations"]}
    assert by_id["community-t"]["source_type"] == "imported"
    assert by_id["community-t"]["lat"] == 42.24
    assert by_id["community-t"]["always_available"] is False
    assert by_id["community-h"]["source_type"] == "web"
    assert "lat" not in by_id["community-h"]
    assert by_id["community-h"]["always_available"] is True


@pytest.mark.parametrize(
    "reply",
    [
        {"error": "down"},
        {"stations": [_row("rb-1")]},
        {"stations": [_row("community-x", stream_url="file:///etc/passwd")]},
        {"stations": [_row("community-x", name=" ")]},
        {"stations": [_row("community-x"), _row("community-x")]},
    ],
)
def test_a_list_the_game_would_refuse_is_not_folded(reply: dict) -> None:
    with pytest.raises(fcs.FoldError):
        fcs.fold(reply)


def test_a_bad_reply_leaves_the_shipped_file_alone(tmp_path: Path) -> None:
    output = tmp_path / "radio_community.json"
    good = tmp_path / "good.json"
    good.write_text(json.dumps({"stations": [_row("community-a")]}), encoding="utf-8")
    assert fcs.main(["--input", str(good), "--output", str(output)]) == 0
    before = output.read_text(encoding="utf-8")

    bad = tmp_path / "bad.json"
    bad.write_text(json.dumps({"error": "down"}), encoding="utf-8")
    assert fcs.main(["--input", str(bad), "--output", str(output)]) == 2
    assert output.read_text(encoding="utf-8") == before

    assert fcs.main(["--input", str(good), "--output", str(output), "--check"]) == 0
    newer = tmp_path / "newer.json"
    newer.write_text(
        json.dumps({"stations": [_row("community-a"), _row("community-b")]}), encoding="utf-8"
    )
    assert fcs.main(["--input", str(newer), "--output", str(output), "--check"]) == 1
    assert output.read_text(encoding="utf-8") == before
