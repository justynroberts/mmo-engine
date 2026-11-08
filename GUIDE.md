# Player guide

How to play the Godot client: the keys, the windows and the chat commands. F1 in the game shows the keys too.

## Getting in

Download the newest client from the [releases](https://github.com/justynroberts/mmo-engine/releases) (`mmo-client-windows.zip` or the macOS zip), unpack it and run it. It updates itself: when a newer release is out, the join screen offers it.

Your character is tied to this computer's identity file, so the same machine always brings back the same character.

## Moving and looking

| Key | What it does |
|---|---|
| W A S D or arrow keys | Move |
| Q / E | Turn |
| J | Jump |
| Mouse wheel | Zoom the camera; zoom in past the closest for first person |
| V | First person on and off (Esc leaves it) |
| L | Torch: auto, always lit, out |
| F11 | Full screen |

## Fighting

| Key | What it does |
|---|---|
| Click | Target someone or something |
| Tab / Shift+Tab | Next / previous target: those fighting you first, nearest first, else what is near |
| Space | Attack (hold to keep swinging) |
| R T Y U O P Z C ' , . ; | Spells, in the order of the spell bar above the action bar |
| K or F2 | Spellbook and journal |

Your spells depend on your class (`/class Name`, chosen once).

## Items and people

| Key | What it does |
|---|---|
| 1-8 | Use the item in that slot of the action bar |
| Shift + 1-8 | Drop it |
| F | Talk to someone near, trade with the player you have targeted, pick something up, or take a boat at a dock |
| H | Hail: a trader or the like stops for you |
| I or F4 | Bag and bank (the bank only at a market) |
| F3 | Quests: your log and the tracker |
| F5 | Group finder: queue for a dungeon |
| F6 | Guild, letters and standing |

## Maps

| Key | What it does |
|---|---|
| M | World map. The wheel zooms towards the pointer, drag to move, double click to see all of it, M or Esc to close |
| Wheel over the minimap | Zoom the minimap |

Both maps have fog of war: you only see the land your character has been through, and places, traders, docks and dungeons appear once you have been near them. Where a quest sends you always shows. What each character has explored is kept on your computer.

You are the cyan arrow on the world map and the small pale arrow in the middle of the minimap.

## Building

| Key | What it does |
|---|---|
| B | Build mode on and off (Esc leaves it) |
| 1-0 - = [ ] \ | Pick a material (`\` is tilled soil: plant seeds beside it) |
| Click | Place a block |
| Hold right click, X or Shift+click | Break one of your own blocks (sweep to keep going) |
| T | Next blueprint |
| R | Turn the blueprint |

Building can cost materials from your bag. A roof over you and walls on three sides is a shelter: night ambushers leave you alone in it.

## Chat

| Key | What it does |
|---|---|
| Enter | Chat. Start the line with `!` to shout |
| N | Sound: all, no music, off |
| G | God mode (only for the accounts the game names) |
| Esc | Close whatever is open, else the menu |

### Talking to people

| Command | What it does |
|---|---|
| `/w Name text` | Whisper (`/r text` answers the last whisper) |
| `/who` | Who is on |
| `/rep` | Your standing with the factions |
| `/name Name` | Your display name |
| `/wave` `/cheer` `/bow` `/dance` `/sit` `/rest` `/thank` `/laugh` | Emotes (sitting lasts until you move) |

### Parties and dungeons

| Command | What it does |
|---|---|
| `/invite Name`, `/accept`, `/decline`, `/leave` | Make and run a party |
| `/p text` | Party chat; `/party` shows the party |
| `/raid` | Make your party a raid (bigger) |
| `/queue`, `/unqueue` | The group finder (also F5) |

### Your character

| Command | What it does |
|---|---|
| `/class Name` | Choose a class (once) |
| `/talents`, `/talent Name`, `/respec` | Talents: a point a level; respec at a market |
| `/faction Name` | Join a side in the faction war |
| `/holds` | Who holds what in the war |
| `/title [Name\|none]` | Wear a title you have earned |
| `/look cape\|helmet` | Your appearance |
| `/skills`, `/recipes`, `/craft Name` | Skills and crafting |

### Land, guilds, mail and trade

| Command | What it does |
|---|---|
| `/claim`, `/unclaim` | Claim land where you stand, or give it up |
| `/plot grow`, `/plot name Name` | Grow or name your plot |
| `/plots`, `/visit Plot` | List named plots and go to one |
| `/unfurnish` | Take back furniture you set down |
| `/guild create\|invite\|accept\|leave\|kick`, `/g text` | Guilds and guild chat |
| `/trade Name` | Ask a player to trade |
| `/mail Name text` | Write a letter (anywhere) |
| `/send Name count item` | Send goods or coin from your bag, e.g. `/send Name 50 gold` (at a market) |
| `/mailbox` | Read your letters; goods sent to you come into your bag at a market |
| `/sail [Place]` | Take a boat from a dock |

## Falling

If you fall you stay down until you choose to rise (Enter), then you rise at the nearest safe place. Falling costs a little experience and coin, and can leave things from your bag where you fell.
