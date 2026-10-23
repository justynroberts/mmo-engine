import type { Daylight } from './daylight'
export type Behaviour = 'idle' | 'wander' | 'chase' | 'flee'
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
  /** Talk to a bot of a type. */
  | { kind: 'talk'; archetype: string }
/** A quest (GameFile.quests, at most 64). Not in the content hash: edits apply live, nobody is disconnected. */
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
}
export type Seek = 'player' | 'bot' | 'any'
export type Cond =
  | { kind: 'always' }
  | { kind: 'sees'; who: Seek; within: number }
  | { kind: 'attr_below'; attr: string; value: number }
  | { kind: 'attr_above'; attr: string; value: number }
  | { kind: 'in_place'; place: string }
  | { kind: 'chance'; pct: number }
export type Act =
  | { kind: 'idle' | 'wander' | 'chase' | 'flee' | 'attack' }
  | { kind: 'patrol'; route: string }
  | { kind: 'goto'; place: string }
  | { kind: 'say'; text: string; shout: boolean }
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
}
/** What a place's sound and music may be (proto::game::SOUNDS, MUSIC). */
export const SOUNDS = ['sea', 'river', 'forest', 'night', 'wind', 'rain', 'fire', 'town', 'market', 'crypt', 'holy', 'battle'] as const
export const MUSIC = ['explore', 'town', 'night', 'battle', 'eerie', 'holy', 'city', 'none'] as const
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
export type DayPart = 'dawn' | 'day' | 'dusk' | 'night'
/** What starts an event on its own, besides its timer. */
export type When =
  | { kind: 'players'; count: number }
  | { kind: 'time'; at: DayPart }
  | { kind: 'after'; event: string; outcome?: 'success' | 'failure' | 'either' }
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
  /** The item that is money: what shops take and pay. Applies live. */
  currency?: string
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
