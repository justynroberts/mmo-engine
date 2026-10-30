// A boss fight's marks on the ground (chat channel 14, server `boss.rs`): "T|id|x|y|r|secs|fx" marks a circle that
// a blow lands in after `secs`, "C|id" takes one back. Drawn by PlayScene filling up until it lands.

export interface BossMark {
  x: number
  y: number
  r: number
  /** performance.now() when it was marked and when it lands. */
  from: number
  until: number
  fx: string
}

/** Apply one channel-14 line to the marks held; false if it was not one. */
export function applyBossLine(marks: Map<number, BossMark>, text: string, now: number): boolean {
  const f = text.split('|')
  if (f[0] === 'C' && f.length >= 2) {
    marks.delete(Number(f[1]))
    return true
  }
  if (f[0] !== 'T' || f.length < 7) return false
  const [id, x, y, r, secs] = f.slice(1, 6).map(Number) as [number, number, number, number, number]
  if (![id, x, y, r, secs].every(Number.isFinite)) return false
  marks.set(id, { x, y, r, from: now, until: now + secs * 1000, fx: f[6] ?? '' })
  return true
}
