// MIT License - Copyright (c) fintonlabs.com
// God: a live control desk for an mmo-engine world. This process holds the admin password and makes every request;
// the window never sees the password and can only reach /api/ on the one server it was pointed at.
'use strict'

const { app, BrowserWindow, ipcMain, safeStorage, shell } = require('electron')
const fs = require('node:fs')
const path = require('node:path')
const { reduceWorld } = require('./world')

const DEFAULT_SERVER = 'http://100.91.37.32:7791'
const TIMEOUT_MS = 15000
const WORLD_TIMEOUT_MS = 120000

// Test affordances, all from the environment: GOD_HOME (a different place for what is saved), GOD_SHOT_SIZE (WxH),
// GOD_DROP_AFTER (ms after which every request fails as if the link were lost).
if (process.env.GOD_HOME) app.setPath('userData', process.env.GOD_HOME)
const SHOT_SIZE = (process.env.GOD_SHOT_SIZE || '').split('x').map(Number)
const STARTED = Date.now()

let win = null
let server = ''
let password = ''

const file = (name) => path.join(app.getPath('userData'), name)

function loadSaved() {
  try {
    const c = JSON.parse(fs.readFileSync(file('desk.json'), 'utf8'))
    server = typeof c.server === 'string' ? c.server : ''
    if (typeof c.secret === 'string' && safeStorage.isEncryptionAvailable()) {
      password = safeStorage.decryptString(Buffer.from(c.secret, 'base64'))
    }
  } catch {
    // Nothing saved yet, or it cannot be read: the desk opens on the connect form.
  }
  // For a run from a terminal: GOD_SERVER and GOD_PASSWORD win over what is saved, and are never written down.
  if (process.env.GOD_SERVER) server = process.env.GOD_SERVER
  if (process.env.GOD_PASSWORD) password = process.env.GOD_PASSWORD
}

function save(remember) {
  const c = { server }
  if (remember && safeStorage.isEncryptionAvailable()) {
    c.secret = safeStorage.encryptString(password).toString('base64')
  }
  fs.mkdirSync(app.getPath('userData'), { recursive: true })
  fs.writeFileSync(file('desk.json'), JSON.stringify(c), { mode: 0o600 })
}

