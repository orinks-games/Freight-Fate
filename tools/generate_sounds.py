"""Generate sound-effect assets via the ElevenLabs Sound Effects API.

Build-time only. Reads the ElevenLabs key from an out-of-repo file (or the
ELEVENLABS_API_KEY env var or local ignored .env), requests each effect, and
converts the returned MP3 to the project's Ogg Vorbis convention with ffmpeg.
Never run at runtime and the key is never bundled.

Some cues are procedural instead: the weigh-station warning and the two
weigh-in-motion transponder verdicts are pure numpy arithmetic (same
deterministic-numpy tradition as generate_radio.py's
generate_static/generate_fringe), so they need no API key and spend no
credits. Both are generated as part of the default "everything" run, and can
also be built alone with --weigh-station-warning / --scale-verdict.

Usage:
    uv run python tools/generate_sounds.py            # generate the default set
    uv run python tools/generate_sounds.py events/police_siren
    uv run python tools/generate_sounds.py --weigh-station-warning  # procedural, no credits
    uv run python tools/generate_sounds.py --scale-verdict  # procedural, no credits
"""

from __future__ import annotations

import json
import os
import re
import subprocess
import sys
import tempfile
import urllib.error
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
ASSETS = ROOT / "assets" / "sounds"
KEY_FILE = Path.home() / "AI API Keys.txt"
SOUND_API = "https://api.elevenlabs.io/v1/sound-generation"
# The current sound-effects model. It is also the API default, but naming it
# pins what a re-run produces when ElevenLabs moves the default again.
SOUND_MODEL = "eleven_text_to_sound_v2"
# When set, the raw MP3 the API returned is kept here too (one file per
# asset), so a later re-encode starts from the master instead of the Vorbis.
MASTERS_ENV = "FREIGHT_FATE_RADIO_MASTERS"

