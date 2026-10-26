# Plan: build, adventure, hang out

The pitch is one line: **a world where you can build, adventure and hang out, and you choose your path.** None of
the three is the main game. Each has to be worth doing on its own, and each has to feed the other two.

How we work: **data rows, not code rows.** Everything a designer tunes lives in the game file (`game.json`,
edited live from the console or the MCP). Code only adds a kind of rule, never the numbers or the content. Each
step below says which part is data (do it now, in the console) and which needs code first.

God mode stays as it is: a god cannot fall, never runs out of anything, moves at top speed, and builds any
material without paying for it.

---

## Where we are

Already built and data-driven: items, shops, gear, mounts, loot tables, recipes (`/craft`), crops and soil,
plots (`/claim`), guilds, dungeons, quests, classes, factions, levels, parties, trading, the bank, the auction
house, weather and events.

New in this round:

- **Blocks can cost materials.** An item with `block: "stone"` *is* stone. Building a stone block uses one up, and
  breaking your own block gives it back. A block nobody owns, like the town's, gives nothing, so nobody can quarry
  the scenery. A material no item names stays free, so the live world plays exactly as before until we add
  material items.
- **`rules`.** The 23 tuning numbers the server kept as constants, plus four for falling, (reaches, cooldowns, party and guild size, chat
  reach, trade and auction limits) are now one section of the game file. Each defaults to its old value, edits
  apply live, and they're in the console's settings.

---

## 0. The more you play, the more you get

Every path levels you, not only fighting. Life experience (`levels.life`) pays for walking, building, planting
(`plant`, 1), bringing in a crop (`harvest`, 3), each minute in the world while awake (`minute`, 1) and each minute
near another player (`company`, 1), all inside `per_min`. Someone who hasn't moved or done anything for five minutes
stops earning by the clock. A level is worth having for a builder too:

- `levels.rewards: [{level, items, text}]` gives things on reaching a level: bricks at 3, glass at 5, a rare seed, coin.
- `plots.sizes[].level`: bigger land needs a level, so building and gardening lead to more land to build on.

**Data to add:** a reward row every level or two up to the cap, with materials for builders, seeds for gardeners
and coin for everyone.

---

## 1. The first ten minutes (all three paths start here)

You arrive in one town square. Three people stand there, and each one opens a path:

| Who | Opens | What you leave with |
|---|---|---|
| A builder (shop + quest giver) | Building | A starter stack of timber and stone, a quest to claim a plot and put up four walls |
| A warden (quest giver) | Adventure | A quest chain that ends at the first small dungeon |
| An innkeeper (shop + quest giver) | Hang out | A quest to visit the tavern and talk to the regulars (quest steps are kill, visit, collect, talk), food and drink to sell |

**Data, doable today:** three bot types with `talk`, `quests` and `shop`, three short quest chains, and a `place`
for the square marked `safe`.

