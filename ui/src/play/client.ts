// A game session over the web gateway: handshake, input loop, snapshots, chat, reconnect.
import {
  channel, ChatClient, ClientHs, ClientView, decodeChatBatch, decodeRedirect, decodeSnapshot, decodeWelcome, dequantize, dpos, encodeInput, fromHex,
  kind, reject, decodeCrowd, decodeGround, decodeInventory, type BagSlot, type ChatMsg, type CrowdEntry, type Defs, type GroundItem, type Session,
} from './protocol'
import { applyBossLine, type BossMark } from './boss'
import { Interpolator } from './interp'
import { GROUND, Predictor, World } from './sim'
import { decodeRegions, maskOf, regionAsk, REGION, type BlockCol } from './blocks'

/** Regions each way from ours whose blocks we keep (a 5 x 5 square, as the walls). */
const BLOCK_WINDOW = 2
import { WallStore, type WallsMeta } from './walls'
import { playUrl } from './base'

export interface PlayInfo {
  /** Sides, callings, recipes, crops and dungeons (all reached by "/" chat commands). */
  realm?: { factions?: { name: string; title?: string; color?: string }[] }
  server_key: string
  content_hash: string
  proto: number
  hz: number
  world_size: number
  seed: number
  move_speed: number
  /** How much faster movement is on a road (older servers do not say). */
  road_speed?: number
  entity_radius: number
  interest_radius: number
  attrs: { name: string; min: number; max: number; bits: number; replicated: boolean }[]
  replicated: number[]
  actions: { id: number; name: string; range: number }[]
  boxes: [number, number, number, number][]
  /** What each inline box is (`WALL` or `SCENERY`, from './walls'); missing means a wall. */
  box_class?: number[]
  /** How the walls travel: inline in `boxes`, or fetched by tile from /play/tiles. */
  walls?: WallsMeta
  /** Set when the server is one zone of several: its index, and each zone's web base URL (null if unknown). */
  zone?: number | null
  zones?: (string | null)[] | null
  /** Zone lines: every zone is a map of its own, joined by exits; a client handed to one loads it afresh. */
  zone_lines?: boolean
  /** Every zone's name, by index (zone lines). */
  zone_names?: string[] | null
  /** What kinds of item exist, by index (the order in the game file). */
  items?: { name: string; color: string; stack_max: number; usable: boolean }[]
  /** The weather right now, or null for clear sky. */
  weather?: { name: string; color: string; intensity: number } | null
  /** Weather zones that are on: a weather standing over a place. */
  weather_zones?: import('./shared').ZoneView[]
  /** Titled places: what to tell the player as they walk in and out. */
  locations?: import('./shared').LocationView[]
  /** Building, when the server offers it: blocks on a grid, listed at /play/blocks. */
  build?: { block: number; reach: number; max_stack: number; kinds: number; act_build: number; act_break: number }
  /** The game file's tuning numbers (only those the play page reads are listed). */
  rules?: { pickup_reach?: number; talk_reach?: number }
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
  /** The block layer its feet are in (GROUND on the bare ground). */
  z: number
  /** Decoded replicated attributes by name. */
  attrs: Record<string, number>
}

/** What to draw this frame: everyone interpolated, you predicted. */
export interface Frame {
  /** Everyone else, drawn slightly in the past. */
  others: Ent[]
  /** Your own character, at its predicted position (null until the first snapshot). */
  self: Ent | null
}

export interface Handlers {
  onStatus: (s: Status, detail?: string) => void
  onChat: (m: ChatMsg) => void
  /** Zone lines: an exit handed us to zone `zone`, a map of its own. The client has stopped; load that zone's info
   *  from its web address (`zones[zone]`) and start a new client there with the same identity. */
  onMoved?: (zone: number) => void
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
  private world: World
  private pred: Predictor | null = null
  private interp: Interpolator<Ent>
  private visual: { x: number; y: number } | null = null
  private lastFrameAt = 0
  private selfAttrs: Record<string, number> = {}
  /** The most recent frame computed, for readouts that do not drive the render loop. */
  last: Frame = { others: [], self: null }
  /** Your bag (8 slots) and what lies near you, as the server last said. */
  bag: BagSlot[] = []
  /** The walls near you (all of them in a small world). */
  readonly walls: WallStore
  private worldRev = -1
  ground: GroundItem[] = []
  /** A boss fight's marks on the ground near us, by id (`boss.ts`). */
  readonly bossMarks = new Map<number, BossMark>()
  /** The crowd past our snapshot, by part (each part replaced when it comes again). */
  crowdParts: CrowdEntry[][] = []
  /** Blocks players have built, by cy << 15 | cx; `blocksRev` counts changes. */
  readonly blocks = new Map<number, BlockCol>()
  blocksRev = 0
  private blocksVer = 0
  /** Blocks stream by region round us (`/play/blocks/r`): the version held of each, by ry << 16 | rx. False once
   * the server has said it has no such thing (an older one: everything from /play/blocks, as before). */
  private blockRegions = new Map<number, number>()
  private byRegion = true
  private blocksTimer: ReturnType<typeof setInterval> | null = null