# key -> (prompt, duration_seconds, prompt_influence)
SPECS: dict[str, tuple[str, float, float]] = {
    "events/hazard_warning": (
        "Short urgent brake warning sound for an audio driving game, loud clear "
        "double beep alert, sharp but not harsh, no speech, no music, no whoosh, "
        "clean, cuts through truck engine noise",
        0.8,
        0.75,
    ),
    "events/police_siren": (
        "A police car siren wailing close behind, urgent up-and-down electronic "
        "yelp and wail, heard from inside a truck cab, no music, clean",
        4.0,
        0.5,
    ),
    "events/cb_radio_chatter": (
        "CB radio squelch burst then a short muffled trucker voice transmission "
        "with static, click off, no music",
        3.0,
        0.4,
    ),
    "events/spike_strip": (
        "Heavy truck tires running over a police spike strip, sharp puncture then "
        "rushing air hiss of a deflating tire, no music",
        3.0,
        0.5,
    ),
    # The turn earcons are panned by the game at playback, so the shape
    # carries the direction too: falling for left, rising for right, steady
    # for straight on -- readable even on a mono or single-ear setup.
    "events/turn_left": (
        "Classic two-tone doorbell chime, ding dong, first tone high and "
        "second tone lower, soft and gentle like a GPS navigation "
        "indication, no words, no voice, no whoosh, no music",
        0.7,
        0.75,
    ),
    # The API resists rising two-note chimes (it keeps returning falling
    # doorbells), so the shipped turn_right.ogg is derived from turn_left
    # with tools/mirror_turn_chime.py; regenerate the left cue first, then
    # re-run the mirror instead of trusting this prompt.
    "events/turn_right": (
        "Cheerful two-note chime going upward like a positive app "
        "notification, a low soft bell note then a clearly higher brighter "
        "bell note, rising interval, gentle GPS navigation indication, "
        "no words, no voice, no whoosh, no music",
        0.7,
        0.75,
    ),
    "events/turn_ahead": (
        "One single marimba note struck twice at exactly the same pitch, two "
        "identical repeated notes with no pitch change at all, soft gentle "
        "GPS navigation indication for straight ahead, no words, no voice, "
        "no whoosh, no music",
        0.7,
        0.75,
    ),
    "events/ramp_light_red": (
        "Short firm traffic light red warning cue for an audio driving game, "
        "two low descending buzzer-tinged tones meaning stop ahead, serious "
        "but not harsh, distinct from a simple beep, heard inside a truck "
        "cab, no speech, no music, no siren",
        1.0,
        0.65,
    ),
    "events/ramp_light_green": (
        "Short bright traffic light green go cue for an audio driving game, "
        "quick friendly upward two-note bell chirp meaning proceed, light "
        "and positive, heard inside a truck cab, no speech, no music, "
        "no whoosh",
        0.9,
        0.65,
    ),
    # A weigh-in-motion transponder's verdict as a truck approaches an open
    # scale: distinct from the ramp light pair above (a different situation,
    # never heard in the same approach) and readable on its own before the
    # spoken line lands. Procedural fallbacks ship first -- see
    # generate_scale_verdict_cues below -- because the ElevenLabs key was
    # dead when these were added (2026-08-20); regenerate from these SPECS
    # once a working key is available, same as every other cue here.
    "events/scale_green": (
        "Short bright electronic readout chirp meaning cleared to proceed, "
        "for an audio driving game weigh station transponder, a quick "
        "friendly upward two-note bell tone like a toll transponder success "
        "beep, light and positive, heard inside a truck cab, no speech, "
        "no music, no whoosh",
        0.7,
        0.65,
    ),
    "events/scale_red": (
        "Short firm electronic readout tone meaning pull in for inspection, "
        "for an audio driving game weigh station transponder, two low "
        "descending buzzer-tinged notes, serious but not harsh, distinct "
        "from the ramp stop-light cue, heard inside a truck cab, no speech, "
        "no music, no siren",
        0.7,
        0.65,
    ),
    "events/hazard_clear": (
        "Very short achievement-style success sound in C major, bright two-note "
        "confirmation ping using C then G, satisfying but subtle, heard in a truck "
        "cab, no whoosh, no melody, clean",
        1.0,
        0.65,
    ),
    "vehicle/air_dryer_purge": (
        "Truck air brake system air dryer purge, a single sharp pneumatic hiss "
        "and pop as the compressor cuts out, heard in the cab, no music",
        2.0,
        0.6,
    ),
    "vehicle/low_air_buzzer": (
        "Truck low air pressure warning buzzer, a steady harsh electronic alarm "
        "buzz on the dash, no music",
        2.5,
        0.6,
    ),
    "traffic/car_pass": (
        "A passenger car passing a truck cab on a highway, brief tire and wind "
        "whoosh from outside, no horn, no voice, no music",
        1.8,
        0.45,
    ),
    "traffic/box_truck_pass": (
        "A medium box truck passing a semi truck cab on a highway, deeper tire "
        "noise and short diesel whoosh, no horn, no voice, no music",
        2.2,
        0.45,
    ),
    "traffic/semi_pass": (
        "A large semi truck passing close by another truck cab on an interstate, "
        "heavy diesel rumble, tire roar, air wash, no horn, no voice, no music",
        2.8,
        0.5,
    ),
    "traffic/pickup_pass": (
        "A pickup truck passing a semi truck cab on a highway, throaty gas "
        "engine and tire whoosh, lighter than a box truck, heavier than a "
        "car, no horn, no voice, no music",
        2.0,
        0.45,
    ),
    "traffic/motorcycle_pass": (
        "A motorcycle passing a truck cab on a highway, high revving engine "
        "whine approaching fast then fading, quick and sharp, no horn, no "
        "voice, no music",
        1.8,
        0.5,
    ),
    "traffic/bus_pass": (
        "A large highway bus passing a truck cab, long deep diesel drone and "
        "heavy tire roar, slower and longer than a car pass, no horn, no "
        "voice, no music",
        2.8,
        0.45,
    ),
    "traffic/tractor_pass": (
        "A slow farm tractor on the road ahead as a truck passes it, "
        "chugging diesel putter with a steady mechanical clatter, slow and "
        "heavy, no horn, no voice, no music",
        2.5,
        0.5,
    ),
    # Crossing variants for the intersection cross bubble: a vehicle driving
    # THROUGH in front of a stopped truck, approach then doppler drop then
    # recede. No direction words in the prompts -- the engine pans the cue to
    # the side the simulated vehicle is actually on, so the source must not
    # bake a direction in.
    "traffic/car_cross": (
        "A passenger car driving through an intersection directly in front "
        "of a stopped truck, approaching engine and tire noise, doppler "
        "pitch drop as it crosses close by, then receding, no horn, no "
        "voice, no music",
        2.2,
        0.5,
    ),
    "traffic/pickup_cross": (
        "A pickup truck driving through an intersection directly in front "
        "of a stopped truck, throaty gas engine approaching, doppler pitch "
        "drop as it crosses close by, then receding, no horn, no voice, no "
        "music",
        2.2,
        0.5,
    ),
    "traffic/box_truck_cross": (
        "A medium box truck driving through an intersection in front of a "
        "stopped semi, diesel engine and tire noise approaching, doppler "
        "pitch drop as it crosses, then receding, no horn, no voice, no "
        "music",
        2.5,
        0.5,
    ),
    "traffic/semi_cross": (
        "A loaded semi truck rolling through an intersection in front of a "
        "stopped truck, heavy diesel rumble and tire roar approaching, "
        "doppler drop as the long trailer passes close by, slow to clear, "
        "then receding, no horn, no voice, no music",
        3.2,
        0.5,
    ),
    "traffic/motorcycle_cross": (
        "A motorcycle darting through an intersection in front of a stopped "
        "truck, high engine whine approaching fast, sharp doppler drop as "
        "it flashes past, gone quickly, no horn, no voice, no music",
        1.6,
        0.55,
    ),
    "traffic/bus_cross": (
        "A large bus driving through an intersection in front of a stopped "
        "truck, long deep diesel drone approaching, doppler drop as the "
        "long body passes close by, slow to clear, then receding, no horn, "
        "no voice, no music",
        3.0,
        0.5,
    ),
    "traffic/tractor_cross": (
        "A slow farm tractor chugging through an intersection in front of a "
        "stopped truck, steady diesel putter and mechanical clatter "
        "crossing very slowly, then receding, no horn, no voice, no music",
        3.5,
        0.55,
    ),
    "traffic/trooper_pass": (
        "A state trooper patrol car cruising past a truck cab on the highway, "
        "clean car tire whoosh with a subtle police radio chirp, no siren, no "
        "voice, no music",
        2.0,
        0.45,
    ),
    "vehicle/road": (
        "Continuous steady tyre roar and road surface noise from inside a "
        "moving semi truck cab at highway speed, smooth broadband rumble, "
        "even and unchanging with no events, no engine, no wind gusts, no "
        "music, no speech, seamless loop",
        22.0,
        0.4,
    ),
    "vehicle/lane_centered": (
        "Very short calm centered-lane confirmation sound for an audio driving "
        "game, soft two-note dashboard chime, clear and positive but subtle, "
        "heard inside a truck cab, no speech, no music, no whoosh",
        0.9,
        0.75,
    ),
    "vehicle/lane_drift": (
        "Very short lane drift warning beep for an audio driving game, single "
        "clean dashboard beep, clear direction cue when panned left or right, "
        "subtle but easy to hear over truck engine noise, no speech, no music",
        0.5,
        0.75,
    ),
    "vehicle/turn_signal": (
        "Truck turn signal indicator clicking inside a cab, steady dry relay "
        "click-clack pattern, four clicks, close and mechanical, no music, "
        "no voice",
        1.6,
        0.7,
    ),
    "vehicle/tire_screech": (
        "Heavy truck tires screeching hard on asphalt during emergency "
        "braking, short aggressive skid, rubber on pavement, no crash impact, "
        "no music, no voice",
        1.6,
        0.6,
    ),
    "vehicle/brake_squeal": (
        "Overheated semi truck brakes squealing under heavy braking on a long "
        "downgrade, metallic high-pitched squeal with an air brake undertone, "
        "heard from the cab, no music, no voice",
        2.2,
        0.6,
    ),
    "ambient/truck_stop": (
        "Daytime truck stop parking lot ambience, several diesel engines "
        "idling at different distances, an occasional air brake hiss, one "
        "truck passing on the nearby interstate, light wind, a distant door "
        "slam, no voices, no music, steady bed suitable for seamless looping",
        12.0,
        0.4,
    ),
    "ambient/warehouse": (
        "Inside a large busy freight warehouse, big reverberant space, a "
        "forklift beeping and driving past, pallets set down, a distant dock "
        "door rattle, low ventilation hum, no voices, no music, steady bed "
        "suitable for seamless looping",
        12.0,
        0.4,
    ),
    # The main menu's beds, picked by the career's clock and business
    # (ff_core::music::select_menu_ambience). The shipped takes were made in
    # the ElevenLabs flows canvas from these prompts on 2026-10-12, then
    # crossfaded at the seam and levelled to about -31 dB RMS.
    "ambient/menu_night_lot": (
        "Truck stop parking lot at night ambience, several semi trucks idling "
        "at different distances, a refrigerated trailer unit humming, distant "
        "interstate traffic, faint crickets, an occasional soft air brake hiss "
        "far away, calm and quiet, no voices, no music, steady bed suitable "
        "for seamless looping",
        20.0,
        0.4,
    ),
    "ambient/menu_yard": (
        "A single diesel semi truck idling steadily in a quiet truck yard at "
        "night, heard from outside a few meters away, low rumble, light wind, "
        "distant highway hum, no voices, no music, steady bed suitable for "
        "seamless looping",
        20.0,
        0.4,
    ),
    "ambient/menu_dispatch_office": (
        "Inside a small trucking dispatch office, quiet ventilation hum, "
        "occasional keyboard typing, a muffled semi truck idling outside the "
        "window, soft distant CB radio static, no clear voices, no music, "
        "steady bed suitable for seamless looping",
        20.0,
        0.4,
    ),
    # The old toll cue was a bright chime around 4 to 5 kHz that clipped;
    # the owner picked this take (2026-10-17) from eight lower ones. The
    # shipped take was levelled to a -3 dBFS peak.
    "events/toll_charged": (
        "Gentle low dashboard bong, single soft rounded tone like a car "
        "door-ajar chime but much lower and softer, no high frequencies, "
        "no speech, no music",
        0.8,
        0.7,
    ),
}

