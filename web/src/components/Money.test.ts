import { QueryClient, VueQueryPlugin } from '@tanstack/vue-query'
import { flushPromises, mount } from '@vue/test-utils'
import { afterEach, describe, expect, it, vi } from 'vitest'
import Money from './Money.vue'

afterEach(() => {
  vi.unstubAllGlobals()
})

/** `Money` with `props`, once the household (USD) and an en-US profile have loaded. */
async function money(props: InstanceType<typeof Money>['$props']) {
  vi.stubGlobal(
    'fetch',
    vi.fn((request: Request) =>
      Promise.resolve(
        new URL(request.url).pathname === '/api/v1/profile'
          ? Response.json({
              username: 'sam',
              display_name: 'Sam',
              display_locale: 'en-US',
              week_start: null,
            })
          : Response.json({ currency: 'USD', timezone: 'America/Chicago' }),
      ),
    ),
  )
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  const wrapper = mount(Money, {
    props,
    global: { plugins: [[VueQueryPlugin, { queryClient }]] },
  })
  await flushPromises()
  return wrapper
}

describe('Money', () => {
  it('shows money out with a real minus in plain foreground, in tabular figures', async () => {
    const wrapper = await money({ minor: -4210, signed: true })
    expect(wrapper.text()).toBe('−$42.10')
    expect(wrapper.classes()).toEqual(
      expect.arrayContaining(['tabular-nums', 'font-medium']) as string[],
    )
    expect(wrapper.classes()).not.toContain('text-money-in')
  })

  it('marks money in with a plus and the money-in color', async () => {
    const wrapper = await money({ minor: 120000, signed: true })
    expect(wrapper.text()).toBe('+$1,200.00')
    expect(wrapper.classes()).toContain('text-money-in')
  })

  it('gives a balance no plus and no color', async () => {
    const wrapper = await money({ minor: 120000 })
    expect(wrapper.text()).toBe('$1,200.00')
    expect(wrapper.classes()).not.toContain('text-money-in')
  })

  it('rounds a headline figure, with the exact amount on hover and on a tap', async () => {
    const wrapper = await money({ minor: 123450, whole: true })
    expect(wrapper.text()).toBe('$1,235')
    expect(wrapper.attributes('title')).toBe('$1,234.50')
    await wrapper.trigger('click')
    expect(wrapper.text()).toBe('$1,234.50')
    await wrapper.trigger('click')
    expect(wrapper.text()).toBe('$1,235')
  })
})
