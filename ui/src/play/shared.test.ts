import { describe, expect, it } from 'vitest'
import { LEAVE_MARGIN, locationAt, zoneAt, type LocationView, type ZoneView } from './shared'

const zone = (name: string, x: number, y: number, r: number): ZoneView => ({ name, weather: name, color: '#ff0000', intensity: 0.5, x, y, r, height: 20 })

describe('zoneAt', () => {
  it('finds the zone round a point, the smallest where they overlap, and none outside', () => {
    const zones = [zone('big', 100, 100, 50), zone('spring', 110, 100, 3)]
    expect(zoneAt(zones, 90, 100)?.name).toBe('big')
    expect(zoneAt(zones, 111, 101)?.name).toBe('spring')
    expect(zoneAt(zones, 300, 300)).toBeNull()
  })
})

describe('locationAt', () => {
  const locs: LocationView[] = [
    { title: 'Hollow Forest', x: 100, y: 100, r: 60 },
    { title: 'Ashby', x: 120, y: 100, r: 10 },
  ]
  it('names the smallest location round us, and none outside them all', () => {
    expect(locationAt(locs, 60, 100, null)).toBe('Hollow Forest')
    expect(locationAt(locs, 121, 101, 'Hollow Forest')).toBe('Ashby')
    expect(locationAt(locs, 300, 300, null)).toBeNull()
  })
  it('keeps us in a location until a little past its edge, so its title does not flicker', () => {
    expect(locationAt(locs, 100 + 60 + LEAVE_MARGIN / 2, 100, null)).toBeNull()
    expect(locationAt(locs, 100 + 60 + LEAVE_MARGIN / 2, 100, 'Hollow Forest')).toBe('Hollow Forest')
    expect(locationAt(locs, 100 + 60 + LEAVE_MARGIN + 1, 100, 'Hollow Forest')).toBeNull()
  })
})
