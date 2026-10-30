import { describe, expect, it } from 'vitest'
import type { CategoryGroup } from '@/api/categories'
import type { ActualAccount, ActualCategory, ActualImport } from '@/api/imports'
import {
  type AccountChoice,
  accountProblems,
  describeUpload,
  goesToCategory,
  LATER,
  offeredCategories,
  startAccountChoices,
  startCategoryChoices,
  startingGroup,
  usedCategories,
} from './import'

const upload = {
  id: 'abc',
  budget_name: 'Example Household',
  accounts: 5,
  transactions: 1234,
  categories: 7,
  first_date: '2025-01-01',
  last_date: '2026-01-05',
}

describe('describeUpload', () => {
  it('counts and dates the export in the locale', () => {
    expect(describeUpload(upload, { locale: 'en-US' })).toEqual({
      counts: '1,234 transactions · 5 accounts · 7 categories',
      dates: 'Jan 1, 2025 – Jan 5, 2026',
    })
    expect(describeUpload(upload, { locale: 'de-DE' }).counts).toBe(
      '1.234 transactions · 5 accounts · 7 categories',
    )
  })

  it('says one of each in the singular', () => {
    const one = { ...upload, transactions: 1, accounts: 1, categories: 1 }
    expect(describeUpload(one, { locale: 'en-US' }).counts).toBe(
      '1 transaction · 1 account · 1 category',
    )
  })

  it('says when there are no transactions to date', () => {
    const none = { ...upload, transactions: 0, first_date: null, last_date: null }
    expect(describeUpload(none, { locale: 'en-US' })).toEqual({
      counts: '0 transactions · 5 accounts · 7 categories',
      dates: 'No transactions',
    })
  })
})

function account(id: string, changes: Partial<ActualAccount> = {}): ActualAccount {
  return {
    id,
    name: `Account ${id}`,
    closed: false,
    off_budget: false,
    rows: 10,
    balance_minor: 0,
    type: 'checking',
    role: 'budgeted',
    same_as: null,
    ...changes,
  }
}

function category(
  id: string,
  name: string,
  rows: [string, number][],
  changes: Partial<ActualCategory> = {},
): ActualCategory {
  return {
    id,
    name,
    group: 'Everyday',
    income: false,
    rows: rows.map(([account_id, count]) => ({ account_id, rows: count })),
    ...changes,
  }
}

const plan: ActualImport = {
  id: 'abc',
  budget_name: 'Example Household',
  accounts: [
    account('checking', { name: 'Everyday checking' }),
    account('card', { name: 'Rewards card', type: 'credit_card', same_as: 7 }),
    account('brokerage', { name: 'Brokerage', type: 'investment', role: 'tracking' }),
  ],
  categories: [
    category('groceries', 'Groceries', [
      ['checking', 3],
      ['card', 14],
    ]),
    category('fuel', 'Fuel', [['card', 12]]),
    category('invest', 'Investing', [['brokerage', 1]]),
  ],
  app_accounts: [{ id: 7, name: 'Shared card' }],
}

describe('the account choices', () => {
  it('start on Create new with what the server guessed, or Same as its match', () => {
    const choices = startAccountChoices(plan)
    expect(choices.checking).toEqual({
      action: 'create',
      name: 'Everyday checking',
      type: 'checking',
      role: 'budgeted',
      balancesOnly: false,
      sameAs: null,
    })
    expect(choices.card).toMatchObject({ action: 'same', sameAs: 7, type: 'credit_card' })
    expect(choices.brokerage).toMatchObject({ action: 'create', role: 'tracking' })
  })

  it('have no problems as they start', () => {
    expect(accountProblems(plan, startAccountChoices(plan))).toEqual({})
  })

  it('need a name no other account has, in any case', () => {
    const choices = startAccountChoices(plan)
    const create = (name: string): AccountChoice => ({
      action: 'create',
      name,
      type: 'checking',
      role: 'budgeted',
      balancesOnly: false,
      sameAs: null,
    })
    expect(
      accountProblems(plan, { ...choices, checking: create('  '), card: create('shared CARD ') }),
    ).toEqual({ checking: 'Enter a name.', card: 'Another account has this name.' })
    expect(accountProblems(plan, { ...choices, checking: create('Brokerage') })).toEqual({
      checking: 'Another account has this name.',
      brokerage: 'Another account has this name.',
    })
    // A skipped account's name is free.
    const skipped = { ...create('Brokerage'), action: 'skip' as const }
    expect(accountProblems(plan, { ...choices, checking: skipped })).toEqual({})
  })

  it('need an account to be the same as', () => {
    const choices = startAccountChoices(plan)
    const same = { ...choices.card, action: 'same', sameAs: null } as AccountChoice
    expect(accountProblems(plan, { ...choices, card: same })).toEqual({
      card: 'Choose the account it’s the same as.',
    })
  })
})

