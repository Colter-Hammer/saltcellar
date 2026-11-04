import { describe, expect, it } from 'vitest'
import { CACHED_MODE, CACHED_PALETTE } from './appearance'
import { DEFAULT_PALETTE, PALETTES, paletteOrDefault } from './palettes'
import indexHtml from '../../index.html?raw'

describe('paletteOrDefault', () => {
  it('keeps a palette it knows, and shows Deep blue for none or one it does not', () => {
    expect(paletteOrDefault('plum')).toBe('plum')
    for (const id of [null, undefined, '', 'not-a-palette'])
      expect(paletteOrDefault(id)).toBe('deep-blue')
  })
})

describe('the registry', () => {
  it('has a CSS file for each palette, and a palette for each CSS file', () => {
    const files = Object.keys(import.meta.glob('../styles/palettes/*.css')).map((path) =>
      path.replace(/^.*\/|\.css$/g, ''),
    )
    expect(PALETTES.map((palette) => palette.id).sort()).toEqual(files.sort())
    expect(PALETTES.some((palette) => palette.id === DEFAULT_PALETTE)).toBe(true)
  })
})

describe("index.html's inline script", () => {
  it('reads the keys the app caches, and falls back to the default palette', () => {
    const html = indexHtml
    expect(html).toContain(`'${CACHED_PALETTE}'`)
    expect(html).toContain(`'${CACHED_MODE}'`)
    expect(html).toContain(`var palette = '${DEFAULT_PALETTE}'`)
  })
})
