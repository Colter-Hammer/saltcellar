// Manual entry (docs/spec/m2-bank-data.md#manual-entry): the entry form's fields, read into what
// the API takes, and a transaction read back into them, for editing and for Undo after a delete.
// Nothing here talks to the server.
import type { ManualEntry, Transaction } from '@/api/transactions'
import type { Formatting } from '@/lib/format'
import { formatMoney, parseMoney } from '@/lib/format'

/** The entry form's fields as typed. Ids are text, empty while none is chosen. */
export interface EntryFields {
  accountId: string
  /** `YYYY-MM-DD`. */
  date: string
  /** Money out unless set to money in *(agent)*. */
  direction: 'out' | 'in'
  /** Without a sign: `direction` gives it. */
  amount: string
  payee: string
  /** A category, or **Transfer to…** another account instead. */
  lands: 'category' | 'transfer'
  categoryId: string
  transferAccountId: string
  memo: string
  /** Separated by commas or spaces, `#` optional *(agent)*. */
  tags: string
}

/** A blank form: money out, dated `today`, in `accountId` if given. */
export function blankFields(today: string, accountId = ''): EntryFields {
  return {
    accountId,
    date: today,
    direction: 'out',
    amount: '',
    payee: '',
    lands: 'category',
    categoryId: '',
    transferAccountId: '',
    memo: '',
    tags: '',
  }
}

/** Today in `timeZone`, as `YYYY-MM-DD`. */
export function todayIn(timeZone: string, now: Date = new Date()): string {
  return new Intl.DateTimeFormat('en-CA', { timeZone }).format(now)
}

/**
 * `minor`'s size as typed in the locale's way, with no sign, symbol or group marks, so
 * `parseMoney` reads it back: 1250 is "12.50" in English, "12,50" in German.
 */
export function amountText(minor: number, formatting: Pick<Formatting, 'currency' | 'locale'>) {
  const format = new Intl.NumberFormat(formatting.locale, {
    style: 'currency',
    currency: formatting.currency,
    useGrouping: false,
  })
  const places = format.resolvedOptions().maximumFractionDigits ?? 0
  const digits = Math.abs(minor)
    .toString()
    .padStart(places + 1, '0')
  const units = digits.slice(0, digits.length - places)
  const decimal = `${units}${places ? `.${digits.slice(digits.length - places)}` : ''}`
  return format
    .formatToParts(decimal as Intl.StringNumericLiteral)
    .filter((part) => ['integer', 'decimal', 'fraction'].includes(part.type))
    .map((part) => part.value)
    .join('')
}

/** The tags as typed, split on commas and spaces, blanks dropped. */
export function splitTags(text: string): string[] {
  return text.split(/[\s,]+/).filter((tag) => tag !== '')
}

/** The form's fields for transaction `row`, to edit it. */
export function fieldsOf(
  row: Transaction,
  formatting: Pick<Formatting, 'currency' | 'locale'>,
): EntryFields {
  const transfer = row.transfer_account_id
  return {
    accountId: String(row.account_id),
    date: row.date,
    direction: row.amount_minor < 0 ? 'out' : 'in',
    amount: amountText(row.amount_minor, formatting),
    payee: row.payee ?? '',
    lands: transfer === null ? 'category' : 'transfer',
    categoryId: row.category_id === null ? '' : String(row.category_id),
    transferAccountId: transfer === null ? '' : String(transfer),
    memo: row.memo ?? '',
    tags: row.tags.join(' '),
  }
}

/** Transaction `row` as the API enters it, to enter it again after a delete (Undo). */
export function entryOf(row: Transaction): ManualEntry {
  return {
    account_id: row.account_id,
    date: row.date,
    amount_minor: row.amount_minor,
    payee: row.payee,
    category_id: row.transfer_account_id === null ? row.category_id : null,
    transfer_account_id: row.transfer_account_id,
    memo: row.memo,
    tags: row.tags,
  }
}

export type EntryErrors = Partial<Record<keyof ManualEntry, string>>

/**
 * The form read into what the API takes, or what's missing, named by the API's field names so the
 * server's own errors land in the same places.
 */
export function readFields(
  fields: EntryFields,
  formatting: Pick<Formatting, 'currency' | 'locale'>,
): { entry: ManualEntry } | { errors: EntryErrors } {
  const errors: EntryErrors = {}
  if (fields.accountId === '') errors.account_id = 'Choose an account.'
  if (fields.date === '') errors.date = 'Enter a date.'
  const minor = parseMoney(fields.amount, formatting)
  if (minor === null) {
    errors.amount_minor = `Enter an amount, like ${formatMoney(1250, formatting)}.`
  } else if (minor === 0) {
    errors.amount_minor = 'Enter an amount other than zero.'
  }
  const transfer = fields.lands === 'transfer'
  if (!transfer && fields.categoryId === '') errors.category_id = 'Choose a category.'
  if (transfer && fields.transferAccountId === '') {
    errors.transfer_account_id = 'Choose an account.'
  } else if (transfer && fields.transferAccountId === fields.accountId) {
    errors.transfer_account_id = 'Choose an account other than this one.'
  }
  if (Object.keys(errors).length > 0 || minor === null) return { errors }
  const size = Math.abs(minor)
  const payee = fields.payee.trim()
  const memo = fields.memo.trim()
  return {
    entry: {
      account_id: Number(fields.accountId),
      date: fields.date,
      amount_minor: fields.direction === 'out' ? -size : size,
      payee: payee === '' ? null : payee,
      category_id: transfer ? null : Number(fields.categoryId),
      transfer_account_id: transfer ? Number(fields.transferAccountId) : null,
      memo: memo === '' ? null : memo,
      tags: splitTags(fields.tags),
    },
  }
}
