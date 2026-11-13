// How sync reads on Settings › Bank connection & sync (docs/spec/m2-bank-data.md#sync-runs).
import type { Bank, SyncRun, SyncState } from '@/api/bank'
import { type Formatting, formatAroundHour, formatInstantTime } from './format'

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

/** What started a run, as Recent syncs names it *(agent)*. */
export function triggerName(trigger: string): string {
  const names: Record<string, string> = {
    scheduled: 'Daily sync',
    manual: 'Sync now',
    retry: 'Retry',
    catch_up: 'Catch-up',
    backfill: 'Back-fill',
  }
  return names[trigger] ?? trigger
}

/** "Syncs daily around 6 AM · 9 of 12 requests left today". */
export function scheduleLine(sync: SyncState, formatting: Pick<Formatting, 'locale'>): string {
  const around = formatAroundHour(sync.time, formatting)
  const left = `${String(sync.requests_left)} of ${String(sync.request_cap)}`
  return `Syncs daily around ${around} · ${left} requests left today`
}

/**
 * Why Sync now waits at `now`, or null when it can go: the request limit ("Sync limit reached,
 * try again after 3:40 PM", in the household's `timeZone`), else the cooldown ("Synced 12 min
 * ago").
 */
export function syncNowWait(
  sync: SyncState,
  now: Date,
  timeZone: string,
  formatting: Pick<Formatting, 'locale'>,
): string | null {
  if (sync.limit_until !== null && new Date(sync.limit_until) > now) {
    const at = formatInstantTime(sync.limit_until, timeZone, formatting)
    return `Sync limit reached, try again after ${at}`
  }
  if (
    sync.cooldown_until !== null &&
    sync.last_synced_at !== null &&
    new Date(sync.cooldown_until) > now
  ) {
    const minutes = Math.max(
      0,
      Math.floor((now.getTime() - new Date(sync.last_synced_at).getTime()) / 60_000),
    )
    return minutes === 0 ? 'Synced just now' : `Synced ${String(minutes)} min ago`
  }
  return null
}

/**
 * An account's failure in a run, naming the account once: the bank's message often names it
 * already *(agent)*.
 */
export function accountProblem(account: {
  name: string
  error: string | null
  error_code: string | null
}): string {
  const said = account.error ?? account.error_code ?? 'failed'
  return said.startsWith(account.name) ? said : `${account.name}: ${said}`
}

/**
 * Where **Fix at SimpleFIN ↗** goes: the Bridge's sign-in, from which its connections page is one
 * click *(agent: the Bridge has no stable deep link to the connections page)*.
 */
export const SIMPLEFIN_FIX_URL = 'https://bridge.simplefin.org/auth/login'

/** What a paused connection's banner says, with where its link goes: the screen's name. */
export function pausedBanner(status: string): { lead: string; link: string; tail: string } {
  const link = 'Settings'
  switch (status) {
    case 'unpaid':
      return {
        lead: 'SimpleFIN subscription unpaid. Bank sync stopped. Pay it, then paste a new setup token in ',
        link,
        tail: '.',
      }
    case 'key_mismatch':
      return {
        lead: 'Bank sync stopped. The secret key changed. Paste a new setup token in ',
        link,
        tail: '.',
      }
    default:
      return {
        lead: 'Bank sync stopped. Get a new setup token from SimpleFIN and paste it in ',
        link,
        tail: '.',
      }
  }
}

/** A bank link's state in a word or two, for the Banks list. */
export function bankState(bank: Bank): string {
  switch (bank.state) {
    case 'ok':
      return 'OK'
    case 'balances_only':
      return 'Balances only'
    case 'stale':
      return 'Stale'
    case 'needs_reconnecting':
      return 'Needs reconnecting'
    case 'needs_attention':
      return 'Needs attention'
  }
}

/** "1 account" / "3 accounts". */
export function accountCount(n: number): string {
  return count(n, 'account', 'accounts')
}
