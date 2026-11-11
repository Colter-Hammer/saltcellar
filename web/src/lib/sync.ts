// How a sync run reads on Settings › Bank connection & sync (docs/spec/m2-bank-data.md#sync-runs).
import type { SyncRun } from '@/api/bank'

function count(n: number, one: string, many: string): string {
  return `${String(n)} ${n === 1 ? one : many}`
}

/**
 * What a run did, in a few words *(agent)*: "Running…", its error, "No changes", or
 * "3 rows added, 1 row updated".
 */
export function runSummary(run: SyncRun): string {
  if (run.finished_at === null) return 'Running…'
  if (run.error !== null) return run.error
  if (run.added === 0 && run.updated === 0) return 'No changes'
  const parts: string[] = []
  if (run.added > 0) parts.push(`${count(run.added, 'row', 'rows')} added`)
  if (run.updated > 0) parts.push(`${count(run.updated, 'row', 'rows')} updated`)
  return parts.join(', ')
}
