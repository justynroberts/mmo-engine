# Effects credits

The spell sounds (`cast_*`, `impact_*`, `heal`, `held`) are synthesised by `scripts/make_audio.py` (same licence as
this repository). Everything else here is a recording: the one-shot effects in the first table and the incidentals,
`amb_<name>_<n>.ogg`, which `scripts/sound.gd` plays now and then in towns and by farms, in the second.
`make_audio.py` lists the recorded effects in `RECORDED` and never overwrites them.

## Effects

Each take is one event cut from the source (inside the seconds given, trimmed to the sound), high-passed at 50 Hz
(120 Hz for water), faded in 3 ms and out with a cosine over the last 0.06-0.6 s, mixed to mono and levelled to the
momentary loudness (EBU R128, 400 ms) of the synthesised sound it replaced, every take of a family to the same
loudness, peaks at or under -1 dBFS (a look-ahead limiter takes off at most 6 dB on transients), encoded as Ogg Vorbis
44.1 kHz quality 4. Grass, road and stone steps are low-passed at 9, 10 and 12 kHz, water steps at 8 kHz with light FFT
denoise (ffmpeg `afftdn`), the splash at 10 kHz. Freesound sources were taken from the site's high-quality previews.
Downloaded 2026-10-25.

| File | Excerpt | Title | Author | Source | Licence |
|---|---|---|---|---|---|
| `step_grass_1..6.ogg` | the six steps, 0.0-4.1 s | Hiking Boot Footsteps on Grass | Fission9 | <https://freesound.org/people/Fission9/sounds/521587/> | CC0 1.0 |
| `step_road_1..4.ogg` | the four steps, 0.0-2.7 s | Hiking Boot Footsteps on Gravel | Fission9 | <https://freesound.org/people/Fission9/sounds/521588/> | CC0 1.0 |
| `step_stone_1..6.ogg` | the six steps, 0.0-3.4 s | Hiking Boot Footsteps on Stone | Fission9 | <https://freesound.org/people/Fission9/sounds/521590/> | CC0 1.0 |
| `step_water_1..6.ogg` | 0.42, 3.92, 5.62, 9.3, 10.27, 11.04 s (0.45 s each) | Water Wading.wav | Motion_S | <https://freesound.org/people/Motion_S/sounds/221764/> | CC BY 4.0 (<https://creativecommons.org/licenses/by/4.0/>) |
| `swing_1.ogg` | whole | Sword Swing 2 | Nomagician | <https://freesound.org/people/Nomagician/sounds/840715/> | CC0 1.0 |
| `swing_2.ogg` | whole | Sword Swing 1 | Nomagician | <https://freesound.org/people/Nomagician/sounds/840716/> | CC0 1.0 |
| `swing_3.ogg` | whole | Sword Swing 3 | Nomagician | <https://freesound.org/people/Nomagician/sounds/840717/> | CC0 1.0 |
| `hit_1..4.ogg` | whole | `impactPunch_heavy_000`, `_001`, `_003`, `_004` from Impact Sounds | Kenney | <https://kenney.nl/assets/impact-sounds> | CC0 1.0 |
| `hurt.ogg` | whole | Voice_AdultMale_PainGrunts_03.wav | MrFossy | <https://freesound.org/people/MrFossy/sounds/547201/> | CC0 1.0 |
| `pickup.ogg` | whole | `handleSmallLeather` from RPG Audio | Kenney | <https://kenney.nl/assets/rpg-audio> | CC0 1.0 |
| `drop.ogg` | whole | `dropLeather` from RPG Audio | Kenney | <https://kenney.nl/assets/rpg-audio> | CC0 1.0 |
| `place.ogg` | 5.3-5.85 s | two large cobblestone blocks | jobel0092 | <https://freesound.org/people/jobel0092/sounds/268055/> | CC0 1.0 |
| `break.ogg` | 0.0-1.0 s | crate_break4 | K-Forces | <https://freesound.org/people/K-Forces/sounds/856631/> | CC0 1.0 |
| `horn.ogg` | 0.1-3.9 s (the first blast) | medieval viking horn | Artiom_Constantinov | <https://freesound.org/people/Artiom_Constantinov/sounds/856540/> | CC0 1.0 |
| `chime.ogg` | 0.4-2.4 s | Small Bell | NachtmahrTV | <https://freesound.org/people/NachtmahrTV/sounds/553214/> | CC0 1.0 |
| `splash.ogg` | 5.5-6.1 s | Splashing Stomps Impacts Shallow Water | ChristopherJngs | <https://freesound.org/people/ChristopherJngs/sounds/861370/> | CC0 1.0 |
| `click.ogg` | the first click | `metalClick` from RPG Audio | Kenney | <https://kenney.nl/assets/rpg-audio> | CC0 1.0 |

