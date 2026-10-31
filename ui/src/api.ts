import type { Daylight } from './daylight'
export type Behaviour = 'idle' | 'wander' | 'chase' | 'flee'
/** Where a townsperson works by day (Archetype.work), found round its home in the world file. */
export type Work = 'square' | 'water' | 'fields' | 'streets' | 'play'
export type Season = 'spring' | 'summer' | 'autumn' | 'winter'
export type Who = 'self' | 'target'

export interface AttrFile {
  name: string
  min: number
  max: number
  default: number
  bits: 8 | 16
  replicated: boolean
  regen_per_sec: number
  on_zero: 'none' | 'respawn'
}
export interface EffectFile {
  to: Who
  attr: string
  delta: number
  /** Spread delta over this many seconds (0..60) instead of all at once: a burn, a slow heal. Absent = at once. */
  secs?: number
  /** Take it back after this many seconds (0..300): a buff or a curse that wears off. Not with secs. */
  lasts?: number
}
export type Aim = 'target' | 'self'
export type Hits = 'any' | 'foes' | 'friends'
export type HoldKind = 'mez' | 'stun' | 'root'
/** Holding a target still: mez (no moving or acting, broken by damage), stun (the same, not broken), root (no moving). */
export interface HoldFile {
  kind: HoldKind
  /** Above 0, at most 30. */
  secs: number
}
/** How a spell looks to clients (proto::game::FX). */
export const FX = ['fire', 'frost', 'shock', 'holy', 'shadow', 'nature', 'arcane', 'poison', 'mind', 'blood'] as const
export type Fx = (typeof FX)[number]
/** The hold kinds as the server reports them (/api/info, and the reserved `held` attribute). */
export const HOLD_CODE: Record<HoldKind, 1 | 2 | 3> = { mez: 1, stun: 2, root: 3 }
export interface ActionFile {
  id: number
  name: string
  cooldown_ticks: number
  range: number
  requires: { attr: string; min: number } | null
  effects: EffectFile[]
  // Spells: every field below is optional, absent means the default, and none restarts the world.
  /** Who it is aimed at; absent = 'target'. */
  aim?: Aim
  /** Radius (0..64) round the target, or the caster when aimed at self: effects on target land on everyone in it. */
  area?: number
  /** Which side it lands on (players are one side, bots the other); absent = 'any'. */
  hits?: Hits
  hold?: HoldFile
  /** One of FX; absent for a plain blow. */
  fx?: Fx
  /** Drawn flying from the caster to the target. */
  bolt?: boolean
  /** Free text (at most 24 characters) to group spells by. */
  school?: string
  /** What it does in plain words, for players' tooltips (at most 200 characters). */
  desc?: string
  /** Lift everyone it lands on off the ground for this many seconds (0..300): blows and areas miss them. */
  lift?: number
  /** The level a player must be to use it. */
  level?: number
  /** Used on a boss, it puts the user at the top of its threat. */
  taunt?: boolean
}
/** Experience and levels (GameFile.levels). Level L needs base * (L - 1) ^ exponent experience in all. */
export interface Levels {
  cap?: number
  base?: number
  exponent?: number
  /** Each level above the first: blows and heals land this share harder. */
  might?: number
  /** Each level above the first: harm taken is divided by 1 + this share. */
  toughness?: number
  /** For finding a named place the first time. */
  discover_xp?: number
  /** Life experience: a little for everything a player does, banked and paid every ten seconds. */
  life?: LifeXp
  /** What reaching a level gives: items (coin is an item) and a line. Levels 2..cap. */
  rewards?: LevelReward[]
}
export interface LevelReward {
  level: number
  items?: QuestItem[]
  text?: string
}
/** Experience per deed (fractions add up; 0 turns one off), at most per_min a minute. Absent: the defaults. */
export interface LifeXp {
  /** For each 100 units walked (1.5). */
  walk?: number
  /** For each action used, a blow or a spell (0.5). */
  act?: number
  /** For each block built or broken (0.5). */
  build?: number
  /** For an item used or picked up, a trade, a word with someone, a quest taken (1). */
  deed?: number
  /** For each seed planted (1), and each ripe crop brought in (3). */
  plant?: number
  harvest?: number
  /** For each minute in the world while awake (1), and more for each such minute near another player (1). */
  minute?: number
  company?: number
  /** The most it pays in a minute (30). */
  per_min?: number
  /** Against macros: walking pays only into ground not among the last `trail` 32-unit cells walked (64), and a
   * deed the same as one of the last `repeat` (8) pays nothing. 0 turns either off. */
  trail?: number
  repeat?: number
}
/** An item and how many (a quest reward). count 1..255. */
export interface QuestItem {
  item: string
  count: number
}
/** One thing to do in a quest, done in order. Beating bots counts for everyone who hurt the one that fell. */
export type QuestStep =
  /** Beat `count` (1..65535) bots of a type. */
  | { kind: 'kill'; archetype: string; count: number }
  /** Walk into a place. */
  | { kind: 'visit'; place: string }
  /** Carry `count` (1..255) of an item; taken when the quest is handed in. */
  | { kind: 'collect'; item: string; count: number }
  /** Build `count` (1..65535) blocks, of one material (a MATERIALS name) or of any when left out. */
  | { kind: 'build'; count: number; material?: string }
  /** Talk to a bot of a type. */
  | { kind: 'talk'; archetype: string }
