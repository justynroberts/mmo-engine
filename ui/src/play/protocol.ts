// The game's wire protocol in the browser: the same encrypted handshake, packet framing,
// snapshot decoding and chat channel the Rust clients use (crates/proto). Everything is
// little-endian. The gateway relays datagrams untouched, so nothing here is browser-specific
// except the crypto libraries.
import { x25519 } from '@noble/curves/ed25519.js'
import { chacha20poly1305 } from '@noble/ciphers/chacha.js'
import { argon2idAsync } from '@noble/hashes/argon2.js'
import { blake3 } from '@noble/hashes/blake3.js'

export const PROTO_VERSION = 9
export const HELLO_LEN = 96
export const CHALLENGE_LEN = 53
export const CONNECT_LEN = 55 + 8 + 32 + 16
const TAG = 16
const DATA_HEADER = 9

export const kind = {
  HELLO: 0x01,
  INPUT: 0x02,
  CONNECT: 0x03,
  CHAT_SEND: 0x04,
  DATA: 0x10,
  WELCOME: 0x81,
  SNAPSHOT: 0x82,
  REJECT: 0x83,
  CHALLENGE: 0x84,
  ACCEPT: 0x85,
  CHAT_BATCH: 0x86,
  REDIRECT: 0x87,
  INVENTORY: 0x88,
  GROUND: 0x89,
  /** Spells and blows used nearby. Not decoded yet: the client ignores inner kinds it does not handle. */
  CASTS: 0x8a,
} as const

export const reject = { BAD_VERSION: 1, CONTENT_MISMATCH: 2, SERVER_FULL: 3 } as const

const enc = new TextEncoder()
const dec = new TextDecoder('utf-8', { fatal: true })

export function concat(...parts: Uint8Array[]): Uint8Array {
  const out = new Uint8Array(parts.reduce((n, p) => n + p.length, 0))
  let o = 0
  for (const p of parts) {
    out.set(p, o)
    o += p.length
  }
  return out
}

export const toHex = (b: Uint8Array) => Array.from(b, (x) => x.toString(16).padStart(2, '0')).join('')
export function fromHex(s: string): Uint8Array {
  const clean = s.trim()
  if (clean.length % 2 || /[^0-9a-f]/i.test(clean)) throw new Error('not hex')
  return Uint8Array.from(clean.match(/../g) ?? [], (h) => parseInt(h, 16))
}

const u32le = (v: number) => {
  const b = new Uint8Array(4)
  new DataView(b.buffer).setUint32(0, v >>> 0, true)
  return b
}
const u16le = (v: number) => {
  const b = new Uint8Array(2)
  new DataView(b.buffer).setUint16(0, v, true)
  return b
}
const u64le = (v: bigint) => {
  const b = new Uint8Array(8)
  new DataView(b.buffer).setBigUint64(0, v, true)
  return b
}

/** blake3 derive_key: the context string selects the mode, the parts are the key material. */
function derive(context: string, ...parts: Uint8Array[]): Uint8Array {
  return blake3(concat(...parts), { context: enc.encode(context), dkLen: 32 })
}

const randomBytes = (n: number) => crypto.getRandomValues(new Uint8Array(n))
const nonce = (ctr: number) => concat(u32le(ctr), new Uint8Array(8))
const eq = (a: Uint8Array, b: Uint8Array) => a.length === b.length && a.every((v, i) => v === b[i])

export const GUEST = new Uint8Array(32)

// ---------------------------------------------------------------- identities

/** The same player on any machine: Argon2id of the passphrase, salted by the account name. */
export async function identityFromPassphrase(account: string, passphrase: string): Promise<Uint8Array> {
  const salt = derive('mmo-engine 1 account salt', enc.encode(account.trim().toLowerCase())).slice(0, 16)
  return argon2idAsync(enc.encode(passphrase), salt, { t: 2, m: 19 * 1024, p: 1, dkLen: 32, asyncTick: 20 })
}

