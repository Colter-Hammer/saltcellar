// Rearranging Settings › Categories (docs/spec/m1-ledger-import.md#categories): each move returns
// the whole new order, which the screen shows at once and sends to the server. Moves step over
// hidden groups and categories, which keep their places among the rest *(agent)*.

interface Item {
  id: number
  hidden: boolean
}

/** A group of `C`s; the extra fields of both come through every move unchanged. */
type Group<G extends Item, C extends Item> = G & { categories: C[] }

/** `items` with the one whose id is `id` taken out and put in at `index`. */
function place<T extends Item>(items: readonly T[], id: number, index: number): T[] {
  const moving = items.find((item) => item.id === id)
  if (moving === undefined) return [...items]
  const rest = items.filter((item) => item.id !== id)
  rest.splice(index, 0, moving)
  return rest
}

/**
 * `items` with `id` swapped with the next shown item before it (`by` -1) or after it (1), or null
 * when it's already first or last of the shown ones.
 */
function step<T extends Item>(items: readonly T[], id: number, by: -1 | 1): T[] | null {
  const shown = items.filter((item) => !item.hidden)
  const at = shown.findIndex((item) => item.id === id)
  const other = shown[at + by]
  if (at < 0 || other === undefined) return null
  return items.map((item) => {
    if (item.id === id) return other
    if (item.id === other.id) return shown[at] ?? item
    return item
  })
}

/** Whether the group `id` can move up (-1) or down (1) among the shown groups. */
export function canMoveGroup(groups: readonly Item[], id: number, by: -1 | 1) {
  return step(groups, id, by) !== null
}

/** The groups with `id` moved one place up (-1) or down (1) among the shown ones. */
export function moveGroup<G extends Item>(groups: readonly G[], id: number, by: -1 | 1): G[] {
  return step(groups, id, by) ?? [...groups]
}

/** The group holding the category `id`. */
function groupOf<G extends Item, C extends Item>(groups: readonly Group<G, C>[], id: number) {
  return groups.find((group) => group.categories.some((category) => category.id === id))
}

/** Whether the category `id` can move up (-1) or down (1) among its group's shown ones. */
export function canMoveCategory(groups: readonly Group<Item, Item>[], id: number, by: -1 | 1) {
  const group = groupOf(groups, id)
  return group !== undefined && step(group.categories, id, by) !== null
}

/** The groups with the category `id` moved one place up (-1) or down (1) in its group. */
export function moveCategory<G extends Item, C extends Item>(
  groups: readonly Group<G, C>[],
  id: number,
  by: -1 | 1,
): Group<G, C>[] {
  return groups.map((group) => {
    const moved = step(group.categories, id, by)
    return moved === null ? group : { ...group, categories: moved }
  })
}

/** The groups with `id` dropped where the group `onto` is: before it, or after it when dragged down. */
export function dropGroup<G extends Item>(groups: readonly G[], id: number, onto: number): G[] {
  const index = groups.findIndex((group) => group.id === onto)
  if (index < 0) return [...groups]
  return place(groups, id, index)
}

/**
 * The groups with the category `id` dropped where the category `onto` is, in `onto`'s group:
 * in the same group it takes `onto`'s place, in another it goes just before `onto`.
 */
export function dropCategory<G extends Item, C extends Item>(
  groups: readonly Group<G, C>[],
  id: number,
  onto: number,
): Group<G, C>[] {
  const from = groupOf(groups, id)
  const to = groupOf(groups, onto)
  if (from === undefined || to === undefined) return [...groups]
  if (from === to) {
    const index = to.categories.findIndex((category) => category.id === onto)
    return groups.map((group) =>
      group === to ? { ...group, categories: place(group.categories, id, index) } : group,
    )
  }
  return moveToGroup(groups, id, to.id, onto)
}

/**
 * The groups with the category `id` moved into the group `groupId`: just before the category
 * `before`, or at the end without it.
 */
export function moveToGroup<G extends Item, C extends Item>(
  groups: readonly Group<G, C>[],
  id: number,
  groupId: number,
  before?: number,
): Group<G, C>[] {
  const moving = groupOf(groups, id)?.categories.find((category) => category.id === id)
  if (moving === undefined || !groups.some((group) => group.id === groupId)) return [...groups]
  return groups.map((group) => {
    const categories = group.categories.filter((category) => category.id !== id)
    if (group.id !== groupId) return { ...group, categories }
    const index = categories.findIndex((category) => category.id === before)
    categories.splice(index < 0 ? categories.length : index, 0, moving)
    return { ...group, categories }
  })
}

/** What's hidden, for the Hidden section: hidden groups, then hidden categories in shown groups. */
export function hiddenOnes<G extends Item, C extends Item>(groups: readonly Group<G, C>[]) {
  return {
    groups: groups.filter((group) => group.hidden),
    categories: groups
      .filter((group) => !group.hidden)
      .flatMap((group) =>
        group.categories
          .filter((category) => category.hidden)
          .map((category) => ({ category, group })),
      ),
  }
}

/** What is dragged or dropped on in Settings › Categories. */
interface Spot {
  kind: string
  id: number
  /** Dropped on the start or end of a group (its header, its foot) rather than on a row. */
  edge?: 'start' | 'end'
}

/**
 * The groups with `drag` dropped on `over`, or null if that's nowhere it goes: a group takes the
 * place of the group it's dropped on (or holds the category it's dropped on); a category takes the
 * place of the category it's dropped on, or goes to the start or end of the group whose header or
 * foot it's dropped on *(agent)*.
 */
export function dropOnto<G extends Item, C extends Item>(
  groups: readonly Group<G, C>[],
  drag: Spot,
  over: Spot,
): Group<G, C>[] | null {
  if (drag.kind === 'group' && over.kind === 'group') {
    return drag.id === over.id ? null : dropGroup(groups, drag.id, over.id)
  }
  if (drag.kind !== 'category') return null
  if (over.kind === 'category') {
    return drag.id === over.id ? null : dropCategory(groups, drag.id, over.id)
  }
  if (over.kind !== 'group' || over.edge === undefined) return null
  const first = groups
    .find((group) => group.id === over.id)
    ?.categories.find((category) => category.id !== drag.id && !category.hidden)
  return moveToGroup(groups, drag.id, over.id, over.edge === 'start' ? first?.id : undefined)
}

/** The groups with `drag` moved one place up (-1) or down (1) among its siblings, or null. */
export function stepOnto<G extends Item, C extends Item>(
  groups: readonly Group<G, C>[],
  drag: Spot,
  by: -1 | 1,
): Group<G, C>[] | null {
  if (drag.kind === 'group') return step(groups, drag.id, by)
  return canMoveCategory(groups, drag.id, by) ? moveCategory(groups, drag.id, by) : null
}

/** The place of `drag` for a screen reader: "Food, 2 of 5" or "Groceries, 1 of 3 in Food". */
export function describeSpot<G extends Item & { name: string }, C extends Item & { name: string }>(
  groups: readonly Group<G, C>[],
  drag: Spot,
): string {
  const shown = groups.filter((group) => !group.hidden)
  if (drag.kind === 'group') {
    const at = shown.findIndex((group) => group.id === drag.id)
    return `${shown[at]?.name ?? ''}, ${String(at + 1)} of ${String(shown.length)}`
  }
  const group = groupOf(groups, drag.id)
  const siblings = group?.categories.filter((category) => !category.hidden) ?? []
  const at = siblings.findIndex((category) => category.id === drag.id)
  return `${siblings[at]?.name ?? ''}, ${String(at + 1)} of ${String(siblings.length)} in ${group?.name ?? ''}`
}
