import { describe, expect, it } from 'vitest'
import type { Transaction } from '@/api/transactions'
import {
  amountText,
  blankFields,
  entryOf,
  fieldsOf,
  readFields,
  splitTags,
  todayIn,
} from './manual'

const us = { currency: 'USD', locale: 'en-US' }
const de = { currency: 'EUR', locale: 'de-DE' }

const row: Transaction = {
  id: 9,
  date: '2026-03-04',
  amount_minor: -123450,
  status: 'posted',
  payee: 'Corner Market',
  raw_description: 'Corner Market',
  account_id: 1,
  account: 'Wallet',
  category_id: 4,
  category: 'Groceries',
  split: false,
  split_categories: 0,
  transfer_account: null,
  transfer_account_id: null,
  tags: ['trip', 'weekly'],
  manual: true,
  memo: 'milk',
  splits: [],
  source: 'manual',
  categorization_reason: 'Set by Sam',
  import_batch: null,
  undecided: false,
  excluded: false,
}

describe('the entry form', () => {
  it('starts blank, money out, on today', () => {
    expect(blankFields('2026-03-04', '2')).toMatchObject({
      accountId: '2',
      date: '2026-03-04',
      direction: 'out',
      lands: 'category',
    })
  })

  it('reads a full form, money out negative and money in positive', () => {
    const fields = {
      ...blankFields('2026-03-04', '1'),
      amount: '12.5',
      payee: ' Corner Market ',
      categoryId: '4',
      memo: ' ',
      tags: '#trip, weekly  ',
    }
    expect(readFields(fields, us)).toEqual({
      entry: {
        account_id: 1,
        date: '2026-03-04',
        amount_minor: -1250,
        payee: 'Corner Market',
        category_id: 4,
        transfer_account_id: null,
        memo: null,
        tags: ['#trip', 'weekly'],
      },
    })
    const money_in = readFields({ ...fields, direction: 'in', amount: '-3' }, us)
    expect('entry' in money_in && money_in.entry.amount_minor).toBe(300)
  })

  it('needs a category unless it is a transfer to another account', () => {
    const fields = { ...blankFields('2026-03-04', '1'), amount: '5' }
    expect(readFields(fields, us)).toEqual({ errors: { category_id: 'Choose a category.' } })
    const transfer = { ...fields, lands: 'transfer' as const }
    expect(readFields(transfer, us)).toEqual({
      errors: { transfer_account_id: 'Choose an account.' },
    })
    expect(readFields({ ...transfer, transferAccountId: '1' }, us)).toEqual({
      errors: { transfer_account_id: 'Choose an account other than this one.' },
    })
    const done = readFields({ ...transfer, transferAccountId: '2', categoryId: '4' }, us)
    expect('entry' in done && done.entry).toMatchObject({
      category_id: null,
      transfer_account_id: 2,
    })
  })

  it('names every missing field at once', () => {
    const errors = readFields({ ...blankFields(''), amount: 'lots' }, us)
    expect(errors).toEqual({
      errors: {
        account_id: 'Choose an account.',
        date: 'Enter a date.',
        amount_minor: 'Enter an amount, like $12.50.',
        category_id: 'Choose a category.',
      },
    })
    const zero = readFields({ ...blankFields('2026-03-04', '1'), amount: '0', categoryId: '1' }, us)
    expect(zero).toEqual({ errors: { amount_minor: 'Enter an amount other than zero.' } })
  })
})

describe('a transaction read back', () => {
  it('fills the form to edit it', () => {
    expect(fieldsOf(row, us)).toEqual({
      accountId: '1',
      date: '2026-03-04',
      direction: 'out',
      amount: '1234.50',
      payee: 'Corner Market',
      lands: 'category',
      categoryId: '4',
      transferAccountId: '',
      memo: 'milk',
      tags: 'trip weekly',
    })
    const transfer = { ...row, amount_minor: 500, category_id: null, transfer_account_id: 2 }
    expect(fieldsOf(transfer, us)).toMatchObject({
      direction: 'in',
      lands: 'transfer',
      transferAccountId: '2',
      categoryId: '',
    })
    // And reads back as it was.
    const again = readFields(fieldsOf(row, de), de)
    expect('entry' in again && again.entry).toEqual({ ...entryOf(row), tags: row.tags })
  })

  it('enters again unchanged for Undo', () => {
    expect(entryOf(row)).toEqual({
      account_id: 1,
      date: '2026-03-04',
      amount_minor: -123450,
      payee: 'Corner Market',
      category_id: 4,
      transfer_account_id: null,
      memo: 'milk',
      tags: ['trip', 'weekly'],
    })
    const transfer = { ...row, category_id: null, transfer_account_id: 2 }
    expect(entryOf(transfer)).toMatchObject({ category_id: null, transfer_account_id: 2 })
  })
})

describe('helpers', () => {
  it('types amounts the locale way with no sign or groups', () => {
    expect(amountText(-123450, us)).toBe('1234.50')
    expect(amountText(5, de)).toBe('0,05')
    expect(amountText(1200, { currency: 'JPY', locale: 'en-US' })).toBe('1200')
  })

  it('splits tags on commas and spaces', () => {
    expect(splitTags(' a, b  c,,')).toEqual(['a', 'b', 'c'])
    expect(splitTags('')).toEqual([])
  })

  it("gives today's date in a timezone", () => {
    const now = new Date('2026-03-04T23:30:00Z')
    expect(todayIn('UTC', now)).toBe('2026-03-04')
    expect(todayIn('Europe/Berlin', now)).toBe('2026-03-05')
  })
})
