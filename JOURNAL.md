# Improvement journal

Things found by playing, testing and measuring that would make the game better, kept as they turn up. `ROADMAP.md`
is the feature plan; this is the running list of fixes and polish beside it. "Continue the roadmap" means: take the
top open item here (or the next phase of `ROADMAP.md` when this is empty), do it with tests, deploy it, and move it
to Done with the date.

Each item: what is wrong or missing, why it matters to a player, and where to start. Ordered by what a player meets
first.

## Open

### Play-testing reach
1. **Make the autoplay a better player.** `--autoplay` (Godot, `scripts/autoplay.gd`) plays through the real client
   and network, but it cannot farm (crops for the bring-N quests), build, or choose quests as a person would, so a
   15-minute session does a handful of quests. Teach it to plant from the bag and harvest, lay the plot blueprint,
   sell what it carries, and take on fewer quests at once.
2. **A newcomer is offered 11 quests in a minute.** In Áth Cliath's square the watch, the traders and the craftsfolk
   all have work at once, most of it bring-N of crops the player cannot grow yet or goods they cannot afford (Herbs
   for the Sick: 16 pennies of herbs, a 10-penny purse). Hold the craft errands behind "A New Arrival" and "The First
   Furrow", or give the first ones coin.

### Content
3. **Two Hibernia quests want the one weekly heart.** "The Stone that Roars" and "The Answerer" both take Crom
   Dubh's heart (boss spoils, once a week), so the second waits a week more; two quests take troll hides as well.
   Give one of each pair a different trophy.
4. **The north is dangerous country, towns and all.** Doire, Cúil Raithin and Béal Feirste sit inside the
   north's great regions (8 km across), where fomorians, snow trolls and white wolves (levels 3-16) live; a
   newcomer carried there by a stone meets them in the hunting grounds. Keep the hunting grounds clear of them, or
   the stones to the north for higher levels.
5. **Characters saved before 2026-11-22 under older curves** were not given their level back (saves only now keep
   the level). Only test characters are affected.

### Client
- **Windows: an install only an administrator can read fails as "not designed to run on Windows".** A player's copy
  ran only as admin (the folder extracted or updated elevated, or under Program Files), and the self-update copies
  into the folder the game runs from. On start, check the folder can be written and its files read, and if not say
  to move it to the user's own folder (`%LOCALAPPDATA%`) or reset its permissions (`icacls <dir> /reset /T`).
6. **Windows client is unsigned**, so Windows warns before running it.

### Server and performance
- **`a_player_on_a_road_covers_more_ground_than_one_on_grass` fails when the machine is busy** (it measures ground
  covered in real time); it passes alone. Measure in server ticks instead.
7. **Load test from a cloud machine.** The home link loses snapshots past ~1,000 bots; the live server's ceiling
   with 4 lanes is unmeasured past 2,000. Costs a cloud machine for an hour: ask first.

## Done

- 2026-11-22 — A bolder land: worldgen's `drama` (Hibernia 2) lifts the mountains to about 260 with ridged crests
  and lays upland hills, long valleys and hollows over the open country, fading out round towns, roads, rivers,
  fields and the shore so nothing laid out moves; the ground under players' builds is held (`keep`). The hunting
  grounds stay gentle (steepest 0.19 of the 0.2 allowed).

- 2026-11-22 — Resting: out of a fight, health and mana come back (2.5% a second on Hibernia, from 6 s after the
  last blow, twice as fast sitting); health never came back at all before, but by potion or a fall.

- 2026-11-22 — Fights as a group game: every bot keeps a hate list (damage, half of heals, taunts; it turns on
  whoever passes the one it is on by a tenth and goes for the next when they fall or get away, so a tank holds it
  and a healer draws it); a beast that starts a fight calls its kind within a few strides (`rules.assist_reach`,
  25 on Hibernia: pull one wolf and its pack comes); a fighter five levels under a player leaves them be
  (`rules.calm_levels`); names and the target frame coloured by level against yours, with a word for the fight
  ("A deadly fight" to "No threat to you").

- 2026-11-22 — Travel stones carry only a player who stands on one (`rules.exit_dwell_secs`, 2 s on Hibernia) and
  say where they go: walking Áth Cliath's square carried the autoplay to Droichead Átha and back four times in ten
  seconds; and Áth Cliath's stones moved from round the castle walls (where the market folk stand, so a player
  stopping to buy was carried off) to the edge of its square, each on the side of the town it goes to. The tracker
  points to the one to speak to when they are in sight, not to their town's middle (finding the
  seed trader took four minutes).
- 2026-11-22 — `--autoplay SECONDS` in the Godot client: plays as a new player (class, the nearest "!", the
  tracker's goals, buying from traders, fighting, rising, handing in) through the real network, and prints what
  happened. It found traders named as places to fetch from (now "bring N" steps point at the nearest seller or
  dropper), the travel stones, and the towns below.
- 2026-11-22 — Hostile beasts no longer live on top of towns (`clear_towns`): Sligeach had a level-13 gruagach,
  a bear, a púca and wisps in its square, Cathair na Mart, Dún Dealgan, Ceatharlach and Cill Airne others.
- 2026-11-22 — Hunting grounds checked against the world file (`Terrain`: dry land, gentle ground; one was
  under water, six a quarter to half wet); the play-through gives each quest's strongest foe against the player's
  level (none more than two up) and pays for what it buys from a purse; a console save that changes no bot type's
  count or homes leaves every bot where it is (it respawned all 19,000 each time). Travel already exists: the
  stones in Áth Cliath's square (the world map names them again).

- 2026-11-22 — A play-through of every quest offered in Áth Cliath as a server test
  (`hibernia_quests_play_through`); it found a giver offering only a repeatable quest once done (now a quest never
  done comes first) and a level curve that left a questing player stuck at 4 (refitted to 500 x 1.6: all 61 quests,
  level 20).
- 2026-11-22 — Deer and stags in a wood by each town; hunting grounds for every town, inland where no farm or
  village is near (Derry, Coleraine); quest steps for dungeon foes point at the dungeon's door; the starter quests'
  words no longer name Áth Cliath in other towns, and its Teamhair journeys are offered only there.
- 2026-11-22 — A save keeps the character's level, so a steeper curve gives back the experience instead of taking
  levels; `in_safe` uses the place index;
  console UI tests given time on a busy machine. `/play/info` was already gzipped by the proxy (69 KB sent).

- 2026-11-22 — Quest steps name every kind that serves them and `/play/info` gives all their homes, so markers point
  to the nearest (`kinds`, `homes_all`); collect steps get markers from the bots that drop the item.
- 2026-11-22 — A console edit that changes a bot type's homes places its bots again.
- 2026-11-22 — Hunting grounds inland of each town for foxes, hares, badgers, boars and bulls; fox pelts and the
  shepherd's pelt quests; every town a place to rise, the wolf wood not; a quest no lower than the one it follows;
  the Wexford fisher's journey level 6.
- 2026-11-22 — New players arrive in Áth Cliath; quests can be tied to a town (`offered_at`); early quests sized to
  the 65,536 map and journeys held to higher levels; level curve refitted; starter sword and tunic; arrival square
  uncrowded; tutorial no longer sends players at townsfolk; the world map useful on a big world; console saves
  0.3-0.5 s instead of 2-3 s; holly, timber and milk obtainable; spell bar by class.
