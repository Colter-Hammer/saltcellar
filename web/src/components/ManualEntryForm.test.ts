import { QueryClient, VueQueryPlugin } from '@tanstack/vue-query'
import { flushPromises, mount } from '@vue/test-utils'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { blankFields } from '@/lib/manual'
import ManualEntryForm from './ManualEntryForm.vue'
import UndoToast from './UndoToast.vue'

const formatting = { currency: 'USD', locale: 'en-US', weekStart: 0 }

const account = (id: number, name: string, extra: object = {}) => ({
  id,
  name,
  institution: null,
  type: 'cash',
  role: 'budgeted',
  closed: false,
  in_net_worth: true,
  balances_only: false,
  connection_id: null,
  balance_minor: 0,
  app_balance_minor: 0,
  opening_balance_minor: 0,
  opening_date: '2026-01-01',
  rows: 0,
  last_transaction_date: null,
  ...extra,
})

beforeEach(() => {
  vi.stubGlobal(
    'fetch',
    vi.fn((request: Request) => {
      const { pathname } = new URL(request.url)
      if (pathname === '/api/v1/accounts') {
        return Response.json({
          accounts: [
            account(1, 'Fake wallet'),
            account(2, 'Fake savings'),
            account(3, 'Fake fund', { balances_only: true }),
            account(4, 'Old card', { closed: true }),
          ],
        })
      }
      if (pathname === '/api/v1/payees') {
        return Response.json({ payees: [{ id: 1, name: 'Corner Market' }] })
      }
      return Response.json({
        groups: [
          {
            id: 1,
            name: 'Food',
            kind: 'expense',
            hidden: false,
            icon: null,
            categories: [{ id: 5, name: 'Groceries', hidden: false }],
          },
        ],
      })
    }),
  )
})

afterEach(() => {
  vi.unstubAllGlobals()
})

async function form(props: object = {}) {
  const wrapper = mount(ManualEntryForm, {
    props: {
      title: 'New transaction',
      action: 'Enter transaction',
      start: blankFields('2026-03-04', '1'),
      formatting,
      ...props,
    },
    global: {
      plugins: [[VueQueryPlugin, { queryClient: new QueryClient() }]],
    },
  })
  await flushPromises()
  return wrapper
}

describe('ManualEntryForm', () => {
  it('offers open accounts that hold transactions, and the payees', async () => {
    const wrapper = await form()
    const accounts = wrapper.findAll('#entry-account option').map((option) => option.text())
    expect(accounts).toEqual(['Choose an account…', 'Fake wallet', 'Fake savings'])
    expect(wrapper.findAll('#entry-payees option').map((o) => o.attributes('value'))).toEqual([
      'Corner Market',
    ])
  })

  it('needs a category, then enters money out', async () => {
    const wrapper = await form()
    await wrapper.get('#entry-amount').setValue('12.50')
    await wrapper.get('form').trigger('submit')
    expect(wrapper.emitted('save')).toBeUndefined()
    expect(wrapper.text()).toContain('Choose a category.')

    await wrapper.get('select[aria-label="Category"]').setValue('5')
    await wrapper.get('#entry-payee').setValue('Fake Bakery')
    await wrapper.get('#entry-tags').setValue('trip')
    await wrapper.get('form').trigger('submit')
    expect(wrapper.emitted('save')?.[0]?.[0]).toEqual({
      account_id: 1,
      date: '2026-03-04',
      amount_minor: -1250,
      payee: 'Fake Bakery',
      category_id: 5,
      transfer_account_id: null,
      memo: null,
      tags: ['trip'],
    })
  })

  it('transfers to another account instead of a category, balances-only ones included', async () => {
    const wrapper = await form()
    await wrapper.get('#entry-amount').setValue('40')
    await wrapper.get('input[value="transfer"]').setValue(true)
    const others = wrapper.findAll('select[aria-label="Transfer to"] option').map((o) => o.text())
    expect(others).toEqual(['To which account?', 'Fake savings', 'Fake fund'])
    await wrapper.get('select[aria-label="Transfer to"]').setValue('2')
    await wrapper.get('form').trigger('submit')
    expect(wrapper.emitted('save')?.[0]?.[0]).toMatchObject({
      amount_minor: -4000,
      category_id: null,
      transfer_account_id: 2,
    })
  })

  it("shows the server's errors by field", async () => {
    const wrapper = await form({ refused: { category_id: "There's no such category." } })
    expect(wrapper.text()).toContain("There's no such category.")
  })
})

describe('UndoToast', () => {
  it('offers Undo, and goes by itself after a while', async () => {
    vi.useFakeTimers()
    const wrapper = mount(UndoToast, { props: { message: 'Transaction deleted.', seconds: 2 } })
    await wrapper.get('button').trigger('click')
    expect(wrapper.emitted('undo')).toHaveLength(1)
    vi.advanceTimersByTime(2000)
    expect(wrapper.emitted('dismiss')).toHaveLength(1)
    vi.useRealTimers()
  })
})