describe('the categories step 3 lists', () => {
  it('are those the imported accounts use, with their rows there', () => {
    const choices = startAccountChoices(plan)
    expect(usedCategories(plan, choices).map((used) => [used.category.id, used.rows])).toEqual([
      ['groceries', 17],
      ['fuel', 12],
      ['invest', 1],
    ])
  })

  it('lose a skipped or balances-only account’s rows, and categories only it used', () => {
    const choices = startAccountChoices(plan)
    const skip = { ...choices, card: { ...choices.card, action: 'skip' } as AccountChoice }
    const balancesOnly = {
      ...skip,
      brokerage: { ...choices.brokerage, balancesOnly: true } as AccountChoice,
    }
    expect(usedCategories(plan, balancesOnly).map((used) => [used.category.id, used.rows])).toEqual(
      [['groceries', 3]],
    )
  })
})

const groups: CategoryGroup[] = [
  {
    id: 1,
    name: 'Food',
    kind: 'expense',
    hidden: false,
    categories: [
      { id: 11, name: 'Groceries', hidden: false },
      { id: 12, name: 'Restaurants', hidden: true },
    ],
  },
  {
    id: 2,
    name: 'Everyday',
    kind: 'expense',
    hidden: false,
    categories: [{ id: 21, name: 'groceries', hidden: false }],
  },
  {
    id: 3,
    name: 'Old',
    kind: 'expense',
    hidden: true,
    categories: [{ id: 31, name: 'Fuel', hidden: false }],
  },
  { id: 4, name: 'Pay', kind: 'income', hidden: false, categories: [] },
]

describe('the Goes to picker', () => {
  it('offers the shown categories in shown groups', () => {
    expect(offeredCategories(groups)).toEqual([
      { id: 1, name: 'Food', categories: [{ id: 11, name: 'Groceries', hidden: false }] },
      { id: 2, name: 'Everyday', categories: [{ id: 21, name: 'groceries', hidden: false }] },
    ])
  })

  it('starts on a category of the same name, in a group of the same name first', () => {
    const choices = startCategoryChoices(plan, groups, {})
    expect(choices).toEqual({ groceries: goesToCategory(21), fuel: LATER, invest: LATER })
    const elsewhere = {
      ...plan,
      categories: [category('groceries', 'GROCERIES', [], { group: 'X' })],
    }
    expect(startCategoryChoices(elsewhere, groups, {})).toEqual({ groceries: 'category:11' })
  })

  it('keeps the choices made already', () => {
    expect(startCategoryChoices(plan, groups, { groceries: 'transfer' })).toMatchObject({
      groceries: 'transfer',
    })
  })
})

describe('+ New category', () => {
  it('starts in the group of the same name, else the first of its kind, else a new one', () => {
    const fuel = category('fuel', 'Fuel', [], { group: 'everyday' })
    expect(startingGroup(fuel, groups)).toBe(2)
    expect(startingGroup({ ...fuel, group: 'Car' }, groups)).toBe(1)
    expect(startingGroup({ ...fuel, group: 'Income', income: true }, groups)).toBe(4)
    expect(startingGroup({ ...fuel, group: 'Old' }, groups)).toBe(1)
    expect(startingGroup(fuel, [])).toBeNull()
  })
})
