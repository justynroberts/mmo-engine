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
}
export interface ActionFile {
  id: number
  name: string
  cooldown_ticks: number
  range: number
  requires: { attr: string; min: number } | null
  effects: EffectFile[]
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
}
export interface Route {
  name: string
  points: [number, number][]
  loop: boolean
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
}
export interface GameFile {
  move_speed: number
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
  game: { zaps: number; respawns: number; rejected_actions: number }
  attrs: string[]
  e: EntityRow[]
  shown: number
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
  actions: { id: number; name: string; range: number }[]
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
  /** [tick, speaker id, channel (0 say, 1 shout), text] */
  lines: [number, number, number, string][]
}
