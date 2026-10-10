"""tools/loudness.py: the meter the music normalizer and sound audit trust."""

import loudness
import numpy as np
import pytest


def _sine(amplitude: float, seconds: float = 5.0, hz: float = 997.0) -> np.ndarray:
    t = np.arange(int(seconds * loudness.RATE)) / loudness.RATE
    tone = amplitude * np.sin(2 * np.pi * hz * t)
    return np.vstack([tone, tone])


def test_stereo_sine_reads_its_level_in_lufs():
    # BS.1770: a 997 Hz sine in both channels reads its dBFS level as LUFS.
    assert loudness.integrated(_sine(0.1)) == pytest.approx(-20.0, abs=0.1)
    assert loudness.true_peak(_sine(0.1)) == pytest.approx(-20.0, abs=0.1)
    assert loudness.loudness_range(_sine(0.1)) == pytest.approx(0.0, abs=0.1)


def test_silence_is_minus_infinity_not_a_crash():
    silent = np.zeros((2, loudness.RATE))
    assert loudness.integrated(silent) == float("-inf")


def test_a_click_shorter_than_one_block_is_still_measured():
    click = _sine(0.5, seconds=0.05)
    assert -40.0 < loudness.max_momentary(click) < loudness.max_momentary(_sine(0.5))


def test_gain_stops_at_the_peak_ceiling():
    assert loudness.normalizing_gain(-14.0, -3.0, -18.0, -1.0) == (-4.0, False)
    gain, limited = loudness.normalizing_gain(-26.0, -4.0, -18.0, -1.0)
    assert (gain, limited) == (3.0, True)


def test_ride_gain_brings_quiet_talk_up_to_a_loud_sting():
    # Twenty seconds 8 dB under the target, then ten seconds 4 dB over it:
    # static gain would leave the long quiet part under; the ride evens them.
    quiet, loud = _sine(0.1 * 10 ** (-6 / 20), 20.0), _sine(0.1 * 10 ** (6 / 20), 10.0)
    samples = np.concatenate([quiet, loud], axis=1)
    gain = loudness.ride_gain(samples, -20.0)
    rode = samples * 10.0 ** (gain / 20.0)
    assert loudness.loudness_range(rode) < loudness.loudness_range(samples) / 2
    middle_of_quiet = slice(8 * loudness.RATE, 12 * loudness.RATE)
    assert loudness.integrated(rode[:, middle_of_quiet]) == pytest.approx(-20.0, abs=1.0)


def test_ride_gain_holds_through_silence_instead_of_raising_hiss():
    tone, gap = _sine(0.1, 10.0), np.zeros((2, 10 * loudness.RATE))
    gain = loudness.ride_gain(np.concatenate([tone, gap, tone], axis=1), -20.0, max_boost_db=8.0)
    # The windows straddling the edge read a little quiet and nudge the gain;
    # ten seconds of nothing must not wind it up to the full boost.
    in_gap = gain[13 * loudness.RATE : 17 * loudness.RATE]
    assert np.ptp(in_gap) < 1e-9
    assert gain.max() < 5.0


def test_ride_gain_is_bounded():
    gain = loudness.ride_gain(_sine(0.001, 20.0), -18.0, max_boost_db=8.0)
    assert gain.max() <= 8.0 + 1e-9


def test_limiter_holds_the_true_peak_under_the_ceiling():
    rng = np.random.default_rng(0)
    noise = rng.normal(0.0, 0.3, (2, 2 * loudness.RATE))
    limited = loudness.limit_true_peak(noise, -6.0)
    assert loudness.true_peak(limited) <= -5.9
    # Quiet material passes untouched.
    quiet = _sine(0.1)
    assert np.allclose(loudness.limit_true_peak(quiet, -1.0), quiet)
