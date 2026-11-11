import { QueryClient, VueQueryPlugin } from '@tanstack/vue-query'
import { flushPromises, mount } from '@vue/test-utils'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { defineComponent } from 'vue'
import { ApiError } from './client'
import { type ManualEntry, useManualEdits, usePayees } from './transactions'

/** Every request sent, as `METHOD path body`. */
let sent: string[]

beforeEach(() => {
  sent = []
  vi.stubGlobal(
    'fetch',
    vi.fn(async (request: Request) => {
      const { pathname } = new URL(request.url)
      const body = await request.text()
      if (request.method === 'DELETE') {
        expect(request.headers.get('Content-Type')).toBe('application/json')
      }
      sent.push(`${request.method} ${pathname}${body ? ` ${body}` : ''}`)
      if (pathname === '/api/v1/payees') {
        return Response.json({ payees: [{ id: 1, name: 'Corner Market' }] })
      }
      if (pathname.endsWith('/7')) {
        return Response.json(
          { error: { code: 'not_manual', message: 'Only a manual transaction…' } },
          { status: 409 },
        )
      }
      if (request.method === 'DELETE') return new Response(null, { status: 204 })
      return Response.json({ id: 3 })
    }),
  )
})

afterEach(() => {
  vi.unstubAllGlobals()
})

async function setUp() {
  let used:
    { payees: ReturnType<typeof usePayees>; edits: ReturnType<typeof useManualEdits> } | undefined
  const Host = defineComponent({
    setup() {
      used = { payees: usePayees(), edits: useManualEdits() }
      return () => null
    },
  })
  mount(Host, { global: { plugins: [[VueQueryPlugin, { queryClient: new QueryClient() }]] } })
  await flushPromises()
  if (used === undefined) throw new Error('the host never ran its setup')
  return used
}

const entry: ManualEntry = {
  account_id: 1,
  date: '2026-03-04',
  amount_minor: -1250,
  category_id: 5,
}

describe('manual entry', () => {
  it('lists the payees', async () => {
    const { payees } = await setUp()
    expect(payees.data.value).toEqual([{ id: 1, name: 'Corner Market' }])
  })

  it('enters, changes and deletes, reloading the payees after each', async () => {
    const { edits } = await setUp()
    sent = []
    await edits.create(entry)
    await edits.change(3, entry)
    await edits.remove(3)
    const body = JSON.stringify(entry)
    expect(sent).toEqual([
      `POST /api/v1/transactions ${body}`,
      'GET /api/v1/payees',
      `PUT /api/v1/transactions/3 ${body}`,
      'GET /api/v1/payees',
      'DELETE /api/v1/transactions/3',
      'GET /api/v1/payees',
    ])
  })

  it('reloads after a refusal too, and throws', async () => {
    const { edits } = await setUp()
    sent = []
    await expect(edits.remove(7)).rejects.toBeInstanceOf(ApiError)
    expect(sent).toEqual(['DELETE /api/v1/transactions/7', 'GET /api/v1/payees'])
  })
})
