import '@testing-library/jest-dom/vitest'

class RO {
  observe() {}
  unobserve() {}
  disconnect() {}
}
// jsdom has no layout engine or ResizeObserver.
;(globalThis as unknown as { ResizeObserver: typeof RO }).ResizeObserver = RO
