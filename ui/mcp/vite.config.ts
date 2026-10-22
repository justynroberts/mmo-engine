import { defineConfig } from 'vite'

// The MCP server as one file for Node, with its dependencies inside it.
export default defineConfig({
  ssr: { noExternal: true, target: 'node' },
  build: {
    ssr: 'mcp/server.ts',
    outDir: 'dist-mcp',
    emptyOutDir: true,
    target: 'node20',
    rolldownOptions: { output: { entryFileNames: 'mmo-mcp.mjs' } },
  },
})