# Ambience beds loop at runtime; ask the API for a seamless loop when it can.
LOOP_KEYS = {
    "ambient/truck_stop",
    "ambient/warehouse",
    "ambient/menu_night_lot",
    "ambient/menu_yard",
    "ambient/menu_dispatch_office",
    "vehicle/road",
}


def _load_dotenv() -> None:
    env_path = ROOT / ".env"
    if not env_path.exists():
        return
    for raw in env_path.read_text(encoding="utf-8", errors="ignore").splitlines():
        line = raw.strip()
        if not line or line.startswith("#") or "=" not in line:
            continue
        name, value = line.split("=", 1)
        os.environ.setdefault(name.strip(), value.strip().strip('"').strip("'"))


def _api_key() -> str:
    _load_dotenv()
    env = os.environ.get("ELEVENLABS_API_KEY")
    if env:
        return env.strip()
    text = KEY_FILE.read_text(encoding="utf-8", errors="ignore")
    m = re.search(r"Eleven Labs:\s*\n\s*\n?\s*(sk_[A-Za-z0-9]+)", text)
    if not m:
        raise SystemExit(f"No ElevenLabs key found in {KEY_FILE}")
    return m.group(1)


def _request_mp3(key: str, payload: dict) -> bytes:
    req = urllib.request.Request(
        SOUND_API,
        data=json.dumps(payload).encode("utf-8"),
        headers={"xi-api-key": key, "Content-Type": "application/json", "Accept": "audio/mpeg"},
    )
    with urllib.request.urlopen(req, timeout=120) as resp:
        # The API reports what it charged in a response header; a scoped key
        # cannot read the account meter, so this is the only cost signal.
        cost = resp.headers.get("character-cost")
        if cost:
            print(f"    cost {cost} credits", flush=True)
        return resp.read()


