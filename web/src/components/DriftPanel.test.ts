import { QueryClient, VueQueryPlugin } from '@tanstack/vue-query'
import { flushPromises, mount, RouterLinkStub } from '@vue/test-utils'
import { afterEach, describe, expect, it, vi } from 'vitest'
import type { Account } from '@/api/accounts'
import DriftPanel from './DriftPanel.vue'

afterEach(() => {
  vi.unstubAllGlobals()
})

const base: Account = {
  id: 4,
  name: 'Demo Everyday Checking',
  institution: 'First Demo Bank',
  type: 'checking',
  role: 'budgeted',
  closed: false,
  in_net_worth: true,
  balances_only: false,
  connection_id: 1,
  balance_entry: 'bank',
  sync: null,
  balance_minor: 110_308,
  app_balance_minor: 111_258,
  opening_balance_minor: 10_800,
  opening_date: '2026-07-31',
  rows: 5,
  last_transaction_date: '2026-11-12',
  drift: {
    sign_corrected: false,
    bank_comparison: {
      as_of: '2026-11-13',
      bank_minor: 110_308,
      app_minor: 111_258,
      drift_minor: -950,
    },
    flag: {
      drift_minor: -950,
      since: '2026-11-12',
      suspect_rows: [
        {
          transaction_id: 31,
          change: 'added',
          date: '2026-11-12',
          amount_minor: -4_500,
          description: 'DEMO HARDWARE',
        },
        {
          transaction_id: null,
          change: 'dropped',
          date: '2026-11-11',
          amount_minor: -2_000,
          description: 'DEMO HOTEL DEPOSIT',
        },
      ],
    },
    adjustments: [],
  },
}

/** The panel for `account`, with the fake server answering `answer` to every request, which
 * `calls` collects. */
async function panel(account: Account, answer: () => Response = () => Response.json(account)) {
  const calls: { path: string; body: unknown; json: boolean }[] = []
  vi.stubGlobal(
    'fetch',
    vi.fn(async (request: Request) => {
      const path = new URL(request.url).pathname
      if (path === '/api/v1/profile')
        return Response.json({
          username: 'sam',
          display_name: 'Sam',
          display_locale: 'en-US',
          week_start: null,
        })
      if (path === '/api/v1/household')
        return Response.json({ currency: 'USD', timezone: 'America/Chicago' })
      const text = request.method === 'POST' ? await request.text() : ''
      calls.push({
        path,
        body: text ? JSON.parse(text) : null,
        json: request.headers.get('Content-Type') === 'application/json',
      })
      return answer()
    }),
  )
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  const wrapper = mount(DriftPanel, {
    props: { account },
    global: {
      plugins: [[VueQueryPlugin, { queryClient }]],
      stubs: { RouterLink: RouterLinkStub },
    },
  })
  await flushPromises()
  return { wrapper, calls }
}

describe('DriftPanel', () => {
  it('shows nothing without a flag or adjustments', async () => {
    const { wrapper } = await panel({ ...base, drift: { ...base.drift, flag: null } })
    expect(wrapper.text()).toBe('')
  })

  it('says how far off and since when, bank against app, and lists the suspect rows', async () => {
    const { wrapper } = await panel(base)
    const flag = wrapper.get('[data-testid="drift-flag"]')
    expect(flag.get('h3').text()).toBe('Balance off by −$9.50 since Nov 12, 2026')
    expect(flag.text()).toContain('Bank says$1,103.08')
    expect(flag.text()).toContain('App says$1,112.58')
    expect(wrapper.find('[data-testid="suspect-rows"]').exists()).toBe(false)

    await wrapper
      .findAll('button')
      .find((b) => b.text() === 'See suspect rows')
      ?.trigger('click')
    const rows = wrapper.get('[data-testid="suspect-rows"]').findAll('li')
    expect(rows.map((row) => row.text())).toEqual([
      'AddedDEMO HARDWARENov 12, 2026−$45.00',
      'DroppedDEMO HOTEL DEPOSITNov 11, 2026−$20.00',
    ])
    // A row still there links to it; a dropped one can't.
    expect(rows[0]?.getComponent(RouterLinkStub).props('to')).toBe('/transactions/31')
    expect(rows[1]?.findComponent(RouterLinkStub).exists()).toBe(false)
  })

  it('accepts the flagged drift with a note', async () => {
    const { wrapper, calls } = await panel(base)
    await wrapper
      .findAll('button')
      .find((b) => b.text() === 'Accept −$9.50 as balance adjustment')
      ?.trigger('click')
    await wrapper.get('input').setValue('Monthly fee')
    await wrapper.get('form').trigger('submit')
    await flushPromises()
    expect(calls).toEqual([
      {
        path: '/api/v1/accounts/4/balance-adjustments',
        body: { amount_minor: -950, note: 'Monthly fee' },
        json: true,
      },
    ])
    expect(wrapper.find('form').exists()).toBe(false)
  })

  it('says why an accept was refused', async () => {
    const { wrapper } = await panel(base, () =>
      Response.json(
        {
          error: {
            code: 'no_drift',
            message: "The balance isn't off by that amount any more. Reload and look again.",
          },
        },
        { status: 409 },
      ),
    )
    await wrapper
      .findAll('button')
      .find((b) => b.text().startsWith('Accept'))
      ?.trigger('click')
    await wrapper.get('form').trigger('submit')
    await flushPromises()
    expect(wrapper.get('[role="alert"]').text()).toBe(
      "The balance isn't off by that amount any more. Reload and look again.",
    )
  })

  it('lists adjustments, undone ones too, with Undo on those that count', async () => {
    const account: Account = {
      ...base,
      drift: {
        ...base.drift,
        flag: null,
        adjustments: [
          {
            id: 8,
            date: '2026-11-13',
            amount_minor: -950,
            note: 'Monthly fee',
            accepted_by: 'Sam',
            accepted_at: '2026-11-13T15:00:00Z',
            undone_at: null,
          },
          {
            id: 7,
            date: '2026-11-13',
            amount_minor: -950,
            note: null,
            accepted_by: 'Sam',
            accepted_at: '2026-11-13T14:00:00Z',
            undone_at: '2026-11-13T14:30:00Z',
          },
        ],
      },
    }
    const { wrapper, calls } = await panel(account)
    const items = wrapper.findAll('[data-testid="adjustment"]')
    expect(items.map((item) => item.text())).toEqual([
      '−$9.50 on Nov 13, 2026Monthly fee · Sam Undo',
      '−$9.50 on Nov 13, 2026No note · Sam · Undone',
    ])
    await items[0]?.get('button').trigger('click')
    await flushPromises()
    // Every mutating request is JSON, body or not: the server refuses one that isn't.
    expect(calls).toEqual([{ path: '/api/v1/balance-adjustments/8/undo', body: null, json: true }])
  })
})
