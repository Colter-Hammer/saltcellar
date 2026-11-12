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

/** The minus sign amounts show, U+2212, never a hyphen (docs/spec/visual-system.md#money). */
const MINUS = '\u2212'

/**
 * An amount in integer minor units, in the currency's decimal places and the locale's style, with
 * a real minus sign and never in parentheses. `signed` marks money in with a plus, as a
 * transaction's amount shows *(agent)*. `whole` rounds to whole units, half away from zero, for
 * headline figures.
 */
export function formatMoney(
  minor: number,
  formatting: Pick<Formatting, 'currency' | 'locale'> = DEFAULT,
  { signed = false, whole = false }: { signed?: boolean; whole?: boolean } = {},
): string {
  const options: Intl.NumberFormatOptions = {
    style: 'currency',
    currency: formatting.currency,
    // `standard`, never `accounting`: no locale's standard style uses parentheses. `negative`
    // so an amount that rounds to zero shows no sign.
    currencySign: 'standard',
    signDisplay: signed ? 'exceptZero' : 'negative',
  }
  const places =
    new Intl.NumberFormat(formatting.locale, options).resolvedOptions().maximumFractionDigits ?? 0
  const format = new Intl.NumberFormat(formatting.locale, {
    ...options,
    ...(whole && {
      minimumFractionDigits: 0,
      maximumFractionDigits: 0,
      roundingMode: 'halfExpand',
    }),
  })
  // A decimal string, never a float, so large amounts keep their last digit.
  const digits = Math.abs(minor)
    .toString()
    .padStart(places + 1, '0')
  const units = digits.slice(0, digits.length - places)
  const fraction = digits.slice(digits.length - places)
  const decimal = `${minor < 0 ? '-' : ''}${units}${places ? `.${fraction}` : ''}`
  return format
    .formatToParts(decimal as Intl.StringNumericLiteral)
    .map((part) => (part.type === 'minusSign' ? MINUS : part.value))
    .join('')
}

/**
 * An amount as typed, in integer minor units, or `null` if it isn't one: digits with the locale's
 * decimal mark and at most the currency's places, a leading minus, and the locale's group marks
 * only between groups of three, so "12.50" isn't read as twelve hundred in German *(agent)*.
 * Spaces and the currency's symbol are ignored.
 */
export function parseMoney(
  text: string,
  formatting: Pick<Formatting, 'currency' | 'locale'> = DEFAULT,
): number | null {
  const parts = new Intl.NumberFormat(formatting.locale, {
    style: 'currency',
    currency: formatting.currency,
  }).formatToParts(-12345.6)
  const mark = (type: Intl.NumberFormatPartTypes) =>
    parts.find((part) => part.type === type)?.value ?? ''
  const places = parts.find((part) => part.type === 'fraction')?.value.length ?? 0
  const [decimal, group, symbol] = [mark('decimal'), mark('group'), mark('currency')]
  let typed = text.replace(/[\s\u00a0\u202f]/g, '').replace(/^[-\u2212]/, '-')
  if (symbol) typed = typed.replace(symbol, '')
  const escape = (literal: string) => literal.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')
  // Group marks that are spaces were stripped with the rest.
  const grouped = /\s/.test(group) || group === '' ? '' : `|\\d{1,3}(?:${escape(group)}\\d{3})+`
  const fraction = places > 0 ? `(?:${escape(decimal)}(\\d{1,${String(places)}}))?` : ''
  const match = new RegExp(`^(-?)(\\d+${grouped})${fraction}$`).exec(typed)
  if (!match) return null
  const [, sign = '', units = '', cents = ''] = match
  const digits = (group ? units.replaceAll(group, '') : units) + cents.padEnd(places, '0')
  const minor = Number(`${sign}${digits}`)
  return Number.isSafeInteger(minor) ? (Object.is(minor, -0) ? 0 : minor) : null
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

/** The day of an RFC 3339 instant in `timeZone` (an IANA name), in the locale's medium style. */
export function formatInstantDay(
  instant: string,
  timeZone: string,
  formatting: Pick<Formatting, 'locale'> = DEFAULT,
): string {
  return new Intl.DateTimeFormat(formatting.locale, { dateStyle: 'medium', timeZone }).format(
    new Date(instant),
  )
}

/** The time of day of an RFC 3339 instant in `timeZone` (an IANA name), like `3:40 PM`. */
export function formatInstantTime(
  instant: string,
  timeZone: string,
  formatting: Pick<Formatting, 'locale'> = DEFAULT,
): string {
  return new Intl.DateTimeFormat(formatting.locale, { timeStyle: 'short', timeZone }).format(
    new Date(instant),
  )
}

/** The day and time of an RFC 3339 instant in `timeZone`, like `Oct 28, 2026, 3:40 PM`. */
export function formatInstant(
  instant: string,
  timeZone: string,
  formatting: Pick<Formatting, 'locale'> = DEFAULT,
): string {
  return new Intl.DateTimeFormat(formatting.locale, {
    dateStyle: 'medium',
    timeStyle: 'short',
    timeZone,
  }).format(new Date(instant))
}

/** An `HH:MM` time of day rounded to the nearest hour, like `6 AM` for `06:13`. */
export function formatAroundHour(
  time: string,
  formatting: Pick<Formatting, 'locale'> = DEFAULT,
): string {
  const [hours, minutes] = time.split(':').map(Number)
  if (hours === undefined || minutes === undefined || Number.isNaN(hours + minutes)) {
    throw new RangeError(`not an HH:MM time: ${time}`)
  }
  const hour = (hours + (minutes >= 30 ? 1 : 0)) % 24
  return new Intl.DateTimeFormat(formatting.locale, { hour: 'numeric', timeZone: 'UTC' }).format(
    new Date(Date.UTC(2026, 0, 1, hour)),
  )
}
