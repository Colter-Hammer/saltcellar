// The tile icons in public/ (docs/spec/visual-system.md#logo): made from the current mark, at
// their sizes, and linked from index.html.
import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { describe, expect, it } from 'vitest'
import { PNGS, readMark, tileSvg } from '../scripts/icons.js'

const read = (path: string) => readFileSync(join(import.meta.dirname, '..', path))

/** A PNG's width and height, from its IHDR chunk. */
function pngSize(png: Buffer): [number, number] {
  expect(png.subarray(1, 4).toString('latin1')).toBe('PNG')
  return [png.readUInt32BE(16), png.readUInt32BE(20)]
}

describe('the tile icons', () => {
  it('favicon.svg is the tile drawn from mark.svg (run `just icons` after changing the mark)', () => {
    expect(read('public/favicon.svg').toString()).toBe(tileSvg(readMark()))
  })

  it.each(PNGS)('$file is $size×$size', ({ file, size }) => {
    expect(pngSize(read(`public/${file}`))).toEqual([size, size])
  })

  it('index.html links the favicon and the apple-touch-icon', () => {
    const html = read('index.html').toString()
    expect(html).toContain('<link rel="icon" href="/favicon.svg" type="image/svg+xml" />')
    expect(html).toContain('<link rel="apple-touch-icon" href="/apple-touch-icon.png" />')
  })
})
