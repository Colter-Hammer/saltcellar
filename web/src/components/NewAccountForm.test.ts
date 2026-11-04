import { mount } from '@vue/test-utils'
import { describe, expect, it } from 'vitest'
import BalanceForm from './BalanceForm.vue'
import NewAccountForm from './NewAccountForm.vue'

const formatting = { currency: 'USD', locale: 'en-US', weekStart: 0 }

describe('NewAccountForm', () => {
  it("creates a cash account from its name and what's in it today", async () => {
    const wrapper = mount(NewAccountForm, { props: { formatting } })
    await wrapper.get('#new-name').setValue('Wallet')
    await wrapper.get('#new-role').setValue('tracking')
    await wrapper.get('#new-balance').setValue('$1,042.50')
    await wrapper.get('form').trigger('submit')
    expect(wrapper.emitted('save')?.[0]?.[0]).toEqual({
      name: 'Wallet',
      type: 'cash',
      role: 'tracking',
      balance_minor: 104250,
    })
  })

  it("says so when the balance isn't an amount, and shows the server's name error", async () => {
    const wrapper = mount(NewAccountForm, {
      props: { formatting, nameError: "There's already an account called Wallet." },
    })
    await wrapper.get('#new-name').setValue('Wallet')
    await wrapper.get('#new-balance').setValue('a lot')
    await wrapper.get('form').trigger('submit')
    expect(wrapper.emitted('save')).toBeUndefined()
    expect(wrapper.text()).toContain('Enter an amount, like $1,234.56.')
    expect(wrapper.text()).toContain("There's already an account called Wallet.")
  })
})

describe('BalanceForm', () => {
  it('saves the balance in minor units, and refuses what isn’t an amount', async () => {
    const wrapper = mount(BalanceForm, { props: { formatting } })
    await wrapper.get('#balance').setValue('12.3.4')
    await wrapper.get('form').trigger('submit')
    expect(wrapper.emitted('save')).toBeUndefined()
    expect(wrapper.text()).toContain('Enter an amount')
    await wrapper.get('#balance').setValue('-20')
    await wrapper.get('form').trigger('submit')
    expect(wrapper.emitted('save')?.[0]).toEqual([-2000])
    expect(wrapper.text()).not.toContain('Enter an amount')
  })
})
