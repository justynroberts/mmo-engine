// MIT License - Copyright (c) fintonlabs.com
// The desk. Everything it knows comes from the server's admin interface through window.desk (preload.js); it holds
// no password. State is polled; every control is one POST and says in the last line what the server answered.
'use strict'

const $ = (id) => document.getElementById(id)
const STATE_EVERY_MS = 2500
const CHAT_EVERY_MS = 3000
const GAME_EVERY_MS = 60000
const STATE_LIMIT = 20000
const CHANNELS = ['say', 'shout', 'name', 'banner', 'whisper', 'whisper', 'party', 'remark']
const PICK_PX = 10

const S = {
  info: null,
  game: null,
  st: null,
  chat: [],
  world: null,
  terrain: null,
  view: { x: 0, y: 0, k: 0.01 },
  fitted: false,
  armed: false,
  selected: 0,
  follow: false,
  banner: true,
  killArmedAt: 0,
  linkOk: false,
  timers: [],
}

// ---------------------------------------------------------------------------------------------- small helpers

function el(tag, props, ...kids) {
  const n = document.createElement(tag)
  for (const [k, v] of Object.entries(props || {})) {
    if (k === 'class') n.className = v
    else if (k === 'text') n.textContent = v
    else if (k.startsWith('on')) n.addEventListener(k.slice(2), v)
    else if (v !== false && v !== null && v !== undefined) n.setAttribute(k, v === true ? '' : String(v))
  }
  for (const c of kids) n.append(c)
  return n
}

function say(text, bad) {
  const n = $('last')
  n.textContent = text
  n.className = bad ? 'err' : ''
}

// One of each control at a time: a second press while the first is still on its way is dropped, so a double click
// cannot spawn twice.
const inFlight = new Set()
async function post(path, body, done) {
  if (inFlight.has(path)) return null
  inFlight.add(path)
  say('Sending…')
  const r = await window.desk.api('POST', path, body)
  inFlight.delete(path)
  if (!r.ok) {
    say(r.error, true)
    return null
  }
  say(typeof done === 'function' ? done(r.data) : done)
  void pollState()
  return r.data
}

const mod = (a, n) => ((a % n) + n) % n
const clock = (hm) => {
  const m = /^\s*(\d{1,2}):(\d{2})\s*$/.exec(hm || '')
  return m && Number(m[1]) < 24 && Number(m[2]) < 60 ? Number(m[1]) * 60 + Number(m[2]) : null
}

// The game file's daylight schedule, the same sums as ui/src/daylight.ts and crates/proto/src/daylight.rs.
function nightShare(d) {
  if (d.mode === 'cycle') return d.night_share
  if (d.mode === 'fixed') return 1 / 3
  return mod((clock(d.sunrise) ?? 360) - (clock(d.sunset) ?? 1140), 1440) / 1440
}
function phaseNow(d, ms) {
  if (d.mode === 'fixed') return d.phase
  if (d.mode === 'cycle') return mod(ms / (d.day_minutes * 60000) + (d.offset ?? 0), 1)
  const local = mod(ms / 60000 + (d.utc_offset_minutes ?? 0), 1440)
  const since = mod(local - (clock(d.sunrise) ?? 360), 1440)
  const night = nightShare(d)
  const dayLen = (1 - night) * 1440
  return since < dayLen ? (since / dayLen) * (1 - night) : 1 - night + ((since - dayLen) / Math.max(night * 1440, 1e-9)) * night
}
function moment(d, at) {
  const night = nightShare(d)
  return { dawn: 0.02, noon: (1 - night) / 2, dusk: 1 - night - 0.02, midnight: 1 - night / 2 }[at]
}
function dayWord(d, p) {
  const day = 1 - nightShare(d)
  if (p >= day) return p < day + 0.03 ? 'dusk' : p > 0.97 ? 'before dawn' : 'night'
  const u = p / day
  return u < 0.06 ? 'dawn' : u < 0.4 ? 'morning' : u < 0.6 ? 'noon' : u < 0.94 ? 'afternoon' : 'sunset'
}
const daylight = () => (S.game && S.game.daylight) || { mode: 'cycle', day_minutes: 24, night_share: 1 / 3, offset: 0 }

function ago(tick) {
  if (!S.st) return ''
  const s = Math.max(0, Math.round((S.st.tick - tick) / (S.st.hz || 20)))
  return s < 90 ? s + 's' : s < 5400 ? Math.round(s / 60) + 'm' : Math.round(s / 3600) + 'h'
}
function span(secs) {
  if (secs === null || secs === undefined) return ''
  const s = Math.max(0, Math.round(secs))
  return s < 90 ? s + 's' : s < 5400 ? Math.round(s / 60) + 'm' : Math.round(s / 3600) + 'h'
}

