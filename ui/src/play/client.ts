// A game session over the web gateway: handshake, input loop, snapshots, chat, reconnect.
import {
  ChatClient, ClientHs, ClientView, decodeChatBatch, decodeRedirect, decodeSnapshot, decodeWelcome, dequantize, dpos, encodeInput, fromHex,
  kind, reject, type ChatMsg, type Defs, type Session,
} from './protocol'

export interface PlayInfo {
  server_key: string
  content_hash: string
  proto: number
  hz: number
  world_size: number
  seed: number
  move_speed: number
  entity_radius: number
  interest_radius: number
  attrs: { name: string; min: number; max: number; bits: number; replicated: boolean }[]
  replicated: number[]
  actions: { id: number; name: string; range: number }[]
  boxes: [number, number, number, number][]
  /** Set when the server is one zone of several: its index, and each zone's web base URL (null if unknown). */
  zone?: number | null
  zones?: (string | null)[] | null
}

/** Where to open the WebSocket for a zone, from the base URL the operator configured. */
export function zoneUrl(base: string): string {
  const u = new URL(base)
  u.protocol = u.protocol === 'https:' ? 'wss:' : 'ws:'
  u.pathname = '/ws'
  u.search = ''
  return u.toString()
}

export type Status = 'connecting' | 'playing' | 'rejected' | 'closed'

export interface Ent {
  id: number
  x: number
  y: number
  /** Decoded replicated attributes by name. */
  attrs: Record<string, number>
}

export interface Handlers {
  onStatus: (s: Status, detail?: string) => void
  onChat: (m: ChatMsg) => void
}

/** WebSocket.send wants a plain ArrayBuffer view; a copy keeps the types and the data honest. */
const bin = (b: Uint8Array) => new Uint8Array(b)

export function defsOf(info: PlayInfo): Defs {
  return { replicated: info.replicated.map((i) => info.attrs[i]!) }
}

const REJECTS: Record<number, string> = {
  [reject.BAD_VERSION]: 'This page is for a different protocol version than the server.',
  [reject.CONTENT_MISMATCH]: 'The game definitions changed since this page loaded. Reload it.',
  [reject.SERVER_FULL]: 'The server is full.',
}

export class GameClient {
  status: Status = 'connecting'
  entity = 0
  view = new ClientView()
  /** Round trip from sending an input to seeing it acknowledged, in ms. */
  rtt = 0
  lastRx = 0
  private ws: WebSocket | null = null
  private hs: ClientHs | null = null
  private session: Session | null = null
  private chat = new ChatClient()
  private seq = 0
  private sent = new Map<number, number>()
  private dir: [number, number] = [0, 0]
  private action = 0
  private target = 0
  private timers: ReturnType<typeof setInterval>[] = []
  private stopped = false
  private defs: Defs

  constructor(
    private info: PlayInfo,
    private identity: Uint8Array | null,
    private url: string,
    private h: Handlers,
  ) {
    this.defs = defsOf(info)
  }

  start() {
    this.stopped = false
    this.open()
  }

  stop() {
    this.stopped = true
    this.teardown()
    this.set('closed')
  }

  private set(s: Status, detail?: string) {
    this.status = s
    this.h.onStatus(s, detail)
  }

  private teardown() {
    for (const t of this.timers) clearInterval(t)
    this.timers = []
    const ws = this.ws
    this.ws = null
    if (ws) {
      ws.onmessage = ws.onclose = ws.onerror = null
      try {
        ws.close()
      } catch {
        /* already closed */
      }
    }
    this.session = null
  }

  private open() {
    this.teardown()
    this.hs = new ClientHs(fromHex(this.info.server_key), BigInt('0x' + this.info.content_hash), this.identity)
    this.view = new ClientView()
    this.set('connecting')
    let ws: WebSocket
    try {
      ws = new WebSocket(this.url)
    } catch {
      this.set('closed', 'could not open a connection')
      return
    }
    ws.binaryType = 'arraybuffer'
    this.ws = ws
    ws.onopen = () => {
      const hello = () => {
        const hs = this.hs
        if (!hs || this.session || ws.readyState !== WebSocket.OPEN) return
        ws.send(bin(hs.connectPacket() ?? hs.hello()))
      }
      hello()
      // HELLO until a CHALLENGE arrives, then CONNECT until the ACCEPT does.
      this.timers.push(setInterval(hello, 250))
    }
    ws.onmessage = (e) => this.onPacket(new Uint8Array(e.data as ArrayBuffer))
    ws.onclose = () => {
      if (this.stopped || this.ws !== ws) return
      this.ws = null
      this.retrySoon('connection lost')
    }
    ws.onerror = () => undefined // onclose follows
  }

