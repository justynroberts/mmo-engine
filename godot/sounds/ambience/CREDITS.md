# Ambience credits

Every loop here is a real recording, not synthesised. `scripts/make_audio.py` never writes any of them, not even
with `--force` (its `RECORDED` set).

**CC-BY files need attribution in the game's credits.** These are `river.ogg`, `rain.ogg`, `town.ogg`, `market.ogg`,
`crypt.ogg` and `holy.ogg` (marked **CC-BY** below). A line like "<title>" by <author> (<source URL>), licensed under
CC BY 4.0 (<https://creativecommons.org/licenses/by/4.0/>), edited (excerpted, filtered, looped, levelled) will do.
CC0 sources need no credit, but they are listed here too.

| File | Title | Author | Source | Licence |
|---|---|---|---|---|
| `shore.ogg`, `sea.ogg` | Ocean Waves on a Tropical Beach | J.D. Savanyu (uploaded to Wikimedia Commons as own work by Jarrod stanley) | <https://commons.wikimedia.org/wiki/File:Ocean_Waves_on_a_Tropical_Beach.ogg> | CC0 1.0 (<https://creativecommons.org/publicdomain/zero/1.0/>) |
| `river.ogg` | riviere-river.ogg | Glaneur de sons | <https://freesound.org/people/Glaneur%20de%20sons/sounds/24511/> | **CC-BY** 4.0 (<https://creativecommons.org/licenses/by/4.0/>) |
| `forest.ogg` | Dawn Chorus Birdsong | squashy555 | <https://freesound.org/people/squashy555/sounds/573080/> | CC0 1.0 (<https://creativecommons.org/publicdomain/zero/1.0/>) |
| `night.ogg` | AMB_Rural_Summer_Night.wav | AnthonyRamirez | <https://freesound.org/people/AnthonyRamirez/sounds/425425/> | CC0 1.0 (<https://creativecommons.org/publicdomain/zero/1.0/>) |
| `night.ogg` (owl) | CricketsandTawnyOwl.wav | raoul_slayer | <https://freesound.org/people/raoul_slayer/sounds/203598/> | CC0 1.0 (<https://creativecommons.org/publicdomain/zero/1.0/>) |
| `wind.ogg` | Wind singing in the mountain, some night cricket in background | felix.blume | <https://freesound.org/people/felix.blume/sounds/165526/> | CC0 1.0 (<https://creativecommons.org/publicdomain/zero/1.0/>) |
| `rain.ogg` | light forest rain.wav | tim.kahn | <https://freesound.org/people/tim.kahn/sounds/169031/> | **CC-BY** 4.0 (<https://creativecommons.org/licenses/by/4.0/>) |
| `fire.ogg` | Campfire 02 | HECKFRICKER | <https://freesound.org/people/HECKFRICKER/sounds/729396/> | CC0 1.0 (<https://creativecommons.org/publicdomain/zero/1.0/>) |
| `town.ogg` | Sehusa Fest Medieval Crowd.wav | inchadney | <https://freesound.org/people/inchadney/sounds/40133/> | **CC-BY** 4.0 (<https://creativecommons.org/licenses/by/4.0/>) |
| `town.ogg` (cart) | G52-05-Ox Cart.wav | craigsmith | <https://freesound.org/people/craigsmith/sounds/437078/> | CC0 1.0 (<https://creativecommons.org/publicdomain/zero/1.0/>) |
| `town.ogg` (horses) | R23-54-Horse Carriage in Distance.wav | craigsmith | <https://freesound.org/people/craigsmith/sounds/479797/> | CC0 1.0 (<https://creativecommons.org/publicdomain/zero/1.0/>) |
| `market.ogg` | Crowds.wav | Metzik | <https://freesound.org/people/Metzik/sounds/244623/> | **CC-BY** 4.0 (<https://creativecommons.org/licenses/by/4.0/>) |
| `crypt.ogg` | Drips In Mine.wav | Benboncan | <https://freesound.org/people/Benboncan/sounds/116730/> | **CC-BY** 4.0 (<https://creativecommons.org/licenses/by/4.0/>) |
| `crypt.ogg` (drone) | Dungeon Air | Flamiffer | <https://freesound.org/people/Flamiffer/sounds/530161/> | CC0 1.0 (<https://creativecommons.org/publicdomain/zero/1.0/>) |
| `holy.ogg` | R4_00197v2_FR_CatholicMonks.wav | kevp888 | <https://freesound.org/people/kevp888/sounds/575757/> | **CC-BY** 4.0 (<https://creativecommons.org/licenses/by/4.0/>) |
| `battle.ogg` | Vikings in battle (swords crossing, shields bashing, men yelling) | DeadVDI | <https://freesound.org/people/DeadVDI/sounds/376646/> | CC0 1.0 (<https://creativecommons.org/publicdomain/zero/1.0/>) |
| `tavern.ogg` | IRISH PUB the cobblestone.WAV | traggeldy | <https://freesound.org/people/traggeldy/sounds/368830/> | CC0 1.0 (<https://creativecommons.org/publicdomain/zero/1.0/>) |
| `fair.ogg` | French Folk Dance.mp3 | DenisTr | <https://freesound.org/people/DenisTr/sounds/267912/> | CC0 1.0 (<https://creativecommons.org/publicdomain/zero/1.0/>) |

The Freesound files were taken from Freesound's high-quality previews (Ogg, 44.1/48 kHz) on 2026-10-25, with the
licence checked on each sound's page.

## How each loop was made

Every loop is an excerpt whose last few seconds are crossfaded (equal power) into its start so it loops without a
seam, filtered, levelled to the integrated loudness (EBU R128) of the synthesised file it replaced (`AMB_DB` in
`godot/scripts/sound.gd` then sets how loud each plays), limited only where a peak would pass -2 dBFS, and encoded as Ogg Vorbis 44.1 kHz
stereo (quality 3).

| File | Excerpt of the source | Crossfade | Filters | Loudness |
|---|---|---|---|---|
| `river.ogg` | 3.5-56.5 s (49 s loop) | 4 s | high-pass 50 Hz | -17.5 LUFS |
| `forest.ogg` | 300-390 s (85 s) | 5 s | 80 Hz-11 kHz | -19.2 LUFS |
| `night.ogg` | crickets 4-98 s (88 s); two tawny owl calls (29.5 s and 87 s of the owl recording, band-passed 600-1250 Hz and gated so only the hoots come through) laid in at 18 s and 62 s | 6 s | 60 Hz-9 kHz | -22.7 LUFS |
| `wind.ogg` | 10-98 s (80 s) | 8 s | high-pass 40 Hz | -19.7 LUFS |
| `rain.ogg` | 200-286 s (80 s) | 6 s | 100 Hz-6 kHz | -16.8 LUFS |
| `fire.ogg` | 30-109 s (75 s) | 4 s | high-pass 40 Hz | -28.7 LUFS |
| `town.ogg` | crowd 8-99 s (85 s), high-passed at 150 Hz to drop a low hum; ox cart (6-26 s) at 15 s and distant horses on cobbles (4.2-19 s, after the slate) at 55 s, both low-passed at 3.5 kHz and well down in the mix | 6 s | 150 Hz-9 kHz | -18.9 LUFS |
| `market.ogg` | 222-316 s (88 s), the medieval marketplace part | 6 s | 80 Hz-10 kHz | -18.5 LUFS |
| `crypt.ogg` | drips 60-145 s with the drone (30-115 s) under it at equal gain (80 s) | 5 s | 30 Hz-9 kHz | -24.1 LUFS |
| `holy.ogg` | 42.3-108.3 s (65 s), cut in the pauses between chanted verses | 1 s | 70 Hz-7 kHz | -20.4 LUFS |
| `shore.ogg` | 18.0-117.75 s (95.75 s, cut at wave troughs) | 4 s | high-pass 25 Hz | -17.5 LUFS (quality 4) |
| `sea.ogg` | the same recording, so the open sea is a soft rumble and the surf's hiss belongs to the shore layer | | low-pass 700-900 Hz | about -19 LUFS |
| `battle.ogg` | 131-204 s (68 s), after the last edit in the source; low-passed at 3.2 kHz with a short echo and a compressor so it sounds distant | 5 s | high-pass 80 Hz | -20.1 LUFS |
| `tavern.ogg` | 115-166 s (48 s), the stretch where the session's tune is clearest over the talk | 3 s | none | -20 LUFS |
| `fair.ogg` | 0-39 s (36 s), musicians playing outdoors in Chinon | 3 s | none | -20 LUFS |
