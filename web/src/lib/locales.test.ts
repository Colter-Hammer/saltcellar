import { describe, expect, it } from 'vitest'
import { localeLabel, localeOptions } from './locales'

describe('localeLabel', () => {
  it('names the language and region in English, beside the tag', () => {
    expect(localeLabel('de-DE')).toBe('German (Germany) · de-DE')
  })

  it("is just the tag when there's no name for it, or it isn't one", () => {
    expect(localeLabel('xqq')).toBe('xqq')
    expect(localeLabel('not a locale')).toBe('not a locale')
  })
})

describe('localeOptions', () => {
  it("lists common locales and the browser's, once each, sorted by name", () => {
    const options = localeOptions('en-US')
    const values = options.map((o) => o.value)
    expect(values.filter((v) => v === 'en-US')).toHaveLength(1)
    expect(values).toContain('de-DE')
    const labels = options.map((o) => o.label)
    expect(labels).toEqual([...labels].sort((a, b) => a.localeCompare(b, 'en')))
    expect(localeOptions('mi-NZ').map((o) => o.value)).toContain('mi-NZ')
  })
})
