import { describe, expect, it } from 'vitest'
import {
  canMoveCategory,
  canMoveGroup,
  dropCategory,
  dropGroup,
  hiddenOnes,
  moveCategory,
  moveGroup,
  moveToGroup,
} from './categories'

const category = (id: number, hidden = false) => ({ id, hidden })

/** Food (1): 11, 12, hidden 13, 14 · hidden Pets (2): 21 · Fun (3): 31. */
const groups = [
  {
    id: 1,
    hidden: false,
    categories: [category(11), category(12), category(13, true), category(14)],
  },
  { id: 2, hidden: true, categories: [category(21)] },
  { id: 3, hidden: false, categories: [category(31)] },
]

const ids = (list: readonly { id: number }[]) => list.map((item) => item.id)
const order = (list: typeof groups) => list.map((group) => [group.id, ids(group.categories)])

describe('moving a group up or down', () => {
  it('steps over hidden groups, which keep their places', () => {
    expect(ids(moveGroup(groups, 3, -1))).toEqual([3, 2, 1])
    expect(ids(moveGroup(groups, 1, 1))).toEqual([3, 2, 1])
  })

  it("stays put at either end of the shown ones, and for a group that isn't there", () => {
    expect(canMoveGroup(groups, 1, -1)).toBe(false)
    expect(canMoveGroup(groups, 3, 1)).toBe(false)
    expect(canMoveGroup(groups, 2, 1)).toBe(false)
    expect(canMoveGroup(groups, 1, 1)).toBe(true)
    expect(ids(moveGroup(groups, 1, -1))).toEqual([1, 2, 3])
    expect(ids(moveGroup(groups, 9, 1))).toEqual([1, 2, 3])
  })
})

describe('moving a category up or down', () => {
  it('stays in its group and steps over hidden ones', () => {
    expect(order(moveCategory(groups, 14, -1))[0]).toEqual([1, [11, 14, 13, 12]])
    expect(order(moveCategory(groups, 11, 1))[0]).toEqual([1, [12, 11, 13, 14]])
  })

  it('stays put at either end of its group', () => {
    expect(canMoveCategory(groups, 11, -1)).toBe(false)
    expect(canMoveCategory(groups, 14, 1)).toBe(false)
    expect(canMoveCategory(groups, 31, 1)).toBe(false)
    expect(canMoveCategory(groups, 12, 1)).toBe(true)
    expect(canMoveCategory(groups, 99, 1)).toBe(false)
    expect(order(moveCategory(groups, 31, -1))).toEqual(order(groups))
  })
})

describe('dropping a dragged group', () => {
  it('lands before the group it is dropped on when dragged up, after it when dragged down', () => {
    expect(ids(dropGroup(groups, 3, 1))).toEqual([3, 1, 2])
    expect(ids(dropGroup(groups, 1, 3))).toEqual([2, 3, 1])
  })

  it('changes nothing when dropped on nothing known', () => {
    expect(ids(dropGroup(groups, 1, 9))).toEqual([1, 2, 3])
  })
})

describe('dropping a dragged category', () => {
  it('takes the place of the one it is dropped on in its own group', () => {
    expect(order(dropCategory(groups, 14, 11))[0]).toEqual([1, [14, 11, 12, 13]])
    expect(order(dropCategory(groups, 11, 14))[0]).toEqual([1, [12, 13, 14, 11]])
  })

  it('moves to another group, just before the one it is dropped on', () => {
    expect(order(dropCategory(groups, 12, 31))).toEqual([
      [1, [11, 13, 14]],
      [2, [21]],
      [3, [12, 31]],
    ])
  })

  it('changes nothing for a category that is not there', () => {
    expect(order(dropCategory(groups, 99, 11))).toEqual(order(groups))
    expect(order(dropCategory(groups, 11, 99))).toEqual(order(groups))
  })
})

describe('moving a category to another group', () => {
  it('puts it at the end', () => {
    expect(order(moveToGroup(groups, 11, 3))).toEqual([
      [1, [12, 13, 14]],
      [2, [21]],
      [3, [31, 11]],
    ])
  })

  it('changes nothing for a group or category that is not there', () => {
    expect(order(moveToGroup(groups, 11, 9))).toEqual(order(groups))
    expect(order(moveToGroup(groups, 99, 3))).toEqual(order(groups))
  })
})

it("lists hidden groups, then hidden categories of shown groups, but not a hidden group's own", () => {
  const hidden = hiddenOnes(groups)
  expect(ids(hidden.groups)).toEqual([2])
  expect(hidden.categories.map(({ category, group }) => [category.id, group.id])).toEqual([[13, 1]])
})
