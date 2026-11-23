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

- **The autoplay cannot find its way.** It walks straight at a goal, sidesteps, and (since 2026-11-23) looks along
  a wall for a gap from the world file; it still loses minutes to the Smith in his forge, the Miller indoors and
  the city walls. 2026-11-23 runs reached level 2-3 in 30 minutes (the server play-through reaches level 10 at
  about the thirtieth quest). Give it a path: the flow fields the server builds for bots, or A* over the walls
  by tile the client already has (`/play/tiles`).

### Content
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

- 2026-11-23 — Quest folk keep their posts: the Seed Trader (A New Arrival, Deer for the Pot, the Shepherd's
  Errand all end with them) went indoors at dusk, by night and in the rain, out of sight, so half the newcomer
  quests could not be finished after dark. Givers, those quests are handed in to or name to speak to, and traders
  now skip `indoors` (`Sim::keeps_post`). The autoplay got its way about: progress towards a goal (not just
  movement) decides it is stuck, a wider search round a long wall (the city's west wall is 300 units), built
  blocks counted as walls, one quarry chased at a time, no pulls into a pack, kills counted only when they pay.

- 2026-11-23 — The Answerer takes a Fomorian champion's bone instead of Crom Dubh's heart (both it and The Stone
  That Roars wanted the one weekly heart; troll hides and wyrm teeth drop from ordinary beasts, so sharing them
  is fine). Travel stones can want a level (`exit.level`); the one to Doire, in the north's great region of
  Fomorian champions and snow trolls, wants 10 (`gate_stones`).

- 2026-11-23 — Hunting made a player a villain: every bot with a home and no weapon counted as a townsperson, so
  each fox, hare or deer beaten was a murder in its hunting ground (which counts as a town, farm buildings in it),
  carried to Áth Cliath at a third, and the watch came for the hunter (found by the play test: shunned in a dozen
  places after "Foxes at the Fold"). Game (a bot type that pays experience) is not townsfolk now; stags no longer
  brawl in villages either.

- 2026-11-23 — Hunting grounds brought in: small game a median 500 units from its town (was 1,467; Áth Cliath's
  600, was 1,373), deer 800 (was 3,405), boars and bulls 1,100 (was 3,524), each the nearest dry, gentle ground
  clear of buildings. A console edit that moves a bot type's home places its bots again (only a change of name
  did).

- 2026-11-22 — A play test to level 10 (autoplay, live content and land). Found and fixed: a quest ready to hand in
  stayed "ready" after another quest's hand-in took its goods (the giver refused it for good; it goes back to the
  collect step now); a newcomer's purse (10 pennies) could not pay for one errand's goods, and errands paid less
  than their goods cost (40 pennies, and pay half again over cost); beasts paid 10-34 experience against 16,800 for
  level 10 (now by level, 10 x level^1.5: a boar 80, a bull 112); night ambushes sent any of six kinds up to
  level 14 in packs of three at a level-2 player (now their match, one at a time early); the seed trader worked the
  fields where nobody could find them (now the square); the curve's first levels were slow (250 x 1.9: level 3 by
  the fourth quest). The autoplay hunts quest beasts when it has nothing near to do, rests when hurt, and leaves
  deadly foes be.

- 2026-11-22 — A bolder land: worldgen's `drama` (Hibernia 2) lifts the mountains to about 260 with ridged crests
  and lays upland hills, long valleys and hollows over the open country, fading out round towns, roads, rivers,
  fields and the shore so nothing laid out moves; the ground under players' builds is held (`keep`). The hunting
  grounds stay gentle (steepest 0.19 of the 0.2 allowed). The first deploy's keep boxes also took away the
  lesser relief under the builds, so the server re-seated 790 columns and dropped 512; worldgen fixed (only the
  bold relief is held back), the world made again, and every block put back from the export taken before
  (`POST /api/blocks/restore`: all 3,218, owners included). Before changing land under a live world, export
  `/api/blocks` first.

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
