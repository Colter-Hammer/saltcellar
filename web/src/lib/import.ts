// The importer's steps (docs/spec/m1-ledger-import.md#import-from-actual): what the File step says
// about an upload, and the choices of the Accounts and Categories steps. The choices live in the
// browser until the import commits *(agent)*, sent as the commit's request; then what the Done step
// says. Nothing here talks to the server.
import type { CategoryGroup, CategoryGroupKind } from '@/api/categories'
import type {
  AccountRole,
  AccountType,
  ActualCategory,
  ActualCommitRequest,
  ActualImport,
  ActualImportDone,
  ActualPreview,
  ActualUpload,
} from '@/api/imports'
import { formatDate, type Formatting } from './format'

/** `count` and the noun, singular or plural: "1 account", "1,234 transactions". */
function counted(count: number, [one, many]: [string, string], locale: string): string {
  return `${new Intl.NumberFormat(locale).format(count)} ${count === 1 ? one : many}`
}

/** The upload's counts, and the dates its transactions span. */
export function describeUpload(
  upload: ActualUpload,
  { locale }: Pick<Formatting, 'locale'>,
): { counts: string; dates: string } {
  const counts = [
    counted(upload.transactions, ['transaction', 'transactions'], locale),
    counted(upload.accounts, ['account', 'accounts'], locale),
    counted(upload.categories, ['category', 'categories'], locale),
  ].join(' · ')
  const dates =
    upload.first_date === null || upload.last_date === null
      ? 'No transactions'
      : `${formatDate(upload.first_date, { locale })} – ${formatDate(upload.last_date, { locale })}`
  return { counts, dates }
}

/** Two names are the same when they differ only in case or surrounding spaces. */
function sameName(a: string, b: string): boolean {
  return a.trim().localeCompare(b.trim(), undefined, { sensitivity: 'accent' }) === 0
}

// ── Step 2: Accounts ──

/**
 * Create a new app account, use one that exists or track a bank account waiting in the new
 * accounts prompt (both Same as), or leave the account out.
 */
type AccountAction = 'create' | 'same' | 'skip'

/**
 * What happens to one Actual account. Every field is kept whichever action is chosen, so switching
 * back finds what was typed.
 */
export interface AccountChoice {
  action: AccountAction
  /** Create new: the new account's name, type, role and whether it holds balances only. */
  name: string
  type: AccountType
  role: AccountRole
  balancesOnly: boolean
  /** Same as: the app account, or null until one is chosen. */
  sameAs: number | null
  /**
   * Same as a waiting bank account instead: its provider id. Tracking it takes `role` and
   * `balancesOnly` too.
   */
  track: string | null
}

/** The Same as picker's value for an app account and for a waiting bank account *(agent)*. */
export function sameAsValue(choice: Pick<AccountChoice, 'sameAs' | 'track'>): string {
  if (choice.track !== null) return `bank:${choice.track}`
  return choice.sameAs === null ? '' : `app:${String(choice.sameAs)}`
}

/** The choice's Same as fields for a picker value. */
export function fromSameAsValue(value: string): Pick<AccountChoice, 'sameAs' | 'track'> {
  if (value.startsWith('bank:')) return { sameAs: null, track: value.slice('bank:'.length) }
  if (value.startsWith('app:')) return { sameAs: Number(value.slice('app:'.length)), track: null }
  return { sameAs: null, track: null }
}

/**
 * Each Actual account's starting choice, by its id: Same as the app account or waiting bank
 * account the server matched it to (by bank-sync id, or an app account by name), otherwise Create
 * new with Actual's name, the guessed type and the role from on or off budget.
 */
export function startAccountChoices(plan: ActualImport): Record<string, AccountChoice> {
  return Object.fromEntries(
    plan.accounts.map((account) => [
      account.id,
      {
        action: account.same_as === null && account.same_as_waiting === null ? 'create' : 'same',
        name: account.name,
        type: account.type,
        role: account.role,
        balancesOnly: false,
        sameAs: account.same_as,
        track: account.same_as_waiting,
      },
    ]),
  )
}

/**
 * What stops each account's choice from going ahead, by Actual id: a new account needs a name no
 * other account has (in any case) *(agent)*, and Same as needs an account. Accounts with no
 * problem aren't listed.
 */
export function accountProblems(
  plan: ActualImport,
  choices: Readonly<Record<string, AccountChoice>>,
): Record<string, string> {
  const names = [
    ...plan.app_accounts.map((account) => account.name),
    ...Object.values(choices)
      .filter((choice) => choice.action === 'create')
      .map((choice) => choice.name),
  ]
  const problems: Record<string, string> = {}
  for (const account of plan.accounts) {
    const choice = choices[account.id]
    if (choice?.action === 'create') {
      if (choice.name.trim() === '') problems[account.id] = 'Enter a name.'
      else if (names.filter((name) => sameName(name, choice.name)).length > 1) {
        problems[account.id] = 'Another account has this name.'
      }
    } else if (choice?.action === 'same' && choice.sameAs === null && choice.track === null) {
      problems[account.id] = 'Choose the account it’s the same as.'
    }
  }
  return problems
}

/**
 * The Actual accounts whose rows are imported: those created or tracked with transactions, and
 * those the same as an app account. A skipped account brings nothing, and a balances-only one only
 * its balance *(agent)*.
 */
function importedAccounts(choices: Readonly<Record<string, AccountChoice>>): Set<string> {
  const imports = (choice: AccountChoice) =>
    choice.action === 'same'
      ? choice.track === null || !choice.balancesOnly
      : choice.action === 'create' && !choice.balancesOnly
  return new Set(
    Object.entries(choices)
      .filter(([, choice]) => imports(choice))
      .map(([id]) => id),
  )
}

// ── Step 3: Categories ──