export function recoveryCode(secret: Uint8Array): string {
  const hex = toHex(secret)
  const key = derive('mmo-engine 1 recovery checksum')
  const sum = blake3(secret, { key })
  const groups = Array.from({ length: 8 }, (_, i) => hex.slice(i * 8, i * 8 + 8))
  groups.push(toHex(sum.slice(0, 2)))
  return groups.join('-')
}

/** Inverse of recoveryCode; null on a typo. */
export function parseRecoveryCode(code: string): Uint8Array | null {
  const clean = code.replace(/[^0-9a-fA-F]/g, '').toLowerCase()
  if (clean.length !== 68) return null
  const secret = fromHex(clean.slice(0, 64))
  return recoveryCode(secret).replace(/-/g, '') === clean ? secret : null
}

// ------------------------------------------------------------------ sessions

/** Sliding window of the last 64 counters seen. */
class Replay {
  private max = 0
  private bits = 0n
  private any = false
  check(c: number): boolean {
    if (!this.any || c > this.max) return true
    const d = this.max - c
    return d < 64 && ((this.bits >> BigInt(d)) & 1n) === 0n
  }
  accept(c: number) {
    if (!this.any) {
      this.max = c
      this.bits = 1n
      this.any = true
    } else if (c > this.max) {
      const shift = c - this.max
      this.bits = shift >= 64 ? 0n : (this.bits << BigInt(shift)) & ((1n << 64n) - 1n)
      this.bits |= 1n
      this.max = c
    } else {
      this.bits |= 1n << BigInt(this.max - c)
    }
  }
}

export class Session {
  private txCtr = 0
  private replay = new Replay()
  constructor(
    readonly sid: number,
    private txKey: Uint8Array,
    private rxKey: Uint8Array,
  ) {}

  seal(plain: Uint8Array): Uint8Array {
    if (this.txCtr === 0xffffffff) throw new Error('session counter exhausted')
    const ctr = this.txCtr++
    const head = concat(Uint8Array.of(kind.DATA), u32le(this.sid), u32le(ctr))
    return concat(head, chacha20poly1305(this.txKey, nonce(ctr), head).encrypt(plain))
  }

  /** Forged, corrupted and replayed packets return null and change nothing. */
  open(pkt: Uint8Array): Uint8Array | null {
    if (pkt.length < DATA_HEADER + TAG || pkt[0] !== kind.DATA) return null
    const ctr = new DataView(pkt.buffer, pkt.byteOffset).getUint32(5, true)
    if (!this.replay.check(ctr)) return null
    try {
      const plain = chacha20poly1305(this.rxKey, nonce(ctr), pkt.subarray(0, DATA_HEADER)).decrypt(pkt.subarray(DATA_HEADER))
      this.replay.accept(ctr)
      return plain
    } catch {
      return null
    }
  }
}

export type HsError = 'malformed' | 'key-mismatch'

/** The client half of the handshake: HELLO, then CONNECT once a CHALLENGE arrives, then ACCEPT. */
export class ClientHs {
  private secret: Uint8Array = randomBytes(32)
  readonly pub: Uint8Array = x25519.getPublicKey(this.secret)
  private dh1: Uint8Array = new Uint8Array(32)
  private kS2c: Uint8Array = new Uint8Array(32)
  private serverPub: Uint8Array | null = null
  private connect: Uint8Array | null = null
  readonly idPub: Uint8Array

  constructor(
    private pinned: Uint8Array | null,
    private defsHash: bigint,
    private identity: Uint8Array | null = null,
  ) {
    this.idPub = identity ? x25519.getPublicKey(identity) : GUEST
  }

  hello(): Uint8Array {
    const out = new Uint8Array(HELLO_LEN)
    out[0] = kind.HELLO
    out.set(u16le(PROTO_VERSION), 1)
    out.set(this.pub, 3)
    return out
  }

  /** The CONNECT to (re)send, once a CHALLENGE has been accepted. */
  connectPacket(): Uint8Array | null {
    return this.connect
  }

