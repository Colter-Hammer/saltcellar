import { mount } from '@vue/test-utils'
import { describe, expect, it } from 'vitest'
import type { Account } from '@/api/accounts'
import AccountEditForm from './AccountEditForm.vue'

function account(rows: number): Account {
  return {
    id: 1,
    name: 'Everyday',
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
    rows,
    last_transaction_date: null,
    drift: { sign_corrected: false, bank_comparison: null, flag: null, adjustments: [] },
  }
}

describe('AccountEditForm', () => {
  it('saves at once when role and balances-only stay', async () => {
    const wrapper = mount(AccountEditForm, { props: { account: account(3) } })
    await wrapper.get('#name').setValue('Daily')
    await wrapper.get('form').trigger('submit')
    expect(wrapper.emitted('save')?.[0]?.[0]).toMatchObject({ name: 'Daily', role: 'budgeted' })
  })

  it('asks first when the role changes on an account with rows, and Cancel changes nothing', async () => {
    const wrapper = mount(AccountEditForm, { props: { account: account(3) } })
    await wrapper.get('#role').setValue('tracking')
    await wrapper.get('form').trigger('submit')
    expect(wrapper.text()).toContain('This changes what counts toward budgets.')
    expect(wrapper.emitted('save')).toBeUndefined()
    await wrapper
      .findAll('button')
      .find((b) => b.text() === 'Cancel')
      ?.trigger('click')
    expect(wrapper.emitted('save')).toBeUndefined()
    await wrapper.get('form').trigger('submit')
    await wrapper
      .findAll('button')
      .find((b) => b.text() === 'Change it')
      ?.trigger('click')
    expect(wrapper.emitted('save')?.[0]?.[0]).toMatchObject({ role: 'tracking' })
  })

  it('does not ask on an account without rows', async () => {
    const wrapper = mount(AccountEditForm, { props: { account: account(0) } })
    await wrapper.get('input[type="checkbox"]').setValue(true)
    await wrapper.get('form').trigger('submit')
    expect(wrapper.emitted('save')).toHaveLength(1)
  })
})
