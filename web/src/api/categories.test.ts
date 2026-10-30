import { QueryClient, VueQueryPlugin } from '@tanstack/vue-query'
import { flushPromises, mount } from '@vue/test-utils'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { defineComponent } from 'vue'
import { useCategories, useCategoryEdits, type CategoryGroup } from './categories'
import { ApiError } from './client'

const food: CategoryGroup = {
  id: 1,
  name: 'Food',
  kind: 'expense',
  hidden: false,
  categories: [
    { id: 11, name: 'Groceries', hidden: false },
    { id: 12, name: 'Restaurants', hidden: false },
  ],
}

/** Every request sent, as `METHOD path body`. */
let sent: string[]

beforeEach(() => {
  sent = []
  vi.stubGlobal(
    'fetch',
    vi.fn(async (request: Request) => {
      const { pathname } = new URL(request.url)
      const body = await request.text()
      sent.push(`${request.method} ${pathname}${body ? ` ${body}` : ''}`)
      expect(request.headers.get('Content-Type')).toBe(
        request.method === 'GET' ? null : 'application/json',
      )
      if (request.method === 'GET') return Response.json({ groups: [food] })
      if (pathname.endsWith('/7')) {
        return Response.json(
          { error: { code: 'not_found', message: "There's no such category." } },
          { status: 404 },
        )
      }
      if (request.method === 'POST' && pathname !== '/api/v1/categories/starter') {
        return Response.json({ id: 99 }, { status: 201 })
      }
      return new Response(null, { status: 204 })
    }),
  )
})

afterEach(() => {
  vi.unstubAllGlobals()
})

/** The list and the edits, used from inside a component as the screen does. */
async function setUp() {
  let used:
    | { list: ReturnType<typeof useCategories>; edits: ReturnType<typeof useCategoryEdits> }
    | undefined
  const Host = defineComponent({
    setup() {
      used = { list: useCategories(), edits: useCategoryEdits() }
      return () => null
    },
  })
  mount(Host, { global: { plugins: [[VueQueryPlugin, { queryClient: new QueryClient() }]] } })
  await flushPromises()
  if (used === undefined) throw new Error('the host never ran its setup')
  return used
}

describe('the categories', () => {
  it('load as the server lists them', async () => {
    const { list } = await setUp()
    expect(list.data.value).toEqual({ groups: [food] })
  })

  it('send each change and reload the list after it', async () => {
    const { edits } = await setUp()
    sent = []
    await edits.applyStarter()
    expect(await edits.addGroup('Pets', 'expense')).toBe(99)
    await edits.changeGroup(1, { name: 'Eating' })
    await edits.deleteGroup(2)
    expect(await edits.addCategory(1, 'Coffee')).toBe(99)
    await edits.changeCategory(11, { hidden: true })
    await edits.deleteCategory(12)
    const reload = 'GET /api/v1/categories'
    expect(sent).toEqual([
      'POST /api/v1/categories/starter',
      reload,
      'POST /api/v1/category-groups {"name":"Pets","kind":"expense"}',
      reload,
      'PATCH /api/v1/category-groups/1 {"name":"Eating"}',
      reload,
      'DELETE /api/v1/category-groups/2',
      reload,
      'POST /api/v1/categories {"group_id":1,"name":"Coffee"}',
      reload,
      'PATCH /api/v1/categories/11 {"hidden":true}',
      reload,
      'DELETE /api/v1/categories/12',
      reload,
    ])
  })

  it('show a new order at once and send it as ids', async () => {
    const { list, edits } = await setUp()
    sent = []
    const swapped = { ...food, categories: [...food.categories].reverse() }
    const saving = edits.order([swapped])
    expect(list.data.value?.groups[0]?.categories.map((c) => c.name)).toEqual([
      'Restaurants',
      'Groceries',
    ])
    await saving
    expect(sent[0]).toBe('PUT /api/v1/categories/order {"groups":[{"id":1,"categories":[12,11]}]}')
  })

  it('reload the list after a change that fails, and throw', async () => {
    const { edits } = await setUp()
    sent = []
    await expect(edits.deleteCategory(7)).rejects.toBeInstanceOf(ApiError)
    expect(sent).toEqual(['DELETE /api/v1/categories/7', 'GET /api/v1/categories'])
  })
})