  onChallenge(pkt: Uint8Array): HsError | null {
    if (this.connect) return null
    if (pkt.length !== CHALLENGE_LEN || pkt[0] !== kind.CHALLENGE) return 'malformed'
    const spub = pkt.slice(1, 33)
    if (this.pinned && !eq(this.pinned, spub)) return 'key-mismatch'
    let dh1: Uint8Array
    try {
      dh1 = x25519.getSharedSecret(this.secret, spub)
    } catch {
      return 'malformed'
    }
    const kC2s = derive('mmo-engine 1 hs c2s', dh1, this.pub, spub)
    this.kS2c = derive('mmo-engine 1 hs s2c', dh1, this.pub, spub)
    this.dh1 = dh1
    this.serverPub = spub
    const head = concat(Uint8Array.of(kind.CONNECT), u16le(PROTO_VERSION), this.pub, pkt.slice(33, 37), pkt.slice(37, 53))
    const body = concat(u64le(this.defsHash), this.idPub)
    this.connect = concat(head, chacha20poly1305(kC2s, nonce(0), head).encrypt(body))
    return null
  }

  /** On success: the session and the encoded welcome that came with it. */
  onAccept(pkt: Uint8Array): { session: Session; welcome: Uint8Array } | null {
    const spub = this.serverPub
    if (!spub || pkt.length < 1 + 36 + TAG || pkt[0] !== kind.ACCEPT) return null
    let body: Uint8Array
    try {
      body = chacha20poly1305(this.kS2c, nonce(0), pkt.subarray(0, 1)).decrypt(pkt.subarray(1))
    } catch {
      return null
    }
    if (body.length < 36) return null
    const ephPub = body.slice(0, 32)
    const sid = new DataView(body.buffer, body.byteOffset).getUint32(32, true)
    let dh2: Uint8Array
    let dh3: Uint8Array
    try {
      dh2 = x25519.getSharedSecret(this.secret, ephPub)
      dh3 = this.identity ? x25519.getSharedSecret(this.identity, ephPub) : new Uint8Array(32)
    } catch {
      return null
    }
    const parts = [this.dh1, dh2, dh3, this.pub, spub, ephPub, this.idPub]
    const kC2s = derive('mmo-engine 1 session c2s', ...parts)
    const kS2c = derive('mmo-engine 1 session s2c', ...parts)
    return { session: new Session(sid, kC2s, kS2c), welcome: body.slice(36) }
  }
}

// ---------------------------------------------------------------- messages

export interface Welcome {
  entity: number
  tick: number
  hz: number
  worldSize: number
  seed: bigint
}

export function decodeWelcome(b: Uint8Array): Welcome | null {
  if (b.length < 22 || b[0] !== kind.WELCOME) return null
  const v = new DataView(b.buffer, b.byteOffset, b.length)
  return { entity: v.getUint32(1, true), tick: v.getUint32(5, true), hz: v.getUint8(9), worldSize: v.getUint32(10, true), seed: v.getBigUint64(14, true) }
}

export interface Redirect {
  zone: number
  ip: string
  port: number
}

/** The zone that just handed us over says where to reconnect. */
export function decodeRedirect(b: Uint8Array): Redirect | null {
  if (b.length !== 8 || b[0] !== kind.REDIRECT) return null
  return { zone: b[1]!, ip: `${b[2]}.${b[3]}.${b[4]}.${b[5]}`, port: b[6]! | (b[7]! << 8) }
}

/** One bag slot: the item index, or -1 when empty, and how many. */
export interface BagSlot {
  item: number
  count: number
}

export function decodeInventory(b: Uint8Array): BagSlot[] | null {
  if (b.length < 2 || b[0] !== kind.INVENTORY || b.length !== 2 + b[1]! * 2) return null
  const out: BagSlot[] = []
  for (let i = 0; i < b[1]!; i++) out.push({ item: b[2 + i * 2]! - 1, count: b[3 + i * 2]! })
  return out
}

/** Something lying on the ground, positions in world units. */
export interface GroundItem {
  id: number
  x: number
  y: number
  item: number
  count: number
}