def stash_master(mp3: bytes, stem: str) -> None:
    """Keep the API's MP3 under ``$FREIGHT_FATE_RADIO_MASTERS/<stem>.mp3``.

    No-op when the variable is unset. Re-encoding later (Opus for the music
    pack, a different Vorbis quality) then costs no credits and adds no
    second lossy generation.
    """
    masters = os.environ.get(MASTERS_ENV)
    if not masters:
        return
    target = Path(masters) / f"{stem}.mp3"
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_bytes(mp3)


def _generate(key: str, spec_key: str, prompt: str, duration: float, influence: float) -> None:
    payload = {
        "text": prompt,
        "duration_seconds": duration,
        "prompt_influence": influence,
        "model_id": SOUND_MODEL,
        "output_format": "mp3_44100_128",
    }
    if spec_key in LOOP_KEYS:
        payload["loop"] = True
    print(f"  requesting {spec_key} ({duration:.0f}s)...", flush=True)
    try:
        mp3 = _request_mp3(key, payload)
    except urllib.error.HTTPError:
        if "loop" not in payload:
            raise
        # Older API plans reject the loop flag; the prompt still asks for a
        # steady bed, so fall back to a plain generation.
        payload.pop("loop")
        print("    loop flag rejected; retrying without it...", flush=True)
        mp3 = _request_mp3(key, payload)
    stash_master(mp3, spec_key.replace("/", "_"))
    out = ASSETS / f"{spec_key}.ogg"
    out.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.NamedTemporaryFile(suffix=".mp3", delete=False) as tmp:
        tmp.write(mp3)
        tmp_path = tmp.name
    try:
        subprocess.run(
            [
                "ffmpeg",
                "-y",
                "-loglevel",
                "error",
                "-i",
                tmp_path,
                "-c:a",
                "libvorbis",
                "-q:a",
                "5",
                str(out),
            ],
            check=True,
        )
    finally:
        os.unlink(tmp_path)
    print(f"    wrote {out} ({out.stat().st_size:,} bytes)", flush=True)


