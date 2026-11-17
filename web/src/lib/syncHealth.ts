// What the sync status spot and sheet say (docs/spec/m2-bank-data.md#sync-health-in-the-ui).
import type { Account } from '@/api/accounts'
import type { Connection } from '@/api/bank'
import { type Formatting, formatInstantTime } from './format'

/** Where a problem's fix leads. */
export type ProblemFix =
  { kind: 'simplefin' } | { kind: 'suspect-rows'; to: string } | { kind: 'account'; to: string }

export interface SyncProblem {
  accountId: number
  account: string
  /** A word or two: "Needs reconnecting", "Needs attention", "Balance off". */
  state: string
  /** One sentence. */
  message: string
  fix: ProblemFix
}

/** A quiet state: listed in the sheet, never counted. */
export interface QuietState {
  accountId: number
  account: string
  message: string
}

/**
 * The problems that count toward the pill, one per problem *(agent)*: a bank link that needs
 * reconnecting or attention, and a balance flagged as drifting. Stale data is quiet.
 */
export function syncProblems(accounts: readonly Account[]): SyncProblem[] {
  const problems: SyncProblem[] = []
  for (const account of accounts) {
    if (account.closed) continue
    const sync = account.sync
    if (sync?.state === 'needs_reconnecting' || sync?.state === 'needs_attention') {
      problems.push({
        accountId: account.id,
        account: account.name,
        state: sync.state === 'needs_reconnecting' ? 'Needs reconnecting' : 'Needs attention',
        message: sync.message ?? 'Its bank data needs attention.',
        fix: sync.fix ? { kind: 'simplefin' } : { kind: 'account', to: `/accounts/${account.id}` },
      })
    }
    if (account.drift.flag) {
      problems.push({
        accountId: account.id,
        account: account.name,
        state: 'Balance off',
        message: 'Its balance and the bank’s have stayed apart.',
        fix: { kind: 'suspect-rows', to: `/accounts/${account.id}?suspect=1` },
      })
    }
  }
  return problems
}

/** Accounts whose data is stale: listed without counting toward the pill. */
export function quietStates(accounts: readonly Account[]): QuietState[] {
  return accounts
    .filter((a) => !a.closed && a.sync?.state === 'stale')
    .map((a) => ({
      accountId: a.id,
      account: a.name,
      message: a.sync?.message ?? 'Data is stale.',
    }))
}

export interface Spot {
  /** A filled dot: something needs the household. */
  filled: boolean
  /** The phone pill's words. */
  short: string
  /** The sidebar button's words. */
  long: string
}

/**
 * The status spot: the last sync's time, or "Sync paused", or the count of problems. Null before
 * a connection exists *(agent)*.
 */
export function spot(
  connection: Connection | null,
  problems: number,
  lastSyncedAt: string | null,
  timeZone: string,
  formatting: Pick<Formatting, 'locale'>,
): Spot | null {
  if (connection === null) return null
  if (connection.paused) return { filled: true, short: 'Sync paused', long: 'Sync paused' }
  if (problems > 0) {
    const text = `${String(problems)} sync ${problems === 1 ? 'problem' : 'problems'}`
    return { filled: true, short: text, long: text }
  }
  if (lastSyncedAt === null) return { filled: false, short: 'Not synced', long: 'Not synced yet' }
  const time = formatInstantTime(lastSyncedAt, timeZone, formatting)
  return { filled: false, short: time, long: `Synced ${time}` }
}
