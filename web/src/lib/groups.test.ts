import { describe, expect, it } from 'vitest'
import { GROUP_ICONS, groupColor, groupIcon, groupLooks } from './groups'

describe('groupColor', () => {
  it('gives the n-th group --cat-n', () => {
    expect(groupColor(0)).toBe('var(--cat-1)')
    expect(groupColor(11)).toBe('var(--cat-12)')
  })

  it('cycles past 12 with a lightness shift, once more each time round', () => {
    expect(groupColor(12)).toBe('oklch(from var(--cat-1) calc(l + 1 * var(--cat-cycle-shift)) c h)')
    expect(groupColor(12)).not.toBe(groupColor(0))
    expect(groupColor(25)).toBe('oklch(from var(--cat-2) calc(l + 2 * var(--cat-cycle-shift)) c h)')
  })
})

describe('groupIcon', () => {
  it("is the group's own when it's in the set", () => {
    expect(groupIcon('heart-pulse').name).toBe('heart-pulse')
  })

  it('is folder for none, or a name not in the set', () => {
    expect(groupIcon(null).name).toBe('folder')
    expect(groupIcon(undefined).name).toBe('folder')
    expect(groupIcon('no-such-icon').name).toBe('folder')
  })

  it('offers about 40 icons, each once, the starter set among them', () => {
    const names = GROUP_ICONS.map((icon) => icon.name)
    expect(names.length).toBe(40)
    expect(new Set(names).size).toBe(names.length)
    for (const starter of [
      'house',
      'utensils',
      'car',
      'heart-pulse',
      'shield',
      'baby',
      'shopping-bag',
      'party-popper',
      'hand-heart',
      'paw-print',
      'landmark',
      'wallet',
    ]) {
      expect(names).toContain(starter)
    }
  })
})

describe('groupLooks', () => {
  const groups = [
    { id: 7, icon: 'house', categories: [{ id: 70 }, { id: 71 }] },
    { id: 3, hidden: true, icon: null, categories: [{ id: 30 }] },
    { id: 9, categories: [] },
  ]

  it('colors by position, hidden groups included, so hiding one changes no other', () => {
    const { byGroup } = groupLooks(groups)
    expect(byGroup.get(7)?.color).toBe('var(--cat-1)')
    expect(byGroup.get(3)?.color).toBe('var(--cat-2)')
    expect(byGroup.get(9)?.color).toBe('var(--cat-3)')
    expect(byGroup.get(9)?.icon.name).toBe('folder')
  })

  it('recolors when the groups are reordered', () => {
    const { byGroup } = groupLooks([...groups].reverse())
    expect(byGroup.get(9)?.color).toBe('var(--cat-1)')
    expect(byGroup.get(7)?.color).toBe('var(--cat-3)')
  })

  it("gives each category its group's look", () => {
    const { byGroup, byCategory } = groupLooks(groups)
    expect(byCategory.get(71)).toBe(byGroup.get(7))
    expect(byCategory.get(30)).toBe(byGroup.get(3))
    expect(byCategory.get(99)).toBeUndefined()
  })
})
