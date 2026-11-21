// The CSV importer's screens (docs/spec/m2-bank-data.md#csv-and-ofxqfx-import): what the mapping
// screen says about columns, the sentences the preview and Done steps use, and which rows to check
// are kept. Nothing here talks to the server.
import type { CsvDateFormat, CsvImportDone, CsvMapping, CsvPreview, CsvRead } from '@/api/csvImport'

/** The most a file may be: the server's limit on the text it takes, in bytes. */
export const MAX_FILE_BYTES = 10 * 1024 * 1024

/** The date formats the mapping screen offers, with a sample of each. */
export const DATE_FORMATS: readonly { value: CsvDateFormat; label: string }[] = [
  { value: 'ymd', label: 'Year-month-day (2026-03-04)' },
  { value: 'mdy', label: 'Month/day/year (03/04/2026)' },
  { value: 'dmy', label: 'Day/month/year (04/03/2026)' },
]

/** The mapping to start from: the one saved for this header, else the guess. */
export function startMapping(read: CsvRead): CsvMapping {
  return read.saved ?? read.guess
}

/** The header's cells: the last record the mapping skips. Empty when it skips none. */
export function headerCells(read: CsvRead, mapping: CsvMapping): string[] {
  return read.records[mapping.skip_rows - 1] ?? []
}

/** What a column is called in a picker: its header, else its number (columns count from 1). */
export function columnLabel(index: number, header: readonly string[]): string {
  const name = header[index]?.trim()
  return name ? `${name} (column ${String(index + 1)})` : `Column ${String(index + 1)}`
}

/** The picker's options for `columns` columns. */
export function columnOptions(
  columns: number,
  header: readonly string[],
): { value: number; label: string }[] {
  return Array.from({ length: columns }, (_, index) => ({
    value: index,
    label: columnLabel(index, header),
  }))
}

/** Switches the amount between one signed column and debit and credit columns. */
export function withAmountKind(
  mapping: CsvMapping,
  kind: 'signed' | 'debit_credit',
  columns: number,
): CsvMapping {
  const { amount } = mapping
  if (amount.kind === kind) return mapping
  if (amount.kind === 'debit_credit') {
    return { ...mapping, amount: { kind: 'signed', column: amount.debit } }
  }
  const credit = Math.min(amount.column + 1, columns - 1)
  return { ...mapping, amount: { kind: 'debit_credit', debit: amount.column, credit } }
}

/** "5 new, 2 already there, 1 to check". */
export function describePreview(preview: Pick<CsvPreview, 'new' | 'already' | 'to_check'>): string {
  return `${String(preview.new)} new, ${String(preview.already)} already there, ${String(preview.to_check)} to check`
}

/** "Skipped 3 rows that couldn't be read" and the like; empty when nothing was skipped. */
export function describeUnreadable(count: number): string {
  if (count === 0) return ''
  return count === 1
    ? '1 row can’t be read and will be skipped.'
    : `${String(count)} rows can’t be read and will be skipped.`
}

/** The Done step's line: "5 added, 2 already there, 1 skipped". */
export function describeDone(done: CsvImportDone): string {
  return `${String(done.added)} added, ${String(done.matched)} already there, ${String(done.skipped)} skipped`
}

/** The rows to check that are kept, as the commit asks for them: `row` numbers, in order. */
export function keptRows(check: readonly { row: number }[], kept: ReadonlySet<number>): number[] {
  return check.map((entry) => entry.row).filter((row) => kept.has(row))
}
