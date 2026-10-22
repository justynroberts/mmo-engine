// End to end: start the built MCP server over stdio against a running world server and build a small scene with
// it, the way an AI would. `node mcp/smoke.mjs [admin url]` (default http://127.0.0.1:7780). It edits that
// server's game and world, so point it at a throwaway server.

import { Client } from '@modelcontextprotocol/sdk/client/index.js'
import { StdioClientTransport } from '@modelcontextprotocol/sdk/client/stdio.js'

const admin = process.argv[2] ?? 'http://127.0.0.1:7780'
const client = new Client({ name: 'mcp-smoke', version: '0' })
await client.connect(new StdioClientTransport({ command: 'node', args: [new URL('../dist-mcp/mmo-mcp.mjs', import.meta.url).pathname], env: { ...process.env, MMO_ADMIN: admin } }))

let failed = 0
const check = (ok, what, detail = '') => {
  console.log(`${ok ? 'PASS' : 'FAIL'} ${what}${ok ? '' : ` ${detail}`}`)
  if (!ok) failed++
}
async function call(name, args = {}) {
  const r = await client.callTool({ name, arguments: args })
  const text = r.content?.[0]?.text ?? ''
  let body = text
  try {
    body = JSON.parse(text)
  } catch {}
  return { error: !!r.isError, body, text }
}

const { tools } = await client.listTools()
check(tools.length >= 30, 'lists the tools', String(tools.length))
check(tools.find((t) => t.name === 'clear_blocks')?.annotations?.destructiveHint === true, 'marks destructive tools')

const ov = await call('overview')
check(!ov.error && ov.body.world_size > 0, 'overview', ov.text)
const size = ov.body.world_size
const cx = Math.round(size / 2) + 40
const cy = Math.round(size / 2) + 40

const schema = await call('game_schema')
check(schema.body.types?.includes('interface EventDef'), 'game_schema carries the types')

let r = await call('upsert', { section: 'places', entry: { name: 'mcp camp', x: cx, y: cy, r: 20, title: 'The Camp' } })
check(!r.error && r.body.added?.title === 'The Camp', 'adds a titled place', r.text)
r = await call('upsert', { section: 'places', entry: { name: 'mcp camp', r: 25 } })
check(!r.error && r.body.changed?.r === 25 && r.body.changed?.title === 'The Camp', 'merges a change into it', r.text)

r = await call('add_preset', { kind: 'bot', preset: 'guard', name: 'mcp guard' })
check(!r.error && r.body.added?.name === 'mcp guard', 'adds a bot type from a preset', r.text)
r = await call('upsert', { section: 'archetypes', entry: { name: 'mcp guard', home: 'mcp camp', count: 2, color: '#aa3322' } })
check(!r.error && r.body.changed?.home === 'mcp camp', 'homes it at the place', r.text)

r = await call('upsert', { section: 'items', entry: { name: 'mcp gem', color: '#33ccff' } })
check(!r.error, 'adds an item', r.text)
r = await call('upsert', {
  section: 'events',
  entry: { name: 'mcp raid', place: 'mcp camp', limit_secs: 60, steps: [{ kind: 'announce', text: 'Raiders at the camp!', banner: true }, { kind: 'spawn', archetype: 'mcp guard', count: 2, place: 'mcp camp' }, { kind: 'defeat', within_secs: 60 }, { kind: 'reward', item: 'mcp gem', count: 1 }] },
})
check(!r.error, 'adds a raid event', r.text)

r = await call('upsert', { section: 'events', entry: { name: 'mcp broken', steps: [{ kind: 'spawn', archetype: 'nobody', count: 1, place: null }] } })
check(r.error && r.text.includes('nobody'), 'refuses a broken event with the reason', r.text)

r = await call('upsert', { section: 'attrs', entry: { name: 'mcp mana', max: 50, default: 50 } })
check(r.error && r.text.includes('allow_restart'), 'asks before a change that restarts the world', r.text)

r = await call('rename', { section: 'places', from: 'mcp camp', to: 'mcp fort' })
check(!r.error, 'renames the place', r.text)
const g = (await call('get_game')).body
check(g.archetypes.find((a) => a.name === 'mcp guard')?.home === 'mcp fort', 'the bot home follows the rename')
check(g.events.find((e) => e.name === 'mcp raid')?.place === 'mcp fort', 'the event place follows the rename')

