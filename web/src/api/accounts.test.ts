import { QueryClient, VueQueryPlugin } from '@tanstack/vue-query'
import { flushPromises, mount } from '@vue/test-utils'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { defineComponent } from 'vue'
import { useAccountEdits, useAccounts, type Account } from './accounts'
import { ApiError } from './client'

const account = (id: number): Account => ({
  id,
  name: `Account ${String(id)}`,
  institution: null,
  type: 'checking',
  role: 'budgeted',
  closed: false,
  in_net_worth: true,
  balances_only: false,
  connection_id: null,
  balance_entry: 'manual',
  sync: null,
  balance_minor: 0,
  app_balance_minor: 0,
  opening_balance_minor: 0,
  opening_date: '2026-01-01',
  rows: 0,
  last_transaction_date: null,
})

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
      if (request.method === 'GET') return Response.json({ accounts: [account(1), account(2)] })
      if (pathname.endsWith('/7')) {
        return Response.json(
          { error: { code: 'not_found', message: "There's no such account." } },
          { status: 404 },
        )
      }
      return new Response(null, { status: 204 })
    }),
  )
})

afterEach(() => {
  vi.unstubAllGlobals()
})

async function setUp() {
  let used:
    { list: ReturnType<typeof useAccounts>; edits: ReturnType<typeof useAccountEdits> } | undefined
  const Host = defineComponent({
    setup() {
      used = { list: useAccounts(), edits: useAccountEdits() }
      return () => null
    },
  })
  mount(Host, { global: { plugins: [[VueQueryPlugin, { queryClient: new QueryClient() }]] } })
  await flushPromises()
  if (used === undefined) throw new Error('the host never ran its setup')
  return used
}

describe('the account edits', () => {
  it('send a change and reload the accounts after it', async () => {
    const { edits } = await setUp()
    sent = []
    await edits.change(1, { name: 'Daily', closed: true })
    expect(sent).toEqual([
      'PATCH /api/v1/accounts/1 {"name":"Daily","closed":true}',
      'GET /api/v1/accounts',
    ])
  })

  it('show a new order at once and send it as ids', async () => {
    const { list, edits } = await setUp()
    sent = []
    const saving = edits.order([account(2), account(1)])
    expect(list.data.value?.accounts.map((a) => a.id)).toEqual([2, 1])
    await saving
    expect(sent[0]).toBe('PUT /api/v1/accounts/order {"accounts":[2,1]}')
  })

  it('create a manual account and record a balance, reloading the accounts after each', async () => {
    const { edits } = await setUp()
    sent = []
    await edits.create({ name: 'Wallet', type: 'cash', role: 'budgeted', balance_minor: 4200 })
    await edits.updateBalance(3, -150)
    expect(sent).toEqual([
      'POST /api/v1/accounts {"name":"Wallet","type":"cash","role":"budgeted","balance_minor":4200}',
      'GET /api/v1/accounts',
      'POST /api/v1/accounts/3/balance {"balance_minor":-150}',
      'GET /api/v1/accounts',
    ])
  })

  it('reload the accounts after a change that fails, and throw', async () => {
    const { edits } = await setUp()
    sent = []
    await expect(edits.change(7, { closed: true })).rejects.toBeInstanceOf(ApiError)
    expect(sent).toEqual(['PATCH /api/v1/accounts/7 {"closed":true}', 'GET /api/v1/accounts'])
  })
})
