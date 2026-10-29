import { QueryClient, VueQueryPlugin } from '@tanstack/vue-query'
import { flushPromises, mount } from '@vue/test-utils'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { defineComponent, h } from 'vue'
import { formatDate, formatMoney, localeWeekStart, useFormatting, weekdayName } from './format'

afterEach(() => {
  vi.unstubAllGlobals()
})

const PROFILE = { username: 'sam', display_name: 'Sam', display_locale: null, week_start: null }

/**
 * What a screen shows for `minor`, and the week start it uses, through useFormatting, with the
 * server answering `household` and `profile`.
 */
async function shown(minor: number, household: Response, profile: Response) {
  vi.stubGlobal(
    'fetch',
    vi.fn((request: Request) =>
      Promise.resolve(
        new URL(request.url).pathname === '/api/v1/profile' ? profile.clone() : household.clone(),
      ),
    ),
  )
  const Amount = defineComponent(() => {
    const formatting = useFormatting()
    return () =>
      h('span', `${formatMoney(minor, formatting.value)} ${String(formatting.value.weekStart)}`)
  })
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  const wrapper = mount(Amount, { global: { plugins: [[VueQueryPlugin, { queryClient }]] } })
  const before = wrapper.text()
  await flushPromises()
  return { before, after: wrapper.text() }
}

const euros = () => Response.json({ currency: 'EUR', timezone: 'Europe/Berlin' })

describe('useFormatting', () => {
  it('formats in the household currency, in the browser locale and its week, until the profile says otherwise', async () => {
    const { after } = await shown(123456, euros(), Response.json(PROFILE))
    expect(after).toBe('€1,234.56 0')
  })

  it("formats in the profile's locale, with its week", async () => {
    const { after } = await shown(
      123456,
      euros(),
      Response.json({ ...PROFILE, display_locale: 'de-DE' }),
    )
    expect(after).toBe('1.234,56\u00a0€ 1')
  })

  it("falls back to the browser's locale for one the browser can't use", async () => {
    const { after } = await shown(
      100,
      euros(),
      Response.json({ ...PROFILE, display_locale: 'en_US' }),
    )
    expect(after).toBe('€1.00 0')
  })

  it("starts weeks on the profile's day over the locale's", async () => {
    const { after } = await shown(
      100,
      euros(),
      Response.json({ ...PROFILE, display_locale: 'de-DE', week_start: 6 }),
    )
    expect(after).toBe('1,00\u00a0€ 6')
  })

  it("formats in USD and the browser locale until the household and profile are known, or if they can't be", async () => {
    const refused = Response.json(
      { error: { code: 'setup_required', message: 'x' } },
      { status: 403 },
    )
    const { before, after } = await shown(150, refused, refused)
    expect(before).toBe('$1.50 0')
    expect(after).toBe('$1.50 0')
  })
})

describe('localeWeekStart', () => {
  it("is the locale's first day of the week, 0 for Sunday", () => {
    expect(localeWeekStart('en-US')).toBe(0)
    expect(localeWeekStart('de-DE')).toBe(1)
    expect(localeWeekStart('ar-EG')).toBe(6)
  })

  it('reads the older getter where there is no method, and says Monday where neither is', () => {
    vi.stubGlobal('Intl', {
      Locale: class {
        weekInfo = { firstDay: 7 }
      },
    })
    expect(localeWeekStart('de-DE')).toBe(0)
    vi.stubGlobal('Intl', {
      Locale: class {
        language = 'de'
      },
    })
    expect(localeWeekStart('de-DE')).toBe(1)
  })

  it("is Monday for what isn't a locale", () => {
    expect(localeWeekStart('not a locale')).toBe(1)
  })
})

describe('weekdayName', () => {
  it('names the day, 0 for Sunday, in the locale', () => {
    expect(weekdayName(0, 'en')).toBe('Sunday')
    expect(weekdayName(1, 'en')).toBe('Monday')
    expect(weekdayName(6, 'en')).toBe('Saturday')
    expect(weekdayName(1, 'de-DE')).toBe('Montag')
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
    expect(formatDate('2026-03-01', { locale: 'de-DE' })).toBe('01.03.2026')
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