/** A server address as typed, tidied: http(s) only, no path. Returns '' when it is not one. */
function tidy(address) {
  let a = String(address || '').trim()
  if (!a) return ''
  if (!/^https?:\/\//i.test(a)) a = 'http://' + a
  try {
    const u = new URL(a)
    return u.protocol === 'http:' || u.protocol === 'https:' ? u.origin : ''
  } catch {
    return ''
  }
}

async function call(method, apiPath, body, timeout = TIMEOUT_MS) {
  if (!server) return { ok: false, status: 0, error: 'No server chosen.' }
  if (typeof apiPath !== 'string' || !apiPath.startsWith('/api/') || apiPath.includes('..')) {
    return { ok: false, status: 0, error: 'Only the admin interface can be called.' }
  }
  const m = String(method || 'GET').toUpperCase()
  if (process.env.GOD_DROP_AFTER && Date.now() - STARTED > Number(process.env.GOD_DROP_AFTER)) {
    return { ok: false, status: 0, error: 'Cannot reach ' + server + '. Is Tailscale up?' }
  }
  if (process.env.GOD_TRACE) console.log('TRACE', m, apiPath, m === 'POST' ? JSON.stringify(body) : '')
  // GOD_DRY=1: look, do not touch. Every write is refused here and never leaves the machine.
  if (process.env.GOD_DRY && m !== 'GET') return { ok: false, status: 0, error: 'Dry run: ' + apiPath + ' was not sent.' }
  if (!['GET', 'POST'].includes(m)) return { ok: false, status: 0, error: 'Only GET and POST are used here.' }
  try {
    const r = await fetch(server + apiPath, {
      method: m,
      // The admin interface refuses a write without X-Admin (a page in a browser cannot send it across origins).
      headers: { Authorization: 'Bearer ' + password, ...(m === 'POST' ? { 'Content-Type': 'application/json', 'X-Admin': '1' } : {}) },
      body: m === 'POST' ? JSON.stringify(body ?? {}) : undefined,
      signal: AbortSignal.timeout(timeout),
      redirect: 'error',
    })
    const text = await r.text()
    let data = null
    try {
      data = JSON.parse(text)
    } catch {
      data = null
    }
    if (r.ok && data !== null) return { ok: true, status: r.status, data }
    const said = (data && (data.error || data.note)) || text.trim().slice(0, 200)
    const why =
      r.status === 401 ? 'The server refused the password.'
      : r.status === 429 ? 'Too many wrong passwords from here: wait a minute.'
      : r.status === 404 && !said ? 'This server does not have that control yet: it needs a newer build.'
      : said || 'The server answered ' + r.status + '.'
    return { ok: false, status: r.status, error: why }
  } catch (e) {
    const slow = e && (e.name === 'TimeoutError' || e.name === 'AbortError')
    return { ok: false, status: 0, error: slow ? 'No answer in ' + Math.round(timeout / 1000) + ' s. Is Tailscale up?' : 'Cannot reach ' + server + '. Is Tailscale up?' }
  }
}

ipcMain.handle('desk:saved', () => ({ server: server || DEFAULT_SERVER, hasPassword: password !== '' }))

ipcMain.handle('desk:connect', async (_e, { address, secret, remember }) => {
  const a = tidy(address)
  if (!a) return { ok: false, error: 'That is not a server address. It looks like http://100.91.37.32:7791.' }
  const before = [server, password]
  server = a
  if (typeof secret === 'string' && secret !== '') password = secret
  if (!password) {
    ;[server, password] = before
    return { ok: false, error: 'Give the admin password.' }
  }
  const r = await call('GET', '/api/info')
  if (!r.ok) {
    ;[server, password] = before
    return { ok: false, error: r.error }
  }
  if (!process.env.GOD_PASSWORD) save(remember !== false)
  return { ok: true, server, info: r.data }
})

ipcMain.handle('desk:forget', () => {
  password = ''
  try {
    fs.writeFileSync(file('desk.json'), JSON.stringify({ server }), { mode: 0o600 })
  } catch {
    // Nothing to forget.
  }
  return { ok: true }
})

ipcMain.handle('desk:api', (_e, { method, path: p, body }) => call(method, p, body))

ipcMain.handle('desk:world', async () => {
  const r = await call('GET', '/api/worldfile', undefined, WORLD_TIMEOUT_MS)
  if (!r.ok) return r
  try {
    return { ok: true, data: reduceWorld(r.data) }
  } catch (e) {
    return { ok: false, status: 0, error: 'The world file could not be read: ' + (e && e.message) }
  }
})

ipcMain.handle('desk:about', () => ({ name: app.getName(), version: app.getVersion() }))

function open() {
  win = new BrowserWindow({
    width: SHOT_SIZE[0] || 1480,
    height: SHOT_SIZE[1] || 940,
    minWidth: 1100,
    minHeight: 700,
    backgroundColor: '#0b0c0e',
    title: 'God',
    show: !process.env.GOD_SHOT,
    webPreferences: { preload: path.join(__dirname, 'preload.js'), contextIsolation: true, sandbox: true, nodeIntegration: false },
  })
  win.webContents.setWindowOpenHandler(({ url }) => {
    if (url === 'https://fintonlabs.com/' || url === 'https://fintonlabs.com') void shell.openExternal(url)
    return { action: 'deny' }
  })
  win.webContents.on('will-navigate', (e) => e.preventDefault())
  void win.loadFile(path.join(__dirname, 'renderer', 'index.html'))
  // GOD_SHOT=file.png [GOD_SHOT_AFTER=ms]: draw the desk, save a picture of it and quit (for checking a build).
  if (process.env.GOD_SHOT) {
    win.webContents.once('did-finish-load', () => {
      setTimeout(async () => {
        if (process.env.GOD_SHOT_JS) {
          // What the script returns is written beside the picture, for measuring the page as drawn.
          const out = await win.webContents.executeJavaScript(process.env.GOD_SHOT_JS, true).catch((e) => ({ error: String(e) }))
          if (out !== undefined) fs.writeFileSync(process.env.GOD_SHOT + '.json', JSON.stringify(out, null, 1))
        }
        await new Promise((r) => setTimeout(r, 600))
        const img = await win.webContents.capturePage()
        fs.writeFileSync(process.env.GOD_SHOT, img.toPNG())
        app.quit()
      }, Number(process.env.GOD_SHOT_AFTER) || 9000)
    })
  }
}

app.whenReady().then(() => {
  loadSaved()
  open()
  app.on('activate', () => {
    if (BrowserWindow.getAllWindows().length === 0) open()
  })
})
app.on('window-all-closed', () => app.quit())
