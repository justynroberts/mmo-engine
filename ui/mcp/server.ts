// The MCP server: the console's functions as tools, over stdio. Run `node dist-mcp/mmo-mcp.mjs` (after
// `npm run build:mcp`); MMO_ADMIN picks the server's admin address (default http://127.0.0.1:7780). The admin
// API only answers on loopback, so for a remote server tunnel its admin port first (ssh -L 7780:127.0.0.1:7780).

import { McpServer } from '@modelcontextprotocol/sdk/server/mcp.js'
import { StdioServerTransport } from '@modelcontextprotocol/sdk/server/stdio.js'
import { Refused, TOOLS } from './tools'
import type { Ctx } from './tools'

let base = (process.env.MMO_ADMIN ?? 'http://127.0.0.1:7780').replace(/\/+$/, '')

const ctx: Ctx = {
  async call(path, method = 'GET', body) {
    let res: Response
    try {
      res = await fetch(base + path, {
        method,
        // Writes carry the console's header: the server refuses writes without it.
        headers: method === 'GET' ? undefined : { 'X-Admin': '1', 'Content-Type': 'application/json' },
        body: body === undefined ? undefined : JSON.stringify(body),
      })
    } catch (e) {
      throw new Refused(`Cannot reach the server's admin API at ${base} (${(e as Error).message}). Is the server running? Its admin port only listens on loopback.`)
    }
    const text = await res.text()
    let parsed: unknown
    try {
      parsed = JSON.parse(text)
    } catch {
      parsed = { error: text.slice(0, 300) }
    }
    return { status: res.status, body: parsed }
  },
  setBase(url) {
    base = url.replace(/\/+$/, '')
  },
  base: () => base,
}

const server = new McpServer(
  { name: 'mmo-engine', version: '0.1.0' },
  {
    instructions:
      'Controls an mmo-engine world server: everything its admin console can do. Call overview first, and game_schema before writing game file entries. Game file changes (upsert, rename, remove, add_preset) apply live to everyone; changing attributes, actions or movement settings restarts the world and needs allow_restart. Coordinates are world units, x east and y south, from 0 to the world size.',
  },
)

for (const t of TOOLS) {
  server.registerTool(
    t.name,
    {
      description: t.description,
      inputSchema: t.input,
      annotations: { readOnlyHint: !!t.readOnly, destructiveHint: !!t.destructive, openWorldHint: false },
    },
    async (args: unknown) => {
      try {
        const out = await t.run(args ?? {}, ctx)
        return { content: [{ type: 'text' as const, text: JSON.stringify(out, null, 1) }] }
      } catch (e) {
        if (e instanceof Refused) return { content: [{ type: 'text' as const, text: e.message }], isError: true }
        throw e
      }
    },
  )
}

await server.connect(new StdioServerTransport())