const archOf = (row) => (S.game && S.game.archetypes && S.game.archetypes[row[3]]) || null
const isPlayer = (row) => row[4] === 1
function nameOf(row) {
  const named = S.st && S.st.names && S.st.names[String(row[0])]
  if (named) return named.split('|')[0]
  const a = archOf(row)
  return a ? a.title || a.name : isPlayer(row) ? 'player ' + row[0] : 'creature ' + row[0]
}
const rowOf = (id) => (S.st && id ? S.st.e.find((r) => r[0] === id) || null : null)

// ---------------------------------------------------------------------------------------------- connect

async function boot() {
  const saved = await window.desk.saved()
  $('c-server').value = saved.server
  $('connect').hidden = false
  if (saved.hasPassword) {
    $('c-secret').placeholder = 'saved'
    const r = await connecting(() => window.desk.connect(saved.server, '', true))
    if (r.ok) return enter(r)
    $('c-error').textContent = r.error
  }
  ;(saved.hasPassword ? $('c-server') : $('c-secret')).focus()
}

/** The form while an answer is awaited: it says so and cannot be sent twice. */
async function connecting(ask) {
  const b = $('c-go')
  b.disabled = true
  b.textContent = 'Connecting…'
  $('c-error').textContent = ''
  const r = await ask()
  b.disabled = false
  b.textContent = 'Connect'
  return r
}

$('connect-form').addEventListener('submit', async (e) => {
  e.preventDefault()
  if ($('c-go').disabled) return
  const r = await connecting(() => window.desk.connect($('c-server').value, $('c-secret').value, $('c-remember').checked))
  if (r.ok) {
    $('c-secret').value = ''
    return enter(r)
  }
  // What was typed is kept, and the field most likely at fault is the one to correct.
  $('c-error').textContent = r.error
  const field = /password/i.test(r.error) ? $('c-secret') : $('c-server')
  field.focus()
  field.select()
})

