import { mount } from '@vue/test-utils'
import { describe, expect, it } from 'vitest'
import type { Account } from '@/api/accounts'
import type { FeedAccount } from '@/api/connectionAccounts'
import NewAccountsPrompt from './NewAccountsPrompt.vue'

function feed(id: string, name: string, extra: Partial<FeedAccount> = {}): FeedAccount {
  return {
    external_account_id: id,
    name,
    institution: 'First Demo Bank',
    balances_only_suggested: false,
    same_as_account_id: null,
    ...extra,
  }
}

const wallet: Account = {
  id: 9,
  name: 'Fake wallet',
  institution: null,
  type: 'cash',
  role: 'budgeted',
  closed: false,
  in_net_worth: true,
  balances_only: false,
  connection_id: null,
  balance_entry: 'manual',
  sync: null,
  balance_minor: 100,
  app_balance_minor: 100,
  opening_balance_minor: 100,
  opening_date: '2026-10-27',
  rows: 0,
  last_transaction_date: null,
  drift: { sign_corrected: false, bank_comparison: null, flag: null, adjustments: [] },
}

const empty = { pending: [], ignored: [], refused: [] }

describe('NewAccountsPrompt', () => {
  it('shows nothing when there is nothing to decide', () => {
    const wrapper = mount(NewAccountsPrompt, { props: { prompt: empty, accounts: [] } })
    expect(wrapper.text()).toBe('')
  })

  it('counts the new ones, and tracks one with the role its button chose', async () => {
    const prompt = {
      ...empty,
      pending: [
        feed('ACT-1', 'Demo Checking'),
        feed('ACT-2', 'Demo Brokerage', { balances_only_suggested: true }),
      ],
    }
    const wrapper = mount(NewAccountsPrompt, { props: { prompt, accounts: [wallet] } })
    expect(wrapper.get('h2').text()).toBe('2 new accounts found')

    const brokerage = wrapper.get('li[aria-label="Demo Brokerage"]')
    const buttons = brokerage.findAll('button')
    await buttons.find((b) => b.text() === 'Track · tracking')?.trigger('click')
    const form = wrapper.get('form[aria-label="Track Demo Brokerage"]')
    expect((form.get('input[type="checkbox"]').element as HTMLInputElement).checked).toBe(true)
    await form.trigger('submit')
    expect(wrapper.emitted('track')?.[0]).toEqual([
      'ACT-2',
      { role: 'tracking', balances_only: true, same_as_account_id: null },
    ])
    expect(wrapper.find('form').exists()).toBe(false)

    const checking = wrapper.get('li[aria-label="Demo Checking"]')
    await checking
      .findAll('button')
      .find((b) => b.text() === 'Ignore')
      ?.trigger('click')
    expect(wrapper.emitted('ignore')?.[0]).toEqual(['ACT-1'])
  })

  it('pre-fills This is the same as…, and revives an ignored one', async () => {
    const prompt = {
      pending: [feed('ACT-3', 'Fake wallet', { same_as_account_id: 9 })],
      ignored: [feed('ACT-4', 'Demo Savings')],
      refused: [
        {
          external_account_id: 'ACT-5',
          name: 'Demo Travel Card',
          institution: null,
          currency: 'CAD',
          message: 'Demo Travel Card reports its money in CAD.',
        },
      ],
    }
    const wrapper = mount(NewAccountsPrompt, { props: { prompt, accounts: [wallet] } })
    expect(wrapper.get('[role="note"]').text()).toBe('Demo Travel Card reports its money in CAD.')

    await wrapper
      .get('li[aria-label="Fake wallet"]')
      .findAll('button')
      .find((b) => b.text() === 'Track · budgeted')
      ?.trigger('click')
    expect((wrapper.get('#track-same-ACT-3').element as HTMLSelectElement).value).toBe('9')
    await wrapper.get('form').trigger('submit')
    expect(wrapper.emitted('track')?.[0]).toEqual([
      'ACT-3',
      { role: 'budgeted', balances_only: false, same_as_account_id: 9 },
    ])

    await wrapper
      .get('li[aria-label="Demo Savings"]')
      .findAll('button')
      .find((b) => b.text() === 'Start tracking…')
      ?.trigger('click')
    await wrapper.get('#track-role-ACT-4').setValue('tracking')
    await wrapper.get('form').trigger('submit')
    expect(wrapper.emitted('track')?.[1]).toEqual([
      'ACT-4',
      { role: 'tracking', balances_only: false, same_as_account_id: null },
    ])
  })
})
