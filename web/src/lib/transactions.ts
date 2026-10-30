// Transactions (docs/spec/m1-ledger-import.md#transactions): how a row names its payee and
// category, the phone list's days, and the words for status and source. Nothing here talks to the
// server.
import type { Transaction, TransactionRow } from '@/api/transactions'

/** The payee, or what the bank sent when it has none *(agent)*. */
export function payeeLabel(row: TransactionRow): string {
  return row.payee ?? row.raw_description
}

/** The category's name, "Split" for a split row, or "Uncategorized". */
export function categoryLabel(row: TransactionRow): string {
  if (row.split) return 'Split'
  return row.category ?? 'Uncategorized'
}

export interface TransactionDay {
  /** `YYYY-MM-DD`. */
  date: string
  rows: TransactionRow[]
}

/** The rows as the phone lists them: one group a day, in the rows' order (newest first). */
export function groupByDate(rows: readonly TransactionRow[]): TransactionDay[] {
  const days: TransactionDay[] = []
  for (const row of rows) {
    const last = days.at(-1)
    if (last?.date === row.date) last.rows.push(row)
    else days.push({ date: row.date, rows: [row] })
  }
  return days
}

/** The status's name, like "Pending". */
export function statusLabel(status: TransactionRow['status']): string {
  return status === 'pending' ? 'Pending' : 'Posted'
}

const SOURCES: Record<Transaction['source'], string> = {
  provider: 'Bank',
  import: 'Import',
  manual: 'Manual',
}

/** Where it came from, like "Import". */
export function sourceLabel(source: Transaction['source']): string {
  return SOURCES[source]
}

/**
 * The list's search and filters, named as the API's query names them. The address carries them
 * the same way, `/transactions?account=3&from=2026-01-01`, so a link reproduces the view *(agent)*.
 * `category` is an id or `uncategorized`; `undecided` is `true` when set.
 */
const FILTER_KEYS = ['q', 'account', 'category', 'from', 'to', 'tag', 'undecided'] as const
export type FilterKey = (typeof FILTER_KEYS)[number]
export type TransactionFilter = Partial<Record<FilterKey, string>>

/** The search and filters in an address's query: the non-empty ones, the first of any repeated. */
export function readFilter(query: Readonly<Record<string, unknown>>): TransactionFilter {
  const filter: TransactionFilter = {}
  for (const key of FILTER_KEYS) {
    const given: unknown = query[key]
    const value: unknown = Array.isArray(given) ? given[0] : given
    if (typeof value === 'string' && value.trim() !== '') filter[key] = value
  }
  return filter
}

/** `filter` with `key` set to `value`, or without it when `value` is blank. */
export function withFilter(
  filter: TransactionFilter,
  key: FilterKey,
  value: string | undefined,
): TransactionFilter {
  const blank = value === undefined || value.trim() === ''
  return Object.fromEntries([
    ...Object.entries(filter).filter(([name]) => name !== key),
    ...(blank ? [] : [[key, value]]),
  ]) as TransactionFilter
}

/** Whether the search or any filter is set. */
export function isFiltered(filter: TransactionFilter): boolean {
  return FILTER_KEYS.some((key) => filter[key] !== undefined)
}

/**
 * The date filter's chip, like "Jan 1, 2026 – Jan 31, 2026", "From Jan 1, 2026" or "Until Jan
 * 31, 2026"; `null` with neither end set.
 */
export function dateRangeLabel(
  filter: TransactionFilter,
  format: (date: string) => string,
): string | null {
  const { from, to } = filter
  if (from !== undefined && to !== undefined) return `${format(from)} – ${format(to)}`
  if (from !== undefined) return `From ${format(from)}`
  if (to !== undefined) return `Until ${format(to)}`
  return null
}
