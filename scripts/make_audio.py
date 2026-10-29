#!/usr/bin/env python3
# MIT License - Copyright (c) fintonlabs.com
"""Make every sound the Godot client plays: ambience loops, effects and music, synthesised from nothing (no samples,
nothing to license). Deterministic: the same script makes the same audio.

    scripts/make_audio.py [OUT_DIR] [--force] [NAME ...]     (default godot/sounds; needs numpy, scipy and ffmpeg)

Files that already exist are skipped unless --force is given: the Ogg encoder stamps every file with a random stream
serial, so rewriting an unchanged sound still changes its bytes. NAMEs keep only the files whose path contains one
of them, e.g. `scripts/make_audio.py cast_ impact_ sfx/heal sfx/held` or `--force sfx/cast_fire`. OUT_DIR must exist.
Spell sounds each have their own seed, so they come out the same whatever else is made alongside them.

ambience/  seamless loops the client blends by what is round the player, or a place's `sound`. Every one is now a
           real recording (CC0 or CC-BY, see godot/sounds/ambience/CREDITS.md), not made here: sea, shore, river,
           forest, night, wind, rain, fire, town, market, crypt, holy and battle. They are in RECORDED and are never
           written, not even with --force; the synthesised versions below are kept only to keep the shared random
           stream (and so every other sound) the same.
sfx/       short one-shots (several takes of the common ones: name_1, name_2, ...). Only the spell sounds (cast_*,
           impact_*, heal, held) are still made here. The rest are real recordings (CC0 or CC-BY, see
           godot/sounds/sfx/CREDITS.md), in RECORDED and never written, not even with --force: step_grass_1-6,
           step_road_1-4, step_stone_1-6, step_water_1-6, swing_1-3, hit_1-4, hurt, pickup, drop, place, break,
           horn, chime, splash and click (and the amb_* incidentals, which this script never made).
music/     seamless pieces in the old Irish modes, one per mood, chosen by place `music` or the moment
"""
import os
import subprocess
import sys
import tempfile

import numpy as np
from scipy import signal

ARGS = [a for a in sys.argv[1:] if a != '--force']
FORCE = len(ARGS) < len(sys.argv) - 1
OUT = ARGS.pop(0) if ARGS and os.path.isdir(ARGS[0]) else os.path.join(os.path.dirname(__file__), '..', 'godot', 'sounds')
ONLY = ARGS  # path fragments: make only the files whose path contains one
# Real recordings (godot/sounds/ambience/CREDITS.md): never written, even with --force.
RECORDED = {f'ambience/{n}.ogg' for n in ('sea', 'shore', 'river', 'forest', 'night', 'wind', 'rain', 'fire', 'town',
                                          'market', 'crypt', 'holy', 'battle', 'tavern', 'fair')}
RECORDED |= {f'sfx/{n}.ogg' for n in ('hurt', 'pickup', 'drop', 'place', 'break', 'horn', 'chime', 'splash', 'click', 'bell')}
RECORDED |= {f'sfx/{n}_{i}.ogg' for n, k in (('step_grass', 6), ('step_road', 4), ('step_stone', 6), ('step_water', 6),
                                             ('swing', 3), ('hit', 4)) for i in range(1, k + 1)}
SR = 32000  # ambience and effects
MR = 44100  # music

rng = np.random.default_rng(1066)


# ------------------------------------------------------------------------------------------------ building blocks

def white(n):
    return rng.standard_normal(n)


def pink(n):
    # Voss-ish: filtered white noise with a -3 dB/octave slope.
    b = [0.049922035, -0.095993537, 0.050612699, -0.004408786]
    a = [1, -2.494956002, 2.017265875, -0.522189400]
    return signal.lfilter(b, a, white(n)) * 8


def brown(n):
    x = np.cumsum(white(n))
    x = signal.lfilter([1, -1], [1, -0.999], x)  # take the drift out
    return x / (np.max(np.abs(x)) + 1e-9)


def band(x, lo, hi, sr=SR, order=2):
    nyq = sr / 2
    if lo <= 0:
        sos = signal.butter(order, min(hi / nyq, 0.99), 'low', output='sos')
    elif hi >= nyq:
        sos = signal.butter(order, lo / nyq, 'high', output='sos')
    else:
        sos = signal.butter(order, [lo / nyq, min(hi / nyq, 0.99)], 'band', output='sos')
    return signal.sosfilt(sos, x)


def wander(n, points_per_sec, sr=SR, lo=0.0, hi=1.0, seed_shift=0):
    """A slow random curve between lo and hi: random points, smoothly joined."""
    k = max(4, int(n / sr * points_per_sec) + 3)
    pts = rng.uniform(lo, hi, k)
    xs = np.linspace(0, n, k)
    t = np.arange(n)
    i = np.clip(np.searchsorted(xs, t) - 1, 0, k - 2)
    f = (t - xs[i]) / (xs[i + 1] - xs[i])
    f = f * f * (3 - 2 * f)
    return pts[i] + (pts[i + 1] - pts[i]) * f


def env(n, a, d, sr=SR):
    """Attack then exponential decay, n samples long."""
    t = np.arange(n) / sr
    e = np.minimum(t / max(a, 1e-4), 1.0) * np.exp(-np.maximum(t - a, 0) / max(d, 1e-4))
    return e


def reverb(x, sr, secs=2.2, wet=0.3, damp=4000, pre=0.012):
    n = int(secs * sr)
    ir = white(n) * np.exp(-np.arange(n) / sr * (6.9 / secs))
    ir = band(ir, 0, damp, sr)
    ir = np.concatenate([np.zeros(int(pre * sr)), ir])
    ir /= np.sqrt(np.sum(ir ** 2)) + 1e-9
    if x.ndim == 1:
        w = signal.fftconvolve(x, ir)[: len(x)]
    else:
        # A slightly different tail in each ear, so the room is wide.
        ir2 = np.roll(ir, int(0.007 * sr))
        w = np.stack([signal.fftconvolve(x[:, 0], ir)[: len(x)], signal.fftconvolve(x[:, 1], ir2)[: len(x)]], 1)
    return x * (1 - wet) + w * wet


def place(buf, snd, at, gain=1.0):
    """Mix snd into buf at sample `at`, wrapping round the end (so loops stay seamless)."""
    n = len(buf)
    at %= n
    end = at + len(snd)
    if end <= n:
        buf[at:end] += snd * gain
    else:
        k = n - at
        buf[at:] += snd[:k] * gain
        buf[: end - n] += snd[k:] * gain


def seamless(x, xf):
    """Make x loop: its last xf samples fade into its first. Returns len(x) - xf samples."""
    n = len(x) - xf
    out = x[:n].copy()
    t = np.linspace(0, np.pi / 2, xf)
    fade_in, fade_out = np.sin(t), np.cos(t)
    if x.ndim == 2:
        fade_in, fade_out = fade_in[:, None], fade_out[:, None]
    out[:xf] = x[:xf] * fade_in + x[n:] * fade_out
    return out


def level(x, rms):
    r = np.sqrt(np.mean(x ** 2)) + 1e-9
    y = x * (rms / r)
    peak = np.max(np.abs(y))
    return y / peak * 0.97 if peak > 0.97 else y


def write(path, x, sr, q=4):
    full = os.path.join(OUT, path)
    os.makedirs(os.path.dirname(full), exist_ok=True)
    data = np.clip(x, -1, 1)
    pcm = (data * 32767).astype('<i2')
    ch = 1 if pcm.ndim == 1 else pcm.shape[1]
    with tempfile.NamedTemporaryFile(suffix='.raw', delete=False) as f:
        f.write(pcm.tobytes())
        raw = f.name
    subprocess.run(['ffmpeg', '-v', 'error', '-y', '-f', 's16le', '-ar', str(sr), '-ac', str(ch), '-i', raw, '-c:a', 'libvorbis', '-q:a', str(q), full], check=True)
    os.unlink(raw)
    print(f'{path:28s} {len(data) / sr:5.1f} s')


