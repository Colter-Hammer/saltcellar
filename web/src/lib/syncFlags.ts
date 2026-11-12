// Sync flags on rows (docs/spec/m2-bank-data.md#sync-health-in-the-ui): the badge's words, and
// the split fix form's lines. Nothing here talks to the server.
import type { SplitLineEntry } from '@/api/syncFlags'
import type { Transaction, TransactionRow } from '@/api/transactions'
import { type Formatting, parseMoney } from '@/lib/format'
import { amountText } from '@/lib/manual'

type SyncFlag = NonNullable<TransactionRow['sync_flag']>

const BADGES: Record<SyncFlag, string> = {
  possible_duplicate: 'Duplicate?',
  possible_manual_match: 'Manual match?',
  not_seen_at_bank: 'Not at bank',
  amount_changed_split: 'Fix split',
}

/** The flag's badge in the list and the detail, like "Duplicate?" *(agent)*. */
export function flagBadge(flag: SyncFlag): string {
  return BADGES[flag]
}

/** A split line as the fix form edits it: the amount as typed, in the transaction's direction. */
export interface SplitLineFields {
  category: string
  amount: string
  memo: string
}

type Money = Pick<Formatting, 'currency' | 'locale'>

/**
 * The form's lines for `transaction`'s split. Amounts are typed in the transaction's direction,
 * without a sign; a line going the other way has a minus *(agent)*.
 */
export function splitFieldsOf(transaction: Transaction, formatting: Money): SplitLineFields[] {
  const way = Math.sign(transaction.amount_minor) || 1
  return transaction.splits.map((line) => ({
    category: String(line.category_id),
    amount: `${line.amount_minor * way < 0 ? '-' : ''}${amountText(line.amount_minor, formatting)}`,
    memo: line.memo ?? '',
  }))
}

/** A line's amount in signed minor units, or `null` while it can't be read. */
function lineMinor(fields: SplitLineFields, total: number, formatting: Money): number | null {
  const typed = parseMoney(fields.amount, formatting)
  return typed === null ? null : typed * (Math.sign(total) || 1)
}

/** What's left of `total` to split over the lines that can be read, in minor units. */
export function splitRemaining(
  lines: readonly SplitLineFields[],
  total: number,
  formatting: Money,
): number {
  return lines.reduce((left, line) => left - (lineMinor(line, total, formatting) ?? 0), total)
}

/**
 * The lines as the API takes them, or the problem in words: every line needs a category and an
 * amount, and together they make the total.
 */
export function readSplit(
  lines: readonly SplitLineFields[],
  total: number,
  formatting: Money,
): SplitLineEntry[] | string {
  const read: SplitLineEntry[] = []
  for (const line of lines) {
    const amount = lineMinor(line, total, formatting)
    if (line.category === '') return 'Choose a category for every line.'
    if (amount === null) return 'Type an amount for every line.'
    const memo = line.memo.trim()
    read.push({
      category_id: Number(line.category),
      amount_minor: amount,
      ...(memo !== '' && { memo }),
    })
  }
  if (read.length === 0) return 'Add a line.'
  if (splitRemaining(lines, total, formatting) !== 0) return 'The lines must add up to the amount.'
  return read
}
