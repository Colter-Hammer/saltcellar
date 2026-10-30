import { describe, expect, it } from 'vitest'
import type { TransactionRow } from '@/api/transactions'
import { categoryLabel, groupByDate, payeeLabel, sourceLabel, statusLabel } from './transactions'

function row(id: number, fields: Partial<TransactionRow> = {}): TransactionRow {
  return {
    id,
    date: '2026-01-05',
    amount_minor: -1250,
    status: 'posted',
    payee: 'Corner Market',
    raw_description: 'CORNER MKT 01',
    account_id: 1,
    account: 'Checking',
    category_id: null,
    category: null,
    split: false,
    tags: [],
    ...fields,
  }
}

describe('payeeLabel', () => {
  it('is the payee, or what the bank sent without one', () => {
    expect(payeeLabel(row(1))).toBe('Corner Market')
    expect(payeeLabel(row(1, { payee: null }))).toBe('CORNER MKT 01')
  })
})

describe('categoryLabel', () => {
  it('names the category, a split, or says Uncategorized', () => {
    expect(categoryLabel(row(1, { category_id: 3, category: 'Groceries' }))).toBe('Groceries')
    expect(categoryLabel(row(1, { split: true }))).toBe('Split')
    expect(categoryLabel(row(1))).toBe('Uncategorized')
  })
})

describe('groupByDate', () => {
  it('makes one group a day, in the order given', () => {
    const rows = [
      row(3, { date: '2026-01-05' }),
      row(2, { date: '2026-01-05' }),
      row(1, { date: '2026-01-02' }),
    ]
    expect(groupByDate(rows).map((day) => [day.date, day.rows.map((r) => r.id)])).toEqual([
      ['2026-01-05', [3, 2]],
      ['2026-01-02', [1]],
    ])
    expect(groupByDate([])).toEqual([])
  })
})

describe('statusLabel and sourceLabel', () => {
  it('name each status and source', () => {
    expect(statusLabel('pending')).toBe('Pending')
    expect(statusLabel('posted')).toBe('Posted')
    expect(sourceLabel('provider')).toBe('Bank')
    expect(sourceLabel('import')).toBe('Import')
    expect(sourceLabel('manual')).toBe('Manual')
  })
})
