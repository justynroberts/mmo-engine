import { describe, expect, it } from 'vitest'
import { zoneUrl } from './client'

describe('zoneUrl', () => {
  it('turns a web base URL into its WebSocket endpoint', () => {
    expect(zoneUrl('http://10.0.0.7:8080')).toBe('ws://10.0.0.7:8080/ws')
    expect(zoneUrl('https://z1.example.com/anything?x=1')).toBe('wss://z1.example.com/ws')
  })
})
