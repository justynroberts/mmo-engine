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

## 3. Gathering and crafting skills — done but for the crafter's name

Skills that rise with use (`GameFile::skills`, `crates/server/src/skills.rs`): a recipe's `skill` and `level` gate it
and making it teaches while the maker is under the level plus 25, and past the level now and then makes a second
batch (the "quality" here, since items are stacks without one-off properties); a crop's `skill` rises on harvest
and gives a sheaf more per 25; a bot type's `skill` rises for whoever beats one (gathering). `/skills`, the F6
window's Skills column, and the Make row's buttons show them; kept per character by name. Both worlds have Cooking,
Farming and Hunting. Not done: a crafter's name on what they make, which needs items with one-off properties.

## 4. Faction war — done but for guards who change sides

Places marked `hold` (`Place::hold`, the first holder) are seats a faction can take: its members (`/faction`)
standing in one, with none of the holder's there and no third side, take it after `rules.capture_secs`, and the
whole world is told. Holding pays: every ten minutes each online member of a holder gets `rules.hold_tithe` coin a
hold. Who holds what is saved with the realm, sent on chat channel 18 and drawn as coloured flags on the Godot map;
`/holds` lists it (`crates/server/src/holds.rs`). Both worlds: the four provinces (Laighin, Ulaidh, Connachta,
Mumhain) as factions, their royal seats as holds and Tara held by nobody. Not done: guards who change sides (no bot
type belongs to a province yet).

## 5. Achievements and titles — done

Deeds counted per character (kills in all and by type, quests, places found, crafts, harvests, builds, falls,
dungeons; `crates/server/src/deeds.rs`), achievements over them (`GameFile::achievements`: a deed, a count,
experience, a title it grants), and a title worn with `/title` and shown with the name (the names feed sends
"Name|Title"; the Godot client puts it on the name plate). The F6 window lists achievements with progress and
buttons to wear a title. Eleven in both worlds.

## 6. Talents — done

`ClassFile::talents` (ranks, a level, might and toughness a rank): a point a level past the first, spent with
`/talent Name` or the F6 window's "+" buttons, added in `Sim::scaled`; `/respec` takes them back at a market for
`rules.respec_cost` coin (`crates/server/src/talents.rs`). Three talents for each of the four classes in both worlds.
A trainer bot type was not needed: any market will do.

## 7. Appearance and dyes — done but for showing one item's look on another

A look: a cape colour from a palette and the helmet shown or not, chosen in the class picker or with `/look`, and
dye items (`ItemDef::dye`) that colour the cape (three sold by the merchant in both worlds). Kept per character
(`Record::look`), served beside names at `/play/looks`, drawn by the Godot client on everyone's model
(`crates/server/src/looks.rs`, `main.gd` `_dress`). The KayKit models have no body or hair variants to choose from,
and gear is not drawn on them, so showing one item's look on another has nothing to show yet.

## 8. Group finder — done

Queue for a dungeon by role (`/queue Name tank|healer|damage`, `/unqueue`, or the Godot client's F5 window); a
party queues together. When the dungeon's `group` (default 3) have queued, a tank and a healer first (or whoever is
there after `rules.finder_wait_secs`), they become a party (a raid past `party_size`) and are put at the
dungeon's door. `crates/server/src/finder.rs`; the queue's state on chat channel 16.

## 9. Daily and weekly rhythm — done

Quests that reset each day — done: a quest with `daily` can be taken again each UTC day once handed in (the day is
kept in its saved count); three in both worlds (the watch's bounty, bread for the poor, the brigand toll). Boss
loot once a week per character — done: a boss's drops and loot table are rolled for each player credited with it,
into their own bag, once a week per boss (`items.rs` `loot_from`, `Record::spoils`). Events on a calendar — done:
an event's `when` may be `calendar` (hour and minute UTC, every day or one weekday); both worlds hold a Great Fair
on Saturdays at 19:00 and a great raid on Fridays at 20:00.

## 10. Swimming and boats — done

Deep water is swum: a world's lakes, where the ground lies more than a body below the lake's level, are water cells
(`World::in_water`, `proto::world::WATER_SPEED` half speed in `integrate`, matched in `ui/src/play/sim.ts`) instead
of walls; they ride after the tiles in `/play/tiles` (older readers stop before them), bots' paths keep out of
them, nobody fights or builds there, and the Godot client draws a swimmer at the surface. Boats: places marked
`dock` (twelve harbour towns in both worlds), `/sail Name` there buys passage by the distance and lands you at the
far quay; F at a dock and anchors on the map (`crates/server/src/boats.rs`).

## 11. A main story — done

A quest's `opening` and `closing` are scenes told across the screen of whoever takes and hands it in, and its giver
says its `done_text` aloud. "The Stone of Destiny" in both worlds: six quests from Áth Cliath to Tara, Kells,
Clonmacnoise and Glendalough, the sluagh, Crom Dubh in Brú na Bóinne (with a company), and the Lia Fáil roaring
again.

## 12. Furnishing houses

Furniture items placed inside a house on a plot, seen by visitors.
