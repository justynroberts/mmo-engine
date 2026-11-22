import '@testing-library/jest-dom/vitest'
import { configure } from '@testing-library/react'

class RO {
  observe() {}
  unobserve() {}
  disconnect() {}
}
// jsdom has no layout engine or ResizeObserver.
;(globalThis as unknown as { ResizeObserver: typeof RO }).ResizeObserver = RO

// findBy and waitFor give up after 1 s by default, which a busy machine can miss on the bigger renders.
configure({ asyncUtilTimeout: 5000 })
