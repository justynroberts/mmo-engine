import { render, screen } from '@testing-library/react'
import axe from 'axe-core'
import { describe, expect, it } from 'vitest'
import Scene3D from './Scene3D'

// jsdom has no WebGL, so this covers the fallback path: it must say so, not throw or go blank.
describe('Scene3D without WebGL', () => {
  it('explains itself and stays accessible', async () => {
    const { container } = render(
      <Scene3D world={null} places={[]} routes={[]} stateRef={{ current: null }} selected={null} onSelect={() => undefined} follow={false} interest={128} />,
    )
    expect(await screen.findByRole('status')).toHaveTextContent(/WebGL is not available/i)
    const r = await axe.run(container, { rules: { 'color-contrast': { enabled: false } } })
    expect(r.violations).toEqual([])
  })
})