export function decodeGround(b: Uint8Array): GroundItem[] | null {
  if (b.length < 2 || b[0] !== kind.GROUND || b.length !== 2 + b[1]! * 14) return null
  const dv = new DataView(b.buffer, b.byteOffset, b.byteLength)
  const out: GroundItem[] = []
  for (let i = 0; i < b[1]!; i++) {
    const o = 2 + i * 14
    out.push({ id: dv.getUint32(o, true), x: dpos(dv.getUint32(o + 4, true)), y: dpos(dv.getUint32(o + 8, true)), item: b[o + 12]!, count: b[o + 13]! })
  }
  return out
}

export interface Input {
  seq: number
  ackTick: number
  dirX: number
  dirY: number
  action: number
  target: number
  /** The action's extra argument: for a build or break, (layer & 0xff) | material << 8. */
  arg?: number
  chatAck: number
}

export function encodeInput(i: Input): Uint8Array {
  const b = new Uint8Array(22)
  const v = new DataView(b.buffer)
  b[0] = kind.INPUT
  v.setUint32(1, i.seq >>> 0, true)
  v.setUint32(5, i.ackTick >>> 0, true)
  v.setInt8(9, i.dirX)
  v.setInt8(10, i.dirY)
  v.setUint8(11, i.action)
  v.setUint32(12, i.target >>> 0, true)
  v.setUint16(16, (i.arg ?? 0) & 0xffff, true)
  v.setUint32(18, i.chatAck >>> 0, true)
  return b
}

// ---------------------------------------------------------------- snapshots

export interface Defs {
  /** Per replicated attribute, in wire order. */
  replicated: { name: string; min: number; max: number; bits: number }[]
}

export interface Entry {
  id: number
  x: number
  y: number
  /** The block layer its feet are in; GROUND (-128) on the bare ground. */
  z: number
  attrs: number[]
}

/** Entry.z for something on the bare ground (proto::world::GROUND). */
export const GROUND = -128

export const POS_SCALE = 16
const RING = 16
export const MAX_REPL = 6

export interface SnapshotInfo {
  tick: number
  baselineTick: number
  lastInputSeq: number
  stale: boolean
}

export type DecodeError = 'truncated' | 'not-snapshot' | 'missing-baseline'

/** What the client holds of the world: a ring of recent snapshots by tick. */
export class ClientView {
  private ring: { tick: number; entries: Entry[] }[] = Array.from({ length: RING }, () => ({ tick: 0, entries: [] }))
  lastTick = 0
  entries(): Entry[] {
    if (this.lastTick === 0) return []
    const s = this.ring[this.lastTick % RING]!
    return s.tick === this.lastTick ? s.entries : []
  }
  get(id: number): Entry | undefined {
    return this.entries().find((e) => e.id === id)
  }
  /** @internal */
  slot(tick: number) {
    return this.ring[tick % RING]!
  }
}

export function decodeSnapshot(buf: Uint8Array, defs: Defs, view: ClientView): { info: SnapshotInfo } | { error: DecodeError } {
  if (buf.length < 17) return { error: 'truncated' }
  if (buf[0] !== kind.SNAPSHOT) return { error: 'not-snapshot' }
  const v = new DataView(buf.buffer, buf.byteOffset, buf.length)
  const tick = v.getUint32(1, true)
  const baselineTick = v.getUint32(5, true)
  const lastInputSeq = v.getUint32(9, true)
  const nChanged = v.getUint16(13, true)
  const nRemoved = v.getUint16(15, true)
  let p = 17
  const changed: Entry[] = []
  for (let i = 0; i < nChanged; i++) {
    if (p + 13 > buf.length) return { error: 'truncated' }
    const e: Entry = { id: v.getUint32(p, true), x: v.getUint32(p + 4, true), y: v.getUint32(p + 8, true), z: v.getInt8(p + 12), attrs: [] }
    p += 13
    for (const a of defs.replicated) {
      if (a.bits <= 8) {
        if (p + 1 > buf.length) return { error: 'truncated' }
        e.attrs.push(v.getUint8(p))
        p += 1
      } else {
        if (p + 2 > buf.length) return { error: 'truncated' }
        e.attrs.push(v.getUint16(p, true))
        p += 2
      }
    }
    changed.push(e)
  }
  const removed = new Set<number>()
  for (let i = 0; i < nRemoved; i++) {
    if (p + 4 > buf.length) return { error: 'truncated' }
    removed.add(v.getUint32(p, true))
    p += 4
  }
  const info: SnapshotInfo = { tick, baselineTick, lastInputSeq, stale: false }
  if (view.lastTick !== 0 && tick <= view.lastTick) return { info: { ...info, stale: true } }
  let base: Entry[] = []
  if (baselineTick !== 0) {
    const b = view.slot(baselineTick)
    if (b.tick !== baselineTick) return { error: 'missing-baseline' }
    base = b.entries
  }
  // baseline + changed - removed, kept sorted by id like the server does
  const merged = new Map<number, Entry>()
  for (const e of base) if (!removed.has(e.id)) merged.set(e.id, e)
  for (const e of changed) merged.set(e.id, e)
  const slot = view.slot(tick)
  slot.tick = tick
  slot.entries = [...merged.values()].sort((a, b) => a.id - b.id)
  view.lastTick = tick
  return { info }
}