/** A quest (GameFile.quests, at most 512; a player holds at most 25 at once). Not in the content hash: edits apply live, nobody is disconnected. */
export interface QuestFile {
  /** Unique id, at most 48 bytes. */
  name: string
  /** What players see, at most 64 characters. */
  title: string
  /** What the giver says offering it (at most 600 characters). */
  text: string
  /** What is said on handing it in (at most 600 characters). */
  done_text?: string
  /** The bot type (archetype) that offers it. */
  giver: string
  /** The bot type it is handed in to (absent: the giver). */
  turn_in?: string
  /** The level a player must be to take it (absent: any). */
  level?: number
  /** A quest that must be done first. */
  after?: string
  /** 1 to 8 steps, done in order. */
  steps: QuestStep[]
  /** Experience on handing it in. */
  xp?: number
  /** Items given on handing it in. */
  reward?: QuestItem[]
  /** May be taken again once done. */
  repeatable?: boolean
  /** May be taken again each day (UTC) once handed in. */
  daily?: boolean
  /** A scene told across the screen of whoever takes it, and of whoever hands it in (a story's). */
  opening?: string
  closing?: string
}
/** The models the Godot fantasy theme draws classes with (ClassFile.look may be any other key a client knows). */
export const CLASS_LOOKS = ['knight', 'barbarian', 'mage', 'rogue'] as const
/** A character class (GameFile.classes, at most 15). A player picks one once, in game, and keeps it; the reserved
 *  attribute "class" (replicated, 0..15) shows its index + 1 (0: none, and every bot). Not in the content hash:
 *  edits apply live, nobody is disconnected. */
/** A class's talent: up to `ranks` (1..5) points from level `level`, each adding `might` and `toughness` (0..0.5). */
export interface TalentFile {
  name: string
  title: string
  text?: string
  ranks?: number
  level?: number
  might?: number
  toughness?: number
}
export interface ClassFile {
  /** Unique id, at most 32 bytes. */
  name: string
  /** What players see, at most 32 characters. */
  title: string
  /** What it is like, at most 300 characters. */
  desc?: string
  /** The model a client draws it with (the fantasy theme has knight, barbarian, mage, rogue). */
  look?: string
  /** 0..2: blows and heals land this share harder, added to the level's and the gear's. */
  might?: number
  /** 0..2: harm taken is divided by 1 + this share, added to the level's and the gear's. */
  toughness?: number
  /** The actions (by name) it may use besides the plain attack (the game's first action); absent or empty: all. */
  spells?: string[]
  /** Schools of spells (ActionFile.school) it may use too: every action of these, and those of no school. Players
   * can also choose a class over chat with /class Name. */
  schools?: string[]
  /** Talents its players spend a point a level on (past the first); each rank adds might and toughness shares. */
  talents?: TalentFile[]
}
export type Seek = 'player' | 'bot' | 'any'
export type Cond =
  | { kind: 'always' }
  | { kind: 'sees'; who: Seek; within: number }
  | { kind: 'attr_below'; attr: string; value: number }
  | { kind: 'attr_above'; attr: string; value: number }
  | { kind: 'in_place'; place: string }
  | { kind: 'chance'; pct: number }
  | { kind: 'sees_kind'; bot: string; within: number }
  | { kind: 'during'; parts: DayPart[] }
  /** The weather where it stands is one of these; none listed means any weather at all. */
  | { kind: 'weather'; names: string[] }
