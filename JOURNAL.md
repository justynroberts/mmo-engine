# Improvement journal

Things found by playing, testing and measuring that would make the game better, kept as they turn up. `ROADMAP.md`
is the feature plan; this is the running list of fixes and polish beside it. "Continue the roadmap" means: take the
top open item here (or the next phase of `ROADMAP.md` when this is empty), do it with tests, deploy it, and move it
to Done with the date.

Each item: what is wrong or missing, why it matters to a player, and where to start. Ordered by what a player meets
first.

## Open

### Play-testing reach
1. **Drive a real client session.** The quest content is now played through by the server test
   `hibernia_quests_play_through` (talk, take, beat or fetch, hand in), but not through the network and a client:
   nobody has scripted a fight, falling and rising, or the Godot windows. A Godot `--autoplay`, or `bots --play`
   reporting quests finished and deaths from the QUESTS and PROGRESS packets it now skips.
2. **The play-through skips walking and fighting.** It counts how far each quest sends a player and how many kills
   it takes, but does not ask whether a level-N character wins those fights. Add each foe's level against the
   player's when it is beaten, and flag fights more than a few levels up.

### Content
3. **Two Hibernia quests want the one weekly heart.** "The Stone that Roars" and "The Answerer" both take Crom
   Dubh's heart (boss spoils, once a week), so the second waits a week more; two quests take troll hides as well.
   Give one of each pair a different trophy.
4. **Hunting grounds are placed by farms and villages, or inland where there are none, not by terrain.** Check
   them against the world file's sea, lakes and slopes, or have worldgen place them.
5. **Travel on a 65 km map.** Long journeys start at level 6-12 and a pony comes at level 2, but crossing the
   country still takes the better part of an hour. Carters as paid travel between towns (like `/sail` fares), or
   roads faster still for a rider.
6. **Characters saved before 2026-11-22 under older curves** were not given their level back (saves only now keep
   the level). Only test characters are affected.

### Client
7. **Windows client is unsigned**, so Windows warns before running it.

### Server and performance
8. **A console save that re-homes bots holds the sim about 0.7 s** (placing ~3,000 bots). Spread respawns over a
   few ticks, or prepare them off the sim thread as reloaded worlds are.
9. **Load test from a cloud machine.** The home link loses snapshots past ~1,000 bots; the live server's ceiling
   with 4 lanes is unmeasured past 2,000. Costs a cloud machine for an hour: ask first.

## Done

- 2026-11-22 — A play-through of every quest offered in Áth Cliath as a server test
  (`hibernia_quests_play_through`); it found a giver offering only a repeatable quest once done (now a quest never
  done comes first) and a level curve that left a questing player stuck at 4 (refitted to 500 x 1.6: all 61 quests,
  level 20).
- 2026-11-22 — Deer and stags in a wood by each town; hunting grounds for every town, inland where no farm or
  village is near (Derry, Coleraine); quest steps for dungeon foes point at the dungeon's door; the starter quests'
  words no longer name Áth Cliath in other towns, and its Teamhair journeys are offered only there.
- 2026-11-22 — A save keeps the character's level, so a steeper curve gives back the experience instead of taking
  levels; the world map leaves out waymarks' names (eleven crowded Áth Cliath's); `in_safe` uses the place index;
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