export const dpos = (q: number) => q / POS_SCALE
export function dequantize(a: { min: number; max: number; bits: number }, q: number): number {
  const levels = 2 ** Math.min(a.bits, 16) - 1
  return a.min + (q / levels) * (a.max - a.min)
}

// --------------------------------------------------------------------- chat

export const MAX_TEXT = 120
/** SAY and SHOUT are heard; NAME (client to server only) claims a display name (crates/proto/src/names.rs). */
export const channel = { SAY: 0, SHOUT: 1, NAME: 2, BANNER: 3 } as const

export interface ChatMsg {
  seq: number
  from: number
  channel: number
  text: string
}

export function encodeChatSend(cseq: number, ch: number, text: string): Uint8Array {
  let t = enc.encode(text)
  if (t.length > MAX_TEXT) t = t.slice(0, MAX_TEXT)
  return concat(Uint8Array.of(kind.CHAT_SEND), u32le(cseq), Uint8Array.of(ch, t.length), t)
}

export function decodeChatBatch(b: Uint8Array): { ack: number; msgs: ChatMsg[] } | null {
  if (b.length < 6 || b[0] !== kind.CHAT_BATCH) return null
  const v = new DataView(b.buffer, b.byteOffset, b.length)
  const ack = v.getUint32(1, true)
  const n = b[5]!
  let p = 6
  const msgs: ChatMsg[] = []
  for (let i = 0; i < n; i++) {
    if (p + 10 > b.length) return null
    const len = b[p + 9]!
    if (p + 10 + len > b.length) return null
    let text: string
    try {
      text = dec.decode(b.subarray(p + 10, p + 10 + len))
    } catch {
      return null
    }
    msgs.push({ seq: v.getUint32(p, true), from: v.getUint32(p + 4, true), channel: b[p + 8]!, text })
    p += 10 + len
  }
  return p === b.length ? { ack, msgs } : null
}

/** Outgoing queue with one message in flight, and in-order receive. */
export class ChatClient {
  private nextCseq = 1
  private outbox: { cseq: number; ch: number; text: string }[] = []
  private nextExpected = 1
  say(ch: number, text: string) {
    this.outbox.push({ cseq: this.nextCseq++, ch, text })
  }
  /** The datagram to (re)send this tick, if a message is waiting for its ack. */
  outgoing(): Uint8Array | null {
    const m = this.outbox[0]
    return m ? encodeChatSend(m.cseq, m.ch, m.text) : null
  }
  ack(): number {
    return (this.nextExpected - 1) >>> 0
  }
  onBatch(ack: number, msgs: ChatMsg[]): ChatMsg[] {
    while (this.outbox[0] && this.outbox[0].cseq <= ack) this.outbox.shift()
    const fresh: ChatMsg[] = []
    for (const m of msgs) {
      if (((m.seq - this.nextExpected) | 0) >= 0) {
        this.nextExpected = (m.seq + 1) >>> 0
        fresh.push(m)
      }
    }
    return fresh
  }
  unsent() {
    return this.outbox.length
  }
}