async function enter(r) {
  S.info = r.info
  $('connect').hidden = true
  $('desk').hidden = false
  $('s-server').textContent = r.server.replace(/^https?:\/\//, '')
  say('Connected.')
  sizeMap()
  await loadGame()
  await pollState()
  void pollChat()
  void loadWorld()
  S.timers.push(setInterval(pollState, STATE_EVERY_MS), setInterval(pollChat, CHAT_EVERY_MS), setInterval(loadGame, GAME_EVERY_MS), setInterval(drawSky, 1000))
  $('map').focus()
}

// ---------------------------------------------------------------------------------------------- reading

async function loadGame() {
  const r = await window.desk.api('GET', '/api/game')
  if (!r.ok) return say(r.error, true)
  const first = !S.game
  S.game = r.data
  if (first) {
    fillSpawn()
    fillItems()
    fillMoments()
  }
  drawSky()
}

let stateBusy = false
async function pollState() {
  if (stateBusy) return
  stateBusy = true
  const r = await window.desk.api('GET', '/api/state?limit=' + STATE_LIMIT)
  stateBusy = false
  if (r.ok && S.st && !S.linkOk) say('The link is back.')
  S.linkOk = r.ok
  const link = $('s-link')
  link.textContent = r.ok ? 'ok' : 'lost'
  link.className = r.ok ? '' : 'lost'
  if (!r.ok) return say(r.error, true)
  S.st = r.data
  if (!S.fitted && !S.world) fit(S.info ? S.info.world_size : 0)
  drawStrip()
  drawWeather()
  drawEvents()
  drawPlayers()
  drawSelected()
  drawLog()
  drawSpawnCount()
  if (S.follow) {
    const row = rowOf(S.selected)
    if (row) {
      S.view.x = row[1]
      S.view.y = row[2]
    }
  }
  drawMap()
}

async function pollChat() {
  const r = await window.desk.api('GET', '/api/chat')
  if (!r.ok) return
  S.chat = r.data.lines || []
  const list = $('chat')
  const stuck = list.scrollTop + list.clientHeight >= list.scrollHeight - 8
  list.replaceChildren(
    ...(S.chat.length
      ? S.chat.slice(-200).map((l) =>
          el('li', {}, el('span', { class: 't', text: ago(l[0]) }), el('span', { class: 'who', text: (l[4] || 'gone') + ' · ' + (CHANNELS[l[2]] || 'ch ' + l[2]) }), el('span', { class: 'text', text: l[3] })),
        )
      : [el('li', {}, el('span', { class: 't', text: '' }), el('span', { class: 'who', text: 'no chat yet' }), el('span', { class: 'text', text: '' }))]),
  )
  if (stuck) list.scrollTop = list.scrollHeight
}

async function loadWorld() {
  $('m-note').textContent = 'loading the land'
  const r = await window.desk.world()
  if (!r.ok) {
    $('m-note').textContent = r.status === 404 ? 'this world has no world file: creatures and places only' : 'no land: ' + r.error
    return
  }
  S.world = r.data
  S.terrain = paintTerrain(r.data)
  for (const set of [S.world.roads, S.world.rivers]) {
    for (const line of set) {
      let x0 = Infinity
      let y0 = Infinity
      let x1 = -Infinity
      let y1 = -Infinity
      for (let i = 0; i < line.length; i += 2) {
        x0 = Math.min(x0, line[i])
        x1 = Math.max(x1, line[i])
        y0 = Math.min(y0, line[i + 1])
        y1 = Math.max(y1, line[i + 1])
      }
      line.box = [x0, y0, x1, y1]
    }
  }
  $('m-note').textContent = ''
  if (!S.fitted) fit(r.data.size)
  drawMap()
}

// ---------------------------------------------------------------------------------------------- the map

const map = $('map')
const ctx = map.getContext('2d')
const css = (name) => getComputedStyle(document.documentElement).getPropertyValue(name).trim()
const INK = { ink: css('--ink'), ink2: css('--ink-2'), ink3: css('--ink-3'), line: css('--line'), line2: css('--line-2'), accent: css('--accent'), bg: css('--bg') }

function sizeMap() {
  const r = map.getBoundingClientRect()
  const d = window.devicePixelRatio || 1
  map.width = Math.max(1, Math.round(r.width * d))
  map.height = Math.max(1, Math.round(r.height * d))
  drawMap()
}
window.addEventListener('resize', sizeMap)

function fit(size) {
  if (!size) return
  const r = map.getBoundingClientRect()
  if (r.width < 2) return
  S.view = { x: size / 2, y: size / 2, k: (Math.min(r.width, r.height) / size) * 0.96 }
  S.fitted = true
}
const toScreen = (wx, wy, r) => [(wx - S.view.x) * S.view.k + r.width / 2, (wy - S.view.y) * S.view.k + r.height / 2]
const toWorld = (sx, sy, r) => [(sx - r.width / 2) / S.view.k + S.view.x, (sy - r.height / 2) / S.view.k + S.view.y]

// Land as a picture: the sea is the panel's own black, land a short grey ramp by height.
function paintTerrain(w) {
  if (!w.grid) return null
  const c = document.createElement('canvas')
  c.width = c.height = w.grid
  const g = c.getContext('2d')
  const img = g.createImageData(w.grid, w.grid)
  for (let i = 0; i < w.land.length; i++) {
    const h = w.land[i]
    const v = h === 0 ? 0 : 30 + Math.round((h / 255) * 46)
    img.data[i * 4] = v
    img.data[i * 4 + 1] = v
    img.data[i * 4 + 2] = v + (h === 0 ? 0 : 3)
    img.data[i * 4 + 3] = h === 0 ? 0 : 255
  }
  g.putImageData(img, 0, 0)
  return c
}

function strokeLines(lines, r, colour, width) {
  const [wx0, wy0] = toWorld(0, 0, r)
  const [wx1, wy1] = toWorld(r.width, r.height, r)
  ctx.strokeStyle = colour
  ctx.lineWidth = width
  ctx.beginPath()
  for (const line of lines) {
    const b = line.box
    if (b && (b[2] < wx0 || b[0] > wx1 || b[3] < wy0 || b[1] > wy1)) continue
    for (let i = 0; i < line.length; i += 2) {
      const [sx, sy] = toScreen(line[i], line[i + 1], r)
      if (i === 0) ctx.moveTo(sx, sy)
      else ctx.lineTo(sx, sy)
    }
  }
  ctx.stroke()
}

function drawMap() {
  const r = map.getBoundingClientRect()
  if (r.width < 2) return
  const d = window.devicePixelRatio || 1
  ctx.setTransform(d, 0, 0, d, 0, 0)
  ctx.clearRect(0, 0, r.width, r.height)
  const size = (S.world && S.world.size) || (S.info && S.info.world_size) || 0
  const k = S.view.k
  if (S.terrain) {
    const [x0, y0] = toScreen(0, 0, r)
    ctx.imageSmoothingEnabled = k * (size / S.terrain.width) < 3
    ctx.drawImage(S.terrain, x0, y0, size * k, size * k)
  }
  if (size) {
    const [x0, y0] = toScreen(0, 0, r)
    ctx.strokeStyle = INK.line2
    ctx.lineWidth = 1
    ctx.strokeRect(Math.round(x0) + 0.5, Math.round(y0) + 0.5, Math.round(size * k), Math.round(size * k))
  }
  if (S.world) {
    strokeLines(S.world.rivers, r, '#3b4654', k > 0.05 ? 2 : 1)
    if (k > 0.02) strokeLines(S.world.roads, r, '#5b5a52', 1)
  }
  ctx.font = "11px 'B612 Mono', monospace"
  ctx.textBaseline = 'middle'
  // Places with a title, once close enough to read them; the settlements always.
  const labelled = new Set()
  // A label is drawn only where no other already is: names in a town pile up otherwise.
  const taken = []
  const label = (text, x, y, colour) => {
    const w = ctx.measureText(text).width
    const box = [x - 2, y - 7, x + w + 2, y + 7]
    if (taken.some((t) => box[0] < t[2] && box[2] > t[0] && box[1] < t[3] && box[3] > t[1])) return false
    taken.push(box)
    ctx.fillStyle = colour
    ctx.fillText(text, x, y)
    return true
  }
  if (S.world) {
    for (const t of S.world.towns) {
      const [sx, sy] = toScreen(t.x, t.y, r)
      if (sx < -80 || sy < -20 || sx > r.width + 20 || sy > r.height + 20) continue
      ctx.fillStyle = INK.ink2
      ctx.fillRect(Math.round(sx) - 2, Math.round(sy) - 2, 4, 4)
      if (t.t === 'city' || k > 0.018) {
        label(t.name, sx + 7, sy, INK.ink)
        labelled.add(t.name)
      }
    }
  }
  if (S.game && k > 0.25) {
    let shown = 0
    for (const p of S.game.places || []) {
      if (shown > 80) break
      const [sx, sy] = toScreen(p.x, p.y, r)
      if (sx < 0 || sy < 0 || sx > r.width || sy > r.height) continue
      const called = p.title || p.name
      ctx.strokeStyle = INK.line2
      ctx.lineWidth = 1
      ctx.beginPath()
      ctx.arc(sx, sy, Math.max(2, (p.r || 0) * k), 0, Math.PI * 2)
      ctx.stroke()
      if (!labelled.has(called) && label(called, sx + 6, sy, INK.ink3)) shown++
    }
  }
  // Creatures: grey; players: the accent; the chosen type: white; the selected one: ringed.
  if (S.st) {
    const want = $('sp-list').value
    const wantIdx = S.game ? S.game.archetypes.findIndex((a) => a.name === want) : -1
    const dot = k > 0.6 ? 3 : 2
    const later = []
    ctx.fillStyle = '#6a6f78'
    for (const e of S.st.e) {
      const sx = (e[1] - S.view.x) * k + r.width / 2
      const sy = (e[2] - S.view.y) * k + r.height / 2
      if (sx < -4 || sy < -4 || sx > r.width + 4 || sy > r.height + 4) continue
      if (e[4] === 1 || e[3] === wantIdx || e[0] === S.selected) later.push(e)
      else ctx.fillRect(sx - dot / 2, sy - dot / 2, dot, dot)
    }
    for (const e of later) {
      const [sx, sy] = toScreen(e[1], e[2], r)
      const player = e[4] === 1
      ctx.fillStyle = player ? INK.accent : INK.ink
      const s = player ? 6 : dot + 1
      ctx.fillRect(sx - s / 2, sy - s / 2, s, s)
      if (player && k > 0.01) {
        ctx.fillStyle = INK.accent
        ctx.fillText(nameOf(e), sx + 8, sy)
      }
      if (e[0] === S.selected) {
        ctx.strokeStyle = INK.ink
        ctx.lineWidth = 1
        ctx.strokeRect(Math.round(sx) - 7.5, Math.round(sy) - 7.5, 15, 15)
      }
    }
  }
  // Where a spawn would land when fired from the keyboard.
  ctx.strokeStyle = S.armed ? INK.accent : INK.ink3
  ctx.lineWidth = 1
  const cx = Math.round(r.width / 2) + 0.5
  const cy = Math.round(r.height / 2) + 0.5
  ctx.beginPath()
  ctx.moveTo(cx - 6, cy)
  ctx.lineTo(cx + 6, cy)
  ctx.moveTo(cx, cy - 6)
  ctx.lineTo(cx, cy + 6)
  ctx.stroke()
  if (S.st && S.st.shown < S.st.live) $('m-note').textContent = 'showing ' + S.st.shown + ' of ' + S.st.live + ' creatures'
}

function zoom(factor, sx, sy) {
  const r = map.getBoundingClientRect()
  const px = sx ?? r.width / 2
  const py = sy ?? r.height / 2
  const [wx, wy] = toWorld(px, py, r)
  S.view.k = Math.min(40, Math.max(0.002, S.view.k * factor))
  S.view.x = wx - (px - r.width / 2) / S.view.k
  S.view.y = wy - (py - r.height / 2) / S.view.k
  S.follow = false
  drawSelected()
  drawMap()
}

let drag = null
map.addEventListener('pointerdown', (e) => {
  map.setPointerCapture(e.pointerId)
  drag = { x: e.clientX, y: e.clientY, moved: false }
})
map.addEventListener('pointermove', (e) => {
  const r = map.getBoundingClientRect()
  const [wx, wy] = toWorld(e.clientX - r.left, e.clientY - r.top, r)
  $('m-at').textContent = Math.round(wx) + ', ' + Math.round(wy)
  if (!drag) return
  const dx = e.clientX - drag.x
  const dy = e.clientY - drag.y
  if (!drag.moved && Math.hypot(dx, dy) < 4) return
  drag.moved = true
  drag.x = e.clientX
  drag.y = e.clientY
  S.view.x -= dx / S.view.k
  S.view.y -= dy / S.view.k
  if (S.follow) {
    S.follow = false
    drawSelected()
  }
  drawMap()
})
map.addEventListener('pointerup', (e) => {
  const was = drag
  drag = null
  if (!was || was.moved) return
  const r = map.getBoundingClientRect()
  const sx = e.clientX - r.left
  const sy = e.clientY - r.top
  const [wx, wy] = toWorld(sx, sy, r)
  if (S.armed) return void spawnAt(wx, wy)
  let best = null
  let bestD = PICK_PX
  for (const row of (S.st && S.st.e) || []) {
    const d = Math.hypot((row[1] - wx) * S.view.k, (row[2] - wy) * S.view.k) - (isPlayer(row) ? 4 : 0)
    if (d < bestD) {
      bestD = d
      best = row
    }
  }
  select(best ? best[0] : 0)
})
map.addEventListener('wheel', (e) => {
  e.preventDefault()
  const r = map.getBoundingClientRect()
  zoom(Math.exp(-e.deltaY * 0.0015), e.clientX - r.left, e.clientY - r.top)
}, { passive: false })
map.addEventListener('keydown', (e) => {
  const step = 60 / S.view.k
  const move = { ArrowLeft: [-step, 0], ArrowRight: [step, 0], ArrowUp: [0, -step], ArrowDown: [0, step] }[e.key]
  if (move) {
    S.view.x += move[0]
    S.view.y += move[1]
    S.follow = false
    drawSelected()
    drawMap()
  } else if (e.key === '+' || e.key === '=') zoom(1.4)
  else if (e.key === '-' || e.key === '_') zoom(1 / 1.4)
  else if (e.key === 'Enter' && S.armed) void spawnAt(S.view.x, S.view.y)
  else return
  e.preventDefault()
})
document.addEventListener('keydown', (e) => {
  if (e.key === 'Escape' && S.armed && !$('about').open) arm(false)
})
$('m-fit').addEventListener('click', () => {
  S.fitted = false
  S.follow = false
  fit((S.world && S.world.size) || (S.info && S.info.world_size))
  drawSelected()
  drawMap()
})
$('m-in').addEventListener('click', () => zoom(1.6))
$('m-out').addEventListener('click', () => zoom(1 / 1.6))

// ---------------------------------------------------------------------------------------------- the strip

function drawStrip() {
  const st = S.st
  $('s-players').textContent = String(st.clients)
  $('s-live').textContent = String(st.live)
  $('s-tick').textContent = String(st.tick)
  $('s-p99').textContent = st.perf ? st.perf.tick_p99_ms.toFixed(1) : '-'
  const w = st.director && st.director.weather
  $('s-sky').textContent = w ? w.name + (w.left_secs !== null ? ' ' + span(w.left_secs) : '') : 'clear'
}

// ---------------------------------------------------------------------------------------------- sky

let phaseHeld = false
function drawSky() {
  if (!S.game) return
  const d = daylight()
  const p = phaseNow(d, Date.now())
  const word = dayWord(d, p)
  $('sky-now').textContent = word.toUpperCase()
  $('sky-word').textContent = 'phase ' + p.toFixed(3) + (d.mode === 'cycle' ? ' · a day is ' + d.day_minutes + ' min' : d.mode === 'fixed' ? ' · held' : ' · by the clock')
  $('s-day').textContent = word
  if (!phaseHeld) $('sky-phase').value = String(p)
  for (const b of $('sky-moments').children) b.setAttribute('aria-pressed', String(dayWord(d, moment(d, b.dataset.at)) === word))
}
function fillMoments() {
  $('sky-moments').replaceChildren(
    ...['dawn', 'noon', 'dusk', 'midnight'].map((at) =>
      el('button', { class: 'key', type: 'button', 'data-at': at, 'aria-pressed': 'false', onclick: () => setPhase(moment(daylight(), at), at) }, at),
    ),
  )
}
async function setPhase(p, word) {
  const r = await post('/api/daylight', { phase: p }, 'The time is now ' + (word || dayWord(daylight(), p)) + '.')
  phaseHeld = false
  if (r) await loadGame()
}
$('sky-phase').addEventListener('input', () => {
  phaseHeld = true
  $('sky-now').textContent = dayWord(daylight(), Number($('sky-phase').value)).toUpperCase()
})
// Sent once the slider has rested: the arrow keys change it a step at a time, and each step is not a request.
let phaseTimer = 0
$('sky-phase').addEventListener('change', () => {
  clearTimeout(phaseTimer)
  phaseTimer = setTimeout(() => void setPhase(Number($('sky-phase').value)), 500)
})

function drawWeather() {
  const d = (S.st && S.st.director) || { weathers: [], weather: null, zones: [] }
  const now = d.weather ? d.weather.name : null
  const zoneList = (d.zones || []).map((z) => [z.name, !!z.on])
  if (!changed($('wx-pads'), [d.weathers, now]) & !changed($('zone-list'), zoneList)) return
  const pads = [...(d.weathers || []), null].map((name) =>
    el(
      'button',
      {
        class: 'key',
        type: 'button',
        title: name || 'clear',
        'aria-pressed': String(name === now),
        onclick: () =>
          post('/api/weather', { name, duration_secs: Number($('wx-for').value) }, name ? 'Weather: ' + name + ($('wx-for').value === '0' ? ', until changed.' : ' for ' + span(Number($('wx-for').value)) + '.') : 'Weather cleared.'),
      },
      name || 'clear',
    ),
  )
  keep($('wx-pads'), pads)
  const zones = (d.zones || []).map((z) =>
    el(
      'li',
      {},
      el('span', { class: 'name', text: z.name }),
      el('span', { class: 'state', text: z.on ? 'on' : 'off' }),
      el('button', { class: 'key small', type: 'button', 'aria-label': (z.on ? 'Turn off ' : 'Turn on ') + z.name, onclick: () => post('/api/zone', { name: z.name, on: !z.on }, 'Weather zone ' + z.name + (z.on ? ' off.' : ' on.')) }, z.on ? 'Off' : 'On'),
    ),
  )
  $('zones').hidden = zones.length === 0
  $('zones-lab').textContent = 'Weather zones · ' + (d.zones || []).filter((z) => z.on).length + ' of ' + zones.length + ' on'
  keep($('zone-list'), zones)
}

/**
 * True when a list has to be rebuilt: what it shows differs from last time. The state is polled every few seconds,
 * and replacing a key between the press and the release of a click loses the click.
 */
const shown = new Map()
function changed(box, what) {
  const sig = JSON.stringify(what)
  if (shown.get(box) === sig) return false
  shown.set(box, sig)
  return true
}
/** Replace a list's rows, keeping focus on the button that had it. */
function keep(box, kids) {
  const at = [...box.querySelectorAll('button')].indexOf(document.activeElement)
  box.replaceChildren(...kids)
  const again = box.querySelectorAll('button')
  if (at >= 0 && again[at]) again[at].focus()
}

// ---------------------------------------------------------------------------------------------- spawn

function fillSpawn() {
  const find = $('sp-find').value.trim().toLowerCase()
  const was = $('sp-list').value
  const all = (S.game.archetypes || []).filter((a) => !find || a.name.toLowerCase().includes(find) || (a.title || '').toLowerCase().includes(find))
  $('sp-list').replaceChildren(...all.map((a) => el('option', { value: a.name, text: (a.title ? a.title + ' · ' : '') + a.name + (a.level ? '  L' + a.level : '') })))
  if (all.some((a) => a.name === was)) $('sp-list').value = was
  else if (all[0]) $('sp-list').value = all[0].name
  drawSpawnCount()
}
$('sp-find').addEventListener('input', fillSpawn)
$('sp-list').addEventListener('change', () => {
  drawSpawnCount()
  drawArmed()
  drawMap()
})
function drawSpawnCount() {
  if (!S.st || !S.game) return
  const idx = S.game.archetypes.findIndex((a) => a.name === $('sp-list').value)
  const n = idx < 0 ? 0 : S.st.e.reduce((k, e) => k + (e[3] === idx && e[4] !== 1 ? 1 : 0), 0)
  $('sp-live').textContent = idx < 0 ? '' : n + ' alive' + (S.st.shown < S.st.live ? ' (of those shown)' : '')
}
function spawnAsk() {
  const name = $('sp-list').value
  const count = Math.min(200, Math.max(1, Math.round(Number($('sp-count').value) || 1)))
  const radius = Math.min(500, Math.max(0, Number($('sp-radius').value) || 0))
  return name ? { name, count, radius } : null
}
async function spawnAt(x, y) {
  const a = spawnAsk()
  if (!a) return say('Choose a creature first.', true)
  await post('/api/spawn', { archetype: a.name, count: a.count, x, y, ...(a.radius > 0 ? { radius: a.radius } : {}) }, (d) => 'Spawned ' + a.count + ' ' + a.name + ' at ' + Math.round(x) + ', ' + Math.round(y) + '.' + (d && d.note ? ' ' + d.note : ''))
}
function arm(on) {
  S.armed = on
  $('sp-arm').setAttribute('aria-pressed', String(on))
  drawArmed()
  drawMap()
}
function drawArmed() {
  const a = spawnAsk()
  $('m-armed').hidden = !S.armed
  $('m-armed').textContent = S.armed && a ? 'armed: ' + a.count + ' ' + a.name + ' · click the map, Enter for the centre, Esc to stand down' : ''
}
$('sp-arm').addEventListener('click', () => {
  if (!S.armed && !spawnAsk()) return say('Choose a creature first.', true)
  arm(!S.armed)
  if (S.armed) map.focus()
})
$('sp-count').addEventListener('input', drawArmed)
$('sp-centre').addEventListener('click', () => void spawnAt(S.view.x, S.view.y))
$('sp-selected').addEventListener('click', () => {
  const row = rowOf(S.selected)
  if (!row) return say('Nothing is selected.', true)
  void spawnAt(row[1], row[2])
})
$('kill-all').addEventListener('click', async () => {
  const b = $('kill-all')
  if (Date.now() - S.killArmedAt > 4000) {
    S.killArmedAt = Date.now()
    b.textContent = 'Press again'
    b.classList.add('on')
    say('Kill every bot in the world? Press again within 4 seconds.')
    setTimeout(() => {
      if (Date.now() - S.killArmedAt >= 4000) {
        b.textContent = 'Kill all bots'
        b.classList.remove('on')
      }
    }, 4100)
    return
  }
  S.killArmedAt = 0
  b.textContent = 'Kill all bots'
  b.classList.remove('on')
  await post('/api/kill', { all_bots: true }, (d) => 'Killed ' + (d && d.killed !== undefined ? d.killed : 'all') + ' bots.')
})

// ---------------------------------------------------------------------------------------------- voice

function voice(banner) {
  S.banner = banner
  $('say-banner').setAttribute('aria-pressed', String(banner))
  $('say-chat').setAttribute('aria-pressed', String(!banner))
}
$('say-banner').addEventListener('click', () => voice(true))
$('say-chat').addEventListener('click', () => voice(false))
async function announce() {
  const text = $('say-text').value.trim()
  if (!text) return say('Nothing to say.', true)
  const r = await post('/api/announce', { text, banner: S.banner }, (d) => (S.banner ? 'Banner' : 'Chat line') + ' sent to ' + d.told + (d.told === 1 ? ' player.' : ' players.'))
  if (r) {
    $('say-text').value = ''
    void pollChat()
  }
}
$('say-send').addEventListener('click', announce)
$('say-text').addEventListener('keydown', (e) => {
  if (e.key === 'Enter' && (e.metaKey || e.ctrlKey)) void announce()
})

// ---------------------------------------------------------------------------------------------- events

function drawEvents() {
  const d = (S.st && S.st.director) || { events: [], runs: [] }
  const running = new Map((d.runs || []).map((r) => [r.name, r]))
  const list = $('events')
  const rowState = (ev) => {
    const run = running.get(ev.name)
    return run ? 'step ' + (run.step + 1) + '/' + run.of + (run.failing ? ' failing' : '') : ev.next_in_secs !== null && ev.next_in_secs !== undefined ? 'in ' + span(ev.next_in_secs) : 'by hand'
  }
  // Which rows there are and which are running decides a rebuild; a countdown is written into the row as it stands.
  if (!changed(list, (d.events || []).map((ev) => [ev.name, running.has(ev.name)]))) {
    const cells = list.querySelectorAll('.state')
    ;(d.events || []).forEach((ev, i) => cells[i] && (cells[i].textContent = rowState(ev)))
    return
  }
  keep(
    list,
    (d.events || []).length
      ? d.events.map((ev) => {
          const run = running.get(ev.name)
          const state = rowState(ev)
          return el(
            'li',
            {},
            el('span', { class: 'name', text: ev.name, title: ev.last ? 'last: ' + ev.last : ev.name }),
            el('span', { class: 'state' + (run ? ' on' : ''), text: state }),
            run
              ? el('button', { class: 'key small', type: 'button', 'aria-label': 'Stop ' + ev.name, onclick: () => post('/api/events/stop', { name: ev.name }, 'Stopped ' + ev.name + '.') }, 'Stop')
              : el('button', { class: 'key small', type: 'button', 'aria-label': 'Fire ' + ev.name, onclick: () => post('/api/events/fire', { name: ev.name }, 'Fired ' + ev.name + '.') }, 'Fire'),
          )
        })
      : [el('li', {}, el('span', { class: 'empty', text: 'This game has no events.' }))],
  )
}

function drawLog() {
  const log = (S.st && S.st.director && S.st.director.log) || []
  $('log').replaceChildren(
    ...(log.length ? log.slice(0, 80).map((l) => el('li', {}, el('span', { class: 't', text: ago(l[0]) }), el('span', { class: 'text', text: l[1] }))) : [el('li', {}, el('span', { class: 't', text: '' }), el('span', { class: 'text', text: 'nothing yet' }))]),
  )
}

// ---------------------------------------------------------------------------------------------- players and the selected one

function drawPlayers() {
  const rows = S.st.e.filter(isPlayer)
  const list = $('players')
  // Who is on decides a rebuild; where they are is written into the row as it stands.
  if (!changed(list, [rows.slice(0, 60).map((row) => [row[0], nameOf(row)]), S.selected, S.st.clients])) {
    const cells = list.querySelectorAll('.state')
    rows.slice(0, 60).forEach((row, i) => cells[i] && (cells[i].textContent = Math.round(row[1]) + ', ' + Math.round(row[2])))
    return
  }
  keep(
    list,
    rows.length
      ? rows.slice(0, 60).map((row) =>
          el(
            'li',
            {},
            el('span', { class: 'name', text: nameOf(row) }),
            el('span', { class: 'state', text: Math.round(row[1]) + ', ' + Math.round(row[2]) }),
            el('button', { class: 'key small' + (row[0] === S.selected ? ' on' : ''), type: 'button', 'aria-label': 'Go to ' + nameOf(row), onclick: () => goTo(row[0]) }, 'Go'),
          ),
        )
      : [el('li', {}, el('span', { class: 'empty', text: S.st.clients ? S.st.clients + ' connected, none in the rows shown.' : 'Nobody is on.' }))],
  )
}
function goTo(id) {
  const row = rowOf(id)
  if (!row) return
  S.view.x = row[1]
  S.view.y = row[2]
  S.view.k = Math.max(S.view.k, 1.2)
  select(id)
}
function select(id) {
  S.selected = id
  S.follow = false
  drawSelected()
  drawPlayers()
  drawMap()
}
function drawSelected() {
  const row = rowOf(S.selected)
  $('sel').hidden = !row
  $('sel-none').hidden = !!row
  if (!row) {
    if (S.selected && S.st) {
      S.selected = 0
      S.follow = false
      say('What was selected is gone: dead, or out of the world.')
    }
    return
  }
  const a = archOf(row)
  $('sel-name').textContent = nameOf(row)
  $('sel-kind').textContent = (isPlayer(row) ? 'player' : row[4] === 2 ? 'driven bot' : 'bot') + (a && !isPlayer(row) ? ' · ' + a.name : '') + ' · id ' + row[0] + ' · ' + Math.round(row[1]) + ', ' + Math.round(row[2])
  $('sel-attrs').replaceChildren(...(S.st.attrs || []).flatMap((name, i) => [el('dt', { text: name }), el('dd', { text: String(Math.round((row[5 + i] ?? 0) * 10) / 10) })]))
  $('sel-follow').setAttribute('aria-pressed', String(S.follow))
  $('sel-god').disabled = $('sel-god-off').disabled = !isPlayer(row)
  if (!isPlayer(row) || killFor !== row[0]) standDownKill()
}
$('sel-follow').addEventListener('click', () => {
  S.follow = !S.follow
  const row = rowOf(S.selected)
  if (S.follow && row) {
    S.view.x = row[1]
    S.view.y = row[2]
  }
  drawSelected()
  drawMap()
})
// A bot dies on one press; a player takes two, since somebody is playing that one.
let killFor = 0
let killTimer = 0
function standDownKill() {
  killFor = 0
  clearTimeout(killTimer)
  $('sel-kill').textContent = 'Kill'
  $('sel-kill').classList.remove('on')
}
$('sel-kill').addEventListener('click', () => {
  const row = rowOf(S.selected)
  if (!row) return
  if (isPlayer(row) && killFor !== row[0]) {
    killFor = row[0]
    $('sel-kill').textContent = 'Press again'
    $('sel-kill').classList.add('on')
    say('Kill the player ' + nameOf(row) + '? Press again within 4 seconds.')
    killTimer = setTimeout(standDownKill, 4000)
    return
  }
  standDownKill()
  void post('/api/kill', { id: row[0] }, 'Killed ' + nameOf(row) + '.')
})
for (const [id, on] of [['sel-god', true], ['sel-god-off', false]]) {
  $(id).addEventListener('click', () => {
    const row = rowOf(S.selected)
    if (row) void post('/api/god', { id: row[0], on }, 'God mode for ' + nameOf(row) + (on ? ' is on.' : ' is off.'))
  })
}
function fillItems() {
  $('give-item').replaceChildren(...(S.game.items || []).map((i) => el('option', { value: i.name, text: (i.title ? i.title + ' · ' : '') + i.name })))
}
$('give-go').addEventListener('click', () => {
  const row = rowOf(S.selected)
  const item = $('give-item').value
  const count = Math.min(999, Math.max(1, Math.round(Number($('give-count').value) || 1)))
  if (row && item) void post('/api/give', { id: row[0], item, count }, 'Gave ' + count + ' ' + item + ' to ' + nameOf(row) + '.')
})
async function sayAs() {
  const row = rowOf(S.selected)
  const text = $('as-text').value.trim()
  if (!row || !text) return
  const r = await post('/api/say', { id: row[0], text, shout: $('as-shout').checked }, nameOf(row) + ($('as-shout').checked ? ' shouted.' : ' spoke.'))
  if (r) {
    $('as-text').value = ''
    void pollChat()
  }
}
$('as-go').addEventListener('click', sayAs)
$('as-text').addEventListener('keydown', (e) => {
  if (e.key === 'Enter') void sayAs()
})

// ---------------------------------------------------------------------------------------------- about

$('about-open').addEventListener('click', async () => {
  const a = await window.desk.about()
  $('about-name').textContent = a.name
  $('about-version').textContent = 'version ' + a.version
  $('about').showModal()
})
$('about-close').addEventListener('click', () => $('about').close())
$('about').addEventListener('click', (e) => {
  if (e.target === $('about')) $('about').close()
})
$('about-forget').addEventListener('click', async () => {
  await window.desk.forget()
  $('about').close()
  say('Password forgotten: it will be asked for next time.')
})

void boot()