export type Act =
  | { kind: 'idle' | 'wander' | 'chase' | 'flee' | 'attack' | 'indoors' }
  | { kind: 'patrol'; route: string }
  | { kind: 'goto'; place: string }
  | { kind: 'say'; text: string; shout: boolean }
  | { kind: 'chat'; lines: string[] }
export interface Rule {
  when: Cond
  then: Act
}
export interface Place {
  name: string
  x: number
  y: number
  r: number
  /** What players see as they walk in and out; empty or missing for a place that is not a location. */
  title?: string
  /** A zone line: walking in takes a player to (x, y) in `zone` (a zone's name), or on this map if `zone` is empty. */
  exit?: { zone?: string; x: number; y: number }
  /** The ambience a client plays inside it over the land's own sounds (one of SOUNDS). */
  sound?: string
  /** The music a client plays inside it (one of MUSIC). */
  music?: string
  /** Players cannot harm each other inside it, even with pvp on. */
  safe?: boolean
  /** A seat factions fight over (faction war): present, the faction holding it at first ('' for none). */
  hold?: string
  /** A harbour: boats sail from it to the other docks for coin (/sail). */
  dock?: boolean
}
/** What a place's sound and music may be (proto::game::SOUNDS, MUSIC). */
export const SOUNDS = ['sea', 'river', 'forest', 'night', 'wind', 'rain', 'fire', 'town', 'market', 'crypt', 'holy', 'battle'] as const
export const MUSIC = ['explore', 'town', 'night', 'battle', 'eerie', 'holy', 'city', 'capital', 'tavern', 'none'] as const
export interface Route {
  name: string
  points: [number, number][]
  loop: boolean
}

export interface UseEffect {
  attr: string
  delta: number
}
export interface ItemDef {
  name: string
  /** What players see it called (absent: its name), e.g. "Oat seed". At most 32 characters. */
  title?: string
  color: string
  stack_max: number
  uses: UseEffect[]
  consume: boolean
  /** What a merchant pays for one, in the game's currency (0..65535); absent or 0, they will not buy it. */
  value?: number
  /** Worn gear: using it puts it on in its slot. */
  equip?: EquipFile
  /** A mount (0..4): riding it adds this to the `speed` attribute. Absent or 0, not a mount. */
  mount?: number
  /** How rare it is (absent: common). A drop of a rare one or better is announced. */
  rarity?: Rarity
  /** A block of this material (one of MATERIALS): building one uses one up, and breaking your own gives it back.
   *  At most one item per material; a material no item is a block of costs nothing to build. Gods build free. */
  block?: string
  /** A dye: using one colours the user's cape this (#rrggbb), used up. */
  dye?: string
  /** Furniture: using one sets this piece (a theme piece name, like "d:stool") down on your own land. */
  furniture?: string
}

/** The block materials a player can build, by number (proto::game::BLOCK_NAMES up to the crops). */
export const MATERIALS = ['stone', 'timber', 'red brick', 'plaster', 'brick', 'rough brick', 'roof tiles', 'iron', 'glass', 'slate', 'mossy stone', 'sandstone', 'lamp', 'fire', 'soil']

export type Rarity = 'common' | 'uncommon' | 'rare' | 'epic' | 'legendary'
export const RARITIES: Rarity[] = ['common', 'uncommon', 'rare', 'epic', 'legendary']
/** How each rarity is coloured wherever an item is named. */
export const RARITY_COLOURS: Record<Rarity, string> = { common: '#d8d8d8', uncommon: '#5fd35f', rare: '#4f9dff', epic: '#b46cff', legendary: '#ff9a2e' }

/** A side players join with /faction; the same side never harms each other. */
/** A calling (/class): the spell schools it may use. */
export interface LootEntry {
  item: string
  weight?: number
  min?: number
  max?: number
}
export interface LootTable {
  name: string
  rolls?: number
  nothing?: number
  entries: LootEntry[]
}
export interface Stack {
  item: string
  count?: number
}
/** /craft name: takes `needs`, gives `count` of `makes`, optionally only near a place. */
export interface Recipe {
  name: string
  needs: Stack[]
  makes: string
  count?: number
  near?: string
  /** A skill (GameFile.skills) it needs `level` of; making it raises the skill while not far below. */
  skill?: string
  level?: number
}
/** An achievement: `count` of one deed (kills, quests, places, crafts, harvests, builds, falls, dungeons, or
 *  "kill:<bot type>"), earned once, with experience and perhaps a title the player may wear (`grants`). */