**Done:** the **starter kit** (`starter: [{item, count}]` in the game file, edited on the console's Items tab),
given once to a new character and to a guest on each visit.

The class picker moves from the first join to the warden's dungeon, so a builder never has to pick one.

---

## 2. Build

**The loop.** Gather or buy materials, claim land, build, and others see what you made. Better materials come from
the other two paths, so building is never a dead end.

| Step | Data or code | What |
|---|---|---|
| 2.1 | **data** | Material items: an item per material with `block` set. Cheap ones (timber, plaster, soil) from the builder's shop, stone and brick from quarrying, the rest (glass, iron, slate, lamp, sandstone) from crafting and dungeon loot. |
| 2.2 | **data** | Quarrying from existing systems: a bot type that doesn't move (a rock outcrop, a stand of trees) with health, a loot table that drops stone or timber, `respawn_secs` (say 120) and `respawn_at: "here"`. Hitting it is gathering. |
| 2.3 | **data** | Recipes: `/craft brick` from clay and fuel at a kiln place, glass from sand at a glassworks, iron from ore at a forge. Each `near` a place, so towns have workshops worth walking to. |
| 2.4 | **data** | Plots: `plots.size`, `plots.per_player`. Start at 16 cells and one each. |
| 2.5 | done | Plot ladder: `plots.cost` (coin to claim) and `plots.sizes: [{size, cost}]`, bought one at a time with `/plot grow` (a bigger square about the same centre). Edited under Land in the console's settings. Plots may already sit right beside a town, just not over it. |
| 2.6 | done (server) | `/plot name The Old Mill`, "You walk into The Old Mill." on the way in, `/plots` (nearest first) and `/visit Name` (which way and how far). Still to do: the names on the client's map, and a waymark to walk to. |
| 2.7 | code | `building.refund` (share returned on break, default 1) and `ItemDef.block_cost` (default 1), so the economy can be tuned without code. Five lines in `build_action`. |
| 2.8 | data | Guild halls: a plot owned by a guild leader; guild members can already build on it. |

**Done when** a new player can claim a plot, build a small house from what they gathered in their first hour, and
have someone else find it on the map.

---

## 3. Adventure

**The loop.** Take quests, explore, fight, clear dungeons, get stronger, bring back things builders want.

| Step | Data or code | What |
|---|---|---|
| 3.1 | **data** | A hand-written first chain per region: 5–8 quests with real places and named bots, not the generated 512. Look over the existing quests and keep the good ones. |
| 3.2 | **data** | Loot tables that drop *materials* as well as gear: dungeons give glass, iron and lamps. This is the line that ties adventuring to building. |
| 3.3 | **data** | `rules`: tune `kill_credit_secs`, `party_share_reach`, `chase_give_up_secs` and `sneak_sight` in play. |
| 3.4 | done | Falling with a cost, in `rules`: `fall_xp_loss_pct` (of what was earned into the level, never a level), `fall_coin_loss_pct` (gone), `fall_drop_pct` (each other bag stack's chance of being left where you fell) and `rise_delay_secs` (the fallen screen counts it down). All 0 by default. Rising where you fell is not done. |
| 3.5 | done | Bot respawn per type: `respawn_secs` (stays gone that long, out of sight) and `respawn_at`: `anywhere` (as before), `home` or `here`. |
| 3.6 | code | Dungeon lockouts and party rewards: `DungeonDef.lockout_secs`, reward to the whole party, `max_party`. |
| 3.7 | code | XP split in a party (`levels.party_split`), instead of everyone getting the full share. |

**Done when** a new player can follow one chain from the square to the first dungeon, clear it with a friend, and
come back with something a builder will buy.

---

## 4. Hang out

**The loop.** Be somewhere with other people, and do something there that isn't fighting or building.

| Step | Data or code | What |
|---|---|---|
| 4.1 | done | Emotes as data: `emotes: [{ cmd: "wave", text: "{name} waves.", at: "{name} waves at {target}.", anim: "Cheer" }]`, edited in the console's settings. Said within `say_reach` on chat channel 11; Godot plays `anim` if the model has a clip of that name. No emotes ship by default: add rows. |
| 4.2 | done | Sitting as an emote row with `stays: true` (`/sit` with `Sit_Floor_Idle`, `/sleep` with `Lie_Idle`): held until you move, shown to others through a replicated `pose` attribute (add it: min 0, max 255, 8 bits; attributes are in the hash, so adding it restarts the world). Clicking a bench to sit on it is not done. **Catch:** a snapshot carries at most 7 replicated attributes (`MAX_REPL`) and our game uses all 7, so `pose` needs a slot freed (or `MAX_REPL` raised, a protocol bump). Until then `/sit` still plays its clip once and says its line. |
| 4.3 | **data** | Taverns: a `place` with `music` and `sound`, an innkeeper bot who sells food and drink, and bots with `chat` lines and `during: night`. |
| 4.4 | **data** | Fairs and gatherings as `events` on a timetable: a market fair on Saturday evenings, a race, a harvest festival. Bots with `lasts_secs` for the stalls. These are what bring a small player base together at the same time. |
| 4.5 | **data** | `rules`: `say_reach`, `shout_reach` (a shout that carries across a town, not the whole world), `party_size`, `guild_size`. |
| 4.6 | code | Auction fee and sell-back share (`rules.auction_fee_pct`, `rules.sell_ratio`), so money leaves the world and prices settle. |
| 4.7 | code | Music you play: an instrument item whose use plays a tune nearby players hear. It reuses the CASTS packet with a sound in place of an FX. |

**Done when** two players who don't want to fight or build still have a reason to log in on the same evening.

---

## 5. Still in code that should be data (next sweep)

From the inventory of hard-coded rules, the ones that need a client change as well:

- Block materials themselves (names, colours, textures) as a game-file table. Today they're six parallel lists, and
  adding one means a client release.
- Weather looks: chosen from keywords in the weather's *name* in `theme.gd`. Should be `WeatherDef.look`.
- Bot looks by name: the cart, horse and fire are special-cased by display name in `main.gd`. Should be
  `Archetype.look`.
- Tutorial, fallen-screen and loading lines: text in the client. Should come from `/play/info`.
- Bag and bank size (24 each): fixed in the INVENTORY packet. Needs a protocol bump.
- Replicated attributes: at most 7 (`MAX_REPL`), and the game uses all 7. Raising it is a protocol bump in `proto`, `protocol.ts` and the crowd decoders.
- Gear slots (3), hold kinds, FX and sounds: fixed lists shared with the client and its assets.

---

## 6. Order

1. Deploy this round (block costs and `rules`). Add material items (sold by the builder and dropped by loot tables) as data.
2. ~~Starter kit~~ (done), then the three people in the square and their first quests (data).
3. ~~Bot respawn rules~~ (done), then quarries (2.2, data), then ~~falling cost~~ (done, defaults off).
4. ~~Emotes and sitting~~ (done: add the rows and the `pose` attribute).
5. ~~Plot ladder, named plots and visiting~~ (done on the server; the map labels are client work).
6. Fairs, taverns and timetabled events (data).
7. Host it and get 5–10 people in. Watch which path each picks and where they stop.
