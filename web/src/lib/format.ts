// Every number and date the UI shows goes through here (docs/spec/foundations.md#frontend).
import { computed, type ComputedRef } from 'vue'
import { useHousehold } from '@/api/household'
import { useProfile } from '@/api/profile'

export interface Formatting {
  /** The household currency, ISO 4217. */
  currency: string
  /** The login's locale, BCP 47. */
  locale: string
  /** The day the login's weeks start on: 0 is Sunday, 6 is Saturday. */
  weekStart: number
}

// What the pure functions below use when not told otherwise.
const DEFAULT: Formatting = { currency: 'USD', locale: 'en-US', weekStart: 0 }

/**
 * How a screen formats: the household currency, in the login's locale (the browser's unless the
 * profile chose one), with weeks starting on the profile's day or the locale's. USD in the
 * browser's locale until both have loaded.
 */
export function useFormatting(): ComputedRef<Formatting> {
  const household = useHousehold()
  const profile = useProfile()
  return computed(() => {
    const locale = usable(profile.data.value?.display_locale) ?? navigator.language
    return {
      currency: household.data.value?.currency ?? DEFAULT.currency,
      locale,
      weekStart: profile.data.value?.week_start ?? localeWeekStart(locale),
    }
  })
}

/** `locale`, if the browser can format with it. */
function usable(locale: string | null | undefined): string | undefined {
  if (locale == null) return undefined
  try {
    return Intl.getCanonicalLocales(locale)[0]
  } catch {
    return undefined
  }
}

/** Week info, which browsers offer as a method or, in older ones, a getter. */
type WithWeekInfo = Intl.Locale & {
  getWeekInfo?: () => { firstDay: number }
  weekInfo?: { firstDay: number }
}

/**
 * The day `locale`'s weeks start on, 0 (Sunday) to 6 (Saturday). Monday where the browser can't
 * say *(agent)*.
 */
export function localeWeekStart(locale: string): number {
  let info: WithWeekInfo
  try {
    info = new Intl.Locale(locale)
  } catch {
    return 1
  }
  // Intl counts 1 (Monday) to 7 (Sunday).
  const firstDay = info.getWeekInfo?.().firstDay ?? info.weekInfo?.firstDay ?? 1
  return firstDay % 7
}

/** The name of day `day` (0 is Sunday) in `locale`, like `Monday`. */
export function weekdayName(day: number, locale: string): string {
  // 2026-03-01 is a Sunday.
  const instant = new Date(Date.UTC(2026, 2, 1 + day))
  return new Intl.DateTimeFormat(locale, { weekday: 'long', timeZone: 'UTC' }).format(instant)
}

/** An amount in integer minor units, in the currency's decimal places and the locale's style. */
export function formatMoney(
  minor: number,
  formatting: Pick<Formatting, 'currency' | 'locale'> = DEFAULT,
): string {
  const format = new Intl.NumberFormat(formatting.locale, {
    style: 'currency',
    currency: formatting.currency,
  })
  const places = format.resolvedOptions().maximumFractionDigits ?? 0
  // A decimal string, never a float, so large amounts keep their last digit.
  const digits = Math.abs(minor)
    .toString()
    .padStart(places + 1, '0')
  const whole = digits.slice(0, digits.length - places)
  const fraction = digits.slice(digits.length - places)
  const decimal = `${minor < 0 ? '-' : ''}${whole}${places ? `.${fraction}` : ''}`
  return format.format(decimal as Intl.StringNumericLiteral)
}

const PLAIN_DATE = /^(\d{4})-(\d{2})-(\d{2})$/

/** A plain `YYYY-MM-DD` date, in the locale's medium style. */
export function formatDate(date: string, formatting: Pick<Formatting, 'locale'> = DEFAULT): string {
  const [, year, month, day] = PLAIN_DATE.exec(date) ?? []
  // Formatted as a UTC instant in UTC, so the browser's timezone can't move it a day.
  const instant = new Date(Date.UTC(Number(year), Number(month) - 1, Number(day)))
  if (
    year === undefined ||
    instant.getUTCFullYear() !== Number(year) ||
    instant.getUTCMonth() !== Number(month) - 1 ||
    instant.getUTCDate() !== Number(day)
  ) {
    throw new RangeError(`not a YYYY-MM-DD date: ${date}`)
  }
  return new Intl.DateTimeFormat(formatting.locale, {
    dateStyle: 'medium',
    timeZone: 'UTC',
  }).format(instant)
}
