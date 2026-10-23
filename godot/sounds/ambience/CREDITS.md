# Ambience credits

Every loop here is synthesised by `scripts/make_audio.py` (same licence as this repository) except the one below.

| File | Title | Author | Source | Licence |
|---|---|---|---|---|
| `shore.ogg` | Ocean Waves on a Tropical Beach | J.D. Savanyu (uploaded to Wikimedia Commons as own work by Jarrod stanley) | <https://commons.wikimedia.org/wiki/File:Ocean_Waves_on_a_Tropical_Beach.ogg> | CC0 1.0 (<https://creativecommons.org/publicdomain/zero/1.0/>) |

`shore.ogg` is a 95.75 s excerpt (18.0 s to 117.75 s of the 6 min 24 s original, cut at wave troughs) with a 25 Hz
high-pass, its last 4 s crossfaded into its start so it loops, turned down 6.3 dB to sit at about -17.5 LUFS (next to
`sea.ogg`, -17.3 LUFS) and encoded as Ogg Vorbis 44.1 kHz stereo (quality 4). Downloaded 2026-10-23.

## sea.ogg

The distant sea is the same recording ("Ocean Waves on a Tropical Beach", J.D. Savanyu, CC0 1.0, above), low-passed
at 700-900 Hz and levelled to about -19 LUFS, so the open sea is a soft rumble and the surf's hiss belongs to the
shore layer near the edge. It replaces the synthesised sea `scripts/make_audio.py` made (which skips files that are
already there, so rerunning it keeps this one).

## rain.ogg

Still the synthesised rain from `scripts/make_audio.py`, low-passed at 3.5 kHz and high-passed at 120 Hz to take the
white-noise edge off it.
