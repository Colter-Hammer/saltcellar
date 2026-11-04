// Sortable lists (Accounts, Settings › Categories): a row is lifted by its grip and follows the
// pointer, the others slide to open the gap where it will land, and the order is saved once on
// drop. Pointer events, so one code path serves a mouse, a finger and a pen; the grip is a button
// with Up / Down keys for the keyboard. Written here rather than pulled in: the lists are short,
// the moves are the pure functions in accounts.ts and categories.ts, and the rest is ~200 lines
// *(agent)*.
//
// How it works: while a drag lasts the list shows a *preview* order, so the DOM really reorders
// and the layout stays true. Every reorder is a FLIP: measure the rows, change the order, measure
// again, and animate each row from where it was. The lifted row isn't animated; it's offset by a
// transform that keeps it under the pointer whatever its layout slot is.
//
// Markup: every row (and anything that holds rows) is a *box*, `data-sort-kind` + `data-sort-id`.
// Inside a box, `data-sort-edge="start" | "end"` marks the part of it that means "the start / end
// of this box" for a row dropped there (a group's header and its foot).
import { computed, nextTick, onScopeDispose, ref, shallowRef } from 'vue'

export interface Drag {
  kind: string
  id: number
}

export interface Over extends Drag {
  /** Set when the pointer is on a part of the box that means its start or its end. */
  edge?: 'start' | 'end'
}

export interface SortableOptions<M> {
  /** The saved order. */
  model: () => M
  /** `model` with `drag` dropped on `over`, or null if it doesn't go there. */
  drop: (model: M, drag: Drag, over: Over) => M | null
  /** `model` with `drag` moved one place (the arrow keys), or null if it can't. */
  step: (model: M, drag: Drag, by: -1 | 1) => M | null
  /** What changed in `model`, so two orders compare: any string that differs when they do. */
  signature: (model: M) => string
  /** Where `drag` stands in `model`, for the screen reader: "Rewards card, 2 of 5". */
  describe: (model: M, drag: Drag) => string
  /** Saves `model`; it should show it at once. Errors are the caller's to show. */
  save: (model: M) => Promise<unknown>
}

const SLIDE_MS = 180
/** Near the top or bottom of the window (clear of the phone's header and tab bar) a drag scrolls it. */
const SCROLL_EDGE = 96
const SCROLL_SPEED = 16
const EASING = 'cubic-bezier(0.2, 0, 0, 1)'

const reducedMotion = () => globalThis.matchMedia('(prefers-reduced-motion: reduce)').matches

/** Every laid-out box on the page, with where it is now (a hidden layout has no size). */
function boxes(): Map<HTMLElement, DOMRect> {
  const found = new Map<HTMLElement, DOMRect>()
  for (const box of document.querySelectorAll<HTMLElement>('[data-sort-kind]')) {
    const rect = box.getBoundingClientRect()
    if (rect.width > 0 || rect.height > 0) found.set(box, rect)
  }
  return found
}

/** The boxes the pointer is over, innermost first, with the edge each is pointed at. */
function boxesAt(x: number, y: number): { box: HTMLElement; over: Over }[] {
  const found: { box: HTMLElement; over: Over }[] = []
  const seen = new Set<HTMLElement>()
  for (const element of document.elementsFromPoint(x, y)) {
    let box = element.closest<HTMLElement>('[data-sort-kind]')
    while (box !== null) {
      if (!seen.has(box)) {
        seen.add(box)
        const edge = element.closest<HTMLElement>('[data-sort-edge]')
        const side = edge !== null && box.contains(edge) ? edge.dataset.sortEdge : undefined
        found.push({
          box,
          over: {
            kind: box.dataset.sortKind ?? '',
            id: Number(box.dataset.sortId),
            ...(side === 'start' || side === 'end' ? { edge: side } : {}),
          },
        })
      }
      box = box.parentElement?.closest<HTMLElement>('[data-sort-kind]') ?? null
    }
  }
  return found
}

