import { describe, expect, it } from 'vitest'
import { NameBook } from './names'

describe('names', () => {
  it('shows a number until the name arrives, then the name', async () => {
    const book = new NameBook()
    const id = 5 | (2 << 24)
    expect(book.name(id)).toBe('#5')
    let asked = ''
    const changed = await book.poll(async (url) => {
      asked = url
      return { [String(id)]: 'Ana Lee' }
    })
    expect(asked).toBe(`/play/names?ids=${id}`)
    expect(changed).toEqual([id])
    expect(book.name(id)).toBe('Ana Lee')
  })

  it('asks again about refreshed ids and reports only real changes', async () => {
    const book = new NameBook()
    book.refresh([1, 2])
    expect(await book.poll(async () => ({ 1: 'Drifter', 2: 'Bob' }))).toEqual([1, 2])
    book.refresh([1, 2])
    expect(await book.poll(async () => ({ 1: 'Drifter', 2: 'Robert' }))).toEqual([2])
    expect(await book.poll(async () => ({}))).toEqual([]) // nothing wanted: no request
  })

  it('keeps wanting ids when the request fails', async () => {
    const book = new NameBook()
    book.name(9)
    expect(await book.poll(async () => Promise.reject(new Error('down')))).toEqual([])
    expect(await book.poll(async () => ({ 9: 'Late' }))).toEqual([9])
  })
})