export interface Achievement {
  name: string
  title: string
  text?: string
  deed: string
  count: number
  grants?: string
  xp?: number
}
/** A skill that rises with use, 0..max (default 100): by recipes, crops and bot types that name it. */
export interface SkillDef {
  name: string
  title?: string
  max?: number
}
/** A seed planted beside tilled soil grows in `grow_secs` into a crop that harvests into `yields`. */
export interface CropDef {
  name: string
  seed: string
  yields: string
  min?: number
  max?: number
  grow_secs?: number
  /** A skill harvesting it raises (a sheaf more each 25). */
  skill?: string
}
/** Land players claim with /claim: `size` blocks on a side, `per_player` each, for `cost` coin. `/plot grow` buys
 *  the next of `sizes` (each bigger than the one before). */
export interface PlotRules {
  size?: number
  per_player?: number
  cost?: number
  sizes?: { size: number; cost?: number; level?: number }[]
}
/** A place that gives each party its own copy of these monsters. */
export interface DungeonDef {
  name: string
  place: string
  bots: { bot: string; count?: number }[]
  xp?: number
  reward?: Stack
  /** Players the group finder brings together for it (3 when left out). */
  group?: number
}
export const GEAR_SLOTS = ['weapon', 'armour', 'trinket'] as const
export type GearSlot = (typeof GEAR_SLOTS)[number]
/** Where an item is worn and what it adds (shares, like a level's: 0..2, 0.1 is a tenth). */
export interface EquipFile {
  slot: GearSlot
  might?: number
  toughness?: number
}
/** Something a bot type sells, at a price (0..65535) in the game's currency. */
export interface ShopItem {
  item: string
  price: number
}
export interface DropDef {
  item: string
  chance_pct: number
  min: number
  max: number
}

export interface Archetype {
  name: string
  /** What players see it called (absent: its name, capitalised), e.g. "Trader". At most 32 characters. */
  title?: string
  /** Given names, one for each bot of this type ("Bridget" the Trader); absent, they go by the type. */
  names?: string[]
  /** Family names to go with names ("Bridget Kavanagh"): no two bots in the world share a pairing. */
  surnames?: string[]
  /** A cutpurse: lifts coin from those it stands beside (rules.lift_pct), runs from the watch, drops it when it falls. */
  thief?: boolean
  /** A dog: keeps at the heel of one of the townsfolk, waits at their door, barks at strangers and runs at a cutpurse. */
  pet?: boolean
  /** A farm beast that strays: townsfolk ask a player to find it and lead it home (rules.plea_pct). */
  stray?: boolean
  /** Tends the sick while a plague is in town (a plague doctor). */
  healer?: boolean
  /** A boss: it fights by threat and a script of moves, phases and adds. */
  boss?: BossFile
  /** More places it lives in besides home: its bots are shared out over all of them. */
  homes?: string[]
  /** It comes for players out in the dark (rules.night_ambush_pct), and is gone by day. */
  ambush?: boolean
  /** Its trade: by day at a stall on a square, by the nearest water, in the fields beyond town, or walking the
   * streets (the watch); of an evening often out on the square. */
  work?: Work
  color: string
  count: number
  behaviour: Behaviour
  speed_mult: number
  sight: number
  attack: string | null
  attrs: Record<string, number>
  home: string | null
  rules: Rule[]
  drops?: DropDef[]
  /** Experience for beating one, shared by every player who hurt it lately. */
  xp?: number
  /** Its level (1 when absent): it hits and takes blows as a player of that level would. */
  level?: number
  /** What it sells (players talk to one to trade), priced in the game's currency. */
  shop?: ShopItem[]
  /** Seconds one lasts before it is gone of itself (absent for as long as it lives): a fire burns down. */
  lasts_secs?: number
  /** Seconds one that falls stays gone before it comes back (absent: at once). */
  respawn_secs?: number
  /** Where it comes back: anywhere free (absent), its home place, or where it fell (a quarry, a tree). */
  respawn_at?: 'anywhere' | 'home' | 'here'
  /** Encounters: the first blow between it and a player locks it on that player, whom it then chases past its
   * sight and fights back (unless a rule says flee) until one falls, or the player gets this far away (world
   * units), reaches a safe place, or no blow lands for 10 seconds. Absent or 0: three times its sight, at least
   * 60. Applies live. */
  leash?: number
  /** The faction it belongs to (GameFile.factions, by name): beating one moves a player's standing with it. */
  faction?: string
  /** A loot table (`GameFile.loot`) rolled when it falls, on top of its drops. */
  loot?: string
  /** A skill (GameFile.skills) beating one raises: gathering from quarries, trees and game. */
  skill?: string
}
/** A faction (GameFile.factions, at most 32). Beating one of its members moves a player's standing by `kill`
 * (default -10), and with every faction that `hates` it by that faction's `bounty` (default 2). Below `hostile`
 * (default -30) its members who can fight attack that player on sight, its traders will not deal and its quest
 * givers will not speak with them. Bad standing creeps back to 0 by one a minute; players see theirs with /rep.
 * Standing runs -1000..1000 and is kept by faction name. Applies live. */
