import { describe as group, expect, it } from 'vitest'
import { DEFAULT_DAYLIGHT, describe, moment, nightShare, phase, problem, type Daylight } from './daylight'

// The same cases as crates/proto/src/daylight.rs.
group('daylight', () => {
  it('a cycle repeats', () => {
    const day = 24 * 60000
    expect(phase(DEFAULT_DAYLIGHT, day * 10.25)).toBeCloseTo(0.25, 9)
    expect(phase({ ...DEFAULT_DAYLIGHT, offset: 0.55 } as Daylight, day * 10.25)).toBeCloseTo(0.8, 9)
  })

  it('a clock follows sunrise and sunset in its time zone', () => {
    const d: Daylight = { mode: 'clock', sunrise: '06:00', sunset: '18:00', utc_offset_minutes: 60 }
    const at = (h: number) => (h - 1) * 3600000
    expect(nightShare(d)).toBeCloseTo(0.5, 9)
    expect(phase(d, at(6))).toBeCloseTo(0, 9)
    expect(phase(d, at(12))).toBeCloseTo(0.25, 9)
    expect(phase(d, at(18))).toBeCloseTo(0.5, 9)
    expect(phase(d, at(24))).toBeCloseTo(0.75, 9)
    const summer: Daylight = { mode: 'clock', sunrise: '05:00', sunset: '21:00' }
    expect(nightShare(summer)).toBeCloseTo(1 / 3, 9)
    expect(phase(summer, 13 * 3600000)).toBeCloseTo(1 / 3, 9)
  })

  it('names the moments and refuses what the server refuses', () => {
    expect(describe(DEFAULT_DAYLIGHT, moment(DEFAULT_DAYLIGHT, 'noon'))).toBe('noon')
    expect(describe(DEFAULT_DAYLIGHT, moment(DEFAULT_DAYLIGHT, 'midnight'))).toBe('night')
    expect(problem({ mode: 'cycle', day_minutes: 0, night_share: 0.3 })).toMatch(/minutes/)
    expect(problem({ mode: 'clock', sunrise: '25:00', sunset: '18:00' })).toMatch(/06:30/)
    expect(problem({ mode: 'fixed', phase: 1.5 })).toMatch(/0 to 1/)
    expect(problem(DEFAULT_DAYLIGHT)).toBeNull()
  })
})
