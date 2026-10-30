import { describe, expect, it } from 'vitest'
import type { TransactionRow } from '@/api/transactions'
import {
  categoryLabel,
  dateRangeLabel,
  groupByDate,
  isFiltered,
  payeeLabel,
  readFilter,
  sourceLabel,
  statusLabel,
  withFilter,
} from './transactions'

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

describe('readFilter', () => {
  it('takes the search and filters from the query, skipping blanks and anything else', () => {
    expect(
      readFilter({
        q: 'corner',
        account: '3',
        category: 'uncategorized',
        from: '2026-01-01',
        to: '',
        tag: ['2', '5'],
        undecided: 'true',
        page: '4',
      }),
    ).toEqual({
      q: 'corner',
      account: '3',
      category: 'uncategorized',
      from: '2026-01-01',
      tag: '2',
      undecided: 'true',
    })
    expect(readFilter({ q: '  ', account: null, tag: [] })).toEqual({})
  })
})

describe('withFilter', () => {
  it('sets one, or drops it when blank, leaving the others', () => {
    const filter = { q: 'corner', account: '3' }
    expect(withFilter(filter, 'tag', '2')).toEqual({ q: 'corner', account: '3', tag: '2' })
    expect(withFilter(filter, 'account', '4')).toEqual({ q: 'corner', account: '4' })
    expect(withFilter(filter, 'account', '')).toEqual({ q: 'corner' })
    expect(withFilter(filter, 'q', undefined)).toEqual({ account: '3' })
    expect(filter).toEqual({ q: 'corner', account: '3' })
  })
})

describe('isFiltered', () => {
  it('is whether the search or any filter is set', () => {
    expect(isFiltered({})).toBe(false)
    expect(isFiltered({ q: 'x' })).toBe(true)
    expect(isFiltered({ undecided: 'true' })).toBe(true)
  })
})

describe('dateRangeLabel', () => {
  const day = (date: string) => `<${date}>`
  it('names either end or both, or is null with neither', () => {
    expect(dateRangeLabel({ from: '2026-01-01', to: '2026-01-31' }, day)).toBe(
      '<2026-01-01> – <2026-01-31>',
    )
    expect(dateRangeLabel({ from: '2026-01-01' }, day)).toBe('From <2026-01-01>')
    expect(dateRangeLabel({ to: '2026-01-31' }, day)).toBe('Until <2026-01-31>')
    expect(dateRangeLabel({ q: 'x' }, day)).toBeNull()
  })
})
