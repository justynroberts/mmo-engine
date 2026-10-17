import { render, screen, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import axe from 'axe-core'
import { useState } from 'react'
import type { GameFile } from '../api'
import { fixture } from '../test/fixture'
import { Builder, initialBuilder, type BuilderState } from './Builder'

function start(): GameFile {
  const g = fixture()
  g.places = [{ name: 'plaza', x: 100, y: 100, r: 12 }]
  g.routes = [{ name: 'ring', points: [[0, 0], [20, 20], [40, 0]], loop: true }]
  g.archetypes[0] = {
    ...g.archetypes[0]!,
    rules: [
      { when: { kind: 'attr_below', attr: 'health', value: 20 }, then: { kind: 'flee' } },
      { when: { kind: 'always' }, then: { kind: 'patrol', route: 'ring' } },
    ],
  }
  return g
}

function Harness({ onChange, initial }: { onChange?: (g: GameFile) => void; initial?: BuilderState }) {
  const [g, setG] = useState(start())
  const [b, setB] = useState<BuilderState>(initial ?? initialBuilder)
  return (
    <Builder
      game={g}
      setGame={(n) => {
        setG(n)
        onChange?.(n)
      }}
      b={b}
      setB={setB}
      finishRoute={() => {}}
      note={null}
    />
  )
}

const run = (el: HTMLElement) => axe.run(el, { rules: { 'color-contrast': { enabled: false } } })

describe('builder panel', () => {
  it('has no axe violations', async () => {
    const { container } = render(<Harness />)
    const res = await run(container)
    expect(res.violations.map((v) => `${v.id}: ${v.nodes[0]?.html}`)).toEqual([])
  })

  it('lists rules with plain-sentence names in order', () => {
    render(<Harness />)
    expect(screen.getByRole('group', { name: 'Rule 1: When health is below 20: flee' })).toBeInTheDocument()
    expect(screen.getByRole('group', { name: 'Rule 2: Always: patrol ring' })).toBeInTheDocument()
  })

  it('moves a rule with the buttons (the keyboard route to what dragging does)', async () => {
    const seen: GameFile[] = []
    render(<Harness onChange={(g) => seen.push(g)} />)
    await userEvent.click(screen.getByRole('button', { name: 'Move rule 2 up' }))
    expect(seen.at(-1)?.archetypes[0]?.rules.map((r) => r.then.kind)).toEqual(['patrol', 'flee'])
    expect(screen.getByRole('group', { name: /^Rule 1: Always: patrol ring/ })).toBeInTheDocument()
    // The first rule cannot go further up, the last cannot go down.
    expect(screen.getByRole('button', { name: 'Move rule 1 up' })).toBeDisabled()
    expect(screen.getByRole('button', { name: 'Move rule 2 down' })).toBeDisabled()
  })

  it('changing a rule kind swaps in sensible defaults and shows the right fields', async () => {
    const seen: GameFile[] = []
    render(<Harness onChange={(g) => seen.push(g)} />)
    const rule2 = screen.getByRole('group', { name: /^Rule 2/ })
    await userEvent.selectOptions(within(rule2).getByLabelText('Action'), 'say')
    expect(seen.at(-1)?.archetypes[0]?.rules[1]?.then).toEqual({ kind: 'say', text: 'Hello.', shout: false })
    expect(within(screen.getByRole('group', { name: /^Rule 2/ })).getByLabelText('Text')).toHaveValue('Hello.')
    await userEvent.selectOptions(within(screen.getByRole('group', { name: /^Rule 2/ })).getByLabelText('Condition'), 'sees')
    expect(seen.at(-1)?.archetypes[0]?.rules[1]?.when).toEqual({ kind: 'sees', who: 'player', within: 30 })
  })

  it('adds and deletes rules', async () => {
    const seen: GameFile[] = []
    render(<Harness onChange={(g) => seen.push(g)} />)
    await userEvent.click(screen.getByRole('button', { name: 'Add rule' }))
    expect(seen.at(-1)?.archetypes[0]?.rules).toHaveLength(3)
    await userEvent.click(screen.getByRole('button', { name: 'Delete rule 1' }))
    expect(seen.at(-1)?.archetypes[0]?.rules.map((r) => r.then.kind)).toEqual(['patrol', 'wander'])
  })

  it('attribute sliders move with the keyboard, and returning to the default drops the override', async () => {
    const seen: GameFile[] = []
    render(<Harness onChange={(g) => seen.push(g)} />)
    const thumb = screen.getByRole('slider', { name: 'Start health' })
    expect(thumb).toHaveAttribute('aria-valuenow', '60')
    thumb.focus()
    await userEvent.keyboard('{ArrowRight}')
    expect(seen.at(-1)?.archetypes[0]?.attrs.health).toBe(61)
    await userEvent.keyboard('{End}')
    // 100 is the attribute's default, so no override is stored.
    expect(seen.at(-1)?.archetypes[0]?.attrs).toEqual({})
  })

  it('renaming a place in the list updates the rules that use it', async () => {
    const g = start()
    g.archetypes[0]!.rules.push({ when: { kind: 'always' }, then: { kind: 'goto', place: 'plaza' } })
    const seen: GameFile[] = []
    function H() {
      const [gg, setG] = useState(g)
      const [b, setB] = useState<BuilderState>(initialBuilder)
      return <Builder game={gg} setGame={(n) => { setG(n); seen.push(n) }} b={b} setB={setB} finishRoute={() => {}} note={null} />
    }
    render(<H />)
    const name = screen.getByRole('textbox', { name: 'Place' })
    await userEvent.clear(name)
    await userEvent.type(name, 'square')
    expect(seen.at(-1)?.archetypes[0]?.rules[2]?.then).toEqual({ kind: 'goto', place: 'square' })
  })

  it('deleting a route removes patrol rules that used it', async () => {
    const seen: GameFile[] = []
    render(<Harness onChange={(g) => seen.push(g)} />)
    await userEvent.click(screen.getByRole('button', { name: 'Delete route ring' }))
    expect(seen.at(-1)?.routes).toEqual([])
    expect(seen.at(-1)?.archetypes[0]?.rules.map((r) => r.then.kind)).toEqual(['flee'])
  })

  it('tool buttons report their pressed state and change the hint', async () => {
    render(<Harness />)
    const place = screen.getByRole('button', { name: 'Add place' })
    expect(place).toHaveAttribute('aria-pressed', 'false')
    await userEvent.click(place)
    expect(place).toHaveAttribute('aria-pressed', 'true')
    expect(screen.getByText('Click the map to drop a place.')).toBeInTheDocument()
  })
})
