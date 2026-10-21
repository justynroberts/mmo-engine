// The game file's daylight schedule (proto::daylight): when it is light and when it is dark. The console only shows
// where the day is and edits the schedule; the same sum is in Rust and in the Godot client, and the cases in
// daylight.test.ts are the ones crates/proto/src/daylight.rs tests.

export type Daylight =
  | { mode: 'cycle'; day_minutes: number; night_share: number; offset?: number }
  | { mode: 'clock'; sunrise: string; sunset: string; utc_offset_minutes?: number }
  | { mode: 'fixed'; phase: number }

export const DEFAULT_DAYLIGHT: Daylight = { mode: 'cycle', day_minutes: 24, night_share: 1 / 3, offset: 0 }

const mod = (a: number, n: number) => ((a % n) + n) % n

/** "HH:MM" to minutes after midnight, or null. */
export function minutes(hm: string): number | null {
  const m = /^\s*(\d{1,2}):(\d{2})\s*$/.exec(hm)
  if (!m) return null
  const h = Number(m[1])
  const mi = Number(m[2])
  return h < 24 && mi < 60 ? h * 60 + mi : null
}

export function nightShare(d: Daylight): number {
  if (d.mode === 'cycle') return d.night_share
  if (d.mode === 'fixed') return 1 / 3
  return mod((minutes(d.sunrise) ?? 360) - (minutes(d.sunset) ?? 1140), 1440) / 1440
}

/** Where the day is at unixMs: 0 sunrise, 1 - nightShare sunset. */
export function phase(d: Daylight, unixMs: number): number {
  if (d.mode === 'fixed') return d.phase
  if (d.mode === 'cycle') return mod(unixMs / (d.day_minutes * 60000) + (d.offset ?? 0), 1)
  const local = mod(unixMs / 60000 + (d.utc_offset_minutes ?? 0), 1440)
  const since = mod(local - (minutes(d.sunrise) ?? 360), 1440)
  const night = nightShare(d)
  const dayLen = (1 - night) * 1440
  return since < dayLen ? (since / dayLen) * (1 - night) : 1 - night + ((since - dayLen) / Math.max(night * 1440, 1e-9)) * night
}

/** The phase of a named moment under a schedule: dawn, noon, dusk or midnight. */
export function moment(d: Daylight, at: 'dawn' | 'noon' | 'dusk' | 'midnight'): number {
  const night = nightShare(d)
  return { dawn: 0.02, noon: (1 - night) / 2, dusk: 1 - night - 0.02, midnight: 1 - night / 2 }[at]
}

/** A phase in words. */
export function describe(d: Daylight, p: number): string {
  const day = 1 - nightShare(d)
  if (p >= day) return p < day + 0.03 ? 'dusk' : p > 0.97 ? 'before dawn' : 'night'
  const u = p / day
  return u < 0.06 ? 'dawn' : u < 0.4 ? 'morning' : u < 0.6 ? 'noon' : u < 0.94 ? 'afternoon' : 'sunset'
}

/** Why a schedule would be refused, or null (the same rules as the server's). */
export function problem(d: Daylight): string | null {
  if (d.mode === 'cycle') {
    if (!(d.day_minutes >= 1 && d.day_minutes <= 10080)) return 'A day lasts 1 to 10080 minutes.'
    if (!(d.night_share >= 0 && d.night_share <= 0.9)) return 'Night is 0 to 90% of the day.'
  } else if (d.mode === 'clock') {
    const r = minutes(d.sunrise)
    const s = minutes(d.sunset)
    if (r === null || s === null) return 'Sunrise and sunset look like 06:30.'
    if (r === s) return 'Sunrise and sunset must differ.'
    if (Math.abs(d.utc_offset_minutes ?? 0) > 840) return 'The time zone is within 14 hours of UTC.'
  } else if (!(d.phase >= 0 && d.phase < 1)) return 'The fixed moment is 0 to 1.'
  return null
}