export interface FactionFile {
  name: string
  /** What players see; empty, the name. */
  title?: string
  /** Its colour (#rrggbb), for its members in clients; players join one as their side with /faction, and
   * members never harm each other nor are taken for enemies by its fighters. */
  color?: string
  kill?: number
  hostile?: number
  hates?: string[]
  bounty?: number
}
export interface Drain {
  attr: string
  per_sec: number
}
export interface WeatherDef {
  name: string
  color: string
  intensity: number
  sight_mult: number
  drain: Drain[]
}
export type Step =
  | { kind: 'announce'; text: string; banner?: boolean; nearby?: boolean }
  | { kind: 'weather'; name: string | null; duration_secs: number }
  | { kind: 'spawn'; archetype: string; count: number; place: string | null; name?: string | null }
  | { kind: 'hit'; attr: string; delta: number; place: string | null }
  | { kind: 'clear'; archetype: string }
  | { kind: 'drop'; item: string; count: number; place: string | null }
  | { kind: 'zone'; zone: string; on: boolean }
  | { kind: 'wait'; secs: number }
  | { kind: 'defeat'; within_secs: number }
  | { kind: 'reward'; item: string; count: number; everyone?: boolean }
  /** The event's town comes out to its square (a market, a feast, a wedding): lines of `occasion`, maybe dancing. */
  | { kind: 'gather'; secs: number; occasion: string; announce?: string | null; dance?: boolean }
  /** The sickness comes to the event's town: `first` fall ill, it spreads to those close by (spread_pct a minute),
   * and deaths_pct of townsfolk cases end in death. Shops shut while it lasts; a healing draught cures. */
  | { kind: 'plague'; secs: number; spread_pct: number; deaths_pct: number; first: number }
export type DayPart = 'dawn' | 'day' | 'dusk' | 'night'
/** What starts an event on its own, besides its timer. */
export type When =
  | { kind: 'players'; count: number }
  | { kind: 'time'; at: DayPart }
  | { kind: 'season'; is: Season }
  | { kind: 'after'; event: string; outcome?: 'success' | 'failure' | 'either' }
  /** A set time by the calendar (UTC): hour:minute every day, or only on weekday (0 Monday .. 6 Sunday). */
  | { kind: 'calendar'; weekday?: number; hour: number; minute?: number }
/** A weather that stays over one place: drawn as mist there, in effect only inside. */
export interface WeatherZone {
  name: string
  place: string
  weather: string
  active?: boolean
  height?: number
}
export interface EventDef {
  name: string
  every_secs: number
  chance_pct: number
  when?: When | null
  /** Least seconds between two starts by `when` (default 600). */
  cooldown_secs?: number
  /** Where it happens: a local event. */
  place?: string | null
  /** Longest it may run before it has failed; 0 for no limit. */
  limit_secs?: number
  /** It comes to a town picked each time (one with players at it first), not to `place`. */
  roams?: boolean
  steps: Step[]
  /** What happens instead if it fails. */
  fail_steps?: Step[]
}
/** An event in progress (`/api/state` director.runs). */
export interface RunRow {
  name: string
  step: number
  of: number
  failing: boolean
  waiting: string | null
  foes: number
  people: number
  secs: number
  left_secs: number | null
}
export interface DirectorState {
  weather: { name: string; left_secs: number | null } | null
  weathers: string[]
  /** Each weather zone and whether it is on right now. */
  zones?: { name: string; on: boolean }[]
  events: { name: string; fired: number; next_in_secs: number | null; last?: string | null }[]
  runs?: RunRow[]
  /** Newest first: [tick, text]. */
  log: [number, string][]
}

