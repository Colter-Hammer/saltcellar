import { QueryClient, VueQueryPlugin } from '@tanstack/vue-query'
import { flushPromises, mount } from '@vue/test-utils'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { defineComponent, h } from 'vue'
import { formatDate, formatMoney, useFormatting } from './format'

afterEach(() => {
  vi.unstubAllGlobals()
})

/** What a screen shows for `minor` through useFormatting, with the server answering `household`. */
async function shown(minor: number, household: Response) {
  vi.stubGlobal(
    'fetch',
    vi.fn(() => Promise.resolve(household)),
  )
  const Amount = defineComponent(() => {
    const formatting = useFormatting()
    return () => h('span', formatMoney(minor, formatting.value))
  })
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  const wrapper = mount(Amount, { global: { plugins: [[VueQueryPlugin, { queryClient }]] } })
  const before = wrapper.text()
  await flushPromises()
  return { before, after: wrapper.text() }
}

describe('useFormatting', () => {
  it('formats in the household currency, in the browser locale', async () => {
    const { after } = await shown(
      123456,
      Response.json({ currency: 'EUR', timezone: 'Europe/Berlin' }),
    )
    expect(after).toBe('€1,234.56')
  })

  it("formats in USD until the household is known, or if it can't be", async () => {
    const { before, after } = await shown(
      150,
      Response.json({ error: { code: 'setup_required', message: 'x' } }, { status: 403 }),
    )
    expect(before).toBe('$1.50')
    expect(after).toBe('$1.50')
  })
})

describe('formatMoney', () => {
  it('turns minor units into the currency with its own decimal places', () => {
    expect(formatMoney(123456, { currency: 'USD', locale: 'en-US' })).toBe('$1,234.56')
  })

  it('shows money leaving as negative', () => {
    expect(formatMoney(-5, { currency: 'USD', locale: 'en-US' })).toBe('-$0.05')
  })

  it('knows currencies without minor units', () => {
    expect(formatMoney(1500, { currency: 'JPY', locale: 'en-US' })).toBe('¥1,500')
  })

  it('knows currencies with three decimal places', () => {
    expect(formatMoney(1234, { currency: 'KWD', locale: 'en-US' })).toBe('KWD\u00a01.234')
  })

  it('uses the locale for separators and symbol placement', () => {
    expect(formatMoney(123456, { currency: 'EUR', locale: 'de-DE' })).toBe('1.234,56\u00a0€')
  })

  it('is exact past the point where floats lose cents', () => {
    expect(formatMoney(900719925474099, { currency: 'USD', locale: 'en-US' })).toBe(
      '$9,007,199,254,740.99',
    )
  })

  it('defaults to USD in en-US until setup chooses a currency', () => {
    expect(formatMoney(100)).toBe('$1.00')
  })
})

describe('formatDate', () => {
  it('formats a plain date in the locale', () => {
    expect(formatDate('2026-03-01', { locale: 'en-US' })).toBe('Mar 1, 2026')
    expect(formatDate('2026-03-01', { locale: 'en-GB' })).toBe('1 Mar 2026')
  })

  it("doesn't shift the day with the browser's timezone", () => {
    expect(formatDate('2026-01-01', { locale: 'en-US' })).toBe('Jan 1, 2026')
    expect(formatDate('2026-12-31', { locale: 'en-US' })).toBe('Dec 31, 2026')
  })

  it('rejects anything but YYYY-MM-DD', () => {
    expect(() => formatDate('2026-3-1')).toThrow('2026-3-1')
    expect(() => formatDate('2026-02-30')).toThrow('2026-02-30')
  })
})