  private retrySoon(why: string) {
    this.teardown()
    this.set('connecting', why + ', reconnecting')
    this.timers.push(setInterval(() => {
      if (this.stopped) return
      this.open()
    }, 1500))
    // One attempt per interval: open() clears timers, so this fires once per cycle.
  }

  private onPacket(p: Uint8Array) {
    this.lastRx = performance.now()
    const hs = this.hs
    switch (p[0]) {
      case kind.CHALLENGE: {
        const err = hs?.onChallenge(p)
        if (err) {
          this.stopped = true
          this.teardown()
          this.set('rejected', err === 'key-mismatch' ? 'This is not the server this page was built for.' : 'The server sent something unreadable.')
        }
        return
      }
      case kind.ACCEPT: {
        if (this.session || !hs) return
        const r = hs.onAccept(p)
        const w = r && decodeWelcome(r.welcome)
        if (!r || !w) return
        this.session = r.session
        this.entity = w.entity
        this.view = new ClientView()
        this.set('playing')
        this.timers.push(setInterval(() => this.tick(), 1000 / this.info.hz))
        this.timers.push(setInterval(() => this.watchdog(), 500))
        return
      }
      case kind.REJECT:
        if (!this.session) {
          this.stopped = true
          this.teardown()
          this.set('rejected', REJECTS[p[1] ?? 0] ?? 'The server refused the connection.')
        }
        return
      case kind.DATA: {
        const inner = this.session?.open(p)
        if (!inner) return
        if (inner[0] === kind.CHAT_BATCH) {
          const b = decodeChatBatch(inner)
          if (b) for (const m of this.chat.onBatch(b.ack, b.msgs)) this.h.onChat(m)
        } else if (inner[0] === kind.REDIRECT) {
          const r = decodeRedirect(inner)
          const base = r && this.info.zones?.[r.zone]
          if (!r) return
          if (!base) {
            this.stopped = true
            this.teardown()
            this.set('closed', 'You crossed into another zone that has no web address.')
            return
          }
          // Same identity, new server: an ordinary handshake, so the character follows us.
          this.url = zoneUrl(base)
          this.info = { ...this.info, zone: r.zone }
          this.open()
          this.set('connecting', 'moving to the next zone')
        } else if (inner[0] === kind.SNAPSHOT) {
          const r = decodeSnapshot(inner, this.defs, this.view)
          if ('info' in r && !r.info.stale) {
            const t = this.sent.get(r.info.lastInputSeq)
            if (t !== undefined) {
              this.rtt = performance.now() - t
              for (const k of this.sent.keys()) if (k <= r.info.lastInputSeq) this.sent.delete(k)
            }
          }
        }
        return
      }
    }
  }

  /** A server that vanished or restarted goes quiet; start over with fresh keys. */
  private watchdog() {
    if (this.status === 'playing' && performance.now() - this.lastRx > 3000) this.retrySoon('the server went quiet')
  }

  private tick() {
    const ws = this.ws
    const s = this.session
    if (!ws || !s || ws.readyState !== WebSocket.OPEN) return
    const chat = this.chat.outgoing()
    if (chat) ws.send(bin(s.seal(chat)))
    this.seq++
    this.sent.set(this.seq, performance.now())
    if (this.sent.size > 128) this.sent.delete(this.sent.keys().next().value as number)
    ws.send(
      bin(
        s.seal(
          encodeInput({ seq: this.seq, ackTick: this.view.lastTick, dirX: this.dir[0], dirY: this.dir[1], action: this.action, target: this.target, chatAck: this.chat.ack() }),
        ),
      ),
    )
    this.action = 0 // an action is a single request, not a held state
    this.target = 0
  }

  /** Movement intent, each axis -1, 0 or 1. */
  setMove(dx: number, dy: number) {
    this.dir = [Math.sign(dx) * 127, Math.sign(dy) * 127]
  }

  act(actionId: number, target: number) {
    this.action = actionId
    this.target = target
  }

  say(text: string, shout: boolean) {
    const t = text.trim()
    if (t) this.chat.say(shout ? 1 : 0, t)
  }

  /** The world as of the newest snapshot, positions in world units. */
  entities(): Ent[] {
    const rep = this.info.replicated.map((i) => this.info.attrs[i]!)
    return this.view.entries().map((e) => {
      const attrs: Record<string, number> = {}
      rep.forEach((a, k) => (attrs[a.name] = dequantize(a, e.attrs[k] ?? 0)))
      return { id: e.id, x: dpos(e.x), y: dpos(e.y), attrs }
    })
  }
}