export interface GameFile {
  /** Where new characters come into the world, and those whose saved spot is gone: a place by name. */
  start_place?: string
  move_speed: number
  /** How much faster everyone moves on a road (1 to 4; absent: 1.5). */
  road_speed?: number
  /** Players may harm each other outside safe places (absent: true). Applies live. */
  pvp?: boolean
  entity_radius: number
  interest_radius: number
  max_visible: number
  attrs: AttrFile[]
  actions: ActionFile[]
  archetypes: Archetype[]
  places: Place[]
  routes: Route[]
  /** Hand-placed walls [x0, y0, x1, y1]; absent means generated from the world seed. */
  walls?: [number, number, number, number][]
  items?: ItemDef[]
  weather?: WeatherDef[]
  events?: EventDef[]
  zones?: WeatherZone[]
  /** When it is light and dark (absent: the default cycle). */
  daylight?: Daylight
  /** Experience and levels; absent, there are none. Applies live. */
  levels?: Levels
  /** Quests bots give players. Applies live. */
  quests?: QuestFile[]
  /** Character classes players pick from once. Applies live. */
  classes?: ClassFile[]
  /** The item that is money: what shops take and pay. Applies live. */
  currency?: string
  /** Limits on building (blocks per player and per 256-unit region; 0 for none). Not in the content hash. */
  building?: { per_player: number; per_region: number }
  /** Tuning numbers (reaches in world units, times in seconds). Every field is optional and defaults to RULE_DEFAULTS.
   *  Not in the content hash: edits apply live. */
  rules?: Partial<Rules>
  /** What a new character starts with in their bag (a guest, each visit): item names and counts. Not in the hash. */
  starter?: { item: string; count?: number }[]
  /** "/" commands that act something out nearby. Not in the hash. */
  emotes?: Emote[]
  /** What townsfolk say, by occasion (insult, cry, greet_new, greet_known, hurt, night, rain, rich, news_theft,
   * news_caught, news_brawl, news_ambush, news_fell, news_level, ...): any occasion named replaces its default lines.
   * Lines may hold {name}, {who}, {other}, {place} and {greeting}. Applies live. */
  talk?: Record<string, string[]>
  factions?: FactionFile[]
  /** Loot tables, crafting, farming, land and dungeons. All apply live. */
  loot?: LootTable[]
  recipes?: Recipe[]
  crops?: CropDef[]
  plots?: PlotRules
  dungeons?: DungeonDef[]
  skills?: SkillDef[]
  achievements?: Achievement[]
}

/** How a boss fights (Archetype.boss). Phases are in order of `below`, highest first. */
export interface BossFile {
  phases: BossPhase[]
  /** Seconds into a fight it enrages (0, never): its blows then count enrage_might times. */
  enrage_secs?: number
  enrage_might?: number
  /** A banner for those fighting it when it falls. */
  defeat?: string
}
export interface BossPhase {
  /** Health percent this phase begins at (100 for the first). */
  below: number
  say?: string
  moves: BossMove[]
  /** Bots that come to its side as the phase begins. */
  adds?: { bot: string; count: number }[]
}
export type MoveAt = 'tank' | 'random' | 'farthest' | 'self'
/** An action used every so often; with warn_secs the spot is marked first and can be walked out of. */
export interface BossMove {
  action: string
  every_secs: number
  warn_secs?: number
  at?: MoveAt
  radius?: number
  say?: string
}