r = await call('spawn', { type: 'mcp guard', count: 3, place: 'mcp fort' })
check(!r.error && r.body.spawned === 3, 'spawns at a place', r.text)
r = await call('list_entities', { type: 'mcp guard', place: 'mcp fort' })
check(!r.error && r.body.total >= 3, 'lists them there', r.text)
const one = r.body.shown?.[0]?.id
r = await call('get_entity', { id: one })
check(!r.error && r.body.type === 'mcp guard' && r.body.attrs, 'reads one', r.text)

r = await call('build', { shape: 'walls', x0: cx - 30, y0: cy - 30, x1: cx - 20, y1: cy - 20, material: 'stone', height: 3 })
check(!r.error && r.body.set > 0, 'builds a hut outline', r.text)
r = await call('build', { shape: 'fill', x0: cx - 30, y0: cy - 30, x1: cx - 20, y1: cy - 20, material: 'roof tiles', bottom: 3 })
check(!r.error && r.body.set > 0, 'roofs it', r.text)
r = await call('get_blocks', { x: cx - 25, y: cy - 25, r: 3 })
const col = r.body.shown?.find((c) => c[2].length >= 4)
check(!r.error && col, 'reads the blocks back', r.text)
r = await call('get_blocks', { x: cx - 30, y: cy - 30, r: 1.5 })
check(r.body.shown?.some((c) => c[2] === '0006'), 'the wall keeps its stone under the roof', JSON.stringify(r.body.shown))
r = await call('build', { shape: 'line', x0: cx - 30, y0: cy - 25, x1: cx - 30, y1: cy - 23, material: 'air', height: 2 })
check(!r.error, 'cuts a door', r.text)

r = await call('clear_blocks', {})
check(r.error && r.text.includes('everything'), 'will not clear every block without being told', r.text)
r = await call('clear_blocks', { x: cx - 25, y: cy - 25, r: 20 })
check(!r.error && r.body.removed > 0, 'clears blocks in a circle', r.text)

r = await call('event', { name: 'mcp raid', op: 'start' })
check(!r.error, 'starts the event', r.text)
await new Promise((res) => setTimeout(res, 1500))
r = await call('director_status')
check(r.body.runs?.some((x) => x.name === 'mcp raid'), 'it is running', r.text)
r = await call('read_chat', { limit: 20 })
check(r.body.some?.((l) => l.channel === 'banner' && l.text.includes('Raiders')), 'its banner is in chat', r.text)
r = await call('event', { name: 'mcp raid', op: 'stop' })
check(!r.error, 'stops it', r.text)

r = await call('set_time', { at: 'dusk' })
check(!r.error && typeof r.body.now === 'string', 'sets the time to dusk', r.text)

r = await call('add_preset', { kind: 'weather', preset: 'fog' })
check(!r.error, 'adds fog', r.text)
const fog = r.body.added?.name
r = await call('set_weather', { name: fog, duration_secs: 30 })
check(!r.error, 'makes it foggy', r.text)

r = await call('get_world', { place: 'mcp fort' })
check(!r.error && r.body.size === size, 'reads the land (or says it has no world file)', r.text)
r = await call('world_preview', { recipe: { villages: 2 } })
check(!r.error && r.body.recipe?.villages === 2, 'previews a new recipe', r.text)

r = await call('remove_entities', { type: 'mcp guard' })
check(!r.error && r.body.killed >= 3, 'removes bots of a type', r.text)
for (const [section, name] of [['events', 'mcp raid'], ['archetypes', 'mcp guard'], ['items', 'mcp gem'], ['weather', fog], ['places', 'mcp fort']]) {
  r = await call('remove', { section, name })
  check(!r.error, `removes ${section} ${name}`, r.text)
}
r = await call('set_weather', { name: null })
check(!r.error, 'clears the weather', r.text)

await client.close()
console.log(failed ? `MCP smoke: ${failed} failed` : 'MCP smoke ok')
process.exit(failed ? 1 : 0)
