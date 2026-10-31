# Roadmap

What the long-lived MMOs have that this one does not yet, in the order it is being built. Each phase is data in the
game file (nothing hard-coded per world), works through the console and the MCP, and ships with tests. A phase is
done when the server, the Godot client and the console all handle it.

## Now: make it a game, not a set of slash commands

The server already has dungeons, parties, farming, plots, crafting, guilds and 24 quests, but most of it is reached
through chat commands a new player never finds. This pass puts each behind a window, a marker or a quest, in this
order (each step is playable on its own):

**A. Dungeons and groups** — done (raids are a party grown past 5 with `/raid`; dungeons do not yet ask for one)
1. The seed world gets its two dungeons (Brú na Bóinne with Crom Dubh, the brigands' camp with the Brigand King),
   bosses included, as the 65,536 world has them.
2. Dungeons marked on the map and minimap, with a level and "party of N" note; a banner on entering and clearing.
3. A party window (P): members, invite the player you have targeted, leave; an invite pops up to accept or decline.
4. Raids: a raid is a party past 5 (up to `rules.raid_size`), drawn as a compact grid; dungeons can ask for one.

**B. Quests that lead somewhere** — done (the goal shows on the map, the minimap and in the tracker with its
distance and compass point, and over the view as a ring or an arrow at the screen's edge; the starter chain is
"a new arrival" to "the brigand king")
1. The tracked quest's goal shown on the map, the minimap and as an arrow at the screen's edge.
2. "E to talk" over a giver in reach.
3. A starter chain through the systems: a town errand, the first furrow (farming), a shelter (building), the
   brigands' camp (a dungeon with a party).

**C. Farming** — done (the land bar counts what is sprouting and ripe and says when the next is ready; making is
a row in the bag)
1. A plot window: claim here, grow, name, with the plot's edge drawn on the ground while it is open.
2. Crops show their stage and time to harvest when looked at; seeds plant from the bag's quick slots.
3. A crafting window over `/craft` (recipes, what you have, make), with cooking recipes for the crops.

**D. Building** — done (the build bar already showed costs; quests have a `build` step; T in build mode picks a
blueprint, `godot/scripts/blueprints.gd`: shelter, cottage, field, watchtower, laid out as a ghost and built a
block at a time while you stand in reach)
1. The build bar shows each material's cost and how many you carry; free ones marked.
2. A quest step "build N blocks (of a material) in your plot", so quests can ask for building.
3. Blueprints: a few small buildings laid out as a ghost to fill in.

**E. The rest behind windows** — done: F6 in the Godot client is guild, letters and standing side by side, from
"/social" (chat channel 17), with buttons for founding, inviting, leaving, writing and taking goods.

## 1. Boss fights — done

A bot type with a `boss` section fights by script rather than by its rule stack alone.

- **Threat.** The boss goes for whoever has the most threat, not whoever struck first: damage done to it adds
  threat, healing someone it is fighting adds half, and an action with `taunt` puts the user on top. It changes
  target only when another player passes the current one by a tenth, so a tank can hold it.
- **Telegraphed moves.** Each move is an action used on a timer. With `warn_secs` the ground is marked first
  (chat channel 14, `T|...`) and the blow lands on whoever is still inside when the time is up: it can be walked
  out of. A move aims at the tank, a random player in the fight, the farthest one, or round the boss.
- **Phases.** Health thresholds (`below`, percent) switch the move set, say a banner and bring in adds.
- **Enrage.** Past `enrage_secs` its blows count `enrage_might` times.
- **Reset.** With nobody left fighting it, it heals fully, its adds go and the fight starts over, so it cannot be
  worn down a bit at a time.

Edited in the bot builder's Boss fight section or through the MCP; Godot and the browser client draw the marks.
Still to do: a dungeon in the deploy seed with a boss in it (changing the seed changes the live server's world).
Damage shown per player (a meter) would help groups learn a fight.

## 2. Mail — first cut done

`/mail Name words` from anywhere; `/send Name 10 gold` (any item, coin included) from a market; `/mailbox` reads
letters anywhere and takes what was sent at a market. Waiting letters are kept with the realm's save; a player is
told when one comes and when they next join. `rules.mail_waiting` caps a mailbox.

Still to do: a mailbox window in the Godot client (it works through chat for now), and auction proceeds arriving as
letters rather than by the auction house's runner.

## Hibernia at 65,536 — built, not deployed

`deploy/hibernia-65536/`: the seed world sixteen times bigger, with tiered roads, farmsteads, fields and hedgerows,
and standing stones between the towns and the capital. Dungeons come separately, as their own maps joined by zone
lines. Deployed 2026-10-30. Since then: regions (flat lowland to rugged upland), mountains where the named ranges
are (with a treeline and snow scaled to their height), heights twice as fine, lakes as one water, a sharp minimap.
Two instanced dungeons with bosses (Brú na Bóinne, Campa na nDíbheirgeach). Still to do: folk for the new
farmsteads, waymarks and inns along the highways, dungeons as maps of their own (zone lines).

## 3. Gathering and crafting skills

Skills that rise with use (mining, herbalism, smithing, cooking...), recipes that need a skill level, and quality
that rises with skill. A crafter's name on what they make.

## 4. Faction war

Keeps and towns a faction can take and hold (a banner to stand at, guards who change sides), with something
worth holding them for: a tax on trade there, a vendor, a bonus to the side.

## 5. Achievements and titles

Deeds counted per character (kills by type, places found, quests, crafts, falls), achievements over them, and
titles shown with the name.

## 6. Talents

Points per level spent in a class's trees; respec at a trainer for coin.

## 7. Appearance and dyes

A look chosen when a character is made (body, hair, colours), dyes for gear, and showing one item's look on another.

## 8. Group finder — done

Queue for a dungeon by role (`/queue Name tank|healer|damage`, `/unqueue`, or the Godot client's F5 window); a
party queues together. When the dungeon's `group` (default 3) have queued, a tank and a healer first (or whoever is
there after `rules.finder_wait_secs`), they become a party (a raid past `party_size`) and are put at the
dungeon's door. `crates/server/src/finder.rs`; the queue's state on chat channel 16.

## 9. Daily and weekly rhythm — dailies and weekly boss spoils done

Quests that reset each day — done: a quest with `daily` can be taken again each UTC day once handed in (the day is
kept in its saved count); three in both worlds (the watch's bounty, bread for the poor, the brigand toll). Boss
loot once a week per character — done: a boss's drops and loot table are rolled for each player credited with it,
into their own bag, once a week per boss (`items.rs` `loot_from`, `Record::spoils`). Still to do: events on a
calendar.

## 10. Swimming and boats

Deep water swum (slow, no fighting), boats bought and sailed between docks.

## 11. A main story

A chain of quests through the world's towns, with scenes told in banners and speech.

## 12. Furnishing houses

Furniture items placed inside a house on a plot, seen by visitors.
