import type { PlayInfo } from './client'

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

export interface Shared {
  /** What the server says the weather is; the scene draws it, the HUD names it. */
  weather: Weather | null,
  target: Target | null
  /** An entity the player chose by clicking or pressing T; null means "whatever is nearest". */
  selected: number | null
  fps: number
  /** Item kinds, kept fresh because the console can edit them while people play. */
  items: NonNullable<PlayInfo['items']>
  /** Camera distance multiplier, 0.5 (close) to 1.8 (far). */
  zoom: number
}

export const newShared = (): Shared => ({ weather: null, target: null, selected: null, fps: 0, items: [], zoom: 1 })

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
export const PICKUP_RANGE = 3
