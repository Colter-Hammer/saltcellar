import { describe, expect, it } from 'vitest'
import type { Transaction } from '@/api/transactions'
import { flagBadge, readSplit, splitFieldsOf, splitRemaining } from './syncFlags'

const usd = { currency: 'USD', locale: 'en-US' }

function split(amount: number, lines: [number, number][]): Transaction {
  return {
    id: 7,
    date: '2026-01-05',
    amount_minor: amount,
    status: 'posted',
    payee: 'Corner Market',
    raw_description: 'CORNER MKT 01',
    account_id: 1,
    account: 'Checking',
    category_id: null,
    category: null,
    split: true,
    split_categories: lines.length,
    transfer_account: null,
    transfer_account_id: null,
    tags: [],
    manual: false,
    sync_flag: 'amount_changed_split',
    memo: null,
    splits: lines.map(([category, minor]) => ({
      category_id: category,
      category: `Category ${String(category)}`,
      amount_minor: minor,
      memo: null,
    })),
    source: 'provider',
    categorization_reason: null,
    import_batch: null,
    undecided: false,
    excluded: false,
    flagged_against: null,
  }
}

describe('flagBadge', () => {
  it('names each flag', () => {
    expect(flagBadge('possible_duplicate')).toBe('Duplicate?')
    expect(flagBadge('possible_manual_match')).toBe('Manual match?')
    expect(flagBadge('not_seen_at_bank')).toBe('Not at bank')
    expect(flagBadge('amount_changed_split')).toBe('Fix split')
  })
})

describe('the split fix', () => {
  it('types amounts in the transaction’s direction, a line going the other way with a minus', () => {
    const fields = splitFieldsOf(
      split(-3500, [
        [1, -4000],
        [2, 500],
      ]),
      usd,
    )
    expect(fields).toEqual([
      { category: '1', amount: '40.00', memo: '' },
      { category: '2', amount: '-5.00', memo: '' },
    ])
  })

  it('shows what is left and reads lines that add up', () => {
    const lines = [
      { category: '1', amount: '30', memo: '' },
      { category: '2', amount: '18.00', memo: ' snacks ' },
    ]
    expect(splitRemaining(lines, -5000, usd)).toBe(-200)
    expect(readSplit(lines, -5000, usd)).toBe('The lines must add up to the amount.')
    expect(readSplit(lines, -4800, usd)).toEqual([
      { category_id: 1, amount_minor: -3000 },
      { category_id: 2, amount_minor: -1800, memo: 'snacks' },
    ])
    expect(splitRemaining(lines, 4800, usd)).toBe(0)
  })

  it('needs a category and an amount on every line, and a line', () => {
    expect(readSplit([{ category: '', amount: '1', memo: '' }], -100, usd)).toBe(
      'Choose a category for every line.',
    )
    expect(readSplit([{ category: '1', amount: 'x', memo: '' }], -100, usd)).toBe(
      'Type an amount for every line.',
    )
    expect(readSplit([], -100, usd)).toBe('Add a line.')
  })
})
