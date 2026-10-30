import { describe, expect, it } from 'vitest'
import { describeUpload } from './import'

const upload = {
  id: 'abc',
  budget_name: 'Example Household',
  accounts: 5,
  transactions: 1234,
  categories: 7,
  first_date: '2025-01-01',
  last_date: '2026-01-05',
}

describe('describeUpload', () => {
  it('counts and dates the export in the locale', () => {
    expect(describeUpload(upload, { locale: 'en-US' })).toEqual({
      counts: '1,234 transactions · 5 accounts · 7 categories',
      dates: 'Jan 1, 2025 – Jan 5, 2026',
    })
    expect(describeUpload(upload, { locale: 'de-DE' }).counts).toBe(
      '1.234 transactions · 5 accounts · 7 categories',
    )
  })

  it('says one of each in the singular', () => {
    const one = { ...upload, transactions: 1, accounts: 1, categories: 1 }
    expect(describeUpload(one, { locale: 'en-US' }).counts).toBe(
      '1 transaction · 1 account · 1 category',
    )
  })

  it('says when there are no transactions to date', () => {
    const none = { ...upload, transactions: 0, first_date: null, last_date: null }
    expect(describeUpload(none, { locale: 'en-US' })).toEqual({
      counts: '0 transactions · 5 accounts · 7 categories',
      dates: 'No transactions',
    })
  })
})
