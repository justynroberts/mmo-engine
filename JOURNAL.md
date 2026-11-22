# Improvement journal

Things found by playing, testing and measuring that would make the game better, kept as they turn up. `ROADMAP.md`
is the feature plan; this is the running list of fixes and polish beside it. "Continue the roadmap" means: take the
top open item here (or the next phase of `ROADMAP.md` when this is empty), do it with tests, deploy it, and move it
to Done with the date.

Each item: what is wrong or missing, why it matters to a player, and where to start. Ordered by what a player meets
first.

## Open

### Play-testing reach
1. **Drive a real session end to end.** Talking to a giver, taking and handing in quests, and a fight are covered
   only by unit and e2e tests; the Godot `--shot` flags can't talk or fight. Either teach `bots --play` to report
   quests finished and deaths per bot against the live content (it already talks, accepts and casts), or add a
   Godot `--autoplay` that walks the starter chain and logs each step. Start: `crates/bots/src/main.rs` `--play`.
2. **Starter experience outside Áth Cliath.** The early quests were checked from Áth Cliath only. Survey each town
   for what a level 1-3 player is offered there (givers that live in only one town, like the Wexford fisher, give
   nothing elsewhere). Start: `scripts/hibernia_content.py`, the audit approach (quests by giver home and distance).

### Content
3. **Deer have no home.** 2,600 deer roam the whole map, so "Deer for the Pot" and "A Hunter's Living" get no
   marker. Give them woodland homes (forest places, or the hunting grounds' edges).
4. **Hunting grounds are placed by farms and villages, not terrain.** `hunting_grounds` picks a spot inland of each
   town; check against the world file's sea, lakes and slopes (or have worldgen place them), and give the 4 towns
   that got fewer than two grounds theirs.
5. **Dungeon-only quest items have no marker, and some need weeks.** "The Crows of the Morrígan" wants 6 raven
   feathers, which come from Uaimh Chruachan's wraiths (30%) and the Morrígan (2-4, boss spoils once a week);
   Crom Dubh's heart (two level 18-19 quests) is weekly too. Point such steps at the dungeon's door (`step_kinds`
   skips bots with count 0), and give the feathers a steadier source.
6. **Travel on a 65 km map.** Long journeys start at level 6-12 and a pony comes at level 2, but crossing the
   country still takes the better part of an hour. Carters as paid travel between towns (like `/sail` fares), or
   roads that are faster still for a rider.
7. **Existing characters under the new level curve.** Characters made before the refit may show a lower level for
   the same experience. A one-off migration that keeps each saved level, or a note to players.

### Client
8. **Milestone labels overlap town names on the world map.** The waymarks (`land.gd` `milestones`) are drawn without
   the collision step the other labels use (`map.gd` `_label`).
9. **Windows client is unsigned**, so Windows warns before running it.

### Server and performance
10. **`/play/info` grew to 332 KB** with `homes_all` (from 189 KB). Gzip it on the web gateway, or send homes once
    by kind and refer to them by index.
11. **`in_safe` scans every place** (1,229 on Hibernia) each call; use the place index (`homes.rs` `places_at`).
12. **A console save that re-homes bots holds the sim about 0.7 s** (placing ~3,000 bots). Spread respawns over a
    few ticks, or prepare them off the sim thread as reloaded worlds are.
13. **Load test from a cloud machine.** The home link loses snapshots past ~1,000 bots; the live server's real
    ceiling with 4 lanes is unmeasured past 2,000.

### Tests
14. **Console UI tests time out under CPU load** (`Editors.test.tsx`, `Builder.test.tsx` fail while Docker builds,
    pass alone). Raise their timeouts or make the slow renders lighter.

## Done

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
