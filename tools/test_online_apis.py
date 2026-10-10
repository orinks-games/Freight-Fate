#!/usr/bin/env python3
"""Probe every online API endpoint configured in the game and report which are reachable.

Usage:
    uv run python tools/test_online_apis.py          # full probe (may take 60 s)
    uv run python tools/test_online_apis.py --quick  # probe only NWS + OHGO + orinks.net
"""

from __future__ import annotations

import json
import ssl
import sys
import time
import urllib.request
from pathlib import Path

TIMEOUT_S = 6.0
USER_AGENT = "FreightFate/1.1 (connectivity probe; https://orinks.net)"

ok, fail, skip = 0, 0, 0
results: list[dict] = []


def _ssl_context() -> ssl.SSLContext:
    ctx = ssl.create_default_context()
    ctx.check_hostname = True
    ctx.verify_mode = ssl.CERT_REQUIRED
    return ctx


def probe(url: str, label: str, *, expect_json: bool = True) -> None:
    global ok, fail
    try:
        req = urllib.request.Request(
            url, headers={"User-Agent": USER_AGENT, "Accept": "application/json"}
        )
        t0 = time.monotonic()
        with urllib.request.urlopen(req, timeout=TIMEOUT_S, context=_ssl_context()) as resp:
            elapsed = time.monotonic() - t0
            status = resp.status
            body = resp.read().decode("utf-8")
            if expect_json:
                json.loads(body)  # verify it's valid JSON
            results.append(
                {
                    "url": url,
                    "label": label,
                    "status": status,
                    "elapsed_s": round(elapsed, 2),
                    "ok": True,
                }
            )
            ok += 1
            print(f"  ✅ {label:<50} HTTP {status} ({elapsed:.1f}s)")
    except Exception as e:
        results.append({"url": url, "label": label, "status": str(e), "ok": False})
        fail += 1
        print(f"  ❌ {label:<50} {e}")


def probe_tcp(url: str, label: str) -> None:
    """Probe TCP connectivity only (no JSON expected)."""
    global ok, fail
    try:
        req = urllib.request.Request(url, headers={"User-Agent": USER_AGENT})
        t0 = time.monotonic()
        with urllib.request.urlopen(req, timeout=TIMEOUT_S, context=_ssl_context()) as resp:
            elapsed = time.monotonic() - t0
            results.append(
                {
                    "url": url,
                    "label": label,
                    "status": resp.status,
                    "elapsed_s": round(elapsed, 2),
                    "ok": True,
                }
            )
            ok += 1
            print(f"  ✅ {label:<50} HTTP {resp.status} ({elapsed:.1f}s)")
    except Exception as e:
        results.append({"url": url, "label": label, "status": str(e), "ok": False})
        fail += 1
        print(f"  ❌ {label:<50} {e}")


def main():
    global ok, fail
    quick = "--quick" in sys.argv

    print("=" * 72)
    print("FREIGHT FATE — Online API Connectivity Test")
    print("=" * 72)
    print()

    # ── 1. National Weather Service ────────────────────────────────────────
    print("\n🌤  NATIONAL WEATHER SERVICE (free, no API key)")
    probe("https://api.weather.gov/points/39.9612,-82.9988", "NWS /points (Columbus, OH)")
    probe("https://api.weather.gov", "NWS API root")

    # ── 2. State 511 APIs ─────────────────────────────────────────────────
    print("\n🚧  STATE 511 TRAFFIC / CONSTRUCTION APIs")
    probes_511 = [
        ("Ohio OHGO", "https://publicapi.ohgo.com/v1/construction"),
        ("Wisconsin 511WI", "https://511wi.gov/api/events"),
        ("New York 511NY", "https://511ny.org/api/events"),
        ("Georgia 511GA", "https://511ga.org/api/events"),
        ("Arizona AZ511", "https://az511.com/api/events"),
        ("Connecticut CTroads", "https://ctroads.org/api/events"),
        ("California Caltrans", "https://511.ca.gov/api/events"),
        ("Colorado COtrip", "https://cotrip.org/api/events"),
        ("Florida FL511", "https://fl511.com/api/events"),
        ("Idaho 511", "https://511.idaho.gov/api/events"),
        ("Indiana 511IN", "https://511in.org/api/events"),
        ("Maryland CHART", "https://roads.maryland.gov/api/events"),
        ("Michigan MDOT", "https://michigan.gov/mdot/api/events"),
        ("Minnesota 511MN", "https://511mn.org/api/events"),
        ("Missouri Gateway Guide", "https://gatewayguide.com/api/events"),
        ("Nevada NVRoads", "https://nvroads.com/api/events"),
        ("New Jersey 511NJ", "https://511nj.org/api/events"),
        ("North Carolina DriveNC", "https://drivenc.gov/api/events"),
        ("Oregon TripCheck", "https://tripcheck.com/WZDx_v4.json"),
        ("Pennsylvania 511PA", "https://511pa.com/api/events"),
        ("Tennessee SmartWay", "https://tnsmartway.com/api/events"),
        ("Texas DriveTexas", "https://api.drivetexas.org/v1/incidents"),
        ("Utah UDOT", "https://udottraffic.utah.gov/api/events"),
        ("Virginia 511", "https://511virginia.org/api/events"),
        ("Washington WSDOT", "https://wsdot.wa.gov/api/traffic"),
    ]

    if quick:
        probes_511 = probes_511[:2]  # just OHGO + Wisconsin

    for label, url in probes_511:
        probe(url, label)

    # ── 3. TPIMS Parking ──────────────────────────────────────────────────
    print("\n🅿  TRUCK PARKING (TPIMS)")
    probe("https://publicapi.ohgo.com/v1/truck-parking", "OHGO TPIMS (Ohio)")

    # ── 4. Orinks.net (presence + cloud saves) ────────────────────────────
    print("\n☁  ORINKS.NET (presence, cloud saves, drivers board)")
    probe_tcp("https://orinks.net", "orinks.net root")
    # Probe the API endpoints that the game uses
    probe("https://orinks.net/api/board", "orinks.net /api/board", expect_json=False)
    probe_tcp("https://orinks.net/api/players", "orinks.net /api/players")
    # Version / update check
    probe(
        "https://api.github.com/repos/orinks-games/Freight-Fate/releases/latest",
        "GitHub releases API",
    )

    # ── Summary ───────────────────────────────────────────────────────────
    print()
    print("=" * 72)
    total = ok + fail
    print(f"RESULTS:  {ok}/{total} reachable,  {fail}/{total} failed")
    if fail > 0:
        print("\nFailed endpoints:")
        for r in results:
            if not r["ok"]:
                print(f"  • {r['label']:40} {r['status']}")
    print("=" * 72)

    # Write results to JSON for programmatic inspection
    out_path = Path("online_api_test_results.json")
    with open(out_path, "w") as f:
        json.dump({"ok": ok, "fail": fail, "results": results}, f, indent=2)
    print(f"\nDetailed results written to {out_path}")


if __name__ == "__main__":
    main()