/** An Actual category step 3 lists, and how many imported rows use it. */
export interface UsedCategory {
  category: ActualCategory
  rows: number
}

/** The Actual categories that rows in the imported accounts use, in Actual's order. */
export function usedCategories(
  plan: ActualImport,
  choices: Readonly<Record<string, AccountChoice>>,
): UsedCategory[] {
  const imported = importedAccounts(choices)
  return plan.categories
    .map((category) => ({
      category,
      rows: category.rows
        .filter((use) => imported.has(use.account_id))
        .reduce((sum, use) => sum + use.rows, 0),
    }))
    .filter((used) => used.rows > 0)
}

/** Goes to: Decide later, Transfer (not a category), or an app category (`category:<id>`). */
export const LATER = 'later'
export const TRANSFER = 'transfer'
/** The picker's + New category…, which opens a form rather than being a choice. */
export const NEW_CATEGORY = 'new'

export function goesToCategory(id: number): string {
  return `category:${String(id)}`
}

/** The app category a choice goes to, or null for Decide later and Transfer. */
export function chosenCategory(goesTo: string): number | null {
  return goesTo.startsWith('category:') ? Number(goesTo.slice('category:'.length)) : null
}

/** The app's categories the picker offers, by group: hidden ones aren't *(agent)*. */
export function offeredCategories(
  groups: readonly CategoryGroup[],
): { id: number; name: string; categories: { id: number; name: string }[] }[] {
  return groups
    .filter((group) => !group.hidden)
    .map((group) => ({
      id: group.id,
      name: group.name,
      categories: group.categories.filter((category) => !category.hidden),
    }))
    .filter((group) => group.categories.length > 0)
}

/**
 * Where an Actual category starts out going: the app category of the same name, ignoring case
 * (one in a group of the same name first), otherwise Decide later.
 */
function prefill(category: ActualCategory, groups: readonly CategoryGroup[]): string {
  const matches = offeredCategories(groups).flatMap((group) =>
    group.categories
      .filter((offered) => sameName(offered.name, category.name))
      .map((offered) => ({ group, offered })),
  )
  const best = matches.find(({ group }) => sameName(group.name, category.group)) ?? matches[0]
  return best === undefined ? LATER : goesToCategory(best.offered.id)
}

/**
 * `choices` with a starting choice added for each Actual category that has none yet. Those made
 * already stay, so going back and forth between the steps keeps them.
 */
export function startCategoryChoices(
  plan: ActualImport,
  groups: readonly CategoryGroup[],
  choices: Readonly<Record<string, string>>,
): Record<string, string> {
  const started = { ...choices }
  for (const category of plan.categories) {
    started[category.id] ??= prefill(category, groups)
  }
  return started
}

/**
 * The group + New category… starts on, for an Actual category: the app group of the same name,
 * else the first of the same kind, else none, to make a new one *(agent)*.
 */
export function startingGroup(
  category: ActualCategory,
  groups: readonly CategoryGroup[],
): number | null {
  const kind: CategoryGroupKind = category.income ? 'income' : 'expense'
  const shown = groups.filter((group) => !group.hidden)
  const group =
    shown.find((group) => sameName(group.name, category.group)) ??
    shown.find((group) => group.kind === kind)
  return group?.id ?? null
}

// ── Step 4: Done ──

/**
 * The preview's and commit's request: every account's choice, the Goes to of each category the
 * imported rows use (any other goes to Decide later on the server too), and the possible
 * duplicates the household says are new.
 */
export function commitRequest(
  plan: ActualImport,
  accountChoices: Readonly<Record<string, AccountChoice>>,
  categoryChoices: Readonly<Record<string, string>>,
  fileName: string,
  newRows: readonly string[] = [],
): ActualCommitRequest {
  const accounts = plan.accounts.flatMap((account) => {
    const choice = accountChoices[account.id]
    if (choice === undefined) return []
    const tracks = choice.action === 'same' && choice.track !== null
    return [
      {
        actual_id: account.id,
        action: tracks ? 'track' : choice.action,
        name: choice.name.trim(),
        type: choice.type,
        role: choice.role,
        balances_only: choice.balancesOnly,
        same_as: choice.action === 'same' && !tracks ? choice.sameAs : null,
        track: tracks ? choice.track : null,
      },
    ] as const
  })
  const categories = usedCategories(plan, accountChoices).map(({ category }) => {
    const goesTo = categoryChoices[category.id] ?? LATER
    const id = chosenCategory(goesTo)
    return {
      actual_id: category.id,
      goes_to: id === null ? (goesTo === TRANSFER ? 'transfer' : 'later') : 'category',
      category_id: id,
    } as const
  })
  return { file_name: fileName, accounts, categories, new_rows: [...newRows] }
}

/** The preview's line: how many rows come in, and how many are there already. */
export function describePreview(preview: ActualPreview, locale: string): string {
  const rows = (count: number) => counted(count, ['row', 'rows'], locale)
  return `${rows(preview.new)} new, ${rows(preview.already)} already here`
}

/** The Done step's lines: what was created, and what happened to the rows. */
export function describeDone(done: ActualImportDone, locale: string): string[] {
  const rows = (count: number) => counted(count, ['row', 'rows'], locale)
  return [
    `${counted(done.accounts_created, ['account', 'accounts'], locale)} created`,
    // Rows that were here already, synced or from an earlier import *(agent: shown only if any)*.
    ...(done.matched > 0 ? [`${rows(done.matched)} already here`] : []),
    `${rows(done.mapped)} mapped to a category`,
    `${rows(done.undecided)} undecided`,
    `${rows(done.transfer)} marked transfer`,
    `${rows(done.uncategorized)} uncategorized`,
    counted(done.linked_transfers, ['linked transfer', 'linked transfers'], locale),
  ]
}