# ------------------------------------------------------------------------------------------------ ambience

def amb_sea(secs=32):
    n = int(secs * SR)
    out = np.zeros(n)
    body = band(brown(n) * 0.6 + pink(n) * 0.08, 0, 900)
    swell = np.zeros(n)
    t = 0.0
    while t < secs:
        # A wave: draws in, breaks, and hisses back down the shingle.
        rise, crash, fall = rng.uniform(1.8, 2.8), rng.uniform(0.3, 0.6), rng.uniform(3.0, 4.5)
        k = int((rise + crash + fall) * SR)
        tt = np.arange(k) / SR
        e = np.where(tt < rise, (tt / rise) ** 2 * 0.5, np.where(tt < rise + crash, 0.5 + 0.5 * (tt - rise) / crash, np.exp(-(tt - rise - crash) / (fall / 3))))
        place(swell, e * rng.uniform(0.7, 1.0), int(t * SR))
        hiss = band(white(k), 1800, 7000) * np.where(tt < rise, 0, np.exp(-(tt - rise) / 1.2)) * 0.35
        place(out, hiss, int(t * SR))
        t += rng.uniform(5.5, 8.0)
    out += body * (0.35 + 0.65 * np.clip(swell, 0, 1.2))
    return level(seamless(out, SR * 2), 0.18)


def amb_river(secs=24):
    n = int(secs * SR)
    flow = band(pink(n), 250, 2600) * (0.7 + 0.3 * wander(n, 0.6))
    bubbles = np.zeros(n)
    for _ in range(int(secs * 70)):
        d = rng.uniform(0.012, 0.04)
        k = int(d * SR)
        tt = np.arange(k) / SR
        f0 = rng.uniform(450, 1500)
        f = f0 * (1 + 1.6 * tt / d)  # a bubble's pitch rises as it closes
        s = np.sin(2 * np.pi * np.cumsum(f) / SR) * np.exp(-tt / (d / 2.5))
        place(bubbles, s * rng.uniform(0.05, 0.25), rng.integers(n))
    return level(seamless(flow + bubbles, SR), 0.13)


def chirp(kind):
    """One bird call."""
    if kind == 'trill':
        notes = rng.integers(5, 12)
        parts = []
        f = rng.uniform(3200, 5200)
        for _ in range(notes):
            d = rng.uniform(0.035, 0.06)
            tt = np.arange(int(d * SR)) / SR
            fr = f * (1 + 0.25 * np.sin(np.pi * tt / d))
            parts.append(np.sin(2 * np.pi * np.cumsum(fr) / SR) * np.sin(np.pi * tt / d) ** 2)
            parts.append(np.zeros(int(rng.uniform(0.01, 0.03) * SR)))
        return np.concatenate(parts)
    if kind == 'whistle':  # two clear notes, falling
        a, b = rng.uniform(2200, 3000), None
        b = a * rng.choice([0.75, 0.8, 0.84])
        out = []
        for f in (a, b):
            d = rng.uniform(0.18, 0.28)
            tt = np.arange(int(d * SR)) / SR
            out.append(np.sin(2 * np.pi * f * tt + 0.02 * np.sin(2 * np.pi * 6 * tt)) * np.sin(np.pi * tt / d) ** 1.5)
            out.append(np.zeros(int(0.06 * SR)))
        return np.concatenate(out)
    # warble: a quick wandering song
    d = rng.uniform(0.6, 1.3)
    k = int(d * SR)
    fr = 2600 + 1300 * wander(k, 18, lo=-1, hi=1)
    am = np.clip(wander(k, 25, lo=-0.3, hi=1), 0, 1)
    return np.sin(2 * np.pi * np.cumsum(fr) / SR) * am * np.sin(np.pi * np.arange(k) / k)


def amb_forest(secs=30):
    n = int(secs * SR)
    leaves = band(pink(n), 300, 3500) * (0.25 + 0.75 * wander(n, 0.25) ** 2) * 0.5
    birds = np.zeros(n)
    for _ in range(int(secs * 0.9)):
        place(birds, chirp(rng.choice(['trill', 'whistle', 'warble'], p=[0.4, 0.3, 0.3])) * rng.uniform(0.04, 0.16), rng.integers(n))
    birds = reverb(birds, SR, 1.6, 0.35, 6000)
    return level(seamless(leaves + birds, SR), 0.1)


def amb_night(secs=30):
    n = int(secs * SR)
    breeze = band(pink(n), 150, 1200) * (0.3 + 0.7 * wander(n, 0.2)) * 0.25
    crickets = np.zeros(n)
    for c in range(4):
        f = rng.uniform(3900, 4700)
        rate = rng.uniform(0.45, 0.8)
        gain = rng.uniform(0.03, 0.07)
        t = rng.uniform(0, rate)
        while t < secs:
            pulses = rng.integers(2, 5)
            k = int(pulses * 0.035 * SR)
            tt = np.arange(k) / SR
            s = np.sin(2 * np.pi * f * tt) * (0.5 + 0.5 * np.sign(np.sin(2 * np.pi * 28 * tt))) * np.sin(np.pi * tt / tt[-1])
            place(crickets, s * gain, int(t * SR))
            t += rate * rng.uniform(0.9, 1.1)
    owl = np.zeros(n)
    for at in (rng.uniform(4, 10), rng.uniform(18, 25)):
        for i, (d, f) in enumerate([(0.35, 390), (0.18, 380), (0.6, 370)]):
            k = int(d * SR)
            tt = np.arange(k) / SR
            s = (np.sin(2 * np.pi * f * tt) + 0.2 * np.sin(4 * np.pi * f * tt)) * np.sin(np.pi * tt / d) ** 2
            place(owl, s * 0.09, int((at + i * 0.5) * SR))
    owl = reverb(owl, SR, 2.5, 0.5, 2500)
    mix = breeze + crickets + owl
    return level(seamless(mix, SR), 0.08)


def amb_wind(secs=28):
    n = int(secs * SR)
    raw = pink(n)
    out = np.zeros(n)
    # A resonance that sweeps slowly, block by block: wind moaning round things.
    centre = 250 + 600 * wander(n, 0.3)
    blk = 2048
    for i in range(0, n, blk):
        c = centre[i]
        out[i:i + blk] = band(raw[max(0, i - 4096): i + blk], c * 0.6, c * 1.6)[-min(blk, n - i):]
    gust = 0.25 + 0.75 * wander(n, 0.18) ** 1.5
    mix = out * gust + band(pink(n), 0, 300) * 0.3
    return level(seamless(mix, SR * 2), 0.14)


def amb_rain(secs=20):
    n = int(secs * SR)
    hiss = band(white(n), 900, 9000) * 0.5 + band(pink(n), 200, 1500) * 0.4
    drops = np.zeros(n)
    for _ in range(int(secs * 140)):
        k = int(0.01 * SR)
        s = band(white(k), 2000, 9000) * np.exp(-np.arange(k) / (0.0015 * SR))
        place(drops, s * rng.uniform(0.05, 0.3), rng.integers(n))
    mix = hiss * (0.85 + 0.15 * wander(n, 0.4)) + drops
    return level(seamless(mix, SR), 0.14)


