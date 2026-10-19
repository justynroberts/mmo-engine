import { describe, expect, it } from 'vitest'
import {
  ChatClient, ClientHs, ClientView, concat, decodeChatBatch, decodeRedirect, decodeSnapshot, decodeWelcome, encodeChatSend, encodeInput,
  identityFromPassphrase, kind, parseRecoveryCode, recoveryCode, Session,
} from './protocol'

describe('identities', () => {
  it('derives the same character as the Rust client from the same account and passphrase', async () => {
    // Printed by `bots --account Justyn --passphrase "x y z" --print-recovery` (Rust, Argon2id + blake3).
    const rust = '299756db-cbb23a83-fe82fa08-88fc5bac-04dcb88e-4716273c-47751076-12676686-00f9'
    const id = await identityFromPassphrase('Justyn', 'x y z')
    expect(recoveryCode(id)).toBe(rust)
    expect(await identityFromPassphrase('  justyn ', 'x y z')).toEqual(id) // account names are trimmed and case-folded
    expect(parseRecoveryCode(rust)).toEqual(id)
  }, 20000)

  it('catches a typo in a recovery code instead of becoming a different character', () => {
    const id = Uint8Array.from({ length: 32 }, (_, i) => i * 7)
    const code = recoveryCode(id)
    expect(parseRecoveryCode(code.toUpperCase().replace(/-/g, ' '))).toEqual(id)
    const typo = code.slice(0, 4) + (code[4] === '0' ? '1' : '0') + code.slice(5)
    expect(parseRecoveryCode(typo)).toBeNull()
    expect(parseRecoveryCode('nope')).toBeNull()
  })
})

describe('handshake and sessions', () => {
  it('rejects a CHALLENGE from a server that is not the pinned one', () => {
    const hs = new ClientHs(new Uint8Array(32).fill(9), 1n)
    const chal = new Uint8Array(53)
    chal[0] = kind.CHALLENGE
    chal.set(new Uint8Array(32).fill(1), 1)
    expect(hs.onChallenge(chal)).toBe('key-mismatch')
    expect(hs.connectPacket()).toBeNull()
    expect(hs.hello().length).toBe(96)
  })

  it('seals and opens, rejects tampering and replays, and tolerates reordering', () => {
    const k1 = new Uint8Array(32).fill(3)
    const k2 = new Uint8Array(32).fill(4)
    const a = new Session(7, k1, k2)
    const b = new Session(7, k2, k1)
    const p1 = a.seal(new TextEncoder().encode('one'))
    const p2 = a.seal(new TextEncoder().encode('two'))
    expect(p1.length).toBe(3 + 25)
    expect(new TextDecoder().decode(b.open(p2)!)).toBe('two')
    expect(new TextDecoder().decode(b.open(p1)!)).toBe('one') // inside the window
    expect(b.open(p1)).toBeNull() // replay
    for (let i = 0; i < p2.length; i++) {
      const bad = p2.slice()
      bad[i]! ^= 1
      expect(new Session(7, k2, k1).open(bad), `flip at ${i}`).toBeNull()
    }
  })
})