export function useSortable<M>(options: SortableOptions<M>) {
  /** The order on screen while a drag or a save is under way; null shows the saved one. */
  const preview = shallowRef<M | null>(null)
  const shown = computed<M>(() => (preview.value as M | null) ?? options.model())
  /** The one being dragged. */
  const dragging = ref<Drag | null>(null)
  /** What a screen reader should hear after a move. */
  const announcement = ref('')

  const animations = new Map<HTMLElement, Animation>()
  let saving = 0

  /** Runs `change`, then slides every box from where it was to where it is, bar `except`. */
  async function slide(change: () => void, except?: HTMLElement) {
    const before = boxes()
    for (const animation of animations.values()) animation.cancel()
    animations.clear()
    change()
    await nextTick()
    if (reducedMotion()) return
    for (const [box, first] of before) {
      if (box === except || !box.isConnected) continue
      const last = box.getBoundingClientRect()
      const dx = first.left - last.left
      const dy = first.top - last.top
      if (Math.abs(dx) < 0.5 && Math.abs(dy) < 0.5) continue
      const animation = box.animate(
        [{ transform: `translate(${String(dx)}px, ${String(dy)}px)` }, { transform: 'none' }],
        { duration: SLIDE_MS, easing: EASING },
      )
      animations.set(box, animation)
      // Finished or cancelled, it's no longer sliding.
      const done = () => {
        if (animations.get(box) === animation) animations.delete(box)
      }
      void animation.finished.then(done, done)
    }
  }

  /** Saves `order`, which is on screen already; the preview goes once nothing is left to save. */
  async function persist(order: M) {
    saving++
    try {
      await options.save(order)
    } finally {
      saving--
      if (saving === 0 && dragging.value === null) await slide(() => (preview.value = null))
    }
  }

  // ── Keyboard ──

  async function key(event: KeyboardEvent, kind: string, id: number) {
    if (event.key !== 'ArrowUp' && event.key !== 'ArrowDown') return
    event.preventDefault()
    if (dragging.value !== null) return
    const drag = { kind, id }
    const from = shown.value
    const next = options.step(from, drag, event.key === 'ArrowUp' ? -1 : 1)
    if (next === null || options.signature(next) === options.signature(from)) return
    const grip = event.currentTarget as HTMLElement
    announcement.value = options.describe(next, drag)
    await slide(() => (preview.value = next))
    // Moving the row in the DOM can drop its focus; put it back.
    if (grip.isConnected) grip.focus()
    await persist(next)
  }

  // ── Pointer ──

  let cleanup: (() => void) | null = null

  function start(event: PointerEvent, kind: string, id: number) {
    if (dragging.value !== null || (event.pointerType === 'mouse' && event.button !== 0)) return
    const grip = event.currentTarget as HTMLElement
    const found = grip.closest<HTMLElement>(
      `[data-sort-kind="${kind}"][data-sort-id="${String(id)}"]`,
    )
    if (found === null) return
    const lifted: HTMLElement = found
    event.preventDefault()

    const drag: Drag = { kind, id }
    const natural = lifted.getBoundingClientRect()
    const grabX = event.clientX - natural.left
    const grabY = event.clientY - natural.top
    const height = natural.height
    const origin = options.signature(shown.value)
    let x = event.clientX
    let y = event.clientY
    let frame = 0
    let scrolling = 0
    let working = false
    let again = false

    dragging.value = drag
    lifted.dataset.sortDragging = ''
    const { userSelect } = document.body.style
    document.body.style.userSelect = 'none'
    preview.value = shown.value

    /** Puts the lifted box under the pointer, whatever slot the layout gave it. */
    function follow() {
      lifted.style.removeProperty('transform')
      const slot = lifted.getBoundingClientRect()
      lifted.style.setProperty(
        'transform',
        `translate(${String(x - grabX - slot.left)}px, ${String(y - grabY - slot.top)}px)`,
      )
    }

    /** Keeps the lifted box under the pointer while the others slide, which can move its parent. */
    function followWhileSliding() {
      if (dragging.value === null) return
      follow()
      if (animations.size > 0) requestAnimationFrame(followWhileSliding)
    }

    /** Whether the pointer is where the lifted box would stand if it took `box`'s place. */
    function lands(box: HTMLElement, below: boolean) {
      const rect = box.getBoundingClientRect()
      return below ? y >= rect.bottom - height : y <= rect.top + height
    }

    /** Whether another pointer move came in while `update` was working. */
    const asked = () => again

    async function update() {
      if (working) {
        again = true
        return
      }
      working = true
      try {
        for (;;) {
          again = false
          follow()
          const from = shown.value
          let next: M | null = null
          for (const { box, over } of boxesAt(x, y)) {
            if (box === lifted || lifted.contains(box)) continue
            const moved = options.drop(from, drag, over)
            if (moved === null) continue
            // Rows of one kind swap once the pointer is in the slot the swap would give the
            // lifted one, so it never swaps straight back whatever the heights.
            const below =
              (lifted.compareDocumentPosition(box) & Node.DOCUMENT_POSITION_FOLLOWING) !== 0
            if (over.kind === kind && !lands(box, below)) continue
            next = moved
            break
          }
          if (next !== null && options.signature(next) !== options.signature(from)) {
            const target = next
            await slide(() => (preview.value = target), lifted)
            followWhileSliding()
          }
          if (!asked()) break
        }
      } finally {
        working = false
      }
    }

    /** Scrolls the window while the pointer is near its top or bottom, so a far target can be reached. */
    function autoscroll() {
      const above = SCROLL_EDGE - y
      const below = y - (window.innerHeight - SCROLL_EDGE)
      const speed = Math.max(above, below, 0) / SCROLL_EDGE
      if (speed > 0) {
        window.scrollBy(0, (above > 0 ? -1 : 1) * Math.min(speed, 1) * SCROLL_SPEED)
        // The content moved under the pointer: find what is there now.
        schedule()
      }
      scrolling = requestAnimationFrame(autoscroll)
    }

    function schedule() {
      if (frame !== 0) return
      frame = requestAnimationFrame(() => {
        frame = 0
        void update()
      })
    }

    function finish(drop: boolean) {
      cleanup?.()
      cleanup = null
      document.body.style.userSelect = userSelect
      const final = shown.value
      const changed = drop && options.signature(final) !== origin
      // The lifted row settles into its slot, then the drag is over.
      void (async () => {
        await slide(() => {
          lifted.style.removeProperty('transform')
          delete lifted.dataset.sortDragging
          if (!changed) preview.value = null
        })
        dragging.value = null
        if (changed) {
          announcement.value = options.describe(final, drag)
          await persist(final)
        }
      })()
    }

    const move = (e: PointerEvent) => {
      if (e.pointerId !== event.pointerId) return
      e.preventDefault()
      x = e.clientX
      y = e.clientY
      schedule()
    }
    const up = (e: PointerEvent) => {
      if (e.pointerId === event.pointerId) finish(true)
    }
    const cancel = (e: PointerEvent) => {
      if (e.pointerId === event.pointerId) finish(false)
    }
    const escape = (e: KeyboardEvent) => {
      if (e.key === 'Escape') finish(false)
    }
    const noMenu = (e: Event) => {
      e.preventDefault()
    }
    window.addEventListener('pointermove', move, { passive: false })
    window.addEventListener('pointerup', up)
    window.addEventListener('pointercancel', cancel)
    window.addEventListener('keydown', escape)
    window.addEventListener('contextmenu', noMenu)
    cleanup = () => {
      cancelAnimationFrame(frame)
      cancelAnimationFrame(scrolling)
      window.removeEventListener('pointermove', move)
      window.removeEventListener('pointerup', up)
      window.removeEventListener('pointercancel', cancel)
      window.removeEventListener('keydown', escape)
      window.removeEventListener('contextmenu', noMenu)
    }
    follow()
    autoscroll()
  }

  onScopeDispose(() => {
    cleanup?.()
    for (const animation of animations.values()) animation.cancel()
  })

  return { shown, dragging, announcement, start, key }
}