The CC BY recording (`step_water_1..6`) needs this attribution wherever the game's credits are shown:
"Water Wading" by Motion_S (CC BY 4.0).

## Incidentals

Each is a short excerpt of the source (the seconds given), high-passed at 70 Hz, faded in 12 ms and out over the last
fifth (at most 0.35 s), mixed to mono, levelled to about -24 LUFS integrated with peaks at or under -6 dBFS, and
encoded as Ogg Vorbis 44.1 kHz (quality 3). Where noted, light FFT denoise (ffmpeg `afftdn`), a gentle compressor
(-30 dB, 3:1) or a 2.5 kHz low-pass was applied as well. Freesound sources were taken from the site's high-quality
previews. Downloaded 2026-10-23.

| File | Excerpt | Title | Author | Source | Licence |
|---|---|---|---|---|---|
| `amb_anvil_1.ogg` | 0.0-3.1 s | Hammering on Anvil | SamsterBirdies | <https://freesound.org/people/SamsterBirdies/sounds/363169/> | CC0 1.0 |
| `amb_anvil_2.ogg` | 6.5-9.9 s | Hammering on Anvil | SamsterBirdies | <https://freesound.org/people/SamsterBirdies/sounds/363169/> | CC0 1.0 |
| `amb_anvil_3.ogg` | 10.2-13.4 s | Hammering on Anvil | SamsterBirdies | <https://freesound.org/people/SamsterBirdies/sounds/363169/> | CC0 1.0 |
| `amb_anvil_4.ogg` | 77.2-81.6 s | Blacksmith's anvil | Work With Sounds / La Fonderie | <https://commons.wikimedia.org/wiki/File:WWS_Blacksmith8217sanvil.ogg> | CC BY 4.0 (<https://creativecommons.org/licenses/by/4.0/>) |
| `amb_anvil_5.ogg` | 86.0-90.8 s, compressed | Blacksmith's anvil | Work With Sounds / La Fonderie | <https://commons.wikimedia.org/wiki/File:WWS_Blacksmith8217sanvil.ogg> | CC BY 4.0 (<https://creativecommons.org/licenses/by/4.0/>) |
| `amb_cow_1.ogg` | 0.05-2.7 s | Cow mooing in south of France (Limousin) | felix.blume | <https://freesound.org/people/felix.blume/sounds/163727/> | CC0 1.0 |
| `amb_cow_2.ogg` | 3.9-6.4 s, denoised | Cow moo #8 | spurioustransients | <https://freesound.org/people/spurioustransients/sounds/513565/> | CC0 1.0 |
| `amb_sheep_1.ogg` | 0.45-1.7 s | Sheep bleating (clipped from "Corner of a sheep field in summer", PDSounds) | earthcalling | <https://commons.wikimedia.org/wiki/File:Sheep_bleating.ogg> | Public domain |
| `amb_sheep_2.ogg` | 5.95-7.7 s | Sheep bleating (as above) | earthcalling | <https://commons.wikimedia.org/wiki/File:Sheep_bleating.ogg> | Public domain |
| `amb_sheep_3.ogg` | whole | Sheep baaing 5 - Norwegian sheep expressing itself concisely | michaelperfect | <https://freesound.org/people/michaelperfect/sounds/710300/> | CC0 1.0 |
| `amb_goat_1.ogg` | 0.9-2.5 s, denoised | Bleating Goat | aisselles | <https://freesound.org/people/aisselles/sounds/722658/> | CC0 1.0 |
| `amb_goat_2.ogg` | 11.5-13.3 s, denoised | Bleating Goat | aisselles | <https://freesound.org/people/aisselles/sounds/722658/> | CC0 1.0 |
| `amb_goat_3.ogg` | 4.9-6.37 s | S034_Goat_Bleeting_Mono.wav | Pól | <https://freesound.org/people/P%C3%B3l/sounds/385913/> | CC0 1.0 |
| `amb_rooster_1.ogg` | whole | Rooster Crow 1 | BenjaminNelan | <https://freesound.org/people/BenjaminNelan/sounds/435508/> | CC0 1.0 |
| `amb_rooster_2.ogg` | 0.5-3.4 s | Rooster - single cry.wav | luckylittleraven | <https://freesound.org/people/luckylittleraven/sounds/268018/> | CC0 1.0 |
| `amb_rooster_3.ogg` | 3.0-4.9 s | Small rooster crowing (PDSounds) | alys | <https://commons.wikimedia.org/wiki/File:Small_rooster_crowing.ogg> | Public domain |
| `amb_hens_1.ogg` | whole | chicken sound - clucking.wav | chanyatenn | <https://freesound.org/people/chanyatenn/sounds/424076/> | CC0 1.0 |
| `amb_hens_2.ogg` | 0.0-4.0 s | Hen announcing she's lain an egg (PDSounds) | alys | <https://commons.wikimedia.org/wiki/File:Hen_announcing_shes_lain_an_egg.ogg> | Public domain |
| `amb_hens_3.ogg` | 6.0-10.0 s | Chickens demanding food (PDSounds) | alys | <https://commons.wikimedia.org/wiki/File:Chickens_demanding_food.ogg> | Public domain |
| `amb_horse_1.ogg` | 0.3-4.2 s | horse's whinny | Kubuzz | <https://freesound.org/people/Kubuzz/sounds/347036/> | CC0 1.0 |
| `amb_horse_2.ogg` | 4.6-6.6 s | Whinnying horse-far away.WAV | L.Finck | <https://freesound.org/people/L.Finck/sounds/636554/> | CC0 1.0 |
| `amb_horse_3.ogg` | whole | Wiehern | Hü. | <https://commons.wikimedia.org/wiki/File:Wiehern.ogg> | Public domain |
| `amb_horse_4.ogg` | 8.1-9.0 s, compressed | horse snort.wav | bruno.auzet | <https://freesound.org/people/bruno.auzet/sounds/538697/> | CC0 1.0 |
| `amb_pig_1.ogg` | 0.3-2.7 s, compressed | Pig grunts | gerainsan | <https://freesound.org/people/gerainsan/sounds/421734/> | CC0 1.0 |
| `amb_pig_2.ogg` | 10.5-12.6 s, compressed | Pig grunts | gerainsan | <https://freesound.org/people/gerainsan/sounds/421734/> | CC0 1.0 |
| `amb_pig_3.ogg` | whole | Pig grunt | erdie (via Freesound) | <https://commons.wikimedia.org/wiki/File:Pig_grunt_-_Erdie.ogg> | CC BY 3.0 (<https://creativecommons.org/licenses/by/3.0/>) |
| `amb_dog_1.ogg` | 5.2-6.4 s, denoised | Rural Distant Dog barks | Sadiquecat | <https://freesound.org/people/Sadiquecat/sounds/737196/> | CC0 1.0 |
| `amb_dog_2.ogg` | whole, low-passed, compressed | Dog barking mono | HaelDB | <https://opengameart.org/content/dog-barking-mono> | CC0 1.0 |
| `amb_dog_3.ogg` | whole, denoised | Dogs Barking in Distance_Rural.wav | rvandemark | <https://freesound.org/people/rvandemark/sounds/581478/> | CC0 1.0 |

CC0 1.0: <https://creativecommons.org/publicdomain/zero/1.0/>. "Public domain" files are PDSounds recordings released
to the public domain by their recordists, as stated on each Wikimedia Commons file page.

The CC BY recordings (`amb_anvil_4`, `amb_anvil_5`, `amb_pig_3`) need this attribution wherever the game's credits are shown:
"Blacksmith's anvil" by Work With Sounds / La Fonderie (CC BY 4.0) and "Pig grunt" by erdie (CC BY 3.0).