export interface Perf {
  tick_avg_ms: number
  tick_p99_ms: number
  recv_ms: number
  sim_ms: number
  snap_ms: number
  send_ms: number
  in_pps: number
  out_pps: number
  mbit: number
  lanes: number
}
export interface NetCounters {
  hellos: number
  connects: number
  inputs: number
  bad: number
  bad_cookie: number
  auth_fail: number
  rejected: number
  timeouts: number
  /** Players who signed in here but had last left through a zone line, so were sent on to that zone. */
  sent_on?: number
}
/** This server's place among the zones, when it runs as one (`--zone`). */
export interface ZoneState {
  index: number
  count: number
  /** Separate maps joined by exits (`--zone-lines`) rather than strips of one world. */
  lines: boolean
  /** Each zone's name, by index. */
  names: string[]
  lo: number
  hi: number
  /** Players handed to another zone, and taken from one. */
  out: number
  in: number
  failed: number
  rejected: number
  ghosts: number
}
/** Entity row: [id, x, y, archetype (-1 none), kind (0 bot, 1 player, 2 driven), ...attrs] */
export type EntityRow = number[]
export interface State {
  tick: number
  hz: number
  live: number
  clients: number
  hash: string
  perf: Perf
  net: NetCounters
  zone?: ZoneState
  game: { zaps: number; respawns: number; rejected_actions: number; ground_items?: number }
  attrs: string[]
  e: EntityRow[]
  shown: number
  /** Players' display names by entity id (bots go by their type). */
  names?: Record<string, string>
  /** Players whose life experience was let go for going over old ground or repeating a deed, most first. */
  looping?: { id: number; name: string; stale: number; xp: number }[]
  director?: DirectorState
  /** Blocks built by players: `ver` changes whenever they do (refetch /api/blocks). */
  blocks?: { ver: number; total: number }
  /** The world file's version, when the server has one (refetch /api/worldfile when it changes). */
  world_ver?: string
}
export interface Info {
  server_key: string
  content_hash: string
  hz: number
  udp_port: number
  world_size: number
  seed: number
  proto: number
  capacity: number
  snapshot_lanes: number
  actions: {
    id: number
    name: string
    range: number
    cooldown_ticks?: number
    requires?: { attr: string; min: number }
    aim?: 'self'
    area?: number
    hits?: Hits
    /** kind: 1 mez, 2 stun, 3 root. */
    hold?: { kind: 1 | 2 | 3; secs: number }
    fx?: Fx
    school?: string
    bolt?: boolean
    desc?: string
    lift?: number
  }[]
}
export interface WorldGeom {
  size: number
  boxes: [number, number, number, number][]
}
export interface PutResult {
  ok: boolean
  error?: string
  reset?: boolean
  dropped_clients?: number
  moved_entities?: number
  content_hash?: string
  saved?: boolean | string
}

export interface Reply<T> {
  status: number
  body: T
}

export async function api<T>(path: string, method = 'GET', body?: unknown, signal?: AbortSignal): Promise<Reply<T>> {
  const res = await fetch(path, {
    method,
    signal,
    headers: method === 'GET' ? undefined : { 'X-Admin': '1', 'Content-Type': 'application/json' },
    body: body === undefined ? undefined : typeof body === 'string' ? body : JSON.stringify(body),
  })
  const text = await res.text()
  let parsed: unknown = null
  try {
    parsed = JSON.parse(text)
  } catch {
    parsed = { error: text.slice(0, 200) }
  }
  return { status: res.status, body: parsed as T }
}

export const KIND_BOT = 0
export const KIND_PLAYER = 1
export const KIND_DRIVEN = 2
/** A mirror of an entity in a neighbouring zone (inert). */
export const KIND_GHOST = 3

export interface ChatLog {
  tick: number
  /** [tick, speaker id, channel (0 say, 1 shout), text, the speaker's name now ("" if gone)] */
  lines: [number, number, number, string, string?][]
}

/** `/cmd` says `text` to those nearby, or `at` when a name follows; `{name}` is who acts it, `{target}` who it is at.
 *  `anim` is a clip played on them, if their model has one of that name. */
export interface Emote {
  cmd: string
  text: string
  at?: string
  anim?: string
  /** Held until they move (sitting): needs a replicated `pose` attribute for others to see it. */
  stays?: boolean
}