def amb_fire(secs=16):
    n = int(secs * SR)
    roar = band(brown(n), 60, 400) * (0.6 + 0.4 * wander(n, 1.2)) * 0.6
    cr = np.zeros(n)
    for _ in range(int(secs * 30)):
        k = int(rng.uniform(0.003, 0.012) * SR)
        s = band(white(k), 1200, 7000) * np.exp(-np.arange(k) / (k / 4))
        place(cr, s * rng.uniform(0.05, 0.5) ** 2, rng.integers(n))
    for _ in range(int(secs * 0.8)):  # the odd loud pop
        k = int(0.03 * SR)
        place(cr, band(white(k), 500, 5000) * np.exp(-np.arange(k) / (0.004 * SR)) * 0.6, rng.integers(n))
    mix = roar + cr
    return level(seamless(mix, SR // 2), 0.12)


def murmur(n, voices=8, lo=180, hi=1400):
    """Many people talking at once, too far off to make out."""
    out = np.zeros(n)
    for _ in range(voices):
        rate = rng.uniform(3.5, 6.0)
        syll = np.clip(wander(n, rate, lo=-0.6, hi=1.0), 0, 1)
        phrase = np.clip(wander(n, 0.35, lo=-0.4, hi=1.0), 0, 1)
        c = rng.uniform(lo * 1.6, hi * 0.7)
        out += band(white(n), c * 0.7, c * 1.4) * syll * phrase
    return out / voices


def clang(f=2400, d=0.6, sr=SR):
    k = int(d * sr)
    tt = np.arange(k) / sr
    ratios = [1, 2.76, 5.4, 8.93]
    return sum(np.sin(2 * np.pi * f * r * tt) * np.exp(-tt * (3 + 3 * i)) / (i + 1) for i, r in enumerate(ratios))


def amb_town(secs=30):
    n = int(secs * SR)
    mix = murmur(n, 10) * 0.8 + band(pink(n), 100, 800) * 0.08
    for _ in range(int(secs * 0.5)):  # a smith's hammer, now and then, in threes
        at = rng.integers(n)
        for j in range(rng.integers(2, 4)):
            place(mix, clang(rng.uniform(1600, 2200), 0.4) * 0.06, at + int(j * 0.45 * SR))
    mix = reverb(mix, SR, 1.2, 0.25, 3500)
    return level(seamless(mix, SR), 0.11)


def amb_market(secs=30):
    n = int(secs * SR)
    mix = murmur(n, 16, 200, 1800) * 1.0
    for _ in range(int(secs * 0.4)):  # a trader's call: a sung vowel that rises and falls
        d = rng.uniform(0.6, 1.1)
        k = int(d * SR)
        tt = np.arange(k) / SR
        f = rng.uniform(150, 230) * (1 + 0.25 * np.sin(np.pi * tt / d))
        saw = signal.sawtooth(2 * np.pi * np.cumsum(f) / SR)
        voice = band(saw, 500, 1300) + 0.5 * band(saw, 2200, 2900)
        place(mix, voice * np.sin(np.pi * tt / d) * 0.12, rng.integers(n))
    for _ in range(int(secs * 0.6)):  # coins
        at = rng.integers(n)
        for j in range(rng.integers(2, 5)):
            place(mix, clang(rng.uniform(3500, 5000), 0.15) * 0.03, at + int(j * rng.uniform(0.05, 0.12) * SR))
    mix = reverb(mix, SR, 1.0, 0.2, 4000)
    return level(seamless(mix, SR), 0.12)


def amb_crypt(secs=32):
    n = int(secs * SR)
    t = np.arange(n) / SR
    drone = (np.sin(2 * np.pi * 43.65 * t) + 0.7 * np.sin(2 * np.pi * 44.1 * t) + 0.3 * np.sin(2 * np.pi * 65.4 * t)) * (0.6 + 0.4 * wander(n, 0.1))
    air = band(pink(n), 180, 420) * (0.3 + 0.7 * wander(n, 0.15)) * 0.6
    drips = np.zeros(n)
    for _ in range(int(secs * 0.5)):
        k = int(0.12 * SR)
        tt = np.arange(k) / SR
        f = rng.uniform(1100, 2000) * np.exp(-tt * 6)
        place(drips, np.sin(2 * np.pi * np.cumsum(f) / SR) * np.exp(-tt * 30) * 0.15, rng.integers(n))
    mix = drone * 0.25 + air + reverb(drips, SR, 3.5, 0.6, 3000)
    return level(seamless(mix, SR * 2), 0.1)


def choir(n, sr, notes, vowel=(730, 1090, 2440)):
    """A soft held chord of voices singing 'ah'."""
    t = np.arange(n) / sr
    out = np.zeros(n)
    for f in notes:
        for det in (-0.004, 0.0, 0.005):
            vib = 1 + 0.004 * np.sin(2 * np.pi * rng.uniform(4.5, 5.5) * t + rng.uniform(0, 6))
            saw = signal.sawtooth(2 * np.pi * np.cumsum(f * (1 + det) * vib) / sr)
            out += sum(band(saw, fm * 0.85, fm * 1.15, sr) * g for fm, g in zip(vowel, (1, 0.5, 0.2)))
    return out / (len(notes) * 3)


def bell(f, d, sr):
    k = int(d * sr)
    tt = np.arange(k) / sr
    ratios = [0.5, 1, 1.19, 1.5, 2, 2.74, 3, 4.1]
    return sum(np.sin(2 * np.pi * f * r * tt) * np.exp(-tt * (0.6 + i * 0.5)) / (1 + i * 0.6) for i, r in enumerate(ratios))


def amb_holy(secs=32):
    n = int(secs * SR)
    seg = n // 2
    a = choir(seg + SR, SR, [146.8, 220.0, 293.7])
    b = choir(n - seg + SR, SR, [130.8, 196.0, 261.6])
    mix = np.zeros(n + SR)
    fa = np.sin(np.linspace(0, np.pi, seg + SR)) ** 0.6
    fb = np.sin(np.linspace(0, np.pi, n - seg + SR)) ** 0.6
    mix[: seg + SR] += a * fa
    mix[seg: n + SR] += b[: n - seg + SR] * fb
    place(mix, bell(392, 5, SR) * 0.15, int(3 * SR))
    mix = reverb(mix, SR, 4.0, 0.55, 4500)
    return level(seamless(mix, SR), 0.1)


def amb_battle(secs=24):
    n = int(secs * SR)
    mix = murmur(n, 12, 250, 1500) * 0.6  # shouting, far off
    for _ in range(int(secs * 2.5)):
        place(mix, clang(rng.uniform(1200, 2600), 0.5) * rng.uniform(0.02, 0.08), rng.integers(n))
    for i in range(int(secs * 1.2)):  # a war drum
        k = int(0.4 * SR)
        tt = np.arange(k) / SR
        place(mix, np.sin(2 * np.pi * np.cumsum(90 * np.exp(-tt * 5) + 45) / SR) * np.exp(-tt * 7) * 0.25, int(i * SR / 1.2))
    mix = reverb(mix, SR, 2.0, 0.4, 2500)
    return level(seamless(mix, SR), 0.11)


# ------------------------------------------------------------------------------------------------ effects

def sfx_step(surface):
    k = int(0.16 * SR)
    tt = np.arange(k) / SR
    if surface == 'grass':
        s = band(white(k), 600, 4500) * np.exp(-tt / 0.03) * 0.6 + band(white(k), 80, 400) * np.exp(-tt / 0.02) * 0.4
    elif surface == 'road':
        s = band(white(k), 300, 3000) * np.exp(-tt / 0.025) * 0.6 + band(white(k), 2000, 8000) * np.exp(-tt / 0.05) * 0.2
    elif surface == 'stone':
        s = band(white(k), 900, 6000) * np.exp(-tt / 0.008) + band(white(k), 120, 500) * np.exp(-tt / 0.015) * 0.5
    else:  # water
        s = band(white(k), 400, 3000) * np.exp(-tt / 0.06) * 0.6
        for _ in range(4):
            place(s, np.sin(2 * np.pi * rng.uniform(600, 1300) * tt[: k // 3]) * np.exp(-tt[: k // 3] / 0.01) * 0.2, rng.integers(k // 2))
    return level(s, 0.2)


def sfx_swing():
    k = int(0.28 * SR)
    tt = np.arange(k) / SR
    raw = white(k)
    out = np.zeros(k)
    for i in range(0, k, 256):  # a band that sweeps up and down: the blade through the air
        c = 600 + 2400 * np.sin(np.pi * i / k)
        out[i:i + 256] = band(raw[max(0, i - 1024): i + 256], c * 0.7, c * 1.3)[-min(256, k - i):]
    return level(out * np.sin(np.pi * tt / tt[-1]) ** 2, 0.18)


def sfx_hit():
    k = int(0.45 * SR)
    tt = np.arange(k) / SR
    thud = np.sin(2 * np.pi * np.cumsum(140 * np.exp(-tt * 18) + 60) / SR) * np.exp(-tt * 14)
    crack = band(white(k), 800, 5000) * np.exp(-tt * 60)
    metal = clang(rng.uniform(1500, 2200), 0.45) * 0.25
    return level(thud * 0.8 + crack * 0.6 + metal, 0.22)


def sfx_hurt():
    k = int(0.3 * SR)
    tt = np.arange(k) / SR
    f = 160 * (1 - 0.3 * tt / tt[-1])
    v = signal.sawtooth(2 * np.pi * np.cumsum(f) / SR)
    s = (band(v, 500, 900) + 0.6 * band(v, 1000, 1400)) * np.sin(np.pi * tt / tt[-1]) * np.exp(-tt * 3)
    return level(s + band(white(k), 100, 600) * np.exp(-tt * 25) * 0.4, 0.18)


def sfx_pickup():
    s = np.zeros(int(0.6 * SR))
    for i, f in enumerate((1318.5, 1975.5)):
        place(s, bell(f, 0.5, SR)[: int(0.5 * SR)] * 0.5, int(i * 0.08 * SR))
    return level(reverb(s, SR, 0.8, 0.2), 0.12)


def sfx_drop():
    k = int(0.25 * SR)
    tt = np.arange(k) / SR
    return level(np.sin(2 * np.pi * np.cumsum(220 * np.exp(-tt * 20) + 90) / SR) * np.exp(-tt * 25) + band(white(k), 300, 2000) * np.exp(-tt * 40) * 0.4, 0.15)


def sfx_place():
    k = int(0.22 * SR)
    tt = np.arange(k) / SR
    knock = np.sin(2 * np.pi * 380 * tt) * np.exp(-tt * 40) + np.sin(2 * np.pi * 760 * tt) * np.exp(-tt * 60) * 0.5
    return level(knock + band(white(k), 1000, 5000) * np.exp(-tt * 90) * 0.5, 0.2)


def sfx_break():
    k = int(0.9 * SR)
    s = band(brown(k), 60, 500) * np.exp(-np.arange(k) / SR * 5) * 0.8
    for _ in range(40):  # stones tumbling
        at = int(rng.uniform(0, 0.6) ** 1.5 * SR)
        kk = int(0.02 * SR)
        place(s, band(white(kk), 700, 5000) * np.exp(-np.arange(kk) / (0.004 * SR)) * rng.uniform(0.1, 0.5), at)
    return level(s, 0.2)


def sfx_horn():
    """A call on a horn: a fifth rising, held, for an event's banner."""
    out = np.zeros(int(3.6 * SR))
    for at, f, d in [(0.0, 196.0, 0.9), (0.95, 293.7, 2.2)]:
        k = int(d * SR)
        tt = np.arange(k) / SR
        vib = 1 + 0.004 * np.sin(2 * np.pi * 5 * tt) * np.minimum(tt / 0.4, 1)
        tone = sum(np.sin(2 * np.pi * np.cumsum(f * h * vib) / SR) / h ** 1.1 for h in range(1, 12))
        tone = band(tone, 150, 2400)
        e = np.minimum(tt / 0.12, 1) * np.minimum((d - tt) / 0.35, 1)
        place(out, tone * e, int(at * SR))
    return level(reverb(out, SR, 2.8, 0.4, 3000), 0.16)


def sfx_chime():
    out = np.zeros(int(2.4 * SR))
    for i, f in enumerate((587.3, 740.0, 880.0)):  # D, F#, A: a bright, open arrival
        place(out, bell(f, 1.8, SR)[: int(1.8 * SR)] * 0.4, int(i * 0.16 * SR))
    return level(reverb(out, SR, 2.0, 0.35), 0.08)


def sfx_click():
    k = int(0.03 * SR)
    return level(band(white(k), 2000, 7000) * np.exp(-np.arange(k) / (0.003 * SR)), 0.12)


def sfx_splash():
    k = int(0.7 * SR)
    tt = np.arange(k) / SR
    s = band(white(k), 300, 4000) * np.exp(-tt * 6)
    for _ in range(12):
        kk = int(0.05 * SR)
        f = rng.uniform(500, 1400)
        place(s, np.sin(2 * np.pi * f * np.arange(kk) / SR * (1 + np.arange(kk) / kk)) * np.exp(-np.arange(kk) / (0.015 * SR)) * 0.3, int(rng.uniform(0.05, 0.5) * SR))
    return level(s, 0.18)


# ------------------------------------------------------------------------------------------------ spells

def sweep(x, centre, width=0.3, hop=256):
    """Band-pass x round a centre frequency that moves (one value per sample), like the swing's blade."""
    out = np.zeros(len(x))
    for i in range(0, len(x), hop):
        c = float(centre[min(i, len(x) - 1)])
        seg = band(x[max(0, i - 1024): i + hop], c * (1 - width), c * (1 + width))
        out[i:i + hop] = seg[-min(hop, len(x) - i):]
    return out


def ping(f, d, decay, sr=SR):
    """A short glassy ping: a sine and a slightly sharp partial."""
    tt = np.arange(int(d * sr)) / sr
    return (np.sin(2 * np.pi * f * tt) + 0.35 * np.sin(2 * np.pi * f * 2.76 * tt)) * np.exp(-tt / decay) * np.minimum(tt / 0.002, 1)


def crackles(k, count, lo=0.0, hi=1.0, gain=(0.1, 0.5), band_=(1500, 7000)):
    s = np.zeros(k)
    for _ in range(count):
        kk = int(0.006 * SR)
        place(s, band(white(kk), *band_) * np.exp(-np.arange(kk) / (0.0012 * SR)) * rng.uniform(*gain), int(rng.uniform(lo, hi) * k))
    return s


def bubbles(k, count, lo=0.0, hi=1.0, f=(250, 900), gain=0.3):
    s = np.zeros(k)
    for _ in range(count):
        kk = int(rng.uniform(0.02, 0.06) * SR)
        t = np.arange(kk) / SR
        f0 = rng.uniform(*f)
        place(s, np.sin(2 * np.pi * np.cumsum(f0 * (1 + 1.5 * t / t[-1])) / SR) * np.exp(-t / 0.012) * gain * rng.uniform(0.4, 1), int(rng.uniform(lo, hi) * k), 1.0)
    return s


def thump(k, f0, f1, rate, decay):
    tt = np.arange(k) / SR
    return np.sin(2 * np.pi * np.cumsum(f0 * np.exp(-tt * rate) + f1) / SR) * np.exp(-tt / decay)


def swell(tt, peak):
    """Rises to 1 at `peak` seconds, then falls away to nothing at the end."""
    return np.where(tt < peak, (tt / peak) ** 2, np.cos(np.pi / 2 * np.clip((tt - peak) / (tt[-1] - peak + 1e-9), 0, 1)) ** 1.5)


def squash(x, drive=2.0):
    """Soft-clip the first crack of an impact so its body can be as loud as the other effects."""
    return np.tanh(drive * x / (np.max(np.abs(x)) + 1e-9))


def cast_fire():
    k = int(0.75 * SR)
    tt = np.arange(k) / SR
    roar = band(pink(k), 120, 1400) * swell(tt, 0.3) * 0.9
    whoosh = sweep(white(k), 500 + 2200 * np.sin(np.pi * tt / tt[-1]) ** 2, 0.4) * swell(tt, 0.35) * 0.7
    return level(roar + whoosh + crackles(k, 45, 0.05, 0.9) * swell(tt, 0.4), 0.16)


def cast_frost():
    k = int(0.8 * SR)
    tt = np.arange(k) / SR
    air = sweep(white(k), 2500 + 4500 * tt / tt[-1], 0.25) * swell(tt, 0.4) * 0.5
    s = air
    for i in range(22):  # ice crystals forming, higher as it goes
        at = rng.uniform(0, 0.65)
        place(s, ping(rng.uniform(2200, 3400) * (1 + at), 0.15, 0.04) * 0.25, int(at * SR))
    return level(reverb(s, SR, 0.9, 0.25, 7000), 0.12)


def cast_shock():
    k = int(0.5 * SR)
    tt = np.arange(k) / SR
    f = 90 + 60 * wander(k, 40) + 400 * np.exp(-tt * 12)
    buzz = np.tanh(4 * signal.sawtooth(2 * np.pi * np.cumsum(f) / SR)) * (0.5 + 0.5 * (wander(k, 60) > 0.4))
    buzz = band(buzz, 200, 6000) * swell(tt, 0.08) * 0.5
    zap = sweep(white(k), 6000 * np.exp(-tt * 6) + 600, 0.3) * np.exp(-tt * 8) * 0.8
    return level(buzz + zap + crackles(k, 60, 0, 0.85, (0.2, 0.8), (2500, 10000)), 0.16)


def cast_holy():
    k = int(0.95 * SR)
    tt = np.arange(k) / SR
    s = np.zeros(k)
    for i, f in enumerate((523.3, 659.3, 784.0, 1046.5)):  # C major, opening upward
        vib = 1 + 0.003 * np.sin(2 * np.pi * 5.5 * tt + i)
        s += sum(np.sin(2 * np.pi * np.cumsum(f * h * vib) / SR) / h ** 1.6 for h in (1, 2, 3)) * np.clip((tt - i * 0.06) / 0.4, 0, 1) ** 2
    s *= np.minimum((tt[-1] - tt) / 0.35, 1)
    place(s, bell(1568.0, 0.5, SR) * 0.25, int(0.4 * SR))
    return level(reverb(s, SR, 1.4, 0.35), 0.11)


def cast_shadow():
    k = int(0.9 * SR)
    tt = np.arange(k) / SR
    rise = np.clip(tt / 0.75, 0, 1) ** 3 * np.minimum((tt[-1] - tt) / 0.06, 1)  # a reversed swell, cut off sharp
    low = band(brown(k), 40, 400) * 1.5 + np.sin(2 * np.pi * np.cumsum(70 - 25 * tt) / SR) * 0.5
    whisper = band(white(k), 1500, 4500) * (0.5 + 0.5 * wander(k, 18)) * 0.35
    s = (low + whisper) * rise
    s = reverb(s[::-1], SR, 1.0, 0.5, 1200)[::-1] * rise  # the tail comes before the sound
    return level(s, 0.15)


def cast_nature():
    k = int(0.75 * SR)
    tt = np.arange(k) / SR
    leaves = band(white(k), 1800, 7000) * wander(k, 35) ** 2 * swell(tt, 0.3) * 0.7
    s = leaves
    for at in (0.0, 0.07, 0.15):  # woody knocks
        place(s, thump(int(0.08 * SR), 120, rng.uniform(320, 520), 40, 0.02) * 0.5, int(at * SR))
    for i, f in enumerate((784.0, 880.0, 1174.7)):  # a soft pentatonic chime
        place(s, ping(f, 0.4, 0.12) * 0.25, int((0.25 + i * 0.09) * SR))
    return level(reverb(s, SR, 0.8, 0.2), 0.13)


def cast_arcane():
    k = int(0.85 * SR)
    tt = np.arange(k) / SR
    f = 400 * 2 ** (2.2 * tt / tt[-1])
    gliss = sum(np.sin(2 * np.pi * np.cumsum(f * d) / SR) for d in (0.995, 1.0, 1.007)) / 3 * swell(tt, 0.5) * 0.5
    s = gliss + sweep(white(k), 1500 + 4000 * tt / tt[-1], 0.2) * swell(tt, 0.5) * 0.3
    for _ in range(26):
        at = rng.uniform(0, 0.75)
        place(s, ping(rng.uniform(1800, 3000) * (1 + 0.8 * at), 0.12, 0.03) * 0.18, int(at * SR))
    return level(reverb(s, SR, 1.0, 0.3, 6000), 0.12)


def cast_poison():
    k = int(0.7 * SR)
    tt = np.arange(k) / SR
    hiss = band(white(k), 3000, 9000) * swell(tt, 0.4) * 0.35
    goo = band(brown(k), 80, 500) * swell(tt, 0.25) * 0.8
    return level(hiss + goo + bubbles(k, 40, 0, 0.85), 0.15)


def cast_mind():
    k = int(0.85 * SR)
    tt = np.arange(k) / SR
    vib = 1 + 0.08 * np.sin(2 * np.pi * (5 + 4 * tt) * tt)
    f = 330 * 2 ** (0.5 * np.sin(np.pi * tt / tt[-1])) * vib
    tone = signal.sawtooth(2 * np.pi * np.cumsum(f) / SR, 0.5) * np.sin(2 * np.pi * np.cumsum(f * 1.41) / SR)  # ring-modded
    s = (band(tone, 200, 3000) + 0.4 * np.sin(2 * np.pi * np.cumsum(f * 0.5) / SR)) * swell(tt, 0.35)
    return level(reverb(s, SR, 1.0, 0.35, 3500), 0.14)


def cast_blood():
    k = int(0.75 * SR)
    tt = np.arange(k) / SR
    s = np.zeros(k)
    for at, g in ((0.0, 0.8), (0.18, 1.0)):  # a heartbeat, then the push
        place(s, thump(int(0.3 * SR), 60, 45, 15, 0.08) * g, int(at * SR))
    wet = sweep(brown(k) * 4, 900 - 600 * tt / tt[-1], 0.5) * swell(tt, 0.3) * 0.6
    s += wet + bubbles(k, 10, 0.15, 0.6, (150, 400), 0.25)
    return level(band(s, 30, 3000), 0.13)


def impact_fire():
    k = int(0.8 * SR)
    tt = np.arange(k) / SR
    burst = band(white(k), 200, 5000) * np.exp(-tt * 14) * 0.8
    rumble = band(brown(k), 40, 400) * np.exp(-tt * 5) * 1.2
    return level(squash(burst + rumble + thump(k, 120, 50, 20, 0.1) * 0.6 + crackles(k, 40, 0.05, 0.85) * np.exp(-tt * 3)), 0.13)


def impact_frost():
    k = int(0.7 * SR)
    tt = np.arange(k) / SR
    s = band(white(k), 2000, 10000) * np.exp(-tt * 50) * 0.9 + thump(k, 200, 150, 30, 0.03) * 0.3
    for _ in range(30):  # shards falling
        at = rng.uniform(0, 0.5) ** 1.4
        place(s, ping(rng.uniform(2500, 6000), 0.1, rng.uniform(0.01, 0.03)) * rng.uniform(0.1, 0.35), int(at * SR))
    return level(squash(reverb(s, SR, 0.6, 0.2, 8000), 2.5), 0.15)


def impact_shock():
    k = int(0.5 * SR)
    tt = np.arange(k) / SR
    crack = white(k) * np.exp(-tt * 120)
    buzz = band(np.tanh(5 * signal.sawtooth(2 * np.pi * (110 + 30 * wander(k, 50)) * tt)), 200, 5000) * np.exp(-tt * 9) * 0.35
    return level(squash(crack * 0.6 + buzz + thump(k, 150, 50, 25, 0.06) * 0.6 + crackles(k, 25, 0, 0.5, (0.1, 0.5), (3000, 10000)), 3.0), 0.12)


def impact_holy():
    s = np.zeros(int(0.9 * SR))
    for i, f in enumerate((1046.5, 1318.5, 1568.0)):
        place(s, bell(f, 0.8, SR)[: int(0.8 * SR)] * 0.4, int(i * 0.03 * SR))
    return level(reverb(s, SR, 1.0, 0.3), 0.11)


def impact_shadow():
    k = int(0.9 * SR)
    tt = np.arange(k) / SR
    s = thump(k, 90, 40, 12, 0.12) + band(brown(k), 30, 250) * np.exp(-tt * 3.5) * 1.2 + band(white(k), 300, 1500) * np.exp(-tt * 40) * 0.3
    return level(reverb(s, SR, 1.2, 0.45, 700), 0.13)


def impact_nature():
    k = int(0.5 * SR)
    tt = np.arange(k) / SR
    s = np.zeros(k)
    for at in (0.0, 0.025, 0.06, 0.1):  # a stick snapping, splinters after
        kk = int(0.04 * SR)
        place(s, band(white(kk), 900, 5000) * np.exp(-np.arange(kk) / (0.005 * SR)) * rng.uniform(0.5, 1), int(at * SR))
    creak = band(signal.sawtooth(2 * np.pi * np.cumsum(180 + 60 * tt) / SR), 300, 1500) * (0.5 + 0.5 * wander(k, 30)) * np.exp(-tt * 9) * 0.3
    return level(squash(s + creak + thump(k, 80, 260, 50, 0.03) * 0.4, 2.5), 0.17)


def impact_arcane():
    k = int(0.7 * SR)
    tt = np.arange(k) / SR
    s = thump(k, 900, 200, 45, 0.03) * 0.7  # the pop
    for _ in range(20):
        at = rng.uniform(0.01, 0.4)
        place(s, ping(rng.uniform(2000, 4500), 0.15, 0.04) * rng.uniform(0.1, 0.3), int(at * SR))
    return level(reverb(s, SR, 0.9, 0.3, 6000), 0.13)


def impact_poison():
    k = int(0.7 * SR)
    tt = np.arange(k) / SR
    splat = band(brown(k) * 3, 100, 1500) * np.exp(-tt * 18) + band(white(k), 500, 3000) * np.exp(-tt * 35) * 0.5
    hiss = band(white(k), 3500, 9000) * np.clip(tt / 0.05, 0, 1) * np.exp(-tt * 4) * 0.3
    return level(splat + hiss + bubbles(k, 18, 0.05, 0.7, (300, 1000), 0.25), 0.16)


def impact_mind():
    k = int(0.9 * SR)
    tt = np.arange(k) / SR
    ring = sum(np.sin(2 * np.pi * f * tt) * g for f, g in ((440, 1), (446, 0.8), (1210, 0.3))) * np.exp(-tt * 3.5)  # beating
    ring = band(ring, 0, 1500)
    return level(ring * 0.5 + thump(k, 80, 60, 20, 0.06) * 0.5, 0.12)


def impact_blood():
    k = int(0.6 * SR)
    tt = np.arange(k) / SR
    squelch = sweep(brown(k) * 4, 1200 * np.exp(-tt * 6) + 200, 0.5) * np.exp(-tt * 7)
    return level(thump(k, 70, 45, 14, 0.1) + squelch * 0.8 + bubbles(k, 8, 0.03, 0.4, (120, 350), 0.3), 0.14)


def heal():
    s = np.zeros(int(1.1 * SR))
    for i, f in enumerate((784.0, 880.0, 1046.5, 1174.7, 1318.5, 1568.0, 1760.0)):  # rising pentatonic sparkle
        place(s, ping(f, 0.5, 0.15) * 0.35, int(i * 0.07 * SR))
    tt = np.arange(len(s)) / SR
    s += band(white(len(s)), 4000, 10000) * swell(tt, 0.45) * 0.08
    return level(reverb(s, SR, 1.2, 0.35, 7000), 0.1)


def held():
    k = int(0.8 * SR)
    tt = np.arange(k) / SR
    f = 700 + 250 * np.sin(2 * np.pi * 6 * tt)  # round and round
    tone = np.sin(2 * np.pi * np.cumsum(f * (1 - 0.3 * tt)) / SR)
    tone *= 0.6 + 0.4 * np.sin(2 * np.pi * 3 * tt)
    s = tone * np.minimum(tt / 0.03, 1) * np.minimum((tt[-1] - tt) / 0.25, 1) * 0.5
    for i in range(5):  # little stars circling
        place(s, ping(2100 + 300 * np.sin(i * 1.3), 0.12, 0.03) * 0.25, int((0.05 + i * 0.13) * SR))
    return level(reverb(s, SR, 0.8, 0.3), 0.13)


SPELLS = ('fire', 'frost', 'shock', 'holy', 'shadow', 'nature', 'arcane', 'poison', 'mind', 'blood')


def seeded(seed, fn):
    """Run fn with its own random stream, so a spell sound never depends on what was made before it."""
    def run():
        global rng
        keep, rng = rng, np.random.default_rng(seed)
        try:
            return fn()
        finally:
            rng = keep
    return run


# ------------------------------------------------------------------------------------------------ music

def midi(m):
    return 440.0 * 2 ** ((m - 69) / 12)


MODES = {'dorian': [0, 2, 3, 5, 7, 9, 10], 'mixolydian': [0, 2, 4, 5, 7, 9, 10], 'aeolian': [0, 2, 3, 5, 7, 8, 10], 'phrygian': [0, 1, 3, 5, 7, 8, 10]}


def scale_note(root, mode, degree):
    s = MODES[mode]
    return root + 12 * (degree // 7) + s[degree % 7]


def harp(f, d, sr=MR, bright=0.5):
    """A plucked string (Karplus-Strong): a burst of noise round a loop as long as one period."""
    n = int(d * sr)
    p = max(2, int(sr / f))
    buf = band(white(p), 0, 2000 + 6000 * bright, sr, 1)
    out = np.empty(n)
    loss = 0.996 if f < 300 else 0.993
    b = buf.copy()
    i = 0
    # Run the loop a period at a time (numpy), averaging each sample with the next.
    while i < n:
        k = min(p, n - i)
        out[i:i + k] = b[:k]
        b = loss * 0.5 * (b + np.roll(b, -1))
        i += k
    return out * np.minimum(np.arange(n) / (0.002 * sr), 1)


def whistle(f, d, sr=MR, cut=False):
    """A low whistle: a near-sine with breath, vibrato once the note settles, and a cut (a grace note) if asked."""
    n = int(d * sr)
    tt = np.arange(n) / sr
    vib = 1 + 0.006 * np.sin(2 * np.pi * 5.2 * tt) * np.clip((tt - 0.25) / 0.3, 0, 1)
    fr = f * vib
    if cut and d > 0.18:
        g = int(0.035 * sr)
        fr[:g] = f * 2 ** (5 / 12)  # flick up to the note above
    ph = 2 * np.pi * np.cumsum(fr) / sr
    tone = np.sin(ph) + 0.12 * np.sin(2 * ph) + 0.05 * np.sin(3 * ph)
    breath = band(white(n), f * 0.9, f * 3.5, sr) * 0.12
    e = np.minimum(tt / 0.03, 1) * np.minimum((d - tt) / 0.05, 1).clip(0, 1)
    return (tone + breath) * e


def fiddle(f, d, sr=MR):
    n = int(d * sr)
    tt = np.arange(n) / sr
    vib = 1 + 0.005 * np.sin(2 * np.pi * 5.8 * tt) * np.clip((tt - 0.12) / 0.2, 0, 1)
    ph = np.cumsum(f * vib) / sr
    saw = sum(np.sin(2 * np.pi * h * ph) / h for h in range(1, 18))
    body = band(saw, 250, 3200, sr) + 0.5 * band(saw, 2500, 3500, sr)
    e = np.minimum(tt / 0.04, 1) * np.minimum((d - tt) / 0.06, 1).clip(0, 1)
    return body * e * 0.5


def drone(freqs, n, sr=MR, xf=3.0):
    """A held drone n samples long plus xf seconds, fading in over its first xf and out over its last: rendered into a
    piece n long, its end falls in the tail that `finish` folds back onto the start, so the drone crosses the loop
    point without a break."""
    k = int(xf * sr)
    n += k
    tt = np.arange(n) / sr
    out = np.zeros(n)
    for f in freqs:
        ph = np.cumsum(f * (1 + 0.001 * np.sin(2 * np.pi * 0.3 * tt))) / sr
        out += band(sum(np.sin(2 * np.pi * h * ph) / h for h in range(1, 10)), 0, 700, sr)
    ramp = np.linspace(0, np.pi / 2, k)
    out[:k] *= np.sin(ramp)
    out[-k:] *= np.cos(ramp)
    return out / len(freqs)


def bodhran(d, sr=MR, accent=1.0):
    n = int(d * sr)
    tt = np.arange(n) / sr
    tone = np.sin(2 * np.pi * np.cumsum(55 + 75 * np.exp(-tt * 30)) / sr) * np.exp(-tt * 9)
    skin = band(white(n), 200, 2000, sr) * np.exp(-tt * 45) * 0.4
    return (tone + skin) * accent


def tune(root, mode, bars, beats, rng_tune, motif_len=None):
    """A tune in the Irish way: an A part and a B part, each a phrase answered by a variation of itself, moving mostly by
    step with leaps to chord tones, and coming home to the root at the end of each part."""
    motif_len = motif_len or beats * 2
    def phrase(start, length, home):
        notes = []
        deg = start
        for i in range(length):
            if i == length - 1 and home:
                deg = 7 * (deg // 7)  # end on the root
            notes.append(deg)
            step = rng_tune.choice([-2, -1, -1, 1, 1, 2, 0, 3, -3], p=[0.08, 0.22, 0.12, 0.22, 0.12, 0.08, 0.06, 0.05, 0.05])
            deg = int(np.clip(deg + step, 0, 11))
        return notes
    a1 = phrase(rng_tune.integers(2, 6), motif_len, False)
    a2 = a1[: motif_len // 2] + phrase(a1[motif_len // 2], motif_len - motif_len // 2, True)
    b1 = phrase(rng_tune.integers(5, 9), motif_len, False)
    b2 = b1[: motif_len // 2] + phrase(b1[motif_len // 2], motif_len - motif_len // 2, True)
    part_a = a1 + a2
    part_b = b1 + b2
    return [scale_note(root, mode, d) for d in part_a + part_a + part_b + part_b]


def render(events, secs, sr=MR, tail=6.0):
    """Mix (start, sound, gain, pan) into a stereo buffer `secs` long plus a tail, then fold the tail onto the start, so the
    piece loops with nothing cut off."""
    n = int(secs * sr)
    buf = np.zeros((n + int(tail * sr), 2))
    for at, snd, g, pan in events:
        i = int(at * sr)
        k = min(len(snd), len(buf) - i)
        if k <= 0:
            continue
        l, r = np.cos((pan + 1) * np.pi / 4), np.sin((pan + 1) * np.pi / 4)
        buf[i:i + k, 0] += snd[:k] * g * l
        buf[i:i + k, 1] += snd[:k] * g * r
    return buf, n


def finish(buf, n, sr, rv=0.3, secs=2.4, rms=0.11):
    buf = reverb(buf, sr, secs, rv, 5000)
    out = buf[:n].copy()
    out[: len(buf) - n] += buf[n:]
    return level(out, rms)


def mus_explore():
    """A slow air in D dorian, 3/4: whistle over harp, a drone underneath."""
    r = np.random.default_rng(7)
    bpm, beats = 66, 3
    beat = 60 / bpm
    notes = tune(62, 'dorian', 16, beats, r, motif_len=12)
    ev = []
    t = 0.0
    chords = [[50, 57, 62], [48, 55, 60], [50, 57, 62], [43, 50, 55]]  # Dm C Dm G, open voicings
    total = len(notes) * beat * 1.0
    # Melody: mostly one beat a note, some held two beats, a breath at phrase ends.
    i = 0
    while i < len(notes):
        d = beat * (2 if r.random() < 0.18 else 1)
        ev.append((t, whistle(midi(notes[i]), d * 0.97, cut=r.random() < 0.25), 0.35, 0.15))
        t += d
        i += 1
    total = t
    bars = int(np.ceil(total / (beat * beats)))
    for b in range(bars):
        ch = chords[(b // 2) % 4]
        for j, m in enumerate(ch + [ch[1] + 12]):
            ev.append((b * beat * beats + j * beat * 0.75, harp(midi(m), 3.5), 0.22, -0.3))
    ev.append((0, drone([midi(38), midi(45)], int(total * MR)), 0.05, 0.0))
    buf, n = render(ev, total)
    return finish(buf, n, MR, 0.32, 2.8, 0.1)


def mus_town():
    """A jig in G mixolydian, 6/8: fiddle and whistle together, harp on the chords, bodhrán driving it."""
    r = np.random.default_rng(11)
    dq = 60 / 108  # a dotted quarter
    e8 = dq / 3
    notes = tune(67, 'mixolydian', 8, 6, r, motif_len=24)
    ev = []
    t = 0.0
    for i, m in enumerate(notes):
        long = (i % 3 == 0) and r.random() < 0.3
        d = e8 * (2 if long else 1)
        ev.append((t, fiddle(midi(m), d * 0.95), 0.32, -0.25))
        if (i // 48) % 2 == 1:
            ev.append((t, whistle(midi(m + 12), d * 0.9, cut=r.random() < 0.15), 0.18, 0.3))
        t += d
    total = t
    bar = dq * 2
    chords = [[55, 59, 62], [53, 57, 60], [55, 59, 62], [50, 54, 57]]  # G F G D
    for b in range(int(np.ceil(total / bar))):
        ch = chords[b % 4]
        for j in (0, 3):
            for k2, m in enumerate(ch):
                ev.append((b * bar + j * e8 + k2 * 0.015, harp(midi(m - 12), 1.2, bright=0.3), 0.14, 0.0))
        for j in range(6):
            ev.append((b * bar + j * e8, bodhran(0.4, accent=1.0 if j in (0, 3) else 0.45), 0.3, 0.1))
    buf, n = render(ev, total)
    return finish(buf, n, MR, 0.18, 1.4, 0.12)


def mus_night():
    """Night: a slow, sparse harp in A aeolian, a soft drone, now and then a held whistle note."""
    r = np.random.default_rng(23)
    beat = 60 / 54
    total = 64 * beat
    ev = [(0, drone([midi(45), midi(52)], int(total * MR)), 0.035, 0.0)]
    for b in range(64):
        if r.random() < 0.7:
            m = scale_note(57, 'aeolian', int(r.integers(0, 9)))
            ev.append((b * beat + r.uniform(0, 0.15), harp(midi(m), 4.5, bright=0.25), 0.2, r.uniform(-0.5, 0.5)))
        if b % 16 == 8:
            ev.append((b * beat, whistle(midi(scale_note(69, 'aeolian', int(r.integers(0, 5)))), beat * 4), 0.12, 0.2))
    buf, n = render(ev, total)
    return finish(buf, n, MR, 0.45, 3.5, 0.08)


def mus_battle():
    """Battle: D phrygian, driving bodhrán eighths, a low drone, a fiddle ostinato and horn calls."""
    r = np.random.default_rng(31)
    beat = 60 / 138
    bars = 24
    total = bars * 4 * beat
    ev = [(0, drone([midi(38), midi(45)], int(total * MR)), 0.09, 0.0)]
    riff = [0, 0, 1, 0, 3, 1, 0, -1]
    for b in range(bars):
        for k in range(8):
            ev.append((b * 4 * beat + k * beat / 2, bodhran(0.35, accent=1.0 if k % 4 == 0 else (0.7 if k % 2 == 0 else 0.4)), 0.4, 0.0))
            deg = riff[k] + (2 if (b // 4) % 2 else 0)
            ev.append((b * 4 * beat + k * beat / 2, fiddle(midi(scale_note(50, 'phrygian', deg + 7)), beat / 2 * 0.9), 0.26, -0.3))
        if b % 4 == 0:
            n = int(beat * 2 * SR)
            tt = np.arange(n) / SR
            horn = sum(np.sin(2 * np.pi * midi(62) * h * tt) / h for h in range(1, 10)) * np.minimum(tt / 0.1, 1) * np.exp(-tt * 0.8)
            horn = signal.resample(horn, int(len(horn) * MR / SR))
            ev.append((b * 4 * beat, horn, 0.07, 0.35))
    buf, n = render(ev, total)
    return finish(buf, n, MR, 0.22, 1.8, 0.13)


def mus_eerie():
    """The eerie: a drone with a tritone in it, harp harmonics at random, and swells that rise out of nothing."""
    r = np.random.default_rng(41)
    total = 56.0
    n = int(total * MR)
    ev = [(0, drone([midi(38), midi(44)], n), 0.06, 0.0)]
    for _ in range(26):
        m = int(r.choice([74, 75, 80, 81, 86, 87, 92]))
        ev.append((r.uniform(0, total), harp(midi(m), 3.0, bright=0.8), 0.07, r.uniform(-0.8, 0.8)))
    for _ in range(5):
        d = r.uniform(5, 8)
        k = int(d * MR)
        tt = np.arange(k) / MR
        sw = choir(k, MR, [midi(int(r.choice([50, 56, 57])))], vowel=(400, 800, 2600)) * (tt / d) ** 2
        ev.append((r.uniform(0, total - d), sw, 0.18, r.uniform(-0.5, 0.5)))
    buf, nn = render(ev, total)
    return finish(buf, nn, MR, 0.55, 5.0, 0.07)


def mus_holy():
    """The holy: voices on slow chords in D, a bell, the harp now and then."""
    total = 64.0
    n = int(total * MR)
    chords = [[50, 57, 62, 66], [48, 55, 60, 64], [43, 50, 59, 62], [45, 52, 57, 61]]
    ev = []
    seg = total / len(chords)
    for i, ch in enumerate(chords):
        k = int((seg + 3) * MR)
        c = choir(k, MR, [midi(m) for m in ch]) * np.sin(np.linspace(0, np.pi, k)) ** 0.7
        ev.append((i * seg, c, 0.5, 0.0))
        ev.append((i * seg + 2, harp(midi(ch[-1] + 12), 4.0, bright=0.3), 0.12, 0.3))
    for t0 in (1.0, 33.0):
        ev.append((t0, bell(midi(62), 6, MR), 0.06, -0.2))
    buf, nn = render(ev, total)
    return finish(buf, nn, MR, 0.55, 4.5, 0.08)


# ------------------------------------------------------------------------------------------------ write it all

if __name__ == '__main__':
    # (path, make, sample rate, quality, own seed). Sounds without their own seed share one random stream in this
    # order, so everything before one being made is made too (and thrown away) to keep it the same sound.
    jobs = []
    for name, fn in [('sea', amb_sea), ('river', amb_river), ('forest', amb_forest), ('night', amb_night), ('wind', amb_wind),
                     ('rain', amb_rain), ('fire', amb_fire), ('town', amb_town), ('market', amb_market), ('crypt', amb_crypt),
                     ('holy', amb_holy), ('battle', amb_battle)]:
        jobs.append((f'ambience/{name}.ogg', fn, SR, 4, False))
    for surface in ('grass', 'road', 'stone', 'water'):
        for i in range(1, 4):
            jobs.append((f'sfx/step_{surface}_{i}.ogg', lambda s=surface: sfx_step(s), SR, 3, False))
    for i in range(1, 3):
        jobs.append((f'sfx/swing_{i}.ogg', sfx_swing, SR, 3, False))
        jobs.append((f'sfx/hit_{i}.ogg', sfx_hit, SR, 3, False))
    for name, fn in [('hurt', sfx_hurt), ('pickup', sfx_pickup), ('drop', sfx_drop), ('place', sfx_place), ('break', sfx_break),
                     ('horn', sfx_horn), ('chime', sfx_chime), ('click', sfx_click), ('splash', sfx_splash)]:
        jobs.append((f'sfx/{name}.ogg', fn, SR, 3, False))
    for name, fn in [('explore', mus_explore), ('town', mus_town), ('night', mus_night), ('battle', mus_battle), ('eerie', mus_eerie), ('holy', mus_holy)]:
        jobs.append((f'music/{name}.ogg', fn, MR, 4, False))
    for i, fx in enumerate(SPELLS):
        jobs.append((f'sfx/cast_{fx}.ogg', seeded(2000 + i, globals()[f'cast_{fx}']), SR, 3, True))
        jobs.append((f'sfx/impact_{fx}.ogg', seeded(3000 + i, globals()[f'impact_{fx}']), SR, 3, True))
    jobs.append(('sfx/heal.ogg', seeded(4000, heal), SR, 3, True))
    jobs.append(('sfx/held.ogg', seeded(4001, held), SR, 3, True))

    want = [(not ONLY or any(o in p for o in ONLY)) and (FORCE or not os.path.exists(os.path.join(OUT, p)))
            and p not in RECORDED for p, *_ in jobs]
    last_shared = max((i for i, (w, j) in enumerate(zip(want, jobs)) if w and not j[4]), default=-1)
    for i, (path, fn, sr, q, own) in enumerate(jobs):
        if want[i]:
            write(path, fn(), sr, q)
        elif not own and i < last_shared:
            fn()
