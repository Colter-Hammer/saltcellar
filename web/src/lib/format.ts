// Every number and date the UI shows goes through here (docs/spec/foundations.md#frontend).

export interface Formatting {
  /** The household currency, ISO 4217. */
  currency: string
  /** The login's locale, BCP 47. */
  locale: string
}

// Until setup and the profile exist (issues #8 and #10), every screen formats in USD and en-US.
const DEFAULT: Formatting = { currency: 'USD', locale: 'en-US' }

/** An amount in integer minor units, in the currency's decimal places and the locale's style. */
export function formatMoney(minor: number, formatting: Formatting = DEFAULT): string {
  const format = new Intl.NumberFormat(formatting.locale, {
    style: 'currency',
    currency: formatting.currency,
  })
  const places = format.resolvedOptions().maximumFractionDigits ?? 0
  // A decimal string, never a float, so large amounts keep their last digit.
  const digits = Math.abs(minor).toString().padStart(places + 1, '0')
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
  return new Intl.DateTimeFormat(formatting.locale, { dateStyle: 'medium', timeZone: 'UTC' }).format(
    instant,
  )
}