/** The game's tuning numbers (proto::rules::Rules). */
export interface Rules {
  /** Talk to a bot, take its quests, use its shop. */
  talk_reach: number
  /** Pick up a pile. */
  pickup_reach: number
  /** The pause after any action before the next. */
  global_cooldown_secs: number
  /** Between using two items. */
  item_cooldown_secs: number
  /** How long a dropped item or loot pile lies before it fades. */
  ground_item_secs: number
  /** How well a bot sees an unlit player at night, as a share of its sight (0..1). */
  sneak_sight: number
  /** A bot gives up a chase after this long with no blows. */
  chase_give_up_secs: number
  /** A fight between players is over after this long with no blows. */
  pvp_quiet_secs: number
  /** Hurting a bot this recently still earns credit when it falls. */
  kill_credit_secs: number
  /** Most players in a party (at least 2). */
  party_size: number
  /** Most players in a raid, a party made one with /raid (at least party_size). */
  raid_size: number
  /** Party mates this near a kill share its experience. */
  party_share_reach: number
  /** Other players this near a kill share its quest credit. */
  quest_share_reach: number
  /** How far a spoken line carries. */
  say_reach: number
  /** How far a shout carries; 0 is the whole world. */
  shout_reach: number
  /** How far a bot's passing remark carries. */
  remark_reach: number
  /** Least time between two lines from one bot. */
  bot_say_secs: number
  /** Most members in a guild (at least 2). */
  guild_size: number
  /** An empty dungeon run closes after this long. */
  dungeon_idle_secs: number
  /** The group finder waits this long for a tank and a healer, then makes a group of whoever is queued. */
  finder_wait_secs: number
  /** Start a trade this near; it is called off past trade_break. */
  trade_reach: number
  trade_break: number
  /** This near a merchant counts as at a market (bank, auction house). */
  market_reach: number
  /** How long an auction listing stays up, in hours. */
  listing_hours: number
  listings_per_seller: number
  /** Most letters waiting for one character. */
  mail_waiting: number
  /** Coin it costs to take back every talent point at a market (/respec). */
  respec_cost: number
  /** Seconds a faction stands in a hold, unopposed, to take it; coin each online member of its holder gets a hold every ten minutes. */
  capture_secs: number
  hold_tithe: number
  /** What falling costs: a share (%) of the experience earned into the level, of the coin in the bag (gone), the
   *  chance (%) each other bag stack is left where they fell, and seconds before rising. All 0 by default. */
  fall_xp_loss_pct: number
  fall_coin_loss_pct: number
  fall_drop_pct: number
  rise_delay_secs: number
  /** How near you hail a bot (H), and how long one that does not fight then stands still for you. */
  hail_reach: number
  hail_secs: number
  /** A wanderer walks to a spot in its home (or within wander_range without one), lingers about
   * wander_linger_secs, and goes on; one that passes a remark to a player stands remark_pause_secs. */
  wander_linger_secs: number
  wander_range: number
  remark_pause_secs: number
  /** At night fighters see night_sight times further, and a player out in the open is set on with
   * night_ambush_pct percent a minute by up to night_pack bots of the types marked ambush. */
  night_sight: number
  night_ambush_pct: number
  night_pack: number
  /** Town life: the chance a minute, in percent, that a quarrel in each town comes to blows, and the chance a
   * second that a cutpurse beside someone lifts their purse. */
  brawl_pct: number
  lift_pct: number
  /** Of an evening a townsperson goes to the inn with inn_pct percent; one killed is replaced by a newcomer
   * newcomer_secs on (0: they come back themselves); plea_pct a minute, one with trouble near asks a player for help. */
  inn_pct: number
  newcomer_secs: number
  plea_pct: number
  /** Game days to a season (0: no seasons), and days the year is shifted on. */
  season_days: number
  season_shift: number
}

export const RULE_DEFAULTS: Rules = {
  talk_reach: 8,
  pickup_reach: 3,
  global_cooldown_secs: 0.5,
  item_cooldown_secs: 0.25,
  ground_item_secs: 300,
  sneak_sight: 0.35,
  chase_give_up_secs: 10,
  pvp_quiet_secs: 8,
  kill_credit_secs: 30,
  party_size: 5,
  raid_size: 20,
  party_share_reach: 80,
  quest_share_reach: 40,
  say_reach: 64,
  shout_reach: 0,
  remark_reach: 24,
  bot_say_secs: 6,
  guild_size: 200,
  dungeon_idle_secs: 30,
  finder_wait_secs: 60,
  trade_reach: 12,
  trade_break: 24,
  market_reach: 16,
  listing_hours: 48,
  listings_per_seller: 12,
  mail_waiting: 50,
  respec_cost: 20,
  capture_secs: 60,
  hold_tithe: 3,
  fall_xp_loss_pct: 0,
  fall_coin_loss_pct: 0,
  fall_drop_pct: 0,
  rise_delay_secs: 0,
  hail_reach: 30,
  hail_secs: 30,
  wander_linger_secs: 6,
  wander_range: 30,
  remark_pause_secs: 4,
  night_sight: 1,
  night_ambush_pct: 0,
  night_pack: 2,
  brawl_pct: 0,
  lift_pct: 0,
  inn_pct: 0,
  newcomer_secs: 0,
  plea_pct: 0,
  season_days: 0,
  season_shift: 0,
}
