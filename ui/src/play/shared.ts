import type { PlayInfo } from './client'
import { NameBook } from './names'

/** What the 3D scene works out each frame and the HUD reads. */
export interface Target {
  id: number
  dist: number
  inRange: boolean
  /** 0..1, or null when the thing has no health attribute. */
  hp: number | null
  hpText: string
  /** True when the player picked it, false when it is just the nearest. */
  locked: boolean
}

/** Mutable state passed by ref between the HUD (keys, buttons) and the scene (clicks, frames). */
export interface Weather {
  name: string
  color: string
  intensity: number
}

/** A weather zone that is on (/play/info weather_zones): its weather stands over a place. */
export interface ZoneView extends Weather {
  /** The weather's name (`name` is the zone's). */
  weather: string
  x: number
  y: number
  r: number
  height: number
}

/** The zone (smallest, where they overlap) holding (x, y), or null. */
export function zoneAt(zones: readonly ZoneView[], x: number, y: number): ZoneView | null {
  let best: ZoneView | null = null
  for (const z of zones) if ((x - z.x) ** 2 + (y - z.y) ** 2 <= z.r * z.r && (!best || z.r < best.r)) best = z
  return best
}

/** A titled place (/play/info locations): players are told its title as they walk in and out. */
export interface LocationView {
  title: string
  x: number
  y: number
  r: number
}

/** How far past a location's edge we still count as in it, so walking along the edge does not flicker its title. */
export const LEAVE_MARGIN = 3

/** The title of the location holding (x, y), the smallest where they overlap (a village inside a forest is the
 *  village), or null. `here` is the one we were in: we stay in it until LEAVE_MARGIN past its edge. */
export function locationAt(locs: readonly LocationView[], x: number, y: number, here: string | null): string | null {
  let best: LocationView | null = null
  let bestR = Infinity
  for (const l of locs) {
    const r = l.r + (l.title === here ? LEAVE_MARGIN : 0)
    if ((x - l.x) ** 2 + (y - l.y) ** 2 <= r * r && r < bestR) {
      best = l
      bestR = r
    }
  }
  return best?.title ?? null
}

export interface Shared {
  /** What the server says the weather is everywhere. */
  weather: Weather | null,
  /** Weather zones that are on. */
  zones: ZoneView[]
  /** Titled places. */
  locations: LocationView[]
  /** What the scene is drawing where we stand: a zone's weather inside one, else `weather`. The HUD names it. */
  shown: Weather | null
  target: Target | null
  /** An entity the player chose by clicking or pressing T; null means "whatever is nearest". */
  selected: number | null
  fps: number
  /** Item kinds, kept fresh because the console can edit them while people play. */
  items: NonNullable<PlayInfo['items']>
  /** Camera distance multiplier, 0.5 (close) to 1.8 (far). */
  zoom: number
  /** What everyone is called (/play/names). */
  names: NameBook
}

export const newShared = (): Shared => ({ weather: null, zones: [], locations: [], shown: null, target: null, selected: null, fps: 0, items: [], zoom: 1, names: new NameBook() })

export const ZOOM_MIN = 0.5
export const ZOOM_MAX = 1.8
export const clampZoom = (z: number) => Math.max(ZOOM_MIN, Math.min(ZOOM_MAX, z))

/** The attribute shown as health: the one named "health", else the first replicated one. */
export function healthAttr(info: PlayInfo) {
  return info.attrs.find((a) => a.name === 'health') ?? info.replicated.map((i) => info.attrs[i]!)[0]
}

/** Bag slot keys: Shift plus 1 to 8 uses it. */
export const BAG_SLOTS = 8
export const ACT_USE = 200
export const ACT_DROP = 216
export const ACT_PICKUP = 250
/** Jump: in the air for a moment, and nothing can target you while you are (a dodge). */
export const ACT_JUMP = 251
const JUMP_HEIGHT = 2.6

/** How high a character is on its jump, from the server's replicated `airborne` attribute (the share still to come). */
export function jumpLift(info: PlayInfo, attrs: Record<string, number>): number {
  const a = info.attrs.find((x) => x.name === 'airborne')
  const v = a ? attrs[a.name] : undefined
  if (!a || v === undefined || v <= a.min) return 0
  const k = 1 - Math.min(1, (v - a.min) / (a.max - a.min || 1))
  return 4 * JUMP_HEIGHT * k * (1 - k)
}
export const PICKUP_RANGE = 3
