import { render, screen, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import axe from 'axe-core'
import { useState } from 'react'
import type { GameFile } from '../api'
import { fixture } from '../test/fixture'
import { ActionsEditor, AttrsEditor, BotsEditor, QuestsEditor } from './Editors'

/** Controlled wrapper so editors behave as they do in the app. */
function Harness({ kind, onChange }: { kind: 'attrs' | 'actions' | 'bots' | 'quests'; onChange?: (g: GameFile) => void }) {
  const [g, setG] = useState<GameFile>(() => ({ ...fixture(), places: [{ name: 'den', x: 10, y: 10, r: 5 }], items: [{ name: 'pelt', color: '#a50', stack_max: 5, uses: [], consume: false }] }))
  const set = (n: GameFile) => {
    setG(n)
    onChange?.(n)
  }
  if (kind === 'attrs') return <AttrsEditor game={g} setGame={set} />
  if (kind === 'actions') return <ActionsEditor game={g} setGame={set} />
  if (kind === 'quests') return <QuestsEditor game={g} setGame={set} />
  return <BotsEditor game={g} setGame={set} liveCounts={[3]} onSpawn={() => {}} onKillAll={() => {}} />
}

// jsdom has no layout, so colour contrast is checked in the real-browser suite instead.
const run = (el: HTMLElement) => axe.run(el, { rules: { 'color-contrast': { enabled: false } } })

describe('editors', () => {
  it.each(['attrs', 'actions', 'bots', 'quests'] as const)('%s editor has no axe violations', async (kind) => {
    const { container } = render(<Harness kind={kind} />)
    const res = await run(container)
    expect(res.violations.map((v) => `${v.id}: ${v.nodes[0]?.html}`)).toEqual([])
  })

  it('editing a number commits only valid input', async () => {
    const seen: GameFile[] = []
    render(<Harness kind="attrs" onChange={(g) => seen.push(g)} />)
    const max = screen.getAllByLabelText('Max')[0] as HTMLInputElement
    await userEvent.clear(max)
    expect(max).toHaveAttribute('aria-invalid', 'true')
    await userEvent.type(max, '250')
    expect(seen.at(-1)?.attrs[0]?.max).toBe(250)
    expect(max).toHaveAttribute('aria-invalid', 'false')
    await userEvent.type(max, 'x')
    // The garbage character did not reach the game file.
    expect(seen.at(-1)?.attrs[0]?.max).toBe(250)
  })

  it('renaming an attribute in the form updates the action editor that refers to it', async () => {
    const seen: GameFile[] = []
    render(<Harness kind="attrs" onChange={(g) => seen.push(g)} />)
    const name = screen.getByDisplayValue('health')
    await userEvent.clear(name)
    await userEvent.type(name, 'hp')
    const last = seen.at(-1)!
    expect(last.attrs[0]?.name).toBe('hp')
    expect(last.actions[0]?.effects[0]?.attr).toBe('hp')
    expect(last.archetypes[0]?.attrs).toEqual({ hp: 60 })
  })

  it('adds and deletes attributes', async () => {
    const seen: GameFile[] = []
    render(<Harness kind="attrs" onChange={(g) => seen.push(g)} />)
    await userEvent.click(screen.getByRole('button', { name: /add attribute/i }))
    expect(seen.at(-1)?.attrs.map((a) => a.name)).toEqual(['health', 'energy', 'attr'])
    await userEvent.click(screen.getByRole('button', { name: 'Delete attribute energy' }))
    expect(seen.at(-1)?.attrs.map((a) => a.name)).toEqual(['health', 'attr'])
    expect(seen.at(-1)?.actions[0]?.requires).toBeNull()
  })

  it('duplicate attribute names are flagged', async () => {
    render(<Harness kind="attrs" />)
    const second = screen.getByDisplayValue('energy')
    await userEvent.clear(second)
    await userEvent.type(second, 'health')
    expect(second).toHaveAttribute('aria-invalid', 'true')
  })

  it('bot editor shows live counts and edits behaviour', async () => {
    const seen: GameFile[] = []
    render(<Harness kind="bots" onChange={(g) => seen.push(g)} />)
    expect(screen.getByLabelText('3 alive')).toBeInTheDocument()
    await userEvent.selectOptions(screen.getByLabelText('Behaviour'), 'flee')
    expect(seen.at(-1)?.archetypes[0]?.behaviour).toBe('flee')
    await userEvent.selectOptions(screen.getByLabelText('Attacks with'), '')
    expect(seen.at(-1)?.archetypes[0]?.attack).toBeNull()
  })

  it('action editor adds an effect and a requirement', async () => {
    const seen: GameFile[] = []
    render(<Harness kind="actions" onChange={(g) => seen.push(g)} />)
    await userEvent.click(screen.getByRole('button', { name: /add effect/i }))
    expect(seen.at(-1)?.actions[0]?.effects).toHaveLength(3)
    await userEvent.click(screen.getByRole('button', { name: 'Remove requirement' }))
    expect(seen.at(-1)?.actions[0]?.requires).toBeNull()
    expect(within(document.body).getByRole('button', { name: /add requirement/i })).toBeEnabled()
  })

  it('quest editor adds a quest, steps of each kind and a reward, and deletes it', async () => {
    const seen: GameFile[] = []
    const { container } = render(<Harness kind="quests" onChange={(g) => seen.push(g)} />)
    await userEvent.click(screen.getByRole('button', { name: /add quest/i }))
    expect(seen.at(-1)?.quests?.[0]).toMatchObject({ name: 'quest', giver: 'drone', steps: [{ kind: 'visit', place: 'den' }] })
    await userEvent.click(screen.getByRole('button', { name: /add step/i }))
    await userEvent.selectOptions(screen.getByLabelText('Step 2'), 'collect')
    expect(seen.at(-1)?.quests?.[0]?.steps[1]).toEqual({ kind: 'collect', item: 'pelt', count: 3 })
    await userEvent.selectOptions(screen.getByLabelText('Step 2'), 'kill')
    expect(seen.at(-1)?.quests?.[0]?.steps[1]).toEqual({ kind: 'kill', archetype: 'drone', count: 5 })
    await userEvent.click(screen.getByRole('button', { name: /add reward/i }))
    expect(seen.at(-1)?.quests?.[0]?.reward).toEqual([{ item: 'pelt', count: 1 }])
    await userEvent.click(screen.getByRole('button', { name: 'Remove reward pelt' }))
    expect(seen.at(-1)?.quests?.[0] && 'reward' in seen.at(-1)!.quests![0]!).toBe(false)
    await userEvent.click(screen.getByRole('button', { name: 'Remove step 2' }))
    expect(seen.at(-1)?.quests?.[0]?.steps).toHaveLength(1)
    const res = await run(container)
    expect(res.violations.map((v) => `${v.id}: ${v.nodes[0]?.html}`)).toEqual([])
    await userEvent.click(screen.getByRole('button', { name: 'Delete quest quest' }))
    expect(seen.at(-1)?.quests).toEqual([])
  })
})