describe('messages', () => {
  it('encodes inputs to the exact layout the server decodes', () => {
    // The same input as proto::wire's input_roundtrip.
    const b = encodeInput({ seq: 7, ackTick: 99, dirX: -1, dirY: 127, action: 1, target: 0x01000042, arg: 0x07fe, chatAck: 5 })
    expect(Array.from(b)).toEqual([2, 7, 0, 0, 0, 99, 0, 0, 0, 255, 127, 1, 0x42, 0, 0, 1, 0xfe, 0x07, 5, 0, 0, 0])
    expect(encodeInput({ seq: 1, ackTick: 0, dirX: 0, dirY: 0, action: 0, target: 0, chatAck: 0 })[16]).toBe(0)
  })

  it('decodes a welcome', () => {
    const b = new Uint8Array(22)
    const v = new DataView(b.buffer)
    b[0] = kind.WELCOME
    v.setUint32(1, 9, true)
    v.setUint32(5, 5, true)
    b[9] = 20
    v.setUint32(10, 200_000, true)
    v.setBigUint64(14, 77n, true)
    expect(decodeWelcome(b)).toEqual({ entity: 9, tick: 5, hz: 20, worldSize: 200_000, seed: 77n })
    expect(decodeWelcome(b.slice(0, 21))).toBeNull()
  })

  const snap = (tick: number, base: number, changed: [number, number, number, number][], removed: number[]) => {
    const parts = [new Uint8Array(17)]
    const v = new DataView(parts[0]!.buffer)
    parts[0]![0] = kind.SNAPSHOT
    v.setUint32(1, tick, true)
    v.setUint32(5, base, true)
    v.setUint32(9, 3, true)
    v.setUint16(13, changed.length, true)
    v.setUint16(15, removed.length, true)
    for (const [id, x, y, hp] of changed) {
      const e = new Uint8Array(14)
      const ev = new DataView(e.buffer)
      ev.setUint32(0, id, true)
      ev.setUint32(4, x, true)
      ev.setUint32(8, y, true)
      ev.setInt8(12, id === 3 ? 5 : -128) // the layer: entity 3 stands five layers up, the rest on the ground
      e[13] = hp
      parts.push(e)
    }
    for (const id of removed) {
      const r = new Uint8Array(4)
      new DataView(r.buffer).setUint32(0, id, true)
      parts.push(r)
    }
    return concat(...parts)
  }
  const defs = { replicated: [{ name: 'health', min: 0, max: 100, bits: 8 }] }

  it('applies deltas against an acknowledged baseline and drops removed entities', () => {
    const view = new ClientView()
    expect('info' in decodeSnapshot(snap(1, 0, [[1, 16, 32, 255], [5, 48, 64, 128]], []), defs, view)).toBe(true)
    expect(view.entries().map((e) => e.id)).toEqual([1, 5])
    decodeSnapshot(snap(2, 1, [[3, 1, 1, 10], [1, 20, 32, 255]], [5]), defs, view)
    expect(view.entries().map((e) => [e.id, e.x, e.z])).toEqual([[1, 20, -128], [3, 1, 5]])
    // Old and duplicate packets change nothing; a delta against a tick we lack is reported.
    const stale = decodeSnapshot(snap(2, 1, [], []), defs, view)
    expect('info' in stale && stale.info.stale).toBe(true)
    expect(decodeSnapshot(snap(9, 7, [], []), defs, view)).toEqual({ error: 'missing-baseline' })
    expect(decodeSnapshot(snap(3, 0, [[1, 1, 1, 1]], []).slice(0, 20), defs, view)).toEqual({ error: 'truncated' })
  })

  it('round-trips chat and keeps the ack and ordering rules of the Rust client', () => {
    const send = encodeChatSend(4, 1, 'hello ☃')
    expect(send[0]).toBe(kind.CHAT_SEND)
    expect(new TextDecoder().decode(send.slice(7))).toBe('hello ☃')
    const text = new TextEncoder().encode('hi')
    const batch = concat(Uint8Array.of(kind.CHAT_BATCH, 1, 0, 0, 0, 1), new Uint8Array([1, 0, 0, 0, 9, 0, 0, 0, 0, text.length]), text)
    const decoded = decodeChatBatch(batch)!
    expect(decoded).toEqual({ ack: 1, msgs: [{ seq: 1, from: 9, channel: 0, text: 'hi' }] })
    // Wrong lengths are rejected rather than half-parsed.
    expect(decodeChatBatch(batch.slice(0, batch.length - 1))).toBeNull()
    const c = new ChatClient()
    c.say(0, 'a')
    c.say(0, 'b')
    expect(c.outgoing()![1]).toBe(1) // one in flight: the first
    expect(c.onBatch(1, decoded.msgs).length).toBe(1)
    expect(c.onBatch(1, decoded.msgs).length).toBe(0) // a resend is not delivered twice
    expect(c.ack()).toBe(1)
    expect(c.outgoing()![1]).toBe(2) // the first was acked, the second goes next
  })
})

describe('zone redirect', () => {
  it('reads the 8 byte message the Rust server sends and refuses anything else', () => {
    // Rust: encode_redirect(Redirect { zone: 2, ip: [10, 0, 0, 7], port: 7779 })
    const b = Uint8Array.of(kind.REDIRECT, 2, 10, 0, 0, 7, 7779 & 255, 7779 >> 8)
    expect(decodeRedirect(b)).toEqual({ zone: 2, ip: '10.0.0.7', port: 7779 })
    expect(decodeRedirect(b.slice(0, 7))).toBeNull()
    expect(decodeRedirect(Uint8Array.of(kind.CHAT_BATCH, ...b.slice(1)))).toBeNull()
  })
})