# -- procedural: the weigh-station warning ------------------------------------
#
# Owner ruling 2026-08-14: the scale gets its own earcon rather than reusing
# events/inspection_warning, and it is synthesized, not requested from
# ElevenLabs -- pure arithmetic, no random source, deterministic like
# generate_radio.py's generate_static/generate_fringe and the enforcement
# signature synthesized at runtime in states/driving_siren.py. Unlike that
# signature this ships as a real asset under events/, packed like every other
# event cue, because it plays from a build-time trigger (the scale-approach
# announcement), not something the audio engine can synthesize cheaply every
# frame.

WEIGH_STATION_WARNING_KEY = "events/weigh_station_warning"


def _synth_edge_fade(samples, rate: int, *, attack_s: float = 0.004, release_s: float = 0.02):
    """Short attack/release taper -- same convention as generate_radio.py's
    fringe picket splashes and _edge_fade, kept local here so this file does
    not have to import from generate_radio (which itself imports from this
    one)."""
    import numpy as np

    n = samples.size
    if n < 2:
        return samples
    attack = min(int(attack_s * rate), n // 2)
    release = min(int(release_s * rate), n - attack)
    env = np.ones(n, dtype=np.float64)
    if attack > 0:
        env[:attack] = np.linspace(0.0, 1.0, attack)
    if release > 0:
        env[-release:] = np.linspace(1.0, 0.0, release)
    return samples * env


def _write_synth_asset(sample, rate: int, relpath: str) -> None:
    """Write a numpy sample array into the loose asset tree as Ogg Vorbis.

    Same temp-WAV-then-ffmpeg convention as the ElevenLabs path above
    (``_generate``), so every asset under assets/sounds/ is the same
    container however it was made.
    """
    import soundfile as sf

    out = ASSETS / relpath
    out.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.NamedTemporaryFile(suffix=".wav", delete=False) as tmp:
        tmp_path = tmp.name
    try:
        sf.write(tmp_path, sample.astype("float32"), rate, format="WAV")
        subprocess.run(
            ["ffmpeg", "-y", "-loglevel", "error", "-i", tmp_path, "-c:a", "libvorbis", str(out)],
            check=True,
        )
    finally:
        os.unlink(tmp_path)
    print(f"    wrote {out} ({out.stat().st_size:,} bytes)", flush=True)


def generate_weigh_station_warning() -> None:
    """Procedural two-part "the scale" cue; no API credits, deterministic.

    Part one is a low weighted thump -- an 85 Hz fundamental with a second
    layer an octave under it -- reading as a heavy platform underfoot. It is
    the only event earcon carrying real low end: the enforcement marker
    (states/driving_siren.SIGNATURE_LOW_HZ, 660-1056 Hz) and ui/notify are
    both dry and high, so the weight alone keeps this unmistakable against
    either. Part two is a short bright two-note beep on its heel, like a
    scale readout locking a number in -- the half that actually reads as
    "the scale" rather than just "a warning". About 0.9 seconds end to end.

    Pure arithmetic, no random source: a rebuild always reproduces the same
    PCM audio (identical to sample precision, like every other numpy-synthed
    cue here). The committed .ogg itself can still change a few bytes run to
    run -- libvorbis stamps a fresh random stream serial into the Ogg
    container on every encode, the one part of this pipeline arithmetic does
    not control -- so the file that ships is whichever encode was committed,
    not something a rebuild should be expected to match byte for byte.
    """
    import numpy as np

    rate = 44100

    # Part 1: the low thump. Two sines (not one) so the sub layer supplies
    # weight a single tone at this length would not read as "heavy".
    thump_s = 0.46
    t1 = np.arange(int(thump_s * rate)) / rate
    thump = 0.7 * np.sin(2.0 * np.pi * 85.0 * t1) + 0.5 * np.sin(2.0 * np.pi * 42.5 * t1)
    thump *= np.exp(-t1 / 0.11)  # fast decay: a struck weight, not a held tone
    thump = _synth_edge_fade(thump, rate, attack_s=0.006, release_s=0.03)

    gap1 = np.zeros(int(0.09 * rate))

    # Part 2: the readout beep, two notes stepping up.
    beep_s = 0.11
    t2 = np.arange(int(beep_s * rate)) / rate
    beep_a = np.sin(2.0 * np.pi * 1180.0 * t2) * np.exp(-t2 / 0.09)
    beep_b = np.sin(2.0 * np.pi * 1560.0 * t2) * np.exp(-t2 / 0.09)
    beep_a = _synth_edge_fade(beep_a, rate, attack_s=0.004, release_s=0.02)
    beep_b = _synth_edge_fade(beep_b, rate, attack_s=0.004, release_s=0.02)
    gap2 = np.zeros(int(0.04 * rate))

    tail = np.zeros(int(0.08 * rate))

    sample = np.concatenate([thump, gap1, beep_a, gap2, beep_b, tail])
    sample = 0.85 * sample / np.max(np.abs(sample))
    _write_synth_asset(sample, rate, f"{WEIGH_STATION_WARNING_KEY}.ogg")


# The weigh-in-motion transponder verdict, procedural like the weigh-station
# warning above -- same deterministic-numpy tradition (no random source, a
# rebuild reproduces the same PCM), and only a fallback: SPECS above still
# carries the ElevenLabs prompts for whenever a working key is available.
SCALE_GREEN_KEY = "events/scale_green"
SCALE_RED_KEY = "events/scale_red"


def _synth_scale_verdict(rate: int, *, rising: bool):
    """One deterministic two-note transponder readout tone.

    Same "readout beep" family as this file's weigh-station-warning part
    two -- a short bright tone stepping between two notes -- so the pair
    reads as kin to the scale cue that precedes it, while staying clear of
    the ramp traffic light pair (events/ramp_light_green,
    events/ramp_light_red): different frequencies, and never heard in the
    same approach as those, since a scale's own ramp has no public
    crossroad or signal (see driving_events._ramp_control_for). Rising for
    the green verdict, falling for red -- the same up/down convention the
    ramp light pair already teaches, so the direction is learnable even
    without ever having heard this specific pair before.
    """
    import numpy as np

    note_s = 0.11
    gap = np.zeros(int(0.035 * rate))
    low_hz, high_hz = 1320.0, 1760.0
    first_hz, second_hz = (low_hz, high_hz) if rising else (high_hz, low_hz)

    def _note(hz: float):
        t = np.arange(int(note_s * rate)) / rate
        tone = np.sin(2.0 * np.pi * hz * t) * np.exp(-t / 0.09)
        return _synth_edge_fade(tone, rate, attack_s=0.004, release_s=0.02)

    sample = np.concatenate([_note(first_hz), gap, _note(second_hz)])
    peak = float(np.max(np.abs(sample))) or 1.0
    return 0.85 * sample / peak


def generate_scale_verdict_cues() -> None:
    """Write the two procedural transponder-verdict cues.

    Pure arithmetic, no API credits -- part of the default "everything" run
    alongside generate_weigh_station_warning, and buildable alone the same
    way with --scale-verdict.
    """
    rate = 44100
    _write_synth_asset(_synth_scale_verdict(rate, rising=True), rate, f"{SCALE_GREEN_KEY}.ogg")
    _write_synth_asset(_synth_scale_verdict(rate, rising=False), rate, f"{SCALE_RED_KEY}.ogg")


# --- Procedural traffic pass and crossing synths (no API, deterministic) ---
#
# The ElevenLabs key went dead mid-expansion (401, 2026-08-20), so these
# eleven cues shipped numpy-synthesized under the SAME keys the API specs
# above describe. The API versions replaced them on 2026-09-11 (the key came
# back with sound-generation scope); the synths stay as the no-key fallback
# (--synth-traffic), and a rebuild reproduces the same PCM, same
# deterministic-numpy tradition as the weigh-station warning. The default
# run and a named key both take the API path, never these.
#
# Anatomy of a vehicle event: an engine tone (sum of harmonics on a phase
# integral, so the doppler bend is continuous), tire/wind noise (bright and
# dark filtered-noise beds crossfaded as the vehicle approaches and recedes),
# and a proximity envelope. A CROSSING is symmetric with a hard doppler flip
# at closest approach -- the vehicle drives through in front of a stopped
# truck. A PASS is gentler: overtaking on the highway, slow swell, small
# relative-speed pitch change. Mono by design: the runtime pans the cue to
# the side the simulated vehicle is actually on.

SYNTH_TRAFFIC_KEYS = (
    "traffic/pickup_pass",
    "traffic/motorcycle_pass",
    "traffic/bus_pass",
    "traffic/tractor_pass",
    "traffic/car_cross",
    "traffic/pickup_cross",
    "traffic/box_truck_cross",
    "traffic/semi_cross",
    "traffic/motorcycle_cross",
    "traffic/bus_cross",
    "traffic/tractor_cross",
)

# key -> (duration_s, engine_hz, harmonic weights, noise mix, engine mix,
#         doppler ratio, fire_rate_hz or 0 for smooth engines)
# Fundamentals sit roughly where each engine idles-to-cruises; the tractor
# and semi get a firing-rate amplitude chug that reads as "big slow diesel".
_TRAFFIC_SYNTH_SPECS: dict[
    str, tuple[float, float, tuple[float, ...], float, float, float, float]
] = {
    "traffic/pickup_pass": (2.0, 95.0, (1.0, 0.55, 0.3, 0.12), 0.75, 0.45, 0.06, 0.0),
    "traffic/motorcycle_pass": (1.8, 190.0, (1.0, 0.7, 0.45, 0.3, 0.18), 0.35, 0.8, 0.09, 0.0),
    "traffic/bus_pass": (2.8, 68.0, (1.0, 0.5, 0.22, 0.1), 0.7, 0.55, 0.05, 0.0),
    "traffic/tractor_pass": (2.5, 52.0, (1.0, 0.6, 0.3), 0.4, 0.85, 0.03, 9.0),
    "traffic/car_cross": (2.2, 110.0, (1.0, 0.4, 0.18), 0.85, 0.35, 0.16, 0.0),
    "traffic/pickup_cross": (2.2, 95.0, (1.0, 0.55, 0.3, 0.12), 0.7, 0.5, 0.16, 0.0),
    "traffic/box_truck_cross": (2.5, 82.0, (1.0, 0.6, 0.3, 0.14), 0.65, 0.6, 0.14, 0.0),
    "traffic/semi_cross": (3.2, 62.0, (1.0, 0.65, 0.35, 0.16), 0.7, 0.7, 0.12, 11.0),
    "traffic/motorcycle_cross": (1.6, 200.0, (1.0, 0.7, 0.45, 0.3, 0.18), 0.3, 0.85, 0.2, 0.0),
    "traffic/bus_cross": (3.0, 68.0, (1.0, 0.5, 0.22, 0.1), 0.6, 0.65, 0.12, 0.0),
    "traffic/tractor_cross": (3.5, 50.0, (1.0, 0.6, 0.3), 0.35, 0.9, 0.05, 8.0),
}


def _filtered_noise(rng, n: int, rate: int, cutoff_hz: float, order: float = 2.0):
    """White noise shaped by an FFT-domain lowpass rolloff."""
    import numpy as np

    noise = rng.standard_normal(n)
    spectrum = np.fft.rfft(noise)
    freqs = np.fft.rfftfreq(n, 1.0 / rate)
    spectrum *= 1.0 / (1.0 + (freqs / cutoff_hz) ** (2.0 * order))
    shaped = np.fft.irfft(spectrum, n)
    peak = float(np.max(np.abs(shaped))) or 1.0
    return shaped / peak


def synth_traffic_event(key: str, rate: int = 44100):
    """One deterministic pass or crossing cue as a mono sample array."""
    import numpy as np

    duration, engine_hz, harmonics, noise_mix, engine_mix, dopp_ratio, fire_hz = (
        _TRAFFIC_SYNTH_SPECS[key]
    )
    is_cross = key.endswith("_cross")
    n = int(duration * rate)
    t = np.arange(n) / rate
    t_c = duration * (0.5 if is_cross else 0.62)  # closest approach

    # Doppler: smooth high-to-low sweep through closest approach. A crossing
    # flips fast (the vehicle really goes by the nose); a pass drifts.
    sharpness = 6.0 if is_cross else 2.2
    dopp = 1.0 + dopp_ratio * np.tanh((t_c - t) * sharpness)

    # Engine: harmonics on one integrated phase so the bend never clicks.
    phase = 2.0 * np.pi * np.cumsum(engine_hz * dopp) / rate
    engine = np.zeros(n)
    for i, weight in enumerate(harmonics, start=1):
        engine += weight * np.sin(i * phase + 0.7 * i)
    engine /= float(np.max(np.abs(engine))) or 1.0
    if fire_hz:
        # Big slow diesels chug: amplitude dips at the firing rate, which is
        # most of what makes a tractor sound like a tractor.
        chug = 0.62 + 0.38 * np.clip(np.sin(2.0 * np.pi * fire_hz * t), -0.6, 1.0)
        engine *= chug

    # Tire and wind: a dark bed always on, a bright bed that opens as the
    # vehicle closes and shuts as it recedes (crossfaded by the same doppler
    # curve, so pitch and brightness move together).
    import zlib

    rng = np.random.default_rng(zlib.crc32(key.encode("utf-8")))
    dark = _filtered_noise(rng, n, rate, 650.0)
    bright = _filtered_noise(rng, n, rate, 2600.0)
    closeness = (dopp - dopp.min()) / ((dopp.max() - dopp.min()) or 1.0)
    noise = dark * (0.55 + 0.45 * (1.0 - closeness)) + bright * (0.25 + 0.75 * closeness)

    # Proximity envelope: swell in, peak at closest approach, fall away.
    width = duration * (0.22 if is_cross else 0.38)
    proximity = 1.0 / (1.0 + ((t - t_c) / width) ** 2)
    proximity = proximity**1.2

    sample = (engine_mix * engine + noise_mix * noise) * proximity
    sample = _synth_edge_fade(sample, rate, attack_s=0.02, release_s=0.08)
    return 0.8 * sample / (float(np.max(np.abs(sample))) or 1.0)


def generate_synth_traffic(keys=None) -> None:
    """Write the procedural traffic cues into the loose asset tree."""
    for key in keys or SYNTH_TRAFFIC_KEYS:
        if key not in _TRAFFIC_SYNTH_SPECS:
            raise SystemExit(f"No traffic synth spec for {key!r}")
        print(f"  synthesizing {key}...", flush=True)
        _write_synth_asset(synth_traffic_event(key), 44100, f"{key}.ogg")


def main(argv: list[str] | None = None) -> int:
    argv = list(sys.argv[1:] if argv is None else argv)
    do_all = not argv
    want_procedural = do_all or "--weigh-station-warning" in argv
    want_scale_verdict = do_all or "--scale-verdict" in argv
    if "--weigh-station-warning" in argv:
        argv.remove("--weigh-station-warning")
    if "--scale-verdict" in argv:
        argv.remove("--scale-verdict")
    if want_procedural:
        generate_weigh_station_warning()
    if want_scale_verdict:
        generate_scale_verdict_cues()
    if "--synth-traffic" in argv:
        argv.remove("--synth-traffic")
        generate_synth_traffic(argv or None)
        return 0
    if not argv and not do_all:
        return 0  # only the procedural flag was given; no API key needed
    wanted = argv or list(SPECS)
    key = _api_key()
    for spec_key in wanted:
        if spec_key not in SPECS:
            raise SystemExit(f"Unknown sound key {spec_key!r}; known: {', '.join(SPECS)}")
        prompt, duration, influence = SPECS[spec_key]
        _generate(key, spec_key, prompt, duration, influence)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
