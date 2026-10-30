import { applyBossLine, type BossMark } from './boss'

describe('boss marks', () => {
  it('are marked, timed and taken back', () => {
    const m = new Map<number, BossMark>()
    expect(applyBossLine(m, 'T|7|305|300|4|2|fire', 1000)).toBe(true)
    expect(m.get(7)).toEqual({ x: 305, y: 300, r: 4, from: 1000, until: 3000, fx: 'fire' })
    expect(applyBossLine(m, 'C|7', 1500)).toBe(true)
    expect(m.size).toBe(0)
    expect(applyBossLine(m, 'T|8|x|300|4|2|', 0)).toBe(false)
    expect(applyBossLine(m, 'hello', 0)).toBe(false)
  })
})