  constructor(
    public info: PlayInfo,
    private identity: Uint8Array | null,
    private url: string,
    private h: Handlers,
  ) {
    this.defs = defsOf(info)
    this.walls = new WallStore(info.world_size, info.walls, info.boxes, info.box_class)
    this.world = new World(info.world_size, this.walls.list(), this.walls.roadCells(), info.road_speed, this.walls.waterCells())
    this.worldRev = this.walls.rev
    this.interp = new Interpolator<Ent>(info.hz)
  }

  /** The map editor can change walls while people are playing; adopt them so prediction stays right. */
  setWalls(fresh: Pick<PlayInfo, 'boxes' | 'box_class' | 'walls'>) {
    this.walls.adopt(fresh.walls, fresh.boxes, fresh.box_class)
  }

  /** How many layers above the blocks' ground a character with its feet in layer z stands at (x, y): this page
   *  draws the ground flat, so blocks are drawn from their column's base. */
  standLevel(x: number, y: number, z: number): number {
    if (z === GROUND) return 0
    return z - (this.world.floor(x, y, this.info.entity_radius) ?? z)
  }

  /** Rebuild the collision world if the walls we hold changed. Cheap when they did not. */
  private syncWorld() {
    if (this.worldRev === this.walls.rev) return
    this.worldRev = this.walls.rev
    this.world = new World(this.info.world_size, this.walls.list(), this.walls.roadCells(), this.info.road_speed, this.walls.waterCells())
    for (const c of this.blocks.values()) this.world.setColumn(c.cx, c.cy, { base: c.base, mask: maskOf(c.kinds) })
  }

  /** What changed in the built blocks since last time: kept for drawing, and put in the collision world so
   * prediction stops at them as the server does. */
  private async pollBlocks(): Promise<void> {
    if (this.byRegion) return this.pollBlockRegions()
    try {
      const r = await fetch(playUrl(`/play/blocks?since=${this.blocksVer}`))
      if (!r.ok) return
      const d = (await r.json()) as { ver: number; full: boolean; cols: [number, number, number, string, number[]][] }
      if (d.full) {
        for (const c of this.blocks.values()) this.world.setColumn(c.cx, c.cy, { base: 0, mask: 0 })
        this.blocks.clear()
      }
      for (const [cx, cy, base, kinds, owners] of d.cols) {
        const k = (cy << 15) | cx
        if (kinds) this.blocks.set(k, { cx, cy, base, kinds, owners })
        else this.blocks.delete(k)
        this.world.setColumn(cx, cy, { base, mask: maskOf(kinds) })
      }
      if (d.full || d.cols.length) this.blocksRev++
      this.blocksVer = d.ver
    } catch {
      // the next poll tries again
    }
  }

  /** The regions round us: each asked for with the version held, only those that changed come back; regions well
   * behind us are let go of. */
  private async pollBlockRegions(): Promise<void> {
    const at = this.pred ?? this.visual
    const block = this.info.build?.block
    if (!at || !block) return
    try {
      const r = await fetch(playUrl(`/play/blocks/r?c=${regionAsk(at.x, at.y, block, BLOCK_WINDOW, this.blockRegions)}`))
      if (r.status === 404) {
        this.byRegion = false
        return this.pollBlocks()
      }
      if (!r.ok) return
      const regions = decodeRegions(await r.arrayBuffer())
      if (!regions) return
      const drop = (rk: number) => {
        for (const [k, c] of this.blocks)
          if (((Math.floor(c.cy / REGION) << 16) | Math.floor(c.cx / REGION)) === rk) {
            this.blocks.delete(k)
            this.world.setColumn(c.cx, c.cy, { base: 0, mask: 0 })
          }
      }
      for (const g of regions) {
        const rk = (g.ry << 16) | g.rx
        drop(rk)
        for (const c of g.cols) {
          this.blocks.set((c.cy << 15) | c.cx, c)
          this.world.setColumn(c.cx, c.cy, { base: c.base, mask: maskOf(c.kinds) })
        }
        if (g.ver) this.blockRegions.set(rk, g.ver)
        else this.blockRegions.delete(rk)
      }
      const span = REGION * block
      const [rx, ry] = [Math.floor(at.x / span), Math.floor(at.y / span)]
      let dropped = false
      for (const rk of [...this.blockRegions.keys()])
        if (Math.abs((rk & 0xffff) - rx) > BLOCK_WINDOW + 1 || Math.abs((rk >> 16) - ry) > BLOCK_WINDOW + 1) {
          drop(rk)
          this.blockRegions.delete(rk)
          dropped = true
        }
      if (regions.length || dropped) this.blocksRev++
    } catch {
      // the next poll tries again
    }
  }

