# Roadmap

What the long-lived MMOs have that this one does not yet, in the order it is being built. Each phase is data in the
game file (nothing hard-coded per world), works through the console and the MCP, and ships with tests. A phase is
done when the server, the Godot client and the console all handle it.

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

## 8. Group finder

Queue for a dungeon by role; a party made from the queue is taken to the dungeon's door.

## 9. Daily and weekly rhythm

Quests that reset each day, boss loot once a week per character, and events on a calendar.

## 10. Swimming and boats

Deep water swum (slow, no fighting), boats bought and sailed between docks.

## 11. A main story

A chain of quests through the world's towns, with scenes told in banners and speech.

## 12. Furnishing houses

Furniture items placed inside a house on a plot, seen by visitors.
