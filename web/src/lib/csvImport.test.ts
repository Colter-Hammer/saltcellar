import { describe, expect, it } from 'vitest'
import type { CsvMapping, CsvRead } from '@/api/csvImport'
import {
  columnLabel,
  columnOptions,
  describeDone,
  describePreview,
  describeUnreadable,
  headerCells,
  keptRows,
  startMapping,
  withAmountKind,
} from './csvImport'

const guess: CsvMapping = {
  date: 0,
  post_date: null,
  description: 1,
  amount: { kind: 'signed', column: 2 },
  flip_sign: false,
  date_format: 'ymd',
  category: null,
  memo: null,
  skip_rows: 1,
}
const read: CsvRead = {
  records: [['Date', 'Description', 'Amount']],
  record_count: 1,
  columns: 3,
  saved: null,
  guess,
}

describe('startMapping', () => {
  it('prefers the saved mapping to the guess', () => {
    expect(startMapping(read)).toBe(guess)
    const saved = { ...guess, flip_sign: true }
    expect(startMapping({ ...read, saved })).toBe(saved)
  })
})

describe('headerCells and column labels', () => {
  it('reads the header from the last skipped record', () => {
    expect(headerCells(read, guess)).toEqual(['Date', 'Description', 'Amount'])
    expect(headerCells(read, { ...guess, skip_rows: 0 })).toEqual([])
    expect(headerCells(read, { ...guess, skip_rows: 5 })).toEqual([])
  })

  it('names a column by its header, else its number', () => {
    expect(columnLabel(1, ['Date', ' Description '])).toBe('Description (column 2)')
    expect(columnLabel(2, ['Date', 'Description', ' '])).toBe('Column 3')
    expect(columnLabel(4, [])).toBe('Column 5')
    expect(columnOptions(2, ['A'])).toEqual([
      { value: 0, label: 'A (column 1)' },
      { value: 1, label: 'Column 2' },
    ])
  })
})

describe('withAmountKind', () => {
  it('switches between one signed column and debit and credit columns', () => {
    expect(withAmountKind(guess, 'signed', 3)).toBe(guess)
    expect(withAmountKind(guess, 'debit_credit', 4).amount).toEqual({
      kind: 'debit_credit',
      debit: 2,
      credit: 3,
    })
    // The last column has no neighbour to be the credit.
    expect(withAmountKind(guess, 'debit_credit', 3).amount).toEqual({
      kind: 'debit_credit',
      debit: 2,
      credit: 2,
    })
    const split: CsvMapping = { ...guess, amount: { kind: 'debit_credit', debit: 3, credit: 4 } }
    expect(withAmountKind(split, 'signed', 5).amount).toEqual({ kind: 'signed', column: 3 })
  })
})

describe('the sentences', () => {
  it('sums the preview up', () => {
    expect(describePreview({ new: 5, already: 2, to_check: 1 })).toBe(
      '5 new, 2 already there, 1 to check',
    )
  })

  it('says how many rows are skipped for not reading', () => {
    expect(describeUnreadable(0)).toBe('')
    expect(describeUnreadable(1)).toBe('1 row can’t be read and will be skipped.')
    expect(describeUnreadable(3)).toBe('3 rows can’t be read and will be skipped.')
  })

  it('sums the import up', () => {
    expect(describeDone({ batch_id: 1, added: 5, matched: 2, skipped: 1 })).toBe(
      '5 added, 2 already there, 1 skipped',
    )
  })
})

describe('keptRows', () => {
  it('lists the rows to check that were kept, in order', () => {
    expect(keptRows([{ row: 4 }, { row: 7 }, { row: 9 }], new Set([9, 4, 99]))).toEqual([4, 9])
  })
})