  start() {
    this.stopped = false
    this.open()
    if (this.info.build && !this.blocksTimer) {
      void this.pollBlocks()
      this.blocksTimer = setInterval(() => void this.pollBlocks(), 1000)
    }
  }

  stop() {
    if (this.blocksTimer) clearInterval(this.blocksTimer)
    this.blocksTimer = null
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
    this.pred = null
    this.visual = null
    this.interp.reset()
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
        this.pred = null
        this.visual = null
        this.interp.reset()
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
          if (b)
            for (const m of this.chat.onBatch(b.ack, b.msgs)) {
              if (m.channel === channel.BOSS && m.from === 0) applyBossLine(this.bossMarks, m.text, performance.now())
              else this.h.onChat(m)
            }
        } else if (inner[0] === kind.INVENTORY) {
          const b = decodeInventory(inner)
          if (b) this.bag = b
        } else if (inner[0] === kind.GROUND) {
          const g = decodeGround(inner)
          if (g) this.ground = g
        } else if (inner[0] === kind.CROWD) {
          const c = decodeCrowd(inner)
          if (c) {
            this.crowdParts.length = c.parts
            this.crowdParts[c.part] = c.list
          }
        } else if (inner[0] === kind.REDIRECT) {
          const r = decodeRedirect(inner)
          const base = r && this.info.zones?.[r.zone]
          if (!r) return
          if (this.info.zone_lines && this.h.onMoved) {
            // Another map: the page loads it and starts again there (Play.tsx), so this client is done.
            this.stopped = true
            this.teardown()
            this.h.onMoved(r.zone)
            return
          }
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
            this.onWorld(r.info.lastInputSeq)
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
    this.syncWorld()
    this.pred?.applyInput(this.world, this.seq, this.dir[0], this.dir[1])
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

  /** Movement intent, each axis -1 to 1 (a stick gives fractions, a key gives whole numbers). */
  setMove(dx: number, dy: number) {
    const q = (v: number) => Math.round(Math.max(-1, Math.min(1, v || 0)) * 127)
    this.dir = [q(dx), q(dy)]
  }

  /** A new snapshot arrived: feed the interpolator and correct the prediction against the server. */
  private onWorld(lastInputSeq: number) {
    const all = this.entities()
    this.interp.push(this.view.lastTick, all, performance.now())
    const me = all.find((e) => e.id === this.entity)
    if (!me) return
    this.selfAttrs = me.attrs
    if (!this.pred) {
      this.pred = new Predictor(me.x, me.y, this.info.move_speed, this.info.hz, this.info.entity_radius)
      this.pred.z = me.z
      this.visual = { x: me.x, y: me.y }
      this.walls.update(me.x, me.y)
      return
    }
    this.walls.update(me.x, me.y)
    this.pred.setSpeed(this.info.move_speed * (me.attrs.speed ?? 1)) // a haste or a slow
    this.syncWorld()
    const err = this.pred.reconcile(this.world, me.x, me.y, me.z, lastInputSeq)
    // A big correction is the server doing something we could not know about (respawn, zone handoff): jump.
    if (err > 8 && this.visual) this.visual = { x: this.pred.x, y: this.pred.y }
  }

  /** The picture for this instant. Call once per rendered frame. */
  frame(now = performance.now()): Frame {
    const dt = this.lastFrameAt ? Math.min(0.1, (now - this.lastFrameAt) / 1000) : 0
    this.lastFrameAt = now
    const others = this.interp.sample(now).filter((e) => e.id !== this.entity)
    let self: Ent | null = null
    if (this.pred && this.visual) {
      // The predicted position is exact but steps 20 times a second; ease the drawn one toward it.
      const k = 1 - Math.exp(-dt * 25)
      this.visual.x += (this.pred.x - this.visual.x) * k
      this.visual.y += (this.pred.y - this.visual.y) * k
      self = { id: this.entity, x: this.visual.x, y: this.visual.y, z: this.pred.z, attrs: this.selfAttrs }
    }
    this.last = { others, self }
    return this.last
  }

  /** Inputs sent that the server has not confirmed yet. */
  get unacked(): number {
    return this.pred?.unacked ?? 0
  }

  act(actionId: number, target: number) {
    this.action = actionId
    this.target = target
  }

  say(text: string, shout: boolean) {
    const t = text.trim()
    if (t) this.chat.say(shout ? 1 : 0, t)
  }

  /** Ask to be known by a name. The server answers with a chat line from id 0 (taken, not allowed, or done). */
  setName(name: string) {
    const t = name.trim()
    if (t) this.chat.say(2, t)
  }

  /** The world as of the newest snapshot, positions in world units. */
  entities(): Ent[] {
    const rep = this.info.replicated.map((i) => this.info.attrs[i]!)
    return this.view.entries().map((e) => {
      const attrs: Record<string, number> = {}
      rep.forEach((a, k) => (attrs[a.name] = dequantize(a, e.attrs[k] ?? 0)))
      return { id: e.id, x: dpos(e.x), y: dpos(e.y), z: e.z, attrs }
    })
  }
}
