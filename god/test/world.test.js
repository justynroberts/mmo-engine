// MIT License - Copyright (c) fintonlabs.com
'use strict'
const test = require('node:test')
const assert = require('node:assert')
const { reduceWorld, GRID } = require('../world')

test('a world file becomes a picture: sea is 0, land rises, lines are flattened', () => {
  // 3 x 3 heights over a world 64 across: the left column under the sea, the right column high.
  const u = Buffer.alloc(18)
  const row = [0, 32768, 65535]
  for (let y = 0; y < 3; y++) for (let x = 0; x < 3; x++) u.writeUInt16LE(row[x], (y * 3 + x) * 2)
  const w = reduceWorld({
    size: 64,
    heights: { w: 3, h: 3, cell: 32, x0: 0, y0: 0, lo: -10, hi: 90, u16le: u.toString('base64') },
    water: { sea: { level: 0 }, rivers: [{ pts: [[0, 0, 1], [10, 10, 1]] }] },
    roads: [{ pts: [[1.4, 2.6], [30, 40]] }, { pts: [[1, 1]] }],
    settled: [{ name: 'Áth Cliath', at: [5, 6], t: 'city' }],
  })
  assert.equal(w.grid, GRID)
  assert.equal(w.land[0], 0)
  assert.ok(w.land[GRID - 1] > w.land[GRID / 2] && w.land[GRID / 2] > 0)
  assert.deepEqual(w.roads, [[1, 3, 30, 40]])
  assert.deepEqual(w.rivers, [[0, 0, 10, 10]])
  assert.deepEqual(w.towns, [{ name: 'Áth Cliath', x: 5, y: 6, t: 'city' }])
})

test('a world file with no heights is still a map of roads and towns', () => {
  const w = reduceWorld({ size: 100, roads: [], settled: [] })
  assert.equal(w.grid, 0)
  assert.equal(w.land.length, 0)
})
