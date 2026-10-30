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
